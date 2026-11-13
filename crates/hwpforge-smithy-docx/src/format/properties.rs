use super::*;

struct Read<'a> {
    e: &'a Element,
    part: &'a str,
    at: &'a str,
    warnings: &'a mut Vec<DocxWarning>,
}
impl Read<'_> {
    fn unsupported(&mut self, loss: &'static str) -> Result<()> {
        warn(
            self.warnings,
            self.part,
            self.at,
            Kind::UnsupportedAttribute,
            loss,
            "ignore",
            format_args!("Unsupported formatting property '{}'", quote(&self.e.name)),
        )
    }
    fn bad(&mut self, value: &str) -> Result<()> {
        self.bad_loss(value, "format_value")
    }
    fn bad_loss(&mut self, value: &str, loss: &'static str) -> Result<()> {
        warn(
            self.warnings,
            self.part,
            self.at,
            Kind::UnsupportedValue,
            loss,
            "inherit_or_default",
            format_args!("Unsupported {} value '{}'", quote(&self.e.name), quote(value)),
        )
    }
    fn boolean(&mut self) -> Result<Option<bool>> {
        boolean(self.e, "val", Some(true), self.part, self.at, self.warnings)
    }

    fn number(&mut self, key: &str, factor: i128, minimum: i32) -> Result<Option<i32>> {
        let Some(v) = attr(self.e, key) else {
            if key == "val" {
                self.bad("missing val")?;
            }
            return Ok(None);
        };
        match units::length(v, factor, minimum) {
            Ok(n) => Ok(Some(n)),
            _ => {
                self.bad(v)?;
                Ok(None)
            }
        }
    }
    fn unsigned(&mut self) -> Result<Option<u32>> {
        let v = attr(self.e, "val").unwrap_or("");
        crate::identifier::check(v, self.part, self.at)?;
        v.parse().map(Some).map_err(|_| {
            DocxError::invalid(
                self.part,
                self.at,
                format!("invalid nonnegative integer '{}'", quote(v)),
            )
        })
    }
    // Inspect attributes even when value reading returns early or fails. Keep the
    // value error if both operations fail, but never skip the attribute inspection.
    fn leaf(
        &mut self,
        keys: &[&str],
        read: impl FnOnce(&mut Self) -> Result<()>,
    ) -> Result<PropertyState> {
        let value = read(self);
        let attributes = self.known_attributes(keys);
        value?;
        attributes?;
        Ok(PropertyState::Leaf)
    }
    fn known_attributes(&mut self, keys: &[&str]) -> Result<()> {
        for (ns, k, _) in &self.e.attributes {
            if *ns != W_NS.0 || !keys.contains(&k.as_str()) {
                record(
                    self.warnings,
                    Kind::UnsupportedAttribute,
                    "format_attribute",
                    "ignore",
                    || {
                        (
                            Some(self.part),
                            Some(self.at),
                            format!(
                                "Unsupported formatting attribute '{}' on '{}'",
                                quote(k),
                                quote(&self.e.name)
                            ),
                        )
                    },
                )?;
            }
        }
        Ok(())
    }
}

