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
        let case = format!("raw={raw:?}, effective={:?}", effective.unwrap_or(raw));
        let mut options = FullFileOptions::default().compression_method(CompressionMethod::Stored);
        if let Some(effective) = effective {
            let mut extra = vec![1];
            extra.extend(crc32(raw.as_bytes()).to_le_bytes());
            extra.extend(effective.as_bytes());
            options
                .add_extra_field(0x7075, extra, false)
                .unwrap_or_else(|error| panic!("{case}: {error}"));
        }
        writer.start_file(*raw, options).unwrap_or_else(|error| panic!("{case}: {error}"));
        writer.write_all(payload).unwrap_or_else(|error| panic!("{case}: {error}"));
    }
    writer.finish().unwrap_or_else(|error| panic!("ZIP fixture finish: {error}")).into_inner()
}

fn zip_raw_name(raw: &[u8], effective: Option<&str>) -> Vec<u8> {
    // Patch a small fixture: ZipWriter limits the combined local header size,
    // while ZIP's name and extra-field lengths are separate u16 fields.
    let mut bytes = zip(&[("raw", None, b"x")]);
    let central = bytes.windows(4).position(|w| w == b"PK\x01\x02").expect("central header");
    let mut fields = raw.to_vec();
    if let Some(effective) = effective {
        fields.extend(0x7075u16.to_le_bytes());
        fields.extend(u16::try_from(5 + effective.len()).unwrap().to_le_bytes());
        fields.push(1);
        fields.extend(crc32(raw).to_le_bytes());
        fields.extend(effective.as_bytes());
    }
    bytes.splice(central + 46..central + 49, fields.iter().copied());
    bytes.splice(30..33, fields.iter().copied());
    let central = central + fields.len() - 3;
    let footer = central + 46 + fields.len();
    for start in [26, central + 28] {
        bytes[start..start + 2].copy_from_slice(&u16::try_from(raw.len()).unwrap().to_le_bytes());
        bytes[start + 2..start + 4]
            .copy_from_slice(&u16::try_from(fields.len() - raw.len()).unwrap().to_le_bytes());
    }
    bytes[footer + 12..footer + 16]
        .copy_from_slice(&u32::try_from(footer - central).unwrap().to_le_bytes());
    bytes[footer + 16..footer + 20].copy_from_slice(&u32::try_from(central).unwrap().to_le_bytes());
    bytes
}

// 이것을 실패시키는 것: lossy로 되돌림, 비UTF-8 표시 제거, raw/effective 표시 혼동.
#[test]
fn name_policy_diagnostics_distinguish_invalid_utf8_and_replacement_character() {
    let mut messages = Vec::new();
    for raw in [b"Unused/./\xff".as_slice(), b"Unused/./\xfe", "Unused/./�".as_bytes()] {
        for effective in [None, Some("Unused/./�")] {
            let case = format!("raw={raw:?}, effective={effective:?}");
            let bytes = zip_raw_name(raw, effective);
            let error = open_package(&bytes, Default::default()).expect_err(&case).to_string();
            let expected = match std::str::from_utf8(raw) {
                Ok(text) => format!("{text:?}"),
                Err(_) => format!("b\"{}\" (not UTF-8)", raw.escape_ascii()),
            };
            assert!(error.contains(&expected), "{case}: {error}");
            if effective.is_some() && raw != "Unused/./�".as_bytes() {
                assert!(error.contains(&format!(
                    "opaque ZIP entry has different raw/effective names: raw {expected}, effective \"Unused/./�\""
                )), "{case}: {error}");
            } else {
                assert!(
                    error.contains("opaque ZIP name exceeds portable reference evidence:"),
                    "{case}: {error}"
                );
            }
            messages.push(error);
        }
    }
    // The last two cases both have the same valid U+FFFD raw/effective name.
    for left in 0..4 {
        for right in left + 1..messages.len() {
            assert_ne!(messages[left], messages[right], "left={left}, right={right}");
        }
    }
}

