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
/// Every count here is the [`InspectSection`] field of the same name, summed
/// over all sections. That struct's `# Scope table` says what each of the
/// four counting scopes visits; all four fields below are the unprefixed one.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct InspectReport {
    /// Document metadata.
    pub metadata: InspectMetadata,
    /// Number of sections.
    pub sections: usize,
    /// Paragraphs, unprefixed scope.
    pub paragraphs: usize,
    /// Tables, unprefixed scope.
    pub tables: usize,
    /// Images, unprefixed scope.
    pub images: usize,
    /// Charts, unprefixed scope — subject to the nested-chart decoder gap
    /// [`InspectSection`]'s scope table records.
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
///
/// # Scope table
///
/// A field's prefix names the *set* it counts. Four scopes share this
/// struct, and they are four incomparable recursion sets rather than a
/// hierarchy — no row is simply a wider version of another.
///
/// | Prefix | Fields | Recurses into | Master pages | Captions |
/// | -- | -- | -- | -- | -- |
/// | `top_level_` | paragraphs, tables, images, charts | nothing: the section's own body-flow runs (`Section::paragraphs`, `Section::content_counts()`) | no | no |
/// | *(none)* | paragraphs, tables, images, charts | `Section::for_each_paragraph` — table cells, text boxes, notes, memos, headers, footers | yes | no |
/// | `deep_` | paragraphs | headers, footers, table cells, and `TextBox`/`Footnote`/`Endnote`/`Ellipse`/`Polygon`/`Memo` bodies | no | no |
/// | `all_` | tables, images, text boxes, lines, rectangles, polygons | everything the section's own XML part physically holds, group children and a memo's anchor run included | no | yes |
///
/// `hwpforge::ops::walk`'s module doc is the canonical detail for the last
/// two rows, which it computes.
///
/// A `non_empty_` infix narrows a row to the paragraphs carrying visible
/// text. It never changes *which* set is counted, so
/// [`Self::top_level_non_empty_paragraphs`] can never exceed
/// [`Self::top_level_paragraphs`]; only the visibility test probes one
/// recursion step deeper than its row (a paragraph with no text of its own
/// but a text-bearing table counts).
///
/// Three facts the table cannot show:
///
/// - **There is no `all_charts`.** `hwpforge-smithy-hwpx`'s decoder
///   reconstructs a `Control::Chart` only for a section's own top-level
///   paragraphs — the `<hp:switch>`/`<hp:chart>` extraction lives in
///   `decode_section`'s top-level loop, not in the recursive per-container
///   dispatch every other nested content type goes through. A chart nested
///   in a table cell, a caption or a text box never becomes a
///   `Control::Chart` at all, so an `all_charts` would silently undercount
///   exactly the documents it would exist for. The same gap makes `charts`
///   and [`Self::top_level_charts`] read alike on every input today. See
///   `hwpforge::ops::walk`'s
///   `chart_nested_in_a_caption_is_a_documented_decoder_gap` test for the
///   reproduction.
/// - **The `all_` row counts decoded objects, not XML elements.** An element
///   the decoder accepts but cannot represent — an `<hp:pic>` with no usable
///   `binaryItemIDRef`, which `convert_picture`
///   (`hwpforge-smithy-hwpx/src/decoder/section.rs`) turns into `Ok(None)`
///   rather than an error — is genuinely in the section XML yet invisible
///   here. `hwpforge-bindings-cli`'s `inspect --json` carries same-scoped
///   keys under its own legacy spelling (`tables`, `images`, `text_boxes`,
///   …) sourced from a raw XML scan, which has no such gap; the two can
///   disagree on such a document, and that CLI deliberately does not read
///   these six. The paragraph rows carry no equivalent caveat: paragraph
///   presence is never silently dropped, so those do agree with the CLI's
///   by construction.
/// - **`#[serde(default)]` marks the twelve fields 0.16.5 never wrote.**
///   JSON from that release or earlier has none of those keys, so they
///   deserialize to `0` — an absent value, not a measured "none". The nine
///   unmarked fields are the whole of what 0.16.5 emitted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct InspectSection {
    /// Section index, 0-based.
    pub index: usize,
    /// Body-flow paragraphs of this section.
    pub top_level_paragraphs: usize,
    /// Tables, `top_level_` scope (`Section::content_counts()`).
    #[serde(default)]
    pub top_level_tables: usize,
    /// Images, `top_level_` scope.
    #[serde(default)]
    pub top_level_images: usize,
    /// Charts, `top_level_` scope — reads the same as [`Self::charts`] on
    /// every input today, per the scope table's nested-chart decoder gap.
    #[serde(default)]
    pub top_level_charts: usize,
    /// Paragraphs, unprefixed scope.
    pub paragraphs: usize,
    /// Tables, unprefixed scope.
    pub tables: usize,
    /// Images, unprefixed scope.
    pub images: usize,
    /// Charts, unprefixed scope — subject to the scope table's
    /// nested-chart decoder gap.
    pub charts: usize,
    /// Whether the section defines a header.
    pub has_header: bool,
    /// Whether the section defines a footer.
    pub has_footer: bool,
    /// Whether the section defines a page number control.
    pub has_page_number: bool,

    // The nine below stay appended after the twelve above, in the order they
    // were added; MCP's `SectionDetail` mirrors that and pins it with a
    // key-order test.
    /// Tables, `all_` scope.
    #[serde(default)]
    pub all_tables: usize,
    /// Images, `all_` scope.
    #[serde(default)]
    pub all_images: usize,
    /// Text boxes (HWPX `<hp:rect>` with a nested `<hp:drawText>`), `all_`
    /// scope.
    #[serde(default)]
    pub all_text_boxes: usize,
    /// Line drawing objects, `all_` scope.
    #[serde(default)]
    pub all_lines: usize,
    /// Pure rectangles (HWPX `<hp:rect>` *without* a nested `<hp:drawText>`
    /// — a text-bearing one counts under [`Self::all_text_boxes`] instead,
    /// never both), `all_` scope.
    #[serde(default)]
    pub all_rectangles: usize,
    /// Polygon drawing objects, `all_` scope.
    #[serde(default)]
    pub all_polygons: usize,
    /// Body-flow paragraphs carrying visible text — [`Self::top_level_paragraphs`]'s
    /// set, narrowed by the scope table's `non_empty_` rule.
    #[serde(default)]
    pub top_level_non_empty_paragraphs: usize,
    /// Paragraphs, `deep_` scope.
    #[serde(default)]
    pub deep_paragraphs: usize,
    /// Paragraphs carrying visible text, `deep_` scope.
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
            all_tables: objects.all_tables,
            all_images: objects.all_images,
            all_text_boxes: objects.all_text_boxes,
            all_lines: objects.all_lines,
            all_rectangles: objects.all_rectangles,
            all_polygons: objects.all_polygons,
            top_level_non_empty_paragraphs: paragraphs.top_level_non_empty_paragraphs,
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
