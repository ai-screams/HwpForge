//! Output-only selection of imported paragraph-end formatting.
use crate::{
    EncodeWarning, HwpxCharShape, HwpxError, HwpxResult, HwpxStyleStore, ParagraphPath, PathSeg,
};
use hwpforge_core::{Document, LayoutProfile, Paragraph, RunContent, Validated};
use hwpforge_foundation::{CharShapeIndex, HeadingType, VerticalPosition};
use std::collections::HashSet;

#[derive(Debug, PartialEq, Eq)]
enum Tail {
    Text(CharShapeIndex),
    EmptyParagraph,
    EmptyLastLine,
    Ambiguous,
}
fn paragraph_tail(p: &Paragraph) -> Tail {
    for run in p.runs.iter().rev() {
        let RunContent::Text(text) = &run.content else { return Tail::Ambiguous };
        let Some(c) = text.chars().next_back() else { continue };
        return if matches!(c, '\n' | '\r') {
            Tail::EmptyLastLine
        } else if c.is_whitespace() || c.is_control() {
            Tail::Ambiguous
        } else {
            Tail::Text(run.char_shape_id)
        };
    }
    Tail::EmptyParagraph
}
pub(crate) fn compatible(body: &HwpxCharShape, mark: &HwpxCharShape) -> bool {
    // Exhaustive classification: new character fields require a policy here.
    let HwpxCharShape {
        font_ref,
        height: _,
        text_color: _,
        shade_color: _,
        bold: _,
        italic: _,
        underline_type: _,
        underline_shape: _,
        underline_color: _,
        strikeout_shape: _,
        strikeout_color: _,
        vertical_position,
        outline_type,
        shadow,
        emboss_type,
        engrave_type,
        emphasis,
        ratio,
        spacing,
        rel_sz,
        char_offset,
        use_kerning,
        use_font_space,
        border_fill_id,
    } = mark;
    *font_ref == body.font_ref
        && *vertical_position == VerticalPosition::Normal
        && body.vertical_position == VerticalPosition::Normal
        && *rel_sz == 100
        && body.rel_sz == 100
        && *char_offset == 0
        && body.char_offset == 0
        && *outline_type == body.outline_type
        && *shadow == body.shadow
        && *emboss_type == body.emboss_type
        && *engrave_type == body.engrave_type
        && *emphasis == body.emphasis
        && *ratio == body.ratio
        && *spacing == body.spacing
        && *use_kerning == body.use_kerning
        && *use_font_space == body.use_font_space
        && *border_fill_id == body.border_fill_id
}
pub(crate) fn output_end_char_shape(
    p: &Paragraph,
    store: &HwpxStyleStore,
    profile: LayoutProfile,
) -> HwpxResult<Option<CharShapeIndex>> {
    let Some(original) = p.end_char_shape else { return Ok(None) };
    if !store.imported_paragraph_marks || profile != LayoutProfile::WordCompatible {
        return Ok(Some(original));
    }
    // The same paraPr heading drives marker emission. Preserve even an outline
    // with uncertain marker semantics so its source end run can supply the height.
    if store.para_shape(p.para_shape_id)?.heading_type != HeadingType::None {
        return Ok(Some(original));
    }
    let Tail::Text(id) = paragraph_tail(p) else { return Ok(Some(original)) };
    let body = store.char_shape(id)?;
    let mark = store.char_shape(original)?;
    if body.height.as_i32() <= 0 || mark.height <= body.height || !compatible(body, mark) {
        return Ok(Some(original));
    }
    for run in &p.runs {
        if let RunContent::Text(text) = &run.content {
            if !text.is_empty() && store.char_shape(run.char_shape_id)?.height >= mark.height {
                return Ok(Some(original));
            }
        }
    }
    Ok(Some(id))
}
fn invalid(detail: &str) -> HwpxError {
    HwpxError::InvalidStructure { detail: detail.into() }
}
pub(crate) struct Projection<'a> {
    store: &'a HwpxStyleStore,
    profile: LayoutProfile,
    prepared: HashSet<Vec<PathSeg>>,
    emitted: HashSet<Vec<PathSeg>>,
    first: Option<ParagraphPath>,
    count: u64,
}
impl Projection<'_> {
    pub(crate) fn changed(&self) -> bool {
        !self.prepared.is_empty()
    }
    pub(crate) fn output_id(
        &mut self,
        p: &Paragraph,
        path: &[PathSeg],
    ) -> HwpxResult<Option<CharShapeIndex>> {
        let id = output_end_char_shape(p, self.store, self.profile)?;
        if id != p.end_char_shape {
            if !self.emitted.insert(path.to_vec()) {
                return Err(invalid("Paragraph-end mark emitted more than once"));
            }
            self.count = self
                .count
                .checked_add(1)
                .ok_or_else(|| invalid("Paragraph-end mark count overflow"))?;
            if self.first.is_none() {
                self.first = Some(ParagraphPath(path.to_vec()));
            }
        }
        Ok(id)
    }
    pub(crate) fn finish(&self) -> HwpxResult<()> {
        if self.prepared != self.emitted
            || u64::try_from(self.emitted.len()).ok() != Some(self.count)
        {
            return Err(invalid("Paragraph-end mark preparation and emission disagree"));
        }
        Ok(())
    }
    pub(crate) fn warning(&self) -> Option<EncodeWarning> {
        self.first
            .clone()
            .map(|path| EncodeWarning::ParagraphEndMarkReplaced { path, count: self.count })
    }
}

