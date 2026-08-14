//! W1b corpus 센서스: `--carry-layout-cache` 변환에서 문단 캐시 드롭률과
//! 사유 분포를 전수 측정한다 (§1g v5 R3#3 — "구현된 Rust ledger 가 곧
//! 측정기"; 파이썬 근사는 오차 위험으로 기각된 그 게이트).
//!
//! ```bash
//! cargo run --release -p hwpforge-convert --example census_layout_carry -- <corpus-dir>
//! ```

use std::collections::BTreeMap;
use std::path::PathBuf;

use hwpforge_convert::{hwp5_to_hwpx_bytes_with_options, ConvertOptions, ConvertWarning};
use hwpforge_smithy_hwpx::EncodeWarning;

fn main() {
    let root = std::env::args().nth(1).expect("usage: census_layout_carry <corpus-dir>");
    let mut files: Vec<PathBuf> = Vec::new();
    collect_hwp(&PathBuf::from(&root), &mut files);
    files.sort();

    let mut converted = 0usize;
    let mut convert_failed = 0usize;
    let mut files_with_drops = 0usize;
    let mut total_drops = 0usize;
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    let options = ConvertOptions::default().with_carry_layout_cache(true);

    for (i, f) in files.iter().enumerate() {
        if i % 200 == 0 {
            eprintln!("[{i}/{}]", files.len());
        }
        let Ok(bytes) = std::fs::read(f) else {
            convert_failed += 1;
            continue;
        };
        match hwp5_to_hwpx_bytes_with_options(&bytes, options) {
            Ok((_out, warnings)) => {
                converted += 1;
                let mut drops_here = 0usize;
                for w in &warnings {
                    if let ConvertWarning::HwpxEncode(EncodeWarning::LayoutCacheDropped {
                        reason,
                        ..
                    }) = w
                    {
                        drops_here += 1;
                        // 카테고리화: 가변 숫자는 지우되 오류 종류는 보존.
                        let key = if let Some((_, kind)) = reason.split_once(": ") {
                            let kind: String = kind
                                .chars()
                                .map(|c| if c.is_ascii_digit() { '#' } else { c })
                                .collect();
                            format!(
                                "to_wire {}",
                                kind.split_whitespace().collect::<Vec<_>>().join(" ")
                            )
                        } else {
                            reason.split('(').next().unwrap_or(reason).trim().to_string()
                        };
                        *reasons.entry(key).or_default() += 1;
                    }
                }
                if drops_here > 0 {
                    files_with_drops += 1;
                    total_drops += drops_here;
                }
            }
            Err(_) => convert_failed += 1,
        }
    }

    println!("files={} converted={converted} convert_failed={convert_failed}", files.len());
    println!("files_with_cache_drops={files_with_drops} total_dropped_paragraphs={total_drops}");
    for (reason, count) in &reasons {
        println!("  {count:>6}  {reason}");
    }
}

fn collect_hwp(dir: &PathBuf, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_hwp(&path, out);
        } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("hwp")) {
            out.push(path);
        }
    }
}
