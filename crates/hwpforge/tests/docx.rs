#![cfg(feature = "docx")]
use std::io::{Cursor, Write};

// 이것을 실패시키는 것: facade의 DOCX 진입점을 실패/다른 결과를 내는 함수로 연결.
#[test]
fn facade_decodes_text_without_other_codec_features() {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, xml) in [
        ("[Content_Types].xml", "<Types xmlns='http://schemas.openxmlformats.org/package/2006/content-types'><Default Extension='rels' ContentType='application/vnd.openxmlformats-package.relationships+xml'/><Default Extension='xml' ContentType='application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml'/></Types>"),
        ("_rels/.rels", "<Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'><Relationship Id='main' Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument' Target='body.xml'/></Relationships>"),
        ("body.xml", "<w:document xmlns:w='http://schemas.openxmlformats.org/wordprocessingml/2006/main'><w:body><w:p><w:r><w:t>A</w:t></w:r></w:p></w:body></w:document>"),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
    }
    let bytes = zip.finish().unwrap().into_inner();
    let decoded: hwpforge::docx::DocxDecoded = hwpforge::docx::decode_docx(&bytes).unwrap();
    assert_eq!(decoded.document.sections()[0].paragraphs[0].text_content(), "A");
    assert!(decoded.warnings.is_empty());
}

// 이것을 실패시키는 것: facade 오류 재노출 변경, variant 필드 매칭에서 `..` 제거.
// 각 variant의 non_exhaustive 제거는 원본 타입의 compile_fail 계약을 실패시킨다.
#[test]
fn facade_error_variants_allow_forward_compatible_matching() {
    use hwpforge::docx::DocxError;
    fn classify(error: &DocxError) -> &str {
        match error {
            DocxError::MissingPart { part, .. } => part,
            DocxError::AmbiguousPart { part, .. } => part,
            DocxError::OpaquePart { part, .. } => part,
            DocxError::MissingContentType { part, .. } => part,
            DocxError::ContentTypeMismatch { actual, .. } => actual,
            DocxError::UnsupportedEncoding { encoding, .. } => encoding,
            DocxError::Invalid { detail, .. } => detail,
            DocxError::Limit { resource, .. } => resource,
            DocxError::Package { part, .. } => part,
            DocxError::TrackedChange { element, .. } => element,
            _ => "future",
        }
    }
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("unrelated", zip::write::SimpleFileOptions::default()).unwrap();
    zip.write_all(b"x").unwrap();
    let error = hwpforge::docx::decode_docx(&zip.finish().unwrap().into_inner()).unwrap_err();
    assert_eq!(classify(&error), "[Content_Types].xml");
}
