//! `inspect` — structural summary of an HWPX document.
//!
//! This is the template every other operation follows: bytes in, a
//! `#[non_exhaustive]` output struct out, no file I/O, warnings carried
//! beside the payload instead of inside it.

use hwpforge_core::control::Control;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::RunContent;
use hwpforge_foundation::{CharShapeIndex, FieldType, FontIndex, ParaShapeIndex};
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxStyleStore};
use serde::{Deserialize, Serialize};

use super::walk;
use super::{OpsError, OpsWarning};

/// Options for [`inspect`].
///
/// Build with [`InspectOptions::default`] and the `with_*` setters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct InspectOptions {
    /// Whether to include the style summary (fonts, char shapes, para shapes).
    pub styles: bool,
}

impl InspectOptions {
    /// Includes (or omits) the style summary in the report.
    #[must_use]
    pub fn with_styles(mut self, styles: bool) -> Self {
        self.styles = styles;
        self
    }
}

/// What [`inspect`] returns: the wire report plus any warnings.
///
/// Like every operation output this struct is `#[non_exhaustive]` and does
/// not derive serde — [`InspectReport`] is the serialisable payload, and the
/// warnings become `WarningInfo` through [`OpsWarning::info`].
#[derive(Debug)]
#[non_exhaustive]
pub struct InspectOutput {
    /// The structural report.
    pub report: InspectReport,
    /// Non-fatal diagnostics raised while decoding.
    pub warnings: Vec<OpsWarning>,
}

/// Structural summary of a document — the `inspect` wire payload.
///
/// # Counting contract
///
/// `paragraphs`, `tables`, `images` and `charts` are **deep** counts: they
/// use Core's shared paragraph traversal, which visits body paragraphs,
/// table cells, text boxes, notes, memos, headers, footers and master
/// pages, and therefore counts content nested inside a table cell too. The
/// traversal deliberately does not descend into image captions. Per-section
/// `top_level_paragraphs` is the body-flow paragraph count of that section,
/// which is what the CLI and the MCP server report as `paragraphs` today;
/// [`InspectSection::top_level_tables`], `top_level_images` and
/// `top_level_charts` are the same top-level rule applied to tables,
/// images and charts — `Section::content_counts()`, matching what the CLI
/// and the pre-migration MCP server report as `tables`/`images`/`charts`
/// today. A table containing an image, or a footnote containing a table,
/// is where the two disagree: the deep fields see it, the top-level ones
/// do not — **except for `charts`**, where that disagreement is currently
/// vacuous: `hwpforge-smithy-hwpx`'s decoder only reconstructs a
/// `Control::Chart` for a section's own top-level paragraphs (the
/// `<hp:switch>`/`<hp:chart>` extraction lives in `decode_section`'s
/// top-level loop, not in the recursive per-container dispatch every other
/// nested content type goes through), so a chart nested anywhere — a table
/// cell, a caption, a text box — is invisible to `charts` and
/// `top_level_charts` alike; they read the same on such input. See
/// `hwpforge::ops::walk`'s module doc and its
/// `chart_nested_in_a_caption_is_a_documented_decoder_gap` test for the
/// reproduction. This is a known decoder gap, not something `inspect`
/// papers over: `charts`/`top_level_charts` still report exactly what the
/// decoded tree contains, they just cannot contain a nested chart today.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct InspectReport {
    /// Document metadata.
    pub metadata: InspectMetadata,
    /// Number of sections.
    pub sections: usize,
    /// Paragraphs the shared traversal visits, across the whole document.
    pub paragraphs: usize,
    /// Tables, nested ones included.
    pub tables: usize,
    /// Images, nested ones included.
    pub images: usize,
    /// Charts, nested ones included — except a chart nested anywhere but
    /// a section's own top-level paragraphs, which the decoder cannot
    /// currently reconstruct at all. See the struct's `# Counting
    /// contract` doc above.
    pub charts: usize,
    /// Names of the click-here fields, in document order, duplicates kept.
    pub fields: Vec<String>,
    /// Per-section summary, in document order.
    pub section_details: Vec<InspectSection>,
    /// Style summary; present only when
    /// [`InspectOptions::with_styles`] asked for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub styles: Option<InspectStyles>,
}

