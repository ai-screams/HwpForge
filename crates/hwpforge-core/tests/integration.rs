//! Integration tests for hwpforge-core.
//!
//! These tests exercise the full document lifecycle across module boundaries:
//! create -> populate -> validate -> serialize -> deserialize -> re-validate.

use hwpforge_core::control::Control;
use hwpforge_core::document::{Document, Draft};
use hwpforge_core::error::{CoreError, ValidationError};
use hwpforge_core::image::{Image, ImageFormat};
use hwpforge_core::metadata::Metadata;
use hwpforge_core::page::PageSettings;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_foundation::{CharShapeIndex, Color, HwpUnit, ParaShapeIndex};

// ==========================================================================
// Helpers
// ==========================================================================

fn text_run(s: &str) -> Run {
    Run::text(s, CharShapeIndex::new(0))
}

fn simple_paragraph(text: &str) -> Paragraph {
    Paragraph::with_runs(vec![text_run(text)], ParaShapeIndex::new(0))
}

fn simple_section(text: &str) -> Section {
    Section::with_paragraphs(vec![simple_paragraph(text)], PageSettings::a4())
}

fn minimal_valid_document() -> Document<Draft> {
    let mut doc = Document::new();
    doc.add_section(simple_section("Hello"));
    doc
}

// ==========================================================================
// Full Lifecycle Tests
// ==========================================================================

#[test]
fn lifecycle_create_validate_serialize_deserialize_revalidate() {
    // Create
    let mut doc = Document::with_metadata(Metadata {
        title: Some("Lifecycle Test".to_string()),
        author: Some("Test Author".to_string()),
        ..Metadata::default()
    });
    doc.add_section(simple_section("First section"));
    doc.add_section(simple_section("Second section"));

    // Validate
    let validated = doc.validate().unwrap();
    assert_eq!(validated.section_count(), 2);
    assert_eq!(validated.metadata().title.as_deref(), Some("Lifecycle Test"));

    // Serialize
    let json = serde_json::to_string_pretty(&validated).unwrap();
    assert!(json.contains("Lifecycle Test"));
    assert!(json.contains("First section"));

    // Deserialize (always to Draft)
    let deserialized: Document<Draft> = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.section_count(), 2);

    // Re-validate
    let re_validated = deserialized.validate().unwrap();
    assert_eq!(validated, re_validated);
}

