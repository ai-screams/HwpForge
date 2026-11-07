//! DOCX-local property deltas, definition resolution and canonical shape caches.
mod definitions;
mod keys;
use keys::{FontKey, Fonts};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct StyleKey(usize);
mod properties;
mod resolve;
mod units;

use crate::{
    error::{quote, Result},
    warning::{record, Kind},
    xml::{Element, W_NS},
    DocxError, DocxWarning,
};
pub(crate) use definitions::Definitions;
pub(crate) use properties::container_attributes;
use properties::read_property;
pub(crate) use properties::PropertyState;
pub(crate) use resolve::Resolver;
use std::rc::Rc;
pub(crate) fn structural_length(
    value: &str,
    minimum: i32,
) -> std::result::Result<i32, &'static str> {
    units::length(value, 5, minimum)
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
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct ParaDelta {
    alignment: Option<u8>,
    left: Option<i32>,
    right: Option<i32>,
    first: Option<i32>,
    before: Option<i32>,
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
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct Delta {
    pub(crate) char: CharDelta,
    pub(crate) para: ParaDelta,
}

macro_rules! overwrite { ($a:expr, $b:expr, $($field:ident),+ $(,)?) => { $(if $b.$field.is_some() { $a.$field = $b.$field.clone(); })+ }; }
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
