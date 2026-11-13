use super::*;
use hwpforge_blueprint::{
    registry::StyleRegistry,
    style::{ParaShape, PartialCharShape},
    EastAsianAutoSpacing, ListMarkerAlign, ListMarkerLayout, ListTextStart,
};
use hwpforge_core::{paragraph::Paragraph, ParagraphListRef};
use hwpforge_foundation::{
    Alignment, CharShapeIndex, Color, FontId, FontIndex, HwpUnit, LineSpacingType, ParaShapeIndex,
    StrikeoutShape, StyleRef, UnderlineType, VerticalPosition,
};
use std::{
    collections::{hash_map::RandomState, HashMap, HashSet},
    hash::BuildHasher,
};

mod marker_messages {
    pub(super) const INDENT: &str = "List indent cannot be projected to a nonnegative marker slot";
    pub(super) const ZERO_SLOT: &str = "Zero marker slot retains the existing list layout";
    pub(super) const TAB: &str =
        "Number tab at or before the anchor cannot select the first-line target";
    pub(super) const LINE_HEIGHT: &str =
        "Marker font or size changes can affect line height; body line height retained";
    pub(super) const DEFAULT_TAB: &str = concat!(
        "Default tab selected from marker alignment and measured width ratios where ",
        "needed; ratios are reused across fonts and styles. The tab grid assumes the ",
        "local text-area left edge; cells, columns and zero-hanging center alignment ",
        "are unmeasured.",
    );
    pub(super) const TAB_BOUNDARY: &str = concat!(
        "Estimated marker end is less than 1 pt before the selected tab; width ",
        "approximation may select an adjacent tab.",
    );
    pub(super) const DEFAULT_CONTINUATION: &str = concat!(
        "First-line target selected; Word continuation lines remain at the anchor and ",
        "are approximated.",
    );
    pub(super) const SPACE_WIDTH: &str =
        "Space advance approximated by measured ratio of marker size";
    pub(super) const CONTINUATION: &str =
        "First-line target selected; independent continuation position is approximated";
    pub(super) const OVERFLOW: &str =
        "Nominal marker width exceeds the fixed slot; overlap is possible";
    pub(super) const SHARED_OVERFLOW: &str =
        "Estimated marker end exceeds the shared first-use slot; overlap is possible";
}

#[derive(Clone, Default, PartialEq, Eq, Hash)]
struct RunInput {
    delta: Delta,
    style: Option<StyleKey>,
}
#[derive(Clone, Copy)]
struct CharResult {
    id: CharShapeIndex,
    font: FontIndex,
    missing: u8,
    collapsed: bool,
    reference_missing: bool,
    line_fonts: [Option<FontKey>; 2],
    size: i32,
}

