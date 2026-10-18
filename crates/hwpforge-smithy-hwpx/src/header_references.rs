//! Wire IDs are registered before any references are resolved.
use crate::paragraph_measure::invalid;
use crate::{HwpxResult, HwpxStyleStore};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex, StyleIndex};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Default)]
/// Registered wire IDs and the shared strict reference resolver.
#[doc(hidden)]
pub struct HeaderReferences {
    chars: BTreeMap<u32, usize>,
    paras: BTreeMap<u32, usize>,
    styles: BTreeMap<u32, usize>,
    numberings: BTreeSet<u32>,
    pub(crate) memos: BTreeSet<u32>,
    pub(crate) track_changes: BTreeSet<u32>,
    pub(crate) track_change_authors: BTreeSet<u32>,
    pub(crate) char_ids: Vec<u32>,
    pub(crate) para_ids: Vec<u32>,
    pub(crate) style_ids: Vec<u32>,
}

fn ids(kind: &str, source: impl Iterator<Item = Option<u32>>) -> HwpxResult<Vec<u32>> {
    let source: Vec<_> = source.collect();
    let mut used = BTreeSet::new();
    for id in source.iter().flatten() {
        if *id == u32::MAX || !used.insert(*id) {
            return Err(invalid(format!("{kind} duplicate/reserved definition ID {id}")));
        }
    }
    let mut next = 0;
    source
        .into_iter()
        .map(|id| {
            if let Some(id) = id {
                return Ok(id);
            }
            while used.contains(&next) {
                next = next.checked_add(1).ok_or_else(|| invalid("ID space exhausted"))?;
            }
            if next == u32::MAX {
                return Err(invalid("ID space exhausted"));
            }
            used.insert(next);
            Ok(next)
        })
        .collect()
}

fn lookup<T>(value: Option<T>, kind: &str, id: impl std::fmt::Display) -> HwpxResult<T> {
    hwpforge_core::style_lookup::lookup(value, kind, id).map_err(|error| invalid(error.to_string()))
}

