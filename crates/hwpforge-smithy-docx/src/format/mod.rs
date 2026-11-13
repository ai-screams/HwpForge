//! DOCX-local property deltas, definition resolution and canonical shape caches.
mod definitions;
mod keys;
mod line_height;
mod lists;
use keys::{FontKey, Fonts};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct StyleKey(usize);
mod properties;
mod resolve;
pub(crate) mod table;
mod units;

use crate::{
    error::{quote, Result},
    warning::{record, Kind},
    xml::{Element, W_NS},
    DocxError, DocxWarning,
};
pub(crate) use definitions::Definitions;
use properties::read_property;
pub(crate) use properties::PropertyState;
pub(crate) use properties::{boolean, container_attributes};
pub(crate) use resolve::Resolver;
use std::rc::Rc;
/// Structural lengths in HwpUnit after unit conversion, checked before rounding.
/// Widths/page sizes require 1..=100_000_000; row heights and margins allow zero.
#[derive(Clone, Copy)]
pub(crate) enum StructuralDimension {
    Width,
    PageSize,
    RowHeight,
    Margin,
}
impl StructuralDimension {
    pub(crate) fn minimum(self) -> i32 {
        match self {
            Self::Width | Self::PageSize => 1,
            Self::RowHeight | Self::Margin => 0,
        }
    }
}
pub(crate) fn structural_length(
    value: &str,
    dimension: StructuralDimension,
) -> std::result::Result<i32, &'static str> {
    units::length(value, 5, dimension.minimum())
}

pub(crate) fn default_tab_length(value: &str) -> std::result::Result<i32, &'static str> {
    units::length(value, 5, 1)
}

const MAX_DEFINITIONS: usize = 16_384;
const MAX_SHAPES: usize = 262_144;
const MAX_INHERITANCE: usize = 64;

