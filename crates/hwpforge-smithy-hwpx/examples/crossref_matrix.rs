//! Wave 12m 시각 검증: HwpForge API 로 cross-ref 종류별 HWPX 생성.
//!
//! HWP5 변환을 거치지 않고 우리 encoder 가 직접 만든 HWPX 가 한컴에서
//! 정상 동작하는지 확인하기 위한 doc. 각 RefType × ContentType 조합으로
//! 별도 파일을 생성하므로 한컴에서 하나씩 열어 비교할 수 있다.
//!
//! Usage:
//!   cargo run --release -p hwpforge-smithy-hwpx --example crossref_matrix
//!
//! Output:
//!   examples/hwp5_review/wave12m/forged-crossref-<kind>.hwpx (8 파일)

use std::path::PathBuf;

use hwpforge_core::control::{Control, RefTarget};
use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::ObjectId;
use hwpforge_core::PageSettings;
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex, RefContentType, RefType};
use hwpforge_smithy_hwpx::style_store::HwpxStyleStore;
use hwpforge_smithy_hwpx::HwpxEncoder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/hwp5_review/wave12m");
    std::fs::create_dir_all(&out_dir)?;

    let cases: &[(&str, RefType, RefContentType, RefTarget, bool, &str)] = &[
        (
            "bookmark-page",
            RefType::Bookmark,
            RefContentType::Page,
            RefTarget::Name("target1".to_string()),
            false,
            "1",
        ),
        (
            "bookmark-page-hyperlink",
            RefType::Bookmark,
            RefContentType::Page,
            RefTarget::Name("target1".to_string()),
            true,
            "1",
        ),
        (
            "bookmark-contents",
            RefType::Bookmark,
            RefContentType::Contents,
            RefTarget::Name("target1".to_string()),
            false,
            "책갈피 위치의 본문",
        ),
        (
            "bookmark-name",
            RefType::Bookmark,
            // OWPML 표 156: Bookmark+Contents = "책갈피 이름" 의미
            RefContentType::Contents,
            RefTarget::Name("target1".to_string()),
            false,
            "target1",
        ),
        (
            "footnote-page",
            RefType::Footnote,
            RefContentType::Page,
            RefTarget::Object(ObjectId::new(1)),
            false,
            "1",
        ),
        (
            "footnote-number",
            RefType::Footnote,
            RefContentType::Number,
            RefTarget::Object(ObjectId::new(1)),
            false,
            "1",
        ),
        (
            "figure-page",
            RefType::Figure,
            RefContentType::Page,
            RefTarget::Object(ObjectId::new(1)),
            false,
            "1",
        ),
        (
            "table-number",
            RefType::Table,
            RefContentType::Number,
            RefTarget::Object(ObjectId::new(1)),
            false,
            "1",
        ),
    ];

    for (slug, ref_type, content_type, target, as_hyperlink, display_text) in cases {
        let path = out_dir.join(format!("forged-crossref-{slug}.hwpx"));
        build_one(
            &path,
            slug,
            ref_type,
            content_type,
            target.clone(),
            *as_hyperlink,
            display_text,
        )?;
        println!("created: {}", path.display());
    }

    println!();
    println!("총 {} 파일 생성. 한컴에서 열어 확인:", cases.len());
    println!("  open {}/forged-crossref-*.hwpx", out_dir.display());

    Ok(())
}

fn build_one(
    path: &std::path::Path,
    slug: &str,
    ref_type: &RefType,
    content_type: &RefContentType,
    target: RefTarget,
    as_hyperlink: bool,
    display_text: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let intro_para = Paragraph::with_runs(
        vec![Run::text(
            format!("Wave 12m 시각 검증 — kind={slug}, ref_type={ref_type:?}, content_type={content_type:?}, as_hyperlink={as_hyperlink}"),
            CharShapeIndex::new(0),
        )],
        ParaShapeIndex::new(0),
    );

    let label_para = Paragraph::with_runs(
        vec![Run::text("여기에 참조: ".to_string(), CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );

    let cross_ref = Control::CrossRef {
        target,
        ref_type: ref_type.clone(),
        content_type: content_type.clone(),
        as_hyperlink,
        display_text: display_text.to_string(),
    };

    let ref_para = Paragraph::with_runs(
        vec![
            Run::text("여기에 참조: ".to_string(), CharShapeIndex::new(0)),
            Run::control(cross_ref, CharShapeIndex::new(0)),
            Run::text(" — 끝".to_string(), CharShapeIndex::new(0)),
        ],
        ParaShapeIndex::new(0),
    );

    let section =
        Section::with_paragraphs(vec![intro_para, label_para, ref_para], PageSettings::a4());

    let mut doc = Document::<hwpforge_core::document::Draft>::new();
    doc.add_section(section);
    let validated = doc.validate()?;

    let style_store = HwpxStyleStore::default();
    let image_store = ImageStore::new();
    let bytes = HwpxEncoder::encode(&validated, &style_store, &image_store)?;
    std::fs::write(path, &bytes)?;

    Ok(())
}
