//! OOXML chart XML parser.
//!
//! Parses `Chart/chartN.xml` files from the HWPX ZIP archive into
//! Core chart types. Uses quick-xml event-based parsing because the
//! OOXML chart namespace (`xmlns:c`) differs from HWPX's serde-based schema.

use hwpforge_core::chart::{
    ChartData, ChartGrouping, ChartSeries, ChartType, LegendPosition, XySeries,
};

use crate::error::{HwpxError, HwpxResult};

/// Parsed chart data extracted from an OOXML chart XML file.
pub(crate) struct ParsedChart {
    /// The chart type (bar, line, pie, etc.).
    pub chart_type: ChartType,
    /// Series data (category-based or XY-based).
    pub data: ChartData,
    /// Optional chart title.
    pub title: Option<String>,
    /// Legend position.
    pub legend: LegendPosition,
    /// Series grouping mode.
    pub grouping: ChartGrouping,
}

/// Parses an OOXML chart XML string into structured chart data.
///
/// Extracts chart type, series data, title, legend position, and grouping
/// from the `<c:chartSpace>` document.
pub(crate) fn parse_chart_xml(xml: &str) -> HwpxResult<ParsedChart> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut chart_type: Option<ChartType> = None;
    let mut bar_dir: Option<String> = None;
    let mut title: Option<String> = None;
    let mut legend = LegendPosition::Right;
    let mut grouping = ChartGrouping::Clustered;
    let mut is_xy = false;

    // Accumulated series data
    let mut all_categories: Vec<String> = Vec::new();
    let mut cat_series_list: Vec<ChartSeries> = Vec::new();
    let mut xy_series_list: Vec<XySeries> = Vec::new();

    // Per-series state
    let mut series_name = String::new();
    let mut cat_values: Vec<String> = Vec::new();
    let mut val_values: Vec<f64> = Vec::new();
    let mut x_values: Vec<f64> = Vec::new();
    let mut y_values: Vec<f64> = Vec::new();

    // Context flags
    let mut in_plot_area = false;
    let mut in_chart_elem = false;
    let mut in_series = false;
    let mut in_tx = false;
    let mut in_cat = false;
    let mut in_val = false;
    let mut in_xval = false;
    let mut in_yval = false;
    let mut in_title = false;
    let mut in_formula = false; // inside <c:f> — skip text

    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = e.name();
                let local = local_name(name.as_ref());
                match local {
                    b"plotArea" => in_plot_area = true,
                    b"title" if !in_series => in_title = true,
                    b"ser" => {
                        in_series = true;
                        series_name.clear();
                        cat_values.clear();
                        val_values.clear();
                        x_values.clear();
                        y_values.clear();
                    }
                    b"f" if in_series => in_formula = true,
                    b"tx" if in_series => in_tx = true,
                    b"cat" if in_series => in_cat = true,
                    b"val" if in_series && !in_xval && !in_yval => in_val = true,
                    b"xVal" if in_series => in_xval = true,
                    b"yVal" if in_series => in_yval = true,
                    _ => {
                        if in_plot_area && !in_chart_elem {
                            if let Some(ct) = detect_chart_type(local) {
                                chart_type = Some(ct);
                                is_xy = matches!(ct, ChartType::Scatter | ChartType::Bubble);
                                in_chart_elem = true;
                            }
                        }
                    }
                }
                process_start_attrs(e, &mut bar_dir, &mut grouping, &mut legend, local);
            }
            Ok(Event::Empty(ref e)) => {
                let name = e.name();
                let local = local_name(name.as_ref());
                process_start_attrs(e, &mut bar_dir, &mut grouping, &mut legend, local);
            }
            Ok(Event::Text(ref e)) => {
                let text = e.unescape().unwrap_or_default().to_string();
                if text.is_empty() || in_formula {
                    // skip formula text (<c:f>Sheet1!...</c:f>)
                } else if in_title && !in_series {
                    title = Some(text);
                } else if in_series && in_tx {
                    series_name = text;
                } else if in_series && in_cat {
                    cat_values.push(text);
                } else if in_series && in_xval {
                    if let Ok(f) = text.parse::<f64>() {
                        x_values.push(f);
                    }
                } else if in_series && in_yval {
                    if let Ok(f) = text.parse::<f64>() {
                        y_values.push(f);
                    }
                } else if in_series && in_val {
                    if let Ok(f) = text.parse::<f64>() {
                        val_values.push(f);
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.name();
                let local = local_name(name.as_ref());
                match local {
                    b"plotArea" => in_plot_area = false,
                    b"title" if in_title => in_title = false,
                    b"ser" => {
                        // Finalize current series
                        if is_xy {
                            xy_series_list.push(XySeries {
                                name: std::mem::take(&mut series_name),
                                x_values: std::mem::take(&mut x_values),
                                y_values: std::mem::take(&mut y_values),
                            });
                        } else {
                            // First series captures the shared categories
                            if all_categories.is_empty() && !cat_values.is_empty() {
                                all_categories = std::mem::take(&mut cat_values);
                            } else {
                                cat_values.clear();
                            }
                            cat_series_list.push(ChartSeries {
                                name: std::mem::take(&mut series_name),
                                values: std::mem::take(&mut val_values),
                            });
                        }
                        in_series = false;
                        in_tx = false;
                        in_cat = false;
                        in_val = false;
                        in_xval = false;
                        in_yval = false;
                    }
                    b"f" => in_formula = false,
                    b"tx" => in_tx = false,
                    b"cat" => in_cat = false,
                    b"val" if in_val => in_val = false,
                    b"xVal" => in_xval = false,
                    b"yVal" => in_yval = false,
                    _ => {
                        if in_chart_elem && detect_chart_type(local).is_some() {
                            in_chart_elem = false;
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(HwpxError::XmlParse {
                    file: "Chart/*.xml".to_string(),
                    detail: e.to_string(),
                })
            }
            _ => {}
        }
        buf.clear();
    }

    // Adjust chart type based on barDir attribute
    let mut ct = chart_type.unwrap_or(ChartType::Bar);
    if let Some(ref dir) = bar_dir {
        ct = adjust_bar_dir(ct, dir);
    }

    let data = if is_xy {
        ChartData::Xy { series: xy_series_list }
    } else {
        ChartData::Category { categories: all_categories, series: cat_series_list }
    };

    Ok(ParsedChart { chart_type: ct, data, title, legend, grouping })
}

/// Strips the namespace prefix from an XML tag name (`c:barChart` → `barChart`).
fn local_name(name: &[u8]) -> &[u8] {
    match name.iter().position(|&b| b == b':') {
        Some(pos) => &name[pos + 1..],
        None => name,
    }
}

/// Extracts the `val` attribute from an XML element.
fn get_val_attr(e: &quick_xml::events::BytesStart) -> Option<String> {
    e.attributes()
        .filter_map(|a| a.ok())
        .find(|a| a.key.as_ref() == b"val")
        .and_then(|a| String::from_utf8(a.value.to_vec()).ok())
}

/// Processes attributes on Start/Empty elements for barDir, grouping, legendPos.
fn process_start_attrs(
    e: &quick_xml::events::BytesStart,
    bar_dir: &mut Option<String>,
    grouping: &mut ChartGrouping,
    legend: &mut LegendPosition,
    local: &[u8],
) {
    match local {
        b"barDir" => {
            if let Some(val) = get_val_attr(e) {
                *bar_dir = Some(val);
            }
        }
        b"grouping" => {
            if let Some(val) = get_val_attr(e) {
                *grouping = parse_grouping(&val);
            }
        }
        b"legendPos" => {
            if let Some(val) = get_val_attr(e) {
                *legend = parse_legend_pos(&val);
            }
        }
        _ => {}
    }
}

/// Maps an OOXML element tag name to a `ChartType`.
fn detect_chart_type(local: &[u8]) -> Option<ChartType> {
    match local {
        b"barChart" => Some(ChartType::Bar),
        b"bar3DChart" => Some(ChartType::Bar3D),
        b"lineChart" => Some(ChartType::Line),
        b"line3DChart" => Some(ChartType::Line3D),
        b"pieChart" => Some(ChartType::Pie),
        b"pie3DChart" => Some(ChartType::Pie3D),
        b"doughnutChart" => Some(ChartType::Doughnut),
        b"ofPieChart" => Some(ChartType::OfPie),
        b"areaChart" => Some(ChartType::Area),
        b"area3DChart" => Some(ChartType::Area3D),
        b"scatterChart" => Some(ChartType::Scatter),
        b"bubbleChart" => Some(ChartType::Bubble),
        b"radarChart" => Some(ChartType::Radar),
        b"surfaceChart" => Some(ChartType::Surface),
        b"surface3DChart" => Some(ChartType::Surface3D),
        b"stockChart" => Some(ChartType::Stock),
        _ => None,
    }
}

/// Adjusts `ChartType::Bar`/`Bar3D` to `Column`/`Column3D` based on barDir attribute.
fn adjust_bar_dir(ct: ChartType, dir: &str) -> ChartType {
    match (ct, dir) {
        (ChartType::Bar, "col") => ChartType::Column,
        (ChartType::Bar3D, "col") => ChartType::Column3D,
        _ => ct,
    }
}

/// Parses a grouping value string to the enum variant.
fn parse_grouping(val: &str) -> ChartGrouping {
    match val {
        "clustered" => ChartGrouping::Clustered,
        "stacked" => ChartGrouping::Stacked,
        "percentStacked" => ChartGrouping::PercentStacked,
        "standard" => ChartGrouping::Standard,
        _ => ChartGrouping::Clustered,
    }
}

/// Parses a legend position value string to the enum variant.
fn parse_legend_pos(val: &str) -> LegendPosition {
    match val {
        "r" => LegendPosition::Right,
        "b" => LegendPosition::Bottom,
        "t" => LegendPosition::Top,
        "l" => LegendPosition::Left,
        _ => LegendPosition::Right,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bar_chart() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart><c:title><c:tx><c:rich><c:p><c:r><c:t>Sales</c:t></c:r></c:p></c:rich></c:tx></c:title><c:plotArea><c:layout/><c:barChart><c:barDir val="bar"/><c:grouping val="clustered"/><c:ser><c:idx val="0"/><c:order val="0"/><c:tx><c:strRef><c:strCache><c:ptCount val="1"/><c:pt idx="0"><c:v>Revenue</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:ptCount val="2"/><c:pt idx="0"><c:v>Q1</c:v></c:pt><c:pt idx="1"><c:v>Q2</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:ptCount val="2"/><c:pt idx="0"><c:v>100</c:v></c:pt><c:pt idx="1"><c:v>200</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="1"/><c:axId val="2"/></c:barChart></c:plotArea><c:legend><c:legendPos val="b"/></c:legend></c:chart></c:chartSpace>"#;

        let parsed = parse_chart_xml(xml).unwrap();
        assert_eq!(parsed.chart_type, ChartType::Bar);
        assert_eq!(parsed.title, Some("Sales".to_string()));
        assert_eq!(parsed.legend, LegendPosition::Bottom);
        assert_eq!(parsed.grouping, ChartGrouping::Clustered);
        match &parsed.data {
            ChartData::Category { categories, series } => {
                assert_eq!(categories, &["Q1", "Q2"]);
                assert_eq!(series.len(), 1);
                assert_eq!(series[0].name, "Revenue");
                assert_eq!(series[0].values, vec![100.0, 200.0]);
            }
            _ => panic!("expected Category data"),
        }
    }

    #[test]
    fn parse_column_via_bar_dir() {
        let xml = r#"<?xml version="1.0"?><c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart><c:plotArea><c:layout/><c:barChart><c:barDir val="col"/><c:grouping val="stacked"/><c:ser><c:idx val="0"/><c:order val="0"/><c:tx><c:strRef><c:strCache><c:ptCount val="1"/><c:pt idx="0"><c:v>S</c:v></c:pt></c:strCache></c:strRef></c:tx><c:val><c:numRef><c:numCache><c:ptCount val="1"/><c:pt idx="0"><c:v>5</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        let parsed = parse_chart_xml(xml).unwrap();
        assert_eq!(parsed.chart_type, ChartType::Column);
        assert_eq!(parsed.grouping, ChartGrouping::Stacked);
    }

    #[test]
    fn parse_scatter_xy_data() {
        let xml = r#"<?xml version="1.0"?><c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart><c:plotArea><c:layout/><c:scatterChart><c:ser><c:idx val="0"/><c:order val="0"/><c:tx><c:strRef><c:strCache><c:ptCount val="1"/><c:pt idx="0"><c:v>Points</c:v></c:pt></c:strCache></c:strRef></c:tx><c:xVal><c:numRef><c:numCache><c:ptCount val="2"/><c:pt idx="0"><c:v>1</c:v></c:pt><c:pt idx="1"><c:v>2</c:v></c:pt></c:numCache></c:numRef></c:xVal><c:yVal><c:numRef><c:numCache><c:ptCount val="2"/><c:pt idx="0"><c:v>3</c:v></c:pt><c:pt idx="1"><c:v>4</c:v></c:pt></c:numCache></c:numRef></c:yVal></c:ser></c:scatterChart></c:plotArea></c:chart></c:chartSpace>"#;
        let parsed = parse_chart_xml(xml).unwrap();
        assert_eq!(parsed.chart_type, ChartType::Scatter);
        match &parsed.data {
            ChartData::Xy { series } => {
                assert_eq!(series.len(), 1);
                assert_eq!(series[0].name, "Points");
                assert_eq!(series[0].x_values, vec![1.0, 2.0]);
                assert_eq!(series[0].y_values, vec![3.0, 4.0]);
            }
            _ => panic!("expected XY data"),
        }
    }

    #[test]
    fn parse_pie_chart() {
        let xml = r#"<?xml version="1.0"?><c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart><c:plotArea><c:layout/><c:pieChart><c:ser><c:idx val="0"/><c:order val="0"/><c:tx><c:strRef><c:strCache><c:ptCount val="1"/><c:pt idx="0"><c:v>Slice</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:ptCount val="2"/><c:pt idx="0"><c:v>A</c:v></c:pt><c:pt idx="1"><c:v>B</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:ptCount val="2"/><c:pt idx="0"><c:v>60</c:v></c:pt><c:pt idx="1"><c:v>40</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:pieChart></c:plotArea></c:chart></c:chartSpace>"#;
        let parsed = parse_chart_xml(xml).unwrap();
        assert_eq!(parsed.chart_type, ChartType::Pie);
        assert_eq!(parsed.legend, LegendPosition::Right); // default (no legend element)
    }

    #[test]
    fn roundtrip_encoder_decoder() {
        // Generate chart XML with the encoder, then parse it with the decoder
        use crate::encoder::chart::generate_chart_xml;
        use hwpforge_core::control::Control;

        let ctrl = Control::chart(
            ChartType::Line,
            ChartData::category(&["A", "B", "C"], &[("Series1", &[10.0, 20.0, 30.0])]),
        );
        let xml = generate_chart_xml(&ctrl).unwrap();
        let parsed = parse_chart_xml(&xml).unwrap();

        assert_eq!(parsed.chart_type, ChartType::Line);
        assert_eq!(parsed.grouping, ChartGrouping::Clustered);
        match &parsed.data {
            ChartData::Category { categories, series } => {
                assert_eq!(categories, &["A", "B", "C"]);
                assert_eq!(series.len(), 1);
                assert_eq!(series[0].name, "Series1");
                assert_eq!(series[0].values, vec![10.0, 20.0, 30.0]);
            }
            _ => panic!("expected Category data"),
        }
    }
}
