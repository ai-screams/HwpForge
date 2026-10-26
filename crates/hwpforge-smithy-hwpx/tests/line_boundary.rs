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

/// A field whose text holds a tab is one folded body on both sides: a line
/// after the field keeps its boundary through encode → decode.
/// 이것을 실패시키는 것: 디코더가 필드 본문 조각(글자·탭·글자)을 한 토큰으로
/// 합치지 않기(경계의 `skip` 이 `Text` 셋이 된다).
#[test]
fn folded_field_body_is_one_token_on_both_sides() {
    use hwpforge_smithy_hwpx::HwpxDecoder;
    let field = Run::control(
        Control::Field {
            field_type: FieldType::ClickHere,
            hint_text: Some("a\tb".into()),
            help_text: None,
            name: Some("f".into()),
            display_text: "a\tb".into(),
        },
        CharShapeIndex::new(0),
    );
    let after = at(&[SectionDef, ColumnDef, FieldBegin, Text, FieldEnd], Some(Table));
    let lines = vec![line(0, None), line(0, after.clone())];
    let mut section =
        Section::with_paragraphs(vec![host(vec![field, table()], lines)], PageSettings::a4());
    section.page_number = None;
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
    let decoded = HwpxDecoder::decode(&outcome.bytes).unwrap();
    let cache = decoded.document.sections()[0].paragraphs[0].layout_cache.clone().expect("cache");
    assert_eq!(cache.lines[1].boundary, after);
}

/// 일반 run 본문은 범위 안에서도 줄을 배치할 수 있다. 접힌 본문은
/// 여전히 거부한다. 이것을 실패시키는 것: 모든 필드를 허용하거나 거부하기.
#[test]
fn only_folded_field_interiors_drop_the_cache() {
    use hwpforge_foundation::BookmarkType;
    use hwpforge_smithy_hwpx::HwpxDecoder;
    let mark = |bookmark_type| {
        Run::control(Control::Bookmark { name: "bm".into(), bookmark_type }, CharShapeIndex::new(0))
    };
    let runs = vec![
        mark(BookmarkType::SpanStart),
        Run::text("가나다", CharShapeIndex::new(0)),
        mark(BookmarkType::SpanEnd),
        Run::text("끝", CharShapeIndex::new(0)),
    ];
    let (cache, warnings) = encode(host(runs, vec![line(0, None), line(1, None)]), false);
    assert_eq!(cache, Some(vec![0, 25]), "{warnings:?}");
    assert!(warnings.is_empty(), "{warnings:?}");

    // Decode: secPr 0, colPr 8, fieldBegin 16, "a" 24, tab 25, "b" 33,
    // fieldEnd 34 — a line at 25 (between the field's pieces).
    let field = Run::control(
        Control::Field {
            field_type: FieldType::ClickHere,
            hint_text: Some("a\tb".into()),
            help_text: None,
            name: Some("f".into()),
            display_text: "a\tb".into(),
        },
        CharShapeIndex::new(0),
    );
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![host(vec![field, Run::text("끝", CharShapeIndex::new(0))], vec![line(0, None)])],
        PageSettings::a4(),
    ));
    let store = style_store_for_preset("default").unwrap();
    let bytes = HwpxEncoder::encode_with_options(
        &doc.validate().unwrap(),
        &store,
        &ImageStore::new(),
        EncodeOptions::default().with_emit_layout_cache(true),
    )
    .unwrap();
    let decoded = HwpxDecoder::decode(&bytes).unwrap();
    assert!(decoded.document.sections()[0].paragraphs[0].layout_cache.is_some(), "premise");
    let edited = {
        use std::io::{Read as _, Write as _};
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes[..])).unwrap();
        let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).unwrap();
            let name = entry.name().expect("ZIP entry name").to_string();
            let mut data = Vec::new();
            entry.read_to_end(&mut data).unwrap();
            if name == "Contents/section0.xml" {
                let xml = String::from_utf8(data).unwrap();
                let at = xml.find("</hp:linesegarray>").unwrap();
                let extra = r#"<hp:lineseg textpos="25" vertpos="1000" vertsize="1000" textheight="1000" baseline="850" spacing="600" horzpos="0" horzsize="40000" flags="0"/>"#;
                data = format!("{}{extra}{}", &xml[..at], &xml[at..]).into_bytes();
            }
            out.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            out.write_all(&data).unwrap();
        }
        out.finish().unwrap().into_inner()
    };
    let decoded = HwpxDecoder::decode(&edited).unwrap();
    assert!(decoded.document.sections()[0].paragraphs[0].layout_cache.is_none());
    assert!(
        decoded.warnings.iter().any(|w| w.code() == "LAYOUT_CACHE_DROPPED"),
        "{:?}",
        decoded.warnings
    );
}

/// S22 r1: a bookmark span that crosses paragraphs writes its begin in the
/// first paragraph and its text one to one after it, so that paragraph keeps
/// its line cache; the end paragraph now retains the shared host span ID
/// and keeps its cache too. 이것을 실패시키는 것: 문단마다 span 맵 초기화.
/// A span closed in its own paragraph keeps both caches.
/// 이것을 실패시키는 것: 인코더에서 "끝 없는 필드 시작이 있으면 캐시 drop" 을
/// 넣는 것(시작 문단의 맞는 캐시가 사라진다).
#[test]
fn a_bookmark_span_across_paragraphs_keeps_its_first_cache() {
    use hwpforge_foundation::BookmarkType;
    let mark = |bookmark_type| {
        Run::control(Control::Bookmark { name: "bm".into(), bookmark_type }, CharShapeIndex::new(0))
    };
    let text = |t: &str| Run::text(t, CharShapeIndex::new(0));
    let caches = |paragraphs: Vec<Paragraph>| {
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(paragraphs, PageSettings::a4()));
        let store = style_store_for_preset("default").unwrap();
        let outcome = HwpxEncoder::encode_with_diagnostics(
            &doc.validate().unwrap(),
            &store,
            &ImageStore::new(),
            EncodeOptions::default().with_emit_layout_cache(true),
        )
        .unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(outcome.bytes)).unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("Contents/section0.xml").unwrap(), &mut xml)
            .unwrap();
        let drops = outcome.warnings.iter().filter(|w| w.code() == "LAYOUT_CACHE_DROPPED").count();
        (xml.matches("<hp:linesegarray>").count(), drops)
    };

    let across = vec![
        host(vec![mark(BookmarkType::SpanStart), text("가나")], vec![line(0, None), line(1, None)]),
        host(vec![text("다"), mark(BookmarkType::SpanEnd)], vec![line(0, None)]),
    ];
    assert_eq!(caches(across), (2, 0));

    let within = vec![
        host(
            vec![mark(BookmarkType::SpanStart), text("가나"), mark(BookmarkType::SpanEnd)],
            vec![line(0, None)],
        ),
        host(vec![text("다")], vec![line(0, None)]),
    ];
    assert_eq!(caches(within), (2, 0));
}

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
