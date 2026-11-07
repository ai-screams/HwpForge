#[allow(dead_code)]
#[path = "../examples/wire_emission.rs"]
mod inputs;
#[path = "support/package.rs"]
mod package;
#[path = "support/xml.rs"]
mod xml;
use hwpforge_core::Control;
use xml::Element;

fn document(name: &str) -> Element {
    let (_, bytes) = inputs::documents().into_iter().find(|(n, _)| *n == name).unwrap();
    Element::parse(&package::section_xml(&bytes))
}
fn coefficients(node: &Element, expected: [&str; 6]) {
    for (key, value) in ["e1", "e2", "e3", "e4", "e5", "e6"].into_iter().zip(expected) {
        assert_eq!(node.attr(key), Some(value), "{} {key}", node.name);
    }
}
fn identity(node: &Element) {
    coefficients(node, ["1", "0", "0", "0", "1", "0"]);
}
fn rendering(node: &Element) -> &Element {
    let ri = node.child("hp:renderingInfo");
    assert_eq!(ri.child_names(), ["hc:transMatrix", "hc:scaMatrix", "hc:rotMatrix"]);
    ri
}
fn point(node: &Element, x: &str, y: &str) {
    assert_eq!(node.attr("x"), Some(x));
    assert_eq!(node.attr("y"), Some(y));
}

// 이것을 실패시키는 것: 글맵시 S의 정수 identity, pt2의 hp 접두사, winBrush 중복, text escape 제거.
#[test]
fn textart_keeps_literal_matrices_points_and_brush() {
    let doc = document("textart");
    let arts = doc.descendants("hp:textart");
    assert_eq!(arts.len(), 2);
    for (art, sx, sy, color) in
        [(arts[0], "1.000000", "1.000000", "#123456"), (arts[1], "2.000000", "3.000000", "#ABCDEF")]
    {
        assert_eq!(art.attr("text"), Some("A&B<글>"));
        let ri = rendering(art);
        identity(ri.child("hc:transMatrix"));
        identity(ri.child("hc:rotMatrix"));
        coefficients(ri.child("hc:scaMatrix"), [sx, "0", "0", "0", sy, "0"]);
        let names = art.child_names();
        let start = names.iter().position(|n| *n == "hc:pt0").unwrap();
        assert_eq!(&names[start..start + 4], ["hc:pt0", "hc:pt1", "hc:pt2", "hc:pt3"]);
        for (name, x, y) in [
            ("hc:pt0", "0", "0"),
            ("hc:pt1", "200", "0"),
            ("hc:pt2", "200", "100"),
            ("hc:pt3", "0", "100"),
        ] {
            point(art.child(name), x, y);
        }
        let brush = art.child("hc:fillBrush");
        assert_eq!(brush.child_names(), ["hc:winBrush"]);
        let win = brush.child("hc:winBrush");
        assert_eq!(win.attr("faceColor"), Some(color));
        assert_eq!(win.attr("hatchColor"), Some("#000000"));
        assert_eq!(win.attr("alpha"), Some("0"));
        assert_eq!(win.attr("hatchStyle"), None);
    }
}