pub(super) fn limit(
    part: &str,
    at: &str,
    resource: &'static str,
    observed: usize,
    maximum: usize,
) -> Result<()> {
    if observed > maximum {
        return Err(DocxError::Limit {
            part: quote(part),
            element: quote(at),
            resource,
            observed,
            limit: maximum,
        });
    }
    Ok(())
}
pub(crate) fn attr<'a>(e: &'a Element, key: &str) -> Option<&'a str> {
    e.attributes.iter().find(|(ns, k, _)| *ns == W_NS.0 && k == key).map(|(_, _, v)| v.as_str())
}
pub(super) fn warn(
    w: &mut Vec<DocxWarning>,
    part: &str,
    at: &str,
    kind: Kind,
    loss: &'static str,
    action: &'static str,
    reason: impl std::fmt::Display,
) -> Result<()> {
    record(w, kind, loss, action, || (Some(part), Some(at), reason))
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct CharDelta {
    // ascii, hAnsi, eastAsia, cs; retain independently until final projection.
    fonts: [Option<FontKey>; 4],
    size: Option<i32>,
    size_cs: Option<i32>,
    // b, i, strike, bCs, iCs. None is not the same operation as false.
    toggles: [Option<bool>; 5],
    underline: Option<bool>,
    color: Option<u32>,
    highlight: Option<Option<u32>>,
    vertical: Option<u8>,
}
impl CharDelta {
    // Fonts that decide line height: Latin (hAnsi, else ascii) and eastAsia.
    fn line_fonts(&self) -> [Option<FontKey>; 2] {
        [self.fonts[1].or(self.fonts[0]), self.fonts[2]]
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct ParaDelta {
    pub(crate) contextual: Option<bool>,
    // Projection cache only; raw line stays unchanged for inherited contexts.
    projected_line: Option<u64>,
    // First-line marker growth survives source spacing collapse and page starts.
    projected_marker_before: i32,
    alignment: Option<u8>,
    left: Option<i32>,
    right: Option<i32>,
    first: Option<i32>,
    pub(crate) before: Option<i32>,
    after: Option<i32>,
    // raw line and rule are inherited independently; line stays rational until projection.
    line: Option<i32>,
    line_rule: Option<u8>,
    keep_next: Option<bool>,
    keep_lines: Option<bool>,
    widow: Option<bool>,
    page_break: Option<bool>,
    num_id: Option<u32>,
    num_present: Option<bool>,
    level: Option<u32>,
    outline: Option<u32>,
    projected_list: Option<lists::ListRef>,
    projected_marker: Option<hwpforge_blueprint::ListMarkerLayout>,
    auto_space_de: Option<bool>,
    auto_space_dn: Option<bool>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct Delta {
    pub(crate) char: CharDelta,
    pub(crate) para: ParaDelta,
}

macro_rules! overwrite { ($a:expr, $b:expr, $($field:ident),+ $(,)?) => { $(if $b.$field.is_some() { $a.$field = $b.$field.clone(); })+ }; }
impl ParaDelta {
    fn overlay_indents(&mut self, other: &Self) {
        overwrite!(self, other, left, right, first);
    }
}
impl Delta {
    fn overlay(&mut self, other: &Self, toggle: bool) {
        for (a, b) in self.char.fonts.iter_mut().zip(&other.char.fonts) {
            if b.is_some() {
                *a = *b;
            }
        }
        for (a, b) in self.char.toggles.iter_mut().zip(other.char.toggles) {
            if toggle {
                if b == Some(true) {
                    *a = Some(!a.unwrap_or(false));
                } else if b.is_some() && a.is_none() {
                    // Preserve an explicitly mentioned script group even when false is a no-op.
                    *a = Some(false);
                }
            } else if b.is_some() {
                *a = b;
            }
        }
        overwrite!(self.char, other.char, size, size_cs, underline, color, highlight, vertical);
        overwrite!(
            self.para,
            other.para,
            contextual,
            auto_space_de,
            auto_space_dn,
            alignment,
            left,
            right,
            first,
            before,
            after,
            line,
            line_rule,
            keep_next,
            keep_lines,
            widow,
            page_break,
            num_id,
            num_present,
            level,
            outline
        );
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StyleKind {
    Paragraph,
    Character,
    Table,
    Numbering,
}
impl StyleKind {
    fn index(self) -> usize {
        self as usize
    }
}
#[derive(Clone, Debug)]
pub(super) struct Style {
    kind: StyleKind,
    parent: Option<StyleKey>,
    name: Option<String>,
    delta: Delta,
    table: table::TableProperties,
    part: Rc<str>,
    at: String,
    default: bool,
}
#[derive(Default)]
pub(super) struct Theme {
    fonts: [[Option<FontKey>; 3]; 2],
}
impl Theme {
    fn font(&self, name: &str) -> Option<FontKey> {
        let (family, slot) = match name {
            "majorAscii" | "majorHAnsi" => (0, 0),
            "majorEastAsia" => (0, 1),
            "majorBidi" => (0, 2),
            "minorAscii" | "minorHAnsi" => (1, 0),
            "minorEastAsia" => (1, 1),
            "minorBidi" => (1, 2),
            _ => return None,
        };
        self.fonts[family][slot]
    }
}

/// A line value can become out of range only after inheriting its rule (or vice versa).
/// Recover that line pair locally while preserving the other properties of this layer.
fn apply(
    base: &mut Delta,
    delta: &Delta,
    toggle: bool,
    part: &str,
    at: &str,
    warnings: &mut Vec<DocxWarning>,
) -> Result<()> {
    let previous = (base.para.line, base.para.line_rule);
    base.overlay(delta, toggle);
    if matches!(base.para.line_rule, Some(1 | 2)) && base.para.line.is_some_and(|v| v > 20_000_000)
    {
        (base.para.line, base.para.line_rule) = previous;
        warn(warnings,part,at,Kind::UnsupportedValue,"format_value","inherit_or_default","Inherited line rule and value exceed the length range; preceding line spacing retained")?;
    }
    Ok(())
}

#[cfg(test)]
mod structural_dimension_tests {
    use super::*;
    // 이것을 실패시키는 것: 폭/쪽 크기와 높이/여백의 최솟값 교환, 상한 또는 반올림 전 경계 검증 제거.
    #[test]
    fn structural_dimensions_have_explicit_inclusive_ranges() {
        for (dimension, minimum) in [
            (StructuralDimension::Width, 1),
            (StructuralDimension::PageSize, 1),
            (StructuralDimension::RowHeight, 0),
            (StructuralDimension::Margin, 0),
        ] {
            assert_eq!(structural_length("0", dimension).ok(), (minimum == 0).then_some(0));
            assert_eq!(structural_length("0.2", dimension).unwrap(), 1);
            assert_eq!(structural_length("20000000", dimension).unwrap(), 100_000_000);
            assert!(structural_length("20000000.01", dimension).is_err());
            assert!(structural_length("-0.01", dimension).is_err());
            if minimum == 1 {
                assert!(structural_length("0.19", dimension).is_err());
            }
        }
    }
}
