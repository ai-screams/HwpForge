//! Equation showcase: generates a single HWPX with diverse HancomEQN equations.
//!
//! Demonstrates:
//! - Basic fractions, roots, powers
//! - Famous formulas (Euler, quadratic, Pythagorean)
//! - Calculus (integrals, limits, summations, products)
//! - Matrices and determinants
//! - Greek symbols and special characters
//! - Colored equations (red, blue, green, purple)
//! - Various sizes and baselines
//! - Inline equations mixed with text paragraphs
//!
//! Usage:
//!   cargo run -p hwpforge-smithy-hwpx --example equation_styles
//!
//! Output:
//!   temp/equation_styles.hwpx

use hwpforge_core::control::Control;
use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::PageSettings;
use hwpforge_foundation::{CharShapeIndex, Color, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::HwpxEncoder;

// ── Helpers ──────────────────────────────────────────────────────

const CS0: CharShapeIndex = CharShapeIndex::new(0);
const PS0: ParaShapeIndex = ParaShapeIndex::new(0);

fn text_para(s: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(s, CS0)], PS0)
}

fn empty_para() -> Paragraph {
    text_para("")
}

/// Build an equation Run with the given parameters.
fn eq_run(script: &str, width: i32, height: i32, base_line: u32, color: Color) -> Run {
    Run::control(
        Control::Equation {
            script: script.to_string(),
            width: HwpUnit::new(width).unwrap(),
            height: HwpUnit::new(height).unwrap(),
            base_line,
            text_color: color,
            font: "HancomEQN".to_string(),
        },
        CS0,
    )
}

/// Build a paragraph containing a single equation.
fn eq_para(script: &str, width: i32, height: i32, base_line: u32, color: Color) -> Paragraph {
    Paragraph::with_runs(vec![eq_run(script, width, height, base_line, color)], PS0)
}

/// Build a paragraph with label text followed by an equation.
fn labeled_eq(label: &str, script: &str, w: i32, h: i32, bl: u32, color: Color) -> Vec<Paragraph> {
    vec![text_para(label), eq_para(script, w, h, bl, color), empty_para()]
}

fn build_store() -> HwpxStyleStore {
    let mut store = HwpxStyleStore::with_default_fonts("함초롬돋움");
    store.push_char_shape(HwpxCharShape::default());
    store.push_para_shape(HwpxParaShape::default());
    store
}

// colour constants used throughout
const BLACK: Color = Color::BLACK;
const DARK_BLUE: Color = Color::from_rgb(0x00, 0x00, 0xCC);
const DARK_RED: Color = Color::from_rgb(0xCC, 0x00, 0x00);
const DARK_GREEN: Color = Color::from_rgb(0x00, 0x66, 0x00);
const PURPLE: Color = Color::from_rgb(0x80, 0x00, 0x80);

fn push_heading(paras: &mut Vec<Paragraph>, heading: &str) {
    paras.push(text_para(heading));
    paras.push(empty_para());
}

fn append_basic_fractions(paras: &mut Vec<Paragraph>) {
    push_heading(paras, "1. 기본 분수 (Fractions)");
    paras.extend(labeled_eq("  단순 분수:", "{c+d} over {a+b}", 2467, 2250, 66, BLACK));
    paras.extend(labeled_eq(
        "  연분수:",
        "1+ {1} over {1+ {1} over {1+ {1} over {x}}}",
        3500,
        5000,
        55,
        BLACK,
    ));
    paras.extend(labeled_eq(
        "  혼합 분수:",
        "3 {2} over {5} +`1 {1} over {3} =`4 {11} over {15}",
        8000,
        2250,
        66,
        BLACK,
    ));
}

fn append_powers_and_roots(paras: &mut Vec<Paragraph>) {
    push_heading(paras, "2. 거듭제곱과 루트 (Powers & Roots)");
    paras.extend(labeled_eq("  제곱근:", "root {2} of {x ^{2} +`1}", 4020, 1308, 90, BLACK));
    paras.extend(labeled_eq("  세제곱근:", "root {3} of {(x ^{2} +1)}", 4020, 1308, 90, BLACK));
    paras.extend(labeled_eq(
        "  중첩 루트:",
        "root {2} of {1+`root {2} of {1+`root {2} of {x}}}",
        8000,
        2200,
        70,
        BLACK,
    ));
    paras.extend(labeled_eq(
        "  지수 표기:",
        "2 ^{10} =`1024,```e ^{i pi } +`1`=`0",
        9000,
        1175,
        88,
        BLACK,
    ));
}

