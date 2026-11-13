use super::*;
use crate::error::CodecStage;
use crate::test_support::xml;

use crate::cell_edit::test_package as package_support;

fn zip(entries: &[(&str, usize)]) -> Vec<u8> {
    super::tests::zeros_zip(entries)
}

fn changes(rewritten: &[&str]) -> OutputChanges {
    OutputChanges {
        rewritten: rewritten.iter().map(|s| (*s).into()).collect(),
        ..Default::default()
    }
}

fn rejects(base: &[u8], output: &[u8], changes: &OutputChanges, reason: &str) {
    let error = RawPackage::verify_output(base, output, changes).unwrap_err();
    assert_eq!(error.stage, CodecStage::Verification);
    assert!(error.detail.contains(reason), "{error:?}");
}

// 이것을 실패시키는 것: 비대상 payload 비교 제거 또는 검증 오류를 Admission으로 분류.
#[test]
fn output_rejects_payload_overwrite() {
    let base = zip(&[("BinData/image.png", 8), ("META-INF/container.rdf", 8)]);
    for part in ["BinData/image.png", "META-INF/container.rdf"] {
        let output = package_support::splice_part(&base, part, "\0", "x");
        rejects(&base, &output, &changes(&[]), "payload");
    }
}

// 이것을 실패시키는 것: 누락 항목 거부 제거 (재작성 선언도 삭제 허용이 아님).
#[test]
fn output_rejects_missing_entry() {
    let base = zip(&[("Contents/section0.xml", 8)]);
    let output = zip(&[]);
    for declaration in [changes(&[]), changes(&["Contents/section0.xml"])] {
        rejects(&base, &output, &declaration, "missing");
    }
}

// 이것을 실패시키는 것: 내용이 같은 매체의 압축 방식 차이를 거부.
#[test]
fn output_accepts_compression_change() {
    let base = zip(&[("BinData/image.png", 8)]);
    let mut output = RawPackage::read_capped(&base, MAX_ENTRY_SIZE, MAX_TOTAL_SIZE).unwrap();
    output.entries[0].compression = CompressionMethod::Stored;
    let output = output.write().unwrap();
    RawPackage::verify_output(&base, &output, &changes(&[])).unwrap();
    RawPackage::verify_output(&output, &base, &changes(&[])).unwrap();
}

// 이것을 실패시키는 것: writer의 mimetype Stored 정규화 제거, payload 교체,
// 또는 최종 압축 검사를 재작성 선언으로 우회.
#[test]
fn output_requires_stored_mimetype() {
    let compressed = zip(&[("mimetype", 8)]);
    let package = RawPackage::read_capped(&compressed, MAX_ENTRY_SIZE, MAX_TOTAL_SIZE).unwrap();
    let stored = package.write().unwrap();
    let mut output = open_package(&stored, Default::default()).unwrap();
    assert_eq!(output.entries()[0].compression, CompressionMethod::Stored);
    assert_eq!(output.read(0).unwrap(), vec![0; 8]);
    for declaration in [changes(&[]), changes(&["mimetype"])] {
        RawPackage::verify_output(&compressed, &stored, &declaration).unwrap();
        rejects(&stored, &compressed, &declaration, "must be stored uncompressed");
        rejects(&compressed, &compressed, &declaration, "must be stored uncompressed");
    }
}

// 이것을 실패시키는 것: opaque 비교 제거. slash 별칭 추가는 기존 항목의 분류도 바꾼다.
#[test]
fn output_rejects_opaque_change() {
    let base = zip(&[("asset", 8)]);
    let output = zip(&[("asset", 8), ("asset/", 0)]);
    rejects(&base, &output, &changes(&["asset/"]), "opaque");
}

// 이것을 실패시키는 것: 출력의 선언하지 않은 새 항목 검사 제거.
#[test]
fn output_rejects_undeclared_addition() {
    let base = zip(&[("asset", 8)]);
    let output = zip(&[("asset", 8), ("extra", 0)]);
    rejects(&base, &output, &changes(&[]), "undeclared");
}

