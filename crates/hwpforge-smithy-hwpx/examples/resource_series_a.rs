//! Generate packages with exact section output sizes.
//! 계열 A 는 예산이 약 166 KiB 미만이면 예산 이내 층이 성립하지 않는다(metadata 읽기가 section 수에 따라 약 14–17 KB).
#[path = "support/plain.rs"]
mod plain;
use hwpforge_core::{Document, ImageStore};
use hwpforge_smithy_hwpx::{
    style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore},
    HwpxEncoder,
};
use sha2::{Digest, Sha256};

fn encode(lengths: &[usize]) -> Vec<u8> {
    let mut doc = Document::new();
    for n in lengths {
        doc.add_section(plain::section(vec![plain::p(&"a".repeat(*n), 0, 0)]));
    }
    let mut styles = HwpxStyleStore::with_default_fonts("Arial");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    HwpxEncoder::encode(&doc.validate().unwrap(), &styles, &ImageStore::new()).unwrap()
}
fn costs(bytes: &[u8]) -> (Vec<usize>, Vec<usize>) {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let sizes: std::collections::BTreeMap<_, _> = (0..zip.len())
        .map(|i| {
            let file = zip.by_index(i).unwrap();
            (file.name().unwrap().into_owned(), file.size() as usize)
        })
        .collect();
    // Decoder reads mimetype, content for ordering, container, header, content
    // for metadata, content for master-page references, then each section.
    // 디코더 읽기 순서가 바뀌면 crates/hwpforge/tests/resource_equivalence.rs의 목록도 함께 고친다.
    let mut units = [
        "mimetype",
        "Contents/content.hpf",
        "META-INF/container.xml",
        "Contents/header.xml",
        "Contents/content.hpf",
        "Contents/content.hpf",
    ]
    .iter()
    .filter_map(|p| sizes.get(*p).copied())
    .collect::<Vec<_>>();
    let mut sections = sizes
        .iter()
        .filter(|(p, _)| p.starts_with("Contents/section") && p.ends_with(".xml"))
        .map(|(p, n)| {
            (
                p.trim_start_matches("Contents/section")
                    .trim_end_matches(".xml")
                    .parse::<usize>()
                    .unwrap(),
                *n,
            )
        })
        .collect::<Vec<_>>();
    sections.sort_by_key(|(index, _)| *index);
    let sections = sections.into_iter().map(|(_, n)| n).collect::<Vec<_>>();
    units.extend(&sections);
    (sections, units)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let out = std::path::Path::new(&args[1]);
    let b: usize = args[2].parse()?;
    assert_eq!(args[3], "--base-commit");
    let base_commit = &args[4];
    std::fs::create_dir_all(out)?;
    let mut manifest = Vec::new();
    for c in grid(b) {
        let targets = &c.units;
        let split = c.split;
        let group = c.layer;
        let case_id = format!("a-{}-{split}-{}", group.to_ascii_lowercase(), c.index);
        let seed = encode(&vec![1; targets.len()]);
        let (sections, _) = costs(&seed);
        // ASCII 'a' needs no XML escaping: each added character adds exactly one
        // byte to section XML. Measure the encoder's fixed tags with one character,
        // then subtract that overhead; do not substitute text length for XML size.
        let lengths = targets
            .iter()
            .zip(&sections)
            .map(|(target, base)| {
                target.checked_sub(base - 1).expect("budget too small for XML overhead")
            })
            .collect::<Vec<_>>();
        let bytes = encode(&lengths);
        let (actual, units) = costs(&bytes);
        let total: usize = units.iter().sum();
        // 이것을 실패시키는 것: section을 번호 대신 이름 사전순으로 수집하기.
        assert_eq!(&actual, targets);
        assert_eq!(actual.iter().sum::<usize>(), c.total);
        assert_eq!(units.iter().any(|n| *n > b), c.layer == "T");
        // Metadata-inclusive acceptance of U is checked by the baseline harness.
        assert_eq!(c.total > b, c.layer != "U");
        let file = format!("{case_id}.hwpx");
        std::fs::write(out.join(&file), &bytes)?;
        let saved = std::fs::read(out.join(&file))?;
        assert_eq!(costs(&saved), (actual.clone(), units.clone()));
        check_package(&saved);
        manifest.push(serde_json::json!({
            "file":file,"generator_sha":hash(&[include_bytes!("resource_series_a.rs").as_slice(), include_bytes!("support/plain.rs").as_slice()].concat()),
            "base_source_sha":base_commit,"family":"A",
            "parameters":{"group":group,"section_bytes":actual,"budget":b},
            "input_sha256":hash(&bytes),"file_bytes":bytes.len(),
            "expected_units":units.len(),"expected_cost_per_unit":units,"expected_total_cost":total,
            "expected_policy_exceed_L":units.iter().any(|n|*n>b),"expected_policy_exceed_D":total>b,
            "case_id":case_id,"layer":group,"split":split,"k":targets.len(),
            "unit_bytes":targets,"unit_bytes_sum":c.total,"placement":"sections",
            "production_limits_inactive":true
        }));
    }
    std::fs::write(
        out.join("manifest.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"allocator_accounting":"A is process-cumulative; per-call A = row.alloc_total_bytes - call_start.alloc_total_bytes. Successful alloc/alloc_zeroed/realloc add full new request to A; realloc adjusts live by new-old; dealloc subtracts live; H is peak live since call_start.","cases":manifest}),
        )?,
    )?;
    Ok(())
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn check_package(bytes: &[u8]) {
    assert!(bytes.len() < 500 * 1024 * 1024);
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut total = 0;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).unwrap();
        let actual = std::io::copy(&mut file, &mut std::io::sink()).unwrap();
        assert_eq!(actual, file.size());
        assert!(actual < 50 * 1024 * 1024);
        total += actual;
    }
    assert!(total < 500 * 1024 * 1024);
}

