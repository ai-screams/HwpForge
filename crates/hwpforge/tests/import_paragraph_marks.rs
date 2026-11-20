#![cfg(feature = "ops-docx")]
use hwpforge::{
    hwpx::{EncodeWarning, HwpxDecoder, ParagraphPath, PathSeg},
    ops::{convert_docx, OpsWarning},
};
#[path = "support/docx.rs"]
mod docx;
#[path = "../../hwpforge-smithy-hwpx/tests/support/xml.rs"]
mod xml;
use std::io::{Cursor, Read};
fn source(n: usize) -> Vec<u8> {
    let font = "<w:rFonts w:ascii='Cambria' w:hAnsi='Cambria' w:eastAsia='Cambria'/>";
    let p=format!("<w:p><w:pPr><w:rPr>{font}<w:sz w:val='40'/></w:rPr></w:pPr><w:r><w:rPr>{font}<w:sz w:val='22'/></w:rPr><w:t>A</w:t></w:r></w:p>");
    docx::package_with_defaults(&p.repeat(n), "")
}
// 이것을 실패시키는 것: 의미 경고로 import를 거부하거나 typed context/count 전달을 누락.
#[test]
fn imported_mark_loss_returns_bytes_and_stable_typed_metadata() {
    for count in [1, 2] {
        let out = convert_docx(&source(count)).unwrap();
        let info = out
            .meta()
            .warnings
            .into_iter()
            .find(|w| w.code == "PARAGRAPH_END_MARK_REPLACED")
            .unwrap();
        let value = serde_json::to_value(&info).unwrap();
        let mut expected = serde_json::json!({"format":"hwpx","stage":"encode","location":"section[0].para[0]","loss":"paragraph_end_mark_formatting","action":"replace_with_text_formatting"});
        if count == 2 {
            expected["count"] = 2.into();
        }
        assert_eq!(value["context"], expected);
        assert_eq!(value["message"],format!("section[0].para[0]: Imported paragraph-end formatting was replaced with the final text run's formatting to avoid increasing line height. The original paragraph-end formatting is not recoverable from this HWPX output. Affected paragraphs: {count}. Only the first location is shown."));
        let decoded = HwpxDecoder::decode(&out.bytes).unwrap();
        for p in &decoded.document.sections()[0].paragraphs {
            let id = p.end_char_shape.unwrap();
            assert_eq!(decoded.style_store.char_shape(id).unwrap().height.as_i32(), 1100);
        }
        let mut z = zip::ZipArchive::new(Cursor::new(&out.bytes)).unwrap();
        let mut text = String::new();
        z.by_name("Contents/section0.xml").unwrap().read_to_string(&mut text).unwrap();
        let t = xml::Element::parse(&text);
        assert_eq!(t.descendants("hp:p").len(), count);
    }
}
// 이것을 실패시키는 것: 표시 개수가 1이면 count를 출력하거나 2이면 생략, 경고 context를 메시지에서 추측.
#[test]
fn typed_mark_warning_uses_existing_context_omission_rules() {
    for count in [1, 2] {
        let info = OpsWarning::Encode(EncodeWarning::ParagraphEndMarkReplaced {
            path: ParagraphPath(vec![PathSeg::Section(0), PathSeg::BodyParagraph(3)]),
            count,
        })
        .info();
        let v = serde_json::to_value(info).unwrap();
        assert_eq!(v["context"]["loss"], "paragraph_end_mark_formatting");
        assert_eq!(v["context"]["location"], "section[0].para[3]");
        if count == 1 {
            assert!(v["context"].get("count").is_none());
        } else {
            assert_eq!(v["context"]["count"], 2);
        }
    }
}

