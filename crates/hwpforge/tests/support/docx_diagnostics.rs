//! Real producer samples for foreign non-exhaustive diagnostic variants.
use hwpforge::docx::{decode_docx, DocxError, DocxWarning};
use hwpforge::ops::{OpsError, OpsWarning};
use std::io::{Cursor, Read, Write};

#[path = "docx.rs"]
mod docx;

fn rewrite(bytes: &[u8], change: impl FnOnce(&mut Vec<(String, String)>)) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut entries = Vec::new();
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).unwrap();
        let mut text = String::new();
        file.read_to_string(&mut text).unwrap();
        entries.push((file.name().unwrap().into_owned(), text));
    }
    change(&mut entries);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, text) in entries {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(text.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

pub fn errors() -> Vec<(&'static str, OpsError)> {
    let base = docx::package("<w:p/>");
    let inputs = [
        ("MissingPart", rewrite(&base, |e| e.retain(|(name, _)| name != "body.xml"))),
        (
            "AmbiguousPart",
            rewrite(&base, |e| {
                e[0].1 = e[0].1.replace("</Types>", "<Override PartName='/mimetype' ContentType='application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml'/></Types>");
                e[1].1 = e[1].1.replace("body.xml", "mimetype");
                e[2].0 = "mimetype".into();
                e.push(("MIMETYPE".into(), "other".into()));
            }),
        ),
        ("OpaquePart", rewrite(&base, |e| e.push(("body.xml/".into(), String::new())))),
        (
            "MissingContentType",
            rewrite(&base, |e| e[0].1 = e[0].1.replace("Extension='xml'", "Extension='unused'")),
        ),
        (
            "ContentTypeMismatch",
            rewrite(&base, |e| {
                e[0].1 = e[0].1.replace("application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml", "text/xml")
            }),
        ),
        (
            "UnsupportedEncoding",
            rewrite(&base, |e| {
                e[2].1 = format!("<?xml version='1.0' encoding='UTF-16'?>{}", e[2].1)
            }),
        ),
        ("Invalid", docx::package("<w:r/>")),
        (
            "Limit",
            docx::package(&format!(
                "{}<w:p/>{}",
                "<w:smartTag>".repeat(130),
                "</w:smartTag>".repeat(130)
            )),
        ),
        ("Package", b"not a ZIP".to_vec()),
        ("TrackedChange", docx::package("<w:p><w:ins/></w:p>")),
    ];
    inputs
        .into_iter()
        .map(|(name, bytes)| {
            let err = decode_docx(&bytes).unwrap_err();
            let actual = match &err {
                DocxError::MissingPart { .. } => "MissingPart",
                DocxError::AmbiguousPart { .. } => "AmbiguousPart",
                DocxError::OpaquePart { .. } => "OpaquePart",
                DocxError::MissingContentType { .. } => "MissingContentType",
                DocxError::ContentTypeMismatch { .. } => "ContentTypeMismatch",
                DocxError::UnsupportedEncoding { .. } => "UnsupportedEncoding",
                DocxError::Invalid { .. } => "Invalid",
                DocxError::Limit { .. } => "Limit",
                DocxError::Package { .. } => "Package",
                DocxError::TrackedChange { .. } => "TrackedChange",
                _ => panic!("unregistered DOCX error: {err}"),
            };
            assert_eq!(actual, name, "{err}");
            (name, OpsError::DocxDecode(err))
        })
        .collect()
}

pub fn warnings() -> Vec<(&'static str, OpsWarning)> {
    let body = "<w:p><w:pPr><w:pStyle w:val='missing'/><w:numPr/><w:sectPr><w:type w:val='continuous'/></w:sectPr></w:pPr><w:r><w:rPr><w:caps/><w:sz w:val='bad'/><w:szCs w:val='24'/><w:vanish/></w:rPr><w:t>X</w:t></w:r><w:smartTag><w:r><w:t> A </w:t><w:unknown/><w:br w:type='page'/><w:br w:type='column'/><w:br w:type='odd'/><w:t w:other='x'>B</w:t></w:r></w:smartTag><w:fldSimple w:instr='PAGE'/><w:fldSimple w:instr='OUTER'><w:fldSimple w:instr='INNER'><w:r><w:t>7</w:t></w:r></w:fldSimple></w:fldSimple><w:hyperlink><w:r><w:t>L</w:t></w:r></w:hyperlink><w:sdt><w:sdtContent><w:r><w:t>S</w:t></w:r></w:sdtContent></w:sdt><mc:AlternateContent xmlns:mc='http://schemas.openxmlformats.org/markup-compatibility/2006'><mc:Fallback><w:r><w:t>F</w:t></w:r></mc:Fallback></mc:AlternateContent></w:p>";
    let bytes = rewrite(&docx::package(body), |e| {
        e[0].1 = e[0].1.replace("</Types>", "<Override PartName='/settings.xml' ContentType='application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml'/></Types>");
        e.push(("_rels/body.xml.rels".into(), "<Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'><Relationship Id='s' Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings' Target='settings.xml'/><Relationship Id='h' Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/header' Target='missing.xml'/></Relationships>".into()));
        e.push(("settings.xml".into(), "<w:settings xmlns:w='http://schemas.openxmlformats.org/wordprocessingml/2006/main'><w:zoom w:percent='100'/></w:settings>".into()));
    });
    let mut seen = std::collections::BTreeSet::new();
    let lists = docx::package_with_numbering(
        "<w:p><w:pPr><w:numPr><w:numId w:val='1'/></w:numPr></w:pPr></w:p><w:p><w:pPr><w:numPr><w:numId w:val='2'/></w:numPr></w:pPr></w:p><w:p><w:pPr><w:numPr><w:numId w:val='3'/></w:numPr></w:pPr></w:p><w:p><w:pPr><w:numPr><w:numId w:val='1'/></w:numPr><w:outlineLvl w:val='0'/></w:pPr></w:p>",
        "<w:abstractNum w:abstractNumId='1'><w:lvl w:ilvl='0'><w:numFmt w:val='ganada'/><w:rPr><w:b/></w:rPr></w:lvl></w:abstractNum><w:abstractNum w:abstractNumId='2'><w:lvl w:ilvl='0'><w:numFmt w:val='bullet'/><w:lvlText w:val='&#xF0B7;'/><w:rPr><w:rFonts w:ascii='Symbol'/></w:rPr></w:lvl></w:abstractNum><w:abstractNum w:abstractNumId='3'><w:lvl w:ilvl='0'><w:numFmt w:val='bullet'/><w:lvlText w:val='&#xF06C;'/></w:lvl></w:abstractNum><w:num w:numId='1'><w:abstractNumId w:val='1'/></w:num><w:num w:numId='2'><w:abstractNumId w:val='2'/></w:num><w:num w:numId='3'><w:abstractNumId w:val='3'/></w:num>",
    );
    // 이것을 실패시키는 것: 실제 표 손실 입력 또는 variant 이름 연결을 제거.
    let tables = docx::package("<w:tbl><w:tblPr><w:tblBorders><w:top w:val='double'/></w:tblBorders></w:tblPr><w:tr><w:tc><w:tcPr><w:vMerge/><w:shd w:fill='FF0000'/></w:tcPr><w:p/></w:tc></w:tr></w:tbl>");
    decode_docx(&bytes)
        .unwrap()
        .warnings
        .into_iter()
        .chain(decode_docx(&lists).unwrap().warnings)
        .chain(decode_docx(&tables).unwrap().warnings)
        .map(|w| {
            let name = match &w {
                DocxWarning::ContentLost { .. } => "ContentLost",
                DocxWarning::TablePositionApproximated { .. } => "TablePositionApproximated",
                DocxWarning::TableWidthApproximated { .. } => "TableWidthApproximated",
                DocxWarning::TableBorderApproximated { .. } => "TableBorderApproximated",
                DocxWarning::TableMergeApproximated { .. } => "TableMergeApproximated",
                DocxWarning::ListReferenceLost { .. } => "ListReferenceLost",
                DocxWarning::ListNumberingLost { .. } => "ListNumberingLost",
                DocxWarning::BulletGlyphApproximated { .. } => "BulletGlyphApproximated",
                DocxWarning::BulletGlyphLost { .. } => "BulletGlyphLost",
                DocxWarning::HeadingNumberLost { .. } => "HeadingNumberLost",
                DocxWarning::UnsupportedAttribute { .. } => "UnsupportedAttribute",
                DocxWarning::UnsupportedValue { .. } => "UnsupportedValue",
                DocxWarning::StyleReferenceLost { .. } => "StyleReferenceLost",
                DocxWarning::FormatDefaulted { .. } => "FormatDefaulted",
                DocxWarning::ScriptFormatCollapsed { .. } => "ScriptFormatCollapsed",
                DocxWarning::HiddenTextExposed { .. } => "HiddenTextExposed",
                DocxWarning::DocumentSettingsOmitted { .. } => "DocumentSettingsOmitted",
                DocxWarning::StructuredWrapperFlattened { .. } => "StructuredWrapperFlattened",
                DocxWarning::FormattingOmitted { .. } => "FormattingOmitted",
                DocxWarning::ContentOmitted { .. } => "ContentOmitted",
                DocxWarning::WhitespaceApproximated { .. } => "WhitespaceApproximated",
                DocxWarning::FieldInstructionOmitted { .. } => "FieldInstructionOmitted",
                DocxWarning::FieldResultMissing { .. } => "FieldResultMissing",
                DocxWarning::NestedFieldFlattened { .. } => "NestedFieldFlattened",
                DocxWarning::LinkTargetOmitted { .. } => "LinkTargetOmitted",
                DocxWarning::ContentControlFlattened { .. } => "ContentControlFlattened",
                DocxWarning::AlternateContentSelected { .. } => "AlternateContentSelected",
                DocxWarning::OptionalPartOmitted { .. } => "OptionalPartOmitted",
                DocxWarning::ParagraphSplitApproximated { .. } => "ParagraphSplitApproximated",
                DocxWarning::ColumnBreakApproximated { .. } => "ColumnBreakApproximated",
                DocxWarning::PageLayoutApproximated { .. } => "PageLayoutApproximated",
                DocxWarning::AttributeOmitted { .. } => "AttributeOmitted",
                DocxWarning::ValueApproximated { .. } => "ValueApproximated",
                _ => panic!("unregistered DOCX warning: {w:?}"),
            };
            (name, OpsWarning::Docx(w))
        })
        .filter(|(name, _)| seen.insert(*name))
        .collect()
}