/// Called only for direct children of a recognized property container (or numPr).
/// Descendants of unsupported effects never inject properties into the parent.
#[allow(clippy::too_many_arguments)]
pub(super) fn read_property(
    d: &mut Delta,
    e: &Element,
    character: bool,
    theme: &Theme,
    fonts: &mut Fonts,
    part: &str,
    at: &str,
    warnings: &mut Vec<DocxWarning>,
) -> Result<PropertyState> {
    let mut r = Read { e, part, at, warnings };
    if e.namespace_id != W_NS.0 {
        r.unsupported(if character {
            "unknown_character_property"
        } else {
            "unknown_paragraph_property"
        })?;
        return Ok(PropertyState::Excluded);
    }
    let name = e.name.as_str();
    if character {
        match name {
            "b" | "i" | "strike" | "bCs" | "iCs" => r.leaf(&["val"], |r| {
                let index = match name {
                    "b" => 0,
                    "i" => 1,
                    "strike" => 2,
                    "bCs" => 3,
                    _ => 4,
                };
                d.char.toggles[index] = r.boolean()?;
                Ok(())
            }),
            "sz" => r.leaf(&["val"], |r| {
                d.char.size = r.number("val", 50, 1)?;
                Ok(())
            }),
            "szCs" => r.leaf(&["val"], |r| {
                d.char.size_cs = r.number("val", 50, 1)?;
                Ok(())
            }),
            "u" => r.leaf(&["val"], |r| {
                d.char.underline = match attr(e, "val").unwrap_or("single") {
                    "single" => Some(true),
                    "none" => Some(false),
                    v => {
                        r.bad(v)?;
                        None
                    }
                };
                Ok(())
            }),
            "rFonts" => r.leaf(
                &[
                    "ascii",
                    "hAnsi",
                    "eastAsia",
                    "cs",
                    "asciiTheme",
                    "hAnsiTheme",
                    "eastAsiaTheme",
                    "cstheme",
                ],
                |r| {
                    for (i, (literal, reference)) in [
                        ("ascii", "asciiTheme"),
                        ("hAnsi", "hAnsiTheme"),
                        ("eastAsia", "eastAsiaTheme"),
                        ("cs", "cstheme"),
                    ]
                    .iter()
                    .enumerate()
                    {
                        if attr(e, literal) == Some("") {
                            r.bad("empty font name")?;
                        }
                        let direct = attr(e, literal)
                            .filter(|v| !v.is_empty())
                            .map(|v| fonts.intern(v, part, at))
                            .transpose()?;
                        d.char.fonts[i] = if let Some(key) = attr(e, reference) {
                            match theme.font(key) {
                                Some(font) => Some(font),
                                None => {
                                    r.bad_loss(key, "theme_font")?;
                                    direct
                                }
                            }
                        } else {
                            direct
                        };
                    }
                    Ok(())
                },
            ),
            "color" => r.leaf(&["val", "themeColor", "themeTint", "themeShade"], |r| {
                let v = attr(e, "val").unwrap_or("");
                d.char.color = rgb(v);
                if ["themeColor", "themeTint", "themeShade"].iter().any(|k| attr(e, k).is_some()) {
                    r.bad_loss(v, "theme_color")?;
                } else if d.char.color.is_none() {
                    r.bad(v)?;
                }
                Ok(())
            }),
            "highlight" => r.leaf(&["val"], |r| {
                let v = attr(e, "val").unwrap_or("");
                d.char.highlight = match v {
                    "none" => Some(None),
                    _ => highlight(v).map(Some),
                };
                if d.char.highlight.is_none() {
                    r.bad(v)?;
                }
                Ok(())
            }),
            "vertAlign" => r.leaf(&["val"], |r| {
                d.char.vertical = match attr(e, "val").unwrap_or("") {
                    "baseline" => Some(0),
                    "superscript" => Some(1),
                    "subscript" => Some(2),
                    v => {
                        r.bad(v)?;
                        None
                    }
                };
                Ok(())
            }),
            "vanish" | "webHidden" => r.leaf(&["val"], |r| {
                if r.boolean()? != Some(false) {
                    warn(
                        r.warnings,
                        part,
                        at,
                        Kind::HiddenTextExposed,
                        "hidden_text_effect",
                        "keep_text",
                        "Hidden text is retained without its hiding effect",
                    )?;
                }
                Ok(())
            }),
            "caps" | "smallCaps" | "w" | "spacing" | "kern" | "position" | "bdr" | "shd"
            | "outline" | "shadow" | "emboss" | "imprint" | "dstrike" => {
                r.unsupported("character_effect")?;
                Ok(PropertyState::Excluded)
            }
            // Run language/hint may affect script choice; no silent claim of script selection.
            "rStyle" => r.leaf(&["val"], |r| {
                if attr(e, "val").is_none() {
                    r.bad("missing style ID")?;
                }
                Ok(())
            }),
            _ => {
                r.unsupported("unknown_character_property")?;
                Ok(PropertyState::Excluded)
            }
        }
    } else {
        match name {
            "jc" => r.leaf(&["val"], |r| {
                d.para.alignment = match attr(e, "val").unwrap_or("") {
                    "left" | "start" => Some(0),
                    "center" => Some(1),
                    "right" | "end" => Some(2),
                    "both" => Some(3),
                    "distribute" => Some(4),
                    v => {
                        r.bad(v)?;
                        None
                    }
                };
                Ok(())
            }),
            "ind" => r.leaf(&["left", "right", "firstLine", "hanging"], |r| {
                for (key, target) in [
                    ("left", &mut d.para.left),
                    ("right", &mut d.para.right),
                    ("firstLine", &mut d.para.first),
                ] {
                    let chars = match key {
                        "left" => "leftChars",
                        "right" => "rightChars",
                        _ => "firstLineChars",
                    };
                    if attr(e, chars).is_none() {
                        *target = r.number(key, 5, -100_000_000)?;
                    }
                }
                if let Some(n) = if attr(e, "hangingChars").is_none() {
                    r.number("hanging", 5, 0)?
                } else {
                    None
                } {
                    if attr(e, "firstLine").is_some() {
                        r.bad("firstLine + hanging; hanging selected")?;
                    }
                    d.para.first = Some(-n);
                }
                Ok(())
            }),
            "spacing" => r.leaf(&["before", "after", "line", "lineRule"], |r| {
                if attr(e, "beforeLines").is_none()
                    && !matches!(attr(e, "beforeAutospacing"), Some("1" | "true" | "on"))
                {
                    d.para.before = r.number("before", 5, 0)?;
                }
                if attr(e, "afterLines").is_none()
                    && !matches!(attr(e, "afterAutospacing"), Some("1" | "true" | "on"))
                {
                    d.para.after = r.number("after", 5, 0)?;
                }
                // A malformed line must not change its inherited rule either.
                let line = if attr(e, "line").is_some_and(|v| {
                    v.bytes().any(|b| !b.is_ascii_digit() && b != b'+' && b != b'-')
                }) {
                    r.bad(attr(e, "line").unwrap())?;
                    None
                } else {
                    r.number("line", 1, 0)?
                };
                if attr(e, "line").is_none() || line.is_some() {
                    let rule = match attr(e, "lineRule") {
                        None => None,
                        Some("auto") => Some(0),
                        Some("exact") => Some(1),
                        Some("atLeast") => Some(2),
                        Some(v) => {
                            r.bad(v)?;
                            return Ok(());
                        }
                    };
                    if line.is_some_and(|v| rule.unwrap_or(0) != 0 && v > 20_000_000) {
                        r.bad("line length out of range")?;
                    } else {
                        d.para.line = line;
                        d.para.line_rule = rule;
                    }
                }
                Ok(())
            }),
            "autoSpaceDE" => r.leaf(&["val"], |r| {
                d.para.auto_space_de = r.boolean()?;
                Ok(())
            }),
            "autoSpaceDN" => r.leaf(&["val"], |r| {
                d.para.auto_space_dn = r.boolean()?;
                Ok(())
            }),
            "contextualSpacing" => r.leaf(&["val"], |r| {
                d.para.contextual = r.boolean()?;
                Ok(())
            }),
            "keepNext" => r.leaf(&["val"], |r| {
                d.para.keep_next = r.boolean()?;
                Ok(())
            }),
            "keepLines" => r.leaf(&["val"], |r| {
                d.para.keep_lines = r.boolean()?;
                Ok(())
            }),
            "widowControl" => r.leaf(&["val"], |r| {
                d.para.widow = r.boolean()?;
                Ok(())
            }),
            "pageBreakBefore" => r.leaf(&["val"], |r| {
                d.para.page_break = r.boolean()?;
                Ok(())
            }),
            "numId" => r.leaf(&["val"], |r| {
                d.para.num_id = r.unsigned()?;
                Ok(())
            }),
            "ilvl" => r.leaf(&["val"], |r| {
                d.para.level = r.unsigned()?;
                Ok(())
            }),
            "outlineLvl" => r.leaf(&["val"], |r| {
                d.para.outline = r.unsigned()?;
                if d.para.outline.is_some_and(|n| n > 9) {
                    r.bad(attr(e, "val").unwrap_or(""))?;
                    d.para.outline = None;
                }
                Ok(())
            }),
            "pStyle" => r.leaf(&["val"], |r| {
                if attr(e, "val").is_none() {
                    r.bad("missing style ID")?;
                }
                Ok(())
            }),
            "numPr" => r.leaf(&[], |_| {
                d.para.num_present = Some(true);
                Ok(())
            }),
            "rPr" => r.leaf(&[], |_| Ok(())),
            "sectPr" => {
                warn(
                    r.warnings,
                    part,
                    at,
                    Kind::PageLayoutApproximated,
                    "nested_section",
                    "omit",
                    "Section settings in formatting definitions are not applied",
                )?;
                Ok(PropertyState::Excluded)
            }
            "tabs" | "pBdr" | "bidi" => {
                r.unsupported("paragraph_effect")?;
                Ok(PropertyState::Excluded)
            }
            _ => {
                r.unsupported("unknown_paragraph_property")?;
                Ok(PropertyState::Excluded)
            }
        }
    }
}
fn rgb(s: &str) -> Option<u32> {
    if s.len() == 6 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
        u32::from_str_radix(s, 16).ok()
    } else {
        None
    }
}
fn highlight(s: &str) -> Option<u32> {
    Some(match s {
        "black" => 0,
        "blue" => 0x0000ff,
        "cyan" => 0x00ffff,
        "green" => 0x00ff00,
        "magenta" => 0xff00ff,
        "red" => 0xff0000,
        "yellow" => 0xffff00,
        "white" => 0xffffff,
        "darkBlue" => 0x000080,
        "darkCyan" => 0x008080,
        "darkGreen" => 0x008000,
        "darkMagenta" => 0x800080,
        "darkRed" => 0x800000,
        "darkYellow" => 0x808000,
        "darkGray" => 0x808080,
        "lightGray" => 0xc0c0c0,
        _ => return None,
    })
}

