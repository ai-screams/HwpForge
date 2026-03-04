#![allow(clippy::vec_init_then_push)]
//! Wave 7 Style Infrastructure verification — generates an actual HWPX file.
//!
//! This example exercises all Wave 7 features and verifies roundtrip fidelity:
//!
//! 1. **StyleIndex on Paragraph** — `with_style()` for 바탕글/본문/개요 1-3
//! 2. **Distribute/DistributeFlush alignment** — new `Alignment` variants
//! 3. **Dynamic borderFills** — 3 defaults + 1 user-defined (colored borders)
//! 4. **Per-style charPr/paraPr** — 7+20 default shapes + user shapes
//! 5. **Roundtrip decode** — encode → decode → verify everything preserved
//!
//! # Usage
//! ```bash
//! cargo run -p hwpforge-smithy-hwpx --example wave7_style_test
//! ```

use hwpforge_core::document::Document;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_core::PageSettings;
use hwpforge_foundation::{Alignment, CharShapeIndex, Color, HwpUnit, ParaShapeIndex, StyleIndex};
use hwpforge_smithy_hwpx::style_store::{
    HwpxBorderFill, HwpxBorderLine, HwpxCharShape, HwpxFill, HwpxParaShape, HwpxStyleStore,
};
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder};

// ── CharShape indices ──────────────────────────────────────────
// `with_default_fonts()` only creates fonts (no default charShapes/paraShapes).
// Default shapes (7 charPr + 20 paraPr) are injected only by `from_registry_with()`.
// Here we use `with_default_fonts()` + manual push, so indices start at 0.
const CS_NORMAL: usize = 0; // 10pt black
const CS_BOLD: usize = 1; // 10pt bold
const CS_TITLE: usize = 2; // 16pt navy bold
const CS_HEADING: usize = 3; // 13pt bold

// ── ParaShape indices ─────────────────────────────────────────
const PS_LEFT: usize = 0; // Left
const PS_CENTER: usize = 1; // Center
const PS_JUSTIFY: usize = 2; // Justify
const PS_DISTRIBUTE: usize = 3; // Distribute (Wave 7 new!)
const PS_DISTFLUSH: usize = 4; // DistributeFlush (Wave 7 new!)

// ── Helpers ────────────────────────────────────────────────────

fn text_para(text: &str, cs: usize, ps: usize) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(cs))], ParaShapeIndex::new(ps))
}

fn styled_para(text: &str, cs: usize, ps: usize, style: usize) -> Paragraph {
    text_para(text, cs, ps).with_style(StyleIndex::new(style))
}

fn empty() -> Paragraph {
    text_para("", CS_NORMAL, PS_LEFT)
}

// ── Style Store Setup ──────────────────────────────────────────

fn build_style_store() -> HwpxStyleStore {
    let mut store = HwpxStyleStore::with_default_fonts("함초롬돋움");

    // User CharShape 0 (→ idx 7): Normal 10pt black
    store.push_char_shape(HwpxCharShape::default());

    // User CharShape 1 (→ idx 8): Bold 10pt
    let mut cs1 = HwpxCharShape::default();
    cs1.bold = true;
    store.push_char_shape(cs1);

    // User CharShape 2 (→ idx 9): Title 16pt navy bold
    let mut cs2 = HwpxCharShape::default();
    cs2.height = HwpUnit::from_pt(16.0).unwrap();
    cs2.bold = true;
    cs2.text_color = Color::from_rgb(0, 51, 102);
    store.push_char_shape(cs2);

    // User CharShape 3 (→ idx 10): Heading 13pt bold
    let mut cs3 = HwpxCharShape::default();
    cs3.height = HwpUnit::from_pt(13.0).unwrap();
    cs3.bold = true;
    store.push_char_shape(cs3);

    // User ParaShape 0 (→ idx 20): Left
    store.push_para_shape(HwpxParaShape::default());

    // User ParaShape 1 (→ idx 21): Center
    let mut ps1 = HwpxParaShape::default();
    ps1.alignment = Alignment::Center;
    store.push_para_shape(ps1);

    // User ParaShape 2 (→ idx 22): Justify
    let mut ps2 = HwpxParaShape::default();
    ps2.alignment = Alignment::Justify;
    store.push_para_shape(ps2);

    // User ParaShape 3 (→ idx 23): Distribute ← Wave 7 new!
    let mut ps3 = HwpxParaShape::default();
    ps3.alignment = Alignment::Distribute;
    store.push_para_shape(ps3);

    // User ParaShape 4 (→ idx 24): DistributeFlush ← Wave 7 new!
    let mut ps4 = HwpxParaShape::default();
    ps4.alignment = Alignment::DistributeFlush;
    store.push_para_shape(ps4);

    // Default border fills (id=1-3): always required for 한글 compatibility.
    // from_registry_with() injects these automatically; with_default_fonts() does not.
    store.push_border_fill(HwpxBorderFill::default_page_border()); // id=1
    store.push_border_fill(HwpxBorderFill::default_char_background()); // id=2
    store.push_border_fill(HwpxBorderFill::default_table_border()); // id=3

    // User BorderFill (id=4): blue double border with light blue fill ← Wave 7 new!
    // Use default_page_border() as base and mutate (non_exhaustive prevents struct expr)
    let mut user_bf = HwpxBorderFill::default_page_border();
    user_bf.id = 4;
    let blue_border = HwpxBorderLine {
        line_type: "DOUBLE".into(),
        width: "0.4 mm".into(),
        color: "#0000FF".into(),
    };
    user_bf.left = blue_border.clone();
    user_bf.right = blue_border.clone();
    user_bf.top = blue_border.clone();
    user_bf.bottom = blue_border;
    user_bf.fill = Some(HwpxFill::WinBrush {
        face_color: "#E8F0FE".into(),
        hatch_color: "#FF000000".into(),
        alpha: "0".into(),
    });
    store.push_border_fill(user_bf);

    store
}

