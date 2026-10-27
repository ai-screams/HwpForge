use super::*;
use std::io::Write as _;

// ═══════════════════════════════════════════════════════════════
// 8. Cross-cutting — 11 tests
// ═══════════════════════════════════════════════════════════════

#[test]
fn json_error_format() {
    let (_, stderr, code) = run(&["--json", "inspect", "/nonexistent/file.hwpx"]);
    assert_ne!(code, 0);
    let err: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(err["status"], "error");
    assert!(err["code"].is_string());
    assert!(err["message"].is_string());
}

#[test]
fn json_error_has_hint() {
    let (_, stderr, code) = run(&["--json", "schema", "foobar"]);
    assert_ne!(code, 0);
    let err: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(err["status"], "error");
    assert!(err["hint"].is_string(), "error should include hint field");
}

#[test]
fn unknown_subcommand() {
    let (_, _, code) = run(&["nonexistent-command"]);
    assert_ne!(code, 0);
}

#[test]
fn invalid_file_format() {
    let tmp = test_tmp();
    // Create a valid ZIP but not HWPX
    let not_hwpx = tmp.join("not_hwpx.hwpx");
    std::fs::write(&not_hwpx, b"PK\x03\x04not a real zip content").unwrap();
    let (_, _, code) = run(&["inspect", not_hwpx.to_str().unwrap()]);
    assert_eq!(code, 2, "expected exit code 2 for decode failure");
}

#[test]
fn binary_garbage_input() {
    let tmp = test_tmp();
    let garbage = tmp.join("garbage.hwpx");
    std::fs::write(&garbage, [0xFF, 0xFE, 0x00, 0x01, 0xAB, 0xCD]).unwrap();
    let (_, _, code) = run(&["inspect", garbage.to_str().unwrap()]);
    assert_ne!(code, 0, "garbage input should fail");
}

#[test]
fn empty_file_input() {
    let tmp = test_tmp();
    let empty = tmp.join("empty.hwpx");
    std::fs::write(&empty, b"").unwrap();
    let (_, _, code) = run(&["inspect", empty.to_str().unwrap()]);
    assert_ne!(code, 0, "empty file should fail");
}

#[test]
fn bad_output_directory() {
    let tmp = test_tmp();
    let md = create_test_md(&tmp, TEST_MD);
    let (_, _, code) =
        run(&["convert", md.to_str().unwrap(), "-o", "/nonexistent/dir/output.hwpx"]);
    assert_eq!(code, 1, "writing to nonexistent directory should fail");
}

#[cfg(unix)]
#[test]
// 이것을 실패시키는 것: to-md InputTooLarge 종료를 2로 변경하거나 convert 상한 힌트를 UTF-8 안내로 교체.
fn fifo_input_over_the_size_cap_reports_input_too_large() {
    for command in ["inspect", "to-md", "convert"] {
        let tmp = test_tmp();
        let fifo_path = tmp.join("oversized.hwpx");
        create_fifo(&fifo_path);

        let writer_path = fifo_path.clone();
        let writer = std::thread::spawn(move || {
            let Ok(mut f) = std::fs::File::create(&writer_path) else { return };
            let chunk = vec![0u8; 1024 * 1024];
            // Comfortably over the 100 MB cap; the reader (`read_input`'s
            // `take(MAX_FILE_SIZE + 1)`) stops consuming once it has the extra
            // byte it needs to detect the overflow and exits, so this loop
            // reliably breaks on the first write past that point — `write_all`
            // then reports `BrokenPipe` (Rust ignores `SIGPIPE`, so a write past
            // a closed reader is an ordinary `io::Error`, not a fatal signal).
            for _ in 0..150 {
                if f.write_all(&chunk).is_err() {
                    break;
                }
            }
        });

        let output = tmp.join("out.hwpx");
        let args = if command == "convert" {
            vec![command, fifo_path.to_str().unwrap(), "-o", output.to_str().unwrap()]
        } else {
            vec![command, fifo_path.to_str().unwrap()]
        };
        let (val, stderr, code) = run_json(&args);
        let _ = writer.join();

        assert_eq!(code, 1, "stderr: {stderr}, value: {val}");
        if command == "to-md" {
            assert_cli_manifest_case("io.to-md.fifo_over_limit", &val, code);
        }
        assert_eq!(val["code"], "INPUT_TOO_LARGE", "{val}");
        if command == "convert" {
            assert_eq!(val["hint"], "Use an input smaller than the 100 MB limit");
        }
    }
}