// 이것을 실패시키는 것: translation 검색 문자열 변경, 부모 원점 누락, chain 마지막 R 삭제·S 순서 교환·첫 S identity·그룹 자신의 S 축척 무시.
#[test]
fn group_children_keep_translation_and_pair_chain() {
    let doc = document("groups");
    for (tag, count, scale) in [
        ("hp:textart", 2, ["1.000000", "0", "0", "0", "1.000000", "0"]),
        ("hp:rect", 3, ["1", "0", "0", "0", "1", "0"]),
    ] {
        let nodes = doc.descendants(tag);
        // 이것을 실패시키는 것: 검사 대상 뒤에 그룹 자식이 하나 더 생기기.
        assert_eq!(nodes.len(), count);
        for (node, x, y) in [(nodes[0], "1200", "800"), (nodes[1], "1700", "1100")] {
            point(node.child("hp:offset"), x, y);
            let ri = rendering(node);
            coefficients(ri.child("hc:transMatrix"), ["1", "0", x, "0", "1", y]);
            identity(ri.child("hc:rotMatrix"));
            coefficients(ri.child("hc:scaMatrix"), scale);
        }
    }
    let containers = doc.descendants("hp:container");
    assert_eq!(containers.len(), 4);
    let scaled_parent = containers.last().unwrap();
    let parent = rendering(scaled_parent);
    identity(parent.child("hc:transMatrix"));
    coefficients(parent.child("hc:scaMatrix"), ["2.000000", "0", "0", "0", "2.000000", "0"]);
    identity(parent.child("hc:rotMatrix"));
    let rects = doc.descendants("hp:rect");
    assert_eq!(rects.len(), 3);
    let scaled_child = rects.last().unwrap();
    let ri = scaled_child.child("hp:renderingInfo");
    assert_eq!(
        ri.child_names(),
        ["hc:transMatrix", "hc:scaMatrix", "hc:rotMatrix", "hc:scaMatrix", "hc:rotMatrix"]
    );
    for (node, want) in ri.children.iter().zip([
        ["1", "0", "100", "0", "1", "40"],
        ["2.000000", "0", "0", "0", "2.000000", "0"],
        ["1", "0", "0", "0", "1", "0"],
        ["3.000000", "0", "0", "0", "3.000000", "0"],
        ["1", "0", "0", "0", "1", "0"],
    ]) {
        coefficients(node, want);
    }
}

// 이것을 실패시키는 것: standalone 정수 중심을 실수 중심으로, T.e3=0, 그룹에도 standalone 표기, flip을 S로 이동.
#[test]
fn odd_width_standalone_flip_keeps_translation() {
    let doc = document("mirrors");
    const STANDALONE_RECT_COUNT: usize = 5;
    const GROUP_RECT_COUNT: usize = 1;
    for (tag, count) in [("hp:rect", 6), ("hp:pic", 5)] {
        let nodes = doc.descendants(tag);
        // 이것을 실패시키는 것: 검사 대상 뒤에 반사 도형이 하나 더 생기기.
        assert_eq!(nodes.len(), count);
        let [horizontal, vertical, both, thirty_degrees, ninety_degrees] =
            &nodes[..STANDALONE_RECT_COUNT]
        else {
            panic!("five standalone mirrors")
        };
        for (node, t, r, cx, cy, w, h) in [
            (
                horizontal,
                ["1", "0", "1", "0", "1", "0"],
                ["-1.000000", "-0.000000", "100.000000", "-0.000000", "1.000000", "0.000000"],
                "51",
                "25",
                "101",
                "50",
            ),
            (
                vertical,
                ["1", "0", "0", "0", "1", "1"],
                ["1.000000", "0.000000", "0.000000", "0.000000", "-1.000000", "50.000000"],
                "50",
                "26",
                "100",
                "51",
            ),
            (
                both,
                ["1", "0", "1", "0", "1", "1"],
                ["-1.000000", "0.000000", "100.000000", "-0.000000", "-1.000000", "50.000000"],
                "51",
                "26",
                "101",
                "51",
            ),
            (
                thirty_degrees,
                ["1", "0", "7", "0", "1", "22"],
                ["-0.866025", "-0.500000", "105.801250", "-0.500000", "0.866025", "28.349375"],
                "57",
                "47",
                "113",
                "95",
            ),
            (
                ninety_degrees,
                ["1", "0", "-24", "0", "1", "26"],
                ["-0.000000", "-1.000000", "75.000000", "-1.000000", "0.000000", "75.000000"],
                "26",
                "51",
                "51",
                "101",
            ),
        ] {
            let ri = rendering(node);
            coefficients(ri.child("hc:transMatrix"), t);
            identity(ri.child("hc:scaMatrix"));
            coefficients(ri.child("hc:rotMatrix"), r);
            assert_eq!(node.child("hp:rotationInfo").attr("centerX"), Some(cx));
            assert_eq!(node.child("hp:rotationInfo").attr("centerY"), Some(cy));
            assert_eq!(node.child("hp:sz").attr("width"), Some(w));
            assert_eq!(node.child("hp:sz").attr("height"), Some(h));
        }
        assert_eq!(thirty_degrees.child("hp:rotationInfo").attr("angle"), Some("30"));
        assert_eq!(ninety_degrees.child("hp:rotationInfo").attr("angle"), Some("90"));
    }
    let rects = doc.descendants("hp:rect");
    assert_eq!(rects.len(), STANDALONE_RECT_COUNT + GROUP_RECT_COUNT);
    let grouped_horizontal = rects.last().unwrap();
    let ri = rendering(grouped_horizontal);
    identity(ri.child("hc:transMatrix"));
    identity(ri.child("hc:scaMatrix"));
    coefficients(ri.child("hc:rotMatrix"), ["-1", "0", "101", "0", "1", "0"]);
}

