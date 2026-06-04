//! Wave 12n Phase 2 Step 7 — 시각 확인용 자동 필드 데모 HWPX 4종.
//!
//! Step 7의 encoder ↔ decoder round-trip 게이트는 본질적으로 메모리 내
//! `Control` ↔ XML 대칭성만 강제한다. 사용자가 실제 한컴오피스/뷰어에서
//! Wave 12n carry가 어떻게 보이는지 눈으로 확인할 수 있도록, Wave 12n에서
//! 새로 다루거나 의미가 바뀐 자동 필드를 모은 데모 HWPX를 생성한다.
//!
//! 산출물:
//!   examples/hwp5_review/forged-field-title.hwpx
//!     — $title (Wave 12n 새 토큰; gen_field_auto.rs는 cover 안 함)
//!   examples/hwp5_review/forged-field-page-current.hwpx
//!     — autoNum numType="PAGE" (현재 페이지 번호)
//!   examples/hwp5_review/forged-field-page-total.hwpx
//!     — autoNum numType="TOTAL_PAGE" (전체 페이지 수)
//!     — architect review CRITICAL: PAGE로 collapse 되지 않음을 시각 확인.
//!   examples/hwp5_review/forged-field-wave12n-all.hwpx
//!     — Wave 12n 5 SUMMERY + 2 InlinePageNumber 를 한 문서에 통합한 데모.
//!     — 한컴오피스에서 열어 각 필드가 정상 렌더링되는지 확인 가능.
//!
//! 사용법:
//!   cargo run -p hwpforge-smithy-hwpx --example gen_field_auto_visual

use hwpforge_core::control::{Control, InlinePageKind};
use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::PageSettings;
use hwpforge_foundation::{CharShapeIndex, FieldType, ParaShapeIndex};
use hwpforge_smithy_hwpx::presets::style_store_for_preset;
use hwpforge_smithy_hwpx::HwpxEncoder;

/// 단일 SUMMERY 필드를 라벨과 함께 담은 Section (한 문단 = 라벨, 다음 문단 = 필드).
fn summery_section(label: &str, field_type: FieldType) -> Section {
    let title = Paragraph::with_runs(
        vec![Run::text(label, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    let field = Control::Field { field_type, hint_text: None, help_text: None, name: None };
    let body = Paragraph::with_runs(
        vec![Run::control(field, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    Section::with_paragraphs(vec![title, body], PageSettings::a4())
}

/// 단일 InlinePageNumber 컨트롤을 라벨과 함께 담은 Section.
fn inline_page_section(label: &str, kind: InlinePageKind, raw_flag: u32) -> Section {
    let title = Paragraph::with_runs(
        vec![Run::text(label, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    let ctrl = Control::InlinePageNumber { kind, raw_flag };
    let body = Paragraph::with_runs(
        vec![Run::control(ctrl, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    Section::with_paragraphs(vec![title, body], PageSettings::a4())
}

/// Wave 12n 자동 필드 7종(SUMMERY 5 + InlinePageNumber 2)을 한 문서에 모은 통합 데모.
fn wave12n_all_section() -> Section {
    let mut paras = vec![Paragraph::with_runs(
        vec![Run::text("Wave 12n 자동 필드 시각 확인 데모", CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    )];

    let summery_cases: &[(&str, FieldType)] = &[
        ("[Author 저자]", FieldType::Author),
        ("[LastSavedBy 마지막 저장한 사람]", FieldType::LastSavedBy),
        ("[CreatedTime 만든 날짜]", FieldType::CreatedTime),
        ("[ModifiedTime 마지막 저장 날짜]", FieldType::ModifiedTime),
        ("[Title 문서 제목 (Wave 12n 신규)]", FieldType::Title),
    ];
    for (label, ft) in summery_cases {
        paras.push(Paragraph::with_runs(
            vec![Run::text(*label, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        ));
        paras.push(Paragraph::with_runs(
            vec![Run::control(
                Control::Field { field_type: *ft, hint_text: None, help_text: None, name: None },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        ));
    }

    let page_cases: &[(&str, InlinePageKind, u32)] = &[
        ("[InlinePageNumber CurrentPage 현재 페이지]", InlinePageKind::CurrentPage, 0),
        ("[InlinePageNumber TotalPages 전체 페이지]", InlinePageKind::TotalPages, 0x06),
    ];
    for (label, kind, flag) in page_cases {
        paras.push(Paragraph::with_runs(
            vec![Run::text(*label, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        ));
        paras.push(Paragraph::with_runs(
            vec![Run::control(
                Control::InlinePageNumber { kind: *kind, raw_flag: *flag },
                CharShapeIndex::new(0),
            )],
            ParaShapeIndex::new(0),
        ));
    }

    Section::with_paragraphs(paras, PageSettings::a4())
}

fn write_one(name: &str, section: Section) {
    let style_store = style_store_for_preset("default").expect("default preset must exist");
    let image_store = ImageStore::new();
    let mut doc = Document::new();
    doc.add_section(section);
    let validated = doc.validate().expect("validation");
    let bytes = HwpxEncoder::encode(&validated, &style_store, &image_store).expect("encode");
    let path = format!("examples/hwp5_review/forged-field-{name}.hwpx");
    std::fs::write(&path, &bytes).expect("write");
    println!("  {path} ({} bytes)", bytes.len());
}

fn main() {
    println!("=== Wave 12n Phase 2 Step 7: 시각 확인용 자동 필드 데모 HWPX ===\n");
    std::fs::create_dir_all("examples/hwp5_review").ok();

    write_one("title", summery_section("[Title 문서 제목 자동 필드]", FieldType::Title));
    write_one(
        "page-current",
        inline_page_section(
            "[현재 페이지 번호 InlinePageNumber CurrentPage]",
            InlinePageKind::CurrentPage,
            0,
        ),
    );
    write_one(
        "page-total",
        inline_page_section(
            "[전체 페이지 수 InlinePageNumber TotalPages]",
            InlinePageKind::TotalPages,
            0x06,
        ),
    );
    write_one("wave12n-all", wave12n_all_section());

    println!("\n시각 확인 절차:");
    println!("  1. 위 4개 .hwpx 파일을 한컴오피스에서 연다");
    println!("  2. 각 자동 필드가 정상 렌더링 되는지 확인");
    println!("     - SUMMERY 5종: 문서 정보 → 해당 필드값으로 치환되어 보임");
    println!("     - PAGE/TOTAL_PAGE: 페이지 번호/전체 페이지가 숫자로 보임");
    println!("  3. forged-field-wave12n-all.hwpx 가 가장 빠른 통합 확인 경로");
}
