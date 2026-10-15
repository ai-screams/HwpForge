//! Core comparison and trusted-cache preparation for a package-backed encode.
//!
//! Paragraph ids are not identities (native files reuse them). Lists are
//! paired by structural position; a length change admits only the identical
//! prefix. No suffix is guessed or matched by text. The shared flow rule then
//! invalidates the changed list's tail and host chain.

use hwpforge_core::{Document, Draft, Validated};
use serde_json::Value;

use crate::decoder::{ParagraphPath, PathSeg};
use crate::{EncodeWarning, HwpxDocument, HwpxError, HwpxResult, HwpxStyleStore};

fn tree<T: serde::Serialize>(value: &T) -> HwpxResult<Value> {
    serde_json::to_value(value).map_err(|e| HwpxError::InvalidStructure { detail: e.to_string() })
}

fn paragraph(value: &Value) -> bool {
    value.get("runs").is_some() && value.get("para_shape_id").is_some()
}

/// Comparison uses the typed Core serialization, never the caller's JSON.
/// Removing all cache fields also covers image captions and future containers
/// that the general Core paragraph walker does not currently visit.
fn clean(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            fields.remove("layout_cache");
            for child in fields.values_mut() {
                clean(child);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(clean),
        _ => {}
    }
}

/// Container geometry and content outside nested paragraph lists. A cell's
/// text change does not change its siblings' geometry; a table width change
/// does. Lists themselves are handled by the paragraph correspondence pass.
fn shell(value: &Value) -> Value {
    match value {
        Value::Array(items) if items.first().is_some_and(paragraph) => Value::Array(Vec::new()),
        Value::Array(items) => Value::Array(items.iter().map(shell).collect()),
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .filter(|(k, _)| k.as_str() != "layout_cache")
                .map(|(k, v)| (k.clone(), shell(v)))
                .collect(),
        ),
        _ => value.clone(),
    }
}

struct Styles {
    base: Value,
    edited: Value,
}