// 이것을 실패시키는 것: 따옴표/개행/ESC 이스케이프 제거, 짧은 이름 생략, 같은 이름 중복 출력.
#[test]
fn name_policy_diagnostics_escape_and_show_short_names_once() {
    for name in ["Unused/./한글", "Unused/./한'\"\n\x1b"] {
        for effective in [None, Some("Other/한'\"\n\x1b")] {
            let case = format!("raw={name:?}, effective={effective:?}");
            let bytes = zip(&[(name, effective, b"x")]);
            let error = open_package(&bytes, Default::default()).expect_err(&case);
            let crate::CoreError::InvalidStructure { reason: detail, .. } = error else {
                panic!("{case}: {error}");
            };
            let expected = if let Some(effective) = effective {
                format!("opaque ZIP entry has different raw/effective names: raw {name:?}, effective {effective:?}")
            } else {
                format!("opaque ZIP name exceeds portable reference evidence: non-ASCII=true; length={} bytes, maximum=259 bytes; raw/effective {name:?}", name.len())
            };
            assert_eq!(detail, expected, "{case}");
            assert_eq!(detail.matches("Unused/./").count(), 1, "{case}: {detail}");
            assert!(!detail.contains(['\n', '\x1b']), "{case}: {detail}");
        }
    }
}

// 이것을 실패시키는 것: 표시 상한 제거, 전체 길이/생략 표시 제거, UTF-8 문자 중간 자르기.
#[test]
fn name_policy_diagnostics_bound_maximum_length_names() {
    for unit in ["a", "\x1b", "한"] {
        let name = format!("Unused/./{}", unit.repeat((65_535 - 9) / unit.len()));
        assert_eq!(name.len(), 65_535);
        // A Unicode Path extra field also needs its version, CRC, and field header.
        let alias = format!("Other/./{}", unit.repeat((65_520 - 8) / unit.len()));
        for effective in [None, Some(alias.as_str())] {
            let case = format!("unit={unit:?}, alias={}", effective.is_some());
            let bytes = zip_raw_name(name.as_bytes(), effective);
            let error = open_package(&bytes, Default::default()).expect_err(&case).to_string();
            assert!(error.len() <= 1024, "{case}: message length={}", error.len());
            assert!(error.contains("... (65535 bytes total)"), "{case}: {error}");
            assert_eq!(error.matches("Unused/./").count(), 1, "{case}: {error}");
            if let Some(effective) = effective {
                assert!(
                    error.contains(&format!("... ({} bytes total)", effective.len())),
                    "{case}: {error}"
                );
                assert!(
                    error.contains("opaque ZIP entry has different raw/effective names: raw "),
                    "{case}: {error}"
                );
            } else {
                assert!(error.contains(&format!("opaque ZIP name exceeds portable reference evidence: non-ASCII={}; length=65535 bytes, maximum=259 bytes;", !name.is_ascii())), "{case}: {error}");
            }
            if unit == "한" {
                // 9 ASCII bytes + 18 complete Hangul scalars = 63 bytes.
                assert!(
                    error.contains(&format!("\"Unused/./{}\"...", "한".repeat(18))),
                    "{case}: {error}"
                );
                assert!(!error.contains("not UTF-8"), "{case}: {error}");
            }
        }
    }
    // The invalid byte is beyond the preview: validity concerns the whole name.
    let mut raw = b"Unused/./".to_vec();
    raw.resize(65_535, b'a');
    raw[65_534] = 0xff;
    let bytes = zip_raw_name(&raw, None);
    let error = open_package(&bytes, Default::default()).unwrap_err().to_string();
    assert!(error.len() <= 1024, "message length={}", error.len());
    assert!(error.contains("(not UTF-8)... (65535 bytes total)"), "{error}");
}