fn append_famous_formulas(paras: &mut Vec<Paragraph>) {
    push_heading(paras, "3. 유명한 공식 (Famous Formulas)");
    paras.extend(labeled_eq(
        "  오일러 항등식 (Euler's Identity):",
        "e ^{(i pi  )} +`1`=`0",
        5164,
        1175,
        88,
        DARK_BLUE,
    ));
    paras.extend(labeled_eq(
        "  근의 공식 (Quadratic Formula):",
        "x= {-b` +-  root {2} of {b ^{2} -`4ac}} over {2a}",
        8779,
        2600,
        71,
        DARK_RED,
    ));
    paras.extend(labeled_eq(
        "  피타고라스 정리:",
        "a ^{2} +`b ^{2} =c ^{2}",
        5000,
        1175,
        88,
        DARK_GREEN,
    ));
    paras.extend(labeled_eq("  원의 넓이:", "A= pi  r ^{2}", 3269, 1163, 89, BLACK));
    paras.extend(labeled_eq("  질량-에너지 등가:", "E`=`mc ^{2}", 3000, 1175, 88, PURPLE));
}

fn append_calculus(paras: &mut Vec<Paragraph>) {
    push_heading(paras, "4. 미적분학 (Calculus)");
    paras.extend(labeled_eq(
        "  정적분 (Definite Integral):",
        "int _{a} ^{b} f(x)`dx",
        4000,
        2586,
        62,
        BLACK,
    ));
    paras.extend(labeled_eq(
        "  가우시안 적분:",
        "int _{0} ^{INF } {e ^{(-x ^{2} )}} dx = { root {2} of { pi }} over {2}",
        10000,
        2600,
        62,
        DARK_BLUE,
    ));
    paras.extend(labeled_eq(
        "  극한 (Limit):",
        "lim _{x rarrow  0} {sin} (x)/x`=1",
        7070,
        1875,
        51,
        BLACK,
    ));
    paras.extend(labeled_eq(
        "  미분의 정의:",
        "f'(x)= lim _{h rarrow  0} {f(x+h)-f(x)} over {h}",
        11000,
        2400,
        60,
        DARK_RED,
    ));
    paras.extend(labeled_eq("  편미분:", "sigma  f/ sigma  x", 2496, 975, 86, BLACK));
}

fn append_series_and_products(paras: &mut Vec<Paragraph>) {
    push_heading(paras, "5. 급수와 곱 (Series & Products)");
    paras.extend(labeled_eq(
        "  제곱의 합:",
        "sum _{k=1} ^{n} k ^{2} =n(n+1)(2n+1)/6",
        11137,
        2700,
        63,
        BLACK,
    ));
    paras.extend(labeled_eq(
        "  곱기호 (Product):",
        "prod _{i=0} ^{n} a _{i}",
        2033,
        2700,
        63,
        BLACK,
    ));
    paras.extend(labeled_eq(
        "  테일러 급수 (e^x):",
        "e ^{x} =1+ {x} over {1!} + {x ^{2}} over {2!} + {x ^{3}} over {3!} +`...",
        16000,
        2438,
        69,
        DARK_GREEN,
    ));
    paras.extend(labeled_eq(
        "  푸리에 급수:",
        "f(x)=a _{0} + sum _{n=1} ^{INF } `(a _{n} cos {n pi  x} over {L} +`b _{n} sin {n pi  x} over {L} )",
        16876,
        2700,
        63,
        DARK_BLUE,
    ));
    paras.extend(labeled_eq(
        "  기하급수 (등비급수):",
        "sum _{k=0} ^{INF } r ^{k} = {1} over {1-r} ,```|r|<1",
        10000,
        2700,
        63,
        BLACK,
    ));
}

fn append_matrices(paras: &mut Vec<Paragraph>) {
    push_heading(paras, "6. 행렬 (Matrices)");
    paras.extend(labeled_eq("  2x2 행렬:", "{matrix{1&2#3&4}}", 1144, 2100, 67, BLACK));
    paras.extend(labeled_eq("  행렬식:", "det(A)=`ad-bc", 7119, 1000, 86, BLACK));
    paras.extend(labeled_eq(
        "  3x3 단위행렬:",
        "I= {matrix{1&0&0#0&1&0#0&0&1}}",
        3500,
        3000,
        60,
        PURPLE,
    ));
}