// 이것을 실패시키는 것: 재작성 면제 제거, 선언된 새 항목 거부.
#[test]
fn output_accepts_declared_changes_and_noop() {
    let base = zip(&[("Contents/section0.xml", 8), ("asset", 8)]);
    RawPackage::verify_output(&base, &base, &changes(&[])).unwrap();
    let output = zip(&[("Contents/section0.xml", 9), ("asset", 8), ("new", 1)]);
    RawPackage::verify_output(&base, &output, &changes(&["Contents/section0.xml", "new"])).unwrap();
}

// 이것을 실패시키는 것: Verification 단계는 유지하면서 이름 오류의 구체적 사유를 지우기.
#[test]
fn output_rejects_invalid_names() {
    let base = zip(&[]);
    let invalid = zip(&[("Contents/../escape", 1)]);
    rejects(
        &base,
        &invalid,
        &changes(&["Contents/../escape"]),
        "invalid package part name 'Contents/../escape'",
    );
}

// 이것을 실패시키는 것: 출력 읽기에 전달하는 작은 한도를 한 바이트 완화.
#[test]
fn output_rejects_limits() {
    let base = zip(&[]);
    let oversized = zip(&[("large", 33)]);
    let error =
        RawPackage::verify_output_capped(&base, None, &oversized, &changes(&["large"]), 32, 64)
            .unwrap_err();
    assert_eq!(error.stage, CodecStage::Verification);
    assert!(error.detail.contains("exceeds limit of 32"), "{error:?}");
}

// 이것을 실패시키는 것: 넘겨받은 원본 대신 base를 다시 읽거나 출력 전체를 RawPackage로 만들기.
#[test]
fn verify_reads_whole_packages_only_for_an_unread_base() {
    let base = zip(&[("asset", 8), ("Contents/section0.xml", 8)]);
    let original = RawPackage::read_capped(&base, MAX_ENTRY_SIZE, MAX_TOTAL_SIZE).unwrap();
    let output = zip(&[("asset", 8), ("Contents/section0.xml", 9)]);
    let declared = changes(&["Contents/section0.xml"]);
    PACKAGE_IO.with(|count| count.set((0, 0)));
    original.verify_output_with(&base, &output, &declared).unwrap();
    let overwritten = package_support::splice_part(&output, "asset", "\0", "x");
    let error = original.verify_output_with(&base, &overwritten, &declared).unwrap_err();
    assert!(error.detail.contains("'asset': payload changed"), "{error:?}");
    assert_eq!(PACKAGE_IO.with(|count| count.get()), (0, 0));
    RawPackage::verify_output(&base, &output, &declared).unwrap();
    assert_eq!(PACKAGE_IO.with(|count| count.get()), (1, 0));
}

// 이것을 실패시키는 것: 오류 문구에 항목 이름을 이스케이프·길이 제한 없이 넣기.
#[test]
fn violation_names_are_escaped_and_bounded() {
    let base = zip(&[]);
    let long = "가".repeat(300);
    for (name, shown) in [
        ("a\nb", "a\\nb".to_owned()),
        (long.as_str(), format!("{}... (900 bytes total)", "가".repeat(64))),
    ] {
        let output = zip(&[(name, 1)]);
        let error = RawPackage::verify_output(&base, &output, &changes(&[])).unwrap_err();
        let expected = format!("output package preservation: '{shown}': undeclared new part");
        assert!(error.detail.contains(&expected), "{error:?}");
    }
}

/// Text of each top-level paragraph of section 0, in order (nested `hp:t` joined).
fn paragraph_texts(bytes: &[u8]) -> Vec<String> {
    fn text(node: &xml::Element, out: &mut String) {
        if node.name == "hp:t" {
            out.push_str(&node.text);
        }
        node.children.iter().for_each(|child| text(child, out));
    }
    let section = xml::Element::parse(&package_support::section_xml(bytes));
    section
        .children
        .iter()
        .filter(|node| node.name == "hp:p")
        .map(|paragraph| {
            let mut out = String::new();
            text(paragraph, &mut out);
            out
        })
        .collect()
}

fn editable_package() -> Vec<u8> {
    editable_package_with_media(false)
}

