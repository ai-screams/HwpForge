//! Whitespace is text at every mixed-content boundary, including nested paragraphs.
#![cfg(feature = "hwpx")]

use std::io::{Cursor, Read};

use hwpforge::core::control::Control;
use hwpforge::core::image::ImageStore;
use hwpforge::core::inline::{InlineSegment, InlineTabAttr, InlineText};
use hwpforge::core::layout::{LayoutCache, LineBoundary, LineSeg, WireTokenKind};
use hwpforge::core::run::{Run, RunContent};
use hwpforge::core::table::{Table, TableCell, TableRow};
use hwpforge::core::{Document, PageSettings, Paragraph, Section};
use hwpforge::foundation::{BookmarkType, CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge::hwpx::{style_store_for_preset, EncodeOptions, HwpxDecoder, HwpxEncoder};

const TEXT: &str = "  \u{1f}  \t   \n  끝\u{1f} ";
const WIRE: &str = "<hp:t>  <hp:fwSpace/>  <hp:tab/>   <hp:lineBreak/>  끝<hp:fwSpace/> </hp:t>";

fn para(text: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}

fn package(cached: bool, rich_tab: bool) -> Vec<u8> {
    let mut target = para(TEXT);
    if rich_tab {
        target.runs[0].content = RunContent::InlineText(InlineText::from_segments([
            InlineSegment::Plain("  \u{1f}  ".into()),
            InlineSegment::Tab(InlineTabAttr {
                width: HwpUnit::new(1200).unwrap(),
                leader: 1,
                tab_type: 2,
            }),
            InlineSegment::Plain("   \n  끝\u{1f} ".into()),
        ]));
    }
    target.runs.push(Run::control(
        Control::Bookmark { name: "경계".into(), bookmark_type: BookmarkType::Point },
        CharShapeIndex::new(0),
    ));
    target.runs.push(Run::text("뒤", CharShapeIndex::new(0)));
    if cached {
        let line = |textpos, boundary| {
            let mut l = LineSeg::default();
            l.textpos = textpos;
            l.vertsize = 1000;
            l.textheight = 1000;
            l.horzsize = 40000;
            l.boundary = boundary;
            l
        };
        target.layout_cache = Some(LayoutCache::new(vec![
            line(0, None),
            line(3, None),
            line(
                TEXT.encode_utf16().count() as u32,
                Some(LineBoundary::new(vec![WireTokenKind::Bookmark], None)),
            ),
        ]));
    }
    let width = HwpUnit::new(8000).unwrap();
    let table = Table::new(vec![TableRow::new(vec![TableCell::new(vec![target.clone()], width)])]);
    let mut table_host = para("");
    table_host.runs.push(Run::table(table, CharShapeIndex::new(0)));
    let mut box_host = para("");
    box_host.runs.push(Run::control(
        Control::text_box(vec![target.clone()], width, width),
        CharShapeIndex::new(0),
    ));
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![para("도입"), target, table_host, box_host],
        PageSettings::a4(),
    ));
    let encoded = HwpxEncoder::encode_with_diagnostics(
        &doc.validate().unwrap(),
        &style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
        EncodeOptions::default().with_emit_layout_cache(cached),
    )
    .unwrap();
    assert!(encoded.warnings.is_empty(), "{:?}", encoded.warnings);
    encoded.bytes
}

fn section(bytes: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("Contents/section0.xml").unwrap().read_to_string(&mut xml).unwrap();
    xml
}

// 이것을 실패시키는 것: mixed 공백 보호 제거(캐시 없는 본문·칸·글상자에서도 실패),
// 또는 캐시 없는 섹션·깊은 문단의 보호 생략, InlineText 경로의 표식 제거 생략
// (속성 있는 탭), 공백을 잘라 인코드하기.
#[test]
fn mixed_spaces_round_trip_in_body_cell_and_text_box_with_or_without_cache() {
    for cached in [false, true] {
        for rich_tab in [false, true] {
            let bytes = package(cached, rich_tab);
            let wire = if rich_tab {
                WIRE.replace("<hp:tab/>", r#"<hp:tab width="1200" leader="1" type="2"/>"#)
            } else {
                WIRE.into()
            };
            let original_xml = section(&bytes);
            assert_eq!(original_xml.matches(&wire).count(), 3);
            let decoded = HwpxDecoder::decode(&bytes).unwrap();
            assert!(decoded.warnings.is_empty(), "{:?}", decoded.warnings);
            let mut targets = Vec::new();
            decoded.document.for_each_paragraph(|p| {
                if p.text_content().contains('끝') {
                    targets.push(p.clone());
                }
            });
            assert_eq!(targets.len(), 3);
            for p in &targets {
                assert_eq!(p.text_content(), format!("{TEXT}뒤"));
                assert_eq!(p.layout_cache.is_some(), cached);
                if let Some(cache) = &p.layout_cache {
                    assert_eq!(
                        cache.lines.iter().map(|l| l.textpos).collect::<Vec<_>>(),
                        [0, 3, TEXT.encode_utf16().count() as u32]
                    );
                    assert_eq!(
                        cache.lines[2].boundary,
                        Some(LineBoundary::new(vec![WireTokenKind::Bookmark], None))
                    );
                }
            }
            let encoded = HwpxEncoder::encode_with_diagnostics(
                &decoded.document.clone().validate().unwrap(),
                &decoded.style_store,
                &decoded.image_store,
                EncodeOptions::default().with_emit_layout_cache(cached),
            )
            .unwrap();
            assert!(encoded.warnings.is_empty(), "{:?}", encoded.warnings);
            assert_eq!(
                section(&encoded.bytes).matches(&wire).count(),
                3,
                "text bytes and positions"
            );
            let again = HwpxDecoder::decode(&encoded.bytes).unwrap();
            assert_eq!(again.document, decoded.document);
        }
    }
}

// 이것을 실패시키는 것: mixed 공백 보호 제거 — styled 내보내기에서도 동일한 공백이 사라진다.
#[cfg(feature = "ops-md")]
#[test]
fn to_md_keeps_spaces_around_inline_elements() {
    use hwpforge::ops::markdown::{to_md, MdExportOptions, MdMode};
    for cached in [false, true] {
        let bytes = package(cached, true);
        let md = to_md(&bytes, &MdExportOptions::default().with_mode(MdMode::Styled)).unwrap();
        assert!(md.markdown.contains("  \u{1f}  "), "{}", md.markdown);
        assert!(md.markdown.contains("끝\u{1f} "), "{}", md.markdown);
    }
}
