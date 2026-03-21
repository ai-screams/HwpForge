//! Explicit bridge for registry-backed encode paths.
//!
//! [`HwpxStyleStore::from_registry`](crate::HwpxStyleStore::from_registry) builds a
//! store-local HWPX style table with injected defaults. Core documents produced
//! from Blueprint/Markdown still carry registry-local char/para shape indices.
//! This bridge records the actual store-local mappings and rebinds those
//! registry-local indices before encode.

use hwpforge_blueprint::registry::StyleRegistry;
use hwpforge_core::caption::Caption;
use hwpforge_core::control::Control;
use hwpforge_core::document::{Document, Draft, Validated};
use hwpforge_core::image::Image;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::section::Section;
use hwpforge_core::table::Table;
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};

use crate::default_styles::HancomStyleSet;
use crate::error::{HwpxError, HwpxResult};
use crate::style_store::{HwpxStyleStore, RegistryStoreBuild};

/// Bridge that converts registry-local style indices into store-local HWPX ids.
#[derive(Debug, Clone)]
pub struct HwpxRegistryBridge {
    style_store: HwpxStyleStore,
    char_shape_map: Vec<CharShapeIndex>,
    para_shape_map: Vec<ParaShapeIndex>,
}

impl HwpxRegistryBridge {
    /// Builds a bridge using the default 한컴 style set.
    pub fn from_registry(registry: &StyleRegistry) -> HwpxResult<Self> {
        Self::from_registry_with(registry, HancomStyleSet::default())
    }

    /// Builds a bridge using a specific 한컴 style set.
    pub fn from_registry_with(
        registry: &StyleRegistry,
        style_set: HancomStyleSet,
    ) -> HwpxResult<Self> {
        let RegistryStoreBuild { store, char_shape_map, para_shape_map } =
            HwpxStyleStore::from_registry_with_mappings(registry, style_set)?;
        Ok(Self { style_store: store, char_shape_map, para_shape_map })
    }

    /// Returns the built HWPX style store.
    pub fn style_store(&self) -> &HwpxStyleStore {
        &self.style_store
    }

    /// Consumes the bridge and returns the underlying style store.
    pub fn into_style_store(self) -> HwpxStyleStore {
        self.style_store
    }

    /// Rebinds a registry-backed draft document into store-local ids.
    ///
    /// This remaps only the unambiguous shared-IR axes:
    /// - `Paragraph.para_shape_id`
    /// - `Run.char_shape_id`
    ///
    /// Paragraph `style_id` is intentionally left untouched because current
    /// Core producers store direct HWPX `styleIDRef` values there already.
    /// HWPX/HWP5 decode round-trip style ids verbatim, Markdown headings target
    /// the built-in Hancom style ids, and registry/template rendering does not
    /// assign registry-local custom style indices to `Paragraph.style_id`.
    pub fn rebind_draft_document(
        &self,
        mut document: Document<Draft>,
    ) -> HwpxResult<Document<Draft>> {
        self.rebind_sections(document.sections_mut())?;
        Ok(document)
    }

    /// Rebinds a validated document by cloning its structure into a draft,
    /// remapping style indices, and validating again.
    pub fn rebind_validated_document(
        &self,
        document: &Document<Validated>,
    ) -> HwpxResult<Document<Validated>> {
        let mut draft = Document::with_metadata(document.metadata().clone());
        for section in document.sections().iter().cloned() {
            draft.add_section(section);
        }
        self.rebind_draft_document(draft)?.validate().map_err(Into::into)
    }

    fn rebind_sections(&self, sections: &mut [Section]) -> HwpxResult<()> {
        for section in sections {
            self.rebind_section(section)?;
        }
        Ok(())
    }

    fn rebind_section(&self, section: &mut Section) -> HwpxResult<()> {
        self.rebind_paragraphs(&mut section.paragraphs)?;
        if let Some(header) = section.header.as_mut() {
            self.rebind_paragraphs(&mut header.paragraphs)?;
        }
        if let Some(footer) = section.footer.as_mut() {
            self.rebind_paragraphs(&mut footer.paragraphs)?;
        }
        if let Some(master_pages) = section.master_pages.as_mut() {
            for master_page in master_pages {
                self.rebind_paragraphs(&mut master_page.paragraphs)?;
            }
        }
        Ok(())
    }

    fn rebind_paragraphs(&self, paragraphs: &mut [Paragraph]) -> HwpxResult<()> {
        for paragraph in paragraphs {
            self.rebind_paragraph(paragraph)?;
        }
        Ok(())
    }

    fn rebind_paragraph(&self, paragraph: &mut Paragraph) -> HwpxResult<()> {
        paragraph.para_shape_id = self.map_para_shape(paragraph.para_shape_id)?;
        for run in &mut paragraph.runs {
            self.rebind_run(run)?;
        }
        Ok(())
    }

    fn rebind_run(&self, run: &mut Run) -> HwpxResult<()> {
        run.char_shape_id = self.map_char_shape(run.char_shape_id)?;
        match &mut run.content {
            RunContent::Table(table) => self.rebind_table(table)?,
            RunContent::Image(image) => self.rebind_image(image)?,
            RunContent::Control(control) => self.rebind_control(control)?,
            RunContent::Text(_) => {}
            _ => {}
        }
        Ok(())
    }