fn append_trigonometric(paras: &mut Vec<Paragraph>) {
    push_heading(paras, "7. 삼각함수 (Trigonometric)");
    paras.extend(labeled_eq(
        "  삼각함수 덧셈정리:",
        "cos` alpha  `+`cos` beta  =2`cos {1} over {2} ( alpha  `+` beta  )`cos {1} over {2} ( alpha  `-` beta  `)",
        18199,
        2250,
        66,
        BLACK,
    ));
    paras.extend(labeled_eq(
        "  사인 법칙:",
        "{a} over {sin`A} = {b} over {sin`B} = {c} over {sin`C} =`2R",
        12000,
        2250,
        66,
        DARK_RED,
    ));
    paras.extend(labeled_eq(
        "  기본 항등식:",
        "sin ^{2}  theta +`cos ^{2}  theta `=`1",
        7000,
        1175,
        88,
        BLACK,
    ));
}

fn append_colored_equations(paras: &mut Vec<Paragraph>) {
    push_heading(paras, "8. 색상별 수식 (Colored Equations)");
    let colored: &[(&str, &str, Color)] = &[
        ("검정 (기본)", "a ^{2} +`b ^{2} =`c ^{2}", BLACK),
        ("빨강", "E`=`mc ^{2}", Color::from_rgb(0xFF, 0x00, 0x00)),
        ("파랑", "F`=`ma", Color::from_rgb(0x00, 0x00, 0xFF)),
        ("초록", "PV`=`nRT", Color::from_rgb(0x00, 0x80, 0x00)),
        ("보라", "e ^{i pi } +`1`=`0", PURPLE),
        ("주황", "f`=`ma", Color::from_rgb(0xFF, 0x8C, 0x00)),
        ("자홍", "lambda `=` {h} over {p}", Color::from_rgb(0xFF, 0x00, 0xFF)),
    ];
    for &(label, script, color) in colored {
        paras.push(text_para(&format!("  {label}:")));
        paras.push(eq_para(script, 5000, 1175, 88, color));
        paras.push(empty_para());
    }
}

fn append_inline_equations(paras: &mut Vec<Paragraph>) {
    push_heading(paras, "9. 인라인 수식 (Inline Equations with Text)");
    paras.push(Paragraph::with_runs(
        vec![
            Run::text("원의 넓이는 ", CS0),
            eq_run("A= pi  r ^{2}", 3269, 1163, 89, DARK_BLUE),
            Run::text(" 이고, 둘레는 ", CS0),
            eq_run("C=`2 pi  r", 3000, 1000, 86, DARK_RED),
            Run::text(" 입니다.", CS0),
        ],
        PS0,
    ));
    paras.push(empty_para());

    paras.push(Paragraph::with_runs(
        vec![
            Run::text("이차방정식 ", CS0),
            eq_run("ax ^{2} +`bx+c`=`0", 5000, 1175, 88, BLACK),
            Run::text(" 의 해는 ", CS0),
            eq_run("x= {-b` +-  root {2} of {b ^{2} -`4ac}} over {2a}", 8779, 2600, 71, DARK_RED),
            Run::text(" 이다.", CS0),
        ],
        PS0,
    ));
    paras.push(empty_para());

    paras.push(Paragraph::with_runs(
        vec![
            Run::text("함수 ", CS0),
            eq_run("f(x)=`x ^{2}", 3500, 1175, 88, DARK_GREEN),
            Run::text(" 의 도함수는 ", CS0),
            eq_run("f'(x)=`2x", 3000, 1000, 86, DARK_GREEN),
            Run::text(" 이다.", CS0),
        ],
        PS0,
    ));
    paras.push(empty_para());
}

fn append_greek_and_symbols(paras: &mut Vec<Paragraph>) {
    push_heading(paras, "10. 그리스 문자 & 특수기호");
    paras.extend(labeled_eq(
        "  그리스 소문자:",
        "alpha  ` beta  ` gamma  ` delta  ` varepsilon  ` zeta  ` eta  ` theta",
        12000,
        975,
        86,
        BLACK,
    ));
    paras.extend(labeled_eq(
        "  그리스 대문자:",
        "ALPHA  `BETA  `GAMMA  `DELTA  `THETA  `LAMBDA  `SIGMA  `OMEGA",
        12000,
        975,
        86,
        BLACK,
    ));
    paras.extend(labeled_eq(
        "  특수 기호:",
        "INF  ` FORALL ` EXISTS ` BECAUSE ` THEREFORE ` SUBSET ` SUPSET ` IN ` NOTIN",
        14000,
        975,
        86,
        PURPLE,
    ));
    paras.extend(labeled_eq(
        "  화살표:",
        "rarrow  ` larrow  ` DARROW  ` UPARROW  ` DOWNARROW  ` LEFTRIGHTARROW",
        14000,
        975,
        86,
        DARK_BLUE,
    ));
}

