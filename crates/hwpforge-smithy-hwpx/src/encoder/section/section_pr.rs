//! `<hp:secPr>` / `<hp:colPr>` builders and enrichment (task #92
//! split from `encoder/section.rs`). Covers page settings, section
//! visibility (hideFirst* bits, Wave 5 gap B), and column layout.

use super::*;

/// Builds `HxSecPr` from Core `PageSettings` and text direction.
pub(super) fn build_sec_pr(ps: &PageSettings, text_direction: TextDirection) -> HxSecPr {
    let gutter_type_str = match ps.gutter_type {
        hwpforge_foundation::GutterType::LeftOnly => "LEFT_ONLY",
        hwpforge_foundation::GutterType::LeftRight => "LEFT_RIGHT",
        hwpforge_foundation::GutterType::TopOnly => "TOP_ONLY",
        hwpforge_foundation::GutterType::TopBottom => "TOP_BOTTOM",
        _ => "LEFT_ONLY",
    };
    HxSecPr {
        text_direction: text_direction.to_string(),
        master_page_cnt: 0,
        visibility: None,
        line_number_shape: None,
        page_pr: Some(HxPagePr {
            // 한글 실제 동작: WIDELY=portrait (세로), NARROWLY=landscape (가로)
            // KS X 6101 스펙과 반대! (gotcha #3: landscape 값 반전)
            landscape: if ps.landscape { "NARROWLY".to_string() } else { "WIDELY".to_string() },
            width: ps.width.as_i32(),
            height: ps.height.as_i32(),
            gutter_type: gutter_type_str.to_string(),
            margin: Some(HxPageMargin {
                header: ps.header_margin.as_i32(),
                footer: ps.footer_margin.as_i32(),
                gutter: ps.gutter.as_i32(),
                left: ps.margin_left.as_i32(),
                right: ps.margin_right.as_i32(),
                top: ps.margin_top.as_i32(),
                bottom: ps.margin_bottom.as_i32(),
            }),
        }),
        page_border_fills: Vec::new(),
        start_num: None,
    }
}

/// Builds the enriched `<hp:secPr>` opening tag with all attributes 한글 expects.
///
/// `master_page_cnt` is set dynamically from the section's master pages.
/// `textVerticalWidthHead` is `"1"` when text direction is not horizontal, `"0"` otherwise.
pub(super) fn build_sec_pr_open_enriched(section: &Section) -> String {
    let master_page_cnt = section.master_pages.as_ref().map_or(0, |v| v.len());
    let text_direction = section.text_direction.to_string();
    let vert_width_head =
        if section.text_direction == TextDirection::Horizontal { "0" } else { "1" };
    format!(
        r#"<hp:secPr id="" textDirection="{text_direction}" spaceColumns="1134" tabStop="8000" tabStopVal="4000" tabStopUnit="HWPUNIT" outlineShapeIDRef="1" memoShapeIDRef="0" textVerticalWidthHead="{vert_width_head}" masterPageCnt="{master_page_cnt}">"#,
    )
}

