//! Text/run splitting helpers for HWP5 → Core projection.
//!
//! This submodule holds the leaf functions that split paragraph text into
//! Core `Run`s according to the decoded `char_shape_runs` table and resolve
//! the active character shape at a given visible position. They are pure,
//! position-oriented helpers with no document-traversal state; the parent
//! `projection` module calls them while assembling paragraphs.

use hwpforge_core::run::Run;
use hwpforge_foundation::{CharShapeIndex, HwpUnit};

use crate::decoder::section::Hwp5Paragraph;
use crate::schema::section::Hwp5CharShapeRun;

pub(super) fn char_shape_id_for_visible_position(runs: &[Hwp5CharShapeRun], position: u32) -> u32 {
    if position == 0 {
        return char_shape_id_at_position(runs, 0);
    }
    char_shape_id_at_position(runs, position.saturating_sub(1))
}

pub(super) fn char_shape_at(hwp_para: &Hwp5Paragraph, visible_utf16: u32) -> CharShapeIndex {
    CharShapeIndex::new(
        char_shape_id_for_visible_position(&hwp_para.char_shape_runs, visible_utf16) as usize,
    )
}

pub(super) fn char_shape_id_at_position(runs: &[Hwp5CharShapeRun], position: u32) -> u32 {
    // `runs` is sorted ascending by `position` (the same invariant the prior
    // linear `take_while(...).last()` relied on). `partition_point` finds the
    // index of the first run whose `position > position`, so `runs[..idx]` is
    // exactly the prefix the old `take_while` accepted — the last of which is
    // the active char shape. O(log R) instead of O(R).
    //
    // Equivalence with the old `take_while` holds only while this ascending
    // invariant holds; assert it in debug builds so a future decoder change
    // emitting unsorted runs is caught instead of silently returning a wrong
    // char shape.
    debug_assert!(
        runs.windows(2).all(|w| w[0].position <= w[1].position),
        "char_shape_runs must be ascending by position for partition_point lookup",
    );
    let idx = runs.partition_point(|run| run.position <= position);
    runs[..idx].last().map(|run| run.char_shape_id).unwrap_or(0)
}

pub(super) fn hwp_unit_from_u32(value: u32) -> HwpUnit {
    i32::try_from(value).ok().and_then(|signed| HwpUnit::new(signed).ok()).unwrap_or(HwpUnit::ZERO)
}

/// Split paragraph text into runs according to `char_shape_runs`.
///
/// Each run entry marks the starting character position (as a UTF-16
/// code-unit index) of a new character shape. For simplicity this
/// implementation treats the positions as Unicode scalar-value indices,
/// which is accurate for all-ASCII or all-Korean text.
pub(super) fn split_text_by_runs(text: &str, runs: &[Hwp5CharShapeRun]) -> Vec<Run> {
    if text.is_empty() && runs.is_empty() {
        return vec![];
    }
    if runs.is_empty() {
        return vec![Run::text(text, CharShapeIndex::new(0))];
    }

    let boundaries = utf16_boundaries(text);
    let mut result: Vec<Run> = Vec::with_capacity(runs.len());

    for (i, run) in runs.iter().enumerate() {
        let start = utf16_offset_to_byte(&boundaries, run.position);
        let end = if i + 1 < runs.len() {
            utf16_offset_to_byte(&boundaries, runs[i + 1].position)
        } else {
            text.len()
        };

        if start >= text.len() {
            break;
        }
        let end = end.min(text.len());
        let segment = &text[start..end];
        if !segment.is_empty() {
            result.push(Run::text(segment, CharShapeIndex::new(run.char_shape_id as usize)));
        }
    }

    if result.is_empty() {
        result.push(Run::text(text, CharShapeIndex::new(0)));
    }
    result
}

pub(super) fn utf16_boundaries(text: &str) -> Vec<(u32, usize)> {
    let mut boundaries = Vec::with_capacity(text.chars().count() + 1);
    let mut utf16_offset = 0u32;

    for (byte_idx, ch) in text.char_indices() {
        boundaries.push((utf16_offset, byte_idx));
        utf16_offset += ch.len_utf16() as u32;
    }
    boundaries.push((utf16_offset, text.len()));
    boundaries
}

