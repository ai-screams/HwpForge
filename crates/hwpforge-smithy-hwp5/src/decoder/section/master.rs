//! Section-owned list dispatch. Payload interpretation lives with the IR;
//! paragraphs reuse the same streaming parser and inherited depth budget.
use super::*;

pub(super) struct MasterCapture {
    master: Hwp5MasterPage,
    parser: Box<BodyTextParserState>,
}

// Decoder-private provenance: record indices are section-stream absolute, not
// nested-parser counters. Memo diagnostics are deferred until the existing join
// determines their final owner; direct master records use the same classifier.
pub(super) struct NoteNumberingSource {
    kind: &'static str,
    record_index: usize,
    level: u16,
}

impl NoteNumberingSource {
    pub(super) fn from_record(record: &Record, record_index: usize) -> Option<Self> {
        if TagId::from(record.header.tag_id) != TagId::CtrlHeader {
            return None;
        }
        let kind = match parse_ctrl_id(&record.data) {
            CTRL_ID_FOOTNOTE => "footnote",
            CTRL_ID_ENDNOTE => "endnote",
            CTRL_ID_NEW_NUMBER => {
                let number = crate::schema::section::Hwp5NewNumberControl::parse(
                    CTRL_ID_NEW_NUMBER,
                    &record.data,
                )?;
                match number.kind_raw {
                    1 => "NewNumber(Footnote)",
                    2 => "NewNumber(Endnote)",
                    _ => return None,
                }
            }
            _ => return None,
        };
        Some(Self { kind, record_index, level: record.header.level })
    }

    pub(super) fn warning(&self, master: &str, memo_id: Option<u32>) -> Hwp5Warning {
        let memo = memo_id.map_or_else(String::new, |id| format!(" memo_id={id};"));
        Hwp5Warning::ProjectionFallback {
            subject: "master_page.note_numbering",
            reason: format!(
                "{master}:{memo} {} CtrlHeader record {} level {}; content retained, but the shared HWPX note planner visits body/header/footer before master pages, so display numbering, NewNumber restarts and cross-reference display text may disagree",
                self.kind, self.record_index, self.level,
            ),
        }
    }
}

impl BodyTextParserState {
    pub(super) fn dispatch_master_record(
        &mut self,
        record: &Record,
        tag: TagId,
        level: u16,
    ) -> bool {
        if self.nested {
            return false;
        }
        if let Some(capture) = &self.master_capture {
            let boundary = level < capture.master.level
                || (level == capture.master.level && tag != TagId::ParaHeader);
            if boundary {
                self.close_master();
            }
        }
        if let Some(capture) = &mut self.master_capture {
            if let Some(source) = NoteNumberingSource::from_record(record, self.record_index - 1) {
                self.warnings.push(source.warning(&capture.master.location(), None));
            }
            capture.parser.handle_record(record);
            return true;
        }
        false
    }