fn editable_package_with_deflated_mimetype() -> Vec<u8> {
    let package = RawPackage::read(&editable_package()).unwrap();
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for entry in package.entries {
        let method =
            if entry.path == "mimetype" { CompressionMethod::Deflated } else { entry.compression };
        writer
            .start_file(entry.path, SimpleFileOptions::default().compression_method(method))
            .unwrap();
        writer.write_all(&entry.bytes).unwrap();
    }
    let bytes = writer.finish().unwrap().into_inner();
    let package = open_package(&bytes, Default::default()).unwrap();
    assert_eq!(package.entries()[0].path, "mimetype");
    assert_eq!(package.entries()[0].compression, CompressionMethod::Deflated);
    bytes
}

fn assert_stored_mimetype(base: &[u8], output: &[u8]) {
    let base = RawPackage::read(base).unwrap();
    let output = RawPackage::read(output).unwrap();
    let entry = &output.entries[output.index_by_path["mimetype"]];
    assert_eq!(entry.compression, CompressionMethod::Stored);
    assert_eq!(entry.bytes, base.read_entry("mimetype").unwrap());
}

// 이것을 실패시키는 것: 구조 삭제 writer에서 원본 mimetype 압축을 그대로 복사.
#[test]
fn delete_normalizes_deflated_mimetype() {
    use crate::structural::{HwpxStructuralEditor, ParagraphLocator};
    let base = editable_package_with_deflated_mimetype();
    let output = HwpxStructuralEditor::delete_paragraphs(
        &base,
        &[ParagraphLocator { section: 0, index: 1 }],
    )
    .unwrap();
    assert_stored_mimetype(&base, &output);
    // 이것을 실패시키는 것: 다른 문단(표 문단)을 지우거나 순서를 바꾸기.
    assert_eq!(paragraph_texts(&output), ["□", ""]);
}

// 이것을 실패시키는 것: 구조 삽입 writer에서 원본 mimetype 압축을 그대로 복사.
#[test]
fn insert_normalizes_deflated_mimetype() {
    use crate::structural::{HwpxStructuralEditor, InsertPosition, ParagraphLocator};
    let base = editable_package_with_deflated_mimetype();
    let output = HwpxStructuralEditor::insert_paragraphs(
        &base,
        ParagraphLocator { section: 0, index: 1 },
        InsertPosition::After,
        &["insert".into()],
    )
    .unwrap();
    assert_stored_mimetype(&base, &output);
    // 이것을 실패시키는 것: 새 문단을 기준 문단 앞에 넣기.
    assert_eq!(paragraph_texts(&output), ["□", "remove", "insert", ""]);
}

fn text_para(text: &str) -> hwpforge_core::Paragraph {
    runs_para(vec![hwpforge_core::Run::text(text, hwpforge_foundation::CharShapeIndex::new(0))])
}

fn runs_para(runs: Vec<hwpforge_core::Run>) -> hwpforge_core::Paragraph {
    hwpforge_core::Paragraph::with_runs(runs, hwpforge_foundation::ParaShapeIndex::new(0))
}