/// Containers have no supported formatting attributes of their own.
pub(crate) fn container_attributes(
    e: &Element,
    part: &str,
    at: &str,
    warnings: &mut Vec<DocxWarning>,
) -> Result<()> {
    for (_, name, _) in &e.attributes {
        record(warnings, Kind::UnsupportedAttribute, "format_attribute", "ignore", || {
            (
                Some(part),
                Some(at),
                format!(
                    "Unsupported formatting attribute '{}' on '{}'",
                    quote(name),
                    quote(&e.name)
                ),
            )
        })?;
    }
    Ok(())
}

/// Both property values and definition flags use the same on/off lexical rules.
pub(super) fn boolean(
    e: &Element,
    key: &str,
    absent: Option<bool>,
    part: &str,
    at: &str,
    warnings: &mut Vec<DocxWarning>,
) -> Result<Option<bool>> {
    match attr(e, key) {
        None => Ok(absent),
        Some("1" | "true" | "on") => Ok(Some(true)),
        Some("0" | "false" | "off") => Ok(Some(false)),
        Some(v) => {
            Read { e, part, at, warnings }.bad(v)?;
            Ok(None)
        }
    }
}

/// A consumed property leaf still needs child-boundary diagnostics; excluded
/// subtrees have already been diagnosed and must never inject formatting.
#[derive(Clone, Copy)]
pub(crate) enum PropertyState {
    Leaf,
    Excluded,
}
impl PropertyState {
    pub(crate) fn child(
        self,
        e: &Element,
        part: &str,
        at: &str,
        warnings: &mut Vec<DocxWarning>,
    ) -> Result<Self> {
        if matches!(self, Self::Leaf) {
            record(warnings, Kind::UnsupportedAttribute, "format_extension", "ignore", || {
                (
                    Some(part),
                    Some(at),
                    format!("Unsupported formatting child subtree '{}'", quote(&e.name)),
                )
            })?;
        }
        Ok(Self::Excluded)
    }
}

#[cfg(test)]
#[path = "properties_tests.rs"]
mod tests;
