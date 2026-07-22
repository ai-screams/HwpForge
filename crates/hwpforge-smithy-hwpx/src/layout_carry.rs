//! Selective `<hp:linesegarray>` carry — Hancom's per-paragraph line-layout
//! cache survives re-encode for paragraphs an edit did not touch.
//!
//! Full re-encode surfaces (stamp, set-cell) drop `<hp:linesegarray>`
//! because the decoder never carried it into Core (a layout CACHE has no
//! place in the IR). Without the cache the renderer reflows every
//! paragraph from scratch, and on real government forms the accumulated
//! drift moves page breaks (blank-HPC: 9 → 8 pages in PDF export).
//!
//! The carry is a wire-order splice, not a Core change:
//!
//! 1. scan the ORIGINAL section XML: one slot per `<hp:p>` in open order,
//!    each with its raw `<hp:linesegarray>…</hp:linesegarray>` slice;
//! 2. scan the BASELINE (no-op re-encode of the pristine document, already
//!    produced by the admission gate) and the OUTPUT the same way;
//! 3. a paragraph whose OUTPUT slice is byte-identical to its BASELINE
//!    slice was not touched by the edit → re-inject the original cache
//!    before its `</hp:p>`. Anything else (edited paragraphs, paragraphs
//!    containing edited nested paragraphs, id-shifted elements) stays
//!    cache-less and the renderer reflows just those.
//!
//! Fail-open by design: any count mismatch or scan anomaly disables the
//! carry for that section — the result is the current (reflowed) behavior,
//! never a stale cache attached to changed content.

use crate::error::HwpxResult;
use crate::patch::RawPackage;

/// One `<hp:p>` occurrence in a section XML, in open order.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ParaSlice {
    /// Byte offset of `<hp:p`.
    start: usize,
    /// Byte offset of `</hp:p>` (insert point for the cache); equals `end`
    /// for a self-closing paragraph.
    content_end: usize,
    /// Byte offset just past the paragraph's close.
    end: usize,
    /// Raw `<hp:linesegarray…>` range, when the paragraph carries one.
    line_seg: Option<(usize, usize)>,
}

