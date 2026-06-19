//! Run-level embedded chart (OLE-backed) builders — the
//! `<hp:switch>` block and BinData-referencing run XML (task #92
//! split from `encoder/section.rs`). The chart *document* XML
//! lives in `encoder/chart.rs`; this module only builds the
//! section-side run that references it.

use super::*;

/// Builds the full `<hp:run>` XML for a [`Control::EmbeddedChart`] (Wave 4c).
///
/// Emits the marker-replaced run that 한컴 expects for a HWP5-sourced
/// chart carry:
///
/// ```xml
/// <hp:run charPrIDRef="N">
///   <hp:switch>
///     <hp:case hp:required-namespace="...">
///       <hp:chart id="..." chartIDRef="Chart/chartN.xml">…</hp:chart>
///     </hp:case>
///     <hp:default>
///       <hp:ole id="..." binaryItemIDRef="oleN" …>…</hp:ole>
///     </hp:default>
///   </hp:switch>
///   <hp:t/>
/// </hp:run>
/// ```
///
/// `HxRunSwitch` only models the `<hp:case>` arm, so we cannot reuse the
/// serde path used by the structured [`Control::Chart`] arm. The OPF
/// manifest entry for `BinData/{ole_item_id}.ole` is registered by the
/// `PackageWriter` callsite via [`SectionEncodeResult::embedded_oles`].
#[allow(clippy::too_many_arguments)]
pub(super) fn build_embedded_chart_run_xml(
    char_pr_id_ref: u32,
    chart_ref: &str,
    ole_item_id: &str,
    width: i32,
    height: i32,
    horz_offset: i32,
    vert_offset: i32,
) -> String {
    // The `id` attributes are render-time instance identifiers; they only
    // need to be unique within the document and stay below i32::MAX (the
    // attribute is read as a signed 32-bit int by Hancom). `generate_instid`
    // already enforces both for us.
    let shared_id = generate_instid();
    format!(
        concat!(
            r#"<hp:run charPrIDRef="{cpr}">"#,
            r#"<hp:switch>"#,
            // ── <hp:case> with the OOXML chart reference ────────────────
            r#"<hp:case hp:required-namespace="http://www.hancom.co.kr/hwpml/2016/ooxmlchart">"#,
            r#"<hp:chart id="{id}" zOrder="0" numberingType="PICTURE" "#,
            r#"textWrap="SQUARE" textFlow="BOTH_SIDES" lock="0" "#,
            r#"dropcapstyle="None" chartIDRef="{chart_ref}">"#,
            r#"<hp:sz width="{w}" widthRelTo="ABSOLUTE" height="{h}" heightRelTo="ABSOLUTE" protect="0"/>"#,
            r#"<hp:pos treatAsChar="0" affectLSpacing="0" flowWithText="1" "#,
            r#"allowOverlap="0" holdAnchorAndSO="0" vertRelTo="PARA" horzRelTo="COLUMN" "#,
            r#"vertAlign="TOP" horzAlign="LEFT" vertOffset="{vo}" horzOffset="{ho}"/>"#,
            r#"<hp:outMargin left="0" right="0" top="0" bottom="0"/>"#,
            r#"</hp:chart>"#,
            r#"</hp:case>"#,
            // ── <hp:default> with the OLE fallback ──────────────────────
            r#"<hp:default>"#,
            r#"<hp:ole id="{id}" zOrder="0" numberingType="PICTURE" "#,
            r#"textWrap="SQUARE" textFlow="BOTH_SIDES" lock="0" "#,
            r#"dropcapstyle="None" href="" groupLevel="0" instid="0" "#,
            r#"objectType="UNKNOWN" binaryItemIDRef="{ole}" hasMoniker="0" "#,
            r#"drawAspect="CONTENT" eqBaseLine="0">"#,
            r#"<hp:offset x="0" y="0"/>"#,
            r#"<hp:orgSz width="7200" height="7200"/>"#,
            r#"<hp:curSz width="0" height="0"/>"#,
            r#"<hp:flip horizontal="0" vertical="0"/>"#,
            r#"<hp:rotationInfo angle="0" centerX="0" centerY="0" rotateimage="1"/>"#,
            r#"<hp:renderingInfo>"#,
            r#"<hc:transMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/>"#,
            r#"<hc:scaMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/>"#,
            r#"<hc:rotMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/>"#,
            r#"</hp:renderingInfo>"#,
            r#"<hc:extent x="7200" y="7200"/>"#,
            r##"<hp:lineShape color="#000000" width="0" style="NONE" endCap="ROUND" "##,
            r#"headStyle="NORMAL" tailStyle="NORMAL" headfill="0" tailfill="0" "#,
            r#"headSz="SMALL_SMALL" tailSz="SMALL_SMALL" outlineStyle="NORMAL" alpha="0"/>"#,
            r#"<hp:sz width="{w}" widthRelTo="ABSOLUTE" height="{h}" heightRelTo="ABSOLUTE" protect="0"/>"#,
            r#"<hp:pos treatAsChar="0" affectLSpacing="0" flowWithText="1" "#,
            r#"allowOverlap="0" holdAnchorAndSO="0" vertRelTo="PARA" horzRelTo="COLUMN" "#,
            r#"vertAlign="TOP" horzAlign="LEFT" vertOffset="{vo}" horzOffset="{ho}"/>"#,
            r#"<hp:outMargin left="0" right="0" top="0" bottom="0"/>"#,
            r#"</hp:ole>"#,
            r#"</hp:default>"#,
            r#"</hp:switch>"#,
            r#"<hp:t/>"#,
            r#"</hp:run>"#,
        ),
        cpr = char_pr_id_ref,
        id = shared_id,
        chart_ref = chart_ref,
        ole = ole_item_id,
        w = width,
        h = height,
        vo = vert_offset,
        ho = horz_offset,
    )
}

