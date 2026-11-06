//! Conservative single-line evidence shared by preserving text and cell edits.
use crate::encoder::package::HEADER_PART;
use crate::patch::RawPackage;
use crate::style_store::{HwpxCharShape, HwpxStyleStore};
use hwpforge_core::{layout::LayoutCache, Paragraph};
use quick_xml::{events::Event, Reader};
use std::collections::HashMap;
use std::rc::Rc;

// Inherited scopes borrow this map; retained scopes share prefix and URI storage.
pub(crate) type NamespaceBindings = HashMap<Rc<str>, Rc<str>>;

// Only attributes actually present on the source count as evidence.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Tag {
    pub(crate) name: String,
    pub(crate) attrs: HashMap<String, String>,
    pub(crate) children: Vec<Tag>,
    bound_namespace: bool,
    declares_namespace: bool,
}
impl Tag {
    pub(crate) fn read(xml: &str) -> Option<Self> {
        Self::read_in(xml, &NamespaceBindings::new())
    }
    pub(crate) fn read_in(xml: &str, namespaces: &NamespaceBindings) -> Option<Self> {
        let mut reader = Reader::from_str(xml);
        // Only declarations in this fragment enter the stack; inherited bindings stay borrowed.
        let mut bindings = Vec::new();
        let mut stack = vec![(Self::default(), bindings.len())];
        loop {
            let event = reader.read_event().ok()?;
            let empty = matches!(event, Event::Empty(_));
            match event {
                Event::Start(e) | Event::Empty(e) => {
                    let scope = bindings.len();
                    let tag = Self::start(&e, namespaces, &mut bindings)?;
                    if empty {
                        stack.last_mut()?.0.children.push(tag);
                        bindings.truncate(scope);
                    } else {
                        if stack.len() > crate::decoder::XML_RECURSION_LIMIT {
                            return None;
                        }
                        stack.push((tag, scope));
                    }
                }
                Event::End(_) => {
                    let (tag, scope) = stack.pop()?;
                    bindings.truncate(scope);
                    stack.last_mut()?.0.children.push(tag);
                }
                Event::Eof => break,
                _ => {}
            }
        }
        if stack.len() != 1 {
            return None;
        }
        let (mut root, _) = stack.pop()?;
        if root.children.len() != 1 {
            return None;
        }
        root.children.pop()
    }
    fn start(
        e: &quick_xml::events::BytesStart<'_>,
        namespaces: &NamespaceBindings,
        bindings: &mut Vec<(Rc<str>, Rc<str>)>,
    ) -> Option<Self> {
        #[cfg(test)]
        START_TAG_READS.with(|n| n.set(n.get() + 1));
        let mut tag = Self::default();
        for attr in e.attributes() {
            let attr = attr.ok()?;
            let key = attr.key.as_ref().to_string();
            let value =
                attr.normalized_value(quick_xml::XmlVersion::Explicit1_0).ok()?.into_owned();
            let prefix = if key == "xmlns" { Some("") } else { key.strip_prefix("xmlns:") };
            if let Some(prefix) = prefix {
                tag.declares_namespace = true;
                bindings.push((prefix.into(), value.into()));
            } else {
                tag.attrs.insert(key, value);
            }
        }
        let qualified = e.name().as_ref().to_string();
        let (prefix, local) = qualified.split_once(':').unwrap_or(("", &qualified));
        let uri = bindings
            .iter()
            .rev()
            .find(|(p, _)| p.as_ref() == prefix)
            .map(|(_, uri)| uri.as_ref())
            .or_else(|| namespaces.get(prefix).map(AsRef::as_ref))
            .unwrap_or_default();
        tag.bound_namespace = uri == namespace(local);
        tag.name = if tag.bound_namespace || (prefix.is_empty() && uri.is_empty()) {
            local.into()
        } else {
            // Keep the local name for ambiguity checks, never the unbounded URI.
            format!("{{foreign}}{local}")
        };
        Some(tag)
    }
    pub(crate) fn opening(xml: &str, namespaces: &NamespaceBindings) -> Option<Self> {
        Self::opening_in(xml, namespaces, &mut Vec::new())
    }
    // Only section/paragraph adapters need bindings beyond an opening tag.
    pub(crate) fn opening_with_bindings(
        xml: &str,
        namespaces: &NamespaceBindings,
    ) -> Option<(Self, NamespaceBindings)> {
        let mut declarations = Vec::new();
        let tag = Self::opening_in(xml, namespaces, &mut declarations)?;
        let mut bindings = namespaces.clone();
        bindings.extend(declarations);
        Some((tag, bindings))
    }
    fn opening_in(
        xml: &str,
        namespaces: &NamespaceBindings,
        declarations: &mut Vec<(Rc<str>, Rc<str>)>,
    ) -> Option<Self> {
        let mut reader = Reader::from_str(xml);
        loop {
            match reader.read_event().ok()? {
                Event::Start(e) | Event::Empty(e) => {
                    return Self::start(&e, namespaces, declarations);
                }
                Event::Decl(_) | Event::Comment(_) | Event::Text(_) => {}
                _ => return None,
            }
        }
    }
    pub(crate) fn child(&self, name: &str) -> Option<&Self> {
        let mut children = self.children.iter().filter(|c| c.name.rsplit('}').next() == Some(name));
        let child = children.next()?;
        if children.next().is_some() {
            return None;
        }
        (child.name == name).then_some(child)
    }
    pub(crate) fn into_child(self, name: &str) -> Option<Self> {
        let mut children =
            self.children.into_iter().filter(|c| c.name.rsplit('}').next() == Some(name));
        let child = children.next()?;
        if children.next().is_some() {
            return None;
        }
        (child.name == name).then_some(child)
    }
    fn definitions(&self, name: &str) -> Option<Vec<&Self>> {
        if name == "font" {
            crate::work::tick_n(self.children.len());
        }
        self.children
            .iter()
            .filter(|c| c.name.rsplit('}').next() == Some(name))
            .map(|c| (c.name == name).then_some(c))
            .collect()
    }
    fn descendant_binding(&self) -> bool {
        self.children.iter().any(|c| c.declares_namespace || c.descendant_binding())
    }
    pub(crate) fn is(&self, key: &str, value: &str) -> bool {
        self.attrs.get(key).map(String::as_str) == Some(value)
    }
}