#[test]
// 이것을 실패시키는 것: 변환 명령 입력 I/O 종료를 1로 변경, InputTooLarge 종료를 2로 변경, 사전 검사 코드를 FileReadFailed로 변경.
fn w1_input_path_and_metadata_boundaries() {
    let dir = test_tmp();
    let missing = dir.join("missing.hwpx");
    let output = dir.join("out.hwpx");
    for args in [
        vec!["inspect", missing.to_str().unwrap()],
        vec!["to-md", missing.to_str().unwrap()],
        vec!["convert-hwp5", missing.to_str().unwrap(), "-o", output.to_str().unwrap()],
        vec!["convert", missing.to_str().unwrap(), "-o", output.to_str().unwrap()],
    ] {
        let (err, _, exit) = run_json(&args);
        assert_eq!(err["code"], "FILE_NOT_FOUND", "{args:?}");
        assert_eq!(exit, if matches!(args[0], "to-md" | "convert-hwp5") { 2 } else { 1 });
    }
    for command in ["to-md", "convert-hwp5"] {
        let args = if command == "convert-hwp5" {
            vec![command, dir.to_str().unwrap(), "-o", output.to_str().unwrap()]
        } else {
            vec![command, dir.to_str().unwrap()]
        };
        let (err, _, exit) = run_json(&args);
        assert_eq!(err["code"], "FILE_READ_FAILED");
        assert_eq!(exit, 2, "unreadable directory: {command}");
    }
    let exact = dir.join("exact.hwpx");
    std::fs::File::create(&exact).unwrap().set_len(hwpforge::ops::fs::MAX_FILE_SIZE).unwrap();
    let (err, _, exit) = run_json(&["inspect", exact.to_str().unwrap()]);
    assert_eq!(err["code"], "DECODE_FAILED", "exact cap must reach the codec");
    assert_eq!(exit, 2);
    let large = dir.join("large.hwpx");
    std::fs::File::create(&large).unwrap().set_len(hwpforge::ops::fs::MAX_FILE_SIZE + 1).unwrap();
    for command in ["to-md", "convert-hwp5"] {
        let args = if command == "convert-hwp5" {
            vec![command, large.to_str().unwrap(), "-o", output.to_str().unwrap()]
        } else {
            vec![command, large.to_str().unwrap()]
        };
        let (err, _, exit) = run_json(&args);
        assert_eq!(err["code"], "INPUT_TOO_LARGE");
        assert_eq!(exit, 1, "metadata preflight: {command}");
        assert_cli_manifest_case(&format!("io.{command}.input_size_preflight"), &err, exit);
    }
    let (err, _, exit) = run_json(&["inspect", large.to_str().unwrap()]);
    assert_eq!(err["code"], "INPUT_TOO_LARGE");
    assert!(err["message"].as_str().unwrap().contains("is 100 MB"));
    assert_eq!(exit, 1);
}

#[test]
// 이것을 실패시키는 것: ReadError::Codec을 InvalidInput으로 매핑하거나 typed 직렬화의 code 키를 바꿈.
fn decode_input_shared_fixture() {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../../../hwpforge/tests/data/error_contract_018.json"))
            .unwrap();
    let case =
        manifest["cases"].as_array().unwrap().iter().find(|c| c["id"] == "decode.input").unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../hwpforge/tests/data/corrupt-input.hwpx");
    let (err, _, exit) = run_json(&["read", path.to_str().unwrap(), "--section", "0"]);
    assert_eq!(err["code"], case["expected_code"]);
    assert_eq!(exit, case["surfaces"]["cli"]["exit"].as_i64().unwrap() as i32);
}

#[test]
// 이것을 실패시키는 것: patch 원본 decode 를 Rejected 로 감싸기(PATCH_FAILED), metadata 검사를
// 원본 decode 뒤로 옮기기(DECODE_FAILED), fill 입력 decode 를 Workflow 로 되돌리기(FILL_FAILED).
fn w2_patch_and_fill_shared_fixtures() {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../hwpforge/tests/data");
    let out = test_tmp().join("out.hwpx");
    let base = data.join("patch-inner-decode.hwpx");
    for (id, json) in [
        ("patch.input_decode", "patch-inner-decode.section.json"),
        ("patch.missing_metadata_first", "patch-no-metadata.section.json"),
    ] {
        let section = data.join(json);
        let (err, _, exit) = run_json(&[
            "patch",
            base.to_str().unwrap(),
            section.to_str().unwrap(),
            "--section",
            "0",
            "-o",
            out.to_str().unwrap(),
        ]);
        assert_cli_manifest_case(id, &err, exit);
    }
    let corrupt = data.join("corrupt-input.hwpx");
    let (err, _, exit) =
        run_json(&["fill", corrupt.to_str().unwrap(), "--set", "a=b", "-o", out.to_str().unwrap()]);
    assert_cli_manifest_case("fill.input_decode", &err, exit);
    assert!(!out.exists(), "a refused edit writes no output");
}