    pub(super) fn intercept_master_record(
        &mut self,
        record: &Record,
        tag: TagId,
        level: u16,
    ) -> bool {
        if self.nested {
            return false;
        }
        if self.secd_level.is_some_and(|owner| level <= owner) {
            self.secd_level = None;
        }
        // Only section-owned secd; a cell/shape secd is interpreted by its
        // existing nested parser, never promoted into this section.
        if tag == TagId::CtrlHeader
            && level == self.base_level + 1
            && parse_ctrl_id(&record.data) == CTRL_ID_SECD
        {
            self.secd_level = Some(level);
            self.secd_declaration = Some((self.record_index - 1, level, record.data.clone()));
            self.extended_master_count =
                record.data.get(30..32).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]));
        }
        if tag != TagId::ListHeader || self.memo_content_capture.is_some() {
            return false;
        }
        let ordinary = self.secd_level.is_some_and(|owner| level == owner + 1);
        let extended = level == self.base_level + 1
            && self.extended_master_count > 0
            && self.table_stack.is_empty()
            && self.subtree_ctx.as_ref().is_none_or(|s| level <= s.ctrl_depth);
        if !ordinary && !extended {
            return false;
        }
        let Some(budget) = self.budget.enter(false, false) else {
            return false;
        };
        let mut parser = self.nested_parser(level);
        parser.budget = budget;
        parser.active_budget = budget;
        let master = Hwp5MasterPage {
            body_paragraph_index: self.paragraphs.len(),
            body_control_index: self.current.as_ref().map_or(0, |p| p.controls.len()),
            raw: record.data.clone(),
            extended: !ordinary,
            index: self.master_pages.len(),
            ordinary_index: self.ordinary_master_count,
            record_index: self.record_index - 1,
            level,
            section_properties: self.section_def_properties.unwrap_or(0),
            paragraphs: Vec::new(),
        };
        if ordinary {
            self.ordinary_master_count += 1;
        } else {
            self.extended_master_count -= 1;
        }
        self.master_capture = Some(MasterCapture { master, parser: Box::new(parser) });
        true
    }

    pub(super) fn check_master_counts(&mut self) {
        let bits = self.section_def_properties.unwrap_or(0) >> 29;
        let expected = match bits {
            0 => Some(0),
            1 => Some(1),
            6 => Some(2),
            _ => None,
        };
        if self.extended_master_count > 0
            || expected.is_some_and(|n| n != self.ordinary_master_count)
        {
            self.warnings.push(Hwp5Warning::ParserFallback {
                subject: "master_page.list_count",
                reason: format!("section end record {}: secd declaration={:?}, scope bits={bits:#x}, ordinary expected={expected:?} recovered={}, extended lists missing={}", self.record_index, self.secd_declaration, self.ordinary_master_count, self.extended_master_count),
            });
        }
    }

    pub(super) fn close_master(&mut self) {
        let Some(capture) = self.master_capture.take() else { return };
        let mut master = capture.master;
        let mut result = capture.parser.finish();
        self.warnings.append(&mut result.warnings);
        master.paragraphs = result.paragraphs;
        let declared = master.raw.get(..4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
        if declared != Some(master.paragraphs.len() as u32) {
            self.warnings.push(Hwp5Warning::ParserFallback {
                subject: "master_page.paragraph_count",
                reason: format!(
                    "{}: declared {declared:?}, recovered {}",
                    master.location(),
                    master.paragraphs.len()
                ),
            });
        }
        self.master_pages.push(master);
    }
}

impl Hwp5MasterPage {
    pub(crate) fn location(&self) -> String {
        format!(
            "master list {} record {} level {} extended={} secd={:#010x} raw={:02x?}",
            self.index,
            self.record_index,
            self.level,
            self.extended,
            self.section_properties,
            self.raw
        )
    }

    /// Only fixture-confirmed scopes. Unknown scopes never become BOTH.
    /// Core is shared by projection and semantic summaries, avoiding a second
    /// interpretation of the wire flags.
    pub(crate) fn core_metadata(&self) -> Result<hwpforge_core::section::MasterPage, String> {
        use hwpforge_core::section::{MasterPage, MasterPageSpecial};
        use hwpforge_foundation::{ApplyPageType, HwpUnit, VerticalAlign};
        if self.raw.len() < 34 {
            return Err("truncated ListHeader (need 34 bytes)".into());
        }
        let word = |at| u32::from_le_bytes(self.raw[at..at + 4].try_into().unwrap());
        let selector = word(18);
        let (apply, special) = if self.extended {
            match selector {
                3 => (ApplyPageType::Both, Some(MasterPageSpecial::LastPage)),
                5 | 6 => (
                    ApplyPageType::Both,
                    Some(MasterPageSpecial::OptionalPage { page_number: selector - 3 }),
                ),
                _ => return Err(format!("unverified extended selector {selector}")),
            }
        } else {
            if selector != 0 {
                return Err(format!("ordinary list has unverified selector {selector}"));
            }
            let apply = match (self.section_properties >> 29, self.ordinary_index) {
                (1, 0) => ApplyPageType::Both,
                (6, 0) => ApplyPageType::Even,
                (6, 1) => ApplyPageType::Odd,
                _ => return Err("unverified secd scope bits/list ordinal".into()),
            };
            (apply, None)
        };
        let mut master = MasterPage::new(apply, Vec::new());
        master.special = special;
        master.page_duplicate = word(22) & 1 != 0;
        master.page_front = word(22) & 2 != 0;
        master.vert_align = match (word(4) >> 5) & 3 {
            0 => VerticalAlign::Top,
            1 => VerticalAlign::Center,
            2 => VerticalAlign::Bottom,
            _ => return Err("unverified vertical alignment 3".into()),
        };
        let unit = |at| -> Result<HwpUnit, String> {
            let value = i32::try_from(word(at)).map_err(|_| "text area exceeds i32".to_string())?;
            HwpUnit::new(value).map_err(|e| e.to_string())
        };
        master.text_width = unit(8)?;
        master.text_height = unit(12)?;
        Ok(master)
    }

