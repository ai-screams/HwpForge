use hwpforge_core::{Document, ImageStore, PageSettings, Paragraph, Run, Section};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge_smithy_hwpx::{style_store_for_preset, HwpxDecoder, HwpxEncoder};
use std::io::{Cursor, Read, Write};

const LIMIT: usize = 64 * 1024; // = decoder MAX_TEXT_BYTES

fn package(inner: Option<&str>) -> Vec<u8> {
    let mut doc = Document::new();
    let mut p = Paragraph::new(ParaShapeIndex::new(0));
    p.runs.push(Run::text("(   )", CharShapeIndex::new(0)));
    doc.add_section(Section::with_paragraphs(vec![p], PageSettings::default()));
    let base = HwpxEncoder::encode(
        &doc.validate().unwrap(),
        &style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
    )
    .unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(base)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        if entry.name().unwrap() == "Contents/content.hpf" {
            if let Some(inner) = inner {
                let mut xml = String::new();
                entry.read_to_string(&mut xml).unwrap();
                let start = xml.find("<opf:metadata>").unwrap();
                let end = xml.find("</opf:metadata>").unwrap() + "</opf:metadata>".len();
                xml.replace_range(start..end, inner);
                out.start_file(entry.name().unwrap(), zip::write::SimpleFileOptions::default())
                    .unwrap();
                out.write_all(xml.as_bytes()).unwrap();
            }
        } else {
            out.raw_copy_file(entry).unwrap();
        }
    }
    out.finish().unwrap().into_inner()
}

fn block(extra: &str) -> String {
    format!(
        r#"<opf:metadata><opf:title>title</opf:title><opf:meta name="creator">author</opf:meta>{extra}</opf:metadata>"#
    )
}

