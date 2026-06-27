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
use hwpforge_core::metadata::Metadata;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::PageSettings;
use hwpforge_foundation::{CharShapeIndex, FieldType, ParaShapeIndex};
use hwpforge_smithy_hwpx::presets::style_store_for_preset;
use hwpforge_smithy_hwpx::HwpxEncoder;

/// 단일 SUMMERY 필드를 라벨과 함께 담은 Section (한 문단 = 라벨, 다음 문단 = 필드).
///
/// `cached_value` 는 `<hp:fieldBegin>`/`<hp:fieldEnd>` 사이에 박히는 stale
/// display text. 한컴 native HWPX는 항상 이 자리에 메타데이터 evaluated
/// 결과를 cache 해 두며, `dirty="0"` 이면 한컴이 그 cached 값을 그대로
/// 화면에 표시한다. HwpForge encoder는 `hint_text` 를 cache 자리로
/// 라우팅하므로, 시각 확인 데모에서는 이 자리에 의미있는 텍스트를 넣어
/// `[문서 정보 시작]값[문서 정보 끝]` 형태가 보이게 한다.
fn summary_section(label: &str, field_type: FieldType, cached_value: &str) -> Section {
    let title = Paragraph::with_runs(
        vec![Run::text(label, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    let field = Control::Field {
        field_type,
        hint_text: Some(cached_value.to_string()),
        help_text: None,
        name: None,
        display_text: String::new(),
    };
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

    // `cached_value` 는 한컴 native HWPX가 fieldBegin/fieldEnd 사이에 박아두는
    // stale display text. HwpForge encoder는 `hint_text` 를 그 자리에 라우팅
    // 하므로 데모용으로 의미있는 텍스트를 채워 시각 확인이 가능하게 한다.
    let summary_cases: &[(&str, FieldType, &str)] = &[
        ("[Author 저자]", FieldType::Author, "홍길동"),
        ("[LastSavedBy 마지막 저장한 사람]", FieldType::LastSavedBy, "김편집"),
        ("[CreatedTime 만든 날짜]", FieldType::CreatedTime, "2026-06-04 09:00:00"),
        ("[ModifiedTime 마지막 저장 날짜]", FieldType::ModifiedTime, "2026-06-04 11:20:00"),
        ("[Title 문서 제목 (Wave 12n 신규)]", FieldType::Title, "HwpForge Wave 12n 데모 문서"),
    ];
    for (label, ft, cached) in summary_cases {
        paras.push(Paragraph::with_runs(
            vec![Run::text(*label, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        ));
        paras.push(Paragraph::with_runs(
            vec![Run::control(
                Control::Field {
                    field_type: *ft,
                    hint_text: Some((*cached).to_string()),
                    help_text: None,
                    name: None,
                    display_text: String::new(),
                },
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

fn write_one(name: &str, metadata: Metadata, section: Section) {
    let style_store = style_store_for_preset("default").expect("default preset must exist");
    let image_store = ImageStore::new();
    let mut doc = Document::with_metadata(metadata);
    doc.add_section(section);
    let validated = doc.validate().expect("validation");
    let bytes = HwpxEncoder::encode(&validated, &style_store, &image_store).expect("encode");
    let path = format!("examples/hwp5_review/forged-field-{name}.hwpx");
    std::fs::write(&path, &bytes).expect("write");
    println!("  {path} ({} bytes)", bytes.len());
}

/// Wave 12o demo metadata — populated so Hancom Office's SUMMERY
/// auto-field evaluator finds real values for `$title`/`$author`/
/// `$lastsaveby`/`$createtime`/`$modifiedtime` instead of falling back
/// to the first paragraph text.
fn demo_metadata(title: &str) -> Metadata {
    Metadata::new()
        .with_title(title)
        .with_author("홍길동")
        .with_subject("Wave 12o 자동 필드 시각 확인")
        .with_description(
            "HwpForge Wave 12o Phase 0-2 통과 후 한컴 byte-parity 메타데이터 carry 확인용",
        )
        .with_last_saved_by("김편집")
        .with_keywords(["wave12o", "metadata", "데모"])
        .with_created("2026-06-04T09:00:00Z")
        .with_modified("2026-06-04T11:20:00Z")
}

fn main() {
    println!("=== Wave 12o Phase 0-2: 메타데이터 carry 시각 확인용 자동 필드 데모 HWPX ===\n");
    std::fs::create_dir_all("examples/hwp5_review").ok();

    write_one(
        "title",
        demo_metadata("HwpForge Wave 12o 데모 문서"),
        summary_section(
            "[Title 문서 제목 자동 필드]",
            FieldType::Title,
            "HwpForge Wave 12o 데모 문서",
        ),
    );
    write_one(
        "page-current",
        demo_metadata("HwpForge Wave 12o 페이지 번호 데모 (현재)"),
        inline_page_section(
            "[현재 페이지 번호 InlinePageNumber CurrentPage]",
            InlinePageKind::CurrentPage,
            0,
        ),
    );
    write_one(
        "page-total",
        demo_metadata("HwpForge Wave 12o 페이지 번호 데모 (전체)"),
        inline_page_section(
            "[전체 페이지 수 InlinePageNumber TotalPages]",
            InlinePageKind::TotalPages,
            0x06,
        ),
    );
    write_one(
        "wave12n-all",
        demo_metadata("HwpForge Wave 12o 자동 필드 통합 데모"),
        wave12n_all_section(),
    );

    println!("\n시각 확인 절차 (Wave 12o):");
    println!("  1. 위 4개 .hwpx 파일을 한컴오피스에서 연다");
    println!("  2. 자동 필드가 metadata 기반으로 정상 렌더링되는지 확인:");
    println!("     - $title → 'HwpForge Wave 12o …'");
    println!("     - $author → '홍길동'");
    println!("     - $lastsaveby → '김편집'");
    println!("     - $createtime / $modifiedtime → 2026-06-04 …");
    println!("  3. ★ 한컴에서 한 번 저장 후 다시 열어도 위 값이 유지되는지 확인 (이전 버그 해소)");
    println!("  4. 각 자동 필드가 정상 렌더링 되는지 확인");
    println!("     - SUMMERY 5종: 문서 정보 → 해당 필드값으로 치환되어 보임");
    println!("     - PAGE/TOTAL_PAGE: 페이지 번호/전체 페이지가 숫자로 보임");
    println!("  3. forged-field-wave12n-all.hwpx 가 가장 빠른 통합 확인 경로");
}
