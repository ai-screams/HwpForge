//! Input-local whole-table properties shared by style definitions and direct properties.
use super::*;
use hwpforge_foundation::HwpUnit;

// left, right, top, bottom, insideH, insideV. None means inherit, not no line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TableProperties {
    pub(crate) borders: [Option<bool>; 6],
    // Source widths in HwpUnit, independent of the approximated output strokes.
    pub(crate) border_widths: [Option<i32>; 6],
    pub(crate) width: Option<Option<i32>>,
    pub(crate) width_kind: WidthKind,
    pub(crate) width_diagnostic: Option<Rc<WidthDiagnostic>>,
    pub(crate) width_diagnostics: Vec<Rc<WidthDiagnostic>>,
    pub(crate) indent: Option<i32>,
    pub(crate) alignment: Option<TableAlignment>,
    pub(crate) bidi: Option<bool>,
    pub(crate) margins: [Option<i32>; 4],
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum WidthKind {
    #[default]
    Unspecified,
    Dxa,
    Auto,
    Nil,
    Pct,
    Unknown,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct WidthDiagnostic {
    warning_index: usize,
    pub(crate) grid: std::cell::Cell<bool>,
    pub(crate) container: std::cell::Cell<bool>,
}
pub(crate) fn prepare_width_diagnostics<'a>(
    diagnostics: impl IntoIterator<Item = &'a Rc<WidthDiagnostic>>,
    warnings: &mut [DocxWarning],
) {
    // Numbering has saved positions in this vector. Keep every source slot
    // until its existing finalizer has inserted and merged delayed warnings.
    let decisions: std::collections::BTreeMap<_, _> = diagnostics
        .into_iter()
        .map(|d| (d.warning_index, !d.grid.get() || d.container.get()))
        .collect();
    for (index, warning) in warnings.iter_mut().enumerate() {
        if let Some(retain) = decisions.get(&index) {
            if let DocxWarning::TableWidthApproximated { action, .. } = warning {
                *action = if *retain { "use_container" } else { "discard_table_width" };
            }
        }
    }
}
pub(crate) fn finish_width_diagnostics(warnings: &mut Vec<DocxWarning>) {
    warnings.retain(|warning| warning.action() != "discard_table_width");
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum TableAlignment {
    Left,
    Center,
    Right,
    Unsupported,
}
#[derive(Clone, Copy, Debug)]
pub(crate) enum TableProperty {
    Root,
    CellRoot,
    Borders,
    Margins,
    Leaf,
}
impl TableProperties {
    pub(crate) fn overlay(&mut self, other: &Self) {
        for (a, b) in self.borders.iter_mut().zip(other.borders) {
            if b.is_some() {
                *a = b;
            }
        }
        for (a, b) in self.border_widths.iter_mut().zip(other.border_widths) {
            if b.is_some() {
                *a = b;
            }
        }
        for (a, b) in self.margins.iter_mut().zip(other.margins) {
            if b.is_some() {
                *a = b;
            }
        }
        if other.indent.is_some() {
            self.indent = other.indent;
        }
        if other.alignment.is_some() {
            self.alignment = other.alignment;
        }
        if other.bidi.is_some() {
            self.bidi = other.bidi;
        }
        if other.width.is_some() {
            self.width = other.width;
            self.width_kind = other.width_kind;
            self.width_diagnostic = other.width_diagnostic.clone();
        }
    }
    // Direct tables are transient; transfer every declaration to the document owner
    // as part of reading it. Style definitions retain their own declaration list.
    pub(crate) fn read_and_collect(
        &mut self,
        state: TableProperty,
        e: &Element,
        source: (&str, &str),
        w: &mut Vec<DocxWarning>,
        diagnostics: &mut Vec<Rc<WidthDiagnostic>>,
    ) -> Result<TableProperty> {
        let state = self.read(state, e, source.0, source.1, w)?;
        diagnostics.append(&mut self.width_diagnostics);
        Ok(state)
    }
    pub(crate) fn read(
        &mut self,
        state: TableProperty,
        e: &Element,
        part: &str,
        at: &str,
        w: &mut Vec<DocxWarning>,
    ) -> Result<TableProperty> {
        if e.namespace_id != W_NS.0 {
            return omitted(part, at, w);
        }
        attributes(e, matches!(state, TableProperty::Root), part, at, w)?;
        match (state, e.name.as_str()) {
            (TableProperty::Root | TableProperty::CellRoot, "tblBorders" | "tcBorders") => {
                Ok(TableProperty::Borders)
            }
            (TableProperty::Root | TableProperty::CellRoot, "tblCellMar" | "tcMar") => {
                Ok(TableProperty::Margins)
            }
            // Only table roots own deferred width decisions. Cell roots use width()
            // below, so they cannot create a declaration without a collector.
            (TableProperty::Root, "tblW") => {
                self.width_kind = match attr(e, "type") {
                    None | Some("dxa") => WidthKind::Dxa,
                    Some("auto") => WidthKind::Auto,
                    Some("nil") => WidthKind::Nil,
                    Some("pct") => WidthKind::Pct,
                    _ => WidthKind::Unknown,
                };
                self.width_diagnostic = None;
                self.width = Some(if matches!(self.width_kind, WidthKind::Auto | WidthKind::Nil) {
                    let diagnostic = Rc::new(WidthDiagnostic {
                        warning_index: w.len(),
                        grid: false.into(),
                        container: false.into(),
                    });
                    let mut pending = Vec::new();
                    warn(&mut pending, part, at, Kind::TableWidthApproximated,
                        "table_width", "pending_table_width",
                        "Relative or automatic width uses the container and known column proportions")?;
                    w.extend(pending);
                    self.width_diagnostics.push(diagnostic.clone());
                    self.width_diagnostic = Some(diagnostic);
                    None
                } else {
                    width(e, part, at, w)?
                });
                Ok(TableProperty::Leaf)
            }
            (TableProperty::CellRoot, "tblW")
            | (TableProperty::Root | TableProperty::CellRoot, "tcW") => {
                self.width = Some(width(e, part, at, w)?);
                Ok(TableProperty::Leaf)
            }
            (TableProperty::Root, "tblInd") => {
                if !matches!(attr(e, "type"), None | Some("dxa")) {
                    return omitted(part, at, w);
                }
                let value = attr(e, "w")
                    .ok_or_else(|| DocxError::invalid(part, at, "missing table indent"))?;
                self.indent = Some(
                    super::units::length(value, 5, -HwpUnit::MAX_VALUE)
                        .map_err(|reason| DocxError::invalid(part, at, reason))?,
                );
                Ok(TableProperty::Leaf)
            }
            (TableProperty::Root, "jc") => {
                self.alignment = Some(match attr(e, "val") {
                    Some("left" | "start") => TableAlignment::Left,
                    Some("center") => TableAlignment::Center,
                    Some("right" | "end") => TableAlignment::Right,
                    _ => TableAlignment::Unsupported,
                });
                Ok(TableProperty::Leaf)
            }
            (TableProperty::Root, "bidiVisual") => {
                self.bidi = boolean(e, "val", Some(true), part, at, w)?;
                Ok(TableProperty::Leaf)
            }
            (TableProperty::Root | TableProperty::CellRoot, "shd") => {
                warn(
                    w,
                    part,
                    at,
                    Kind::ContentLost,
                    "cell_background",
                    "omit",
                    "Table/cell background is omitted",
                )?;
                Ok(TableProperty::Leaf)
            }
            (TableProperty::Borders, name) => {
                if let Some(index) = ["left", "right", "top", "bottom", "insideH", "insideV"]
                    .iter()
                    .position(|n| *n == name)
                {
                    let line = attr(e, "val");
                    let solid = !matches!(line, Some("none" | "nil"));
                    self.borders[index] = Some(solid);
                    self.border_widths[index] = Some(if solid {
                        let eighths = attr(e, "sz")
                            .unwrap_or("4")
                            .parse::<u32>()
                            .map_err(|_| DocxError::invalid(part, at, "invalid border width"))?;
                        // 1pt = 100 HwpUnit; round half a unit upward.
                        i32::try_from((u64::from(eighths) * 100 + 4) / 8)
                            .ok()
                            .filter(|width| *width <= HwpUnit::MAX_VALUE)
                            .ok_or_else(|| DocxError::invalid(part, at, "border width overflow"))?
                    } else {
                        0
                    });
                    if solid
                        && (line != Some("single")
                            || attr(e, "sz").is_some()
                            || attr(e, "color").is_some_and(|c| !matches!(c, "auto" | "000000"))
                            || e.attributes.iter().any(|(_, k, v)| {
                                !(matches!(k.as_str(), "val" | "sz" | "color")
                                    || k == "space" && v == "0")
                            }))
                    {
                        warn(
                            w,
                            part,
                            at,
                            Kind::TableBorderApproximated,
                            "table_border",
                            "use_default",
                            "Border line, width or color uses the default solid line",
                        )?;
                    }
                    Ok(TableProperty::Leaf)
                } else {
                    omitted(part, at, w)
                }
            }
            (TableProperty::Margins, name) => {
                if let Some(index) =
                    ["left", "right", "top", "bottom"].iter().position(|n| *n == name)
                {
                    if !matches!(attr(e, "type"), None | Some("dxa")) {
                        return omitted(part, at, w);
                    }
                    self.margins[index] =
                        Some(length(e, "w", StructuralDimension::Margin, part, at)?);
                    Ok(TableProperty::Leaf)
                } else {
                    omitted(part, at, w)
                }
            }
            // Layout algorithm and look masks cannot change the supported uniform style.
            (TableProperty::Root | TableProperty::CellRoot, "tblLayout" | "tblLook") => {
                omitted(part, at, w)
            }
            _ => omitted(part, at, w),
        }
    }
}
pub(crate) fn length(
    e: &Element,
    key: &str,
    dimension: StructuralDimension,
    part: &str,
    at: &str,
) -> Result<i32> {
    let value =
        attr(e, key).ok_or_else(|| DocxError::invalid(part, at, "missing structural length"))?;
    structural_length(value, dimension).map_err(|reason| DocxError::invalid(part, at, reason))
}
pub(crate) fn width(
    e: &Element,
    part: &str,
    at: &str,
    w: &mut Vec<DocxWarning>,
) -> Result<Option<i32>> {
    match attr(e, "type") {
        None | Some("dxa") => Ok(Some(length(e, "w", StructuralDimension::Width, part, at)?)),
        Some("auto" | "nil" | "pct") => {
            warn(
                w,
                part,
                at,
                Kind::TableWidthApproximated,
                "table_width",
                "use_container",
                "Relative or automatic width uses the container and known column proportions",
            )?;
            Ok(None)
        }
        _ => {
            warn(
                w,
                part,
                at,
                Kind::UnsupportedAttribute,
                "table_width_type",
                "omit",
                "Unknown width type omitted; width uses the container",
            )?;
            Ok(None)
        }
    }
}
fn omitted(part: &str, at: &str, w: &mut Vec<DocxWarning>) -> Result<TableProperty> {
    warn(
        w,
        part,
        at,
        Kind::UnsupportedAttribute,
        "table_property",
        "omit",
        "Unsupported table property is omitted",
    )?;
    Ok(TableProperty::Leaf)
}

// Attribute names are finite; input strings never become diagnostic keys.
pub(crate) fn attributes(
    e: &Element,
    table_root: bool,
    part: &str,
    at: &str,
    w: &mut Vec<DocxWarning>,
) -> Result<()> {
    let known: &[&str] = match e.name.as_str() {
        "tblW" | "tcW" | "tblInd" => &["w", "type"],
        "gridCol" => &["w"],
        "jc" | "bidiVisual" if table_root => &["val"],
        "gridBefore" | "gridAfter" | "gridSpan" | "vMerge" | "vAlign" | "tblStyle" => &["val"],
        "trHeight" => &["val", "hRule"],
        "left" | "right" | "top" | "bottom" | "insideH" | "insideV" => {
            &["w", "type", "val", "sz", "color", "space", "themeColor"]
        }
        "shd" | "tblLook" | "tblLayout" => return Ok(()), // The whole property is already diagnosed.
        _ => &[],
    };
    for (ns, key, _) in &e.attributes {
        if *ns == W_NS.0
            && (known.contains(&key.as_str())
                || matches!(key.as_str(), "rsidR" | "rsidRPr" | "rsidRDefault" | "rsidTr"))
        {
            continue;
        }
        warn(
            w,
            part,
            at,
            Kind::UnsupportedAttribute,
            "table_attribute",
            "omit",
            "Unsupported table attribute omitted",
        )?;
    }
    Ok(())
}