fn editable_package_with_media(media: bool) -> Vec<u8> {
    use hwpforge_core::{Document, PageSettings, Run, Section, Table, TableCell, TableRow};
    use hwpforge_foundation::{CharShapeIndex, HwpUnit};
    let table = Table::new(vec![TableRow::new(vec![TableCell::new(
        vec![text_para("")],
        HwpUnit::new(8000).unwrap(),
    )])]);
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![
            text_para("□"),
            text_para("remove"),
            runs_para(vec![Run::table(table, CharShapeIndex::new(0))]),
        ],
        PageSettings::a4(),
    ));
    doc.add_section(Section::with_paragraphs(vec![text_para("untouched")], PageSettings::a4()));
    let mut styles = crate::HwpxStyleStore::with_default_fonts("함초롬돋움");
    styles.push_char_shape(crate::HwpxCharShape::default());
    styles.push_para_shape(crate::HwpxParaShape::default());
    let mut images = hwpforge_core::image::ImageStore::new();
    if media {
        use hwpforge_core::chart::{ChartData, ChartType};
        use hwpforge_core::{Control, Image};
        let section = &mut doc.sections_mut()[0];
        let image = Image::new(
            "swatch.png",
            HwpUnit::new(1000).unwrap(),
            HwpUnit::new(1000).unwrap(),
            hwpforge_core::image::ImageFormat::Png,
        );
        images.insert(
            "swatch.png",
            vec![
                137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0,
                1, 8, 2, 0, 0, 0, 144, 119, 83, 222, 0, 0, 0, 12, 73, 68, 65, 84, 120, 156, 99,
                176, 89, 16, 0, 0, 2, 72, 1, 45, 108, 176, 0, 156, 0, 0, 0, 0, 73, 69, 78, 68, 174,
                66, 96, 130,
            ],
        );
        let chart =
            Control::chart(ChartType::Bar, ChartData::category(&["A", "B"], &[("S", &[1.0, 2.0])]));
        let embedded = Control::EmbeddedChart {
            chart_xml: crate::encoder::chart::generate_chart_xml(&chart).unwrap(),
            ole_bytes: vec![0xD0, 0xCF, 0x11, 0xE0],
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(1000).unwrap(),
            placement: None,
        };
        section.paragraphs.push(runs_para(vec![Run::image(image, CharShapeIndex::new(0))]));
        for control in [chart, embedded] {
            section.paragraphs.push(runs_para(vec![Run::control(control, CharShapeIndex::new(0))]));
        }
        section.master_pages = Some(vec![
            hwpforge_core::section::MasterPage::new(
                hwpforge_foundation::ApplyPageType::Even,
                vec![text_para("watermark")],
            ),
            hwpforge_core::section::MasterPage::new(
                hwpforge_foundation::ApplyPageType::Odd,
                vec![text_para("watermark")],
            ),
        ]);
        doc.sections_mut()[1].master_pages = Some(vec![hwpforge_core::section::MasterPage::new(
            hwpforge_foundation::ApplyPageType::Both,
            vec![text_para("watermark")],
        )]);
    }
    crate::HwpxEncoder::encode(&doc.validate().unwrap(), &styles, &images).unwrap()
}

fn observed_changes() -> OutputChanges {
    VERIFIED_CHANGES
        .with(|observed| observed.borrow_mut().take().expect("final output was checked"))
}

fn expect_reencoded(mut actual: OutputChanges, masters: bool) {
    actual.rewritten.sort();
    let mut expected = changes(&[
        "mimetype",
        "version.xml",
        "META-INF/container.xml",
        "META-INF/manifest.xml",
        "Contents/content.hpf",
        "settings.xml",
        "Contents/header.xml",
        "Contents/section0.xml",
        "Contents/section1.xml",
    ]);
    if masters {
        expected.rewritten.extend([
            "Contents/masterpage0.xml".into(),
            "Contents/masterpage1.xml".into(),
            "Contents/masterpage2.xml".into(),
        ]);
    }
    expected.rewritten.sort();
    assert_eq!(actual, expected);
}

// 이것을 실패시키는 것: delete 출력 검사 제거 또는 선언을 다른 section까지 확장.
#[test]
fn delete_declares_only_edited_sections() {
    use crate::structural::{HwpxStructuralEditor, ParagraphLocator};
    let base = editable_package();
    HwpxStructuralEditor::delete_paragraphs(&base, &[ParagraphLocator { section: 0, index: 1 }])
        .unwrap();
    let declaration = observed_changes();
    assert_eq!(declaration, changes(&["Contents/section0.xml"]));
    let output =
        package_support::splice_part(&base, "Contents/section1.xml", "untouched", "overwritten");
    rejects(&base, &output, &declaration, "payload");
}

// 이것을 실패시키는 것: insert 출력 검사 제거 또는 선언을 다른 section까지 확장.
#[test]
fn insert_declares_only_edited_section() {
    use crate::structural::{HwpxStructuralEditor, InsertPosition, ParagraphLocator};
    let base = editable_package();
    HwpxStructuralEditor::insert_paragraphs(
        &base,
        ParagraphLocator { section: 0, index: 1 },
        InsertPosition::After,
        &["insert".into()],
    )
    .unwrap();
    assert_eq!(observed_changes(), changes(&["Contents/section0.xml"]));
}

