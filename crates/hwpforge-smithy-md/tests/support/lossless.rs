#![allow(dead_code)]

use hwpforge_core::{Document, Run};
use hwpforge_smithy_md::{MdDecoder, MdEncoder, MdError};
#[path = "document.rs"]
pub(super) mod document;
use document::{doc, text};

const SECTION: &str = r#"<section data-index="0" data-width-mm="210.0" data-height-mm="297.0"><p data-para-shape="0">"#;

pub(crate) fn encode(runs: Vec<Run>) -> Result<String, MdError> {
    MdEncoder::encode_lossless(&doc(runs).validate().unwrap())
}

pub(crate) fn round_trip(run: Run) -> Run {
    let md = encode(vec![run]).unwrap();
    MdDecoder::decode_lossless(&md).unwrap().sections()[0].paragraphs[0].runs[0].clone()
}

pub(crate) fn decode_body(body: &str) -> Result<Document, MdError> {
    let md = encode(vec![text("t")]).unwrap();
    let frontmatter = &md[..md.find("<section").unwrap()];
    MdDecoder::decode_lossless(&format!("{frontmatter}{SECTION}{body}</p></section>\n"))
}
