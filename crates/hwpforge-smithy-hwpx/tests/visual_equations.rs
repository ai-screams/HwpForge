use std::io::Write as _;

use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxVisualEquationDomain, HwpxVisualEquationParentKind};
use zip::write::SimpleFileOptions;

const HEADER_XML: &str = r##"<head version="1.4" secCnt="1">
  <refList>
    <fontfaces itemCnt="1"><fontface lang="HANGUL" fontCnt="1"><font id="0" face="함초롬돋움" type="TTF" isEmbedded="0"/></fontface></fontfaces>
    <charProperties itemCnt="1"><charPr id="0" height="1000" textColor="#000000" shadeColor="none" useFontSpace="0" useKerning="0" symMark="NONE" borderFillIDRef="0"><fontRef hangul="0" latin="0" hanja="0" japanese="0" other="0" symbol="0" user="0"/></charPr></charProperties>
    <paraProperties itemCnt="1"><paraPr id="0"><align horizontal="LEFT" vertical="BASELINE"/><switch><default><lineSpacing type="PERCENT" value="160"/></default></switch></paraPr></paraProperties>
  </refList>
</head>"##;

const SECTION_XML: &str = r#"<sec>
  <p id="1" paraPrIDRef="0" styleIDRef="0">
    <run charPrIDRef="0">
      <equation id="ordinary-1" zOrder="99"><pos horzOffset="900" vertOffset="901"/><script>ordinary</script></equation>
      <tbl id="ordinary-table" rowCnt="1" colCnt="1"><tr><tc><subList><p paraPrIDRef="0"><run charPrIDRef="0"><equation id="ordinary-table-eq"><script>ordinary table</script></equation></run></p></subList><cellAddr colAddr="0" rowAddr="0"/><cellSpan rowSpan="1" colSpan="1"/></tc></tr></tbl>
      <rect id="textbox-1" instid="textbox-inst" zOrder="98"><drawText><subList><p paraPrIDRef="0"><run charPrIDRef="0"><equation id="textbox-eq"><script>textbox</script></equation></run></p></subList></drawText></rect>
      <ctrl><endNote instId="9007199254740993"><subList><p paraPrIDRef="0"><run charPrIDRef="0">
        <pic id="9007199254740995" instid="9007199254740996" zOrder="7">
          <pos horzOffset="101" vertOffset="102"/>
          <caption><subList><p paraPrIDRef="0"><run charPrIDRef="0">
            <t>caption</t>
            <equation id="9007199254740997" zOrder="31"><pos horzOffset="111" vertOffset="112"/><script>{a} over {b}</script></equation>
          </run></p></subList></caption>
        </pic>
      </run></p></subList></endNote></ctrl>
      <container id="group-1" instid="group-inst" zOrder="8">
        <rect id="9007199254740999" instid="9007199254741000" zOrder="41">
          <pos horzOffset="201" vertOffset="202"/>
          <drawText><subList><p paraPrIDRef="0"><run charPrIDRef="0">
            <equation zOrder="42"><script>x ^{2}</script></equation>
          </run></p></subList></drawText>
        </rect>
      </container>
    </run>
  </p>
</sec>"#;

fn visual_equations_fixture() -> Vec<u8> {
    fixture_with_section(SECTION_XML)
}

fn fixture_with_section(section_xml: &str) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default();
    writer.start_file("mimetype", stored).unwrap();
    writer.write_all(b"application/hwp+zip").unwrap();
    writer.start_file("Contents/header.xml", deflated).unwrap();
    writer.write_all(HEADER_XML.as_bytes()).unwrap();
    writer.start_file("Contents/section0.xml", deflated).unwrap();
    writer.write_all(section_xml.as_bytes()).unwrap();
    writer.finish().unwrap().into_inner()
}

fn nested_containers_section(levels: usize) -> String {
    let mut visual = concat!(
        r#"<rect id="deep-rect" instid="deep-rect-inst" zOrder="17">"#,
        r#"<offset x="701" y="702"/>"#,
        r#"<drawText><subList><p paraPrIDRef="0"><run charPrIDRef="0">"#,
        r#"<equation id="deep-equation"><script>deep</script></equation>"#,
        r#"</run></p></subList></drawText></rect>"#,
    )
    .to_string();
    for level in (0..levels).rev() {
        visual = format!(
            r#"<container groupLevel="{level}" instid="container-{level}">{visual}</container>"#
        );
    }
    format!(
        r#"<sec><p id="1" paraPrIDRef="0" styleIDRef="0"><run charPrIDRef="0">{visual}</run></p></sec>"#
    )
}

