//! Wave 12p Step 4 시각 검증: figure + 그 figure 를 가리키는 cross-ref
//! 가 같은 `inst_id` 로 매칭되는지 확인.
//!
//! 핵심: `Image.inst_id = Some(N)` 으로 명시하면 encoder 가
//! `<hp:pic id="N">` 로 emit. cross-ref Command 는
//! `RefTarget::SystemId(N)` + `RefType::Figure` + `RefContentType::Number`
//! 로 같은 N 을 가리킴. 한컴에서 F9 시 `?` → `1` (그림 번호) 로 변경되어야 함.
//!
//! Usage:
//!   cargo run --release -p hwpforge-smithy-hwpx --example forged_figure_crossref_instid
//!
//! Output:
//!   examples/hwp5_review/forged-figure-crossref-instid.hwpx

use std::path::PathBuf;

use hwpforge_core::caption::{Caption, CaptionSide};
use hwpforge_core::control::{Control, RefTarget};
use hwpforge_core::document::Document;
use hwpforge_core::image::{Image, ImageFormat, ImageStore};
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::PageSettings;
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex, RefContentType, RefType};
use hwpforge_smithy_hwpx::style_store::HwpxStyleStore;
use hwpforge_smithy_hwpx::HwpxEncoder;

const FIGURE_INST_ID: u64 = 0x4221_E000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/hwp5_review");
    std::fs::create_dir_all(&out_dir)?;

    let image_bytes = std::fs::read("assets/mascot-main.png")
        .or_else(|_| std::fs::read("../../assets/mascot-main.png"))?;
    let mut images = ImageStore::new();
    images.insert("image1.png", image_bytes);

    // Figure with explicit inst_id + caption "그림 1".
    let mut img = Image::new(
        "BinData/image1.png",
        HwpUnit::from_mm(60.0).unwrap(),
        HwpUnit::from_mm(45.0).unwrap(),
        ImageFormat::Png,
    );
    img.inst_id = Some(FIGURE_INST_ID);
    img.caption = Some(Caption::new(
        vec![Paragraph::with_runs(
            vec![Run::text(
                "그림 1: 마스코트 (cross-ref target)".to_string(),
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        )],
        CaptionSide::Bottom,
    ));

    let header = Paragraph::with_runs(
        vec![Run::text(
            "Wave 12p Step 4 검증: Figure inst_id ↔ cross-ref SystemId 매칭".to_string(),
            CharShapeIndex::new(0),
        )],
        ParaShapeIndex::new(0),
    );

    let img_para =
        Paragraph::with_runs(vec![Run::image(img, CharShapeIndex::new(0))], ParaShapeIndex::new(0));

    // cross-ref Command targeting that same inst_id.
    let xref = Control::CrossRef {
        target: RefTarget::SystemId(FIGURE_INST_ID),
        ref_type: RefType::Figure,
        content_type: RefContentType::Number,
        as_hyperlink: false,
        display_text: "?".to_string(),
    };
    let ref_para = Paragraph::with_runs(
        vec![
            Run::text("위 그림의 번호: ".to_string(), CharShapeIndex::new(0)),
            Run::control(xref, CharShapeIndex::new(0)),
            Run::text(
                format!(" (encoder emit id={FIGURE_INST_ID}; F9 시 \"1\" 로 갱신 기대)"),
                CharShapeIndex::new(0),
            ),
        ],
        ParaShapeIndex::new(0),
    );

    let section = Section::with_paragraphs(vec![header, img_para, ref_para], PageSettings::a4());

    let mut doc = Document::<hwpforge_core::document::Draft>::new();
    doc.add_section(section);
    let validated = doc.validate()?;

    let style_store = HwpxStyleStore::default();
    let bytes = HwpxEncoder::encode(&validated, &style_store, &images)?;

    let path = out_dir.join("forged-figure-crossref-instid.hwpx");
    std::fs::write(&path, &bytes)?;
    println!("created: {}", path.display());
    println!();
    println!("한컴에서 열기:");
    println!("  open {}", path.display());
    println!();
    println!("검증:");
    println!("  1. 처음 열 때 cross-ref 가 \"?\" 표시 (display_text)");
    println!("  2. F9 (필드 갱신) → \"1\" 로 변경되면 Step 4 encoder 정상 동작");
    println!("  3. 변경 없으면 한컴이 id={FIGURE_INST_ID} 의 <hp:pic> 를 못 찾은 것");

    Ok(())
}
