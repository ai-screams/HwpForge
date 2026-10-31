use hwpforge_convert::ops::{
    open_as_hwpx, ConversionReport, ConvertOpsError, ConvertOpsWarning, OpenOptions, SourceFormat,
};
use hwpforge_convert::ConvertWarning;
use hwpforge_smithy_hwp5::Hwp5Warning;
use hwpforge_smithy_hwpx::EncodeWarning;
use std::borrow::Cow;

#[test]
// 이것을 실패시키는 것: 무변환 bytes 복사 또는 차용을 소유로 전환.
fn untouched_inputs_keep_their_allocation_and_borrow() {
    let bytes = b"PK malformed, left to ops";
    let out = open_as_hwpx(bytes.as_slice(), &OpenOptions::default()).unwrap();
    assert!(matches!(out.bytes, Cow::Borrowed(_)));
    assert_eq!(out.bytes.as_ptr(), bytes.as_ptr());
    assert_eq!(out.source, SourceFormat::Hwpx);
    assert!(out.conversion.is_none());
    let owned = bytes.to_vec();
    let pointer = owned.as_ptr();
    let out = open_as_hwpx(owned, &OpenOptions::default()).unwrap();
    assert_eq!(out.bytes.as_ptr(), pointer);
}

#[test]
// 이것을 실패시키는 것: 경고 계열 또는 알 수 없는 subject 허용, cache 손실 거부, 인코더 판정 무시.
fn loss_classification_covers_every_warning_family_and_encoder_policy() {
    let warnings = vec![
        Hwp5Warning::UnsupportedTag { tag_id: 0xffff, offset: 12 },
        Hwp5Warning::SkippedStream { name: "unknown".into() },
        Hwp5Warning::DroppedControl {
            control: "nested_section_settings",
            reason: "omitted".into(),
        },
        Hwp5Warning::ProjectionFallback { subject: "picture.crop_lost", reason: "lost".into() },
        Hwp5Warning::ProjectionFallback { subject: "unclassified", reason: "unknown".into() },
        Hwp5Warning::ParserFallback { subject: "unclassified", reason: "unknown".into() },
        Hwp5Warning::ContentLost {
            kind: "group".into(),
            reason: "lost".into(),
            location: "s0".into(),
        },
    ];
    for warning in warnings {
        assert!(warning.is_semantic_loss());
        let report = ConversionReport {
            warnings: vec![ConvertOpsWarning::Convert(ConvertWarning::Hwp5(warning))],
        };
        let error = report.ensure_accepted(false).unwrap_err();
        assert_eq!(error.code().as_str(), "HWP5_CONVERSION_LOSS");
        let ConvertOpsError::ConversionLoss { warnings } = error else { panic!("loss") };
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].stage, "convert");
        report.ensure_accepted(true).unwrap();
    }
    let cache = Hwp5Warning::LayoutCacheDropped { reason: "cache".into() };
    assert!(!cache.is_semantic_loss());
    let report = ConversionReport {
        warnings: vec![
            ConvertOpsWarning::Convert(ConvertWarning::Hwp5(cache)),
            ConvertOpsWarning::Convert(ConvertWarning::HwpxEncode(
                EncodeWarning::LayoutCacheDropped {
                    path: Default::default(),
                    reason: "cache".into(),
                },
            )),
        ],
    };
    report.ensure_accepted(false).unwrap();
    let warning =
        ConvertOpsWarning::Convert(ConvertWarning::HwpxEncode(EncodeWarning::NoteHeadSkipped {
            path: Default::default(),
            reason: "heading".into(),
        }));
    assert!(warning.is_semantic_loss());
    let mut report = report;
    report.warnings.push(warning);
    let error = report.ensure_accepted(false).unwrap_err();
    let ConvertOpsError::ConversionLoss { warnings } = error else { panic!("loss") };
    assert_eq!(warnings.len(), 1);
    let unknown =
        ConvertOpsWarning::Decode(hwpforge_smithy_hwpx::DecodeWarning::LayoutCacheDropped {
            path: Default::default(),
            reason: "unknown stage".into(),
        });
    assert!(unknown.is_semantic_loss());
}