fn namespace(name: &str) -> &'static str {
    match name {
        "sec" => "http://www.hancom.co.kr/hwpml/2011/section",
        "p" | "run" | "secPr" | "grid" | "subList" | "tc" | "tr" | "tbl" | "switch" | "case"
        | "default" => "http://www.hancom.co.kr/hwpml/2011/paragraph",
        "intent" | "left" | "right" | "prev" | "next" => "http://www.hancom.co.kr/hwpml/2011/core",
        _ => "http://www.hancom.co.kr/hwpml/2011/head",
    }
}

/// Explicit source references, collected identically by splice and re-encode consumers.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct References {
    paragraph: usize,
    style: usize,
    characters: Vec<usize>,
}

/// Only opening tags are read. The caller supplies decoder-owned paragraph/run ranges.
pub(crate) fn references<'a>(
    paragraph: &str,
    runs: impl IntoIterator<Item = &'a str>,
    namespaces: &NamespaceBindings,
) -> Option<References> {
    #[cfg(test)]
    EVIDENCE_READS.with(|n| n.set(n.get() + 1));
    let (p, bindings) = Tag::opening_with_bindings(paragraph, namespaces)?;
    if p.name != "p" || !p.bound_namespace {
        return None;
    }
    let characters = runs
        .into_iter()
        .map(|xml| {
            let run = Tag::opening(xml, &bindings)?;
            (run.name == "run" && run.bound_namespace)
                .then(|| run.attrs.get("charPrIDRef")?.parse().ok())
                .flatten()
        })
        .collect::<Option<Vec<_>>>()?;
    if characters.is_empty() {
        return None;
    }
    Some(References {
        paragraph: p.attrs.get("paraPrIDRef")?.parse().ok()?,
        style: p.attrs.get("styleIDRef")?.parse().ok()?,
        characters,
    })
}

