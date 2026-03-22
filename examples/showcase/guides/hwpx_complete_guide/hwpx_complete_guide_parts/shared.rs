use hwpforge_core::control::Control;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::table::TableCell;
use hwpforge_foundation::{CharShapeIndex, Color, HwpUnit, ParaShapeIndex, StyleIndex};

pub(crate) const CS_NORMAL: usize = 0;
pub(crate) const CS_TITLE: usize = 1;
pub(crate) const CS_HEADING: usize = 2;
pub(crate) const CS_SMALL: usize = 3;
pub(crate) const CS_RED_BOLD: usize = 4;
pub(crate) const CS_BLUE: usize = 5;
pub(crate) const CS_GREEN_ITALIC: usize = 6;
pub(crate) const CS_GRAY: usize = 7;

pub(crate) const PS_BODY: usize = 0;
pub(crate) const PS_CENTER: usize = 1;
pub(crate) const PS_LEFT: usize = 2;
pub(crate) const PS_RIGHT: usize = 3;
pub(crate) const PS_DISTRIBUTE: usize = 4;

pub(crate) fn p(text: &str, cs: usize, ps: usize) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(cs))], ParaShapeIndex::new(ps))
}

pub(crate) fn empty() -> Paragraph {
    p("", CS_NORMAL, PS_BODY)
}

pub(crate) fn ctrl_p(ctrl: Control, cs: usize, ps: usize) -> Paragraph {
    Paragraph::with_runs(vec![Run::control(ctrl, CharShapeIndex::new(cs))], ParaShapeIndex::new(ps))
}

pub(crate) fn runs_p(runs: Vec<Run>, ps: usize) -> Paragraph {
    Paragraph::with_runs(runs, ParaShapeIndex::new(ps))
}

pub(crate) fn styled_p(text: &str, cs: usize, ps: usize, style_id: usize) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(cs))], ParaShapeIndex::new(ps))
        .with_style(StyleIndex::new(style_id))
}

pub(crate) fn csi(idx: usize) -> CharShapeIndex {
    CharShapeIndex::new(idx)
}

pub(crate) fn text_cell(text: &str, width_mm: f64, cs: usize, ps: usize) -> TableCell {
    TableCell::new(vec![p(text, cs, ps)], HwpUnit::from_mm(width_mm).unwrap())
}

pub(crate) fn colored_cell(
    text: &str,
    width_mm: f64,
    cs: usize,
    ps: usize,
    r: u8,
    g: u8,
    b: u8,
) -> TableCell {
    text_cell(text, width_mm, cs, ps).with_background(Color::from_rgb(r, g, b))
}
