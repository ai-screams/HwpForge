use super::*;

// ═══════════════════════════════════════════════════════════════
// stamp-plan / stamp — E6 템플릿 스탬핑 게이트
// ═══════════════════════════════════════════════════════════════

/// stamp-plan candidates 에서 시험용 spec 맵을 만든다: 무가드 후보는
/// 순번 이름으로 승격, 가드 후보는 ignore.
fn stamp_map_from_plan(plan: &serde_json::Value, skip_first: bool) -> serde_json::Value {
    let mut specs = Vec::new();
    let mut n = 0usize;
    for (idx, c) in plan["candidates"].as_array().unwrap().iter().enumerate() {
        if skip_first && idx == 0 {
            continue;
        }
        // 가드 후보는 맵에서 제외 — 스펙 없는 가드 후보의 skip 경로 검증.
        if !c["guard"].is_null() {
            continue;
        }
        n += 1;
        let action = serde_json::json!({"field": {"name": format!("게이트필드{n}"), "hint": null}});
        specs.push(serde_json::json!({
            "section": c["section"],
            "path": c["path"],
            "span": c["span"],
            "marker": c["marker"],
            "action": action,
        }));
    }
    serde_json::Value::Array(specs)
}

#[test]
fn stamp_plan_lists_candidates_with_guards() {
    let f = fixture("stamp/placeholder_basic.hwpx");
    let (value, _, code) = run_json(&["stamp-plan", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    let candidates = value["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 3, "paren blank + cell checkbox + guarded checkbox: {value}");
    let guarded: Vec<_> = candidates.iter().filter(|c| !c["guard"].is_null()).collect();
    assert_eq!(guarded.len(), 1, "※ 안내문의 □ 만 가드되어야 한다");
    assert_eq!(guarded[0]["marker"], "□");
}

#[test]
fn stamp_end_to_end_then_fillable() {
    let f = fixture("stamp/placeholder_basic.hwpx");
    let tmp = test_tmp();
    let (plan, _, code) = run_json(&["stamp-plan", f.to_str().unwrap()]);
    assert_eq!(code, 0);

    let map = tmp.join("map.json");
    std::fs::write(&map, stamp_map_from_plan(&plan, false).to_string()).unwrap();
    let out = tmp.join("stamped.hwpx");
    let (value, _, code) = run_json(&[
        "stamp",
        f.to_str().unwrap(),
        "--map",
        map.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "stamp must succeed: {value}");
    assert_eq!(value["stamped"].as_array().unwrap().len(), 2);
    assert_eq!(value["skipped_guarded"], 1);
    let manifest_path = value["manifest"].as_str().unwrap().to_string();
    assert!(std::path::Path::new(&manifest_path).exists(), "manifest must be written");

    // 스탬프 산출물은 즉시 fields/fill 로 소비 가능해야 한다.
    let (fields, _, code) = run_json(&["fields", out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(fields["fields"].as_array().unwrap().len(), 2);

    let filled = tmp.join("filled.hwpx");
    let (fill, _, code) = run_json(&[
        "fill",
        out.to_str().unwrap(),
        "--set",
        "게이트필드1=홍길동",
        "-o",
        filled.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "fill on stamped output must succeed: {fill}");
}

#[test]
fn stamp_uncovered_candidate_rejected_all_or_nothing() {
    let f = fixture("stamp/placeholder_basic.hwpx");
    let tmp = test_tmp();
    let (plan, _, _) = run_json(&["stamp-plan", f.to_str().unwrap()]);

    // 첫 후보를 맵에서 누락 → preflight 거부, 산출물 없음
    let map = tmp.join("partial-map.json");
    std::fs::write(&map, stamp_map_from_plan(&plan, true).to_string()).unwrap();
    let out = tmp.join("never-stamped.hwpx");
    let (value, _, code) = run_json(&[
        "stamp",
        f.to_str().unwrap(),
        "--map",
        map.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "STAMP_CANDIDATE_UNCOVERED");
    assert!(!out.exists(), "preflight 실패 시 산출물이 없어야 한다 (fail-closed)");
}

#[test]
fn stamp_error_codes_for_bad_maps() {
    let f = fixture("stamp/placeholder_basic.hwpx");
    let tmp = test_tmp();
    let (plan, _, _) = run_json(&["stamp-plan", f.to_str().unwrap()]);
    let out = tmp.join("never3.hwpx");
    let run_map = |specs: &serde_json::Value| {
        let map = tmp.join("bad-map.json");
        std::fs::write(&map, specs.to_string()).unwrap();
        run_json(&[
            "stamp",
            f.to_str().unwrap(),
            "--map",
            map.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ])
    };
    let base = stamp_map_from_plan(&plan, false);

    // stale span → STAMP_SPEC_STALE
    let mut stale = base.clone();
    stale[0]["span"] = serde_json::json!({"start": 0, "end": 1});
    let (value, _, code) = run_map(&stale);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "STAMP_SPEC_STALE");

    // marker 불일치 → STAMP_MARKER_MISMATCH
    let mut mismatch = base.clone();
    mismatch[0]["marker"] = serde_json::json!("(다름)");
    let (value, _, code) = run_map(&mismatch);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "STAMP_MARKER_MISMATCH");

    // 이름 중복 → STAMP_NAME_DUPLICATE
    let mut dup = base.clone();
    dup[0]["action"] = serde_json::json!({"field": {"name": "같음", "hint": null}});
    dup[1]["action"] = serde_json::json!({"field": {"name": "같음", "hint": null}});
    let (value, _, code) = run_map(&dup);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "STAMP_NAME_DUPLICATE");

    // 깨진 맵 JSON → INVALID_STAMP_MAP
    let map = tmp.join("broken-map.json");
    std::fs::write(&map, "{ not json").unwrap();
    let (value, _, code) = run_json(&[
        "stamp",
        f.to_str().unwrap(),
        "--map",
        map.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "INVALID_STAMP_MAP");

    assert!(!out.exists(), "모든 거부에서 산출물이 없어야 한다 (fail-closed)");
}

#[test]
fn stamp_manifest_write_failure_removes_output() {
    // Review L1: manifest 기록 실패 시 .hwpx 산출물도 남기지 않아야 한다.
    let f = fixture("stamp/placeholder_basic.hwpx");
    let tmp = test_tmp();
    let (plan, _, _) = run_json(&["stamp-plan", f.to_str().unwrap()]);
    let map = tmp.join("map.json");
    std::fs::write(&map, stamp_map_from_plan(&plan, false).to_string()).unwrap();
    let out = tmp.join("orphan.hwpx");
    let (value, _, code) = run_json(&[
        "stamp",
        f.to_str().unwrap(),
        "--map",
        map.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--manifest",
        "/nonexistent-dir/never.manifest.json",
    ]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "FILE_WRITE_FAILED");
    assert!(!out.exists(), "manifest 실패 시 산출물이 제거되어야 한다 (fail-closed)");
}

// ═══════════════════════════════════════════════════════════════
// stamp v2 — E6 Wave 2 클래스-B 셀 스탬핑 게이트
// ═══════════════════════════════════════════════════════════════

/// merged_grid_form 의 셀 후보 3개(성명·비고·비고-병합경계)에 대한 v2 맵.
fn stamp_v2_map(plan: &serde_json::Value) -> serde_json::Value {
    let sha = plan["source_sha256"].as_str().unwrap();
    serde_json::json!({
        "schema_version": 2,
        "source_sha256": sha,
        "cells": [
            {"table": 0, "at": {"row": 0, "col": 1},
             "label": {"at": {"row": 0, "col": 0}, "text": "성명"},
             "action": {"field": {"name": "성명값", "hint": "성명 입력"}}},
            {"table": 0, "at": {"row": 1, "col": 1},
             "action": {"field": {"name": "비고값", "hint": "비고 입력"}}},
            {"table": 0, "at": {"row": 2, "col": 1}, "action": "ignore"},
        ],
    })
}

#[test]
fn stamp_plan_v2_lists_cell_candidates_with_source_hash() {
    let f = fixture("tables/merged_grid_form.hwpx");
    let (value, _, code) = run_json(&["stamp-plan", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(value["schema_version"], 2);
    let sha = value["source_sha256"].as_str().unwrap();
    assert_eq!(sha.len(), 64, "hex sha256: {sha}");
    let cells = value["cells"].as_array().unwrap();
    assert_eq!(cells.len(), 3, "성명·비고·병합경계 비고: {value}");
    assert!(value["skipped_tables"].as_array().unwrap().is_empty());
    // 병합 라벨(비고, rowspan 2)이 (2,1) 의 shared-boundary 라벨로 잡혀야 한다.
    let covered_boundary = &cells[2];
    assert_eq!(covered_boundary["at"], serde_json::json!({"row": 2, "col": 1}));
    assert_eq!(covered_boundary["labels"][0]["normalized"], "비고");
    // duplicate_count 는 문서 내 라벨 "텍스트" 중복 기준 — 비고 셀은 유일하므로
    // 두 후보 모두 같은 suggested_name 을 받는다 (제안은 비구속; 그대로 복사해
    // 쓰면 preflight DuplicateName 이 거부).
    assert_eq!(cells[1]["suggested_name"], "비고");
    assert_eq!(covered_boundary["suggested_name"], "비고");
}

#[test]
fn stamp_v2_cells_end_to_end_then_fillable() {
    let f = fixture("tables/merged_grid_form.hwpx");
    let tmp = test_tmp();
    let (plan, _, code) = run_json(&["stamp-plan", f.to_str().unwrap()]);
    assert_eq!(code, 0);

    let map = tmp.join("v2-map.json");
    std::fs::write(&map, stamp_v2_map(&plan).to_string()).unwrap();
    let out = tmp.join("v2-stamped.hwpx");
    let (value, _, code) = run_json(&[
        "stamp",
        f.to_str().unwrap(),
        "--map",
        map.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "v2 stamp must succeed: {value}");
    assert_eq!(value["stamped_cells"].as_array().unwrap().len(), 2);
    assert_eq!(value["ignored"], 1);
    let manifest_path = value["manifest"].as_str().unwrap().to_string();
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
    assert_eq!(manifest["schema_version"], 2);
    let cell_origins: Vec<_> = manifest["fields"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| !f["stamp"]["cell"].is_null())
        .collect();
    assert_eq!(cell_origins.len(), 2, "{manifest}");

    // 산출물은 즉시 fields/fill 로 소비 가능.
    let (fields, _, code) = run_json(&["fields", out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(fields["fields"].as_array().unwrap().len(), 2);
    let filled = tmp.join("v2-filled.hwpx");
    let (fill, _, code) = run_json(&[
        "fill",
        out.to_str().unwrap(),
        "--set",
        "성명값=홍길동",
        "-o",
        filled.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "fill on cell-stamped output must succeed: {fill}");
}

#[test]
fn stamp_v2_source_hash_mismatch_rejected() {
    let f = fixture("tables/merged_grid_form.hwpx");
    let tmp = test_tmp();
    let (plan, _, _) = run_json(&["stamp-plan", f.to_str().unwrap()]);
    let mut map_value = stamp_v2_map(&plan);
    map_value["source_sha256"] = serde_json::Value::String("0".repeat(64));
    let map = tmp.join("stale-map.json");
    std::fs::write(&map, map_value.to_string()).unwrap();
    let out = tmp.join("never-v2.hwpx");
    let (value, _, code) = run_json(&[
        "stamp",
        f.to_str().unwrap(),
        "--map",
        map.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "STAMP_SOURCE_HASH_MISMATCH");
    assert!(!out.exists(), "sha 불일치 시 산출물이 없어야 한다 (fail-closed)");
}

#[test]
fn stamp_v2_uncovered_cell_and_unknown_field_rejected() {
    let f = fixture("tables/merged_grid_form.hwpx");
    let tmp = test_tmp();
    let (plan, _, _) = run_json(&["stamp-plan", f.to_str().unwrap()]);
    let sha = plan["source_sha256"].as_str().unwrap();

    // 후보 3개 중 1개만 커버 → STAMP_CANDIDATE_UNCOVERED.
    let partial = serde_json::json!({
        "schema_version": 2, "source_sha256": sha,
        "cells": [{"table": 0, "at": {"row": 0, "col": 1},
                   "action": {"field": {"name": "성명값", "hint": "h"}}}],
    });
    let map = tmp.join("partial-v2.json");
    std::fs::write(&map, partial.to_string()).unwrap();
    let out = tmp.join("never-v2b.hwpx");
    let (value, _, code) = run_json(&[
        "stamp",
        f.to_str().unwrap(),
        "--map",
        map.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "STAMP_CANDIDATE_UNCOVERED");
    assert!(!out.exists());

    // unknown field 가 든 v2 맵은 parse 단계에서 거부.
    let typo = serde_json::json!({
        "schema_version": 2, "source_sha256": sha, "cell": [],
    });
    std::fs::write(&map, typo.to_string()).unwrap();
    let (value, _, code) = run_json(&[
        "stamp",
        f.to_str().unwrap(),
        "--map",
        map.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "INVALID_STAMP_MAP");
}

/// R1 F4 회귀 잠금 (CLI stamp): 의미 손상은 typed `StamperError::SemanticLoss`
/// 를 canonical `ENCODE_SEMANTIC_LOSS`로 내보내며 exit 2를 유지한다.
/// 의미 손상을 포괄 실패로 축약하면 이 계약이 깨진다.
///
/// fixture 는 `smithy-hwpx/tests/note_numbering_roundtrip.rs` 의
/// `stamper_fails_closed_on_note_head_skip` 과 같은 구성이다: 각주 본문의
/// 첫 문단이 heading 이라 번호 머리를 넣을 안전한 지점이 없다.
#[test]
// 이것을 실패시키는 것: canonical 출력 코드를 MUTANT로 치환.
fn stamp_semantic_loss_keeps_the_codec_error_contract() {
    use hwpforge_core::control::Control;
    use hwpforge_core::image::ImageStore;
    use hwpforge_core::page::PageSettings;
    use hwpforge_core::run::Run;
    use hwpforge_core::section::Section;
    use hwpforge_core::{Document, Paragraph};
    use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
    use hwpforge_smithy_hwpx::style_store::{
        HwpxCharShape, HwpxFont, HwpxParaShape, HwpxStyleStore,
    };
    use hwpforge_smithy_hwpx::HwpxEncoder;

    let mut store = HwpxStyleStore::new();
    for &lang in &["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"] {
        store.push_font(HwpxFont::new(0, "함초롬돋움", lang));
    }
    store.push_char_shape(HwpxCharShape::default());
    store.push_para_shape(HwpxParaShape::default());

    let mut heading_body = Paragraph::with_runs(
        vec![Run::text("제목 각주", CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    heading_body.heading_level = Some(1);
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::control(Control::footnote(vec![heading_body]), CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    let validated = doc.validate().expect("validate");
    let base = HwpxEncoder::encode(&validated, &store, &ImageStore::new()).expect("encode");

    let tmp = test_tmp();
    let src = tmp.join("titlemark-note.hwpx");
    std::fs::write(&src, &base).unwrap();
    // 빈 legacy 맵 = `HwpxStamper::stamp(&base, &[])` 와 같은 경로.
    let map = tmp.join("empty-map.json");
    std::fs::write(&map, "[]").unwrap();
    let out = tmp.join("stamped.hwpx");

    let (err, _, code) = run_json(&[
        "stamp",
        src.to_str().unwrap(),
        "--map",
        map.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);

    assert_eq!(code, 2, "exit code 는 과거 Codec 경로와 같아야 한다: {err}");
    assert_eq!(err["code"], "ENCODE_SEMANTIC_LOSS", "{err}");
    let message = err["message"].as_str().unwrap_or_default();
    assert!(
        message.starts_with("encode produced a semantic-loss warning (fail-closed): "),
        "메시지가 드리프트했다 (접두사 없는 형태여야 한다): {message}"
    );
    assert!(message.contains("note number head skipped"), "{message}");
    assert!(!out.exists(), "fail-closed 인데 산출물이 생성됐다");
}

#[test]
fn stamp_plan_surfaces_layout_cache_dropped_decode_warning() {
    let f = fixture("layout/stale-line-cache.hwpx");
    let (value, _, code) = run_json(&["stamp-plan", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(value["warnings"][0]["code"], "LAYOUT_CACHE_DROPPED", "{value}");

    let (_, text_stderr, code2) = run(&["stamp-plan", f.to_str().unwrap()]);
    assert_eq!(code2, 0);
    assert!(text_stderr.contains("[stamp-plan] LAYOUT_CACHE_DROPPED"), "{text_stderr}");
}

#[test]
fn stamp_plan_has_no_warnings_key_on_a_clean_fixture() {
    let f = fixture("stamp/placeholder_basic.hwpx");
    let (value, stderr, code) = run_json(&["stamp-plan", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(!value.as_object().unwrap().contains_key("warnings"), "{value}");
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn stamp_surfaces_layout_cache_dropped_decode_warning() {
    let f = fixture("layout/stale-line-cache.hwpx");
    let (plan, _, code) = run_json(&["stamp-plan", f.to_str().unwrap()]);
    assert_eq!(code, 0);

    let tmp = test_tmp();
    let map = tmp.join("map.json");
    // Legacy 배열 맵 — 셀 후보는 legacy 스펙이 표현 못 하므로 커버 대상이 아니다
    // (본문 하나짜리 placeholder 만 커버해도 all-or-nothing 을 만족한다).
    std::fs::write(&map, stamp_map_from_plan(&plan, false).to_string()).unwrap();
    let out = tmp.join("stamped.hwpx");
    let (value, _, code) = run_json(&[
        "stamp",
        f.to_str().unwrap(),
        "--map",
        map.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{value}");
    assert_eq!(value["warnings"][0]["code"], "LAYOUT_CACHE_DROPPED", "{value}");

    let out2 = tmp.join("stamped2.hwpx");
    let (_, text_stderr, code2) = run(&[
        "stamp",
        f.to_str().unwrap(),
        "--map",
        map.to_str().unwrap(),
        "-o",
        out2.to_str().unwrap(),
    ]);
    assert_eq!(code2, 0);
    assert!(text_stderr.contains("[stamp] LAYOUT_CACHE_DROPPED"), "{text_stderr}");
}

#[test]
fn stamp_has_no_warnings_key_on_a_clean_fixture() {
    let f = fixture("stamp/placeholder_basic.hwpx");
    let (plan, _, code) = run_json(&["stamp-plan", f.to_str().unwrap()]);
    assert_eq!(code, 0);

    let tmp = test_tmp();
    let map = tmp.join("map.json");
    std::fs::write(&map, stamp_map_from_plan(&plan, false).to_string()).unwrap();
    let out = tmp.join("stamped.hwpx");
    let (value, stderr, code) = run_json(&[
        "stamp",
        f.to_str().unwrap(),
        "--map",
        map.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{value}");
    assert!(!value.as_object().unwrap().contains_key("warnings"), "{value}");
    assert!(stderr.is_empty(), "{stderr}");
}
