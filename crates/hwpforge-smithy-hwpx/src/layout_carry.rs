//! Selective `<hp:linesegarray>` carry — Hancom's per-paragraph line-layout
//! cache survives re-encode for paragraphs an edit did not touch.
//!
//! Default re-encoding omits Core layout caches. Carry restores only caches
//! whose encoder-validated coordinates still match the edited flow.
//!
//! The carry is a wire-order splice:
//!
//! 1. scan the BASELINE (no-op re-encode of the pristine document, already
//!    produced by the admission gate) and the PROBE: one slot per `<hp:p>`
//!    in open order;
//! 2. scan the SOURCE — the edited document encoded once more with its Core
//!    caches emitted ([`cache_source`]), which must equal the OUTPUT but for
//!    those caches;
//! 3. a paragraph whose PROBE slice is byte-identical to its BASELINE
//!    slice was not touched by the edit → take its cache from the SOURCE
//!    and insert it before its `</hp:p>` — unless it comes after a changed
//!    paragraph in the same flow, or after a changed paragraph's host in
//!    the host's flow (its position moved). Anything else (edited
//!    paragraphs, paragraphs containing edited nested paragraphs,
//!    id-shifted elements) stays cache-less and the renderer reflows just
//!    those.
//!
//! A cell edit may restore only a supported one-line cell's old Text in the
//! probe. Nontext changes and unproven edits remain visible. Stamp uses its
//! output as the probe. Insert positions and flow membership always come
//! from the actual output.
//!
//! The cache comes from the encoder, not from the original XML: its line
//! starts are written for the tokens the OUTPUT holds (the encoder may add
//! one the input omitted, such as a `colPr`), and a cache the decoder
//! dropped is not in Core, so it is not revived.
//!
//! Fail-open by design: any count mismatch or scan anomaly disables the
//! carry for that section — the result is the current (reflowed) behavior,
//! never a stale cache attached to changed content.

use crate::error::{CodecFailure, CodecStage, HwpxError, HwpxResult};
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

