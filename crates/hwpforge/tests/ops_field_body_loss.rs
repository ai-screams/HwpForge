#![cfg(feature = "ops-hwpx")]

#[path = "support/document.rs"]
mod document;
use document::editable;
#[path = "support/stamping.rs"]
mod stamping;
use stamping::stamp_all;
use std::io::{Cursor, Read, Write};

use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::hwpx::HwpxDecoder;
use hwpforge::ops::edit::{set_cell, SetCellOptions};
use hwpforge::ops::style::{restyle, RestyleOptions};

/// `bytes` with one more body paragraph whose single run holds a closed
/// bookmark field around "IMPORTANT_BODY", with or without a line cache.
fn with_field_body(bytes: &[u8], cached: bool, kind: &str, open: bool) -> Vec<u8> {
    let cache = if cached {
        r#"<hp:linesegarray><hp:lineseg textpos="0" vertpos="0" vertsize="1000" textheight="1000" baseline="850" spacing="600" horzpos="0" horzsize="40000" flags="393216"/></hp:linesegarray>"#
    } else {
        ""
    };
    let mut para = format!(
        r#"<hp:p id="0" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0"><hp:ctrl><hp:fieldBegin id="7" type="{kind}" name="b" editable="0" dirty="1" zorder="-1" fieldid="7"/></hp:ctrl><hp:t>IMPORTANT_BODY</hp:t><hp:ctrl><hp:fieldEnd beginIDRef="7" fieldid="7"/></hp:ctrl></hp:run>{cache}</hp:p>"#
    );
    if open {
        let end = r#"<hp:ctrl><hp:fieldEnd beginIDRef="7" fieldid="7"/></hp:ctrl>"#;
        para = para.replace(end, "");
        para.push_str(&format!(r#"<hp:p id="0" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0">{end}</hp:run></hp:p>"#));
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).expect("entry");
        let name = entry.name().expect("ZIP entry name").to_string();
        let options =
            zip::write::SimpleFileOptions::default().compression_method(entry.compression());
        let mut data = Vec::new();
        entry.read_to_end(&mut data).expect("read");
        if name == "Contents/section0.xml" {
            let xml = String::from_utf8(data).expect("utf-8");
            let at = xml.rfind("</hs:sec>").expect("section end");
            data = format!("{}{para}{}", &xml[..at], &xml[at..]).into_bytes();
        }
        out.start_file(name.as_str(), options).expect("start");
        out.write_all(&data).expect("write");
    }
    out.finish().expect("finish").into_inner()
}

/// 이것을 실패시키는 것: 본문 소실 보고(`ContentLost` field_body) 삭제, 또는
/// 캐시가 있을 때만 보고(캐시 없는 경우 세 편집이 통과).
#[test]
fn a_lost_field_body_is_refused_by_the_editors() {
    let source = editable();
    let cell = |b: &[u8]| {
        set_cell(b, &SetCellOptions::default().with_table(0).with_at("0,1").with_text("쇠부리"))
    };
    let opts = RestyleOptions::default().with_preset("modern");
    for cached in [false, true] {
        let lost = with_field_body(&source, cached, "UNSUPPORTED_FIELD", false);
        let decoded = HwpxDecoder::decode(&lost).expect("decode");
        assert!(
            decoded
                .warnings
                .iter()
                .any(|w| w.is_content_loss() && format!("{w:?}").contains("field_body")),
            "cached={cached}: {:?}",
            decoded.warnings
        );
        for (surface, err) in [
            ("restyle", restyle(&lost, &opts).map(|_| ()).unwrap_err()),
            ("set-cell", cell(&lost).map(|_| ()).unwrap_err()),
            ("stamp", stamp_all(&lost).map(|_| ()).unwrap_err()),
        ] {
            assert_eq!(
                err.code(),
                OpsCode::InputNotRoundtripSafe,
                "{surface} cached={cached}: {err}"
            );
            assert!(err.to_string().contains("field_body"), "{surface} cached={cached}: {err}");
        }
        // 이것을 실패시키는 것: F9 ops-md gate 제거 — ops-hwpx 전용 컴파일 실패.
        #[cfg(feature = "ops-md")]
        {
            // Reading succeeds and says what it lost.
            let md = hwpforge::ops::markdown::to_md(&lost, &Default::default()).expect("to-md");
            assert!(
                md.warnings.iter().any(|w| format!("{w:?}").contains("field_body")),
                "cached={cached}: {:?}",
                md.warnings
            );
        }
    }
    // The same document without that paragraph edits fine.
    restyle(&source, &opts).expect("plain restyle");
    cell(&source).expect("plain set-cell");
    stamp_all(&source).expect("plain stamp");
}

/// 이것을 실패시키는 것: 복구된 본문을 버리거나 field_body 경고를 계속 내기.
#[test]
fn recovered_bodies_survive_all_editors() {
    let source = editable();
    for cached in [false, true] {
        for (kind, open) in [
            ("BOOKMARK", false),
            ("FORMULA", false),
            ("MEMO", false),
            ("CLICK_HERE", true),
            ("BOOKMARK", true),
            ("FORMULA", true),
            ("MEMO", true),
        ] {
            let bytes = with_field_body(&source, cached, kind, open);
            let decoded = HwpxDecoder::decode(&bytes).unwrap();
            assert!(
                !decoded.warnings.iter().any(|w| w.is_content_loss()),
                "{kind}: {:?}",
                decoded.warnings
            );
            let _outputs = [
                restyle(&bytes, &RestyleOptions::default().with_preset("modern")).unwrap().bytes,
                set_cell(
                    &bytes,
                    &SetCellOptions::default().with_table(0).with_at("0,1").with_text("쇠부리"),
                )
                .unwrap()
                .bytes,
                stamp_all(&bytes).unwrap().bytes,
            ];
            // 이것을 실패시키는 것: F10 ops-md gate 제거 — ops-hwpx 전용 컴파일 실패.
            // All editors still run above when Markdown checks are disabled.
            #[cfg(feature = "ops-md")]
            for output in _outputs {
                let md = hwpforge::ops::markdown::to_md(&output, &Default::default()).unwrap();
                assert!(
                    md.markdown.contains(r"IMPORTANT\_BODY"),
                    "{kind} cached={cached}: {}",
                    md.markdown
                );
            }
        }
    }
}