// ── Document Builder ───────────────────────────────────────────

fn build_document() -> Document {
    let mut paras = Vec::new();

    // ── Title (styleIDRef=0 = 바탕글, default) ──
    paras.push(text_para("Wave 7 Style Infrastructure 검증 문서", CS_TITLE, PS_CENTER));
    paras.push(empty());

    // ── 개요 1 (styleIDRef=2) ──
    paras.push(styled_para(
        "1. StyleIndex 테스트",
        CS_HEADING,
        PS_LEFT,
        2, // 개요 1
    ));
    paras.push(empty());

    // ── 본문 (styleIDRef=1) ──
    paras.push(styled_para(
        "이 문단은 '본문' 스타일(styleIDRef=1)을 사용합니다. Wave 7에서 Paragraph.style_id 필드가 추가되어 각 문단에 named style을 지정할 수 있습니다.",
        CS_NORMAL,
        PS_JUSTIFY,
        1, // 본문
    ));
    paras.push(empty());

    // ── 개요 2 (styleIDRef=3) ──
    paras.push(styled_para(
        "1.1 하위 개요 테스트",
        CS_HEADING,
        PS_LEFT,
        3, // 개요 2
    ));

    // ── 바탕글 (no style_id = None → styleIDRef=0) ──
    paras.push(text_para(
        "이 문단은 style_id가 None이므로 바탕글(styleIDRef=0)입니다.",
        CS_NORMAL,
        PS_JUSTIFY,
    ));
    paras.push(empty());

    // ── 개요 3 (styleIDRef=4) ──
    paras.push(styled_para(
        "1.1.1 더 깊은 개요",
        CS_BOLD,
        PS_LEFT,
        4, // 개요 3
    ));
    paras.push(empty());

    // ── Section 2: Distribute Alignment Test ──
    paras.push(styled_para(
        "2. Distribute/DistributeFlush 정렬 테스트",
        CS_HEADING,
        PS_LEFT,
        2, // 개요 1
    ));
    paras.push(empty());

    // Distribute alignment
    paras.push(text_para(
        "균등 배분 (Distribute): 글자 사이에 균등하게 공간을 배분합니다.",
        CS_NORMAL,
        PS_DISTRIBUTE,
    ));

    // DistributeFlush alignment
    paras.push(text_para(
        "균등 배분 정렬 (DistributeFlush): 마지막 줄도 양쪽 정렬됩니다.",
        CS_NORMAL,
        PS_DISTFLUSH,
    ));

    // Other alignments for comparison
    paras.push(text_para("왼쪽 정렬 (Left)", CS_NORMAL, PS_LEFT));
    paras.push(text_para("가운데 정렬 (Center)", CS_NORMAL, PS_CENTER));
    paras.push(text_para(
        "양쪽 정렬 (Justify): 긴 문장이 양쪽 끝에 맞춰 정렬됩니다. 한글 문서에서 가장 많이 사용되는 정렬 방식입니다.",
        CS_NORMAL,
        PS_JUSTIFY,
    ));
    paras.push(empty());

    // ── Section 3: BorderFill Test via Table ──
    paras.push(styled_para(
        "3. BorderFill 동적 생성 테스트",
        CS_HEADING,
        PS_LEFT,
        2, // 개요 1
    ));
    paras.push(empty());

    paras.push(text_para(
        "아래 표는 기본 borderFill(id=3, SOLID 테두리)을 사용합니다. Wave 7에서 borderFill이 상수 XML에서 동적 serde 기반 생성으로 변경되었습니다.",
        CS_NORMAL,
        PS_JUSTIFY,
    ));
    paras.push(empty());

    // Table: Wave 7 features summary
    let w = HwpUnit::new(14000).unwrap();
    let table = Table::new(vec![
        TableRow::new(vec![
            TableCell::new(vec![text_para("기능", CS_BOLD, PS_CENTER)], w),
            TableCell::new(vec![text_para("상태", CS_BOLD, PS_CENTER)], w),
            TableCell::new(vec![text_para("설명", CS_BOLD, PS_CENTER)], w),
        ]),
        TableRow::new(vec![
            TableCell::new(vec![text_para("7.1 StyleIndex", CS_NORMAL, PS_LEFT)], w),
            TableCell::new(vec![text_para("구현 완료", CS_BOLD, PS_CENTER)], w),
            TableCell::new(
                vec![text_para("Paragraph.style_id: Option<StyleIndex>", CS_NORMAL, PS_LEFT)],
                w,
            ),
        ]),
        TableRow::new(vec![
            TableCell::new(vec![text_para("7.2 BorderFill", CS_NORMAL, PS_LEFT)], w),
            TableCell::new(vec![text_para("구현 완료", CS_BOLD, PS_CENTER)], w),
            TableCell::new(
                vec![text_para("동적 serde 기반 borderFill 생성", CS_NORMAL, PS_LEFT)],
                w,
            ),
        ]),
        TableRow::new(vec![
            TableCell::new(vec![text_para("7.3 Per-Style", CS_NORMAL, PS_LEFT)], w),
            TableCell::new(vec![text_para("구현 완료", CS_BOLD, PS_CENTER)], w),
            TableCell::new(
                vec![text_para("7 charPr + 20 paraPr 기본 스타일", CS_NORMAL, PS_LEFT)],
                w,
            ),
        ]),
        TableRow::new(vec![
            TableCell::new(vec![text_para("7.4 Alignment", CS_NORMAL, PS_LEFT)], w),
            TableCell::new(vec![text_para("구현 완료", CS_BOLD, PS_CENTER)], w),
            TableCell::new(
                vec![text_para("Distribute + DistributeFlush 추가", CS_NORMAL, PS_LEFT)],
                w,
            ),
        ]),
    ]);

    paras.push(Paragraph::with_runs(
        vec![Run::table(table, CharShapeIndex::new(CS_NORMAL))],
        ParaShapeIndex::new(PS_CENTER),
    ));
    paras.push(empty());

    // ── Section 4: Per-style formatting summary ──
    paras.push(styled_para(
        "4. Per-Style charPr/paraPr 매핑",
        CS_HEADING,
        PS_LEFT,
        2, // 개요 1
    ));
    paras.push(empty());
    paras.push(text_para(
        "from_registry_with()는 7개 기본 charPr과 20개 기본 paraPr을 주입한 후, 사용자 정의 스타일을 오프셋 적용하여 추가합니다. 각 style의 charPrIDRef/paraPrIDRef가 golden fixture와 일치합니다.",
        CS_NORMAL,
        PS_JUSTIFY,
    ));
    paras.push(empty());
    paras.push(text_para("— Wave 7 Style Infrastructure 검증 완료 —", CS_BOLD, PS_CENTER));

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(paras, PageSettings::a4()));
    doc
}

