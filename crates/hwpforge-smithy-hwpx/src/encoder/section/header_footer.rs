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

/// `<hp:subList textWidth>` written for a master page whose Core
/// [`text_width`](hwpforge_core::section::MasterPage::text_width) is 0 (not
/// given) — the value every master page had before the size was carried.
const MASTER_PAGE_DEFAULT_TEXT_WIDTH: u32 = 42520;
/// `<hp:subList textHeight>` counterpart of [`MASTER_PAGE_DEFAULT_TEXT_WIDTH`].
const MASTER_PAGE_DEFAULT_TEXT_HEIGHT: u32 = 65762;

/// Generates the masterpage parts for a section's master pages.
///
/// Returns `(ZIP path, XML content)` pairs. Each master page is a part of its
/// own (`Contents/masterpage{N}.xml`, `N` = `masterpage_offset` + index), so
/// it is written the way a header's sublist is — through the common
/// paragraph encoder — and then gets the per-part steps the section part
/// gets: marker replacement and the XML-forbidden character scan. Its
/// warnings carry its own part name and a `section[s].masterpage[N]` path and
/// are appended to `section_sink`.
///
/// # Errors
///
/// Propagates paragraph encoding, serialization and marker replacement
/// failures.
pub(super) fn build_masterpage_entries(
    section: &Section,
    section_index: usize,
    masterpage_offset: usize,
    options: EncodeOptions,
    section_sink: &mut EncodeSink,
) -> HwpxResult<Vec<(String, String)>> {
    use hwpforge_core::section::MasterPageSpecial;
    use std::fmt::Write as _;
    let Some(ref masters) = section.master_pages else {
        return Ok(Vec::new());
    };
    let mut entries = Vec::with_capacity(masters.len());
    for (i, mp) in masters.iter().enumerate() {
        let idx = masterpage_offset + i;
        let mut sink = EncodeSink::for_master_page(section_index, idx);
        let (apply_type, page_number) = match mp.special {
            None => (
                match mp.apply_page_type {
                    hwpforge_foundation::ApplyPageType::Even => "EVEN",
                    hwpforge_foundation::ApplyPageType::Odd => "ODD",
                    _ => "BOTH",
                },
                0,
            ),
            Some(MasterPageSpecial::LastPage) => ("LAST_PAGE", 0),
            Some(MasterPageSpecial::OptionalPage { page_number }) => ("OPTIONAL_PAGE", page_number),
            Some(_) => {
                // 이 버전이 모르는 미래 범위 — 모든 쪽으로 번지지 않게 알린다.
                sink.content_lost(
                    "master_page_apply_type",
                    "master page scope unknown to this encoder; written as BOTH".to_string(),
                );
                ("BOTH", 0)
            }
        };
        let vert_align = match mp.vert_align {
            hwpforge_foundation::VerticalAlign::Center => "CENTER",
            hwpforge_foundation::VerticalAlign::Bottom => "BOTTOM",
            _ => "TOP",
        };

        let mut replacements: Vec<(String, String)> = Vec::new();
        let mut sub_list = crate::encoder::section::encode_paragraphs_to_sublist_with_align(
            &mp.paragraphs,
            0,
            vert_align,
            &mut replacements,
            options,
            &mut sink,
        )?;
        sub_list.text_width = match u32::try_from(mp.text_width.as_i32()) {
            Ok(0) | Err(_) => MASTER_PAGE_DEFAULT_TEXT_WIDTH,
            Ok(w) => w,
        };
        sub_list.text_height = match u32::try_from(mp.text_height.as_i32()) {
            Ok(0) | Err(_) => MASTER_PAGE_DEFAULT_TEXT_HEIGHT,
            Ok(h) => h,
        };
        #[allow(clippy::disallowed_methods)] // normalized on the next line
        let sub_xml = quick_xml::se::to_string(&sub_list)
            .map_err(|e| HwpxError::XmlSerialize { detail: e.to_string() })?;
        let sub_xml = crate::wire_xml::normalize_attr_control_whitespace(&sub_xml);
        let sub_xml = sub_xml.replacen("<HxSubList", "<hp:subList", 1);
        let sub_xml = sub_xml.replacen("</HxSubList>", "</hp:subList>", 1);

        let mut xml = String::with_capacity(sub_xml.len() + 1024);
        xml.push_str(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?>"#);
        // Root element uses NO namespace prefix (like real 한글 output).
        // All 15 xmlns declarations are required.
        let _ = write!(
            xml,
            r#"<masterPage{} id="masterpage{idx}" type="{apply_type}" pageNumber="{page_number}" pageDuplicate="{}" pageFront="{}">"#,
            crate::encoder::package::XMLNS_DECLS,
            u8::from(mp.page_duplicate),
            u8::from(mp.page_front),
        );
        xml.push_str(&sub_xml);
        xml.push_str("</masterPage>");

        let xml = apply_run_xml_replacements(xml, &replacements)?;
        let part = sink.part().to_string();
        let (clean, sites) = crate::wire_xml::strip_xml_forbidden_chars(&xml);
        let xml = match clean {
            std::borrow::Cow::Owned(clean) => clean,
            std::borrow::Cow::Borrowed(_) => xml,
        };
        sink.warnings.extend(crate::encoder::forbidden_char_warnings(&part, sites));
        section_sink.warnings.append(&mut sink.warnings);
        entries.push((part, xml));
    }
    Ok(entries)
}

/// Injects header, footer, and page number `<hp:ctrl>` blocks into
/// the section XML after the colPr ctrl (in the first run).
///
/// In real HWPX from 한글, these appear as:
/// - `<hp:ctrl><hp:header><hp:p>...</hp:p></hp:header></hp:ctrl>`
/// - `<hp:ctrl><hp:footer><hp:p>...</hp:p></hp:footer></hp:ctrl>`
/// - `<hp:ctrl><hp:autoNum numType="PAGE" ...></hp:ctrl>`
///
/// Headers/footers anchored to a later body paragraph
/// ([`HeaderFooter::anchor_paragraph`](hwpforge_core::section::HeaderFooter::anchor_paragraph))
/// go into that paragraph instead: `build_section` put a placeholder run in
/// front of it ([`super::AnchorRun`]) and this registers the run's
/// replacement — a run holding the `<hp:ctrl>` blocks, headers before
/// footers, each in list order.
pub(super) fn inject_header_footer_pagenum(
    xml: &mut String,
    section: &Section,
    anchor_runs: &[super::AnchorRun],
    hyperlink_entries: &mut Vec<(String, String)>,
    options: EncodeOptions,
    sink: &mut EncodeSink,
) -> HwpxResult<()> {
    // Header — emit each `<hp:header>` element preserving the HWPX
    // multi-cardinality wire shape (ADR-002) — into the paragraph it is
    // anchored to (§19.17 F2).
    let mut by_paragraph: std::collections::BTreeMap<usize, String> =
        std::collections::BTreeMap::new();
    for (i, header) in section.headers.iter().enumerate() {
        sink.enter(crate::decoder::PathSeg::Header(i));
        let xml_result = build_header_xml(header, "header", hyperlink_entries, options, sink);
        sink.leave();
        by_paragraph.entry(super::header_footer_anchor(header)).or_default().push_str(&xml_result?);
    }

    // Footer — same cardinality model.
    for (i, footer) in section.footers.iter().enumerate() {
        sink.enter(crate::decoder::PathSeg::Footer(i));
        let xml_result = build_header_xml(footer, "footer", hyperlink_entries, options, sink);
        sink.leave();
        by_paragraph.entry(super::header_footer_anchor(footer)).or_default().push_str(&xml_result?);
    }

    for anchor in anchor_runs {
        let ctrls = by_paragraph.remove(&anchor.paragraph).unwrap_or_default();
        hyperlink_entries.push((
            anchor.key.clone(),
            format!(r#"<hp:run charPrIDRef="{}">{ctrls}</hp:run>"#, anchor.char_pr_id_ref),
        ));
    }
    let mut injection = by_paragraph.remove(&0).unwrap_or_default();
    if let Some((&paragraph, _)) = by_paragraph.iter().next() {
        // build_section 이 같은 앵커 표로 자리표시 run 을 만들었으므로 도달 불가.
        return Err(HwpxError::InvalidStructure {
            detail: format!("encoder bug: no placeholder run for headers/footers anchored to paragraph {paragraph}"),
        });
    }

    // Find insertion point: after the last </hp:ctrl> that contains colPr
    // (or after </hp:secPr> if no colPr).
    // We inject after the colPr ctrl block.
    let insert_pos = find_ctrl_injection_point(xml);
    if insert_pos == 0 {
        return Ok(()); // no suitable injection point found
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
    #[allow(clippy::disallowed_methods)] // normalized on the next line
    let sub_xml = quick_xml::se::to_string(&sub_list)
        .map_err(|e| crate::error::HwpxError::InvalidStructure { detail: e.to_string() })?;
    let sub_xml = crate::wire_xml::normalize_attr_control_whitespace(&sub_xml);
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
        let mut sink = EncodeSink::new(0);
        let entries =
            build_masterpage_entries(&section, 0, 0, EncodeOptions::default(), &mut sink).unwrap();
        assert!(entries.is_empty());
    }
}