pub(crate) struct Resolver {
    pub(crate) definitions: Definitions,
    pub(crate) registry: StyleRegistry,
    inherited: HashMap<StyleKey, (Delta, usize)>,
    inherited_tables: HashMap<StyleKey, table::TableProperties>,
    named: HashMap<StyleKey, StyleRef>,
    default_paragraph: Option<StyleKey>,
    default_table: Option<StyleKey>,
    pub(crate) table_style: Option<StyleKey>,
    deltas: HashMap<RunInput, usize>,
    inputs: Vec<RunInput>,
    contexts: HashMap<(Delta, CharDelta), usize>,
    combinations: HashMap<(usize, usize), CharResult>,
    chars: HashMap<u64, Vec<(CharKey, CharShapeIndex)>>,
    marks: HashMap<(usize, usize), CharResult>,
    fallback_font: FontKey,
    unknown_metrics: HashSet<FontKey>,
    paras: HashMap<u64, Vec<ParaShapeIndex>>,
    #[cfg(test)]
    para_comparisons: std::cell::Cell<usize>,
    fonts: HashMap<FontKey, FontIndex>,
    hasher: RandomState,
    owned_font_bytes: usize,
    lists: lists::Lists,
    pub(crate) default_tab_stop: i32,
    // Each marker loss below has a fixed kind, action and reason.
    marker_warnings: HashSet<(lists::ListRef, &'static str)>,
    // Mirror the shared head's first candidate, including an unspecified request.
    // This follows the first-use contract documented on Blueprint
    // `ListMarkerLayout`; it is diagnostic only and is checked for zero-hanging
    // default-tab items, so other items sharing the head are not re-checked.
    first_marker_layouts: HashMap<lists::ListRef, Option<ListMarkerLayout>>,
    #[cfg(test)]
    combinations_built: usize,
    #[cfg(test)]
    char_shape_calls: usize,
    #[cfg(test)]
    copied_font_bytes: usize,
}
type CharKey = ([FontKey; 3], i32, [bool; 3], u32, bool, Option<u32>, u8);

impl Resolver {
    pub(crate) fn new(
        mut definitions: Definitions,
        warnings: &mut Vec<DocxWarning>,
    ) -> Result<Self> {
        let fallback_font = definitions.fonts.fallback()?;
        definitions.warn_numbering_links(warnings)?;
        let lists = lists::Lists::new(&definitions)?;
        let mut r = Self {
            definitions,
            registry: StyleRegistry::with_fonts(vec![]),
            inherited: HashMap::new(),
            inherited_tables: HashMap::new(),
            named: HashMap::new(),
            default_paragraph: None,
            default_table: None,
            table_style: None,
            deltas: HashMap::new(),
            inputs: vec![],
            contexts: HashMap::new(),
            combinations: HashMap::new(),
            chars: HashMap::new(),
            marks: HashMap::new(),
            fallback_font,
            unknown_metrics: HashSet::new(),
            paras: HashMap::new(),
            #[cfg(test)]
            para_comparisons: Default::default(),
            fonts: HashMap::new(),
            hasher: RandomState::new(),
            owned_font_bytes: 0,
            lists,
            default_tab_stop: 3600,
            marker_warnings: HashSet::new(),
            first_marker_layouts: HashMap::new(),
            #[cfg(test)]
            combinations_built: 0,
            #[cfg(test)]
            char_shape_calls: 0,
            #[cfg(test)]
            copied_font_bytes: 0,
        };
        let ids = r.definitions.ordered_styles();
        for id in &ids {
            r.inherit(*id, &mut Vec::new(), warnings)?;
        }
        for id in ids {
            let Some(s) = r.definitions.style(id) else { continue };
            if s.kind == StyleKind::Table && s.default && r.default_table.replace(id).is_some() {
                return Err(DocxError::invalid(&s.part, &s.at, "multiple default table styles"));
            }
            if s.kind != StyleKind::Paragraph {
                continue;
            }
            if s.default {
                if r.default_paragraph.is_some() {
                    return Err(DocxError::invalid(
                        &s.part,
                        &s.at,
                        "multiple default paragraph styles",
                    ));
                }
                r.default_paragraph = Some(id);
            }
            let (part, at, name) = (s.part.clone(), s.at.clone(), s.name.clone());
            let mut effective = r.context(None, Some(id), &Delta::default(), &part, warnings)?;
            let c =
                r.character(&effective, &Delta::default(), &RunInput::default(), &part, warnings)?;
            if effective.para.line.is_some() && effective.para.line_rule == Some(2) {
                // Definitions have no text: use the empty paragraph mark policy.
                // Body paragraphs instead select script slots from actual characters.
                let east = c.line_fonts[1].is_some();
                let mut height = line_height::Height::default();
                r.add_height(&mut height, c, east, &part, warnings)?;
                effective.para.projected_line = height.project(&effective.para, east)?;
            }
            let p = r.para_shape(&effective)?;
            format_warnings(
                c.missing | para_missing(&effective.para),
                c.collapsed,
                &part,
                &at,
                warnings,
            )?;
            let font = c.font;
            let key = format!("import_p_{:06}", r.named.len());
            let reference = r
                .registry
                .register_style(key, c.id, p, font)
                .map_err(|e| DocxError::invalid(&part, &at, quote(e)))?;
            if let Some(name) = name {
                r.registry
                    .set_style_display_name(reference, name)
                    .map_err(|e| DocxError::invalid(&part, &at, quote(e)))?;
            }
            r.named.insert(id, reference);
        }
        Ok(r)
    }
    fn inherit(
        &mut self,
        id: StyleKey,
        path: &mut Vec<StyleKey>,
        warnings: &mut Vec<DocxWarning>,
    ) -> Result<(Delta, usize)> {
        let s = self
            .definitions
            .style(id)
            .ok_or_else(|| DocxError::invalid("#format", "style", "missing admitted style"))?;
        if let Some((d, height)) = self.inherited.get(&id) {
            limit(&s.part, &s.at, "style inheritance depth", path.len() + height, MAX_INHERITANCE)?;
            return Ok((d.clone(), *height));
        }
        limit(&s.part, &s.at, "style inheritance depth", path.len() + 1, MAX_INHERITANCE)?;
        let s = s.clone();
        path.push(id);
        let mut result = Delta::default();
        let mut table = table::TableProperties::default();
        let mut height = 1;
        if let Some(parent) = s.parent {
            let valid = self.definitions.style(parent).is_some_and(|p| p.kind == s.kind);
            if valid && !path.contains(&parent) {
                let (base, h) = self.inherit(parent, path, warnings)?;
                result = base;
                table = self.inherited_tables[&parent].clone();
                height = h + 1;
            } else {
                warn(
                    warnings,
                    &s.part,
                    &s.at,
                    Kind::StyleReferenceLost,
                    "style_parent",
                    "ignore_link",
                    "Missing, incompatible or cyclic basedOn connection ignored",
                )?;
            }
        }
        apply(&mut result, &s.delta, false, &s.part, &s.at, warnings)?;
        path.pop();
        table.overlay(&s.table);
        self.inherited_tables.insert(id, table);
        self.inherited.insert(id, (result.clone(), height));
        Ok((result, height))
    }
    pub(crate) fn property(
        &mut self,
        d: &mut Delta,
        e: &Element,
        character: bool,
        part: &str,
        at: &str,
        w: &mut Vec<DocxWarning>,
    ) -> Result<PropertyState> {
        read_property(
            d,
            e,
            character,
            &self.definitions.theme,
            &mut self.definitions.fonts,
            part,
            at,
            w,
        )
    }
    pub(crate) fn run_input(
        &mut self,
        delta: Delta,
        style: Option<StyleKey>,
    ) -> Result<CharShapeIndex> {
        let input = RunInput { delta, style };
        if let Some(id) = self.deltas.get(&input) {
            return Ok(CharShapeIndex::new(*id));
        }
        limit("#format", "direct", "direct formatting deltas", self.inputs.len() + 1, MAX_SHAPES)?;
        let id = self.inputs.len();
        self.inputs.push(input.clone());
        self.deltas.insert(input, id);
        Ok(CharShapeIndex::new(id))
    }
    fn style(
        &self,
        id: Option<StyleKey>,
        kind: StyleKind,
        part: &str,
        w: &mut Vec<DocxWarning>,
    ) -> Result<Option<Delta>> {
        let Some(id) = id else { return Ok(None) };
        if self.definitions.style(id).is_some_and(|s| s.kind == kind) {
            Ok(Some(self.inherited[&id].0.clone()))
        } else {
            warn(
                w,
                part,
                "style",
                Kind::StyleReferenceLost,
                "style_reference",
                "ignore_link",
                "Missing or incompatible style",
            )?;
            Ok(None)
        }
    }
    pub(crate) fn empty_properties() -> Delta {
        Delta {
            para: ParaDelta {
                num_id: Some(0),
                outline: Some(9),
                before: Some(0),
                after: Some(0),
                first: Some(0),
                left: Some(0),
                right: Some(0),
                page_break: Some(false),
                keep_next: Some(false),
                keep_lines: Some(false),
                contextual: Some(false),
                ..ParaDelta::default()
            },
            ..Delta::default()
        }
    }
    pub(crate) fn table_properties(
        &self,
        id: Option<StyleKey>,
        part: &str,
        w: &mut Vec<DocxWarning>,
    ) -> Result<(Option<StyleKey>, table::TableProperties)> {
        let key = id.or(self.default_table);
        let valid = self.style(key, StyleKind::Table, part, w)?.is_some();
        let key = if valid { key } else { self.default_table };
        Ok((key, key.and_then(|k| self.inherited_tables.get(&k)).cloned().unwrap_or_default()))
    }
    fn context(
        &self,
        table: Option<StyleKey>,
        style: Option<StyleKey>,
        direct: &Delta,
        part: &str,
        w: &mut Vec<DocxWarning>,
    ) -> Result<Delta> {
        let mut result = self.definitions.defaults.clone();
        if let Some(d) = self.style(table, StyleKind::Table, part, w)? {
            apply(&mut result, &d, true, part, "style", w)?;
        }
        let paragraph = self.style(style, StyleKind::Paragraph, part, w)?;
        let mut selection = result.clone();
        if let Some(d) = &paragraph {
            selection.overlay(d, false);
        }
        selection.overlay(direct, false);
        let mut number_indent = ParaDelta::default();
        if let Some(num) = selection.para.num_id.filter(|id| *id != 0) {
            let level = self.definitions.effective_level(num, selection.para.level.unwrap_or(0));
            let number = self.definitions.numbers.get(&num);
            let base = number
                .and_then(|n| n.abstract_id)
                .and_then(|id| self.definitions.numbering_base(id))
                .map(|(_, levels)| levels)
                .and_then(|levels| levels.get(&level));
            let over = number.and_then(|n| n.levels.get(&level));
            if let Some(d) = base {
                apply(&mut result, &d.delta, true, part, "numPr", w)?;
                number_indent.overlay_indents(&d.delta.para);
            }
            if let Some(d) = over {
                apply(&mut result, &d.delta, true, part, "numPr", w)?;
                number_indent.overlay_indents(&d.delta.para);
            }
        }
        if let Some(d) = paragraph {
            apply(&mut result, &d, true, part, "style", w)?;
        }
        // Word places directly numbered paragraphs using the numbering level's
        // indents ahead of paragraph-style indents: left/hanging 440/440 and
        // 880/440 twips yield marker anchors 0 and 440, even with style left 720.
        // With a 72 pt page margin, measured marker/text positions are 72/94
        // and 94/116 pt. Only w:ind fields are reapplied; firstLine/hanging
        // share `first`.
        // Direct pPr remains last. Style-inherited numPr is unmeasured and keeps
        // the existing numbering-before-paragraph-style order.
        // Unmeasured: direct ilvl with a style numId inherits; matching style/direct numId is direct.
        if direct.para.num_id.is_some() {
            result.para.overlay_indents(&number_indent);
        }
        apply(&mut result, direct, false, part, "pPr", w)?;
        Ok(result)
    }
    pub(crate) fn finish(
        &mut self,
        paragraph: (&mut Paragraph, &[bool], Option<usize>),
        style: Option<StyleKey>,
        direct: &Delta,
        mark: Option<(&Delta, Option<StyleKey>)>,
        part: &str,
        w: &mut Vec<DocxWarning>,
    ) -> Result<(Delta, i32)> {
        let (p, diagnose, fragment_end) = paragraph;
        let measure_lines = fragment_end.is_some();
        let fragment_end = fragment_end.unwrap_or(0);
        let style = style.or(self.default_paragraph);
        let mut base = self.context(self.table_style, style, direct, part, w)?;
        p.style_ref = style.as_ref().and_then(|s| self.named.get(s)).copied();

        p.page_break = base.para.page_break.unwrap_or(false);
        format_warnings(para_missing(&base.para), false, part, "pPr", w)?;
        // character() reapplies direct character values after the style toggle.
        let context_key = (base.clone(), direct.char.clone());
        let next = self.contexts.len();
        let context = if let Some(&id) = self.contexts.get(&context_key) {
            id
        } else {
            limit(part, "pPr", "format contexts", next + 1, MAX_SHAPES)?;
            self.contexts.insert(context_key, next);
            next
        };
        let exact = base.para.line.is_some() && base.para.line_rule == Some(1);
        let project_minimum = base.para.line.is_some() && base.para.line_rule == Some(2);
        let measure_height = project_minimum || measure_lines;
        let mut height = line_height::Height::default();
        let mut line_height = line_height::Height::default();
        let mut line_east = false;
        let mut line_used = false;
        let mut line_format = None;
        let mut minimum_height = 0_i32;
        let mut east = false;
        let empty = p.runs.iter().all(|run| run.content.as_text().is_none_or(str::is_empty));
        let last_run = p.runs.len().saturating_sub(1);
        for (index, run) in p.runs.iter_mut().enumerate() {
            let diagnose = diagnose.get(index).copied().unwrap_or(true);
            let input_id = run.char_shape_id.get();
            let key = (context, input_id);
            let result = if let Some(c) = self.combinations.get(&key) {
                if diagnose {
                    reference_warning(*c, part, w)?;
                }
                *c
            } else {
                limit(part, "rPr", "format combinations", self.combinations.len() + 1, MAX_SHAPES)?;
                #[cfg(test)]
                {
                    self.combinations_built += 1;
                }
                let input = self.inputs[input_id].clone();
                let result = self.character(&base, direct, &input, part, w)?;
                self.combinations.insert(key, result);
                result
            };
            run.char_shape_id = result.id;
            // Empty source/boundary runs diagnose formatting but cannot supply
            // script evidence in a nonempty paragraph. For an empty paragraph,
            // only its explicit mark or final effective run supplies the height.
            let empty_mark = empty && mark.is_none() && index == last_run;
            if !exact
                && measure_height
                && (empty_mark || run.content.as_text().is_some_and(|t| !t.is_empty()))
            {
                let text = run.content.as_text().unwrap_or("");
                let has_east = text.chars().any(line_height::east_asian);
                let has_latin = text.chars().any(|c| !line_height::east_asian(c));
                let empty_east = empty_mark && result.line_fonts[1].is_some();
                east |= has_east || empty_east;
                if has_east || empty_east {
                    self.add_height(&mut height, result, true, part, w)?;
                }
                if has_latin || (!has_east && !empty_east) {
                    self.add_height(&mut height, result, false, part, w)?;
                }
            }
            if measure_lines && index < fragment_end {
                line_format = Some(result);
                let text = run.content.as_text().unwrap_or("");
                let mut segments = text.split('\n').peekable();
                while let Some(text) = segments.next() {
                    let ended = segments.peek().is_some();
                    if !exact && (!text.is_empty() || ended) {
                        let has_east = text.chars().any(line_height::east_asian);
                        let empty_east = text.is_empty() && result.line_fonts[1].is_some();
                        line_east |= has_east || empty_east;
                        if has_east || empty_east {
                            self.add_height(&mut line_height, result, true, part, w)?;
                        }
                        if text.chars().any(|c| !line_height::east_asian(c))
                            || (!has_east && !empty_east)
                        {
                            self.add_height(&mut line_height, result, false, part, w)?;
                        }
                        line_used = true;
                    }
                    if ended {
                        minimum_height = minimum_height
                            .checked_add(line_height.word_line(&base.para, line_east)?)
                            .filter(|h| *h <= HwpUnit::MAX_VALUE)
                            .ok_or_else(|| {
                                DocxError::invalid(part, "p", "cell line height overflow")
                            })?;
                        line_height = line_height::Height::default();
                        line_east = false;
                        line_used = false;
                    }
                }
            }
            if diagnose {
                format_warnings(result.missing, result.collapsed, part, "rPr", w)?;
            }
        }
        // Without pPr/rPr the final effective run already supplies the mark's
        // height. Do not invent a separate default-font mark above that run.
        if let Some((mark, style)) = mark {
            let delta = self.run_input(mark.clone(), style)?.get();
            let key = (context, delta);
            let result = if let Some(&result) = self.marks.get(&key) {
                reference_warning(result, part, w)?;
                result
            } else {
                limit(
                    part,
                    "rPr",
                    "paragraph mark combinations",
                    self.marks.len() + 1,
                    MAX_SHAPES,
                )?;
                let input = self.inputs[delta].clone();
                let result = self.character(&base, direct, &input, part, w)?;
                self.marks.insert(key, result);
                result
            };
            p.end_char_shape = Some(result.id);
            format_warnings(result.missing, result.collapsed, part, "rPr", w)?;
            // An unresolved explicit mark makes the entire height unverified.
            if !exact && measure_height {
                east |= empty && result.line_fonts[1].is_some();
                self.add_height(&mut height, result, east, part, w)?;
                if measure_lines && fragment_end == p.runs.len() {
                    line_east |= !line_used && result.line_fonts[1].is_some();
                    self.add_height(&mut line_height, result, line_east, part, w)?;
                    line_used = true;
                }
            }
        }
        if project_minimum {
            base.para.projected_line = height.project(&base.para, east)?;
        }
        if !exact && measure_height && height.mixed() {
            warn(w, part, "pPr", Kind::ValueApproximated, "mixed_font_line_height",
                "approximate", "Paragraph-wide ascent/descent maxima approximate mixed fonts or sizes without line breaking")?;
        }
        base.para.projected_list =
            self.lists.resolve(&self.definitions, &mut self.registry, &base.para, part, w)?;
        base.para.projected_marker = self.marker_layout(&base, direct, mark, part, w)?;
        if measure_lines {
            // A trailing empty line uses the paragraph mark, or the final effective
            // run at this boundary. Page/column breaks exclude all later fragments.
            if !exact && !line_used {
                if let Some(result) = line_format {
                    line_east = result.line_fonts[1].is_some();
                    self.add_height(&mut line_height, result, line_east, part, w)?;
                }
            }
            minimum_height = minimum_height
                .checked_add(line_height.word_line(&base.para, line_east)?)
                .filter(|h| *h <= HwpUnit::MAX_VALUE)
                .ok_or_else(|| DocxError::invalid(part, "p", "cell line height overflow"))?;
        }
        Ok((base, minimum_height))
    }
    fn marker_layout(
        &mut self,
        base: &Delta,
        direct: &Delta,
        mark: Option<(&Delta, Option<StyleKey>)>,
        part: &str,
        w: &mut Vec<DocxWarning>,
    ) -> Result<Option<ListMarkerLayout>> {
        if !matches!(
            base.para.projected_list,
            Some(lists::ListRef::Number(..) | lists::ListRef::Bullet(..))
        ) {
            return Ok(None);
        }
        let group = base.para.projected_list.expect("resolved marker has its output group");
        // One bullet definition owns the marker across all paragraph nesting levels.
        let group = match group {
            lists::ListRef::Bullet(id, _) => lists::ListRef::Bullet(id, 0),
            group => group,
        };
        let (source, tabs) = self
            .definitions
            .marker_level(&base.para)
            .expect("resolved marker has its source level");
        let align = source.align.unwrap_or(ListMarkerAlign::Left);
        let suffix = source.suffix.unwrap_or_default();
        let mut marker_char = source.marker_char.clone();
        // Symbol-font input selects the Unicode glyph; the output uses ordinary paragraph fonts.
        if source.mapped_bullet() {
            marker_char.fonts = [None; 4];
        }
        let left = base.para.left.unwrap_or(0);
        let first = base.para.first.unwrap_or(0);
        if first > 0 {
            if self.marker_warnings.insert((group, "list_marker_indent")) {
                warn(
                    w,
                    part,
                    "ind",
                    Kind::ValueApproximated,
                    "list_marker_indent",
                    "retain_indent",
                    marker_messages::INDENT,
                )?;
            }
            self.first_marker_layouts.entry(group).or_insert(None);
            return Ok(None);
        }
        // A nonpositive first line and A < tab < left keep the target at or after A.
        let anchor = marker_anchor(left, first)?;
        let target = tabs
            .iter()
            .filter(|(pos, num)| **num && **pos > anchor && **pos < left)
            .map(|(&pos, _)| pos)
            .min()
            .unwrap_or(left);
        let default_tab =
            first == 0 && suffix == lists::Suffix::Tab && !tabs.values().any(|&number| number);
        let ratio = if default_tab {
            match group {
                lists::ListRef::Bullet(..)
                    if source.measured_symbol_bullet(&self.definitions.fonts) =>
                {
                    Some(508)
                }
                lists::ListRef::Number(id, index) => self
                    .lists
                    .current(&self.definitions, &base.para)
                    .map(|n| {
                        measured_number_ratio(
                            &self.registry.numberings[id].levels[usize::from(index)],
                            index,
                            n,
                        )
                    })
                    .transpose()?
                    .flatten(),
                _ => None,
            }
        } else {
            None
        };
        if suffix == lists::Suffix::Tab && target == anchor && ratio.is_none() {
            if self.marker_warnings.insert((group, "list_marker_zero_slot")) {
                warn(
                    w,
                    part,
                    "ind",
                    Kind::ValueApproximated,
                    "list_marker_zero_slot",
                    "retain_indent",
                    marker_messages::ZERO_SLOT,
                )?;
            }
            self.first_marker_layouts.entry(group).or_insert(None);
            return Ok(None);
        }
        if tabs.iter().any(|(&pos, &num)| num && pos <= anchor)
            && self.marker_warnings.insert((group, "list_marker_tab"))
        {
            warn(
                w,
                part,
                "tab",
                Kind::ValueApproximated,
                "list_marker_tab",
                "use_indent",
                marker_messages::TAB,
            )?;
        }
        let mut effective = base.clone();
        if let Some((delta, style)) = mark {
            if let Some(d) = self.style(style, StyleKind::Character, part, w)? {
                effective.overlay(&d, true);
            }
            effective.overlay(direct, false);
            effective.overlay(delta, false);
        }
        let body = self.char_shape(&effective)?;
        effective.overlay(&Delta { char: marker_char, ..Default::default() }, false);
        let marker = self.char_shape(&effective)?;
        if marker.collapsed && self.marker_warnings.insert((group, "list_marker_script")) {
            format_warnings(0, true, part, "numPr", w)?;
        }
        let marker_shape = &self.registry.char_shapes[marker.id.get()];
        let body_shape = &self.registry.char_shapes[body.id.get()];
        if (marker.size > body.size
            || marker.font != body.font
            || marker_shape.font_latin != body_shape.font_latin)
            && self.marker_warnings.insert((group, "list_marker_line_height"))
        {
            warn(
                w,
                part,
                "rPr",
                Kind::ValueApproximated,
                "list_marker_line_height",
                "retain_body_line_height",
                marker_messages::LINE_HEIGHT,
            )?;
        }
        let selected = if let Some(ratio) = ratio {
            let width = measured_marker_width(marker.size, ratio)?;
            if let Some(Some(ListMarkerLayout {
                align: selected_align,
                text_start: ListTextStart::FromAnchor(selected_gap),
                ..
            })) = self.first_marker_layouts.get(&group)
            {
                let end = match selected_align {
                    ListMarkerAlign::Left => i64::from(width) * 2,
                    ListMarkerAlign::Center => i64::from(width),
                    ListMarkerAlign::Right => 0,
                    _ => return Err(marker_arithmetic()),
                };
                if end > i64::from(selected_gap.as_i32()) * 2
                    && self.marker_warnings.insert((group, "list_marker_overflow"))
                {
                    warn(
                        w,
                        part,
                        "numPr",
                        Kind::ValueApproximated,
                        "list_marker_overflow",
                        "retain_slot",
                        marker_messages::SHARED_OVERFLOW,
                    )?;
                }
            }
            let (gap, boundary) = default_tab_gap(anchor, width, self.default_tab_stop, align)?;
            if self.marker_warnings.insert((group, "list_marker_default_tab")) {
                warn(
                    w,
                    part,
                    "numPr",
                    Kind::ValueApproximated,
                    "list_marker_default_tab",
                    "use_measured_ratio",
                    marker_messages::DEFAULT_TAB,
                )?;
            }
            if boundary && self.marker_warnings.insert((group, "list_marker_tab_boundary")) {
                warn(
                    w,
                    part,
                    "numPr",
                    Kind::ValueApproximated,
                    "list_marker_tab_boundary",
                    "retain_selected_tab",
                    marker_messages::TAB_BOUNDARY,
                )?;
            }
            if gap > 0 && self.marker_warnings.insert((group, "list_marker_continuation")) {
                warn(
                    w,
                    part,
                    "numPr",
                    Kind::ValueApproximated,
                    "list_marker_continuation",
                    "match_first_line",
                    marker_messages::DEFAULT_CONTINUATION,
                )?;
            }
            Some(ListTextStart::FromAnchor(unit(gap)?))
        } else {
            None
        };
        let text_start = if let Some(selected) = selected {
            selected
        } else {
            match suffix {
                lists::Suffix::Space => {
                    if self.marker_warnings.insert((group, "list_marker_space_width")) {
                        warn(
                            w,
                            part,
                            "suff",
                            Kind::ValueApproximated,
                            "list_marker_space_width",
                            "use_measured_ratio",
                            marker_messages::SPACE_WIDTH,
                        )?;
                    }
                    let distance = i64::from(marker.size)
                        .checked_mul(306)
                        .and_then(|n| n.checked_add(550))
                        .ok_or_else(|| DocxError::invalid(part, "suff", "space width overflow"))?
                        / 1100;
                    ListTextStart::AfterMarker(unit(
                        i32::try_from(distance).map_err(|_| {
                            DocxError::invalid(part, "suff", "space width overflow")
                        })?,
                    )?)
                }
                lists::Suffix::Nothing => ListTextStart::AfterMarker(HwpUnit::ZERO),
                lists::Suffix::Tab => ListTextStart::FromAnchor(unit(
                    i32::try_from(
                        i64::from(target)
                            .checked_sub(i64::from(anchor))
                            .ok_or_else(|| DocxError::invalid(part, "ind", "slot overflow"))?,
                    )
                    .map_err(|_| DocxError::invalid(part, "ind", "slot overflow"))?,
                )?),
            }
        };
        if selected.is_none()
            && (suffix != lists::Suffix::Tab || target < left)
            && self.marker_warnings.insert((group, "list_marker_continuation"))
        {
            warn(
                w,
                part,
                "numPr",
                Kind::ValueApproximated,
                "list_marker_continuation",
                "match_first_line",
                marker_messages::CONTINUATION,
            )?;
        }
        if let (
            None,
            ListTextStart::FromAnchor(g),
            Some(n),
            Some(lists::ListRef::Number(id, index)),
        ) = (
            selected,
            text_start,
            self.lists.current(&self.definitions, &base.para),
            base.para.projected_list,
        ) {
            let head = &self.registry.numberings[id].levels[usize::from(index)];
            if head.num_format == hwpforge_foundation::NumberFormatType::Digit {
                let own = format!("^{}", u32::from(index) + 1);
                if let Some((prefix, suffix)) = head.text.split_once(&own) {
                    if prefix.is_ascii()
                        && suffix.is_ascii()
                        && !prefix.contains('^')
                        && !suffix.contains('^')
                    {
                        let count = u64::from(n.checked_ilog10().unwrap_or(0))
                            + 1
                            + u64::try_from(prefix.len() + suffix.len()).map_err(|_| {
                                DocxError::invalid(part, "numPr", "marker length overflow")
                            })?;
                        let width = count
                            .checked_mul(u64::try_from(marker.size).map_err(|_| {
                                DocxError::invalid(part, "numPr", "negative marker size")
                            })?)
                            .ok_or_else(|| {
                                DocxError::invalid(part, "numPr", "nominal marker width overflow")
                            })?;
                        let slot = i64::from(g.as_i32())
                            .checked_mul(if align == ListMarkerAlign::Center { 2 } else { 1 })
                            .ok_or_else(|| DocxError::invalid(part, "numPr", "slot overflow"))?;
                        if width
                            > u64::try_from(slot)
                                .map_err(|_| DocxError::invalid(part, "numPr", "negative slot"))?
                                .checked_mul(2)
                                .ok_or_else(|| DocxError::invalid(part, "numPr", "slot overflow"))?
                            && self.marker_warnings.insert((group, "list_marker_overflow"))
                        {
                            let reason = marker_messages::OVERFLOW;
                            warn(
                                w,
                                part,
                                "numPr",
                                Kind::ValueApproximated,
                                "list_marker_overflow",
                                "retain_slot",
                                reason,
                            )?;
                        }
                    }
                }
            }
        }
        let mut layout = ListMarkerLayout::new(align, text_start)
            .map_err(|e| DocxError::invalid(part, "numPr", quote(e)))?;
        layout.char_shape = Some(marker.id);
        self.first_marker_layouts.entry(group).or_insert(Some(layout));
        Ok(Some(layout))
    }
    pub(crate) fn paragraph_style(&self, style: Option<StyleKey>) -> Option<StyleKey> {
        style.or(self.default_paragraph)
    }
    fn add_height(
        &mut self,
        height: &mut line_height::Height,
        c: CharResult,
        east: bool,
        part: &str,
        w: &mut Vec<DocxWarning>,
    ) -> Result<()> {
        let key = if east {
            c.line_fonts[1].or(c.line_fonts[0])
        } else {
            c.line_fonts[0].or(c.line_fonts[1])
        };
        let metrics = key.and_then(|key| self.definitions.fonts.metrics(key));
        if let Some(key) = key.filter(|_| metrics.is_none()) {
            if self.unknown_metrics.insert(key) {
                warn(w, part, "rFonts", Kind::ValueApproximated, "unknown_font_line_height",
                    "retain_spacing", format_args!("Unknown natural line metrics for '{}'; cell minimum or specified minimum uses fallback metrics", self.definitions.fonts.name(key)))?;
            }
        }
        height.add(c.size, metrics);
        Ok(())
    }
    pub(crate) fn spacing_after(paragraph: &Delta) -> i32 {
        paragraph.para.after.unwrap_or(0)
    }
    pub(crate) fn spacing_boundary(
        previous: &mut Delta,
        current: &mut Delta,
        enabled: (bool, bool),
    ) -> i32 {
        let after = previous.para.after.unwrap_or(0);
        let before = current.para.before.unwrap_or(0);
        // ISO/IEC 29500: collapse every source boundary with max, then subtract
        // each enabled contextual side (10pt / 12pt, left enabled => 2pt).
        let remaining = (after.max(before)
            - if enabled.0 { after } else { 0 }
            - if enabled.1 { before } else { 0 })
        .max(0);
        // Store the gap once: ordinary boundaries use the preceding after,
        // excluding forced page starts. Preserve the contextual residual's old side.
        previous.para.after = Some(if enabled.0 { 0 } else { remaining });
        current.para.before = Some(if enabled.0 { remaining } else { 0 });
        remaining
    }
    pub(crate) fn fragment_shape(base: &Delta, first: bool, last: bool) -> Delta {
        let mut derived = base.clone();
        if !first {
            derived.para.before = Some(0);
            derived.para.first = Some(0);
            derived.para.num_id = Some(0);
            derived.para.num_present = Some(false);
            derived.para.outline = Some(9);
            derived.para.projected_list = None;
            derived.para.projected_marker = None;
        }
        if !last {
            derived.para.after = Some(0);
            derived.para.keep_next = Some(false);
        }
        derived.para.keep_lines = Some(false);
        // Hancom treats each fragment as a separate paragraph, so preserve the
        // original effective widow control to protect lone lines at page ends.
        derived
    }
    // Runs and paragraph marks share character style toggles and direct overrides.
    fn character(
        &mut self,
        base: &Delta,
        direct: &Delta,
        input: &RunInput,
        part: &str,
        w: &mut Vec<DocxWarning>,
    ) -> Result<CharResult> {
        let mut effective = base.clone();
        if let Some(d) = self.style(input.style, StyleKind::Character, part, w)? {
            effective.overlay(&d, true);
        }
        effective.overlay(direct, false);
        effective.overlay(&input.delta, false);
        let mut result = self.char_shape(&effective)?;
        result.reference_missing = input.style.is_some_and(|id| {
            !self.definitions.style(id).is_some_and(|s| s.kind == StyleKind::Character)
        });
        Ok(result)
    }
    fn font(&mut self, key: FontKey) -> Result<FontIndex> {
        if let Some(id) = self.fonts.get(&key) {
            return Ok(*id);
        }
        // Output storage has the same bounded-entry budget as generated shapes;
        // only Fonts::admit_source counts original font definitions.
        limit("#format", "font", "output fonts", self.fonts.len() + 1, MAX_SHAPES)?;
        self.admit_font_bytes(self.definitions.fonts.name(key).len())?;
        let id = FontIndex::new(self.registry.fonts.len());
        let name = self.definitions.fonts.name(key);
        #[cfg(test)]
        {
            self.copied_font_bytes += name.len();
        }
        self.registry
            .fonts
            .push(FontId::new(name).map_err(|e| DocxError::invalid("#format", "font", quote(e)))?);
        self.fonts.insert(key, id);
        Ok(id)
    }
    fn char_shape(&mut self, d: &Delta) -> Result<CharResult> {
        #[cfg(test)]
        {
            self.char_shape_calls += 1;
        }
        let c = &d.char;
        let latin = c.fonts[1].or(c.fonts[0]);
        let east = c.fonts[2];
        let family = east.or(latin).unwrap_or(self.fallback_font);
        let missing = u8::from(c.size.is_none())
            | (u8::from(east.is_none() && latin.is_none()) << 1)
            | (u8::from(c.color.is_none()) << 2)
            | (u8::from(east.is_none() && latin.is_some()) << 3);
        let size = c.size.unwrap_or(1000);
        let bold = c.toggles[0].unwrap_or(false);
        let italic = c.toggles[1].unwrap_or(false);
        let strike = c.toggles[2].unwrap_or(false);
        let collapsed = c.fonts[0].zip(c.fonts[1]).is_some_and(|(a, b)| a != b)
            || c.fonts[3].is_some_and(|v| v != family)
            || c.size_cs.is_some_and(|v| v != size)
            || c.toggles[3].is_some_and(|v| v != bold)
            || c.toggles[4].is_some_and(|v| v != italic);
        let key = (
            [family, latin.unwrap_or(family), east.unwrap_or(family)],
            size,
            [bold, italic, strike],
            c.color.unwrap_or(0),
            c.underline.unwrap_or(false),
            c.highlight.flatten(),
            c.vertical.unwrap_or(0),
        );
        let hash = self.hasher.hash_one(key);
        // Exact canonical equality verifies every variable field before allocating strings.
        if let Some((_, id)) = self
            .chars
            .get(&hash)
            .and_then(|bucket| bucket.iter().find(|(candidate, _)| *candidate == key))
        {
            return Ok(CharResult {
                id: *id,
                font: self.fonts[&family],
                missing,
                collapsed,
                reference_missing: false,
                line_fonts: [latin, east],
                size,
            });
        }
        self.admit_shape()?;
        self.admit_font_bytes(key.0.iter().map(|&k| self.definitions.fonts.name(k).len()).sum())?;
        for k in key.0 {
            self.font(k)?;
        }
        #[cfg(test)]
        {
            self.copied_font_bytes += self.definitions.fonts.name(family).len()
                + key.0.iter().map(|&k| self.definitions.fonts.name(k).len()).sum::<usize>();
        }
        let mut partial = PartialCharShape::default();
        partial.font = Some(self.definitions.fonts.name(family).to_owned());
        partial.size = Some(unit(size)?);
        let mut shape = partial
            .resolve("import")
            .map_err(|e| DocxError::invalid("#format", "char", quote(e)))?;
        // Word uses the font's space glyph width; Hancom defaults to its own space width.
        shape.use_font_space = true;
        shape.font_latin = Some(self.definitions.fonts.name(key.0[1]).to_owned());
        shape.font_east_asian = Some(self.definitions.fonts.name(key.0[2]).to_owned());
        shape.bold = bold;
        shape.italic = italic;
        shape.color = color(key.3);
        shape.underline_type = if key.4 { UnderlineType::Bottom } else { UnderlineType::None };
        shape.strikeout_shape = if strike { StrikeoutShape::Solid } else { StrikeoutShape::None };
        shape.shade_color = key.5.map(color);
        shape.vertical_position = match key.6 {
            1 => VerticalPosition::Superscript,
            2 => VerticalPosition::Subscript,
            _ => VerticalPosition::Normal,
        };
        let id = CharShapeIndex::new(self.registry.char_shapes.len());
        self.registry.char_shapes.push(shape);
        self.chars.entry(hash).or_default().push((key, id));
        Ok(CharResult {
            id,
            font: self.fonts[&family],
            missing,
            collapsed,
            reference_missing: false,
            line_fonts: [latin, east],
            size,
        })
    }
    pub(crate) fn para_shape(&mut self, d: &Delta) -> Result<ParaShapeIndex> {
        let p = &d.para;
        let mut shape = ParaShape::default();
        shape.list = p.projected_list.map(lists::ListRef::core);
        shape.list_marker = p.projected_marker;
        shape.alignment = match p.alignment {
            Some(1) => Alignment::Center,
            Some(2) => Alignment::Right,
            Some(3) => Alignment::Justify,
            Some(4) => Alignment::Distribute,
            _ => Alignment::Left,
        };
        let rule = if p.line.is_none() { 0 } else { p.line_rule.unwrap_or(0) };
        shape.line_spacing_type = match rule {
            1 => LineSpacingType::Fixed,
            2 => LineSpacingType::AtLeast,
            _ => LineSpacingType::Percentage,
        };
        shape.line_spacing_value =
            units::canonical_float(if let Some(value) = p.projected_line {
                f64::from_bits(value)
            } else if rule == 0 {
                f64::from(p.line.unwrap_or(240)) * 100.0 / 240.0
            } else {
                line_height::physical_spacing(f64::from(
                    unit(p.line.unwrap_or(240).checked_mul(5).ok_or_else(|| {
                        DocxError::invalid("#format", "line", "numeric overflow")
                    })?)?
                    .as_i32(),
                ))
            })?;
        shape.space_before = unit(p.before.unwrap_or(0))?;
        shape.space_after = unit(p.after.unwrap_or(0))?;
        let (left, first) = marker_indent(&shape.list, p.left.unwrap_or(0), p.first.unwrap_or(0))?;
        shape.indent_left = unit(left)?;
        shape.indent_right = unit(p.right.unwrap_or(0))?;
        shape.indent_first_line = unit(first)?;
        shape.keep_with_next = p.keep_next.unwrap_or(false);
        shape.keep_lines_together = p.keep_lines.unwrap_or(false);
        // Word defaults widowControl to on after style/direct inheritance.
        shape.widow_orphan = p.widow.unwrap_or(true);
        shape.east_asian_auto_spacing = if p.auto_space_de.is_none() && p.auto_space_dn.is_none() {
            None
        } else {
            Some(EastAsianAutoSpacing::new(
                p.auto_space_de.unwrap_or(true),
                p.auto_space_dn.unwrap_or(true),
            ))
        };
        self.intern_para(shape)
    }
    fn intern_para(&mut self, shape: ParaShape) -> Result<ParaShapeIndex> {
        let hash = self.hasher.hash_one((
            shape.alignment,
            shape.line_spacing_type,
            shape.line_spacing_value.to_bits(),
            shape.space_before,
            shape.space_after,
            shape.indent_left,
            shape.indent_right,
            shape.indent_first_line,
            shape.keep_with_next,
            shape.keep_lines_together,
            (
                shape.widow_orphan,
                lists::ListRef::from_core(shape.list),
                shape.list_marker,
                shape.east_asian_auto_spacing,
            ),
        ));
        if let Some(id) = self.paras.get(&hash).and_then(|bucket| {
            bucket.iter().find(|id| {
                #[cfg(test)]
                self.para_comparisons.set(self.para_comparisons.get() + 1);
                self.registry.para_shapes[id.get()] == shape
            })
        }) {
            return Ok(*id);
        }
        self.admit_shape()?;
        let id = ParaShapeIndex::new(self.registry.para_shapes.len());
        self.registry.para_shapes.push(shape);
        self.paras.entry(hash).or_default().push(id);
        Ok(id)
    }
    fn admit_font_bytes(&mut self, additional: usize) -> Result<()> {
        let observed = self.owned_font_bytes.saturating_add(additional);
        limit("#format", "font", "materialized font bytes", observed, 64 * 1024 * 1024)?;
        self.owned_font_bytes = observed;
        Ok(())
    }
    fn admit_shape(&self) -> Result<()> {
        limit(
            "#format",
            "shape",
            "generated shapes",
            self.registry.char_shapes.len() + self.registry.para_shapes.len() + 1,
            MAX_SHAPES,
        )
    }
}
// The same diagnostic projection serves exported definitions, runs and paragraph marks.
// Bits 0..3 describe character fallbacks; bits 4..6 describe paragraph fallbacks.
fn para_missing(p: &ParaDelta) -> u8 {
    (u8::from(p.line.is_none()) << 4)
        | (u8::from(p.before.is_none() || p.after.is_none()) << 5)
        | (u8::from(p.left.is_none() || p.right.is_none() || p.first.is_none()) << 6)
}
fn format_warnings(
    missing: u8,
    collapsed: bool,
    part: &str,
    at: &str,
    w: &mut Vec<DocxWarning>,
) -> Result<()> {
    for (mask, loss, action, reason) in [
        (
            1,
            "font_size",
            "use_fallback",
            "Unresolved character formatting uses 10pt, 함초롬바탕 or black",
        ),
        (
            2,
            "font_family",
            "use_fallback",
            "Unresolved character formatting uses 10pt, 함초롬바탕 or black",
        ),
        (
            4,
            "text_color",
            "use_fallback",
            "Unresolved character formatting uses 10pt, 함초롬바탕 or black",
        ),
        (
            8,
            "east_asian_font",
            "use_general_font",
            "Unresolved East Asian font uses the general font",
        ),
        (
            16,
            "line_spacing",
            "use_fallback",
            "Unresolved paragraph formatting uses single-line spacing or zero length",
        ),
        (
            32,
            "paragraph_spacing",
            "use_fallback",
            "Unresolved paragraph formatting uses single-line spacing or zero length",
        ),
        (
            64,
            "indentation",
            "use_fallback",
            "Unresolved paragraph formatting uses single-line spacing or zero length",
        ),
    ] {
        if missing & mask != 0 {
            warn(w, part, at, Kind::FormatDefaulted, loss, action, reason)?;
        }
    }
    if collapsed {
        warn(
            w,
            part,
            at,
            Kind::ScriptFormatCollapsed,
            "script_format",
            "select_supported_slots",
            "Different script fonts, sizes or effects were collapsed",
        )?;
    }
    Ok(())
}
/// Word puts a list marker at the hanging position (`left + first`, `first < 0`) and the text at
/// `left`. Hancom ignores a negative first line on a numbered or bulleted paragraph and puts the
/// marker at the left margin, wrapping later lines after the marker. Folding the hanging amount
/// into the left margin keeps the marker where Word draws it.
fn marker_indent(list: &Option<ParagraphListRef>, left: i32, first: i32) -> Result<(i32, i32)> {
    match list {
        Some(ParagraphListRef::Number { .. } | ParagraphListRef::Bullet { .. }) if first < 0 => {
            Ok((marker_anchor(left, first)?, 0))
        }
        _ => Ok((left, first)),
    }
}
fn marker_anchor(left: i32, first: i32) -> Result<i32> {
    Ok(unit(
        i32::try_from(
            i64::from(left)
                .checked_add(i64::from(first))
                .ok_or_else(|| DocxError::invalid("#format", "ind", "anchor overflow"))?,
        )
        .map_err(|_| DocxError::invalid("#format", "ind", "anchor overflow"))?,
    )?
    .as_i32())
}
fn unit(n: i32) -> Result<HwpUnit> {
    HwpUnit::new(n).map_err(|e| DocxError::invalid("#format", "length", quote(e)))
}
fn color(rgb: u32) -> Color {
    Color::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

#[cfg(test)]
#[path = "resolve_tests.rs"]
mod tests;

fn reference_warning(result: CharResult, part: &str, w: &mut Vec<DocxWarning>) -> Result<()> {
    if result.reference_missing {
        warn(
            w,
            part,
            "style",
            Kind::StyleReferenceLost,
            "style_reference",
            "ignore_link",
            "Missing or incompatible character style",
        )?;
    }
    Ok(())
}

/// Measured decimal digit/dot ratios are restricted to a single own-level token.
/// Counts digits without allocating a string proportional to the current number.
fn measured_number_ratio(head: &hwpforge_core::ParaHead, index: u8, n: u64) -> Result<Option<u64>> {
    if head.num_format != hwpforge_foundation::NumberFormatType::Digit {
        return Ok(None);
    }
    let token = format!("^{}", u32::from(index) + 1);
    let Some((prefix, suffix)) = head.text.split_once(&token) else {
        return Ok(None);
    };
    let mut ratio = u64::from(n.checked_ilog10().unwrap_or(0))
        .checked_add(1)
        .and_then(|v| v.checked_mul(606))
        .ok_or_else(marker_arithmetic)?;
    for c in prefix.chars().chain(suffix.chars()) {
        let value = match c {
            '0'..='9' => 606,
            '.' => 242,
            _ => return Ok(None),
        };
        ratio = ratio.checked_add(value).ok_or_else(marker_arithmetic)?;
    }
    Ok(Some(ratio))
}
fn marker_arithmetic() -> DocxError {
    DocxError::invalid("#format", "numPr", "default tab marker arithmetic out of range")
}
/// The small digit/dot/Symbol table applies only to zero-hanging default tabs.
/// Sum ratios first, then round half up once; these are estimates across fonts/styles.
fn measured_marker_width(size: i32, ratio: u64) -> Result<i32> {
    let size = u64::try_from(size).map_err(|_| marker_arithmetic())?;
    let width =
        size.checked_mul(ratio).and_then(|v| v.checked_add(550)).ok_or_else(marker_arithmetic)?
            / 1100;
    let width = i32::try_from(width).map_err(|_| marker_arithmetic())?;
    unit(width)?;
    Ok(width)
}
/// Coordinates are local to the text area's left edge (unmeasured in cells/columns).
/// Candidates are the anchor A and the default grid, selecting the first at or beyond
/// the marker end. Right ends at A and therefore always chooses A, with zero distance.
/// Doubled coordinates preserve center half-units; signed ceiling includes exact tabs.
fn default_tab_gap(
    anchor: i32,
    width: i32,
    tab: i32,
    align: ListMarkerAlign,
) -> Result<(i32, bool)> {
    if tab <= 0 {
        return Err(marker_arithmetic());
    }
    let anchor = i64::from(anchor);
    let end = anchor
        .checked_mul(2)
        .and_then(|v| {
            v.checked_add(match align {
                ListMarkerAlign::Left => i64::from(width).checked_mul(2)?,
                ListMarkerAlign::Right => 0,
                ListMarkerAlign::Center => i64::from(width),
                _ => return None,
            })
        })
        .ok_or_else(marker_arithmetic)?;
    let tab = i64::from(tab);
    let denominator = tab.checked_mul(2).ok_or_else(marker_arithmetic)?;
    let k = end
        .div_euclid(denominator)
        .checked_add(i64::from(end.rem_euclid(denominator) != 0))
        .ok_or_else(marker_arithmetic)?;
    let target = if anchor.checked_mul(2).ok_or_else(marker_arithmetic)? >= end {
        anchor
    } else {
        k.checked_mul(tab).ok_or_else(marker_arithmetic)?
    };
    unit(i32::try_from(target).map_err(|_| marker_arithmetic())?)?;
    let gap = target.checked_sub(anchor).ok_or_else(marker_arithmetic)?;
    let gap = i32::try_from(gap).map_err(|_| marker_arithmetic())?;
    unit(gap)?;
    let spare =
        target.checked_mul(2).and_then(|v| v.checked_sub(end)).ok_or_else(marker_arithmetic)?;
    Ok((gap, align != ListMarkerAlign::Right && (0..200).contains(&spare)))
}
