//! Conservative single-line evidence shared by preserving text and cell edits.
use crate::encoder::package::HEADER_PART;
use crate::patch::RawPackage;
use crate::style_store::{HwpxCharShape, HwpxStyleStore};
use hwpforge_core::{layout::LayoutCache, Paragraph};
use quick_xml::{events::Event, Reader};
use std::collections::HashMap;

// Only attributes actually present on the source count as evidence.
#[derive(Debug, Clone, Default)]
pub(crate) struct Tag {
    pub(crate) name: String,
    pub(crate) attrs: HashMap<String, String>,
    pub(crate) children: Vec<Tag>,
}
impl Tag {
    pub(crate) fn read(xml: &str) -> Option<Self> {
        let mut reader = Reader::from_str(xml);
        let mut stack = vec![Self::default()];
        loop {
            let event = reader.read_event().ok()?;
            let empty = matches!(event, Event::Empty(_));
            match event {
                Event::Start(e) | Event::Empty(e) => {
                    let mut tag =
                        Self { name: e.local_name().as_ref().to_string(), ..Self::default() };
                    for attr in e.attributes() {
                        let attr = attr.ok()?;
                        let value = attr
                            .normalized_value(quick_xml::XmlVersion::Explicit1_0)
                            .ok()?
                            .into_owned();
                        tag.attrs.insert(attr.key.as_ref().to_string(), value);
                    }
                    if empty {
                        stack.last_mut()?.children.push(tag);
                    } else {
                        if stack.len() > crate::decoder::XML_RECURSION_LIMIT {
                            return None;
                        }
                        stack.push(tag);
                    }
                }
                Event::End(_) => {
                    let tag = stack.pop()?;
                    stack.last_mut()?.children.push(tag);
                }
                Event::Eof => break,
                _ => {}
            }
        }
        if stack.len() != 1 {
            return None;
        }
        let mut root = stack.pop()?;
        if root.children.len() != 1 {
            return None;
        }
        root.children.pop()
    }
    pub(crate) fn child(&self, name: &str) -> Option<&Self> {
        let mut children = self.children.iter().filter(|c| c.name == name);
        let child = children.next()?;
        if children.next().is_some() {
            return None;
        }
        Some(child)
    }
    pub(crate) fn into_child(self, name: &str) -> Option<Self> {
        let mut children = self.children.into_iter().filter(|c| c.name == name);
        let child = children.next()?;
        if children.next().is_some() {
            return None;
        }
        Some(child)
    }
    pub(crate) fn is(&self, key: &str, value: &str) -> bool {
        self.attrs.get(key).map(String::as_str) == Some(value)
    }
}

/// A cell needs explicit horizontal text with ordinary line wrapping.
// 이것을 실패시키는 것: 누락된 방향 또는 줄바꿈 값을 기본 승인.
pub(crate) fn cell_flow_allows(direction: Option<&str>, wrap: Option<&str>) -> bool {
    direction == Some("HORIZONTAL") && wrap == Some("BREAK")
}

fn weight(text: &str) -> Option<u64> {
    if text.is_empty() {
        return None;
    }
    text.chars().try_fold(0u64, |sum, c| {
        // Conservative measured advances for the supported character set.
        let weight = match c {
            '\u{ac00}'..='\u{d7a3}' => 105,
            ' ' => 50,
            '0'..='9' | '.' | ',' | '-' | '(' | ')' | '/' | ':' => 100,
            _ => return None,
        };
        sum.checked_add(weight)
    })
}

pub(crate) fn width(text: &str, shape: &HwpxCharShape) -> Option<u64> {
    // Positive spacing at reduced relative size has no measured width bound.
    if shape.rel_sz < 100 && shape.spacing > 0 {
        return None;
    }
    let height = u64::try_from(shape.height.as_i32()).ok().filter(|v| *v > 0)?;
    let rel = u64::try_from(shape.rel_sz).ok().filter(|v| *v > 0)?;
    let ratio = u64::try_from(shape.ratio).ok().filter(|v| *v > 0)?;
    let spacing = shape.spacing.max(0) as u64;
    weight(text)?
        .checked_mul(height)?
        .checked_mul(rel)?
        .checked_mul(ratio.checked_add(spacing)?)?
        .checked_add(999_999)
        .map(|n| n / 1_000_000)
}

/// Checks only the promoted cache; never rebuilds one from source XML.
pub(crate) fn eligible<'a>(
    para: &'a Paragraph,
    old: &str,
    text: &str,
    styles: &HwpxStyleStore,
) -> Option<&'a LayoutCache> {
    if para.end_char_shape.is_some() {
        return None;
    }
    let first = para.runs.first()?.char_shape_id;
    if para.runs.iter().any(|r| r.char_shape_id != first) {
        return None;
    }
    let cache = para.layout_cache.as_ref()?;
    let [line] = cache.lines.as_slice() else {
        return None;
    };
    if line.textpos != 0 || line.boundary.is_some() {
        return None;
    }
    weight(old)?;
    let shape = styles.char_shape(first).ok()?;
    let available = u64::try_from(line.horzsize).ok()?;
    (width(text, shape)? <= available).then_some(cache)
}

