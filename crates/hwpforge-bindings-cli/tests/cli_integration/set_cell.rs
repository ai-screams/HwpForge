use super::*;
use std::io::Write as _;

// ═══════════════════════════════════════════════════════════════
// E3 Wave 3: set-cell (격자 주소 셀 편집)
// ═══════════════════════════════════════════════════════════════

#[test]
fn set_cell_edits_anchor_and_resolves_covered_coordinate() {
    let f = fixture("tables/merged_grid_form.hwpx");
    let tmp = test_tmp();

    // export 로 첫 표의 병합 앵커(row_span > 1)를 찾는다.
    let json_out = tmp.join("grid.json");
    let (_, _, code) = run(&["to-json", f.to_str().unwrap(), "-o", json_out.to_str().unwrap()]);
    assert_eq!(code, 0);
    let parsed: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&json_out).unwrap()).unwrap();
    let (pi, ri) = first_table_run_indices(&parsed);
    let table =
        &parsed["document"]["sections"][0]["paragraphs"][pi]["runs"][ri]["content"]["Table"];
    let merged = table["rows"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|row| row["cells"].as_array().unwrap())
        .find(|cell| cell["row_span"].as_u64().unwrap_or(1) > 1)
        .expect("merge fixture must contain a row-span anchor");
    let (arow, acol) =
        (merged["addr"]["row"].as_u64().unwrap(), merged["addr"]["col"].as_u64().unwrap());

    // 피병합 좌표(앵커 바로 아래)를 지정 → 앵커로 resolve 되어야 한다.
    let covered = format!("{},{}", arow + 1, acol);
    let out = tmp.join("edited.hwpx");
    let (value, _, code) = run_json(&[
        "set-cell",
        f.to_str().unwrap(),
        "--table",
        "0",
        "--at",
        &covered,
        "--text",
        "격자값",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{value}");
    assert_eq!(value["cells"][0]["resolution"], "covered_to_anchor");
    assert_eq!(value["cells"][0]["anchor"], serde_json::json!({"row": arow, "col": acol}));

    // 편집 결과 검증: 재-export 에서 앵커 셀 텍스트가 바뀌었는지.
    let verify_json = tmp.join("verify.json");
    let (_, _, code) =
        run(&["to-json", out.to_str().unwrap(), "-o", verify_json.to_str().unwrap()]);
    assert_eq!(code, 0);
    let verify: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&verify_json).unwrap()).unwrap();
    let (vpi, vri) = first_table_run_indices(&verify);
    let vtable =
        &verify["document"]["sections"][0]["paragraphs"][vpi]["runs"][vri]["content"]["Table"];
    let edited = vtable["rows"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|row| row["cells"].as_array().unwrap())
        .find(|cell| cell["addr"] == serde_json::json!({"row": arow, "col": acol}))
        .expect("anchor cell present");
    let text = edited["paragraphs"][0]["runs"][0]["content"]["Text"].as_str().unwrap_or("");
    assert_eq!(text, "격자값");
}

#[test]
fn set_cell_error_codes_and_all_or_nothing() {
    let f = fixture("tables/merged_grid_form.hwpx");
    let tmp = test_tmp();
    let out = tmp.join("never.hwpx");
    let run_edit = |args: &[&str]| {
        let mut full = vec!["set-cell", f.to_str().unwrap()];
        full.extend_from_slice(args);
        full.extend_from_slice(&["-o", out.to_str().unwrap()]);
        run_json(&full)
    };

    let (value, _, code) = run_edit(&["--table", "99", "--at", "0,0", "--text", "x"]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "TABLE_NOT_FOUND");

    let (value, _, code) = run_edit(&["--table", "0", "--at", "9999,0", "--text", "x"]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "CELL_NOT_FOUND");

    let (value, _, code) =
        run_edit(&["--table", "0", "--right-of", "존재하지않는라벨", "--text", "x"]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "CELL_NOT_FOUND");

    let (value, _, code) = run_edit(&["--table", "0", "--at", "abc", "--text", "x"]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "INVALID_SET_CELL_ARGS");

    let (value, _, code) = run_edit(&["--table", "0", "--text", "x"]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "INVALID_SET_CELL_ARGS");

    assert!(!out.exists(), "rejected edits must not produce output (all-or-nothing)");
}

// ═══════════════════════════════════════════════════════════════
// set-cell --map 배치·인자 충돌 게이트
// ═══════════════════════════════════════════════════════════════