fn append_physics_formulas(paras: &mut Vec<Paragraph>) {
    push_heading(paras, "11. 물리학 공식 (Physics Formulas)");
    paras.extend(labeled_eq(
        "  뉴턴의 만유인력:",
        "F`=`G {m _{1} m _{2}} over {r ^{2}}",
        7000,
        2250,
        66,
        DARK_RED,
    ));
    paras.extend(labeled_eq(
        "  슈뢰딩거 방정식:",
        "i`HBAR ` { sigma } over { sigma `t} psi `=`H psi",
        8000,
        2250,
        66,
        DARK_BLUE,
    ));
    paras.extend(labeled_eq(
        "  맥스웰 방정식 (가우스 법칙):",
        "NABLA  CDOT `E`= { rho } over { varepsilon  _{0}}",
        6000,
        2250,
        66,
        DARK_GREEN,
    ));
    paras.extend(labeled_eq(
        "  드브로이 파장:",
        "lambda `=` {h} over {p} =` {h} over {mv}",
        7000,
        2250,
        66,
        PURPLE,
    ));
}

fn append_complex_formulas(paras: &mut Vec<Paragraph>) {
    push_heading(paras, "12. 복잡한 수식 (Complex Formulas)");
    paras.extend(labeled_eq(
        "  스털링 근사:",
        "n!` APPROX  root {2} of {2 pi  n}` ({ {n} over {e} }) ^{n}",
        10000,
        2600,
        62,
        BLACK,
    ));
    paras.extend(labeled_eq(
        "  바젤 문제:",
        "sum _{n=1} ^{INF } {1} over {n ^{2}} = { pi  ^{2}} over {6}",
        8000,
        2700,
        63,
        DARK_RED,
    ));
    paras.extend(labeled_eq(
        "  이항정리:",
        "(a+b) ^{n} = sum _{k=0} ^{n} {matrix{n#k}}`a ^{n-k} b ^{k}",
        14000,
        2700,
        63,
        DARK_BLUE,
    ));
    paras.extend(labeled_eq(
        "  코시-슈바르츠 부등식:",
        "|`X CDOT `Y`| <=  ||`X`||`||`Y`||",
        8000,
        1175,
        88,
        DARK_GREEN,
    ));
}

fn build_paragraphs() -> Vec<Paragraph> {
    let mut paras: Vec<Paragraph> = Vec::new();
    paras.push(text_para("HwpForge 수식(Equation) API 종합 데모"));
    paras.push(empty_para());
    append_basic_fractions(&mut paras);
    append_powers_and_roots(&mut paras);
    append_famous_formulas(&mut paras);
    append_calculus(&mut paras);
    append_series_and_products(&mut paras);
    append_matrices(&mut paras);
    append_trigonometric(&mut paras);
    append_colored_equations(&mut paras);
    append_inline_equations(&mut paras);
    append_greek_and_symbols(&mut paras);
    append_physics_formulas(&mut paras);
    append_complex_formulas(&mut paras);
    paras.push(empty_para());
    paras.push(text_para("--- 수식 데모 끝 ---"));
    paras
}

// ── Main ─────────────────────────────────────────────────────────

fn main() {
    println!("=== Equation Style Showcase ===\n");
    std::fs::create_dir_all("temp").unwrap();

    let store: HwpxStyleStore = build_store();
    let images: ImageStore = ImageStore::new();
    let paras: Vec<Paragraph> = build_paragraphs();

    let mut doc: Document = Document::new();
    doc.add_section(Section::with_paragraphs(paras, PageSettings::a4()));

    let validated = doc.validate().expect("validation failed");
    let bytes = HwpxEncoder::encode(&validated, &store, &images).expect("encode failed");

    let output_path = "temp/equation_styles.hwpx";
    std::fs::write(output_path, &bytes).expect("write failed");

    println!("  Generated: {output_path} ({} bytes)", bytes.len());
    println!("\n한글(Hancom Office)에서 열어 확인하세요!");
}