#[cfg(test)]
thread_local! {
    pub(crate) static START_TAG_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(crate) static EVIDENCE_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
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
    pub(crate) foreign_grid: bool,
    pub(crate) char_grid: Option<String>,
    pub(crate) wonggoji: Option<String>,
    pub(crate) line_grid: Option<String>,
    pub(crate) namespaces: NamespaceBindings,
}
impl SectionEvidence {
    pub(crate) fn record_grid(&mut self, grid: &Tag) {
        self.grids += 1;
        self.foreign_grid |= grid.name != "grid";
        self.char_grid = grid.attrs.get("charGrid").cloned();
        self.line_grid = grid.attrs.get("lineGrid").cloned();
        self.wonggoji = grid.attrs.get("wonggojiFormat").cloned();
    }
    // Cell edits parse each part once; text edits materialize only recorded property tags.
    pub(crate) fn read(xml: &str) -> Option<Self> {
        let section = Tag::read(xml)?;
        if section.name != "sec" || section.descendant_binding() {
            return None;
        }
        let (_, namespaces) = Tag::opening_with_bindings(xml, &HashMap::new())?;
        let mut evidence = Self { namespaces, ..Self::default() };
        // Collect by local name before checking namespaces, so foreign carriers
        // cannot hide a second section property behind one supported property.
        for p in section.children.iter().filter(|t| t.name.rsplit('}').next() == Some("p")) {
            for run in p.children.iter().filter(|t| t.name.rsplit('}').next() == Some("run")) {
                for property in
                    run.children.iter().filter(|t| t.name.rsplit('}').next() == Some("secPr"))
                {
                    if p.name != "p" || run.name != "run" || property.name != "secPr" {
                        return None;
                    }
                    evidence.properties += 1;
                    for grid in property
                        .children
                        .iter()
                        .filter(|t| t.name.rsplit('}').next() == Some("grid"))
                    {
                        evidence.record_grid(grid);
                    }
                }
            }
        }
        Some(evidence)
    }
}

/// The sole section/paragraph admission predicate for both edit paths.
pub(crate) fn source_allows(section: &SectionEvidence, paragraph_allowed: bool) -> bool {
    section.properties == 1
        && section.grids == 1
        && !section.foreign_grid
        && section.char_grid.as_deref() == Some("0")
        && section.wonggoji.as_deref() == Some("0")
        && section.line_grid.as_deref() == Some("0")
        && paragraph_allowed
}

fn paragraph_allows(definition: &Tag, shape: &crate::style_store::HwpxParaShape) -> Option<()> {
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
    // The encoder owns the mapping from stored values to wire values, including
    // margin units and legacy/default branches. Unknown source branches close the proof.
    let expected = crate::encoder::header::build_para_pr(0, shape);
    #[allow(clippy::disallowed_methods)]
    let xml = quick_xml::se::to_string(&expected).ok()?;
    let expected =
        Tag::read(&format!("<root{}>{xml}</root>", crate::encoder::package::XMLNS_DECLS))?
            .children
            .pop()?;
    for key in ["tabPrIDRef", "snapToGrid", "fontLineHeight", "condense"] {
        if definition.attrs.get(key) != expected.attrs.get(key) {
            return None;
        }
    }
    for name in ["align", "breakSetting", "heading"] {
        if !same_values(definition.child(name)?, expected.child(name)?) {
            return None;
        }
    }
    for name in ["border", "switch", "margin", "lineSpacing"] {
        if definition.definitions(name)?.len() > 1 {
            return None;
        }
    }
    match (definition.child("border"), expected.child("border")) {
        (None, None) => {}
        (Some(raw), Some(expected)) if same_values(raw, expected) => {}
        _ => return None,
    }
    let switch = expected.child("switch")?;
    if let Some(raw) = definition.child("switch") {
        if definition.child("margin").is_some()
            || definition.child("lineSpacing").is_some()
            || !same_values(raw, switch)
        {
            return None;
        }
    } else {
        let fallback = switch.child("default")?;
        for name in ["margin", "lineSpacing"] {
            if !same_values(definition.child(name)?, fallback.child(name)?) {
                return None;
            }
        }
    }
    Some(())
}

