//! OOXML chart XML generator.
//!
//! Generates `<c:chartSpace>` XML documents for embedding as `Chart/chartN.xml`
//! files within the HWPX ZIP archive. Uses template-based `write!()` approach
//! (not serde) because the OOXML chart namespace differs from HWPX's.

use std::fmt::Write;

use hwpforge_core::chart::{
    BarShape, ChartData, ChartGrouping, ChartSeries, ChartType, LegendPosition, OfPieType,
    RadarStyle, ScatterStyle, StockVariant, XySeries,
};
use hwpforge_core::control::Control;

use crate::encoder::escape_xml;
use crate::error::{HwpxError, HwpxResult};

/// OOXML chart namespace.
const CHART_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
/// OOXML relationships namespace.
const REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
/// OOXML drawing namespace.
const DRAW_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";

/// Generates an OOXML chart XML string from a `Control::Chart`.
///
/// Returns a complete XML document suitable for writing to `Chart/chartN.xml`.
pub(crate) fn generate_chart_xml(ctrl: &Control) -> HwpxResult<String> {
    let (
        chart_type,
        data,
        title,
        legend,
        grouping,
        bar_shape,
        explosion,
        of_pie_type,
        radar_style,
        wireframe,
        bubble_3d,
        scatter_style,
        show_markers,
        stock_variant,
    ) = match ctrl {
        Control::Chart {
            chart_type,
            data,
            title,
            legend,
            grouping,
            bar_shape,
            explosion,
            of_pie_type,
            radar_style,
            wireframe,
            bubble_3d,
            scatter_style,
            show_markers,
            stock_variant,
            ..
        } => (
            chart_type,
            data,
            title,
            legend,
            grouping,
            bar_shape,
            explosion,
            of_pie_type,
            radar_style,
            wireframe,
            bubble_3d,
            scatter_style,
            show_markers,
            stock_variant,
        ),
        _ => {
            return Err(HwpxError::InvalidStructure {
                detail: "generate_chart_xml called with non-Chart control".to_string(),
            })
        }
    };

    let mut xml = String::with_capacity(4096);
    write!(
        xml,
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?><c:chartSpace xmlns:r="{REL_NS}" xmlns:a="{DRAW_NS}" xmlns:c="{CHART_NS}">"#,
    )
    .unwrap();

    // chartSpace-level settings (required by 한글)
    xml.push_str(r#"<c:date1904 val="0"/><c:roundedCorners val="0"/>"#);

    xml.push_str("<c:chart>");

    // Title
    if let Some(t) = title {
        write_title(&mut xml, t);
    }
    xml.push_str(r#"<c:autoTitleDeleted val="0"/>"#);

    // 3D perspective (required by 한글 for all 3D chart types)
    if is_3d_chart(*chart_type) {
        // Surface3D uses rAngAx="0", others use "1" (matching real 한글 output)
        let r_ang = if *chart_type == ChartType::Surface3D { "0" } else { "1" };
        write!(
            xml,
            r#"<c:view3D><c:rAngAx val="{r_ang}"/><c:rotX val="15"/><c:rotY val="20"/><c:perspective val="30"/><c:depthPercent val="100"/></c:view3D>"#,
        )
        .unwrap();
    }

    // Plot area
    let is_volume_stock =
        matches!(stock_variant, Some(StockVariant::Vhlc) | Some(StockVariant::Vohlc));
    xml.push_str("<c:plotArea><c:layout/>");

    if is_volume_stock {
        // Composite plotArea: barChart (volume) + stockChart (price)
        write_stock_volume_chart_element(&mut xml, data, *grouping)?;
    } else {
        write_chart_type_element(
            &mut xml,
            *chart_type,
            data,
            *grouping,
            *bar_shape,
            *explosion,
            *of_pie_type,
            *radar_style,
            *wireframe,
            *bubble_3d,
            *scatter_style,
            *show_markers,
        )?;
    }

    // Axes (pie/doughnut/ofPie have none)
    if needs_axes(*chart_type) {
        if is_volume_stock {
            // 4-axis layout for combo chart (standard OOXML approach):
            // Primary: catAx(1) + valAx(2) — stockChart (price, left)
            // Secondary: catAx(3, hidden) + valAx(4) — barChart (volume, right)
            xml.push_str(r#"<c:catAx><c:axId val="1"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:axPos val="b"/><c:crossAx val="2"/><c:delete val="0"/><c:majorTickMark val="out"/><c:minorTickMark val="none"/><c:tickLblPos val="nextTo"/><c:crosses val="autoZero"/><c:auto val="1"/><c:lblAlgn val="ctr"/><c:lblOffset val="100"/></c:catAx>"#);
            xml.push_str(r#"<c:valAx><c:axId val="2"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:axPos val="l"/><c:crossAx val="1"/><c:delete val="0"/><c:majorGridlines/><c:numFmt formatCode="General" sourceLinked="1"/><c:majorTickMark val="out"/><c:minorTickMark val="none"/><c:tickLblPos val="nextTo"/><c:crosses val="autoZero"/><c:crossBetween val="between"/></c:valAx>"#);
            xml.push_str(r#"<c:catAx><c:axId val="3"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:axPos val="b"/><c:crossAx val="4"/><c:delete val="1"/><c:majorTickMark val="out"/><c:minorTickMark val="none"/><c:tickLblPos val="nextTo"/><c:crosses val="autoZero"/><c:auto val="1"/><c:lblAlgn val="ctr"/><c:lblOffset val="100"/></c:catAx>"#);
            xml.push_str(r#"<c:valAx><c:axId val="4"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:axPos val="r"/><c:crossAx val="3"/><c:delete val="0"/><c:numFmt formatCode="General" sourceLinked="1"/><c:majorTickMark val="out"/><c:minorTickMark val="none"/><c:tickLblPos val="nextTo"/><c:crosses val="max"/><c:crossBetween val="between"/></c:valAx>"#);
        } else if is_xy_chart(*chart_type) {
            // Scatter/Bubble: two value axes
            xml.push_str(r#"<c:valAx><c:axId val="1"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:axPos val="b"/><c:crossAx val="2"/><c:delete val="0"/><c:majorTickMark val="out"/><c:minorTickMark val="none"/><c:tickLblPos val="nextTo"/><c:crosses val="autoZero"/></c:valAx>"#);
            xml.push_str(r#"<c:valAx><c:axId val="2"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:axPos val="l"/><c:crossAx val="1"/><c:delete val="0"/><c:majorTickMark val="out"/><c:minorTickMark val="none"/><c:tickLblPos val="nextTo"/><c:crosses val="autoZero"/></c:valAx>"#);
        } else {
            // Category + Value axis
            xml.push_str(r#"<c:catAx><c:axId val="1"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:axPos val="b"/><c:crossAx val="2"/><c:delete val="0"/><c:majorTickMark val="out"/><c:minorTickMark val="none"/><c:tickLblPos val="nextTo"/><c:crosses val="autoZero"/><c:auto val="1"/><c:lblAlgn val="ctr"/><c:lblOffset val="100"/></c:catAx>"#);
            xml.push_str(r#"<c:valAx><c:axId val="2"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:axPos val="l"/><c:crossAx val="1"/><c:delete val="0"/><c:majorGridlines/><c:numFmt formatCode="General" sourceLinked="1"/><c:majorTickMark val="out"/><c:minorTickMark val="none"/><c:tickLblPos val="nextTo"/><c:crosses val="autoZero"/><c:crossBetween val="between"/></c:valAx>"#);
        }
    }

    xml.push_str("</c:plotArea>");

    // Legend
    write_legend(&mut xml, *legend);

    // Chart-level settings
    xml.push_str(r#"<c:plotVisOnly val="0"/><c:dispBlanksAs val="gap"/>"#);

    xml.push_str("</c:chart></c:chartSpace>");
    Ok(xml)
}

/// Returns `true` if this chart type requires axis elements.
fn needs_axes(ct: ChartType) -> bool {
    !matches!(ct, ChartType::Pie | ChartType::Pie3D | ChartType::Doughnut | ChartType::OfPie)
}

/// Returns `true` if this chart type uses XY data (two value axes).
fn is_xy_chart(ct: ChartType) -> bool {
    matches!(ct, ChartType::Scatter | ChartType::Bubble)
}

/// Returns `true` if this is a pie-family chart (varies colors per data point).
fn is_pie_family(ct: ChartType) -> bool {
    matches!(ct, ChartType::Pie | ChartType::Pie3D | ChartType::Doughnut | ChartType::OfPie)
}

/// Returns `true` if this is a 3D chart type requiring `<c:view3D>`.
fn is_3d_chart(ct: ChartType) -> bool {
    matches!(
        ct,
        ChartType::Bar3D
            | ChartType::Column3D
            | ChartType::Line3D
            | ChartType::Pie3D
            | ChartType::Area3D
            | ChartType::Surface3D
    )
}

/// Returns the OOXML element name and optional barDir for a chart type.
fn chart_element_name(ct: ChartType) -> (&'static str, Option<&'static str>) {
    match ct {
        ChartType::Bar => ("barChart", Some("bar")),
        ChartType::Column => ("barChart", Some("col")),
        ChartType::Bar3D => ("bar3DChart", Some("bar")),
        ChartType::Column3D => ("bar3DChart", Some("col")),
        ChartType::Line => ("lineChart", None),
        ChartType::Line3D => ("line3DChart", None),
        ChartType::Pie => ("pieChart", None),
        ChartType::Pie3D => ("pie3DChart", None),
        ChartType::Doughnut => ("doughnutChart", None),
        ChartType::OfPie => ("ofPieChart", None),
        ChartType::Area => ("areaChart", None),
        ChartType::Area3D => ("area3DChart", None),
        ChartType::Scatter => ("scatterChart", None),
        ChartType::Bubble => ("bubbleChart", None),
        ChartType::Radar => ("radarChart", None),
        ChartType::Surface => ("surfaceChart", None),
        ChartType::Surface3D => ("surface3DChart", None),
        ChartType::Stock => ("stockChart", None),
        _ => ("barChart", None), // Forward-compatible fallback (#[non_exhaustive])
    }
}

/// Writes the grouping OOXML value string.
fn grouping_val(g: ChartGrouping) -> &'static str {
    match g {
        ChartGrouping::Clustered => "clustered",
        ChartGrouping::Stacked => "stacked",
        ChartGrouping::PercentStacked => "percentStacked",
        ChartGrouping::Standard => "standard",
    }
}

/// Writes the chart type element with series data.
#[allow(clippy::too_many_arguments)]
fn write_chart_type_element(
    xml: &mut String,
    ct: ChartType,
    data: &ChartData,
    grouping: ChartGrouping,
    bar_shape: Option<BarShape>,
    explosion: Option<u32>,
    of_pie_type: Option<OfPieType>,
    radar_style: Option<RadarStyle>,
    wireframe: Option<bool>,
    bubble_3d: Option<bool>,
    scatter_style: Option<ScatterStyle>,
    show_markers: Option<bool>,
) -> HwpxResult<()> {
    let (elem, bar_dir) = chart_element_name(ct);

    write!(xml, "<c:{elem}>").unwrap();

    // barDir (only for barChart/bar3DChart)
    if let Some(dir) = bar_dir {
        write!(xml, r#"<c:barDir val="{dir}"/>"#).unwrap();
    }

    // Grouping (skip for pie/doughnut/ofPie/stock/scatter/bubble)
    if !matches!(
        ct,
        ChartType::Pie
            | ChartType::Pie3D
            | ChartType::Doughnut
            | ChartType::OfPie
            | ChartType::Stock
            | ChartType::Scatter
            | ChartType::Bubble
    ) {
        write!(xml, r#"<c:grouping val="{}"/>"#, grouping_val(grouping)).unwrap();
    }

    // varyColors: "1" for pie-family (distinct per-point colors), "0" for others
    let vary = if is_pie_family(ct) { "1" } else { "0" };
    write!(xml, r#"<c:varyColors val="{vary}"/>"#).unwrap();

    // scatterStyle (for scatter charts)
    if matches!(ct, ChartType::Scatter) {
        let style_val = match scatter_style {
            Some(ScatterStyle::LineMarker) => "lineMarker",
            Some(ScatterStyle::SmoothMarker) => "smoothMarker",
            Some(ScatterStyle::Line) => "line",
            Some(ScatterStyle::Smooth) => "smooth",
            Some(ScatterStyle::Dots) | None => "lineMarker",
        };
        write!(xml, r#"<c:scatterStyle val="{style_val}"/>"#).unwrap();
    }

    // radarStyle (for radar charts)
    if matches!(ct, ChartType::Radar) {
        let style_val = match radar_style {
            Some(RadarStyle::Marker) => "marker",
            Some(RadarStyle::Filled) => "filled",
            Some(RadarStyle::Standard) | None => "standard",
        };
        write!(xml, r#"<c:radarStyle val="{style_val}"/>"#).unwrap();
    }

    // ofPieType (for ofPie charts)
    if matches!(ct, ChartType::OfPie) {
        let type_val = match of_pie_type {
            Some(OfPieType::Bar) => "bar",
            Some(OfPieType::Pie) | None => "pie",
        };
        write!(xml, r#"<c:ofPieType val="{type_val}"/>"#).unwrap();
    }

    // wireframe (for surface charts)
    if matches!(ct, ChartType::Surface | ChartType::Surface3D) && wireframe == Some(true) {
        xml.push_str(r#"<c:wireframe val="1"/>"#);
    }

    // Series data
    let pie = is_pie_family(ct);
    let is_bubble = matches!(ct, ChartType::Bubble);
    let is_line = matches!(ct, ChartType::Line | ChartType::Line3D);
    match data {
        ChartData::Category { categories, series } => {
            for (idx, s) in series.iter().enumerate() {
                write_category_series(
                    xml,
                    idx,
                    s,
                    categories,
                    pie,
                    explosion,
                    is_line,
                    show_markers,
                );
            }
        }
        ChartData::Xy { series } => {
            for (idx, s) in series.iter().enumerate() {
                write_xy_series(xml, idx, s, is_bubble, bubble_3d);
            }
        }
    }

    // Chart-type-specific trailing elements (must appear after series, before axId)
    match ct {
        ChartType::Pie | ChartType::Pie3D => {
            xml.push_str(r#"<c:firstSliceAng val="0"/>"#);
        }
        ChartType::Doughnut => {
            xml.push_str(r#"<c:holeSize val="50"/>"#);
        }
        ChartType::Bar3D | ChartType::Column3D => {
            // 3D bar shape (must appear after series)
            if let Some(shape) = bar_shape {
                let shape_val = match shape {
                    BarShape::Box => "box",
                    BarShape::Cylinder => "cylinder",
                    BarShape::Cone => "cone",
                    BarShape::Pyramid => "pyramid",
                };
                if shape != BarShape::Box {
                    write!(xml, r#"<c:shape val="{shape_val}"/>"#).unwrap();
                }
            }
        }
        _ => {}
    }

    // Axis references (for bar/line/area charts)
    if needs_axes(ct) {
        xml.push_str(r#"<c:axId val="1"/><c:axId val="2"/>"#);
    }

    write!(xml, "</c:{elem}>").unwrap();
    Ok(())
}

/// Writes a category-based series (`<c:ser>`).
///
/// When `pie_family` is `true`, emits `<c:explosion val="N"/>` per series.
/// When `show_markers` is `true` for line charts, emits a marker block.
#[allow(clippy::too_many_arguments)]
fn write_category_series(
    xml: &mut String,
    idx: usize,
    s: &ChartSeries,
    categories: &[String],
    pie_family: bool,
    explosion: Option<u32>,
    is_line: bool,
    show_markers: Option<bool>,
) {
    write!(xml, "<c:ser><c:idx val=\"{idx}\"/><c:order val=\"{idx}\"/>").unwrap();

    // Series name (direct value, matching 한글 output)
    write!(xml, "<c:tx><c:v>{}</c:v></c:tx>", escape_xml(&s.name)).unwrap();

    // Pie-family: explosion attribute (required by 한글 for correct rendering)
    if pie_family {
        let exp_val = explosion.unwrap_or(0);
        write!(xml, r#"<c:explosion val="{exp_val}"/>"#).unwrap();
    } else {
        xml.push_str(r#"<c:invertIfNegative val="0"/>"#);
    }

    // Line chart markers
    if is_line && show_markers == Some(true) {
        xml.push_str(r#"<c:marker><c:symbol val="circle"/><c:size val="5"/></c:marker>"#);
    }

    // Categories (c:f is required by 한글 to read cache data)
    if !categories.is_empty() {
        let end_row = categories.len() + 1;
        write!(
            xml,
            r#"<c:cat><c:strRef><c:f>Sheet1!$A$2:$A${end_row}</c:f><c:strCache><c:ptCount val="{}"/>"#,
            categories.len()
        )
        .unwrap();
        for (i, cat) in categories.iter().enumerate() {
            write!(xml, "<c:pt idx=\"{i}\"><c:v>{}</c:v></c:pt>", escape_xml(cat)).unwrap();
        }
        xml.push_str("</c:strCache></c:strRef></c:cat>");
    }

    // Values (c:f is required by 한글 to read cache data)
    // Guard: column letters A-Z support at most 25 value series (B..Z).
    // Series beyond index 24 are silently skipped to avoid non-letter characters.
    if idx >= 25 {
        xml.push_str("</c:ser>");
        return;
    }
    let val_col = (b'B' + idx as u8) as char;
    let end_row = s.values.len() + 1;
    write!(
        xml,
        r#"<c:val><c:numRef><c:f>Sheet1!${val_col}$2:${val_col}${end_row}</c:f><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val="{}"/>"#,
        s.values.len()
    )
    .unwrap();
    for (i, v) in s.values.iter().enumerate() {
        write!(xml, "<c:pt idx=\"{i}\"><c:v>{v}</c:v></c:pt>").unwrap();
    }
    xml.push_str("</c:numCache></c:numRef></c:val>");

    xml.push_str("</c:ser>");
}

/// Writes an XY-based series (`<c:ser>` with xVal/yVal).
///
/// For bubble charts, emits `<c:bubble3D val="1"/>` when `bubble_3d` is `true`.
fn write_xy_series(
    xml: &mut String,
    idx: usize,
    s: &XySeries,
    is_bubble: bool,
    bubble_3d: Option<bool>,
) {
    write!(xml, "<c:ser><c:idx val=\"{idx}\"/><c:order val=\"{idx}\"/>").unwrap();

    // Series name (direct value, matching 한글 output)
    write!(xml, "<c:tx><c:v>{}</c:v></c:tx>", escape_xml(&s.name)).unwrap();

    xml.push_str(r#"<c:invertIfNegative val="0"/>"#);

    // Bubble 3D effect
    if is_bubble && bubble_3d == Some(true) {
        xml.push_str(r#"<c:bubble3D val="1"/>"#);
    }

    // Guard: XY series use two columns each (A+B, C+D, …). With 26 letters
    // (A-Z), at most 13 XY series fit. Series beyond index 12 are skipped.
    if idx >= 13 {
        xml.push_str("</c:ser>");
        return;
    }

    // X values (c:f required by 한글)
    let x_col = (b'A' + (idx as u8) * 2) as char;
    let end_row = s.x_values.len() + 1;
    write!(
        xml,
        r#"<c:xVal><c:numRef><c:f>Sheet1!${x_col}$2:${x_col}${end_row}</c:f><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val="{}"/>"#,
        s.x_values.len()
    )
    .unwrap();
    for (i, v) in s.x_values.iter().enumerate() {
        write!(xml, "<c:pt idx=\"{i}\"><c:v>{v}</c:v></c:pt>").unwrap();
    }
    xml.push_str("</c:numCache></c:numRef></c:xVal>");

    // Y values (c:f required by 한글)
    let y_col = (b'B' + (idx as u8) * 2) as char;
    write!(
        xml,
        r#"<c:yVal><c:numRef><c:f>Sheet1!${y_col}$2:${y_col}${end_row}</c:f><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val="{}"/>"#,
        s.y_values.len()
    )
    .unwrap();
    for (i, v) in s.y_values.iter().enumerate() {
        write!(xml, "<c:pt idx=\"{i}\"><c:v>{v}</c:v></c:pt>").unwrap();
    }
    xml.push_str("</c:numCache></c:numRef></c:yVal>");

    xml.push_str("</c:ser>");
}

/// Writes the chart title element.
///
/// Uses `a:` (DrawingML) namespace inside `<c:rich>` as required by OOXML.
/// Includes `<a:bodyPr/>`, `<a:lstStyle/>`, and `<a:rPr lang="ko-KR"/>` so
/// that 한글 recognises the custom title instead of falling back to "차트 제목".
fn write_title(xml: &mut String, title: &str) {
    write!(
        xml,
        r#"<c:title><c:tx><c:rich><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="ko-KR"/><a:t>{}</a:t></a:r></a:p></c:rich></c:tx><c:layout/><c:overlay val="0"/></c:title>"#,
        escape_xml(title),
    )
    .unwrap();
}

/// Writes the legend element.
fn write_legend(xml: &mut String, pos: LegendPosition) {
    let val = match pos {
        LegendPosition::Right => "r",
        LegendPosition::Bottom => "b",
        LegendPosition::Top => "t",
        LegendPosition::Left => "l",
        LegendPosition::None => return, // No legend element
    };
    write!(xml, r#"<c:legend><c:legendPos val="{val}"/></c:legend>"#).unwrap();
}

/// Writes a composite plotArea for VHLC/VOHLC stock charts.
///
/// Uses the standard OOXML 4-axis layout for combo charts:
/// - stockChart (primary): catAx(1) + valAx(2) — price on left axis
/// - barChart (secondary): catAx(3, hidden) + valAx(4) — volume on right axis
///
/// The data series order convention:
/// - Series 0: Volume (bar chart, secondary axes)
/// - Series 1+: High, Low, Close (and optionally Open before H for VOHLC)
fn write_stock_volume_chart_element(
    xml: &mut String,
    data: &ChartData,
    grouping: ChartGrouping,
) -> HwpxResult<()> {
    let (categories, series) = match data {
        ChartData::Category { categories, series } => (categories, series),
        ChartData::Xy { .. } => {
            return Err(HwpxError::InvalidStructure {
                detail: "Stock VHLC/VOHLC requires Category data".to_string(),
            })
        }
    };

    // Split volume (first series) from price series (rest)
    let (volume_series, price_series) =
        if series.is_empty() { (&[][..], &[][..]) } else { (&series[..1], &series[1..]) };

    // barChart for volume — secondary axis pair (catAx 3 + valAx 4)
    write!(
        xml,
        r#"<c:barChart><c:barDir val="col"/><c:grouping val="{}"/><c:varyColors val="0"/>"#,
        grouping_val(grouping)
    )
    .unwrap();
    for (idx, s) in volume_series.iter().enumerate() {
        write_category_series(xml, idx, s, categories, false, None, false, None);
    }
    xml.push_str(r#"<c:axId val="3"/><c:axId val="4"/></c:barChart>"#);

    // stockChart for price series — primary axis pair (catAx 1 + valAx 2)
    xml.push_str("<c:stockChart>");
    let price_offset = volume_series.len();
    for (idx, s) in price_series.iter().enumerate() {
        write_category_series(xml, price_offset + idx, s, categories, false, None, false, None);
    }
    // Stock chart trailing elements (hiLowLines required for both HLC and OHLC)
    xml.push_str(r#"<c:hiLowLines/><c:axId val="1"/><c:axId val="2"/></c:stockChart>"#);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_core::chart::ChartData;
    use hwpforge_foundation::HwpUnit;

    fn make_chart_control(ct: ChartType, data: ChartData) -> Control {
        Control::Chart {
            chart_type: ct,
            data,
            width: HwpUnit::new(32250).unwrap(),
            height: HwpUnit::new(18750).unwrap(),
            title: Some("Test Chart".to_string()),
            legend: LegendPosition::Bottom,
            grouping: ChartGrouping::Clustered,
            bar_shape: None,
            explosion: None,
            of_pie_type: None,
            radar_style: None,
            wireframe: None,
            bubble_3d: None,
            scatter_style: None,
            show_markers: None,
            stock_variant: None,
        }
    }

    #[test]
    fn generate_bar_chart_xml() {
        let data = ChartData::category(&["A", "B", "C"], &[("Sales", &[10.0, 20.0, 30.0])]);
        let ctrl = make_chart_control(ChartType::Bar, data);
        let xml = generate_chart_xml(&ctrl).unwrap();
        assert!(xml.contains("<c:chartSpace"), "missing chartSpace root");
        assert!(xml.contains("<c:barChart>"), "missing barChart element");
        assert!(xml.contains(r#"<c:barDir val="bar"/>"#), "missing barDir=bar");
        assert!(xml.contains("<c:catAx>"), "bar chart needs category axis");
        assert!(xml.contains("<c:valAx>"), "bar chart needs value axis");
        assert!(xml.contains("Test Chart"), "missing title");
    }

    #[test]
    fn generate_column_chart_xml() {
        let data = ChartData::category(&["X"], &[("S", &[1.0])]);
        let ctrl = make_chart_control(ChartType::Column, data);
        let xml = generate_chart_xml(&ctrl).unwrap();
        assert!(xml.contains(r#"<c:barDir val="col"/>"#), "column should have barDir=col");
    }

    #[test]
    fn generate_pie_chart_no_axes() {
        let data = ChartData::category(&["A", "B"], &[("Slice", &[60.0, 40.0])]);
        let ctrl = make_chart_control(ChartType::Pie, data);
        let xml = generate_chart_xml(&ctrl).unwrap();
        assert!(xml.contains("<c:pieChart>"), "missing pieChart");
        assert!(!xml.contains("<c:catAx>"), "pie should have no category axis");
        assert!(!xml.contains("<c:valAx>"), "pie should have no value axis");
    }

    #[test]
    fn generate_scatter_chart_xy() {
        let data = ChartData::xy(&[("Points", &[1.0, 2.0], &[3.0, 4.0])]);
        let ctrl = make_chart_control(ChartType::Scatter, data);
        let xml = generate_chart_xml(&ctrl).unwrap();
        assert!(xml.contains("<c:scatterChart>"), "missing scatterChart");
        assert!(xml.contains("<c:xVal>"), "scatter needs xVal");
        assert!(xml.contains("<c:yVal>"), "scatter needs yVal");
        // Scatter uses two valAx
        let val_ax_count = xml.matches("<c:valAx>").count();
        assert_eq!(val_ax_count, 2, "scatter needs 2 value axes");
    }

    #[test]
    fn generate_line_chart_with_grouping() {
        let data = ChartData::category(&["A"], &[("S", &[5.0])]);
        let ctrl = Control::Chart {
            chart_type: ChartType::Line,
            data,
            width: HwpUnit::new(32250).unwrap(),
            height: HwpUnit::new(18750).unwrap(),
            title: None,
            legend: LegendPosition::None,
            grouping: ChartGrouping::Stacked,
            bar_shape: None,
            explosion: None,
            of_pie_type: None,
            radar_style: None,
            wireframe: None,
            bubble_3d: None,
            scatter_style: None,
            show_markers: None,
            stock_variant: None,
        };
        let xml = generate_chart_xml(&ctrl).unwrap();
        assert!(xml.contains("<c:lineChart>"), "missing lineChart");
        assert!(xml.contains(r#"<c:grouping val="stacked"/>"#), "missing stacked grouping");
        assert!(!xml.contains("<c:legend>"), "None legend should omit element");
        assert!(!xml.contains("<c:title>"), "None title should omit element");
    }

    #[test]
    fn escape_xml_special_chars() {
        let result = escape_xml("a < b & c > d \"e\"");
        assert_eq!(result, "a &lt; b &amp; c &gt; d &quot;e&quot;");
    }

    #[test]
    fn all_18_chart_types_generate_valid_xml() {
        let types = [
            ChartType::Bar,
            ChartType::Column,
            ChartType::Bar3D,
            ChartType::Column3D,
            ChartType::Line,
            ChartType::Line3D,
            ChartType::Pie,
            ChartType::Pie3D,
            ChartType::Doughnut,
            ChartType::OfPie,
            ChartType::Area,
            ChartType::Area3D,
            ChartType::Scatter,
            ChartType::Bubble,
            ChartType::Radar,
            ChartType::Surface,
            ChartType::Surface3D,
            ChartType::Stock,
        ];
        for ct in types {
            let data = if is_xy_chart(ct) {
                ChartData::xy(&[("S", &[1.0], &[2.0])])
            } else {
                ChartData::category(&["A"], &[("S", &[1.0])])
            };
            let ctrl = make_chart_control(ct, data);
            let xml = generate_chart_xml(&ctrl).unwrap();
            assert!(xml.starts_with("<?xml"), "{ct:?} should start with XML decl");
            assert!(xml.contains("<c:chartSpace"), "{ct:?} missing chartSpace");
            assert!(xml.contains("</c:chartSpace>"), "{ct:?} missing closing tag");
        }
    }

    #[test]
    fn vhlc_stock_generates_composite_plot_area() {
        use hwpforge_core::chart::StockVariant;
        let data = ChartData::category(
            &["Mon", "Tue", "Wed"],
            &[
                ("Volume", &[1000.0, 1500.0, 1200.0]),
                ("High", &[110.0, 115.0, 112.0]),
                ("Low", &[100.0, 105.0, 102.0]),
                ("Close", &[108.0, 112.0, 109.0]),
            ],
        );
        let ctrl = Control::Chart {
            chart_type: ChartType::Stock,
            data,
            width: HwpUnit::new(32250).unwrap(),
            height: HwpUnit::new(18750).unwrap(),
            title: None,
            legend: LegendPosition::Right,
            grouping: ChartGrouping::Clustered,
            bar_shape: None,
            explosion: None,
            of_pie_type: None,
            radar_style: None,
            wireframe: None,
            bubble_3d: None,
            scatter_style: None,
            show_markers: None,
            stock_variant: Some(StockVariant::Vhlc),
        };
        let xml = generate_chart_xml(&ctrl).unwrap();
        assert!(xml.contains("<c:barChart>"), "VHLC needs barChart for volume");
        assert!(xml.contains("<c:stockChart>"), "VHLC needs stockChart for price");
        let val_ax_count = xml.matches("<c:valAx>").count();
        assert_eq!(val_ax_count, 2, "VHLC needs 2 valAx (price + volume)");
        let cat_ax_count = xml.matches("<c:catAx>").count();
        assert_eq!(cat_ax_count, 2, "VHLC needs 2 catAx (primary + hidden secondary)");
        // barChart should use secondary axis pair (3+4)
        assert!(
            xml.contains(r#"<c:axId val="3"/><c:axId val="4"/></c:barChart>"#),
            "barChart should use axId 3+4"
        );
        // stockChart should use primary axis pair (1+2)
        assert!(
            xml.contains(r#"<c:axId val="1"/><c:axId val="2"/></c:stockChart>"#),
            "stockChart should use axId 1+2"
        );
    }

    #[test]
    fn bar_shape_cylinder_encodes_correctly() {
        let data = ChartData::category(&["A", "B"], &[("S", &[10.0, 20.0])]);
        let ctrl = Control::Chart {
            chart_type: ChartType::Bar3D,
            data,
            width: HwpUnit::new(32250).unwrap(),
            height: HwpUnit::new(18750).unwrap(),
            title: None,
            legend: LegendPosition::Right,
            grouping: ChartGrouping::Clustered,
            bar_shape: Some(BarShape::Cylinder),
            explosion: None,
            of_pie_type: None,
            radar_style: None,
            wireframe: None,
            bubble_3d: None,
            scatter_style: None,
            show_markers: None,
            stock_variant: None,
        };
        let xml = generate_chart_xml(&ctrl).unwrap();
        assert!(xml.contains(r#"<c:shape val="cylinder"/>"#), "cylinder shape missing");
    }

    #[test]
    fn explosion_encodes_in_pie_series() {
        let data = ChartData::category(&["A", "B"], &[("Slice", &[60.0, 40.0])]);
        let ctrl = Control::Chart {
            chart_type: ChartType::Pie,
            data,
            width: HwpUnit::new(32250).unwrap(),
            height: HwpUnit::new(18750).unwrap(),
            title: None,
            legend: LegendPosition::Right,
            grouping: ChartGrouping::Clustered,
            bar_shape: None,
            explosion: Some(25),
            of_pie_type: None,
            radar_style: None,
            wireframe: None,
            bubble_3d: None,
            scatter_style: None,
            show_markers: None,
            stock_variant: None,
        };
        let xml = generate_chart_xml(&ctrl).unwrap();
        assert!(xml.contains(r#"<c:explosion val="25"/>"#), "explosion 25% missing");
    }

    #[test]
    fn scatter_style_smooth_marker_encodes() {
        let data = ChartData::xy(&[("S", &[1.0, 2.0], &[3.0, 4.0])]);
        let ctrl = Control::Chart {
            chart_type: ChartType::Scatter,
            data,
            width: HwpUnit::new(32250).unwrap(),
            height: HwpUnit::new(18750).unwrap(),
            title: None,
            legend: LegendPosition::Right,
            grouping: ChartGrouping::Clustered,
            bar_shape: None,
            explosion: None,
            of_pie_type: None,
            radar_style: None,
            wireframe: None,
            bubble_3d: None,
            scatter_style: Some(ScatterStyle::SmoothMarker),
            show_markers: None,
            stock_variant: None,
        };
        let xml = generate_chart_xml(&ctrl).unwrap();
        assert!(
            xml.contains(r#"<c:scatterStyle val="smoothMarker"/>"#),
            "smoothMarker style missing"
        );
    }

    #[test]
    fn line_markers_encode_in_line_chart() {
        let data = ChartData::category(&["A", "B"], &[("S", &[1.0, 2.0])]);
        let ctrl = Control::Chart {
            chart_type: ChartType::Line,
            data,
            width: HwpUnit::new(32250).unwrap(),
            height: HwpUnit::new(18750).unwrap(),
            title: None,
            legend: LegendPosition::Right,
            grouping: ChartGrouping::Clustered,
            bar_shape: None,
            explosion: None,
            of_pie_type: None,
            radar_style: None,
            wireframe: None,
            bubble_3d: None,
            scatter_style: None,
            show_markers: Some(true),
            stock_variant: None,
        };
        let xml = generate_chart_xml(&ctrl).unwrap();
        assert!(xml.contains("<c:marker>"), "marker block missing");
        assert!(xml.contains(r#"<c:symbol val="circle"/>"#), "circle symbol missing");
    }
}