// 이것을 실패시키는 것: 그룹 Vertical의 e6 누락, 0.3°를 실수각으로 행렬화, Both의 한 축 누락.
#[test]
fn group_mirrors_keep_integer_coefficients() {
    let doc = document("group-mirrors");
    let rects = doc.descendants("hp:rect");
    assert_eq!(rects.len(), 2);
    for (rect, expected) in
        rects.iter().zip([["1", "0", "0", "0", "-1", "51"], ["-1", "0", "101", "0", "-1", "51"]])
    {
        let ri = rendering(rect);
        identity(ri.child("hc:transMatrix"));
        identity(ri.child("hc:scaMatrix"));
        coefficients(ri.child("hc:rotMatrix"), expected);
    }
}

// 이것을 실패시키는 것: 각주·미주 ID 제거·32bit 절삭·None에 발급, instId 오타, suffix 변경, autoNum 누락.
#[test]
fn note_instance_ids_on_both_kinds() {
    let doc = document("notes");
    for (tag, kind, body, last_id, last_text) in [
        ("hp:footNote", "FOOTNOTE", "foot", "77", "cell foot"),
        ("hp:endNote", "ENDNOTE", "end", "88", "cell end"),
    ] {
        let notes = doc.descendants(tag);
        assert_eq!(notes.len(), 6);
        for (i, (note, id)) in notes
            .iter()
            .zip([
                Some("42"),
                Some("4294967297"),
                None,
                Some("0"),
                Some("18446744073709551615"),
                Some(last_id),
            ])
            .enumerate()
        {
            assert_eq!(note.attr("instId"), id);
            assert_eq!(note.attr("number"), Some(["1", "2", "3", "4", "5", "6"][i]));
            assert_eq!(note.attr("suffixChar"), Some("41"));
            assert_eq!(note.child_names(), ["hp:subList"]);
            let sub = note.child("hp:subList");
            let paragraphs = sub.descendants("hp:p");
            assert_eq!(paragraphs.len(), [2, 2, 2, 2, 2, 1][i]);
            let runs = &paragraphs.first().expect("note marker paragraph").children;
            let first_run = runs.first().expect("note marker run");
            assert_eq!(first_run.name, "hp:run");
            assert_eq!(first_run.children.first().expect("note marker control").name, "hp:ctrl");
            let num = first_run.child("hp:ctrl").child("hp:autoNum");
            assert_eq!(num.attr("numType"), Some(kind));
            assert_eq!(num.attr("num"), Some(["1", "2", "3", "4", "5", "6"][i]));
            let texts = sub.descendants("hp:t");
            assert_eq!(
                texts.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(),
                if i == 5 { vec![" ", last_text] } else { vec![" ", body, "tail"] }
            );
        }
    }
}