#[test]
fn lifecycle_complex_document_with_all_content_types() {
    let mut doc = Document::with_metadata(Metadata {
        title: Some("Complex Document".to_string()),
        keywords: vec!["test".to_string(), "complex".to_string()],
        created: Some("2026-02-07T10:00:00Z".to_string()),
        ..Metadata::default()
    });

    // Section 1: Mixed content
    let cell = TableCell::new(
        vec![simple_paragraph("Cell content")],
        HwpUnit::from_mm(70.0).unwrap(),
    );
    let table = Table {
        rows: vec![
            TableRow {
                cells: vec![cell.clone(), cell.clone()],
                height: None,
            },
            TableRow {
                cells: vec![cell.clone(), cell],
                height: Some(HwpUnit::from_mm(15.0).unwrap()),
            },
        ],
        width: Some(HwpUnit::from_mm(140.0).unwrap()),
        caption: Some("Table 1: Test Data".to_string()),
    };

    let image = Image::new(
        "BinData/chart.png",
        HwpUnit::from_mm(120.0).unwrap(),
        HwpUnit::from_mm(80.0).unwrap(),
        ImageFormat::Png,
    );

    let hyperlink = Control::Hyperlink {
        text: "Visit our site".to_string(),
        url: "https://hwpforge.dev".to_string(),
    };

    let text_box = Control::TextBox {
        paragraphs: vec![simple_paragraph("Inside text box")],
        width: HwpUnit::from_mm(60.0).unwrap(),
        height: HwpUnit::from_mm(30.0).unwrap(),
    };

    let footnote = Control::Footnote {
        paragraphs: vec![simple_paragraph("Footnote body")],
    };

    let section1 = Section::with_paragraphs(
        vec![
            Paragraph::with_runs(
                vec![
                    Run::text("Introduction paragraph with ", CharShapeIndex::new(0)),
                    Run::text("multiple runs.", CharShapeIndex::new(1)),
                ],
                ParaShapeIndex::new(0),
            ),
            Paragraph::with_runs(
                vec![Run::table(table, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            ),
            Paragraph::with_runs(
                vec![Run::image(image, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            ),
            Paragraph::with_runs(
                vec![Run::control(hyperlink, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            ),
            Paragraph::with_runs(
                vec![Run::control(text_box, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            ),
            Paragraph::with_runs(
                vec![Run::control(footnote, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            ),
        ],
        PageSettings::a4(),
    );

    // Section 2: Landscape letter
    let section2 = Section::with_paragraphs(
        vec![simple_paragraph("Landscape section")],
        PageSettings::letter(),
    );

    doc.add_section(section1);
    doc.add_section(section2);

    let validated = doc.validate().unwrap();
    assert_eq!(validated.section_count(), 2);

    // Round-trip
    let json = serde_json::to_string(&validated).unwrap();
    let back: Document<Draft> = serde_json::from_str(&json).unwrap();
    let re_validated = back.validate().unwrap();
    assert_eq!(validated, re_validated);
}

// ==========================================================================
// Validation Error Tests (Cross-module)
// ==========================================================================

#[test]
fn validation_rejects_empty_document() {
    let doc = Document::new();
    let err = doc.validate().unwrap_err();
    match err {
        CoreError::Validation(ValidationError::EmptyDocument) => {}
        other => panic!("Expected EmptyDocument, got: {other}"),
    }
}

#[test]
fn validation_rejects_section_with_empty_paragraph() {
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::new(ParaShapeIndex::new(0))],
        PageSettings::a4(),
    ));
    assert!(doc.validate().is_err());
}

#[test]
fn validation_rejects_table_with_zero_col_span() {
    let cell = TableCell::with_span(
        vec![simple_paragraph("cell")],
        HwpUnit::from_mm(50.0).unwrap(),
        0, // invalid!
        1,
    );
    let table = Table::new(vec![TableRow { cells: vec![cell], height: None }]);

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::table(table, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));

    let err = doc.validate().unwrap_err();
    match err {
        CoreError::Validation(ValidationError::InvalidSpan { field: "col_span", .. }) => {}
        other => panic!("Expected InvalidSpan, got: {other}"),
    }
}

#[test]
fn validation_rejects_empty_text_box() {
    let ctrl = Control::TextBox {
        paragraphs: vec![],
        width: HwpUnit::from_mm(80.0).unwrap(),
        height: HwpUnit::from_mm(40.0).unwrap(),
    };

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::control(ctrl, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));

    assert!(doc.validate().is_err());
}

// ==========================================================================
// Text Extraction (Cross-module)
// ==========================================================================

#[test]
fn text_extraction_from_complex_document() {
    let doc = minimal_valid_document();
    let section = &doc.sections()[0];
    assert_eq!(section.paragraphs[0].text_content(), "Hello");
}

#[test]
fn text_extraction_multi_run_paragraph() {
    let para = Paragraph::with_runs(
        vec![
            text_run("Hello "),
            Run::table(Table::new(vec![TableRow {
                cells: vec![TableCell::new(
                    vec![simple_paragraph("ignored")],
                    HwpUnit::from_mm(50.0).unwrap(),
                )],
                height: None,
            }]), CharShapeIndex::new(0)),
            text_run("world"),
        ],
        ParaShapeIndex::new(0),
    );
    assert_eq!(para.text_content(), "Hello world");
}

// ==========================================================================
// Korean Text
// ==========================================================================

#[test]
fn korean_document_lifecycle() {
    let mut doc = Document::with_metadata(Metadata {
        title: Some("한글 문서".to_string()),
        author: Some("김철수".to_string()),
        ..Metadata::default()
    });
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![
                Run::text("안녕하세요, ", CharShapeIndex::new(0)),
                Run::text("세계!", CharShapeIndex::new(1)),
            ],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));

    let validated = doc.validate().unwrap();
    let json = serde_json::to_string(&validated).unwrap();
    assert!(json.contains("안녕하세요"));
    assert!(json.contains("한글 문서"));

    let back: Document<Draft> = serde_json::from_str(&json).unwrap();
    let re_validated = back.validate().unwrap();
    assert_eq!(validated, re_validated);
}

// ==========================================================================
// Edge Cases
// ==========================================================================

#[test]
fn large_document_with_many_sections() {
    let mut doc = Document::new();
    for i in 0..100 {
        doc.add_section(simple_section(&format!("Section {i}")));
    }
    let validated = doc.validate().unwrap();
    assert_eq!(validated.section_count(), 100);
}

#[test]
fn paragraph_with_many_runs() {
    let runs: Vec<Run> = (0..1000).map(|i| text_run(&format!("run{i} "))).collect();
    let para = Paragraph::with_runs(runs, ParaShapeIndex::new(0));
    assert_eq!(para.run_count(), 1000);
    assert!(para.text_content().starts_with("run0 "));
    assert!(para.text_content().contains("run999 "));
}

#[test]
fn empty_text_run_is_valid() {
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::text("", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    assert!(doc.validate().is_ok());
}

#[test]
fn merged_cell_table_validates() {
    let merged_cell = TableCell::with_span(
        vec![simple_paragraph("merged")],
        HwpUnit::from_mm(100.0).unwrap(),
        3,
        2,
    );
    let regular_cell = TableCell::new(
        vec![simple_paragraph("normal")],
        HwpUnit::from_mm(50.0).unwrap(),
    );
    let table = Table::new(vec![
        TableRow { cells: vec![merged_cell], height: None },
        TableRow { cells: vec![regular_cell], height: None },
    ]);

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::table(table, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    assert!(doc.validate().is_ok());
}

#[test]
fn table_cell_with_background_color() {
    let mut cell = TableCell::new(
        vec![simple_paragraph("colored")],
        HwpUnit::from_mm(50.0).unwrap(),
    );
    cell.background = Some(Color::from_rgb(255, 200, 200));

    let table = Table::new(vec![TableRow { cells: vec![cell], height: None }]);

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::table(table, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));

    let validated = doc.validate().unwrap();
    let json = serde_json::to_string(&validated).unwrap();
    let back: Document<Draft> = serde_json::from_str(&json).unwrap();
    let re_validated = back.validate().unwrap();
    assert_eq!(validated, re_validated);
}

#[test]
fn unknown_control_preserved_through_roundtrip() {
    let ctrl = Control::Unknown {
        tag: "custom:element".to_string(),
        data: Some("<custom>data</custom>".to_string()),
    };

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::control(ctrl, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));

    let validated = doc.validate().unwrap();
    let json = serde_json::to_string(&validated).unwrap();
    assert!(json.contains("custom:element"));
    assert!(json.contains("<custom>data</custom>"));

    let back: Document<Draft> = serde_json::from_str(&json).unwrap();
    let re_validated = back.validate().unwrap();
    assert_eq!(validated, re_validated);
}

// ==========================================================================
// Typestate Enforcement (Compile-time)
// ==========================================================================

// The compile_fail doc tests in document.rs verify that:
// - Document<Validated> cannot call add_section()
// - Document<Validated> cannot call set_metadata()
// These are tested via rustdoc compile_fail blocks.

// ==========================================================================
// Proptest
// ==========================================================================

use proptest::prelude::*;

fn arb_text() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-zA-Z0-9 ]{1,50}").unwrap()
}

fn arb_run() -> impl Strategy<Value = Run> {
    arb_text().prop_map(|s| Run::text(s, CharShapeIndex::new(0)))
}

fn arb_paragraph() -> impl Strategy<Value = Paragraph> {
    prop::collection::vec(arb_run(), 1..5)
        .prop_map(|runs| Paragraph::with_runs(runs, ParaShapeIndex::new(0)))
}

fn arb_section() -> impl Strategy<Value = Section> {
    prop::collection::vec(arb_paragraph(), 1..5)
        .prop_map(|paragraphs| Section::with_paragraphs(paragraphs, PageSettings::a4()))
}

proptest! {
    #[test]
    fn prop_document_roundtrip(sections in prop::collection::vec(arb_section(), 1..4)) {
        let mut doc = Document::new();
        for section in sections {
            doc.add_section(section);
        }

        let validated = doc.validate().unwrap();
        let json = serde_json::to_string(&validated).unwrap();
        let back: Document<Draft> = serde_json::from_str(&json).unwrap();
        let re_validated = back.validate().unwrap();
        prop_assert_eq!(validated, re_validated);
    }

    #[test]
    fn prop_validated_has_sections(sections in prop::collection::vec(arb_section(), 1..10)) {
        let mut doc = Document::new();
        for section in &sections {
            doc.add_section(section.clone());
        }
        let validated = doc.validate().unwrap();
        prop_assert!(validated.section_count() >= 1);
        prop_assert_eq!(validated.section_count(), sections.len());
    }

    #[test]
    fn prop_text_content_extraction(
        texts in prop::collection::vec(arb_text(), 1..5)
    ) {
        let runs: Vec<Run> = texts.iter().map(|t| Run::text(t.as_str(), CharShapeIndex::new(0))).collect();
        let para = Paragraph::with_runs(runs, ParaShapeIndex::new(0));
        let expected: String = texts.concat();
        prop_assert_eq!(para.text_content(), expected);
    }

    #[test]
    fn prop_run_text_accessor(s in arb_text()) {
        let run = Run::text(s.as_str(), CharShapeIndex::new(0));
        prop_assert_eq!(run.content.as_text(), Some(s.as_str()));
    }
}
