//! Generate a compact HWPX sample that exercises multiple tab semantics.
//!
//! Output:
//!   temp/tab-visual-check/tab-edge-cases.hwpx

use std::fs;

use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::tab::{TabDef, TabStop};
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_core::PageSettings;
use hwpforge_foundation::{
    Alignment, CharShapeIndex, HwpUnit, ParaShapeIndex, TabAlign, TabLeader,
};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::HwpxEncoder;

const FONT: &str = "함초롬돋움";
const OUT_DIR: &str = "temp/tab-visual-check";
const OUT_FILE: &str = "temp/tab-visual-check/tab-edge-cases.hwpx";

const CS_BODY: CharShapeIndex = CharShapeIndex::new(0);
const CS_HEADING: CharShapeIndex = CharShapeIndex::new(1);

const PS_BODY: ParaShapeIndex = ParaShapeIndex::new(0);
const PS_TITLE: ParaShapeIndex = ParaShapeIndex::new(1);
const PS_LEFT_DOT: ParaShapeIndex = ParaShapeIndex::new(2);
const PS_CENTER: ParaShapeIndex = ParaShapeIndex::new(3);
const PS_DECIMAL: ParaShapeIndex = ParaShapeIndex::new(4);
const PS_MULTI_STOP: ParaShapeIndex = ParaShapeIndex::new(5);
const PS_BUILTIN_RIGHT: ParaShapeIndex = ParaShapeIndex::new(6);
const PS_CONSECUTIVE: ParaShapeIndex = ParaShapeIndex::new(7);
const PS_LEADING: ParaShapeIndex = ParaShapeIndex::new(8);

fn text_para(text: &str, cs: CharShapeIndex, ps: ParaShapeIndex) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, cs)], ps)
}

fn heading(text: &str) -> Paragraph {
    text_para(text, CS_HEADING, PS_BODY)
}

fn build_style_store() -> HwpxStyleStore {
    let mut store = HwpxStyleStore::with_default_fonts(FONT);

    let body = HwpxCharShape::default();
    store.push_char_shape(body);

    let mut title = HwpxCharShape::default();
    title.height = HwpUnit::from_pt(13.0).unwrap();
    title.bold = true;
    store.push_char_shape(title);

    store.push_tab(TabDef {
        id: 3,
        auto_tab_left: false,
        auto_tab_right: false,
        stops: vec![TabStop {
            position: HwpUnit::from_mm(70.0).unwrap(),
            align: TabAlign::Left,
            leader: TabLeader::dot(),
        }],
    });

    store.push_tab(TabDef {
        id: 4,
        auto_tab_left: false,
        auto_tab_right: false,
        stops: vec![TabStop {
            position: HwpUnit::from_mm(95.0).unwrap(),
            align: TabAlign::Center,
            leader: TabLeader::none(),
        }],
    });

    store.push_tab(TabDef {
        id: 5,
        auto_tab_left: false,
        auto_tab_right: false,
        stops: vec![TabStop {
            position: HwpUnit::from_mm(120.0).unwrap(),
            align: TabAlign::Decimal,
            leader: TabLeader::none(),
        }],
    });

    store.push_tab(TabDef {
        id: 6,
        auto_tab_left: false,
        auto_tab_right: false,
        stops: vec![
            TabStop {
                position: HwpUnit::from_mm(35.0).unwrap(),
                align: TabAlign::Left,
                leader: TabLeader::none(),
            },
            TabStop {
                position: HwpUnit::from_mm(95.0).unwrap(),
                align: TabAlign::Center,
                leader: TabLeader::none(),
            },
            TabStop {
                position: HwpUnit::from_mm(165.0).unwrap(),
                align: TabAlign::Right,
                leader: TabLeader::none(),
            },
        ],
    });

    store.push_tab(TabDef {
        id: 7,
        auto_tab_left: false,
        auto_tab_right: false,
        stops: vec![
            TabStop {
                position: HwpUnit::from_mm(40.0).unwrap(),
                align: TabAlign::Left,
                leader: TabLeader::none(),
            },
            TabStop {
                position: HwpUnit::from_mm(90.0).unwrap(),
                align: TabAlign::Left,
                leader: TabLeader::none(),
            },
        ],
    });

    let body_ps = HwpxParaShape::default();
    store.push_para_shape(body_ps);

    let mut title_ps = HwpxParaShape::default();
    title_ps.alignment = Alignment::Center;
    title_ps.spacing_after = HwpUnit::from_mm(3.0).unwrap();
    store.push_para_shape(title_ps);

    let mut left_dot = HwpxParaShape::default();
    left_dot.tab_pr_id_ref = 3;
    store.push_para_shape(left_dot);

    let mut centered = HwpxParaShape::default();
    centered.tab_pr_id_ref = 4;
    store.push_para_shape(centered);

    let mut decimal = HwpxParaShape::default();
    decimal.tab_pr_id_ref = 5;
    store.push_para_shape(decimal);

    let mut multi_stop = HwpxParaShape::default();
    multi_stop.tab_pr_id_ref = 6;
    store.push_para_shape(multi_stop);

    let mut builtin_right = HwpxParaShape::default();
    builtin_right.tab_pr_id_ref = 2;
    store.push_para_shape(builtin_right);

    let mut consecutive = HwpxParaShape::default();
    consecutive.tab_pr_id_ref = 7;
    store.push_para_shape(consecutive);

    let mut leading = HwpxParaShape::default();
    leading.tab_pr_id_ref = 3;
    store.push_para_shape(leading);

    store
}

