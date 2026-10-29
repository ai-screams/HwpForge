//! Generate packages with exact section output sizes.
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
    // 디코더 읽기 순서가 바뀌면 이 두 목록을 함께 고친다
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
    let mut cases = Vec::new();
    for n in [b - 1, b, b + 1, 2 * b] {
        cases.push(("single", vec![n], false));
    }
    for r in [1, 2, 4, 8] {
        cases.push(("sum", vec![b / 4; r], false));
    }
    for n in [b - 1, b, b + 1, 2 * b] {
        cases.push(("cumulative", vec![n], true));
    }
    for sizes in holdout_sizes(b) {
        cases.push(("unequal", sizes, false));
    }
    let mut manifest = Vec::new();
    for (i, (group, targets, cumulative)) in cases.into_iter().enumerate() {
        let seed = encode(&vec![1; targets.len()]);
        let (sections, units) = costs(&seed);
        let overhead = units.iter().sum::<usize>() - sections.iter().sum::<usize>();
        // ASCII 'a' needs no XML escaping: each added character adds exactly one
        // byte to section XML. Measure the encoder's fixed tags with one character,
        // then subtract that overhead; do not substitute text length for XML size.
        let lengths = targets
            .iter()
            .zip(&sections)
            .map(|(target, base)| {
                target
                    .checked_sub(base - 1 + if cumulative { overhead } else { 0 })
                    .expect("budget too small for XML overhead")
            })
            .collect::<Vec<_>>();
        let bytes = encode(&lengths);
        let (actual, units) = costs(&bytes);
        let total: usize = units.iter().sum();
        if cumulative {
            assert_eq!(total, targets[0]);
        } else {
            // 이것을 실패시키는 것: 12개 section의 크기를 이름 사전순으로 수집하기.
            assert_eq!(actual, targets);
        }
        let split = if group == "unequal" { "holdout" } else { "development" };
        let file = format!("{split}-{i}.hwpx");
        std::fs::write(out.join(&file), &bytes)?;
        manifest.push(serde_json::json!({
            "file":file,"generator_sha":hash(&[include_bytes!("resource_series_a.rs").as_slice(), include_bytes!("support/plain.rs").as_slice()].concat()),
            "base_source_sha":base_commit,"family":"A",
            "parameters":{"group":group,"section_bytes":actual,"budget":b},
            "input_sha256":hash(&bytes),"file_bytes":bytes.len(),
            "expected_units":units.len(),"expected_cost_per_unit":units,"expected_total_cost":total,
            "expected_policy_exceed_L":units.iter().any(|n|*n>b),"expected_policy_exceed_D":total>b,
            "split":split,"production_limits_inactive":units.iter().all(|n|*n<50*1024*1024) && total<500*1024*1024
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

fn holdout_sizes(b: usize) -> Vec<Vec<usize>> {
    let cases = [2, 4, 8, 12]
        .into_iter()
        .map(|r| {
            if r == 12 {
                let size = 2 * b / r;
                let mut sizes = vec![size; r];
                sizes[r - 1] = 2 * b - size * (r - 1);
                sizes
            } else {
                let mut sizes = vec![b / 4; r];
                sizes[0] -= b / 8;
                sizes[1] += b / 8;
                sizes
            }
        })
        .collect::<Vec<_>>();
    // 이것을 실패시키는 것: 보류 분배가 개발 합계 수준을 벗어나기.
    assert!(cases.iter().all(|sizes| [b / 4, b / 2, b, 2 * b].contains(&sizes.iter().sum())));
    cases
}

#[test]
fn holdout_totals_preserve_development_levels() {
    let cases = holdout_sizes(65536);
    assert_eq!(cases.last().unwrap().len(), 12);
}
