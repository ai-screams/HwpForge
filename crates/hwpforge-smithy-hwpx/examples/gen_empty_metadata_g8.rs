//! G8 gate — confirm that emitting HWPX with Metadata::default() does
//! NOT trigger Hancom Office's first-paragraph fallback for SUMMERY
//! auto-fields. Wave 12o §11.7 final gate.

use hwpforge_core::control::Control;
use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::metadata::Metadata;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::PageSettings;
use hwpforge_foundation::{CharShapeIndex, FieldType, ParaShapeIndex};
use hwpforge_smithy_hwpx::presets::style_store_for_preset;
use hwpforge_smithy_hwpx::HwpxEncoder;

fn main() {
    let style_store = style_store_for_preset("default").expect("default preset");
    let image_store = ImageStore::new();

    // First paragraph text that the OLD broken behavior would fall back to
    // when metadata was empty. With Wave 12o, encoder still emits the 9
    // self-closing slots — Hancom should keep the auto fields blank or
    // show its OS-derived defaults, NOT this paragraph text.
    let para1 = Paragraph::with_runs(
        vec![Run::text(
            "G8 BAIT — 이 텍스트가 자동 필드 자리에 노출되면 G8 실패",
            CharShapeIndex::new(0),
        )],
        ParaShapeIndex::new(0),
    );
    let label = Paragraph::with_runs(
        vec![Run::text("[Title 자동 필드 (빈 metadata)]", CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    let field = Paragraph::with_runs(
        vec![Run::control(
            Control::Field {
                field_type: FieldType::Title,
                hint_text: None,
                help_text: None,
                name: None,
            },
            CharShapeIndex::new(0),
        )],
        ParaShapeIndex::new(0),
    );

    let mut doc = Document::with_metadata(Metadata::default());
    doc.add_section(Section::with_paragraphs(vec![para1, label, field], PageSettings::a4()));
    let validated = doc.validate().expect("validate");
    let bytes = HwpxEncoder::encode(&validated, &style_store, &image_store).expect("encode");
    let path = "examples/hwp5_review/forged-field-g8-empty-metadata.hwpx";
    std::fs::write(path, &bytes).expect("write");
    println!("G8 데모 생성: {path} ({} bytes)", bytes.len());
    println!();
    println!("한컴오피스에서 열고 확인할 점:");
    println!("  1. 첫 paragraph 'G8 BAIT — …' 텍스트가 자동 필드 자리(라벨 아래)에 ");
    println!("     노출되지 않아야 함");
    println!("  2. $title 자동 필드는 비어 있거나 한컴 OS 기본값으로 표시되어야 함");
    println!("  3. 그러나 절대 'G8 BAIT' 텍스트로 fallback되면 안 됨 → 그러면 G8 실패");
}