// 이것을 실패시키는 것: 빈 delete 또는 insert 요청에서 패키지 검사를 실행.
#[test]
fn structural_noops_return_unchecked_input() {
    use crate::structural::{HwpxStructuralEditor, InsertPosition, ParagraphLocator};
    let base = b"not a ZIP";
    VERIFIED_CHANGES.with(|observed| *observed.borrow_mut() = None);
    assert_eq!(HwpxStructuralEditor::delete_paragraphs(base, &[]).unwrap(), base);
    assert_eq!(
        HwpxStructuralEditor::insert_paragraphs(
            base,
            ParagraphLocator { section: usize::MAX, index: usize::MAX },
            InsertPosition::After,
            &[],
        )
        .unwrap(),
        base
    );
    assert!(VERIFIED_CHANGES.with(|observed| observed.borrow().is_none()));
}

// 이것을 실패시키는 것: set-cell 출력 검사 제거, 고정 part 선언 누락, writer의 고정 part 중복.
#[test]
fn set_cell_declares_only_changed_parts() {
    use crate::cell_edit::{CellSpec, CellTarget, HwpxCellEditor};
    let base = editable_package();
    let preserving = HwpxCellEditor::set_cells(
        &base,
        &[CellSpec {
            table: 0,
            target: CellTarget::At(hwpforge_core::table::grid::GridCoord::new(0, 0)),
            text: "value".into(),
        }],
    )
    .unwrap();
    assert_eq!(observed_changes(), changes(&["Contents/section0.xml"]));
    assert_public_cell_patch(&base, &preserving.bytes);
    let base = package_support::two_text_runs(&base, "<hp:t/>");
    HwpxCellEditor::set_cells(
        &base,
        &[CellSpec {
            table: 0,
            target: CellTarget::At(hwpforge_core::table::grid::GridCoord::new(0, 0)),
            text: "value".into(),
        }],
    )
    .unwrap();
    expect_reencoded(observed_changes(), false);
}

fn stamp_spec(base: &[u8]) -> crate::stamp::StampSpec {
    let plan = crate::stamp::HwpxStamper::plan_bytes(base).unwrap();
    let candidate = &plan[0];
    crate::stamp::StampSpec {
        section: candidate.section,
        path: candidate.path.clone(),
        span: candidate.span.clone(),
        marker: candidate.marker.clone(),
        action: crate::stamp::StampAction::Field { name: "name".into(), hint: None },
    }
}

// 이것을 실패시키는 것: stamp v1 출력 검사 제거 또는 매체까지 재작성 선언 확장.
#[test]
fn stamp_declares_only_reencoded_document_parts() {
    let base = editable_package();
    crate::stamp::HwpxStamper::stamp(&base, &[stamp_spec(&base)]).unwrap();
    expect_reencoded(observed_changes(), false);
}

// 이것을 실패시키는 것: stamp v2 출력 검사 제거 또는 매체까지 재작성 선언 확장.
#[test]
fn stamp_v2_declares_only_reencoded_document_parts() {
    let base = editable_package();
    crate::stamp::HwpxStamper::stamp_v2(
        &base,
        &crate::stamp::StampRequestV2 {
            schema_version: crate::stamp::STAMP_MAP_VERSION,
            source_sha256: crate::patch::sha256_hex(&base),
            text: vec![stamp_spec(&base)],
            cells: Vec::new(),
        },
    )
    .unwrap();
    expect_reencoded(observed_changes(), false);
}

// 이것을 실패시키는 것: 정규화한 header를 선언하지 않거나 양쪽 편집기의 허용 범위 확대.
#[test]
fn structural_declares_header_only_when_normalized() {
    use crate::structural::{HwpxStructuralEditor, InsertPosition, ParagraphLocator};
    let base = editable_package();
    let mut package = RawPackage::read(&base).unwrap();
    let xml = package.read_text_entry("Contents/header.xml").unwrap();
    let mut head: crate::schema::header::HxHead = crate::decoder::xml_from_str(&xml).unwrap();
    let para = &mut head.ref_list.as_mut().unwrap().para_properties.as_mut().unwrap().items[0];
    para.switches[0].default = None;
    #[allow(clippy::disallowed_methods)] // Normalize serialized attribute whitespace below.
    let xml = quick_xml::se::to_string(&head).unwrap();
    package.replace_text_entry(
        "Contents/header.xml",
        crate::wire_xml::normalize_attr_control_whitespace(&xml).into_owned(),
    );
    let base = package.write().unwrap();
    assert!(crate::HwpxDecoder::decode(&base).unwrap().requires_header_normalization());
    HwpxStructuralEditor::delete_paragraphs(&base, &[ParagraphLocator { section: 0, index: 1 }])
        .unwrap();
    assert_eq!(observed_changes(), changes(&["Contents/section0.xml", "Contents/header.xml"]));
    HwpxStructuralEditor::insert_paragraphs(
        &base,
        ParagraphLocator { section: 0, index: 1 },
        InsertPosition::After,
        &["insert".into()],
    )
    .unwrap();
    assert_eq!(observed_changes(), changes(&["Contents/section0.xml", "Contents/header.xml"]));
}

