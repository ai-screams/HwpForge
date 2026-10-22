use super::*;
use std::io::Write;
use zip::{write::FullFileOptions, ZipWriter};

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

fn zip(entries: &[(&str, Option<&str>, &[u8])]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (raw, effective, payload) in entries {
        let mut options = FullFileOptions::default().compression_method(CompressionMethod::Stored);
        if let Some(effective) = effective {
            let mut extra = vec![1];
            extra.extend(crc32(raw.as_bytes()).to_le_bytes());
            extra.extend(effective.as_bytes());
            options.add_extra_field(0x7075, extra, false).unwrap();
        }
        writer.start_file(*raw, options).unwrap();
        writer.write_all(payload).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

// 이것을 실패시키는 것: dot/빈 성분/끝 slash 축약 충돌/내용 있는 폴더의 전역 거부 복원.
#[test]
fn w2_isolated_names_and_directory_payloads_are_readable() {
    for names in [
        vec!["Unused/./a.bin"],
        vec!["./Unused/a.bin"],
        vec!["Unused//a.bin"],
        vec!["../Unused/a.bin"],
        vec!["Unused/.."],
        vec!["Unused/a.bin/", "Unused/a.bin"],
        vec!["Unused/"],
    ] {
        let specs: Vec<_> = names.iter().map(|n| (*n, None, b"x".as_slice())).collect();
        let bytes = zip(&specs);
        let mut package = open_package(&bytes, Default::default()).unwrap();
        for (index, name) in names.iter().enumerate() {
            assert!(package.is_opaque(index));
            assert!(!package.is_opaque(usize::MAX));
            assert_eq!(package.entries()[index].path, *name);
            assert_eq!(package.read(index).unwrap(), b"x");
        }
    }
}

// 이것을 실패시키는 것: raw 또는 effective T 불변 검사 생략, NUL/숫자 drive/부모/구분자 검사 제거.
#[test]
fn w2_transform_rejected_for_every_entry_before_payload() {
    for name in [
        "Unused/../Contents/section0.xml",
        "Unused/../Preview/image1.png",
        "Unused/../Preview/app.xml",
        "Unused/../Contents/content.hpf",
        "1:Contents/a",
        "한:Contents/a",
        "Unused/a\0suffix",
        "/mimetype",
        "\\a",
        "a\\b",
        "a\\..\\b",
        "a\\../b",
        "a/..\\b",
        "a/../b",
    ] {
        for (raw, effective) in [
            (name, None),
            (name, Some("Unused/./c.bin")),
            ("Unused/./c.bin", Some(name)),
            (name, Some("Unused/c.bin")),
        ] {
            let mut bytes = zip(&[(raw, effective, b"x")]);
            // Corrupt only payload: rejection must be metadata-only, before CRC reading.
            let payload =
                30 + raw.len() + u16::from_le_bytes(bytes[28..30].try_into().unwrap()) as usize;
            bytes[payload] ^= 1;
            let err = open_package(&bytes, Default::default()).unwrap_err();
            assert!(err.to_string().contains("invalid package part name"), "{raw:?}: {err}");
        }
    }
}

// 이것을 실패시키는 것: opaque raw/effective 불일치 거부 삭제 또는 K 검사 삭제.
// alias 행렬은 선행 불일치 오류, dot 행렬은 K 충돌 오류를 각각 단언한다.
#[test]
fn w2_collisions_cover_all_raw_effective_crosses_and_orders() {
    for path in ["Preview/image1.png", "Preview/app.xml", "Unused/"] {
        let upper = path.to_ascii_uppercase();
        for left_extra in [false, true] {
            for right_extra in [false, true] {
                let left = if left_extra {
                    ("Unused/./left", Some(path), b"".as_slice())
                } else {
                    (path, None, b"".as_slice())
                };
                let right = if right_extra {
                    ("Unused/./right", Some(upper.as_str()), b"x".as_slice())
                } else {
                    (upper.as_str(), Some("Unused/./right"), b"x".as_slice())
                };
                for entries in [[left, right], [right, left]] {
                    let bytes = zip(&entries);
                    assert!(open_package(&bytes, Default::default())
                        .unwrap_err()
                        .to_string()
                        .contains("opaque ZIP entry has different raw/effective names"));
                }
            }
        }
    }
    for path in ["Preview/./image1.png", "Preview/./app.xml"] {
        let upper = path.to_ascii_uppercase();
        for entries in [
            [(path, None, b"".as_slice()), (upper.as_str(), None, b"x".as_slice())],
            [(upper.as_str(), None, b"x".as_slice()), (path, None, b"".as_slice())],
        ] {
            let bytes = zip(&entries);
            assert!(open_package(&bytes, Default::default())
                .unwrap_err()
                .to_string()
                .contains("opaque name collision"));
        }
    }
}

// 이것을 실패시키는 것: 정상 항목의 raw 키 색인 누락 또는 effective 키 색인 누락.
#[test]
fn w2_normal_alias_keys_collide_with_opaque_pair_in_both_orders() {
    for (raw, effective) in [("UNUSED/A", "Other/normal"), ("Other/normal", "UNUSED/A")] {
        let normal = (raw, Some(effective), b"normal".as_slice());
        let file = ("Unused/a", None, b"x".as_slice());
        let directory = ("Unused/a/", None, b"".as_slice());
        // The alias alone is normal; the slash pair alone is accepted and opaque.
        let normal_bytes = zip(&[normal]);
        assert!(!open_package(&normal_bytes, Default::default()).unwrap().is_opaque(0));
        let pair_bytes = zip(&[file, directory]);
        let pair = open_package(&pair_bytes, Default::default()).unwrap();
        assert!(pair.is_opaque(0) && pair.is_opaque(1));
        for entries in [[normal, file, directory], [directory, file, normal]] {
            let bytes = zip(&entries);
            let error = open_package(&bytes, Default::default()).unwrap_err();
            assert!(
                error.to_string().contains("opaque name collision"),
                "{raw} -> {effective}: {error}"
            );
        }
    }
}

// 이것을 실패시키는 것: 정상 이름끼리 K 충돌 거부 또는 같은 물리 소유자의 두 키를 충돌로 세기.
#[test]
fn w2_normal_case_groups_and_self_keys_stay_accepted() {
    for entries in [
        vec![("mimetype", None, b"x".as_slice()), ("MIMETYPE", None, b"y".as_slice())],
        vec![("MIMETYPE", None, b"y".as_slice()), ("mimetype", None, b"x".as_slice())],
        vec![("Unused/a", Some("UNUSED/A"), b"x".as_slice())],
        vec![("Unused/./a", Some("Unused/./a"), b"x".as_slice())],
        vec![("한글/정상", None, b"x".as_slice())],
    ] {
        let bytes = zip(&entries);
        assert!(open_package(&bytes, Default::default()).is_ok());
    }
}

// 이것을 실패시키는 것: raw 문법 판정을 제거하거나 기존 정상 raw/effective 교차를 폴더 축약 충돌로 세기.
#[test]
fn w2_opaque_flags_keep_raw_evidence_and_normal_aliases() {
    let bytes = zip(&[
        ("Unused/./a", None, b"x"),
        ("alias", Some("normal"), b"x"),
        ("other", Some("alias"), b"x"),
        ("Empty/", None, b""),
    ]);
    let p = open_package(&bytes, Default::default()).unwrap();
    assert!(p.is_opaque(0));
    assert!(!p.is_opaque(1));
    assert!(!p.is_opaque(2));
    assert!(!p.is_opaque(3));
    assert!(!p.is_opaque(4));
}

// 이것을 실패시키는 것: opaque 비ASCII 또는 참조 고정 버퍼 경계 보류를 제거하기.
#[test]
fn w2_new_names_require_portable_evidence() {
    for name in ["Unused/./한글".to_owned(), format!("Unused/./{}", "a".repeat(251))] {
        let bytes = zip(&[(&name, None, b"x")]);
        assert!(open_package(&bytes, Default::default())
            .unwrap_err()
            .to_string()
            .contains("portable reference evidence"));
    }
    let short = format!("Unused/./{}", "a".repeat(250));
    assert_eq!(short.len(), 259);
    let bytes = zip(&[(&short, None, b"x")]);
    assert!(open_package(&bytes, Default::default()).is_ok());
}

// 이것을 실패시키는 것: source_xml에서 opaque 표시 검사를 제거하기.
#[test]
fn w2_source_provenance_rejects_opaque() {
    use crate::node_path::CorePath;
    use crate::source_xml::{UnclosedOrigin, XmlSource};
    let xml = "<sec><p><run><fieldBegin id=\"open\"/></run></p></sec>";
    let (_, source) = XmlSource::read(xml).unwrap();
    let origin = source
        .unclosed_origin(4, CorePath::new(0, "paragraphs[0].runs[0]"), "Unused/./section.xml")
        .unwrap();
    let bytes = zip(&[("Unused/./section.xml", None, xml.as_bytes())]);
    assert!(!UnclosedOrigin::verify_package_batch(&[origin], &bytes));
}

// 이것을 실패시키는 것: 끝 slash 축약 충돌에서 어느 한 소유자의 opaque 표시를 빼기.
#[test]
fn w2_empty_directory_file_collision_marks_both_owners() {
    for entries in [
        [("Unused/a/", None, b"".as_slice()), ("Unused/a", None, b"x".as_slice())],
        [("Unused/a", None, b"x".as_slice()), ("Unused/a/", None, b"".as_slice())],
    ] {
        let bytes = zip(&entries);
        let p = open_package(&bytes, Default::default()).unwrap();
        assert!(p.is_opaque(0));
        assert!(p.is_opaque(1));
    }
}

// 이것을 실패시키는 것: target 부재/빈 payload/폴더 속성/CD 뒤 순서에서 T 검사를 생략하기.
#[test]
fn w2_alias_target_orders_and_empty_directory_do_not_escape() {
    for target in ["Contents/section0.xml", "Preview/image1.png", "Preview/app.xml"] {
        for suffix in ["", "/"] {
            let alias = format!("Unused/../{target}{suffix}");
            for entries in [
                [(alias.as_str(), None, b"".as_slice()), (target, None, b"x".as_slice())],
                [(target, None, b"x".as_slice()), (alias.as_str(), None, b"".as_slice())],
            ] {
                let bytes = zip(&entries);
                assert!(open_package(&bytes, Default::default()).is_err());
            }
        }
    }
}

// 이것을 실패시키는 것: 내용 있는 폴더를 정상 항목으로 분류하기.
#[test]
fn w2_raw_nonempty_directory_is_opaque() {
    let bytes = zip(&[("Unused/", None, b"x")]);
    assert!(open_package(&bytes, Default::default()).unwrap().is_opaque(0));
}

// 이것을 실패시키는 것: raw == effective에도 K 키를 두 개 생성하거나 alias 키를 생략하기.
#[test]
fn w2_equal_names_build_one_folded_key() {
    let prefix = "a".repeat(4091);
    let long = format!("{prefix}00000");
    for (specs, expected) in [
        (vec![(long.as_str(), None, b"".as_slice())], 1),
        (vec![("same", Some("same"), b"".as_slice())], 1),
        (vec![("normal", None, b"".as_slice()), ("alias", Some("effective"), b"")], 3),
        (vec![("mimetype", None, b"".as_slice()), ("MIMETYPE", None, b"")], 2),
    ] {
        let bytes = zip(&specs);
        let package = open_package(&bytes, Default::default()).unwrap();
        let (records, keys, _) = names::WORK.get();
        assert_eq!(records, specs.len());
        assert_eq!(keys, expected);
        assert!((0..records).all(|i| !package.is_opaque(i)));
    }
}

// 이것을 실패시키는 것: K 구축을 첫 4,500개로 제한하거나 끝의 충돌을 검사하지 않기.
#[test]
fn w2_name_work_is_bounded_for_long_prefixes() {
    for count in [4500, 9000] {
        let prefix = "a".repeat(180);
        let names: Vec<_> = (0..count).map(|i| format!("{prefix}/raw{i:05}")).collect();
        let effective: Vec<_> = (0..count).map(|i| format!("{prefix}/effective{i:05}")).collect();
        let specs: Vec<_> = names
            .iter()
            .zip(&effective)
            .map(|(a, b)| (a.as_str(), Some(b.as_str()), b"".as_slice()))
            .collect();
        let bytes = zip(&specs);
        assert_eq!(open_package(&bytes, Default::default()).unwrap().entries().len(), count);
        let (records, keys, compared) = names::WORK.get();
        assert_eq!(records, count);
        assert_eq!(keys, 2 * count);
        let total_name_bytes: usize = names.iter().chain(&effective).map(String::len).sum();
        assert!(compared < total_name_bytes * 64);
        eprintln!(
            "W2 name work N={count}, B={total_name_bytes}, keys={keys}, compared_bytes={compared}"
        );
        let upper = effective[count - 1].to_ascii_uppercase();
        let mut attack = specs;
        let directory = format!("{upper}/");
        attack.push((&upper, None, b"x"));
        attack.push((&directory, None, b""));
        assert!(open_package(&zip(&attack), Default::default()).is_err());
    }
}

// 이것을 실패시키는 것: UTF-16 코드 단위 대신 char 위치로 두 번째 콜론을 검사하기.
#[test]
fn w2_utf16_second_unit_matches_reference() {
    for name in ["😀:Unused", "😀", "한글/정상", "Unused/😀:x"] {
        for (raw, effective) in
            [(name, None), ("Unused/alias", Some(name)), (name, Some("Unused/alias"))]
        {
            let bytes = zip(&[(raw, effective, b"x")]);
            let mut package = open_package(&bytes, Default::default()).unwrap();
            assert!(!package.is_opaque(0));
            assert_eq!(package.read(0).unwrap(), b"x");
        }
    }
    for name in ["1:Unused", "한:Unused", "a:x"] {
        let bytes = zip(&[(name, None, b"x")]);
        assert!(open_package(&bytes, Default::default())
            .unwrap_err()
            .to_string()
            .contains("invalid package part name"));
    }
}

// 이것을 실패시키는 것: opaque raw/effective 동등 검사 삭제, 정상 alias까지 거부,
// 또는 끝 slash 충돌 표시가 확정되기 전에만 검사하기.
#[test]
fn w2_opaque_requires_identical_raw_effective_names() {
    for (raw, effective) in [
        ("Unused/original.bin", "Unused/./opaque.bin"), // 리뷰 반례 A
        ("Unused/./alias", "Unused/normal"),            // 리뷰 반례 B
        ("Unused/./a", "UNUSED/./A"),
        ("Unused/", "Unused/normal"),
        ("Unused/normal", "Unused/"),
    ] {
        let bytes = zip(&[(raw, Some(effective), b"x")]);
        assert!(open_package(&bytes, Default::default())
            .unwrap_err()
            .to_string()
            .contains("different raw/effective names"));
    }
    // Empty directory/file collision marks the earlier owner only after its visit.
    for entries in [
        [("alias", Some("Unused"), b"x".as_slice()), ("Unused/", None, b"".as_slice())],
        [("Unused/", None, b"".as_slice()), ("alias", Some("Unused"), b"x".as_slice())],
    ] {
        assert!(open_package(&zip(&entries), Default::default())
            .unwrap_err()
            .to_string()
            .contains("different raw/effective names"));
    }
    for name in ["Unused/./a", "Unused/"] {
        let bytes = zip(&[(name, Some(name), b"x")]);
        assert!(open_package(&bytes, Default::default()).unwrap().is_opaque(0));
    }
    let bytes = zip(&[("Unused/original", Some("Unused/normal"), b"x")]);
    assert!(!open_package(&bytes, Default::default()).unwrap().is_opaque(0));
}