fn nested_table_controls_section(levels: usize) -> String {
    let mut visual = concat!(
        r#"<pic id="deep-picture" instid="deep-picture-inst" zOrder="19">"#,
        r#"<pos horzOffset="801" vertOffset="802"/>"#,
        r#"<caption><subList><p paraPrIDRef="0"><run charPrIDRef="0">"#,
        r#"<equation id="deep-caption-equation"><script>deep caption</script></equation>"#,
        r#"</run></p></subList></caption></pic>"#,
    )
    .to_string();
    for level in (0..levels).rev() {
        visual = if level % 2 == 0 {
            format!(
                r#"<tbl id="table-{level}" rowCnt="1" colCnt="1"><tr><tc><subList><p paraPrIDRef="0"><run charPrIDRef="0">{visual}</run></p></subList><cellAddr colAddr="0" rowAddr="0"/><cellSpan rowSpan="1" colSpan="1"/></tc></tr></tbl>"#
            )
        } else {
            format!(
                r#"<ctrl><endNote instId="{level}"><subList><p paraPrIDRef="0"><run charPrIDRef="0">{visual}</run></p></subList></endNote></ctrl>"#
            )
        };
    }
    format!(
        r#"<sec><p id="1" paraPrIDRef="0" styleIDRef="0"><run charPrIDRef="0">{visual}</run></p></sec>"#
    )
}

#[test]
fn visual_equations_report_preserves_only_supported_visual_domains() {
    let (_document, report) = HwpxDecoder::decode_with_report(&visual_equations_fixture()).unwrap();

    assert_eq!(report.schema_version, 4);
    assert_eq!(report.equations.len(), 2);

    let picture = &report.equations[0];
    assert_eq!(picture.domain, HwpxVisualEquationDomain::PictureCaption);
    assert_eq!(picture.parent_kind, HwpxVisualEquationParentKind::Picture);
    assert_eq!(picture.id, "9007199254740997");
    assert_eq!(picture.equation_object_id.as_deref(), Some("9007199254740997"));
    assert_eq!(picture.parent_object_id.as_deref(), Some("9007199254740995"));
    assert_eq!(picture.parent_instance_id.as_deref(), Some("9007199254740996"));
    assert_eq!(
        picture.parent_path,
        "section[0]/paragraph[0]/run[0]/ctrl[0]/endnote/paragraph[0]/run[0]/picture[0]"
    );
    assert_eq!(picture.document_order, 0);
    assert_eq!(picture.parent_order, 0);
    assert_eq!(picture.z_order, 31);
    assert_eq!(picture.raw_position.horz_offset, 111);
    assert_eq!(picture.raw_position.vert_offset, 112);
    assert_eq!(picture.geometry.raw_box_size, None);
    assert_eq!(picture.geometry.raw_equation_size, None);
    assert_eq!(picture.geometry.raw_base_unit, None);
    assert_eq!(picture.geometry.scale.horz, "1");
    assert_eq!(picture.geometry.scale.vert, "1");
    assert_eq!(picture.geometry.display_box_size, None);
    assert_eq!(picture.geometry.display_equation_size, None);
    assert_eq!(picture.geometry.render_base_unit, None);
    assert_eq!(picture.script, "{a} over {b}");
    assert_eq!(picture.latex, None);

    let grouped = &report.equations[1];
    assert_eq!(grouped.domain, HwpxVisualEquationDomain::GroupDrawText);
    assert_eq!(grouped.parent_kind, HwpxVisualEquationParentKind::Container);
    assert_eq!(grouped.id, "section[0]/paragraph[0]/run[0]/container[0]/rect[0]/equation[0]");
    assert_eq!(grouped.equation_object_id, None);
    assert_eq!(grouped.parent_object_id.as_deref(), Some("9007199254740999"));
    assert_eq!(grouped.parent_instance_id.as_deref(), Some("9007199254741000"));
    assert_eq!(grouped.parent_path, "section[0]/paragraph[0]/run[0]/container[0]/rect[0]");
    assert_eq!(grouped.document_order, 1);
    assert_eq!(grouped.parent_order, 0);
    assert_eq!(grouped.z_order, 42);
    assert_eq!(grouped.raw_position.horz_offset, 201);
    assert_eq!(grouped.raw_position.vert_offset, 202);
    assert_eq!(grouped.geometry.raw_box_size, None);
    assert_eq!(grouped.geometry.raw_equation_size, None);
    assert_eq!(grouped.geometry.display_box_size, None);
    assert_eq!(grouped.geometry.display_equation_size, None);
    assert_eq!(grouped.script, "x ^{2}");
    assert_eq!(grouped.latex, None);

    let serialized = serde_json::to_value(report).unwrap();
    assert_eq!(serialized["equations"][0]["equation_object_id"], "9007199254740997");
    assert_eq!(serialized["equations"][1]["parent_instance_id"], "9007199254741000");
    assert!(serialized["equations"][0]["geometry"]["display_box_size"].is_null());
}

