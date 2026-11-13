//! Input-local whole-table properties shared by style definitions and direct properties.
use super::*;
use hwpforge_foundation::HwpUnit;

// left, right, top, bottom, insideH, insideV. None means inherit, not no line.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct TableProperties {
    pub(crate) borders: [Option<bool>; 6],
    // Source widths in HwpUnit, independent of the approximated output strokes.
    pub(crate) border_widths: [Option<i32>; 6],
    pub(crate) width: Option<Option<i32>>,
    pub(crate) margins: [Option<i32>; 4],
}
#[derive(Clone, Copy, Debug)]
pub(crate) enum TableProperty {
    Root,
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
        if other.width.is_some() {
            self.width = other.width;
        }
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
        attributes(e, part, at, w)?;
        match (state, e.name.as_str()) {
            (TableProperty::Root, "tblBorders" | "tcBorders") => Ok(TableProperty::Borders),
            (TableProperty::Root, "tblCellMar" | "tcMar") => Ok(TableProperty::Margins),
            (TableProperty::Root, "tblW" | "tcW") => {
                self.width = Some(width(e, part, at, w)?);
                Ok(TableProperty::Leaf)
            }
            (TableProperty::Root, "shd") => {
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
            (TableProperty::Root, "tblLayout" | "tblLook") => omitted(part, at, w),
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
    part: &str,
    at: &str,
    w: &mut Vec<DocxWarning>,
) -> Result<()> {
    let known: &[&str] = match e.name.as_str() {
        "tblW" | "tcW" | "tblInd" => &["w", "type"],
        "gridCol" => &["w"],
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