/// The metadata fields both frontends show today.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct InspectMetadata {
    /// Document title; empty when the document sets none.
    pub title: String,
    /// Document author; empty when the document sets none.
    pub author: String,
    /// Document subject; empty when the document sets none.
    pub subject: String,
    /// Free-text description; empty when the document sets none.
    pub description: String,
    /// Who saved the document last; empty when unknown.
    pub last_saved_by: String,
    /// Creation timestamp as the document stores it (ISO 8601), if any.
    pub created: Option<String>,
    /// Last-modified timestamp as the document stores it (ISO 8601), if any.
    pub modified: Option<String>,
    /// Keyword list; empty when the document sets none.
    pub keywords: Vec<String>,
}

/// One section's contribution to [`InspectReport`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct InspectSection {
    /// Section index, 0-based.
    pub index: usize,
    /// Body-flow paragraphs of this section.
    pub top_level_paragraphs: usize,
    /// Top-level tables in this section — `Section::content_counts()`,
    /// matching the CLI/MCP `inspect` contract (a table nested inside a
    /// table cell, note, header/footer or master page is not counted).
    /// Deep counts, nested ones included, are [`Self::tables`].
    ///
    /// `#[serde(default)]` (`0`): JSON written by hwpforge 0.16.5 or
    /// earlier has no `top_level_tables` key, from before this field
    /// existed. A defaulted `0` on such input is not a real "no tables"
    /// answer — [`Self::tables`] is the field that input still carries.
    #[serde(default)]
    pub top_level_tables: usize,
    /// Top-level images in this section — top-level only, the same rule
    /// [`Self::top_level_tables`] documents. Deep counts are
    /// [`Self::images`].
    ///
    /// `#[serde(default)]` (`0`): same older-writer absence as
    /// [`Self::top_level_tables`].
    #[serde(default)]
    pub top_level_images: usize,
    /// Top-level charts in this section — top-level only, the same rule
    /// [`Self::top_level_tables`] documents. Deep counts are
    /// [`Self::charts`] — though for charts specifically the two currently
    /// read the same on any input, since the decoder cannot reconstruct a
    /// nested chart at all (see [`InspectReport`]'s `# Counting contract`
    /// doc).
    ///
    /// `#[serde(default)]` (`0`): same older-writer absence as
    /// [`Self::top_level_tables`].
    #[serde(default)]
    pub top_level_charts: usize,
    /// Paragraphs the shared traversal visits inside this section.
    pub paragraphs: usize,
    /// Tables in this section, nested ones included.
    pub tables: usize,
    /// Images in this section, nested ones included.
    pub images: usize,
    /// Charts in this section, nested ones included — except one nested
    /// anywhere but this section's own top-level paragraphs, a decoder gap
    /// [`InspectReport`]'s `# Counting contract` doc explains.
    pub charts: usize,
    /// Whether the section defines a header.
    pub has_header: bool,
    /// Whether the section defines a footer.
    pub has_footer: bool,
    /// Whether the section defines a page number control.
    pub has_page_number: bool,

    // ── package-scope counts (W6b: single-decode CLI parity) ──────
    //
    // The nine fields below give the CLI's pre-migration local scanner
    // (`hwpforge-bindings-cli`'s `analysis/deep_counts.rs`) everything it
    // needs from this one decode, so it no longer has to decode the same
    // bytes a second time — except charts, which stay a raw scan in that
    // CLI (`hwpforge-bindings-cli/src/commands/inspect.rs`'s own doc
    // explains why: `hwpforge-smithy-hwpx`'s decoder only reconstructs
    // `Control::Chart` for a section's own top-level paragraphs, never for
    // one nested in a caption, a table cell, a text box or anywhere else
    // `hwpforge::ops::walk`'s `object_counts` also reaches — see that
    // module's doc and its
    // `chart_nested_in_a_caption_is_a_documented_decoder_gap` test for the
    // reproduction. Offering a decode-based chart count here would
    // silently undercount relative to the CLI's existing raw-XML-scan
    // `charts` field for exactly the documents that gap affects, which is
    // the "no fake support" line this crate holds elsewhere too.
    //
    // Each field below is a *third* scope, distinct from both
    // [`Self::top_level_tables`] (no nesting at all) and [`Self::tables`]
    // (nested, but blind to captions, and — unlike these — walks master
    // pages too): captions included, master pages excluded. See
    // `hwpforge::ops::walk`'s module doc for exactly which containers each
    // policy recurses into, and
    // `inspect_deep_counts_table_image_chart_nested_in_image_caption_and_master_page`
    // in `hwpforge-bindings-cli`'s `cli_integration.rs` for the fixture that
    // pins the difference.
    //
    // `#[serde(default)]` (`0`): JSON written by hwpforge 0.16.5 or earlier
    // has none of these keys — same reasoning as
    // [`Self::top_level_tables`].
    /// Tables, captions included, master pages excluded — see the note
    /// above [`Self::has_page_number`].
    #[serde(default)]
    pub tables_all: usize,
    /// Images, captions included, master pages excluded — same scope as
    /// [`Self::tables_all`].
    #[serde(default)]
    pub images_all: usize,
    /// Text boxes (HWPX `<hp:rect>` with a nested `<hp:drawText>`), same
    /// scope as [`Self::tables_all`].
    #[serde(default)]
    pub text_boxes: usize,
    /// Line drawing objects, same scope as [`Self::tables_all`].
    #[serde(default)]
    pub lines: usize,
    /// Pure rectangles (HWPX `<hp:rect>` *without* a nested `<hp:drawText>`
    /// — a text-bearing one counts under [`Self::text_boxes`] instead,
    /// never both), same scope as [`Self::tables_all`].
    #[serde(default)]
    pub rectangles: usize,
    /// Polygon drawing objects, same scope as [`Self::tables_all`].
    #[serde(default)]
    pub polygons: usize,
    /// Body-flow paragraphs with visible text — [`Self::top_level_paragraphs`]
    /// scope (no recursion into headers/footers/master pages), but each
    /// paragraph's visibility probes one recursion step deeper (e.g. a
    /// paragraph with no direct text but a text-bearing table counts).
    #[serde(default)]
    pub non_empty_paragraphs: usize,
    /// Body + header + footer paragraphs, recursing into table cells and
    /// `TextBox`/`Footnote`/`Endnote`/`Ellipse`/`Polygon`/`Memo` content —
    /// **not** captions, group children or master pages (contrast
    /// [`Self::paragraphs`], which walks master pages but not this field's
    /// containers' captions either).
    #[serde(default)]
    pub deep_paragraphs: usize,
    /// Same recursion as [`Self::deep_paragraphs`], counting only the
    /// paragraphs with visible text (per the same deep probe as
    /// [`Self::non_empty_paragraphs`]).
    #[serde(default)]
    pub deep_non_empty_paragraphs: usize,
}

