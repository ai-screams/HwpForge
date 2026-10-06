//! HWP5 shape outlines and fills against Hancom's own HWPX save of the same
//! file (x6, §19.18): F1 colour + hatch, F2 colour, F3 linear gradient, F4
//! conical gradient (centre 30/20, 100 steps, step centre 70), F5 picture
//! fill whose image nothing else uses.

use std::io::{Cursor, Read};
use std::path::PathBuf;

use hwpforge_convert::{hwp5_to_hwpx_bytes_with_diagnostics, ConvertOptions};
use hwpforge_core::control::{Control, Fill};
use hwpforge_core::run::RunContent;
use hwpforge_foundation::ImageFillMode;
use hwpforge_smithy_hwpx::HwpxDecoder;

fn fixture(name: &str) -> Vec<u8> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/shapes").join(name);
    std::fs::read(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

fn section0(hwpx: &[u8]) -> String {
    let mut archive = zip::ZipArchive::new(Cursor::new(hwpx)).expect("zip");
    let mut xml = String::new();
    archive.by_name("Contents/section0.xml").expect("section").read_to_string(&mut xml).unwrap();
    xml
}

/// Each shape's `lineShape` and `fillBrush`, in order, with the picture
/// reference blanked (the two files name the same image differently).
fn looks(xml: &str) -> Vec<String> {
    xml.match_indices("<hp:lineShape ")
        .map(|(at, _)| {
            let rest = &xml[at..];
            let line = &rest[..rest.find("/>").expect("line end")];
            let fill = rest
                .find("<hc:fillBrush>")
                .map(|f| &rest[f..f + rest[f..].find("</hc:fillBrush>").expect("fill end")])
                .unwrap_or("");
            let fill = match fill.find("binaryItemIDRef=\"") {
                Some(r) => {
                    let start = r + "binaryItemIDRef=\"".len();
                    let end = start + fill[start..].find('"').unwrap();
                    format!("{}{}", &fill[..start], &fill[end..])
                }
                None => fill.to_string(),
            };
            format!("{line} | {fill}")
        })
        .collect()
}

/// 1b-T3/T4/T5: the five rectangles convert with Hancom's outline and fill,
/// attribute for attribute, and the picture fill's image is in the package.
///
/// 이것을 실패시키는 것: 선·채우기 투영 삭제(전부 흰 칠·기본 선) / BGR
/// 변환 누락(F1 `#00FFFF`) / 무늬 −1 을 0 으로 / 그러데이션 형 표 한 줄
/// 교체·색 순서 뒤집기·추가 속성(단계 중심) 무시 / 그림 자산 보충 제외.
#[test]
fn x6_fills_match_hancom() {
    let (hwpx, warnings) =
        hwp5_to_hwpx_bytes_with_diagnostics(&fixture("x6-fill.hwp"), ConvertOptions::default())
            .expect("convert");
    assert!(warnings.is_empty(), "{warnings:?}");
    let ours = looks(&section0(&hwpx));
    let hancom = looks(&section0(&fixture("x6-fill.hwpx")));
    assert_eq!(hancom.len(), 5);
    assert_eq!(ours, hancom);

    let reference = {
        let xml = section0(&hwpx);
        let at = xml.find("binaryItemIDRef=\"").expect("picture fill") + 17;
        xml[at..at + xml[at..].find('"').unwrap()].to_string()
    };
    let mut zip = zip::ZipArchive::new(Cursor::new(&hwpx)).expect("zip");
    let packaged: Vec<String> =
        (0..zip.len()).map(|i| zip.by_index(i).unwrap().name().into()).collect();
    assert!(
        packaged.iter().any(|n| n.starts_with(&format!("BinData/{reference}."))),
        "{reference} not in {packaged:?}"
    );
}

/// 1b-T5: HWP5 → HWPX → HWPX decode gives the picture fill back.
/// 이것을 실패시키는 것: HWPX 인코더의 그림 채우기 갈래를 빈 갈래로.
#[test]
fn x6_picture_fill_round_trips_through_hwpx() {
    let (hwpx, _) =
        hwp5_to_hwpx_bytes_with_diagnostics(&fixture("x6-fill.hwp"), ConvertOptions::default())
            .expect("convert");
    let doc = HwpxDecoder::decode(&hwpx).expect("decode");
    let mut fills = Vec::new();
    doc.document.sections()[0].for_each_paragraph(|p| {
        for run in &p.runs {
            if let RunContent::Control(c) = &run.content {
                if let Control::Rect { style: Some(s), .. } = c.as_ref() {
                    if let Some(Fill::Image { image_id, mode }) = &s.fill {
                        fills.push((image_id.clone(), *mode));
                    }
                }
            }
        }
    });
    assert_eq!(fills.len(), 1, "{fills:?}");
    assert_eq!(fills[0].1, ImageFillMode::Stretch);
    let stem = fills[0].0.trim_start_matches("BinData/");
    assert!(
        doc.image_store.iter().any(|(key, _)| key.split('.').next() == Some(stem)),
        "{stem} not in the image store"
    );
}
