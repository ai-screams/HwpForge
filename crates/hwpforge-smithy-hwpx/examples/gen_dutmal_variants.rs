//! Task #73 dutmal (덧말) tail-field fixture — sz_ratio / align variants.
//!
//! HWP5 의 `tdut` tail 영역 (`sz_ratio`, `align`, `styleIDRef`, reserved
//! words) 은 지금까지 모든 한컴 fixture 가 default 값이라 wire offset 을
//! 관찰하지 못했다 (Wave 12i 보류). 이 generator 는 **한 문단에 한
//! 속성만** baseline 에서 바꾼 변형 6종을 emit 한다 — 한컴에서 열어
//! `.hwp` 로 저장하면 각 변형의 tail bytes 를 baseline 과 diff 해서
//! offset 을 1:1 귀속할 수 있다.
//!
//! | # | label | position | sz_ratio | align |
//! |---|-------|----------|----------|-------|
//! | 1 | baseline | Top | 0 (auto) | Center |
//! | 2 | szratio-50 | Top | 50 | Center |
//! | 3 | szratio-75 | Top | 75 | Center |
//! | 4 | align-left | Top | 0 | Left |
//! | 5 | align-right | Top | 0 | Right |
//! | 6 | pos-bottom (대조군 — 이미 carry 되는 축) | Bottom | 0 | Center |
//!
//! 사용법:
//! ```text
//! cargo run -p hwpforge-smithy-hwpx --example gen_dutmal_variants -- \
//!     temp/sample-dutmal-variants.hwpx
//! ```
//! 이후 한컴에서 열어 모든 덧말이 의도대로 보이는지 확인하고
//! `examples/hwp5_review/sample-dutmal-variants.hwp` 로 다른 이름 저장.

use std::path::PathBuf;

use hwpforge_core::control::{Control, DutmalAlign, DutmalMetadata, DutmalPosition};
use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::PageSettings;
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge_smithy_hwpx::style_store::HwpxStyleStore;
use hwpforge_smithy_hwpx::HwpxEncoder;

/// (label, position, sz_ratio, align) — one knob changes per row.
const VARIANTS: &[(&str, DutmalPosition, u32, DutmalAlign)] = &[
    ("baseline", DutmalPosition::Top, 0, DutmalAlign::Center),
    ("szratio-50", DutmalPosition::Top, 50, DutmalAlign::Center),
    ("szratio-75", DutmalPosition::Top, 75, DutmalAlign::Center),
    ("align-left", DutmalPosition::Top, 0, DutmalAlign::Left),
    ("align-right", DutmalPosition::Top, 0, DutmalAlign::Right),
    ("pos-bottom", DutmalPosition::Bottom, 0, DutmalAlign::Center),
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args();
    let _program = args.next();
    let output: PathBuf = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("temp/sample-dutmal-variants.hwpx"));

    let cs = CharShapeIndex::new(0);
    let ps = ParaShapeIndex::new(0);

    let paragraphs: Vec<Paragraph> = VARIANTS
        .iter()
        .map(|(label, position, sz_ratio, align)| {
            let prefix = format!("[{label}]: ");
            Paragraph::with_runs(
                vec![
                    Run::text(prefix.as_str(), cs),
                    Run::control(
                        Control::Dutmal {
                            main_text: "한글".to_string(),
                            sub_text: "주석".to_string(),
                            position: *position,
                            sz_ratio: *sz_ratio,
                            align: *align,
                            metadata: DutmalMetadata::default(),
                        },
                        cs,
                    ),
                ],
                ps,
            )
        })
        .collect();

    let section = Section::with_paragraphs(paragraphs, PageSettings::a4());

    let mut doc = Document::new();
    doc.add_section(section);
    let validated = doc.validate()?;

    let store = HwpxStyleStore::with_default_fonts("함초롬바탕");
    let image_store = ImageStore::new();
    let bytes = HwpxEncoder::encode(&validated, &store, &image_store)?;

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&output, &bytes)?;
    println!(
        "Wrote {} ({} bytes) — {} dutmal variants",
        output.display(),
        bytes.len(),
        VARIANTS.len(),
    );
    Ok(())
}
