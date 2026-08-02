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

    assert_eq!(report.schema_version, 1);
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
    assert_eq!(picture.position.horz_offset, 111);
    assert_eq!(picture.position.vert_offset, 112);
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
    assert_eq!(grouped.position.horz_offset, 201);
    assert_eq!(grouped.position.vert_offset, 202);
    assert_eq!(grouped.script, "x ^{2}");
    assert_eq!(grouped.latex, None);

    let serialized = serde_json::to_value(report).unwrap();
    assert_eq!(serialized["equations"][0]["equation_object_id"], "9007199254740997");
    assert_eq!(serialized["equations"][1]["parent_instance_id"], "9007199254741000");
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
    assert_eq!(report.equations[0].position.horz_offset, 321);
    assert_eq!(report.equations[0].position.vert_offset, -654);
    assert_eq!(report.equations[1].id, "picture-placement");
    assert_eq!(report.equations[1].position.horz_offset, 421);
    assert_eq!(report.equations[1].position.vert_offset, 422);
}

#[test]
fn visual_equation_group_position_adds_rect_placement_to_child_local_pos() {
    let section = r#"<sec><p id="1" paraPrIDRef="0" styleIDRef="0"><run charPrIDRef="0">
      <container instid="group-inst">
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
            (equation.id.as_str(), equation.position.horz_offset, equation.position.vert_offset)
        })
        .collect();
    assert_eq!(
        positions,
        vec![
            ("explicit-zero-child", 0, 11428),
            ("local-child", 332, -642),
            ("unsigned-offset", -448, 11428),
        ],
        "group positions must combine rect placement with equation-local coordinates"
    );
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
