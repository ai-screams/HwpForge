//! colLine(다단 구분선) 시각 검증: 우리 encoder 가 만든 2단 + 구분선 HWPX 가
//! 한컴에서 정상 렌더되는지 확인.
//!
//! 구분선 값은 한컴 native fixture(`examples/hwp5_review/_verify/nativ-colline.hwpx`)
//! 와 동일: `type="DOUBLE_SLIM" width="0.7 mm" color="#CA56A7"`.
//!
//! Usage:
//!   cargo run --release -p hwpforge-smithy-hwpx --example forged_colline
//!
//! Output:
//!   examples/hwp5_review/forged-colline.hwpx

use std::path::PathBuf;

use hwpforge_core::column::{ColumnLine, ColumnSettings};
use hwpforge_core::document::{Document, Draft};
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::PageSettings;
use hwpforge_foundation::{BorderLineType, CharShapeIndex, Color, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::style_store::HwpxStyleStore;
use hwpforge_smithy_hwpx::HwpxEncoder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/hwp5_review");
    std::fs::create_dir_all(&out_dir)?;

    // Enough body text that the two columns visibly fill and the divider shows.
    let line = "가나다라마바사아자차카타파하 ABCDEFG 0123456789 다단 구분선 테스트 문장입니다. ";
    let paragraphs: Vec<Paragraph> = (0..24)
        .map(|i| {
            Paragraph::with_runs(
                vec![Run::text(format!("{i:02}) {line}"), CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )
        })
        .collect();

    let mut section = Section::with_paragraphs(paragraphs, PageSettings::a4());
    // 2 columns with a Hancom-native-matching separator line.
    section.column_settings = Some(
        ColumnSettings::equal_columns(2, HwpUnit::new(2268).unwrap())?.with_separator(ColumnLine {
            line_type: BorderLineType::DoubleSlim,
            width: HwpUnit::from_mm(0.7).unwrap(),
            color: Color::from_rgb(0xCA, 0x56, 0xA7),
        }),
    );

    let mut doc = Document::<Draft>::new();
    doc.add_section(section);
    let validated = doc.validate()?;

    let style_store = HwpxStyleStore::default();
    let images = ImageStore::new();
    let bytes = HwpxEncoder::encode(&validated, &style_store, &images)?;

    let path = out_dir.join("forged-colline.hwpx");
    std::fs::write(&path, &bytes)?;
    println!("created: {}", path.display());
    println!();
    println!("한컴에서 열기:");
    println!("  open {}", path.display());
    println!();
    println!("검증:");
    println!("  1. 본문이 2단으로 나뉘어 보이는지");
    println!("  2. 두 단 사이에 이중선(자주색 #CA56A7) 구분선이 그려지는지");
    println!("  3. [쪽 → 단] 대화상자에서 구분선 종류/색이 보존되는지");

    Ok(())
}
