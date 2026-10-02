//! Issue #199: a multi-line 글맵시 (TextArt) keeps its line breaks.
//!
//! Hancom writes each line break in `<hp:textart text="…">` as the visible
//! pair `␍␊` (U+240D U+240A). A raw control character in an attribute value
//! would be turned into a space by XML attribute-value normalization. The fixture was saved by Hancom (macOS) with two TextArts:
//! `첫 줄⏎둘쨰 줄⏎셋째 줄` and `위⏎⏎아래` (the typo is in the source file).
//!
//! Core carries the break as `\r\n`, the form the HWP5 record uses.

use hwpforge_core::control::Control;
use hwpforge_core::run::RunContent;
use hwpforge_core::Document;
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder, PackageReader};

const FIXTURE: &str = "../../tests/fixtures/shapes/textart_multiline.hwpx";

/// The two `text` attributes exactly as Hancom wrote them.
const HANCOM_WIRE: [&str; 2] = ["첫 줄␍␊둘쨰 줄␍␊셋째 줄", "위␍␊␍␊아래"];

fn fixture_bytes() -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE);
    std::fs::read(&path).unwrap_or_else(|e| panic!("fixture {} must exist: {e}", path.display()))
}

/// Every `text` attribute of `<hp:textart>` in document order, as written.
fn textart_wire_texts(section_xml: &str) -> Vec<String> {
    section_xml
        .match_indices("<hp:textart ")
        .map(|(start, _)| {
            let tag = &section_xml[start..];
            let tag = &tag[..tag.find('>').expect("textart tag closes")];
            let value = tag.split(" text=\"").nth(1).expect("textart has a text attribute");
            value[..value.find('"').expect("text attribute closes")].to_string()
        })
        .collect()
}

fn core_textart_texts<S>(doc: &Document<S>) -> Vec<String> {
    doc.sections()
        .iter()
        .flat_map(|s| s.paragraphs.iter())
        .flat_map(|p| p.runs.iter())
        .filter_map(|r| match &r.content {
            RunContent::Control(c) => match c.as_ref() {
                Control::TextArt { text, .. } => Some(text.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

fn section0(bytes: &[u8]) -> String {
    PackageReader::new(bytes).expect("package").read_section_xml(0).expect("section0.xml")
}

/// 이것을 실패시키는 것: 디코더가 `␍␊` 를 `\r\n` 으로 풀지 않음(기호가 Core 에 그대로 남음).
#[test]
fn hancom_textart_line_breaks_decode_to_crlf() {
    // Guard the oracle itself: the fixture really carries the Hancom form.
    assert_eq!(textart_wire_texts(&section0(&fixture_bytes())), HANCOM_WIRE);

    let decoded = HwpxDecoder::decode(&fixture_bytes()).expect("decode");
    assert_eq!(
        core_textart_texts(&decoded.document),
        ["첫 줄\r\n둘쨰 줄\r\n셋째 줄", "위\r\n\r\n아래"]
    );
}

/// 이것을 실패시키는 것: 인코더가 `\r\n` 을 속성에 그대로 씀(다시 읽으면 공백), 또는
/// 쌍마다 `␍␊` 를 두 번 씀.
#[test]
fn hancom_textart_reencodes_to_the_hancom_wire_form() {
    let decoded = HwpxDecoder::decode(&fixture_bytes()).expect("decode");
    let original = core_textart_texts(&decoded.document);
    let validated = decoded.document.validate().expect("validate");
    let bytes = HwpxEncoder::encode(&validated, &decoded.style_store, &decoded.image_store)
        .expect("encode");

    assert_eq!(textart_wire_texts(&section0(&bytes)), HANCOM_WIRE);

    let again = HwpxDecoder::decode(&bytes).expect("re-decode");
    assert_eq!(
        core_textart_texts(&again.document),
        original,
        "HWPX → Core → HWPX → Core keeps every line break"
    );
}
