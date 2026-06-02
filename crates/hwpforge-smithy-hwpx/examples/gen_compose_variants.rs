//! Wave 12j compose (글자겹침) circleType/composeType 전조합 fixture.
//!
//! OWPML enumdef.h §SHAPECIRCLETYPE (14 종) × COMPOSETYPE (2 종)을 모두
//! 한 문서에 emit해서 사용자가 한컴에서 열고 `.hwp` 로 다른 이름 저장 →
//! HWP5 round-trip 검증용 fixture로 사용한다.
//!
//! 14 × 2 = 28 paragraphs.
//!
//! `Control::compose(...)` helper는 `circleType="SHAPE_REVERSAL_TIRANGLE"`,
//! `composeType="SPREAD"` 으로 하드코딩되어 있어서, 변형 fixture는 struct
//! literal 로 직접 만들어야 함.
//!
//! 사용법:
//! ```text
//! cargo run -p hwpforge-smithy-hwpx --example gen_compose_variants -- \
//!     /tmp/wave12j-compose/sample-compose-all-shapes.hwpx
//! ```

use std::path::PathBuf;

use hwpforge_core::control::Control;
use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::PageSettings;
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge_smithy_hwpx::style_store::HwpxStyleStore;
use hwpforge_smithy_hwpx::HwpxEncoder;

/// 14 circleType labels in OWPML enum order
/// (see `.docs/references/hwpx-owpml-model/OWPML/Class/enumdef.h` 623-639).
const CIRCLE_TYPES: &[&str] = &[
    "CHAR",
    "SHAPE_CIRCLE",
    "SHAPE_REVERSAL_CIRCLE",
    "SHAPE_RECTANGLE",
    "SHAPE_REVERSAL_RECTANGLE",
    "SHAPE_TRIANGLE",
    "SHAPE_REVERSAL_TIRANGLE", // 한컴 공식 spec 오타 — TRIANGLE 아님
    "SHAPE_LIGHT",
    "SHAPE_RHOMBUS",
    "SHAPE_REVERSAL_RHOMBUS",
    "SHAPE_ROUNDED_RECTANGLE",
    "SHAPE_EMPTY_CIRCULATE_TRIANGLE",
    "SHAPE_THIN_CIRCULATE_TRIANGLE",
    "SHAPE_THICK_CIRCULATE_TRIANGLE",
];

/// 2 composeType labels.
const COMPOSE_TYPES: &[&str] = &["SPREAD", "OVERLAP"];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args();
    let _program = args.next();
    let output: PathBuf = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/wave12j-compose/sample-compose-all-shapes.hwpx"));

    let cs = CharShapeIndex::new(0);
    let ps = ParaShapeIndex::new(0);

    let mut paragraphs: Vec<Paragraph> = Vec::new();
    for ct in COMPOSE_TYPES {
        for circle in CIRCLE_TYPES {
            let prefix = format!("[{} / {}]: ", circle, ct);
            paragraphs.push(Paragraph::with_runs(
                vec![
                    Run::text(prefix.as_str(), cs),
                    Run::control(
                        Control::Compose {
                            compose_text: "한韓".to_string(),
                            circle_type: (*circle).to_string(),
                            char_sz: -3,
                            compose_type: (*ct).to_string(),
                            // 10 × no-override sentinel (HWPX charPrCnt is fixed at 10).
                            char_pr_ids: vec![u32::MAX; 10],
                        },
                        cs,
                    ),
                ],
                ps,
            ));
        }
    }

    let section = Section::with_paragraphs(paragraphs, PageSettings::a4());

    let mut doc = Document::new();
    doc.add_section(section);
    let validated = doc.validate()?;

    let store = HwpxStyleStore::with_default_fonts("함초롬바탕");
    let image_store = ImageStore::new();
    let bytes = HwpxEncoder::encode(&validated, &store, &image_store)?;

    std::fs::create_dir_all(output.parent().expect("output path has a parent"))?;
    std::fs::write(&output, &bytes)?;
    println!(
        "Wrote {} ({} bytes) — {} paragraphs ({} compose elements)",
        output.display(),
        bytes.len(),
        CIRCLE_TYPES.len() * COMPOSE_TYPES.len(),
        CIRCLE_TYPES.len() * COMPOSE_TYPES.len(),
    );
    Ok(())
}