impl Styles {
    fn item<'a>(store: &'a Value, kind: &str, id: u64) -> &'a Value {
        if matches!(kind, "border_fills" | "tabs" | "numberings" | "bullets") {
            return store[kind]
                .as_array()
                .into_iter()
                .flatten()
                .find(|v| v["id"].as_u64() == Some(id))
                .unwrap_or(&Value::Null);
        }
        usize::try_from(id).ok().and_then(|i| store.get(kind)?.get(i)).unwrap_or(&Value::Null)
    }

    /// Resolve the language-local ids, including the encoder's first-group
    /// fallback when a language group is absent.
    fn fonts<'a>(store: &'a Value, shape: &Value) -> Vec<Option<&'a Value>> {
        let fonts = store["fonts"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        ["hangul", "latin", "hanja", "japanese", "other", "symbol", "user"]
            .iter()
            .map(|lang| {
                let upper = lang.to_ascii_uppercase();
                let group = if fonts.iter().any(|f| f["lang"].as_str() == Some(&upper)) {
                    upper.as_str()
                } else {
                    fonts.first().and_then(|f| f["lang"].as_str()).unwrap_or("")
                };
                fonts.iter().find(|f| {
                    f["lang"].as_str() == Some(group) && f["id"] == shape["font_ref"][*lang]
                })
            })
            .collect()
    }

    fn same(&self, kind: &str, id: u64) -> bool {
        let a = Self::item(&self.base, kind, id);
        let b = Self::item(&self.edited, kind, id);
        if a != b {
            return false;
        }
        match kind {
            "char_shapes" => {
                Self::fonts(&self.base, a) == Self::fonts(&self.edited, b)
                    && self.same("border_fills", a["border_fill_id"].as_u64().unwrap_or(2))
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

    fn references_unchanged(&self, value: &Value) -> bool {
        match value {
            Value::Object(fields) => {
                fields.iter().all(|(key, value)| {
                    let kind = match key.as_str() {
                        "char_shape_id" | "end_char_shape" => Some("char_shapes"),
                        "para_shape_id" => Some("para_shapes"),
                        "style_id" => Some("styles"),
                        "border_fill_id" => Some("border_fills"),
                        _ => None,
                    };
                    kind.zip(value.as_u64()).is_none_or(|(kind, id)| self.same(kind, id))
                        && self.references_unchanged(value)
                }) && (!paragraph(value)
                    || self.same("styles", value["style_id"].as_u64().unwrap_or(0)))
            }
            Value::Array(items) => items.iter().all(|v| self.references_unchanged(v)),
            _ => true,
        }
    }
}

struct Candidate {
    pointer: String,
    path: ParagraphPath,
    cache: Option<Value>,
    supplied_cache: bool,
    unmatched: bool,
}

#[derive(Default)]
struct Plan {
    candidates: Vec<Candidate>,
    para_list: Vec<usize>,
    list_host: Vec<Option<usize>>,
    changed: Vec<usize>,
    warnings: Vec<EncodeWarning>,
    masterpage_offset: usize,
}

impl Plan {
    #[allow(clippy::too_many_arguments)]
    fn walk(
        &mut self,
        base: Option<&Value>,
        edited: &Value,
        trusted: &Value,
        supplied: &Value,
        pointer: &str,
        path: &[PathSeg],
        host: Option<usize>,
        context_changed: bool,
        styles: &Styles,
    ) {
        match edited {
            Value::Array(items)
                if items.first().is_some_and(paragraph)
                    || base
                        .and_then(Value::as_array)
                        .and_then(|v| v.first())
                        .is_some_and(paragraph) =>
            {
                let old = base.and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
                let list = self.list_host.len();
                self.list_host.push(host);
                let structural = old.len() != items.len();
                // 이것을 실패시키는 것: 길이가 다른 목록을 위치만으로 끝까지 대응시키기.
                let prefix = if structural {
                    old.iter().zip(items).take_while(|(a, b)| a == b).count()
                } else {
                    items.len()
                };
                if structural {
                    if let Some(host) = host {
                        self.changed.push(host);
                    }
                    self.warnings.push(EncodeWarning::LayoutCacheDropped {
                        path: ParagraphPath(path.to_vec()),
                        reason: format!("paragraph structure changed at {pointer}; only the identical prefix has base correspondence"),
                    });
                }
                for (i, p) in items.iter().enumerate() {
                    let mut pp = path.to_vec();
                    pp.push(if matches!(path.last(), Some(PathSeg::Section(_))) {
                        PathSeg::BodyParagraph(i)
                    } else {
                        PathSeg::NestedParagraph(i)
                    });
                    let at = self.candidates.len();
                    let matched = i < prefix && old.get(i).is_some();
                    let own = shell(p);
                    // Nested text seeds its own paragraph; the shared host
                    // chain, not an enclosing-value comparison, moves hosts.
                    let changed = context_changed
                        || !matched
                        || old.get(i).is_none_or(|b| shell(b) != own)
                        || !styles.references_unchanged(&own);
                    if changed {
                        self.changed.push(at);
                    }
                    self.para_list.push(list);
                    self.candidates.push(Candidate {
                        pointer: format!("{pointer}/{i}"),
                        path: ParagraphPath(pp.clone()),
                        cache: matched
                            .then(|| trusted.get(i).and_then(|v| v.get("layout_cache")).cloned())
                            .flatten(),
                        supplied_cache: supplied
                            .get(i)
                            .and_then(|v| v.get("layout_cache"))
                            .is_some(),
                        unmatched: !matched,
                    });
                    self.walk(
                        old.get(i).filter(|_| matched),
                        p,
                        &trusted[i],
                        &supplied[i],
                        &format!("{pointer}/{i}"),
                        &pp,
                        Some(at),
                        context_changed || !matched,
                        styles,
                    );
                }
            }
            Value::Array(items) => {
                for (i, item) in items.iter().enumerate() {
                    let mut pp = path.to_vec();
                    let key = pointer.rsplit('/').next().unwrap_or("");
                    match key {
                        "headers" => pp.push(PathSeg::Header(i)),
                        "footers" => pp.push(PathSeg::Footer(i)),
                        "master_pages" => pp.push(PathSeg::MasterPage(self.masterpage_offset + i)),
                        "cells" => {
                            let row = pointer
                                .split('/')
                                .rev()
                                .nth(1)
                                .and_then(|s| s.parse().ok())
                                .unwrap_or(0);
                            pp.push(PathSeg::TableCell { row, cell: i });
                        }
                        _ => {}
                    }
                    self.walk(
                        base.and_then(|v| v.get(i)),
                        item,
                        &trusted[i],
                        &supplied[i],
                        &format!("{pointer}/{i}"),
                        &pp,
                        host,
                        context_changed,
                        styles,
                    );
                }
            }
            Value::Object(fields) => {
                // Container shells capture cell widths, object geometry and
                // page-level list settings without treating a child's text
                // as an edit to every sibling list.
                let geometry = |v: &Value| {
                    let mut result = shell(v);
                    if v.get("page_settings").is_some() {
                        if let Some(fields) = result.as_object_mut() {
                            for key in ["headers", "footers", "master_pages"] {
                                fields.remove(key);
                            }
                        }
                    }
                    result
                };
                let owns_flow = fields.contains_key("paragraphs")
                    || fields.contains_key("rows")
                    || fields.contains_key("children")
                    || fields.get("caption").is_some_and(|v| !v.is_null())
                    || fields
                        .values()
                        .any(|v| v.as_array().and_then(|a| a.first()).is_some_and(paragraph));
                let context = geometry(edited);
                let geometry_changed = !paragraph(edited)
                    && owns_flow
                    && (context != base.map(geometry).unwrap_or(Value::Null)
                        || !styles.references_unchanged(&context));
                for (key, item) in fields {
                    let mut pp = path.to_vec();
                    match key.as_str() {
                        "caption" => pp.push(PathSeg::Caption),
                        "Footnote" => pp.push(PathSeg::Footnote),
                        "Endnote" => pp.push(PathSeg::Endnote),
                        "Memo" => pp.push(PathSeg::Memo),
                        "TextBox" | "Ellipse" | "Polygon" => pp.push(PathSeg::TextBox),
                        _ => {}
                    }
                    let independent =
                        matches!(key.as_str(), "headers" | "footers" | "master_pages");
                    let old = base.and_then(|v| v.get(key));
                    self.walk(
                        old,
                        item,
                        &trusted[key],
                        &supplied[key],
                        &format!("{pointer}/{key}"),
                        &pp,
                        if independent { None } else { host },
                        context_changed || geometry_changed,
                        styles,
                    );
                }
            }
            _ => {}
        }
    }
}

/// Returns a Core copy whose only paragraph caches came from the base and
/// survived the shared flow invalidation rule. Hand-edited JSON caches are
/// ignored even if they look plausible; deleting a JSON cache does not revoke
/// an unchanged base cache either.
pub(crate) fn prepare(
    base: &HwpxDocument,
    document: &Document<Validated>,
    style_store: &HwpxStyleStore,
) -> HwpxResult<(Document<Validated>, Vec<EncodeWarning>)> {
    let trusted = tree(&base.document)?;
    let supplied = tree(document)?;
    let mut original = trusted.clone();
    let mut edited = supplied.clone();
    clean(&mut original);
    clean(&mut edited);
    let styles = Styles { base: tree(&base.style_store)?, edited: tree(style_store)? };
    let mut plan = Plan::default();
    let old = original["sections"].as_array().expect("Core sections");
    let sections = edited["sections"].as_array().expect("Core sections");
    if old.len() != sections.len() {
        plan.warnings.push(EncodeWarning::LayoutCacheDropped {
            path: ParagraphPath(vec![PathSeg::Section(0)]),
            reason: "section structure changed; only the identical prefix has base correspondence"
                .into(),
        });
    }
    let prefix = if old.len() == sections.len() {
        sections.len()
    } else {
        old.iter().zip(sections).take_while(|(a, b)| a == b).count()
    };
    for (s, section) in sections.iter().enumerate() {
        plan.walk(
            old.get(s).filter(|_| s < prefix),
            section,
            &trusted["sections"][s],
            &supplied["sections"][s],
            &format!("/sections/{s}"),
            &[PathSeg::Section(s)],
            None,
            s >= prefix,
            &styles,
        );
        plan.masterpage_offset += section["master_pages"].as_array().map_or(0, Vec::len);
    }
    // 이것을 실패시키는 것: 공용 무효화 대신 편집 문단만 지우기 또는 host 사슬 끊기.
    let stale = crate::cache_flow::stale_paragraphs(
        &plan.para_list,
        &plan.list_host,
        plan.changed,
        &mut || {},
    );
    for (candidate, stale) in plan.candidates.into_iter().zip(stale) {
        if stale || candidate.cache.is_none() {
            if candidate.cache.is_some() || candidate.supplied_cache {
                plan.warnings.push(EncodeWarning::LayoutCacheDropped {
                    path: candidate.path,
                    reason: if candidate.unmatched {
                        "no base paragraph correspondence"
                    } else if stale {
                        "intentionally invalidated by a Core edit or its flow/host chain"
                    } else {
                        "no trusted base cache; supplied layout_cache is not trusted"
                    }
                    .into(),
                });
            }
        } else if let Some(cache) = candidate.cache {
            // 이것을 실패시키는 것: base 캐시 대신 JSON의 layout_cache를 복사하기.
            edited
                .pointer_mut(&candidate.pointer)
                .expect("Core paragraph pointer")
                .as_object_mut()
                .expect("Core paragraph")
                .insert("layout_cache".into(), cache);
        }
    }
    let draft: Document<Draft> = serde_json::from_value(edited)
        .map_err(|e| HwpxError::InvalidStructure { detail: e.to_string() })?;
    Ok((draft.validate()?, plan.warnings))
}
