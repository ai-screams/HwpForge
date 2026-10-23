//! #243: malformed lengths through the public record and document APIs.
use hwpforge_smithy_hwp5::schema::border_fill::Hwp5RawBorderFill;
use hwpforge_smithy_hwp5::{Hwp5Decoder, Hwp5Error, Hwp5Warning};

mod wire {
    include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/support/hwp5.rs"));
}

fn oversized_fill_record(kind: u32) -> Vec<u8> {
    // property (2) + five border lines (6 each), then the fill-kind word.
    let mut record = vec![0; 32];
    record.extend_from_slice(&kind.to_le_bytes());
    match kind {
        0 => {}
        1 => record.extend_from_slice(&[0; 13]), // two colors, pattern, alpha
        4 => record.extend_from_slice(&[0; 17]), // type, angle, center x/y, blur
        _ => panic!("unsupported test kind {kind}"),
    }
    record.extend_from_slice(&u32::MAX.to_le_bytes());
    record
}

/// 이것을 실패시키는 것: kind 0/1의 extra_len 또는 kind 4의 color_count
/// checked_len 검사 제거. 세 변이 각각 실제 실행에서 대형 할당 시도로 SIGABRT.
/// 변이 실행만 임시 할당기로 단일 할당을 64 MiB로 제한했다(최종 시험에는 없음).
#[test]
fn complete_border_fill_records_reject_oversized_lengths() {
    for (kind, size) in [(4, 57), (0, 40), (1, 53)] {
        let record = oversized_fill_record(kind);
        assert_eq!(record.len(), size, "kind {kind}: full record layout");
        let error = Hwp5RawBorderFill::parse(&record).unwrap_err();
        let Hwp5Error::RecordParse { offset, detail } = error else {
            panic!("kind {kind}: expected RecordParse, got {error:?}");
        };
        assert_eq!(offset, size, "kind {kind}: the declared length was reached");
        assert!(detail.contains(&format!("fill_kind=0x{kind:08X}")), "{detail}");
        assert!(detail.contains("do not fit"), "kind {kind}: {detail}");
    }
}

/// The decoder's existing contract retains a placeholder and reports a warning.
/// 이것을 실패시키는 것: kind 0/1 extra_len 또는 kind 4 color_count 검사 제거.
/// 세 변이 각각 64 MiB 임시 할당 제한 아래 실제 실행하여 SIGABRT를 확인했다.
#[test]
fn document_decode_reports_oversized_border_fill_lengths() {
    for kind in [4, 0, 1] {
        // Minimal style definitions keep the document valid apart from the fill.
        let mut info = wire::make_record(0x15, 0, &[0; 68]); // CharShape
        info.extend(wire::make_record(0x19, 0, &[0; 42])); // ParaShape
        let record = oversized_fill_record(kind);
        info.extend(wire::make_record(0x14, 0, &record)); // BorderFill
        let bytes = wire::make_test_cfb(&info, &[]);
        let decoded = Hwp5Decoder::decode(&bytes).expect("malformed fill uses a placeholder");
        let reasons: Vec<_> = decoded
            .warnings
            .iter()
            .filter_map(|warning| match warning {
                Hwp5Warning::ParserFallback { subject: "border_fill.parse", reason } => {
                    Some(reason)
                }
                _ => None,
            })
            .collect();
        assert_eq!(reasons.len(), 1, "kind {kind}: {:?}", decoded.warnings);
        let reason = reasons[0];
        for expected in [
            "border_fill_id=1".to_string(),
            format!("fill_kind=0x{kind:08X}"),
            format!("payload={} bytes", record.len()),
            "do not fit".to_string(),
        ] {
            assert!(reason.contains(&expected), "missing {expected}: {reason}");
        }
    }
}
