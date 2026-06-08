//! Wave 12m 시각 검증 part 2: target (책갈피) + cross-ref 같은 파일에.
//!
//! `crossref_matrix.rs` 가 cross-ref 만 만들었다면, 한컴이 target 을
//! 못 찾아 `?` 를 표시. 이 example 은 같은 파일 안에 책갈피 "target1"
//! 도 함께 넣어, F9 시 한컴이 페이지 번호를 lookup 할 수 있는지 검증.
//!
//! Usage:
//!   cargo run --release -p hwpforge-smithy-hwpx --example crossref_with_target
//!
//! Output:
//!   examples/hwp5_review/wave12m/forged-crossref-with-bookmark.hwpx

use std::path::PathBuf;

use hwpforge_core::control::{Control, RefTarget};
use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::PageSettings;
use hwpforge_foundation::{
    BookmarkType, CharShapeIndex, ParaShapeIndex, RefContentType, RefType,
};
use hwpforge_smithy_hwpx::style_store::HwpxStyleStore;
use hwpforge_smithy_hwpx::HwpxEncoder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/hwp5_review/wave12m");
    std::fs::create_dir_all(&out_dir)?;

    // ── 첫 페이지: 책갈피 "target1" 설치 ──
    let bm_para = Paragraph::with_runs(
        vec![
            Run::text("여기가 책갈피 target1 위치입니다.".to_string(), CharShapeIndex::new(0)),
            Run::control(
                Control::Bookmark {
                    name: "target1".to_string(),
                    bookmark_type: BookmarkType::Point,
                },
                CharShapeIndex::new(0),
            ),
        ],
        ParaShapeIndex::new(0),
    );

    // ── 두 번째 페이지로 강제 page-break ──
    let break_para = Paragraph::with_runs(
        vec![Run::text("이 단락 끝에서 페이지가 넘어갑니다.".to_string(), CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );

    // ── cross-ref 4종 ──
    let xref_page = Control::CrossRef {
        target: RefTarget::Name("target1".to_string()),
        ref_type: RefType::Bookmark,
        content_type: RefContentType::Page,
        as_hyperlink: false,
        display_text: "1".to_string(),
    };

    let xref_page_hyperlink = Control::CrossRef {
        target: RefTarget::Name("target1".to_string()),
        ref_type: RefType::Bookmark,
        content_type: RefContentType::Page,
        as_hyperlink: true,
        display_text: "1".to_string(),
    };

    let xref_contents = Control::CrossRef {
        target: RefTarget::Name("target1".to_string()),
        ref_type: RefType::Bookmark,
        content_type: RefContentType::Contents,
        as_hyperlink: false,
        display_text: "여기가 책갈피 target1 위치입니다.".to_string(),
    };

    let xref_name = Control::CrossRef {
        target: RefTarget::Name("target1".to_string()),
        ref_type: RefType::Bookmark,
        // OWPML 표 156: Bookmark+Contents = "책갈피 이름" (별도 enum 없음).
        // HWP5 N2 code 2 (book mark 이름 dropdown) 도 이 variant 에 매핑.
        content_type: RefContentType::Contents,
        as_hyperlink: false,
        display_text: "target1".to_string(),
    };

    let ref1 = Paragraph::with_runs(
        vec![
            Run::text("1) Page (책갈피 위치 페이지): ".to_string(), CharShapeIndex::new(0)),
            Run::control(xref_page, CharShapeIndex::new(0)),
            Run::text("쪽".to_string(), CharShapeIndex::new(0)),
        ],
        ParaShapeIndex::new(0),
    );

    let ref2 = Paragraph::with_runs(
        vec![
            Run::text("2) Page + Hyperlink (Ctrl+클릭 점프): ".to_string(), CharShapeIndex::new(0)),
            Run::control(xref_page_hyperlink, CharShapeIndex::new(0)),
            Run::text("쪽".to_string(), CharShapeIndex::new(0)),
        ],
        ParaShapeIndex::new(0),
    );

    let ref3 = Paragraph::with_runs(
        vec![
            Run::text("3) Contents (책갈피 본문): ".to_string(), CharShapeIndex::new(0)),
            Run::control(xref_contents, CharShapeIndex::new(0)),
        ],
        ParaShapeIndex::new(0),
    );

    let ref4 = Paragraph::with_runs(
        vec![
            Run::text("4) BookmarkName (책갈피 이름): ".to_string(), CharShapeIndex::new(0)),
            Run::control(xref_name, CharShapeIndex::new(0)),
        ],
        ParaShapeIndex::new(0),
    );

    let header = Paragraph::with_runs(
        vec![Run::text(
            "Wave 12m: 책갈피 target1 + cross-ref 4종 (한컴에서 F9 또는 Ctrl+클릭 시도)".to_string(),
            CharShapeIndex::new(0),
        )],
        ParaShapeIndex::new(0),
    );

    let section = Section::with_paragraphs(
        vec![header, bm_para, break_para, ref1, ref2, ref3, ref4],
        PageSettings::a4(),
    );

    let mut doc = Document::<hwpforge_core::document::Draft>::new();
    doc.add_section(section);
    let validated = doc.validate()?;

    let style_store = HwpxStyleStore::default();
    let image_store = ImageStore::new();
    let bytes = HwpxEncoder::encode(&validated, &style_store, &image_store)?;

    let path = out_dir.join("forged-crossref-with-bookmark.hwpx");
    std::fs::write(&path, &bytes)?;
    println!("created: {}", path.display());
    println!();
    println!("한컴에서 열기:");
    println!("  open {}", path.display());
    println!();
    println!("확인:");
    println!("  1. 처음 열 때 ?, 1, 등 어느 것이 표시되는가");
    println!("  2. F9 (필드 갱신) 후 실제 페이지 번호 / 책갈피 본문 / 책갈피 이름 으로 바뀌는가");
    println!("  3. 2번 항목 Ctrl+클릭으로 책갈피 위치로 점프되는가");

    Ok(())
}
