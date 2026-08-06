//! CI-runnable 렌더 e2e — 커밋된 테스트 폰트 + 합성 문서.
//!
//! `render_pdf.rs` 의 실물 fixture 게이트는 한컴 폰트가 설치된 머신 전용이다
//! (fixture-optional). 이 파일은 렌더 파이프라인(source→shape/align→paint→
//! krilla)이 전 환경에서 실행되도록 자체 제작 테스트 폰트
//! (`tests/fonts/`, `generate_test_fonts.py` — space 0.3em / Latin 0.6em /
//! 한글 1.0em 고정 메트릭)로 e2e 를 돈다.

use std::path::PathBuf;

use hwpforge_core::document::{Document, Validated};
use hwpforge_core::layout::{LayoutCache, LineSeg};
use hwpforge_core::page::PageSettings;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::StyleLookup;
use hwpforge_foundation::{Alignment, CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_pdf::{render_document, PdfError, PdfInput, PdfOptions, PdfWarning};

/// 테스트 폰트만 등록하는 스타일 컨텍스트 (10pt 고정).
struct TestStyles {
    alignment: Alignment,
    bold: bool,
    face: &'static str,
}

impl Default for TestStyles {
    fn default() -> Self {
        Self { alignment: Alignment::Left, bold: false, face: "HwpForge Test" }
    }
}

impl StyleLookup for TestStyles {
    fn char_bold(&self, _id: CharShapeIndex) -> Option<bool> {
        Some(self.bold)
    }

    fn char_font_name(&self, _id: CharShapeIndex) -> Option<&str> {
        Some(self.face)
    }

    fn char_font_size(&self, _id: CharShapeIndex) -> Option<HwpUnit> {
        Some(HwpUnit::from_pt(10.0).expect("10pt"))
    }

    fn para_alignment(&self, _id: ParaShapeIndex) -> Option<Alignment> {
        Some(self.alignment)
    }
}

/// 스타일 결손 컨텍스트 — `StyleUnavailable` 경로 검증용.
struct NoStyles;
impl StyleLookup for NoStyles {}

fn options() -> PdfOptions {
    let mut options = PdfOptions::default();
    options.font_dirs = vec![PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fonts"))];
    options
}

fn seg(textpos: u32, vertpos: i32) -> LineSeg {
    LineSeg {
        textpos,
        vertpos,
        vertsize: 1000,
        textheight: 1000,
        baseline: 850,
        spacing: 600,
        horzpos: 0,
        horzsize: 42548,
        flags: 0,
    }
}

fn para_with_cache(text: &str, lines: Vec<LineSeg>) -> Paragraph {
    let mut p =
        Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0));
    p.layout_cache = Some(LayoutCache::new(lines));
    p
}

fn doc_of(paragraphs: Vec<Paragraph>) -> Document<Validated> {
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(paragraphs, PageSettings::a4()));
    doc.validate().expect("validate")
}

fn page_count(pdf: &[u8]) -> usize {
    // krilla 는 공백 없는 사전 문법으로 쓴다 — /Type/Pages(트리) 를 빼면 잎 쪽 수.
    let hay = String::from_utf8_lossy(pdf);
    hay.matches("/Type/Page").count() - hay.matches("/Type/Pages").count()
}

#[test]
fn renders_synthetic_two_page_document() {
    // p1 = 2줄 (textpos 4 에서 절단 — 줄1 "가나다 " 꼬리 공백은 렌더가 trim),
    // p2 = v==0 재출현 → 새 쪽 (규칙 §2 쪽분할).
    let doc = doc_of(vec![
        para_with_cache("가나다 라마바", vec![seg(0, 0), seg(4, 1600)]),
        para_with_cache("A가나", vec![seg(0, 0)]),
    ]);
    let styles = TestStyles::default();
    let input = PdfInput { document: &doc, styles: &styles };
    let output = render_document(&input, &options()).expect("render");
    assert!(output.bytes.starts_with(b"%PDF-"), "PDF 헤더");
    assert_eq!(page_count(&output.bytes), 2, "v 리셋 = 새 쪽");
    assert!(output.warnings.is_empty(), "{:?}", output.warnings);
}

