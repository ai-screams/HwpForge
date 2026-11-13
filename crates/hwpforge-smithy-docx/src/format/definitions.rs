use super::*;
use crate::{
    body::tracked,
    xml::{self, TextBudget, XmlEvent, A_NS},
};
use std::{
    collections::{hash_map::RandomState, BTreeMap, HashMap},
    hash::BuildHasher,
};

#[derive(Default)]
pub(super) struct Number {
    pub(super) abstract_id: Option<u32>,
    pub(super) has_override: bool,
    pub(super) levels: BTreeMap<u32, super::lists::Level>,
}
#[derive(Default)]
pub(super) struct AbstractMetadata {
    single_level: bool,
    style_link: Option<StyleKey>,
}
#[derive(Default)]
pub(crate) struct Definitions {
    pub(super) styles: Vec<Option<Style>>,
    style_ids: HashMap<u64, Vec<StyleKey>>,
    style_names: Vec<Rc<str>>,
    style_hasher: RandomState,
    pub(crate) defaults: Delta,
    pub(super) theme: Theme,
    pub(super) fonts: Fonts,
    pub(super) abstracts: HashMap<u32, BTreeMap<u32, super::lists::Level>>,
    pub(super) numbers: HashMap<u32, Number>,
    abstract_metadata: HashMap<u32, AbstractMetadata>,
    counts: [usize; 4],
    level_count: usize,
    tab_count: usize,
    pub(super) numbering_part: Option<Rc<str>>,
    #[cfg(test)]
    pub(super) part_copied_bytes: usize,
    #[cfg(test)]
    pub(super) style_lookup_bytes: std::cell::Cell<usize>,
    #[cfg(test)]
    pub(super) style_comparison_bytes: std::cell::Cell<usize>,
    #[cfg(test)]
    pub(super) style_copied_bytes: usize,
}
// Only an admitted frame grants ownership to its immediate children. Unknown
// frames carry no owner, and popping them cannot clear an outer definition.
#[derive(Clone, Copy)]
enum Owner {
    Defaults,
    Style(StyleKey),
    Abstract(u32, u32),
    Number(u32, u32),
}
#[derive(Clone, Copy, Default)]
enum Frame {
    #[default]
    Ignored,
    Root,
    Defaults,
    LatentStyles,
    Leaf,
    Property(PropertyState),
    Metadata,
    DefaultRun,
    DefaultPara,
    Style(StyleKey),
    Table(StyleKey),
    TableProperties(StyleKey, table::TableProperty),
    CellProperties,
    Properties(Owner, bool),
    NumPr(Owner),
    NumberTabs(Owner),
    NumberTab,
    Abstract(u32),
    Number(u32),
    Override {
        num: u32,
        level: u32,
        has_level: bool,
    },
    Level(Owner),
    ThemeElements,
    FontScheme,
    Family(usize, Option<FontKey>),
    Section,
}
impl Definitions {
    fn marker_level_mut(&mut self, owner: Owner) -> Option<&mut super::lists::Level> {
        match owner {
            Owner::Abstract(id, level) => self.abstracts.get_mut(&id)?.get_mut(&level),
            Owner::Number(id, level) => self.numbers.get_mut(&id)?.levels.get_mut(&level),
            _ => None,
        }
    }
    pub(super) fn marker_level(
        &self,
        para: &ParaDelta,
    ) -> Option<(&super::lists::Level, BTreeMap<i32, bool>)> {
        let num = para.num_id?;
        let level = self.effective_level(num, para.level.unwrap_or(0));
        let number = self.numbers.get(&num)?;
        let base = self.numbering_base(number.abstract_id?).and_then(|(_, ls)| ls.get(&level));
        let over = number.levels.get(&level);
        let source = super::lists::select_level(base, over)?;
        let mut tabs = base.map(|l| l.tabs.clone()).unwrap_or_default();
        tabs.extend(over.into_iter().flat_map(|l| l.tabs.iter().map(|(&k, &v)| (k, v))));
        Some((source, tabs))
    }
    pub(super) fn numbering_base(
        &self,
        id: u32,
    ) -> Option<(u32, &BTreeMap<u32, super::lists::Level>)> {
        let levels = self.abstracts.get(&id)?;
        if !levels.is_empty() {
            return Some((id, levels));
        }
        let key = self.abstract_metadata.get(&id)?.style_link?;
        let style = self.style(key).filter(|s| s.kind == StyleKind::Numbering)?;
        let number = self.numbers.get(&style.delta.para.num_id.filter(|&n| n != 0)?)?;
        let linked = number.abstract_id.filter(|&next| next != id)?;
        // One hop only. An empty destination never recursively resolves a link.
        self.abstracts.get(&linked).filter(|ls| !ls.is_empty()).map(|ls| (linked, ls))
    }
    pub(super) fn effective_level(&self, num: u32, level: u32) -> u32 {
        let Some(number) = self.numbers.get(&num) else {
            return level;
        };
        let Some(id) = number.abstract_id else {
            return level;
        };
        let Some((base_id, levels)) = self.numbering_base(id) else {
            return level;
        };
        if self.abstract_metadata.get(&base_id).is_some_and(|m| m.single_level)
            && !levels.contains_key(&level)
            && !number.levels.get(&level).is_some_and(|l| l.defined)
        {
            0
        } else {
            level
        }
    }
    pub(super) fn warn_numbering_links(&self, warnings: &mut Vec<DocxWarning>) -> Result<()> {
        for (&id, meta) in &self.abstract_metadata {
            if meta.style_link.is_some() && self.numbering_base(id).is_none() {
                warn(
                    warnings,
                    self.numbering_part.as_deref().unwrap_or("#numbering"),
                    "numStyleLink",
                    Kind::UnsupportedAttribute,
                    "numbering_style_link",
                    "ignore",
                    "Numbering style link is missing, cyclic or requires another hop",
                )?;
            }
        }
        Ok(())
    }

