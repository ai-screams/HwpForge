//! Shared encoder output spellings and fixed control tags.
use hwpforge_core::{NumberingDef, ParaHead};
use hwpforge_foundation::{DropCapStyle, NumberFormatType};

/// One-based numbering-table position used by section outline references.
/// Definition ID labels are independent of this position.
pub(crate) const OUTLINE_NUMBERING_POSITION: u32 = 1;
// The outline definition is placed by putting it first in an empty table, so
// a different position would need an explicit placement rule.
const _: () = assert!(OUTLINE_NUMBERING_POSITION == 1);

pub(crate) const DEFAULT_DROP_CAP: DropCapStyle = DropCapStyle::None;

/// `textOffset` (percent of the font size) of a bullet: one cell of marker.
pub(crate) const BULLET_TEXT_OFFSET: i32 = 100;

/// `textOffset` (percent of the font size) that starts a list item's text two
/// font sizes past its marker, or `None` for a marker that does not fit or
/// whose width was never measured.
///
/// With `useInstWidth="0"` Hancom gives a marker a fixed width: the font size
/// times the cells of the level's format text plus `textOffset` percent, for
/// any number. A digit and a literal ASCII character are half a cell, a Hangul
/// syllable and a circled number are one. At the default 10pt this is 20pt,
/// the distance `hwpforge_blueprint::markdown` puts continuations at.
pub(crate) fn list_text_offset(head: &ParaHead) -> Option<i32> {
    let number = match head.num_format {
        NumberFormatType::Digit => 1,
        NumberFormatType::HangulSyllable
        | NumberFormatType::CircledDigit
        | NumberFormatType::CircledHangulSyllable => 2,
        _ => return None,
    };
    let own_level = match head.level {
        1..=9 => char::from_digit(head.level, 10)?,
        10 => ':',
        _ => return None,
    };
    let (mut half_cells, mut numbers) = (0, 0);
    let mut chars = head.text.chars();
    while let Some(c) = chars.next() {
        if c == '^' {
            if chars.next()? != own_level {
                return None;
            }
            half_cells += number;
            numbers += 1;
        } else if c.is_ascii() {
            half_cells += 1;
        } else {
            return None;
        }
    }
    (numbers == 1 && half_cells <= 4).then_some((4 - half_cells) * 50)
}
// Big-endian ASCII control IDs: %clk, %smr, %pat, %xrf.
pub(crate) const CLICK_HERE_FIELD_ID: u32 = u32::from_be_bytes(*b"%clk");
pub(crate) const SUMMARY_FIELD_ID: u32 = u32::from_be_bytes(*b"%smr");
pub(crate) const PATH_FIELD_ID: u32 = u32::from_be_bytes(*b"%pat");
pub(crate) const CROSSREF_FIELD_ID: u32 = u32::from_be_bytes(*b"%xrf");