fn build_table() -> Table {
    let width = HwpUnit::from_mm(160.0).unwrap();
    let row1 = TableRow::new(vec![TableCell::new(
        vec![
            heading("표 셀 내부 탭"),
            text_para("CELLLEFT\tCELLRIGHT", CS_BODY, PS_LEFT_DOT),
            text_para("이름\t1234.50", CS_BODY, PS_DECIMAL),
        ],
        width,
    )]);

    let row2 = TableRow::new(vec![TableCell::new(
        vec![heading("연속 탭 / 빈 칸"), text_para("첫 칸\t\t세 번째 칸", CS_BODY, PS_CONSECUTIVE)],
        width,
    )]);

    Table::new(vec![row1, row2]).with_width(width)
}

fn build_document() -> Document {
    let paragraphs = vec![
        text_para("탭 기능 시각 검증 샘플", CS_HEADING, PS_TITLE),
        text_para("이 문서는 왼쪽/가운데/오른쪽/소수점/다중 정지점/표 셀 내부/연속 탭을 한 번에 확인하기 위한 샘플이다.", CS_BODY, PS_BODY),
        heading("1. 점선 리더 + 왼쪽 정지점"),
        text_para("과제명\t쇠부리의 왕립 민원 자동화", CS_BODY, PS_LEFT_DOT),
        heading("2. 가운데 정렬 정지점"),
        text_para("좌측 텍스트\t가운데 배치\t뒤쪽 꼬리", CS_BODY, PS_CENTER),
        heading("3. 소수점 정렬 정지점"),
        text_para("마력 효율\t1234.56", CS_BODY, PS_DECIMAL),
        text_para("보조 지표\t98.7", CS_BODY, PS_DECIMAL),
        heading("4. 다중 정지점 (좌/중/우)"),
        text_para("동부 길드\t중앙 집계\t최종 승인", CS_BODY, PS_MULTI_STOP),
        heading("5. 기본 right auto-tab"),
        text_para("좌측 라벨\t오른쪽 끝 값", CS_BODY, PS_BUILTIN_RIGHT),
        heading("6. 연속 탭과 빈 칸"),
        text_para("첫 칸\t\t세 번째 칸", CS_BODY, PS_CONSECUTIVE),
        heading("7. 선행 탭"),
        text_para("\t들여쓴 시작 텍스트", CS_BODY, PS_LEADING),
        heading("8. 표 셀 내부 탭"),
        Paragraph::with_runs(vec![Run::table(build_table(), CS_BODY)], PS_BODY),
    ];

    let section = Section::with_paragraphs(paragraphs, PageSettings::a4());
    let mut doc = Document::new();
    doc.add_section(section);
    doc
}

fn main() {
    fs::create_dir_all(OUT_DIR).expect("failed to create output directory");

    let document = build_document();
    let validated = document.validate().expect("validation failed");
    let style_store = build_style_store();
    let images = ImageStore::new();

    HwpxEncoder::encode_file(OUT_FILE, &validated, &style_store, &images)
        .expect("failed to encode tab edge case sample");

    println!("generated {OUT_FILE}");
}