#[test]
fn visual_equations_preserve_interleaved_nested_container_order() {
    let section = r#"<sec><p id="1" paraPrIDRef="0" styleIDRef="0"><run charPrIDRef="0">
      <container instid="outer">
        <container instid="nested-first"><rect id="nested-rect" instid="nested-rect-inst">
          <drawText><subList><p paraPrIDRef="0"><run charPrIDRef="0">
            <equation id="nested-equation"><script>nested first</script></equation>
          </run></p></subList></drawText>
        </rect></container>
        <rect id="sibling-rect" instid="sibling-rect-inst">
          <drawText><subList><p paraPrIDRef="0"><run charPrIDRef="0">
            <equation id="sibling-equation"><script>sibling second</script></equation>
          </run></p></subList></drawText>
        </rect>
      </container>
    </run></p></sec>"#;

    let (_document, report) =
        HwpxDecoder::decode_with_report(&fixture_with_section(section)).unwrap();

    let ids: Vec<&str> = report.equations.iter().map(|equation| equation.id.as_str()).collect();
    assert_eq!(ids, vec!["nested-equation", "sibling-equation"]);
    assert_eq!(report.equations[0].document_order, 0);
    assert_eq!(report.equations[1].document_order, 1);
}

#[test]
fn visual_equations_report_traverses_header_and_footer_controls() {
    let section = r#"<sec><p id="1" paraPrIDRef="0" styleIDRef="0"><run charPrIDRef="0">
      <ctrl><header id="header-1" applyPageType="BOTH"><subList><p paraPrIDRef="0"><run charPrIDRef="0">
        <pic id="header-picture" instid="header-picture-inst"><caption><subList><p paraPrIDRef="0"><run charPrIDRef="0">
          <equation id="header-equation"><script>header</script></equation>
        </run></p></subList></caption></pic>
      </run></p></subList></header></ctrl>
      <ctrl><footer id="footer-1" applyPageType="BOTH"><subList><p paraPrIDRef="0"><run charPrIDRef="0">
        <container instid="footer-container"><rect id="footer-rect" instid="footer-rect-inst">
          <drawText><subList><p paraPrIDRef="0"><run charPrIDRef="0">
            <equation id="footer-equation"><script>footer</script></equation>
          </run></p></subList></drawText>
        </rect></container>
      </run></p></subList></footer></ctrl>
    </run></p></sec>"#;

    let (_document, report) =
        HwpxDecoder::decode_with_report(&fixture_with_section(section)).unwrap();

    let ids: Vec<&str> = report.equations.iter().map(|equation| equation.id.as_str()).collect();
    assert_eq!(ids, vec!["header-equation", "footer-equation"]);
    assert!(report.equations[0].parent_path.contains("/header/"));
    assert!(report.equations[1].parent_path.contains("/footer/"));
}

