#![allow(dead_code)]

use hwpforge_core::{Paragraph, Run};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};

pub(crate) fn text(s: &str) -> Run {
    Run::text(s, CharShapeIndex::new(0))
}

pub(crate) fn para(runs: Vec<Run>) -> Paragraph {
    Paragraph::with_runs(runs, ParaShapeIndex::new(0))
}