/// Encodes a Core `Control::Chart` into an `HxRunSwitch` wrapping `HxChart`.
///
/// Charts use `<hp:switch><hp:case><hp:chart>` structure in section XML,
/// referencing a separate OOXML chart XML file in the ZIP archive.
pub(super) fn encode_chart_switch(ctrl: &Control, chart_ref: &str) -> HxRunSwitch {
    let (width, height) = match ctrl {
        Control::Chart { width, height, .. } => (*width, *height),
        _ => unreachable!("encode_chart_switch called with non-Chart"),
    };

    HxRunSwitch {
        case: Some(HxRunCase {
            required_namespace: "http://www.hancom.co.kr/hwpml/2016/ooxmlchart".to_string(),
            chart: Some(HxChart {
                id: generate_instid(),
                z_order: 0,
                numbering_type: "PICTURE".to_string(),
                text_wrap: "TOP_AND_BOTTOM".to_string(),
                text_flow: "BOTH_SIDES".to_string(),
                lock: 0,
                dropcap_style: DropCapStyle::None.to_string(),
                chart_id_ref: chart_ref.to_string(),
                sz: Some(HxTableSz {
                    width: width.as_i32(),
                    width_rel_to: "ABSOLUTE".to_string(),
                    height: height.as_i32(),
                    height_rel_to: "ABSOLUTE".to_string(),
                    protect: 0,
                }),
                pos: Some(HxTablePos {
                    treat_as_char: 0,
                    affect_l_spacing: 0,
                    flow_with_text: 1,
                    allow_overlap: 0,
                    hold_anchor_and_so: 0,
                    vert_rel_to: "PARA".to_string(),
                    horz_rel_to: "COLUMN".to_string(),
                    vert_align: "TOP".to_string(),
                    horz_align: "LEFT".to_string(),
                    vert_offset: 0,
                    horz_offset: 0,
                }),
                out_margin: Some(HxTableMargin { left: 0, right: 0, top: 0, bottom: 0 }),
            }),
        }),
    }
}
