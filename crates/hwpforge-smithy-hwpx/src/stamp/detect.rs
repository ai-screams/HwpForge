//! Class-A inline marker detection (E6 Wave 1A).
//!
//! Pure text-level scanners: given one run/paragraph text, find placeholder
//! markers by **syntax alone**. The pattern list is closed by design — any
//! extension must come from caller-provided rules, never from semantic-word
//! heuristics (see `.docs/algorithms/e6-placeholder-detection.md`).

use std::ops::Range;

/// Built-in class-A marker patterns (closed list).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BuiltinPattern {
    /// Literal `□` / `☑` checkbox glyph.
    Checkbox,
    /// `(` + whitespace only (≥1, full-width space included) + `)`.
    ParenBlank,
    /// Date blank: `년 … 월 … 일` with only whitespace between, or
    /// `NNNN.␣␣.␣␣.` dotted form with only whitespace between dots.
    DateBlank,
    /// Standalone `@` (both neighbours whitespace or line boundary).
    EmailAt,
    /// Exact seal/sign token: `(인)`, `(서명)`, `(직인)`, `(인/서명)`,
    /// `(서명 또는 인)`.
    SealSign,
}

impl BuiltinPattern {
    /// Stable detector id used in manifests and rule references.
    pub fn id(&self) -> &'static str {
        match self {
            Self::Checkbox => "checkbox",
            Self::ParenBlank => "paren_blank",
            Self::DateBlank => "date_blank",
            Self::EmailAt => "email_at",
            Self::SealSign => "seal_sign",
        }
    }
}

/// One detected marker in a text payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkerHit {
    /// Which built-in pattern matched.
    pub pattern: BuiltinPattern,
    /// UTF-8 byte span of the marker within the scanned text.
    pub span: Range<usize>,
    /// The marker text, verbatim.
    pub marker: String,
}

/// Why a candidate is downgraded to guarded (never auto-applied).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum GuardReason {
    /// The surrounding paragraph is instruction/example prose
    /// (`※` prefix, `【작성방법】`, `(예시)`), not a fillable slot.
    InstructionContext,
}

/// Scans one text payload for class-A markers, ordered by span start.
///
/// Detection is syntax-only; hits never overlap (patterns are mutually
/// exclusive by construction). Filled forms (`( 50 )`, `2026. 5. 1.`,
/// `a@b.com`) must NOT match.
pub fn detect_markers(text: &str) -> Vec<MarkerHit> {
    let mut hits = Vec::new();
    scan_checkbox(text, &mut hits);
    scan_paren(text, &mut hits);
    scan_korean_date(text, &mut hits);
    scan_dotted_date(text, &mut hits);
    scan_standalone_at(text, &mut hits);
    hits.sort_by_key(|h| h.span.start);
    hits
}

/// Exact seal/sign tokens, longest first so `(서명 또는 인)` wins over `(서명)`.
const SEAL_TOKENS: [&str; 5] = ["(서명 또는 인)", "(인/서명)", "(서명)", "(직인)", "(인)"];

fn push(hits: &mut Vec<MarkerHit>, pattern: BuiltinPattern, text: &str, span: Range<usize>) {
    hits.push(MarkerHit { pattern, marker: text[span.clone()].to_string(), span });
}

fn scan_checkbox(text: &str, hits: &mut Vec<MarkerHit>) {
    for (i, ch) in text.char_indices() {
        if ch == '□' || ch == '☑' {
            push(hits, BuiltinPattern::Checkbox, text, i..i + ch.len_utf8());
        }
    }
}

/// Handles both [`BuiltinPattern::SealSign`] (exact token) and
/// [`BuiltinPattern::ParenBlank`] (whitespace-only interior, ≥1 char).
fn scan_paren(text: &str, hits: &mut Vec<MarkerHit>) {
    for (i, ch) in text.char_indices() {
        if ch != '(' {
            continue;
        }
        if let Some(tok) = SEAL_TOKENS.iter().find(|t| text[i..].starts_with(**t)) {
            push(hits, BuiltinPattern::SealSign, text, i..i + tok.len());
            continue;
        }
        // Whitespace-only interior up to the closing `)` on the same line.
        // The scan consumes ONLY whitespace before deciding, so total work
        // across all `(` is linear — review M1: the previous
        // `rest.find(')')` scanned to EOF per `(`, O(n²) on adversarial
        // `(((…` input (24s at 1MB, bench-verified).
        let rest = &text[i + 1..];
        let mut saw_ws = false;
        for (j, c) in rest.char_indices() {
            if c == ')' {
                if saw_ws {
                    push(hits, BuiltinPattern::ParenBlank, text, i..i + 1 + j + 1);
                }
                break;
            }
            if c == '\n' || !c.is_whitespace() {
                break; // non-whitespace interior → not a blank
            }
            saw_ws = true;
        }
    }
}