pub(crate) fn placeholder_run_key(char_pr_id_ref: u32, marker: &str) -> String {
    format!(r#"<hp:run charPrIDRef="{char_pr_id_ref}"><hp:t>{marker}</hp:t></hp:run>"#)
}

pub(crate) fn series_name_xml(name: &str) -> String {
    format!("<c:tx><c:v>{}</c:v></c:tx>", super::escape_xml(name))
}

/// Integer identity spelling shared by DTO and manual XML writers.
pub(crate) const IDENTITY: [&str; 6] = ["1", "0", "0", "0", "1", "0"];

/// Wrap in f32 before rounding; the saturating cast keeps nonfinite input at zero.
pub(crate) fn rotation_angle(rotation: Option<f32>) -> i32 {
    rotation.unwrap_or(0.0).rem_euclid(360.0).round() as i32
}

/// Keep six decimal places, including signed zero, for matrix coefficients.
pub(crate) fn decimal_coefficient(value: f64) -> String {
    format!("{value:.6}")
}

/// Group chains use integer 0/1, including negative zero; other values keep six decimals.
pub(crate) fn group_coefficient(value: f64) -> String {
    if value == 0.0 {
        "0".to_string()
    } else if value == 1.0 {
        "1".to_string()
    } else {
        decimal_coefficient(value)
    }
}

/// Write six coefficients in HWPX row order under the common namespace.
pub(crate) fn matrix_xml(tag: &str, [e1, e2, e3, e4, e5, e6]: [&str; 6]) -> String {
    format!(r#"<hc:{tag} e1="{e1}" e2="{e2}" e3="{e3}" e4="{e4}" e5="{e5}" e6="{e6}"/>"#)
}

/// Write an integer translation with the remaining identity coefficients.
pub(crate) fn translation_matrix_xml(x: i32, y: i32) -> String {
    let (x, y) = (x.to_string(), y.to_string());
    let mut values = IDENTITY;
    values[2] = &x;
    values[5] = &y;
    matrix_xml("transMatrix", values)
}

/// Write rectangle corners in order under the common namespace.
pub(crate) fn rect_points_xml(width: i32, height: i32) -> String {
    format!(
        r#"<hc:pt0 x="0" y="0"/><hc:pt1 x="{width}" y="0"/><hc:pt2 x="{width}" y="{height}"/><hc:pt3 x="0" y="{height}"/>"#
    )
}

/// Attributes only: callers retain the brush selection and surrounding elements.
pub(crate) fn win_brush_attributes(
    face: &str,
    hatch: &str,
    style: Option<&str>,
    alpha: &str,
) -> String {
    let hatch_style = style.map(|style| format!(r#" hatchStyle="{style}""#)).unwrap_or_default();
    format!(r#"faceColor="{face}" hatchColor="{hatch}"{hatch_style} alpha="{alpha}""#)
}

/// The measured nominal format width, rounded half up in HwpUnit.
/// Unknown number formats deliberately use one em and require a diagnostic.
pub(crate) fn marker_base_width(
    number: Option<(&NumberingDef, &ParaHead)>,
    size: i32,
) -> crate::HwpxResult<(i32, bool)> {
    let (half_cells, approximate) = match number {
        None => (2, false),
        Some((definition, head)) => match nominal_half_cells(definition, head) {
            Some(cells) => (cells, false),
            None => (2, true),
        },
    };
    let width = i64::from(size)
        .checked_mul(half_cells)
        .and_then(|n| n.checked_add(1))
        .ok_or_else(|| crate::paragraph_measure::invalid("marker base width overflow"))?
        / 2;
    Ok((marker_unit(width)?, approximate))
}

fn nominal_half_cells(definition: &NumberingDef, head: &ParaHead) -> Option<i64> {
    // 이것을 실패시키는 것: 미측정 문자나 소비되지 않은 ^를 ASCII 폭으로 승인.
    let literal =
        |text: &str| text.bytes().all(|c| c.is_ascii() && c != b'^').then_some(text.len() as i64);
    let mut cells = 0;
    let mut end = 0;
    for (index, range) in super::list_numbering::tokens(&head.text) {
        cells += literal(&head.text[end..range.start])?;
        let level = index as u32 + 1;
        // 이것을 실패시키는 것: 현재 head보다 깊은 참조도 측정된 것으로 처리.
        if level > head.level {
            return None;
        }
        // 이것을 실패시키는 것: 벡터 위치로 형식을 조회하거나 없는 수준을 십진으로 합성.
        let reference = definition.levels.iter().find(|h| h.level == level)?;
        cells += match reference.num_format {
            NumberFormatType::Digit
            | NumberFormatType::LatinCapital
            | NumberFormatType::LatinSmall => 1,
            NumberFormatType::HangulSyllable
            | NumberFormatType::RomanCapital
            | NumberFormatType::RomanSmall
            | NumberFormatType::CircledDigit
            | NumberFormatType::CircledHangulSyllable => 2,
            _ => return None,
        };
        end = range.end;
    }
    // 한컴 명목 폭은 네 칸을 넘어도 선형이며 현재 번호 값과 무관하다.
    Some(cells + literal(&head.text[end..])?)
}
pub(crate) fn marker_unit(value: i64) -> crate::HwpxResult<i32> {
    let value = i32::try_from(value)
        .map_err(|_| crate::paragraph_measure::invalid("marker coordinate overflow"))?;
    hwpforge_foundation::HwpUnit::new(value)?;
    Ok(value)
}

/// Loss reason for a right-aligned fixed slot narrower than the nominal base width.
pub(crate) const NARROW_RIGHT_SLOT: &str =
    "Unmeasured narrow right-aligned marker slot is widened to the nominal base width";
/// Loss reason for a right or center marker whose text follows its measured width.
pub(crate) const MEASURED_WIDTH_ALIGNMENT: &str =
    "Right/center measured-width marker uses left alignment";

/// Converts format-neutral marker coordinates to the measured HWPX head contract.
/// Right slots use W = max(g, B) and margin A − W: a zero-width right-aligned
/// slot does not draw its marker. The right edge stays at A and text at A + g.
/// A gap with 0 < g < B is widened to B; how that slot renders is unmeasured.
///
/// The second value names the approximation, if any, for the caller's loss report.
pub(crate) fn marker_metrics(
    layout: hwpforge_blueprint::ListMarkerLayout,
    anchor: i32,
    continuation: i32,
    base: i32,
    char_ref: u32,
) -> crate::HwpxResult<(crate::style_store::fidelity::HeadMetrics, Option<&'static str>)> {
    use crate::style_store::fidelity::HeadMetrics;
    use hwpforge_blueprint::{ListMarkerAlign as A, ListTextStart as T};
    let mut metrics = HeadMetrics::generated(0);
    metrics.char_pr_id_ref = char_ref;
    metrics.text_offset = 0;
    metrics.auto_indent = 0;
    let anchor = i64::from(anchor);
    let approximation = match layout.text_start {
        T::FromAnchor(g) => {
            let g = i64::from(g.as_i32());
            marker_unit(anchor.checked_add(g).ok_or_else(|| {
                crate::paragraph_measure::invalid("marker text position overflow")
            })?)?;
            metrics.auto_indent = u32::from(i64::from(continuation) == anchor + g);
            let slot = if layout.align == A::Center {
                marker_unit(g.checked_mul(2).ok_or_else(|| {
                    crate::paragraph_measure::invalid("center marker slot overflow")
                })?)?
            } else if layout.align == A::Right {
                marker_unit(g.max(i64::from(base)))?
            } else {
                marker_unit(g)?
            };
            metrics.width_adjust =
                marker_unit(i64::from(slot).checked_sub(i64::from(base)).ok_or_else(|| {
                    crate::paragraph_measure::invalid("marker width adjustment overflow")
                })?)?;
            match layout.align {
                A::Left => {
                    metrics.align = "LEFT".into();
                    metrics.text_offset_type = "PERCENT".into();
                    None
                }
                A::Right | A::Center => {
                    metrics.align =
                        if layout.align == A::Right { "RIGHT" } else { "CENTER" }.into();
                    metrics.text_offset_type = "HWPUNIT".into();
                    metrics.text_offset =
                        if layout.align == A::Right { marker_unit(g)? } else { 0 };
                    (layout.align == A::Right && g > 0 && g < i64::from(base))
                        .then_some(NARROW_RIGHT_SLOT)
                }
                _ => return Err(crate::paragraph_measure::invalid("unsupported marker alignment")),
            }
        }
        T::AfterMarker(d) => {
            metrics.use_inst_width = 1;
            metrics.text_offset_type = "HWPUNIT".into();
            metrics.text_offset = d.as_i32();
            (layout.align != A::Left).then_some(MEASURED_WIDTH_ALIGNMENT)
        }
        _ => return Err(crate::paragraph_measure::invalid("unsupported marker text start")),
    };
    Ok((metrics, approximation))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(num_format: NumberFormatType, level: u32, text: &str) -> ParaHead {
        ParaHead { start: 1, level, num_format, text: text.into(), checkable: false }
    }

    // 이것을 실패시키는 것: 칸 수 계산 변경 · 형식별 숫자 칸 변경.
    #[test]
    fn list_text_offset_fills_two_font_sizes() {
        use NumberFormatType::{CircledDigit, CircledHangulSyllable, Digit, HangulSyllable};
        for (format, level, text, offset) in [
            (Digit, 1, "^1.", 100),
            (HangulSyllable, 2, "^2.", 50),
            (Digit, 3, "^3)", 100),
            (HangulSyllable, 4, "^4)", 50),
            (Digit, 5, "(^5)", 50),
            (HangulSyllable, 6, "(^6)", 0),
            (CircledDigit, 7, "^7", 100),
            (CircledHangulSyllable, 8, "^8", 100),
            (Digit, 10, "^:.", 100),
        ] {
            assert_eq!(list_text_offset(&head(format, level, text)), Some(offset), "{text}");
        }
    }

    // 이것을 실패시키는 것: 넘치는 표지를 음수로 쓰기 · 측정하지 않은 형식이나 글자를 계산하기 ·
    // 다른 수준의 번호를 이 수준의 형식으로 세기.
    #[test]
    fn list_text_offset_leaves_what_was_not_measured() {
        use NumberFormatType::{Digit, HangulSyllable, RomanCapital};
        for (format, text) in [
            (Digit, "((^1))"),
            (HangulSyllable, "(^1)."),
            (RomanCapital, "^1."),
            (Digit, "제^1"),
            (Digit, "^1.^2."),
            (Digit, "."),
            (Digit, "^"),
            (Digit, "^x."),
            (Digit, "^2."),
        ] {
            assert_eq!(list_text_offset(&head(format, 1, text)), None, "{text}");
        }
        assert_eq!(list_text_offset(&head(Digit, 0, "^0.")), None);
        assert_eq!(list_text_offset(&head(Digit, 11, "^:.")), None);
    }

    // 이것을 실패시키는 것: DEFAULT_DROP_CAP을 DoubleLine으로 변경.
    #[test]
    fn dropcap_spelling() {
        assert_eq!(DEFAULT_DROP_CAP.to_string(), "None");
    }

    // 이것을 실패시키는 것: 각 컨트롤 ID의 마지막 ASCII 바이트 변경.
    #[test]
    fn field_control_ids() {
        assert_eq!(CLICK_HERE_FIELD_ID, 627272811);
        assert_eq!(SUMMARY_FIELD_ID, 628321650);
        assert_eq!(PATH_FIELD_ID, 628121972);
        assert_eq!(CROSSREF_FIELD_ID, 628650598);
    }

    // 이것을 실패시키는 것: run 키의 hp:t를 hp:x로 변경.
    #[test]
    fn placeholder_key_format() {
        assert_eq!(
            placeholder_run_key(7, "MARKER"),
            r#"<hp:run charPrIDRef="7"><hp:t>MARKER</hp:t></hp:run>"#
        );
    }

    // 이것을 실패시키는 것: c:v를 c:f로 변경하거나 XML 이스케이프 제거.
    #[test]
    fn series_name_format() {
        assert_eq!(series_name_xml("A&B"), "<c:tx><c:v>A&amp;B</c:v></c:tx>");
    }
}

#[cfg(test)]
mod nominal_tests {
    use super::*;
    use hwpforge_core::NumberingDef;
    use NumberFormatType::*;

    fn definition(formats: &[NumberFormatType], text: &str) -> NumberingDef {
        let mut levels: Vec<_> = formats
            .iter()
            .enumerate()
            .map(|(i, &num_format)| ParaHead {
                level: i as u32 + 1,
                start: 1,
                num_format,
                text: format!("^{}.", i + 1),
                checkable: false,
            })
            .collect();
        levels.last_mut().unwrap().text = text.into();
        NumberingDef { id: 1, start: 1, levels }
    }
    fn width(def: &NumberingDef, level: u32, size: i32) -> (i32, bool) {
        let head = def.levels.iter().find(|h| h.level == level).unwrap();
        marker_base_width(Some((def, head)), size).unwrap()
    }
    // 이것을 실패시키는 것: 로마·라틴·한글·원문자 명목 칸 수를 바꾸거나 자기 형식만 사용.
    #[test]
    fn nominal_marker_widths_match_measured_formats() {
        for (formats, text, small, large) in [
            (vec![Digit], "^1.", 1000, 2000),
            (vec![HangulSyllable], "^1.", 1500, 3000),
            (vec![RomanCapital], "^1.", 1500, 3000),
            (vec![RomanSmall], "^1.", 1500, 3000),
            (vec![LatinCapital], "^1.", 1000, 2000),
            (vec![LatinSmall], "^1.", 1000, 2000),
            (vec![LatinSmall], "^1)", 1000, 2000),
            (vec![RomanSmall], "(^1)", 2000, 4000),
            (vec![RomanCapital, Digit], "^1.^2", 2000, 4000),
            (vec![LatinCapital, Digit], "^1.^2", 1500, 3000),
            (vec![Digit, Digit], "^1.^2", 1500, 3000),
            (vec![Digit, Digit], "^1.^2.", 2000, 4000),
            (vec![Digit, Digit, Digit], "^1.^2.^3.", 3000, 6000),
            (vec![CircledDigit], "^1", 1000, 2000),
            (vec![CircledHangulSyllable], "^1", 1000, 2000),
        ] {
            let def = definition(&formats, text);
            assert_eq!(
                width(&def, formats.len() as u32, 1000),
                (small, false),
                "{formats:?} {text}"
            );
            assert_eq!(
                width(&def, formats.len() as u32, 2000),
                (large, false),
                "{formats:?} {text}"
            );
        }
    }
    // 이것을 실패시키는 것: 시작값의 자릿수를 명목 폭에 곱함.
    #[test]
    fn nominal_width_does_not_depend_on_counter_value() {
        for format in [RomanCapital, LatinCapital, LatinSmall] {
            let mut def = definition(&[format], "^1.");
            let expected = match format {
                RomanCapital => 1500,
                _ => 1000,
            };
            for start in [1, 8, 23] {
                def.levels[0].start = start;
                assert_eq!(width(&def, 1, 1000), (expected, false));
            }
        }
    }
    // 이것을 실패시키는 것: head.level 대신 벡터 위치로 참조 형식을 조회.
    #[test]
    fn nominal_width_resolves_referenced_level_formats() {
        let mut def = definition(&[RomanCapital, Digit], "^1.^2");
        def.levels.reverse();
        assert_eq!(width(&def, 2, 1000), (2000, false));
        def.levels[0].text = "^1.".into();
        assert_eq!(width(&def, 2, 1000), (1500, false));
    }
    // 이것을 실패시키는 것: 명목 폭에도 반각 네 칸 상한 적용.
    #[test]
    fn nominal_width_remains_linear_beyond_four_cells() {
        let def = definition(&[Digit, Digit, Digit], "^1.^2.^3.");
        assert_eq!(width(&def, 3, 1000), (3000, false));
        assert_eq!(width(&def, 3, 2000), (6000, false));
    }
    // 이것을 실패시키는 것: 미측정 형식·비ASCII·깨진 참조·미래 참조를 측정값으로 승인.
    #[test]
    fn nominal_width_preserves_unmeasured_fallbacks() {
        for (format, text) in [
            (CircledLatinSmall, "^1."),
            (HanjaDigit, "^1."),
            (Digit, "제^1"),
            (Digit, "^x."),
            (Digit, "^2."),
        ] {
            assert_eq!(width(&definition(&[format], text), 1, 1100), (1100, true));
        }
        let mut def = definition(&[Digit, Digit], "^1.^2");
        def.levels.remove(0);
        assert_eq!(width(&def, 2, 1100), (1100, true));
        let mut def = definition(&[Digit, Digit], "^2.");
        def.levels[0].text = "^2.".into();
        assert_eq!(width(&def, 1, 1100), (1100, true));
    }
    // 이것을 실패시키는 것: ^10을 ^1과 0으로 읽거나 ^:를 같은 토큰으로 처리.
    #[test]
    fn nominal_width_uses_counter_token_boundaries() {
        let def = definition(&[Digit; 10], "^10.");
        assert_eq!(width(&def, 10, 1100), (1100, false));
        let def = definition(&[Digit; 10], "^:.");
        assert_eq!(width(&def, 10, 1100), (1100, true));
        assert_eq!(def.levels[9].text, "^:.");
    }
    // 이것을 실패시키는 것: 리터럴 ASCII 명목 폭을 무조건 1em로 대체.
    #[test]
    fn literal_markers_have_nominal_ascii_width() {
        assert_eq!(width(&definition(&[Digit], "(0)."), 1, 1100), (2200, false));
        assert_eq!(width(&definition(&[Digit], "0."), 1, 1100), (1100, false));
    }
}
