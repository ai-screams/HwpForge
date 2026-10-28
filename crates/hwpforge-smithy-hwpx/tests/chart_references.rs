use hwpforge_core::{
    chart::{ChartData, ChartType},
    Control, Document, ImageStore, PageSettings, Paragraph, Run, Section,
};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge_smithy_hwpx::{style_store_for_preset, HwpxDecoder, HwpxEncoder};
use std::io::{Cursor, Write};

fn package(xml: &str) -> Vec<u8> {
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::control(
                Control::chart(ChartType::Column, ChartData::category(&["a"], &[("s", &[1.0])])),
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    let base = HwpxEncoder::encode(
        &doc.validate().unwrap(),
        &style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
    )
    .unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(base)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let entry = archive.by_index(i).unwrap();
        if entry.name().unwrap().starts_with("Chart/") {
            out.start_file(entry.name().unwrap(), zip::write::SimpleFileOptions::default())
                .unwrap();
            out.write_all(xml.as_bytes()).unwrap();
        } else {
            out.raw_copy_file(entry).unwrap();
        }
    }
    out.finish().unwrap().into_inner()
}

fn xml(kind: &str, text: &str, values: &str) -> String {
    format!(
        r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><c:chart><c:title><c:tx><c:rich><a:p><a:r><a:t>{text}</a:t></a:r></a:p></c:rich></c:tx></c:title><c:plotArea><c:{kind}><c:ser><c:tx><c:strRef><c:f>ignore</c:f><c:strCache><c:pt idx="0"><c:v>{text}</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strLit><c:pt idx="0"><c:v>{text}</c:v></c:pt><c:pt idx="1"><c:v>next</c:v></c:pt></c:strLit></c:cat>{values}</c:ser></c:{kind}></c:plotArea></c:chart></c:chartSpace>"#
    )
}
fn points(tag: &str, cache: &str) -> String {
    format!("<c:{tag}><c:{cache}><c:f>999</c:f><c:formatCode>777</c:formatCode><c:pt idx=\"0\"><c:v> 1&#50;3 </c:v></c:pt><c:pt idx=\"1\"><c:v>&#52;</c:v></c:pt><c:pt idx=\"2\"><c:v>&#53;&#54;</c:v></c:pt></c:{cache}></c:{tag}>")
}

// 이것을 실패시키는 것: 참조·Text 무시, 조각별 trim, 시작/종료 t 경계 제거,
// 이중 unescape, 수식·formatCode 수집, 빈 값 생략 제거.
#[test]
fn chart_value_references() {
    for (wire, expected) in [
        (" T &amp; &lt; &#xAC00; Z ", "T & < 가 Z"),
        ("&amp;", "&"),
        ("&amp;&lt;&#xAC00;", "&<가"),
        ("&amp;amp;", "&amp;"),
        ("  ", ""),
    ] {
        for cache in ["numLit", "numCache"] {
            let d = HwpxDecoder::decode(&package(&xml("barChart", wire, &points("val", cache))))
                .unwrap();
            let c = d
                .document
                .sections()
                .iter()
                .flat_map(|s| &s.paragraphs)
                .flat_map(|p| &p.runs)
                .filter_map(|r| r.content.as_control())
                .find(|c| c.is_chart())
                .unwrap();
            let Control::Chart { title, data, .. } = c else { panic!("{c:?}") };
            assert_eq!(title.as_deref(), if expected.is_empty() { None } else { Some(expected) });
            let ChartData::Category { categories, series } = data else { panic!() };
            assert_eq!(
                categories,
                &if expected.is_empty() { vec!["next"] } else { vec![expected, "next"] }
            );
            assert_eq!(series[0].name, expected);
            assert_eq!(series[0].values, [123.0, 4.0, 56.0]);
        }
    }
}

// 이것을 실패시키는 것: 산점도·거품형 x/y에서 참조 무시 또는 시작/종료 v 경계 제거.
#[test]
fn chart_xy_references() {
    for kind in ["scatterChart", "bubbleChart"] {
        let values =
            points("xVal", "numLit") + &points("yVal", "numCache").replace("1&#50;3", "4&#53;6");
        let d = HwpxDecoder::decode(&package(&xml(kind, "&amp;", &values))).unwrap();
        let c = d
            .document
            .sections()
            .iter()
            .flat_map(|s| &s.paragraphs)
            .flat_map(|p| &p.runs)
            .filter_map(|r| r.content.as_control())
            .find(|c| c.is_chart())
            .unwrap();
        let Control::Chart { data: ChartData::Xy { series }, .. } = c else { panic!() };
        assert_eq!(series[0].name, "&");
        assert_eq!(series[0].x_values, [123.0, 4.0, 56.0]);
        assert_eq!(series[0].y_values, [456.0, 4.0, 56.0]);
    }
}

// 이것을 실패시키는 것: 참조 오류를 무시하여 문서 디코드를 성공시키기.
#[test]
fn chart_invalid_references_fail_document() {
    for reference in ["&unknown;", "&#0;", "&#xD800;", "&#x110000;"] {
        assert!(HwpxDecoder::decode(&package(&xml(
            "barChart",
            reference,
            &points("val", "numLit")
        )))
        .is_err());
    }
}

// 이것을 실패시키는 것: 참조 오류 또는 XML 구문 오류의 실제 파트 이름을 wildcard로 교체.
#[test]
fn chart_error_identifies_part() {
    for content in ["<c:chart><c:v>&unknown;</c:v></c:chart>", "<c:chart></wrong>"] {
        let error = HwpxDecoder::decode(&package(content)).unwrap_err();
        let hwpforge_smithy_hwpx::HwpxError::XmlParse { file, .. } = error else {
            panic!("{error:?}")
        };
        assert_eq!(file, "Chart/chart1.xml");
    }
}