    fn rebind_table(&self, table: &mut Table) -> HwpxResult<()> {
        for row in &mut table.rows {
            for cell in &mut row.cells {
                self.rebind_paragraphs(&mut cell.paragraphs)?;
            }
        }
        self.rebind_caption(table.caption.as_mut())
    }

    fn rebind_image(&self, image: &mut Image) -> HwpxResult<()> {
        self.rebind_caption(image.caption.as_mut())
    }

    fn rebind_control(&self, control: &mut Control) -> HwpxResult<()> {
        match control {
            Control::TextBox { paragraphs, caption, .. } => {
                self.rebind_paragraphs(paragraphs)?;
                self.rebind_caption(caption.as_mut())?;
            }
            Control::Footnote { paragraphs, .. } | Control::Endnote { paragraphs, .. } => {
                self.rebind_paragraphs(paragraphs)?;
            }
            Control::Ellipse { paragraphs, caption, .. }
            | Control::Polygon { paragraphs, caption, .. } => {
                self.rebind_paragraphs(paragraphs)?;
                self.rebind_caption(caption.as_mut())?;
            }
            Control::Line { caption, .. }
            | Control::Arc { caption, .. }
            | Control::Curve { caption, .. }
            | Control::ConnectLine { caption, .. } => {
                self.rebind_caption(caption.as_mut())?;
            }
            Control::Memo { content, .. } => self.rebind_paragraphs(content)?,
            Control::Hyperlink { .. }
            | Control::Equation { .. }
            | Control::Chart { .. }
            | Control::Dutmal { .. }
            | Control::Compose { .. }
            | Control::Bookmark { .. }
            | Control::CrossRef { .. }
            | Control::Field { .. }
            | Control::IndexMark { .. }
            | Control::Unknown { .. } => {}
            _ => {}
        }
        Ok(())
    }

    fn rebind_caption(&self, caption: Option<&mut Caption>) -> HwpxResult<()> {
        if let Some(caption) = caption {
            self.rebind_paragraphs(&mut caption.paragraphs)?;
        }
        Ok(())
    }

    fn map_char_shape(&self, index: CharShapeIndex) -> HwpxResult<CharShapeIndex> {
        self.char_shape_map.get(index.get()).copied().ok_or(HwpxError::IndexOutOfBounds {
            kind: "registry char_shape",
            index: index.get() as u32,
            max: self.char_shape_map.len() as u32,
        })
    }