fn same_values(raw: &Tag, expected: &Tag) -> bool {
    raw.name == expected.name
        && raw.attrs == expected.attrs
        && raw.children.len() == expected.children.len()
        && raw.children.iter().zip(&expected.children).all(|(a, b)| same_values(a, b))
}

fn character_allows(definition: &Tag, shape: &HwpxCharShape) -> Option<()> {
    if definition.children.iter().any(|c| c.name.starts_with('{')) {
        return None;
    }
    let height: i32 = definition.attrs.get("height")?.parse().ok()?;
    if height <= 0 || height != shape.height.as_i32() || shape.char_offset != 0 {
        return None;
    }
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
    fonts: std::cell::OnceCell<Vec<bool>>,
    root: Tag,
}
impl Header {
    pub(crate) fn read(
        package: &RawPackage,
        styles: &HwpxStyleStore,
        mut definition: impl FnMut(),
    ) -> Option<Self> {
        crate::paragraph_measure::validate_measure_output(styles).ok()?;
        let head = Tag::read(&package.read_text_entry(HEADER_PART).ok()?)?;
        if head.name != "head" {
            return None;
        }
        let mut root = head.into_child("refList")?;
        let paragraphs = root
            .child("paraProperties")?
            .definitions("paraPr")?
            .into_iter()
            .enumerate()
            .map(|(i, p)| {
                definition();
                styles
                    .para_shape(hwpforge_foundation::ParaShapeIndex::new(i))
                    .ok()
                    .is_some_and(|shape| paragraph_allows(p, shape).is_some())
            })
            .collect();
        let characters = root
            .child("charProperties")?
            .definitions("charPr")?
            .into_iter()
            .enumerate()
            .map(|(i, c)| {
                let shape = styles.char_shape(hwpforge_foundation::CharShapeIndex::new(i)).ok();
                shape.is_some_and(|shape| character_allows(c, shape).is_some())
            })
            .collect();
        // Keep only the source definitions needed by the deferred font check.
        root.children
            .retain(|t| matches!(t.name.rsplit('}').next(), Some("fontfaces" | "charProperties")));
        Some(Self { paragraphs, characters, fonts: std::cell::OnceCell::new(), root })
    }
    fn fonts(&self, styles: &HwpxStyleStore) -> &[bool] {
        self.fonts.get_or_init(|| {
            let Some(fonts) = font_evidence(&self.root, styles) else {
                return Vec::new();
            };
            self.root
                .child("charProperties")
                .and_then(|c| c.definitions("charPr"))
                .unwrap_or_default()
                .into_iter()
                .enumerate()
                .map(|(i, c)| {
                    styles
                        .char_shape(hwpforge_foundation::CharShapeIndex::new(i))
                        .ok()
                        .is_some_and(|shape| fonts_allow(c, shape, &fonts).is_some())
                })
                .collect()
        })
    }

