//! Header references are zero/one-based list positions (Hancom probes).
use crate::paragraph_measure::invalid;
use crate::{HwpxResult, HwpxStyleStore};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex, StyleIndex};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default)]
/// Registered positions and the shared strict reference resolver.
///
/// The bound of a style reference is the number of definitions the header
/// holds: a `charPrIDRef` or `paraPrIDRef` at or past that count fails the
/// decode for every paragraph and run converted to Core (body, table cell,
/// master page, chart run). Runs the decoder leaves out, such as a note's
/// injected number head, are not looked up.
#[doc(hidden)]
pub struct HeaderReferences {
    chars: usize,
    paras: usize,
    pub(crate) styles: usize,
    numberings: usize,
    pub(crate) memos: usize,
    pub(crate) track_changes: usize,
    pub(crate) track_change_authors: usize,
}

fn position(index: usize, count: usize, kind: &str) -> HwpxResult<usize> {
    lookup((index < count).then_some(index), kind, index)
}

fn one_based(id: usize, count: usize, kind: &str) -> HwpxResult<()> {
    lookup((id != 0 && id <= count).then_some(()), kind, id)
}

fn lookup<T>(value: Option<T>, kind: &str, id: impl std::fmt::Display) -> HwpxResult<T> {
    hwpforge_core::style_lookup::lookup(value, kind, id).map_err(|error| invalid(error.to_string()))
}

impl HeaderReferences {
    /// Register definitions before resolving forward references.
    pub fn build(store: &HwpxStyleStore) -> HwpxResult<Self> {
        let refs = Self {
            chars: store.char_shape_count(),
            paras: store.para_shape_count(),
            styles: store.style_count(),
            numberings: store.numbering_count(),
            ..Self::default()
        };
        for count in [refs.chars, refs.paras, refs.styles, refs.numberings] {
            u32::try_from(count).map_err(|_| invalid("definition position exceeds u32"))?;
        }
        Ok(refs)
    }
    /// Resolve an original character definition position without substitution.
    pub fn char_index(&self, id: u32) -> HwpxResult<CharShapeIndex> {
        position(id as usize, self.chars, "charPr").map(CharShapeIndex::new)
    }
    /// Resolve an original paragraph definition position without substitution.
    pub fn para_index(&self, id: u32) -> HwpxResult<ParaShapeIndex> {
        position(id as usize, self.paras, "paraPr").map(ParaShapeIndex::new)
    }
    pub(crate) fn style_index(&self, id: u32) -> HwpxResult<StyleIndex> {
        position(id as usize, self.styles, "style").map(StyleIndex::new)
    }
    pub(crate) fn output_ids(&self, kind: &str, index: usize) -> HwpxResult<u32> {
        let count = match kind {
            "charPr" => self.chars,
            "paraPr" => self.paras,
            "style" => self.styles,
            _ => unreachable!(),
        };
        // 0.16 public new-document examples register fonts only and use char/para 0.
        // Decoded inputs are checked against original definitions at decode time:
        // both decoders' `empty_definitions_zero_decode_gate`, HWP5's
        // `implicit_zero_decode_gate`, and
        // `body_style_references_check_original_definitions_in_every_container`.
        // 이것을 실패시키는 것: 예외 삭제, 빈 표/0/char·para 한정 조건 각각 제거.
        if count == 0 && index == 0 && matches!(kind, "charPr" | "paraPr") {
            return Ok(0);
        }
        position(index, count, kind).map(|position| position as u32)
    }
    pub(crate) fn border_fill(store: &HwpxStyleStore, id: u32) -> HwpxResult<()> {
        one_based(id as usize, store.border_fill_count(), "cell zone borderFill")
    }

    /// Check original body border IDs before generating output definitions.
    pub fn validate_body_borders<S>(
        document: &hwpforge_core::Document<S>,
        definitions: impl IntoIterator<Item = u32>,
    ) -> HwpxResult<()> {
        hwpforge_core::style_lookup::validate_body_borders(document, definitions)
            .map_err(|error| invalid(error.to_string()))
    }

