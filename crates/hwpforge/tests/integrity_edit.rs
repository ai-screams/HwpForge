//! Synthetic package integrity markers must never produce edited bytes.
#![cfg(feature = "ops-hwpx")]
#[path = "support/document.rs"]
mod document;
#[path = "../../hwpforge-smithy-hwpx/tests/support/integrity.rs"]
mod integrity_cases;
#[path = "../../hwpforge-smithy-hwpx/tests/support/package.rs"]
mod package;

use hwpforge::core::{Control, Run};
use hwpforge::foundation::{CharShapeIndex, FieldType};
use hwpforge::hwpx::stamp::{StampAction, StampMap, StampRequestV2, StampSpec};
use hwpforge::hwpx::{
    HwpxDecoder, HwpxEncoder, HwpxStructuralEditor, InsertPosition, ParagraphLocator,
};
use hwpforge::ops::{edit, exchange, inspect, stamp, OpsError};
use std::io::{Cursor, Read, Write};
use zip::unstable::write::FileOptionsExt;

fn check_namespace_edit(editor: &str) {
    let base = base();
    let mut failures = Vec::new();
    for (cases, refused) in [(integrity_cases::REFUSED, true), (integrity_cases::EDITABLE, false)] {
        for xml in cases {
            let bytes = append(&base, "META-INF/manifest.xml", xml.as_bytes(), false);
            // Read paths keep their existing namespace interpretation.
            inspect::inspect(&bytes, &Default::default()).unwrap();
            let result = match editor {
                "restyle" => hwpforge::ops::style::restyle(
                    &bytes,
                    &hwpforge::ops::style::RestyleOptions::default().with_preset("modern"),
                )
                .map(|o| o.bytes),
                "patch" => {
                    let request = Requests::new(&bytes);
                    exchange::patch(
                        &bytes,
                        &exchange::PatchOptions::default().with_patch(request.patch),
                    )
                    .map(|o| o.bytes)
                }
                "structural" => edit::insert_para(
                    &bytes,
                    &edit::InsertParaOptions::default()
                        .with_anchor(2)
                        .with_text(vec!["inserted".into()]),
                )
                .map(|o| o.bytes),
                _ => unreachable!(),
            };
            match result {
                Err(error)
                    if refused
                        && error.code().as_str() == "DECODE_FAILED"
                        && error.to_string().contains("package integrity") => {}
                Ok(_) if !refused => {}
                result => {
                    let observed = result
                        .map(|bytes| format!("{} bytes", bytes.len()))
                        .map_err(|e| e.to_string());
                    failures.push(format!(
                        "{editor}: {xml}: expected refused={refused}, got {observed:?}"
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// 이것을 실패시키는 것: namespace 정규화 제거/실패 허용/이중 정규화.
#[test]
fn integrity_namespace_restyle() {
    check_namespace_edit("restyle");
}

// 이것을 실패시키는 것: namespace 정규화 제거/실패 허용/이중 정규화.
#[test]
fn integrity_namespace_patch() {
    check_namespace_edit("patch");
}

// 이것을 실패시키는 것: namespace 정규화 제거/실패 허용/이중 정규화.
#[test]
fn integrity_namespace_structural() {
    check_namespace_edit("structural");
}

fn base() -> Vec<u8> {
    let mut d = HwpxDecoder::decode(&document::editable()).unwrap();
    let p = &mut d.document.sections_mut()[0].paragraphs;
    p.push(document::text_para("removable"));
    p.push(document::run_para(Run::control(
        Control::Field {
            field_type: FieldType::ClickHere,
            name: Some("existing".into()),
            hint_text: Some("hint".into()),
            help_text: None,
            display_text: "hint".into(),
        },
        CharShapeIndex::new(0),
    )));
    HwpxEncoder::encode(&d.document.validate().unwrap(), &d.style_store, &d.image_store).unwrap()
}

fn append(base: &[u8], path: &str, body: &[u8], encrypted: bool) -> Vec<u8> {
    let mut input = zip::ZipArchive::new(Cursor::new(base)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..input.len() {
        let mut entry = input.by_index(i).unwrap();
        let name = entry.name().unwrap().to_string();
        if name.eq_ignore_ascii_case(path) {
            continue;
        }
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        out.start_file(
            name,
            zip::write::SimpleFileOptions::default().compression_method(entry.compression()),
        )
        .unwrap();
        out.write_all(&data).unwrap();
    }
    let options = zip::write::SimpleFileOptions::default();
    let options =
        if encrypted { options.with_deprecated_encryption(b"secret").unwrap() } else { options };
    out.start_file(path, options).unwrap();
    out.write_all(body).unwrap();
    out.finish().unwrap().into_inner()
}

fn legacy(json: &str) -> String {
    let mut value: serde_json::Value = serde_json::from_str(json).unwrap();
    value.as_object_mut().unwrap().remove("format_version");
    for para in value["styles"]["para_shapes"].as_array_mut().unwrap() {
        for key in ["margin_left", "margin_right", "indent", "spacing_before", "spacing_after"] {
            para[key] = 0.into();
        }
    }
    fn refs(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(r) = map.remove("style_ref") {
                    map.insert("style_id".into(), r);
                }
                for child in map.values_mut() {
                    refs(child);
                }
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(refs),
            _ => {}
        }
    }
    refs(&mut value);
    value.to_string()
}

// Export requests before damaging the input: decode-error tests must reach the editors.
struct Requests {
    json: String,
    legacy_json: String,
    patch: String,
    legacy_patch: String,
    text: Vec<StampSpec>,
}
impl Requests {
    fn new(base: &[u8]) -> Self {
        let json = exchange::to_json(base, &Default::default()).unwrap().document.to_string();
        let mut exported = exchange::export_section(base, &Default::default()).unwrap().exported;
        exported.section.paragraphs[2].runs[0] = Run::text("patched", CharShapeIndex::new(0));
        let patch = serde_json::to_string(&exported).unwrap();
        let text = stamp::stamp_plan(base)
            .unwrap()
            .plan
            .text
            .into_iter()
            .map(|c| StampSpec {
                section: c.section,
                path: c.path,
                span: c.span,
                marker: c.marker,
                action: StampAction::Field { name: "new_field".into(), hint: None },
            })
            .collect();
        Self { legacy_json: legacy(&json), legacy_patch: legacy(&patch), json, patch, text }
    }
    fn edits(&self, bytes: &[u8]) -> Vec<(&'static str, Result<Vec<u8>, OpsError>)> {
        // Hash a valid request for these exact bytes, including corrupt decode inputs.
        let mut request = StampRequestV2 {
            schema_version: 2,
            source_sha256: String::new(),
            text: self.text.clone(),
            cells: vec![hwpforge::hwpx::stamp::CellStampSpec {
                table: 0,
                at: hwpforge::core::table::grid::GridCoord::new(0, 1),
                label: None,
                action: hwpforge::hwpx::stamp::CellStampAction::Ignore,
            }],
        };
        let hwpforge::hwpx::stamp::StamperError::SourceHashMismatch { actual: hash, .. } =
            hwpforge::hwpx::stamp::HwpxStamper::stamp_v2(bytes, &request).unwrap_err()
        else {
            panic!("hash preflight")
        };
        request.source_sha256 = hash;
        vec![
            (
                "insert",
                edit::insert_para(
                    bytes,
                    &edit::InsertParaOptions::default()
                        .with_anchor(2)
                        .with_text(vec!["inserted".into()]),
                )
                .map(|o| o.bytes),
            ),
            (
                "delete",
                edit::delete_para(bytes, &edit::DeleteParaOptions::default().with_indexes(vec![2]))
                    .map(|o| o.bytes),
            ),
            (
                "set-cell",
                edit::set_cell(
                    bytes,
                    &edit::SetCellOptions::default().with_table(0).with_at("0,1").with_text("name"),
                )
                .map(|o| o.bytes),
            ),
            (
                "stamp",
                stamp::stamp(bytes, &StampMap::Legacy(self.text.clone()), &Default::default())
                    .map(|o| o.bytes),
            ),
            (
                "stamp-v2",
                stamp::stamp(bytes, &StampMap::V2(request), &Default::default()).map(|o| o.bytes),
            ),
            (
                "patch",
                exchange::patch(
                    bytes,
                    &exchange::PatchOptions::default().with_patch(self.patch.clone()),
                )
                .map(|o| o.bytes),
            ),
            (
                "fill",
                edit::fill(bytes, &[("existing".into(), "filled".into())], &Default::default())
                    .map(|o| o.bytes),
            ),
            (
                "fill-noop",
                edit::fill(bytes, &[("existing".into(), "hint".into())], &Default::default())
                    .map(|o| o.bytes),
            ),
            (
                "fill-empty-library",
                hwpforge::hwpx::HwpxFiller::fill(bytes, &Default::default())
                    .map(|o| o.bytes)
                    .map_err(OpsError::from),
            ),
            (
                "patch-legacy",
                exchange::patch(
                    bytes,
                    &exchange::PatchOptions::default().with_patch(self.legacy_patch.clone()),
                )
                .map(|o| o.bytes),
            ),
            (
                "from-json-legacy",
                exchange::from_json(
                    &self.legacy_json,
                    &exchange::FromJsonOptions::default().with_base(bytes),
                )
                .map(|o| o.bytes),
            ),
            (
                "from-json",
                exchange::from_json(
                    &self.json,
                    &exchange::FromJsonOptions::default().with_base(bytes),
                )
                .map(|o| o.bytes),
            ),
        ]
    }
}

fn markers(base: &[u8]) -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("encryption path", append(base, "META-INF/encryption.xml", b"", false)),
        ("opf encryption", package::splice_part(base, "Contents/content.hpf", "<opf:item ", "<opf:item encryption=\"true\" ")),
        ("path", append(base, "META-INF/signatures.xml", b"", false)),
        ("mixed-case path", append(base, "meta-inf/Signatures.XML", b"", false)),
        ("mixed-case manifest", append(base, "mEtA-InF/MaNiFeSt.XmL", b"<encryption-data xmlns='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/>", false)),
        ("numbered path", append(base, "META-INF/signatures1.xml", b"not XML", false)),
        ("manifest signature", package::splice_part(base, "META-INF/manifest.xml", "/>", "><s:Signature xmlns:s=\"http://www.w3.org/2000/09/xmldsig#\"/></odf:manifest>")),
        ("container signature", package::splice_part(base, "META-INF/container.xml", "</ocf:container>", "<x:signatures xmlns:x=\"urn:oasis:names:tc:opendocument:xmlns:container\"/></ocf:container>")),
        ("content signature reference", package::splice_part(base, "Contents/content.hpf", "</opf:manifest>", "<opf:item id=\"sig\" href=\"META-INF/signatures.xml\" media-type=\"application/xml\"/></opf:manifest>")),
        ("manifest encryption", package::splice_part(base, "META-INF/manifest.xml", "/>", "><e:encryption-data xmlns:e=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\"/></odf:manifest>")),
        ("capital manifest", append(base, "META-INF/Manifest.xml", b"<manifest xmlns='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'><encryption-data/></manifest>", false)),
    ]
}

// 이것을 실패시키는 것: 경로/namespace/descriptor 참조 또는 각 편집기 검사 삭제(빈/동일 값 fill 포함).
#[test]
fn integrity_markers_reject_every_edit_but_keep_readers() {
    let base = base();
    for (case, bytes) in markers(&base) {
        let requests = Requests::new(&bytes);
        inspect::inspect(&bytes, &Default::default()).unwrap();
        #[cfg(feature = "ops-md")]
        hwpforge::ops::markdown::to_md(&bytes, &Default::default()).unwrap();
        for (editor, result) in requests.edits(&bytes) {
            let error = result.expect_err(&format!("{case}: {editor} returned bytes"));
            assert_eq!(error.code().as_str(), "DECODE_FAILED", "{case}: {editor}: {error}");
            assert!(error.to_string().contains("package integrity"), "{case}: {editor}: {error}");
        }
    }
}

// 이것을 실패시키는 것: 정상 manifest까지 거부하거나 namespace를 local name만으로 판정.
#[test]
fn integrity_normal_and_unrelated_names_remain_editable() {
    let base = base();
    let lookalike = package::splice_part(&base, "META-INF/manifest.xml", "/>", "><x:encryption-data xmlns:x=\"urn:unrelated\"/><x:Signature xmlns:x=\"urn:unrelated\"/><!-- encryption-data signatures.xml --></odf:manifest>");
    for bytes in [base, lookalike] {
        for (editor, result) in Requests::new(&bytes).edits(&bytes) {
            result.unwrap_or_else(|e| panic!("{editor}: {e}"));
        }
    }
}

// 이것을 실패시키는 것: 암호화 entry의 제한 읽기 우회 또는 decode 앞 무결성 검사 이동.
#[test]
fn integrity_decode_and_zip_encryption_errors_win() {
    let base = base();
    let requests = Requests::new(&base);
    let encrypted = append(&base, "private.bin", b"locked", true);
    let signed = append(&base, "META-INF/signatures.xml", b"", false);
    let corrupt = package::splice_part(&signed, "Contents/section0.xml", "</hs:sec>", "");
    for (bytes, encrypted) in [(encrypted, true), (corrupt, false)] {
        for (editor, result) in requests.edits(&bytes) {
            let error = result.expect_err(&format!("{editor} returned bytes"));
            assert_eq!(error.code().as_str(), "DECODE_FAILED", "{editor}: {error}");
            assert!(!error.to_string().contains("package integrity"), "{editor}: {error}");
            if encrypted {
                assert!(
                    error.to_string().contains("Password required to decrypt file"),
                    "{editor}: {error}"
                );
            }
        }
    }
}

// 이것을 실패시키는 것: structural 빈 요청보다 앞에서 패키지 검사 실행.
#[test]
fn integrity_empty_structural_requests_return_exact_input() {
    let base = base();
    let mut inputs: Vec<Vec<u8>> = markers(&base).into_iter().map(|(_, bytes)| bytes).collect();
    inputs.push(append(&base, "private.bin", b"locked", true));
    inputs.push(b"not a package".to_vec());
    for bytes in inputs {
        assert_eq!(HwpxStructuralEditor::delete_paragraphs(&bytes, &[]).unwrap(), bytes);
        assert_eq!(
            HwpxStructuralEditor::insert_paragraphs(
                &bytes,
                ParagraphLocator { section: 999, index: 999 },
                InsertPosition::After,
                &[]
            )
            .unwrap(),
            bytes
        );
    }
}
