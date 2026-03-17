//! TextDirection, DropCapStyle, page_break, char border 테스트
//!
//! Usage:
//!   cargo run -p hwpforge-smithy-hwpx --example text_direction_and_dropcap
//!
//! Output:
//!   temp/text_direction_and_dropcap.hwpx
//!
//! Open in 한글 to verify:
//! - Section 1: 가로쓰기 (horizontal, default) with page_break between paragraphs
//! - Section 2: 세로쓰기 (vertical text direction)
//! - Section 3: DropCapStyle on a TextBox shape
//! - Section 4: char_border_fill_id (borderFillIDRef = 3)

use hwpforge_core::control::{Control, ShapeStyle};
use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::PageSettings;
use hwpforge_foundation::{CharShapeIndex, DropCapStyle, HwpUnit, ParaShapeIndex, TextDirection};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::HwpxEncoder;

// ── Style indices ──────────────────────────────────────────────
const CS_NORMAL: CharShapeIndex = CharShapeIndex::new(0);
const CS_BORDER: CharShapeIndex = CharShapeIndex::new(1);
const PS_BODY: ParaShapeIndex = ParaShapeIndex::new(0);

// ── Helpers ────────────────────────────────────────────────────

fn p(text: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CS_NORMAL)], PS_BODY)
}

fn p_border(text: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CS_BORDER)], PS_BODY)
}

fn build_store() -> HwpxStyleStore {
    let mut store = HwpxStyleStore::with_default_fonts("함초롬돋움");

    // CS 0: normal (10pt, black)
    store.push_char_shape(HwpxCharShape::default());

    // CS 1: char border (10pt, black, borderFillIDRef = 3)
    let mut cs_border = HwpxCharShape::default();
    cs_border.border_fill_id = Some(3);
    store.push_char_shape(cs_border);

    // PS 0: body (justified)
    store.push_para_shape(HwpxParaShape::default());

    store
}

fn main() {
    std::fs::create_dir_all("temp").unwrap();

    let ps = PageSettings::a4();

    // ── Section 1: Horizontal + page_break ─────────────────────
    let sec1 = {
        let paras = vec![
            p("【섹션 1: 가로쓰기 + 페이지 나누기 테스트】"),
            p(""),
            p("이 문단 다음에 페이지 나누기가 삽입됩니다. \
               아래 문단은 새 페이지에서 시작해야 합니다."),
            // page_break: next paragraph starts on a new page
            p("이 문단 뒤에서 페이지가 나뉩니다.").with_page_break(),
            p("이 문단은 새 페이지에서 시작됩니다. \
               페이지 나누기가 정상 작동하면 이전 문단과 다른 페이지에 있어야 합니다."),
            p(""),
            p("페이지 나누기 테스트 완료."),
        ];
        Section::with_paragraphs(paras, ps)
    };

    // ── Section 2: Vertical text direction ─────────────────────
    let sec2 = {
        let paras = vec![
            p("【섹션 2: 세로쓰기 테스트】"),
            p(""),
            p("이 섹션은 세로쓰기(TextDirection::Vertical)로 설정되어 있습니다."),
            p("한글에서 세로쓰기가 적용되면 텍스트가 위에서 아래로, 오른쪽에서 왼쪽으로 흐릅니다."),
            p("가나다라마바사아자차카타파하"),
            p("ABCDEFGHIJKLMNOP"),
            p("1234567890"),
        ];
        Section::with_paragraphs(paras, ps).with_text_direction(TextDirection::Vertical)
    };

    // ── Section 3: DropCapStyle on TextBox ─────────────────────
    let sec3 = {
        // TextBox with DropCapStyle::DoubleLine
        let textbox = Control::TextBox {
            paragraphs: vec![p("바탕글 테스트 문단입니다. \
                 이 글상자는 dropcapstyle=\"DoubleLine\"으로 설정되어 \
                 2줄 바탕글 효과가 적용됩니다.")],
            width: HwpUnit::from_mm(120.0).unwrap(),
            height: HwpUnit::from_mm(40.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: Some(ShapeStyle {
                drop_cap_style: DropCapStyle::DoubleLine,
                ..Default::default()
            }),
        };

        // TextBox with DropCapStyle::Margin
        let textbox2 = Control::TextBox {
            paragraphs: vec![p("여백 바탕글 테스트입니다. \
                 이 글상자는 dropcapstyle=\"Margin\"으로 설정되어 \
                 여백 바탕글 효과가 적용됩니다.")],
            width: HwpUnit::from_mm(120.0).unwrap(),
            height: HwpUnit::from_mm(40.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: Some(ShapeStyle { drop_cap_style: DropCapStyle::Margin, ..Default::default() }),
        };

        let paras = vec![
            p("【섹션 3: DropCapStyle 테스트 (TextBox)】"),
            p(""),
            p("아래 글상자에 DropCapStyle::DoubleLine이 설정되어 있습니다."),
            Paragraph::with_runs(vec![Run::control(textbox, CS_NORMAL)], PS_BODY),
            p(""),
            Paragraph::with_runs(vec![Run::control(textbox2, CS_NORMAL)], PS_BODY),
        ];
        Section::with_paragraphs(paras, ps)
    };

    // ── Section 4: Character border/shading ────────────────────
    let sec4 = {
        let paras = vec![
            p("【섹션 4: 글자 테두리/음영 테스트】"),
            p(""),
            p("아래 문단의 텍스트에는 borderFillIDRef=3이 적용되어 있습니다."),
            p_border(
                "이 텍스트에 글자 테두리/음영이 적용되어야 합니다. \
                 charPr의 borderFillIDRef가 3으로 설정되어 기본값(2)과 다릅니다.",
            ),
            p(""),
            p("위 문단과 이 문단을 비교해 보세요. 이 문단은 기본 스타일입니다."),
        ];
        Section::with_paragraphs(paras, ps)
    };

    // ── Build document ─────────────────────────────────────────
    let mut doc = Document::new();
    doc.add_section(sec1);
    doc.add_section(sec2);
    doc.add_section(sec3);
    doc.add_section(sec4);
    let doc = doc.validate().expect("validation failed");

    let store = build_store();
    let images = ImageStore::new();

    let bytes = HwpxEncoder::encode(&doc, &store, &images).expect("encode failed");

    std::fs::write("temp/text_direction_and_dropcap.hwpx", &bytes).unwrap();
    println!("Written: temp/text_direction_and_dropcap.hwpx ({} bytes)", bytes.len());
    println!();
    println!("Open in 한글 and verify:");
    println!("  Section 1: 가로쓰기 + page_break (문단 사이 페이지 나뉨)");
    println!("  Section 2: 세로쓰기 (텍스트 세로 방향)");
    println!("  Section 3: DropCapStyle (DoubleLine + Margin 글상자)");
    println!("  Section 4: 글자 테두리/음영 (borderFillIDRef=3)");
}
