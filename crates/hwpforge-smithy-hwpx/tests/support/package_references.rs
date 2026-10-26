use super::*;

pub(super) fn part(bytes: &[u8], name: &str) -> String {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip");
    let mut text = String::new();
    zip.by_name(name).expect("part").read_to_string(&mut text).expect("utf-8");
    text
}

/// `opf:item@id` values of a `content.hpf`, read with an XML reader.
pub(super) fn written_ids(hpf: &str) -> Vec<String> {
    let mut reader = quick_xml::Reader::from_str(hpf);
    let mut ids = Vec::new();
    loop {
        match reader.read_event().expect("well-formed content.hpf") {
            quick_xml::events::Event::Empty(e) | quick_xml::events::Event::Start(e)
                if e.name().as_ref() == "opf:item" =>
            {
                for attr in e.attributes() {
                    let attr = attr.expect("attribute");
                    if attr.key.as_ref() == "id" {
                        ids.push(
                            attr.normalized_value(quick_xml::XmlVersion::Explicit1_0)
                                .expect("value")
                                .into_owned(),
                        );
                    }
                }
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
    }
    ids
}

/// Decodes `bytes` and encodes the result again with `images`.
pub(super) fn reencode(bytes: &[u8], images: Option<&ImageStore>) -> EncodeOutcome {
    let decoded = HwpxDecoder::decode(bytes).expect("decode");
    HwpxEncoder::encode_with_diagnostics(
        &decoded.document.validate().expect("validate"),
        &decoded.style_store,
        images.unwrap_or(&decoded.image_store),
        EncodeOptions::default(),
    )
    .expect("encode")
}

pub(super) fn unresolved(warnings: &[EncodeWarning]) -> Vec<(String, String, String)> {
    warnings
        .iter()
        .filter_map(|w| match w {
            EncodeWarning::ImageReferenceUnresolved { part, location, reference } => {
                Some((part.clone(), location.clone(), reference.clone()))
            }
            _ => None,
        })
        .collect()
}

pub(super) fn triple(part: &str, location: &str, reference: &str) -> (String, String, String) {
    (part.into(), location.into(), reference.into())
}
