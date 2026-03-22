use hwpforge_core::control::Control;
use hwpforge_core::document::Document;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::PageSettings;
use hwpforge_foundation::{Color, HwpUnit};

use crate::{
    csi, empty, encode_and_save, mascot_intro, p, psi, showcase_store, CS_BOLD, CS_NORMAL,
    CS_SMALL, CS_TITLE, PS_CENTER, PS_LEFT,
};

const EQUATION_BASE_LINE: u32 = 850;

#[derive(Clone, Copy)]
struct EquationSize {
    width: HwpUnit,
    height: HwpUnit,
}

#[derive(Clone, Copy)]
struct EquationStyles {
    block: EquationSize,
    inline: EquationSize,
    text_color: Color,
}

fn equation_control(script: &str, size: EquationSize, text_color: Color) -> Control {
    Control::Equation {
        script: script.to_string(),
        width: size.width,
        height: size.height,
        base_line: EQUATION_BASE_LINE,
        text_color,
        font: "HancomEQN".to_string(),
    }
}

fn equation_paragraph(script: &str, size: EquationSize, text_color: Color) -> Paragraph {
    Paragraph::with_runs(
        vec![Run::control(equation_control(script, size, text_color), csi(CS_NORMAL))],
        psi(PS_CENTER),
    )
}

fn inline_equation_run(script: &str, styles: EquationStyles) -> Run {
    Run::control(equation_control(script, styles.inline, styles.text_color), csi(CS_NORMAL))
}

fn push_block_equation(
    paras: &mut Vec<Paragraph>,
    label: &str,
    script: &str,
    size: EquationSize,
    text_color: Color,
) {
    paras.push(p(label, CS_BOLD, PS_LEFT));
    paras.push(equation_paragraph(script, size, text_color));
    paras.push(empty());
}

fn append_equation_showcase(paras: &mut Vec<Paragraph>, styles: EquationStyles) {
    paras.push(p("[블록 수식]", CS_TITLE, PS_LEFT));
    paras.push(empty());

    for (label, script) in [
        ("1. 분수 (Fraction):", "{a+b} over {c+d}"),
        ("2. 제곱근 (Square root):", "root {2} of {x^2 + y^2}"),
        ("3. 근의 공식 (Quadratic formula):", "x= {-b` +-  root {2} of {b ^{2} -`4ac}} over {2a}"),
        ("4. 오일러 항등식 (Euler's identity):", "e^{i pi} + 1 = 0"),
        (
            "5. 적분 (Gaussian integral):",
            "int _{0} ^{INF } {e ^{(-x ^{2} )}} dx = { root {2} of { pi }} over {2}",
        ),
        ("6. 급수 (Basel problem):", "sum _{n=1} ^{INF } {1} over {n ^{2}} = { pi  ^{2}} over {6}"),
        ("7. 극한 (Limit):", "lim _{x rarrow  0} {sin} (x)/x`=1"),
        ("8. 도함수 정의 (Derivative):", "f'(x)= lim _{h rarrow  0} {f(x+h)-f(x)} over {h}"),
        ("9. 행렬 (2x2 Matrix):", "{matrix{a&b#c&d}}"),
        ("10. 단위행렬 (3x3 Identity):", "I= {matrix{1&0&0#0&1&0#0&0&1}}"),
        (
            "11. 사인 법칙 (Law of sines):",
            "{a} over {sin`A} = {b} over {sin`B} = {c} over {sin`C} =`2R",
        ),
        ("12. 뉴턴 만유인력 (Gravitation):", "F`=`G {m _{1} m _{2}} over {r ^{2}}"),
        (
            "13. 이항정리 (Binomial theorem):",
            "(a+b) ^{n} = sum _{k=0} ^{n} {matrix{n#k}}`a ^{n-k} b ^{k}",
        ),
        (
            "14. 스털링 근사 (Stirling):",
            "n!` APPROX  root {2} of {2 pi  n}` ({ {n} over {e} }) ^{n}",
        ),
    ] {
        push_block_equation(paras, label, script, styles.block, styles.text_color);
    }

    paras.push(p("[인라인 수식]", CS_TITLE, PS_LEFT));
    paras.push(empty());

    paras.push(p("15. 인라인 수식 예시:", CS_BOLD, PS_LEFT));
    paras.push(Paragraph::with_runs(
        vec![
            Run::text("아인슈타인의 유명한 공식 ", csi(CS_NORMAL)),
            inline_equation_run("E`=`mc ^{2}", styles),
            Run::text(" 은 질량과 에너지의 등가성을 나타냅니다.", csi(CS_NORMAL)),
        ],
        psi(PS_LEFT),
    ));
    paras.push(empty());

    paras.push(Paragraph::with_runs(
        vec![
            Run::text("이차방정식 ", csi(CS_NORMAL)),
            inline_equation_run("ax ^{2} +bx+c=0", styles),
            Run::text(" 의 해는 근의 공식으로 구합니다.", csi(CS_NORMAL)),
        ],
        psi(PS_LEFT),
    ));
    paras.push(empty());

    paras.push(Paragraph::with_runs(
        vec![
            Run::text("원주율 ", csi(CS_NORMAL)),
            inline_equation_run("pi  APPROX 3.14159", styles),
            Run::text(" 는 무리수입니다.", csi(CS_NORMAL)),
        ],
        psi(PS_LEFT),
    ));
    paras.push(empty());

    paras.push(Paragraph::with_runs(
        vec![
            Run::text("확률 ", csi(CS_NORMAL)),
            inline_equation_run("P(A|B)= {P(B|A) CDOT P(A)} over {P(B)}", styles),
            Run::text(" 는 베이즈 정리입니다.", csi(CS_NORMAL)),
        ],
        psi(PS_LEFT),
    ));
}