// 이것을 실패시키는 것: Smithy round 뒤 modulo, 각 그룹·단독 도형·그림 호출처의 truncation 우회.
#[test]
fn rotation_metadata_boundaries_on_shapes_groups_and_pictures() {
    for (rotation, angle) in [
        (None, "0"),
        (Some(0.), "0"),
        (Some(29.5), "30"),
        (Some(30.5), "31"),
        (Some(-30.), "330"),
        (Some(359.6), "360"),
        (Some(-0.4), "360"),
    ] {
        let rect = inputs::rect(100, 50, rotation, None);
        let bytes = inputs::encode(vec![
            inputs::run(rect.clone()),
            inputs::run(inputs::group(vec![rect], 100, 50)),
            inputs::picture(rotation, None, 100, 50),
        ]);
        let doc = Element::parse(&package::section_xml(&bytes));
        let metadata = doc.descendants("hp:rotationInfo");
        assert_eq!(metadata.len(), 4);
        let [standalone, _group, child, picture] = metadata.as_slice() else {
            panic!("standalone, group, child and picture metadata")
        };
        for node in [standalone, child, picture] {
            assert_eq!(node.attr("angle"), Some(angle));
        }
    }
}

// 이것을 실패시키는 것: gradient/image에 winBrush 유지, 도형별 fill 전달 제거, None의 빈 brush, header/글맵시 자식 중복.
#[test]
fn fill_brush_has_one_child_on_each_output_path() {
    for fill in inputs::fills() {
        for mut shape in inputs::drawings() {
            inputs::set_fill(&mut shape, fill.clone());
            for shape in [shape.clone(), inputs::group(vec![shape], 200, 100)] {
                let bytes = inputs::encode(vec![inputs::run(shape)]);
                let doc = Element::parse(&package::section_xml(&bytes));
                let brushes = doc.descendants("hc:fillBrush");
                let expected = match &fill {
                    Some(hwpforge_core::control::Fill::Gradient { .. }) => vec!["hc:gradation"],
                    Some(hwpforge_core::control::Fill::Image { .. }) => vec!["hc:imgBrush"],
                    Some(hwpforge_core::control::Fill::None) => vec![],
                    _ => vec!["hc:winBrush"],
                };
                assert_eq!(brushes.len(), usize::from(!expected.is_empty()));
                for brush in brushes {
                    assert_eq!(brush.child_names(), expected);
                }
            }
        }
    }
    let (_, bytes) = inputs::documents().into_iter().find(|(n, _)| *n == "shapes").unwrap();
    let header = Element::parse(&package::part(&bytes, "Contents/header.xml"));
    let brushes = header.descendants("hc:fillBrush");
    assert_eq!(brushes.len(), 8);
    for (brush, kind) in brushes.iter().zip([
        "hc:winBrush",
        "hc:winBrush",
        "hc:winBrush",
        "hc:winBrush",
        "hc:winBrush",
        "hc:winBrush",
        "hc:gradation",
        "hc:imgBrush",
    ]) {
        assert_eq!(brush.child_names(), [kind]);
    }
    let doc = document("shapes");
    for tag in ["hp:line", "hp:connectLine"] {
        let lines = doc.descendants(tag);
        assert!(!lines.is_empty(), "{tag} missing");
        for line in lines {
            assert_eq!(line.descendants("hc:fillBrush").len(), 0, "{tag} carries a fill");
        }
    }
}