#[test]
fn section_bytes_and_metadata() {
    for targets in [vec![65536], divide(262144, 12)] {
        let (base, _) = costs(&encode(&vec![1; targets.len()]));
        let lengths: Vec<_> = targets.iter().zip(base).map(|(n, base)| n - base + 1).collect();
        let bytes = encode(&lengths);
        let (actual, units) = costs(&bytes);
        assert_eq!(actual, targets);
        assert!(units.iter().sum::<usize>() > targets.iter().sum());
        check_package(&bytes);
    }
}

#[derive(Debug)]
struct Case {
    layer: &'static str,
    split: &'static str,
    index: usize,
    units: Vec<usize>,
    total: usize,
}

fn divide(n: usize, k: usize) -> Vec<usize> {
    let a = n / k;
    let r = n - k * a;
    let mut units = vec![a; k];
    units[k - 1] += r;
    if r == 0 && k > 1 {
        units[0] -= 1;
        units[k - 1] += 1;
    }
    units
}

fn grid(b: usize) -> Vec<Case> {
    assert!((65536..=8_388_608).contains(&b), "supported budget: 65536..=8388608");
    let mut cases = Vec::new();
    let mut add = |layer, split, index, n, k| {
        cases.push(Case { layer, split, index, units: divide(n, k), total: n });
    };
    for i in 0..36 {
        add("T", "development", i, b * (105 * 35 + 295 * i) / (100 * 35), 1);
    }
    for i in 0..12 {
        add("T", "holdout", i, b * (11 * 11 + 24 * i) / (10 * 11), 1);
    }
    for (j, k) in [2, 3, 4, 6, 8, 12].into_iter().enumerate() {
        let high = 400.min(90 * k - 5);
        for i in 0..6 {
            add("S", "development", j * 6 + i, b * (110 * (5 - i) + high * i) / 500, k);
        }
    }
    for (j, k) in [5, 7, 10].into_iter().enumerate() {
        for i in 0..4 {
            add("S", "holdout", j * 4 + i, b * (12 * (3 - i) + 38 * i) / 30, k);
        }
    }
    for (i, (n, k)) in
        [(b / 4, 1), (b / 2, 1), (9 * b / 10, 1), (b / 2, 4), (9 * b / 10, 4), (9 * b / 10, 8)]
            .into_iter()
            .enumerate()
    {
        add("U", "development", i, n, k);
    }
    check_grid(&cases, b);
    cases
}

fn check_grid(cases: &[Case], b: usize) {
    assert_eq!(cases.len(), 102);
    for (layer, split, count) in [
        ("T", "development", 36),
        ("T", "holdout", 12),
        ("S", "development", 36),
        ("S", "holdout", 12),
        ("U", "development", 6),
    ] {
        assert_eq!(cases.iter().filter(|c| c.layer == layer && c.split == split).count(), count);
    }
    for c in cases {
        // 이것을 실패시키는 것: 정수 분할의 나머지 누락.
        assert_eq!(c.units.iter().sum::<usize>(), c.total);
        if c.units.len() > 1 {
            // 이것을 실패시키는 것: 나머지가 0일 때 1바이트 이동 제거.
            assert_ne!(c.units.first(), c.units.last());
        }
        if c.layer == "T" {
            assert_eq!(c.units.len(), 1);
            // 이것을 실패시키는 것: T 끝점 식을 바꾸어 4B 초과.
            assert!(c.total > b && c.total <= 4 * b);
        } else {
            // 이것을 실패시키는 것: s_max의 여유 0.05B 제거.
            assert!(c.units.iter().all(|n| *n <= 9 * b / 10));
            assert_eq!(c.total > b, c.layer == "S");
        }
    }
    for c in cases {
        let i = c.index;
        let expected = match (c.layer, c.split) {
            ("T", "development") => b * (3675 + 295 * i) / 3500,
            ("T", "holdout") => b * (121 + 24 * i) / 110,
            ("S", "development") => {
                let k = [2, 3, 4, 6, 8, 12][i / 6];
                assert_eq!(c.units.len(), k);
                let high = [175, 265, 355, 400, 400, 400][i / 6];
                b * (550 + (high - 110) * (i % 6)) / 500
            }
            ("S", "holdout") => {
                assert_eq!(c.units.len(), [5, 7, 10][i / 4]);
                b * (36 + 26 * (i % 4)) / 30
            }
            ("U", "development") => {
                assert_eq!(c.units.len(), [1, 1, 1, 4, 4, 8][i]);
                [b / 4, b / 2, 9 * b / 10, b / 2, 9 * b / 10, 9 * b / 10][i]
            }
            _ => unreachable!(),
        };
        assert_eq!(c.total, expected);
    }
    assert_eq!(cases[35].total, 4 * b);
}

#[test]
fn layer_grid() {
    for b in [65536, 393216, 1_048_576, 8_388_608] {
        grid(b);
    }
}