/// Splices line-layout caches into `output` for paragraphs whose probe slice
/// is byte-identical to the baseline's, taking each cache from `source` — the
/// same edited document encoded with its Core caches emitted, so every line
/// start is written through the encoder's own token ledger for exactly the
/// tokens `output` holds (the original cache XML counted
/// the original's tokens, and an encoder that adds one — a `colPr` the
/// input omitted — moved every line start after it). Returns `None` when
/// the carry cannot be aligned (paragraph count mismatch, scan anomaly, or
/// a `source` that differs from `output` by more than its caches).
///
/// Alongside the spliced section come the marker ordinals (within this
/// section) of the caches the encoder could not place for a paragraph that
/// would have kept its cache — the drops the caller reports;
/// a paragraph the edit changed or moved loses its cache on purpose and is
/// not among them.
fn splice_section(
    baseline: &str,
    output: &str,
    probe: &str,
    source: &str,
) -> Option<(String, Vec<usize>)> {
    let base = scan_paragraphs(baseline)?;
    let out = scan_paragraphs(output)?;
    let src = scan_paragraphs(source)?;
    let comparison = scan_paragraphs(probe)?;
    if base.len() != out.len() || src.len() != out.len() || comparison.len() != out.len() {
        return None;
    }
    // The caches are the only difference: same tokens, same coordinates.
    // 이것을 실패시키는 것: 이 검사 삭제(`splice_bails_when_the_source_differs_beyond_caches`).
    if without_caches(source, &src) != output {
        return None;
    }

    //: a changed paragraph moves everything after it in its
    // flow, so the paragraphs after it in its list keep no cache even when
    // their bytes are the same. Its host is changed too (its bytes hold the
    // edit), so the same holds one level up, to the section body.
    // 이것을 실패시키는 것: 꼬리 대신 문단 하나만 · 목록 처음부터 지우기
    // (`edit_in_a_cell_drops_the_flow_tail_and_the_host_chain`).
    let changed: Vec<bool> = (0..out.len())
        .map(|i| {
            baseline[base[i].start..base[i].end] != probe[comparison[i].start..comparison[i].end]
        })
        .collect();
    // The conservative re-encoding rule (`cache_flow::stale_paragraphs`), one
    // pass over the paragraphs (walking to the end of
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
    let mut dropped = Vec::new();
    for i in 0..out.len() {
        let Some((seg_start, seg_end)) = src[i].line_seg else {
            continue;
        };
        let unchanged = !changed[i] && !stale[i];
        let has_close = out[i].content_end < out[i].end;
        let seg = &source[seg_start..seg_end];
        // A marker cache is a drop with its reason.
        // 이것을 실패시키는 것: 표지를 캐시로 넣거나 보고하지 않기
        // (`an_untouched_cache_the_encoder_cannot_place_is_reported`).
        if let Some(warning) = dropped_mark(seg) {
            if unchanged && has_close {
                dropped.push(warning);
            }
            continue;
        }
        if unchanged && has_close {
            inserts.push((out[i].content_end, seg));
        }
    }
    if inserts.is_empty() {
        return Some((output.to_string(), dropped));
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
    Some((result, dropped))
}

/// The ordinal a marker cache ([`crate::encoder::DROPPED_CACHE_MARK`])
/// carries within its section, or `None` for a real cache.
fn dropped_mark(seg: &str) -> Option<usize> {
    let mark = format!(r#"textpos="{}""#, crate::encoder::DROPPED_CACHE_MARK);
    if !seg.contains(&mark) {
        return None;
    }
    let at = seg.find(r#"vertpos=""#)? + r#"vertpos=""#.len();
    seg[at..].split('"').next()?.parse().ok()
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
/// whose `original` held any. It compares `baseline` (the pristine no-op
/// re-encode) with `probe`, which restores only proven cell text and keeps
/// nontext changes visible. Caches come from `source` ([`cache_source`])
/// and are inserted into `output`. A cache the decoder dropped is not in
/// Core, so it is not in `source` and is not revived.
///
/// The warnings returned are the source encode's `LayoutCacheDropped` for
/// each untouched paragraph whose cache it could not place, so that drop is
/// reported; caches the edit made stale are dropped without one.
///
/// Fail-open: sections that cannot be aligned are left as-is; the function
/// only errors on package-level I/O problems.
pub(crate) fn carry_line_segs(
    original: &[u8],
    baseline: &[u8],
    output: &[u8],
    probe: &[u8],
    source: &CacheSource,
) -> Result<(Vec<u8>, Vec<crate::EncodeWarning>), CodecFailure> {
    let mut reported = Vec::new();
    let orig_pkg =
        RawPackage::read(original).map_err(|e| CodecFailure::hwpx(CodecStage::Verification, &e))?;
    let base_pkg =
        RawPackage::read(baseline).map_err(|e| CodecFailure::hwpx(CodecStage::Verification, &e))?;
    let probe_pkg =
        RawPackage::read(probe).map_err(|e| CodecFailure::hwpx(CodecStage::Verification, &e))?;
    let src_pkg = RawPackage::read(&source.bytes)
        .map_err(|e| CodecFailure::hwpx(CodecStage::Verification, &e))?;
    let mut out_pkg =
        RawPackage::read(output).map_err(|e| CodecFailure::hwpx(CodecStage::Verification, &e))?;

    let section_paths: Vec<String> = out_pkg
        .entry_paths()
        .filter(|p| p.starts_with("Contents/section") && p.ends_with(".xml"))
        .map(str::to_string)
        .collect();

    let mut changed = false;
    for path in section_paths {
        // Output part numbers are Core indices; only the original retains its spine names.
        // 이것을 실패시키는 것: 출력 파일명으로 원본 조회, 경로 누락 때 unwrap.
        let Some(section) = section_index(&path) else {
            continue;
        };
        let Ok(original_path) = orig_pkg.section_path(section) else {
            continue;
        };
        // Alignment/decoding fallback must not suppress a semantic selection
        // error. These borrowed reads do not decompress or reparse metadata.
        for (package, part) in [
            (&orig_pkg, &original_path),
            (&base_pkg, &path),
            (&out_pkg, &path),
            (&src_pkg, &path),
            (&probe_pkg, &path),
        ] {
            match package.read_entry(part) {
                Ok(_) | Err(HwpxError::MissingFile { .. }) => {}
                Err(error) => return Err(CodecFailure::hwpx(CodecStage::Verification, &error)),
            }
        }
        let (Ok(orig_xml), Ok(base_xml), Ok(out_xml), Ok(src_xml), Ok(probe_xml)) = (
            orig_pkg.read_text_entry(&original_path),
            base_pkg.read_text_entry(&path),
            out_pkg.read_text_entry(&path),
            src_pkg.read_text_entry(&path),
            probe_pkg.read_text_entry(&path),
        ) else {
            continue;
        };
        if !orig_xml.contains("<hp:linesegarray") {
            continue;
        }
        if let Some((spliced, dropped)) = splice_section(&base_xml, &out_xml, &probe_xml, &src_xml)
        {
            // a marker's ordinal counts within its own section.
            // 이것을 실패시키는 것: 문서 전체 번호로 읽기(`drops_in_two_sections_are_each_reported`).
            let section_drops = source.dropped.get(section);
            reported.extend(
                dropped.into_iter().filter_map(|i| section_drops.and_then(|d| d.get(i)).cloned()),
            );
            if spliced != out_xml {
                out_pkg.replace_text_entry(&path, spliced);
                changed = true;
            }
        }
    }

    let bytes = if changed {
        out_pkg.write().map_err(|e| CodecFailure::hwpx(CodecStage::Encode, &e))?
    } else {
        output.to_vec()
    };
    Ok((bytes, reported))
}

/// The cache source of [`carry_line_segs`]: the encoded bytes (dropped
/// caches marked) and, per section, the warning of each marked cache by its
/// marker ordinal.
pub(crate) struct CacheSource {
    bytes: Vec<u8>,
    dropped: Vec<Vec<crate::EncodeWarning>>,
    warnings: Vec<crate::EncodeWarning>,
}

impl CacheSource {
    #[cfg(test)]
    pub(crate) fn replace_bytes_for_test(&mut self, bytes: Vec<u8>) {
        self.bytes = bytes;
    }

    /// Decodes marked output only when a source cache was refused.
    pub(crate) fn marked_document(&self) -> HwpxResult<Option<crate::HwpxDocument>> {
        if self.dropped.iter().all(Vec::is_empty) {
            return Ok(None);
        }
        crate::HwpxDecoder::decode(&self.bytes).map(Some)
    }
}

/// The section index of a `Contents/sectionN.xml` part name.
fn section_index(part: &str) -> Option<usize> {
    part.strip_prefix("Contents/section")?.strip_suffix(".xml")?.parse().ok()
}

/// The edited document encoded with its Core line caches emitted: the
/// cache source of [`carry_line_segs`]. A cache the encoder cannot place is
/// written as a marker with an ordinal, and its `LayoutCacheDropped`
/// warning kept under that ordinal; the carry reports it when the paragraph
/// was untouched.
pub(crate) fn cache_source(
    document: &hwpforge_core::Document<hwpforge_core::Validated>,
    style_store: &crate::style_store::HwpxStyleStore,
    image_store: &hwpforge_core::image::ImageStore,
) -> HwpxResult<CacheSource> {
    cache_source_with_options(document, style_store, image_store, crate::EncodeOptions::default())
}

/// The same ledger-backed source, preserving caller-controlled memo times.
pub(crate) fn cache_source_with_options(
    document: &hwpforge_core::Document<hwpforge_core::Validated>,
    style_store: &crate::style_store::HwpxStyleStore,
    image_store: &hwpforge_core::image::ImageStore,
    options: crate::EncodeOptions,
) -> HwpxResult<CacheSource> {
    let mut options = options.with_emit_layout_cache(true);
    options.mark_dropped_caches = true;
    crate::HwpxEncoder::encode_reporting_dropped_caches(document, style_store, image_store, options)
        .map(|(outcome, dropped)| CacheSource {
            bytes: outcome.bytes,
            dropped,
            warnings: outcome.warnings,
        })
}

/// Carries a source whose Core caches have already been selected by
/// `layout_edit`. The ordinary splice still verifies token identity and
/// removes ledger-failure markers. All source drops now concern retained
/// candidates, so none are suppressed as intentional edit invalidations.
pub(crate) fn carry_prepared(
    output: &[u8],
    source: CacheSource,
) -> HwpxResult<crate::EncodeOutcome> {
    let src = RawPackage::read(&source.bytes)?;
    let mut out = RawPackage::read(output)?;
    let paths: Vec<String> = out
        .entry_paths()
        .filter(|p| {
            (p.starts_with("Contents/section") || p.starts_with("Contents/masterpage"))
                && p.ends_with(".xml")
        })
        .map(str::to_string)
        .collect();
    let mut warnings = source.warnings;
    for path in paths {
        let output = out.read_text_entry(&path)?;
        let source = src.read_text_entry(&path)?;
        // 이것을 실패시키는 것: 출처/출력 토큰 검증 없이 캐시를 삽입하기.
        if let Some((spliced, _)) = splice_section(&output, &output, &output, &source) {
            out.replace_text_entry(&path, spliced);
        } else {
            warnings.push(crate::EncodeWarning::LayoutCacheDropped {
                path: crate::ParagraphPath(
                    section_index(&path)
                        .map(crate::PathSeg::Section)
                        .or_else(|| {
                            path.strip_prefix("Contents/masterpage")?
                                .strip_suffix(".xml")?
                                .parse()
                                .ok()
                                .map(crate::PathSeg::MasterPage)
                        })
                        .into_iter()
                        .collect(),
                ),
                reason: format!("cache source cannot be aligned with encoded part {path}"),
            });
        }
    }
    Ok(crate::EncodeOutcome { bytes: out.write()?, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn carry_read_failure_keeps_verification_payload() {
        // 이것을 실패시키는 것: carry의 원본 package 읽기 stage를 Decode로 변경.
        let source = CacheSource { bytes: vec![], dropped: vec![], warnings: vec![] };
        let original = RawPackage::read(b"bad").err().unwrap();
        let error = carry_line_segs(b"bad", b"bad", b"bad", b"bad", &source).unwrap_err();
        assert_eq!(
            error,
            CodecFailure {
                stage: CodecStage::Verification,
                code: original.code(),
                detail: original.to_string()
            }
        );
    }

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

    /// 이것을 실패시키는 것: 공유 stale_paragraphs를 자기 문단만 제거하도록 완화.
    #[test]
    fn preserve_policy_does_not_change_carry_bytes() {
        let baseline =
            format!("<hs:sec>{}{}{}</hs:sec>", p("앞", false), p("원래 값", false), p("뒤", false));
        let output = baseline.replace("원래 값", "새 값");
        let source =
            format!("<hs:sec>{}{}{}</hs:sec>", p("앞", true), p("새 값", true), p("뒤", true));
        let expected =
            format!("<hs:sec>{}{}{}</hs:sec>", p("앞", true), p("새 값", false), p("뒤", false));
        assert_eq!(
            splice_section(&baseline, &output, &output, &source),
            Some((expected, Vec::new()))
        );
    }

    #[test]
    fn splice_restores_cache_for_unchanged_paragraphs_only() {
        // baseline = 캐시 없는 no-op 재인코드, output = 둘째 문단만 편집된
        // 재인코드, source = output 에 Core 캐시를 방출한 것.
        let baseline = format!("<hs:sec>{}{}</hs:sec>", p("하나", false), p("둘", false));
        let output = format!("<hs:sec>{}{}</hs:sec>", p("하나", false), p("수정됨", false));
        let source = format!("<hs:sec>{}{}</hs:sec>", p("하나", true), p("수정됨", true));

        let (spliced, _) = splice_section(&baseline, &output, &output, &source).unwrap();
        // 첫 문단: 캐시 복원. 둘째 문단: 편집됨 → 캐시 없음.
        assert_eq!(spliced, format!("<hs:sec>{}{}</hs:sec>", p("하나", true), p("수정됨", false)));
    }

    #[test]
    fn splice_noop_roundtrip_restores_everything() {
        let baseline = format!("<hs:sec>{}{}</hs:sec>", p("하나", false), p("둘", false));
        let source = format!("<hs:sec>{}{}</hs:sec>", p("하나", true), p("둘", true));
        let (spliced, _) = splice_section(&baseline, &baseline, &baseline, &source).unwrap();
        assert_eq!(spliced, source, "무편집 왕복은 방출한 캐시로 완전 복원");
    }

    #[test]
    fn splice_bails_on_paragraph_count_mismatch() {
        let one = format!("<hs:sec>{}</hs:sec>", p("하나", true));
        let two = format!("<hs:sec>{}{}</hs:sec>", p("하나", false), p("둘", false));
        assert!(splice_section(&two, &two, &two, &one).is_none());
    }

    /// A batch edit changing every one of N paragraphs
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
        let (spliced, _) = splice_section(&baseline, &output, &output, &source).unwrap();
        let work = crate::work::WORK.with(std::cell::Cell::get);
        assert_eq!(spliced, output);
        assert!(work <= 3 * N, "{work} work units for {N} paragraphs");
    }

    /// An untouched paragraph whose cache the encoder
    /// could not place (a marker in the source) is reported by its warning
    /// index; a changed paragraph's marker is not, and no marker is spliced
    /// in.
    /// 이것을 실패시키는 것: 표지를 캐시로 넣기 · 보고하지 않기 · 바뀐 문단도 보고하기.
    #[test]
    fn an_untouched_cache_the_encoder_cannot_place_is_reported() {
        let mark = |i: usize| {
            format!(
                r#"<hp:p id="0"><hp:run charPrIDRef="0"><hp:t>{{}}</hp:t></hp:run><hp:linesegarray><hp:lineseg textpos="{}" vertpos="{i}"/></hp:linesegarray></hp:p>"#,
                crate::encoder::DROPPED_CACHE_MARK
            )
        };
        let marked = |i: usize, body: &str| mark(i).replace("{}", body);
        let baseline = format!("<hs:sec>{}{}</hs:sec>", p("하나", false), p("둘", false));
        let output = format!("<hs:sec>{}{}</hs:sec>", p("하나", false), p("바뀜", false));
        let source = format!("<hs:sec>{}{}</hs:sec>", marked(3, "하나"), marked(5, "바뀜"));
        let (spliced, dropped) = splice_section(&baseline, &output, &output, &source).unwrap();
        assert_eq!(spliced, output);
        assert_eq!(dropped, [3]);
    }

    /// The cache source marks a cache the encoder cannot place — here a
    /// line said to start after a table where the paragraph has none — with
    /// the index of the warning that says why.
    /// 이것을 실패시키는 것: 인코더 표지 삭제(캐시 없는 문단과 구별되지 않는다).
    #[test]
    fn the_cache_source_marks_a_cache_it_cannot_place() {
        use hwpforge_core::layout::{LayoutCache, LineBoundary, LineSeg, WireTokenKind};
        use hwpforge_core::run::Run;
        use hwpforge_core::{Document, PageSettings, Paragraph, Section};
        use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
        let line = |textpos: u32, boundary: Option<LineBoundary>| {
            let mut l = LineSeg::default();
            l.textpos = textpos;
            l.vertsize = 1000;
            l.horzsize = 42520;
            l.boundary = boundary;
            l
        };
        let mut para = Paragraph::with_runs(
            vec![Run::text("AB", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        para.layout_cache = Some(LayoutCache::new(vec![
            line(0, None),
            line(1, Some(LineBoundary::new(vec![WireTokenKind::Table], None))),
        ]));
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![
                Paragraph::with_runs(
                    vec![Run::text("앞", CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                ),
                para,
            ],
            PageSettings::a4(),
        ));
        let styles = crate::style_store_for_preset("default").unwrap();
        let source = cache_source(
            &doc.validate().unwrap(),
            &styles,
            &hwpforge_core::image::ImageStore::new(),
        )
        .unwrap();
        let xml = RawPackage::read(&source.bytes)
            .unwrap()
            .read_text_entry("Contents/section0.xml")
            .unwrap();
        let at = xml.rfind("<hp:linesegarray").unwrap();
        let index = dropped_mark(&xml[at..]).expect("a marker cache");
        assert!(matches!(
            source.dropped[0][index],
            crate::EncodeWarning::LayoutCacheDropped { .. }
        ));
    }

    /// An untouched cache the encoder cannot place in
    /// each of two sections is reported once each, with its own section's
    /// path — the marker ordinals count within a section.
    /// 이것을 실패시키는 것: 표지 번호를 문서 전체 번호로 읽기(구역 0 의 경고가
    /// 두 번, 구역 1 은 빠진다).
    #[test]
    fn drops_in_two_sections_are_each_reported() {
        use hwpforge_core::layout::{LayoutCache, LineBoundary, LineSeg, WireTokenKind};
        use hwpforge_core::run::Run;
        use hwpforge_core::{Document, PageSettings, Paragraph, Section};
        use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
        let line = |textpos: u32, boundary: Option<LineBoundary>| {
            let mut l = LineSeg::default();
            l.textpos = textpos;
            l.vertsize = 1000;
            l.boundary = boundary;
            l
        };
        let text = |t: &str| {
            Paragraph::with_runs(vec![Run::text(t, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
        };
        let mut doc = Document::new();
        for _ in 0..2 {
            let mut bad = text("AB");
            bad.layout_cache = Some(LayoutCache::new(vec![
                line(0, None),
                line(1, Some(LineBoundary::new(vec![WireTokenKind::Table], None))),
            ]));
            doc.add_section(Section::with_paragraphs(vec![text("앞"), bad], PageSettings::a4()));
        }
        let doc = doc.validate().unwrap();
        let styles = crate::style_store_for_preset("default").unwrap();
        let images = hwpforge_core::image::ImageStore::new();
        let output = crate::HwpxEncoder::encode(&doc, &styles, &images).unwrap();
        let source = cache_source(&doc, &styles, &images).unwrap();
        let (_, reported) =
            carry_line_segs(&source.bytes, &output, &output, &output, &source).unwrap();
        let paths: Vec<String> = reported
            .iter()
            .map(|w| match w {
                crate::EncodeWarning::LayoutCacheDropped { path, .. } => path.to_string(),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(paths.len(), 2, "{paths:?}");
        assert!(
            paths[0].starts_with("section[0]") && paths[1].starts_with("section[1]"),
            "{paths:?}"
        );
    }

    // 이것을 실패시키는 것: mixed_text의 CRLF 캐시 거부 또는 carry의 drop 보고 삭제.
    #[test]
    fn crlf_cache_drop_survives_layout_carry() {
        use hwpforge_core::layout::{LayoutCache, LineSeg};
        use hwpforge_core::{Document, PageSettings, Paragraph, Run, Section};
        use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
        let mut para = Paragraph::with_runs(
            vec![Run::text("가😀\r\n나", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        para.layout_cache = Some(LayoutCache::new(vec![LineSeg::default()]));
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(vec![para], PageSettings::default()));
        let doc = doc.validate().unwrap();
        let styles = crate::style_store_for_preset("default").unwrap();
        let images = hwpforge_core::image::ImageStore::new();
        let output = crate::HwpxEncoder::encode(&doc, &styles, &images).unwrap();
        let source = cache_source(&doc, &styles, &images).unwrap();
        let (bytes, reported) =
            carry_line_segs(&source.bytes, &output, &output, &output, &source).unwrap();
        let xml =
            RawPackage::read(&bytes).unwrap().read_text_entry("Contents/section0.xml").unwrap();
        assert!(!xml.contains("<hp:linesegarray"));
        assert!(
            matches!(reported.as_slice(), [crate::EncodeWarning::LayoutCacheDropped { reason, .. }] if reason.contains("CRLF"))
        );
    }

    /// The cache source must hold the output's tokens
    /// exactly — otherwise its coordinates count other tokens.
    #[test]
    fn splice_bails_when_the_source_differs_beyond_caches() {
        let output = format!("<hs:sec>{}{}</hs:sec>", p("하나", false), p("둘", false));
        let source = format!("<hs:sec>{}{}</hs:sec>", p("하나", true), p("둘셋", true));
        assert!(splice_section(&output, &output, &output, &source).is_none());
    }

    /// Cell cache invalidation: editing the second of three cell paragraphs
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
        let (spliced, _) = splice_section(&baseline, &output, &output, &source).unwrap();
        let slices = scan_paragraphs(&spliced).unwrap();
        // open order: before, host, c0, c1, c2, after.
        let cached: Vec<bool> = slices.iter().map(|s| s.line_seg.is_some()).collect();
        assert_eq!(cached, [true, false, true, false, false, false]);
    }
    // 이것을 실패시키는 것: ledger 실패 표지를 내보내거나 그 문단의 진단을 버리기.
    #[test]
    fn prepared_carry_drops_only_the_failed_paragraph_and_keeps_its_diagnostic() {
        use hwpforge_core::layout::{LayoutCache, LineBoundary, LineSeg, WireTokenKind};
        use hwpforge_core::{Document, PageSettings, Paragraph, Run, Section};
        use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
        let para = |bad: bool| {
            let mut p = Paragraph::with_runs(
                vec![Run::text("AB", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            );
            let mut line = LineSeg::default();
            line.vertsize = 1000;
            if bad {
                line.textpos = 1;
                line.boundary = Some(LineBoundary::new(vec![WireTokenKind::Table], None));
            }
            p.layout_cache = Some(LayoutCache::new(vec![line]));
            p
        };
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![para(false), para(true), para(false)],
            PageSettings::a4(),
        ));
        let doc = doc.validate().unwrap();
        let styles = crate::style_store_for_preset("default").unwrap();
        let images = hwpforge_core::image::ImageStore::new();
        let output = crate::HwpxEncoder::encode(&doc, &styles, &images).unwrap();
        let source = cache_source(&doc, &styles, &images).unwrap();
        let result = carry_prepared(&output, source).unwrap();
        assert_eq!(result.warnings.len(), 1);
        let crate::EncodeWarning::LayoutCacheDropped { path, reason } = &result.warnings[0] else {
            panic!("cache warning")
        };
        assert_eq!(path.to_string(), "section[0].para[1]");
        assert!(!reason.contains("intentionally"));
        let xml = RawPackage::read(&result.bytes)
            .unwrap()
            .read_text_entry("Contents/section0.xml")
            .unwrap();
        assert_eq!(
            scan_paragraphs(&xml).unwrap().iter().map(|p| p.line_seg.is_some()).collect::<Vec<_>>(),
            [true, false, true]
        );
        assert!(!xml.contains(&format!("textpos=\"{}\"", crate::encoder::DROPPED_CACHE_MARK)));
    }

    // 이것을 실패시키는 것: 출처와 출력이 캐시 외에도 다를 때 출처를 그대로 사용하기.
    #[test]
    fn prepared_carry_reports_a_source_token_mismatch() {
        use hwpforge_core::{Document, PageSettings, Paragraph, Run, Section};
        use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::text("original", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        ));
        let doc = doc.validate().unwrap();
        let styles = crate::style_store_for_preset("default").unwrap();
        let images = hwpforge_core::image::ImageStore::new();
        let output = crate::HwpxEncoder::encode(&doc, &styles, &images).unwrap();
        let mut source = cache_source(&doc, &styles, &images).unwrap();
        let mut pkg = RawPackage::read(&source.bytes).unwrap();
        let xml =
            pkg.read_text_entry("Contents/section0.xml").unwrap().replace("original", "different");
        pkg.replace_text_entry("Contents/section0.xml", xml);
        source.bytes = pkg.write().unwrap();
        let result = carry_prepared(&output, source).unwrap();
        assert!(result.warnings.iter().any(|w| w.message().contains("cannot be aligned")));
        assert_eq!(
            RawPackage::read(&result.bytes)
                .unwrap()
                .read_text_entry("Contents/section0.xml")
                .unwrap(),
            RawPackage::read(&output).unwrap().read_text_entry("Contents/section0.xml").unwrap()
        );
    }
    /// 이것을 실패시키는 것: probe 슬롯 수 검사 삭제, source 동일성 검사를 probe 기준으로 바꾸기.
    #[test]
    fn probe_changes_only_the_comparison_not_output_alignment() {
        let base = format!("<hs:sec>{}{}</hs:sec>", p("가", false), p("나", false));
        let output = format!("<hs:sec>{}{}</hs:sec>", p("힣힣힣", false), p("나", false));
        let source = format!("<hs:sec>{}{}</hs:sec>", p("힣힣힣", true), p("나", true));
        assert_eq!(splice_section(&base, &output, &base, &source).unwrap().0, source);
        let base_flow = format!(
            "<hs:sec><hp:subList>{}{}</hp:subList>{}</hs:sec>",
            p("가", false),
            p("나", false),
            p("다", false)
        );
        let probe_flow = base_flow.replace("가", "힣");
        let output_flow = format!(
            "<hs:sec>{}<hp:subList>{}{}</hp:subList></hs:sec>",
            p("힣", false),
            p("나", false),
            p("다", false)
        );
        let source_flow = format!(
            "<hs:sec>{}<hp:subList>{}{}</hp:subList></hs:sec>",
            p("힣", true),
            p("나", true),
            p("다", true)
        );
        let expected_flow = format!(
            "<hs:sec>{}<hp:subList>{}{}</hp:subList></hs:sec>",
            p("힣", false),
            p("나", true),
            p("다", true)
        );
        assert_eq!(
            splice_section(&base_flow, &output_flow, &probe_flow, &source_flow).unwrap().0,
            expected_flow
        );
        let short = format!("<hs:sec>{}</hs:sec>", p("가", false));
        assert!(splice_section(&base, &output, &short, &source).is_none());
        assert!(splice_section(&short, &output, &base, &source).is_none());
        assert!(splice_section(&base, &short, &base, &source).is_none());
        assert!(splice_section(&base, &output, &base, &short).is_none());
        assert!(splice_section(&base, &output, &base, &source.replace("힣힣힣", "가")).is_none());
    }

    fn cached_document(count: usize) -> hwpforge_core::Document<hwpforge_core::Draft> {
        use hwpforge_core::layout::{LayoutCache, LineSeg};
        use hwpforge_core::{Document, PageSettings, Paragraph, Run, Section};
        use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
        let mut doc = Document::new();
        for _ in 0..count {
            let mut p = Paragraph::with_runs(
                vec![Run::text("AB", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            );
            p.layout_cache = Some(LayoutCache::new(vec![LineSeg::default()]));
            doc.add_section(Section::with_paragraphs(vec![p], PageSettings::a4()));
        }
        doc
    }

    // 이것을 실패시키는 것: section_index 실패 때 unwrap, section_path 실패 때 unwrap.
    #[test]
    fn unresolved_original_sections_and_nonnumeric_output_parts_are_skipped() {
        let styles = crate::style_store_for_preset("default").unwrap();
        let images = hwpforge_core::ImageStore::new();
        let doc = cached_document(2).validate().unwrap();
        let source = cache_source(&doc, &styles, &images).unwrap();
        let output = crate::HwpxEncoder::encode(&doc, &styles, &images).unwrap();
        let original =
            crate::HwpxEncoder::encode(&cached_document(1).validate().unwrap(), &styles, &images)
                .unwrap();
        use std::io::{Cursor, Write};
        let package = RawPackage::read(&output).unwrap();
        let section = package.read_text_entry("Contents/section0.xml").unwrap();
        let mut zip = zip::ZipWriter::new_append(Cursor::new(output)).unwrap();
        zip.start_file("Contents/section-extra.xml", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(section.as_bytes()).unwrap();
        let output = zip.finish().unwrap().into_inner();
        let (bytes, warnings) =
            carry_line_segs(&original, &output, &output, &output, &source).unwrap();
        assert_eq!(bytes, output);
        assert!(warnings.is_empty());
    }

    // 이것을 실패시키는 것: 원본 대응이 없는 출력 구역에서 건너뛰지 않고 출력 파일명으로 원본 조회.
    // 원본 spine 은 캐시가 있는 `section1.xml` 하나뿐이라, 출력 section0 은 그 캐시를 받고
    // 대응이 없는 출력 section1 은 같은 이름의 원본 캐시를 받지 않아야 한다.
    #[test]
    fn an_output_section_without_an_original_counterpart_takes_no_same_named_cache() {
        let styles = crate::style_store_for_preset("default").unwrap();
        let images = hwpforge_core::ImageStore::new();
        let doc = cached_document(2).validate().unwrap();
        let output = crate::HwpxEncoder::encode(&doc, &styles, &images).unwrap();
        let source = cache_source(&doc, &styles, &images).unwrap();
        let mut original_doc = cached_document(2);
        original_doc.sections_mut()[0].paragraphs[0].layout_cache = None;
        let original_source =
            cache_source(&original_doc.validate().unwrap(), &styles, &images).unwrap();
        let mut original = RawPackage::read(&original_source.bytes).unwrap();
        let mut content = original.read_text_entry("Contents/content.hpf").unwrap();
        for tag in ["<opf:item id=\"section0\"", "<opf:itemref idref=\"section0\""] {
            let start = content.find(tag).unwrap();
            let end = start + content[start..].find("/>").unwrap() + 2;
            content.replace_range(start..end, "");
        }
        original.replace_text_entry("Contents/content.hpf", content);
        original.remove_entries_exact(&["Contents/section0.xml".to_string()]);
        let original = original.write().unwrap();
        assert_eq!(
            RawPackage::read(&original).unwrap().section_path(0).unwrap(),
            "Contents/section1.xml"
        );

        let (bytes, warnings) =
            carry_line_segs(&original, &output, &output, &output, &source).unwrap();
        assert!(warnings.is_empty());
        let package = RawPackage::read(&bytes).unwrap();
        let caches: Vec<usize> = (0..2)
            .map(|i| {
                crate::test_support::xml::Element::parse(
                    &package.read_text_entry(&format!("Contents/section{i}.xml")).unwrap(),
                )
                .descendants("hp:linesegarray")
                .len()
            })
            .collect();
        assert_eq!(caches, [1, 0]);
    }

    // 이것을 실패시키는 것: 선행 오류 검사에서 원본에도 출력 파일명을 사용.
    #[test]
    fn original_semantic_read_errors_follow_reading_order_before_cache_fallback() {
        use std::io::{Cursor, Write};
        let styles = crate::style_store_for_preset("default").unwrap();
        let images = hwpforge_core::ImageStore::new();
        let doc = cached_document(1).validate().unwrap();
        let output = crate::HwpxEncoder::encode(&doc, &styles, &images).unwrap();
        let source = cache_source(&doc, &styles, &images).unwrap();
        let original =
            crate::HwpxEncoder::encode(&cached_document(2).validate().unwrap(), &styles, &images)
                .unwrap();
        let mut package = RawPackage::read(&original).unwrap();
        let content = package.read_text_entry("Contents/content.hpf").unwrap();
        package.replace_text_entry(
            "Contents/content.hpf",
            content
                .replace("idref=\"section0\"", "idref=\"swap\"")
                .replace("idref=\"section1\"", "idref=\"section0\"")
                .replace("idref=\"swap\"", "idref=\"section1\""),
        );
        let mut zip = zip::ZipWriter::new_append(Cursor::new(package.write().unwrap())).unwrap();
        zip.start_file("Contents/section1.xml/", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(b"").unwrap();
        let original = zip.finish().unwrap().into_inner();
        let error = carry_line_segs(&original, &output, &output, &output, &source)
            .expect_err("opaque original section must be refused");
        assert_eq!(error.stage, CodecStage::Verification);
        assert_eq!(error.detail, "Invalid HWPX structure: opaque ZIP entry selected semantically: 'Contents/section1.xml'");
    }

    // 이것을 실패시키는 것: source.dropped를 원본 파일 번호 또는 문서 전체 marker 번호로 조회.
    #[test]
    fn permuted_sections_report_each_marker_reason_at_its_output_path() {
        use hwpforge_core::layout::LineSeg;
        let styles = crate::style_store_for_preset("default").unwrap();
        let images = hwpforge_core::ImageStore::new();
        let mut doc = cached_document(3);
        doc.sections_mut()[0].paragraphs[0].layout_cache.as_mut().unwrap().lines[0].textpos = 99;
        doc.sections_mut()[1].paragraphs[0]
            .layout_cache
            .as_mut()
            .unwrap()
            .lines
            .push(LineSeg::default());
        doc.sections_mut()[2].paragraphs[0].layout_cache = None;
        let doc = doc.validate().unwrap();
        let source = cache_source(&doc, &styles, &images).unwrap();
        assert_eq!(source.dropped.iter().map(Vec::len).collect::<Vec<_>>(), [1, 1, 0]);
        let output = crate::HwpxEncoder::encode(&doc, &styles, &images).unwrap();
        let mut original = RawPackage::read(&source.bytes).unwrap();
        let sections: Vec<_> = (0..3)
            .map(|i| original.read_text_entry(&format!("Contents/section{i}.xml")).unwrap())
            .collect();
        for (i, file) in [2, 0, 1].into_iter().enumerate() {
            original
                .replace_text_entry(&format!("Contents/section{file}.xml"), sections[i].clone());
        }
        let content = original.read_text_entry("Contents/content.hpf").unwrap();
        original.replace_text_entry(
            "Contents/content.hpf",
            content
                .replace("idref=\"section0\"", "idref=\"swap\"")
                .replace("idref=\"section1\"", "idref=\"section0\"")
                .replace("idref=\"section2\"", "idref=\"section1\"")
                .replace("idref=\"swap\"", "idref=\"section2\""),
        );
        let (bytes, warnings) =
            carry_line_segs(&original.write().unwrap(), &output, &output, &output, &source)
                .unwrap();
        let observed: Vec<_> = warnings
            .iter()
            .map(|warning| {
                let crate::EncodeWarning::LayoutCacheDropped { path, reason } = warning else {
                    panic!("cache warning")
                };
                (path.to_string(), reason.as_str())
            })
            .collect();
        assert_eq!(
            observed,
            [
                ("section[0].para[0]".into(), "core textpos 99: coordinate 99 outside [0, 2]"),
                ("section[1].para[0]".into(), "line starts do not increase: [0, 0]")
            ]
        );
        for i in 0..3 {
            let package = RawPackage::read(&bytes).unwrap();
            let tree = crate::test_support::xml::Element::parse(
                &package.read_text_entry(&format!("Contents/section{i}.xml")).unwrap(),
            );
            assert!(tree.descendants("hp:linesegarray").is_empty());
        }
    }

    // 이것을 실패시키는 것: carry_prepared의 파트 필터에서 masterpage 제외.
    #[test]
    fn prepared_carry_preserves_master_page_cache() {
        use hwpforge_core::MasterPage;
        use hwpforge_foundation::ApplyPageType;
        let mut doc = cached_document(1);
        let paragraph = doc.sections()[0].paragraphs[0].clone();
        doc.sections_mut()[0].master_pages =
            Some(vec![MasterPage::new(ApplyPageType::Both, vec![paragraph])]);
        let doc = doc.validate().unwrap();
        let styles = crate::style_store_for_preset("default").unwrap();
        let images = hwpforge_core::ImageStore::new();
        let output = crate::HwpxEncoder::encode(&doc, &styles, &images).unwrap();
        let source = cache_source(&doc, &styles, &images).unwrap();
        let result = carry_prepared(&output, source).unwrap();
        assert!(result.warnings.is_empty());
        let package = RawPackage::read(&result.bytes).unwrap();
        let tree = crate::test_support::xml::Element::parse(
            &package.read_text_entry("Contents/masterpage0.xml").unwrap(),
        );
        assert_eq!(tree.descendants("hp:linesegarray").len(), 1);
        assert_eq!(tree.find("hp:lineseg").attr("textpos"), Some("0"));
    }
}