#[test]
fn legacy_decode_ignores_visual_report_projection_overflow() {
    let section = r#"<sec><p id="1" paraPrIDRef="0" styleIDRef="0"><run charPrIDRef="0">
      <container instid="group-inst"><rect id="overflow-rect" instid="overflow-rect-inst">
        <offset x="2147483647" y="0"/>
        <drawText><subList><p paraPrIDRef="0"><run charPrIDRef="0">
          <equation id="overflow-equation"><pos horzOffset="1" vertOffset="0"/><script>x</script></equation>
        </run></p></subList></drawText>
      </rect></container>
    </run></p></sec>"#;
    let fixture = fixture_with_section(section);

    let path =
        std::env::temp_dir().join(format!("hwpforge-visual-overflow-{}.hwpx", std::process::id()));
    std::fs::write(&path, &fixture).unwrap();
    let legacy_file_result = HwpxDecoder::decode_file(&path);
    let report_file_result = HwpxDecoder::decode_file_with_report(&path);
    std::fs::remove_file(&path).unwrap();

    assert!(HwpxDecoder::decode(&fixture).is_ok());
    assert!(HwpxDecoder::decode_with_report(&fixture).is_err());
    assert!(legacy_file_result.is_ok());
    assert!(report_file_result.is_err());
}

#[test]
fn visual_equation_explicit_zero_z_order_does_not_inherit_parent() {
    let section = r#"<sec><p id="1" paraPrIDRef="0" styleIDRef="0"><run charPrIDRef="0">
      <container instid="group-inst"><rect id="rect-1" instid="rect-inst" zOrder="41">
        <drawText><subList><p paraPrIDRef="0"><run charPrIDRef="0">
          <equation id="explicit-zero" zOrder="0"><script>zero</script></equation>
          <equation id="absent-z-order"><script>absent</script></equation>
        </run></p></subList></drawText>
      </rect></container>
    </run></p></sec>"#;

    let (_document, report) =
        HwpxDecoder::decode_with_report(&fixture_with_section(section)).unwrap();

    assert_eq!(report.equations.len(), 2);
    assert_eq!(report.equations[0].id, "explicit-zero");
    assert_eq!(
        report.equations[0].z_order, 0,
        "an explicit wire zOrder=0 must not inherit the parent z-order"
    );
    assert_eq!(report.equations[1].id, "absent-z-order");
    assert_eq!(
        report.equations[1].z_order, 41,
        "only an absent equation zOrder may inherit the visual parent"
    );
}

#[test]
fn visual_equation_group_offset_and_picture_placement_are_preserved() {
    let section = r#"<sec><p id="1" paraPrIDRef="0" styleIDRef="0"><run charPrIDRef="0">
      <container instid="group-inst"><rect id="rect-1" instid="rect-inst" zOrder="41">
        <offset x="321" y="-654"/>
        <drawText><subList><p paraPrIDRef="0"><run charPrIDRef="0">
          <equation id="group-offset"><script>group</script></equation>
        </run></p></subList></drawText>
      </rect></container>
      <pic id="picture-1" instid="picture-inst" zOrder="42">
        <offset x="11" y="12"/><pos horzOffset="421" vertOffset="422"/>
        <caption><subList><p paraPrIDRef="0"><run charPrIDRef="0">
          <equation id="picture-placement"><script>picture</script></equation>
        </run></p></subList></caption>
      </pic>
    </run></p></sec>"#;

    let (_document, report) =
        HwpxDecoder::decode_with_report(&fixture_with_section(section)).unwrap();

    assert_eq!(report.equations.len(), 2);
    assert_eq!(report.equations[0].id, "group-offset");
    assert_eq!(report.equations[0].raw_position.horz_offset, 321);
    assert_eq!(report.equations[0].raw_position.vert_offset, -654);
    assert_eq!(report.equations[1].id, "picture-placement");
    assert_eq!(report.equations[1].raw_position.horz_offset, 421);
    assert_eq!(report.equations[1].raw_position.vert_offset, 422);
}

