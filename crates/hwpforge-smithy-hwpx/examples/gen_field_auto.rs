//! Wave 12n Phase 1A — HwpForge 의 현재 SUMMERY (Date/Time/DocSummary/UserInfo)
//! emission 을 4개의 독립 HWPX 파일로 박제.
//!
//! 산출물:
//!   examples/hwp5_review/forged-field-date.hwpx
//!   examples/hwp5_review/forged-field-time.hwpx
//!   examples/hwp5_review/forged-field-docsummary.hwpx
//!   examples/hwp5_review/forged-field-userinfo.hwpx
//!
//! 사용법:
//!   cargo run -p hwpforge-smithy-hwpx --example gen_field_auto
//!
//! 목적: 사용자 한컴 native .hwp → .hwpx export 결과와 cross-compare 하여
//! 현재 build_summery_field_xml 의 하드코딩/추측 (fieldid=628321650,
//! $modifiedtime, " " display 등) 이 한컴 진실과 일치하는지 검증.

use hwpforge_core::control::Control;
use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::PageSettings;
use hwpforge_foundation::{CharShapeIndex, FieldType, ParaShapeIndex};
use hwpforge_smithy_hwpx::presets::style_store_for_preset;
use hwpforge_smithy_hwpx::HwpxEncoder;

fn field_section(label: &str, field_type: FieldType, hint: &str, help: &str) -> Section {
    let title = Paragraph::with_runs(
        vec![Run::text(label, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    let field = Control::Field {
        field_type,
        hint_text: if hint.is_empty() { None } else { Some(hint.to_string()) },
        help_text: if help.is_empty() { None } else { Some(help.to_string()) },
        name: None,
        display_text: String::new(),
    };
    let body = Paragraph::with_runs(
        vec![Run::control(field, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    Section::with_paragraphs(vec![title, body], PageSettings::a4())
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
    println!("=== Wave 12n Phase 1A: HwpForge HWPX auto-field fixtures ===");
    std::fs::create_dir_all("examples/hwp5_review").ok();

    write_one(
        "date",
        field_section(
            "[ModifiedTime 자동 필드]",
            FieldType::ModifiedTime,
            "날짜",
            "마지막 저장한 날짜",
        ),
    );
    write_one(
        "time",
        field_section("[CreatedTime 자동 필드]", FieldType::CreatedTime, "시간", "만든 날짜"),
    );
    write_one(
        "docsummary",
        field_section("[Author 자동 필드]", FieldType::Author, "저자명", "문서 정보 → 만든 사람"),
    );
    write_one(
        "userinfo",
        field_section(
            "[LastSavedBy 자동 필드]",
            FieldType::LastSavedBy,
            "사용자명",
            "마지막 저장한 사람",
        ),
    );

    println!("\n다음 단계: 사용자가 한컴에서 4종 native .hwp 를 작성하면");
    println!("  examples/hwp5_review/sample-field-{{date,time,docsummary,userinfo}}.hwp");
    println!("이후 cross-compare 로 HWPX 인코더 갭 식별");
}
