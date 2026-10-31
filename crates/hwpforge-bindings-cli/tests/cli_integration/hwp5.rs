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

mod input_editing {
    use serde_json::{json, Value};
    use std::io::{Cursor, Write};
    use std::path::Path;
    use std::process::Command;

    fn record(tag: u32, level: u32, payload: &[u8]) -> Vec<u8> {
        let mut bytes =
            (tag | (level << 10) | ((payload.len() as u32) << 20)).to_le_bytes().to_vec();
        bytes.extend(payload);
        bytes
    }

    fn paragraph(text: &str) -> Vec<u8> {
        let units: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let mut header = vec![0; 22];
        header[..4].copy_from_slice(&(text.encode_utf16().count() as u32).to_le_bytes());
        [record(0x42, 0, &header), record(0x43, 1, &units), record(0x44, 1, &[0; 8])].concat()
    }

    fn marker(code: u16, id: u32) -> Vec<u8> {
        [code.to_le_bytes().as_slice(), &id.to_le_bytes(), &[0; 8], &code.to_le_bytes()].concat()
    }
    fn field() -> Vec<u8> {
        let id = u32::from_be_bytes(*b"%clk");
        let mut text = marker(3, id);
        text.extend("기존값".encode_utf16().flat_map(u16::to_le_bytes));
        text.extend(marker(4, 0));
        let command = "Clickhere:set:0:Direction:wstring:0: HelpState:wstring:0:  ";
        let mut control = id.to_le_bytes().to_vec();
        control.extend([0; 4]);
        control.push(9);
        control.extend((command.encode_utf16().count() as u16).to_le_bytes());
        control.extend(command.encode_utf16().flat_map(u16::to_le_bytes));
        control.extend([0; 8]);
        let mut name = vec![0x1b, 2, 1, 0, 0, 0, 0, 0x40, 1, 0, 4, 0];
        name.extend("name".encode_utf16().flat_map(u16::to_le_bytes));
        [
            record(0x42, 0, &[0; 22]),
            record(0x43, 1, &text),
            record(0x44, 1, &[0; 8]),
            record(0x47, 1, &control),
            record(0x57, 2, &name),
        ]
        .concat()
    }
    fn table() -> Vec<u8> {
        let mut wire = paragraph("표");
        let mut control = vec![0; 40];
        control[..4].copy_from_slice(&u32::from_be_bytes(*b"tbl ").to_le_bytes());
        let mut table = vec![0; 20];
        table[4..8].copy_from_slice(&[1, 0, 1, 0]);
        let mut cell = vec![0; 32];
        cell[0] = 1;
        cell[12..16].copy_from_slice(&[1, 0, 1, 0]);
        cell[16..20].copy_from_slice(&9000i32.to_le_bytes());
        cell[20..24].copy_from_slice(&3000i32.to_le_bytes());
        wire.extend(record(0x47, 1, &control));
        wire.extend(record(0x4d, 2, &table));
        wire.extend(record(0x48, 2, &cell));
        wire.extend(record(0x42, 2, &[0; 22]));
        wire.extend(record(
            0x43,
            3,
            &"셀".encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<_>>(),
        ));
        wire.extend(record(0x44, 3, &[0; 8]));
        wire
    }

    fn source(loss: bool) -> Vec<u8> {
        let mut package = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        let mut header = vec![0; 256];
        header[..18].copy_from_slice(b"HWP Document File\0");
        header[32..36].copy_from_slice(&0x05000205u32.to_le_bytes());
        package.create_stream("/FileHeader").unwrap().write_all(&header).unwrap();
        let info =
            [record(0x15, 0, &[0; 68]), record(0x19, 0, &[0; 42]), record(0x1a, 0, &[0; 14])]
                .concat();
        package.create_stream("/DocInfo").unwrap().write_all(&info).unwrap();
        let mut section = [
            paragraph("이름: (   )"),
            paragraph("삭제 대상"),
            paragraph("마지막"),
            field(),
            table(),
        ]
        .concat();
        if loss {
            // Unknown control: a conversion warning that cannot be accepted silently.
            section.extend(record(0x47, 1, b"zzzz"));
        }
        package.create_storage("/BodyText").unwrap();
        package.create_stream("/BodyText/Section0").unwrap().write_all(&section).unwrap();
        package.into_inner().into_inner()
    }

