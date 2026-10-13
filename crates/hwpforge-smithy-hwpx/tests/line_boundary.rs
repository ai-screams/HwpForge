//! S22: the encoder places a line that carries a boundary before the
//! zero-length token it names, and drops the cache (with a warning) when the
//! boundary does not match the paragraph's tokens.

use hwpforge_core::control::Control;
use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::layout::{LayoutCache, LineBoundary, LineSeg, WireTokenKind};
use hwpforge_core::page::PageSettings;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::{HeaderFooter, PageNumber, Section};
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_foundation::{CharShapeIndex, FieldType, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::{style_store_for_preset, EncodeOptions, HwpxEncoder};

use WireTokenKind::*;

fn line(textpos: u32, boundary: Option<LineBoundary>) -> LineSeg {
    let mut l = LineSeg::default();
    l.textpos = textpos;
    l.vertsize = 1000;
    l.textheight = 1000;
    l.horzsize = 40000;
    l.boundary = boundary;
    l
}

fn at(skip: &[WireTokenKind], next: Option<WireTokenKind>) -> Option<LineBoundary> {
    Some(LineBoundary::new(skip.to_vec(), next))
}

fn table() -> Run {
    let cell = TableCell::new(
        vec![Paragraph::with_runs(
            vec![Run::text("칸", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        HwpUnit::new(4000).unwrap(),
    );
    Run::table(Table::new(vec![TableRow::new(vec![cell])]), CharShapeIndex::new(0))
}

/// The wire `textpos` list of the first body paragraph, or `None` when its
/// cache was dropped; and the encode warnings' codes.
fn encode(first: Paragraph, with_header_and_page_number: bool) -> (Option<Vec<u32>>, Vec<String>) {
    let mut section = Section::with_paragraphs(vec![first], PageSettings::a4());
    if with_header_and_page_number {
        section.headers.push(HeaderFooter::all_pages(vec![Paragraph::with_runs(
            vec![Run::text("머리", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )]));
        section.page_number = Some(PageNumber::bottom_center());
    }
    let mut doc = Document::new();
    doc.add_section(section);
    let store = style_store_for_preset("default").unwrap();
    let outcome = HwpxEncoder::encode_with_diagnostics(
        &doc.validate().unwrap(),
        &store,
        &ImageStore::new(),
        EncodeOptions::default().with_emit_layout_cache(true),
    )
    .unwrap();
    let xml = {
        use std::io::Read as _;
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(outcome.bytes)).unwrap();
        let mut xml = String::new();
        zip.by_name("Contents/section0.xml").unwrap().read_to_string(&mut xml).unwrap();
        xml
    };
    // The first body paragraph's own cache comes after its runs: take the
    // last linesegarray before the end of the first top-level paragraph.
    let body_end = xml.rfind("</hp:p>").unwrap();
    let cache = xml[..body_end].rfind("<hp:linesegarray>").and_then(|at| {
        let end = at + xml[at..].find("</hp:linesegarray>").unwrap();
        let segment = &xml[at..end];
        (!xml[end..body_end].contains("<hp:p ")).then(|| {
            segment
                .match_indices("textpos=\"")
                .map(|(i, m)| {
                    let v = &segment[i + m.len()..];
                    v[..v.find('"').unwrap()].parse().unwrap()
                })
                .collect()
        })
    });
    (cache, outcome.warnings.iter().map(|w| w.code().to_string()).collect())
}

fn host(runs: Vec<Run>, lines: Vec<LineSeg>) -> Paragraph {
    let mut p = Paragraph::with_runs(runs, ParaShapeIndex::new(0));
    p.layout_cache = Some(LayoutCache::new(lines));
    p
}

/// Tokens of `[table, table, "끝"]` in a section's first paragraph:
/// secPr 0, colPr 8, table 16, table 24, text 32. Lines before the second
/// table and after both land there; a line without a boundary keeps its old
/// (earliest) place.
/// 이것을 실패시키는 것: 경계를 보지 않고 `to_wire` 만 쓰기(24·32 대신 0) ·
/// 선행 주입 토큰을 구역 정의 앞에 기록하기(종류 열이 어긋나 drop).
#[test]
fn boundary_places_the_line_before_its_token() {
    let lines = vec![
        line(0, None),
        line(0, at(&[SectionDef, ColumnDef, Table], Some(Table))),
        line(0, at(&[SectionDef, ColumnDef, Table, Table], None)),
    ];
    let runs = vec![table(), table(), Run::text("끝", CharShapeIndex::new(0))];
    let (cache, warnings) = encode(host(runs, lines), false);
    assert_eq!(cache, Some(vec![0, 24, 32]), "{warnings:?}");
}

/// With a header and a page number, the injected tokens follow the column
/// definition in the order they are written: header, then page number.
/// 이것을 실패시키는 것: 주입 순서를 바꾸기(쪽 번호를 머리말 앞에).
#[test]
fn injected_tokens_keep_their_written_order() {
    let lines = vec![
        line(0, None),
        line(0, at(&[SectionDef, ColumnDef, Header], Some(PageNumber))),
        line(0, at(&[SectionDef, ColumnDef, Header, PageNumber], Some(Table))),
    ];
    let (cache, warnings) = encode(host(vec![table()], lines), true);
    assert_eq!(cache, Some(vec![0, 24, 32]), "{warnings:?}");
}

/// A boundary whose kinds do not match, or that points inside a folded
/// field, drops the cache with a warning.
/// 이것을 실패시키는 것: 종류 대조 삭제 · 토큰 안쪽 검사 삭제.
#[test]
fn mismatched_or_inside_boundary_drops_the_cache() {
    let wrong = vec![line(0, None), line(0, at(&[SectionDef, ColumnDef], Some(Picture)))];
    let (cache, warnings) = encode(host(vec![table()], wrong), false);
    assert_eq!(cache, None);
    assert!(warnings.iter().any(|w| w == "LAYOUT_CACHE_DROPPED"), "{warnings:?}");
    // Right count and next kind, wrong kind skipped.
    let wrong_skip =
        vec![line(0, None), line(0, at(&[SectionDef, ColumnDef, Picture], Some(Table)))];
    let (cache, _) = encode(host(vec![table(), table()], wrong_skip), false);
    assert_eq!(cache, None);

    let field = Run::control(
        Control::Field {
            field_type: FieldType::ClickHere,
            hint_text: Some("값".into()),
            help_text: None,
            name: Some("f".into()),
            display_text: "값".into(),
        },
        CharShapeIndex::new(0),
    );
    let inside = vec![line(0, None), line(0, at(&[SectionDef, ColumnDef, FieldBegin], Some(Text)))];
    let (cache, warnings) =
        encode(host(vec![field, Run::text("뒤", CharShapeIndex::new(0))], inside), false);
    assert_eq!(cache, None);
    assert!(warnings.iter().any(|w| w == "LAYOUT_CACHE_DROPPED"), "{warnings:?}");
}

/// Q6: lines that still fall on one place (no boundary to tell them apart)
/// leave the paragraph without a cache, with a warning.
/// 이것을 실패시키는 것: 순증가 검사 삭제(`[0, 0]` 캐시가 나간다).
#[test]
fn collapsed_lines_are_dropped() {
    let collapsed = vec![line(0, None), line(0, None)];
    let (cache, warnings) = encode(host(vec![table(), table()], collapsed), false);
    assert_eq!(cache, None);
    assert!(warnings.iter().any(|w| w == "LAYOUT_CACHE_DROPPED"), "{warnings:?}");
}