    /// Inspect references whose XML fields are not represented by Core,
    /// before serde discards them. Unsupported recovery is always explicit.
    pub(crate) fn validate_auxiliary_xml(
        &self,
        xml: &str,
        part: &str,
    ) -> HwpxResult<Vec<crate::DecodeWarning>> {
        use quick_xml::events::Event;
        let mut warnings = Vec::new();
        let mut reader = quick_xml::Reader::from_str(xml);
        loop {
            let element = match reader.read_event().map_err(|e| invalid(e.to_string()))? {
                Event::Start(e) | Event::Empty(e) => e,
                Event::Eof => break,
                _ => continue,
            };
            for attr in element.attributes() {
                let attr = attr.map_err(|e| invalid(e.to_string()))?;
                let field = attr.key.as_ref();
                let owner = element.local_name();
                let owner = owner.as_ref();
                if !matches!(
                    (owner, field),
                    ("secPr", "outlineShapeIDRef" | "memoShapeIDRef")
                        | ("t", "charStyleIDRef")
                        | ("formCharPr", "charPrIDRef")
                        | ("trackChange", "authorID" | "charshapeID" | "parashapeID")
                        | ("run", "charTcId")
                        | ("p", "paraTcId")
                ) {
                    continue;
                }
                let value = attr
                    .normalized_value(quick_xml::XmlVersion::Explicit1_0)
                    .map_err(|e| invalid(e.to_string()))?;
                let id: u32 = value
                    .parse()
                    .map_err(|_| invalid(format!("{owner}.{field}: invalid ID {value}")))?;
                let reason = match field {
                    "authorID" | "charshapeID" | "parashapeID" | "charTcId" | "paraTcId" => {
                        if id == 0 && matches!(field, "charTcId" | "paraTcId") {
                            continue;
                        }
                        let found = match field {
                            "authorID" => one_based(
                                id as usize,
                                self.track_change_authors,
                                "trackChangeAuthor",
                            )
                            .map(|_| ()),
                            "charshapeID" => self.char_index(id).map(|_| ()),
                            "parashapeID" => self.para_index(id).map(|_| ()),
                            _ => one_based(id as usize, self.track_changes, "trackChange")
                                .map(|_| ()),
                        };
                        format!(
                            "{}; change tracking and its reference are not carried",
                            found.err().map_or_else(
                                || format!("validated {owner}.{field} {id}"),
                                |e| e.to_string()
                            )
                        )
                    }

                    "outlineShapeIDRef" => {
                        if id == 0 {
                            continue;
                        }
                        let found = one_based(id as usize, self.numberings, "section numbering");
                        if id == 1 && found.is_ok() {
                            continue;
                        }
                        format!(
                            "{}; Core does not carry section outline selection; output selects 1",
                            found.err().map_or_else(
                                || format!("validated numbering {id}"),
                                |e| e.to_string()
                            )
                        )
                    }
                    "memoShapeIDRef" => {
                        if id == 0 {
                            continue;
                        }
                        let found = one_based(id as usize, self.memos, "memoPr");
                        format!(
                            "{}; memo shape is unsupported; output selects 0",
                            found.err().map_or_else(
                                || format!("validated memoPr {id}"),
                                |e| e.to_string()
                            )
                        )
                    }
                    "charStyleIDRef" => {
                        self.style_index(id)?;
                        format!("validated inline style {id} is not carried by Core")
                    }
                    _ => {
                        self.char_index(id)?;
                        format!("validated form charPr {id} is not carried by Core")
                    }
                };
                warnings.push(crate::DecodeWarning::ContentLost {
                    part: part.into(),
                    path: crate::decoder::ParagraphPath::default(),
                    kind: format!("{owner}.{field}"),
                    reason,
                });
            }
        }
        Ok(warnings)
    }