    fn cli(args: &[&str]) -> (i32, Value) {
        cli_in(Path::new(env!("CARGO_MANIFEST_DIR")), args)
    }
    fn cli_in(dir: &Path, args: &[&str]) -> (i32, Value) {
        let out = Command::new(env!("CARGO_BIN_EXE_hwpforge"))
            .current_dir(dir)
            .args(args)
            .arg("--json")
            .output()
            .unwrap();
        let wire = if out.status.success() { &out.stdout } else { &out.stderr };
        let value = serde_json::from_slice(wire)
            .unwrap_or_else(|_| json!({"raw":String::from_utf8_lossy(wire)}));
        (out.status.code().unwrap(), value)
    }
    fn path(path: &Path) -> &str {
        path.to_str().unwrap()
    }

    #[test]
    // 이것을 실패시키는 것: 공용 HWP5 열기 또는 포함 읽기 명령의 변환 연결 삭제.
    fn hwp5_read_commands_match_explicit_conversion() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source.hwp");
        let converted = dir.path().join("converted.hwpx");
        std::fs::write(&input, source(false)).unwrap();
        let (code, value) = cli(&["convert-hwp5", path(&input), "-o", path(&converted)]);
        assert_eq!(code, 0, "{value}");
        for command in ["fields", "outline", "inspect", "validate", "stamp-plan"] {
            let (code, mut direct) = cli(&[command, path(&input)]);
            assert_eq!(code, 0, "{command}: {direct}");
            assert_eq!(direct["conversion"]["source_format"], "hwp5");
            direct.as_object_mut().unwrap().remove("conversion");
            let (code, mut explicit) = cli(&[command, path(&converted)]);
            assert_eq!(code, 0, "{command}: {explicit}");
            // inspect's input label is intentionally the user's original path.
            direct.as_object_mut().unwrap().remove("file");
            explicit.as_object_mut().unwrap().remove("file");
            assert_eq!(direct, explicit, "{command}");
        }
    }

    fn prepare(
        dir: &Path,
        loss: bool,
    ) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
        let input = dir.join("source.bin");
        let converted = dir.join("converted.hwpx");
        let map = dir.join("map.json");
        std::fs::write(&input, source(loss)).unwrap();
        let (code, value) = cli(&["convert-hwp5", path(&input), "-o", path(&converted)]);
        assert_eq!(code, 0, "{value}");
        let (code, plan) = cli(&["stamp-plan", path(&converted)]);
        assert_eq!(code, 0, "{plan}");
        let specs: Vec<Value> = plan["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            json!({
                "section":c["section"], "path":c["path"], "span":c["span"], "marker":c["marker"],
                "action":{"field":{"name":"stamped","hint":null}}
            })
        })
        .collect();
        assert!(!specs.is_empty(), "a nonempty stamp must change the document");
        std::fs::write(&map, serde_json::to_vec(&specs).unwrap()).unwrap();
        (input, converted, map)
    }
    fn edit_args<'a>(
        command: &'a str,
        input: &'a Path,
        output: &'a Path,
        map: &'a Path,
    ) -> Vec<&'a str> {
        let mut args = vec![command, path(input), "-o", path(output)];
        args.extend(match command {
            "fill" => vec!["--set", "name=새값"],
            "set-cell" => vec!["--table", "0", "--at", "0,0", "--text", "새셀"],
            "insert-para" => vec!["--section", "0", "--anchor", "1", "--text", "새문단"],
            "delete-para" => vec!["--section", "0", "--index", "1"],
            "stamp" => vec!["--map", path(map)],
            _ => panic!("unknown edit"),
        });
        args
    }
    const EDITS: [&str; 5] = ["fill", "set-cell", "insert-para", "delete-para", "stamp"];

    #[test]
    // 이것을 실패시키는 것: 편집 명령 하나의 변환 연결·허용 플래그·출력 metadata 삭제.
    fn edits_change_bytes_and_equal_explicit_conversion() {
        let dir = tempfile::tempdir().unwrap();
        let (input, converted, map) = prepare(dir.path(), false);
        for command in EDITS {
            let direct = dir.path().join(format!("{command}-direct.hwpx"));
            let explicit = dir.path().join(format!("{command}-explicit.hwpx"));
            let mut args = edit_args(command, &input, &direct, &map);
            let (code, value) = cli(&args);
            assert_eq!(code, 0, "default policy {command}: {value}");
            args.push("--accept-conversion-loss");
            let (code, value) = cli(&args);
            assert_eq!(code, 0, "{command}: {value}");
            assert_eq!(value["conversion"]["source_format"], "hwp5");
            let (code, value) = cli(&edit_args(command, &converted, &explicit, &map));
            assert_eq!(code, 0, "{command}: {value}");
            let direct_bytes = std::fs::read(&direct).unwrap();
            assert_eq!(direct_bytes, std::fs::read(&explicit).unwrap(), "{command}");
            assert_ne!(direct_bytes, std::fs::read(&converted).unwrap(), "{command} must edit");
            // 이것을 실패시키는 것: 쓰기는 성공하지만 요청한 편집 내용을 적용하지 않음.
            let flags: &[&str] = match command {
                "fill" => &["--field", "name"],
                "set-cell" => &["--table", "0"],
                _ => &["--section", "0"],
            };
            let mut read = vec!["read", path(&direct)];
            read.extend_from_slice(flags);
            let (code, content) = cli(&read);
            assert_eq!(code, 0, "{content}");
            let content = content.to_string();
            match command {
                "fill" => assert!(content.contains("새값"), "{content}"),
                "set-cell" => assert!(content.contains("새셀"), "{content}"),
                "insert-para" => assert!(content.contains("새문단"), "{content}"),
                "delete-para" => assert!(!content.contains("삭제 대상"), "{content}"),
                "stamp" => {
                    let (code, fields) = cli(&["fields", path(&direct)]);
                    assert_eq!(code, 0, "{fields}");
                    assert!(fields["fields"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|field| field["name"] == "stamped" && field["fillable"] == true));
                }
                _ => unreachable!(),
            }
        }
    }

    #[test]
    // 이것을 실패시키는 것: 손실 거부 제거, 경고 목록 누락, 허용 무시 또는 편집 admission 우회.
    fn loss_refusal_and_acceptance_preserve_edit_validation() {
        let dir = tempfile::tempdir().unwrap();
        let (input, _, map) = prepare(dir.path(), true);
        for command in EDITS {
            let output = dir.path().join(format!("{command}.hwpx"));
            let args = edit_args(command, &input, &output, &map);
            let (code, value) = cli(&args);
            assert_eq!(
                (code, value["code"].as_str()),
                (1, Some("HWP5_CONVERSION_LOSS")),
                "{command}: {value}"
            );
            assert!(!value["details"]["warnings"].as_array().unwrap().is_empty());
            assert!(!output.exists());
            let mut accepted = args.clone();
            accepted.push("--accept-conversion-loss");
            let (code, value) = cli(&accepted);
            assert_eq!(code, 0, "{command}: {value}");
        }
        let output = dir.path().join("invalid.hwpx");
        for accept in [false, true] {
            let mut args = vec!["set-cell", path(&input), "-o", path(&output)];
            if accept {
                args.push("--accept-conversion-loss");
            }
            let (code, value) = cli(&args);
            assert_eq!((code, value["code"].as_str()), (1, Some("INVALID_SET_CELL_ARGS")));
            assert!(!output.exists());
        }
    }

    #[test]
    // 이것을 실패시키는 것: .hwp 검사 제거·대소문자 검사 축소·HWP5만 확장자 검사.
    fn document_hwp_extension_is_rejected_for_both_input_formats() {
        let dir = tempfile::tempdir().unwrap();
        let (input, converted, map) = prepare(dir.path(), false);
        for source in [&input, &converted] {
            for command in EDITS {
                for extension in ["hwp", "HwP"] {
                    let output = dir.path().join(format!("out.{extension}"));
                    let mut args = edit_args(command, source, &output, &map);
                    args.push("--accept-conversion-loss");
                    let (code, value) = cli(&args);
                    assert_eq!(
                        (code, value["code"].as_str()),
                        (1, Some("INVALID_EXTENSION")),
                        "{command}: {value}"
                    );
                    assert!(!output.exists());
                }
            }
        }
    }

    #[test]
    // 이것을 실패시키는 것: 입력 동일 파일 검사 제거·경로 문자열 비교 대체·manifest 누락·HWPX도 보호.
    fn input_aliases_are_protected_and_hwpx_in_place_is_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let (input, converted, map) = prepare(dir.path(), false);
        let original = std::fs::read(&input).unwrap();
        let hard = dir.path().join("hard.hwpx");
        std::fs::hard_link(&input, &hard).unwrap();
        let mut aliases =
            vec![input.clone(), dir.path().join("./source.bin"), hard, "source.bin".into()];
        #[cfg(unix)]
        {
            let sym = dir.path().join("symbolic.hwpx");
            std::os::unix::fs::symlink(&input, &sym).unwrap();
            aliases.push(sym);
        }
        let upper = dir.path().join("SOURCE.BIN");
        if upper.exists() {
            aliases.push(upper);
        }
        for command in EDITS {
            for alias in &aliases {
                let mut args = edit_args(command, &input, alias, &map);
                args.push("--accept-conversion-loss");
                let (code, value) = cli_in(dir.path(), &args);
                assert_eq!(
                    (code, value["code"].as_str()),
                    (1, Some("INVALID_OUTPUT_PATH")),
                    "{command}: {value}"
                );
                assert_eq!(std::fs::read(&input).unwrap(), original);
            }
            let inplace = dir.path().join(format!("{command}-inplace.hwpx"));
            std::fs::copy(&converted, &inplace).unwrap();
            let (code, value) = cli(&edit_args(command, &inplace, &inplace, &map));
            assert_eq!(code, 0, "{command}: {value}");
            assert_ne!(std::fs::read(&inplace).unwrap(), std::fs::read(&converted).unwrap());
        }
        for alias in &aliases {
            let output = dir.path().join("stamped.hwpx");
            let mut args = edit_args("stamp", &input, &output, &map);
            args.extend(["--manifest", path(alias), "--accept-conversion-loss"]);
            let (code, value) = cli_in(dir.path(), &args);
            assert_eq!((code, value["code"].as_str()), (1, Some("INVALID_OUTPUT_PATH")), "{value}");
            assert_eq!(std::fs::read(&input).unwrap(), original);
            assert!(!output.exists());
        }
        let missing = dir.path().join("missing/out.hwpx");
        let mut args = edit_args("stamp", &input, &missing, &map);
        args.extend(["--manifest", path(&missing)]);
        let (code, value) = cli_in(dir.path(), &args);
        assert_eq!((code, value["code"].as_str()), (1, Some("MANIFEST_PATH_CONFLICT")));
    }

    #[test]
    // 이것을 실패시키는 것: to-json/to-md 본문에 conversion 삽입, read 경로 누락, diff 역할 병합.
    fn exports_and_diff_keep_document_bodies_separate() {
        let dir = tempfile::tempdir().unwrap();
        let (input, converted, _) = prepare(dir.path(), false);
        for flags in [vec!["--section", "0"], vec!["--table", "0"], vec!["--field", "name"]] {
            let mut args = vec!["read", path(&input)];
            args.extend(&flags);
            let (code, mut direct) = cli(&args);
            assert_eq!(code, 0, "{direct}");
            assert!(direct.as_object_mut().unwrap().remove("conversion").is_some());
            args[1] = path(&converted);
            let (code, explicit) = cli(&args);
            assert_eq!(code, 0);
            assert_eq!(direct, explicit);
        }
        for (command, ext) in [("to-json", "json"), ("to-md", "md")] {
            let direct = dir.path().join(format!("direct.{ext}"));
            let explicit = dir.path().join(format!("explicit.{ext}"));
            for (source, output) in [(&input, &direct), (&converted, &explicit)] {
                let (code, value) = cli(&[command, path(source), "-o", path(output)]);
                assert_eq!(code, 0, "{value}");
                assert_eq!(value.get("conversion").is_some(), source == &input);
            }
            assert_eq!(std::fs::read(&direct).unwrap(), std::fs::read(&explicit).unwrap());
        }
        for (base, revised, roles) in [
            (&input, &converted, vec!["base"]),
            (&converted, &input, vec!["revised"]),
            (&input, &input, vec!["base", "revised"]),
        ] {
            let output = dir.path().join("diff.json");
            let (code, value) = cli(&["diff", path(base), path(revised), "-o", path(&output)]);
            assert_eq!(code, 0, "{value}");
            assert_eq!(value["conversion"].as_object().unwrap().len(), roles.len());
            for role in roles {
                assert_eq!(value["conversion"][role]["source_format"], "hwp5");
            }
            assert_eq!(
                serde_json::from_slice::<Value>(&std::fs::read(&output).unwrap()).unwrap(),
                value["diff"]
            );
        }
        let (code, plan) = cli(&["stamp-plan", path(&input)]);
        assert_eq!(code, 0);
        let text: Vec<_> = plan["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                json!({
                    "section": c["section"], "path": c["path"], "span": c["span"],
                    "marker": c["marker"], "action": {"field": {"name": "v2", "hint": null}}
                })
            })
            .collect();
        let map = dir.path().join("v2.json");
        std::fs::write(
        &map,
        json!({"schema_version":2,"source_sha256":plan["source_sha256"],"text":text,"cells":[]})
            .to_string(),
    )
    .unwrap();
        let output = dir.path().join("v2.hwpx");
        let mut args = edit_args("stamp", &input, &output, &map);
        args.push("--accept-conversion-loss");
        let (code, value) = cli(&args);
        assert_eq!(code, 0, "{value}");
        assert_eq!(value["stamped_text"].as_array().unwrap().len(), 1);
    }

    #[test]
    // 이것을 실패시키는 것: 비CFB 선행 오류 변경, CFB 접두사 ZIP 강제 변환, 손상 CFB 변환 누락.
    fn input_classification_keeps_non_cfb_argument_precedence() {
        let dir = tempfile::tempdir().unwrap();
        let (input, converted, _) = prepare(dir.path(), false);
        let output = dir.path().join("out.hwpx");
        for bytes in [b"".as_slice(), b"PK broken", b"not a document", &[0xd0, 0xcf]] {
            std::fs::write(&input, bytes).unwrap();
            let (code, value) = cli(&["set-cell", path(&input), "-o", path(&output)]);
            assert_eq!((code, value["code"].as_str()), (1, Some("INVALID_SET_CELL_ARGS")));
        }
        let cfb = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
        std::fs::write(&input, cfb).unwrap();
        let (code, value) = cli(&["set-cell", path(&input), "-o", path(&output)]);
        assert_eq!((code, value["code"].as_str()), (2, Some("HWP5_DECODE_FAILED")));
        for prefix in [b"prefix".as_slice(), cfb.as_slice()] {
            std::fs::write(&input, [prefix, &std::fs::read(&converted).unwrap()].concat()).unwrap();
            let (code, value) = cli(&["fields", path(&input)]);
            assert_eq!(code, 0, "{value}");
            assert!(value.get("conversion").is_none());
        }
    }

    #[cfg(unix)]
    #[test]
    // 이것을 실패시키는 것: try_exists 오류 삼키기, 쓰기 전용 핸들 제거·truncate 추가, 미존재 거부.
    fn output_identity_errors_and_write_only_files_preserve_contents() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let (input, _, map) = prepare(dir.path(), true);
        for command in EDITS {
            let output = dir.path().join("loop.hwpx");
            symlink(&output, &output).unwrap();
            let mut args = edit_args(command, &input, &output, &map);
            args.push("--accept-conversion-loss");
            let (code, value) = cli(&args);
            assert_eq!(value["code"], "FILE_WRITE_FAILED", "{command}: {value}");
            assert!(value["message"].as_str().unwrap().contains("Cannot verify output"));
            assert_eq!(code, if ["insert-para", "delete-para"].contains(&command) { 2 } else { 1 });
            std::fs::remove_file(&output).unwrap();
            std::fs::write(&output, b"keep me").unwrap();
            std::fs::set_permissions(&output, std::fs::Permissions::from_mode(0o200)).unwrap();
            let (code, value) = cli(&edit_args(command, &input, &output, &map));
            std::fs::set_permissions(&output, std::fs::Permissions::from_mode(0o600)).unwrap();
            assert_eq!(
                (code, value["code"].as_str()),
                (1, Some("HWP5_CONVERSION_LOSS")),
                "{value}"
            );
            assert_eq!(std::fs::read(&output).unwrap(), b"keep me");
            std::fs::set_permissions(&output, std::fs::Permissions::from_mode(0o200)).unwrap();
            let (code, value) = cli(&args);
            std::fs::set_permissions(&output, std::fs::Permissions::from_mode(0o600)).unwrap();
            assert_eq!(code, 0, "{command}: {value}");
            assert!(std::fs::read(&output).unwrap().starts_with(b"PK"));
            std::fs::remove_file(&output).unwrap();
            // A dangling symlink is a missing target; the existing writer creates it.
            let target = dir.path().join("target.hwpx");
            symlink(&target, &output).unwrap();
            let (code, value) = cli(&args);
            assert_eq!(code, 0, "{value}");
            assert!(target.exists());
            std::fs::remove_file(&output).unwrap();
            std::fs::remove_file(&target).unwrap();
        }
    }

    #[test]
    // 이것을 실패시키는 것: 사람용 변환 경고·.hwpx 안내 제거 또는 JSON stderr에 비JSON 진단 추가.
    fn human_conversion_warnings_name_the_output_format() {
        let dir = tempfile::tempdir().unwrap();
        let (input, _, _) = prepare(dir.path(), true);
        let out = Command::new(env!("CARGO_BIN_EXE_hwpforge"))
            .args(["fields", path(&input)])
            .output()
            .unwrap();
        assert!(out.status.success());
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert!(stderr.contains(".hwpx"));
        assert!(stderr.contains("DROPPED_CONTROL"));
        let out = Command::new(env!("CARGO_BIN_EXE_hwpforge"))
            .args(["fields", path(&input), "--json"])
            .output()
            .unwrap();
        assert!(out.status.success());
        assert!(out.stderr.is_empty());
    }

    #[test]
    // 이것을 실패시키는 것: 읽기 출력 가드 누락, diff 한 입력 누락, 이미지 쓰기 전 검사 지연.
    fn read_output_aliases_preserve_all_inputs_and_artifacts() {
        for (command, role, ext) in [
            ("diff", "base", "json"),
            ("diff", "revised", "json"),
            ("to-json", "input", "json"),
            ("to-md", "input", "md"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let input = dir.path().join(format!("source.{ext}"));
            let other = dir.path().join("other.hwp");
            let original = source(false);
            std::fs::write(&input, &original).unwrap();
            std::fs::write(&other, &original).unwrap();
            let hard = dir.path().join(format!("hard.{ext}"));
            std::fs::hard_link(&input, &hard).unwrap();
            let mut aliases = vec![
                input.clone(),
                format!("source.{ext}").into(),
                dir.path().join(format!("./source.{ext}")),
                hard,
            ];
            #[cfg(unix)]
            {
                let sym = dir.path().join(format!("link.{ext}"));
                std::os::unix::fs::symlink(&input, &sym).unwrap();
                aliases.push(sym);
            }
            let upper = dir.path().join(format!("SOURCE.{ext}"));
            if upper.exists() {
                aliases.push(upper);
            }
            for alias in aliases {
                let mut args = match role {
                    "base" => vec![command, path(&input), path(&other)],
                    "revised" => vec![command, path(&other), path(&input)],
                    _ => vec![command, path(&input)],
                };
                args.extend(["-o", path(&alias)]);
                let (code, value) = cli_in(dir.path(), &args);
                assert_eq!(
                    (code, value["code"].as_str()),
                    (1, Some("INVALID_OUTPUT_PATH")),
                    "{command}/{role}/{alias:?}: {value}"
                );
                assert_eq!(std::fs::read(&input).unwrap(), original);
                assert_eq!(std::fs::read(&other).unwrap(), original);
                assert!(!dir.path().join("images").exists());
            }
        }
    }

    #[cfg(unix)]
    #[test]
    // 이것을 실패시키는 것: 일반 파일 검사 제거로 FIFO 정체성 조회가 쓰기 전에 멈춤.
    fn fifo_output_reaches_loss_rejection_without_opening_the_pipe() {
        let dir = tempfile::tempdir().unwrap();
        let (input, _, map) = prepare(dir.path(), true);
        let output = dir.path().join("output.hwpx");
        assert!(Command::new("mkfifo").arg(&output).status().unwrap().success());
        let child = Command::new(env!("CARGO_BIN_EXE_hwpforge"))
            .args(edit_args("fill", &input, &output, &map))
            .arg("--json")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let out = wait_for_fifo_preflight(child);
        assert_eq!(out.status.code(), Some(1));
        let value: Value = serde_json::from_slice(&out.stderr).unwrap();
        assert_eq!(value["code"], "HWP5_CONVERSION_LOSS");
    }

    #[cfg(unix)]
    fn wait_for_fifo_preflight(mut child: std::process::Child) -> std::process::Output {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if std::time::Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("FIFO identity preflight blocked");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        child.wait_with_output().unwrap()
    }

    #[cfg(unix)]
    #[test]
    // 이것을 실패시키는 것: 입력의 일반 파일 검사 제거로 FIFO를 두 번째 열며 대기.
    fn fifo_input_reaches_loss_rejection_without_reopening_the_pipe() {
        let dir = tempfile::tempdir().unwrap();
        let (input, _, map) = prepare(dir.path(), true);
        let bytes = std::fs::read(&input).unwrap();
        std::fs::remove_file(&input).unwrap();
        assert!(Command::new("mkfifo").arg(&input).status().unwrap().success());
        let output = dir.path().join("output.hwpx");
        std::fs::write(&output, b"original output").unwrap();
        let writer_path = input.clone();
        let writer = std::thread::spawn(move || {
            std::fs::OpenOptions::new()
                .write(true)
                .open(writer_path)
                .unwrap()
                .write_all(&bytes)
                .unwrap();
        });
        let child = Command::new(env!("CARGO_BIN_EXE_hwpforge"))
            .args(edit_args("fill", &input, &output, &map))
            .arg("--json")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let out = wait_for_fifo_preflight(child);
        writer.join().unwrap();
        assert_eq!(out.status.code(), Some(1));
        let value: Value = serde_json::from_slice(&out.stderr).unwrap();
        assert_eq!(value["code"], "HWP5_CONVERSION_LOSS");
        assert_eq!(std::fs::read(&output).unwrap(), b"original output");
    }

    fn source_with_image() -> Vec<u8> {
        let mut package = cfb::CompoundFile::open(Cursor::new(source(false))).unwrap();
        let mut bin = vec![1, 0, 1, 0, 3, 0];
        bin.extend("png".encode_utf16().flat_map(u16::to_le_bytes));
        {
            use std::io::{Seek, SeekFrom};
            let mut info = package.open_stream("/DocInfo").unwrap();
            info.seek(SeekFrom::End(0)).unwrap();
            info.write_all(&record(0x12, 0, &bin)).unwrap();
        }
        let mut wire = paragraph("그림");
        let mut control = vec![0; 24];
        control[..4].copy_from_slice(&u32::from_be_bytes(*b"gso ").to_le_bytes());
        control[4] = 1;
        control[16..20].copy_from_slice(&7200u32.to_le_bytes());
        control[20..24].copy_from_slice(&7200u32.to_le_bytes());
        let mut shape = b"cip$cip$".to_vec();
        shape.extend([0; 8]);
        shape.extend(0u16.to_le_bytes());
        shape.extend(1u16.to_le_bytes());
        shape.extend(7200u32.to_le_bytes());
        shape.extend(7200u32.to_le_bytes());
        shape.extend([0; 22]);
        shape.extend(1u16.to_le_bytes());
        for _ in 0..3 {
            for v in [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0] {
                shape.extend(v.to_le_bytes());
            }
        }
        shape.extend([0; 17]);
        let mut picture = vec![0; 73];
        picture[71] = 1;
        wire.extend(record(0x47, 1, &control));
        wire.extend(record(0x4c, 2, &shape));
        wire.extend(record(0x55, 2, &picture));
        package.create_stream("/BodyText/Section0").unwrap().write_all(&wire).unwrap();
        package.create_storage("/BinData").unwrap();
        package
            .create_stream("/BinData/BIN0001.png")
            .unwrap()
            .write_all(b"\x89PNG\r\n\x1a\n")
            .unwrap();
        package.into_inner().into_inner()
    }

    #[cfg(unix)]
    #[test]
    // 이것을 실패시키는 것: 이미지 출력 보호 누락 또는 검사 전에 Markdown·다른 이미지 기록.
    fn markdown_preflights_image_aliases_before_writing_any_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source.hwp");
        let original = source_with_image();
        std::fs::write(&input, &original).unwrap();
        let probe = dir.path().join("probe");
        let (code, value) = cli(&["to-md", path(&input), "-o", path(&probe)]);
        assert_eq!(code, 0, "{value}");
        assert_eq!(value["images"], 1, "fixture must export an image: {value}");
        let name =
            std::fs::read_dir(probe.join("images")).unwrap().next().unwrap().unwrap().file_name();
        for kind in ["absolute", "relative", "symlink", "hardlink", "case"] {
            let out = dir.path().join(kind);
            let images = out.join("images");
            std::fs::create_dir_all(&images).unwrap();
            let image = images.join(&name);
            let source = match kind {
                "absolute" | "relative" => {
                    std::fs::write(&image, &original).unwrap();
                    image.clone()
                }
                "hardlink" => {
                    std::fs::hard_link(&input, &image).unwrap();
                    input.clone()
                }
                "case" => {
                    let upper = images.join(name.to_string_lossy().to_uppercase());
                    std::fs::write(&upper, &original).unwrap();
                    if !image.exists() {
                        continue;
                    }
                    upper
                }
                _ => {
                    std::os::unix::fs::symlink(&input, &image).unwrap();
                    input.clone()
                }
            };
            let source_arg =
                if kind == "relative" { source.strip_prefix(dir.path()).unwrap() } else { &source };
            let md = out.join("result.md");
            let (code, value) = cli_in(dir.path(), &["to-md", path(source_arg), "-o", path(&md)]);
            assert_eq!(
                (code, value["code"].as_str()),
                (1, Some("INVALID_OUTPUT_PATH")),
                "{kind}: {value}"
            );
            assert_eq!(std::fs::read(&source).unwrap(), original);
            assert!(!md.exists());
            assert_eq!(std::fs::read_dir(&images).unwrap().count(), 1);
        }
        let md = dir.path().join("source.md");
        std::os::unix::fs::symlink(&input, &md).unwrap();
        let (code, value) = cli(&["to-md", path(&input)]);
        assert_eq!((code, value["code"].as_str()), (1, Some("INVALID_OUTPUT_PATH")), "{value}");
        assert!(!dir.path().join("images").exists());
        assert_eq!(std::fs::read(&input).unwrap(), original);
    }

    #[test]
    // 이것을 실패시키는 것: 원본 보호를 HWPX 읽기 출력에도 적용.
    fn hwpx_read_outputs_keep_existing_in_place_behavior() {
        let dir = tempfile::tempdir().unwrap();
        let (_, converted, _) = prepare(dir.path(), false);
        for (command, ext) in [("diff", "json"), ("to-json", "json"), ("to-md", "md")] {
            let input = dir.path().join(format!("inplace.{ext}"));
            std::fs::copy(&converted, &input).unwrap();
            let mut args = vec![command, path(&input)];
            if command == "diff" {
                args.push(path(&converted));
            }
            args.extend(["-o", path(&input)]);
            let (code, value) = cli(&args);
            assert_eq!(code, 0, "{command}: {value}");
            assert_ne!(std::fs::read(&input).unwrap(), std::fs::read(&converted).unwrap());
        }
    }
}