impl HeaderReferences {
    /// Register definitions before resolving forward references.
    pub fn build(store: &HwpxStyleStore) -> HwpxResult<Self> {
        let char_ids = ids("charPr", store.iter_char_shapes().map(|s| s.source_id))?;
        let para_ids = ids("paraPr", store.iter_para_shapes().map(|s| s.source_id))?;
        let style_ids = ids("style", store.iter_styles().map(|s| Some(s.id)))?;
        Ok(Self {
            chars: char_ids.iter().enumerate().map(|(i, id)| (*id, i)).collect(),
            paras: para_ids.iter().enumerate().map(|(i, id)| (*id, i)).collect(),
            styles: style_ids.iter().enumerate().map(|(i, id)| (*id, i)).collect(),
            numberings: store.iter_numberings().map(|n| n.id).collect(),
            memos: BTreeSet::new(),
            track_changes: BTreeSet::new(),
            track_change_authors: BTreeSet::new(),
            char_ids,
            para_ids,
            style_ids,
        })
    }
    /// Resolve an original character definition ID without substitution.
    pub fn char_index(&self, id: u32) -> HwpxResult<CharShapeIndex> {
        lookup(self.chars.get(&id).copied(), "charPr", id).map(CharShapeIndex::new)
    }
    /// Resolve an original paragraph definition ID without substitution.
    pub fn para_index(&self, id: u32) -> HwpxResult<ParaShapeIndex> {
        lookup(self.paras.get(&id).copied(), "paraPr", id).map(ParaShapeIndex::new)
    }
    pub(crate) fn style_index(&self, id: u32) -> HwpxResult<StyleIndex> {
        lookup(self.styles.get(&id).copied(), "style", id).map(StyleIndex::new)
    }
    pub(crate) fn output_ids(&self, kind: &str, index: usize) -> HwpxResult<u32> {
        let ids = match kind {
            "charPr" => &self.char_ids,
            "paraPr" => &self.para_ids,
            "style" => &self.style_ids,
            _ => unreachable!(),
        };
        // 0.16 public new-document examples register fonts only and use char/para 0.
        // Decoded inputs are checked against original definitions at decode time:
        // both decoders' `empty_definitions_zero_decode_gate`, HWP5's
        // `implicit_zero_decode_gate`, and
        // `body_style_references_check_original_definitions_in_every_container`.
        // 이것을 실패시키는 것: 예외 삭제, 빈 표/0/char·para 한정 조건 각각 제거.
        if ids.is_empty() && index == 0 && matches!(kind, "charPr" | "paraPr") {
            return Ok(0);
        }
        lookup(ids.get(index).copied(), kind, index)
    }
    pub(crate) fn border_fill(store: &HwpxStyleStore, id: u32) -> HwpxResult<()> {
        lookup(store.border_fill(id).ok(), "cell zone borderFill", id).map(|_| ())
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
                            "authorID" => {
                                lookup(self.track_change_authors.get(&id), "trackChangeAuthor", id)
                                    .map(|_| ())
                            }
                            "charshapeID" => self.char_index(id).map(|_| ()),
                            "parashapeID" => self.para_index(id).map(|_| ()),
                            _ => lookup(self.track_changes.get(&id), "trackChange", id).map(|_| ()),
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
                        let found = lookup(self.numberings.get(&id), "section numbering", id);
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
                        let found = lookup(self.memos.get(&id), "memoPr", id);
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
                let found = lookup(self.memos.get(&id), "memoPr", id);
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
        let mut fonts = BTreeSet::new();
        for f in store.iter_fonts() {
            if !fonts.insert((f.lang.as_str(), f.id)) {
                return Err(invalid(format!("duplicate font {} ID {}", f.lang, f.id)));
            }
        }
        // Both decode and output resolve explicit references only in the original
        // tables. Output supplementation belongs after this validation.
        let borders: BTreeSet<_> =
            ids("borderFill", store.iter_border_fills().map(|x| Some(x.id)))?.into_iter().collect();
        let tabs: BTreeSet<_> =
            ids("tabPr", store.iter_tabs().map(|x| Some(x.id)))?.into_iter().collect();
        let numbering: BTreeSet<_> =
            ids("numbering", store.iter_numberings().map(|x| Some(x.id)))?.into_iter().collect();
        let bullets: BTreeSet<_> =
            ids("bullet", store.iter_bullets().map(|x| Some(x.id)))?.into_iter().collect();
        for c in store.iter_char_shapes() {
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
                lookup(
                    u32::try_from(id.get()).ok().and_then(|id| fonts.get(&(lang, id))),
                    &format!("charPr {:?} font {lang}", c.source_id),
                    id.get(),
                )?;
            }
            if let Some(b) = c.border_fill_id {
                if b != 0 {
                    lookup(borders.get(&b), &format!("charPr {:?} borderFill", c.source_id), b)?;
                }
            }
        }
        for p in store.iter_para_shapes() {
            if !hwpforge_core::TabDef::is_builtin_id(p.tab_pr_id_ref) {
                lookup(
                    tabs.get(&p.tab_pr_id_ref),
                    &format!("paraPr {:?} tabPr", p.source_id),
                    p.tab_pr_id_ref,
                )?;
            }
            if let Some(b) = p.border_fill_id {
                lookup(
                    u32::try_from(b.get()).ok().and_then(|id| borders.get(&id)),
                    &format!("paraPr {:?} borderFill", p.source_id),
                    b.get(),
                )?;
            }
            use hwpforge_foundation::HeadingType;
            if p.heading_id_ref != 0 {
                match p.heading_type {
                    HeadingType::Number | HeadingType::Outline => {
                        lookup(numbering.get(&p.heading_id_ref), "numbering", p.heading_id_ref)?;
                    }
                    HeadingType::Bullet => {
                        lookup(bullets.get(&p.heading_id_ref), "bullet", p.heading_id_ref)?;
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
    fn new_document_zero_exception_does_not_replace_original_ids() {
        // 이것을 실패시키는 것: 예외의 빈 표 조건 제거 또는 원본 lookup까지 0 허용.
        let mut store = HwpxStyleStore::with_default_fonts("바탕");
        store.push_char_shape(crate::HwpxCharShape { source_id: Some(7), ..Default::default() });
        store.push_para_shape(crate::HwpxParaShape { source_id: Some(9), ..Default::default() });
        let refs = HeaderReferences::build(&store).unwrap();
        assert!(refs.char_index(0).is_err());
        assert!(refs.para_index(0).is_err());
        assert_eq!(refs.output_ids("charPr", 0).unwrap(), 7);
        assert_eq!(refs.output_ids("paraPr", 0).unwrap(), 9);
        for kind in ["charPr", "paraPr"] {
            for index in [1, 2, 3] {
                assert!(refs.output_ids(kind, index).is_err());
            }
        }
    }

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn fix2_wide_references_cannot_alias_existing_definitions() {
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
    fn w1_sparse_reordered_forward_references_and_new_ids() {
        let mut store = HwpxStyleStore::new();
        for id in [7, 3, 0] {
            store.push_char_shape(HwpxCharShape { source_id: Some(id), ..Default::default() });
        }
        store.push_char_shape(HwpxCharShape::default());
        store.push_para_shape(HwpxParaShape { source_id: Some(31), ..Default::default() });
        store.push_style(HwpxStyle::new(0, "PARA", "base", "base", 0, 0, 1, 1042, 0));
        store.push_style(HwpxStyle::new(9, "PARA", "next", "next", 0, 1, 0, 1042, 0));
        let refs = HeaderReferences::build(&store).unwrap();
        assert_eq!(refs.char_index(7).unwrap().get(), 0);
        assert_eq!(refs.char_index(0).unwrap().get(), 2);
        assert_eq!(refs.para_index(31).unwrap().get(), 0);
        assert_eq!(refs.style_index(9).unwrap().get(), 1);
        assert_eq!(refs.char_ids, [7, 3, 0, 1]);
        assert_eq!(refs.output_ids("charPr", 0).unwrap(), 7);
        assert!(refs.char_index(1_000_000).is_err());
        assert!(refs.para_index(0).is_err());
        store.push_char_shape(HwpxCharShape { source_id: Some(7), ..Default::default() });
        assert!(HeaderReferences::build(&store).is_err());
    }
    #[test]
    fn w1_duplicate_and_reserved_ids_are_not_deduplicated() {
        assert!(ids("charPr", [Some(7), Some(7)].into_iter()).is_err());
        assert!(ids("charPr", [Some(u32::MAX)].into_iter()).is_err());
        assert_eq!(
            ids("paraPr", [Some(5), None, Some(0), None].into_iter()).unwrap(),
            [5, 1, 0, 2]
        );
    }
}
