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
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxFont, HwpxParaShape, HwpxStyleStore};
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
fn eq_run(script: &str, width: i32, height: i32, base_line: u32, color: &str) -> Run {
    Run::control(
        Control::Equation {
            script: script.to_string(),
            width: HwpUnit::new(width).unwrap(),
            height: HwpUnit::new(height).unwrap(),
            base_line,
            text_color: color.to_string(),
            font: "HancomEQN".to_string(),
        },
        CS0,
    )
}

/// Build a paragraph containing a single equation.
fn eq_para(script: &str, width: i32, height: i32, base_line: u32, color: &str) -> Paragraph {
    Paragraph::with_runs(vec![eq_run(script, width, height, base_line, color)], PS0)
}

/// Build a paragraph with label text followed by an equation.
fn labeled_eq(label: &str, script: &str, w: i32, h: i32, bl: u32, color: &str) -> Vec<Paragraph> {
    vec![text_para(label), eq_para(script, w, h, bl, color), empty_para()]
}

fn build_store() -> HwpxStyleStore {
    let mut store = HwpxStyleStore::new();
    for &lang in &["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"] {
        store.push_font(HwpxFont::new(0, "함초롬돋움", lang));
    }
    store.push_char_shape(HwpxCharShape::default());
    store.push_para_shape(HwpxParaShape::default());
    store
}

// ── Main ─────────────────────────────────────────────────────────