// 이것을 실패시키는 것: 정상 이름 쌍 검사 제거, 순서/정확 일치/확장자/폴더 성분에 따른 검사 생략.
#[test]
fn casefold_normal_pairs_rejected_before_payload() {
    for (left, right) in [
        ("Contents/section0.xml", "Contents/SECTION0.XML"), // c1/c2
        ("Contents/Section0.xml", "Contents/SECTION0.XML"), // c3: no exact match
        ("BinData/a.png", "BinData/A.PNG"),
        ("Contents/x.xml", "CONTENTS/x.xml"),
        ("Contents/header.xml", "Contents/HEADER.XML"),
        ("Contents/content.hpf", "Contents/CONTENT.HPF"),
        ("META-INF/container.xml", "META-INF/CONTAINER.XML"),
        ("Contents/masterpage0.xml", "Contents/MASTERPAGE0.XML"),
        ("Chart/a.xml", "Chart/A.XML"),
        ("settings.xml", "SETTINGS.XML"),
        ("Unused/", "UNUSED/"),
    ] {
        for (first, second) in [(left, right), (right, left)] {
            let case = format!("ordered raw/effective pair: first={first:?}, second={second:?}");
            let mut bytes = zip(&[(first, None, b"x"), (second, None, b"y")]);
            // A bad payload CRC must not mask the metadata-only collision error.
            bytes[30 + first.len()] ^= 1;
            let err = open_package(&bytes, Default::default()).expect_err(&case);
            assert!(
                matches!(&err, crate::CoreError::InvalidStructure { context, .. } if context == "ZIP package"),
                "{case}"
            );
            let message = err.to_string();
            assert!(message.contains("case-insensitive name collision"), "{case}: {message}");
            assert!(message.contains(left) && message.contains(right), "{case}: {message}");
            assert!(message.contains("Hancom and this reader"), "{case}: {message}");
        }
    }
}

// 이것을 실패시키는 것: effective 키 색인 누락 또는 raw 키 색인 누락, 정상 alias 교차 허용.
#[test]
fn casefold_normal_alias_crosses_rejected() {
    for left in [
        ("Contents/section0.xml", Some("Other/left")),
        ("Other/left", Some("Contents/section0.xml")),
    ] {
        for right in [
            ("Contents/SECTION0.XML", Some("Other/right")),
            ("Other/right", Some("Contents/SECTION0.XML")),
        ] {
            for (first, second) in [(left, right), (right, left)] {
                let case =
                    format!("ordered raw/effective pair: first={first:?}, second={second:?}");
                let bytes = zip(&[(first.0, first.1, b"x"), (second.0, second.1, b"y")]);
                let error = open_package(&bytes, Default::default()).expect_err(&case).to_string();
                assert!(error.contains("case-insensitive name collision"), "{case}: {error}");
                for name in [
                    first.0,
                    first.1.unwrap_or_else(|| panic!("{case}: missing first effective name")),
                    second.0,
                    second.1.unwrap_or_else(|| panic!("{case}: missing second effective name")),
                ] {
                    assert!(error.contains(name), "{case}: {error}");
                }
            }
        }
    }
    // Previously accepted raw/effective equality across two physical owners.
    let case = "raw/effective: alias/normal, other/alias".to_owned();
    let bytes = zip(&[("alias", Some("normal"), b"x"), ("other", Some("alias"), b"y")]);
    assert!(
        open_package(&bytes, Default::default())
            .expect_err(&case)
            .to_string()
            .contains("case-insensitive name collision"),
        "{case}"
    );
}

// 이것을 실패시키는 것: dot/빈 성분/끝 slash 축약 충돌/내용 있는 폴더의 전역 거부 복원.
#[test]
fn name_policy_isolated_names_and_directory_payloads_are_readable() {
    for names in [
        vec!["Unused/./a.bin"],
        vec!["./Unused/a.bin"],
        vec!["Unused//a.bin"],
        vec!["../Unused/a.bin"],
        vec!["Unused/.."],
        vec!["Unused/a.bin/", "Unused/a.bin"],
        vec!["Unused/"],
    ] {
        let case = format!("raw/effective names={names:?}");
        let specs: Vec<_> = names.iter().map(|n| (*n, None, b"x".as_slice())).collect();
        let bytes = zip(&specs);
        let mut package = open_package(&bytes, Default::default())
            .unwrap_or_else(|error| panic!("{case}: {error}"));
        for (index, name) in names.iter().enumerate() {
            let case = format!("{case}, index={index}, name={name:?}");
            assert!(package.is_opaque(index), "{case}");
            assert!(!package.is_opaque(usize::MAX), "{case}");
            assert_eq!(package.entries()[index].path, *name, "{case}");
            assert_eq!(
                package.read(index).unwrap_or_else(|error| panic!("{case}: {error}")),
                b"x",
                "{case}"
            );
        }
    }
}

