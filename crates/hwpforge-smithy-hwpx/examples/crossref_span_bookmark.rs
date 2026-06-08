//! Wave 12m 시각 검증 part 3: Span 책갈피 + Contents cross-ref.
//!
//! Part 2 (`crossref_with_target.rs`) 의 Point 책갈피로는 "책갈피 내용"
//! reference 가 표시 불가 (한컴이 책갈피의 본문을 모름). 이 example 은
//! BookmarkType::SpanStart + SpanEnd 로 책갈피 범위를 두어, Contents
//! reference 가 한컴에서 정상 표시되는지 검증.
//!
//! Usage:
//!   cargo run --release -p hwpforge-smithy-hwpx --example crossref_span_bookmark
//!
//! Output:
//!   examples/hwp5_review/wave12m/forged-crossref-span-bookmark.hwpx

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

    // ── 첫 페이지: 책갈피 "target1" 의 범위(span) 설치 ──
    // SpanStart → 본문 텍스트 → SpanEnd 로 범위 책갈피.
    // 한컴 cross-ref Contents 가 이 범위 안의 텍스트를 보여준다.
    let bm_para = Paragraph::with_runs(
        vec![
            Run::text("범위 시작 전. ".to_string(), CharShapeIndex::new(0)),
            Run::control(
                Control::Bookmark {
                    name: "target1".to_string(),
                    bookmark_type: BookmarkType::SpanStart,
                },
                CharShapeIndex::new(0),
            ),
            Run::text("이 부분이 책갈피 본문입니다".to_string(), CharShapeIndex::new(0)),
            Run::control(
                Control::Bookmark {
                    name: "target1".to_string(),
                    bookmark_type: BookmarkType::SpanEnd,
                },
                CharShapeIndex::new(0),
            ),
            Run::text(". 범위 끝 후.".to_string(), CharShapeIndex::new(0)),
        ],
        ParaShapeIndex::new(0),
    );

    let break_para = Paragraph::with_runs(
        vec![Run::text("다음 페이지 강제 break".to_string(), CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );

    // ── cross-ref 2종 ──
    let xref_page = Control::CrossRef {
        target: RefTarget::Name("target1".to_string()),
        ref_type: RefType::Bookmark,
        content_type: RefContentType::Page,
        as_hyperlink: false,
        display_text: "1".to_string(),
    };

    let xref_contents = Control::CrossRef {
        target: RefTarget::Name("target1".to_string()),
        ref_type: RefType::Bookmark,
        content_type: RefContentType::Contents,
        as_hyperlink: false,
        display_text: "이 부분이 책갈피 본문입니다".to_string(),
    };

    let ref1 = Paragraph::with_runs(
        vec![
            Run::text("1) Page (책갈피 범위가 있는 페이지): ".to_string(), CharShapeIndex::new(0)),
            Run::control(xref_page, CharShapeIndex::new(0)),
            Run::text("쪽".to_string(), CharShapeIndex::new(0)),
        ],
        ParaShapeIndex::new(0),
    );

    let ref2 = Paragraph::with_runs(
        vec![
            Run::text("2) Contents (책갈피 범위 안의 본문): ".to_string(), CharShapeIndex::new(0)),
            Run::control(xref_contents, CharShapeIndex::new(0)),
        ],
        ParaShapeIndex::new(0),
    );

    let header = Paragraph::with_runs(
        vec![Run::text(
            "Wave 12m Span 책갈피: SpanStart/SpanEnd 로 범위 책갈피를 만들면 \
             Contents reference 가 범위 안 본문을 표시하는지 검증."
                .to_string(),
            CharShapeIndex::new(0),
        )],
        ParaShapeIndex::new(0),
    );

    let section = Section::with_paragraphs(
        vec![header, bm_para, break_para, ref1, ref2],
        PageSettings::a4(),
    );

    let mut doc = Document::<hwpforge_core::document::Draft>::new();
    doc.add_section(section);
    let validated = doc.validate()?;

    let style_store = HwpxStyleStore::default();
    let image_store = ImageStore::new();
    let bytes = HwpxEncoder::encode(&validated, &style_store, &image_store)?;

    let path = out_dir.join("forged-crossref-span-bookmark.hwpx");
    std::fs::write(&path, &bytes)?;
    println!("created: {}", path.display());
    println!();
    println!("한컴에서 열기:");
    println!("  open {}", path.display());
    println!();
    println!("확인:");
    println!("  1) Page: 책갈피가 있는 페이지 번호 표시?");
    println!("  2) Contents: '이 부분이 책갈피 본문입니다' 표시? (또는 ?)");

    Ok(())
}