#[test]
fn visual_equation_group_position_adds_rect_placement_to_child_local_pos() {
    let section = r#"<sec><p id="1" paraPrIDRef="0" styleIDRef="0"><run charPrIDRef="0">
      <container instid="group-inst">
        <pos horzOffset="100" vertOffset="200"/>
        <rect id="2010924737" instid="rect-zero-child" zOrder="41">
          <offset x="0" y="11428"/>
          <drawText><subList><p paraPrIDRef="0"><run charPrIDRef="0">
            <equation id="explicit-zero-child"><pos horzOffset="0" vertOffset="0"/><script>zero child</script></equation>
          </run></p></subList></drawText>
        </rect>
        <rect id="rect-local-child" instid="rect-local-child-inst" zOrder="42">
          <offset x="321" y="-654"/>
          <drawText><subList><p paraPrIDRef="0"><run charPrIDRef="0">
            <equation id="local-child"><pos horzOffset="11" vertOffset="12"/><script>local child</script></equation>
          </run></p></subList></drawText>
        </rect>
        <rect id="rect-unsigned-offset" instid="rect-unsigned-offset-inst" zOrder="43">
          <offset x="4294966848" y="11428"/>
          <drawText><subList><p paraPrIDRef="0"><run charPrIDRef="0">
            <equation id="unsigned-offset"><pos horzOffset="0" vertOffset="0"/><script>unsigned offset</script></equation>
          </run></p></subList></drawText>
        </rect>
      </container>
    </run></p></sec>"#;

    let (_document, report) =
        HwpxDecoder::decode_with_report(&fixture_with_section(section)).unwrap();

    let positions: Vec<(&str, i32, i32)> = report
        .equations
        .iter()
        .map(|equation| {
            (
                equation.id.as_str(),
                equation.raw_position.horz_offset,
                equation.raw_position.vert_offset,
            )
        })
        .collect();
    assert_eq!(
        positions,
        vec![
            ("explicit-zero-child", 100, 11628),
            ("local-child", 432, -442),
            ("unsigned-offset", -348, 11628),
        ],
        "group positions must combine container, rect, and equation-local coordinates"
    );
}

#[test]
fn q524_group_geometry_keeps_render_base_unit_unscaled_while_scaling_display_sizes() {
    let section = r#"<sec><p id="1" paraPrIDRef="0" styleIDRef="0"><run charPrIDRef="0">
      <container instid="group-inst"><rect id="rect-1" instid="rect-inst" zOrder="41">
        <offset x="18928" y="7966"/><orgSz width="8504" height="8504"/>
        <renderingInfo>
          <transMatrix e1="1" e2="0" e3="18928" e4="0" e5="1" e6="7966"/>
          <scaMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/>
          <rotMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/>
          <scaMatrix e1="0.242542" e2="0" e3="1381.427002" e4="0" e5="0.252211" e6="532.402954"/>
          <rotMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/>
        </renderingInfo>
        <drawText><subList><p paraPrIDRef="0"><run charPrIDRef="0">
          <equation id="scaled-label" baseUnit="1100"><sz width="607" height="1125"/><pos horzOffset="0" vertOffset="0"/><script>rm B</script></equation>
        </run></p></subList></drawText>
      </rect></container>
    </run></p></sec>"#;

    let (_document, report) =
        HwpxDecoder::decode_with_report(&fixture_with_section(section)).unwrap();

    assert_eq!(report.schema_version, 4);
    let geometry = &report.equations[0].geometry;
    assert_eq!(geometry.raw_box_size.unwrap().width, 8504);
    assert_eq!(geometry.raw_box_size.unwrap().height, 8504);
    assert_eq!(geometry.raw_equation_size.unwrap().width, 607);
    assert_eq!(geometry.raw_equation_size.unwrap().height, 1125);
    assert_eq!(geometry.raw_base_unit, Some(1100));
    assert_eq!(geometry.scale.horz, "0.242542");
    assert_eq!(geometry.scale.vert, "0.252211");
    assert_eq!(geometry.display_box_size.unwrap().width, 2063);
    assert_eq!(geometry.display_box_size.unwrap().height, 2145);
    assert_eq!(geometry.display_equation_size.unwrap().width, 147);
    assert_eq!(geometry.display_equation_size.unwrap().height, 284);
    assert_eq!(geometry.render_base_unit, Some(1100));

    let serialized = serde_json::to_value(report).unwrap();
    assert_eq!(
        serialized["equations"][0]["raw_position"],
        serde_json::json!({"horz_offset": 18928, "vert_offset": 7966})
    );
    assert_eq!(
        serialized["equations"][0]["translation"],
        serde_json::json!({"horz": "1381.427002", "vert": "532.402954"})
    );
    assert_eq!(
        serialized["equations"][0]["display_position"],
        serde_json::json!({"horz_offset": 20309, "vert_offset": 8498})
    );
    assert_eq!(serialized["equations"][0]["geometry"]["render_base_unit"], 1100);
    assert!(
        serialized["equations"][0]["geometry"].get("display_base_unit").is_none(),
        "schema v4 must not expose the misleading scale-applied font base unit"
    );
}