// ── Main ───────────────────────────────────────────────────────

fn main() {
    println!("=== Wave 7 Style Infrastructure Test ===\n");

    // 1. Build style store
    let store = build_style_store();
    println!(
        "[1] Style store: {} fonts, {} charShapes, {} paraShapes, {} borderFills",
        store.font_count(),
        store.char_shape_count(),
        store.para_shape_count(),
        store.border_fill_count(),
    );

    // 2. Build document
    let doc = build_document();
    // Clone paragraphs for post-roundtrip comparison (validate() consumes doc)
    let orig_paragraphs = doc.sections()[0].paragraphs.clone();
    let para_count = orig_paragraphs.len();
    println!("[2] Document: 1 section, {para_count} paragraphs");

    // Count styled paragraphs
    let styled_count = orig_paragraphs.iter().filter(|p| p.style_id.is_some()).count();
    println!("    Paragraphs with style_id: {styled_count}");
    for (i, p) in orig_paragraphs.iter().enumerate() {
        if let Some(sid) = p.style_id {
            let text = p.text_content();
            let preview: String = text.chars().take(20).collect();
            println!("      p[{i}] styleIDRef={} \"{preview}...\"", sid.get());
        }
    }

    // 3. Validate
    let validated = doc.validate().expect("validation failed");
    println!("[3] Validation: OK");

    // 4. Encode
    let image_store = hwpforge_core::image::ImageStore::new();
    let bytes = HwpxEncoder::encode(&validated, &store, &image_store).expect("encode failed");

    std::fs::create_dir_all("temp").ok();
    let path = "temp/wave7_style_test.hwpx";
    std::fs::write(path, &bytes).expect("write failed");
    println!("[4] Encoded: {path} ({} bytes)", bytes.len());

    // 5. Roundtrip decode
    let result = HwpxDecoder::decode(&bytes).expect("decode failed");
    let d = &result.document;
    println!("[5] Roundtrip decode: OK ({} sections)", d.sections().len());

    let decoded_sec = &d.sections()[0];
    println!("    Decoded paragraphs: {}", decoded_sec.paragraphs.len());

    // 5a. Verify style_id roundtrip
    let mut style_mismatches = 0;
    for (i, (orig, decoded)) in
        orig_paragraphs.iter().zip(decoded_sec.paragraphs.iter()).enumerate()
    {
        if orig.style_id != decoded.style_id {
            println!(
                "    MISMATCH p[{i}]: orig={:?} decoded={:?}",
                orig.style_id, decoded.style_id
            );
            style_mismatches += 1;
        }
    }
    if style_mismatches == 0 {
        println!("    style_id roundtrip: ALL MATCH ({styled_count} styled paragraphs)");
    } else {
        println!("    style_id roundtrip: {style_mismatches} MISMATCHES!");
    }

    // 5b. Verify alignment roundtrip via decoded style store paraShapes
    let ds = &result.style_store;
    println!(
        "[6] Decoded style store: {} charShapes, {} paraShapes, {} borderFills, {} styles",
        ds.char_shape_count(),
        ds.para_shape_count(),
        ds.border_fill_count(),
        ds.style_count(),
    );

    // Check the user paraShapes have correct alignment
    let alignments_to_check = [
        (PS_LEFT, "LEFT", Alignment::Left),
        (PS_CENTER, "CENTER", Alignment::Center),
        (PS_JUSTIFY, "JUSTIFY", Alignment::Justify),
        (PS_DISTRIBUTE, "DISTRIBUTE", Alignment::Distribute),
        (PS_DISTFLUSH, "DISTRIBUTE_FLUSH", Alignment::DistributeFlush),
    ];
    let mut alignment_ok = true;
    for (idx, label, expected) in &alignments_to_check {
        match ds.para_shape(ParaShapeIndex::new(*idx)) {
            Ok(ps) if ps.alignment == *expected => {
                println!("    paraShape[{idx}] alignment={label}: OK");
            }
            Ok(ps) => {
                println!(
                    "    paraShape[{idx}] alignment={label}: MISMATCH (got {:?})",
                    ps.alignment
                );
                alignment_ok = false;
            }
            Err(e) => {
                println!("    paraShape[{idx}]: ERROR {e}");
                alignment_ok = false;
            }
        }
    }

    // 5c. Verify border fills
    let bf_count = ds.border_fill_count();
    println!("[7] Border fills: {bf_count} total");
    let mut bf_ok = true;
    // Check defaults (1-3) + user (4)
    for id in 1..=4u32 {
        match ds.border_fill(id) {
            Ok(bf) => {
                let desc = match id {
                    1 => "page border (NONE)",
                    2 => "char background (winBrush)",
                    3 => "table border (SOLID)",
                    4 => "user DOUBLE blue",
                    _ => "unknown",
                };
                println!("    borderFill id={}: {} — {desc}", bf.id, bf.left.line_type);
            }
            Err(e) => {
                println!("    borderFill id={id}: ERROR {e}");
                bf_ok = false;
            }
        }
    }

    // 5d. Verify charPr/paraPr counts survive roundtrip
    println!(
        "[8] Shape counts: charShapes={} (orig {}), paraShapes={} (orig {})",
        ds.char_shape_count(),
        store.char_shape_count(),
        ds.para_shape_count(),
        store.para_shape_count(),
    );

    // Decoded store should have at least as many shapes as we encoded
    let cs_ok = ds.char_shape_count() >= store.char_shape_count();
    let ps_ok = ds.para_shape_count() >= store.para_shape_count();

    // Final summary
    println!("\n=== RESULTS ===");
    println!(
        "  StyleIndex roundtrip:       {}",
        if style_mismatches == 0 { "PASS" } else { "FAIL" }
    );
    println!("  Alignment roundtrip:        {}", if alignment_ok { "PASS" } else { "FAIL" });
    println!("  BorderFill dynamic:         {}", if bf_ok { "PASS" } else { "FAIL" });
    println!("  CharShape counts:           {}", if cs_ok { "PASS" } else { "FAIL" });
    println!("  ParaShape counts:           {}", if ps_ok { "PASS" } else { "FAIL" });

    let all_pass = style_mismatches == 0 && alignment_ok && bf_ok && cs_ok && ps_ok;
    if all_pass {
        println!("\n  ALL TESTS PASSED!");
    } else {
        println!("\n  SOME TESTS FAILED!");
        std::process::exit(1);
    }

    println!("\n한글에서 열어서 확인하세요: {path}");
}
