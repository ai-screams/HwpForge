#[cfg(feature = "resource-experiment")]
#[path = "../examples/support/resource_runner.rs"]
mod resource_runner;
// Default comparison exercises shared production statements with no configuration.
use std::path::{Path, PathBuf};
fn fixtures(dir: &Path, paths: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            fixtures(&path, paths);
        } else if path.extension().is_some_and(|e| e == "hwpx") {
            paths.push(path);
        }
    }
}
fn decoded(bytes: &[u8]) -> serde_json::Value {
    match hwpforge_smithy_hwpx::HwpxDecoder::decode(bytes) {
        Ok(d) => serde_json::json!({
            "document":d.document, "styles":d.style_store, "images":d.image_store,
            "validation":format!("{:?}",d.document.clone().validate().map(|_| ())),
            "warnings":format!("{:?}",d.warnings)
        }),
        Err(e) => serde_json::json!({"error":e.to_string()}),
    }
}
// 이것을 실패시키는 것: feature 경로에서 XML 첫 바이트 또는 검증 결과 변경, 예산을 1바이트로 낮추기.
#[test]
fn public_fixture_equivalence() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    let mut paths = Vec::new();
    fixtures(&root, &mut paths);
    paths.sort();
    let mut baseline = Vec::new();
    for path in paths {
        let bytes = std::fs::read(&path).unwrap();
        let expected = decoded(&bytes);
        #[cfg(feature = "resource-experiment")]
        {
            use hwpforge_core::resource_experiment::*;
            for timing in [Timing::Measured, Timing::Post] {
                for scope in [Scope::Local, Scope::Document] {
                    set_experiment(Experiment { timing, scope, budget: 8 * 1024 * 1024 });
                    assert_eq!(decoded(&bytes), expected, "{}", path.display());
                    clear_experiment();
                }
            }
        }
        baseline.push((path.strip_prefix(&root).unwrap().display().to_string(), expected));
    }
    if let Ok(path) = std::env::var("RESOURCE_BASELINE") {
        let value = serde_json::to_value(baseline).unwrap();
        if !cfg!(feature = "resource-experiment") {
            std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        }
        #[cfg(feature = "resource-experiment")]
        assert_eq!(
            value,
            serde_json::from_slice::<serde_json::Value>(&std::fs::read(path).unwrap()).unwrap()
        );
    }
}

