//! Source list instances; lifetime is the document, independent of spacing boundaries.
use super::*;
mod counter;
use counter::CounterState;
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
    pub(super) restart: Option<u32>,
    pub(super) legal: bool,
    pub(super) picture: bool,
    pub(super) compiled: std::cell::OnceCell<Rc<Compiled>>,
}
impl Level {
    pub(super) fn own_format(&self) -> Format {
        match self.format {
            Format::Number(_) if self.legal => Format::Number(NumberFormatType::Digit),
            format => format,
        }
    }
    pub(super) fn compiled_text(&self) -> Rc<str> {
        self.compiled.get().expect("compiled source level").text.clone()
    }
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
pub(super) struct Compiled {
    text: Rc<str>,
    marker: Marker,
    references: u16,
}
struct ResolvedLevel {
    compiled: Rc<Compiled>,
    format: NumberFormatType,
    numbered: bool,
    legal: bool,
}
#[derive(Clone, Copy)]
enum Marker {
    Number,
    Bullet { glyph: Option<char>, approximated: bool, slots: bool },
    Lost,
}
struct Instance {
    levels: [Option<ResolvedLevel>; 10],
    mixed: bool,
}
struct OutputSegment {
    id: usize,
    counter: CounterState,
    texts: [Rc<str>; 10],
    formats: [NumberFormatType; 10],
}
struct Group {
    counter: CounterState,
    restarts: [Option<u32>; 10],
    lost: u16,
    seen: u16,
    output: OutputGroup,
}
#[derive(Default)]
struct OutputGroup {
    first: Option<usize>,
    active: Option<OutputSegment>,
    literals: HashMap<(usize, Rc<str>), usize>,
    stopped: bool,
    reserved: bool,
}
pub(super) enum ObservedList {
    Ready(ListRef),
    Number(NumberRequest),
}
pub(super) struct NumberRequest {
    key: u32,
    num: u32,
    pub(super) level: usize,
    values: [u64; 10],
    seeds: [u32; 10],
}
#[derive(Default)]
pub(super) struct Lists {
    groups: HashMap<u32, Group>,
    instances: HashMap<u32, Instance>,
    overrides_used: HashSet<(u32, usize)>,
    legal_warnings: HashSet<(u32, usize)>,
    unemitted_groups: usize,
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
            let key = abstract_id;
            let instance = build_instance(definitions, abstract_id, number);
            let group =
                lists.groups.entry(key).or_insert_with(|| build_group(definitions, abstract_id));
            if !group.output.reserved
                && instance
                    .levels
                    .iter()
                    .flatten()
                    .any(|l| matches!(l.compiled.marker, Marker::Number))
            {
                group.output.reserved = true;
                lists.unemitted_groups += 1;
            }
            lists.instances.insert(id, instance);
        }
        Ok(lists)
    }
    #[cfg(test)]
    pub(super) fn resolve(
        &mut self,
        definitions: &Definitions,
        registry: &mut StyleRegistry,
        para: &ParaDelta,
        part: &str,
        warnings: &mut Vec<DocxWarning>,
    ) -> Result<Option<ListRef>> {
        let observed = self.observe(definitions, registry, para, part, warnings)?;
        match observed {
            Some(ObservedList::Number(request)) => {
                self.assign_numbering(registry, &request, part, warnings)
            }
            Some(ObservedList::Ready(list)) => Ok(Some(list)),
            None => Ok(None),
        }
    }
    pub(super) fn observe(
        &mut self,
        definitions: &Definitions,
        registry: &mut StyleRegistry,
        para: &ParaDelta,
        part: &str,
        warnings: &mut Vec<DocxWarning>,
    ) -> Result<Option<ObservedList>> {
        // 이것을 실패시키는 것: 원본 문단을 생략 경로에서 다시 소비하거나 암묵 상위가 override를 소비.
        if let Some((key, level)) = source_group(definitions, para) {
            if let Some(group) = self.groups.get_mut(&key).filter(|_| level < 10) {
                let num = para.num_id.unwrap();
                let reset = self
                    .overrides_used
                    .insert((num, level as usize))
                    .then(|| {
                        definitions.numbers[&num].levels.get(&level).and_then(|l| l.start_override)
                    })
                    .flatten();
                group.counter.advance_word(level as usize, reset, &group.restarts, part)?;
                if reset.is_some() {
                    group.lost &= (1 << level) - 1;
                }
            }
        }
        let marker = self.resolve_marker(definitions, registry, para, part, warnings)?;
        // Every omitted source number can advance its own sequence and restart
        // already-seen descendants, regardless of why its marker was omitted.
        if matches!(marker, None | Some(ObservedList::Ready(ListRef::Outline(_)))) {
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
    ) -> Result<Option<ObservedList>> {
        if let Some(level) = para.outline.filter(|&l| l < 9) {
            return outline(level as u8, para.num_id.is_some_and(|id| id != 0), part, warnings)
                .map(|list| list.map(ObservedList::Ready));
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
        let key = abstract_id;
        let group = self.groups.get_mut(&key).unwrap();
        let instance = &self.instances[&num];
        let Some(resolved) = &instance.levels[level as usize] else {
            reference_lost(part, warnings)?;
            return Ok(None);
        };
        let bit = 1 << level;
        group.seen |= bit;
        if group.lost & bit != 0 {
            numbering_lost(part, warnings, "numbering_sequence")?;
            return Ok(None);
        }
        if instance.mixed {
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

                Ok(Some(ObservedList::Ready(ListRef::Bullet(id, level as u8))))
            }
            Marker::Number => {
                // 이것을 실패시키는 것: 누락·비번호 참조를 십진 형식으로 합성.
                if (0..10).any(|i| {
                    resolved.compiled.references & (1 << i) != 0
                        && !instance.levels[i].as_ref().is_some_and(|l| l.numbered)
                }) {
                    numbering_lost(part, warnings, "numbering_format")?;
                    return Ok(None);
                }
                group.lost &= (bit << 1) - 1;
                let seeds = std::array::from_fn(|i| {
                    if self.overrides_used.contains(&(num, i)) {
                        group.counter.starts[i]
                    } else {
                        number
                            .levels
                            .get(&(i as u32))
                            .and_then(|l| l.start_override)
                            .unwrap_or(group.counter.starts[i])
                    }
                });
                Ok(Some(ObservedList::Number(NumberRequest {
                    key,
                    num,
                    level: level as usize,
                    values: group.counter.values,
                    seeds,
                })))
            }
        }
    }
    pub(super) fn assign_numbering(
        &mut self,
        registry: &mut StyleRegistry,
        request: &NumberRequest,
        part: &str,
        warnings: &mut Vec<DocxWarning>,
    ) -> Result<Option<ListRef>> {
        let NumberRequest { key, num, level, values, seeds } = *request;
        let instance = &self.instances[&num];
        let source = instance.levels[level].as_ref().unwrap();
        let group = &mut self.groups.get_mut(&key).unwrap().output;
        // 참조 수준은 현재 문단의 legal 여부로만 정규화한다.
        let formats = std::array::from_fn(|i| {
            instance.levels[i].as_ref().map_or(NumberFormatType::Digit, |l| {
                let referenced = source.compiled.references & (1 << i) != 0;
                if (referenced && source.legal) || (!referenced && l.legal) {
                    NumberFormatType::Digit
                } else {
                    l.format
                }
            })
        });
        let token = format!("^{}", level + 1);
        let literal_zero = values[level] == 0
            && formats[level] == NumberFormatType::Digit
            && source.compiled.references == 1 << level
            && source.compiled.text.matches(&token).count() == 1;
        // 이것을 실패시키는 것: 리터럴 재사용에 다시 정의·텍스트 예산을 청구.
        if literal_zero {
            if let Some(&id) = group.literals.get(&(level, source.compiled.text.clone())) {
                return Ok(Some(ListRef::Number(id, level as u8)));
            }
        } else if (0..10).any(|i| source.compiled.references & (1 << i) != 0 && values[i] == 0) {
            approximation(
                warnings,
                part,
                "list_number_zero",
                "Zero-valued compound or non-decimal numbering uses positive output counters",
            )?;
        }
        let compatible = if let Some(active) = &group.active {
            let mut next = active.counter.clone();
            next.advance_core(level, part)?;
            (active.texts[level] == source.compiled.text)
                && (0..10)
                    .filter(|&i| source.compiled.references & (1 << i) != 0)
                    .all(|i| active.formats[i] == formats[i] && next.values[i] == values[i].max(1))
        } else {
            false
        };
        // 이것을 실패시키는 것: 리터럴 0보다 정상 후보의 값 비교를 먼저 적용.
        if literal_zero || !compatible {
            let can_allocate = group.first.is_none()
                || registry.numberings.len() + self.unemitted_groups < MAX_DEFINITIONS;
            if can_allocate {
                let starts = CounterState::segment_starts(values, seeds, level, part)?;
                let mut texts: [Rc<str>; 10] = std::array::from_fn(|i| {
                    instance.levels[i]
                        .as_ref()
                        .filter(|l| matches!(l.compiled.marker, Marker::Number))
                        .map_or_else(|| Rc::from(""), |l| l.compiled.text.clone())
                });
                let mut additional = texts.iter().map(|t| t.len()).sum::<usize>();
                if literal_zero {
                    additional = additional - token.len() + 1;
                }
                let owned = self.owned_marker_bytes.saturating_add(additional);
                // 이것을 실패시키는 것: 정의의 문자열을 복사한 뒤에만 64MiB 상한 검사.
                limit(part, "numPr", "materialized marker bytes", owned, 64 * 1024 * 1024)?;
                limit(
                    part,
                    "numPr",
                    "output numbering definitions",
                    registry.numberings.len() + 1,
                    MAX_DEFINITIONS,
                )?;
                if literal_zero {
                    texts[level] = Rc::from(source.compiled.text.replacen(&token, "0", 1));
                }
                let id = registry.numberings.len();
                let levels = (0..10)
                    .map(|i| ParaHead {
                        start: starts[i],
                        num_format: formats[i],
                        text: texts[i].to_string(),
                        ..empty_head(i)
                    })
                    .collect();
                registry.numberings.push(NumberingDef {
                    id: id as u32 + 1,
                    start: starts[0],
                    levels,
                });
                self.owned_marker_bytes = owned;
                if group.first.is_some() {
                    approximation(warnings, part, "list_numbering_split", "List numbering uses separate definitions with fixed starts; later edits do not propagate across segments")?;
                } else {
                    group.first = Some(id);
                    self.unemitted_groups -= usize::from(group.reserved);
                }
                if literal_zero {
                    group.literals.insert((level, source.compiled.text.clone()), id);
                    return Ok(Some(ListRef::Number(id, level as u8)));
                }
                group.active =
                    Some(OutputSegment { id, counter: CounterState::new(starts), texts, formats });
            } else if !group.stopped {
                group.stopped = true;
                approximation(warnings, part, "list_numbering_budget", "The numbering definition limit prevents further splits; remaining numbering is approximated")?;
            }
        }
        // 이것을 실패시키는 것: 상한에서 기존 정의의 형식·텍스트·시작값을 사후 변경.
        let id = if let Some(active) = &mut group.active {
            active.counter.advance_core(level, part)?;
            active.id
        } else {
            group.first.expect("first definition space is reserved")
        };
        // 과거 문단이나 값 차이만으로 legal 근사를 소급하지 않는다.
        if source.legal
            && (0..10).any(|i| {
                source.compiled.references & (1 << i) != 0
                    && registry.numberings[id].levels[i].num_format != NumberFormatType::Digit
            })
            && self.legal_warnings.insert((num, level))
        {
            approximation(
                warnings,
                part,
                "list_number_legal",
                "The numbering limit retains non-decimal reference formats in a legal marker",
            )?;
        }
        Ok(Some(ListRef::Number(id, level as u8)))
    }
    pub(super) fn current(&self, defs: &Definitions, para: &ParaDelta) -> Option<u64> {
        let (key, level) = source_group(defs, para)?;
        let counter = &self.groups.get(&key)?.counter;
        counter.used.get(level as usize).copied().filter(|used| *used)?;
        Some(counter.values[level as usize])
    }
    /// Source formats follow the current paragraph's legal flag, including ancestor references.
    pub(super) fn current_reference(
        &self,
        defs: &Definitions,
        para: &ParaDelta,
        index: u8,
    ) -> Option<(NumberFormatType, u64)> {
        let (key, level) = source_group(defs, para)?;
        let instance = self.instances.get(&para.num_id?)?;
        let source = instance.levels.get(level as usize)?.as_ref()?;
        if source.compiled.references & (1 << index) == 0 {
            return None;
        }
        let referenced = instance.levels.get(usize::from(index))?.as_ref()?;
        let counter = &self.groups.get(&key)?.counter;
        if !counter.used[usize::from(index)] {
            return None;
        }
        let format = if source.legal { NumberFormatType::Digit } else { referenced.format };
        Some((format, counter.values[usize::from(index)]))
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
        let key = abstract_id;
        let Some(group) = self.groups.get_mut(&key) else {
            return;
        };
        let Some(resolved) = self.instances[&num].levels.get(level as usize) else {
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
    let mut references = 0;
    let marker = match source.format {
        Format::Bullet => {
            let (glyph, approximated, slots) = bullet_glyph(source, fonts);
            Marker::Bullet { glyph, approximated, slots }
        }
        Format::Number(_) => {
            if let Some((converted, refs)) =
                source.text.as_deref().and_then(|text| template(text, index))
            {
                text = Rc::from(converted);
                references = refs;
                Marker::Number
            } else {
                Marker::Lost
            }
        }
        Format::Unsupported => Marker::Lost,
    };
    let _ = source.compiled.set(Rc::new(Compiled { text, marker, references }));
    Ok(())
}
fn build_instance(defs: &Definitions, abstract_id: u32, number: &definitions::Number) -> Instance {
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
            numbered: matches!(source.format, Format::Number(_)),
            legal: source.legal,
            format: match source.format {
                Format::Number(format) => format,
                _ => NumberFormatType::Digit,
            },
        });
    }
    Instance { levels, mixed: bullet && numbered }
}