// 이것을 실패시키는 것: raw 또는 effective T 불변 검사 생략, NUL/숫자 drive/부모/구분자 검사 제거.
#[test]
fn name_policy_transform_rejected_for_every_entry_before_payload() {
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
            let case = format!("raw={raw:?}, effective={:?}", effective.unwrap_or(raw));
            let mut bytes = zip(&[(raw, effective, b"x")]);
            // Corrupt only payload: rejection must be metadata-only, before CRC reading.
            let payload = 30
                + raw.len()
                + u16::from_le_bytes(
                    bytes[28..30].try_into().unwrap_or_else(|error| panic!("{case}: {error}")),
                ) as usize;
            bytes[payload] ^= 1;
            let err = open_package(&bytes, Default::default()).expect_err(&case);
            assert!(
                err.to_string().contains("invalid package part name"),
                "{case}: {raw:?}: {err}"
            );
        }
    }
}

// 이것을 실패시키는 것: opaque raw/effective 불일치 거부 삭제 또는 K 검사 삭제.
// alias 행렬은 선행 불일치 오류, dot 행렬은 K 충돌 오류를 각각 단언한다.
#[test]
fn name_policy_collisions_cover_all_raw_effective_crosses_and_orders() {
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
                    let case = format!("path={path:?}, left_extra={left_extra}, right_extra={right_extra}, ordered raw/effective entries={entries:?}");
                    let bytes = zip(&entries);
                    assert!(
                        open_package(&bytes, Default::default())
                            .expect_err(&case)
                            .to_string()
                            .contains("opaque ZIP entry has different raw/effective names"),
                        "{case}"
                    );
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
            let case = format!("ordered raw/effective entries={entries:?}");
            let bytes = zip(&entries);
            assert!(
                open_package(&bytes, Default::default())
                    .expect_err(&case)
                    .to_string()
                    .contains("case-insensitive name collision"),
                "{case}"
            );
        }
    }
}

// 이것을 실패시키는 것: 정상 항목의 raw 키 색인 누락 또는 effective 키 색인 누락.
#[test]
fn name_policy_normal_alias_keys_collide_with_opaque_pair_in_both_orders() {
    for (raw, effective) in [("UNUSED/A", "Other/normal"), ("Other/normal", "UNUSED/A")] {
        let case = format!("raw={raw:?}, effective={effective:?}");
        let normal = (raw, Some(effective), b"normal".as_slice());
        let file = ("Unused/a", None, b"x".as_slice());
        let directory = ("Unused/a/", None, b"".as_slice());
        // The alias alone is normal; the slash pair alone is accepted and opaque.
        let normal_bytes = zip(&[normal]);
        assert!(
            !open_package(&normal_bytes, Default::default())
                .unwrap_or_else(|error| panic!("{case}: {error}"))
                .is_opaque(0),
            "{case}"
        );
        let pair_bytes = zip(&[file, directory]);
        let pair = open_package(&pair_bytes, Default::default())
            .unwrap_or_else(|error| panic!("{case}: {error}"));
        assert!(pair.is_opaque(0) && pair.is_opaque(1), "{case}");
        for entries in [[normal, file, directory], [directory, file, normal]] {
            let case = format!("ordered raw/effective entries={entries:?}");
            let bytes = zip(&entries);
            let error = open_package(&bytes, Default::default()).expect_err(&case);
            assert!(
                error.to_string().contains("case-insensitive name collision"),
                "{case}: {raw} -> {effective}: {error}"
            );
        }
    }
}

// 이것을 실패시키는 것: mimetype 예외 제거 또는 같은 물리 소유자의 두 키를 충돌로 세기.
#[test]
fn name_policy_mimetype_case_group_and_self_keys_stay_accepted() {
    for entries in [
        vec![("mimetype", None, b"x".as_slice()), ("MIMETYPE", None, b"y".as_slice())],
        vec![("MIMETYPE", None, b"y".as_slice()), ("mimetype", None, b"x".as_slice())],
        vec![("Unused/a", Some("UNUSED/A"), b"x".as_slice())],
        vec![("Unused/./a", Some("Unused/./a"), b"x".as_slice())],
        vec![("한글/정상", None, b"x".as_slice())],
    ] {
        let case = format!("ordered raw/effective entries={entries:?}");
        let bytes = zip(&entries);
        assert!(open_package(&bytes, Default::default()).is_ok(), "{case}");
    }
}

