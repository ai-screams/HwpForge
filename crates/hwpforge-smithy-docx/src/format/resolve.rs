use super::*;
use hwpforge_blueprint::{
    registry::StyleRegistry,
    style::{ParaShape, PartialCharShape},
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
    named: HashMap<StyleKey, StyleRef>,
    default_paragraph: Option<StyleKey>,
    deltas: HashMap<RunInput, usize>,
    inputs: Vec<RunInput>,
    contexts: HashMap<(Delta, CharDelta), usize>,
    combinations: HashMap<(usize, usize), CharResult>,
    chars: HashMap<u64, Vec<(CharKey, CharShapeIndex)>>,
    marks: HashMap<(usize, usize), CharResult>,
    fallback_font: FontKey,
    unknown_metrics: HashSet<FontKey>,
    paras: HashMap<u64, Vec<ParaShapeIndex>>,
    fonts: HashMap<FontKey, FontIndex>,
    hasher: RandomState,
    owned_font_bytes: usize,
    lists: lists::Lists,
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
            named: HashMap::new(),
            default_paragraph: None,
            deltas: HashMap::new(),
            inputs: vec![],
            contexts: HashMap::new(),
            combinations: HashMap::new(),
            chars: HashMap::new(),
            marks: HashMap::new(),
            fallback_font,
            unknown_metrics: HashSet::new(),
            paras: HashMap::new(),
            fonts: HashMap::new(),
            hasher: RandomState::new(),
            owned_font_bytes: 0,
            lists,
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
            if effective.para.line.is_none() || effective.para.line_rule != Some(1) {
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
        let mut height = 1;
        if let Some(parent) = s.parent {
            let valid = self.definitions.style(parent).is_some_and(|p| p.kind == s.kind);
            if valid && !path.contains(&parent) {
                let (base, h) = self.inherit(parent, path, warnings)?;
                result = base;
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
    // The table argument is retained for whole-table contexts, even while table output is omitted.
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
            }
            if let Some(d) = over {
                apply(&mut result, &d.delta, true, part, "numPr", w)?;
            }
        }
        if let Some(d) = paragraph {
            apply(&mut result, &d, true, part, "style", w)?;
        }
        apply(&mut result, direct, false, part, "pPr", w)?;
        Ok(result)
    }
    pub(crate) fn finish(
        &mut self,
        paragraph: (&mut Paragraph, &[bool]),
        style: Option<StyleKey>,
        direct: &Delta,
        mark: Option<(&Delta, Option<StyleKey>)>,
        part: &str,
        w: &mut Vec<DocxWarning>,
    ) -> Result<Delta> {
        let (p, diagnose) = paragraph;
        let style = style.or(self.default_paragraph);
        let mut base = self.context(None, style, direct, part, w)?;
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
        let mut height = line_height::Height::default();
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
            if !exact && (empty_mark || run.content.as_text().is_some_and(|t| !t.is_empty())) {
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
            if !exact {
                east |= empty && result.line_fonts[1].is_some();
                self.add_height(&mut height, result, east, part, w)?;
            }
        }
        if !exact {
            base.para.projected_line = height.project(&base.para, east)?;
            if height.mixed() {
                warn(w, part, "pPr", Kind::ValueApproximated, "mixed_font_line_height",
                    "approximate", "Paragraph-wide ascent/descent maxima approximate mixed fonts or sizes without line breaking")?;
            }
        }
        base.para.projected_list =
            self.lists.resolve(&self.definitions, &mut self.registry, &base.para, part, w)?;
        Ok(base)
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
                    "retain_spacing", format_args!("Unknown natural line metrics for '{}'; raw auto multiplier or specified minimum retained", self.definitions.fonts.name(key)))?;
            }
        }
        height.add(c.size, metrics);
        Ok(())
    }
    pub(crate) fn spacing_boundary(
        previous: &mut Delta,
        current: &mut Delta,
        enabled: (bool, bool),
    ) {
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
        let (left, first) = marker_indent(&shape.list, p.left.unwrap_or(0), p.first.unwrap_or(0));
        shape.indent_left = unit(left)?;
        shape.indent_right = unit(p.right.unwrap_or(0))?;
        shape.indent_first_line = unit(first)?;
        shape.keep_with_next = p.keep_next.unwrap_or(false);
        shape.keep_lines_together = p.keep_lines.unwrap_or(false);
        // Word defaults widowControl to on after style/direct inheritance.
        shape.widow_orphan = p.widow.unwrap_or(true);
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
            (shape.widow_orphan, lists::ListRef::from_core(shape.list)),
        ));
        if let Some(id) = self.paras.get(&hash).and_then(|bucket| {
            bucket.iter().find(|id| self.registry.para_shapes[id.get()] == shape)
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
fn marker_indent(list: &Option<ParagraphListRef>, left: i32, first: i32) -> (i32, i32) {
    match list {
        Some(ParagraphListRef::Number { .. } | ParagraphListRef::Bullet { .. }) if first < 0 => {
            (left.saturating_add(first), 0)
        }
        _ => (left, first),
    }
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
