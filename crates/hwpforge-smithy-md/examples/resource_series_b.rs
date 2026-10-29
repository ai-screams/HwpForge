//! Generate plain ASCII footnote expansion boundaries.
use sha2::{Digest, Sha256};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let out = std::path::Path::new(&args[1]);
    let b: usize = args[2].parse()?;
    assert_eq!(args[3], "--base-commit");
    let base_commit = &args[4];
    std::fs::create_dir_all(out)?;
    let mut manifest = Vec::new();
    for c in grid(b) {
        let units = &c.units;
        let r = units.len();
        let split = c.split;
        let group = c.layer;
        let text = document(units, split == "holdout");
        let total = c.total;
        let case_id = format!("b-{}-{split}-{}", group.to_ascii_lowercase(), c.index);
        let file = format!("{case_id}.md");
        std::fs::write(out.join(&file), &text)?;
        let saved = std::fs::read_to_string(out.join(&file))?;
        check_document(&saved, units, split == "holdout");
        assert_eq!(units.iter().any(|n| *n > b), group == "T");
        assert_eq!(total > b, group != "U");
        manifest.push(serde_json::json!({
                "file": file, "generator_sha": hash(include_bytes!("resource_series_b.rs")),
                "base_source_sha": base_commit,
                "family":"B", "parameters":{"group":group,"q":units,"r":r,"budget":b},
                "input_sha256":hash(text.as_bytes()),
                "file_bytes":text.len(),"expected_units":r,"expected_cost_per_unit":units,
                "expected_total_cost":total,"expected_policy_exceed_L":units.iter().any(|q|*q>b),"expected_policy_exceed_D":total>b,
                "md_expansion_limit_replaced_by_budget":true,
                "case_id":case_id,"layer":group,"split":split,"k":r,
                "unit_bytes":units,"unit_bytes_sum":total,
                "placement":if split == "holdout" {"table_cell"} else {"body"},
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

fn document(units: &[usize], table: bool) -> String {
    let refs = units
        .iter()
        .enumerate()
        .map(|(j, _)| if table { format!("| x[^a{j}] |\n") } else { format!("x[^a{j}] ") })
        .collect::<String>();
    let refs = if table { format!("| note |\n|---|\n{refs}") } else { refs };
    let definitions = units
        .iter()
        .enumerate()
        .map(|(j, q)| format!("[^a{j}]: {}\n\n", "a".repeat(*q)))
        .collect::<String>();
    format!("{refs}\n\n{definitions}")
}

fn check_document(text: &str, units: &[usize], table: bool) {
    // MD 디코더에는 입력 크기 상한이 없고 생산 상한은 각주 확장 8 MiB 뿐이다.
    // 50 MiB 는 러너 입력 읽기의 안전 한도이며 생산 정책이 아니다.
    assert!(text.len() < 50 * 1024 * 1024);
    let (refs, definitions) = text.split_once("\n\n").unwrap();
    assert_eq!(refs.starts_with("| note |\n|---|\n"), table);
    let bodies: Vec<_> = definitions.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(bodies.len(), units.len());
    for (j, (line, expected)) in bodies.iter().zip(units).enumerate() {
        let marker = format!("[^a{j}]");
        assert_eq!(refs.matches(&marker).count(), 1);
        let body = line.strip_prefix(&format!("{marker}: ")).unwrap();
        assert!(body.bytes().all(|b| b == b'a'));
        assert_eq!(body.len(), *expected);
        if table {
            assert!(refs.lines().any(|line| line == format!("| x{marker} |")));
        }
    }
    assert_eq!(refs.matches("[^a").count(), units.len());
}

#[test]
fn reference_expansion_and_placement() {
    for table in [false, true] {
        let units = divide(65536, 12);
        check_document(&document(&units, table), &units, table);
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
