use super::*;

#[derive(Clone, Copy)]
pub(super) enum MarkerGroup {
    Ready(lists::ListRef),
    Pending(usize, u8),
}
// Candidate payloads own no strings. The body part is supplied only after
// final definition/level suppression, when the warning is materialized.
#[derive(Clone, Copy)]
pub(super) enum MarkerDiagnostic {
    Script,
    LineHeight { before: bool },
    Indent,
    ZeroSlot,
    Tab,
    DefaultTab,
    TabBoundary,
    SpaceWidth,
    Overflow { uncertain: bool },
    SharedOverflow(i32),
}
impl MarkerDiagnostic {
    fn class(self) -> &'static str {
        match self {
            Self::Script => "list_marker_script",
            Self::LineHeight { .. } => "list_marker_line_height",
            Self::Indent => "list_marker_indent",
            Self::ZeroSlot => "list_marker_zero_slot",
            Self::Tab => "list_marker_tab",
            Self::DefaultTab => "list_marker_default_tab",
            Self::TabBoundary => "list_marker_tab_boundary",
            Self::SpaceWidth => "list_marker_space_width",
            Self::Overflow { .. } | Self::SharedOverflow(_) => "list_marker_overflow",
        }
    }
    fn emit(self, part: &str, warnings: &mut Vec<DocxWarning>) -> Result<()> {
        let (location, action, message) = match self {
            Self::Script => return format_warnings(0, true, part, "numPr", warnings),
            Self::LineHeight { before: true } => {
                ("rPr", "approximate_space_before", marker_messages::LINE_HEIGHT_BEFORE)
            }
            Self::LineHeight { before: false } => {
                ("rPr", "retain_body_line_height", marker_messages::LINE_HEIGHT)
            }
            Self::Indent => ("ind", "retain_indent", marker_messages::INDENT),
            Self::ZeroSlot => ("ind", "retain_indent", marker_messages::ZERO_SLOT),
            Self::Tab => ("tab", "use_indent", marker_messages::TAB),
            Self::DefaultTab => ("numPr", "use_measured_ratio", marker_messages::DEFAULT_TAB),
            Self::TabBoundary => ("numPr", "retain_selected_tab", marker_messages::TAB_BOUNDARY),
            Self::SpaceWidth => ("suff", "use_measured_ratio", marker_messages::SPACE_WIDTH),
            Self::Overflow { uncertain: false } => {
                ("numPr", "retain_slot", marker_messages::OVERFLOW)
            }
            Self::Overflow { uncertain: true } => {
                ("numPr", "retain_slot", marker_messages::UNCERTAIN_OVERFLOW)
            }
            Self::SharedOverflow(_) => ("numPr", "retain_slot", marker_messages::SHARED_OVERFLOW),
        };
        warn(warnings, part, location, Kind::ValueApproximated, self.class(), action, message)
    }
}
struct PositionedMarker {
    position: (usize, usize),
    diagnostic: MarkerDiagnostic,
}
struct PositionedWarning {
    position: (usize, usize),
    warning: DocxWarning,
}
pub(super) struct PendingNumber {
    request: Option<lists::NumberRequest>,
    position: (usize, usize),
    diagnostics: Vec<PositionedMarker>,
    layout: Option<ListMarkerLayout>,
    resolved: Option<lists::ListRef>,
}
impl PendingNumber {
    pub(super) fn new(request: lists::NumberRequest, position: (usize, usize)) -> Self {
        Self { request: Some(request), position, diagnostics: vec![], layout: None, resolved: None }
    }
    pub(super) fn level(&self) -> u8 {
        self.request.as_ref().expect("unassigned source number").level as u8
    }
}
impl Resolver {
    pub(crate) fn defer_shape(&mut self, delta: Delta, number: Option<usize>) -> ParaShapeIndex {
        let index = self.pending_shapes.len();
        self.pending_shapes.push((delta, number));
        ParaShapeIndex::new(index)
    }
    pub(crate) fn warning_serial(&self) -> usize {
        self.warning_serial
    }
    pub(super) fn warning_position(&mut self, warnings: &[DocxWarning]) -> (usize, usize) {
        let position = (warnings.len(), self.warning_serial);
        self.warning_serial += 1;
        position
    }
    pub(crate) fn insert_warning_at(&mut self, index: usize, serial: usize) {
        for pending in &mut self.pending_numbers {
            for position in std::iter::once(&mut pending.position)
                .chain(pending.diagnostics.iter_mut().map(|d| &mut d.position))
            {
                if *position >= (index, serial) {
                    position.0 += 1;
                }
            }
        }
    }
    pub(super) fn emit_marker(
        &mut self,
        group: MarkerGroup,
        diagnostic: MarkerDiagnostic,
        part: &str,
        warnings: &mut Vec<DocxWarning>,
    ) -> Result<()> {
        match group {
            // The suppression key and the warning come from the same diagnostic value.
            // 이것을 실패시키는 것: 문자권 축약 진단의 억제 키를 표식 높이 진단의 키와 같게 둠.
            MarkerGroup::Ready(list) => {
                if self.marker_warnings.insert((list, diagnostic.class())) {
                    diagnostic.emit(part, warnings)?;
                }
                Ok(())
            }
            MarkerGroup::Pending(id, _) => {
                let position = self.warning_position(warnings);
                let diagnostics = &mut self.pending_numbers[id].diagnostics;
                // Most paragraphs have only one or two candidates.
                diagnostics.reserve_exact(1);
                diagnostics.push(PositionedMarker { position, diagnostic });
                Ok(())
            }
        }
    }
    pub(super) fn note_marker_layout(
        &mut self,
        group: MarkerGroup,
        layout: Option<ListMarkerLayout>,
    ) {
        match group {
            MarkerGroup::Ready(list) => {
                self.first_marker_layouts.entry(list).or_insert(layout);
            }
            MarkerGroup::Pending(id, _) => self.pending_numbers[id].layout = layout,
        }
    }
    pub(super) fn shared_marker_overflow(
        &mut self,
        group: MarkerGroup,
        width: i32,
        part: &str,
        warnings: &mut Vec<DocxWarning>,
    ) -> Result<()> {
        match group {
            MarkerGroup::Ready(list) => {
                if !shared_overflow(self.first_marker_layouts.get(&list), width)? {
                    return Ok(());
                }
            }
            MarkerGroup::Pending(..) => {}
        }
        self.emit_marker(group, MarkerDiagnostic::SharedOverflow(width), part, warnings)
    }
    pub(crate) fn finalize_numbering(
        &mut self,
        sections: &mut [&mut hwpforge_core::section::Section],
        part: &str,
        warnings: &mut Vec<DocxWarning>,
    ) -> Result<()> {
        let invalid = || DocxError::invalid(part, "p", "unresolved paragraph numbering reference");
        let mut order = vec![];
        // 이것을 실패시키는 것: 표 칸을 부모보다 먼저 배정하거나 중첩 칸 순회를 생략.
        for section in sections.iter() {
            for paragraph in &section.paragraphs {
                paragraph.for_each_paragraph(|p| order.push(p.para_shape_id.get()));
            }
        }
        let mut delayed = vec![];
        for shape in order {
            let &(_, number) = self.pending_shapes.get(shape).ok_or_else(invalid)?;
            let Some(id) = number else { continue };
            let pending = self.pending_numbers.get_mut(id).ok_or_else(invalid)?;
            let request = pending.request.take().ok_or_else(invalid)?;
            let mut assigned_warnings = vec![];
            let list = self
                .lists
                .assign_numbering(&mut self.registry, &request, part, &mut assigned_warnings)?
                .ok_or_else(invalid)?;
            pending.resolved = Some(list);
            delayed.extend(
                assigned_warnings
                    .into_iter()
                    .map(|warning| PositionedWarning { position: pending.position, warning }),
            );
            for diagnostic in std::mem::take(&mut pending.diagnostics) {
                if let MarkerDiagnostic::SharedOverflow(width) = diagnostic.diagnostic {
                    if !shared_overflow(self.first_marker_layouts.get(&list), width)? {
                        continue;
                    }
                }
                if self.marker_warnings.insert((list, diagnostic.diagnostic.class())) {
                    let mut admitted = vec![];
                    diagnostic.diagnostic.emit(part, &mut admitted)?;
                    delayed.extend(admitted.into_iter().map(|warning| PositionedWarning {
                        position: diagnostic.position,
                        warning,
                    }));
                }
            }
            // The first unspecified candidate is also authoritative.
            self.first_marker_layouts.entry(list).or_insert(pending.layout);
        }
        let mut shapes = vec![];
        for (mut delta, number) in std::mem::take(&mut self.pending_shapes) {
            if let Some(id) = number {
                delta.para.projected_list = Some(
                    self.pending_numbers.get(id).and_then(|p| p.resolved).ok_or_else(invalid)?,
                );
            }
            shapes.push(self.para_shape(&delta)?);
        }
        for section in sections.iter_mut() {
            for paragraph in &mut section.paragraphs {
                paragraph
                    .for_each_paragraph_mut(|p| p.para_shape_id = shapes[p.para_shape_id.get()]);
            }
        }
        if self.pending_numbers.iter().any(|p| p.request.is_some() || p.resolved.is_none()) {
            return Err(invalid());
        }
        delayed.sort_by_key(|d| d.position);
        let original = std::mem::take(warnings);
        let mut delayed = delayed.into_iter().peekable();
        for (index, original) in original.into_iter().enumerate() {
            while delayed.peek().is_some_and(|d| d.position.0 <= index) {
                merge_warning(warnings, delayed.next().unwrap().warning)?;
            }
            merge_warning(warnings, original)?;
        }
        for diagnostic in delayed {
            merge_warning(warnings, diagnostic.warning)?;
        }
        Ok(())
    }
}
fn shared_overflow(first: Option<&Option<ListMarkerLayout>>, width: i32) -> Result<bool> {
    let Some(Some(ListMarkerLayout { align, text_start: ListTextStart::FromAnchor(gap), .. })) =
        first
    else {
        return Ok(false);
    };
    let end = match align {
        ListMarkerAlign::Left => i64::from(width) * 2,
        ListMarkerAlign::Center => i64::from(width),
        ListMarkerAlign::Right => 0,
        _ => return Err(marker_arithmetic()),
    };
    Ok(end > i64::from(gap.as_i32()) * 2)
}
fn merge_warning(warnings: &mut Vec<DocxWarning>, warning: DocxWarning) -> Result<()> {
    let key = (warning.code(), warning.loss(), warning.action());
    if let Some(first) = warnings.iter_mut().find(|w| (w.code(), w.loss(), w.action()) == key) {
        let total = first.count().checked_add(warning.count()).ok_or_else(|| {
            DocxError::invalid(
                first.part().unwrap_or("#unknown"),
                first.location().unwrap_or("#unknown"),
                crate::warning::COUNT_OVERFLOW,
            )
        })?;
        *first.count_mut() = total;
    } else {
        warnings.push(warning);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_core::{PageSettings, Section};

    fn resolver() -> Resolver {
        let levels: String = (0..2).map(|level| format!("<w:lvl w:ilvl='{level}'><w:start w:val='1'/><w:numFmt w:val='decimal'/><w:lvlText w:val='%{}.'/></w:lvl>", level+1)).collect();
        let nums: String = [(1,2),(2,3),(3,1)].into_iter().map(|(id,start)| format!("<w:num w:numId='{id}'><w:abstractNumId w:val='1'/><w:lvlOverride w:ilvl='0'><w:startOverride w:val='{start}'/></w:lvlOverride><w:lvlOverride w:ilvl='1'><w:startOverride w:val='{start}'/></w:lvlOverride></w:num>")).collect();
        let xml = format!("<w:numbering xmlns:w='{}'><w:abstractNum w:abstractNumId='1'>{levels}</w:abstractNum>{nums}</w:numbering>",crate::xml::W_NS.1);
        let mut defs = Definitions::default();
        defs.read(
            xml.as_bytes(),
            "numbering.xml",
            "numbering",
            &mut crate::xml::TextBudget::default(),
            &mut vec![],
        )
        .unwrap();
        Resolver::new(defs, false, &mut vec![]).unwrap()
    }

    fn request(
        r: &mut Resolver,
        num: u32,
        level: u8,
        layout: Option<ListMarkerLayout>,
        warnings: &mut Vec<DocxWarning>,
    ) -> usize {
        let para =
            ParaDelta { num_id: Some(num), level: Some(u32::from(level)), ..Default::default() };
        let Some(lists::ObservedList::Number(request)) =
            r.lists.observe(&r.definitions, &mut r.registry, &para, "body", warnings).unwrap()
        else {
            panic!("number request")
        };
        let id = r.pending_numbers.len();
        let pos = r.warning_position(warnings);
        r.pending_numbers.push(PendingNumber::new(request, pos));
        r.note_marker_layout(MarkerGroup::Pending(id, level), layout);
        let mut delta = Delta::default();
        delta.para.projected_marker = layout;
        r.defer_shape(delta, Some(id)).get()
    }

    // 이것을 실패시키는 것: 삽입 위치 보정 누락 또는 같은 위치의 지연 진단을 뒤로 밀기.
    #[test]
    fn inserted_warning_keeps_delayed_diagnostics_between_neighbors() {
        let mut r = resolver();
        let mut warnings = vec![];
        let shape = request(&mut r, 1, 0, None, &mut warnings);
        r.emit_marker(MarkerGroup::Pending(0, 0), MarkerDiagnostic::Indent, "body", &mut warnings)
            .unwrap();
        warn(
            &mut warnings,
            "body",
            "p",
            Kind::ValueApproximated,
            "earlier",
            "approximate",
            "earlier",
        )
        .unwrap();
        r.insert_warning_at(0, 0);
        warn(&mut warnings, "body", "p", Kind::ValueApproximated, "later", "approximate", "later")
            .unwrap();
        let mut section = Section::new(PageSettings::a4());
        section.paragraphs.push(Paragraph::new(ParaShapeIndex::new(shape)));
        r.finalize_numbering(&mut [&mut section], "body", &mut warnings).unwrap();
        assert_eq!(
            warnings.iter().map(DocxWarning::loss).collect::<Vec<_>>(),
            ["earlier", "list_marker_indent", "later"]
        );
    }

    // 이것을 실패시키는 것: 억제 전 후보마다 진단 문자열을 생성.
    #[test]
    fn pending_marker_candidates_allocate_no_diagnostic_strings() {
        let mut r = resolver();
        let mut warnings = vec![];
        let mut section = Section::new(PageSettings::a4());
        crate::warning::take_diagnostic_bytes();
        for _ in 0..1024 {
            let id = request(&mut r, 1, 0, None, &mut warnings);
            r.emit_marker(
                MarkerGroup::Pending(id, 0),
                MarkerDiagnostic::DefaultTab,
                "body",
                &mut warnings,
            )
            .unwrap();
            r.shared_marker_overflow(MarkerGroup::Pending(id, 0), 100, "body", &mut warnings)
                .unwrap();
            section.paragraphs.push(Paragraph::new(ParaShapeIndex::new(id)));
        }
        assert_eq!(crate::warning::take_diagnostic_bytes(), 0);
        assert!(warnings.is_empty());
        r.finalize_numbering(&mut [&mut section], "body", &mut warnings).unwrap();
        assert!(crate::warning::take_diagnostic_bytes() > 0);
        assert_eq!(warnings.len(), 1);
        assert_eq!((warnings[0].loss(), warnings[0].count()), ("list_marker_default_tab", 1));
    }

    // 이것을 실패시키는 것: 미연결 색인을 0번 서식으로 대체하거나 공유 빈 문단을 거부.
    #[test]
    fn unresolved_shapes_fail_while_empty_paragraphs_can_share_a_pending_shape() {
        for index in [usize::MAX, 1, 0] {
            let mut r = resolver();
            r.defer_shape(Delta::default(), None);
            let mut section = Section::new(PageSettings::a4());
            section.paragraphs.push(Paragraph::new(ParaShapeIndex::new(0)));
            section.paragraphs.push(Paragraph::new(ParaShapeIndex::new(index)));
            let result = r.finalize_numbering(&mut [&mut section], "body", &mut vec![]);
            if index == 0 {
                result.unwrap();
                assert_eq!(
                    section.paragraphs[0].para_shape_id,
                    section.paragraphs[1].para_shape_id
                );
            } else {
                assert!(matches!(result, Err(DocxError::Invalid { part, element, detail })
                    if part == "body" && element == "p" && detail == "unresolved paragraph numbering reference"));
            }
        }
    }

    // 이것을 실패시키는 것: 집계할 경고 변형을 번호 진단 두 종류로 한정.
    #[test]
    fn warning_merge_accepts_other_kinds_and_preserves_count_overflow_error() {
        let make = |count| DocxWarning::ContentLost {
            part: Some("body".into()),
            location: Some("p".into()),
            reason: "first".into(),
            count,
            loss: "merge_continuation",
            action: "omit",
        };
        let mut warnings = vec![make(2)];
        merge_warning(&mut warnings, make(3)).unwrap();
        assert_eq!(warnings, vec![make(5)]);
        let error = merge_warning(&mut warnings, make(u64::MAX)).unwrap_err();
        assert!(
            matches!(error, DocxError::Invalid { detail, .. } if detail == "warning count overflow (u64)")
        );
        assert_eq!(warnings, vec![make(5)]);
    }

    fn layout(gap: i32) -> Option<ListMarkerLayout> {
        Some(
            ListMarkerLayout::new(
                ListMarkerAlign::Left,
                ListTextStart::FromAnchor(unit(gap).unwrap()),
            )
            .unwrap(),
        )
    }

    // 이것을 실패시키는 것: 원본 첫 후보 사용, 최초 None 덮어쓰기, 수준 키 누락, 두 넘침 분류 분리.
    #[test]
    fn final_marker_candidates_keep_none_and_shared_overflow_per_level() {
        for (candidates, shared, individual, expected) in [
            ([layout(50), layout(1000), None], true, false, 0),
            ([None, layout(1000), layout(50)], true, false, 2),
            ([layout(50), None, layout(1000)], true, false, 0),
            ([layout(1000), layout(1000), layout(50)], true, true, 2),
        ] {
            let mut r = resolver();
            let mut warnings = vec![];
            for level in 0..2 {
                for (index, candidate) in candidates.iter().enumerate() {
                    let id = request(&mut r, index as u32 + 1, level, *candidate, &mut warnings);
                    let group = MarkerGroup::Pending(id, level);
                    if shared {
                        r.shared_marker_overflow(group, 100, "body", &mut warnings).unwrap();
                    }
                    if individual {
                        r.emit_marker(
                            group,
                            MarkerDiagnostic::Overflow { uncertain: false },
                            "body",
                            &mut warnings,
                        )
                        .unwrap();
                    }
                }
            }
            let mut section = Section::new(PageSettings::a4());
            for id in [2, 0, 1, 5, 3, 4] {
                section.paragraphs.push(Paragraph::new(ParaShapeIndex::new(id)));
            }
            r.finalize_numbering(&mut [&mut section], "body", &mut warnings).unwrap();
            assert_eq!(r.registry.numberings.len(), 1);
            for level in 0..2 {
                assert_eq!(
                    r.first_marker_layouts.get(&lists::ListRef::Number(0, level)),
                    Some(&candidates[2])
                );
                assert_eq!(
                    r.marker_warnings
                        .contains(&(lists::ListRef::Number(0, level), "list_marker_overflow")),
                    expected != 0
                );
            }
            assert_eq!(
                warnings
                    .iter()
                    .filter(|w| w.loss() == "list_marker_overflow")
                    .map(|w| w.count())
                    .sum::<u64>(),
                expected
            );
            for (p, index) in section.paragraphs.iter().zip([2, 0, 1, 2, 0, 1]) {
                assert_eq!(
                    r.registry.para_shapes[p.para_shape_id.get()].list_marker,
                    candidates[index]
                );
            }
        }
    }
}
