use super::*;
use std::io::Write as _;

#[test]
fn audit_hwp5_nonexistent_source() {
    let tmp = test_tmp();
    let out = tmp.join("output.hwpx");
    let (_, _, code) = run(&["audit-hwp5", "/nonexistent/file.hwp", out.to_str().unwrap()]);
    assert_eq!(code, 1);
}

#[test]
// 이것을 실패시키는 것: to-md/convert-hwp5 입력 I/O 종료를 2에서 1로 변경.
fn convert_hwp5_nonexistent_file() {
    let tmp = test_tmp();
    let out = tmp.join("missing.hwpx");

    let (_, _, code) = run(&["convert-hwp5", "/nonexistent/file.hwp", "-o", out.to_str().unwrap()]);
    assert_eq!(code, 2);
}

#[cfg(unix)]
#[test]
// 이것을 실패시키는 것: convert-hwp5 InputTooLarge 종료를 1에서 2로 변경.
fn convert_hwp5_fifo_over_the_size_cap_reports_input_too_large() {
    // `convert-hwp5` used to read its input path twice — an unbounded
    // `std::fs::read` inside the (now removed) `inspect_hwp5_file` call,
    // then a bounded `read_bounded` for conversion. That let an oversized
    // FIFO grow memory without limit through the first, unbounded read
    // (this test's chunk loop is comfortably over the 100 MB cap), and
    // would have hung a *finite* FIFO on the second `open` once the
    // original writer had already exited. The command now reads the FIFO
    // exactly once through `read_bounded`, so this exits promptly with the
    // canonical `INPUT_TOO_LARGE`/exit 1, matching regular-file preflight.
    let tmp = test_tmp();
    let out = tmp.join("never.hwpx");
    let fifo_path = tmp.join("oversized.hwp");
    create_fifo(&fifo_path);

    let writer_path = fifo_path.clone();
    let writer = std::thread::spawn(move || {
        let Ok(mut f) = std::fs::File::create(&writer_path) else { return };
        let chunk = vec![0u8; 1024 * 1024];
        // Comfortably over the 100 MB cap; `read_bounded`'s single capped
        // read stops consuming once it has confirmed the overflow, so
        // `write_all` reliably reports `BrokenPipe` and this loop breaks —
        // proving the writer thread does not outlive the reader.
        for _ in 0..150 {
            if f.write_all(&chunk).is_err() {
                break;
            }
        }
    });

    let (val, stderr, code) =
        run_json(&["convert-hwp5", fifo_path.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    writer.join().expect("writer thread panicked");

    assert_eq!(code, 1, "stderr: {stderr}, value: {val}");
    assert_eq!(val["code"], "INPUT_TOO_LARGE", "{val}");
    assert_cli_manifest_case("io.convert-hwp5.fifo_over_limit", &val, code);
    assert!(!out.exists());
}