#[test]
fn q448_group_position_uses_translation_but_render_base_unit_stays_raw() {
    let section = r#"<sec><p id="1" paraPrIDRef="0" styleIDRef="0"><run charPrIDRef="0">
      <container id="2010952039" instid="937210216"><rect id="0" instid="937210219" zOrder="28100">
        <offset x="4294965625" y="343"/><orgSz width="2477" height="1883"/>
        <renderingInfo>
          <transMatrix e1="1" e2="0" e3="-1671" e4="0" e5="1" e6="343"/>
          <scaMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/>
          <rotMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/>
          <scaMatrix e1="0.478401" e2="0" e3="1861" e4="0" e5="0.831652" e6="679"/>
          <rotMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/>
        </renderingInfo>
        <drawText><subList><p paraPrIDRef="0"><run charPrIDRef="0">
          <equation id="2010952060" baseUnit="900"><sz width="405" height="900"/><pos horzOffset="0" vertOffset="0"/><script>y</script></equation>
        </run></p></subList></drawText>
      </rect></container>
    </run></p></sec>"#;

    let (_document, report) =
        HwpxDecoder::decode_with_report(&fixture_with_section(section)).unwrap();
    let serialized = serde_json::to_value(report).unwrap();
    let equation = &serialized["equations"][0];

    assert_eq!(equation["equation_object_id"], "2010952060");
    assert_eq!(
        equation["raw_position"],
        serde_json::json!({"horz_offset": -1671, "vert_offset": 343})
    );
    assert_eq!(equation["translation"], serde_json::json!({"horz": "1861", "vert": "679"}));
    assert_eq!(
        equation["display_position"],
        serde_json::json!({"horz_offset": 190, "vert_offset": 1022})
    );
    assert_eq!(equation["geometry"]["scale"]["vert"], "0.831652");
    assert_eq!(equation["geometry"]["display_equation_size"]["height"], 748);
    assert_eq!(equation["geometry"]["render_base_unit"], 900);
    assert!(equation["geometry"].get("display_base_unit").is_none());
}

#[test]
fn visual_equation_render_base_unit_falls_back_to_raw_equation_height() {
    let section = r#"<sec><p id="1" paraPrIDRef="0" styleIDRef="0"><run charPrIDRef="0">
      <container instid="group-inst"><rect id="rect-1" instid="rect-inst">
        <orgSz width="2477" height="1883"/>
        <renderingInfo><scaMatrix e1="0.478401" e2="0" e3="0" e4="0" e5="0.831652" e6="0"/></renderingInfo>
        <drawText><subList><p paraPrIDRef="0"><run charPrIDRef="0">
          <equation id="height-fallback"><sz width="405" height="900"/><script>y</script></equation>
        </run></p></subList></drawText>
      </rect></container>
    </run></p></sec>"#;

    let (_document, report) =
        HwpxDecoder::decode_with_report(&fixture_with_section(section)).unwrap();
    let geometry = &report.equations[0].geometry;

    assert_eq!(geometry.raw_base_unit, None);
    assert_eq!(geometry.raw_equation_size.unwrap().height, 900);
    assert_eq!(geometry.render_base_unit, Some(900));
    assert_eq!(geometry.display_equation_size.unwrap().height, 748);
}

