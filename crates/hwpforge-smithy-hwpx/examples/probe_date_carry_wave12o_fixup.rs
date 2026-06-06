//! Wave 12o-fixup §Top-2 시각 검증 — HWPX → HwpForge → HWPX round-trip
//! 에서 한컴이 채운 `<opf:meta name="date">` 값이 보존되는지 확인.
//!
//! Wave 12o-fixup 적용 전: encoder 가 typed-collision guard 에서
//! `extras["date"]` 를 drop → 1-cycle 후 silent data loss.
//! Wave 12o-fixup 적용 후: encoder 가 9-slot canonical 위치에서 그대로
//! emit → round-trip 시 값 보존.
//!
//! 실행:
//! ```bash
//! cargo run -p hwpforge-smithy-hwpx --example probe_date_carry_wave12o_fixup
//! ```

use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder};

fn main() {
    let input_path = "examples/hwp5_review/converted-field-docsummary-wave12o.hwpx";
    let output_path = "examples/hwp5_review/roundtrip-date-carry-wave12o-fixup.hwpx";

    println!("Wave 12o-fixup §Top-2 시각 검증 (extras['date'] round-trip)\n");
    println!("입력 : {input_path}");
    println!("출력 : {output_path}\n");

    // Step 1 — Decode the Hancom-saved HWPX
    let decoded = HwpxDecoder::decode_file(input_path)
        .unwrap_or_else(|e| panic!("decode failed: {e}\n경로 확인: {input_path}"));

    let metadata = decoded.document.metadata().clone();
    println!("Step 1 — decode 결과 metadata:");
    println!("  title         = {:?}", metadata.title);
    println!("  author        = {:?}", metadata.author);
    println!("  subject       = {:?}", metadata.subject);
    println!("  created       = {:?}", metadata.created);
    println!("  modified      = {:?}", metadata.modified);
    println!("  last_saved_by = {:?}", metadata.last_saved_by);
    println!("  extras['date'] = {:?}\n", metadata.extras.get("date"));

    if !metadata.extras.contains_key("date") {
        println!("⚠️  입력 파일에 `<opf:meta name=\"date\">` 값이 없음.");
        println!("   한컴에서 한 번 열고 저장한 파일을 입력으로 사용하세요.\n");
    }

    // Step 2 — Re-encode through HwpxEncoder (Wave 12o-fixup 패치 적용)
    let validated = decoded.document.validate().expect("validate");
    let bytes = HwpxEncoder::encode(&validated, &decoded.style_store, &decoded.image_store)
        .expect("encode");
    std::fs::write(output_path, &bytes).expect("write output");
    println!("Step 2 — encode 완료 ({} bytes)\n", bytes.len());

    // Step 3 — Re-decode to confirm round-trip preserves the date carry
    let reread = HwpxDecoder::decode_file(output_path).expect("re-decode");
    let re_meta = reread.document.metadata().clone();
    println!("Step 3 — round-trip 후 decode 결과 metadata:");
    println!("  title         = {:?}", re_meta.title);
    println!("  author        = {:?}", re_meta.author);
    println!("  extras['date'] = {:?}\n", re_meta.extras.get("date"));

    let preserved =
        metadata.extras.get("date") == re_meta.extras.get("date") && metadata == re_meta;
    println!("Step 4 — Wave 12o-fixup §Top-2 검증:");
    if preserved {
        println!("  ✅ PASS — metadata 가 round-trip 후 동일 (date 포함)");
    } else {
        println!("  ❌ FAIL — round-trip 후 metadata 변화 감지");
        println!("    input.extras  = {:?}", metadata.extras);
        println!("    output.extras = {:?}", re_meta.extras);
    }

    println!();
    println!("시각 확인 (선택):");
    println!("  1. 한컴오피스에서 두 파일 열어서 자동 필드 표시 비교:");
    println!("     - open absolute: {}/{input_path}", std::env::current_dir().unwrap().display());
    println!("     - open absolute: {}/{output_path}", std::env::current_dir().unwrap().display());
    println!("  2. 또는 content.hpf 직접 비교:");
    println!("     unzip -p {input_path} Contents/content.hpf | grep '\"date\"'");
    println!("     unzip -p {output_path} Contents/content.hpf | grep '\"date\"'");
    println!("     → 두 줄에 동일한 `<opf:meta name=\"date\">...</opf:meta>` 값이 보이면 PASS");
}
