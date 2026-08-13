//! Header/footer, masterpage-reference, and page-number injection
//! builders (task #92 split from `encoder/section.rs`; Wave 5
//! per-ctrl applyPageType carry).

use super::*;

/// Builds `<hp:masterPage idRef="masterpageN"/>` references for secPr.
pub(super) fn build_masterpage_refs(section: &Section, masterpage_offset: usize) -> String {
    use std::fmt::Write as _;
    let Some(ref masters) = section.master_pages else {
        return String::new();
    };
    let mut xml = String::new();
    for (i, _mp) in masters.iter().enumerate() {
        let idx = masterpage_offset + i;
        let _ = write!(xml, r#"<hp:masterPage idRef="masterpage{idx}"/>"#);
    }
    xml
}

/// Generates masterpage XML files for a section's master pages.
///
/// Returns `(ZIP path, XML content)` pairs for each master page.
pub(super) fn build_masterpage_entries(
    section: &Section,
    masterpage_offset: usize,
) -> Vec<(String, String)> {
    use std::fmt::Write as _;
    let Some(ref masters) = section.master_pages else {
        return Vec::new();
    };
    masters
        .iter()
        .enumerate()
        .map(|(i, mp)| {
            let idx = masterpage_offset + i;
            let mp_id = format!("masterpage{idx}");
            let apply_type = match mp.apply_page_type {
                hwpforge_foundation::ApplyPageType::Both => "BOTH",
                hwpforge_foundation::ApplyPageType::Even => "EVEN",
                hwpforge_foundation::ApplyPageType::Odd => "ODD",
                _ => "BOTH",
            };

            let mut xml = String::with_capacity(1024);
            let _ = write!(xml, r#"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?>"#);
            // Root element uses NO namespace prefix (like real 한글 output).
            // All 15 xmlns declarations are required.
            let _ = write!(
                xml,
                r#"<masterPage{} id="{mp_id}" type="{apply_type}" pageNumber="0" pageDuplicate="0" pageFront="0">"#,
                crate::encoder::package::XMLNS_DECLS,
            );
            // subList uses hp: prefix (NOT hm:)
            let _ = write!(
                xml,
                r#"<hp:subList id="" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP" linkListIDRef="0" linkListNextIDRef="0" textWidth="42520" textHeight="65762" hasTextRef="0" hasNumRef="0">"#,
            );

            for (pidx, para) in mp.paragraphs.iter().enumerate() {
                let _ = write!(
                    xml,
                    r#"<hp:p id="{pidx}" paraPrIDRef="{}" styleIDRef="{}" pageBreak="0" columnBreak="0" merged="0">"#,
                    para.para_shape_id.get(),
                    para.style_id.map_or(0, |s| s.get()),
                );
                for run in &para.runs {
                    // Master page paragraphs emit a stripped-down
                    // run-with-text element. `InlineText` falls
                    // through to its tab-attribute-preserving builder
                    // so a master page that contains rich tabs (rare
                    // but possible after the HWPX decoder Phase 3
                    // carry) survives encode. See debug doc §3a-B11.
                    match &run.content {
                        RunContent::Text(text) => {
                            let _ = write!(
                                xml,
                                r#"<hp:run charPrIDRef="{}"><hp:t>{}</hp:t></hp:run>"#,
                                run.char_shape_id.get(),
                                super::escape_xml(text),
                            );
                        }
                        RunContent::InlineText(it) => {
                            let _ = write!(
                                xml,
                                r#"<hp:run charPrIDRef="{}">{}</hp:run>"#,
                                run.char_shape_id.get(),
                                build_inline_text_element_xml(it),
                            );
                        }
                        _ => {}
                    }
                }
                xml.push_str("</hp:p>");
            }

            xml.push_str("</hp:subList></masterPage>");
            (format!("Contents/masterpage{idx}.xml"), xml)
        })
        .collect()
}

/// Injects header, footer, and page number `<hp:ctrl>` blocks into
/// the section XML after the colPr ctrl (in the first run).
///
/// In real HWPX from 한글, these appear as:
/// - `<hp:ctrl><hp:header><hp:p>...</hp:p></hp:header></hp:ctrl>`
/// - `<hp:ctrl><hp:footer><hp:p>...</hp:p></hp:footer></hp:ctrl>`
/// - `<hp:ctrl><hp:autoNum numType="PAGE" ...></hp:ctrl>`
pub(super) fn inject_header_footer_pagenum(
    xml: &mut String,
    section: &Section,
    hyperlink_entries: &mut Vec<(String, String)>,
    options: EncodeOptions,
    sink: &mut EncodeSink,
) -> HwpxResult<()> {
    // Find insertion point: after the last </hp:ctrl> that contains colPr
    // (or after </hp:secPr> if no colPr).
    // We inject after the colPr ctrl block.
    let insert_pos = find_ctrl_injection_point(xml);
    if insert_pos == 0 {
        return Ok(()); // no suitable injection point found
    }

    let mut injection = String::new();

    // Header — emit each `<hp:header>` element preserving the HWPX
    // multi-cardinality wire shape (ADR-002).
    for (i, header) in section.headers.iter().enumerate() {
        sink.enter(crate::decoder::PathSeg::Header(i));
        let xml_result = build_header_xml(header, "header", hyperlink_entries, options, sink);
        sink.leave();
        injection.push_str(&xml_result?);
    }

    // Footer — same cardinality model.
    for (i, footer) in section.footers.iter().enumerate() {
        sink.enter(crate::decoder::PathSeg::Footer(i));
        let xml_result = build_header_xml(footer, "footer", hyperlink_entries, options, sink);
        sink.leave();
        injection.push_str(&xml_result?);
    }

    // Page number
    if let Some(ref page_number) = section.page_number {
        injection.push_str(&build_page_number_xml(page_number));
    }

    if !injection.is_empty() {
        xml.insert_str(insert_pos, &injection);
    }

    Ok(())
}

/// Finds the insertion point for header/footer/pagenum ctrl blocks.
///
/// Returns the byte offset after the colPr `</hp:ctrl>` block.
/// Falls back to after `</hp:secPr>` if no colPr is found.
///
/// NOTE: `<hp:colPr>` is typically emitted as a self-closing element
/// (`<hp:colPr .../>`). Looking for `</hp:colPr>` fails in that case and
/// causes controls (pageNum/header/footer) to be injected before colPr.
/// We anchor on the `<hp:colPr` start tag and then find the enclosing
/// `</hp:ctrl>`.
pub(super) fn find_ctrl_injection_point(xml: &str) -> usize {
    // Look for colPr ctrl: find "<hp:colPr" and then the next "</hp:ctrl>"
    // so both self-closing and expanded colPr forms are supported.
    if let Some(col_pr_pos) = xml.find("<hp:colPr") {
        if let Some(ctrl_close) = xml[col_pr_pos..].find("</hp:ctrl>") {
            return col_pr_pos + ctrl_close + "</hp:ctrl>".len();
        }
    }
    // Fallback: after </hp:secPr>
    if let Some(sec_pr_end) = xml.find("</hp:secPr>") {
        return sec_pr_end + "</hp:secPr>".len();
    }
    0
}

/// Builds `<hp:ctrl><hp:header>` or `<hp:ctrl><hp:footer>` XML.
///
/// `tag_name` should be `"header"` or `"footer"`.
pub(super) fn build_header_xml(
    hf: &hwpforge_core::section::HeaderFooter,
    tag_name: &str,
    hyperlink_entries: &mut Vec<(String, String)>,
    options: EncodeOptions,
    sink: &mut EncodeSink,
) -> HwpxResult<String> {
    use std::fmt::Write as _;

    let apply_page = match hf.apply_page_type {
        hwpforge_foundation::ApplyPageType::Both => "BOTH",
        hwpforge_foundation::ApplyPageType::Even => "EVEN",
        hwpforge_foundation::ApplyPageType::Odd => "ODD",
        _ => "BOTH",
    };

    let hf_id = generate_instid();
    let mut xml = String::new();
    write!(xml, r#"<hp:ctrl><hp:{tag_name} applyPageType="{apply_page}" id="{hf_id}">"#,)
        .expect("write to String is infallible");
    // W5-α H1: 컨테이너 기하(vertAlign/textWidth/textHeight) 실값 왕복 —
    // 일반 꼬리말은 vertAlign=BOTTOM 을 쓰므로 하드코딩하면 데이터 손실.
    let vert_align = match hf.vert_align {
        hwpforge_foundation::VerticalAlign::Center => "CENTER",
        hwpforge_foundation::VerticalAlign::Bottom => "BOTTOM",
        _ => "TOP",
    };
    let mut sub_list = crate::encoder::section::encode_paragraphs_to_sublist_with_align(
        &hf.paragraphs,
        0,
        vert_align,
        hyperlink_entries,
        options,
        sink,
    )?;
    sub_list.text_width = u32::try_from(hf.text_width.as_i32()).unwrap_or(0);
    sub_list.text_height = u32::try_from(hf.text_height.as_i32()).unwrap_or(0);
    let sub_xml = quick_xml::se::to_string(&sub_list)
        .map_err(|e| crate::error::HwpxError::InvalidStructure { detail: e.to_string() })?;
    let sub_xml = sub_xml.replacen("<HxSubList", "<hp:subList", 1);
    let sub_xml = sub_xml.replacen("</HxSubList>", "</hp:subList>", 1);
    xml.push_str(&sub_xml);
    write!(xml, "</hp:{tag_name}></hp:ctrl>").expect("write to String is infallible");
    Ok(xml)
}

/// Builds `<hp:ctrl><hp:pageNum>` XML for page numbers.
///
/// Uses the HWPX `<hp:pageNum>` element (not `<hp:autoNum>`) which is
/// the correct representation for page number controls. The `pos` attribute
/// specifies where the page number appears, `formatType` controls the
/// numbering style, and `sideChar` adds surrounding characters.
pub(super) fn build_page_number_xml(pn: &hwpforge_core::section::PageNumber) -> String {
    use std::fmt::Write as _;

    let pos = match pn.position {
        hwpforge_foundation::PageNumberPosition::None => "NONE",
        hwpforge_foundation::PageNumberPosition::TopLeft => "TOP_LEFT",
        hwpforge_foundation::PageNumberPosition::TopCenter => "TOP_CENTER",
        hwpforge_foundation::PageNumberPosition::TopRight => "TOP_RIGHT",
        hwpforge_foundation::PageNumberPosition::BottomLeft => "BOTTOM_LEFT",
        hwpforge_foundation::PageNumberPosition::BottomCenter => "BOTTOM_CENTER",
        hwpforge_foundation::PageNumberPosition::BottomRight => "BOTTOM_RIGHT",
        hwpforge_foundation::PageNumberPosition::OutsideTop => "OUTSIDE_TOP",
        hwpforge_foundation::PageNumberPosition::OutsideBottom => "OUTSIDE_BOTTOM",
        hwpforge_foundation::PageNumberPosition::InsideTop => "INSIDE_TOP",
        hwpforge_foundation::PageNumberPosition::InsideBottom => "INSIDE_BOTTOM",
        _ => "BOTTOM_CENTER",
    };

    let format_type = match pn.number_format {
        hwpforge_foundation::NumberFormatType::Digit => "DIGIT",
        hwpforge_foundation::NumberFormatType::CircledDigit => "CIRCLED_DIGIT",
        hwpforge_foundation::NumberFormatType::RomanCapital => "ROMAN_CAPITAL",
        hwpforge_foundation::NumberFormatType::RomanSmall => "ROMAN_SMALL",
        hwpforge_foundation::NumberFormatType::LatinCapital => "LATIN_CAPITAL",
        hwpforge_foundation::NumberFormatType::LatinSmall => "LATIN_SMALL",
        hwpforge_foundation::NumberFormatType::CircledLatinSmall => "CIRCLED_LATIN_SMALL",
        hwpforge_foundation::NumberFormatType::HangulSyllable => "HANGUL_SYLLABLE",
        hwpforge_foundation::NumberFormatType::HangulJamo => "HANGUL_JAMO",
        hwpforge_foundation::NumberFormatType::HanjaDigit => "HANJA_DIGIT",
        _ => "DIGIT",
    };

    let mut xml = String::new();
    write!(
        xml,
        r#"<hp:ctrl><hp:pageNum pos="{pos}" formatType="{format_type}" sideChar="{side_char}"/></hp:ctrl>"#,
        side_char = escape_xml(&pn.decoration),
    )
    .expect("write to String is infallible");
    xml
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_core::section::{HeaderFooter, PageNumber};
    use hwpforge_foundation::{ApplyPageType, NumberFormatType, PageNumberPosition};

    #[test]
    fn page_number_xml_maps_every_position() {
        let cases: &[(PageNumberPosition, &str)] = &[
            (PageNumberPosition::None, "NONE"),
            (PageNumberPosition::TopLeft, "TOP_LEFT"),
            (PageNumberPosition::TopCenter, "TOP_CENTER"),
            (PageNumberPosition::TopRight, "TOP_RIGHT"),
            (PageNumberPosition::BottomLeft, "BOTTOM_LEFT"),
            (PageNumberPosition::BottomCenter, "BOTTOM_CENTER"),
            (PageNumberPosition::BottomRight, "BOTTOM_RIGHT"),
            (PageNumberPosition::OutsideTop, "OUTSIDE_TOP"),
            (PageNumberPosition::OutsideBottom, "OUTSIDE_BOTTOM"),
            (PageNumberPosition::InsideTop, "INSIDE_TOP"),
            (PageNumberPosition::InsideBottom, "INSIDE_BOTTOM"),
        ];
        for (pos, want) in cases {
            let pn = PageNumber::new(*pos, NumberFormatType::Digit);
            let xml = build_page_number_xml(&pn);
            assert!(xml.contains(&format!(r#"pos="{want}""#)), "{want}: {xml}");
        }
    }

    #[test]
    fn page_number_xml_maps_every_format() {
        let cases: &[(NumberFormatType, &str)] = &[
            (NumberFormatType::Digit, "DIGIT"),
            (NumberFormatType::CircledDigit, "CIRCLED_DIGIT"),
            (NumberFormatType::RomanCapital, "ROMAN_CAPITAL"),
            (NumberFormatType::RomanSmall, "ROMAN_SMALL"),
            (NumberFormatType::LatinCapital, "LATIN_CAPITAL"),
            (NumberFormatType::LatinSmall, "LATIN_SMALL"),
            (NumberFormatType::CircledLatinSmall, "CIRCLED_LATIN_SMALL"),
            (NumberFormatType::HangulSyllable, "HANGUL_SYLLABLE"),
            (NumberFormatType::HangulJamo, "HANGUL_JAMO"),
            (NumberFormatType::HanjaDigit, "HANJA_DIGIT"),
        ];
        for (fmt, want) in cases {
            let pn = PageNumber::new(PageNumberPosition::BottomCenter, *fmt);
            let xml = build_page_number_xml(&pn);
            assert!(xml.contains(&format!(r#"formatType="{want}""#)), "{want}: {xml}");
        }
    }

    #[test]
    fn page_number_xml_escapes_decoration() {
        let mut pn = PageNumber::new(PageNumberPosition::BottomCenter, NumberFormatType::Digit);
        pn.decoration = "<&>".to_string();
        let xml = build_page_number_xml(&pn);
        assert!(xml.contains("sideChar=\"&lt;&amp;&gt;\""), "{xml}");
    }

    #[test]
    fn header_xml_maps_apply_page_type() {
        let mut entries = Vec::new();
        for (apply, want) in [
            (ApplyPageType::Both, "BOTH"),
            (ApplyPageType::Even, "EVEN"),
            (ApplyPageType::Odd, "ODD"),
        ] {
            let hf = HeaderFooter::new(Vec::new(), apply);
            let xml = build_header_xml(
                &hf,
                "header",
                &mut entries,
                EncodeOptions::default(),
                &mut EncodeSink::new(0),
            )
            .unwrap();
            assert!(xml.contains(&format!(r#"applyPageType="{want}""#)), "{want}: {xml}");
            assert!(xml.starts_with("<hp:ctrl><hp:header"));
        }
    }

    #[test]
    fn header_xml_carries_sublist_geometry() {
        // W5-α H1: 컨테이너 기하 실값 왕복 — 일반 꼬리말 = vertAlign BOTTOM.
        let mut entries = Vec::new();
        let mut hf = HeaderFooter::new(Vec::new(), ApplyPageType::Both);
        hf.vert_align = hwpforge_foundation::VerticalAlign::Bottom;
        hf.text_width = hwpforge_foundation::HwpUnit::new(42520).unwrap();
        hf.text_height = hwpforge_foundation::HwpUnit::new(4252).unwrap();
        let xml = build_header_xml(
            &hf,
            "footer",
            &mut entries,
            EncodeOptions::default(),
            &mut EncodeSink::new(0),
        )
        .unwrap();
        assert!(xml.contains(r#"vertAlign="BOTTOM""#), "{xml}");
        assert!(xml.contains(r#"textWidth="42520""#), "{xml}");
        assert!(xml.contains(r#"textHeight="4252""#), "{xml}");
    }

    #[test]
    fn ctrl_injection_point_anchors_on_colpr_then_falls_back() {
        // Self-closing colPr inside a ctrl: anchor after the enclosing ctrl close.
        let with_col = r#"<hp:secPr></hp:secPr><hp:ctrl><hp:colPr/></hp:ctrl>REST"#;
        let pos = find_ctrl_injection_point(with_col);
        assert_eq!(&with_col[pos..], "REST");

        // No colPr: fall back to just after </hp:secPr>.
        let no_col = r#"<hp:secPr></hp:secPr>BODY"#;
        let pos = find_ctrl_injection_point(no_col);
        assert_eq!(&no_col[pos..], "BODY");

        // Neither anchor present: returns 0 (no injection).
        assert_eq!(find_ctrl_injection_point("<hp:p></hp:p>"), 0);
    }

    #[test]
    fn masterpage_refs_empty_without_masters() {
        let section = Section::new(hwpforge_core::PageSettings::a4());
        assert!(build_masterpage_refs(&section, 0).is_empty());
        assert!(build_masterpage_entries(&section, 0).is_empty());
    }
}
