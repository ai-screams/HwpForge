use hwpforge_smithy_md::hancom_eqn_to_latex;

#[test]
fn visual_equations_convert_hancom_source_to_exact_latex() {
    assert_eq!(hancom_eqn_to_latex("{a} over {b}"), r"$\frac{a}{b}$");
    assert_eq!(hancom_eqn_to_latex("x ^{2}"), "$x^{2}$");
}