// 이것을 실패시키는 것: 빈 디렉터리 면제 삭제, 헤더 크기 분류, 자리표시자에 내용 추가 허용.
#[test]
fn empty_directories_use_actual_bytes() {
    let empty = zip(&[]);
    let mut base = zip(&[("Unused/", 0)]);
    super::tests::forge_declared_size(&mut base, 100);
    RawPackage::verify_output(&base, &empty, &changes(&[])).unwrap();
    RawPackage::verify_output(&empty, &base, &changes(&[])).unwrap();
    crate::stamp::check_zip_carry(&base, &empty).unwrap();
    rejects(&base, &zip(&[("Unused/", 1)]), &changes(&[]), "undeclared");
    for base in [zip(&[("Unused/", 1)]), zip(&[("Unused/./", 0)])] {
        rejects(&base, &empty, &changes(&[]), "missing");
        assert!(crate::stamp::check_zip_carry(&base, &empty).is_err());
    }
}

// 이것을 실패시키는 것: 바탕쪽 선언 누락·초과, 매체 선언 확대, 매체 압축 차이 거부.
#[test]
fn media_survives_real_editors_and_rejects_overwrite() {
    use crate::cell_edit::{CellSpec, CellTarget, HwpxCellEditor};
    use crate::stamp::{HwpxStamper, StampRequestV2, STAMP_MAP_VERSION};
    let base = editable_package_with_media(true);
    let mut package = RawPackage::read(&base).unwrap();
    for entry in &mut package.entries {
        if entry.path.starts_with("BinData/") {
            entry.compression = CompressionMethod::Deflated;
        }
    }
    let base = package.write().unwrap();
    for path in
        ["BinData/swatch.png", "Chart/chart1.xml", "BinData/ole1.ole", "Contents/masterpage0.xml"]
    {
        assert!(
            package.index_by_path.contains_key(path),
            "{path}: {:?}",
            package.entry_paths().collect::<Vec<_>>()
        );
    }
    for editor in 0..3 {
        let output = match editor {
            0 => {
                HwpxCellEditor::set_cells(
                    &base,
                    &[CellSpec {
                        table: 0,
                        target: CellTarget::At(hwpforge_core::table::grid::GridCoord::new(0, 0)),
                        text: "value".into(),
                    }],
                )
                .unwrap()
                .bytes
            }
            1 => HwpxStamper::stamp(&base, &[stamp_spec(&base)]).unwrap().bytes,
            _ => {
                HwpxStamper::stamp_v2(
                    &base,
                    &StampRequestV2 {
                        schema_version: STAMP_MAP_VERSION,
                        source_sha256: HwpxStamper::plan_bytes_v2(&base).unwrap().source_sha256,
                        text: vec![stamp_spec(&base)],
                        cells: vec![],
                    },
                )
                .unwrap()
                .bytes
            }
        };
        let declaration = observed_changes();
        if editor == 0 {
            assert_public_cell_patch(&base, &output);
            assert_eq!(declaration, changes(&["Contents/section0.xml"]));
        } else {
            expect_reencoded(declaration.clone(), true);
        }
        RawPackage::verify_output(&base, &output, &declaration).unwrap();
        for path in
            ["Contents/masterpage0.xml", "Contents/masterpage1.xml", "Contents/masterpage2.xml"]
        {
            let rewritten = package_support::splice_part(&output, path, "watermark", "rebuilt");
            if editor == 0 {
                rejects(&base, &rewritten, &declaration, "payload changed");
            } else {
                RawPackage::verify_output(&base, &rewritten, &declaration).unwrap();
            }
        }
        for path in
            ["BinData/swatch.png", "BinData/ole1.ole", "Chart/chart1.xml", "Chart/chart2.xml"]
        {
            let mut corrupted = RawPackage::read(&output).unwrap();
            let index = corrupted.index_by_path[path];
            corrupted.entries[index].bytes[0] ^= 1;
            rejects(&base, &corrupted.write().unwrap(), &declaration, "payload changed");
        }
    }
}