#[test]
fn justify_line_with_interior_space_needs_no_approximation() {
    // 줄1 = "가나 다 " → 꼬리 공백 trim 후 내부 공백 1개가 JUSTIFY 분모 —
    // 근사 경고 없이 배분돼야 한다 (마지막 줄은 자연폭).
    let doc = doc_of(vec![para_with_cache("가나 다 라마바", vec![seg(0, 0), seg(5, 1600)])]);
    let styles = TestStyles { alignment: Alignment::Justify, ..TestStyles::default() };
    let input = PdfInput { document: &doc, styles: &styles };
    let output = render_document(&input, &options()).expect("render");
    assert!(
        !output.warnings.iter().any(|w| matches!(w, PdfWarning::AlignmentApproximated { .. })),
        "{:?}",
        output.warnings
    );
}

#[test]
fn missing_cache_paragraph_warns_and_still_renders() {
    // 기본 정책 WarnAndSkip: 캐시 결손 문단은 경고 + 스킵, PDF 는 나온다.
    let uncached = Paragraph::with_runs(
        vec![Run::text("결손", CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    let doc = doc_of(vec![para_with_cache("가나다", vec![seg(0, 0)]), uncached]);
    let styles = TestStyles::default();
    let input = PdfInput { document: &doc, styles: &styles };
    let output = render_document(&input, &options()).expect("render");
    assert!(output.bytes.starts_with(b"%PDF-"));
    assert_eq!(
        output.warnings.iter().filter(|w| matches!(w, PdfWarning::ParagraphSkipped { .. })).count(),
        1
    );
}

#[test]
fn bold_run_warns_non_regular_but_renders() {
    // W2 는 regular 만 정합 보장 — bold run 은 경고 후 regular face 로 그린다.
    let doc = doc_of(vec![para_with_cache("가나다", vec![seg(0, 0)])]);
    let styles = TestStyles { bold: true, ..TestStyles::default() };
    let input = PdfInput { document: &doc, styles: &styles };
    let output = render_document(&input, &options()).expect("render");
    assert!(output.bytes.starts_with(b"%PDF-"));
    assert!(output.warnings.iter().any(|w| matches!(w, PdfWarning::NonRegularRun { .. })));
}

#[test]
fn unknown_face_fails_closed_without_fallback() {
    let doc = doc_of(vec![para_with_cache("가나다", vec![seg(0, 0)])]);
    let styles = TestStyles { face: "존재하지 않는 서체", ..TestStyles::default() };
    let input = PdfInput { document: &doc, styles: &styles };
    let err = render_document(&input, &options()).unwrap_err();
    assert!(matches!(err, PdfError::FontUnresolved { .. }), "{err:?}");
}

#[test]
fn style_without_font_name_is_unavailable() {
    let doc = doc_of(vec![para_with_cache("가나다", vec![seg(0, 0)])]);
    let input = PdfInput { document: &doc, styles: &NoStyles };
    let err = render_document(&input, &options()).unwrap_err();
    assert!(matches!(err, PdfError::StyleUnavailable { what: "font name", .. }), "{err:?}");
}

/// 폰트명은 있지만 크기가 결손 — `StyleUnavailable { what: "font size" }` 경로.
struct NoSizeStyles;
impl StyleLookup for NoSizeStyles {
    fn char_font_name(&self, _id: CharShapeIndex) -> Option<&str> {
        Some("HwpForge Test")
    }
}

#[test]
fn style_without_font_size_is_unavailable() {
    let doc = doc_of(vec![para_with_cache("가나다", vec![seg(0, 0)])]);
    let input = PdfInput { document: &doc, styles: &NoSizeStyles };
    let err = render_document(&input, &options()).unwrap_err();
    assert!(matches!(err, PdfError::StyleUnavailable { what: "font size", .. }), "{err:?}");
}

#[test]
fn whitespace_only_line_renders_without_glyphs() {
    // 줄 전체가 공백 → 꼬리 trim 후 빈 텍스트 — 그리지 않고 넘어가되 PDF 는 유효.
    let doc = doc_of(vec![para_with_cache("   ", vec![seg(0, 0)])]);
    let styles = TestStyles::default();
    let input = PdfInput { document: &doc, styles: &styles };
    let output = render_document(&input, &options()).expect("render");
    assert!(output.bytes.starts_with(b"%PDF-"));
    assert_eq!(page_count(&output.bytes), 1);
    assert!(output.warnings.is_empty(), "{:?}", output.warnings);
}