/// `년 … 월 … 일` with ≥1 whitespace (and nothing else) between the units —
/// a filled date (`2026년 5월 1일`) has digits between and must not match.
fn scan_korean_date(text: &str, hits: &mut Vec<MarkerHit>) {
    for (i, ch) in text.char_indices() {
        if ch != '년' {
            continue;
        }
        let mut rest = text[i + ch.len_utf8()..].char_indices();
        if expect_gap_then(&mut rest, '월').is_none() {
            continue;
        }
        if let Some(end) = expect_gap_then(&mut rest, '일') {
            let base = i + ch.len_utf8();
            push(hits, BuiltinPattern::DateBlank, text, i..base + end);
        }
    }
}

/// Consumes ≥1 whitespace chars then `want` from `iter`; returns the byte
/// offset just past `want` (relative to the iterator's origin) on success.
fn expect_gap_then(iter: &mut std::str::CharIndices<'_>, want: char) -> Option<usize> {
    let mut saw_ws = false;
    for (j, c) in iter.by_ref() {
        if c.is_whitespace() {
            saw_ws = true;
            continue;
        }
        if saw_ws && c == want {
            return Some(j + c.len_utf8());
        }
        return None;
    }
    None
}

/// `NNNN.␣␣.␣␣.` — four digits, then three dots separated by whitespace only.
fn scan_dotted_date(text: &str, hits: &mut Vec<MarkerHit>) {
    let bytes = text.as_bytes();
    for i in 0..bytes.len() {
        if !text.is_char_boundary(i) || i + 4 > bytes.len() {
            continue;
        }
        if !bytes[i..i + 4].iter().all(u8::is_ascii_digit) {
            continue;
        }
        // reject when the digit run is longer than 4
        if i > 0 && bytes[i - 1].is_ascii_digit() {
            continue;
        }
        if bytes.get(i + 4) != Some(&b'.') {
            continue;
        }
        let mut rest = text[i + 5..].char_indices();
        if let Some(mid) = expect_gap_then(&mut rest, '.') {
            let _ = mid;
            if let Some(end) = expect_gap_then(&mut rest, '.') {
                push(hits, BuiltinPattern::DateBlank, text, i..i + 5 + end);
            }
        }
    }
}

/// `@` whose neighbours are whitespace or the text boundary.
fn scan_standalone_at(text: &str, hits: &mut Vec<MarkerHit>) {
    for (i, ch) in text.char_indices() {
        if ch != '@' {
            continue;
        }
        let prev_ok = text[..i].chars().next_back().is_none_or(char::is_whitespace);
        let next_ok = text[i + 1..].chars().next().is_none_or(char::is_whitespace);
        if prev_ok && next_ok {
            push(hits, BuiltinPattern::EmailAt, text, i..i + 1);
        }
    }
}

