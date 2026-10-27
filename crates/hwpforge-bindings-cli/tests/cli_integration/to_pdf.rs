use super::*;

// ─── W6a: to-pdf (조판 캐시 재생 렌더) ───

#[test]
fn to_pdf_rejects_unrecognized_container() {
    let dir = test_tmp();
    let garbage = dir.join("garbage.hwpx");
    std::fs::write(&garbage, b"not a container at all").expect("write");
    let (value, _stderr, code) = run_json(&["to-pdf", garbage.to_str().unwrap()]);
    assert_eq!(code, 2);
    assert_eq!(value["code"], "UNRECOGNIZED_FORMAT");
}

#[test]
fn to_pdf_rejects_cacheless_hwpx_fail_closed() {
    // 폰트 무관 CI 경로: 순수 생성 HWPX(캐시 없음) → 스니핑·디코드·검증까지
    // 통과 후 렌더에서 fail-closed (한컴 재저장 안내 힌트).
    let dir = test_tmp();
    let md = dir.join("doc.md");
    std::fs::write(&md, "# 제목\n\n본문 문단.\n").expect("write md");
    let hwpx = dir.join("doc.hwpx");
    let (_v, _s, code) = run_json(&["convert", md.to_str().unwrap(), "-o", hwpx.to_str().unwrap()]);
    assert_eq!(code, 0);
    let (value, _stderr, code) = run_json(&["to-pdf", hwpx.to_str().unwrap()]);
    assert_eq!(code, 2, "{value}");
    assert_eq!(value["code"], "PDF_RENDER_FAILED");
    assert_eq!(value["cause"]["stage"], "render", "{value}");
    assert_eq!(value["cause"]["code"], "NO_RENDERABLE_CACHE", "{value}");
    assert_eq!(value["cause"]["location"], "s0", "{value}");
}

#[test]
fn to_pdf_extension_mismatch_detected_without_fonts() {
    // 위장 확장자(.hwp 탈 HWPX)도 폰트 없이 스니핑 라우팅까지는 CI 에서 검증
    // 가능 — 이후 캐시 결손 fail-closed 는 위 게이트와 동일.
    let dir = test_tmp();
    let md = dir.join("doc.md");
    std::fs::write(&md, "# 제목\n").expect("write md");
    let hwpx = dir.join("doc.hwpx");
    let (_v, _s, code) = run_json(&["convert", md.to_str().unwrap(), "-o", hwpx.to_str().unwrap()]);
    assert_eq!(code, 0);
    let misnamed = dir.join("doc-misnamed.hwp");
    std::fs::copy(&hwpx, &misnamed).expect("copy");
    let (value, _stderr, code) = run_json(&["to-pdf", misnamed.to_str().unwrap()]);
    assert_eq!(code, 2, "{value}"); // cacheless — 렌더 거부는 동일
    assert_eq!(value["code"], "PDF_RENDER_FAILED");
    assert_eq!(value["cause"]["code"], "NO_RENDERABLE_CACHE", "{value}");
}

#[test]
fn to_pdf_human_mode_error_output() {
    let dir = test_tmp();
    let garbage = dir.join("garbage.bin");
    std::fs::write(&garbage, b"???").expect("write");
    let (_stdout, stderr, code) = run(&["to-pdf", garbage.to_str().unwrap()]);
    assert_eq!(code, 2);
    assert!(stderr.contains("UNRECOGNIZED_FORMAT"), "{stderr}");
}