/// Style summary of the document's header definitions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct InspectStyles {
    /// Distinct fonts, keyed by face name and language.
    pub fonts: Vec<FontSummary>,
    /// Character shapes in definition order.
    pub char_shapes: Vec<CharShapeSummary>,
    /// Paragraph shapes in definition order.
    pub para_shapes: Vec<ParaShapeSummary>,
}

/// One font definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct FontSummary {
    /// Index in the header's font table.
    pub id: usize,
    /// Face name as the document spells it.
    pub face_name: String,
    /// Language bucket the face belongs to.
    pub lang: String,
}

/// One character shape definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct CharShapeSummary {
    /// Index in the header's char-shape table.
    pub id: usize,
    /// Hangul font index this shape refers to.
    pub font_id: usize,
    /// Height in points.
    pub size_pt: f64,
    /// Whether the shape is bold.
    pub bold: bool,
    /// Whether the shape is italic.
    pub italic: bool,
    /// Text colour as `#RRGGBB`.
    pub color: String,
}

/// One paragraph shape definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct ParaShapeSummary {
    /// Index in the header's para-shape table.
    pub id: usize,
    /// Horizontal alignment, in the document's own wire spelling.
    pub alignment: String,
    /// Line spacing value, in the unit the shape's spacing type implies.
    pub line_spacing: i32,
}

