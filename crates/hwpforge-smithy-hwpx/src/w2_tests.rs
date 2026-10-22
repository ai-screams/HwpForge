//! Synthetic B1–B4 consumer matrix; no corpus inputs.
use crate::decoder::package::PackageReader;
use crate::patch::RawPackage;
use crate::{EncodeOptions, HwpxDecoder, HwpxEncoder, HwpxPatcher};
use hwpforge_core::zip_package::open_package;
use hwpforge_core::{image::ImageStore, Document, PageSettings, Paragraph, Run, Section};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
use std::io::{Cursor, Write};

fn base() -> Vec<u8> {
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::text("alpha", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    HwpxEncoder::encode(
        &doc.validate().unwrap(),
        &crate::style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
    )
    .unwrap()
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

fn rewrite(base: &[u8], entries: &[(&str, Option<&str>, &[u8])]) -> Vec<u8> {
    let mut input = open_package(base, Default::default()).unwrap();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..input.entries().len() {
        let name = input.entries()[i].path.clone();
        if entries.iter().any(|(raw, effective, _)| effective.unwrap_or(raw) == name) {
            continue;
        }
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(&input.read(i).unwrap()).unwrap();
    }
    for (raw, effective, payload) in entries {
        let mut options = zip::write::FullFileOptions::default();
        if let Some(name) = effective {
            let mut extra = vec![1];
            extra.extend(crc32(raw.as_bytes()).to_le_bytes());
            extra.extend(name.as_bytes());
            options.add_extra_field(0x7075, extra, false).unwrap();
        }
        zip.start_file(*raw, options).unwrap();
        zip.write_all(payload).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

// 이것을 실패시키는 것: 디코더/편집 패키지가 공통 이름 충돌 검사를 우회하기.
#[test]
fn casefold_consumers_reject_normal_collisions() {
    let original = base();
    for (raw, effective) in
        [("Contents/SECTION0.XML", None), ("Other/alias.xml", Some("Contents/SECTION0.XML"))]
    {
        let bytes = rewrite(&original, &[(raw, effective, b"invalid XML")]);
        let decoded = HwpxDecoder::decode(&bytes).unwrap_err();
        assert!(matches!(&decoded, crate::HwpxError::InvalidStructure { .. }));
        assert!(decoded.to_string().contains("case-insensitive name collision"));
        let patched = RawPackage::read(&bytes).unwrap_err();
        assert!(patched.to_string().contains("case-insensitive name collision"));
    }
}

// 이것을 실패시키는 것: mimetype 예외 제거 또는 mimetype 조회를 대소문자 무시로 바꾸기.
#[test]
fn casefold_mimetype_keeps_exact_selection() {
    let bytes = rewrite(&base(), &[("MIMETYPE", None, b"not a valid mimetype")]);
    assert!(HwpxDecoder::decode(&bytes).is_ok());
    let raw = RawPackage::read(&bytes).unwrap();
    assert_eq!(raw.read_entry("MIMETYPE").unwrap(), b"not a valid mimetype");
    assert!(raw.read_entry("mimetype").unwrap().starts_with(b"application/hwp+zip"));
}

// 이것을 실패시키는 것: PackageReader/RawPackage의 opaque 게이트 생략 또는 파일/빈 폴더 충돌의 표시 누락.
#[test]
fn w2_semantic_lookup_and_enumeration_reject_opaque() {
    for path in [
        "mimetype",
        "Contents/content.hpf",
        "META-INF/container.xml",
        "Contents/header.xml",
        "Contents/section0.xml",
        "Contents/masterpage0.xml",
        "BinData/image.png",
        "Chart/chart.xml",
    ] {
        let original = base();
        let mut pkg = open_package(&original, Default::default()).unwrap();
        let payload = match pkg.entries().iter().position(|e| e.path == path) {
            Some(i) => pkg.read(i).unwrap(),
            None => b"<x/>".to_vec(),
        };
        let directory = format!("{path}/");
        let bytes = rewrite(&original, &[(path, None, &payload), (&directory, None, b"")]);
        let mut p = open_package(&bytes, Default::default()).unwrap();
        let i = p.entries().iter().position(|e| e.path == path).unwrap();
        assert_eq!(p.read(i).unwrap(), payload);
        assert!(HwpxDecoder::decode(&bytes).is_err(), "{path}");
        if let Ok(mut reader) = PackageReader::new(&bytes) {
            assert!(reader.read_text_entry(path).is_err(), "reader {path}");
        }
        if let Ok(raw) = RawPackage::read(&bytes) {
            assert!(raw.read_text_entry(path).is_err(), "raw {path}");
        }
    }
    for path in ["BinData/./image.png", "Chart/./chart.xml"] {
        let bytes = rewrite(&base(), &[(path, None, b"<x/>")]);
        assert!(HwpxDecoder::decode(&bytes).is_err(), "{path}");
    }
}

// 이것을 실패시키는 것: diff의 의미 이름 읽기 복원/폴더 제외, RawPackage 쓰기 정규화,
// 편집 encoder의 opaque 복사 제거, unknown opaque XML을 의미 XML로 해석하기.
// 이것을 실패시키는 것: 빈 opaque 폴더를 patch/encoder 복사 또는 diff 비교 목록에서 제외하기.
// 이번 보강을 실패시키는 것: opaque payload 복사 대신 빈 바이트를 저장하기.
#[test]
fn w2_opaque_copy_diff_patch_and_encoder_matrix() {
    for names in [
        vec!["Unused/./a.bin"],
        vec!["Unused/./"],
        vec!["Unused/a.bin/", "Unused/a.bin"],
        vec!["Unused/"],
        vec!["Unused/./unknown.xml"],
    ] {
        let original = base();
        let payload = if names[0] == "Unused/./" { b"".as_slice() } else { b"x".as_slice() };
        let specs: Vec<_> = names.iter().map(|n| (*n, None, payload)).collect();
        let bytes = rewrite(&original, &specs);
        let decoded = HwpxDecoder::decode(&bytes).unwrap();
        let listed = PackageReader::new(&bytes).unwrap().list_entries().unwrap();
        for name in &names {
            assert!(listed
                .iter()
                .any(|entry| entry.path == *name && entry.size == payload.len() as u64));
        }
        let same = crate::diff::HwpxDiffer::diff(&bytes, &bytes).unwrap();
        assert!(same.identical);
        let removed = crate::diff::HwpxDiffer::diff(&bytes, &original).unwrap();
        assert_eq!(removed.package.removed, names);
        let changed = rewrite(&bytes, &[(names[0], None, b"y")]);
        let delta = crate::diff::HwpxDiffer::diff(&bytes, &changed).unwrap();
        assert_eq!(delta.package.changed, vec![names[0]]);
        assert!(delta.semantic.is_empty());
        let preservation =
            HwpxPatcher::export_section_preservation(&bytes, 0, &decoded.document.sections()[0])
                .unwrap();
        let mut section = decoded.document.sections()[0].clone();
        section.paragraphs[0].runs[0] = Run::text("beta", CharShapeIndex::new(0));
        let patched =
            HwpxPatcher::patch_section_preserving(&bytes, 0, &section, None, Some(&preservation))
                .unwrap();
        let noop = HwpxPatcher::patch_section_preserving(
            &bytes,
            0,
            &decoded.document.sections()[0],
            None,
            Some(&preservation),
        )
        .unwrap();
        let mut doc = decoded.document.clone();
        doc.sections_mut()[0] = section;
        let edited = HwpxEncoder::encode_with_base(
            &bytes,
            &doc.validate().unwrap(),
            &decoded.style_store,
            &decoded.image_store,
            EncodeOptions::default(),
        )
        .unwrap()
        .bytes;
        for output in [patched, noop, edited] {
            let mut package = open_package(&output, Default::default()).unwrap();
            for name in &names {
                let index = package.entries().iter().position(|e| e.path == *name).unwrap();
                assert_eq!(package.read(index).unwrap(), payload);
                assert!(package.is_opaque(index), "{name}");
            }
            HwpxDecoder::decode(&output).unwrap();
        }
    }
}

// 이것을 실패시키는 것: 쓰기 뒤 새 CD 검증 제거, 생성 항목과 opaque의 같은 path 덮어쓰기 허용.
#[test]
fn w2_output_rechecks_collisions() {
    let bytes = rewrite(&base(), &[("Preview/./image.png", None, b"x")]);
    let input = RawPackage::read(&bytes).unwrap();
    let same = rewrite(&base(), &[("Preview/./image.png", None, b"y")]);
    assert!(RawPackage::read(&same).unwrap().carry_opaque_from(&input).is_err());
    let other = rewrite(&base(), &[("Preview/./IMAGE.PNG", None, b"y")]);
    let mut output = RawPackage::read(&other).unwrap();
    output.carry_opaque_from(&input).unwrap();
    assert!(output.write().unwrap_err().to_string().contains("case-insensitive name collision"));
}

// 이것을 실패시키는 것: layout manifest 조회의 opaque 오류를 MissingFile처럼 삼키기.
#[test]
fn w2_layout_manifest_semantic_selection_rejects_opaque() {
    let original = base();
    let raw = RawPackage::read(&original).unwrap();
    let content=raw.read_text_entry("Contents/content.hpf").unwrap().replace("</opf:manifest>",
        "<opf:item id=\"extra\" href=\"Unused/./x.bin\" media-type=\"application/octet-stream\"/></opf:manifest>");
    let bytes = rewrite(
        &original,
        &[("Contents/content.hpf", None, content.as_bytes()), ("Unused/./x.bin", None, b"x")],
    );
    HwpxDecoder::decode(&bytes).unwrap(); // manifest-connection repair remains out of scope.
    assert!(crate::layout_edit::PackageAnalysis::read(&bytes).is_err());
}

// 이것을 실패시키는 것: stamp carry 목록에서 내용 있는 폴더 항목을 빼기.
#[test]
fn w2_stamp_carry_keeps_nonempty_directory_accounted() {
    let original = base();
    let bytes = rewrite(&original, &[("Unused/", None, b"x")]);
    assert!(crate::stamp::check_zip_carry(&bytes, &original).is_err());
}

// 이것을 실패시키는 것: 캐시 정렬 실패 fallback이 opaque 의미 읽기 오류까지 삼키기.
#[test]
fn w2_layout_carry_propagates_opaque_selection() {
    let original = base();
    let decoded = HwpxDecoder::decode(&original).unwrap();
    let source = crate::layout_carry::cache_source(
        &decoded.document.validate().unwrap(),
        &decoded.style_store,
        &decoded.image_store,
    )
    .unwrap();
    let raw = RawPackage::read(&original).unwrap();
    let section = raw.read_entry("Contents/section0.xml").unwrap();
    let opaque = rewrite(
        &original,
        &[("Contents/section0.xml", None, section), ("Contents/section0.xml/", None, b"")],
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("mimetype", zip::write::SimpleFileOptions::default()).unwrap();
    zip.write_all(b"application/hwp+zip").unwrap();
    let missing = zip.finish().unwrap().into_inner();
    assert!(crate::layout_carry::carry_line_segs(&missing, &original, &original, &source).is_ok());
    let undecodable = rewrite(&original, &[("Contents/section0.xml", None, b"\xff")]);
    assert!(
        crate::layout_carry::carry_line_segs(&undecodable, &original, &original, &source).is_ok()
    );
    for (a, b, c) in [
        (&opaque, &original, &original),
        (&original, &opaque, &original),
        (&original, &original, &opaque),
    ] {
        assert!(crate::layout_carry::carry_line_segs(a, b, c, &source).is_err());
    }
}

// 이것을 실패시키는 것: mimetype 의미 조회를 대문자 항목으로 바꾸기(R3는 추가하지 않음).
#[test]
fn w2_mimetype_keeps_existing_effective_exact_selection() {
    let original = base();
    for entries in [
        vec![
            ("mimetype", None, b"application/hwp+zip".as_slice()),
            ("MIMETYPE", None, b"invalid".as_slice()),
        ],
        vec![
            ("MIMETYPE", None, b"invalid".as_slice()),
            ("mimetype", None, b"application/hwp+zip".as_slice()),
        ],
        vec![("other", Some("mimetype"), b"application/hwp+zip".as_slice())],
    ] {
        HwpxDecoder::decode(&rewrite(&original, &entries)).unwrap();
    }
}

// 이것을 실패시키는 것: layout carry가 출력 패키지 대신 캐시 source 패키지를 저장하거나 빈 opaque 폴더를 제외하기.
#[test]
fn w2_layout_carry_resave_preserves_opaque_names_bytes_and_flags() {
    use hwpforge_core::layout::{LayoutCache, LineSeg};
    for names in
        [vec!["Unused/./a.bin"], vec!["Unused/./"], vec!["Unused", "Unused/"], vec!["Unused/"]]
    {
        let decoded = HwpxDecoder::decode(&base()).unwrap();
        let mut doc = decoded.document;
        doc.sections_mut()[0].paragraphs[0].layout_cache =
            Some(LayoutCache::new(vec![LineSeg::default()]));
        let validated = doc.validate().unwrap();
        let baseline =
            HwpxEncoder::encode(&validated, &decoded.style_store, &decoded.image_store).unwrap();
        let source = crate::layout_carry::cache_source(
            &validated,
            &decoded.style_store,
            &decoded.image_store,
        )
        .unwrap();
        let payload =
            if names[0] == "Unused/./" { b"".as_slice() } else { b"opaque payload".as_slice() };
        let specs: Vec<_> = names.iter().map(|n| (*n, None, payload)).collect();
        let output = rewrite(&baseline, &specs);
        let original = HwpxEncoder::encode_with_options(
            &validated,
            &decoded.style_store,
            &decoded.image_store,
            EncodeOptions::default().with_emit_layout_cache(true),
        )
        .unwrap();
        let (saved, _) =
            crate::layout_carry::carry_line_segs(&original, &baseline, &output, &source).unwrap();
        assert_ne!(saved, output, "must exercise the writer, not the no-change return");
        assert!(RawPackage::read(&saved)
            .unwrap()
            .read_text_entry("Contents/section0.xml")
            .unwrap()
            .contains("<hp:linesegarray"));
        let prepared = crate::layout_carry::carry_prepared(&output, source).unwrap().bytes;
        assert_ne!(prepared, output);
        for saved in [saved, prepared] {
            let mut reopened = open_package(&saved, Default::default()).unwrap();
            for name in &names {
                let index = reopened.entries().iter().position(|e| e.path == *name).unwrap();
                assert_eq!(reopened.read(index).unwrap(), payload);
                assert!(reopened.is_opaque(index), "{name}");
            }
        }
    }
}

// 이것을 실패시키는 것: stamp의 carry 이름 목록에서 dot 이름을 제외하기.
#[test]
fn w2_stamp_rejects_uncarried_opaque_probe_shapes() {
    for names in [vec!["Unused/./a.bin"], vec!["Unused", "Unused/"], vec!["Unused/"]] {
        let specs: Vec<_> = names.iter().map(|n| (*n, None, b"x".as_slice())).collect();
        let bytes = rewrite(&base(), &specs);
        let error = crate::stamp::HwpxStamper::stamp(&bytes, &[]).unwrap_err();
        assert!(matches!(error, crate::stamp::StamperError::UncarriedZipEntries { .. }), "{error}");
    }
}

// 이것을 실패시키는 것: carry 목록에서 is_opaque 우선 검사를 제거하거나 정상 빈 폴더까지 필수화하기.
// stamp v1/v2 또는 cell-edit의 공용 carry 게이트 호출을 제거해도 실패한다.
#[test]
fn w2_empty_directory_carry_gate_covers_stamp_v1_v2_and_cell_edit() {
    use crate::cell_edit::{CellEditError, HwpxCellEditor};
    use crate::stamp::{HwpxStamper, StampRequestV2, StamperError, STAMP_MAP_VERSION};
    let original = base();
    for (name, opaque) in [("Unused/./", true), ("Unused/", false)] {
        let bytes = rewrite(&original, &[(name, None, b"")]);
        let package = open_package(&bytes, Default::default()).unwrap();
        let index = package.entries().iter().position(|e| e.path == name).unwrap();
        assert_eq!(package.is_opaque(index), opaque);
        crate::stamp::check_zip_carry(&bytes, &bytes).unwrap();
        let request = StampRequestV2 {
            schema_version: STAMP_MAP_VERSION,
            source_sha256: HwpxStamper::plan_bytes_v2(&bytes).unwrap().source_sha256,
            text: vec![],
            cells: vec![],
        };
        let v1 = HwpxStamper::stamp(&bytes, &[]).map(|r| r.bytes);
        let v2 = HwpxStamper::stamp_v2(&bytes, &request).map(|r| r.bytes);
        for result in [v1, v2] {
            if opaque {
                assert!(
                    matches!(result.unwrap_err(), StamperError::UncarriedZipEntries { entries } if entries == [name])
                );
            } else {
                HwpxDecoder::decode(&result.unwrap()).unwrap();
            }
        }
        // Even an empty spec executes admission, regeneration and layout carry.
        let cells = HwpxCellEditor::set_cells(&bytes, &[]);
        if opaque {
            assert!(
                matches!(cells.unwrap_err(), CellEditError::UncarriedZipEntries { entries } if entries == [name])
            );
        } else {
            HwpxDecoder::decode(&cells.unwrap().bytes).unwrap();
        }
    }
}
