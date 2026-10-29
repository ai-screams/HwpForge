//! Generate plain ASCII footnote expansion boundaries.
use sha2::{Digest, Sha256};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let out = std::path::Path::new(&args[1]);
    let b: usize = args[2].parse()?;
    assert_eq!(args[3], "--base-commit");
    let base_commit = &args[4];
    std::fs::create_dir_all(out)?;
    let mut cases = Vec::new();
    for q in [b - 1, b, b + 1, 2 * b] {
        cases.push(("single", vec![q]));
    }
    for r in [1, 2, 4, 8] {
        cases.push(("sum", vec![b / 4; r]));
    }
    for q in [b - 1, b, b + 1, 2 * b] {
        let mut units = vec![q / 4; 4];
        units[3] += q % 4;
        cases.push(("cumulative", units));
    }
    let mut manifest = Vec::new();
    for split in ["development", "holdout"] {
        for (i, (group, units)) in cases.iter().enumerate() {
            let r = units.len();
            let refs = (0..r)
                .map(|j| {
                    let j = if *group == "cumulative" { j } else { 0 };
                    if split == "holdout" {
                        format!("| x[^a{j}] |\n")
                    } else {
                        format!("x[^a{j}] ")
                    }
                })
                .collect::<String>();
            let refs = if split == "holdout" { format!("| note |\n|---|\n{refs}") } else { refs };
            let definitions = units
                .iter()
                .enumerate()
                .take(if *group == "cumulative" { r } else { 1 })
                .map(|(j, q)| format!("[^a{j}]: {}\n\n", "a".repeat(*q)))
                .collect::<String>();
            let text = format!("{refs}\n\n{definitions}");
            let total: usize = units.iter().sum();
            let file = format!("{split}-{i}.md");
            std::fs::write(out.join(&file), &text)?;
            manifest.push(serde_json::json!({
                "file": file, "generator_sha": hash(include_bytes!("resource_series_b.rs")),
                "base_source_sha": base_commit,
                "family":"B", "parameters":{"group":group,"q":units,"r":r,"budget":b},
                "input_sha256":hash(text.as_bytes()),
                "file_bytes":text.len(),"expected_units":r,"expected_cost_per_unit":units,
                "expected_total_cost":total,"expected_policy_exceed_L":units.iter().any(|q|*q>b),"expected_policy_exceed_D":total>b,
                "md_expansion_limit_replaced_by_budget":true,
                "split":split,"production_limits_inactive":text.len()<50*1024*1024
            }));
        }
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