    fn map_para_shape(&self, index: ParaShapeIndex) -> HwpxResult<ParaShapeIndex> {
        self.para_shape_map.get(index.get()).copied().ok_or(HwpxError::IndexOutOfBounds {
            kind: "registry para_shape",
            index: index.get() as u32,
            max: self.para_shape_map.len() as u32,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_blueprint::builtins::builtin_default;
    use hwpforge_blueprint::registry::StyleRegistry;
    use hwpforge_core::caption::{Caption, CaptionSide};
    use hwpforge_core::control::Control;
    use hwpforge_core::document::Document;
    use hwpforge_core::image::{Image, ImageFormat};
    use hwpforge_core::paragraph::Paragraph;
    use hwpforge_core::run::Run;
    use hwpforge_core::section::{HeaderFooter, MasterPage, Section};
    use hwpforge_core::table::{Table, TableCell, TableRow};
    use hwpforge_core::PageSettings;
    use hwpforge_core::StyleLookup;
    use hwpforge_foundation::{ApplyPageType, HwpUnit, StyleIndex};

    fn body_registry() -> (StyleRegistry, CharShapeIndex, ParaShapeIndex) {
        let template = builtin_default().unwrap();
        let registry = StyleRegistry::from_template(&template).unwrap();
        let body = *registry.get_style("body").unwrap();
        (registry, body.char_shape_id, body.para_shape_id)
    }

    fn simple_paragraph(char_shape_id: CharShapeIndex, para_shape_id: ParaShapeIndex) -> Paragraph {
        Paragraph::with_runs(vec![Run::text("body", char_shape_id)], para_shape_id)
    }

    #[test]
    fn rebind_draft_document_remaps_nested_registry_indices() {
        let (registry, body_cs, body_ps) = body_registry();
        let bridge = HwpxRegistryBridge::from_registry(&registry).unwrap();
        let expected_char = bridge.char_shape_map[body_cs.get()];
        let expected_para = bridge.para_shape_map[body_ps.get()];

        let mut root = simple_paragraph(body_cs, body_ps).with_style(StyleIndex::new(2));
        let table = Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![simple_paragraph(body_cs, body_ps)],
            HwpUnit::from_mm(30.0).unwrap(),
        )])])
        .with_caption(Caption::new(vec![simple_paragraph(body_cs, body_ps)], CaptionSide::Bottom));
        root.add_run(Run::table(table, body_cs));
        root.add_run(Run::image(
            Image::new(
                "BinData/image1.png",
                HwpUnit::from_mm(10.0).unwrap(),
                HwpUnit::from_mm(10.0).unwrap(),
                ImageFormat::Png,
            )
            .with_caption(Caption::new(
                vec![simple_paragraph(body_cs, body_ps)],
                CaptionSide::Bottom,
            )),
            body_cs,
        ));
        root.add_run(Run::control(
            Control::TextBox {
                paragraphs: vec![simple_paragraph(body_cs, body_ps)],
                width: HwpUnit::from_mm(20.0).unwrap(),
                height: HwpUnit::from_mm(10.0).unwrap(),
                horz_offset: 0,
                vert_offset: 0,
                caption: Some(Caption::new(
                    vec![simple_paragraph(body_cs, body_ps)],
                    CaptionSide::Bottom,
                )),
                style: None,
            },
            body_cs,
        ));
        root.add_run(Run::control(
            Control::memo(vec![simple_paragraph(body_cs, body_ps)], "tester", "2026-03-20"),
            body_cs,
        ));

        let mut section = Section::with_paragraphs(vec![root], PageSettings::a4());
        section.header = Some(HeaderFooter::all_pages(vec![simple_paragraph(body_cs, body_ps)]));
        section.footer = Some(HeaderFooter::all_pages(vec![simple_paragraph(body_cs, body_ps)]));
        section.master_pages = Some(vec![MasterPage::new(
            ApplyPageType::Both,
            vec![simple_paragraph(body_cs, body_ps)],
        )]);

        let mut document = Document::new();
        document.add_section(section);

        let rebound = bridge.rebind_draft_document(document).unwrap();
        let section = &rebound.sections()[0];
        let root = &section.paragraphs[0];
        assert_eq!(root.para_shape_id, expected_para);
        assert_eq!(root.style_id, Some(StyleIndex::new(2)));
        assert_eq!(root.runs[0].char_shape_id, expected_char);

        let table_run = root.runs[1].content.as_table().unwrap();
        assert_eq!(table_run.rows[0].cells[0].paragraphs[0].para_shape_id, expected_para);
        assert_eq!(table_run.rows[0].cells[0].paragraphs[0].runs[0].char_shape_id, expected_char);
        assert_eq!(table_run.caption.as_ref().unwrap().paragraphs[0].para_shape_id, expected_para);

        let image_run = root.runs[2].content.as_image().unwrap();
        assert_eq!(image_run.caption.as_ref().unwrap().paragraphs[0].para_shape_id, expected_para);

        match root.runs[3].content.as_control().unwrap() {
            Control::TextBox { paragraphs, caption, .. } => {
                assert_eq!(paragraphs[0].para_shape_id, expected_para);
                assert_eq!(caption.as_ref().unwrap().paragraphs[0].para_shape_id, expected_para);
            }
            other => panic!("expected textbox, got {other:?}"),
        }
        match root.runs[4].content.as_control().unwrap() {
            Control::Memo { content, .. } => {
                assert_eq!(content[0].para_shape_id, expected_para);
            }
            other => panic!("expected memo, got {other:?}"),
        }

        assert_eq!(section.header.as_ref().unwrap().paragraphs[0].para_shape_id, expected_para);
        assert_eq!(section.footer.as_ref().unwrap().paragraphs[0].para_shape_id, expected_para);
        assert_eq!(
            section.master_pages.as_ref().unwrap()[0].paragraphs[0].para_shape_id,
            expected_para
        );
    }

    #[test]
    fn rebind_validated_document_preserves_structure() {
        let (registry, body_cs, body_ps) = body_registry();
        let bridge = HwpxRegistryBridge::from_registry(&registry).unwrap();

        let mut draft = Document::new();
        draft.add_section(Section::with_paragraphs(
            vec![simple_paragraph(body_cs, body_ps).with_style(StyleIndex::new(2))],
            PageSettings::a4(),
        ));
        let validated = draft.validate().unwrap();

        let rebound = bridge.rebind_validated_document(&validated).unwrap();
        assert_eq!(rebound.section_count(), 1);
        assert_eq!(rebound.sections()[0].paragraphs[0].style_id, Some(StyleIndex::new(2)));
        assert_eq!(
            rebound.sections()[0].paragraphs[0].para_shape_id,
            bridge.para_shape_map[body_ps.get()]
        );
    }

    #[test]
    fn registry_bridge_preserves_builtin_style_id_space() {
        let (mut registry, _, _) = body_registry();
        let body = *registry.get_style("body").unwrap();
        registry.style_entries.insert("custom-body-copy".to_string(), body);

        let bridge = HwpxRegistryBridge::from_registry(&registry).unwrap();
        let default_style_count = HancomStyleSet::default().default_styles().len();
        let custom_style_id = default_style_count + registry.style_entries.len() - 1;

        assert_eq!(bridge.style_store().style_name(StyleIndex::new(2)), Some("개요 1"));
        assert_eq!(
            bridge.style_store().style_name(StyleIndex::new(default_style_count)),
            Some("body")
        );
        assert_eq!(
            bridge.style_store().style_name(StyleIndex::new(custom_style_id)),
            Some("custom-body-copy")
        );
    }
}