#[test]
fn set_cell_map_batch_reports_resolution_and_clear() {
    let f = fixture("tables/merged_grid_form.hwpx");
    let tmp = test_tmp();
    let map = tmp.join("cells.json");
    std::fs::write(
        &map,
        serde_json::json!([
            {"table": 0, "at": {"row": 2, "col": 0}, "text": "세로병합값"},
            {"table": 0, "right_of": "성명", "text": ""}
        ])
        .to_string(),
    )
    .unwrap();
    let out = tmp.join("batch.hwpx");

    // 사람용 출력 경로: covered → anchor 리다이렉트와 clear 마커가 보여야 한다.
    let (stdout, stderr, code) = run(&[
        "set-cell",
        f.to_str().unwrap(),
        "--map",
        map.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("Set 2 cell(s)"), "{stdout}");
    assert!(stdout.contains("covered -> anchor (1, 0)"), "{stdout}");
    assert!(stdout.contains("[cleared]"), "{stdout}");
    assert!(out.exists());
}

#[test]
fn set_cell_map_arg_conflicts_rejected() {
    let f = fixture("tables/merged_grid_form.hwpx");
    let tmp = test_tmp();
    let out = tmp.join("never2.hwpx");
    let map = tmp.join("cells.json");
    std::fs::write(&map, "[]").unwrap();

    // --map 과 단건 플래그 동시 사용 금지.
    let (value, _, code) = run_json(&[
        "set-cell",
        f.to_str().unwrap(),
        "--map",
        map.to_str().unwrap(),
        "--table",
        "0",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "INVALID_SET_CELL_ARGS");

    // 빈 맵 거부.
    let (value, _, code) = run_json(&[
        "set-cell",
        f.to_str().unwrap(),
        "--map",
        map.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "INVALID_SET_CELL_MAP");

    // 맵 파싱 실패.
    std::fs::write(&map, "{not json").unwrap();
    let (value, _, code) = run_json(&[
        "set-cell",
        f.to_str().unwrap(),
        "--map",
        map.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "INVALID_SET_CELL_MAP");

    // --at 와 --right-of 동시 지정.
    let (value, _, code) = run_json(&[
        "set-cell",
        f.to_str().unwrap(),
        "--table",
        "0",
        "--at",
        "0,0",
        "--right-of",
        "성명",
        "--text",
        "x",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "INVALID_SET_CELL_ARGS");
    assert!(!out.exists());
}

#[test]
fn set_cell_map_arg_conflict_rejected_before_map_file_is_read() {
    // W3 remediation: the --map/single-target mutual-exclusion guard must
    // run before the (possibly nonexistent) --map file is read — a
    // combined-flags misuse should never surface as FILE_READ_FAILED just
    // because the map path happens not to exist.
    let f = fixture("tables/merged_grid_form.hwpx");
    let tmp = test_tmp();
    let out = tmp.join("never3.hwpx");

    let (value, _, code) = run_json(&[
        "set-cell",
        f.to_str().unwrap(),
        "--map",
        "/nonexistent/missing.json",
        "--table",
        "0",
        "--at",
        "0,0",
        "--text",
        "x",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "INVALID_SET_CELL_ARGS");
    assert!(!out.exists());
}

#[test]
fn set_cell_surfaces_layout_cache_dropped_decode_warning() {
    let f = fixture("layout/stale-line-cache.hwpx");
    let tmp = test_tmp();
    let out = tmp.join("sc.hwpx");
    let (value, _, code) = run_json(&[
        "set-cell",
        f.to_str().unwrap(),
        "--table",
        "0",
        "--at",
        "0,1",
        "--text",
        "값",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{value}");
    assert_eq!(value["warnings"][0]["code"], "LAYOUT_CACHE_DROPPED", "{value}");

    let out2 = tmp.join("sc2.hwpx");
    let (_, text_stderr, code2) = run(&[
        "set-cell",
        f.to_str().unwrap(),
        "--table",
        "0",
        "--at",
        "0,1",
        "--text",
        "값",
        "-o",
        out2.to_str().unwrap(),
    ]);
    assert_eq!(code2, 0);
    assert!(text_stderr.contains("[set-cell] LAYOUT_CACHE_DROPPED"), "{text_stderr}");
}

#[test]
fn set_cell_has_no_warnings_key_on_a_clean_fixture() {
    let f = fixture("tables/merged_grid_form.hwpx");
    let tmp = test_tmp();
    let out = tmp.join("sc.hwpx");
    let (value, stderr, code) = run_json(&[
        "set-cell",
        f.to_str().unwrap(),
        "--table",
        "0",
        "--at",
        "0,0",
        "--text",
        "값",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{value}");
    assert!(!value.as_object().unwrap().contains_key("warnings"), "{value}");
    assert!(stderr.is_empty(), "{stderr}");
}

#[cfg(unix)]
#[test]
// 이것을 실패시키는 것: canonical 출력 코드를 MUTANT로 치환.
fn set_cell_fifo_map_over_the_size_cap_reports_input_too_large() {
    // `set-cell --map` had no `check_file_size` preflight at all before the
    // `read_to_string` bounded-read fix (audit follow-up) — unlike every
    // other read site, it never had an `INPUT_TOO_LARGE` code of its own to
    // reuse. The cap surfaces through this site's existing
    // `FILE_READ_FAILED`/exit 1 (`read_bounded_string`'s `Err` arm), not a
    // newly introduced `INPUT_TOO_LARGE`.
    let f = fixture("tables/merged_grid_form.hwpx");
    let tmp = test_tmp();
    let out = tmp.join("never.hwpx");
    let fifo_path = tmp.join("oversized_map.json");
    create_fifo(&fifo_path);

    let writer_path = fifo_path.clone();
    let writer = std::thread::spawn(move || {
        let Ok(mut w) = std::fs::File::create(&writer_path) else { return };
        let chunk = vec![b'a'; 1024 * 1024];
        for _ in 0..150 {
            if w.write_all(&chunk).is_err() {
                break;
            }
        }
    });

    let (val, stderr, code) = run_json(&[
        "set-cell",
        f.to_str().unwrap(),
        "--map",
        fifo_path.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    let _ = writer.join();

    assert_eq!(code, 1, "stderr: {stderr}, value: {val}");
    assert_eq!(val["code"], "INPUT_TOO_LARGE", "{val}");
    assert!(!out.exists());
}
