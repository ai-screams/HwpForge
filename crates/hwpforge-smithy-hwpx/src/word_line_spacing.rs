//! Output-only projection of imported percentage spacing for measured Latin faces.
use crate::{HwpxCharShape, HwpxError, HwpxResult, HwpxStyleStore, PathSeg};
use hwpforge_core::{Document, LayoutProfile, Paragraph, RunContent, Validated};
use hwpforge_foundation::{HeadingType, LineSpacingType, ParaShapeIndex, VerticalPosition};
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
};

type Key = (u32, i32);

// Font metrics from the measured Hancom Cambria 6.82m and Calibri 5.87m.
// The denominator includes hhea lineGap; apply the ratio before rounding.
fn ratio(family: &str) -> Option<(f64, f64)> {
    match family {
        "Cambria" => Some((2401., 2754.)),
        "Calibri" => Some((2500., 2952.)),
        _ => None,
    }
}
fn text_shape<'a>(store: &'a HwpxStyleStore, shape: &HwpxCharShape) -> Option<(&'a str, i32)> {
    if shape.vertical_position != VerticalPosition::Normal
        || shape.rel_sz != 100
        || shape.char_offset != 0
        || shape.height.as_i32() <= 0
    {
        return None;
    }
    let family = &store
        .iter_fonts()
        .filter(|f| f.lang == "LATIN")
        .nth(shape.font_ref.latin.get())?
        .face_name;
    ratio(family)?;
    Some((family, shape.height.as_i32()))
}
fn candidate(
    p: &Paragraph,
    store: &HwpxStyleStore,
    profile: LayoutProfile,
    in_body: bool,
) -> Option<Key> {
    if !in_body || profile != LayoutProfile::WordCompatible {
        return None;
    }
    let position = u32::try_from(p.para_shape_id.get()).ok()?;
    let original = *store.imported_percentages.get(&position)?;
    let para = store.para_shape(p.para_shape_id).ok()?;
    // Equality with the current i32 also excludes nonfinite and out-of-range values.
    if original <= 0.
        || para.line_spacing_type != LineSpacingType::Percentage
        || original.round() != f64::from(para.line_spacing)
        || para.heading_type != HeadingType::None
    {
        return None;
    }
    let mut first = None;
    let mut last = None;
    let mut text = String::new();
    for run in &p.runs {
        let RunContent::Text(value) = &run.content else {
            return None;
        };
        if value.is_empty() {
            continue;
        }
        let shape = store.char_shape(run.char_shape_id).ok()?;
        let identity = text_shape(store, shape)?;
        if first.is_some_and(|previous| previous != identity) {
            return None;
        }
        first = Some(identity);
        last = Some(shape);
        text.push_str(value);
    }
    let (family, _) = first?;
    if !text.chars().all(|c| (' '..='~').contains(&c) || c == '\n')
        || text.split('\n').any(|line| line.trim_matches(' ').is_empty())
    {
        return None;
    }
    if let Some(id) = p.end_char_shape {
        let mark = store.char_shape(id).ok()?;
        let last = last?;
        if Some(text_shape(store, mark)?) != first
            || mark.bold != last.bold
            || mark.italic != last.italic
        {
            return None;
        }
    }
    let (numerator, denominator) = ratio(family)?;
    let value = (original * numerator / denominator).round() as i32;
    (value != para.line_spacing).then_some((position, value))
}

// The encoder already owns the container path. Allow only body and table-cell
// ancestry, so a table inside an excluded container stays excluded too.
pub(crate) fn body_scope(path: &[PathSeg]) -> bool {
    path.iter().all(|segment| {
        matches!(
            segment,
            PathSeg::Section(_)
                | PathSeg::BodyParagraph(_)
                | PathSeg::Run(_)
                | PathSeg::TableCell { .. }
                | PathSeg::NestedParagraph(_)
        )
    })
}