#[cfg(feature = "resource-experiment")]
mod recovery {
    use hwpforge_core::{Document, ImageStore, PageSettings, Paragraph, Run, Section};
    use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
    use hwpforge_smithy_hwpx::{style_store_for_preset, HwpxDecoder, HwpxEncoder};
    use std::io::{Cursor, Read, Write};
    fn begin(id: u32, kind: &str) -> String {
        format!(
            r#"<hp:ctrl><hp:fieldBegin id="{id}" type="{kind}" name="본문" editable="1" dirty="0" zorder="-1" fieldid="627469685" metaTag=""><hp:parameters cnt="5" name=""><hp:integerParam name="Prop">8</hp:integerParam><hp:stringParam name="Command">=SUM(A1:A2)??%g,;;3</hp:stringParam><hp:stringParam name="Formula">=SUM(A1:A2)</hp:stringParam><hp:stringParam name="ResultFormat">%g,</hp:stringParam><hp:stringParam name="LastResult">3</hp:stringParam></hp:parameters></hp:fieldBegin></hp:ctrl>"#
        )
    }
    fn run(body: &str) -> String {
        format!(r#"<hp:run charPrIDRef="0">{body}</hp:run>"#)
    }
    fn para(body: &str, lines: &[u32]) -> String {
        let cache = if lines.is_empty() {
            String::new()
        } else {
            format!("<hp:linesegarray>{}</hp:linesegarray>", lines.iter().map(|n| format!(r#"<hp:lineseg textpos="{n}" vertpos="0" vertsize="1000" textheight="1000" baseline="850" spacing="600" horzpos="0" horzsize="40000" flags="393216"/>"#)).collect::<String>())
        };
        format!(
            r#"<hp:p id="0" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0">{body}{cache}</hp:p>"#
        )
    }
    fn package(body: &str) -> Vec<u8> {
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::text("앞", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        ));
        let styles = style_store_for_preset("default").unwrap();
        let bytes =
            HwpxEncoder::encode(&doc.validate().unwrap(), &styles, &ImageStore::new()).unwrap();
        let mut source = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..source.len() {
            let mut entry = source.by_index(i).unwrap();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            if entry.name().expect("ZIP entry name") == "Contents/section0.xml" {
                let xml = String::from_utf8(bytes).unwrap();
                bytes = xml.replace("</hs:sec>", &format!("{body}</hs:sec>")).into_bytes();
            }
            out.start_file(
                entry.name().expect("ZIP entry name").to_string(),
                zip::write::SimpleFileOptions::default().compression_method(entry.compression()),
            )
            .unwrap();
            out.write_all(&bytes).unwrap();
        }
        out.finish().unwrap().into_inner()
    }

    // 이것을 실패시키는 것: 항상 성공 또는 첫 거부를 실패로 기록하기, 오류 코드 기록 삭제.
    #[test]
    fn runner_records_api_result_and_swallowed_rejection() {
        use hwpforge_core::resource_experiment::*;
        let bytes = package(&para(&run(&(begin(7, "FORMULA") + "<hp:t>open</hp:t>")), &[]));
        let mut zip = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
        let mut budget = 0;
        // 디코더 읽기 순서가 바뀌면 이 두 목록을 함께 고친다
        for name in [
            "mimetype",
            "Contents/content.hpf",
            "META-INF/container.xml",
            "Contents/header.xml",
            "Contents/content.hpf",
            "Contents/content.hpf",
            "Contents/section0.xml",
        ] {
            budget += zip.by_name(name).unwrap().size();
        }
        for (input, entrypoint, ok, reject) in [
            (b"hello".as_slice(), "md-decode", true, false),
            (b"x[^a]\n\n[^a]: abcdefgh\n".as_slice(), "md-decode", false, true),
            (b"invalid".as_slice(), "hwpx-decode", false, false),
        ] {
            let rows = super::resource_runner::run(
                input,
                entrypoint,
                Experiment { timing: Timing::Measured, scope: Scope::Document, budget: 1 },
                "api",
            )
            .unwrap();
            let row = rows.iter().find(|r| r["stage"] == "call_complete").unwrap();
            assert_eq!(row["api_ok"], ok);
            assert!(row.get("result_ok").is_none());
            assert!(row.get("first_reject_q").is_none());
            assert_eq!(row["error_code"].is_null(), ok);
            assert_eq!(!row["Q_at_first_reject"].is_null(), reject);
        }
        for timing in [Timing::Measured, Timing::Post] {
            for (limit, ok, reject) in
                [(8 * 1024 * 1024, true, false), (1, false, true), (budget, true, true)]
            {
                let rows = super::resource_runner::run(
                    &bytes,
                    "hwpx-decode",
                    Experiment { timing, scope: Scope::Document, budget: limit },
                    "recovery",
                )
                .unwrap();
                let row = rows.iter().find(|r| r["stage"] == "call_complete").unwrap();
                assert_eq!(row["api_ok"], ok);
                assert!(row.get("result_ok").is_none());
                assert!(row.get("first_reject_q").is_none());
                assert_eq!(row["error_code"].is_null(), ok);
                assert!(row.get("error_detail").is_none());
                let detail = super::resource_runner::error_detail(
                    &bytes,
                    "hwpx-decode",
                    Experiment { timing, scope: Scope::Document, budget: limit },
                )
                .unwrap();
                assert_eq!(detail.is_none(), ok);
                assert_eq!(!row["Q_at_first_reject"].is_null(), reject);
                if let Ok(dir) = std::env::var("RESOURCE_SMOKE_DIR") {
                    std::fs::create_dir_all(&dir).unwrap();
                    std::fs::write(
                        format!("{dir}/{timing:?}-{limit}.json"),
                        serde_json::to_vec_pretty(&rows).unwrap(),
                    )
                    .unwrap();
                    std::fs::write(format!("{dir}/open-field.hwpx"), &bytes).unwrap();
                }
            }
        }
    }

    // 이것을 실패시키는 것: 두 번째 Package에서 Q를 패키지 누적으로 덮어쓰기.
    #[test]
    fn second_package_keeps_completed_output() {
        use hwpforge_core::resource_experiment::*;
        let bytes = package(&para(&run(&(begin(7, "FORMULA") + "<hp:t>open</hp:t>")), &[]));
        let mut zip = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
        let header = zip.by_name("Contents/header.xml").unwrap().size();
        let minimum = header + zip.by_name("Contents/section0.xml").unwrap().size();
        for timing in [Timing::Measured, Timing::Post] {
            set_experiment(Experiment {
                timing,
                scope: Scope::Document,
                budget: 1024 * 1024 * 1024,
            });
            HwpxDecoder::decode(&bytes).unwrap();
            assert!(snapshot().q >= minimum, "{:?} < {minimum}", snapshot());
            clear_experiment();
        }
    }
}