/// Summarises the structure of an HWPX document.
///
/// Decodes `hwpx` once and walks the result; nothing is read from or
/// written to disk.
///
/// # Errors
///
/// [`OpsError::Decode`] (code `DECODE_FAILED`) when the bytes are not a
/// decodable HWPX package.
///
/// # Examples
///
/// ```no_run
/// use hwpforge::ops::{inspect, InspectOptions};
///
/// let bytes = std::fs::read("document.hwpx")?;
/// let out = inspect(&bytes, &InspectOptions::default().with_styles(true))?;
/// println!("{} sections, {} tables", out.report.sections, out.report.tables);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn inspect(hwpx: &[u8], opts: &InspectOptions) -> Result<InspectOutput, OpsError> {
    let decoded = HwpxDecoder::decode(hwpx).map_err(OpsError::decode)?;
    let document = &decoded.document;
    let metadata = document.metadata();

    let mut fields: Vec<String> = Vec::new();
    let mut section_details: Vec<InspectSection> = Vec::new();

    for (index, section) in document.sections().iter().enumerate() {
        let mut counts = Counts::default();
        section.for_each_paragraph(|paragraph| counts.visit(paragraph, &mut fields));
        let top_level = section.content_counts();
        let objects = walk::object_counts(section);
        let paragraphs = walk::paragraph_counts(section);
        section_details.push(InspectSection {
            index,
            top_level_paragraphs: section.paragraphs.len(),
            top_level_tables: top_level.tables,
            top_level_images: top_level.images,
            top_level_charts: top_level.charts,
            paragraphs: counts.paragraphs,
            tables: counts.tables,
            images: counts.images,
            charts: counts.charts,
            has_header: !section.headers.is_empty(),
            has_footer: !section.footers.is_empty(),
            has_page_number: section.page_number.is_some(),
            tables_all: objects.tables,
            images_all: objects.images,
            text_boxes: objects.text_boxes,
            lines: objects.lines,
            rectangles: objects.rectangles,
            polygons: objects.polygons,
            non_empty_paragraphs: paragraphs.non_empty_paragraphs,
            deep_paragraphs: paragraphs.deep_paragraphs,
            deep_non_empty_paragraphs: paragraphs.deep_non_empty_paragraphs,
        });
    }

    let report = InspectReport {
        metadata: InspectMetadata {
            title: metadata.title.clone().unwrap_or_default(),
            author: metadata.author.clone().unwrap_or_default(),
            subject: metadata.subject.clone().unwrap_or_default(),
            description: metadata.description.clone().unwrap_or_default(),
            last_saved_by: metadata.last_saved_by.clone().unwrap_or_default(),
            created: metadata.created.clone(),
            modified: metadata.modified.clone(),
            keywords: metadata.keywords.clone(),
        },
        sections: section_details.len(),
        paragraphs: section_details.iter().map(|s| s.paragraphs).sum(),
        tables: section_details.iter().map(|s| s.tables).sum(),
        images: section_details.iter().map(|s| s.images).sum(),
        charts: section_details.iter().map(|s| s.charts).sum(),
        fields,
        section_details,
        styles: opts.styles.then(|| summarize_styles(&decoded.style_store)),
    };

    Ok(InspectOutput {
        report,
        warnings: decoded.warnings.into_iter().map(OpsWarning::Decode).collect(),
    })
}

/// Running totals for one section's traversal.
#[derive(Default)]
struct Counts {
    paragraphs: usize,
    tables: usize,
    images: usize,
    charts: usize,
}