pub(crate) struct Projection {
    original: HwpxStyleStore,
    profile: LayoutProfile,
    definitions: BTreeMap<Key, u32>,
    emitted: BTreeSet<Key>,
}
impl Projection {
    pub(crate) fn output_id(&mut self, p: &Paragraph, in_body: bool) -> HwpxResult<Option<u32>> {
        let Some(key) = candidate(p, &self.original, self.profile, in_body) else {
            return Ok(None);
        };
        let id = self.definitions.get(&key).copied().ok_or_else(mismatch)?;
        self.emitted.insert(key);
        Ok(Some(id))
    }
    pub(crate) fn finish(&self) -> HwpxResult<()> {
        if self.definitions.keys().copied().collect::<BTreeSet<_>>() != self.emitted {
            return Err(mismatch());
        }
        Ok(())
    }
}
fn mismatch() -> HwpxError {
    HwpxError::InvalidStructure {
        detail: "Imported line spacing preparation and emission disagree".into(),
    }
}
pub(crate) fn prepare<'a>(
    document: &Document<Validated>,
    store: &'a HwpxStyleStore,
) -> HwpxResult<(Cow<'a, HwpxStyleStore>, Option<Projection>)> {
    fn collect(
        paragraphs: &[Paragraph],
        store: &HwpxStyleStore,
        profile: LayoutProfile,
        keys: &mut BTreeSet<Key>,
    ) {
        for p in paragraphs {
            if let Some(key) = candidate(p, store, profile, true) {
                keys.insert(key);
            }
            for run in &p.runs {
                if let RunContent::Table(table) = &run.content {
                    for row in &table.rows {
                        for cell in &row.cells {
                            collect(&cell.paragraphs, store, profile, keys);
                        }
                    }
                }
            }
        }
    }
    let profile = document.layout_profile();
    let mut keys = BTreeSet::new();
    for section in document.sections() {
        collect(&section.paragraphs, store, profile, &mut keys);
    }
    if keys.is_empty() {
        return Ok((Cow::Borrowed(store), None));
    }
    let mut output = store.clone();
    let mut definitions = BTreeMap::new();
    for (position, value) in keys {
        let mut shape = store.para_shape(ParaShapeIndex::new(position as usize))?.clone();
        shape.line_spacing = value;
        let id = u32::try_from(output.push_para_shape(shape).get()).map_err(|_| {
            HwpxError::InvalidStructure { detail: "paragraph shape position overflow".into() }
        })?;
        if let Some(fidelity) = store.header_fidelity.auto_spacing.get(&position) {
            output.header_fidelity.auto_spacing.insert(id, *fidelity);
        }
        definitions.insert((position, value), id);
    }
    Ok((
        Cow::Owned(output),
        Some(Projection {
            original: store.clone(),
            profile,
            definitions,
            emitted: BTreeSet::new(),
        }),
    ))
}
#[cfg(test)]
use crate::{cell_edit::test_package, test_support::xml as test_xml};
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EncodeOptions, HwpxDecoder, HwpxEncoder, HwpxParaShape};
    use hwpforge_core::{ImageStore, PageSettings, Run, Section};
    use hwpforge_foundation::{CharShapeIndex, FontIndex};
    fn fixture() -> (Document<Validated>, HwpxStyleStore) {
        let mut s = HwpxStyleStore::with_default_fonts("Cambria");
        s.push_char_shape(HwpxCharShape::default());
        s.push_para_shape(HwpxParaShape { line_spacing: 100, ..Default::default() });
        s.imported_percentages.insert(0, 100.);
        let mut d = Document::new();
        d.set_layout_profile(LayoutProfile::WordCompatible);
        d.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::text("ASCII", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        ));
        (d.validate().unwrap(), s)
    }
    // 이것을 실패시키는 것: 비유한/비양수/범위/상태 검사 제거, LATIN 대신 전체 벡터나 HANGUL 조회.
    #[test]
    fn word_line_spacing_state_and_language_groups() {
        let (d, mut s) = fixture();
        let p = &d.sections()[0].paragraphs[0];
        for original in [f64::NAN, f64::INFINITY, -1., 0., f64::from(i32::MAX) + 1., 160.] {
            s.imported_percentages.insert(0, original);
            assert_eq!(candidate(p, &s, d.layout_profile(), true), None);
        }
        s.imported_percentages.clear();
        s.imported_percentages.insert(9, 100.);
        assert_eq!(candidate(p, &s, d.layout_profile(), true), None);
        s.imported_percentages.insert(0, 100.);
        assert_eq!(candidate(p, &s, d.layout_profile(), false), None);
        // The same numeric ID refers to different families in different groups.
        s.push_font(crate::HwpxFont::new(1, "Cambria", "HANGUL"));
        s.push_font(crate::HwpxFont::new(1, "Arial", "LATIN"));
        s.char_shape_mut(CharShapeIndex::new(0)).unwrap().font_ref.latin = FontIndex::new(1);
        assert_eq!(candidate(p, &s, d.layout_profile(), true), None);
        s.char_shape_mut(CharShapeIndex::new(0)).unwrap().font_ref.latin = FontIndex::new(0);
        assert_eq!(candidate(p, &s, d.layout_profile(), true), Some((0, 87)));
        let mut control = p.clone();
        control.runs[0] = Run::text("a\u{1f}b", CharShapeIndex::new(0));
        assert_eq!(candidate(&control, &s, d.layout_profile(), true), None);
        s.char_shape_mut(CharShapeIndex::new(0)).unwrap().height =
            hwpforge_foundation::HwpUnit::ZERO;
        assert_eq!(candidate(p, &s, d.layout_profile(), true), None);
        s.char_shape_mut(CharShapeIndex::new(0)).unwrap().height =
            hwpforge_foundation::HwpUnit::new(1000).unwrap();
        s.imported_percentages.insert(0, -100.);
        s.para_shape_mut(ParaShapeIndex::new(0)).unwrap().line_spacing = -100;
        assert_eq!(candidate(p, &s, d.layout_profile(), true), None);
        let mut plain = s.clone();
        plain.imported_percentages.clear();
        assert_ne!(plain, s);
        assert!(plain.same_definitions(&s));
    }
    // 이것을 실패시키는 것: 사전/방출 키 검증 삭제, 원본 ID를 출력 파생 범위로 검증, fidelity 누락, 동일값 파생.
    #[test]
    fn word_line_spacing_projection_consistency_and_fidelity() {
        let (d, mut s) = fixture();
        s.header_fidelity.auto_spacing.insert(0, Some([0, 1]));
        s.para_shape_mut(ParaShapeIndex::new(0)).unwrap().indent =
            hwpforge_core::ParagraphMeasure::from_hwp_unit(
                hwpforge_foundation::HwpUnit::new(125).unwrap(),
            );

        let (out, projection) = prepare(&d, &s).unwrap();
        let mut projection = projection.unwrap();
        assert!(matches!(projection.finish(), Err(HwpxError::InvalidStructure { .. })));
        let p = &d.sections()[0].paragraphs[0];
        assert_eq!(projection.output_id(p, true).unwrap(), Some(1));
        projection.finish().unwrap();
        assert_eq!(out.header_fidelity.auto_spacing.get(&1), Some(&Some([0, 1])));
        assert_eq!(out.imported_percentages.len(), 1);
        let mut expected = s.para_shape(ParaShapeIndex::new(0)).unwrap().clone();
        expected.line_spacing = 87;
        assert_eq!(out.para_shape(ParaShapeIndex::new(1)).unwrap(), &expected);

        projection.definitions.clear();
        assert!(matches!(projection.output_id(p, true), Err(HwpxError::InvalidStructure { .. })));
        let mut invalid = serde_json::from_value::<Document<hwpforge_core::Draft>>(
            serde_json::to_value(&d).unwrap(),
        )
        .unwrap();
        invalid.sections_mut()[0].paragraphs.push(Paragraph::with_runs(
            vec![Run::text("bad", CharShapeIndex::new(0))],
            ParaShapeIndex::new(1),
        ));
        assert!(HwpxEncoder::encode(&invalid.validate().unwrap(), &s, &ImageStore::new()).is_err());
        s.para_shape_mut(ParaShapeIndex::new(0)).unwrap().line_spacing = 1;
        s.imported_percentages.insert(0, 1.);
        let (output, projection) = prepare(&d, &s).unwrap();
        assert!(projection.is_none());
        assert!(matches!(output, Cow::Borrowed(_)));
    }
    // 이것을 실패시키는 것: base 무변경 비교 뒤에만 상태 제거, 내부 재인코드에 상태 전달, 호출자 상태 소비.
    #[test]
    fn word_line_spacing_base_ignores_state_before_fast_path_and_edits() {
        for profile in [LayoutProfile::Hancom, LayoutProfile::WordCompatible] {
            let (d, mut s) = fixture();
            s.imported_percentages.clear();
            let mut d = serde_json::from_value::<Document<hwpforge_core::Draft>>(
                serde_json::to_value(&d).unwrap(),
            )
            .unwrap();
            d.set_layout_profile(profile);
            let bytes =
                HwpxEncoder::encode(&d.validate().unwrap(), &s, &ImageStore::new()).unwrap();
            let bytes = test_package::splice(
                &bytes,
                "<hp:t>ASCII</hp:t>",
                "<hp:t>ASCII</hp:t><hp:unknown/>",
            );
            let decoded = HwpxDecoder::decode(&bytes).unwrap();
            let plain = decoded.style_store.clone();
            let mut state = plain.clone();
            let id =
                u32::try_from(decoded.document.sections()[0].paragraphs[0].para_shape_id.get())
                    .unwrap();
            state.imported_percentages.insert(id, 100.);
            state.imported_percentages.insert(999, f64::INFINITY);
            let doc = decoded.document.clone().validate().unwrap();
            let before = state.clone();
            let a = HwpxEncoder::encode_with_base(
                &bytes,
                &doc,
                &state,
                &decoded.image_store,
                EncodeOptions::default(),
            )
            .unwrap();
            let b = HwpxEncoder::encode_with_base(
                &bytes,
                &doc,
                &plain,
                &decoded.image_store,
                EncodeOptions::default(),
            )
            .unwrap();
            assert_eq!(a.bytes, bytes);
            assert_eq!(a.bytes, b.bytes);
            assert_eq!(a.warnings, b.warnings);
            assert_eq!(state, before);
            // Remove unsupported source markup for a legitimate edit; no-op above must retain it.
            let clean = test_package::splice(&bytes, "<hp:unknown/>", "");
            let decoded = HwpxDecoder::decode(&clean).unwrap();
            let plain = decoded.style_store.clone();
            let mut state = plain.clone();
            state.imported_percentages.insert(id, 100.);
            let mut edit = decoded.document;
            edit.sections_mut()[0].paragraphs[0].runs[0].content =
                RunContent::Text("edited".into());
            let edit = edit.validate().unwrap();
            let a = HwpxEncoder::encode_with_base(
                &clean,
                &edit,
                &state,
                &decoded.image_store,
                EncodeOptions::default(),
            )
            .unwrap();
            let b = HwpxEncoder::encode_with_base(
                &clean,
                &edit,
                &plain,
                &decoded.image_store,
                EncodeOptions::default(),
            )
            .unwrap();
            assert_eq!(a.bytes, b.bytes);
            assert_eq!(a.warnings, b.warnings);
        }
    }
    // 이것을 실패시키는 것: 억제된 캐시의 폐기 표지 생성 또는 scope의 보조 컨테이너 허용.
    #[test]
    fn word_line_spacing_cache_markers_and_excluded_scopes() {
        let (d, s) = fixture();
        let mut d: Document<hwpforge_core::Draft> =
            serde_json::from_value(serde_json::to_value(d).unwrap()).unwrap();
        d.sections_mut()[0].paragraphs[0].layout_cache =
            Some(hwpforge_core::layout::LayoutCache::new(vec![
                hwpforge_core::layout::LineSeg::default(),
            ]));
        let mut options = EncodeOptions::default().with_emit_layout_cache(true);
        options.mark_dropped_caches = true;
        let (out, marks) = crate::encoder::HwpxEncoder::encode_reporting_dropped_caches(
            &d.validate().unwrap(),
            &s,
            &ImageStore::new(),
            options,
        )
        .unwrap();
        let tree =
            test_xml::Element::parse(&test_package::part(&out.bytes, "Contents/section0.xml"));
        assert!(tree.descendants("hp:linesegarray").is_empty());
        assert_eq!(out.warnings.len(), 1);
        assert!(marks.iter().all(Vec::is_empty));
        for segment in [
            PathSeg::Header(0),
            PathSeg::Footer(0),
            PathSeg::MasterPage(0),
            PathSeg::Footnote,
            PathSeg::Endnote,
            PathSeg::TextBox,
            PathSeg::Memo,
            PathSeg::Caption,
            PathSeg::GroupChild(0),
        ] {
            assert!(!body_scope(&[
                PathSeg::Section(0),
                segment,
                PathSeg::Run(0),
                PathSeg::TableCell { row: 0, cell: 0 },
                PathSeg::NestedParagraph(0)
            ]));
        }
    }
}
