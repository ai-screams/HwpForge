#![allow(dead_code)]

use hwpforge_core::control::FieldBoundary;
use hwpforge_core::{Control, Paragraph, Run};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};

pub(crate) fn runs_para(runs: Vec<Run>) -> Paragraph {
    Paragraph::with_runs(runs, ParaShapeIndex::new(0))
}

pub(crate) fn text_para(text: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}

pub(crate) fn text_run(text: &str) -> Run {
    Run::text(text, CharShapeIndex::new(0))
}

pub(crate) fn control_run(control: Control) -> Run {
    Run::control(control, CharShapeIndex::new(0))
}

pub(crate) fn end_field(id: &str) -> Run {
    control_run(Control::FieldBoundary {
        boundary: FieldBoundary::End { begin_id: id.into(), field_id: String::new() },
    })
}