#[allow(clippy::vec_init_then_push)]
fn main() {
    println!("=== Equation Style Showcase ===\n");
    std::fs::create_dir_all("temp").unwrap();

    let store = build_store();
    let images = ImageStore::new();
    let mut paras: Vec<Paragraph> = Vec::new();

    // ── Title ──
    paras.push(text_para("HwpForge 수식(Equation) API 종합 데모"));
    paras.push(empty_para());

    // ════════════════════════════════════════════════════════════════
    // 1. 기본 분수 (Fractions)
    // ════════════════════════════════════════════════════════════════
    paras.push(text_para("1. 기본 분수 (Fractions)"));
    paras.push(empty_para());

    paras.extend(labeled_eq("  단순 분수:", "{c+d} over {a+b}", 2467, 2250, 66, "#000000"));
    paras.extend(labeled_eq(
        "  연분수:",
        "1+ {1} over {1+ {1} over {1+ {1} over {x}}}",
        3500,
        5000,
        55,
        "#000000",
    ));
    paras.extend(labeled_eq(
        "  혼합 분수:",
        "3 {2} over {5} +`1 {1} over {3} =`4 {11} over {15}",
        8000,
        2250,
        66,
        "#000000",
    ));

    // ════════════════════════════════════════════════════════════════
    // 2. 거듭제곱과 루트 (Powers & Roots)
    // ════════════════════════════════════════════════════════════════
    paras.push(text_para("2. 거듭제곱과 루트 (Powers & Roots)"));
    paras.push(empty_para());

    paras.extend(labeled_eq("  제곱근:", "root {2} of {x ^{2} +`1}", 4020, 1308, 90, "#000000"));
    paras.extend(labeled_eq("  세제곱근:", "root {3} of {(x ^{2} +1)}", 4020, 1308, 90, "#000000"));
    paras.extend(labeled_eq(
        "  중첩 루트:",
        "root {2} of {1+`root {2} of {1+`root {2} of {x}}}",
        8000,
        2200,
        70,
        "#000000",
    ));
    paras.extend(labeled_eq(
        "  지수 표기:",
        "2 ^{10} =`1024,```e ^{i pi } +`1`=`0",
        9000,
        1175,
        88,
        "#000000",
    ));

    // ════════════════════════════════════════════════════════════════
    // 3. 유명한 공식 (Famous Formulas)
    // ════════════════════════════════════════════════════════════════
    paras.push(text_para("3. 유명한 공식 (Famous Formulas)"));
    paras.push(empty_para());

    // Euler's identity
    paras.extend(labeled_eq(
        "  오일러 항등식 (Euler's Identity):",
        "e ^{(i pi  )} +`1`=`0",
        5164,
        1175,
        88,
        "#0000CC",
    ));

    // Quadratic formula
    paras.extend(labeled_eq(
        "  근의 공식 (Quadratic Formula):",
        "x= {-b` +-  root {2} of {b ^{2} -`4ac}} over {2a}",
        8779,
        2600,
        71,
        "#CC0000",
    ));

    // Pythagorean theorem
    paras.extend(labeled_eq(
        "  피타고라스 정리:",
        "a ^{2} +`b ^{2} =c ^{2}",
        5000,
        1175,
        88,
        "#006600",
    ));

    // Area of circle
    paras.extend(labeled_eq("  원의 넓이:", "A= pi  r ^{2}", 3269, 1163, 89, "#000000"));

    // E=mc^2
    paras.extend(labeled_eq("  질량-에너지 등가:", "E`=`mc ^{2}", 3000, 1175, 88, "#800080"));

    // ════════════════════════════════════════════════════════════════
    // 4. 미적분학 (Calculus)
    // ════════════════════════════════════════════════════════════════
    paras.push(text_para("4. 미적분학 (Calculus)"));
    paras.push(empty_para());

    // Definite integral
    paras.extend(labeled_eq(
        "  정적분 (Definite Integral):",
        "int _{a} ^{b} f(x)`dx",
        4000,
        2586,
        62,
        "#000000",
    ));

    // Gaussian integral
    paras.extend(labeled_eq(
        "  가우시안 적분:",
        "int _{0} ^{INF } {e ^{(-x ^{2} )}} dx = { root {2} of { pi }} over {2}",
        10000,
        2600,
        62,
        "#0000CC",
    ));

    // Limit
    paras.extend(labeled_eq(
        "  극한 (Limit):",
        "lim _{x rarrow  0} {sin} (x)/x`=1",
        7070,
        1875,
        51,
        "#000000",
    ));

    // Derivative limit definition
    paras.extend(labeled_eq(
        "  미분의 정의:",
        "f'(x)= lim _{h rarrow  0} {f(x+h)-f(x)} over {h}",
        11000,
        2400,
        60,
        "#CC0000",
    ));

    // Partial derivative
    paras.extend(labeled_eq("  편미분:", "sigma  f/ sigma  x", 2496, 975, 86, "#000000"));

    // ════════════════════════════════════════════════════════════════
    // 5. 급수와 곱 (Series & Products)
    // ════════════════════════════════════════════════════════════════
    paras.push(text_para("5. 급수와 곱 (Series & Products)"));
    paras.push(empty_para());

    // Sum of squares
    paras.extend(labeled_eq(
        "  제곱의 합:",
        "sum _{k=1} ^{n} k ^{2} =n(n+1)(2n+1)/6",
        11137,
        2700,
        63,
        "#000000",
    ));

    // Product
    paras.extend(labeled_eq(
        "  곱기호 (Product):",
        "prod _{i=0} ^{n} a _{i}",
        2033,
        2700,
        63,
        "#000000",
    ));

    // Taylor series for e^x
    paras.extend(labeled_eq(
        "  테일러 급수 (e^x):",
        "e ^{x} =1+ {x} over {1!} + {x ^{2}} over {2!} + {x ^{3}} over {3!} +`...",
        16000,
        2438,
        69,
        "#006600",
    ));

    // Fourier series
    paras.extend(labeled_eq(
        "  푸리에 급수:",
        "f(x)=a _{0} + sum _{n=1} ^{INF } `(a _{n} cos {n pi  x} over {L} +`b _{n} sin {n pi  x} over {L} )",
        16876, 2700, 63, "#0000CC",
    ));

    // Geometric series
    paras.extend(labeled_eq(
        "  기하급수 (등비급수):",
        "sum _{k=0} ^{INF } r ^{k} = {1} over {1-r} ,```|r|<1",
        10000,
        2700,
        63,
        "#000000",
    ));

    // ════════════════════════════════════════════════════════════════
    // 6. 행렬 (Matrices)
    // ════════════════════════════════════════════════════════════════
    paras.push(text_para("6. 행렬 (Matrices)"));
    paras.push(empty_para());

    // 2x2 matrix
    paras.extend(labeled_eq("  2x2 행렬:", "{matrix{1&2#3&4}}", 1144, 2100, 67, "#000000"));

    // Determinant
    paras.extend(labeled_eq("  행렬식:", "det(A)=`ad-bc", 7119, 1000, 86, "#000000"));

    // 3x3 identity matrix
    paras.extend(labeled_eq(
        "  3x3 단위행렬:",
        "I= {matrix{1&0&0#0&1&0#0&0&1}}",
        3500,
        3000,
        60,
        "#800080",
    ));

    // ════════════════════════════════════════════════════════════════
    // 7. 삼각함수 (Trigonometric)
    // ════════════════════════════════════════════════════════════════
    paras.push(text_para("7. 삼각함수 (Trigonometric)"));
    paras.push(empty_para());

    paras.extend(labeled_eq(
        "  삼각함수 덧셈정리:",
        "cos` alpha  `+`cos` beta  =2`cos {1} over {2} ( alpha  `+` beta  )`cos {1} over {2} ( alpha  `-` beta  `)",
        18199, 2250, 66, "#000000",
    ));

    paras.extend(labeled_eq(
        "  사인 법칙:",
        "{a} over {sin`A} = {b} over {sin`B} = {c} over {sin`C} =`2R",
        12000,
        2250,
        66,
        "#CC0000",
    ));

    paras.extend(labeled_eq(
        "  기본 항등식:",
        "sin ^{2}  theta +`cos ^{2}  theta `=`1",
        7000,
        1175,
        88,
        "#000000",
    ));

    // ════════════════════════════════════════════════════════════════
    // 8. 색상별 수식 (Colored Equations)
    // ════════════════════════════════════════════════════════════════
    paras.push(text_para("8. 색상별 수식 (Colored Equations)"));
    paras.push(empty_para());

    let colored = &[
        ("검정 (기본)", "a ^{2} +`b ^{2} =`c ^{2}", "#000000"),
        ("빨강", "E`=`mc ^{2}", "#FF0000"),
        ("파랑", "F`=`ma", "#0000FF"),
        ("초록", "PV`=`nRT", "#008000"),
        ("보라", "e ^{i pi } +`1`=`0", "#800080"),
        ("주황", "f`=`ma", "#FF8C00"),
        ("자홍", "lambda `=` {h} over {p}", "#FF00FF"),
    ];
    for &(label, script, color) in colored {
        paras.push(text_para(&format!("  {label}:")));
        paras.push(eq_para(script, 5000, 1175, 88, color));
        paras.push(empty_para());
    }

    // ════════════════════════════════════════════════════════════════
    // 9. 인라인 수식 (Inline Equations with Text)
    // ════════════════════════════════════════════════════════════════
    paras.push(text_para("9. 인라인 수식 (Inline Equations with Text)"));
    paras.push(empty_para());

    // Paragraph with text + equation + text + equation + text
    paras.push(Paragraph::with_runs(
        vec![
            Run::text("원의 넓이는 ", CS0),
            eq_run("A= pi  r ^{2}", 3269, 1163, 89, "#0000CC"),
            Run::text(" 이고, 둘레는 ", CS0),
            eq_run("C=`2 pi  r", 3000, 1000, 86, "#CC0000"),
            Run::text(" 입니다.", CS0),
        ],
        PS0,
    ));
    paras.push(empty_para());

    paras.push(Paragraph::with_runs(
        vec![
            Run::text("이차방정식 ", CS0),
            eq_run("ax ^{2} +`bx+c`=`0", 5000, 1175, 88, "#000000"),
            Run::text(" 의 해는 ", CS0),
            eq_run("x= {-b` +-  root {2} of {b ^{2} -`4ac}} over {2a}", 8779, 2600, 71, "#CC0000"),
            Run::text(" 이다.", CS0),
        ],
        PS0,
    ));
    paras.push(empty_para());

    paras.push(Paragraph::with_runs(
        vec![
            Run::text("함수 ", CS0),
            eq_run("f(x)=`x ^{2}", 3500, 1175, 88, "#006600"),
            Run::text(" 의 도함수는 ", CS0),
            eq_run("f'(x)=`2x", 3000, 1000, 86, "#006600"),
            Run::text(" 이다.", CS0),
        ],
        PS0,
    ));
    paras.push(empty_para());

    // ════════════════════════════════════════════════════════════════
    // 10. 그리스 문자 & 특수기호 (Greek & Special Symbols)
    // ════════════════════════════════════════════════════════════════
    paras.push(text_para("10. 그리스 문자 & 특수기호"));
    paras.push(empty_para());

    paras.extend(labeled_eq(
        "  그리스 소문자:",
        "alpha  ` beta  ` gamma  ` delta  ` varepsilon  ` zeta  ` eta  ` theta",
        12000,
        975,
        86,
        "#000000",
    ));
    paras.extend(labeled_eq(
        "  그리스 대문자:",
        "ALPHA  `BETA  `GAMMA  `DELTA  `THETA  `LAMBDA  `SIGMA  `OMEGA",
        12000,
        975,
        86,
        "#000000",
    ));
    paras.extend(labeled_eq(
        "  특수 기호:",
        "INF  ` FORALL ` EXISTS ` BECAUSE ` THEREFORE ` SUBSET ` SUPSET ` IN ` NOTIN",
        14000,
        975,
        86,
        "#800080",
    ));
    paras.extend(labeled_eq(
        "  화살표:",
        "rarrow  ` larrow  ` DARROW  ` UPARROW  ` DOWNARROW  ` LEFTRIGHTARROW",
        14000,
        975,
        86,
        "#0000CC",
    ));

    // ════════════════════════════════════════════════════════════════
    // 11. 물리학 공식 (Physics)
    // ════════════════════════════════════════════════════════════════
    paras.push(text_para("11. 물리학 공식 (Physics Formulas)"));
    paras.push(empty_para());

    paras.extend(labeled_eq(
        "  뉴턴의 만유인력:",
        "F`=`G {m _{1} m _{2}} over {r ^{2}}",
        7000,
        2250,
        66,
        "#CC0000",
    ));
    paras.extend(labeled_eq(
        "  슈뢰딩거 방정식:",
        "i`HBAR ` { sigma } over { sigma `t} psi `=`H psi",
        8000,
        2250,
        66,
        "#0000CC",
    ));
    paras.extend(labeled_eq(
        "  맥스웰 방정식 (가우스 법칙):",
        "NABLA  CDOT `E`= { rho } over { varepsilon  _{0}}",
        6000,
        2250,
        66,
        "#006600",
    ));
    paras.extend(labeled_eq(
        "  드브로이 파장:",
        "lambda `=` {h} over {p} =` {h} over {mv}",
        7000,
        2250,
        66,
        "#800080",
    ));

    // ════════════════════════════════════════════════════════════════
    // 12. 큰 수식 모음 (Complex Formulas)
    // ════════════════════════════════════════════════════════════════
    paras.push(text_para("12. 복잡한 수식 (Complex Formulas)"));
    paras.push(empty_para());

    // Stirling's approximation
    paras.extend(labeled_eq(
        "  스털링 근사:",
        "n!` APPROX  root {2} of {2 pi  n}` ({ {n} over {e} }) ^{n}",
        10000,
        2600,
        62,
        "#000000",
    ));

    // Basel problem
    paras.extend(labeled_eq(
        "  바젤 문제:",
        "sum _{n=1} ^{INF } {1} over {n ^{2}} = { pi  ^{2}} over {6}",
        8000,
        2700,
        63,
        "#CC0000",
    ));

    // Binomial theorem
    paras.extend(labeled_eq(
        "  이항정리:",
        "(a+b) ^{n} = sum _{k=0} ^{n} {matrix{n#k}}`a ^{n-k} b ^{k}",
        14000,
        2700,
        63,
        "#0000CC",
    ));

    // Cauchy-Schwarz inequality
    paras.extend(labeled_eq(
        "  코시-슈바르츠 부등식:",
        "|`X CDOT `Y`| <=  ||`X`||`||`Y`||",
        8000,
        1175,
        88,
        "#006600",
    ));

    // ── Done ──
    paras.push(empty_para());
    paras.push(text_para("--- 수식 데모 끝 ---"));

    // Build document
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(paras, PageSettings::a4()));

    let validated = doc.validate().expect("validation failed");
    let bytes = HwpxEncoder::encode(&validated, &store, &images).expect("encode failed");

    let output_path = "temp/equation_styles.hwpx";
    std::fs::write(output_path, &bytes).expect("write failed");

    println!("  Generated: {output_path} ({} bytes)", bytes.len());
    println!("\n한글(Hancom Office)에서 열어 확인하세요!");
}