fn append_fhe_report(paras: &mut Vec<Paragraph>, styles: EquationStyles) {
    let wide_size: EquationSize =
        EquationSize { width: HwpUnit::from_mm(70.0).unwrap(), height: styles.block.height };
    let medium_size: EquationSize =
        EquationSize { width: HwpUnit::from_mm(60.0).unwrap(), height: styles.block.height };

    paras.push(empty().with_page_break());
    paras.push(p("동형암호(Homomorphic Encryption)의 수학적 기초", CS_TITLE, PS_CENTER));
    paras.push(empty());

    paras.push(p("1. 서론", CS_BOLD, PS_LEFT));
    paras.push(empty());
    paras.push(p(
        "동형암호(Homomorphic Encryption, HE)는 암호화된 데이터에 대해 \
         복호화 없이 직접 연산을 수행할 수 있는 암호 체계이다. \
         기존의 공개키 암호 체계에서는 암호화된 데이터를 연산하기 위해 \
         반드시 복호화를 거쳐야 하므로, 클라우드 환경이나 제3자 서버에 \
         데이터를 위탁하는 경우 민감 정보의 노출 위험이 존재한다. \
         반면, 동형암호를 사용하면 암호문 상에서의 연산 결과가 \
         평문 연산 결과의 암호문과 동일하므로, 데이터 소유자의 비밀키 \
         없이도 서버가 연산을 대행할 수 있다.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(empty());

    push_block_equation(
        paras,
        "정의 1. 동형성 (덧셈):",
        "Dec(Enc(m _{1}) oplus Enc(m _{2})) = m _{1} + m _{2}",
        styles.block,
        styles.text_color,
    );
    push_block_equation(
        paras,
        "정의 2. 동형성 (곱셈):",
        "Dec(Enc(m _{1}) otimes Enc(m _{2})) = m _{1} cdot m _{2}",
        styles.block,
        styles.text_color,
    );

    paras.push(p(
        "완전동형암호(Fully Homomorphic Encryption, FHE)의 수학적 가능성은 \
         2009년 Gentry에 의해 처음 증명되었으며, 이는 격자(lattice) 이론의 \
         난제인 오류 학습(Learning With Errors, LWE) 문제의 계산적 어려움에 \
         기반한다.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(empty());

    paras.push(p("2. 수학적 기반: LWE 및 RLWE", CS_BOLD, PS_LEFT));
    paras.push(empty());
    paras.push(p("2.1. 다항식 환(Polynomial Ring)", CS_BOLD, PS_LEFT));
    paras.push(p(
        "현대의 동형암호 체계는 대부분 Ring-LWE 문제에 기반하며, \
         다음의 다항식 환 위에서 동작한다.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(equation_paragraph(
        "R _{q} = Z _{q} [X] / (X ^{n} + 1) ,`` n = 2 ^{k} ,` k in N",
        styles.block,
        styles.text_color,
    ));
    paras.push(empty());

    paras.push(p("2.2. LWE (Learning With Errors) 문제", CS_BOLD, PS_LEFT));
    paras.push(p(
        "Regev(2005)가 제안한 LWE 문제는 다음과 같이 정의된다. \
         비밀 벡터 s와 소규모 오차 e에 대해, 표본 (a, b)가 주어질 때 \
         이를 균일 난수 쌍과 구별하는 문제이다.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(equation_paragraph(
        "b = langle a , s rangle + e ,`` a in Z _{q} ^{n} ,` s in Z _{q} ^{n} ,` e ~ chi",
        wide_size,
        styles.text_color,
    ));
    paras.push(empty());

    paras.push(p("2.3. Ring-LWE (RLWE) 문제", CS_BOLD, PS_LEFT));
    paras.push(p(
        "RLWE는 LWE의 환(ring) 변형으로, 다항식 환 R_q 위에서 정의된다. \
         이상적 격자(ideal lattice) 문제의 최악 사례 어려움으로 환원되므로 \
         양자컴퓨터 공격에도 안전한 후양자(post-quantum) 암호로 분류된다.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(equation_paragraph(
        "b = a cdot s + e ,`` a in _{R} R _{q} ,` e ~ chi _{sigma} ,` sigma approx 3.2",
        wide_size,
        styles.text_color,
    ));
    paras.push(empty());

    paras.push(p("3. BFV 체계 (Brakerski/Fan/Vercauteren)", CS_BOLD, PS_LEFT));
    paras.push(empty());
    paras.push(p("3.1. 키 생성 (Key Generation)", CS_BOLD, PS_LEFT));
    paras.push(p(
        "공개키는 RLWE 표본으로 구성되며, 비밀키 s는 소규모 다항식이다.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(equation_paragraph(
        "pk = ( -(a cdot s + e) , a ) ,`` a in _{R} R _{q} ,` e ~ chi",
        wide_size,
        styles.text_color,
    ));
    paras.push(empty());

    paras.push(p("3.2. 암호화 (Encryption)", CS_BOLD, PS_LEFT));
    paras.push(p(
        "메시지 m을 평문 공간 R_t에서 암호화한다. \
         스케일 팩터 Δ = ⌊q/t⌋가 메시지를 암호문 공간으로 확장한다.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(equation_paragraph(
        "c _{0} = pk _{0} cdot u + e _{1} + lfloor {q} over {t} rfloor cdot m",
        wide_size,
        styles.text_color,
    ));
    paras.push(equation_paragraph(
        "c _{1} = pk _{1} cdot u + e _{2}",
        styles.block,
        styles.text_color,
    ));
    paras.push(empty());

    paras.push(p("3.3. 복호화 (Decryption)", CS_BOLD, PS_LEFT));
    paras.push(equation_paragraph(
        "m = left [ lfloor {t} over {q} cdot [c _{0} + c _{1} cdot s] _{q} rfloor right ] _{t}",
        medium_size,
        styles.text_color,
    ));
    paras.push(empty());

    paras.push(p("3.4. 동형 연산", CS_BOLD, PS_LEFT));
    paras.push(p("동형 덧셈:", CS_NORMAL, PS_LEFT));
    paras.push(equation_paragraph(
        "c ^{add} = (c _{0} + c _{0} prime , c _{1} + c _{1} prime ) mod q",
        wide_size,
        styles.text_color,
    ));
    paras.push(empty());
    paras.push(p("동형 곱셈 (재선형화 전):", CS_NORMAL, PS_LEFT));
    paras.push(equation_paragraph(
        "c _{2} ^{*} = lfloor {t} over {q} cdot c _{1} c _{1} prime rfloor",
        styles.block,
        styles.text_color,
    ));
    paras.push(empty());

    paras.push(p("4. CKKS 체계 (Cheon/Kim/Kim/Song)", CS_BOLD, PS_LEFT));
    paras.push(empty());
    paras.push(p(
        "CKKS(2017)는 실수 및 복소수에 대한 근사 동형암호 체계이다. \
         스케일 팩터 Δ가 정밀도를 제어하며, 반올림 오차를 \
         암호문 잡음의 일부로 취급하는 것이 핵심이다.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(empty());

    paras.push(p("4.1. 암호화 및 복호화", CS_BOLD, PS_LEFT));
    paras.push(equation_paragraph(
        "c = (c _{0} , c _{1} ) = [u cdot pk + ( Delta cdot m + e _{1} , e _{2} )] _{q}",
        wide_size,
        styles.text_color,
    ));
    paras.push(empty());
    paras.push(Paragraph::with_runs(
        vec![
            Run::text("복호화: ", csi(CS_NORMAL)),
            inline_equation_run("m approx {1} over {Delta} (c _{0} + c _{1} cdot s)", styles),
            Run::text(" 로 근사값을 복원한다.", csi(CS_NORMAL)),
        ],
        psi(PS_LEFT),
    ));
    paras.push(empty());

    paras.push(p("4.2. 재스케일링 (Rescaling)", CS_BOLD, PS_LEFT));
    paras.push(p(
        "곱셈 후 스케일이 Δ²으로 증가하므로, 재스케일링으로 Δ로 복원한다.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(equation_paragraph(
        "RS(c) = lfloor {q prime} over {q} cdot c rfloor ,`` q prime = {q} over {Delta}",
        medium_size,
        styles.text_color,
    ));
    paras.push(empty());

    paras.push(p("5. 잡음 성장 분석 (Noise Growth)", CS_BOLD, PS_LEFT));
    paras.push(empty());
    paras.push(p(
        "동형 연산 과정에서의 잡음 누적은 동형암호의 핵심 도전 과제이다. \
         동형 덧셈은 잡음이 선형적으로, 곱셈은 지수적으로 증가한다.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(empty());
    paras.push(Paragraph::with_runs(
        vec![
            Run::text("덧셈 후 잡음: ", csi(CS_NORMAL)),
            inline_equation_run("|| v _{add} || leq || v _{1} || + || v _{2} ||", styles),
        ],
        psi(PS_LEFT),
    ));
    paras.push(empty());
    paras.push(p("지원 가능한 곱셈 깊이(multiplicative depth):", CS_NORMAL, PS_LEFT));
    paras.push(equation_paragraph(
        "L approx { log _{2} q } over { log _{2} (n cdot B _{err}) }",
        styles.block,
        styles.text_color,
    ));
    paras.push(empty());
    paras.push(p("BFV 초기 암호문 잡음 분산:", CS_NORMAL, PS_LEFT));
    paras.push(equation_paragraph(
        "V _{v} = t ^{2} left ( {1} over {12} + sigma ^{2} left ( {4n} over {3} + 1 right ) right )",
        medium_size,
        styles.text_color,
    ));
    paras.push(empty());

    paras.push(p("6. 부트스트래핑 (Bootstrapping)", CS_BOLD, PS_LEFT));
    paras.push(empty());
    paras.push(p(
        "Gentry(2009)가 제안한 부트스트래핑은 잡음이 누적된 암호문을 \
         동형적으로 복호화 회로를 평가하여 잡음을 갱신(refresh)하는 기법이다. \
         이를 통해 무한한 깊이의 동형 연산이 이론적으로 가능해진다.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(empty());
    paras.push(equation_paragraph(
        "c ^{*} = Dec _{Enc(sk)} (c) approx Enc _{pk _{2}} (m)",
        styles.block,
        styles.text_color,
    ));
    paras.push(empty());

    paras.push(p("참고문헌", CS_BOLD, PS_LEFT));
    paras.push(empty());
    for reference in [
        "[1] C. Gentry, \"Fully Homomorphic Encryption Using Ideal Lattices,\" STOC 2009.",
        "[2] Z. Brakerski, C. Gentry, V. Vaikuntanathan, \"(Leveled) Fully Homomorphic Encryption without Bootstrapping,\" ITCS 2012.",
        "[3] J. Fan, F. Vercauteren, \"Somewhat Practical Fully Homomorphic Encryption,\" IACR ePrint 2012/144.",
        "[4] J.H. Cheon, A. Kim, M. Kim, Y. Song, \"Homomorphic Encryption for Arithmetic of Approximate Numbers,\" ASIACRYPT 2017.",
        "[5] O. Regev, \"On Lattices, Learning with Errors, Random Linear Codes, and Cryptography,\" STOC 2005.",
    ] {
        paras.push(p(reference, CS_SMALL, PS_LEFT));
    }
}

pub(crate) fn gen_13_equation() {
    let store = showcase_store();
    let (mut paras, images) = mascot_intro(
        "13. 수식",
        "HancomEQN 스크립트 형식의 다양한 수식을 확인합니다. \
         블록 수식과 인라인 수식을 모두 포함합니다.",
    );
    let styles = EquationStyles {
        block: EquationSize {
            width: HwpUnit::from_mm(50.0).unwrap(),
            height: HwpUnit::from_mm(15.0).unwrap(),
        },
        inline: EquationSize {
            width: HwpUnit::from_mm(25.0).unwrap(),
            height: HwpUnit::from_mm(8.0).unwrap(),
        },
        text_color: Color::from_rgb(0, 0, 0),
    };

    append_equation_showcase(&mut paras, styles);
    append_fhe_report(&mut paras, styles);

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(paras, PageSettings::a4()));
    encode_and_save("13_equation.hwpx", &store, &doc, &images);
}