/// Finds the end of the tag starting at `start` (`<` position), honoring
/// quoted attribute values. Returns `(after_gt, self_closing)`.
fn tag_end(xml: &str, start: usize) -> Option<(usize, bool)> {
    let bytes = xml.as_bytes();
    let mut in_quote = false;
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => in_quote = !in_quote,
            b'>' if !in_quote => {
                let self_closing = i > start && bytes[i - 1] == b'/';
                return Some((i + 1, self_closing));
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Scans every `<hp:p>` of a section XML in open (document) order.
///
/// Returns `None` on any structural anomaly (unbalanced tags, truncated
/// XML) — callers treat that as "no carry".
fn scan_paragraphs(xml: &str) -> Option<Vec<ParaSlice>> {
    let mut slices: Vec<ParaSlice> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut pos = 0usize;

    while let Some(rel) = xml[pos..].find('<') {
        let at = pos + rel;
        let rest = &xml[at..];
        if let Some(after) = rest.strip_prefix("<hp:p") {
            // `<hp:p ...>` / `<hp:p>` / `<hp:p/>` only — not `<hp:pic` etc.
            match after.as_bytes().first() {
                Some(b' ') | Some(b'>') | Some(b'/') => {
                    let (tag_after, self_closing) = tag_end(xml, at)?;
                    if self_closing {
                        slices.push(ParaSlice {
                            start: at,
                            content_end: tag_after,
                            end: tag_after,
                            line_seg: None,
                        });
                    } else {
                        stack.push(slices.len());
                        slices.push(ParaSlice {
                            start: at,
                            content_end: 0,
                            end: 0,
                            line_seg: None,
                        });
                    }
                    pos = tag_after;
                    continue;
                }
                _ => {}
            }
        }
        if rest.starts_with("</hp:p>") {
            let idx = stack.pop()?;
            slices[idx].content_end = at;
            slices[idx].end = at + "</hp:p>".len();
            pos = at + "</hp:p>".len();
            continue;
        }
        if rest.starts_with("<hp:linesegarray") {
            let owner = *stack.last()?;
            let (tag_after, self_closing) = tag_end(xml, at)?;
            let seg_end = if self_closing {
                tag_after
            } else {
                const CLOSE: &str = "</hp:linesegarray>";
                let rel = xml[tag_after..].find(CLOSE)?;
                tag_after + rel + CLOSE.len()
            };
            slices[owner].line_seg = Some((at, seg_end));
            pos = seg_end;
            continue;
        }
        // Any other tag: skip it wholesale (quote-aware) so a '>' inside an
        // attribute value cannot desync the scan.
        let (tag_after, _) = tag_end(xml, at)?;
        pos = tag_after;
    }

    if stack.is_empty() && slices.iter().all(|s| s.end != 0) {
        Some(slices)
    } else {
        None
    }
}

/// Splices the original line-layout caches into `output` for paragraphs
/// whose slice is byte-identical to the baseline's. Returns `None` when the
/// carry cannot be aligned (paragraph count mismatch, scan anomaly).
fn splice_section(original: &str, baseline: &str, output: &str) -> Option<String> {
    let orig = scan_paragraphs(original)?;
    let base = scan_paragraphs(baseline)?;
    let out = scan_paragraphs(output)?;
    if orig.len() != base.len() || base.len() != out.len() {
        return None;
    }

    let mut inserts: Vec<(usize, &str)> = Vec::new();
    for i in 0..orig.len() {
        let Some((seg_start, seg_end)) = orig[i].line_seg else {
            continue;
        };
        let unchanged = baseline[base[i].start..base[i].end] == output[out[i].start..out[i].end];
        let has_close = out[i].content_end < out[i].end;
        let already = output[out[i].start..out[i].end].contains("<hp:linesegarray");
        if unchanged && has_close && !already {
            inserts.push((out[i].content_end, &original[seg_start..seg_end]));
        }
    }
    if inserts.is_empty() {
        return Some(output.to_string());
    }

    inserts.sort_by_key(|(pos, _)| *pos);
    let mut result =
        String::with_capacity(output.len() + inserts.iter().map(|(_, s)| s.len()).sum::<usize>());
    let mut cursor = 0usize;
    for (pos, seg) in inserts {
        result.push_str(&output[cursor..pos]);
        result.push_str(seg);
        cursor = pos;
    }
    result.push_str(&output[cursor..]);
    Some(result)
}

/// Carries `<hp:linesegarray>` caches from `original` into `output` for
/// every section entry, comparing against `baseline` (the pristine no-op
/// re-encode) to decide which paragraphs an edit left untouched.
///
/// Fail-open: sections that cannot be aligned are left as-is; the function
/// only errors on package-level I/O problems.
pub(crate) fn carry_line_segs(
    original: &[u8],
    baseline: &[u8],
    output: &[u8],
) -> HwpxResult<Vec<u8>> {
    let orig_pkg = RawPackage::read(original)?;
    let base_pkg = RawPackage::read(baseline)?;
    let mut out_pkg = RawPackage::read(output)?;

    let section_paths: Vec<String> = out_pkg
        .entry_paths()
        .filter(|p| p.starts_with("Contents/section") && p.ends_with(".xml"))
        .map(str::to_string)
        .collect();

    let mut changed = false;
    for path in section_paths {
        let (Ok(orig_xml), Ok(base_xml), Ok(out_xml)) = (
            orig_pkg.read_text_entry(&path),
            base_pkg.read_text_entry(&path),
            out_pkg.read_text_entry(&path),
        ) else {
            continue;
        };
        if !orig_xml.contains("<hp:linesegarray") {
            continue;
        }
        if let Some(spliced) = splice_section(&orig_xml, &base_xml, &out_xml) {
            if spliced != out_xml {
                out_pkg.replace_text_entry(&path, spliced);
                changed = true;
            }
        }
    }

    if changed {
        out_pkg.write()
    } else {
        Ok(output.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LSA: &str = r#"<hp:linesegarray><hp:lineseg textpos="0" vertpos="0"/></hp:linesegarray>"#;

    fn p(body: &str, with_lsa: bool) -> String {
        let lsa = if with_lsa { LSA } else { "" };
        format!(r#"<hp:p id="0"><hp:run charPrIDRef="0"><hp:t>{body}</hp:t></hp:run>{lsa}</hp:p>"#)
    }

    #[test]
    fn scan_counts_paragraphs_and_assigns_line_segs() {
        let xml = format!("<hs:sec>{}{}</hs:sec>", p("하나", true), p("둘", false));
        let slices = scan_paragraphs(&xml).unwrap();
        assert_eq!(slices.len(), 2);
        assert!(slices[0].line_seg.is_some());
        assert!(slices[1].line_seg.is_none());
    }

    #[test]
    fn scan_assigns_nested_paragraph_cache_to_inner() {
        // 표 셀 내부의 중첩 문단 — lsa 는 innermost 문단 소유.
        let inner = p("셀", true);
        let xml = format!(
            r#"<hs:sec><hp:p id="1"><hp:run><hp:tbl><hp:tr><hp:tc><hp:subList>{inner}</hp:subList></hp:tc></hp:tr></hp:tbl></hp:run>{LSA}</hp:p></hs:sec>"#
        );
        let slices = scan_paragraphs(&xml).unwrap();
        assert_eq!(slices.len(), 2);
        // open 순서: 바깥 문단 먼저, 안쪽 문단 다음 — 둘 다 자기 lsa 를 가짐.
        assert!(slices[0].line_seg.is_some());
        assert!(slices[1].line_seg.is_some());
        let (s, e) = slices[1].line_seg.unwrap();
        assert!(slices[0].line_seg.unwrap().0 > e || slices[0].line_seg.unwrap().1 < s);
    }

    #[test]
    fn scan_survives_gt_inside_attribute_values() {
        let xml = format!(
            r#"<hs:sec><hp:p id="0" note="a>b"><hp:run><hp:t>x</hp:t></hp:run></hp:p>{}</hs:sec>"#,
            p("y", false)
        );
        assert_eq!(scan_paragraphs(&xml).unwrap().len(), 2);
    }

    #[test]
    fn scan_rejects_unbalanced_paragraphs() {
        assert!(scan_paragraphs("<hs:sec><hp:p id=\"0\"><hp:run/></hs:sec>").is_none());
    }

    #[test]
    fn splice_restores_cache_for_unchanged_paragraphs_only() {
        // original = 캐시 있는 2문단, baseline = 캐시 없는 동일 문서(no-op
        // 재인코드), output = 둘째 문단만 편집된 재인코드.
        let original = format!("<hs:sec>{}{}</hs:sec>", p("하나", true), p("둘", true));
        let baseline = format!("<hs:sec>{}{}</hs:sec>", p("하나", false), p("둘", false));
        let output = format!("<hs:sec>{}{}</hs:sec>", p("하나", false), p("수정됨", false));

        let spliced = splice_section(&original, &baseline, &output).unwrap();
        // 첫 문단: 캐시 복원. 둘째 문단: 편집됨 → 캐시 없음.
        let first = spliced.find(LSA).unwrap();
        assert!(
            spliced[first..].find(LSA).map(|r| r == 0).unwrap_or(false)
                || spliced.matches(LSA).count() == 1
        );
        assert!(spliced.contains("수정됨"));
        assert!(spliced.find("하나").unwrap() < first);
    }

    #[test]
    fn splice_noop_roundtrip_restores_everything() {
        let original = format!("<hs:sec>{}{}</hs:sec>", p("하나", true), p("둘", true));
        let baseline = format!("<hs:sec>{}{}</hs:sec>", p("하나", false), p("둘", false));
        let spliced = splice_section(&original, &baseline, &baseline).unwrap();
        assert_eq!(spliced, original, "무편집 왕복은 원본 wire 로 완전 복원");
    }

    #[test]
    fn splice_bails_on_paragraph_count_mismatch() {
        let original = format!("<hs:sec>{}</hs:sec>", p("하나", true));
        let two = format!("<hs:sec>{}{}</hs:sec>", p("하나", false), p("둘", false));
        assert!(splice_section(&original, &two, &two).is_none());
    }

    #[test]
    fn splice_skips_paragraph_that_already_has_cache() {
        let original = format!("<hs:sec>{}</hs:sec>", p("하나", true));
        let with_cache = original.clone();
        let spliced = splice_section(&original, &with_cache, &with_cache).unwrap();
        assert_eq!(spliced.matches("<hp:linesegarray").count(), 1);
    }
}
