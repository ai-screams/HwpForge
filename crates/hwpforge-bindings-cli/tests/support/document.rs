#![allow(dead_code)]

use hwpforge_core::{Paragraph, Run};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};

pub(crate) fn text_para(text: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}
