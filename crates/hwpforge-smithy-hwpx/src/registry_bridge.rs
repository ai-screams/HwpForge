//! Explicit bridge for registry-backed encode paths.
//!
//! [`HwpxStyleStore::from_registry`](crate::HwpxStyleStore::from_registry) builds a
//! store-local HWPX style table with injected defaults. Core documents produced
//! from Blueprint/Markdown still carry registry-local char/para shape indices.
//! This bridge records the actual store-local mappings and rebinds those
//! registry-local indices before encode.

use hwpforge_blueprint::registry::StyleRegistry;
use hwpforge_core::document::{Document, Draft, Validated};
use hwpforge_core::image::Image;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::RunContent;
use hwpforge_core::section::Section;
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};

use crate::default_styles::HancomStyleSet;
use crate::error::{HwpxError, HwpxResult};
use crate::style_store::{HwpxStyleStore, RegistryStoreBuild};

/// Controls whether list heads follow template spacing or retain effective values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ListHeadPolicy {
    /// Keeps the existing outline definitions and template marker widths.
    Template,
    /// Reserves a numberless outline and leaves ordinary marker metrics intact.
    ///
    /// Every registry numbering is treated as an ordinary list, so a template
    /// registry whose first numbering is an outline should use [`Self::Template`].
    PreserveEffective,
}

/// A registry projection diagnostic, aggregated by variant and font slot.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RegistryBridgeWarning {
    /// An explicitly requested font was absent from the nonempty font table.
    FontFallback {
        /// Requested name from the first occurrence in this slot.
        requested_font: String,
        /// `font`, `latin`, or `east_asian`; inherited empty slots are not counted.
        slot: &'static str,
        /// Zero-based registry character-shape index of the first occurrence.
        char_shape: usize,
        /// Total occurrences in this slot, including the first one.
        count: u64,
    },
}

impl RegistryBridgeWarning {
    /// Stable diagnostic code, independent of the requested name and location.
    pub fn code(&self) -> &'static str {
        match self {
            Self::FontFallback { .. } => "FONT_FALLBACK",
        }
    }

    /// Stable classification of the requested font slot that was lost.
    pub fn loss(&self) -> &'static str {
        match self {
            Self::FontFallback { slot: "latin", .. } => "font_latin",
            Self::FontFallback { slot: "east_asian", .. } => "font_east_asian",
            Self::FontFallback { .. } => "font",
        }
    }

    /// Stable classification of the actual replacement.
    pub fn action(&self) -> &'static str {
        match self {
            Self::FontFallback { .. } => "use_first_font",
        }
    }

    /// First registry location; no source package part is inferred.
    pub fn location(&self) -> Option<String> {
        match self {
            Self::FontFallback { char_shape, slot, .. } => {
                Some(format!("char_shapes[{char_shape}].{slot}"))
            }
        }
    }

    /// Total occurrences already aggregated by the bridge.
    pub fn count(&self) -> u64 {
        match self {
            Self::FontFallback { count, .. } => *count,
        }
    }

    /// First example and the number of omitted later locations.
    pub fn message(&self) -> String {
        let first = match self {
            Self::FontFallback { requested_font, .. } => format!(
                "Requested font '{}' is absent; using the first registered font",
                quote_font(requested_font)
            ),
        };
        if self.count() == 1 {
            first
        } else {
            format!(
                "{first} (total {} occurrences; first example only; subsequent locations omitted)",
                self.count()
            )
        }
    }
}

// Bound diagnostic names by Unicode characters before copying them.
fn quote_font(value: &str) -> String {
    match value.char_indices().nth(512) {
        Some((end, _)) => format!("{}…", &value[..end]),
        None => value.to_owned(),
    }
}

pub(crate) fn record_font_fallback(
    warnings: &mut Vec<RegistryBridgeWarning>,
    requested_font: &str,
    slot: &'static str,
    char_shape: usize,
) -> HwpxResult<()> {
    for warning in warnings.iter_mut() {
        match warning {
            RegistryBridgeWarning::FontFallback { slot: existing, count, .. }
                if *existing == slot =>
            {
                *count = count.checked_add(1).ok_or_else(|| HwpxError::InvalidStructure {
                    detail: "font fallback count overflow".into(),
                })?;
                return Ok(());
            }
            _ => {}
        }
    }
    warnings.push(RegistryBridgeWarning::FontFallback {
        requested_font: requested_font.into(),
        slot,
        char_shape,
        count: 1,
    });
    Ok(())
}