pub(crate) fn prepare<'a>(
    document: &Document<Validated>,
    store: &'a HwpxStyleStore,
) -> HwpxResult<Projection<'a>> {
    let mut result = Projection {
        store,
        profile: document.layout_profile(),
        prepared: HashSet::new(),
        emitted: HashSet::new(),
        first: None,
        count: 0,
    };
    if !store.imported_paragraph_marks {
        return Ok(result);
    }
    let mut visitor = Visitor { result: &mut result, store, profile: document.layout_profile() };
    let mut master = 0;
    for (si, s) in document.sections().iter().enumerate() {
        visitor.paragraphs(&s.paragraphs, &mut vec![PathSeg::Section(si)], true)?;
        for (i, h) in s.headers.iter().enumerate() {
            visitor.paragraphs(
                &h.paragraphs,
                &mut vec![PathSeg::Section(si), PathSeg::Header(i)],
                false,
            )?;
        }
        for (i, h) in s.footers.iter().enumerate() {
            visitor.paragraphs(
                &h.paragraphs,
                &mut vec![PathSeg::Section(si), PathSeg::Footer(i)],
                false,
            )?;
        }
        for m in s.master_pages.iter().flatten() {
            visitor.paragraphs(
                &m.paragraphs,
                &mut vec![PathSeg::Section(si), PathSeg::MasterPage(master)],
                false,
            )?;
            master += 1;
        }
    }
    Ok(result)
}
struct Visitor<'a, 's> {
    result: &'a mut Projection<'s>,
    store: &'s HwpxStyleStore,
    profile: LayoutProfile,
}
impl Visitor<'_, '_> {
    fn paragraphs(
        &mut self,
        paragraphs: &[Paragraph],
        path: &mut Vec<PathSeg>,
        body: bool,
    ) -> HwpxResult<()> {
        for (i, p) in paragraphs.iter().enumerate() {
            path.push(if body { PathSeg::BodyParagraph(i) } else { PathSeg::NestedParagraph(i) });
            if output_end_char_shape(p, self.store, self.profile)? != p.end_char_shape
                && !self.result.prepared.insert(path.clone())
            {
                return Err(invalid("Paragraph-end mark prepared more than once"));
            }
            for (i, run) in p.runs.iter().enumerate() {
                let parent = std::mem::replace(path, {
                    let mut key = path.clone();
                    key.push(PathSeg::Run(i));
                    key
                });
                match &run.content {
                    RunContent::Table(t) => {
                        if let Some(c) = &t.caption {
                            self.caption(c, path)?;
                        }
                        for (r, row) in t.rows.iter().enumerate() {
                            for (c, cell) in row.cells.iter().enumerate() {
                                path.push(PathSeg::TableCell { row: r, cell: c });
                                self.paragraphs(&cell.paragraphs, path, false)?;
                                path.pop();
                            }
                        }
                    }
                    RunContent::Image(i) => {
                        if let Some(c) = &i.caption {
                            self.caption(c, path)?;
                        }
                    }
                    RunContent::Control(c) => self.control(c, path)?,
                    _ => {}
                }
                *path = parent;
            }
            path.pop();
        }
        Ok(())
    }
    fn caption(
        &mut self,
        c: &hwpforge_core::caption::Caption,
        path: &mut Vec<PathSeg>,
    ) -> HwpxResult<()> {
        path.push(PathSeg::Caption);
        self.paragraphs(&c.paragraphs, path, false)?;
        path.pop();
        Ok(())
    }
    fn control(
        &mut self,
        c: &hwpforge_core::control::Control,
        path: &mut Vec<PathSeg>,
    ) -> HwpxResult<()> {
        use hwpforge_core::control::{Control, GroupChild};
        match c {
            Control::Footnote { paragraphs, .. } | Control::Endnote { paragraphs, .. } => {
                path.push(if matches!(c, Control::Footnote { .. }) {
                    PathSeg::Footnote
                } else {
                    PathSeg::Endnote
                });
                self.paragraphs(paragraphs, path, false)?;
                path.pop();
            }
            Control::TextBox { paragraphs, caption, .. }
            | Control::Ellipse { paragraphs, caption, .. }
            | Control::Polygon { paragraphs, caption, .. } => {
                path.push(PathSeg::TextBox);
                self.paragraphs(paragraphs, path, false)?;
                path.pop();
                if let Some(c) = caption {
                    self.caption(c, path)?;
                }
            }
            Control::Rect { caption, .. }
            | Control::Line { caption, .. }
            | Control::Arc { caption, .. }
            | Control::Curve { caption, .. }
            | Control::ConnectLine { caption, .. } => {
                if let Some(c) = caption {
                    self.caption(c, path)?;
                }
            }
            Control::FieldBoundary { boundary }
                if crate::encoder::section::field_boundary_is_encodable(boundary) =>
            {
                if let hwpforge_core::control::FieldBoundary::Start(start) = boundary {
                    if let Some(content) = &start.annotation {
                        path.push(PathSeg::Memo);
                        self.paragraphs(content, path, false)?;
                        path.pop();
                    }
                }
            }
            Control::Memo { content, .. } => {
                path.push(PathSeg::Memo);
                self.paragraphs(content, path, false)?;
                path.pop();
            }
            Control::Group { children, .. } => {
                let mut i = 0;
                for c in children {
                    if let GroupChild::Shape(shape) = c {
                        if !crate::encoder::shapes::group_child_is_encodable(shape) {
                            continue;
                        }
                    }
                    path.push(PathSeg::GroupChild(i));
                    match c {
                        GroupChild::Shape(c) => self.control(c, path)?,
                        GroupChild::Picture(p) => {
                            if let Some(c) = &p.caption {
                                self.caption(c, path)?;
                            }
                        }
                        _ => {}
                    }
                    path.pop();
                    i += 1;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_core::Run;
    use hwpforge_foundation::*;
    fn fixture() -> (Paragraph, HwpxStyleStore) {
        let mut s = HwpxStyleStore::with_default_fonts("Cambria");
        let mut c = HwpxCharShape { height: HwpUnit::new(1100).unwrap(), ..Default::default() };
        s.push_char_shape(c.clone());
        c.height = HwpUnit::new(2000).unwrap();
        s.push_char_shape(c);
        s.imported_paragraph_marks = true;
        s.push_para_shape(crate::HwpxParaShape::default());
        let mut p = Paragraph::with_runs(
            vec![Run::text("A", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        p.end_char_shape = Some(CharShapeIndex::new(1));
        (p, s)
    }
    // 이것을 실패시키는 것: 언어별 글꼴/수직 위치/상대 크기/offset/보수적 동일 요구 gate 중 하나 삭제.
    #[test]
    fn geometric_character_fields_each_require_compatibility() {
        type Change = fn(&mut HwpxCharShape);
        let cases: &[(&str, Change)] = &[
            ("hangul", |c| c.font_ref.hangul = FontIndex::new(1)),
            ("latin", |c| c.font_ref.latin = FontIndex::new(1)),
            ("hanja", |c| c.font_ref.hanja = FontIndex::new(1)),
            ("japanese", |c| c.font_ref.japanese = FontIndex::new(1)),
            ("other", |c| c.font_ref.other = FontIndex::new(1)),
            ("symbol", |c| c.font_ref.symbol = FontIndex::new(1)),
            ("user", |c| c.font_ref.user = FontIndex::new(1)),
            ("superscript", |c| c.vertical_position = VerticalPosition::Superscript),
            ("subscript", |c| c.vertical_position = VerticalPosition::Subscript),
            ("relative", |c| c.rel_sz = 99),
            ("offset", |c| c.char_offset = 1),
            ("outline", |c| c.outline_type = OutlineType::Solid),
            ("shadow", |c| c.shadow = CharShadow::new(ShadowType::Drop)),
            ("emboss", |c| c.emboss_type = EmbossType::Emboss),
            ("engrave", |c| c.engrave_type = EngraveType::Engrave),
            ("emphasis", |c| c.emphasis = EmphasisType::DotAbove),
            ("ratio", |c| c.ratio = 101),
            ("spacing", |c| c.spacing = 1),
            ("kerning", |c| c.use_kerning = !c.use_kerning),
            ("font_space", |c| c.use_font_space = !c.use_font_space),
            ("border", |c| c.border_fill_id = Some(3)),
        ];
        for (name, change) in cases {
            for id in [0, 1] {
                let (p, mut s) = fixture();
                change(s.char_shape_mut(CharShapeIndex::new(id)).unwrap());
                assert_eq!(
                    output_end_char_shape(&p, &s, LayoutProfile::WordCompatible).unwrap(),
                    p.end_char_shape,
                    "{name}/{id}"
                );
            }
        }
        let (p, mut s) = fixture();
        s.char_shape_mut(CharShapeIndex::new(0)).unwrap().height = HwpUnit::ZERO;
        assert_eq!(
            output_end_char_shape(&p, &s, LayoutProfile::WordCompatible).unwrap(),
            p.end_char_shape
        );
    }
    // 이것을 실패시키는 것: 페인트/face만의 차이로 치환하거나 큰 끝 표시의 부수 서식 차이를 거부.
    #[test]
    fn paint_fields_do_not_trigger_or_prevent_size_replacement() {
        type Change = fn(&mut HwpxCharShape);
        let changes: &[Change] = &[
            |c| c.bold = true,
            |c| c.italic = true,
            |c| c.text_color = Color::RED,
            |c| c.shade_color = Some(Color::RED),
            |c| c.underline_type = UnderlineType::Bottom,
            |c| c.underline_shape = UnderlineShape::Dash,
            |c| c.underline_color = Some(Color::RED),
            |c| c.strikeout_shape = StrikeoutShape::Solid,
            |c| c.strikeout_color = Some(Color::RED),
        ];
        for change in changes {
            for (height, expected) in [(900, 1), (1100, 1), (2000, 0)] {
                let (p, mut s) = fixture();
                let c = s.char_shape_mut(CharShapeIndex::new(1)).unwrap();
                c.height = HwpUnit::new(height).unwrap();
                change(c);
                assert_eq!(
                    output_end_char_shape(&p, &s, LayoutProfile::WordCompatible).unwrap(),
                    Some(CharShapeIndex::new(expected))
                );
            }
        }
    }
    // 이것을 실패시키는 것: import bool/profile gate 삭제, 상태를 JSON에 노출하거나 정의 동등성에 포함.
    #[test]
    fn authority_is_private_nonserialized_and_profile_scoped() {
        let (p, s) = fixture();
        let mut plain = s.clone();
        plain.imported_paragraph_marks = false;
        assert_ne!(plain, s);
        assert!(plain.same_definitions(&s));
        assert_eq!(serde_json::to_value(&plain).unwrap(), serde_json::to_value(&s).unwrap());
        let mut wire = serde_json::to_value(&s).unwrap();
        wire["imported_paragraph_marks"] = serde_json::json!(true);
        let restored: HwpxStyleStore = serde_json::from_value(wire).unwrap();
        assert!(!restored.imported_paragraph_marks);
        assert_eq!(
            output_end_char_shape(&p, &plain, LayoutProfile::WordCompatible).unwrap(),
            p.end_char_shape
        );
        assert_eq!(output_end_char_shape(&p, &s, LayoutProfile::Hancom).unwrap(), p.end_char_shape);
        assert_eq!(
            output_end_char_shape(&p, &s, LayoutProfile::WordCompatible).unwrap(),
            Some(CharShapeIndex::new(0))
        );
        #[cfg(feature = "schemars")]
        assert_eq!(
            serde_json::to_value(schemars::schema_for!(HwpxStyleStore))
                .unwrap()
                .pointer("/properties/imported_paragraph_marks"),
            None
        );
    }
    fn projection(s: &HwpxStyleStore, paths: Vec<Vec<PathSeg>>) -> Projection<'_> {
        Projection {
            store: s,
            profile: LayoutProfile::WordCompatible,
            prepared: paths.into_iter().collect(),
            emitted: HashSet::new(),
            first: None,
            count: 0,
        }
    }
    // 이것을 실패시키는 것: 집합 비교를 개수 비교로 축소, 검증 삭제, 중복 은폐, checked count를 포화 처리.
    #[test]
    fn preparation_requires_identical_paths_and_exactly_once_emission() {
        let (p, s) = fixture();
        let a = vec![PathSeg::Section(0), PathSeg::BodyParagraph(0)];
        let b = vec![PathSeg::Section(0), PathSeg::BodyParagraph(1)];
        let mut plan = projection(&s, vec![a.clone()]);
        assert!(plan.finish().is_err());
        plan.output_id(&p, &b).unwrap();
        assert!(plan.finish().is_err());
        let mut plan = projection(&s, vec![]);
        plan.output_id(&p, &a).unwrap();
        assert!(plan.finish().is_err());
        let mut plan = projection(&s, vec![a.clone(), b.clone()]);
        plan.output_id(&p, &a).unwrap();
        plan.output_id(&p, &b).unwrap();
        plan.finish().unwrap();
        assert_eq!(plan.count, 2);
        assert_eq!(plan.first, Some(ParagraphPath(a.clone())));
        assert!(plan.output_id(&p, &a).is_err());
        let mut plan = projection(&s, vec![a.clone()]);
        plan.count = u64::MAX;
        assert!(plan.output_id(&p, &a).is_err());
        assert!(plan.finish().is_err());
    }
    // 이것을 실패시키는 것: 의미 경고를 advisory로 분류하거나 편의 API에서 의미 경고를 오류로 승격.
    #[test]
    fn replacement_warning_is_semantic_and_editors_fail_closed() {
        let w = EncodeWarning::ParagraphEndMarkReplaced {
            path: ParagraphPath(vec![PathSeg::Section(0), PathSeg::BodyParagraph(3)]),
            count: 2,
        };
        assert!(w.is_semantic_loss());
        assert_eq!(w.code(), "PARAGRAPH_END_MARK_REPLACED");
        assert_eq!(w.part(), None);
        assert_eq!(w.location().as_deref(), Some("section[0].para[3]"));
        assert_eq!(w.message(),"Imported paragraph-end formatting was replaced with the final text run's formatting to avoid increasing line height. The original paragraph-end formatting is not recoverable from this HWPX output. Affected paragraphs: 2. Only the first location is shown.");
        assert_eq!(w.to_string(), format!("section[0].para[3]: {}", w.message()));
        assert!(crate::encoder::split_successful_encode(crate::EncodeOutcome {
            bytes: vec![1],
            warnings: vec![w]
        })
        .is_err());
    }
    // 이것을 실패시키는 것: 다른 import map이 비었을 때 bool 제거 생략, no-op 비교 뒤로 제거 지연.
    #[test]
    fn base_encoding_discards_mark_authority_before_comparison_and_edit() {
        use crate::{EncodeOptions, HwpxDecoder, HwpxEncoder, HwpxParaShape};
        use hwpforge_core::{Document, ImageStore, PageSettings, Section};
        for profile in [LayoutProfile::Hancom, LayoutProfile::WordCompatible] {
            let (p, mut s) = fixture();
            s.imported_paragraph_marks = false;
            s.push_para_shape(HwpxParaShape::default());
            let mut d = Document::new();
            d.set_layout_profile(profile);
            d.add_section(Section::with_paragraphs(vec![p], PageSettings::a4()));
            let bytes =
                HwpxEncoder::encode(&d.validate().unwrap(), &s, &ImageStore::new()).unwrap();
            let bytes = crate::cell_edit::test_package::splice(
                &bytes,
                "<hp:t>A</hp:t>",
                "<hp:t>A</hp:t><hp:unknown/>",
            );
            let decoded = HwpxDecoder::decode(&bytes).unwrap();
            let plain = decoded.style_store.clone();
            let mut state = plain.clone();
            state.imported_paragraph_marks = true;
            let d = decoded.document.validate().unwrap();
            let before = state.clone();
            let out = HwpxEncoder::encode_with_base(
                &bytes,
                &d,
                &state,
                &decoded.image_store,
                EncodeOptions::default(),
            );
            assert!(matches!(&out,Ok(value) if value.bytes==bytes));
            let out = out.unwrap();
            assert_eq!(out.bytes, bytes);
            assert_eq!(state, before);
            let bytes = crate::cell_edit::test_package::splice(&bytes, "<hp:unknown/>", "");
            let decoded = HwpxDecoder::decode(&bytes).unwrap();
            let plain = decoded.style_store.clone();
            let mut state = plain.clone();
            state.imported_paragraph_marks = true;
            let mut d = decoded.document;
            d.sections_mut()[0].paragraphs[0].runs[0].content = RunContent::Text("edited".into());
            let d = d.validate().unwrap();
            let a = HwpxEncoder::encode_with_base(
                &bytes,
                &d,
                &state,
                &decoded.image_store,
                EncodeOptions::default(),
            )
            .unwrap();
            let b = HwpxEncoder::encode_with_base(
                &bytes,
                &d,
                &plain,
                &decoded.image_store,
                EncodeOptions::default(),
            )
            .unwrap();
            assert_eq!(a.bytes, b.bytes);
            assert_eq!(a.warnings, b.warnings);
        }
    }
    // 이것을 실패시키는 것: 최초 자격을 문단 map에 고정하거나 원본 표시를 Core에 덮어쓰기.
    #[test]
    fn current_text_and_size_edits_recompute_mark_eligibility() {
        let (mut p, mut s) = fixture();
        for (text, expected) in [("A", 0), ("A\n", 1), ("A", 0)] {
            p.runs[0].content = RunContent::Text(text.into());
            assert_eq!(
                output_end_char_shape(&p, &s, LayoutProfile::WordCompatible).unwrap(),
                Some(CharShapeIndex::new(expected))
            );
        }
        s.char_shape_mut(CharShapeIndex::new(1)).unwrap().height = HwpUnit::new(900).unwrap();
        assert_eq!(
            output_end_char_shape(&p, &s, LayoutProfile::WordCompatible).unwrap(),
            p.end_char_shape
        );
        s.char_shape_mut(CharShapeIndex::new(1)).unwrap().height = HwpUnit::new(2000).unwrap();
        s.char_shape_mut(CharShapeIndex::new(0)).unwrap().height = HwpUnit::new(2000).unwrap();
        assert_eq!(
            output_end_char_shape(&p, &s, LayoutProfile::WordCompatible).unwrap(),
            p.end_char_shape
        );
    }

    // 이것을 실패시키는 것: 방출하지 않는 묶음 자식까지 방문하거나 준비 경로 중복을 은폐.
    #[test]
    fn omitted_group_subtrees_and_duplicate_preparation_are_checked() {
        use hwpforge_core::{
            control::{Control, GroupChild},
            ObjectFrame,
        };
        let (p, s) = fixture();
        let mut plan = projection(&s, vec![]);
        let group = Control::Group {
            children: vec![GroupChild::Shape(Control::footnote(vec![p.clone()]))],
            width: HwpUnit::new(10000).unwrap(),
            height: HwpUnit::new(10000).unwrap(),
            frame: ObjectFrame::new(),
            placement: None,
            inst_id: None,
        };
        let mut visitor =
            Visitor { result: &mut plan, store: &s, profile: LayoutProfile::WordCompatible };
        visitor.control(&group, &mut vec![PathSeg::Section(0)]).unwrap();
        assert!(!plan.changed());
        assert_eq!(plan.count, 0);
        let mut visitor =
            Visitor { result: &mut plan, store: &s, profile: LayoutProfile::WordCompatible };
        let mut path = vec![PathSeg::Section(0)];
        visitor.paragraphs(std::slice::from_ref(&p), &mut path, true).unwrap();
        assert!(visitor.paragraphs(&[p], &mut path, true).is_err());
    }

    // 이것을 실패시키는 것: 필드 경계를 건너 앞의 일반 Text를 끝으로 고름.
    #[test]
    fn field_boundaries_do_not_supply_plain_text_tails() {
        use hwpforge_core::control::{Control, FieldBoundary};
        for boundary in [
            FieldBoundary::Start(Box::default()),
            FieldBoundary::End { begin_id: "field".into(), field_id: String::new() },
        ] {
            let (mut p, s) = fixture();
            p.runs.push(Run::control(Control::FieldBoundary { boundary }, CharShapeIndex::new(0)));
            assert_eq!(
                output_end_char_shape(&p, &s, LayoutProfile::WordCompatible).unwrap(),
                p.end_char_shape
            );
        }
    }
    // 이것을 실패시키는 것: import 권한만 있고 실제 치환은 없어도 요청한 캐시를 억제.
    #[test]
    fn authority_without_replacements_keeps_requested_caches() {
        use crate::{EncodeOptions, HwpxEncoder, HwpxParaShape};
        use hwpforge_core::{
            layout::{LayoutCache, LineSeg},
            Document, ImageStore, PageSettings, Section,
        };
        let (mut p, mut s) = fixture();
        s.push_para_shape(HwpxParaShape::default());
        s.char_shape_mut(CharShapeIndex::new(1)).unwrap().height = HwpUnit::new(1100).unwrap();
        let mut line = LineSeg::default();
        line.vertsize = 1100;
        line.textheight = 1100;
        line.horzsize = 10000;
        p.layout_cache = Some(LayoutCache::new(vec![line]));
        let mut d = Document::new();
        d.set_layout_profile(LayoutProfile::WordCompatible);
        d.add_section(Section::with_paragraphs(vec![p], PageSettings::a4()));
        let o = HwpxEncoder::encode_with_diagnostics(
            &d.validate().unwrap(),
            &s,
            &ImageStore::new(),
            EncodeOptions::default().with_emit_layout_cache(true),
        )
        .unwrap();
        assert_eq!(
            o.warnings
                .iter()
                .filter(|w| w.code() == "PARAGRAPH_END_MARK_REPLACED"
                    || w.code() == "LAYOUT_CACHE_DROPPED")
                .count(),
            0
        );
    }
}
