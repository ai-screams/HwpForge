//! Selective `<hp:linesegarray>` carry — Hancom's per-paragraph line-layout
//! cache survives re-encode for paragraphs an edit did not touch.
//!
//! Full re-encode surfaces (stamp, set-cell) drop `<hp:linesegarray>`
//! because the decoder never carried it into Core (a layout CACHE has no
//! place in the IR). Without the cache the renderer reflows every
//! paragraph from scratch, and on real government forms the accumulated
//! drift moves page breaks (blank-HPC: 9 → 8 pages in PDF export).
//!
//! The carry is a wire-order splice:
//!
//! 1. scan the BASELINE (no-op re-encode of the pristine document, already
//!    produced by the admission gate) and the OUTPUT: one slot per `<hp:p>`
//!    in open order;
//! 2. scan the SOURCE — the edited document encoded once more with its Core
//!    caches emitted ([`cache_source`]), which must equal the OUTPUT but for
//!    those caches;
//! 3. a paragraph whose OUTPUT slice is byte-identical to its BASELINE
//!    slice was not touched by the edit → take its cache from the SOURCE
//!    and insert it before its `</hp:p>` — unless it comes after a changed
//!    paragraph in the same flow, or after a changed paragraph's host in
//!    the host's flow (its position moved; W2). Anything else (edited
//!    paragraphs, paragraphs containing edited nested paragraphs,
//!    id-shifted elements) stays cache-less and the renderer reflows just
//!    those.
//!
//! The cache comes from the encoder, not from the original XML: its line
//! starts are written for the tokens the OUTPUT holds (the encoder may add
//! one the input omitted, such as a `colPr`), and a cache the decoder
//! dropped is not in Core, so it is not revived (branch review A2).
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
    /// The paragraph list (section body = 0, each `<hp:subList>` its own)
    /// the paragraph sits in.
    list: usize,
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
    // Open paragraph lists, innermost last; the section body is list 0.
    let mut lists: Vec<usize> = vec![0];
    let mut next_list = 1usize;
    let mut pos = 0usize;

    while let Some(rel) = xml[pos..].find('<') {
        let at = pos + rel;
        let rest = &xml[at..];
        if let Some(after) = rest.strip_prefix("<hp:p") {
            // `<hp:p ...>` / `<hp:p>` / `<hp:p/>` only — not `<hp:pic` etc.
            match after.as_bytes().first() {
                Some(b' ') | Some(b'>') | Some(b'/') => {
                    let (tag_after, self_closing) = tag_end(xml, at)?;
                    let list = *lists.last()?;
                    if self_closing {
                        slices.push(ParaSlice {
                            start: at,
                            content_end: tag_after,
                            end: tag_after,
                            line_seg: None,
                            list,
                        });
                    } else {
                        stack.push(slices.len());
                        slices.push(ParaSlice {
                            start: at,
                            content_end: 0,
                            end: 0,
                            line_seg: None,
                            list,
                        });
                    }
                    pos = tag_after;
                    continue;
                }
                _ => {}
            }
        }
        if rest.starts_with("<hp:subList") {
            let (tag_after, self_closing) = tag_end(xml, at)?;
            if !self_closing {
                lists.push(next_list);
                next_list += 1;
            }
            pos = tag_after;
            continue;
        }
        if rest.starts_with("</hp:subList>") {
            if lists.len() < 2 {
                return None;
            }
            lists.pop();
            pos = at + "</hp:subList>".len();
            continue;
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

    if stack.is_empty() && lists.len() == 1 && slices.iter().all(|s| s.end != 0) {
        Some(slices)
    } else {
        None
    }
}

/// Splices line-layout caches into `output` for paragraphs whose slice is
/// byte-identical to the baseline's, taking each cache from `source` — the
/// same edited document encoded with its Core caches emitted, so every line
/// start is written through the encoder's own token ledger for exactly the
/// tokens `output` holds (branch review A2: the original cache XML counted
/// the original's tokens, and an encoder that adds one — a `colPr` the
/// input omitted — moved every line start after it). Returns `None` when
/// the carry cannot be aligned (paragraph count mismatch, scan anomaly, or
/// a `source` that differs from `output` by more than its caches).
fn splice_section(baseline: &str, output: &str, source: &str) -> Option<String> {
    let base = scan_paragraphs(baseline)?;
    let out = scan_paragraphs(output)?;
    let src = scan_paragraphs(source)?;
    if base.len() != out.len() || src.len() != out.len() {
        return None;
    }
    // The caches are the only difference: same tokens, same coordinates.
    // 이것을 실패시키는 것: 이 검사 삭제(`splice_bails_when_the_source_differs_beyond_caches`).
    if without_caches(source, &src) != output {
        return None;
    }

    // W2 (design 3.5): a changed paragraph moves everything after it in its
    // flow, so the paragraphs after it in its list keep no cache even when
    // their bytes are the same. Its host is changed too (its bytes hold the
    // edit), so the same holds one level up, to the section body.
    // 이것을 실패시키는 것: 꼬리 대신 문단 하나만 · 목록 처음부터 지우기
    // (`edit_in_a_cell_drops_the_flow_tail_and_the_host_chain`).
    let changed: Vec<bool> = (0..out.len())
        .map(|i| baseline[base[i].start..base[i].end] != output[out[i].start..out[i].end])
        .collect();
    // The rule `fill` and `patch` use (`cache_flow::stale_paragraphs`), one
    // pass over the paragraphs (branch review r2 M4: walking to the end of
    // the list from every changed paragraph was quadratic in a batch edit).
    // 이것을 실패시키는 것: 바뀐 문단마다 끝까지 훑기(`carry_stays_linear`).
    let para_list: Vec<usize> = out.iter().map(|s| s.list).collect();
    let lists = para_list.iter().max().map_or(0, |m| m + 1);
    let stale = crate::cache_flow::stale_paragraphs(
        &para_list,
        &vec![None; lists],
        (0..out.len()).filter(|&i| changed[i]),
        &mut || {},
    );

    let mut inserts: Vec<(usize, &str)> = Vec::new();
    for i in 0..out.len() {
        let Some((seg_start, seg_end)) = src[i].line_seg else {
            continue;
        };
        let unchanged = !changed[i] && !stale[i];
        let has_close = out[i].content_end < out[i].end;
        if unchanged && has_close {
            inserts.push((out[i].content_end, &source[seg_start..seg_end]));
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

/// `xml` without the `<hp:linesegarray>` ranges its `slices` found.
fn without_caches(xml: &str, slices: &[ParaSlice]) -> String {
    let mut ranges: Vec<(usize, usize)> = slices.iter().filter_map(|s| s.line_seg).collect();
    ranges.sort_unstable();
    let mut out = String::with_capacity(xml.len());
    let mut cursor = 0;
    for (start, end) in ranges {
        out.push_str(&xml[cursor..start]);
        cursor = end;
    }
    out.push_str(&xml[cursor..]);
    out
}

/// Carries `<hp:linesegarray>` caches into `output` for every section entry
/// whose `original` held any, comparing against `baseline` (the pristine
/// no-op re-encode) to decide which paragraphs an edit left untouched and
/// taking each cache from `source` ([`cache_source`]). A cache the decoder
/// dropped is not in Core, so it is not in `source` and is not revived.
///
/// Fail-open: sections that cannot be aligned are left as-is; the function
/// only errors on package-level I/O problems.
pub(crate) fn carry_line_segs(
    original: &[u8],
    baseline: &[u8],
    output: &[u8],
    source: &[u8],
) -> HwpxResult<Vec<u8>> {
    let orig_pkg = RawPackage::read(original)?;
    let base_pkg = RawPackage::read(baseline)?;
    let src_pkg = RawPackage::read(source)?;
    let mut out_pkg = RawPackage::read(output)?;

    let section_paths: Vec<String> = out_pkg
        .entry_paths()
        .filter(|p| p.starts_with("Contents/section") && p.ends_with(".xml"))
        .map(str::to_string)
        .collect();

    let mut changed = false;
    for path in section_paths {
        let (Ok(orig_xml), Ok(base_xml), Ok(out_xml), Ok(src_xml)) = (
            orig_pkg.read_text_entry(&path),
            base_pkg.read_text_entry(&path),
            out_pkg.read_text_entry(&path),
            src_pkg.read_text_entry(&path),
        ) else {
            continue;
        };
        if !orig_xml.contains("<hp:linesegarray") {
            continue;
        }
        if let Some(spliced) = splice_section(&base_xml, &out_xml, &src_xml) {
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

/// The edited document encoded with its Core line caches emitted: the
/// cache source of [`carry_line_segs`]. A cache the encoder cannot place is
/// left out (`LayoutCacheDropped`); those warnings are not returned — the
/// paragraph then simply keeps no cache, as an edited one does.
pub(crate) fn cache_source(
    document: &hwpforge_core::Document<hwpforge_core::Validated>,
    style_store: &crate::style_store::HwpxStyleStore,
    image_store: &hwpforge_core::image::ImageStore,
) -> HwpxResult<Vec<u8>> {
    crate::HwpxEncoder::encode_with_diagnostics(
        document,
        style_store,
        image_store,
        crate::EncodeOptions::default().with_emit_layout_cache(true),
    )
    .map(|outcome| outcome.bytes)
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
        // baseline = 캐시 없는 no-op 재인코드, output = 둘째 문단만 편집된
        // 재인코드, source = output 에 Core 캐시를 방출한 것.
        let baseline = format!("<hs:sec>{}{}</hs:sec>", p("하나", false), p("둘", false));
        let output = format!("<hs:sec>{}{}</hs:sec>", p("하나", false), p("수정됨", false));
        let source = format!("<hs:sec>{}{}</hs:sec>", p("하나", true), p("수정됨", true));

        let spliced = splice_section(&baseline, &output, &source).unwrap();
        // 첫 문단: 캐시 복원. 둘째 문단: 편집됨 → 캐시 없음.
        assert_eq!(spliced, format!("<hs:sec>{}{}</hs:sec>", p("하나", true), p("수정됨", false)));
    }

    #[test]
    fn splice_noop_roundtrip_restores_everything() {
        let baseline = format!("<hs:sec>{}{}</hs:sec>", p("하나", false), p("둘", false));
        let source = format!("<hs:sec>{}{}</hs:sec>", p("하나", true), p("둘", true));
        let spliced = splice_section(&baseline, &baseline, &source).unwrap();
        assert_eq!(spliced, source, "무편집 왕복은 방출한 캐시로 완전 복원");
    }

    #[test]
    fn splice_bails_on_paragraph_count_mismatch() {
        let one = format!("<hs:sec>{}</hs:sec>", p("하나", true));
        let two = format!("<hs:sec>{}{}</hs:sec>", p("하나", false), p("둘", false));
        assert!(splice_section(&two, &two, &one).is_none());
    }

    /// Branch review r2 M4: a batch edit changing every one of N paragraphs
    /// costs work linear in N.
    #[test]
    fn carry_stays_linear() {
        const N: usize = 2000;
        let section = |text: &str, lsa: bool| {
            let body: String = (0..N).map(|i| p(&format!("{text}{i}"), lsa)).collect();
            format!("<hs:sec>{body}</hs:sec>")
        };
        let baseline = section("a", false);
        let output = section("b", false);
        let source = section("b", true);
        crate::work::WORK.with(|w| w.set(0));
        let spliced = splice_section(&baseline, &output, &source).unwrap();
        let work = crate::work::WORK.with(std::cell::Cell::get);
        assert_eq!(spliced, output);
        assert!(work <= 3 * N, "{work} work units for {N} paragraphs");
    }

    /// Branch review A2: the cache source must hold the output's tokens
    /// exactly — otherwise its coordinates count other tokens.
    #[test]
    fn splice_bails_when_the_source_differs_beyond_caches() {
        let output = format!("<hs:sec>{}{}</hs:sec>", p("하나", false), p("둘", false));
        let source = format!("<hs:sec>{}{}</hs:sec>", p("하나", true), p("둘셋", true));
        assert!(splice_section(&output, &output, &source).is_none());
    }

    /// W2 (design 3.5 test 2): editing the second of three cell paragraphs
    /// drops the caches of that paragraph, the cell paragraph after it, the
    /// table's host paragraph and the body paragraph after the host; the
    /// first cell paragraph and the body paragraph before keep theirs.
    #[test]
    fn edit_in_a_cell_drops_the_flow_tail_and_the_host_chain() {
        let section = |cell1: &str, lsa: bool| {
            let cell = format!("{}{}{}", p("c0", lsa), p(cell1, lsa), p("c2", lsa));
            let host_lsa = if lsa { LSA } else { "" };
            format!(
                r#"<hs:sec>{}<hp:p id="1"><hp:run><hp:tbl><hp:tr><hp:tc><hp:subList>{cell}</hp:subList></hp:tc></hp:tr></hp:tbl></hp:run>{host_lsa}</hp:p>{}</hs:sec>"#,
                p("before", lsa),
                p("after", lsa)
            )
        };
        let baseline = section("c1", false);
        let output = section("c1 이 여러 줄로 길어졌다", false);
        let source = section("c1 이 여러 줄로 길어졌다", true);
        let spliced = splice_section(&baseline, &output, &source).unwrap();
        let slices = scan_paragraphs(&spliced).unwrap();
        // open order: before, host, c0, c1, c2, after.
        let cached: Vec<bool> = slices.iter().map(|s| s.line_seg.is_some()).collect();
        assert_eq!(cached, [true, false, true, false, false, false]);
    }
}