    /// Use the already deserialized parameters: XML entity spelling must not
    /// change reference validation or the loss classification.
    pub(crate) fn validate_memo_parameters(
        &self,
        parameters: &crate::schema::section::HxFieldParameters,
        part: &str,
    ) -> HwpxResult<Vec<crate::DecodeWarning>> {
        let mut warnings = Vec::new();
        for parameter in &parameters.string_params {
            if parameter.name != "MemoShapeIDRef" {
                continue;
            }
            let value = &parameter.value;
            let id: u32 =
                value.parse().map_err(|_| invalid(format!("MemoShapeIDRef invalid ID {value}")))?;
            if id != 65535 {
                let found = one_based(id as usize, self.memos, "memoPr");
                warnings.push(crate::DecodeWarning::ContentLost {
                    part: part.into(),
                    path: crate::decoder::ParagraphPath::default(),
                    kind: "MemoShapeIDRef".into(),
                    reason: format!(
                        "{}; unsupported memo shape definition is not carried; reference retained",
                        found
                            .err()
                            .map_or_else(|| format!("validated memoPr {id}"), |e| e.to_string())
                    ),
                });
            }
        }
        Ok(warnings)
    }

    /// Validate all header references using the same resolver as body references.
    pub fn validate(&self, store: &HwpxStyleStore, _output: bool) -> HwpxResult<()> {
        for style in store.iter_styles() {
            self.char_index(style.char_pr_id_ref)?;
            if style.style_type == "PARA" {
                self.para_index(style.para_pr_id_ref)?;
            }
            // CHAR style paraPrIDRef is inactive; native files contain arbitrary
            // values here. Preserve it as a wire ID, never use it as an index.
            self.style_index(style.next_style_id_ref)?;
        }
        let mut font_counts = BTreeMap::<&str, usize>::new();
        for f in store.iter_fonts() {
            *font_counts.entry(&f.lang).or_default() += 1;
        }
        // Explicit references are checked before output supplementation.
        for (index, c) in store.iter_char_shapes().enumerate() {
            let f = &c.font_ref;
            for (lang, id) in [
                ("HANGUL", f.hangul),
                ("LATIN", f.latin),
                ("HANJA", f.hanja),
                ("JAPANESE", f.japanese),
                ("OTHER", f.other),
                ("SYMBOL", f.symbol),
                ("USER", f.user),
            ] {
                position(
                    id.get(),
                    font_counts.get(lang).copied().unwrap_or(0),
                    &format!("charPr {:?} font {lang}", index),
                )?;
            }
            if let Some(b) = c.border_fill_id {
                if b != 0 {
                    one_based(
                        b as usize,
                        store.border_fill_count(),
                        &format!("charPr {:?} borderFill", index),
                    )?;
                }
            }
        }
        for (index, p) in store.iter_para_shapes().enumerate() {
            if !hwpforge_core::TabDef::is_builtin_id(p.tab_pr_id_ref) {
                position(
                    p.tab_pr_id_ref as usize,
                    store.tab_count(),
                    &format!("paraPr {:?} tabPr", index),
                )?;
            }
            if let Some(b) = p.border_fill_id {
                one_based(
                    b.get(),
                    store.border_fill_count(),
                    &format!("paraPr {:?} borderFill", index),
                )?;
            }
            use hwpforge_foundation::HeadingType;
            if p.heading_id_ref != 0 {
                match p.heading_type {
                    HeadingType::Number | HeadingType::Outline => {
                        one_based(p.heading_id_ref as usize, self.numberings, "numbering")?;
                    }
                    HeadingType::Bullet => {
                        one_based(p.heading_id_ref as usize, store.bullet_count(), "bullet")?;
                    }
                    _ => {}
                }
            }
        }
        for heads in
            store.header_fidelity.numbering.values().chain(store.header_fidelity.bullet.values())
        {
            for h in heads.values() {
                if h.char_pr_id_ref != u32::MAX {
                    self.char_index(h.char_pr_id_ref)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HwpxCharShape, HwpxParaShape, HwpxStyle};
    #[test]
    fn position_ranges_preserve_boundaries() {
        // 이것을 실패시키는 것: 0/1 시작 혼동, 상한 포함/제외 뒤집기, usize 절삭.
        for count in [0, 1, 3] {
            for id in [0, 1, 2, 3, 4, usize::MAX] {
                assert_eq!(position(id, count, "charPr").is_ok(), id < count);
                assert_eq!(one_based(id, count, "numbering").is_ok(), id > 0 && id <= count);
            }
        }
        let refs = HeaderReferences::build(&HwpxStyleStore::new()).unwrap();
        for kind in ["charPr", "paraPr"] {
            assert_eq!(refs.output_ids(kind, 0).unwrap(), 0);
            assert!(refs.output_ids(kind, 1).is_err());
        }
        assert!(refs.output_ids("style", 0).is_err());
        assert!(refs.char_index(0).is_err());
        assert!(refs.para_index(0).is_err());
    }

    #[test]
    fn new_document_zero_exception_does_not_replace_original_ids() {
        // 이것을 실패시키는 것: 예외의 빈 표 조건 제거 또는 원본 lookup까지 0 허용.
        let mut store = HwpxStyleStore::with_default_fonts("바탕");
        store.push_char_shape(crate::HwpxCharShape::default());
        store.push_para_shape(crate::HwpxParaShape::default());
        let refs = HeaderReferences::build(&store).unwrap();
        assert_eq!(refs.char_index(0).unwrap().get(), 0);
        assert_eq!(refs.para_index(0).unwrap().get(), 0);
        assert_eq!(refs.output_ids("charPr", 0).unwrap(), 0);
        assert_eq!(refs.output_ids("paraPr", 0).unwrap(), 0);
        for kind in ["charPr", "paraPr"] {
            for index in [1, 2, 3] {
                assert!(refs.output_ids(kind, index).is_err());
            }
        }
    }

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn wide_references_cannot_alias_existing_definitions() {
        // 이것을 실패시키는 것: font/borderFill usize 참조를 as u32로 잘라 검증.
        let wide = usize::try_from(u64::from(u32::MAX) + 1).unwrap();
        let mut store = HwpxStyleStore::with_default_fonts("바탕");
        let mut shape = HwpxCharShape::default();
        shape.font_ref.hangul = hwpforge_foundation::FontIndex::new(wide);
        store.push_char_shape(shape);
        let refs = HeaderReferences::build(&store).unwrap();
        assert!(refs.validate(&store, true).unwrap_err().to_string().contains("font HANGUL"));
        let mut store = HwpxStyleStore::with_default_fonts("바탕");
        store.push_para_shape(HwpxParaShape {
            border_fill_id: Some(hwpforge_foundation::BorderFillIndex::new(wide + 1)),
            ..Default::default()
        });
        let refs = HeaderReferences::build(&store).unwrap();
        assert!(refs.validate(&store, true).unwrap_err().to_string().contains("borderFill"));
    }

    #[test]
    fn positions_ignore_style_labels_and_reject_out_of_range() {
        // 이것을 실패시키는 것: 한컴 실측과 반대로 id로 조회/원본 id 출력.
        let mut store = HwpxStyleStore::new();
        for _ in 0..4 {
            store.push_char_shape(HwpxCharShape::default());
        }
        store.push_para_shape(HwpxParaShape::default());
        store.push_style(HwpxStyle::new(9, "PARA", "base", "base", 0, 0, 1, 1042, 0));
        store.push_style(HwpxStyle::new(9, "PARA", "next", "next", 0, 1, 0, 1042, 0));
        let refs = HeaderReferences::build(&store).unwrap();
        assert_eq!(refs.char_index(3).unwrap().get(), 3);
        assert_eq!(refs.para_index(0).unwrap().get(), 0);
        assert_eq!(refs.style_index(1).unwrap().get(), 1);
        assert_eq!(refs.output_ids("charPr", 3).unwrap(), 3);
        assert_eq!(refs.output_ids("style", 0).unwrap(), 0);
        assert!(refs.style_index(9).is_err());
        assert!(refs.char_index(4).is_err());
        assert!(refs.para_index(1).is_err());
    }
}
