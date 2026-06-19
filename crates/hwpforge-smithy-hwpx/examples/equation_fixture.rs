//! Minimal equation fixture for Phase 12d (HWP5 equation carry).
//!
//! Emits a single HWPX equation (`<hp:equation>`) with a short, valid HancomEQN
//! script so it can be opened in 한컴 Office, re-saved as `.hwp` (HWP5), and its
//! `eqed` ctrl + `HWPTAG_EQEDIT` (0x58) script record hex-inspected.
//!
//! Staging input only. The authoritative fixture committed under
//! `tests/fixtures/user_samples/` is the 한컴-produced `.hwp`/`.hwpx` pair.
//! If 한컴 renders this equation oddly, type a simple equation directly in the
//! 수식 편집기 instead — that yields an equally valid `eqed` record.
//!
//! Usage:
//!   cargo run -p hwpforge-smithy-hwpx --example equation_fixture
//!
//! Output:
//!   examples/hwp5_review/sample-equation-basic.hwpx

use std::path::Path;

use hwpforge_core::control::Control;
use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::PageSettings;
use hwpforge_foundation::{Alignment, CharShapeIndex, ParaShapeIndex};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::HwpxEncoder;

const OUT_DIR: &str = "examples/hwp5_review";

fn store() -> HwpxStyleStore {
    let mut store = HwpxStyleStore::with_default_fonts("함초롬바탕");
    store.push_char_shape(HwpxCharShape::default());
    let mut centered = HwpxParaShape::default();
    centered.alignment = Alignment::Center;
    store.push_para_shape(centered);
    store
}

fn label(text: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}

fn main() {
    std::fs::create_dir_all(OUT_DIR).expect("create staging output dir");

    // A simple fraction — proven valid HancomEQN syntax (see section4 guide).
    let equation = Control::equation("{a + b} over {c + d}");
    let paras = vec![
        label("수식(Equation) 단일 객체"),
        Paragraph::with_runs(
            vec![Run::control(equation, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        ),
    ];
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(paras, PageSettings::a4()));
    let validated = doc.validate().expect("fixture document should validate");
    let path = Path::new(OUT_DIR).join("sample-equation-basic.hwpx");
    HwpxEncoder::encode_file(&path, &validated, &store(), &ImageStore::new())
        .expect("fixture HWPX should encode");
    println!("wrote {}", path.display());
}