// 이것을 실패시키는 것: 번호·글머리표 문단 끝 표시를 치환하거나 표지 원본 서식을 바꾸거나 백분율 보정 적용.
#[test]
fn list_paragraph_retains_end_mark_and_marker_definitions() {
    use hwpforge::core::ImageStore;
    use hwpforge::hwpx::{HwpxEncoder, HwpxRegistryBridge};
    for (format, text, definition) in
        [("decimal", "%1.", "hh:numbering"), ("bullet", "•", "hh:bullet")]
    {
        let font = "<w:rFonts w:ascii='Cambria' w:hAnsi='Cambria' w:eastAsia='Cambria'/>";
        let p=format!("<w:p><w:pPr><w:numPr><w:numId w:val='7'/></w:numPr><w:rPr>{font}<w:sz w:val='40'/><w:b/></w:rPr></w:pPr><w:r><w:rPr>{font}<w:sz w:val='22'/></w:rPr><w:t>A</w:t></w:r></w:p>");
        let marker_fonts = if format == "bullet" {
            "<w:rPr><w:rFonts w:ascii='Arial' w:hAnsi='Arial'/></w:rPr>"
        } else {
            ""
        };
        let defs=format!("<w:abstractNum w:abstractNumId='1'><w:lvl w:ilvl='0'><w:start w:val='1'/><w:numFmt w:val='{format}'/><w:lvlText w:val='{text}'/>{marker_fonts}</w:lvl></w:abstractNum><w:num w:numId='7'><w:abstractNumId w:val='1'/></w:num>");
        let input = docx::package_with_numbering(&p, &defs);
        let source = hwpforge::docx::decode_docx(&input).unwrap();
        let original = source.document.sections()[0].paragraphs[0].end_char_shape.unwrap();
        assert_eq!(source.registry.char_shapes[original.get()].size.as_i32(), 2000);
        let out = convert_docx(&input).unwrap();
        assert!(
            out.meta().warnings.iter().all(|w| w.code != "PARAGRAPH_END_MARK_REPLACED"),
            "{format}: {:?}",
            out.meta().warnings
        );
        let bridge =
            HwpxRegistryBridge::from_registry_for_import(&source.registry, Default::default())
                .unwrap()
                .value;
        let doc = bridge.rebind_draft_document(source.document).unwrap().validate().unwrap();
        let store = bridge.into_style_store();
        let plain = serde_json::from_value(serde_json::to_value(&store).unwrap()).unwrap();
        let old = HwpxEncoder::encode(&doc, &plain, &ImageStore::new()).unwrap();
        let tree = |bytes: &[u8], part: &str| {
            let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
            let mut s = String::new();
            z.by_name(part).unwrap().read_to_string(&mut s).unwrap();
            xml::Element::parse(&s)
        };
        let before = tree(&old, "Contents/header.xml");
        let after = tree(&out.bytes, "Contents/header.xml");
        let section = tree(&out.bytes, "Contents/section0.xml");
        let last = section.descendants("hp:p")[0]
            .children
            .iter()
            .rev()
            .find(|r| r.name == "hp:run")
            .unwrap();
        let end_id = last.attr("charPrIDRef").unwrap();
        let end_shape = after
            .descendants("hh:charPr")
            .into_iter()
            .find(|c| c.attr("id") == Some(end_id))
            .unwrap();
        assert_eq!(end_shape.attr("height"), Some("2000"));
        assert_eq!(end_shape.descendants("hh:bold").len(), 1);
        let para_id = section.descendants("hp:p")[0].attr("paraPrIDRef").unwrap();
        let para = after
            .descendants("hh:paraPr")
            .into_iter()
            .find(|p| p.attr("id") == Some(para_id))
            .unwrap();
        let heading = para.descendants("hh:heading")[0];
        let numbering = after
            .descendants(definition)
            .into_iter()
            .find(|n| n.attr("id") == heading.attr("idRef"))
            .unwrap();
        let marker = numbering.descendants("hh:paraHead")[0];
        let marker_id = marker.attr("charPrIDRef").unwrap();
        if format == "bullet" {
            assert_eq!(marker_id, "4294967295");
        } else {
            let shape = after
                .descendants("hh:charPr")
                .into_iter()
                .find(|c| c.attr("id") == Some(marker_id))
                .unwrap();
            assert_eq!(shape.attr("height"), Some("2000"));
            assert_eq!(shape.descendants("hh:bold").len(), 1);
        }
        assert_eq!(before.descendants(definition), after.descendants(definition));
        assert_eq!(before.descendants("hh:charPr"), after.descendants("hh:charPr"));
        let before = tree(&old, "Contents/section0.xml");
        let after = tree(&out.bytes, "Contents/section0.xml");
        assert_eq!(
            before.descendants("hp:p")[0].attr("paraPrIDRef"),
            after.descendants("hp:p")[0].attr("paraPrIDRef")
        );
    }
}