/// Builds sub-elements inserted before `<hp:pagePr>` inside secPr.
///
/// Reads visibility and line number settings from the Section, falling back
/// to 한글 defaults when not specified.
pub(super) fn build_sec_pr_pre_elements(section: &Section) -> String {
    use std::fmt::Write as _;

    let vis = section.visibility.as_ref().cloned().unwrap_or_default();
    let lns = section.line_number_shape.as_ref().copied().unwrap_or_default();

    let border_str = show_mode_to_hwpx(vis.border);
    let fill_str = show_mode_to_hwpx(vis.fill);

    let mut xml = String::with_capacity(512);
    let _ = write!(xml, r#"<hp:grid lineGrid="0" charGrid="0" wonggojiFormat="0"/>"#);

    // Use section's begin_num if set, otherwise default to all zeros
    let bn = section.begin_num.as_ref();
    let page = bn.map_or(0, |b| b.page);
    let pic = bn.map_or(0, |b| b.pic);
    let tbl = bn.map_or(0, |b| b.tbl);
    let equation = bn.map_or(0, |b| b.equation);
    let _ = write!(
        xml,
        r#"<hp:startNum pageStartsOn="BOTH" page="{page}" pic="{pic}" tbl="{tbl}" equation="{equation}"/>"#,
    );
    let _ = write!(
        xml,
        r#"<hp:visibility hideFirstHeader="{}" hideFirstFooter="{}" hideFirstMasterPage="{}" border="{border_str}" fill="{fill_str}" hideFirstPageNum="{}" hideFirstEmptyLine="{}" showLineNumber="{}"/>"#,
        u8::from(vis.hide_first_header),
        u8::from(vis.hide_first_footer),
        u8::from(vis.hide_first_master_page),
        u8::from(vis.hide_first_page_num),
        u8::from(vis.hide_first_empty_line),
        u8::from(vis.show_line_number),
    );
    let _ = write!(
        xml,
        r#"<hp:lineNumberShape restartType="{}" countBy="{}" distance="{}" startNumber="{}"/>"#,
        lns.restart_type,
        lns.count_by,
        lns.distance.as_i32(),
        lns.start_number,
    );
    xml
}

/// Converts a [`ShowMode`] enum to the HWPX SCREAMING_SNAKE string.
pub(super) fn show_mode_to_hwpx(mode: hwpforge_foundation::ShowMode) -> &'static str {
    use hwpforge_foundation::ShowMode;
    match mode {
        ShowMode::ShowAll => "SHOW_ALL",
        ShowMode::HideAll => "HIDE_ALL",
        ShowMode::ShowOdd => "SHOW_ODD",
        ShowMode::ShowEven => "SHOW_EVEN",
        _ => "SHOW_ALL",
    }
}