    pub(crate) fn unsupported_properties(&self) -> bool {
        self.raw.len() >= 34
            && (self.raw[4] & 0x9f != 0
                || self.raw[5..8].iter().any(|b| *b != 0)
                || self.raw[16..18].iter().any(|b| *b != 0)
                || self.raw[22] & !3 != 0
                || self.raw[23..].iter().any(|b| *b != 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(tag: TagId, level: u16, data: &[u8]) -> Record {
        let word =
            u32::from(u16::from(tag)) | (u32::from(level) << 10) | ((data.len() as u32) << 20);
        let mut bytes = word.to_le_bytes().to_vec();
        bytes.extend_from_slice(data);
        Record::parse_stream(&mut std::io::Cursor::new(bytes)).unwrap().remove(0)
    }
    fn para(level: u16, text: &str) -> Vec<Record> {
        vec![
            record(TagId::ParaHeader, level, &[0; 22]),
            record(
                TagId::ParaText,
                level + 1,
                &text.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<_>>(),
            ),
        ]
    }
    fn secd(properties: u32, extended: u16) -> Record {
        let mut data = vec![0; 32];
        data[..4].copy_from_slice(&CTRL_ID_SECD.to_le_bytes());
        data[4..8].copy_from_slice(&properties.to_le_bytes());
        data[30..32].copy_from_slice(&extended.to_le_bytes());
        record(TagId::CtrlHeader, 1, &data)
    }
    fn list(level: u16, count: u32, selector: u32) -> Record {
        let mut data = vec![0; 34];
        data[..4].copy_from_slice(&count.to_le_bytes());
        data[8..12].copy_from_slice(&48190u32.to_le_bytes());
        data[12..16].copy_from_slice(&67181u32.to_le_bytes());
        data[18..22].copy_from_slice(&selector.to_le_bytes());
        record(TagId::ListHeader, level, &data)
    }
    fn parse(records: &[Record]) -> SectionResult {
        let mut parser = BodyTextParserState::default();
        let work = parser.work.clone();
        for record in records {
            parser.record_identity.set(record as *const Record);
            parser.handle_record(record);
        }
        let result = parser.finish();
        assert!(work.get() <= records.len() * 4, "repeated parsing");
        result
    }

    // 이것을 실패시키는 것: 동급 ParaHeader를 경계로 닫기 / master를 본문에 주입하기 / EOF 마감 누락.
    #[test]
    fn master_lists_keep_body_and_sibling_paragraphs_separate() {
        let mut records = para(0, "body");
        records.push(secd(6 << 29, 0));
        records.push(list(2, 2, 0));
        records.extend(para(2, "even one"));
        records.extend(para(2, "even two"));
        records.push(list(2, 1, 0));
        records.extend(para(2, "odd"));
        let result = parse(&records);
        assert_eq!(result.paragraphs.len(), 1);
        assert_eq!(result.paragraphs[0].text, "body");
        assert_eq!(result.master_pages.len(), 2);
        assert_eq!(result.master_pages[0].paragraphs.len(), 2);
        assert_eq!(result.master_pages[1].paragraphs[0].text, "odd");
        assert_eq!(
            result.master_pages[0].core_metadata().unwrap().apply_page_type,
            hwpforge_foundation::ApplyPageType::Even
        );
        assert_eq!(
            result.master_pages[1].core_metadata().unwrap().apply_page_type,
            hwpforge_foundation::ApplyPageType::Odd
        );
    }

    // 이것을 실패시키는 것: 모든 level 1 목록을 바탕쪽으로 읽기 / MemoList 경계 무시 / 확장 목록 하나만 수집.
    #[test]
    fn extended_master_lists_end_before_memo_cluster() {
        let mut records = para(0, "body");
        records.push(secd(0, 2));
        records.push(list(1, 1, 3));
        records.extend(para(1, "last"));
        records.push(list(1, 1, 5));
        records.extend(para(1, "page2"));
        records.push(record(TagId::MemoList, 1, &7u32.to_le_bytes()));
        records.push(record(TagId::ListHeader, 1, &[0; 16]));
        records.extend(para(1, "memo"));
        let result = parse(&records);
        assert_eq!(result.master_pages.len(), 2);
        assert_eq!(result.master_pages[1].paragraphs[0].text, "page2");
        assert_eq!(result.paragraphs[0].text, "body");
        assert!(result.warnings.iter().any(|w| matches!(
            w,
            Hwp5Warning::DroppedControl { control: "memo_content_cluster", .. }
        )));
    }

    // 이것을 실패시키는 것: secd가 끝난 뒤 일반 목록으로 오인 / 비대상 일반 ListHeader 가로채기.
    #[test]
    fn master_capture_closes_at_section_owner_boundary() {
        let mut records = para(0, "body1");
        records.push(secd(1 << 29, 0));
        records.push(list(2, 1, 0));
        records.extend(para(2, "master"));
        records.extend(para(0, "body2"));
        records.push(list(1, 0, 0));
        let result = parse(&records);
        assert_eq!(result.master_pages.len(), 1);
        assert_eq!(
            result.paragraphs.iter().map(|p| p.text.as_str()).collect::<Vec<_>>(),
            ["body1", "body2"]
        );
    }

    // 이것을 실패시키는 것: 복원 목록이 0이면 EOF 선언 검사를 건너뛰기.
    #[test]
    fn master_missing_all_declared_lists_reports_source_declaration() {
        for bits in [1, 6] {
            let mut records = para(0, "body");
            records.push(secd(bits << 29, 1));
            let result = parse(&records);
            assert!(result.master_pages.is_empty());
            let reasons: Vec<_> = result
                .warnings
                .iter()
                .filter_map(|warning| match warning {
                    Hwp5Warning::ParserFallback { subject: "master_page.list_count", reason } => {
                        Some(reason)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(reasons.len(), 1);
            assert!(reasons[0].contains("secd declaration=Some((2, 1, ["));
            assert!(reasons[0].contains("recovered=0, extended lists missing=1"));
            assert!(reasons[0].contains(&format!("scope bits={bits:#x}")));
        }
    }

    // 이것을 실패시키는 것: 미확정 단독 비트/selector를 BOTH로 확대 / 잘린 목록·문단수 불일치 무음 처리.
    #[test]
    fn master_unverified_scope_and_malformed_list_are_diagnosed() {
        for properties in [1 << 30, 1 << 31, 7 << 29] {
            let mut records = para(0, "body");
            records.push(secd(properties, 0));
            records.push(list(2, 2, 0));
            records.extend(para(2, "unverified"));
            let result = parse(&records);
            assert!(result.master_pages[0].core_metadata().is_err());
            assert!(result.warnings.iter().any(|w| matches!(
                w,
                Hwp5Warning::ParserFallback { subject: "master_page.paragraph_count", .. }
            )));
            let (_, warnings) = crate::projection::project_to_core(vec![result]).unwrap();
            assert!(warnings.iter().any(|w| matches!(w, Hwp5Warning::DroppedControl { control: "master_page", reason } if reason.contains("raw=") && reason.contains("record"))));
        }
        let mut records = para(0, "body");
        records.push(secd(1 << 29, 0));
        records.push(record(TagId::ListHeader, 2, &[1, 0, 0, 0]));
        let result = parse(&records);
        assert!(result.master_pages[0].core_metadata().unwrap_err().contains("truncated"));
    }
    // 이것을 실패시키는 것: master 중첩 파서 대신 본문 파서로 표 전달 / master hint 큐에서 셀 누락 / 메모 연결에서 master 생략.
    #[test]
    fn master_reuses_nested_tables_and_joins_section_memo_contents() {
        let mut records = para(0, "body");
        records.push(secd(1 << 29, 0));
        records.push(list(2, 1, 0));
        records.extend(para(2, "master"));
        let mut table = Vec::new();
        super::super::tests::one_cell_table(&mut table, 1);
        let mut table_records = Record::parse_stream(&mut std::io::Cursor::new(table)).unwrap();
        for r in &mut table_records {
            r.header.level += 3;
        }
        records.extend(table_records);
        records.extend(para(4, "cell"));
        let mut parser = BodyTextParserState::default();
        for record in &records {
            parser.handle_record(record);
        }
        // An inline memo was already parsed by the common control interpreter.
        let command = crate::schema::section::Hwp5MemoCommand {
            raw: "MEMO/65535/7/1/2/test/;".into(),
            shape_id: 65535,
            memo_id: 7,
            hancom_inst_a: 1,
            hancom_inst_b: 2,
            author: "test".into(),
            terminator: ";".into(),
        };
        parser.master_capture.as_mut().unwrap().parser.current.as_mut().unwrap().controls.push(
            Hwp5Control::Memo(Hwp5MemoControl {
                ctrl_id: CTRL_ID_MEMO,
                command,
                paragraphs: Vec::new(),
            }),
        );
        for record in [
            record(TagId::MemoList, 1, &7u32.to_le_bytes()),
            record(TagId::ListHeader, 1, &[0; 16]),
        ]
        .into_iter()
        .chain(para(1, "memo text"))
        {
            parser.handle_record(&record);
        }
        let result = parser.finish();
        assert_eq!(result.master_pages.len(), 1);
        let controls = &result.master_pages[0].paragraphs[0].controls;
        let table = controls
            .iter()
            .find_map(|c| if let Hwp5Control::Table(t) = c { Some(t) } else { None })
            .unwrap();
        assert_eq!(table.cells[0].paragraphs[0].text, "cell");
        let memo = controls
            .iter()
            .find_map(|c| if let Hwp5Control::Memo(m) = c { Some(m) } else { None })
            .unwrap();
        assert_eq!(memo.paragraphs[0].text, "memo text");
        assert!(!result.warnings.iter().any(|w| matches!(
            w,
            Hwp5Warning::DroppedControl { control: "memo_content_cluster", .. }
        )));
        let mut warnings = Vec::new();
        let bytes = std::fs::read(crate::test_support::workspace_fixture_path(
            "master_pages/f5f-shapes.hwp",
        ))
        .unwrap();
        let mut decoded = crate::decoder::decode_intermediate(&bytes).unwrap();
        decoded.sections = vec![result];
        let semantic = crate::semantic_adapter::adapt_to_semantic(
            &decoded,
            &crate::Hwp5JoinedImageAssetPlan {
                ordered_assets: Vec::new(),
                assets_by_reference: Default::default(),
                unavailable_reasons: Default::default(),
            },
        );
        assert!(semantic.graph_is_coherent());
        let cell = semantic.sections[0].paragraphs.iter().find(|p| p.text == "cell").unwrap();
        assert_eq!(
            cell.container.segments[0],
            crate::Hwp5SemanticContainerKind::MasterPageSubList(0)
        );
        assert_eq!(
            cell.container.terminal_kind(),
            &crate::Hwp5SemanticContainerKind::TableCellSubList
        );
        let hints =
            crate::layout_hint_patch::capture_master_layout_hints(&decoded.sections, &mut warnings);
        assert_eq!(hints[0][0].tables.len(), 1);
        assert_eq!(hints[0][0].paragraphs.len(), 3);
    }
    // 이것을 실패시키는 것: 이전 표가 아직 stack에 있다는 이유로 확장 목록을 건너뛰기.
    #[test]
    fn extended_master_can_follow_a_body_table_without_new_body_paragraph() {
        let mut records = para(0, "body");
        records.push(secd(0, 1));
        let mut table = Vec::new();
        super::super::tests::one_cell_table(&mut table, 1);
        let mut table_records = Record::parse_stream(&mut std::io::Cursor::new(table)).unwrap();
        for r in &mut table_records {
            r.header.level += 1;
        }
        records.extend(table_records);
        records.extend(para(2, "body cell"));
        records.push(list(1, 1, 3));
        records.extend(para(1, "last master"));
        let result = parse(&records);
        assert_eq!(result.paragraphs.len(), 1);
        assert!(result.paragraphs[0].controls.iter().any(|c| matches!(c, Hwp5Control::Table(_))));
        assert_eq!(result.master_pages.len(), 1);
        assert_eq!(result.master_pages[0].paragraphs[0].text, "last master");
    }
}
