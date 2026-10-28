//! Supplemental legacy provenance rejection (never sufficient to admit a cache).
//! Emitted XML fingerprints in `wire` are the authoritative dependency check.
//! Indexed, fail-closed style dependencies. Each definition is compared once;
//! paragraph/run references only read the precomputed result.
use std::collections::HashMap;

use serde_json::Value;

// Core uses branded positions; resolve raw style references through the shared registry.
pub(super) fn cache_tree(store: &crate::HwpxStyleStore) -> crate::HwpxResult<Value> {
    let refs = crate::header_references::HeaderReferences::build(store)?;
    let mut value = super::tree(store)?;
    for (i, style) in store.iter_styles().enumerate() {
        value["styles"][i]["char_pr_id_ref"] =
            serde_json::json!(refs.char_index(style.char_pr_id_ref)?.get());
        if style.style_type == "PARA" {
            value["styles"][i]["para_pr_id_ref"] =
                serde_json::json!(refs.para_index(style.para_pr_id_ref)?.get());
        }
    }
    Ok(value)
}

// Dependency order: a definition only refers to an earlier kind.
const KINDS: [&str; 7] =
    ["border_fills", "tabs", "numberings", "bullets", "char_shapes", "para_shapes", "styles"];

#[derive(Default)]
struct Work {
    #[cfg(test)]
    steps: std::cell::Cell<usize>,
    #[cfg(test)]
    font_comparisons: std::cell::Cell<usize>,
}

impl Work {
    fn charge(&self) {
        #[cfg(test)]
        self.steps.set(self.steps.get() + 1);
    }
}

struct Index<'a> {
    definitions: HashMap<&'static str, HashMap<u64, Option<&'a Value>>>,
    fonts: HashMap<&'a str, HashMap<u64, Option<&'a Value>>>,
    first_language: Option<&'a str>,
}

impl<'a> Index<'a> {
    fn new(store: &'a Value, work: &Work) -> Self {
        let mut definitions = HashMap::new();
        for kind in KINDS {
            let mut items = HashMap::new();
            for (i, value) in store[kind].as_array().into_iter().flatten().enumerate() {
                work.charge();
                let base = u64::from(matches!(kind, "border_fills" | "numberings" | "bullets"));
                items.insert(i as u64 + base, Some(value));
            }
            definitions.insert(kind, items);
        }
        let mut fonts: HashMap<&str, HashMap<u64, Option<&Value>>> = HashMap::new();
        let mut first_language = None;
        for font in store["fonts"].as_array().into_iter().flatten() {
            work.charge();
            if let Some(lang) = font["lang"].as_str() {
                first_language.get_or_insert(lang);
                let group = fonts.entry(lang).or_default();
                group.insert(group.len() as u64, Some(font));
            }
        }
        Self { definitions, fonts, first_language }
    }

    fn item(&self, kind: &str, id: u64) -> Option<&'a Value> {
        self.definitions.get(kind)?.get(&id).copied().flatten()
    }

    fn font(&self, lang: &str, id: u64) -> Option<&'a Value> {
        let group = self.fonts.get(lang).or_else(|| self.fonts.get(self.first_language?))?;
        group.get(&id).copied().flatten()
    }
}

fn compare_font(a: Option<&Value>, b: Option<&Value>, work: &Work) -> bool {
    work.charge();
    #[cfg(test)]
    work.font_comparisons.set(work.font_comparisons.get() + 1);
    match (a, b) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

pub(super) struct Styles<'a> {
    base: Index<'a>,
    edited: Index<'a>,
    unchanged: HashMap<&'static str, HashMap<u64, bool>>,
    work: Work,
    unchanged_fonts: HashMap<&'a str, HashMap<u64, bool>>,
}

impl<'a> Styles<'a> {
    pub(super) fn new(base: &'a Value, edited: &'a Value) -> Self {
        let work = Work::default();
        let base = Index::new(base, &work);
        let edited = Index::new(edited, &work);
        let unchanged_fonts = base
            .fonts
            .iter()
            .map(|(&lang, fonts)| {
                let comparisons = fonts
                    .iter()
                    .map(|(&id, &a)| {
                        let b = edited
                            .fonts
                            .get(lang)
                            .and_then(|fonts| fonts.get(&id))
                            .copied()
                            .flatten();
                        (id, compare_font(a, b, &work))
                    })
                    .collect();
                (lang, comparisons)
            })
            .collect();
        let mut result = Self { base, edited, unchanged: HashMap::new(), work, unchanged_fonts };
        for kind in KINDS {
            let comparisons = result.base.definitions[kind]
                .keys()
                .map(|&id| (id, result.compare(kind, id)))
                .collect();
            result.unchanged.insert(kind, comparisons);
        }
        result
    }

    pub(super) fn same(&self, kind: &str, id: u64) -> bool {
        self.work.charge();
        self.unchanged.get(kind).and_then(|items| items.get(&id)).copied().unwrap_or(false)
    }