/// Collected by each caller's existing section traversal.
#[derive(Debug, Clone, Default)]
pub(crate) struct SectionEvidence {
    pub(crate) properties: usize,
    pub(crate) grids: usize,
    pub(crate) char_grid: Option<String>,
    pub(crate) wonggoji: Option<String>,
}
impl SectionEvidence {
    // Cell edits parse each part once; text edits collect this during their existing source scan.
    pub(crate) fn read(xml: &str) -> Option<Self> {
        let section = Tag::read(xml)?;
        let mut evidence = Self::default();
        for property in section
            .children
            .iter()
            .filter(|p| p.name == "p")
            .flat_map(|p| &p.children)
            .filter(|r| r.name == "run")
            .flat_map(|r| &r.children)
            .filter(|t| t.name == "secPr")
        {
            evidence.properties += 1;
            for grid in property.children.iter().filter(|t| t.name == "grid") {
                evidence.grids += 1;
                evidence.char_grid = grid.attrs.get("charGrid").cloned();
                evidence.wonggoji = grid.attrs.get("wonggojiFormat").cloned();
            }
        }
        Some(evidence)
    }
}

/// The sole section/paragraph admission predicate for both edit paths.
pub(crate) fn source_allows(section: &SectionEvidence, paragraph_allowed: bool) -> bool {
    section.properties == 1
        && section.grids == 1
        && section.char_grid.as_deref() == Some("0")
        && section.wonggoji.as_deref() == Some("0")
        && paragraph_allowed
}

fn paragraph_allows(definition: &Tag) -> Option<()> {
    if !definition.is("fontLineHeight", "0") || !definition.is("condense", "0") {
        return None;
    }
    let auto = definition.child("autoSpacing")?;
    if !auto.is("eAsianEng", "0") || !auto.is("eAsianNum", "0") {
        return None;
    }
    if !definition.child("breakSetting")?.is("lineWrap", "BREAK")
        || !definition.child("heading")?.is("type", "NONE")
    {
        return None;
    }
    Some(())
}

fn character_allows(definition: &Tag, shape: &HwpxCharShape) -> Option<()> {
    if !definition.is("useFontSpace", "0")
        || !definition.is("useKerning", "0")
        || !definition.is("symMark", "NONE")
    {
        return None;
    }
    for effect in ["bold", "italic", "emboss", "engrave", "supscript", "subscript"] {
        if definition.children.iter().any(|c| c.name == effect) {
            return None;
        }
    }
    for (effect, key) in
        [("underline", "type"), ("strikeout", "shape"), ("outline", "type"), ("shadow", "type")]
    {
        let mut children = definition.children.iter().filter(|c| c.name == effect);
        if let Some(child) = children.next() {
            if !child.is(key, "NONE") || children.next().is_some() {
                return None;
            }
        }
    }
    for (name, expected) in [
        ("ratio", shape.ratio),
        ("spacing", shape.spacing),
        ("relSz", shape.rel_sz),
        ("offset", shape.char_offset),
    ] {
        let child = definition.child(name)?;
        let mut values = [0; 7];
        for (value, language) in values
            .iter_mut()
            .zip(["hangul", "latin", "hanja", "japanese", "other", "symbol", "user"])
        {
            *value = child.attrs.get(language)?.parse().ok()?;
        }
        if !crate::schema::header::uniform(values) || values[0] != expected {
            return None;
        }
    }
    Some(())
}

/// Header definitions are checked once, in reference order, not authored-id order.
pub(crate) struct Header {
    paragraphs: Vec<bool>,
    characters: Vec<bool>,
}
impl Header {
    pub(crate) fn read(
        package: &RawPackage,
        styles: &HwpxStyleStore,
        mut definition: impl FnMut(),
    ) -> Option<Self> {
        let root = Tag::read(&package.read_text_entry(HEADER_PART).ok()?)?.into_child("refList")?;
        let paragraphs = root
            .child("paraProperties")?
            .children
            .iter()
            .filter_map(|p| {
                definition();
                (p.name == "paraPr").then(|| paragraph_allows(p).is_some())
            })
            .collect();
        let characters = root
            .child("charProperties")?
            .children
            .iter()
            .filter(|c| c.name == "charPr")
            .enumerate()
            .map(|(i, c)| {
                let shape = styles.char_shape(hwpforge_foundation::CharShapeIndex::new(i)).ok();
                shape.is_some_and(|shape| character_allows(c, shape).is_some())
            })
            .collect();
        Some(Self { paragraphs, characters })
    }
    pub(crate) fn allows(
        &self,
        section: &SectionEvidence,
        paragraph: usize,
        character: usize,
    ) -> bool {
        source_allows(section, self.paragraphs.get(paragraph).copied().unwrap_or(false))
            && self.characters.get(character).copied().unwrap_or(false)
    }
}

#[cfg(test)]
#[path = "line_keep_tests.rs"]
mod tests;
