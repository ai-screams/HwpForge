use hwpforge_core::{PageSettings, Paragraph, Run, Section};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
pub fn p(text: &str, cs: u32, ps: u32) -> Paragraph {
    Paragraph::with_runs(
        vec![Run::text(text, CharShapeIndex::new(cs as usize))],
        ParaShapeIndex::new(ps as usize),
    )
}
pub fn section(paragraphs: Vec<Paragraph>) -> Section {
    Section::with_paragraphs(paragraphs, PageSettings::a4())
}
