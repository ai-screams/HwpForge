//! Source list instances; lifetime is the document, independent of spacing boundaries.
use super::*;
use hwpforge_blueprint::registry::StyleRegistry;
use hwpforge_core::{BulletDef, NumberingDef, ParaHead, ParagraphListRef};
use hwpforge_foundation::{BulletIndex, NumberFormatType, NumberingIndex};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum ListRef {
    Outline(u8),
    Number(usize, u8),
    Bullet(usize, u8),
}
impl ListRef {
    pub(super) fn core(self) -> ParagraphListRef {
        match self {
            Self::Outline(level) => ParagraphListRef::Outline { level },
            Self::Number(id, level) => {
                ParagraphListRef::Number { numbering_id: NumberingIndex::new(id), level }
            }
            Self::Bullet(id, level) => {
                ParagraphListRef::Bullet { bullet_id: BulletIndex::new(id), level }
            }
        }
    }
    pub(super) fn from_core(value: Option<ParagraphListRef>) -> Option<Self> {
        value.map(|v| match v {
            ParagraphListRef::Outline { level } => Self::Outline(level),
            ParagraphListRef::Number { numbering_id, level } => {
                Self::Number(numbering_id.get(), level)
            }
            ParagraphListRef::Bullet { bullet_id, level } => Self::Bullet(bullet_id.get(), level),
            ParagraphListRef::CheckBullet { .. } => {
                unreachable!("import does not generate checkbox state")
            }
        })
    }
}
#[derive(Clone, Copy, Default)]
pub(super) enum Format {
    Number(NumberFormatType),
    Bullet,
    #[default]
    Unsupported,
}
impl Format {
    pub(super) fn parse(value: Option<&str>) -> Self {
        match value {
            Some("decimal") => Self::Number(NumberFormatType::Digit),
            Some("upperRoman") => Self::Number(NumberFormatType::RomanCapital),
            Some("lowerRoman") => Self::Number(NumberFormatType::RomanSmall),
            Some("upperLetter") => Self::Number(NumberFormatType::LatinCapital),
            Some("lowerLetter") => Self::Number(NumberFormatType::LatinSmall),
            Some("bullet") => Self::Bullet,
            _ => Self::Unsupported,
        }
    }
}
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Suffix {
    #[default]
    Tab,
    Space,
    Nothing,
}
#[derive(Default)]
pub(super) struct Level {
    pub(super) delta: Delta,
    pub(super) defined: bool,
    pub(super) start: Option<u32>,
    pub(super) start_override: Option<u32>,
    pub(super) format: Format,
    pub(super) text: Option<Rc<str>>,
    pub(super) marker_char: CharDelta,
    pub(super) align: Option<hwpforge_blueprint::ListMarkerAlign>,
    pub(super) suffix: Option<Suffix>,
    pub(super) tabs: std::collections::BTreeMap<i32, bool>,
    pub(super) restart: bool,
    pub(super) picture: bool,
    pub(super) compiled: std::cell::OnceCell<Rc<Compiled>>,
}
impl Level {
    /// Only the measured source font/glyph pair qualifies, using the same font-slot selection
    /// as glyph conversion. An ordinary Unicode bullet is not evidence for this ratio.
    pub(super) fn measured_symbol_bullet(&self, fonts: &Fonts) -> bool {
        matches!(self.format, Format::Bullet)
            && self.text.as_deref() == Some("\u{f0b7}")
            && bullet_glyph(self, fonts).0 == Some('•')
    }
    pub(super) fn mapped_bullet(&self) -> bool {
        self.compiled
            .get()
            .is_some_and(|c| matches!(c.marker, Marker::Bullet { approximated: true, .. }))
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum GroupKey {
    Abstract(u32),
    Instance(u32),
}
pub(super) struct Compiled {
    text: Rc<str>,
    marker: Marker,
}
struct ResolvedLevel {
    compiled: Rc<Compiled>,
    start: u32,
    format: NumberFormatType,
    restart: bool,
    current: Option<u64>,
}
#[derive(Clone, Copy)]
enum Marker {
    Number,
    Bullet { glyph: Option<char>, approximated: bool, slots: bool },
    Lost,
}
struct Group {
    levels: [Option<ResolvedLevel>; 10],
    lost: u16,
    seen: u16,
    numbering: Option<usize>,
    mixed: bool,
}
#[derive(Default)]
pub(super) struct Lists {
    groups: HashMap<GroupKey, Group>,
    bullets: HashMap<char, usize>,
    bullet_warnings: HashSet<(usize, &'static str)>,
    owned_marker_bytes: usize,
    #[cfg(test)]
    interpreted_bytes: usize,
}
impl Lists {
    pub(super) fn new(definitions: &Definitions) -> Result<Self> {
        let mut lists = Self::default();
        let mut interpreted_bytes = 0;
        let part = definitions.numbering_part.as_deref().unwrap_or("#numbering");
        // Cache success AND failure on the original level, before any instance
        // groups. Group construction only clones Rc handles and combines starts.
        for levels in
            definitions.abstracts.values().chain(definitions.numbers.values().map(|n| &n.levels))
        {
            for (&index, source) in levels {
                if source.defined && index <= u32::from(ParagraphListRef::MAX_LEVEL) {
                    compile(
                        source,
                        index as usize,
                        &definitions.fonts,
                        &mut interpreted_bytes,
                        part,
                    )?;
                }
            }
        }
        #[cfg(test)]
        {
            lists.interpreted_bytes = interpreted_bytes;
        }
        for (&id, number) in &definitions.numbers {
            let Some(abstract_id) =
                number.abstract_id.filter(|id| definitions.abstracts.contains_key(id))
            else {
                continue;
            };
            let key = group_key(id, abstract_id, number.has_override);
            if let std::collections::hash_map::Entry::Vacant(entry) = lists.groups.entry(key) {
                entry.insert(build_group(definitions, abstract_id, number));
            }
        }
        Ok(lists)
    }
    pub(super) fn resolve(
        &mut self,
        definitions: &Definitions,
        registry: &mut StyleRegistry,
        para: &ParaDelta,
        part: &str,
        warnings: &mut Vec<DocxWarning>,
    ) -> Result<Option<ListRef>> {
        let marker = self.resolve_marker(definitions, registry, para, part, warnings)?;
        // Every omitted source number can advance its own sequence and restart
        // already-seen descendants, regardless of why its marker was omitted.
        if matches!(marker, None | Some(ListRef::Outline(_))) {
            self.observe_omitted(definitions, para);
        }
        Ok(marker)
    }
    fn resolve_marker(
        &mut self,
        definitions: &Definitions,
        registry: &mut StyleRegistry,
        para: &ParaDelta,
        part: &str,
        warnings: &mut Vec<DocxWarning>,
    ) -> Result<Option<ListRef>> {
        if let Some(level) = para.outline.filter(|&l| l < 9) {
            return outline(level as u8, para.num_id.is_some_and(|id| id != 0), part, warnings);
        }
        let Some(num) = para.num_id else {
            if para.num_present == Some(true) {
                reference_lost(part, warnings)?;
            }
            return Ok(None);
        };
        if num == 0 {
            return Ok(None);
        }
        let level = definitions.effective_level(num, para.level.unwrap_or(0));
        if level > u32::from(ParagraphListRef::MAX_LEVEL) {
            warn(
                warnings,
                part,
                "numPr",
                Kind::UnsupportedValue,
                "list_level",
                "omit",
                "List level exceeds the shared level range",
            )?;
            return Ok(None);
        }
        let Some(number) = definitions.numbers.get(&num) else {
            reference_lost(part, warnings)?;
            return Ok(None);
        };
        let Some(abstract_id) =
            number.abstract_id.filter(|id| definitions.abstracts.contains_key(id))
        else {
            reference_lost(part, warnings)?;
            return Ok(None);
        };
        let key = group_key(num, abstract_id, number.has_override);
        let group = self.groups.get_mut(&key).unwrap();
        let Some(resolved) = &group.levels[level as usize] else {
            reference_lost(part, warnings)?;
            return Ok(None);
        };
        let bit = 1 << level;
        group.seen |= bit;
        if resolved.restart {
            group.lost |= bit;
        }
        if group.lost & bit != 0 {
            numbering_lost(part, warnings, "numbering_sequence")?;
            return Ok(None);
        }
        if group.mixed {
            numbering_lost(part, warnings, "numbering_format")?;
            return Ok(None);
        }
        match &resolved.compiled.marker {
            Marker::Lost => {
                numbering_lost(part, warnings, "numbering_format")?;
                Ok(None)
            }
            Marker::Bullet { glyph, approximated, slots } => {
                let id = glyph.map(|glyph| {
                    *self.bullets.entry(glyph).or_insert_with(|| {
                        let id = registry.bullets.len();
                        registry.bullets.push(BulletDef {
                            id: id as u32 + 1,
                            bullet_char: glyph.to_string(),
                            checked_char: None,
                            use_image: false,
                            para_head: empty_head(0),
                        });
                        id
                    })
                });
                if *slots
                    && id.is_none_or(|id| self.bullet_warnings.insert((id, "bullet_font_slots")))
                {
                    let reason = "Bullet ascii and hAnsi fonts differ; character-specific slot priority used";
                    warn(
                        warnings,
                        part,
                        "numPr",
                        Kind::ScriptFormatCollapsed,
                        "bullet_font_slots",
                        "select_slot",
                        reason,
                    )?;
                }
                let Some(id) = id else {
                    warn(
                        warnings,
                        part,
                        "numPr",
                        Kind::BulletGlyphLost,
                        "bullet_glyph",
                        "omit",
                        "Bullet glyph is outside the supported font and Unicode mapping",
                    )?;
                    return Ok(None);
                };
                if *approximated && self.bullet_warnings.insert((id, "bullet_glyph")) {
                    warn(
                        warnings,
                        part,
                        "numPr",
                        Kind::BulletGlyphApproximated,
                        "bullet_glyph",
                        "approximate",
                        "Font-specific bullet mapped to a fixed Unicode glyph using the paragraph's ordinary font",
                    )?;
                }

                Ok(Some(ListRef::Bullet(id, level as u8)))
            }
            Marker::Number => {
                // Only an emitted ancestor resets the output sequence below it.
                group.lost &= (bit << 1) - 1;
                let start = group.levels[0].as_ref().or(Some(resolved)).map_or(1, |l| l.start);
                let id = if let Some(id) = group.numbering {
                    id
                } else {
                    let id = registry.numberings.len();
                    limit(part, "numPr", "output numbering definitions", id + 1, MAX_DEFINITIONS)?;
                    let additional = group
                        .levels
                        .iter()
                        .flatten()
                        .filter(|l| matches!(l.compiled.marker, Marker::Number))
                        .map(|l| l.compiled.text.len())
                        .sum::<usize>();
                    let observed = self.owned_marker_bytes.saturating_add(additional);
                    // Reserve the entire group before allocating any owned marker.
                    limit(part, "numPr", "materialized marker bytes", observed, 64 * 1024 * 1024)?;
                    self.owned_marker_bytes = observed;

                    let levels = group
                        .levels
                        .iter()
                        .enumerate()
                        .map(|(i, l)| {
                            l.as_ref()
                                .filter(|l| matches!(l.compiled.marker, Marker::Number))
                                .map(|l| ParaHead {
                                    start: l.start,
                                    num_format: l.format,
                                    text: l.compiled.text.to_string(),
                                    ..empty_head(i)
                                })
                                .unwrap_or_else(|| empty_head(i))
                        })
                        .collect();
                    registry.numberings.push(NumberingDef { id: id as u32 + 1, start, levels });
                    group.numbering = Some(id);
                    id
                };
                let resolved = group.levels[level as usize].as_mut().unwrap();
                resolved.current = Some(match resolved.current {
                    Some(n) => n.checked_add(1).ok_or_else(|| {
                        DocxError::invalid(part, "numPr", "source numbering overflow")
                    })?,
                    None => u64::from(resolved.start),
                });
                for child in group.levels.iter_mut().skip(level as usize + 1).flatten() {
                    child.current = None;
                }
                Ok(Some(ListRef::Number(id, level as u8)))
            }
        }
    }
    pub(super) fn current(&self, defs: &Definitions, para: &ParaDelta) -> Option<u64> {
        let (key, level) = source_group(defs, para)?;
        self.groups.get(&key)?.levels.get(level as usize)?.as_ref()?.current
    }
    fn observe_omitted(&mut self, defs: &Definitions, para: &ParaDelta) {
        let Some(num) = para.num_id.filter(|&id| id != 0) else {
            return;
        };
        let Some(number) = defs.numbers.get(&num) else {
            return;
        };
        let Some(abstract_id) = number.abstract_id else {
            return;
        };
        let level = defs.effective_level(num, para.level.unwrap_or(0));
        let key = group_key(num, abstract_id, number.has_override);
        let Some(group) = self.groups.get_mut(&key) else {
            return;
        };
        let Some(resolved) = group.levels.get(level as usize) else {
            return;
        };
        // A missing/unsupported number still consumes source progression. Known
        // bullets have no numeric counter to lose, even if their glyph is lost.
        if !resolved.as_ref().is_some_and(|l| matches!(l.compiled.marker, Marker::Bullet { .. })) {
            let bit = 1 << level;
            group.lost |= bit | (group.seen & !(bit - 1));
            group.seen |= bit;
        }
    }
}
fn compile(
    source: &Level,
    index: usize,
    fonts: &Fonts,
    bytes: &mut usize,
    part: &str,
) -> Result<()> {
    if source.compiled.get().is_some() {
        return Ok(());
    }
    // Charge inspected input even when template validation fails. No instance
    // reference can rescan this input or spend the interpretation budget again.
    let size = source.text.as_deref().map_or(0, str::len);
    let total = bytes.saturating_add(size);
    limit(part, "numPr", "interpreted marker bytes", total, 64 * 1024 * 1024)?;
    *bytes = total;
    let mut text = Rc::from("");
    let marker = match source.format {
        Format::Bullet => {
            let (glyph, approximated, slots) = bullet_glyph(source, fonts);
            Marker::Bullet { glyph, approximated, slots }
        }
        Format::Number(_) => {
            if let Some((prefix, suffix)) =
                source.text.as_deref().and_then(|text| template(text, index))
            {
                text = Rc::from(format!("{prefix}^{}{suffix}", index + 1));
                Marker::Number
            } else {
                Marker::Lost
            }
        }
        Format::Unsupported => Marker::Lost,
    };
    let _ = source.compiled.set(Rc::new(Compiled { text, marker }));
    Ok(())
}
fn build_group(defs: &Definitions, abstract_id: u32, number: &definitions::Number) -> Group {
    let base = defs.numbering_base(abstract_id).map(|(_, levels)| levels);
    let mut levels: [Option<ResolvedLevel>; 10] = Default::default();
    let mut bullet = false;
    let mut numbered = false;
    for (i, target) in levels.iter_mut().enumerate() {
        let over = number.levels.get(&(i as u32));
        let Some(source) = select_level(base.and_then(|ls| ls.get(&(i as u32))), over) else {
            continue;
        };
        let compiled =
            source.compiled.get().expect("original levels compiled before groups").clone();
        bullet |= matches!(source.format, Format::Bullet);
        numbered |= !matches!(source.format, Format::Bullet);
        *target = Some(ResolvedLevel {
            compiled,
            start: over.and_then(|l| l.start_override).or(source.start).unwrap_or(1),
            format: match source.format {
                Format::Number(format) => format,
                _ => NumberFormatType::Digit,
            },
            restart: source.restart,
            current: None,
        });
    }
    Group { levels, lost: 0, seen: 0, numbering: None, mixed: bullet && numbered }
}

fn empty_head(index: usize) -> ParaHead {
    ParaHead {
        start: 1,
        level: index as u32 + 1,
        num_format: NumberFormatType::Digit,
        text: String::new(),
        checkable: false,
    }
}
fn template(text: &str, index: usize) -> Option<(&str, &str)> {
    let token = format!("%{}", index + 1);
    let (prefix, suffix) = text.split_once(&token)?;
    let placeholder = |text: &str| {
        text.as_bytes()
            .windows(2)
            .any(|pair| matches!(pair[0], b'%' | b'^') && pair[1].is_ascii_digit())
    };
    if placeholder(prefix)
        || placeholder(suffix)
        || suffix.starts_with(|c: char| c.is_ascii_digit())
    {
        return None;
    }
    Some((prefix, suffix))
}
fn bullet_glyph(level: &Level, fonts: &Fonts) -> (Option<char>, bool, bool) {
    let mut chars = level.text.as_deref().unwrap_or("").chars();
    let Some(glyph) = chars.next().filter(|_| chars.next().is_none()) else {
        return (None, false, false);
    };
    if level.picture {
        return (None, false, false);
    }
    let [ascii, hansi, _, _] = level.marker_char.fonts;
    let slots = ascii.zip(hansi).is_some_and(|(a, b)| a != b);
    let private = matches!(glyph as u32,0xe000..=0xf8ff | 0xf0000..=0xffffd | 0x100000..=0x10fffd);
    let font = if glyph.is_ascii() { ascii.or(hansi) } else { hansi.or(ascii) };
    let Some(font) = font else {
        return (None, false, slots);
    };
    let font = fonts.name(font);
    let mapped = if font.eq_ignore_ascii_case("Symbol") && glyph == '\u{f0b7}' {
        Some('•')
    } else if font.eq_ignore_ascii_case("Wingdings") && glyph == '\u{f0a7}' {
        Some('▪')
    } else if font.eq_ignore_ascii_case("Wingdings") && glyph == '\u{f06c}' {
        Some('●')
    } else if font.eq_ignore_ascii_case("Wingdings") && glyph == '\u{f06e}' {
        Some('■')
    } else if font.eq_ignore_ascii_case("Courier New") && glyph == 'o' {
        Some('◦')
    } else {
        None
    };
    if let Some(mapped) = mapped {
        return (Some(mapped), true, slots);
    }
    // Only known ordinary text faces establish that a non-private Unicode
    // character is independent of a font-specific symbol/marker encoding.
    let ordinary = line_height::metrics(font).is_some()
        || [
            "Arial",
            "Times New Roman",
            "Courier New",
            "Verdana",
            "Tahoma",
            "Helvetica",
            "Aptos",
            "Aptos Display",
            "Cambria",
            "Calibri",
            "Noto Sans",
            "Noto Sans CJK KR",
            "Noto Serif CJK KR",
        ]
        .iter()
        .any(|f| font.eq_ignore_ascii_case(f));
    (
        if private || !ordinary || glyph.is_control() || glyph.is_whitespace() {
            None
        } else {
            Some(glyph)
        },
        false,
        slots,
    )
}
fn reference_lost(part: &str, warnings: &mut Vec<DocxWarning>) -> Result<()> {
    warn(
        warnings,
        part,
        "numPr",
        Kind::ListReferenceLost,
        "list_reference",
        "omit",
        "List instance, abstract definition or level is missing",
    )
}
fn numbering_lost(part: &str, warnings: &mut Vec<DocxWarning>, loss: &'static str) -> Result<()> {
    warn(
        warnings,
        part,
        "numPr",
        Kind::ListNumberingLost,
        loss,
        "omit",
        "Numbering sequence or marker cannot be represented without inventing numbers",
    )
}
fn outline(
    level: u8,
    numbered: bool,
    part: &str,
    warnings: &mut Vec<DocxWarning>,
) -> Result<Option<ListRef>> {
    if numbered {
        warn(
            warnings,
            part,
            "outlineLvl",
            Kind::HeadingNumberLost,
            "heading_number",
            "omit",
            "Outline level retained without the source list number",
        )?;
    }
    Ok(Some(ListRef::Outline(level)))
}

fn source_group(defs: &Definitions, para: &ParaDelta) -> Option<(GroupKey, u32)> {
    let num = para.num_id.filter(|&id| id != 0)?;
    let number = defs.numbers.get(&num)?;
    let abstract_id = number.abstract_id?;
    Some((
        group_key(num, abstract_id, number.has_override),
        defs.effective_level(num, para.level.unwrap_or(0)),
    ))
}

fn group_key(num: u32, abstract_id: u32, has_override: bool) -> GroupKey {
    if has_override {
        GroupKey::Instance(num)
    } else {
        GroupKey::Abstract(abstract_id)
    }
}
pub(super) fn select_level<'a>(
    base: Option<&'a Level>,
    over: Option<&'a Level>,
) -> Option<&'a Level> {
    over.filter(|l| l.defined).or(base)
}

#[cfg(test)]
#[path = "lists_tests.rs"]
mod tests;