// 이것을 실패시키는 것: raw 문법 판정을 제거하거나 고립된 정상 alias를 opaque로 세기.
#[test]
fn name_policy_opaque_flags_keep_raw_evidence_and_normal_aliases() {
    let bytes = zip(&[
        ("Unused/./a", None, b"x"),
        ("alias", Some("normal"), b"x"),
        ("other", Some("separate"), b"x"),
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
fn name_policy_new_names_require_portable_evidence() {
    for (case, name, reason) in [
        (
            "non-ASCII",
            "Unused/./한글".to_owned(),
            "non-ASCII=true; length=15 bytes, maximum=259 bytes",
        ),
        (
            "length",
            format!("Unused/./{}", "a".repeat(251)),
            "non-ASCII=false; length=260 bytes, maximum=259 bytes",
        ),
    ] {
        let case = format!("{case}, raw={name:?}, effective={name:?}");
        let bytes = zip(&[(&name, None, b"x")]);
        let error = open_package(&bytes, Default::default()).expect_err(&case).to_string();
        assert!(
            error.contains(&format!(
                "opaque ZIP name exceeds portable reference evidence: {reason}; raw/effective "
            )),
            "{case}: {error}"
        );
    }
    let case = "ASCII opaque boundary: 259 bytes".to_owned();
    let short = format!("Unused/./{}", "a".repeat(250));
    assert_eq!(short.len(), 259, "{case}");
    let bytes = zip(&[(&short, None, b"x")]);
    assert!(open_package(&bytes, Default::default()).is_ok(), "{case}");
}

// 이것을 실패시키는 것: source_xml에서 opaque 표시 검사를 제거하기.
#[test]
fn name_policy_source_provenance_rejects_opaque() {
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
fn name_policy_empty_directory_file_collision_marks_both_owners() {
    for entries in [
        [("Unused/a/", None, b"".as_slice()), ("Unused/a", None, b"x".as_slice())],
        [("Unused/a", None, b"x".as_slice()), ("Unused/a/", None, b"".as_slice())],
    ] {
        let case = format!("ordered raw/effective entries={entries:?}");
        let bytes = zip(&entries);
        let p = open_package(&bytes, Default::default())
            .unwrap_or_else(|error| panic!("{case}: {error}"));
        assert!(p.is_opaque(0), "{case}");
        assert!(p.is_opaque(1), "{case}");
    }
}

// 이것을 실패시키는 것: target 부재/빈 payload/폴더 속성/CD 뒤 순서에서 T 검사를 생략하기.
#[test]
fn name_policy_alias_target_orders_and_empty_directory_do_not_escape() {
    for target in ["Contents/section0.xml", "Preview/image1.png", "Preview/app.xml"] {
        for suffix in ["", "/"] {
            let alias = format!("Unused/../{target}{suffix}");
            for entries in [
                [(alias.as_str(), None, b"".as_slice()), (target, None, b"x".as_slice())],
                [(target, None, b"x".as_slice()), (alias.as_str(), None, b"".as_slice())],
            ] {
                let case = format!("ordered raw/effective entries={entries:?}");
                let bytes = zip(&entries);
                assert!(open_package(&bytes, Default::default()).is_err(), "{case}");
            }
        }
    }
}

// 이것을 실패시키는 것: raw == effective에도 K 키를 두 개 생성하거나 alias 키를 생략하기.
#[test]
fn name_policy_equal_names_build_one_folded_key() {
    let prefix = "a".repeat(4091);
    let long = format!("{prefix}00000");
    for (specs, expected) in [
        (vec![(long.as_str(), None, b"".as_slice())], 1),
        (vec![("same", Some("same"), b"".as_slice())], 1),
        (vec![("normal", None, b"".as_slice()), ("alias", Some("effective"), b"")], 3),
        (vec![("mimetype", None, b"".as_slice()), ("MIMETYPE", None, b"")], 2),
    ] {
        let case = format!("raw/effective specs={specs:?}, expected keys={expected}");
        let bytes = zip(&specs);
        let package = open_package(&bytes, Default::default())
            .unwrap_or_else(|error| panic!("{case}: {error}"));
        let (records, keys, _) = names::WORK.get();
        assert_eq!(records, specs.len(), "{case}");
        assert_eq!(keys, expected, "{case}");
        assert!((0..records).all(|i| !package.is_opaque(i)), "{case}");
    }
}

// 이것을 실패시키는 것: K 구축을 첫 4,500개로 제한하거나 끝의 충돌을 검사하지 않기.
#[test]
fn name_policy_name_work_is_bounded_for_long_prefixes() {
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
fn name_policy_utf16_second_unit_matches_reference() {
    for name in ["😀:Unused", "😀", "한글/정상", "Unused/😀:x"] {
        for (raw, effective) in
            [(name, None), ("Unused/alias", Some(name)), (name, Some("Unused/alias"))]
        {
            let case = format!("raw={raw:?}, effective={:?}", effective.unwrap_or(raw));
            let bytes = zip(&[(raw, effective, b"x")]);
            let mut package = open_package(&bytes, Default::default())
                .unwrap_or_else(|error| panic!("{case}: {error}"));
            assert!(!package.is_opaque(0), "{case}");
            assert_eq!(
                package.read(0).unwrap_or_else(|error| panic!("{case}: {error}")),
                b"x",
                "{case}"
            );
        }
    }
    for name in ["1:Unused", "한:Unused", "a:x"] {
        let case = format!("raw={name:?}, effective={name:?}");
        let bytes = zip(&[(name, None, b"x")]);
        assert!(
            open_package(&bytes, Default::default())
                .expect_err(&case)
                .to_string()
                .contains("invalid package part name"),
            "{case}"
        );
    }
}

// 이것을 실패시키는 것: opaque raw/effective 동등 검사 삭제, 정상 alias까지 거부,
// 또는 끝 slash 충돌 표시가 확정되기 전에만 검사하기.
#[test]
fn name_policy_opaque_requires_identical_raw_effective_names() {
    for (raw, effective) in [
        ("Unused/original.bin", "Unused/./opaque.bin"), // 별칭 경로 반례 A
        ("Unused/./alias", "Unused/normal"),            // 별칭 경로 반례 B
        ("Unused/./a", "UNUSED/./A"),
        ("Unused/", "Unused/normal"),
        ("Unused/normal", "Unused/"),
    ] {
        let case = format!("raw={raw:?}, effective={effective:?}");
        let bytes = zip(&[(raw, Some(effective), b"x")]);
        assert!(open_package(&bytes, Default::default())
            .expect_err(&case)
            .to_string()
            .contains(&format!("opaque ZIP entry has different raw/effective names: raw {raw:?}, effective {effective:?}")), "{case}");
    }
    // Empty directory/file collision marks the earlier owner only after its visit.
    for entries in [
        [("alias", Some("Unused"), b"x".as_slice()), ("Unused/", None, b"".as_slice())],
        [("Unused/", None, b"".as_slice()), ("alias", Some("Unused"), b"x".as_slice())],
    ] {
        let case = format!("ordered raw/effective entries={entries:?}");
        assert!(
            open_package(&zip(&entries), Default::default())
                .expect_err(&case)
                .to_string()
                .contains("different raw/effective names"),
            "{case}"
        );
    }
    for name in ["Unused/./a", "Unused/"] {
        let case = format!("raw={name:?}, effective={name:?}");
        let bytes = zip(&[(name, Some(name), b"x")]);
        assert!(
            open_package(&bytes, Default::default())
                .unwrap_or_else(|error| panic!("{case}: {error}"))
                .is_opaque(0),
            "{case}"
        );
    }
    let case = "raw=Unused/original, effective=Unused/normal".to_owned();
    let bytes = zip(&[("Unused/original", Some("Unused/normal"), b"x")]);
    assert!(
        !open_package(&bytes, Default::default())
            .unwrap_or_else(|error| panic!("{case}: {error}"))
            .is_opaque(0),
        "{case}"
    );
}