    fn lookup_style(&self, id: &str, hash: u64) -> Option<StyleKey> {
        self.style_ids.get(&hash)?.iter().copied().find(|key| {
            let name = self.style_names[key.0].as_ref();
            #[cfg(test)]
            {
                self.style_comparison_bytes
                    .set(self.style_comparison_bytes.get() + name.len().min(id.len()));
            }
            name == id
        })
    }
    fn intern_style(&mut self, id: &str, part: &str, at: &str) -> Result<StyleKey> {
        crate::identifier::check(id, part, at)?;
        #[cfg(test)]
        {
            self.style_lookup_bytes.set(self.style_lookup_bytes.get() + id.len());
        }
        let hash = self.style_hasher.hash_one(id);
        if let Some(key) = self.lookup_style(id, hash) {
            return Ok(key);
        }
        limit(part, at, "style identifiers", self.styles.len() + 1, MAX_DEFINITIONS * 8)?;
        let key = StyleKey(self.styles.len());
        self.styles.push(None);
        #[cfg(test)]
        {
            self.style_copied_bytes += id.len();
        }
        self.style_names.push(Rc::from(id));
        self.style_ids.entry(hash).or_default().push(key);
        Ok(key)
    }
    pub(crate) fn style_key(&self, id: &str, part: &str, at: &str) -> Result<StyleKey> {
        crate::identifier::check(id, part, at)?;
        #[cfg(test)]
        {
            self.style_lookup_bytes.set(self.style_lookup_bytes.get() + id.len());
        }
        let hash = self.style_hasher.hash_one(id);
        // Missing is distinct from an absent reference, without retaining arbitrary IDs.
        Ok(self.lookup_style(id, hash).unwrap_or(StyleKey(usize::MAX)))
    }
    pub(super) fn style(&self, key: StyleKey) -> Option<&Style> {
        self.styles.get(key.0).and_then(Option::as_ref)
    }
    pub(super) fn ordered_styles(&self) -> Vec<StyleKey> {
        let mut ids: Vec<_> =
            (0..self.styles.len()).map(StyleKey).filter(|&key| self.style(key).is_some()).collect();
        ids.sort_unstable_by(|a, b| self.style_names[a.0].cmp(&self.style_names[b.0]));
        ids
    }
    fn target(&mut self, owner: Owner) -> Option<&mut Delta> {
        match owner {
            Owner::Defaults => Some(&mut self.defaults),
            Owner::Style(k) => self.styles.get_mut(k.0)?.as_mut().map(|s| &mut s.delta),
            Owner::Abstract(id, level) => {
                self.abstracts.get_mut(&id)?.get_mut(&level).map(|l| &mut l.delta)
            }
            Owner::Number(id, level) => {
                self.numbers.get_mut(&id)?.levels.get_mut(&level).map(|l| &mut l.delta)
            }
        }
    }
    pub(crate) fn read(
        &mut self,
        bytes: &[u8],
        part: &str,
        root: &str,
        budget: &mut TextBudget,
        warnings: &mut Vec<DocxWarning>,
    ) -> Result<bool> {
        let mut stack: Vec<Frame> = Vec::new();
        let mut matches = false;
        let mut hang_selected = false;
        let mut ordinal = 0;
        let shared_part: Rc<str> = Rc::from(part);
        #[cfg(test)]
        {
            self.part_copied_bytes += part.len();
        }
        xml::walk(bytes, part, budget, |event| {
            match event {
                XmlEvent::Start(e) => {
                    ordinal += 1;
                    let w = e.namespace_id == W_NS.0;
                    if w && tracked(&e.name) {
                        return Err(DocxError::TrackedChange {
                            part: quote(part),
                            location: format!("{}#{ordinal}", quote(&e.name)),
                            element: quote(&e.name),
                        });
                    }
                    let parent = stack.last().copied().unwrap_or_default();
                    let mut frame = Frame::Ignored;
                    let mut diagnosed = false;
                    if stack.is_empty() {
                        matches = e.name == root
                            && e.namespace_id == if root == "theme" { A_NS.0 } else { W_NS.0 };
                        if matches {
                            frame = Frame::Root;
                            if root == "numbering" {
                                self.numbering_part = Some(shared_part.clone());
                            }
                        }
                    } else if matches {
                        if w && matches!(parent, Frame::Section) {
                            units::structural(&e, part)?;
                        }
                        if root == "theme" && e.namespace_id == A_NS.0 {
                            frame = match (parent, e.name.as_str()) {
                                (Frame::Root, "themeElements") => Frame::ThemeElements,
                                (Frame::ThemeElements, "fontScheme") => Frame::FontScheme,
                                (Frame::FontScheme, "majorFont") => Frame::Family(0, None),
                                (Frame::FontScheme, "minorFont") => Frame::Family(1, None),
                                (Frame::Family(f, _), "latin" | "ea" | "cs") => {
                                    let slot = match e.name.as_str() {
                                        "latin" => 0,
                                        "ea" => 1,
                                        _ => 2,
                                    };
                                    self.theme.fonts[f][slot] = e
                                        .attribute("typeface")
                                        .filter(|v| !v.is_empty())
                                        .map(|v| self.fonts.intern(v, part, &e.name))
                                        .transpose()?;
                                    Frame::Leaf
                                }
                                (Frame::Family(f, hang), "font") => {
                                    diagnosed = true;
                                    if let Some(name) =
                                        e.attribute("typeface").filter(|v| !v.is_empty())
                                    {
                                        if e.attribute("script") == Some("Hang") && hang.is_none() {
                                            let key = self.fonts.intern(name, part, &e.name)?;
                                            *stack.last_mut().unwrap() =
                                                Frame::Family(f, Some(key));
                                        } else {
                                            theme_script_warning(warnings, part)?;
                                        }
                                    }
                                    Frame::Ignored
                                }
                                _ => Frame::Ignored,
                            };
                        } else if w && root == "styles" {
                            frame = match (parent, e.name.as_str()) {
                                (Frame::Root, "docDefaults") => Frame::Defaults,
                                (Frame::Root, "latentStyles") => Frame::LatentStyles,
                                (Frame::LatentStyles, "lsdException") => Frame::Metadata,
                                (Frame::Defaults, "rPrDefault") => Frame::DefaultRun,
                                (Frame::Defaults, "pPrDefault") => Frame::DefaultPara,
                                (Frame::Root, "style") => {
                                    let id = attr(&e, "styleId").ok_or_else(|| {
                                        DocxError::invalid(part, "style", "missing styleId")
                                    })?;
                                    let key = self.intern_style(id, part, "style")?;
                                    if self.style(key).is_some() {
                                        return Err(DocxError::invalid(
                                            part,
                                            "style",
                                            "duplicate styleId",
                                        ));
                                    }
                                    let kind = match attr(&e, "type") {
                                        Some("paragraph") => StyleKind::Paragraph,
                                        Some("character") => StyleKind::Character,
                                        Some("table") => StyleKind::Table,
                                        Some("numbering") => StyleKind::Numbering,
                                        _ => {
                                            return Err(DocxError::invalid(
                                                part,
                                                "style",
                                                "unknown or missing style type",
                                            ))
                                        }
                                    };
                                    limit(
                                        part,
                                        "style",
                                        "style definitions",
                                        self.counts[kind.index()] + 1,
                                        MAX_DEFINITIONS,
                                    )?;
                                    self.counts[kind.index()] += 1;
                                    self.styles[key.0] = Some(Style {
                                        kind,
                                        parent: None,
                                        name: None,
                                        delta: Delta::default(),
                                        table: table::TableProperties::default(),
                                        part: shared_part.clone(),
                                        at: format!("style:{}", quote(id)),
                                        default: properties::boolean(
                                            &e,
                                            "default",
                                            None,
                                            part,
                                            &format!("style:{}", quote(id)),
                                            warnings,
                                        )?
                                        .unwrap_or(false),
                                    });
                                    Frame::Style(key)
                                }
                                (Frame::Style(key), "basedOn") => {
                                    if self
                                        .style(key)
                                        .is_some_and(|s| s.kind != StyleKind::Numbering)
                                    {
                                        let parent = attr(&e, "val")
                                            .map(|v| self.intern_style(v, part, "basedOn"))
                                            .transpose()?;
                                        let style =
                                            self.styles[key.0].as_mut().ok_or_else(|| {
                                                DocxError::invalid(
                                                    part,
                                                    "style",
                                                    "missing admitted style",
                                                )
                                            })?;
                                        style.parent = parent;
                                        if parent.is_none() {
                                            warn(
                                                warnings,
                                                part,
                                                &style.at,
                                                Kind::StyleReferenceLost,
                                                "style_parent",
                                                "ignore_link",
                                                "basedOn is missing its parent ID",
                                            )?;
                                        }
                                    } else {
                                        definition_warning(
                                            &e,
                                            part,
                                            root,
                                            "numbering basedOn link",
                                            warnings,
                                        )?;
                                    }
                                    Frame::Leaf
                                }
                                (Frame::Style(key), "name") => {
                                    if let Some(s) = self.styles[key.0].as_mut() {
                                        s.name = attr(&e, "val").map(str::to_owned);
                                    }
                                    Frame::Leaf
                                }
                                (Frame::Style(key), "tblPr") => {
                                    Frame::TableProperties(key, table::TableProperty::Root)
                                }
                                (Frame::Style(_) | Frame::Table(_), "tcPr") => {
                                    Frame::CellProperties
                                }
                                (Frame::CellProperties, "shd") => {
                                    table::TableProperties::default().read(
                                        table::TableProperty::Root,
                                        &e,
                                        part,
                                        "shd",
                                        warnings,
                                    )?;
                                    diagnosed = true;
                                    Frame::Ignored
                                }
                                (Frame::TableProperties(key, state), _) => {
                                    let mut props = std::mem::take(
                                        &mut self.styles[key.0].as_mut().unwrap().table,
                                    );
                                    let state = props.read(state, &e, part, &e.name, warnings)?;
                                    self.styles[key.0].as_mut().unwrap().table = props;
                                    Frame::TableProperties(key, state)
                                }
                                (Frame::Style(key), "tblStylePr") => {
                                    if attr(&e, "type") == Some("wholeTable") {
                                        Frame::Table(key)
                                    } else {
                                        diagnosed = true;
                                        warn(
                                            warnings,
                                            part,
                                            "tblStylePr",
                                            Kind::UnsupportedAttribute,
                                            "conditional_table_style",
                                            "ignore",
                                            "Conditional table formatting is not applied",
                                        )?;
                                        Frame::Ignored
                                    }
                                }
                                (Frame::Style(key) | Frame::Table(key), "rPr" | "pPr") => {
                                    Frame::Properties(Owner::Style(key), e.name == "rPr")
                                }
                                (Frame::DefaultRun, "rPr") => {
                                    Frame::Properties(Owner::Defaults, true)
                                }
                                (Frame::DefaultPara, "pPr") => {
                                    Frame::Properties(Owner::Defaults, false)
                                }
                                // Editor/UI metadata does not change the saved effective formatting.
                                (
                                    Frame::Style(_),
                                    "next" | "link" | "aliases" | "uiPriority" | "qFormat"
                                    | "semiHidden" | "unhideWhenUsed" | "locked" | "personal"
                                    | "personalCompose" | "personalReply" | "rsid" | "autoRedefine",
                                ) => Frame::Metadata,
                                _ => Frame::Ignored,
                            };
                        } else if w && root == "numbering" {
                            frame = match (parent, e.name.as_str()) {
                                (Frame::Root, "abstractNum" | "num") => {
                                    let abstract_definition = e.name == "abstractNum";
                                    let id = integer(
                                        &e,
                                        if abstract_definition { "abstractNumId" } else { "numId" },
                                        part,
                                    )?;
                                    if abstract_definition {
                                        limit(
                                            part,
                                            &e.name,
                                            "abstract numbering definitions",
                                            self.abstracts.len() + 1,
                                            MAX_DEFINITIONS,
                                        )?;
                                        if self.abstracts.insert(id, BTreeMap::new()).is_some() {
                                            return Err(DocxError::invalid(
                                                part,
                                                &e.name,
                                                "duplicate abstractNumId",
                                            ));
                                        }
                                        Frame::Abstract(id)
                                    } else {
                                        limit(
                                            part,
                                            &e.name,
                                            "numbering definitions",
                                            self.numbers.len() + 1,
                                            MAX_DEFINITIONS,
                                        )?;
                                        if self.numbers.insert(id, Number::default()).is_some() {
                                            return Err(DocxError::invalid(
                                                part,
                                                &e.name,
                                                "duplicate numId",
                                            ));
                                        }
                                        Frame::Number(id)
                                    }
                                }
                                (Frame::Number(id), "abstractNumId") => {
                                    let value = integer(&e, "val", part)?;
                                    if let Some(n) = self.numbers.get_mut(&id) {
                                        n.abstract_id = Some(value);
                                    }
                                    Frame::Leaf
                                }
                                (Frame::Number(num), "lvlOverride") => {
                                    let level = integer(&e, "ilvl", part)?;
                                    limit(
                                        part,
                                        "lvlOverride",
                                        "numbering levels",
                                        self.level_count + 1,
                                        MAX_DEFINITIONS,
                                    )?;
                                    let n = self.numbers.get_mut(&num).ok_or_else(|| {
                                        DocxError::invalid(part, "num", "missing admitted number")
                                    })?;
                                    if n.levels
                                        .insert(level, super::lists::Level::default())
                                        .is_some()
                                    {
                                        return Err(DocxError::invalid(
                                            part,
                                            "lvlOverride",
                                            "duplicate level override",
                                        ));
                                    }
                                    self.level_count += 1;
                                    Frame::Override { num, level, has_level: false }
                                }
                                (Frame::Abstract(id), "lvl") => {
                                    let level = integer(&e, "ilvl", part)?;
                                    limit(
                                        part,
                                        "lvl",
                                        "numbering levels",
                                        self.level_count + 1,
                                        MAX_DEFINITIONS,
                                    )?;
                                    let levels = self.abstracts.get_mut(&id).ok_or_else(|| {
                                        DocxError::invalid(
                                            part,
                                            "abstractNum",
                                            "missing admitted abstract",
                                        )
                                    })?;
                                    if levels
                                        .insert(
                                            level,
                                            super::lists::Level {
                                                defined: true,
                                                ..Default::default()
                                            },
                                        )
                                        .is_some()
                                    {
                                        return Err(DocxError::invalid(
                                            part,
                                            "lvl",
                                            "duplicate numbering level",
                                        ));
                                    }
                                    self.level_count += 1;
                                    Frame::Level(Owner::Abstract(id, level))
                                }
                                (Frame::Override { num, level, has_level }, "lvl") => {
                                    if has_level || integer(&e, "ilvl", part)? != level {
                                        return Err(DocxError::invalid(
                                            part,
                                            "lvl",
                                            "duplicate or mismatched override level",
                                        ));
                                    }
                                    if let Some(f) = stack.last_mut() {
                                        *f = Frame::Override { num, level, has_level: true };
                                    }
                                    self.numbers.get_mut(&num).unwrap().has_override = true;
                                    self.numbers
                                        .get_mut(&num)
                                        .unwrap()
                                        .levels
                                        .get_mut(&level)
                                        .unwrap()
                                        .defined = true;
                                    Frame::Level(Owner::Number(num, level))
                                }
                                (Frame::Level(owner), "pPr" | "rPr") => {
                                    Frame::Properties(owner, e.name == "rPr")
                                }
                                (
                                    Frame::Level(owner),
                                    "start" | "numFmt" | "lvlText" | "lvlRestart"
                                    | "lvlPicBulletId",
                                ) => {
                                    let target = match owner {
                                        Owner::Abstract(id, level) => self
                                            .abstracts
                                            .get_mut(&id)
                                            .unwrap()
                                            .get_mut(&level)
                                            .unwrap(),
                                        Owner::Number(id, level) => self
                                            .numbers
                                            .get_mut(&id)
                                            .unwrap()
                                            .levels
                                            .get_mut(&level)
                                            .unwrap(),
                                        _ => unreachable!(),
                                    };
                                    match e.name.as_str() {
                                        "start" => target.start = Some(integer(&e, "val", part)?),
                                        "lvlRestart" => {
                                            let index = match owner {
                                                Owner::Abstract(_, level)
                                                | Owner::Number(_, level) => level,
                                                _ => unreachable!(),
                                            };
                                            let value = integer(&e, "val", part)?;
                                            target.restart = value != index;
                                        }
                                        "lvlPicBulletId" => {
                                            integer(&e, "val", part)?;
                                            target.picture = true;
                                        }
                                        "numFmt" => {
                                            target.format =
                                                super::lists::Format::parse(attr(&e, "val"))
                                        }
                                        _ => target.text = attr(&e, "val").map(Rc::from),
                                    }
                                    Frame::Leaf
                                }
                                (Frame::Override { num, level, .. }, "startOverride") => {
                                    self.numbers.get_mut(&num).unwrap().has_override = true;
                                    self.numbers
                                        .get_mut(&num)
                                        .unwrap()
                                        .levels
                                        .get_mut(&level)
                                        .unwrap()
                                        .start_override = Some(integer(&e, "val", part)?);
                                    Frame::Leaf
                                }
                                (Frame::Abstract(id), "multiLevelType") => {
                                    self.abstract_metadata.entry(id).or_default().single_level =
                                        attr(&e, "val") == Some("singleLevel");
                                    Frame::Metadata
                                }
                                (Frame::Abstract(id), "numStyleLink") => {
                                    let key = attr(&e, "val")
                                        .map(|value| self.intern_style(value, part, "numStyleLink"))
                                        .transpose()?;
                                    self.abstract_metadata.entry(id).or_default().style_link = key;
                                    Frame::Leaf
                                }
                                (Frame::Level(owner), "suff" | "lvlJc") => {
                                    let target = self.marker_level_mut(owner).unwrap();
                                    if e.name == "lvlJc" {
                                        use hwpforge_blueprint::ListMarkerAlign::*;
                                        target.align = Some(match attr(&e, "val") {
                                            None | Some("left" | "start") => Left,
                                            Some("center") => Center,
                                            Some("right" | "end") => Right,
                                            _ => {
                                                warn(
                                                    warnings,
                                                    part,
                                                    "lvlJc",
                                                    Kind::UnsupportedValue,
                                                    "list_marker_alignment",
                                                    "use_left",
                                                    "Unsupported marker alignment; using left",
                                                )?;
                                                Left
                                            }
                                        });
                                    } else {
                                        target.suffix = Some(match attr(&e, "val") {
                                            None | Some("tab") => super::lists::Suffix::Tab,
                                            Some("space") => super::lists::Suffix::Space,
                                            Some("nothing") => super::lists::Suffix::Nothing,
                                            _ => {
                                                warn(
                                                    warnings,
                                                    part,
                                                    "suff",
                                                    Kind::UnsupportedValue,
                                                    "list_marker_suffix",
                                                    "use_tab",
                                                    "Unsupported marker suffix; using tab",
                                                )?;
                                                super::lists::Suffix::Tab
                                            }
                                        });
                                    }
                                    Frame::Leaf
                                }
                                (Frame::Level(_), "pStyle") => Frame::Ignored,
                                (Frame::NumberTabs(owner), "tab") => {
                                    match attr(&e, "val") {
                                        Some("num" | "clear") => {
                                            let pos = attr(&e, "pos").ok_or_else(|| {
                                                DocxError::invalid(
                                                    part,
                                                    "tab",
                                                    "missing tab position",
                                                )
                                            })?;
                                            let pos = units::length(pos, 5, -100_000_000)
                                                .map_err(|r| DocxError::invalid(part, "tab", r))?;
                                            let previous = self
                                                .marker_level_mut(owner)
                                                .unwrap()
                                                .tabs
                                                .insert(pos, attr(&e, "val") == Some("num"));
                                            self.tab_count += usize::from(previous.is_none());
                                            limit(
                                                part,
                                                "tab",
                                                "number tabs",
                                                self.tab_count,
                                                MAX_DEFINITIONS,
                                            )?;
                                            if previous.is_some() {
                                                warn(
                                                    warnings,
                                                    part,
                                                    "tab",
                                                    Kind::ValueApproximated,
                                                    "list_marker_tab",
                                                    "use_last",
                                                    "Duplicate number tab position; using last declaration",
                                                )?;
                                            }
                                        }
                                        _ => definition_warning(
                                            &e,
                                            part,
                                            root,
                                            "unsupported number tab",
                                            warnings,
                                        )?,
                                    }
                                    Frame::NumberTab
                                }
                                // Numbering identity/edit-history metadata, not level formatting.
                                (Frame::Abstract(_), "nsid" | "tmpl" | "name")
                                | (Frame::Root, "numIdMacAtCleanup") => Frame::Metadata,
                                (
                                    Frame::Abstract(_) | Frame::Number(_),
                                    "numStyleLink" | "styleLink",
                                ) => {
                                    diagnosed = true;
                                    warn(
                                        warnings,
                                        part,
                                        &e.name,
                                        Kind::UnsupportedAttribute,
                                        "numbering_style_link",
                                        "ignore",
                                        "Numbering style link is not followed",
                                    )?;
                                    Frame::Ignored
                                }
                                _ => Frame::Ignored,
                            };
                        }
                        if let Frame::Properties(owner, _) | Frame::NumPr(owner) = parent {
                            let character = matches!(parent, Frame::Properties(_, true));
                            if w && e.name == "tabs"
                                && matches!(parent, Frame::Properties(_, false))
                                && self.marker_level_mut(owner).is_some()
                            {
                                super::container_attributes(&e, part, &e.name, warnings)?;
                                stack.push(Frame::NumberTabs(owner));
                                return Ok(());
                            }
                            let mut delta = Delta::default();
                            let state = read_property(
                                &mut delta,
                                &e,
                                character,
                                &self.theme,
                                &mut self.fonts,
                                part,
                                &e.name,
                                warnings,
                            )?;
                            frame = Frame::Property(state);
                            let marker = match owner {
                                Owner::Abstract(id, level) if character => {
                                    self.abstracts.get_mut(&id).and_then(|ls| ls.get_mut(&level))
                                }
                                Owner::Number(id, level) if character => {
                                    self.numbers.get_mut(&id).and_then(|n| n.levels.get_mut(&level))
                                }
                                _ => None,
                            };
                            if let Some(marker) = marker {
                                if matches!(state, PropertyState::Excluded) {
                                    warn(
                                        warnings,
                                        part,
                                        &e.name,
                                        Kind::FormattingOmitted,
                                        "marker_format",
                                        "omit",
                                        "Unsupported marker character property omitted",
                                    )?;
                                }
                                let mut marker_delta = Delta {
                                    char: marker.marker_char.clone(),
                                    ..Default::default()
                                };
                                marker_delta.overlay(&delta, false);
                                marker.marker_char = marker_delta.char;
                            } else if let Some(target) = self.target(owner) {
                                target.overlay(&delta, false);
                            }
                            if w && e.name == "numPr"
                                && matches!(parent, Frame::Properties(_, false))
                            {
                                frame = Frame::NumPr(owner);
                            }
                            if w && e.name == "sectPr" {
                                frame = Frame::Section;
                            }
                        }
                        if let Frame::Property(state) = parent {
                            frame = Frame::Property(state.child(&e, part, &e.name, warnings)?);
                        }
                        if matches!(frame, Frame::Properties(_, _)) {
                            super::container_attributes(&e, part, &e.name, warnings)?;
                        }
                    }
                    if matches {
                        if matches!(frame, Frame::Ignored) {
                            if !diagnosed
                                && !matches!(
                                    parent,
                                    Frame::Ignored
                                        | Frame::Properties(_, _)
                                        | Frame::NumPr(_)
                                        | Frame::Section
                                )
                            {
                                definition_warning(&e, part, root, "child subtree", warnings)?;
                            }
                        } else {
                            definition_attributes(frame, &e, part, root, warnings)?;
                        }
                    }
                    stack.push(frame);
                }
                XmlEvent::End => {
                    if let Some(Frame::Family(f, Some(hang))) = stack.pop() {
                        // Resolve at family end: explicit ea wins regardless of child order.
                        if self.theme.fonts[f][1].is_none() {
                            self.theme.fonts[f][1] = Some(hang);
                            hang_selected = true;
                        } else {
                            theme_script_warning(warnings, part)?;
                        }
                    }
                }
                XmlEvent::Text(_) => {}
            }
            Ok(())
        })?;
        if hang_selected {
            for warning in warnings
                .iter_mut()
                .filter(|w| w.loss() == "theme_script_font" && w.part() == Some(part))
            {
                if let DocxWarning::UnsupportedAttribute { reason, .. } = warning {
                    *reason = "Hang script font used for the Korean slot; other script-specific fonts are not selected".into();
                }
            }
        }
        Ok(matches)
    }
}
fn theme_script_warning(warnings: &mut Vec<DocxWarning>, part: &str) -> Result<()> {
    warn(
        warnings,
        part,
        "font",
        Kind::UnsupportedAttribute,
        "theme_script_font",
        "ignore",
        "Theme script-specific fonts are not selected",
    )
}
// Explicit attributes for admitted definition frames only. Unsupported subtrees are
// diagnosed once at their boundary and never acquire an owner from descendant names.
fn definition_attributes(
    frame: Frame,
    e: &Element,
    part: &str,
    root: &str,
    warnings: &mut Vec<DocxWarning>,
) -> Result<()> {
    let (consumed, ignored): (&[&str], &[&str]) = match frame {
        Frame::Root if root == "theme" => (&[], &["name"]), // Display label only.
        Frame::Root
        | Frame::Defaults
        | Frame::DefaultRun
        | Frame::DefaultPara
        | Frame::CellProperties
        | Frame::ThemeElements
        | Frame::Family(_, _) => (&[], &[]),
        Frame::Style(_) => (&["type", "styleId", "default"], &["customStyle"]), // Authoring provenance only.
        Frame::Table(_) => (&["type"], &[]),
        Frame::Abstract(_) => (&["abstractNumId"], &[]),
        Frame::Number(_) => (&["numId"], &[]),
        Frame::Override { .. } => (&["ilvl"], &[]),
        Frame::Level(_) => (&["ilvl"], &["tplc", "tentative"]), // Gallery/template provenance only.
        Frame::FontScheme => (&[], &["name"]),                  // Display label only.
        Frame::LatentStyles => (
            &[],
            &[
                "count",
                "defLockedState",
                "defUIPriority",
                "defSemiHidden",
                "defUnhideWhenUsed",
                "defQFormat",
            ],
        ), // UI defaults for styles not present in the document.
        Frame::Metadata if e.name == "lsdException" => {
            (&[], &["name", "locked", "uiPriority", "semiHidden", "unhideWhenUsed", "qFormat"])
        } // UI visibility/priority, not effective formatting.
        Frame::Metadata => (&[], &["val"]), // Style editor links/labels or numbering edit-history IDs.
        Frame::NumberTabs(_) => (&[], &[]),
        Frame::NumberTab => (&["val", "pos"], &[]),
        Frame::Leaf if root == "theme" => (&["typeface"], &[]),
        Frame::Leaf => (&["val"], &[]),
        Frame::Ignored
        | Frame::Property(_)
        | Frame::Properties(_, _)
        | Frame::NumPr(_)
        | Frame::Section
        | Frame::TableProperties(_, _) => return Ok(()), // Property reader or subtree loss already accounts for these.
    };
    let namespace = if root == "theme" { xml::NamespaceId::NONE } else { W_NS.0 };
    for (ns, key, _) in &e.attributes {
        if *ns != namespace
            || !(consumed.contains(&key.as_str()) || ignored.contains(&key.as_str()))
        {
            definition_warning(e, part, root, format_args!("attribute '{}'", key), warnings)?;
        }
    }
    let flags: &[&str] = match frame {
        Frame::Style(_) => &["customStyle"], // default is consumed while constructing the Style.
        Frame::Level(_) => &["tentative"],
        Frame::LatentStyles => {
            &["defLockedState", "defSemiHidden", "defUnhideWhenUsed", "defQFormat"]
        }
        Frame::Metadata if e.name == "lsdException" => {
            &["locked", "semiHidden", "unhideWhenUsed", "qFormat"]
        }
        Frame::Metadata
            if matches!(
                e.name.as_str(),
                "qFormat"
                    | "semiHidden"
                    | "unhideWhenUsed"
                    | "locked"
                    | "personal"
                    | "personalCompose"
                    | "personalReply"
                    | "autoRedefine"
            ) =>
        {
            &["val"]
        }
        _ => &[],
    };
    for key in flags {
        properties::boolean(e, key, None, part, &e.name, warnings)?;
    }
    Ok(())
}
fn definition_warning(
    e: &Element,
    part: &str,
    root: &str,
    detail: impl std::fmt::Display,
    warnings: &mut Vec<DocxWarning>,
) -> Result<()> {
    let loss = match root {
        "styles" => "style_definition",
        "numbering" => "numbering_definition",
        _ => "theme_definition",
    };
    record(warnings, Kind::UnsupportedAttribute, loss, "ignore", || {
        (
            Some(part),
            Some(e.name.as_str()),
            format!("Unsupported definition {} on '{}'", quote(detail), quote(&e.name)),
        )
    })
}

fn integer(e: &Element, key: &str, part: &str) -> Result<u32> {
    let value = attr(e, key)
        .ok_or_else(|| DocxError::invalid(part, &e.name, format!("invalid or missing {key}")))?;
    crate::identifier::check(value, part, &e.name)?;
    value
        .parse()
        .map_err(|_| DocxError::invalid(part, &e.name, format!("invalid or missing {key}")))
}

#[cfg(test)]
#[path = "definitions_tests.rs"]
mod tests;