#[test]
// 이것을 실패시키는 것: convert 또는 facade 한쪽에서 ZIP 오류 reason 조건 제거.
fn conversion_and_facade_classify_the_same_input_matrix() {
    use std::io::{Cursor, Write};
    let cfb = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for name in ["first", "other"] {
        writer.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        writer.write_all(b"x").unwrap();
    }
    let zip = writer.finish().unwrap().into_inner();
    let mut policy = zip.clone();
    for index in 0..policy.len() - 4 {
        if &policy[index..index + 5] == b"other" {
            policy[index..index + 5].copy_from_slice(b"first");
        }
    }
    // A second EOCD has an unsupported disk number. Both end records are
    // candidates, but recovery cannot select the last one as a valid directory.
    let mut ambiguous_zip = [cfb.as_slice(), &zip].concat();
    let mut extra_end = [0; 22];
    extra_end[..4].copy_from_slice(b"PK\x05\x06");
    extra_end[4] = 1;
    ambiguous_zip.extend(extra_end);
    let error =
        hwpforge_core::zip_package::open_package(&ambiguous_zip, Default::default()).unwrap_err();
    assert!(
        matches!(
            &error,
            hwpforge_core::CoreError::InvalidStructure { context, reason }
                if context == "ZIP" && reason == "ambiguous ZIP end records"
        ),
        "{error:?}"
    );
    for (name, bytes, expected) in [
        ("CFB", cfb.to_vec(), true),
        ("CFB + ZIP", [cfb.as_slice(), &zip].concat(), false),
        ("CFB + rejected ZIP", [cfb.as_slice(), &policy].concat(), false),
        ("CFB + ambiguous ZIP end records", ambiguous_zip, false),
        ("ZIP", zip, false),
        ("short", cfb[..7].to_vec(), false),
        ("arbitrary", b"arbitrary bytes".to_vec(), false),
    ] {
        let facade = hwpforge::ops::fields(&bytes).unwrap_err().code().as_str() == "HWP5_INPUT";
        let convert = match open_as_hwpx(bytes.as_slice(), &OpenOptions::default()) {
            Ok(out) => out.source == SourceFormat::Hwp5,
            Err(error) => {
                assert_eq!(error.code().as_str(), "HWP5_DECODE_FAILED");
                true
            }
        };
        assert_eq!(facade, expected, "facade/{name}");
        assert_eq!(convert, expected, "convert/{name}");
    }
}

#[test]
// 이것을 실패시키는 것: 비손실 정의 경고를 conversion.warnings에서 제거하거나 다시 거부.
fn exempt_definitions_remain_in_conversion_metadata() {
    let subjects =
        [Hwp5Warning::FONT_TYPE_INFO_SUBJECT, Hwp5Warning::MEMO_SHAPE_DEFINITION_SUBJECT];
    for (i, subject) in subjects.into_iter().enumerate() {
        let warning = if i == 0 {
            Hwp5Warning::ProjectionFallback { subject, reason: "definition".into() }
        } else {
            Hwp5Warning::ParserFallback { subject, reason: "definition".into() }
        };
        assert!(!warning.is_semantic_loss(), "{subject}");
        let report = ConversionReport {
            warnings: vec![ConvertOpsWarning::Convert(ConvertWarning::Hwp5(warning))],
        };
        report.ensure_accepted(false).unwrap();
        assert_eq!(report.meta().warnings.len(), 1);
        assert!(report.meta().warnings[0].message.contains(subject));
    }
}

#[test]
// 이것을 실패시키는 것: subject만 비교하여 잘못된 경고 계열까지 비손실로 허용.
fn exempt_subjects_do_not_exempt_other_warning_families() {
    for subject in [Hwp5Warning::OUTLINE_SHAPE_SUBJECT, Hwp5Warning::MEMO_SHAPE_DEFINITION_SUBJECT]
    {
        assert!(Hwp5Warning::ProjectionFallback { subject, reason: "unknown".into() }
            .is_semantic_loss());
    }
    assert!(Hwp5Warning::ParserFallback {
        subject: Hwp5Warning::FONT_TYPE_INFO_SUBJECT,
        reason: "unknown".into()
    }
    .is_semantic_loss());
    for subject in ["style.char_shape.language", "style.para_shape.auto_spacing", "unclassified"] {
        assert!(
            Hwp5Warning::ProjectionFallback { subject, reason: "lost".into() }.is_semantic_loss()
        );
        assert!(Hwp5Warning::ParserFallback { subject, reason: "lost".into() }.is_semantic_loss());
    }
}

#[test]
// 이것을 실패시키는 것: 개요 참조 또는 변경 추적 subject를 비손실 목록에 추가.
fn outline_and_change_tracking_definitions_require_loss_acceptance() {
    for subject in
        [Hwp5Warning::OUTLINE_SHAPE_SUBJECT, Hwp5Warning::TRACK_CHANGE_DEFINITION_SUBJECT]
    {
        let warning = Hwp5Warning::ParserFallback { subject, reason: "definition omitted".into() };
        assert!(warning.is_semantic_loss(), "{subject}");
        let report = ConversionReport {
            warnings: vec![ConvertOpsWarning::Convert(ConvertWarning::Hwp5(warning))],
        };
        let ConvertOpsError::ConversionLoss { warnings } =
            report.ensure_accepted(false).unwrap_err()
        else {
            panic!("loss expected for {subject}")
        };
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains(subject));
        report.ensure_accepted(true).unwrap();
        assert_eq!(report.meta().warnings.len(), 1);
        assert!(report.meta().warnings[0].message.contains(subject));
    }
}