fn build_group(defs: &Definitions, abstract_id: u32) -> Group {
    let base = defs.numbering_base(abstract_id).map(|(_, levels)| levels);
    let starts = std::array::from_fn(|i| {
        base.and_then(|ls| ls.get(&(i as u32))).and_then(|l| l.start).unwrap_or(0)
    });
    let restarts =
        std::array::from_fn(|i| base.and_then(|ls| ls.get(&(i as u32))).and_then(|l| l.restart));
    Group {
        counter: CounterState::new(starts),
        restarts,
        lost: 0,
        seen: 0,
        output: OutputGroup::default(),
    }
}
fn approximation(
    warnings: &mut Vec<DocxWarning>,
    part: &str,
    loss: &'static str,
    reason: &'static str,
) -> Result<()> {
    warn(warnings, part, "numPr", Kind::ValueApproximated, loss, "approximate", reason)
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
fn template(text: &str, index: usize) -> Option<(String, u16)> {
    let bytes = text.as_bytes();
    let mut converted = String::with_capacity(text.len());
    let mut references = 0;
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        // 원문 caret 번호는 출력 토큰으로 해석될 수 있으므로 거부한다.
        if bytes[i] == b'^' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit) {
            return None;
        }
        if bytes[i] == b'%' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit) {
            let begin = i;
            i += 1;
            let mut n = 0usize;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                n = n.saturating_mul(10).saturating_add(usize::from(bytes[i] - b'0'));
                i += 1;
            }
            if n > index + 1
                || bytes[begin + 1] == b'0'
                || (n == index + 1 && references & (1 << index) != 0)
            {
                return None;
            }
            converted.push_str(&text[start..begin]);
            converted.push('^');
            converted.push_str(&text[begin + 1..i]);
            references |= 1 << (n - 1);
            start = i;
        } else {
            i += 1;
        }
    }
    if references & (1 << index) == 0 {
        return None;
    }
    converted.push_str(&text[start..]);
    Some((converted, references))
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

fn source_group(defs: &Definitions, para: &ParaDelta) -> Option<(u32, u32)> {
    let num = para.num_id.filter(|&id| id != 0)?;
    let number = defs.numbers.get(&num)?;
    let abstract_id = number.abstract_id?;
    Some((abstract_id, defs.effective_level(num, para.level.unwrap_or(0))))
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