    fn compare(&self, kind: &str, id: u64) -> bool {
        self.work.charge();
        let (Some(a), Some(b)) = (self.base.item(kind, id), self.edited.item(kind, id)) else {
            return false;
        };
        if a != b {
            return false;
        }
        match kind {
            "char_shapes" => {
                ["hangul", "latin", "hanja", "japanese", "other", "symbol", "user"].iter().all(
                    |lang| {
                        self.work.charge();
                        let Some(id) = a["font_ref"][lang].as_u64() else { return false };
                        let upper = lang.to_ascii_uppercase();
                        match (self.base.font(&upper, id), self.edited.font(&upper, id)) {
                            (Some(a), Some(b)) => {
                                a["lang"] == b["lang"]
                                    && a["lang"]
                                        .as_str()
                                        .and_then(|lang| self.unchanged_fonts.get(lang)?.get(&id))
                                        .copied()
                                        .unwrap_or(false)
                            }
                            _ => false,
                        }
                    },
                ) && self.same("border_fills", a["border_fill_id"].as_u64().unwrap_or(2))
            }
            "para_shapes" => {
                self.same("tabs", a["tab_pr_id_ref"].as_u64().unwrap_or(0))
                    && a["border_fill_id"].as_u64().is_none_or(|i| self.same("border_fills", i))
                    && match a["heading_type"].as_str() {
                        Some("Bullet") => {
                            self.same("bullets", a["heading_id_ref"].as_u64().unwrap_or(0))
                        }
                        Some("Outline") => self
                            .same("numberings", a["heading_id_ref"].as_u64().unwrap_or(0).max(1)),
                        Some("Number") => {
                            self.same("numberings", a["heading_id_ref"].as_u64().unwrap_or(0))
                        }
                        _ => true,
                    }
            }
            "styles" => {
                self.same("char_shapes", a["char_pr_id_ref"].as_u64().unwrap_or(0))
                    && self.same("para_shapes", a["para_pr_id_ref"].as_u64().unwrap_or(0))
            }
            _ => true,
        }
    }