/// Builds sub-elements inserted after `</hp:pagePr>` and before `</hp:secPr>`.
///
/// Reads page border fill entries from the Section, falling back to 한글
/// defaults (3 entries: BOTH/EVEN/ODD with borderFillIDRef=1).
pub(super) fn build_sec_pr_post_elements(section: &Section) -> String {
    use hwpforge_core::section::PageBorderFillEntry;
    use std::fmt::Write as _;

    let mut xml = String::with_capacity(1024);

    // Footnote/endnote properties — newNum uses begin_num if set
    let footnote_new_num = section.begin_num.as_ref().map_or(1, |b| b.footnote);
    let endnote_new_num = section.begin_num.as_ref().map_or(1, |b| b.endnote);
    let _ = write!(
        xml,
        r#"<hp:footNotePr><hp:autoNumFormat type="DIGIT" userChar="" prefixChar="" suffixChar=")" supscript="0"/>"#,
    );
    let _ = write!(
        xml,
        r##"<hp:noteLine length="-1" type="SOLID" width="0.12 mm" color="#000000"/>"##,
    );
    let _ = write!(xml, r#"<hp:noteSpacing betweenNotes="283" belowLine="567" aboveLine="850"/>"#,);
    let _ = write!(xml, r#"<hp:numbering type="CONTINUOUS" newNum="{footnote_new_num}"/>"#,);
    let _ = write!(xml, r#"<hp:placement place="EACH_COLUMN" beneathText="0"/></hp:footNotePr>"#,);
    let _ = write!(
        xml,
        r#"<hp:endNotePr><hp:autoNumFormat type="DIGIT" userChar="" prefixChar="" suffixChar=")" supscript="0"/>"#,
    );
    let _ = write!(
        xml,
        r##"<hp:noteLine length="14692344" type="SOLID" width="0.12 mm" color="#000000"/>"##,
    );
    let _ = write!(xml, r#"<hp:noteSpacing betweenNotes="0" belowLine="567" aboveLine="850"/>"#,);
    let _ = write!(xml, r#"<hp:numbering type="CONTINUOUS" newNum="{endnote_new_num}"/>"#,);
    let _ =
        write!(xml, r#"<hp:placement place="END_OF_DOCUMENT" beneathText="0"/></hp:endNotePr>"#,);

    // Page border fills
    let default_entries = vec![
        PageBorderFillEntry { apply_type: "BOTH".to_string(), ..Default::default() },
        PageBorderFillEntry { apply_type: "EVEN".to_string(), ..Default::default() },
        PageBorderFillEntry { apply_type: "ODD".to_string(), ..Default::default() },
    ];
    let entries = section.page_border_fills.as_deref().unwrap_or(&default_entries);
    for entry in entries {
        let hi = u8::from(entry.header_inside);
        let fi = u8::from(entry.footer_inside);
        let [l, r, t, b] = entry.offset;
        let _ = write!(
            xml,
            r#"<hp:pageBorderFill type="{}" borderFillIDRef="{}" textBorder="{}" headerInside="{hi}" footerInside="{fi}" fillArea="{}">"#,
            entry.apply_type, entry.border_fill_id, entry.text_border, entry.fill_area,
        );
        let _ = write!(
            xml,
            r#"<hp:offset left="{}" right="{}" top="{}" bottom="{}"/>"#,
            l.as_i32(),
            r.as_i32(),
            t.as_i32(),
            b.as_i32(),
        );
        let _ = write!(xml, "</hp:pageBorderFill>");
    }

    xml
}

/// Builds `<hp:ctrl><hp:colPr>...</hp:colPr></hp:ctrl>` XML string.
///
/// When `column_settings` is `None`, produces the single-column default
/// matching 한글's standard output. Otherwise generates multi-column
/// XML with the appropriate attributes and optional `<hp:col>` children.
pub(super) fn build_col_pr_xml(column_settings: Option<&ColumnSettings>) -> String {
    match column_settings {
        None => {
            // Single-column default
            concat!(
                r#"<hp:ctrl>"#,
                r#"<hp:colPr id="" type="NEWSPAPER" layout="LEFT" colCount="1" sameSz="1" sameGap="0"/>"#,
                r#"</hp:ctrl>"#,
            )
            .to_string()
        }
        Some(cs) => {
            let col_type = match cs.column_type {
                ColumnType::Newspaper => "NEWSPAPER",
                ColumnType::Parallel => "PARALLEL",
                _ => "NEWSPAPER",
            };
            let layout = match cs.layout_mode {
                ColumnLayoutMode::Left => "LEFT",
                ColumnLayoutMode::Right => "RIGHT",
                ColumnLayoutMode::Mirror => "MIRROR",
                _ => "LEFT",
            };
            let col_count = cs.columns.len();
            let all_same = cs.is_equal_width();
            let (same_sz, same_gap) = if all_same {
                // sameSz=1: 한글 calculates equal widths, we just specify gap
                (1, if col_count >= 2 { cs.columns[0].gap.as_i32() } else { 0 })
            } else {
                (0, 0)
            };

            // OWPML orders children as colLine (separator) then colSz (<hp:col>).
            // `<hp:col>` children only exist when sameSz=0 (variable widths).
            let mut children = cs.col_line.as_ref().map(build_col_line_xml).unwrap_or_default();
            if same_sz == 0 {
                for col in &cs.columns {
                    children.push_str(&format!(
                        r#"<hp:col width="{}" gap="{}"/>"#,
                        col.width.as_i32(),
                        col.gap.as_i32()
                    ));
                }
            }

            // No children → self-closing colPr (byte-identical to the prior
            // no-separator path). Children present → container colPr.
            if children.is_empty() {
                format!(
                    r#"<hp:ctrl><hp:colPr id="" type="{col_type}" layout="{layout}" colCount="{col_count}" sameSz="{same_sz}" sameGap="{same_gap}"/></hp:ctrl>"#
                )
            } else {
                format!(
                    r#"<hp:ctrl><hp:colPr id="" type="{col_type}" layout="{layout}" colCount="{col_count}" sameSz="{same_sz}" sameGap="{same_gap}">{children}</hp:colPr></hp:ctrl>"#
                )
            }
        }
    }
}

/// Maps a Core [`BorderLineType`](hwpforge_foundation::BorderLineType) to the
/// HWPX `colLine`/border wire string (`LineType2`: UPPER_SNAKE). Types with no
/// `LineType2` equivalent fall back to `SOLID`.
fn col_line_type_to_hwpx(t: hwpforge_foundation::BorderLineType) -> &'static str {
    use hwpforge_foundation::BorderLineType as B;
    match t {
        B::None => "NONE",
        B::Solid => "SOLID",
        B::Dash => "DASH",
        B::Dot => "DOT",
        B::DashDot => "DASH_DOT",
        B::DashDotDot => "DASH_DOT_DOT",
        B::LongDash => "LONG_DASH",
        B::DoubleSlim => "DOUBLE_SLIM",
        _ => "SOLID",
    }
}

/// Formats a width in millimetres for HWPX (e.g. `0.7` → `"0.7"`), rounding to
/// two decimals and stripping trailing zeros to match OWPML samples
/// (`"0.7 mm"`, `"0.12 mm"`).
///
/// NOTE: exact 한컴 precision/format is confirmed against a native fixture in
/// the colLine round-trip gate.
fn format_mm(mm: f64) -> String {
    let rounded = (mm * 100.0).round() / 100.0;
    format!("{rounded}")
}

/// Builds the `<hp:colLine .../>` separator element for a column layout.
fn build_col_line_xml(line: &hwpforge_core::column::ColumnLine) -> String {
    format!(
        r#"<hp:colLine type="{}" width="{} mm" color="{}"/>"#,
        col_line_type_to_hwpx(line.line_type),
        format_mm(line.width.to_mm()),
        line.color.to_hex_rgb(),
    )
}

/// Enriches the minimal `<hp:secPr>` output with sub-elements required
/// by 한글 for proper rendering.
///
/// Replaces the opening tag with an enriched version carrying all expected
/// attributes, inserts grid/visibility elements before `<hp:pagePr>`,
/// appends footnote/endnote/pageBorderFill after `</hp:pagePr>`,
/// and injects `<hp:ctrl><hp:colPr>` after the closing `</hp:secPr>`.
pub(super) fn enrich_sec_pr(xml: &str, section: &Section, masterpage_offset: usize) -> String {
    let sec_pr_prefix = r#"<hp:secPr "#;

    // If no secPr to enrich, return as-is
    let Some(start) = xml.find(sec_pr_prefix) else {
        return xml.to_string();
    };

    // Find the closing `>` of the opening tag to replace the entire opening element
    let Some(end) = xml[start..].find('>') else {
        return xml.to_string();
    };
    let open_end = start + end + 1; // byte index just past the opening tag's `>`

    let open_enriched = build_sec_pr_open_enriched(section);
    let pre_elements = build_sec_pr_pre_elements(section);
    let post_elements = build_sec_pr_post_elements(section);
    let masterpage_refs = build_masterpage_refs(section, masterpage_offset);
    let col_pr = build_col_pr_xml(section.column_settings.as_ref());

    const CLOSE: &str = "</hp:secPr>";

    // Single forward-scan build (was: replacen + 2× find + 2× insert_str over the
    // whole serialized XML). Splices, in order:
    //   [prefix] open_enriched pre_elements [secPr inner] post_elements
    //   masterpage_refs </hp:secPr> col_pr [tail]
    // — byte-identical to the prior sequential edits.
    match xml[open_end..].find(CLOSE) {
        Some(rel_close) => {
            let close_at = open_end + rel_close;
            let mut result = String::with_capacity(
                xml.len()
                    + open_enriched.len()
                    + pre_elements.len()
                    + post_elements.len()
                    + masterpage_refs.len()
                    + col_pr.len(),
            );
            result.push_str(&xml[..start]);
            result.push_str(&open_enriched);
            result.push_str(&pre_elements);
            result.push_str(&xml[open_end..close_at]);
            result.push_str(&post_elements);
            result.push_str(&masterpage_refs);
            result.push_str(CLOSE);
            result.push_str(&col_pr);
            result.push_str(&xml[close_at + CLOSE.len()..]);
            result
        }
        // No closing tag: mirror the old behavior (replacen ran, both inserts
        // were skipped) — only the opening tag is enriched.
        None => {
            let mut result =
                String::with_capacity(xml.len() + open_enriched.len() + pre_elements.len());
            result.push_str(&xml[..start]);
            result.push_str(&open_enriched);
            result.push_str(&pre_elements);
            result.push_str(&xml[open_end..]);
            result
        }
    }
}