#[test]
// 이것을 실패시키는 것(W4): convert ops 의 decoder Core 갈래를 HWP5_CONVERT_FAILED 로 되돌리기
// (inspect 단계는 이 입력을 통과시키므로 convert-hwp5 도 ops 분류를 그대로 보인다).
fn w4_hwp5_decoder_core_shared_fixture() {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../hwpforge/tests/data");
    let source = data.join("hwp5-dangling-table-border-fill.hwp");
    let out = test_tmp().join("out.hwpx");
    let (err, _, exit) =
        run_json(&["convert-hwp5", source.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert_cli_manifest_case("hwp5.decode_core", &err, exit);
    assert!(!out.exists(), "a refused conversion writes no output");
    let pdf = test_tmp().join("out.pdf");
    let (err, _, exit) =
        run_json(&["to-pdf", source.to_str().unwrap(), "-o", pdf.to_str().unwrap()]);
    assert_cli_manifest_case("hwp5.to_pdf.decode_core", &err, exit);
    assert!(!pdf.exists(), "a refused render writes no output");
}

#[test]
fn w3_editors_share_decode_and_admission_fixtures() {
    // 이것을 실패시키는 것: E2/E4 cell·stamp admission 호출 목적을 Output으로 변경.
    // 이것을 실패시키는 것: F4 v2 admission 호출 목적을 Output으로 변경.
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../hwpforge/tests/data");
    let dir = test_tmp();
    let out = dir.join("out.hwpx");
    let map = dir.join("map.json");
    std::fs::write(&map, "[]").unwrap();
    for (fixture, ids) in [
        (
            "corrupt-input.hwpx",
            [
                "cell.input_decode",
                "structural.input_decode",
                "structural_delete.input_decode",
                "stamp.input_decode",
            ],
        ),
        (
            "edit-empty-section.hwpx",
            [
                "cell.admission",
                "structural.admission",
                "structural_delete.admission",
                "stamp.admission",
            ],
        ),
    ] {
        let input = data.join(fixture);
        let file = input.to_str().unwrap();
        let output = out.to_str().unwrap();
        for (id, args) in [
            (
                ids[0],
                vec![
                    "set-cell", file, "--table", "0", "--at", "0,0", "--text", "text", "-o", output,
                ],
            ),
            (
                ids[1],
                vec![
                    "insert-para",
                    file,
                    "--section",
                    "0",
                    "--anchor",
                    "0",
                    "--text",
                    "text",
                    "-o",
                    output,
                ],
            ),
            (ids[2], vec!["delete-para", file, "--section", "0", "--index", "0", "-o", output]),
            (ids[3], vec!["stamp", file, "--map", map.to_str().unwrap(), "-o", output]),
        ] {
            let (error, _, exit) = run_json(&args);
            assert_cli_manifest_case(id, &error, exit);
            assert!(!out.exists());
        }
    }
    let input = data.join("edit-empty-section.hwpx");
    let v2_map = data.join("stamp-v2-admission.json");
    let (error, _, exit) = run_json(&[
        "stamp",
        input.to_str().unwrap(),
        "--map",
        v2_map.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_cli_manifest_case("stamp.v2_admission", &error, exit);
    assert!(!out.exists());
    let input = data.join("corrupt-input.hwpx");
    let (error, _, exit) = run_json(&["stamp-plan", input.to_str().unwrap()]);
    assert_cli_manifest_case("stamp_plan.input_decode", &error, exit);
    for name in ["depth-table-42", "depth-table-33", "depth-group-32", "depth-xml-broken"] {
        let input = data.join(format!("{name}.hwpx"));
        let (error, _, exit) = run_json(&[
            "set-cell",
            input.to_str().unwrap(),
            "--table",
            "0",
            "--at",
            "0,0",
            "--text",
            "text",
            "-o",
            out.to_str().unwrap(),
        ]);
        assert_cli_manifest_case(&format!("cell.{name}"), &error, exit);
        assert!(!error["hint"].as_str().unwrap_or_default().contains("Report this as a bug."));
    }
}