    pub(super) fn references_unchanged(&self, value: &Value) -> bool {
        match value {
            Value::Object(fields) => {
                fields.iter().all(|(key, value)| {
                    let kind = match key.as_str() {
                        "char_shape_id" | "end_char_shape" => Some("char_shapes"),
                        "para_shape_id" => Some("para_shapes"),
                        "style_ref" => Some("styles"),
                        "border_fill_id" => Some("border_fills"),
                        _ => None,
                    };
                    kind.zip(value.as_u64()).is_none_or(|(kind, id)| self.same(kind, id))
                        && self.references_unchanged(value)
                }) && (!super::paragraph(value)
                    || self.same("styles", value["style_ref"].as_u64().unwrap_or(0)))
            }
            Value::Array(items) => items.iter().all(|v| self.references_unchanged(v)),
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn store() -> Value {
        let mut doc = hwpforge_core::Document::new();
        doc.add_section(hwpforge_core::Section::with_paragraphs(
            vec![hwpforge_core::Paragraph::with_runs(
                vec![hwpforge_core::Run::text("text", hwpforge_foundation::CharShapeIndex::new(0))],
                hwpforge_foundation::ParaShapeIndex::new(0),
            )],
            hwpforge_core::PageSettings::a4(),
        ));
        let bytes = crate::HwpxEncoder::encode(
            &doc.validate().unwrap(),
            &crate::style_store_for_preset("default").unwrap(),
            &hwpforge_core::image::ImageStore::new(),
        )
        .unwrap();
        serde_json::to_value(crate::HwpxDecoder::decode(&bytes).unwrap().style_store).unwrap()
    }

    // 이것을 실패시키는 것: Core 스타일 인덱스를 원본 wire ID로 오인하기.
    #[test]
    fn sparse_style_ids_and_reordering_resolve_dependencies() {
        let mut a = store();
        a["styles"] = json!([a["styles"][0], a["styles"][7]]);
        let mut b = a.clone();
        b["styles"].as_array_mut().unwrap().reverse();
        assert!(Styles::new(&a, &a).same("styles", 1));
        assert!(!Styles::new(&a, &b).same("styles", 1));
        for key in ["para_pr_id_ref", "char_pr_id_ref"] {
            let mut edited = a.clone();
            edited["styles"][1][key] = json!(if edited["styles"][1][key] == 0 { 1 } else { 0 });
            assert!(!Styles::new(&a, &edited).same("styles", 1), "{key}");
        }
    }

    // 이것을 실패시키는 것: 양쪽 미해결 참조 또는 중복 ID를 동일 정의로 인정하기.
    #[test]
    fn missing_and_duplicate_definitions_are_not_equal() {
        let mut a = store();
        assert!(!Styles::new(&a, &a).same("styles", 900));
        for kind in KINDS {
            let saved = a[kind].clone();
            a[kind] = json!([]);
            assert!(!Styles::new(&a, &a).same(kind, 0), "{kind}");
            a[kind] = saved;
        }
        for kind in ["styles", "tabs", "border_fills", "numberings", "bullets"] {
            let mut b = a.clone();
            let entry = json!({"id": 997});
            b[kind] = json!([entry, entry]);
            assert!(!Styles::new(&b, &b).same(kind, 997), "{kind}");
        }
        a["styles"][0]["char_pr_id_ref"] = json!(9999);
        assert!(!Styles::new(&a, &a).same("styles", 0));
    }

    // 이것을 실패시키는 것: 글꼴 미해결을 None==None으로 인정하거나 언어별 ID를 합치기.
    #[test]
    fn fonts_require_unique_resolved_language_local_ids() {
        let a = store();
        let mut b = a.clone();
        b["fonts"].as_array_mut().unwrap().reverse();
        // 한컴 실측: 언어 내부 순서를 뒤집으면 같은 위치가 다른 글꼴이다.
        assert!(!Styles::new(&a, &b).same("char_shapes", 0));
        b = a.clone();
        let font_id = a["char_shapes"][0]["font_ref"]["latin"].clone();
        let fonts = b["fonts"].as_array_mut().unwrap();
        let font = fonts.iter_mut().find(|f| f["lang"] == "LATIN" && f["id"] == font_id).unwrap();
        font["face_name"] = json!("Changed Latin");
        assert!(!Styles::new(&a, &b).same("char_shapes", 0));
        let fonts = b["fonts"].as_array_mut().unwrap();
        let duplicate =
            fonts.iter().find(|f| f["lang"] == "LATIN" && f["id"] == font_id).unwrap().clone();
        fonts.push(duplicate);
        // 한컴 실측: 중복 id는 새 위치일 뿐 기존 참조를 모호하게 하지 않는다.
        assert!(Styles::new(&b, &b).same("char_shapes", 0));
        b["fonts"] = json!([]);
        assert!(!Styles::new(&b, &b).same("char_shapes", 0));
    }

    // 이것을 실패시키는 것: 없는 언어 그룹의 첫 그룹 fallback을 누락하거나 잘못된 ID 허용.
    #[test]
    fn absent_language_uses_first_group_but_missing_id_is_unresolved() {
        let mut a = store();
        a["fonts"].as_array_mut().unwrap().retain(|f| f["lang"] == "HANGUL");
        assert!(Styles::new(&a, &a).same("char_shapes", 0));
        a["char_shapes"][0]["font_ref"]["latin"] = json!(999);
        assert!(!Styles::new(&a, &a).same("char_shapes", 0));
    }

    // 이것을 실패시키는 것: 중복/미해결 글꼴 정의에 동일 판정을 저장하기.
    #[test]
    fn font_definition_comparison_rejects_unresolved_pairs() {
        let font = json!({"lang":"LATIN", "id":7, "face_name":"Font"});
        let changed = json!({"lang":"LATIN", "id":7, "face_name":"Changed"});
        let work = Work::default();
        for (a, b) in
            [(None, None), (Some(&font), None), (None, Some(&font)), (Some(&font), Some(&changed))]
        {
            assert!(!compare_font(a, b, &work));
        }
        assert!(compare_font(Some(&font), Some(&font), &work));
    }

    // 이것을 실패시키는 것: run마다 정의·글꼴을 다시 비교하거나 배열을 선형 검색하기.
    #[test]
    fn style_work_is_linear_in_definitions_fonts_and_references() {
        fn measure(n: usize) -> usize {
            let mut a = store();
            let font = a["fonts"][0].clone();
            for i in 0..n {
                let mut f = font.clone();
                f["id"] = json!(10000 + i);
                a["fonts"].as_array_mut().unwrap().push(f);
                for kind in KINDS {
                    let mut value =
                        a[kind].as_array().unwrap().first().cloned().unwrap_or(json!({}));
                    value["id"] = json!(10000 + i);
                    a[kind].as_array_mut().unwrap().push(value);
                }
            }
            let styles = Styles::new(&a, &a);
            assert_eq!(styles.work.font_comparisons.get(), a["fonts"].as_array().unwrap().len(), "each font definition must be compared exactly once, independent of char shape count");
            let before = styles.work.steps.get();
            for _ in 0..n {
                assert!(styles.references_unchanged(
                    &json!({"runs": [{"char_shape_id": 0}], "para_shape_id": 0, "style_ref": 0})
                ));
            }
            assert_eq!(
                styles.work.steps.get() - before,
                4 * n,
                "definitions must not be re-compared per reference"
            );
            styles.work.steps.get()
        }
        let small = measure(1000);
        let large = measure(2000);
        assert!(large <= small * 2 && large < 100 * 2000, "{small} -> {large}");
        eprintln!("style work: n=1000 {small}, n=2000 {large}");
    }
}