pub(super) fn utf16_offset_to_byte(boundaries: &[(u32, usize)], utf16_offset: u32) -> usize {
    match boundaries.binary_search_by_key(&utf16_offset, |(offset, _)| *offset) {
        Ok(idx) => boundaries[idx].1,
        Err(idx) => boundaries
            .get(idx)
            .map(|(_, byte_idx)| *byte_idx)
            .unwrap_or_else(|| boundaries.last().map(|(_, byte_idx)| *byte_idx).unwrap_or(0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference (pre-optimization) linear implementation of
    /// `char_shape_id_at_position`, kept here so the table test below proves
    /// the `partition_point` rewrite is behaviorally identical.
    fn char_shape_id_at_position_linear(runs: &[Hwp5CharShapeRun], position: u32) -> u32 {
        runs.iter()
            .take_while(|run| run.position <= position)
            .last()
            .map(|run| run.char_shape_id)
            .unwrap_or(0)
    }

    fn char_run(position: u32, char_shape_id: u32) -> Hwp5CharShapeRun {
        Hwp5CharShapeRun { position, char_shape_id }
    }

    fn hwp5_char_run(position: u32, char_shape_id: u32) -> Hwp5CharShapeRun {
        Hwp5CharShapeRun { position, char_shape_id }
    }

    #[test]
    fn char_shape_id_at_position_matches_linear_reference() {
        // Sorted-ascending runs (the invariant the lookup relies on).
        let runs = [char_run(0, 10), char_run(5, 20), char_run(12, 30)];
        let empty: [Hwp5CharShapeRun; 0] = [];

        // Boundary table: before-first, exact starts, between, past-last, and
        // the empty-slice case. Each row asserts new == old reference.
        let cases = [0u32, 1, 4, 5, 6, 11, 12, 13, 1000];
        for &pos in &cases {
            assert_eq!(
                char_shape_id_at_position(&runs, pos),
                char_shape_id_at_position_linear(&runs, pos),
                "non-empty mismatch at position {pos}",
            );
        }

        // Empty slice → 0 in both implementations.
        assert_eq!(char_shape_id_at_position(&empty, 0), 0);
        assert_eq!(
            char_shape_id_at_position(&empty, 0),
            char_shape_id_at_position_linear(&empty, 0),
        );

        // Spot-check the actual expected values (not just equivalence).
        assert_eq!(char_shape_id_at_position(&runs, 0), 10, "exact first start");
        assert_eq!(char_shape_id_at_position(&runs, 4), 10, "before second start");
        assert_eq!(char_shape_id_at_position(&runs, 5), 20, "exact second start");
        assert_eq!(char_shape_id_at_position(&runs, 11), 20, "before third start");
        assert_eq!(char_shape_id_at_position(&runs, 12), 30, "exact third start");
        assert_eq!(char_shape_id_at_position(&runs, 1000), 30, "past last start");

        // A run that starts after 0 leaves positions before it unattributed → 0.
        let gapped = [char_run(3, 99)];
        assert_eq!(char_shape_id_at_position(&gapped, 0), 0, "position before first run");
        assert_eq!(
            char_shape_id_at_position(&gapped, 0),
            char_shape_id_at_position_linear(&gapped, 0),
        );
    }

    #[test]
    fn split_empty_text_empty_runs() {
        let result = split_text_by_runs("", &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn split_text_no_runs_returns_single_run() {
        let result = split_text_by_runs("Hello", &[]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content.as_text(), Some("Hello"));
        assert_eq!(result[0].char_shape_id, CharShapeIndex::new(0));
    }

    #[test]
    fn split_single_run_covers_all_text() {
        let runs = vec![hwp5_char_run(0, 7)];
        let result = split_text_by_runs("Hello", &runs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content.as_text(), Some("Hello"));
        assert_eq!(result[0].char_shape_id, CharShapeIndex::new(7));
    }

    #[test]
    fn split_two_runs() {
        // "HelloWorld" split at position 5
        let runs = vec![hwp5_char_run(0, 2), hwp5_char_run(5, 3)];
        let result = split_text_by_runs("HelloWorld", &runs);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].content.as_text(), Some("Hello"));
        assert_eq!(result[0].char_shape_id, CharShapeIndex::new(2));
        assert_eq!(result[1].content.as_text(), Some("World"));
        assert_eq!(result[1].char_shape_id, CharShapeIndex::new(3));
    }

    #[test]
    fn split_run_start_beyond_text_length_ignored() {
        // Run starting at position 100 in a 5-char string → ignored.
        let runs = vec![hwp5_char_run(0, 1), hwp5_char_run(100, 2)];
        let result = split_text_by_runs("Hello", &runs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content.as_text(), Some("Hello"));
        assert_eq!(result[0].char_shape_id, CharShapeIndex::new(1));
    }

    #[test]
    fn split_korean_text_by_runs() {
        // "안녕하세요" = 5 chars; split at char 2
        let runs = vec![hwp5_char_run(0, 10), hwp5_char_run(2, 11)];
        let result = split_text_by_runs("안녕하세요", &runs);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].content.as_text(), Some("안녕"));
        assert_eq!(result[1].content.as_text(), Some("하세요"));
    }

    #[test]
    fn split_text_by_utf16_code_units_handles_surrogate_pairs() {
        let runs = vec![hwp5_char_run(0, 1), hwp5_char_run(1, 2), hwp5_char_run(3, 3)];
        let result = split_text_by_runs("A😀B", &runs);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].content.as_text(), Some("A"));
        assert_eq!(result[1].content.as_text(), Some("😀"));
        assert_eq!(result[2].content.as_text(), Some("B"));
    }
}