impl Counts {
    /// Counts one visited paragraph and collects the field names it carries.
    ///
    /// The traversal already descends into nested paragraphs, so this only
    /// looks at the runs the paragraph owns directly.
    fn visit(&mut self, paragraph: &Paragraph, fields: &mut Vec<String>) {
        self.paragraphs += 1;
        for run in &paragraph.runs {
            match &run.content {
                RunContent::Table(_) => self.tables += 1,
                RunContent::Image(_) => self.images += 1,
                RunContent::Control(control) => match control.as_ref() {
                    Control::Chart { .. } => self.charts += 1,
                    Control::Field {
                        field_type: FieldType::ClickHere, name: Some(name), ..
                    } => {
                        fields.push(name.clone());
                    }
                    _ => {}
                },
                RunContent::Text(_) | RunContent::InlineText(_) => {}
                _ => {}
            }
        }
    }
}

fn summarize_styles(store: &HwpxStyleStore) -> InspectStyles {
    let mut seen = std::collections::HashSet::new();
    let mut fonts = Vec::new();
    for id in 0..store.font_count() {
        if let Ok(font) = store.font(FontIndex::new(id)) {
            if seen.insert((font.face_name.clone(), font.lang.clone())) {
                fonts.push(FontSummary {
                    id,
                    face_name: font.face_name.clone(),
                    lang: font.lang.clone(),
                });
            }
        }
    }

    let char_shapes = (0..store.char_shape_count())
        .filter_map(|id| {
            store.char_shape(CharShapeIndex::new(id)).ok().map(|shape| CharShapeSummary {
                id,
                font_id: shape.font_ref.hangul.get(),
                size_pt: f64::from(shape.height.as_i32()) / 100.0,
                bold: shape.bold,
                italic: shape.italic,
                color: shape.text_color.to_hex_rgb(),
            })
        })
        .collect();

    let para_shapes = (0..store.para_shape_count())
        .filter_map(|id| {
            store.para_shape(ParaShapeIndex::new(id)).ok().map(|shape| ParaShapeSummary {
                id,
                // The wire spelling comes from the enum's own serde
                // representation, so it cannot drift from what the codec writes.
                alignment: serde_json::to_value(shape.alignment)
                    .ok()
                    .and_then(|v| v.as_str().map(String::from))
                    .unwrap_or_else(|| format!("{:?}", shape.alignment)),
                line_spacing: shape.line_spacing,
            })
        })
        .collect();

    InspectStyles { fonts, char_shapes, para_shapes }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_core::run::Run;
    use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};

    fn click_here(name: Option<&str>) -> Control {
        Control::Field {
            field_type: FieldType::ClickHere,
            hint_text: Some("이름을 입력하세요".into()),
            help_text: None,
            name: name.map(str::to_owned),
            display_text: String::new(),
        }
    }

    #[test]
    fn collects_named_click_here_fields_in_run_order() {
        let mut paragraph = Paragraph::new(ParaShapeIndex::new(0));
        let shape = CharShapeIndex::new(0);
        paragraph.runs.push(Run::text("성명: ", shape));
        paragraph.runs.push(Run::control(click_here(Some("성명")), shape));
        paragraph.runs.push(Run::control(click_here(None), shape));
        paragraph.runs.push(Run::control(click_here(Some("소속")), shape));

        let mut counts = Counts::default();
        let mut fields = Vec::new();
        counts.visit(&paragraph, &mut fields);

        assert_eq!(fields, ["성명", "소속"], "unnamed fields cannot be addressed");
        assert_eq!(counts.paragraphs, 1);
        assert_eq!((counts.tables, counts.images, counts.charts), (0, 0, 0));
    }

    #[test]
    fn a_paragraph_without_runs_still_counts_as_one() {
        let mut counts = Counts::default();
        let mut fields = Vec::new();
        counts.visit(&Paragraph::new(ParaShapeIndex::new(0)), &mut fields);

        assert_eq!(counts.paragraphs, 1);
        assert!(fields.is_empty());
    }
}