    pub(crate) fn proves(
        &self,
        section: &SectionEvidence,
        source: &References,
        paragraph: &Paragraph,
        store: &HwpxStyleStore,
        reencoded: bool,
    ) -> bool {
        let p = paragraph.para_shape_id.get();
        let Some(first) = paragraph.runs.first() else {
            return false;
        };
        let c = first.char_shape_id.get();
        // The style position check only confirms the decode kept the source
        // reference; layout uses the direct paragraph and run references.
        source.paragraph == p
            && paragraph.style_ref.and_then(|r| store.style_position(r).ok()) == Some(source.style)
            && source.characters.iter().all(|&id| id == c)
            && self.allows(section, p, c)
            && (!reencoded || self.fonts(store).get(c) == Some(&true))
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

const LANGUAGES: [&str; 7] = ["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"];

// Resolve each source/store language list once, then compare font metadata once
// per position. Character definitions only perform seven indexed lookups.
fn font_evidence(root: &Tag, store: &HwpxStyleStore) -> Option<[Vec<bool>; 7]> {
    let groups = root.child("fontfaces")?.definitions("fontface")?;
    let mut stored: [Vec<_>; 7] = std::array::from_fn(|_| Vec::new());
    for font in store.iter_fonts() {
        crate::work::tick();
        if let Some(language) = LANGUAGES.iter().position(|&lang| lang == font.lang) {
            stored[language].push(font);
        }
    }
    let mut evidence = std::array::from_fn(|_| Vec::new());
    for (language, lang) in LANGUAGES.iter().enumerate() {
        let mut matching = groups.iter().filter(|g| g.is("lang", lang));
        let group = matching.next()?;
        if matching.next().is_some() {
            return None;
        }
        let raw = group.definitions("font")?;
        evidence[language] = raw
            .into_iter()
            .zip(&stored[language])
            .map(|(font, stored)| font_matches(font, stored).is_some())
            .collect();
    }
    Some(evidence)
}

fn fonts_allow(character: &Tag, shape: &HwpxCharShape, fonts: &[Vec<bool>; 7]) -> Option<()> {
    let refs = character.child("fontRef")?;
    let ids = shape.font_ref;
    for ((language, index), fonts) in [
        ("hangul", ids.hangul),
        ("latin", ids.latin),
        ("hanja", ids.hanja),
        ("japanese", ids.japanese),
        ("other", ids.other),
        ("symbol", ids.symbol),
        ("user", ids.user),
    ]
    .into_iter()
    .zip(fonts)
    {
        crate::work::tick();
        let raw_index: usize = refs.attrs.get(language)?.parse().ok()?;
        if raw_index != index.get() {
            return None;
        }
        if fonts.get(raw_index) != Some(&true) {
            return None;
        }
    }
    Some(())
}

fn font_matches(font: &Tag, stored: &crate::style_store::HwpxFont) -> Option<()> {
    crate::work::tick();
    if font.definitions("substFont")?.len() > 1 {
        return None;
    }
    if !font.is("face", &stored.face_name)
        || !font.is("type", stored.font_type?.as_hwpx_str())
        || !font.is("isEmbedded", "0")
    {
        return None;
    }
    match (font.child("substFont"), &stored.substitute) {
        (None, None) => {}
        (Some(raw), Some(sub))
            if raw.is("face", &sub.face_name)
                && raw.is("type", sub.font_type?.as_hwpx_str())
                && raw.is("isEmbedded", "0")
                && raw.is("binaryItemIDRef", "") => {}
        _ => return None,
    }
    let infos = font.definitions("typeInfo")?;
    let (info, stored_info) = match (infos.as_slice(), stored.type_info.as_ref()) {
        ([], None) => return Some(()),
        ([info], Some(stored)) => (*info, stored),
        _ => return None,
    };
    if !info.is("familyType", &stored_info.family_type) {
        return None;
    }
    for (key, value) in [
        "weight",
        "proportion",
        "contrast",
        "strokeVariation",
        "armStyle",
        "letterform",
        "midline",
        "xHeight",
    ]
    .into_iter()
    .zip(stored_info.values)
    {
        if info.attrs.get(key)?.parse::<u32>().ok()? != value {
            return None;
        }
    }
    Some(())
}

#[cfg(test)]
#[path = "line_keep_tests.rs"]
mod tests;
