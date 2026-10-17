//! Typed Core correspondence adapter; package policy lives in `planner`.
use super::{paragraph, shell, Styles};
use crate::{EncodeWarning, ParagraphPath, PathSeg};
use serde_json::Value;

pub(super) struct Candidate {
    pub(super) pointer: String,
    pub(super) path: ParagraphPath,
    pub(super) cache: Option<Value>,
    pub(super) supplied_cache: bool,
    pub(super) unmatched: bool,
}

#[derive(Default)]
pub(super) struct CoreCorrespondence {
    pub(super) candidates: Vec<Candidate>,
    pub(super) para_list: Vec<usize>,
    pub(super) list_host: Vec<Option<usize>>,
    pub(super) changed: Vec<usize>,
    pub(super) warnings: Vec<EncodeWarning>,
    pub(super) masterpage_offset: usize,
}

impl CoreCorrespondence {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn walk(
        &mut self,
        base: Option<&Value>,
        edited: &Value,
        trusted: &Value,
        supplied: &Value,
        pointer: &str,
        path: &[PathSeg],
        host: Option<usize>,
        context_changed: bool,
        styles: &Styles<'_>,
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
                    && (context != base.map(geometry).unwrap_or(Value::Null));
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
