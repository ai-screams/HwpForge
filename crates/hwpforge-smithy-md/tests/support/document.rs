#![allow(dead_code)]

use hwpforge_core::{Document, PageSettings, Paragraph, Run, Section};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};

pub(crate) fn u(v: i32) -> HwpUnit {
    HwpUnit::new(v).unwrap()
}

pub(crate) fn para(runs: Vec<Run>) -> Paragraph {
    Paragraph::with_runs(runs, ParaShapeIndex::new(0))
}

pub(crate) fn text(t: &str) -> Run {
    Run::text(t, CharShapeIndex::new(0))
}

pub(crate) fn doc(runs: Vec<Run>) -> Document {
    let mut d = Document::new();
    d.add_section(Section::with_paragraphs(vec![para(runs)], PageSettings::a4()));
    d
}