// Re-encoding names charts by reference order, so a base that references
// chart2 before chart1 comes back with the two parts' payloads swapped. The
// check compares part names, not meaning, and fails closed on that input.
// 이것을 실패시키는 것: 차트 part를 payload 비교에서 빼거나 재작성 선언에 넣기.
#[test]
fn reencode_rejects_charts_referenced_out_of_name_order() {
    use crate::cell_edit::{CellEditError, CellSpec, CellTarget, HwpxCellEditor};
    let base = editable_package_with_media(true);
    let mut package = RawPackage::read(&base).unwrap();
    let [first, second] =
        ["Chart/chart1.xml", "Chart/chart2.xml"].map(|p| package.index_by_path[p]);
    let section = package.read_text_entry("Contents/section0.xml").unwrap();
    assert!(section.contains("Chart/chart1.xml") && section.contains("Chart/chart2.xml"));
    let swapped = section
        .replace("Chart/chart1.xml", "\u{1}")
        .replace("Chart/chart2.xml", "Chart/chart1.xml")
        .replace('\u{1}', "Chart/chart2.xml");
    package.replace_text_entry("Contents/section0.xml", swapped);
    // The fixture's two charts share one payload; make them distinguishable.
    let relabeled = package.read_text_entry("Chart/chart2.xml").unwrap();
    assert!(relabeled.contains("<c:v>A</c:v>"));
    package
        .replace_text_entry("Chart/chart2.xml", relabeled.replace("<c:v>A</c:v>", "<c:v>C</c:v>"));
    let chart = package.entries[second].bytes.clone();
    package.entries[second].bytes = package.entries[first].bytes.clone();
    package.entries[first].bytes = chart;
    assert!(package.entries[first].bytes != package.entries[second].bytes);
    let base = package.write().unwrap();
    let output = HwpxCellEditor::set_cells(
        &base,
        &[CellSpec {
            table: 0,
            target: CellTarget::At(hwpforge_core::table::grid::GridCoord::new(0, 0)),
            text: "value".into(),
        }],
    )
    .unwrap();
    assert_public_cell_patch(&base, &output.bytes);
    for part in ["Chart/chart1.xml", "Chart/chart2.xml"] {
        assert_eq!(package_support::part(&output.bytes, part), package_support::part(&base, part));
    }
    let base = package_support::two_text_runs(&base, "<hp:t/>");
    let error = HwpxCellEditor::set_cells(
        &base,
        &[CellSpec {
            table: 0,
            target: CellTarget::At(hwpforge_core::table::grid::GridCoord::new(0, 0)),
            text: "value".into(),
        }],
    )
    .unwrap_err();
    let CellEditError::Codec(failure) = error else { panic!("{error:?}") };
    assert_eq!(failure.stage, CodecStage::Verification);
    assert!(failure.detail.contains("'Chart/chart1.xml': payload changed"), "{failure:?}");
}

fn assert_public_cell_patch(base: &[u8], output: &[u8]) {
    let decoded = crate::HwpxDecoder::decode(base).unwrap();
    let source = &decoded.document.sections()[0];
    let metadata = crate::HwpxPatcher::export_section_preservation(base, 0, source).unwrap();
    let mut replacement = source.clone();
    let table = replacement
        .paragraphs
        .iter_mut()
        .flat_map(|p| &mut p.runs)
        .find_map(|run| match &mut run.content {
            hwpforge_core::RunContent::Table(table) => Some(table),
            _ => None,
        })
        .unwrap();
    table.rows[0].cells[0].paragraphs[0].runs[0].content =
        hwpforge_core::RunContent::Text("value".into());
    let patched =
        crate::HwpxPatcher::patch_section_preserving(base, 0, &replacement, None, Some(&metadata))
            .unwrap();
    assert_eq!(package_support::section_xml(output), package_support::section_xml(&patched));
}