// 이것을 실패시키는 것: 넘침 전체 기본값 복원, 경계 >= 변경, 경고 전달 삭제.
#[test]
fn metadata_value_boundaries() {
    for n in [LIMIT, (LIMIT + 1)] {
        for attribute in [false, true] {
            let big = "x".repeat(n);
            let extra = if attribute {
                format!(r#"<opf:meta name="{big}">v</opf:meta>"#)
            } else {
                format!(r#"<opf:meta name="description">{big}</opf:meta>"#)
            };
            let bytes = package(Some(&block(&extra)));
            let decoded = HwpxDecoder::decode(&bytes).unwrap();
            let m = decoded.document.metadata();
            assert_eq!(m.title.as_deref(), Some("title"));
            assert_eq!(m.author.as_deref(), Some("author"));
            assert_eq!(decoded.warnings.len(), usize::from(n > LIMIT), "{:?}", decoded.warnings);
            if n == LIMIT {
                if attribute {
                    assert_eq!(m.extras.get(&big).unwrap(), "v");
                } else {
                    assert_eq!(m.description.as_deref(), Some(big.as_str()));
                }
            } else {
                assert!(m.description.is_none());
                assert!(m.extras.is_empty());
                assert_eq!(decoded.warnings[0].code(), "CONTENT_LOST");
                assert!(decoded.warnings[0].is_content_loss());
                assert!(decoded.warnings[0].to_string().contains(&(LIMIT + 1).to_string()));
            }
        }
    }
}

// 이것을 실패시키는 것: GeneralRef 무시 또는 참조 바이트 합산 삭제.
#[test]
fn metadata_references_and_expanded_boundary() {
    let xml = r#"<opf:metadata><opf:title>A &amp; B &#xAC00; C</opf:title><opf:meta name="creator">A &amp; B &#xAC00; C</opf:meta></opf:metadata>"#;
    let d = HwpxDecoder::decode(&package(Some(xml))).unwrap();
    assert_eq!(d.document.metadata().title.as_deref(), Some("A & B 가 C"));
    assert_eq!(d.document.metadata().author.as_deref(), Some("A & B 가 C"));
    for n in [(LIMIT - 3), (LIMIT - 2)] {
        let extra = format!(r#"<opf:meta name="description">{}&#xAC00;</opf:meta>"#, "x".repeat(n));
        let d = HwpxDecoder::decode(&package(Some(&block(&extra)))).unwrap();
        assert_eq!(d.warnings.len(), usize::from(n == (LIMIT - 2)));
        assert_eq!(
            d.document.metadata().description.as_ref().map(String::len),
            if n == (LIMIT - 3) { Some(LIMIT) } else { None }
        );
    }
}

// 이것을 실패시키는 것: 파서 실패/블록 없음에 무조건 경고 추가.
#[test]
fn metadata_absence_and_malformed_warning_counts() {
    for (xml, count) in
        [(None, 2), (Some(""), 0), (Some("<opf:metadata><opf:title>x</opf:bad></opf:metadata>"), 1)]
    {
        let d = HwpxDecoder::decode(&package(xml)).unwrap();
        assert_eq!(d.warnings.len(), count, "{:?}", d.warnings);
        assert!(d.warnings.iter().all(|w| w.is_content_loss()));
    }
}

// 이것을 실패시키는 것: 메타데이터 경고 전달 삭제.
#[test]
fn metadata_loss_rejects_stamp() {
    use hwpforge_smithy_hwpx::stamp::{HwpxStamper, StampAction, StampSpec};
    let bytes = package(Some(&block(&format!(
        r#"<opf:meta name="description">{}</opf:meta>"#,
        "x".repeat(LIMIT + 1)
    ))));
    let candidates = HwpxStamper::plan_bytes(&bytes).unwrap();
    let c = &candidates[0];
    let specs = [StampSpec {
        section: c.section,
        path: c.path.clone(),
        span: c.span.clone(),
        marker: c.marker.clone(),
        action: StampAction::Field { name: "slot".into(), hint: None },
    }];
    let error = HwpxStamper::stamp(&bytes, &specs).unwrap_err();
    assert!(
        matches!(error, hwpforge_smithy_hwpx::stamp::StamperError::NotRoundTripSafe { component, .. } if component == "input decode: CONTENT_LOST meta[name=description]")
    );
}

// 이것을 실패시키는 것: 참조 오류를 전체 기본값으로 변경하거나 경고 없이 값만 삭제.
#[test]
fn unresolved_metadata_reference_preserves_siblings() {
    for reference in ["&nbsp;", "&#0;", "&#xD800;", "&#x110000;"] {
        let xml = format!(
            r#"<opf:metadata><opf:title>A{reference}B</opf:title><opf:meta name="creator">x</opf:meta></opf:metadata>"#
        );
        let decoded = HwpxDecoder::decode(&package(Some(&xml))).unwrap();
        assert!(decoded.document.metadata().title.is_none());
        assert_eq!(decoded.document.metadata().author.as_deref(), Some("x"));
        assert_eq!(decoded.warnings.len(), 1);
        assert_eq!(decoded.warnings[0].code(), "CONTENT_LOST");
        assert!(decoded.warnings[0].to_string().contains("unresolved reference"));
    }
}

// 이것을 실패시키는 것: 전체 파서 오류 경고 삭제, 패키지 경고 중복 억제 삭제.
#[test]
fn metadata_parser_failures_warn_once() {
    let inputs = [
        block("<opf:title><![CDATA[lost]]></opf:title>"),
        block(&r#"<opf:meta name="x"/>"#.repeat(257)),
        block(&format!("{}lost{}", "<opf:x>".repeat(16), "</opf:x>".repeat(16))),
        r#"<opf:metadata xmlns:hp="urn:other"><hp:title>lost</hp:title></opf:metadata>"#.into(),
        block("<opf:title>lost</opf:bad>"),
    ];
    for xml in inputs {
        let decoded = HwpxDecoder::decode(&package(Some(&xml))).unwrap();
        assert!(decoded.document.metadata().title.is_none());
        assert_eq!(decoded.warnings.len(), 1, "{xml}: {:?}", decoded.warnings);
        assert_eq!(decoded.warnings[0].code(), "CONTENT_LOST");
    }
}

fn rewrite_entry(bytes: &[u8], name: &str, edit: impl Fn(&str) -> String) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        if entry.name().unwrap() == name {
            let mut xml = String::new();
            entry.read_to_string(&mut xml).unwrap();
            out.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            out.write_all(edit(&xml).as_bytes()).unwrap();
        } else {
            out.raw_copy_file(entry).unwrap();
        }
    }
    out.finish().unwrap().into_inner()
}

fn warning_count(bytes: &[u8]) -> usize {
    HwpxDecoder::decode(bytes).unwrap().warnings.len()
}

// 이것을 실패시키는 것: 컨테이너 문제만 있어도 메타데이터 실패 경고를 막도록 억제 조건을 넓히기,
// 또는 manifest·spine 문제가 있어도 메타데이터 실패 경고를 내도록 억제 조건을 지우기.
#[test]
fn metadata_failure_warning_depends_only_on_content_hpf_problems() {
    let unreadable = block("<opf:title><![CDATA[lost]]></opf:title>");
    let extra_rootfile = |xml: &str| {
        xml.replace(
            "</ocf:rootfiles>",
            r#"<ocf:rootfile full-path="Preview/Missing.txt" media-type="text/plain"/></ocf:rootfiles>"#,
        )
    };
    let unknown_spine_item = |xml: &str| {
        xml.replace(
            r#"<opf:spine>"#,
            r#"<opf:spine><opf:itemref idref="missing-item" linear="yes"/>"#,
        )
    };

    // A container-only problem does not cover content.hpf: the metadata
    // failure adds its own warning.
    let readable =
        rewrite_entry(&package(Some(&block(""))), "META-INF/container.xml", extra_rootfile);
    let lost = rewrite_entry(&package(Some(&unreadable)), "META-INF/container.xml", extra_rootfile);
    assert!(warning_count(&readable) >= 1);
    assert_eq!(warning_count(&lost), warning_count(&readable) + 1);

    // A spine problem in content.hpf already marks the document lossy: the
    // metadata failure is not reported a second time.
    let readable =
        rewrite_entry(&package(Some(&block(""))), "Contents/content.hpf", unknown_spine_item);
    let lost =
        rewrite_entry(&package(Some(&unreadable)), "Contents/content.hpf", unknown_spine_item);
    assert!(warning_count(&readable) >= 1);
    assert_eq!(warning_count(&lost), warning_count(&readable));
}