/// Returns the guard classification for a whole paragraph's text, if any.
///
/// A guarded paragraph still reports its candidates, but they are never
/// auto-applied — the caller must approve each one explicitly by path,
/// original marker, and occurrence.
pub fn paragraph_guard(text: &str) -> Option<GuardReason> {
    if text.trim_start().starts_with('※') {
        return Some(GuardReason::InstructionContext);
    }
    if text.contains("【작성방법】") || text.contains("(예시)") {
        return Some(GuardReason::InstructionContext);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hits(text: &str) -> Vec<(BuiltinPattern, &str)> {
        detect_markers(text)
            .into_iter()
            .map(|h| {
                let m: &str = &text[h.span.clone()];
                assert_eq!(m, h.marker, "span must slice exactly the marker text");
                (h.pattern, m)
            })
            .collect()
    }

    // ── edge cases first ────────────────────────────────────────────

    #[test]
    fn empty_text_has_no_markers() {
        assert!(detect_markers("").is_empty());
    }

    #[test]
    fn plain_prose_has_no_markers() {
        assert!(detect_markers("신청 기관의 명칭을 기재하시오.").is_empty());
    }

    #[test]
    fn empty_paren_pair_is_not_a_blank() {
        // `()` has no whitespace inside — not a fill slot.
        assert!(detect_markers("()").is_empty());
    }

    #[test]
    fn adversarial_open_paren_flood_stays_linear() {
        // Review M1: the old scanner ran `rest.find(')')` per `(` — O(n²),
        // 24s on this input. The fixed scanner consumes only whitespace per
        // `(`, so total work is linear (~ms). The generous bound below only
        // trips on an algorithmic regression, not on slow CI.
        let flood = "(".repeat(1_000_000);
        let started = std::time::Instant::now();
        assert!(detect_markers(&flood).is_empty());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "paren scan must stay linear on `(((…` flood, took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn paren_with_content_is_not_a_blank() {
        assert!(detect_markers("(예시)").is_empty());
        assert!(detect_markers("( 50 )").is_empty());
        assert!(detect_markers("(작성)").is_empty());
    }

    #[test]
    fn filled_date_is_not_a_blank() {
        assert!(detect_markers("2026년 5월 1일").is_empty());
        assert!(detect_markers("2026. 5. 1.").is_empty());
    }

    #[test]
    fn email_address_at_is_not_standalone() {
        assert!(detect_markers("hanyul@example.com").is_empty());
    }

    // ── checkbox ────────────────────────────────────────────────────

    #[test]
    fn checkbox_glyphs_detected_with_exact_spans() {
        let got = hits("□ 예  □ 아니오");
        assert_eq!(got, vec![(BuiltinPattern::Checkbox, "□"), (BuiltinPattern::Checkbox, "□")]);
        let checked = hits("☑ 동의함");
        assert_eq!(checked, vec![(BuiltinPattern::Checkbox, "☑")]);
    }

    // ── paren blank ─────────────────────────────────────────────────

    #[test]
    fn whitespace_only_paren_blank_detected() {
        assert_eq!(hits("금액: (   )억원"), vec![(BuiltinPattern::ParenBlank, "(   )")]);
        // full-width space (U+3000) counts as blank
        assert_eq!(hits("(\u{3000})"), vec![(BuiltinPattern::ParenBlank, "(\u{3000})")]);
    }

    #[test]
    fn multiple_paren_blanks_in_one_text() {
        let got = hits("성명 (   ) 소속 (  )");
        assert_eq!(
            got,
            vec![(BuiltinPattern::ParenBlank, "(   )"), (BuiltinPattern::ParenBlank, "(  )")]
        );
    }

    // ── date blank ──────────────────────────────────────────────────

    #[test]
    fn korean_date_blank_detected() {
        assert_eq!(hits("년  월  일"), vec![(BuiltinPattern::DateBlank, "년  월  일")]);
        assert_eq!(hits("2026년  월  일 작성"), vec![(BuiltinPattern::DateBlank, "년  월  일")]);
    }

    #[test]
    fn dotted_date_blank_detected() {
        assert_eq!(hits("작성일: 2026.  .  ."), vec![(BuiltinPattern::DateBlank, "2026.  .  .")]);
    }

    // ── standalone @ ────────────────────────────────────────────────

    #[test]
    fn standalone_at_detected() {
        assert_eq!(hits("이메일:  @ "), vec![(BuiltinPattern::EmailAt, "@")]);
        // at start/end of text also counts as standalone
        assert_eq!(hits("@"), vec![(BuiltinPattern::EmailAt, "@")]);
    }

    // ── seal/sign tokens ────────────────────────────────────────────

    #[test]
    fn seal_tokens_detected_exactly() {
        assert_eq!(hits("신청인(대표)      (인)"), vec![(BuiltinPattern::SealSign, "(인)")]);
        assert_eq!(hits("(서명 또는 인)"), vec![(BuiltinPattern::SealSign, "(서명 또는 인)")]);
        assert_eq!(hits("(직인)"), vec![(BuiltinPattern::SealSign, "(직인)")]);
    }

    #[test]
    fn label_paren_is_not_a_seal() {
        // `(대표)` is a label, not in the closed seal token list
        assert!(detect_markers("(대표)").is_empty());
    }

    // ── mixed / ordering ────────────────────────────────────────────

    #[test]
    fn mixed_markers_ordered_by_span_start() {
        let text = "□ 동의 (   ) 신청인 (인)";
        let got = hits(text);
        assert_eq!(
            got,
            vec![
                (BuiltinPattern::Checkbox, "□"),
                (BuiltinPattern::ParenBlank, "(   )"),
                (BuiltinPattern::SealSign, "(인)"),
            ]
        );
    }

    // ── paragraph guard ─────────────────────────────────────────────

    #[test]
    fn instruction_prefix_guards_paragraph() {
        assert_eq!(
            paragraph_guard("※ 해당하는 항목의 □에 표시"),
            Some(GuardReason::InstructionContext)
        );
        // leading whitespace before ※ still guards
        assert_eq!(
            paragraph_guard("  ※ 바탕색이 어두운 난은 적지 않습니다"),
            Some(GuardReason::InstructionContext)
        );
    }

    #[test]
    fn instruction_markers_guard_paragraph() {
        assert_eq!(
            paragraph_guard("【작성방법】 각 항목을 기재"),
            Some(GuardReason::InstructionContext)
        );
        assert_eq!(
            paragraph_guard("(예시) 교육자료 개발비 : 10명 × 500,000원"),
            Some(GuardReason::InstructionContext)
        );
    }

    #[test]
    fn normal_paragraph_is_not_guarded() {
        assert_eq!(paragraph_guard("신청 기관: (   )"), None);
        // `※` mid-text (not a prefix) does not guard by itself
        assert_eq!(paragraph_guard("금액 (   ) ※단위: 억원"), None);
    }
}
