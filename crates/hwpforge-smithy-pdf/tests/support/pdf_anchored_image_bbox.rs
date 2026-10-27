use super::*;

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

pub(super) fn hwpunit_to_pt(v: i32) -> f64 {
    f64::from(v) / 100.0
}

/// 문서의 첫 body 앵커 이미지의 기대 bbox 를 margin + placement offset +
/// **마커 소속 lineseg vertpos** 로 독립 계산한다 (floor-search;
/// 마커가 첫 줄이면 종전 first-lineseg 모델과 동치. 렌더 파이프라인과 무관
/// — self-consistency; 판별 oracle 은 한컴 실측이 담당).
pub(super) fn expected_anchored(doc: &Document<Validated>) -> Bbox {
    for section in doc.sections() {
        let body_top = section.page_settings.margin_top.as_i32()
            + section.page_settings.header_margin.as_i32();
        let body_left = section.page_settings.margin_left.as_i32();
        for para in &section.paragraphs {
            let Some(cache) = para.layout_cache.as_ref() else { continue };
            if cache.lines.is_empty() {
                continue;
            }
            let mut marker_pos: u32 = 0;
            for run in &para.runs {
                if let RunContent::Image(img) = &run.content {
                    if let Some(p) = img.placement.as_ref().filter(|p| !p.treat_as_char) {
                        let idx =
                            cache.lines.iter().rposition(|s| s.textpos <= marker_pos).unwrap_or(0);
                        return Bbox {
                            x: hwpunit_to_pt(body_left + p.horz_offset.as_i32()),
                            y: hwpunit_to_pt(
                                body_top + cache.lines[idx].vertpos + p.vert_offset.as_i32(),
                            ),
                            width: hwpunit_to_pt(img.width.as_i32()),
                            height: hwpunit_to_pt(img.height.as_i32()),
                        };
                    }
                }
                if let Some(t) = run.content.plain_text() {
                    marker_pos += u32::try_from(t.encode_utf16().count()).expect("len");
                }
            }
        }
    }
    panic!("no body anchored image found in document");
}

pub(super) fn oracle_bbox(entry: &serde_json::Value) -> Bbox {
    let b = &entry["hancom_image_bbox_pt"];
    Bbox {
        x: b["x"].as_f64().expect("x"),
        y: b["y"].as_f64().expect("y"),
        width: b["width"].as_f64().expect("width"),
        height: b["height"].as_f64().expect("height"),
    }
}