/// Bridge that converts registry-local style indices into store-local HWPX ids.
#[derive(Debug, Clone)]
pub struct HwpxRegistryBridge {
    style_store: HwpxStyleStore,
    char_shape_map: Vec<CharShapeIndex>,
    para_shape_map: Vec<ParaShapeIndex>,
    border_fill_map: Option<Vec<u32>>,
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
        Ok(Self::from_registry_with_diagnostics(registry, style_set, ListHeadPolicy::Template)?
            .value)
    }

    /// Builds a bridge and reports actual font substitutions during projection.
    ///
    /// Warnings retain the first registry character-shape index and requested
    /// name per font slot, with the total number of substitutions in that slot.
    ///
    /// # Errors
    /// Returns an error for invalid registry references or an overflowing
    /// warning count. Effective lists must not reference the reserved outline.
    pub fn from_registry_with_diagnostics(
        registry: &StyleRegistry,
        style_set: HancomStyleSet,
        list_policy: ListHeadPolicy,
    ) -> HwpxResult<crate::WithBridgeWarnings<Self>> {
        let RegistryStoreBuild { store, char_shape_map, para_shape_map, warnings } =
            HwpxStyleStore::from_registry_with_mappings(registry, style_set, list_policy)?;
        Ok(crate::WithBridgeWarnings {
            value: Self {
                style_store: store,
                char_shape_map,
                para_shape_map,
                border_fill_map: None,
            },
            warnings,
        })
    }

    /// Builds an import bridge with effective lists and registry-local table borders.
    ///
    /// Every explicit table/cell `border_fill_id` must be a one-based reference
    /// registered in `registry`. Rebinding maps it to the actual store position
    /// exactly once, including nested tables, captions, headers and footers.
    /// `None` remains an absent reference, not a borderless definition.
    /// The caller must keep the document and registry paired: raw store IDs
    /// cannot be distinguished from local IDs with the same numeric value.
    /// Existing constructors retain their raw border-reference semantics.
    ///
    /// # Errors
    /// Returns an error for invalid registry references, unsupported character
    /// or paragraph border references, or overflowing diagnostic counts.
    /// Rebinding rejects unregistered table/cell references and explicit page
    /// or cell-zone borders, which this import contract does not yet support.
    pub fn from_registry_for_import(
        registry: &StyleRegistry,
        style_set: HancomStyleSet,
    ) -> HwpxResult<crate::WithBridgeWarnings<Self>> {
        let RegistryStoreBuild { mut store, char_shape_map, para_shape_map, warnings } =
            HwpxStyleStore::from_registry_import_mappings(registry, style_set)?;
        let mut border_fill_map = Vec::new();
        let mut id = 1;
        while let Some(value) = registry.border_fill(id) {
            let mut fill = crate::style_store::HwpxBorderFill::default_table_border();
            fill.diagonal = None;
            for (source, target) in [
                (&value.border.left, &mut fill.left),
                (&value.border.right, &mut fill.right),
                (&value.border.top, &mut fill.top),
                (&value.border.bottom, &mut fill.bottom),
            ] {
                if source.line_type == hwpforge_foundation::BorderLineType::None {
                    target.line_type = "NONE".into();
                }
            }
            border_fill_map.push(store.push_border_fill(fill));
            id += 1;
        }
        Ok(crate::WithBridgeWarnings {
            value: Self {
                style_store: store,
                char_shape_map,
                para_shape_map,
                border_fill_map: Some(border_fill_map),
            },
            warnings,
        })
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
    /// All constructors remap these shared-IR axes:
    /// - `Paragraph.para_shape_id`
    /// - `Run.char_shape_id`
    ///
    /// Bridges made with [`Self::from_registry_for_import`] additionally remap
    /// every table/cell border reference from the paired registry. Other
    /// constructors leave raw table/cell border references unchanged.
    ///
    /// Style references retain their document identity; the store resolves them
    /// to the selected default set's definition positions.
    ///
    /// # Errors
    ///
    /// Nesting depth above 32 returns [`crate::HwpxError::Core`] containing
    /// `CoreError::InvalidStructure` with `nesting_depth` context. Retrying
    /// gives the same rejection; reduce nesting before retrying.
    pub fn rebind_draft_document(
        &self,
        mut document: Document<Draft>,
    ) -> HwpxResult<Document<Draft>> {
        // 이것을 실패시키는 것: rebind_draft_document의 깊이 관문 삭제.
        for (si, section) in document.sections().iter().enumerate() {
            crate::depth::check_section_depth(section, si)?;
        }

        if let Some(map) = &self.border_fill_map {
            if document.sections().iter().any(|section| section.page_border_fills.is_some()) {
                return Err(HwpxError::InvalidStructure {
                    detail: "import page border references are unsupported".into(),
                });
            }
            let mut error = None;
            document.for_each_table_mut(|table| {
                if error.is_some() {
                    return;
                }
                let result = (|| {
                    if !table.cell_zones.is_empty() {
                        return Err(HwpxError::InvalidStructure {
                            detail: "import cell-zone border references are unsupported".into(),
                        });
                    }
                    rebind_border(&mut table.border_fill_id, map)?;
                    for row in &mut table.rows {
                        for cell in &mut row.cells {
                            rebind_border(&mut cell.border_fill_id, map)?;
                        }
                    }
                    Ok(())
                })();
                error = result.err();
            });
            if let Some(error) = error {
                return Err(error);
            }
        }
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
        // Each `<hp:header>` / `<hp:footer>` is independently
        // rebound to keep all multi-cardinality variants consistent.
        for header in section.headers.iter_mut() {
            self.rebind_paragraphs(&mut header.paragraphs)?;
        }
        for footer in section.footers.iter_mut() {
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
            self.rebind_paragraph_tree(paragraph)?;
        }
        Ok(())
    }

    /// Rebinds `paragraph` and every paragraph nested in it, through Core's
    /// one paragraph traversal ([`Paragraph::for_each_paragraph_mut`]) — so a
    /// container Core walks (group children, nested groups, …) cannot be
    /// missed here (the group arm used to be skipped).
    /// That traversal does not enter run pictures' captions; those are
    /// rebound from their run ([`Self::rebind_own`]). Each paragraph is
    /// visited once — the mapping is not idempotent.
    fn rebind_paragraph_tree(&self, paragraph: &mut Paragraph) -> HwpxResult<()> {
        let mut first_error = None;
        paragraph.for_each_paragraph_mut(|p| {
            if first_error.is_none() {
                first_error = self.rebind_own(p).err();
            }
        });
        first_error.map_or(Ok(()), Err)
    }

    /// The style references of `paragraph` itself and its runs — not of the
    /// paragraphs nested in them, which the traversal visits on its own.
    fn rebind_own(&self, paragraph: &mut Paragraph) -> HwpxResult<()> {
        self.style_store.effective_style_position(paragraph.style_ref)?;
        paragraph.para_shape_id = self.map_para_shape(paragraph.para_shape_id)?;
        // The end mark's char shape is a char shape reference too.
        // 이것을 실패시키는 것: 이 다시 매기기를 지우기
        // (`end_char_shape_is_rebound_in_a_cell`).
        if let Some(end) = paragraph.end_char_shape {
            paragraph.end_char_shape = Some(self.map_char_shape(end)?);
        }
        for run in &mut paragraph.runs {
            run.char_shape_id = self.map_char_shape(run.char_shape_id)?;
            if let RunContent::Image(Image { caption: Some(caption), .. }) = &mut run.content {
                self.rebind_paragraphs(&mut caption.paragraphs)?;
            }
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

fn rebind_border(reference: &mut Option<u32>, map: &[u32]) -> HwpxResult<()> {
    if let Some(id) = reference {
        *id = id.checked_sub(1).and_then(|index| map.get(index as usize)).copied().ok_or_else(
            || HwpxError::InvalidStructure {
                detail: format!("unregistered import border reference {id}"),
            },
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    // 이것을 실패시키는 것: 글꼴 인용 제한 제거, 512 경계 변경, 바이트 단위 절단, 생략 기호 제거.
    #[test]
    fn font_messages_bound_unicode_names_and_keep_aggregate_details() {
        for (length, expected_name) in [
            (511, concat!(
                "가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가",
                "가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가",
                "가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가",
                "가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가",
                "가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가",
                "가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가",
                "가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가",
                "가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가",
            )),
            (512, concat!(
                "가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가",
                "가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가",
                "가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가",
                "가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가",
                "가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가",
                "가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가",
                "가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가",
                "가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가",
            )),
            (513, concat!(
                "가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가",
                "가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가",
                "가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가",
                "가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가",
                "가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가",
                "가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가",
                "가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가",
                "가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가가",
                "…",
            )),
        ] {
            for (count, suffix) in [(1, ""), (2, " (total 2 occurrences; first example only; subsequent locations omitted)")] {
                let warning = RegistryBridgeWarning::FontFallback {
                    requested_font: "가".repeat(length), slot: "font", char_shape: 0, count,
                };
                assert_eq!(warning.message(), format!("Requested font '{expected_name}' is absent; using the first registered font{suffix}"));
                assert_eq!(warning.count(), count);
            }
        }
    }

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
    use hwpforge_foundation::{ApplyPageType, HwpUnit, StyleRef};

    #[test]
    fn fallback_count_overflow_uses_the_existing_error_path() {
        // 이것을 실패시키는 것: checked_add를 wrapping_add 또는 saturating_add로 바꾸기.
        let mut warnings = vec![RegistryBridgeWarning::FontFallback {
            requested_font: "First".into(),
            slot: "font",
            char_shape: 2,
            count: u64::MAX,
        }];
        let error = record_font_fallback(&mut warnings, "Later", "font", 3).unwrap_err();
        assert!(matches!(error, HwpxError::InvalidStructure { .. }));
    }

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

        let mut root = simple_paragraph(body_cs, body_ps).with_style(StyleRef::new(2));
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
                frame: hwpforge_core::ObjectFrame::new(),
                text_margin: None,
                paragraphs: vec![simple_paragraph(body_cs, body_ps)],
                width: HwpUnit::from_mm(20.0).unwrap(),
                height: HwpUnit::from_mm(10.0).unwrap(),
                placement: None,
                caption: Some(Caption::new(
                    vec![simple_paragraph(body_cs, body_ps)],
                    CaptionSide::Bottom,
                )),
                style: None,
                text_vertical_align: hwpforge_foundation::VerticalAlign::Top,
            },
            body_cs,
        ));
        root.add_run(Run::control(
            Control::memo(vec![simple_paragraph(body_cs, body_ps)]),
            body_cs,
        ));

        let mut section = Section::with_paragraphs(vec![root], PageSettings::a4());
        section.headers.push(HeaderFooter::all_pages(vec![simple_paragraph(body_cs, body_ps)]));
        section.footers.push(HeaderFooter::all_pages(vec![simple_paragraph(body_cs, body_ps)]));
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
        assert_eq!(root.style_ref, Some(StyleRef::new(2)));
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

        assert_eq!(section.headers.first().unwrap().paragraphs[0].para_shape_id, expected_para);
        assert_eq!(section.footers.first().unwrap().paragraphs[0].para_shape_id, expected_para);
        assert_eq!(
            section.master_pages.as_ref().unwrap()[0].paragraphs[0].para_shape_id,
            expected_para
        );
    }

    /// A paragraph's end-mark char shape is rebound like its
    /// runs', also in a table cell.
    ///
    /// 이것을 실패시키는 것: `end_char_shape` 다시 매기기를 지우기.
    #[test]
    fn end_char_shape_is_rebound_in_a_cell() {
        let (registry, body_cs, body_ps) = body_registry();
        let bridge = HwpxRegistryBridge::from_registry(&registry).unwrap();
        let expected = bridge.char_shape_map[body_cs.get()];
        assert_ne!(expected, body_cs, "the test needs a remapped id");
        let mut cell_para = simple_paragraph(body_cs, body_ps);
        cell_para.end_char_shape = Some(body_cs);
        let table = Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![cell_para],
            HwpUnit::from_mm(30.0).unwrap(),
        )])]);
        let mut root = simple_paragraph(body_cs, body_ps);
        root.add_run(Run::table(table, body_cs));
        let mut document = Document::new();
        document.add_section(Section::with_paragraphs(vec![root], PageSettings::a4()));
        let rebound = bridge.rebind_draft_document(document).unwrap();
        let table = rebound.sections()[0].paragraphs[0].runs[1].content.as_table().unwrap();
        assert_eq!(table.rows[0].cells[0].paragraphs[0].end_char_shape, Some(expected));
    }

    /// A group's picture caption and a text box inside a
    /// nested group are rebound too (char shape, end mark and para shape).
    /// 이것을 실패시키는 것: 묶음을 건너뛰기 · 중첩 묶음으로 내려가지 않기 ·
    /// 같은 문단을 두 번 다시 매기기(매핑이 항등이 아니라 값이 어긋난다).
    #[test]
    fn group_children_are_rebound() {
        use hwpforge_core::control::GroupChild;
        let (registry, body_cs, body_ps) = body_registry();
        let bridge = HwpxRegistryBridge::from_registry(&registry).unwrap();
        let expected_char = bridge.char_shape_map[body_cs.get()];
        let expected_para = bridge.para_shape_map[body_ps.get()];
        assert_ne!(expected_char, body_cs, "the test needs a remapped id");
        let mm = |v: f64| HwpUnit::from_mm(v).unwrap();
        let mut captioned = simple_paragraph(body_cs, body_ps);
        captioned.end_char_shape = Some(body_cs);
        let picture = Image::new("BinData/image1.png", mm(10.0), mm(10.0), ImageFormat::Png)
            .with_caption(Caption::new(vec![captioned], CaptionSide::Bottom));
        let group = |children| Control::Group {
            frame: hwpforge_core::ObjectFrame::new(),
            children,
            width: mm(20.0),
            height: mm(20.0),
            placement: None,
            inst_id: None,
        };
        let inner = group(vec![GroupChild::Shape(Control::text_box(
            vec![simple_paragraph(body_cs, body_ps)],
            mm(10.0),
            mm(10.0),
        ))]);
        let outer = group(vec![GroupChild::Picture(picture), GroupChild::Shape(inner)]);
        let mut root = simple_paragraph(body_cs, body_ps);
        root.add_run(Run::control(outer, body_cs));
        let mut document = Document::new();
        document.add_section(Section::with_paragraphs(vec![root], PageSettings::a4()));

        let rebound = bridge.rebind_draft_document(document).unwrap();
        let Some(Control::Group { children, .. }) =
            rebound.sections()[0].paragraphs[0].runs[1].content.as_control()
        else {
            panic!("group");
        };
        let GroupChild::Picture(picture) = &children[0] else { panic!("picture") };
        let caption = &picture.caption.as_ref().unwrap().paragraphs[0];
        assert_eq!(caption.para_shape_id, expected_para);
        assert_eq!(caption.runs[0].char_shape_id, expected_char);
        assert_eq!(caption.end_char_shape, Some(expected_char));
        let GroupChild::Shape(Control::Group { children: nested, .. }) = &children[1] else {
            panic!("nested group")
        };
        let GroupChild::Shape(Control::TextBox { paragraphs, .. }) = &nested[0] else {
            panic!("text box")
        };
        assert_eq!(paragraphs[0].para_shape_id, expected_para);
        assert_eq!(paragraphs[0].runs[0].char_shape_id, expected_char);
    }

    #[test]
    fn rebind_validated_document_preserves_structure() {
        let (registry, body_cs, body_ps) = body_registry();
        let bridge = HwpxRegistryBridge::from_registry(&registry).unwrap();

        let mut draft = Document::new();
        draft.add_section(Section::with_paragraphs(
            vec![simple_paragraph(body_cs, body_ps).with_style(StyleRef::new(2))],
            PageSettings::a4(),
        ));
        let validated = draft.validate().unwrap();

        let rebound = bridge.rebind_validated_document(&validated).unwrap();
        assert_eq!(rebound.section_count(), 1);
        assert_eq!(rebound.sections()[0].paragraphs[0].style_ref, Some(StyleRef::new(2)));
        assert_eq!(
            rebound.sections()[0].paragraphs[0].para_shape_id,
            bridge.para_shape_map[body_ps.get()]
        );
    }

    #[test]
    fn registry_bridge_preserves_builtin_style_id_space() {
        let (mut registry, _, _) = body_registry();
        let body = *registry.get_style("body").unwrap();
        registry
            .register_style(
                "custom_body_copy".to_string(),
                body.char_shape_id,
                body.para_shape_id,
                body.font_id,
            )
            .unwrap();

        let bridge = HwpxRegistryBridge::from_registry(&registry).unwrap();

        assert_eq!(
            bridge.style_store().style_name(bridge.style_store().style_ref_at(2).unwrap()),
            Some("개요 1")
        );
        assert_eq!(
            bridge.style_store().style_name(registry.get_style("body").unwrap().style_ref()),
            Some("body")
        );
        assert_eq!(
            bridge
                .style_store()
                .style_name(registry.get_style("custom_body_copy").unwrap().style_ref()),
            Some("custom_body_copy")
        );
    }
}