// 이것을 실패시키는 것: hatchStyle 오타·누락, 전경/배경 교환, 대각선 wire 표기 재반전.
#[test]
fn hatch_style_is_wired_on_shapes_and_borders() {
    let (_, bytes) = inputs::documents().into_iter().find(|(n, _)| *n == "shapes").unwrap();
    let doc = Element::parse(&package::section_xml(&bytes));
    let header = Element::parse(&package::part(&bytes, "Contents/header.xml"));
    let rects = doc.descendants("hp:rect");
    const FILL_RECT_COUNT: usize = 5 * 2;
    const PATTERN_RECT_COUNT: usize = 6;
    const GROUP_RECT_COUNT: usize = 2;
    // 이것을 실패시키는 것: 무늬 검사 범위 뒤에 사각형이 하나 더 생기기.
    assert_eq!(rects.len(), FILL_RECT_COUNT + PATTERN_RECT_COUNT + GROUP_RECT_COUNT);
    for (rect, (_, spelling)) in
        rects[FILL_RECT_COUNT..FILL_RECT_COUNT + PATTERN_RECT_COUNT].iter().zip(inputs::patterns())
    {
        let win = rect.child("hc:fillBrush").child("hc:winBrush");
        assert_eq!(win.attr("faceColor"), Some("#123456"));
        assert_eq!(win.attr("hatchColor"), Some("#ABCDEF"));
        assert_eq!(win.attr("hatchStyle"), Some(spelling));
        assert_eq!(win.attr("alpha"), Some("0"));
    }
    let wins = header.descendants("hc:winBrush");
    let patterned: Vec<_> = wins.into_iter().filter(|n| n.attr("hatchStyle").is_some()).collect();
    assert_eq!(patterned.len(), 6);
    for (win, spelling) in patterned.iter().zip([
        "HORIZONTAL",
        "VERTICAL",
        "SLASH",
        "BACK_SLASH",
        "CROSS",
        "CROSS_DIAGONAL",
    ]) {
        assert_eq!(win.attr("hatchStyle"), Some(spelling));
        assert_eq!(win.attr("faceColor"), Some("#123456"));
        assert_eq!(win.attr("hatchColor"), Some("#ABCDEF"));
    }
    assert_eq!(rects[0].child("hc:fillBrush").child("hc:winBrush").attr("hatchStyle"), None);
}

// 이것을 실패시키는 것: 각 schema 좌표 접두사 변경, 곡선 seg·연결선 점 누락, OLE R 누락·identity 소수 표기.
#[test]
fn coordinate_namespaces_and_matrices_on_each_output_path() {
    let doc = document("shapes");
    for tag in [
        "hp:rect",
        "hp:ellipse",
        "hp:polygon",
        "hp:curve",
        "hp:line",
        "hp:connectLine",
        "hp:pic",
        "hp:ole",
    ] {
        let nodes = doc.descendants(tag);
        assert!(!nodes.is_empty(), "{tag}");
        for node in nodes {
            let ri = rendering(node);
            for matrix in &ri.children {
                identity(matrix);
            }
            match tag {
                "hp:rect" => {
                    for (name, x, y) in [
                        ("hc:pt0", "0", "0"),
                        ("hc:pt1", "200", "0"),
                        ("hc:pt2", "200", "100"),
                        ("hc:pt3", "0", "100"),
                    ] {
                        point(node.child(name), x, y);
                    }
                }
                "hp:line" => {
                    point(node.child("hc:startPt"), "0", "0");
                    point(node.child("hc:endPt"), "200", "100");
                }
                "hp:ellipse" => {
                    for (name, x, y) in [
                        ("hc:center", "100", "50"),
                        ("hc:ax1", "200", "50"),
                        ("hc:ax2", "100", "100"),
                    ] {
                        point(node.child(name), x, y);
                    }
                    let ends = match node.attr("hasArcPr") {
                        Some("0") => [
                            ("hc:start1", "0", "0"),
                            ("hc:end1", "0", "0"),
                            ("hc:start2", "0", "0"),
                            ("hc:end2", "0", "0"),
                        ],
                        Some("1") => [
                            ("hc:start1", "200", "50"),
                            ("hc:end1", "100", "0"),
                            ("hc:start2", "200", "50"),
                            ("hc:end2", "100", "0"),
                        ],
                        _ => panic!("arc kind"),
                    };
                    for (name, x, y) in ends {
                        point(node.child(name), x, y);
                    }
                }
                "hp:polygon" => {
                    let pts: Vec<_> = node.children.iter().filter(|c| c.name == "hc:pt").collect();
                    assert_eq!(pts.len(), 4);
                    for (pt, (x, y)) in pts.into_iter().zip([
                        ("0", "0"),
                        ("200", "0"),
                        ("200", "100"),
                        ("0", "100"),
                    ]) {
                        point(pt, x, y);
                    }
                }
                "hp:curve" => {
                    assert_eq!(node.descendants("hc:pt").len(), 0);
                    assert_eq!(node.descendants("hp:seg").len(), 3);
                }
                "hp:connectLine" => {
                    point(node.child("hp:startPt"), "0", "0");
                    point(node.child("hp:endPt"), "200", "100");
                    let pts = node.child("hp:controlPoints");
                    assert_eq!(pts.child_names(), ["hp:point", "hp:point"]);
                    point(&pts.children[0], "0", "0");
                    point(&pts.children[1], "200", "100");
                }
                "hp:pic" => {
                    let rect = node.child("hp:imgRect");
                    assert_eq!(rect.child_names(), ["hc:pt0", "hc:pt1", "hc:pt2", "hc:pt3"]);
                }
                _ => {}
            }
        }
    }
}

