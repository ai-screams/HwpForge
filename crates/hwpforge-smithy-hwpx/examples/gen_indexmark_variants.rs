//! Wave 12k IndexMark (찾아보기 표시) fixture 생성기.
//!
//! 한컴 메뉴: 입력 → 참조 → 찾아보기 표시. 두 단계(primary/secondary)
//! 키워드 지원. 사용자가 한컴에서 .hwp 로 저장하면 HWP5 wire 진단용
//! fixture가 된다.
//!
//! 변형:
//! 1. Primary only — 가장 단순 (1-keyword indexmark)
//! 2. Primary + secondary — 2-level (sub-entry)
//! 3. 같은 paragraph 안의 여러 indexmark — 위치 관계
//! 4. 한글 / 영문 / 한자 / 혼합 키워드 — 인코딩 stress
//! 5. paragraph 시작/중간/끝 위치
//!
//! 사용법:
//! ```text
//! cargo run -p hwpforge-smithy-hwpx --example gen_indexmark_variants -- \
//!     /tmp/wave12k-indexmark/sample-indexmark-multi.hwpx
//! ```

use std::path::PathBuf;

use hwpforge_core::control::Control;
use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::PageSettings;
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge_smithy_hwpx::style_store::HwpxStyleStore;
use hwpforge_smithy_hwpx::HwpxEncoder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args();
    let _program = args.next();
    let output: PathBuf = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/wave12k-indexmark/sample-indexmark-multi.hwpx"));

    let cs = CharShapeIndex::new(0);
    let ps = ParaShapeIndex::new(0);

    // Helper: paragraph with text + IndexMark inserted at a given anchor.
    fn anchored(
        prefix: &str,
        anchor: &str,
        suffix: &str,
        primary: &str,
        secondary: Option<&str>,
        cs: CharShapeIndex,
        ps: ParaShapeIndex,
    ) -> Paragraph {
        Paragraph::with_runs(
            vec![
                Run::text(prefix, cs),
                Run::text(anchor, cs),
                Run::control(
                    Control::IndexMark {
                        primary: primary.to_string(),
                        secondary: secondary.map(str::to_string),
                    },
                    cs,
                ),
                Run::text(suffix, cs),
            ],
            ps,
        )
    }

    let paragraphs: Vec<Paragraph> = vec![
        // 1. Primary only — 한글
        anchored("예시 1: ", "컴퓨터", "는 정보 처리 장치다.", "컴퓨터", None, cs, ps),
        // 2. Primary + secondary — 한글 2-level
        anchored(
            "예시 2: ",
            "하드웨어",
            "는 컴퓨터의 물리적 부품이다.",
            "컴퓨터",
            Some("하드웨어"),
            cs,
            ps,
        ),
        // 3. Primary + secondary — 영문
        anchored("예시 3: ", "RAM", " is a type of memory.", "Memory", Some("RAM"), cs, ps),
        // 4. 한자 키워드
        anchored("예시 4: ", "韓國", "은 동아시아의 나라다.", "韓國", None, cs, ps),
        // 5. 혼합 — primary 한글 / secondary 영문
        anchored("예시 5: ", "운영체제", "는 OS라고도 한다.", "운영체제", Some("OS"), cs, ps),
        // 6. Same paragraph, multiple indexmarks (2개)
        Paragraph::with_runs(
            vec![
                Run::text("예시 6: ", cs),
                Run::text("CPU", cs),
                Run::control(
                    Control::IndexMark { primary: "CPU".to_string(), secondary: None },
                    cs,
                ),
                Run::text(" 와 ", cs),
                Run::text("GPU", cs),
                Run::control(
                    Control::IndexMark { primary: "GPU".to_string(), secondary: None },
                    cs,
                ),
                Run::text(" 는 모두 처리 장치다.", cs),
            ],
            ps,
        ),
        // 7. 빈 secondary (None) vs. 빈 문자열 secondary — 동일 처리 확인용
        anchored(
            "예시 7: ",
            "네트워크",
            "는 통신망이다.",
            "네트워크",
            Some(""), // 빈 문자열 — None 과 차이가 wire에 보이는지 확인
            cs,
            ps,
        ),
    ];

    let section = Section::with_paragraphs(paragraphs, PageSettings::a4());

    let mut doc = Document::new();
    doc.add_section(section);
    let validated = doc.validate()?;

    let store = HwpxStyleStore::with_default_fonts("함초롬바탕");
    let image_store = ImageStore::new();
    let bytes = HwpxEncoder::encode(&validated, &store, &image_store)?;

    std::fs::create_dir_all(output.parent().expect("output path has a parent"))?;
    std::fs::write(&output, &bytes)?;
    println!("Wrote {} ({} bytes) — 7 paragraphs with 8 IndexMarks", output.display(), bytes.len());
    Ok(())
}