#[test]
fn visual_equation_display_position_overflow_fails_closed_without_losing_wire_translation() {
    let section = r#"<sec><p id="1" paraPrIDRef="0" styleIDRef="0"><run charPrIDRef="0">
      <container instid="group-inst"><rect id="rect-1" instid="rect-inst">
        <offset x="2147483647" y="0"/><orgSz width="100" height="100"/>
        <renderingInfo>
          <transMatrix e1="1" e2="0" e3="2147483647" e4="0" e5="1" e6="0"/>
          <scaMatrix e1="1" e2="0" e3="1" e4="0" e5="1" e6="0"/>
        </renderingInfo>
        <drawText><subList><p paraPrIDRef="0"><run charPrIDRef="0">
          <equation id="overflow" baseUnit="100"><sz width="100" height="100"/><script>x</script></equation>
        </run></p></subList></drawText>
      </rect></container>
    </run></p></sec>"#;

    let (_document, report) =
        HwpxDecoder::decode_with_report(&fixture_with_section(section)).unwrap();
    let serialized = serde_json::to_value(report).unwrap();
    let equation = &serialized["equations"][0];

    assert_eq!(
        equation["raw_position"],
        serde_json::json!({"horz_offset": 2147483647_i64, "vert_offset": 0})
    );
    assert_eq!(equation["translation"], serde_json::json!({"horz": "1", "vert": "0"}));
    assert!(equation["display_position"].is_null());
}

#[test]
fn visual_equation_invalid_scale_preserves_geometry_and_raw_render_base_unit() {
    let section = r#"<sec><p id="1" paraPrIDRef="0" styleIDRef="0"><run charPrIDRef="0">
      <container instid="group-inst"><rect id="rect-1" instid="rect-inst">
        <orgSz width="8504" height="8504"/>
        <renderingInfo><scaMatrix e1="not-a-number" e2="0" e3="0" e4="0" e5="-1" e6="0"/></renderingInfo>
        <drawText><subList><p paraPrIDRef="0"><run charPrIDRef="0">
          <equation id="invalid-scale" baseUnit="1100"><sz width="607" height="1125"/><script>rm B</script></equation>
        </run></p></subList></drawText>
      </rect></container>
    </run></p></sec>"#;

    let (_document, report) =
        HwpxDecoder::decode_with_report(&fixture_with_section(section)).unwrap();

    let geometry = &report.equations[0].geometry;
    assert_eq!(geometry.raw_box_size.unwrap().width, 8504);
    assert_eq!(geometry.raw_equation_size.unwrap().height, 1125);
    assert_eq!(geometry.raw_base_unit, Some(1100));
    assert_eq!(geometry.scale.horz, "not-a-number");
    assert_eq!(geometry.scale.vert, "-1");
    assert_eq!(geometry.display_box_size, None);
    assert_eq!(geometry.display_equation_size, None);
    assert_eq!(geometry.render_base_unit, Some(1100));

    let serialized = serde_json::to_value(report).unwrap();
    assert!(serialized["equations"][0]["geometry"]["display_box_size"].is_null());
    assert!(serialized["equations"][0]["geometry"]["display_equation_size"].is_null());
    assert_eq!(serialized["equations"][0]["geometry"]["render_base_unit"], 1100);
    assert!(serialized["equations"][0]["geometry"].get("display_base_unit").is_none());
}

#[test]
fn visual_equation_nesting_boundary_succeeds() {
    let section = nested_containers_section(32);

    let (_document, report) = HwpxDecoder::decode_with_report(&fixture_with_section(&section))
        .expect("32 visual-container levels must remain within the decoder boundary");

    assert_eq!(report.equations.len(), 1);
    assert_eq!(report.equations[0].id, "deep-equation");
}

#[test]
fn visual_equation_container_nesting_depth_exceeded_fails_closed() {
    let section = nested_containers_section(33);

    let result = HwpxDecoder::decode_with_report(&fixture_with_section(&section));

    match result {
        Ok(_) => panic!("33 visual-container levels must fail closed"),
        Err(error) => assert!(
            error.to_string().contains("visual-equation nesting depth 32 exceeds limit of 32"),
            "unexpected error: {error}"
        ),
    }
}

#[test]
fn visual_equation_table_control_nesting_depth_exceeded_fails_closed() {
    let section = nested_table_controls_section(33);

    let result = HwpxDecoder::decode_with_report(&fixture_with_section(&section));

    match result {
        Ok(_) => panic!("33 table/control levels must fail closed"),
        Err(error) => assert!(
            error.to_string().contains("visual-equation nesting depth 32 exceeds limit of 32"),
            "unexpected error: {error}"
        ),
    }
}