// 이것을 실패시키는 것: 도형 gradient 우선순위 변경·fill_color 제거, header win 우선순위 변경, 글맵시 손실 경고 삭제.
#[test]
fn multiple_fill_children_keep_existing_decode_projection() {
    use hwpforge_core::{control::Fill, RunContent};
    use hwpforge_foundation::Color;
    use hwpforge_smithy_hwpx::{style_store::HwpxFill, HwpxDecoder};
    let win = r##"<hc:winBrush faceColor="#123456" hatchColor="#ABCDEF" alpha="0"/>"##;
    let gradient = concat!(
        r#"<hc:gradation type="LINEAR" angle="17" centerX="0" centerY="0" "#,
        r#"step="255" colorNum="2" stepCenter="50" alpha="0">"#,
        r##"<hc:color value="#000000"/><hc:color value="#FFFFFF"/></hc:gradation>"##,
    );
    let image = concat!(
        r#"<hc:imgBrush mode="TILE"><hc:img binaryItemIDRef="wire" "#,
        r#"bright="0" contrast="0" effect="REAL_PIC" alpha="0"/></hc:imgBrush>"#,
    );
    let invalid = r#"<hc:imgBrush mode="TILE"><hc:img binaryItemIDRef=""/></hc:imgBrush>"#;
    let cases = [
        (format!("{win}{gradient}"), "gradient", Some(Color::from_rgb(18, 52, 86)), 0),
        (format!("{gradient}{win}"), "gradient", Some(Color::from_rgb(18, 52, 86)), 0),
        (format!("{win}{image}"), "image", Some(Color::from_rgb(18, 52, 86)), 0),
        (format!("{gradient}{image}"), "gradient", None, 0),
        (format!("{win}{gradient}{image}"), "gradient", Some(Color::from_rgb(18, 52, 86)), 0),
        (format!("{win}{invalid}"), "solid", Some(Color::from_rgb(18, 52, 86)), 1),
        (String::new(), "solid", None, 0),
    ];
    let base = inputs::encode(vec![inputs::run(inputs::rect(200, 100, None, None))]);
    let original = concat!(
        r##"<hc:fillBrush><hc:winBrush faceColor="#FFFFFF" "##,
        r##"hatchColor="#000000" alpha="0"/></hc:fillBrush>"##,
    );
    for (children, expected, color, warnings) in cases {
        let wire = format!("<hc:fillBrush>{children}</hc:fillBrush>");
        let bytes = package::splice(&base, original, &wire);
        let parsed = Element::parse(&package::section_xml(&bytes));
        assert_eq!(
            parsed.find("hp:rect").child("hc:fillBrush").children.len(),
            Element::parse(&wire).children.len()
        );
        let decoded = HwpxDecoder::decode(&bytes).unwrap();
        let RunContent::Control(ctrl) =
            &decoded.document.sections()[0].paragraphs[0].runs[0].content
        else {
            panic!("control")
        };
        let Control::Rect { style: Some(style), .. } = ctrl.as_ref() else { panic!("rect style") };
        assert_eq!(style.fill_color, color);
        match expected {
            "gradient" => assert!(matches!(style.fill, Some(Fill::Gradient { angle: 17, .. }))),
            "image" => assert!(matches!(style.fill, Some(Fill::Image { .. }))),
            "solid" => assert!(style.fill.is_none()),
            _ => unreachable!(),
        }
        assert_eq!(decoded.warnings.len(), warnings, "{:?}", decoded.warnings);
    }
    let missing = HwpxDecoder::decode(&package::splice(&base, original, "")).unwrap();
    let RunContent::Control(ctrl) = &missing.document.sections()[0].paragraphs[0].runs[0].content
    else {
        panic!("control")
    };
    let Control::Rect { style: Some(style), .. } = ctrl.as_ref() else { panic!("rect style") };
    assert_eq!(style.fill, Some(Fill::None));
    let head = package::part(&base, "Contents/header.xml");
    let start = head.find("<hc:fillBrush>").unwrap();
    let end = start + head[start..].find("</hc:fillBrush>").unwrap() + "</hc:fillBrush>".len();
    for (children, expected) in [
        (format!("{win}{gradient}"), "win"),
        (format!("{win}{image}"), "win"),
        (format!("{gradient}{image}"), "gradient"),
        (format!("{win}{gradient}{image}"), "win"),
        (format!("{image}{gradient}{win}"), "win"),
        (format!("{invalid}{win}"), "win"),
        (String::new(), "none"),
    ] {
        let bytes = package::splice_part(
            &base,
            "Contents/header.xml",
            &head[start..end],
            &format!("<hc:fillBrush>{children}</hc:fillBrush>"),
        );
        let decoded = HwpxDecoder::decode(&bytes).unwrap();
        let border = decoded.style_store.border_fill(1).unwrap();
        match expected {
            "win" => {
                assert!(matches!(
                    &border.fill,
                    Some(HwpxFill::WinBrush { face_color, .. }) if face_color == "#123456"
                ));
                assert_eq!(border.gradient_fill, None);
                assert_eq!(border.image_fill, None);
            }
            "gradient" => {
                assert_eq!(border.fill, None);
                assert_eq!(border.gradient_fill.as_ref().unwrap().angle, 17);
                assert_eq!(border.image_fill, None);
            }
            "none" => {
                assert_eq!(border.fill, None);
                assert_eq!(border.gradient_fill, None);
                assert_eq!(border.image_fill, None);
            }
            _ => unreachable!(),
        }
    }
    let base =
        inputs::encode(vec![inputs::run(inputs::art(200, 100, Color::from_rgb(18, 52, 86)))]);
    let from = concat!(
        r##"<hc:fillBrush><hc:winBrush faceColor="#123456" "##,
        r##"hatchColor="#000000" alpha="0"/></hc:fillBrush>"##,
    );
    for extra in [gradient, image] {
        let bytes =
            package::splice(&base, from, &format!("<hc:fillBrush>{win}{extra}</hc:fillBrush>"));
        let decoded = HwpxDecoder::decode(&bytes).unwrap();
        let RunContent::Control(ctrl) =
            &decoded.document.sections()[0].paragraphs[0].runs[0].content
        else {
            panic!("control")
        };
        let Control::TextArt { fill_color, .. } = ctrl.as_ref() else { panic!("textart") };
        assert_eq!(*fill_color, None);
        assert!(
            decoded.warnings.iter().any(|warning| matches!(
                warning,
                hwpforge_smithy_hwpx::DecodeWarning::ContentLost { kind, .. }
                    if kind == "textart_fill"
            )),
            "{:?}",
            decoded.warnings
        );
    }
    let duplicate =
        package::splice(&base, from, &format!("<hc:fillBrush>{win}{win}</hc:fillBrush>"));
    assert!(HwpxDecoder::decode(&duplicate).is_err());
}

// 이것을 실패시키는 것: 루트 누락, 재귀 생략, 순서 반전, QName 대신 접미사 비교, 없는 이름에 루트 반환.
#[test]
fn xml_descendants_keep_root_names_and_order() {
    let root = Element::parse(
        r#"<hp:p id="root"><hp:p id="first"><hp:p id="nested"/></hp:p><x:p/><hp:p id="last"/></hp:p>"#,
    );
    let nodes = root.descendants("hp:p");
    assert_eq!(
        nodes.iter().map(|node| node.attr("id")).collect::<Vec<_>>(),
        [Some("root"), Some("first"), Some("nested"), Some("last")]
    );
    assert!(root.descendants("missing").is_empty());
}
