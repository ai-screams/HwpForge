//! HWP5 IR → Core document projection.
//!
//! This module converts the decoded HWP5 intermediate representation
//! (parsed records, style tables) into HwpForge Core's `Document<Draft>`
//! structure, bridging the format-specific layer to the format-agnostic core.

use std::collections::{BTreeSet, VecDeque};

use hwpforge_core::document::{Document, Draft};
use hwpforge_core::image::{
    Image, ImageFormat, ImagePlacement, ImageRelativeTo, ImageStore, ImageTextFlow, ImageTextWrap,
};
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::section::{HeaderFooter, PageBorderFillEntry, PageNumber, Section};
use hwpforge_core::table::{Table, TableCell, TableMargin, TableRow};
use hwpforge_core::control::RefTarget;
use hwpforge_core::Control;
use hwpforge_core::PageSettings;
use hwpforge_foundation::{
    ArcType, BookmarkType, CharShapeIndex, CurveSegmentType, HwpUnit, NumberFormatType,
    PageNumberPosition, ParaShapeIndex, RefContentType, RefType, StyleIndex,
};

use crate::decoder::chart_ole::{extract_chart_payload, ChartOleError};
use crate::decoder::section::{
    Hwp5ArcControl, Hwp5ConnectLineControl, Hwp5Control, Hwp5CurveControl, Hwp5EllipseControl,
    Hwp5EquationControl, Hwp5ImageControl, Hwp5LineControl, Hwp5MemoControl, Hwp5NestedSubtree,
    Hwp5OleObjectControl, Hwp5PageBorderFill, Hwp5Paragraph, Hwp5PolygonControl, Hwp5RectControl,
    Hwp5Table, Hwp5TableCell, Hwp5TextBoxControl, SectionResult,
};
use crate::decoder::Hwp5Warning;
use crate::error::Hwp5Result;
use crate::numeric::positive_i32_from_u32;
use crate::schema::section::Hwp5DutmalControl;
use crate::schema::section::{
    Hwp5CharShapeRun, Hwp5PageDef, Hwp5ShapeComponentGeometry, Hwp5ShapePoint,
};
use crate::table_cell_vertical_align::{
    core_table_cell_vertical_align, unknown_hwp5_table_cell_vertical_align_raw,
};
use crate::table_page_break::{core_table_page_break, unknown_hwp5_table_page_break_raw};
use crate::warning_utils::push_projection_fallback;
use crate::{Hwp5JoinedImageAsset, Hwp5JoinedImageAssetPlan, Hwp5OleAssetPlan};

const CTRL_ID_SECTION_DEF: u32 = 0x7365_6364; // "secd"
const CTRL_ID_COLUMN_DEF: u32 = 0x636F_6C64; // "cold"
const CTRL_ID_PAGE_NUMBER: u32 = 0x7067_6E70; // "pgnp"
const CTRL_ID_BOOKMARK_SPAN: u32 = 0x2562_6D6B; // "%bmk"
const CTRL_ID_HYPERLINK: u32 = 0x2568_6C6B; // "%hlk"
const CTRL_ID_CROSSREF: u32 = 0x2578_7266; // "%xrf"
/// Wire code for the Bookmark `RefType` variant (Wave 12m Phase 2). The
/// HWP5 `%xrf` Command's N1 slot uses these codes; boundary functions
/// in this file map them to typed [`RefType`].
const HWP5_CROSSREF_REF_TYPE_TABLE: u8 = 0;
/// Wire code for the Figure `RefType` variant (Wave 12m).
const HWP5_CROSSREF_REF_TYPE_FIGURE: u8 = 1;
/// Wire code for the Equation `RefType` variant (Wave 12m).
const HWP5_CROSSREF_REF_TYPE_EQUATION: u8 = 2;
/// Wire code for the Footnote `RefType` variant (Wave 12m).
const HWP5_CROSSREF_REF_TYPE_FOOTNOTE: u8 = 3;
/// Wire code for the Endnote `RefType` variant (Wave 12m).
const HWP5_CROSSREF_REF_TYPE_ENDNOTE: u8 = 4;
/// Wire code for the Outline `RefType` variant (Wave 12m).
const HWP5_CROSSREF_REF_TYPE_OUTLINE: u8 = 5;
/// Wire code for the Bookmark `RefType` variant (Wave 12m).
const HWP5_CROSSREF_REF_TYPE_BOOKMARK: u8 = 6;
const CTRL_ID_BOOKMARK_POINT: u32 = 0x626F_6B6D; // "bokm"
/// Wave 12l: ClickHere (누름틀) inline `FieldBegin` ctrl_id `%clk`.
/// Hint/help live in the matching `%clk` CtrlHeader payload and the
/// form-mode `name` lives in the trailing `0x57 lvl=2` sub-record —
/// see `schema::section::Hwp5ClickHereControl`.
const CTRL_ID_CLICK_HERE: u32 = 0x2563_6C6B; // "%clk"
/// SUMMERY auto-field ctrl_id (Wave 12n). Matches the inline `FieldBegin`
/// `extra` bytes so the projection layer can pop the parsed payload from
/// `summery_fields` when this id arrives.
const CTRL_ID_FIELD_SUMMERY: u32 = 0x2573_6D72; // "%smr"
/// `%dte` date/time format-code ctrl_id (Wave 12n).
const CTRL_ID_FIELD_DATE_CODE: u32 = 0x2564_7465; // "%dte"
/// `%pat` path/file-name ctrl_id (Wave 12n).
const CTRL_ID_FIELD_PATH: u32 = 0x2570_6174; // "%pat"
/// Inline `FieldBegin` ctrl_id for memo anchors (`%%me` BE-ascii).
///
/// In the HWP5 body text stream, memos are embedded as `FieldBegin` /
/// `FieldEnd` markers whose `extra[0..4]` raw bytes are
/// `65 6D 25 25` (ASCII `e m % %` — same "LE-stored u32 of BE-ascii name"
/// convention as `%bmk` / `%hlk` / `%xrf` above). After
/// `ctrl_id_from_inline_extra` reverses + reads BE, that yields
/// `0x2525_6D65`.
///
/// This is *not* the same identifier as the `CtrlHeader` ctrl_id for memo
/// placeholders — that one is `%unk` (`0x2575_6E6B`), defined in the
/// decoder. HWP5 uses one ID for the inline anchor and another for the
/// `CtrlHeader` placeholder; only the inline id is needed here, where we
/// translate `FieldBegin` markers into `ActiveField::MemoAnchor`.
const CTRL_ID_MEMO_INLINE: u32 = 0x2525_6D65;

#[derive(Debug, Default)]
struct SectionProjectionHints {
    unresolved_bookmark_names: VecDeque<String>,
}

impl SectionProjectionHints {
    fn from_paragraphs(paragraphs: &[Hwp5Paragraph]) -> Self {
        // Wave 12m Phase 2 Step 4: %xrf is now `Hwp5Control::CrossRef`
        // (typed schema). Only Bookmark cross-refs (ref_type_code == 6)
        // carry a bookmark NAME in `target_raw`; other ref types use
        // `#<id>` SystemIds and won't resolve to a bookmark span, so
        // they are skipped here. Preserves the previous behavior of
        // back-feeding bookmark span names from forward cross-refs.
        let mut seen = BTreeSet::new();
        let mut unresolved_bookmark_names = VecDeque::new();
        for paragraph in paragraphs {
            for control in &paragraph.controls {
                let Hwp5Control::CrossRef(xrf) = control else {
                    continue;
                };
                if xrf.ref_type_code != HWP5_CROSSREF_REF_TYPE_BOOKMARK {
                    continue;
                }
                let target_name = xrf.target_raw.clone();
                if target_name.is_empty() {
                    continue;
                }
                if seen.insert(target_name.clone()) {
                    unresolved_bookmark_names.push_back(target_name);
                }
            }
        }
        Self { unresolved_bookmark_names }
    }

    fn take_bookmark_name(&mut self) -> Option<String> {
        self.unresolved_bookmark_names.pop_front()
    }
}

#[derive(Debug)]
struct ProjectedParagraph {
    paragraph: Paragraph,
}

#[derive(Debug, Clone, Copy)]
struct UnknownControlHeader<'a> {
    ctrl_id: u32,
    header_data: &'a [u8],
}

#[derive(Debug)]
struct ParagraphProjectionQueues<'a> {
    marker_headers: VecDeque<UnknownControlHeader<'a>>,
    object_controls: VecDeque<&'a Hwp5Control>,
    /// Pending memo placeholders in document order. Consumed by
    /// `FieldBegin %unk MEMO` inline segments via `start_active_field`;
    /// any leftovers are drained at end-of-paragraph as a safety net.
    memo_controls: VecDeque<Hwp5MemoControl>,
    /// Pending ClickHere (`%clk`) press-fields in document order
    /// (Wave 12l). Consumed by `FieldBegin %clk` inline segments via
    /// `start_active_field`. Like `memo_controls`, leftovers do not
    /// emit visible runs — they only carry metadata.
    clickhere_controls: VecDeque<crate::schema::section::Hwp5ClickHereControl>,
    /// Pending SUMMERY (`%smr`) auto-fields in document order (Wave 12n).
    /// Consumed by `FieldBegin %smr` inline segments via `start_active_field`.
    /// Same lifecycle as `clickhere_controls`.
    summery_fields: VecDeque<crate::schema::section::Hwp5SummeryControl>,
    /// Pending `%dte` date/time format-code fields in document order
    /// (Wave 12n). Consumed by `FieldBegin %dte` inline segments.
    datecode_fields: VecDeque<crate::schema::section::Hwp5DateCodeControl>,
    /// Pending `%pat` path/file-name fields in document order (Wave 12n).
    /// Consumed by `FieldBegin %pat` inline segments.
    pathfield_controls: VecDeque<crate::schema::section::Hwp5PathFieldControl>,
    /// Pending `%xrf` cross-reference controls in document order
    /// (Wave 12m Phase 2 Step 4). Consumed by `FieldBegin %xrf` inline
    /// segments via `start_active_field`. Same lifecycle as the other
    /// CtrlHeader-backed field queues above.
    crossref_controls: VecDeque<crate::schema::section::Hwp5CrossRefControl>,
    point_bookmark_names: VecDeque<String>,
}

#[derive(Debug)]
enum ActiveField {
    Hyperlink {
        url: String,
        start_utf16: u32,
        display_text: String,
    },
    BookmarkSpan {
        name: String,
        start_utf16: u32,
    },
    /// `%xrf` cross-reference field span (Wave 12m Phase 2 Step 4).
    /// Carries the structured wire payload parsed at the decoder
    /// boundary. `display_text` accumulates body chars between
    /// `FieldBegin` and `FieldEnd` so the HWPX encoder can embed it
    /// between `<hp:fieldBegin>` and `<hp:fieldEnd>`.
    CrossRef {
        control: crate::schema::section::Hwp5CrossRefControl,
        start_utf16: u32,
        display_text: String,
    },
    PlainTextFallback {
        start_utf16: u32,
    },
    /// Memo anchor: the inline `FieldBegin %unk MEMO` to `FieldEnd` span
    /// whose anchor text flows directly into `runs` (not dropped). The
    /// `Hwp5Control::Memo` run is emitted at `FieldEnd` after the anchor
    /// text. Memo body is cloned at queue-build time to avoid leaking a
    /// borrow into `ActiveField`.
    MemoAnchor {
        start_utf16: u32,
        memo: Hwp5MemoControl,
    },
    /// ClickHere (누름틀, CLICK_HERE press-field) — Wave 12l. The
    /// inline `FieldBegin %clk` to `FieldEnd` span is rendered as a
    /// single `Control::Field { field_type: ClickHere, hint_text,
    /// help_text, name }` Run emitted at `FieldEnd`. Per Codex review
    /// the span text between the markers is *not* accumulated: in
    /// HWP5 wire that span is empty, and HWPX renders `hint_text` as
    /// the visible placeholder, so accumulating display text would
    /// risk double-emitting it.
    ClickHere {
        start_utf16: u32,
        hint_text: Option<String>,
        help_text: Option<String>,
        name: Option<String>,
    },
    /// SUMMERY auto-field (Wave 12n). `command_token` carries the wire
    /// `$X` token (e.g. `$author`, `$modifiedtime`). On `FieldEnd` the
    /// token is mapped to a typed [`hwpforge_foundation::FieldType`] or,
    /// for unknown tokens, surfaced as `Control::UnknownSummery { token }`.
    /// The inline span text is not accumulated: HWP5 wire leaves it
    /// empty and HWPX renders the field value at display time.
    SummeryField {
        start_utf16: u32,
        command_token: String,
    },
    /// `%dte` date/time format-code field (Wave 12n). Carries the raw
    /// Command pattern + 8-byte trailer for round-trip fidelity. On
    /// `FieldEnd` the projection emits `Control::DateCodeField` with
    /// `is_time_mode` derived from the `T` prefix.
    DateCodeField {
        start_utf16: u32,
        raw_command: String,
        raw_trailer: [u8; 8],
    },
    /// `%pat` path/file-name field (Wave 12n). On `FieldEnd` the
    /// projection maps the raw Command to a typed `PathFieldCommand`
    /// (or `Unknown` for forward compatibility) and emits
    /// `Control::PathField`.
    PathField {
        start_utf16: u32,
        raw_command: String,
    },
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Project decoded HWP5 sections into a Core `Document<Draft>`.
///
/// Returns the document and any warnings accumulated during projection.
pub(crate) fn project_to_core(
    sections: Vec<SectionResult>,
) -> Hwp5Result<(Document<Draft>, Vec<Hwp5Warning>)> {
    let (document, _image_store, warnings) = project_to_core_internal(sections, None, None)?;
    Ok((document, warnings))
}

/// Project decoded HWP5 sections into Core with the current image slice enabled.
pub(crate) fn project_to_core_with_images(
    sections: Vec<SectionResult>,
    image_assets: &Hwp5JoinedImageAssetPlan,
) -> Hwp5Result<(Document<Draft>, ImageStore, Vec<Hwp5Warning>)> {
    project_to_core_internal(sections, Some(image_assets), None)
}

/// Project decoded HWP5 sections into Core with both image and OLE asset plans.
///
/// Used by [`crate::hwp5_to_hwpx_bytes`] so the projection layer can attempt
/// chart payload extraction from `/BinData/BIN*.OLE` entries and emit
/// [`hwpforge_core::Control::EmbeddedChart`] runs (Wave 4c carry).
pub(crate) fn project_to_core_with_images_and_ole(
    sections: Vec<SectionResult>,
    image_assets: &Hwp5JoinedImageAssetPlan,
    ole_assets: &Hwp5OleAssetPlan,
) -> Hwp5Result<(Document<Draft>, ImageStore, Vec<Hwp5Warning>)> {
    project_to_core_internal(sections, Some(image_assets), Some(ole_assets))
}

fn project_to_core_internal(
    sections: Vec<SectionResult>,
    image_assets: Option<&Hwp5JoinedImageAssetPlan>,
    ole_assets: Option<&Hwp5OleAssetPlan>,
) -> Hwp5Result<(Document<Draft>, ImageStore, Vec<Hwp5Warning>)> {
    let mut doc = Document::<Draft>::new();
    let mut all_warnings: Vec<Hwp5Warning> = Vec::new();
    let mut projection_images = ProjectionImageState::new(image_assets, ole_assets);

    for section_result in sections {
        // Collect warnings from decoding.
        all_warnings.extend(section_result.warnings);

        // Convert page definition.
        let page_settings = section_result
            .page_def
            .as_ref()
            .map(page_def_to_settings)
            .unwrap_or_else(PageSettings::a4);

        let mut section = Section::new(page_settings);
        // Gap B: decode the `secd` ctrl property word (HWP 5.0 spec
        // §4.3.10.1 표 130) into Core's `Visibility`. Bits 0~5 + 8/9 +
        // 19 map 1:1 to the HWPX `<hp:visibility>` element. `None` →
        // don't override the Core default (matches pre-Wave-5
        // behavior). See
        // `.docs/debug/2026-05-27_hwp5_page_features_lost.md` (gap B).
        if let Some(properties) = section_result.section_def_properties {
            section.visibility = Some(hwp5_section_properties_to_visibility(properties));
        }
        // Wave 7: carry the section's page border/fill references
        // (HWPTAG_PAGE_BORDER_FILL, 0x4B). The borderFill *definitions*
        // already reach the HWPX style store; without this the encoder
        // fabricates a default `borderFillIDRef="1"` (an invisible
        // border). 한글 emits exactly three records in [BOTH, EVEN, ODD]
        // order. See `.docs/debug/2026-05-29_hwp5_page_border_fill.md`.
        if !section_result.page_border_fills.is_empty() {
            section.page_border_fills = Some(hwp5_page_border_fills_to_entries(
                &section_result.page_border_fills,
                &mut projection_images.warnings,
            ));
        }
        let mut section_field_hints =
            SectionProjectionHints::from_paragraphs(&section_result.paragraphs);
        // ADR-002 + gap A: collect per-ctrl subtrees instead of
        // flattening all headers/footers into one. Each tuple is
        // `(projected paragraphs, raw 4-byte property field)` so the
        // applyPageType bits survive (HWP 5.0 spec §4.3.10.3 표 141).
        let mut header_subtrees: Vec<(Vec<Paragraph>, u32)> = Vec::new();
        let mut footer_subtrees: Vec<(Vec<Paragraph>, u32)> = Vec::new();

        // The page number is a section-level property. 한글 stores its `pgnp`
        // control in a top-level body paragraph OR inside a layout table cell,
        // so resolve it once by scanning the whole body (recursing into table
        // cells) rather than treating it as a per-paragraph output.
        section.page_number =
            find_section_page_number(&section_result.paragraphs, &mut projection_images.warnings);

        // Project each paragraph.
        for hwp_para in section_result.paragraphs {
            header_subtrees.extend(collect_header_subtrees(&hwp_para, &mut projection_images));
            footer_subtrees.extend(collect_footer_subtrees(&hwp_para, &mut projection_images));

            let projected = project_paragraph_with_images(
                &hwp_para,
                &mut projection_images,
                ImageProjectionContext::Flow,
                Some(&mut section_field_hints),
            );
            section.add_paragraph(projected.paragraph);
        }

        // ADR-002 + gap A: per-ctrl applyPageType carry.
        //
        // HWP 5.0 spec §4.3.10.3 표 141: header/footer ctrl payload's
        // bytes [4..8] hold a property word whose bit 0~1 encode the
        // page-type scope (0=BOTH, 1=EVEN, 2=ODD). The decoder
        // preserved this as `Hwp5NestedSubtree.properties_raw`; here
        // each subtree becomes its own `HeaderFooter` entry so HWPX
        // emits matching `<hp:header applyPageType="..."/>` × N.
        for (paragraphs, properties_raw) in header_subtrees {
            section.headers.push(HeaderFooter::new(
                paragraphs,
                hwp5_header_property_to_apply_page_type(properties_raw),
            ));
        }
        for (paragraphs, properties_raw) in footer_subtrees {
            section.footers.push(HeaderFooter::new(
                paragraphs,
                hwp5_header_property_to_apply_page_type(properties_raw),
            ));
        }

        // Ensure every section has at least one paragraph (validation requirement).
        if section.is_empty() {
            section.add_paragraph(Paragraph::with_runs(
                vec![Run::text("", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            ));
        }

        doc.add_section(section);
    }

    // Ensure document has at least one section.
    if doc.is_empty() {
        let mut section = Section::new(PageSettings::a4());
        section.add_paragraph(Paragraph::with_runs(
            vec![Run::text("", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        ));
        doc.add_section(section);
    }

    all_warnings.extend(projection_images.warnings);
    Ok((doc, projection_images.image_store, all_warnings))
}

// ---------------------------------------------------------------------------
// Paragraph projection
// ---------------------------------------------------------------------------

struct ProjectionImageState<'a> {
    image_assets: Option<&'a Hwp5JoinedImageAssetPlan>,
    ole_assets: Option<&'a Hwp5OleAssetPlan>,
    image_store: ImageStore,
    warnings: Vec<Hwp5Warning>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImageProjectionContext {
    Flow,
    TextBox,
}

impl<'a> ProjectionImageState<'a> {
    fn new(
        image_assets: Option<&'a Hwp5JoinedImageAssetPlan>,
        ole_assets: Option<&'a Hwp5OleAssetPlan>,
    ) -> Self {
        Self { image_assets, ole_assets, image_store: ImageStore::new(), warnings: Vec::new() }
    }

    /// Look up raw `/BinData/BIN*.OLE` bytes by `binary_data_id`, if a plan
    /// was supplied. `None` means no OLE plan is wired (e.g. inspect path).
    fn ole_bytes_for_binary_data_id(&self, binary_data_id: u16) -> Option<&[u8]> {
        self.ole_assets.and_then(|plan| plan.bytes_for_binary_data_id(binary_data_id))
    }

    fn build_image(
        &mut self,
        image: &Hwp5ImageControl,
        context: ImageProjectionContext,
    ) -> Option<Image> {
        let Some(image_assets): Option<&Hwp5JoinedImageAssetPlan> = self.image_assets else {
            self.warnings.push(Hwp5Warning::DroppedControl {
                control: "image",
                reason: "projection_image_assets_unavailable".to_string(),
            });
            return None;
        };
        let Some(asset): Option<&Hwp5JoinedImageAsset> =
            image_assets.asset_for_binary_data_id(image.binary_data_id)
        else {
            self.warnings.push(Hwp5Warning::DroppedControl {
                control: "image",
                reason: format!("missing_image_asset_for_binary_data_id={}", image.binary_data_id),
            });
            return None;
        };
        let resolved_dimensions: ResolvedImageDimensions =
            resolve_image_dimensions(image, &asset.payload);

        if resolved_dimensions.width_hwp <= 0 || resolved_dimensions.height_hwp <= 0 {
            self.warnings.push(Hwp5Warning::DroppedControl {
                control: "image",
                reason: format!(
                    "image_zero_size_projection binary_data_id={} width={} height={}",
                    image.binary_data_id,
                    resolved_dimensions.width_hwp,
                    resolved_dimensions.height_hwp
                ),
            });
            return None;
        }

        self.image_store.insert(asset.payload.storage_name.clone(), asset.bytes.clone());

        Some(
            Image::new(
                asset.payload.package_path.clone(),
                HwpUnit::new(resolved_dimensions.width_hwp).unwrap_or(HwpUnit::ZERO),
                HwpUnit::new(resolved_dimensions.height_hwp).unwrap_or(HwpUnit::ZERO),
                core_image_format(&asset.payload.format),
            )
            .with_placement(image_placement_from_geometry(&image.geometry, context)),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResolvedImageDimensions {
    width_hwp: i32,
    height_hwp: i32,
}

fn resolve_image_dimensions(
    image: &Hwp5ImageControl,
    payload: &crate::Hwp5SemanticImagePayload,
) -> ResolvedImageDimensions {
    let control_width_hwp: Option<i32> = positive_i32_from_u32(image.geometry.width);
    let control_height_hwp: Option<i32> = positive_i32_from_u32(image.geometry.height);
    let joined_width_hwp: Option<i32> = payload.width_hwp.filter(|width| *width > 0);
    let joined_height_hwp: Option<i32> = payload.height_hwp.filter(|height| *height > 0);

    let width_hwp: i32 = control_width_hwp
        .or(joined_width_hwp)
        .unwrap_or_else(|| i32::try_from(image.geometry.width).unwrap_or(0));
    let height_hwp: i32 = control_height_hwp
        .or(joined_height_hwp)
        .unwrap_or_else(|| i32::try_from(image.geometry.height).unwrap_or(0));
    ResolvedImageDimensions { width_hwp, height_hwp }
}

fn image_placement_from_geometry(
    geometry: &Hwp5ShapeComponentGeometry,
    context: ImageProjectionContext,
) -> ImagePlacement {
    match context {
        ImageProjectionContext::TextBox => ImagePlacement {
            text_wrap: ImageTextWrap::Square,
            text_flow: ImageTextFlow::BothSides,
            treat_as_char: false,
            flow_with_text: true,
            allow_overlap: false,
            vert_rel_to: ImageRelativeTo::Para,
            horz_rel_to: ImageRelativeTo::Para,
            vert_offset: HwpUnit::new(geometry.y).unwrap_or(HwpUnit::ZERO),
            horz_offset: HwpUnit::new(geometry.x).unwrap_or(HwpUnit::ZERO),
        },
        ImageProjectionContext::Flow if geometry.x != 0 || geometry.y != 0 => ImagePlacement {
            text_wrap: ImageTextWrap::InFrontOfText,
            text_flow: ImageTextFlow::BothSides,
            treat_as_char: false,
            flow_with_text: false,
            allow_overlap: true,
            vert_rel_to: ImageRelativeTo::Paper,
            horz_rel_to: ImageRelativeTo::Paper,
            vert_offset: HwpUnit::new(geometry.y).unwrap_or(HwpUnit::ZERO),
            horz_offset: HwpUnit::new(geometry.x).unwrap_or(HwpUnit::ZERO),
        },
        ImageProjectionContext::Flow => ImagePlacement::legacy_inline_defaults(),
    }
}

fn project_paragraph_with_images(
    hwp_para: &Hwp5Paragraph,
    projection_images: &mut ProjectionImageState<'_>,
    image_context: ImageProjectionContext,
    field_hints: Option<&mut SectionProjectionHints>,
) -> ProjectedParagraph {
    let mut projected = if !paragraph_needs_structural_projection(hwp_para) {
        ProjectedParagraph {
            paragraph: project_paragraph_with_images_flat(
                hwp_para,
                projection_images,
                image_context,
            ),
        }
    } else {
        project_paragraph_with_images_structural(
            hwp_para,
            projection_images,
            image_context,
            field_hints,
        )
    };

    // Carry inline `<hp:tab>` attributes (width / leader / type) by
    // lifting `RunContent::Text(String)` runs that contain `\t` into
    // `RunContent::InlineText(InlineText)` whenever the matching tab
    // metadata is non-default. The flat and structural projection
    // paths both leave tabs as plain `\t` chars, so this is the one
    // place that bridges to the rich inline representation.
    //
    // Any tab metadata that isn't carried (because the corresponding
    // tab character was consumed by a hyperlink display or field
    // begin/end pair) still falls through to the warning below, so the
    // audit baseline keeps a foothold on the silent loss.
    let unconsumed = carry_inline_tab_attrs(&mut projected.paragraph, &hwp_para.text_segments);
    for (width, leader, tab_type) in unconsumed {
        projection_images.warnings.push(crate::decoder::Hwp5Warning::ProjectionFallback {
            subject: "inline_tab.attributes",
            reason: format!(
                "inline <hp:tab> attributes dropped (no run carrier): width={width} leader={leader} tab_type={tab_type}"
            ),
        });
    }

    projected
}

/// Walks `text_segments` collecting non-default inline tab attributes
/// in order, then upgrades each `Text(String)` run that contains `\t`
/// into `InlineText(InlineText)` with the corresponding tab attributes
/// substituted at each `\t` position.
///
/// Returns the list of `(width, leader, tab_type)` tuples for tabs
/// that could not be associated with any text run (e.g. tabs consumed
/// inside a hyperlink display or by a field begin/end pair). Callers
/// turn these into warnings so the audit baseline still surfaces the
/// loss.
fn carry_inline_tab_attrs(
    paragraph: &mut Paragraph,
    text_segments: &[crate::schema::section::TextSegment],
) -> Vec<(u32, u8, u8)> {
    use hwpforge_core::inline::{InlineSegment, InlineTabAttr, InlineText};
    use hwpforge_core::tab::TabDef;
    use hwpforge_foundation::HwpUnit;
    use std::collections::VecDeque;

    let mut attrs_q: VecDeque<InlineTabAttr> = VecDeque::new();
    for seg in text_segments {
        let crate::schema::section::TextSegment::Tab { extra } = seg else {
            continue;
        };
        let width = u32::from_le_bytes([extra[0], extra[1], extra[2], extra[3]]);
        let leader = extra[4];
        let tab_type = extra[5];
        attrs_q.push_back(InlineTabAttr {
            // HWP5 stores raw HwpUnit; cap by the same helper that
            // backs `TabStop.position` so the value always fits Core's
            // ±100M `HwpUnit` range without losing the inline tab.
            width: TabDef::clamp_position_from_unsigned(u64::from(width)),
            leader,
            tab_type,
        });
    }
    if attrs_q.is_empty() {
        return Vec::new();
    }

    for run in &mut paragraph.runs {
        let text = match &run.content {
            RunContent::Text(s) if s.contains('\t') => s.clone(),
            _ => continue,
        };
        let tab_count = text.chars().filter(|&c| c == '\t').count();
        let mut run_attrs: Vec<InlineTabAttr> = Vec::with_capacity(tab_count);
        for _ in 0..tab_count {
            run_attrs.push(attrs_q.pop_front().unwrap_or(InlineTabAttr {
                width: HwpUnit::ZERO,
                leader: 0,
                tab_type: 0,
            }));
        }
        // Skip the upgrade when every tab is the default — keeps the
        // common `Text(String)` representation for plain `\t` runs.
        if run_attrs.iter().all(InlineTabAttr::is_default) {
            continue;
        }
        let mut segments: Vec<InlineSegment> = Vec::new();
        let mut current = String::new();
        let mut iter = run_attrs.into_iter();
        for ch in text.chars() {
            if ch == '\t' {
                if !current.is_empty() {
                    segments.push(InlineSegment::Plain(std::mem::take(&mut current)));
                }
                if let Some(attr) = iter.next() {
                    segments.push(InlineSegment::Tab(attr));
                }
            } else {
                current.push(ch);
            }
        }
        if !current.is_empty() {
            segments.push(InlineSegment::Plain(current));
        }
        run.content = RunContent::InlineText(InlineText::from_segments(segments));
    }

    attrs_q
        .into_iter()
        .filter(|a| !a.is_default())
        .map(|a| (a.width.as_i32() as u32, a.leader, a.tab_type))
        .collect()
}

fn project_paragraph_with_images_flat(
    hwp_para: &Hwp5Paragraph,
    projection_images: &mut ProjectionImageState<'_>,
    image_context: ImageProjectionContext,
) -> Paragraph {
    let mut runs: Vec<Run> = Vec::new();
    // Marker-header controls (secd / cold / %bmk / %hlk / %xrf / bokm /
    // pgnp) are consumed by `SectionColumnDef` / `FieldBegin` text
    // segments in the structural path, never by `ControlRef`
    // (`\u{FFFC}`) markers. The flat path historically iterated *all*
    // controls and so mis-aligned the FFFC↔control pairing for first
    // section paragraphs (which always carry `secd`/`cold`). Filter
    // them out here so the FFFC iterator only sees object controls.
    // See `.docs/algorithms/2026-06-01_dutmal_carry.md` (companion-fix
    // section) for the full root-cause + rationale.
    let mut control_iter = hwp_para.controls.iter().filter(|control| {
        !matches!(
            control,
            Hwp5Control::Unknown {
                ctrl_id: CTRL_ID_SECTION_DEF
                    | CTRL_ID_COLUMN_DEF
                    | CTRL_ID_BOOKMARK_SPAN
                    | CTRL_ID_HYPERLINK
                    | CTRL_ID_CROSSREF
                    | CTRL_ID_BOOKMARK_POINT
                    | CTRL_ID_PAGE_NUMBER,
                ..
            }
        )
    });
    let mut segment_start_utf16: u32 = 0;
    let mut current_utf16: u32 = 0;

    for ch in hwp_para.text.chars() {
        let char_utf16_len = ch.len_utf16() as u32;
        if ch == '\u{FFFC}' {
            runs.extend(project_text_segment(
                &hwp_para.text,
                &hwp_para.char_shape_runs,
                segment_start_utf16,
                current_utf16,
            ));

            if let Some(control) = control_iter.next() {
                if let Some(run) = project_control_run(control, projection_images, image_context) {
                    runs.push(run);
                }
            }

            current_utf16 += char_utf16_len;
            segment_start_utf16 = current_utf16;
            continue;
        }

        current_utf16 += char_utf16_len;
    }

    runs.extend(project_text_segment(
        &hwp_para.text,
        &hwp_para.char_shape_runs,
        segment_start_utf16,
        current_utf16,
    ));

    for control in control_iter {
        if let Some(run) = project_control_run(control, projection_images, image_context) {
            runs.push(run);
        }
    }

    if runs.is_empty() {
        runs.push(Run::text("", CharShapeIndex::new(0)));
    }

    let mut paragraph =
        Paragraph::with_runs(runs, ParaShapeIndex::new(hwp_para.para_shape_id as usize));
    if hwp_para.style_id > 0 {
        paragraph = paragraph.with_style(StyleIndex::new(hwp_para.style_id as usize));
    }
    paragraph
}

fn project_paragraph_with_images_structural(
    hwp_para: &Hwp5Paragraph,
    projection_images: &mut ProjectionImageState<'_>,
    image_context: ImageProjectionContext,
    mut field_hints: Option<&mut SectionProjectionHints>,
) -> ProjectedParagraph {
    let mut queues =
        build_paragraph_projection_queues(hwp_para, projection_images, field_hints.as_deref_mut());
    let mut runs: Vec<Run> = Vec::new();
    let mut visible_utf16: u32 = 0;
    let mut active_field: Option<ActiveField> = None;

    for segment in &hwp_para.text_segments {
        match segment {
            crate::schema::section::TextSegment::Text(text) => {
                let len = text.encode_utf16().count() as u32;
                if let Some(active) = active_field.as_mut() {
                    match active {
                        ActiveField::Hyperlink { display_text, .. }
                        | ActiveField::CrossRef { display_text, .. } => display_text.push_str(text),
                        // (Wave 12m Phase 2 Step 4: control field carries
                        //  the structured payload; display_text is body-only.)
                        // BookmarkSpan / PlainTextFallback / MemoAnchor all emit
                        // their anchor text in `finish_active_field` via
                        // `project_text_segment(start, end)`, so silently
                        // advance the visible cursor here.
                        ActiveField::BookmarkSpan { .. }
                        | ActiveField::PlainTextFallback { .. }
                        | ActiveField::MemoAnchor { .. }
                        | ActiveField::ClickHere { .. }
                        | ActiveField::SummeryField { .. }
                        | ActiveField::DateCodeField { .. }
                        | ActiveField::PathField { .. } => {}
                    }
                } else {
                    runs.extend(project_text_segment(
                        &hwp_para.text,
                        &hwp_para.char_shape_runs,
                        visible_utf16,
                        visible_utf16 + len,
                    ));
                }
                visible_utf16 += len;
            }
            crate::schema::section::TextSegment::Tab { .. } => {
                // Inline tab metadata is dropped here; `<hp:tab>`
                // attribute carry is tracked separately by
                // `warn_on_inline_tab_attributes` to cover both the
                // flat and structural projection branches uniformly.
                append_visible_unit(
                    hwp_para,
                    &mut runs,
                    &mut active_field,
                    &mut visible_utf16,
                    '\t',
                );
            }
            crate::schema::section::TextSegment::LineBreak => {
                append_visible_unit(
                    hwp_para,
                    &mut runs,
                    &mut active_field,
                    &mut visible_utf16,
                    '\n',
                );
            }
            crate::schema::section::TextSegment::NonBreakingSpace => {
                // Sentinel: U+00A0 is the canonical NBSP code-point and is
                // what `inline_text::encode_inline_text_xml` translates back
                // into `<hp:nbSpace/>` on HWPX emit.
                append_visible_unit(
                    hwp_para,
                    &mut runs,
                    &mut active_field,
                    &mut visible_utf16,
                    '\u{00A0}',
                );
            }
            crate::schema::section::TextSegment::FwSpace => {
                // Sentinel: U+001F mirrors the HWP5 wire control byte for
                // fixed-width space and is what `inline_text` translates back
                // into `<hp:fwSpace/>` on HWPX emit.
                append_visible_unit(
                    hwp_para,
                    &mut runs,
                    &mut active_field,
                    &mut visible_utf16,
                    '\u{001F}',
                );
            }
            crate::schema::section::TextSegment::ControlRef { .. }
            | crate::schema::section::TextSegment::ExtendedControlRef { .. } => {
                if active_field.is_none() {
                    if let Some(control) = queues.object_controls.pop_front() {
                        if let Some(run) =
                            project_control_run(control, projection_images, image_context)
                        {
                            runs.push(run);
                        }
                    }
                }
                visible_utf16 += 1;
            }
            crate::schema::section::TextSegment::SectionColumnDef { extra } => {
                let ctrl_id = ctrl_id_from_inline_extra(extra);
                let _ = consume_marker_header(&mut queues.marker_headers, ctrl_id);
            }
            crate::schema::section::TextSegment::FieldBegin { extra } => {
                let ctrl_id = ctrl_id_from_inline_extra(extra);
                let header = consume_marker_header(&mut queues.marker_headers, ctrl_id);
                let memo = if ctrl_id == CTRL_ID_MEMO_INLINE {
                    queues.memo_controls.pop_front()
                } else {
                    None
                };
                let clickhere = if ctrl_id == CTRL_ID_CLICK_HERE {
                    queues.clickhere_controls.pop_front()
                } else {
                    None
                };
                let summery = if ctrl_id == CTRL_ID_FIELD_SUMMERY {
                    queues.summery_fields.pop_front()
                } else {
                    None
                };
                let datecode = if ctrl_id == CTRL_ID_FIELD_DATE_CODE {
                    queues.datecode_fields.pop_front()
                } else {
                    None
                };
                let pathfield = if ctrl_id == CTRL_ID_FIELD_PATH {
                    queues.pathfield_controls.pop_front()
                } else {
                    None
                };
                let crossref = if ctrl_id == CTRL_ID_CROSSREF {
                    queues.crossref_controls.pop_front()
                } else {
                    None
                };
                active_field = Some(start_active_field(
                    ctrl_id,
                    header,
                    memo,
                    clickhere,
                    summery,
                    datecode,
                    pathfield,
                    crossref,
                    visible_utf16,
                    projection_images,
                    field_hints.as_deref_mut(),
                ));
            }
            crate::schema::section::TextSegment::FieldEnd => {
                if let Some(field) = active_field.take() {
                    finish_active_field(
                        field,
                        hwp_para,
                        visible_utf16,
                        &mut runs,
                        projection_images,
                    );
                }
            }
            crate::schema::section::TextSegment::ParaBreak => {}
        }
    }

    if let Some(field) = active_field.take() {
        finish_active_field(field, hwp_para, visible_utf16, &mut runs, projection_images);
    }

    for bookmark_name in queues.point_bookmark_names {
        let char_shape_id = CharShapeIndex::new(char_shape_id_for_visible_position(
            &hwp_para.char_shape_runs,
            visible_utf16,
        ) as usize);
        runs.push(Run::control(
            Control::Bookmark { name: bookmark_name, bookmark_type: BookmarkType::Point },
            char_shape_id,
        ));
    }

    for control in queues.object_controls {
        if let Some(run) = project_control_run(control, projection_images, image_context) {
            runs.push(run);
        }
    }

    // Drain any memo placeholders that did not get consumed by a matching
    // `FieldBegin %unk MEMO` inline segment. This is defensive — properly
    // anchored memos always consume their queue entry — but it preserves
    // memo body content rather than silently dropping it.
    for memo in queues.memo_controls {
        if !memo.paragraphs.is_empty() {
            projection_images.warnings.push(Hwp5Warning::ProjectionFallback {
                subject: "field.memo",
                reason: format!(
                    "memo_id={} had no matching FieldBegin anchor; \
                     emitting Run at end of paragraph",
                    memo.command.memo_id
                ),
            });
            runs.push(project_memo_run(
                &memo,
                projection_images,
                CharShapeIndex::new(0),
                Vec::new(),
            ));
        }
    }

    if runs.is_empty() {
        runs.push(Run::text("", CharShapeIndex::new(0)));
    }

    let mut paragraph =
        Paragraph::with_runs(runs, ParaShapeIndex::new(hwp_para.para_shape_id as usize));
    if hwp_para.style_id > 0 {
        paragraph = paragraph.with_style(StyleIndex::new(hwp_para.style_id as usize));
    }

    ProjectedParagraph { paragraph }
}

fn append_visible_unit(
    hwp_para: &Hwp5Paragraph,
    runs: &mut Vec<Run>,
    active_field: &mut Option<ActiveField>,
    visible_utf16: &mut u32,
    ch: char,
) {
    if let Some(active) = active_field.as_mut() {
        match active {
            ActiveField::Hyperlink { display_text, .. }
            | ActiveField::CrossRef { display_text, .. } => display_text.push(ch),
            ActiveField::BookmarkSpan { .. }
            | ActiveField::PlainTextFallback { .. }
            | ActiveField::MemoAnchor { .. }
            | ActiveField::ClickHere { .. }
            | ActiveField::SummeryField { .. }
            | ActiveField::DateCodeField { .. }
            | ActiveField::PathField { .. } => {}
        }
    } else {
        runs.extend(project_text_segment(
            &hwp_para.text,
            &hwp_para.char_shape_runs,
            *visible_utf16,
            *visible_utf16 + 1,
        ));
    }
    *visible_utf16 += 1;
}

fn paragraph_needs_structural_projection(hwp_para: &Hwp5Paragraph) -> bool {
    hwp_para
        .text_segments
        .iter()
        .any(|segment| matches!(segment, crate::schema::section::TextSegment::FieldBegin { .. }))
        || hwp_para.controls.iter().any(|control| {
            matches!(
                control,
                Hwp5Control::Unknown { ctrl_id: CTRL_ID_PAGE_NUMBER | CTRL_ID_BOOKMARK_POINT, .. }
            ) || matches!(control, Hwp5Control::Memo(_))
                || matches!(control, Hwp5Control::ClickHere(_))
                || matches!(control, Hwp5Control::SummeryField(_))
                || matches!(control, Hwp5Control::DateCodeField(_))
                || matches!(control, Hwp5Control::PathField(_))
                || matches!(control, Hwp5Control::CrossRef(_))
                || matches!(control, Hwp5Control::InlinePageNumber(_))
        })
}

fn build_paragraph_projection_queues<'a>(
    hwp_para: &'a Hwp5Paragraph,
    projection_images: &mut ProjectionImageState<'_>,
    field_hints: Option<&mut SectionProjectionHints>,
) -> ParagraphProjectionQueues<'a> {
    let mut marker_headers = VecDeque::new();
    let mut object_controls = VecDeque::new();
    let mut memo_controls = VecDeque::new();
    let mut clickhere_controls = VecDeque::new();
    let mut summery_fields = VecDeque::new();
    let mut datecode_fields = VecDeque::new();
    let mut pathfield_controls = VecDeque::new();
    let mut crossref_controls = VecDeque::new();
    let mut point_bookmark_names = VecDeque::new();
    let mut field_hints = field_hints;

    for control in &hwp_para.controls {
        // Memos consume a dedicated queue so the `FieldBegin %unk MEMO`
        // inline segment can pull the matching placeholder without
        // entangling object/marker dispatch.
        if let Hwp5Control::Memo(memo) = control {
            memo_controls.push_back(memo.clone());
            continue;
        }
        // ClickHere press-fields (Wave 12l) — same dedicated-queue
        // pattern as memos. Hint/help/name live in the parsed control;
        // the inline `FieldBegin %clk` marker pulls the next entry off
        // this queue in `start_active_field`.
        if let Hwp5Control::ClickHere(clickhere) = control {
            clickhere_controls.push_back(clickhere.clone());
            continue;
        }
        // SUMMERY auto-fields (Wave 12n) — same pattern. The Command
        // token lives in the parsed control; `FieldBegin %smr` pulls the
        // next entry off this queue.
        if let Hwp5Control::SummeryField(summery) = control {
            summery_fields.push_back(summery.clone());
            continue;
        }
        // `%dte` date/time format-code fields (Wave 12n) — same pattern.
        if let Hwp5Control::DateCodeField(date_code) = control {
            datecode_fields.push_back(date_code.clone());
            continue;
        }
        // `%pat` path fields (Wave 12n) — same pattern.
        if let Hwp5Control::PathField(pat) = control {
            pathfield_controls.push_back(pat.clone());
            continue;
        }
        // `%xrf` cross-reference fields (Wave 12m Phase 2 Step 4) — same
        // dedicated-queue pattern. The structured Command (RefType /
        // ContentType / hyperlink / target) lives in the parsed control;
        // `FieldBegin %xrf` pulls the next entry off this queue.
        if let Hwp5Control::CrossRef(xrf) = control {
            crossref_controls.push_back(xrf.clone());
            continue;
        }
        // `atno` inline page-number controls (Wave 12n) intentionally
        // fall through to `object_controls`. Codex 4차 review: atno's
        // ParaText marker is `0x12 ControlRef`, not `0x03 FieldBegin`,
        // and atno has no `FieldEnd`. The TextSegment::ControlRef arm
        // in `project_paragraph_with_images_structural` pops the next
        // `object_controls` entry and routes typed
        // `Hwp5Control::InlinePageNumber` through `project_control_run`.
        let Some(unknown) = unknown_control_header(control) else {
            object_controls.push_back(control);
            continue;
        };

        match unknown.ctrl_id {
            CTRL_ID_SECTION_DEF
            | CTRL_ID_COLUMN_DEF
            | CTRL_ID_BOOKMARK_SPAN
            | CTRL_ID_HYPERLINK => marker_headers.push_back(unknown),
            // Page numbers are resolved at section level by
            // `find_section_page_number` (which also reaches `pgnp` controls
            // inside table cells). Skip here so it is not mistaken for a
            // generic object control.
            CTRL_ID_PAGE_NUMBER => {}
            CTRL_ID_BOOKMARK_POINT => {
                if let Some(name) =
                    field_hints.as_deref_mut().and_then(SectionProjectionHints::take_bookmark_name)
                {
                    point_bookmark_names.push_back(name);
                } else {
                    projection_images.warnings.push(Hwp5Warning::ProjectionFallback {
                        subject: "field.bookmark_point",
                        reason: "bookmark point name unavailable; dropping bookmark control"
                            .to_string(),
                    });
                }
            }
            _ => object_controls.push_back(control),
        }
    }

    ParagraphProjectionQueues {
        marker_headers,
        object_controls,
        memo_controls,
        clickhere_controls,
        summery_fields,
        datecode_fields,
        pathfield_controls,
        crossref_controls,
        point_bookmark_names,
    }
}

// Wave 12n added 3 more optional carriers (summery/datecode/pathfield) on
// top of the existing memo/clickhere set. Refactoring into a struct here
// would add boilerplate without solving anything — each carrier is
// independently `None` for every other CTRL_ID. Tracked as follow-up
// backlog refactor #90 (handle_top_level_record helper extraction).
#[allow(clippy::too_many_arguments)]
fn start_active_field(
    ctrl_id: u32,
    header: Option<UnknownControlHeader<'_>>,
    memo: Option<Hwp5MemoControl>,
    clickhere: Option<crate::schema::section::Hwp5ClickHereControl>,
    summery: Option<crate::schema::section::Hwp5SummeryControl>,
    datecode: Option<crate::schema::section::Hwp5DateCodeControl>,
    pathfield: Option<crate::schema::section::Hwp5PathFieldControl>,
    crossref: Option<crate::schema::section::Hwp5CrossRefControl>,
    start_utf16: u32,
    projection_images: &mut ProjectionImageState<'_>,
    field_hints: Option<&mut SectionProjectionHints>,
) -> ActiveField {
    match ctrl_id {
        CTRL_ID_MEMO_INLINE => {
            // Anchor body is preserved via the BookmarkSpan/PlainTextFallback
            // pattern; the memo Run is emitted in `finish_active_field` after
            // the anchor text.
            if let Some(memo) = memo {
                ActiveField::MemoAnchor { start_utf16, memo }
            } else {
                projection_images.warnings.push(Hwp5Warning::ProjectionFallback {
                    subject: "field.memo",
                    reason: "memo placeholder unavailable for inline anchor; \
                             preserving only visible text"
                        .to_string(),
                });
                ActiveField::PlainTextFallback { start_utf16 }
            }
        }
        CTRL_ID_HYPERLINK => {
            if let Some(url) = header.and_then(|header| parse_hyperlink_url(header.header_data)) {
                ActiveField::Hyperlink { url, start_utf16, display_text: String::new() }
            } else {
                projection_images.warnings.push(Hwp5Warning::ProjectionFallback {
                    subject: "field.hyperlink",
                    reason: "hyperlink url unavailable; preserving only visible text".to_string(),
                });
                ActiveField::PlainTextFallback { start_utf16 }
            }
        }
        CTRL_ID_BOOKMARK_SPAN => {
            if let Some(name) = field_hints.and_then(SectionProjectionHints::take_bookmark_name) {
                ActiveField::BookmarkSpan { name, start_utf16 }
            } else {
                projection_images.warnings.push(Hwp5Warning::ProjectionFallback {
                    subject: "field.bookmark_span",
                    reason: "bookmark span name unavailable; preserving only visible text"
                        .to_string(),
                });
                ActiveField::PlainTextFallback { start_utf16 }
            }
        }
        CTRL_ID_CROSSREF => {
            // Wave 12m Phase 2 Step 4: %xrf now flows through the typed
            // `Hwp5Control::CrossRef` schema, not `Unknown`. The structured
            // Command (target / N1..N4) lives in `crossref`; legacy
            // `header` (UnknownControlHeader) never arrives anymore for
            // %xrf and is ignored here.
            let _ = header;
            if let Some(control) = crossref {
                ActiveField::CrossRef { control, start_utf16, display_text: String::new() }
            } else {
                projection_images.warnings.push(Hwp5Warning::ProjectionFallback {
                    subject: "field.crossref",
                    reason: "cross-reference payload unavailable for inline anchor; \
                             preserving only visible text"
                        .to_string(),
                });
                ActiveField::PlainTextFallback { start_utf16 }
            }
        }
        CTRL_ID_CLICK_HERE => {
            if let Some(clickhere) = clickhere {
                // hint/help/name pulled from the decoded
                // `Hwp5ClickHereControl` (which already merged the
                // trailing 0x57 sub-record at the decoder boundary).
                ActiveField::ClickHere {
                    start_utf16,
                    hint_text: clickhere.hint_text,
                    help_text: clickhere.help_text,
                    name: clickhere.name,
                }
            } else {
                projection_images.warnings.push(Hwp5Warning::ProjectionFallback {
                    subject: "field.clickhere",
                    reason: "click-here press-field metadata unavailable; \
                             preserving only visible text"
                        .to_string(),
                });
                ActiveField::PlainTextFallback { start_utf16 }
            }
        }
        CTRL_ID_FIELD_SUMMERY => {
            if let Some(summery) = summery {
                ActiveField::SummeryField { start_utf16, command_token: summery.command_token }
            } else {
                projection_images.warnings.push(Hwp5Warning::ProjectionFallback {
                    subject: "field.summery",
                    reason: "summery auto-field metadata unavailable; \
                             preserving only visible text"
                        .to_string(),
                });
                ActiveField::PlainTextFallback { start_utf16 }
            }
        }
        CTRL_ID_FIELD_DATE_CODE => {
            if let Some(date_code) = datecode {
                ActiveField::DateCodeField {
                    start_utf16,
                    raw_command: date_code.raw_command,
                    raw_trailer: date_code.raw_trailer,
                }
            } else {
                projection_images.warnings.push(Hwp5Warning::ProjectionFallback {
                    subject: "field.date_code",
                    reason: "date-code field metadata unavailable; \
                             preserving only visible text"
                        .to_string(),
                });
                ActiveField::PlainTextFallback { start_utf16 }
            }
        }
        CTRL_ID_FIELD_PATH => {
            if let Some(pat) = pathfield {
                ActiveField::PathField { start_utf16, raw_command: pat.raw_command }
            } else {
                projection_images.warnings.push(Hwp5Warning::ProjectionFallback {
                    subject: "field.path",
                    reason: "path field metadata unavailable; \
                             preserving only visible text"
                        .to_string(),
                });
                ActiveField::PlainTextFallback { start_utf16 }
            }
        }
        _ => ActiveField::PlainTextFallback { start_utf16 },
    }
}

fn finish_active_field(
    field: ActiveField,
    hwp_para: &Hwp5Paragraph,
    end_utf16: u32,
    runs: &mut Vec<Run>,
    projection_images: &mut ProjectionImageState<'_>,
) {
    match field {
        ActiveField::Hyperlink { url, start_utf16, display_text } => {
            if display_text.is_empty() {
                return;
            }
            let char_shape_id = CharShapeIndex::new(char_shape_id_for_visible_position(
                &hwp_para.char_shape_runs,
                start_utf16,
            ) as usize);
            runs.push(Run::control(Control::Hyperlink { text: display_text, url }, char_shape_id));
        }
        ActiveField::BookmarkSpan { name, start_utf16 } => {
            let char_shape_id = CharShapeIndex::new(char_shape_id_for_visible_position(
                &hwp_para.char_shape_runs,
                start_utf16,
            ) as usize);
            runs.push(Run::control(
                Control::Bookmark { name: name.clone(), bookmark_type: BookmarkType::SpanStart },
                char_shape_id,
            ));
            runs.extend(project_text_segment(
                &hwp_para.text,
                &hwp_para.char_shape_runs,
                start_utf16,
                end_utf16,
            ));
            runs.push(Run::control(
                Control::Bookmark { name, bookmark_type: BookmarkType::SpanEnd },
                char_shape_id,
            ));
        }
        ActiveField::CrossRef { control, start_utf16, display_text } => {
            // Wave 12m Phase 2 Step 4: emit native `Control::CrossRef`.
            // Boundary functions decode the wire codes into typed
            // `RefType` / `RefContentType` / `RefTarget`. The HWPX
            // encoder embeds `display_text` between fieldBegin/fieldEnd.
            let char_shape_id = CharShapeIndex::new(char_shape_id_for_visible_position(
                &hwp_para.char_shape_runs,
                start_utf16,
            ) as usize);
            if display_text.is_empty() {
                // No visible body between FieldBegin/FieldEnd — preserve
                // any latent body text so users at least see the source
                // span. This mirrors the pre-Step-4 fallback.
                runs.extend(project_text_segment(
                    &hwp_para.text,
                    &hwp_para.char_shape_runs,
                    start_utf16,
                    end_utf16,
                ));
                return;
            }
            let ref_type = decode_hwp5_crossref_ref_type(control.ref_type_code);
            let content_type =
                decode_hwp5_crossref_content_type(control.ref_type_code, control.content_type_code);
            let target = decode_hwp5_crossref_target(&control.target_raw, control.ref_type_code);
            let as_hyperlink = control.hyperlink_code != 0;
            runs.push(Run::control(
                Control::CrossRef { target, ref_type, content_type, as_hyperlink, display_text },
                char_shape_id,
            ));
        }
        ActiveField::PlainTextFallback { start_utf16 } => {
            runs.extend(project_text_segment(
                &hwp_para.text,
                &hwp_para.char_shape_runs,
                start_utf16,
                end_utf16,
            ));
        }
        ActiveField::MemoAnchor { start_utf16, memo } => {
            // Capture the FieldBegin..FieldEnd span as `anchor_runs` *inside*
            // `Control::Memo`. The HWPX encoder then emits them between
            // `<hp:fieldBegin>` and `<hp:fieldEnd>` in the same `<hp:run>` —
            // the layout 한컴 uses for `[메모 시작]anchor[메모 끝]`. Emitting
            // anchor text as a separate Run *outside* the memo, as we did in
            // Wave 12e/12f-pre-fix, made 한컴 mis-render the end marker as
            // generic `[필드 끝]` because the field span was effectively
            // empty.
            let anchor_runs = project_text_segment(
                &hwp_para.text,
                &hwp_para.char_shape_runs,
                start_utf16,
                end_utf16,
            );
            let char_shape_id = CharShapeIndex::new(char_shape_id_for_visible_position(
                &hwp_para.char_shape_runs,
                start_utf16,
            ) as usize);
            runs.push(project_memo_run(&memo, projection_images, char_shape_id, anchor_runs));
        }
        ActiveField::ClickHere { start_utf16, hint_text, help_text, name } => {
            // Emit a single Control::Field Run at the span start. The
            // HWPX encoder builds `<fieldBegin> + visible hint + <fieldEnd>`
            // from this control, so we must *not* additionally project
            // the span text (HWP5 wire span is empty between
            // FIELD_BEGIN/FIELD_END; double-emitting would duplicate
            // hint as both placeholder and run text in HWPX).
            let char_shape_id = CharShapeIndex::new(char_shape_id_for_visible_position(
                &hwp_para.char_shape_runs,
                start_utf16,
            ) as usize);
            let _ = end_utf16; // span text intentionally not consumed
            runs.push(Run::control(
                Control::Field {
                    field_type: hwpforge_foundation::FieldType::ClickHere,
                    hint_text,
                    help_text,
                    name,
                },
                char_shape_id,
            ));
        }
        ActiveField::SummeryField { start_utf16, command_token } => {
            // Emit a single Run carrying either typed `Control::Field`
            // (for known `$X` tokens) or `Control::UnknownSummery` for
            // future-compat raw carry. The HWPX encoder renders the
            // value at display time, so the span text between
            // FIELD_BEGIN/FIELD_END is intentionally dropped (Wave 12n).
            let char_shape_id = CharShapeIndex::new(char_shape_id_for_visible_position(
                &hwp_para.char_shape_runs,
                start_utf16,
            ) as usize);
            let _ = end_utf16;
            let control = match hwpforge_foundation::FieldType::from_summery_token(&command_token) {
                Some(field_type) => {
                    Control::Field { field_type, hint_text: None, help_text: None, name: None }
                }
                None => Control::UnknownSummery { token: command_token },
            };
            runs.push(Run::control(control, char_shape_id));
        }
        ActiveField::DateCodeField { start_utf16, raw_command, raw_trailer } => {
            // Emit Control::DateCodeField with `is_time_mode` derived
            // from the `T` prefix convention. The 8-byte trailer is
            // preserved verbatim for future round-trip fidelity. Span
            // text is intentionally dropped (Wave 12n).
            let char_shape_id = CharShapeIndex::new(char_shape_id_for_visible_position(
                &hwp_para.char_shape_runs,
                start_utf16,
            ) as usize);
            let _ = end_utf16;
            let is_time_mode = raw_command.starts_with('T');
            runs.push(Run::control(
                Control::DateCodeField { raw_command, is_time_mode, raw_trailer },
                char_shape_id,
            ));
        }
        ActiveField::PathField { start_utf16, raw_command } => {
            // Map raw `$P`/`$F`/`$P$F` to a typed PathFieldCommand
            // (Unknown for forward compatibility). Wave 12n.
            let char_shape_id = CharShapeIndex::new(char_shape_id_for_visible_position(
                &hwp_para.char_shape_runs,
                start_utf16,
            ) as usize);
            let _ = end_utf16;
            use hwpforge_core::control::PathFieldCommand;
            let command = PathFieldCommand::from_wire(&raw_command);
            runs.push(Run::control(Control::PathField { command }, char_shape_id));
        }
    }
}

/// Projects a HWP5 memo placeholder into a Core `Run` carrying
/// `Control::Memo`. Body paragraphs come from the joined
/// `HWPTAG_MEMO_LIST` cluster (filled by the decoder during `finish`);
/// they are projected with the standard `Flow` context.
fn project_memo_run(
    memo: &Hwp5MemoControl,
    projection_images: &mut ProjectionImageState<'_>,
    char_shape_id: CharShapeIndex,
    anchor_runs: Vec<Run>,
) -> Run {
    let paragraphs = project_nested_paragraphs(
        &memo.paragraphs,
        projection_images,
        ImageProjectionContext::Flow,
    );
    // Map the parsed wire command onto the format-agnostic
    // `MemoMetadata`. `id` and `create_datetime` are left at their default
    // (empty) so the HWPX encoder derives `"memo{number}"` and a current-UTC
    // timestamp at emit time — wire never carried either field. We go
    // through `Default::default()` because `MemoMetadata` is
    // `#[non_exhaustive]` and can't be constructed positionally outside
    // `hwpforge-core`.
    let mut metadata = hwpforge_core::MemoMetadata::default();
    metadata.shape_id_ref = memo.command.shape_id;
    metadata.number = memo.command.memo_id;
    metadata.author = memo.command.author.clone();
    metadata.command = memo.command.raw.clone();
    Run::control(Control::Memo { content: paragraphs, anchor_runs, metadata }, char_shape_id)
}

/// Projects a HWP5 dutmal (덧말) control into a Core `Run` carrying
/// `Control::Dutmal`. Position is mapped from the wire's raw u32; other
/// metadata (align/sz_ratio/option/styleIDRef) defaults — every 한컴
/// fixture we've inspected leaves these at their default value, so we
/// don't promote them to fields until a future fixture forces fidelity
/// work. See `schema::section::Hwp5DutmalControl` for the wire layout.
fn project_dutmal_run(dutmal: &Hwp5DutmalControl) -> Run {
    let position = match dutmal.pos_type_raw {
        0 => hwpforge_core::control::DutmalPosition::Top,
        1 => hwpforge_core::control::DutmalPosition::Bottom,
        2 => hwpforge_core::control::DutmalPosition::Right,
        3 => hwpforge_core::control::DutmalPosition::Left,
        _ => hwpforge_core::control::DutmalPosition::Top,
    };
    let mut metadata = hwpforge_core::DutmalMetadata::default();
    metadata.option = dutmal.option_raw;
    Run::control(
        Control::Dutmal {
            main_text: dutmal.main_text.clone(),
            sub_text: dutmal.sub_text.clone(),
            position,
            sz_ratio: 0,
            align: hwpforge_core::control::DutmalAlign::Center,
            metadata,
        },
        CharShapeIndex::new(0),
    )
}

/// Gathers each `Hwp5Control::Header` subtree separately, returning a
/// `(projected paragraphs, raw 4-byte properties)` tuple per ctrl.
///
/// ADR-002 + gap A: cardinality is preserved (one tuple per `head`
/// ctrl) so projection can map each ctrl to its own `<hp:header
/// applyPageType="..."/>` element.
fn collect_header_subtrees(
    paragraph: &Hwp5Paragraph,
    projection_images: &mut ProjectionImageState<'_>,
) -> Vec<(Vec<Paragraph>, u32)> {
    collect_subtree_units(paragraph, projection_images, |control| match control {
        Hwp5Control::Header(subtree) => Some((&subtree.paragraphs, subtree.properties_raw)),
        _ => None,
    })
}

/// Mirror of [`collect_header_subtrees`] for `Hwp5Control::Footer`.
fn collect_footer_subtrees(
    paragraph: &Hwp5Paragraph,
    projection_images: &mut ProjectionImageState<'_>,
) -> Vec<(Vec<Paragraph>, u32)> {
    collect_subtree_units(paragraph, projection_images, |control| match control {
        Hwp5Control::Footer(subtree) => Some((&subtree.paragraphs, subtree.properties_raw)),
        _ => None,
    })
}

/// Decode the `applyPageType` semantic from a HWP5 head/foot ctrl's
/// raw property word (HWP 5.0 spec §4.3.10.3 표 141).
fn hwp5_header_property_to_apply_page_type(
    properties_raw: u32,
) -> hwpforge_foundation::ApplyPageType {
    use hwpforge_foundation::ApplyPageType;
    match properties_raw & 0b11 {
        1 => ApplyPageType::Even,
        2 => ApplyPageType::Odd,
        // 0 (BOTH) and any unspecified/extension bits default to Both.
        _ => ApplyPageType::Both,
    }
}

/// Decode the `secd` ctrl property word (HWP 5.0 spec §4.3.10.1
/// 표 130) into Core's [`Visibility`](hwpforge_core::section::Visibility).
///
/// Bit-to-field mapping (matches HWPX `<hp:visibility>` 1:1):
///
/// | bit | spec gloss | Core field |
/// |----:|------------|------------|
/// | 0 | 머리말을 감출지 여부 | `hide_first_header` |
/// | 1 | 꼬리말을 감출지 여부 | `hide_first_footer` |
/// | 2 | 바탕쪽을 감출지 여부 | `hide_first_master_page` |
/// | 3 | 테두리를 감출지 여부 | (informational; Core `border` enum is `ShowMode`) |
/// | 4 | 배경을 감출지 여부   | (informational; `fill` is `ShowMode`) |
/// | 5 | 쪽 번호 위치를 감출지 여부 | `hide_first_page_num` |
/// | 19 | 빈 줄 감춤 여부 | `hide_first_empty_line` |
///
/// `border` / `fill` themselves stay at their `ShowMode::ShowAll`
/// default — these are full-section visibility, not first-page-only,
/// and a separate slice promotes them.
fn hwp5_section_properties_to_visibility(properties: u32) -> hwpforge_core::section::Visibility {
    hwpforge_core::section::Visibility {
        hide_first_header: (properties & 1) != 0,
        hide_first_footer: (properties & (1 << 1)) != 0,
        hide_first_master_page: (properties & (1 << 2)) != 0,
        hide_first_page_num: (properties & (1 << 5)) != 0,
        hide_first_empty_line: (properties & (1 << 19)) != 0,
        ..hwpforge_core::section::Visibility::default()
    }
}

/// Maps decoded `HWPTAG_PAGE_BORDER_FILL` records to Core
/// [`PageBorderFillEntry`] values.
///
/// `apply_type` (`BOTH` / `EVEN` / `ODD`) is **not** carried inside each
/// record — it is purely positional. 한글 writes exactly three records in
/// `[BOTH, EVEN, ODD]` order, so the index selects `apply_type`. (The
/// EVEN/ODD records are byte-identical in the common "no border" case, so
/// only the leading `BOTH` slot has been empirically confirmed; see the
/// backlog note in `.docs/debug/2026-05-29_hwp5_page_border_fill.md`.)
///
/// Per the project's "warning-first for unknowns" rule, a record count
/// other than three is surfaced as a `ProjectionFallback` warning and the
/// mapping is bounded to the three known slots so we never silently emit a
/// duplicate `ODD` entry.
///
/// `border_fill_id` indexes the HWPX style store directly (1-based, no
/// remapping — the borderFill definitions decode into the store with
/// matching ids).
///
/// `properties` bit semantics (verified against the
/// `sample-page-border-fill` 한글 fixture; only this fixture so far, so
/// the bit-0 → text-border mapping is asserted from observed truth):
/// - bit 0: border base — set → `"PAPER"` (paper edge), clear →
///   `"CONTENT"` (text area)
/// - bit 1 / 2: include header / footer in the border area
/// - bit 3: fill area — set → `"PAGE"`, clear → `"PAPER"`
fn hwp5_page_border_fills_to_entries(
    records: &[Hwp5PageBorderFill],
    warnings: &mut Vec<Hwp5Warning>,
) -> Vec<PageBorderFillEntry> {
    if records.len() != 3 {
        warnings.push(Hwp5Warning::ProjectionFallback {
            subject: "page_border_fill.count",
            reason: format!(
                "expected 3 page border fill records ([BOTH, EVEN, ODD]); found {}. \
                 apply_type is positional, so mapping the first 3 by index",
                records.len()
            ),
        });
    }
    records
        .iter()
        .take(3)
        .enumerate()
        .map(|(idx, rec)| {
            let apply_type = match idx {
                0 => "BOTH",
                1 => "EVEN",
                _ => "ODD",
            };
            let text_border = if rec.properties & 0b1 != 0 { "PAPER" } else { "CONTENT" };
            let fill_area = if rec.properties & 0b1000 != 0 { "PAGE" } else { "PAPER" };
            let offset = |raw: u16| HwpUnit::new(i32::from(raw)).unwrap_or_default();
            PageBorderFillEntry {
                apply_type: apply_type.to_string(),
                border_fill_id: u32::from(rec.border_fill_id),
                text_border: text_border.to_string(),
                header_inside: rec.properties & 0b10 != 0,
                footer_inside: rec.properties & 0b100 != 0,
                fill_area: fill_area.to_string(),
                offset: [
                    offset(rec.offsets[0]),
                    offset(rec.offsets[1]),
                    offset(rec.offsets[2]),
                    offset(rec.offsets[3]),
                ],
            }
        })
        .collect()
}

/// Cardinality-preserving collector for header/footer-style ctrls:
/// returns one `(projected paragraphs, extra)` tuple per matching ctrl
/// instead of flattening across all ctrls. Used by gap A to keep each
/// `head`/`foot` ctrl separable for `applyPageType` decoding.
fn collect_subtree_units<F, X>(
    paragraph: &Hwp5Paragraph,
    projection_images: &mut ProjectionImageState<'_>,
    unit_for_control: F,
) -> Vec<(Vec<Paragraph>, X)>
where
    F: Fn(&Hwp5Control) -> Option<(&Vec<Hwp5Paragraph>, X)>,
{
    let mut units: Vec<(Vec<Paragraph>, X)> = Vec::new();
    for control in &paragraph.controls {
        if let Some((nested_paragraphs, extra)) = unit_for_control(control) {
            let projected = project_nested_paragraphs(
                nested_paragraphs,
                projection_images,
                ImageProjectionContext::Flow,
            );
            units.push((projected, extra));
        }
    }
    units
}

fn project_nested_paragraphs(
    paragraphs: &[Hwp5Paragraph],
    projection_images: &mut ProjectionImageState<'_>,
    image_context: ImageProjectionContext,
) -> Vec<Paragraph> {
    paragraphs
        .iter()
        .map(|nested| {
            project_paragraph_with_images(nested, projection_images, image_context, None).paragraph
        })
        .collect()
}

fn unknown_control_header(control: &Hwp5Control) -> Option<UnknownControlHeader<'_>> {
    match control {
        Hwp5Control::Unknown { ctrl_id, header_data } => {
            Some(UnknownControlHeader { ctrl_id: *ctrl_id, header_data })
        }
        _ => None,
    }
}

fn ctrl_id_from_inline_extra(extra: &[u8; 14]) -> u32 {
    u32::from_be_bytes([extra[3], extra[2], extra[1], extra[0]])
}

fn consume_marker_header<'a>(
    marker_headers: &mut VecDeque<UnknownControlHeader<'a>>,
    expected_ctrl_id: u32,
) -> Option<UnknownControlHeader<'a>> {
    let front = marker_headers.front().copied()?;
    if front.ctrl_id == expected_ctrl_id {
        return marker_headers.pop_front();
    }
    None
}

fn parse_utf16_command_string(header_data: &[u8]) -> Option<String> {
    if header_data.len() < 10 {
        return None;
    }
    let char_len = u16::from_be_bytes([header_data[8], header_data[9]]) as usize;
    let byte_len = char_len.checked_mul(2)?;
    let end = 10usize.checked_add(byte_len)?;
    if header_data.len() < end {
        return None;
    }
    let units: Vec<u16> = header_data[10..end]
        .chunks_exact(2)
        .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
        .collect();
    String::from_utf16(&units).ok()
}

fn parse_hyperlink_url(header_data: &[u8]) -> Option<String> {
    let command = parse_utf16_command_string(header_data)?;
    let raw_url =
        command.split('|').next().unwrap_or(&command).split(';').next().unwrap_or(&command);
    Some(raw_url.replace("\\:", ":"))
}

/// Wave 12m Phase 2 Step 4 boundary: HWP5 `%xrf` N1 (RefType) wire code
/// → typed [`RefType`]. Unknown codes are preserved as
/// `RefType::Unknown(u8)`. Keeps the projection layer free from raw
/// `u8`-vs-enum knowledge.
fn decode_hwp5_crossref_ref_type(code: u8) -> RefType {
    match code {
        HWP5_CROSSREF_REF_TYPE_TABLE => RefType::Table,
        HWP5_CROSSREF_REF_TYPE_FIGURE => RefType::Figure,
        HWP5_CROSSREF_REF_TYPE_EQUATION => RefType::Equation,
        HWP5_CROSSREF_REF_TYPE_FOOTNOTE => RefType::Footnote,
        HWP5_CROSSREF_REF_TYPE_ENDNOTE => RefType::Endnote,
        HWP5_CROSSREF_REF_TYPE_OUTLINE => RefType::Outline,
        HWP5_CROSSREF_REF_TYPE_BOOKMARK => RefType::Bookmark,
        other => RefType::Unknown(other),
    }
}

/// Wave 12p pre-fix boundary: HWP5 `%xrf` N2 (ContentType) is
/// RefType-relative. 한컴 native wire 분석 결과:
///
/// | RefType        | N2=0 | N2=1   | N2=2          | N2=3      |
/// |----------------|------|--------|---------------|-----------|
/// | Bookmark       | Page | Number | BookmarkName  | UpDownPos |
/// | 그 외 (T/F/Eq/…) | Page | Number | Contents      | UpDownPos |
///
/// 책갈피 N2=1 은 한컴에서 "책갈피 본문/번호" 의미 (OBJECT_TYPE_NUMBER
/// emit), N2=2 는 "책갈피 이름" (OBJECT_TYPE_CONTENTS emit). spec 외
/// 의미이지만 native wire 와 일치. Wave 12m fixup 의 (Bookmark, 2) →
/// Contents 통일은 잘못이었고 본 fix 에서 보정.
fn decode_hwp5_crossref_content_type(ref_type_code: u8, code: u8) -> RefContentType {
    match (ref_type_code, code) {
        (_, 0) => RefContentType::Page,
        (HWP5_CROSSREF_REF_TYPE_BOOKMARK, 1) => RefContentType::Number,
        (HWP5_CROSSREF_REF_TYPE_BOOKMARK, 2) => RefContentType::BookmarkName,
        (_, 1) => RefContentType::Number,
        (_, 2) => RefContentType::Contents,
        (_, 3) => RefContentType::UpDownPos,
        (_, other) => RefContentType::Unknown(other),
    }
}

/// Wave 12m Phase 2 Step 4 boundary: HWP5 `%xrf` Command's target slot
/// → typed [`RefTarget`]. Bookmark refs (`ref_type_code == 6`) carry a
/// raw bookmark NAME; other refs carry a `#<u64>` SystemId. Anything
/// else lands in `RefTarget::Raw` (no fabrication).
fn decode_hwp5_crossref_target(target_raw: &str, ref_type_code: u8) -> RefTarget {
    if ref_type_code == HWP5_CROSSREF_REF_TYPE_BOOKMARK {
        return RefTarget::Name(target_raw.to_string());
    }
    if let Some(rest) = target_raw.strip_prefix('#') {
        if let Ok(id) = rest.parse::<u64>() {
            return RefTarget::SystemId(id);
        }
    }
    RefTarget::Raw(target_raw.to_string())
}

/// Resolves a `pgnp` (page-number) control header into a [`PageNumber`],
/// falling back to a BOTTOM_CENTER digit page number (with a warning) when the
/// decoration payload can't be parsed.
fn page_number_from_pgnp_header(header_data: &[u8], warnings: &mut Vec<Hwp5Warning>) -> PageNumber {
    parse_page_number_control(header_data).unwrap_or_else(|| {
        warnings.push(Hwp5Warning::ProjectionFallback {
            subject: "field.page_number",
            reason: "falling back to BOTTOM_CENTER digit page number".to_string(),
        });
        PageNumber::with_decoration(
            PageNumberPosition::BottomCenter,
            NumberFormatType::Digit,
            "-".to_string(),
        )
    })
}

/// Finds the section's page number: the first `pgnp` control anywhere in the
/// section body, including inside layout-table cells (recursively).
///
/// A page number is a section-level property even when 한글 stores its control
/// inside a table cell, so a body-paragraph-only scan misses those. The search
/// short-circuits on the first match. Header/footer/note subtrees are
/// intentionally excluded — those carry their own content and are projected
/// separately.
fn find_section_page_number(
    paragraphs: &[Hwp5Paragraph],
    warnings: &mut Vec<Hwp5Warning>,
) -> Option<PageNumber> {
    for paragraph in paragraphs {
        for control in &paragraph.controls {
            match control {
                Hwp5Control::Unknown { ctrl_id: CTRL_ID_PAGE_NUMBER, header_data } => {
                    return Some(page_number_from_pgnp_header(header_data, warnings));
                }
                Hwp5Control::Table(table) => {
                    for cell in &table.cells {
                        if let Some(found) = find_section_page_number(&cell.paragraphs, warnings) {
                            return Some(found);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    None
}

fn parse_page_number_control(header_data: &[u8]) -> Option<PageNumber> {
    let pos_code = *header_data.get(5)?;
    let position = match pos_code {
        0 => PageNumberPosition::None,
        1 => PageNumberPosition::TopLeft,
        2 => PageNumberPosition::TopCenter,
        3 => PageNumberPosition::TopRight,
        4 => PageNumberPosition::BottomLeft,
        5 => PageNumberPosition::BottomCenter,
        6 => PageNumberPosition::BottomRight,
        7 => PageNumberPosition::OutsideTop,
        8 => PageNumberPosition::OutsideBottom,
        9 => PageNumberPosition::InsideTop,
        10 => PageNumberPosition::InsideBottom,
        _ => PageNumberPosition::BottomCenter,
    };
    let decoration = header_data
        .iter()
        .rev()
        .find(|byte| **byte != 0)
        .copied()
        .filter(|byte| byte.is_ascii())
        .map(|byte| char::from(byte).to_string())
        .unwrap_or_else(|| "-".to_string());
    Some(PageNumber::with_decoration(position, NumberFormatType::Digit, decoration))
}

fn char_shape_id_for_visible_position(runs: &[Hwp5CharShapeRun], position: u32) -> u32 {
    if position == 0 {
        return char_shape_id_at_position(runs, 0);
    }
    char_shape_id_at_position(runs, position.saturating_sub(1))
}


// ---------------------------------------------------------------------------
// Text splitting
// ---------------------------------------------------------------------------

fn project_control_run(
    control: &Hwp5Control,
    projection_images: &mut ProjectionImageState<'_>,
    image_context: ImageProjectionContext,
) -> Option<Run> {
    match control {
        Hwp5Control::Table(table) => Some(Run::table(
            build_table_with_images(table, projection_images),
            CharShapeIndex::new(0),
        )),
        Hwp5Control::Image(image) => projection_images
            .build_image(image, image_context)
            .map(|core_image| Run::image(core_image, CharShapeIndex::new(0))),
        Hwp5Control::Line(line) => Some(project_line_run(line)),
        Hwp5Control::Rect(rect) => project_rect_run(rect),
        Hwp5Control::Polygon(polygon) => Some(project_polygon_run(polygon)),
        Hwp5Control::Ellipse(ellipse) => project_ellipse_run(ellipse),
        Hwp5Control::Arc(arc) => project_arc_run(arc),
        Hwp5Control::Curve(curve) => project_curve_run(curve),
        Hwp5Control::ConnectLine(connect_line) => project_connectline_run(connect_line),
        Hwp5Control::Equation(equation) => Some(project_equation_run(equation)),
        Hwp5Control::TextBox(textbox) => Some(project_textbox_run(textbox, projection_images)),
        Hwp5Control::Footnote(subtree) => Some(project_footnote_run(subtree, projection_images)),
        Hwp5Control::Endnote(subtree) => Some(project_endnote_run(subtree, projection_images)),
        // Memo emission flows through the `FieldBegin`/`MemoAnchor` machinery in
        // `project_paragraph_with_images_structural`, not through this dispatch.
        // If a Memo control ever reaches here (no matching FieldBegin in text
        // segments), prefer dropping over silently double-emitting.
        Hwp5Control::Memo(_)
        | Hwp5Control::Header(_)
        | Hwp5Control::Footer(_)
        | Hwp5Control::Unknown { .. } => None,
        Hwp5Control::Dutmal(dutmal) => Some(project_dutmal_run(dutmal)),
        Hwp5Control::Compose(compose) => Some(project_compose_run(compose)),
        Hwp5Control::IndexMark(indexmark) => Some(project_indexmark_run(indexmark)),
        // ClickHere emission flows through the `FieldBegin`/`ActiveField::ClickHere`
        // machinery in `project_paragraph_with_images_structural` (mirroring the
        // Memo dispatch above). If a ClickHere ever reaches this flat dispatch
        // path it means the structural pairing failed — drop rather than
        // silently emit a free-floating field run.
        Hwp5Control::ClickHere(_) => None,
        // SUMMERY auto-fields (Wave 12n) follow the same structural-pairing
        // pattern as ClickHere. Free-floating SummeryField means the inline
        // FieldBegin marker did not pair with this CtrlHeader; drop.
        Hwp5Control::SummeryField(_) => None,
        // %dte date/time format-code fields (Wave 12n) — same pattern.
        Hwp5Control::DateCodeField(_) => None,
        // %pat path fields (Wave 12n) — same pattern.
        Hwp5Control::PathField(_) => None,
        // atno inline page-number controls (Wave 12n) emit immediately.
        // The 0x12 inline marker is a ControlRef (no FieldEnd), so the
        // emission flows through the object-control queue, not an
        // ActiveField/FieldBegin pair.
        Hwp5Control::InlinePageNumber(atno) => {
            let kind = hwpforge_core::control::InlinePageKind::from_raw_flag(atno.raw_flag);
            Some(Run::control(
                Control::InlinePageNumber { kind, raw_flag: atno.raw_flag },
                CharShapeIndex::new(0),
            ))
        }
        Hwp5Control::OleObject(ole) => project_ole_object_run(ole, projection_images),
        // %xrf cross-reference fields (Wave 12m) flow through the
        // `FieldBegin`/`ActiveField::CrossRef` machinery in
        // `project_paragraph_with_images_structural` — same pattern as
        // ClickHere / SummeryField / DateCodeField / PathField. A
        // free-floating CrossRef CtrlHeader means the inline `FieldBegin`
        // marker did not pair with it; drop rather than silently emit.
        Hwp5Control::CrossRef(_) => None,
    }
}

/// Projects a HWP5 IndexMark (찾아보기 표시) control into a Core
/// `Run` carrying `Control::IndexMark`. The wire's `secondary_units_len
/// == 0` case is decoded as `None` rather than `Some("")` — 한컴
/// HWP5 cannot distinguish the two on save, so the decode matches
/// 한컴's intent. See
/// `.docs/algorithms/2026-06-02_indexmark_carry.md` for the
/// Codex-reviewed empty-secondary discussion.
fn project_indexmark_run(indexmark: &crate::schema::section::Hwp5IndexMarkControl) -> Run {
    Run::control(
        Control::IndexMark {
            primary: indexmark.primary.clone(),
            secondary: indexmark.secondary.clone(),
        },
        CharShapeIndex::new(0),
    )
}

/// Projects a HWP5 compose (글자겹침) control into a Core `Run`
/// carrying `Control::Compose`. Raw `circle_type` and `compose_type`
/// bytes are mapped to the OWPML enum strings 한컴 expects on the
/// HWPX side; unknown values fall back to the spec defaults so the
/// HWPX encoder always emits a well-formed `<hp:compose>` element.
/// See `.docs/algorithms/2026-06-01_compose_carry.md` for the layout
/// rationale and enum-mapping tables.
fn project_compose_run(compose: &crate::schema::section::Hwp5ComposeControl) -> Run {
    let circle_type = compose_circle_type_label(compose.circle_type_raw);
    let compose_type = compose_compose_type_label(compose.compose_type_raw);
    Run::control(
        Control::Compose {
            compose_text: compose.compose_text.clone(),
            circle_type: circle_type.to_string(),
            char_sz: i32::from(compose.char_sz),
            compose_type: compose_type.to_string(),
            char_pr_ids: compose.char_pr_ids.clone(),
        },
        CharShapeIndex::new(0),
    )
}

/// Maps the OWPML `SHAPECIRCLETYPE` enum (defined in
/// `.docs/references/hwpx-owpml-model/OWPML/Class/enumdef.h` lines
/// 623-639) from the raw wire byte to the HWPX attribute string.
fn compose_circle_type_label(raw: u8) -> &'static str {
    match raw {
        0 => "CHAR",
        1 => "SHAPE_CIRCLE",
        2 => "SHAPE_REVERSAL_CIRCLE",
        3 => "SHAPE_RECTANGLE",
        4 => "SHAPE_REVERSAL_RECTANGLE",
        5 => "SHAPE_TRIANGLE",
        // 한컴의 공식 spec 오타 — `TIRANGLE` (not TRIANGLE) 그대로 보존해야
        // HWPX truth와 round-trip이 닫힌다.
        6 => "SHAPE_REVERSAL_TIRANGLE",
        7 => "SHAPE_LIGHT",
        8 => "SHAPE_RHOMBUS",
        9 => "SHAPE_REVERSAL_RHOMBUS",
        10 => "SHAPE_ROUNDED_RECTANGLE",
        11 => "SHAPE_EMPTY_CIRCULATE_TRIANGLE",
        12 => "SHAPE_THIN_CIRCULATE_TRIANGLE",
        13 => "SHAPE_THICK_CIRCULATE_TRIANGLE",
        // Unknown values fall back to the spec default ("CHAR"). 한컴이
        // verify 단계에서 unknown을 받으면 거부할 수 있으니 안전 기본값.
        _ => "CHAR",
    }
}

/// Maps the OWPML `COMPOSETYPE` enum (`enumdef.h` lines 661-665) from
/// the raw wire byte to the HWPX attribute string.
fn compose_compose_type_label(raw: u8) -> &'static str {
    match raw {
        0 => "SPREAD",
        1 => "OVERLAP",
        _ => "SPREAD",
    }
}

/// Projects a HWP5 OLE object control into a Core run.
///
/// HWP5 represents charts as OLE-backed BinData blobs (DEFLATE-compressed
/// `.OLE` streams whose inner OLE2 carries `/OOXMLChartContents`). When the
/// payload is recognizable as a chart we carry it as
/// [`Control::EmbeddedChart`] (Wave 4c); otherwise we fall back to a
/// `DroppedControl:ole_object` warning whose reason explains why.
///
/// Requires a populated [`Hwp5OleAssetPlan`] in `projection_images`; if no
/// plan is wired (e.g. inspect-only paths), we drop with a clear reason.
fn project_ole_object_run(
    ole: &Hwp5OleObjectControl,
    projection_images: &mut ProjectionImageState<'_>,
) -> Option<Run> {
    let Some(raw_bytes) = projection_images.ole_bytes_for_binary_data_id(ole.binary_data_id) else {
        projection_images.warnings.push(Hwp5Warning::DroppedControl {
            control: "ole_object",
            reason: format!("ole_bin_data_unavailable binary_data_id={}", ole.binary_data_id),
        });
        return None;
    };

    match extract_chart_payload(raw_bytes) {
        Ok(payload) => {
            // Dimensions come from the `ShapeComponentOle` extent fields,
            // which the HWP5 decoder already stored as i32 HWPUNIT. The
            // geometry x/y mirror the placement convention used by the
            // other shape projections (zero-offset == inline).
            let Some(width) = chart_dimension(ole.extent_width) else {
                projection_images.warnings.push(Hwp5Warning::DroppedControl {
                    control: "ole_object",
                    reason: format!(
                        "ole_chart_invalid_width binary_data_id={} width={}",
                        ole.binary_data_id, ole.extent_width
                    ),
                });
                return None;
            };
            let Some(height) = chart_dimension(ole.extent_height) else {
                projection_images.warnings.push(Hwp5Warning::DroppedControl {
                    control: "ole_object",
                    reason: format!(
                        "ole_chart_invalid_height binary_data_id={} height={}",
                        ole.binary_data_id, ole.extent_height
                    ),
                });
                return None;
            };

            Some(Run::control(
                Control::EmbeddedChart {
                    chart_xml: payload.chart_xml,
                    ole_bytes: payload.ole_bytes,
                    width,
                    height,
                    horz_offset: ole.geometry.x,
                    vert_offset: ole.geometry.y,
                },
                CharShapeIndex::new(0),
            ))
        }
        Err(ChartOleError::NotChart) => {
            // Genuine non-chart OLE (e.g. embedded HWP table, Excel sheet).
            // We do not yet have a passthrough story for those — keep
            // the drop warning but with a more specific reason than before.
            projection_images.warnings.push(Hwp5Warning::DroppedControl {
                control: "ole_object",
                reason: format!("ole_payload_not_chart binary_data_id={}", ole.binary_data_id),
            });
            None
        }
        Err(err) => {
            projection_images.warnings.push(Hwp5Warning::DroppedControl {
                control: "ole_object",
                reason: format!(
                    "ole_extract_failed binary_data_id={} detail={}",
                    ole.binary_data_id, err
                ),
            });
            None
        }
    }
}

/// Convert HWP5 OLE extent (i32 HWPUNIT, possibly zero) into a strictly
/// positive [`HwpUnit`] suitable for [`Control::EmbeddedChart`].
fn chart_dimension(value: i32) -> Option<HwpUnit> {
    if value <= 0 {
        return None;
    }
    HwpUnit::new(value).ok()
}

fn project_textbox_run(
    textbox: &Hwp5TextBoxControl,
    projection_images: &mut ProjectionImageState<'_>,
) -> Run {
    let paragraphs = project_nested_paragraphs(
        &textbox.paragraphs,
        projection_images,
        ImageProjectionContext::TextBox,
    );
    Run::control(
        Control::TextBox {
            paragraphs,
            width: hwp_unit_from_u32(textbox.geometry.width),
            height: hwp_unit_from_u32(textbox.geometry.height),
            horz_offset: textbox.geometry.x,
            vert_offset: textbox.geometry.y,
            caption: None,
            style: None,
        },
        CharShapeIndex::new(0),
    )
}

/// Projects a HWP5 footnote subtree into a Core `Run` carrying `Control::Footnote`.
///
/// HWP5 does not carry a stable `instId` for footnotes the way HWPX does; the
/// surrounding `CtrlHeader`/inline 0x06 marker does not expose one to the
/// decoder layer. We therefore leave `inst_id` as `None` and let the HWPX
/// encoder generate placement-specific ids if it needs to (its existing
/// encoder uses `Option<u32>` and serializes the attribute only when set).
fn project_footnote_run(
    subtree: &Hwp5NestedSubtree,
    projection_images: &mut ProjectionImageState<'_>,
) -> Run {
    let paragraphs = project_nested_paragraphs(
        &subtree.paragraphs,
        projection_images,
        ImageProjectionContext::Flow,
    );
    Run::control(Control::Footnote { inst_id: None, paragraphs }, CharShapeIndex::new(0))
}

/// Projects a HWP5 endnote subtree into a Core `Run` carrying `Control::Endnote`.
///
/// Same caveat as [`project_footnote_run`]: HWP5 ctrl payload does not surface
/// an `instId`, so we leave it `None`.
fn project_endnote_run(
    subtree: &Hwp5NestedSubtree,
    projection_images: &mut ProjectionImageState<'_>,
) -> Run {
    let paragraphs = project_nested_paragraphs(
        &subtree.paragraphs,
        projection_images,
        ImageProjectionContext::Flow,
    );
    Run::control(Control::Endnote { inst_id: None, paragraphs }, CharShapeIndex::new(0))
}

fn project_line_run(line: &Hwp5LineControl) -> Run {
    let projected_start = scale_point_into_geometry(
        line.start,
        line.start.x.min(line.end.x),
        line.start.x.max(line.end.x),
        line.geometry.width,
        100,
        Axis::Horizontal,
    );
    let projected_end = scale_point_into_geometry(
        line.end,
        line.start.x.min(line.end.x),
        line.start.x.max(line.end.x),
        line.geometry.width,
        100,
        Axis::Horizontal,
    );
    let projected_start_y = scale_point_into_geometry(
        line.start,
        line.start.y.min(line.end.y),
        line.start.y.max(line.end.y),
        line.geometry.height,
        100,
        Axis::Vertical,
    );
    let projected_end_y = scale_point_into_geometry(
        line.end,
        line.start.y.min(line.end.y),
        line.start.y.max(line.end.y),
        line.geometry.height,
        100,
        Axis::Vertical,
    );

    let scaled_start =
        hwpforge_core::control::ShapePoint { x: projected_start, y: projected_start_y };
    let scaled_end = hwpforge_core::control::ShapePoint { x: projected_end, y: projected_end_y };
    let mut control = hwpforge_core::control::Control::line(scaled_start, scaled_end)
        .expect("scaled line points remain non-degenerate");
    if let Control::Line { horz_offset, vert_offset, .. } = &mut control {
        *horz_offset = line.geometry.x;
        *vert_offset = line.geometry.y;
    }
    Run::control(control, CharShapeIndex::new(0))
}

fn project_rect_run(rect: &Hwp5RectControl) -> Option<Run> {
    let width = HwpUnit::new(positive_i32_from_u32(rect.geometry.width)?).ok()?;
    let height = HwpUnit::new(positive_i32_from_u32(rect.geometry.height)?).ok()?;
    let mut control = hwpforge_core::control::Control::rect(width, height).ok()?;
    if let Control::Rect { horz_offset, vert_offset, .. } = &mut control {
        *horz_offset = rect.geometry.x;
        *vert_offset = rect.geometry.y;
    }
    Some(Run::control(control, CharShapeIndex::new(0)))
}

fn project_polygon_run(polygon: &Hwp5PolygonControl) -> Run {
    let vertices = scale_polygon_points(&polygon.points, &polygon.geometry);
    let mut control =
        hwpforge_core::control::Control::polygon(vertices).expect("fixture polygon is valid");
    if let Control::Polygon { horz_offset, vert_offset, .. } = &mut control {
        *horz_offset = polygon.geometry.x;
        *vert_offset = polygon.geometry.y;
    }
    Run::control(control, CharShapeIndex::new(0))
}

/// Project a plain ellipse. Center/axes are derived from the bounding box
/// (`Control::ellipse`), which matches how a HWP5 plain ellipse is defined.
fn project_ellipse_run(ellipse: &Hwp5EllipseControl) -> Option<Run> {
    let width = HwpUnit::new(positive_i32_from_u32(ellipse.geometry.width)?).ok()?;
    let height = HwpUnit::new(positive_i32_from_u32(ellipse.geometry.height)?).ok()?;
    let mut control = hwpforge_core::control::Control::ellipse(width, height);
    if let Control::Ellipse { horz_offset, vert_offset, .. } = &mut control {
        *horz_offset = ellipse.geometry.x;
        *vert_offset = ellipse.geometry.y;
    }
    Some(Run::control(control, CharShapeIndex::new(0)))
}

/// Project an arc. 한컴 stores arcs inside the ellipse (`0x50`) record; we have
/// verified the `Normal` open-arc shape end to end. Pie/chord arc types and
/// exact arc-sweep endpoints are a future refinement that needs dedicated
/// fixtures, so we carry a `Normal` arc sized from the bounding box rather than
/// guess a sweep we cannot yet validate.
fn project_arc_run(arc: &Hwp5ArcControl) -> Option<Run> {
    let width = HwpUnit::new(positive_i32_from_u32(arc.geometry.width)?).ok()?;
    let height = HwpUnit::new(positive_i32_from_u32(arc.geometry.height)?).ok()?;
    let mut control = hwpforge_core::control::Control::arc(ArcType::Normal, width, height);
    if let Control::Arc { horz_offset, vert_offset, .. } = &mut control {
        *horz_offset = arc.geometry.x;
        *vert_offset = arc.geometry.y;
    }
    Some(Run::control(control, CharShapeIndex::new(0)))
}

/// Project a curve, scaling its control points into the bounding box like a
/// polygon and mapping the decoded per-segment type bytes onto the Core enum.
fn project_curve_run(curve: &Hwp5CurveControl) -> Option<Run> {
    let vertices = scale_polygon_points(&curve.points, &curve.geometry);
    let mut control = hwpforge_core::control::Control::curve(vertices).ok()?;
    if let Control::Curve { horz_offset, vert_offset, segment_types, .. } = &mut control {
        *horz_offset = curve.geometry.x;
        *vert_offset = curve.geometry.y;
        let decoded: Vec<CurveSegmentType> = curve
            .segment_types
            .iter()
            .map(|byte| match byte {
                0 => CurveSegmentType::Line,
                _ => CurveSegmentType::Curve,
            })
            .collect();
        if !decoded.is_empty() {
            *segment_types = decoded;
        }
    }
    Some(Run::control(control, CharShapeIndex::new(0)))
}

/// Project a connect line. 한컴 stores it in the same `ShapeComponentLine`
/// record as a plain line, so endpoints are scaled into the bounding box the
/// same way; only a straight connector is carried (the source object-link
/// references have no `<hp:connectLine>` representation).
fn project_connectline_run(connect_line: &Hwp5ConnectLineControl) -> Option<Run> {
    let min_x = connect_line.start.x.min(connect_line.end.x);
    let max_x = connect_line.start.x.max(connect_line.end.x);
    let min_y = connect_line.start.y.min(connect_line.end.y);
    let max_y = connect_line.start.y.max(connect_line.end.y);
    let scaled_start = hwpforge_core::control::ShapePoint {
        x: scale_point_into_geometry(
            connect_line.start,
            min_x,
            max_x,
            connect_line.geometry.width,
            100,
            Axis::Horizontal,
        ),
        y: scale_point_into_geometry(
            connect_line.start,
            min_y,
            max_y,
            connect_line.geometry.height,
            100,
            Axis::Vertical,
        ),
    };
    let scaled_end = hwpforge_core::control::ShapePoint {
        x: scale_point_into_geometry(
            connect_line.end,
            min_x,
            max_x,
            connect_line.geometry.width,
            100,
            Axis::Horizontal,
        ),
        y: scale_point_into_geometry(
            connect_line.end,
            min_y,
            max_y,
            connect_line.geometry.height,
            100,
            Axis::Vertical,
        ),
    };
    let mut control =
        hwpforge_core::control::Control::connect_line(scaled_start, scaled_end).ok()?;
    if let Control::ConnectLine { horz_offset, vert_offset, .. } = &mut control {
        *horz_offset = connect_line.geometry.x;
        *vert_offset = connect_line.geometry.y;
    }
    Some(Run::control(control, CharShapeIndex::new(0)))
}

/// Project an equation. The HancomEQN script is carried verbatim; the box size
/// comes from the `eqed` ctrl-header geometry when positive (equations are
/// always inline, so there is no offset to set).
fn project_equation_run(equation: &Hwp5EquationControl) -> Run {
    let mut control = hwpforge_core::control::Control::equation(&equation.script);
    if let Control::Equation { width, height, .. } = &mut control {
        if let Some(w) =
            positive_i32_from_u32(equation.geometry.width).and_then(|v| HwpUnit::new(v).ok())
        {
            *width = w;
        }
        if let Some(h) =
            positive_i32_from_u32(equation.geometry.height).and_then(|v| HwpUnit::new(v).ok())
        {
            *height = h;
        }
    }
    Run::control(control, CharShapeIndex::new(0))
}

#[derive(Debug, Clone, Copy)]
enum Axis {
    Horizontal,
    Vertical,
}

fn scale_polygon_points(
    points: &[Hwp5ShapePoint],
    geometry: &Hwp5ShapeComponentGeometry,
) -> Vec<hwpforge_core::control::ShapePoint> {
    let min_x = points.iter().map(|point| point.x).min().unwrap_or(0);
    let max_x = points.iter().map(|point| point.x).max().unwrap_or(0);
    let min_y = points.iter().map(|point| point.y).min().unwrap_or(0);
    let max_y = points.iter().map(|point| point.y).max().unwrap_or(0);

    points
        .iter()
        .map(|point| hwpforge_core::control::ShapePoint {
            x: scale_point_into_geometry(*point, min_x, max_x, geometry.width, 1, Axis::Horizontal),
            y: scale_point_into_geometry(*point, min_y, max_y, geometry.height, 1, Axis::Vertical),
        })
        .collect()
}

fn scale_point_into_geometry(
    point: Hwp5ShapePoint,
    raw_min: i32,
    raw_max: i32,
    geometry_span: u32,
    minimum_target_span: i32,
    axis: Axis,
) -> i32 {
    let raw_span = i64::from(raw_max) - i64::from(raw_min);
    let target_span =
        i64::from(i32::try_from(geometry_span).unwrap_or(i32::MAX).max(minimum_target_span));
    if raw_span <= 0 {
        return 0;
    }

    let raw_value = match axis {
        Axis::Horizontal => point.x,
        Axis::Vertical => point.y,
    };
    let relative = i64::from(raw_value) - i64::from(raw_min);
    let scaled = (relative * target_span + (raw_span / 2)) / raw_span;
    i32::try_from(scaled).unwrap_or(i32::MAX)
}

fn project_text_segment(
    text: &str,
    runs: &[Hwp5CharShapeRun],
    start_utf16: u32,
    end_utf16: u32,
) -> Vec<Run> {
    if start_utf16 >= end_utf16 {
        return Vec::new();
    }

    let boundaries = utf16_boundaries(text);
    let start_byte = utf16_offset_to_byte(&boundaries, start_utf16);
    let end_byte = utf16_offset_to_byte(&boundaries, end_utf16);
    if start_byte >= end_byte {
        return Vec::new();
    }

    let segment = &text[start_byte..end_byte];
    let mut segment_runs: Vec<Hwp5CharShapeRun> = Vec::new();
    let active_char_shape_id = char_shape_id_at_position(runs, start_utf16);
    segment_runs.push(Hwp5CharShapeRun { position: 0, char_shape_id: active_char_shape_id });

    for run in runs {
        if run.position > start_utf16 && run.position < end_utf16 {
            segment_runs.push(Hwp5CharShapeRun {
                position: run.position - start_utf16,
                char_shape_id: run.char_shape_id,
            });
        }
    }

    split_text_by_runs(segment, &segment_runs)
}

fn char_shape_id_at_position(runs: &[Hwp5CharShapeRun], position: u32) -> u32 {
    runs.iter()
        .take_while(|run| run.position <= position)
        .last()
        .map(|run| run.char_shape_id)
        .unwrap_or(0)
}

fn core_image_format(format: &crate::Hwp5SemanticImageFormat) -> ImageFormat {
    match format {
        crate::Hwp5SemanticImageFormat::Png => ImageFormat::Png,
        crate::Hwp5SemanticImageFormat::Jpeg => ImageFormat::Jpeg,
        crate::Hwp5SemanticImageFormat::Gif => ImageFormat::Gif,
        crate::Hwp5SemanticImageFormat::Bmp => ImageFormat::Bmp,
        crate::Hwp5SemanticImageFormat::Wmf => ImageFormat::Wmf,
        crate::Hwp5SemanticImageFormat::Emf => ImageFormat::Emf,
        crate::Hwp5SemanticImageFormat::Unknown(value) => ImageFormat::Unknown(value.clone()),
    }
}

fn hwp_unit_from_u32(value: u32) -> HwpUnit {
    i32::try_from(value).ok().and_then(|signed| HwpUnit::new(signed).ok()).unwrap_or(HwpUnit::ZERO)
}

/// Split paragraph text into runs according to `char_shape_runs`.
///
/// Each run entry marks the starting character position (as a UTF-16
/// code-unit index) of a new character shape. For simplicity this
/// implementation treats the positions as Unicode scalar-value indices,
/// which is accurate for all-ASCII or all-Korean text.
fn split_text_by_runs(text: &str, runs: &[Hwp5CharShapeRun]) -> Vec<Run> {
    if text.is_empty() && runs.is_empty() {
        return vec![];
    }
    if runs.is_empty() {
        return vec![Run::text(text, CharShapeIndex::new(0))];
    }

    let boundaries = utf16_boundaries(text);
    let mut result: Vec<Run> = Vec::with_capacity(runs.len());

    for (i, run) in runs.iter().enumerate() {
        let start = utf16_offset_to_byte(&boundaries, run.position);
        let end = if i + 1 < runs.len() {
            utf16_offset_to_byte(&boundaries, runs[i + 1].position)
        } else {
            text.len()
        };

        if start >= text.len() {
            break;
        }
        let end = end.min(text.len());
        let segment = &text[start..end];
        if !segment.is_empty() {
            result.push(Run::text(segment, CharShapeIndex::new(run.char_shape_id as usize)));
        }
    }

    if result.is_empty() {
        result.push(Run::text(text, CharShapeIndex::new(0)));
    }
    result
}

fn utf16_boundaries(text: &str) -> Vec<(u32, usize)> {
    let mut boundaries = Vec::with_capacity(text.chars().count() + 1);
    let mut utf16_offset = 0u32;

    for (byte_idx, ch) in text.char_indices() {
        boundaries.push((utf16_offset, byte_idx));
        utf16_offset += ch.len_utf16() as u32;
    }
    boundaries.push((utf16_offset, text.len()));
    boundaries
}

fn utf16_offset_to_byte(boundaries: &[(u32, usize)], utf16_offset: u32) -> usize {
    match boundaries.binary_search_by_key(&utf16_offset, |(offset, _)| *offset) {
        Ok(idx) => boundaries[idx].1,
        Err(idx) => boundaries
            .get(idx)
            .map(|(_, byte_idx)| *byte_idx)
            .unwrap_or_else(|| boundaries.last().map(|(_, byte_idx)| *byte_idx).unwrap_or(0)),
    }
}

// ---------------------------------------------------------------------------
// Table construction
// ---------------------------------------------------------------------------

/// Build a structurally minimal table with `rows × cols` empty cells.
fn build_empty_table(table: &Hwp5Table, warnings: &mut Vec<Hwp5Warning>) -> Table {
    let row_count = table.rows.max(1) as usize;
    let col_count = table.cols.max(1) as usize;

    let table_rows: Vec<TableRow> = (0..row_count)
        .map(|_| {
            let cells: Vec<TableCell> = (0..col_count)
                .map(|_| {
                    TableCell::new(
                        vec![Paragraph::with_runs(
                            vec![Run::text("", CharShapeIndex::new(0))],
                            ParaShapeIndex::new(0),
                        )],
                        HwpUnit::ZERO,
                    )
                })
                .collect();
            TableRow::new(cells)
        })
        .collect();

    let mut core_table = Table::new(table_rows);
    apply_table_projection_metadata(table, &mut core_table, warnings);
    core_table
}

fn build_table_with_images(
    table: &Hwp5Table,
    projection_images: &mut ProjectionImageState<'_>,
) -> Table {
    if table.cells.is_empty() {
        return build_empty_table(table, &mut projection_images.warnings);
    }

    let inferred_rows =
        table.cells.iter().map(|cell| cell.row.saturating_add(cell.row_span)).max().unwrap_or(0);
    let row_count = table.rows.max(inferred_rows).max(1) as usize;

    let mut grouped: Vec<Vec<&Hwp5TableCell>> = vec![Vec::new(); row_count];
    for cell in &table.cells {
        let row_idx = cell.row as usize;
        if row_idx >= grouped.len() {
            grouped.resize(row_idx + 1, Vec::new());
        }
        grouped[row_idx].push(cell);
    }

    let mut rows: Vec<TableRow> = grouped
        .into_iter()
        .map(|mut cells| {
            cells.sort_by_key(|cell| cell.column);
            let row_is_header = projected_row_is_header(&cells, &mut projection_images.warnings);
            let projected = if cells.is_empty() {
                vec![empty_cell()]
            } else {
                cells
                    .iter()
                    .copied()
                    .map(|cell| project_table_cell_with_images(cell, projection_images))
                    .collect()
            };
            let row_height = cells.iter().map(|cell| cell.height).max().unwrap_or(0);
            match HwpUnit::new(row_height) {
                Ok(height) if row_height > 0 => {
                    TableRow::with_height(projected, height).with_header(row_is_header)
                }
                _ => TableRow::new(projected).with_header(row_is_header),
            }
        })
        .collect();

    demote_non_leading_header_rows(&mut rows, &mut projection_images.warnings);

    let mut core_table = Table::new(rows);
    apply_table_projection_metadata(table, &mut core_table, &mut projection_images.warnings);
    core_table
}

/// Enforces the Core/HWPX invariant that header rows form a single leading
/// contiguous block.
///
/// Real 한글 documents sometimes mark a repeat-header row in the middle of a
/// table (for example a column header restated after a sectioning row).
/// `hwpforge_core` validation rejects such a layout
/// ([`ValidationError::NonLeadingTableHeaderRow`]), which previously aborted
/// the whole `convert-hwp5` run. We keep the leading header block and demote
/// any later header row to a normal row, emitting a warning so the dropped
/// repeat-header semantic is surfaced rather than silently lost.
///
/// The traversal mirrors `hwpforge_core::validate`'s `seen_non_header_row`
/// logic exactly, so the demoted result is guaranteed to pass validation.
fn demote_non_leading_header_rows(rows: &mut [TableRow], warnings: &mut Vec<Hwp5Warning>) {
    let mut seen_non_header = false;
    for (row_idx, row) in rows.iter_mut().enumerate() {
        if row.is_header {
            if seen_non_header {
                row.is_header = false;
                push_projection_fallback(
                    warnings,
                    "table.header_row",
                    format!(
                        "non_leading_hwp5_table_header_row row={row_idx}; demoting_to=non_header_row (HWPX requires a single leading header block)"
                    ),
                );
            }
        } else {
            seen_non_header = true;
        }
    }
}

fn projected_row_is_header(cells: &[&Hwp5TableCell], warnings: &mut Vec<Hwp5Warning>) -> bool {
    if cells.is_empty() {
        return false;
    }

    let header_count = cells.iter().filter(|cell| cell.is_header).count();
    if header_count == 0 {
        false
    } else if header_count == cells.len() {
        true
    } else {
        push_projection_fallback(
            warnings,
            "table.header_row",
            format!(
                "mixed_hwp5_table_header_cells row={} header_cells={} total_cells={}; defaulting_to=non_header_row",
                cells[0].row,
                header_count,
                cells.len()
            ),
        );
        false
    }
}

fn apply_table_projection_metadata(
    table: &Hwp5Table,
    core_table: &mut Table,
    warnings: &mut Vec<Hwp5Warning>,
) {
    core_table.repeat_header = table.repeat_header;
    core_table.cell_spacing = (table.cell_spacing > 0)
        .then(|| HwpUnit::new(i32::from(table.cell_spacing)))
        .transpose()
        .unwrap_or(None);
    core_table.border_fill_id = table.border_fill_id.map(u32::from);

    match core_table_page_break(table.page_break) {
        Some(page_break) => core_table.page_break = page_break,
        None => push_projection_fallback(
            warnings,
            "table.page_break",
            format!(
                "unknown_hwp5_table_page_break_raw={}; defaulting_to=cell",
                unknown_hwp5_table_page_break_raw(table.page_break)
                    .expect("known table page-break values must not use projection fallback",),
            ),
        ),
    }
}

fn project_table_cell_with_images(
    cell: &Hwp5TableCell,
    projection_images: &mut ProjectionImageState<'_>,
) -> TableCell {
    let width = HwpUnit::new(cell.width).unwrap_or(HwpUnit::ZERO);
    let paragraphs = if cell.paragraphs.is_empty() {
        vec![empty_paragraph()]
    } else {
        cell.paragraphs
            .iter()
            .map(|paragraph| {
                project_paragraph_with_images(
                    paragraph,
                    projection_images,
                    ImageProjectionContext::Flow,
                    None,
                )
                .paragraph
            })
            .collect()
    };

    let mut core_cell =
        TableCell::with_span(paragraphs, width, cell.col_span.max(1), cell.row_span.max(1));
    core_cell.height =
        (cell.height > 0).then(|| HwpUnit::new(cell.height)).transpose().unwrap_or(None);
    core_cell.border_fill_id = cell.border_fill_id.map(u32::from);
    core_cell.margin = Some(TableMargin {
        left: HwpUnit::new(i32::from(cell.margin.left)).unwrap_or(HwpUnit::ZERO),
        right: HwpUnit::new(i32::from(cell.margin.right)).unwrap_or(HwpUnit::ZERO),
        top: HwpUnit::new(i32::from(cell.margin.top)).unwrap_or(HwpUnit::ZERO),
        bottom: HwpUnit::new(i32::from(cell.margin.bottom)).unwrap_or(HwpUnit::ZERO),
    });
    match core_table_cell_vertical_align(cell.vertical_align) {
        Some(vertical_align) => core_cell.vertical_align = Some(vertical_align),
        None => push_projection_fallback(
            &mut projection_images.warnings,
            "table.cell.vertical_align",
            format!(
                "row={} col={} unknown_hwp5_table_cell_vertical_align_raw={}; dropping_vertical_align",
                cell.row,
                cell.column,
                unknown_hwp5_table_cell_vertical_align_raw(cell.vertical_align).expect(
                    "known table cell vertical-align values must not use projection fallback",
                ),
            ),
        ),
    }
    core_cell
}

fn empty_paragraph() -> Paragraph {
    Paragraph::with_runs(vec![Run::text("", CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}

fn empty_cell() -> TableCell {
    TableCell::new(vec![empty_paragraph()], HwpUnit::ZERO)
}

// ---------------------------------------------------------------------------
// PageDef → PageSettings
// ---------------------------------------------------------------------------

/// Convert an `Hwp5PageDef` (raw HWP5 units) into Core `PageSettings`.
///
/// HWP5 page dimensions are already in HwpUnit (720ths of an inch).
/// `HwpUnit::new` rejects values outside ±100,000,000; for the rare case
/// where a malformed file has an out-of-range value, `HwpUnit::ZERO` is
/// used as a safe fallback.
fn page_def_to_settings(pd: &Hwp5PageDef) -> PageSettings {
    let u = |v: u32| HwpUnit::new(v as i32).unwrap_or(HwpUnit::ZERO);
    PageSettings {
        width: u(pd.width),
        height: u(pd.height),
        margin_left: u(pd.margin_left),
        margin_right: u(pd.margin_right),
        margin_top: u(pd.margin_top),
        margin_bottom: u(pd.margin_bottom),
        header_margin: u(pd.header_margin),
        footer_margin: u(pd.footer_margin),
        gutter: u(pd.gutter),
        landscape: pd.landscape,
        ..PageSettings::a4()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeMap;

    use hwpforge_core::table::TablePageBreak;

    use crate::decoder::section::{
        Hwp5ImageControl, Hwp5LineControl, Hwp5PolygonControl, Hwp5TablePageBreak,
        Hwp5TextBoxControl,
    };
    use crate::Hwp5SemanticImageFormat;

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn make_paragraph(text: &str, para_shape_id: u16, style_id: u8) -> Hwp5Paragraph {
        Hwp5Paragraph {
            text: text.to_string(),
            text_segments: Vec::new(),
            para_shape_id,
            style_id,
            char_shape_runs: vec![],
            line_segments: Vec::new(),
            controls: vec![],
        }
    }

    fn _make_paragraph_with_runs(text: &str, runs: Vec<Hwp5CharShapeRun>) -> Hwp5Paragraph {
        Hwp5Paragraph {
            text: text.to_string(),
            text_segments: Vec::new(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: runs,
            line_segments: Vec::new(),
            controls: vec![],
        }
    }

    fn make_section(
        paragraphs: Vec<Hwp5Paragraph>,
        page_def: Option<Hwp5PageDef>,
    ) -> SectionResult {
        SectionResult {
            paragraphs,
            page_def,
            section_def_properties: None,
            page_border_fills: Vec::new(),
            warnings: vec![],
        }
    }

    fn hwp5_char_run(position: u32, char_shape_id: u32) -> Hwp5CharShapeRun {
        Hwp5CharShapeRun { position, char_shape_id }
    }

    fn image_plan<'a>(
        assets: impl IntoIterator<Item = (u16, &'a str, Hwp5SemanticImageFormat, Vec<u8>)>,
    ) -> Hwp5JoinedImageAssetPlan {
        let ordered_assets: Vec<Hwp5JoinedImageAsset> = assets
            .into_iter()
            .map(|(binary_data_id, storage_name, format, bytes)| Hwp5JoinedImageAsset {
                payload: crate::Hwp5SemanticImagePayload {
                    binary_data_id,
                    storage_name: storage_name.to_string(),
                    package_path: format!("BinData/{storage_name}"),
                    format,
                    width_hwp: None,
                    height_hwp: None,
                },
                bytes,
            })
            .collect();
        let assets_by_binary_data_id: BTreeMap<u16, Hwp5JoinedImageAsset> = ordered_assets
            .iter()
            .cloned()
            .map(|asset| (asset.payload.binary_data_id, asset))
            .collect();
        Hwp5JoinedImageAssetPlan { ordered_assets, assets_by_binary_data_id }
    }

    fn image_plan_with_dimensions(
        binary_data_id: u16,
        storage_name: &str,
        format: Hwp5SemanticImageFormat,
        width_hwp: Option<i32>,
        height_hwp: Option<i32>,
        bytes: Vec<u8>,
    ) -> Hwp5JoinedImageAssetPlan {
        let asset = Hwp5JoinedImageAsset {
            payload: crate::Hwp5SemanticImagePayload {
                binary_data_id,
                storage_name: storage_name.to_string(),
                package_path: format!("BinData/{storage_name}"),
                format,
                width_hwp,
                height_hwp,
            },
            bytes,
        };
        let assets_by_binary_data_id: BTreeMap<u16, Hwp5JoinedImageAsset> =
            [(binary_data_id, asset.clone())].into_iter().collect();
        Hwp5JoinedImageAssetPlan { ordered_assets: vec![asset], assets_by_binary_data_id }
    }

    // ── project_to_core ───────────────────────────────────────────────────────

    #[test]
    fn empty_sections_produces_default_document() {
        let (doc, warnings) = project_to_core(vec![]).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(doc.section_count(), 1, "empty input must produce 1 fallback section");
        assert_eq!(doc.sections()[0].paragraph_count(), 1);
    }

    #[test]
    fn single_section_with_one_paragraph() {
        let para = make_paragraph("Hello", 3, 0);
        let section = make_section(vec![para], None);
        let (doc, warnings) = project_to_core(vec![section]).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(doc.section_count(), 1);
        let s = &doc.sections()[0];
        assert_eq!(s.paragraph_count(), 1);
        let p = &s.paragraphs[0];
        assert_eq!(p.para_shape_id, ParaShapeIndex::new(3));
        assert_eq!(p.text_content(), "Hello");
    }

    #[test]
    fn style_id_zero_maps_to_none() {
        let para = make_paragraph("text", 0, 0);
        let section = make_section(vec![para], None);
        let (doc, _) = project_to_core(vec![section]).unwrap();
        assert_eq!(doc.sections()[0].paragraphs[0].style_id, None);
    }

    #[test]
    fn style_id_nonzero_maps_to_some() {
        let para = make_paragraph("text", 0, 5);
        let section = make_section(vec![para], None);
        let (doc, _) = project_to_core(vec![section]).unwrap();
        assert_eq!(doc.sections()[0].paragraphs[0].style_id, Some(StyleIndex::new(5)));
    }

    #[test]
    fn multiple_sections_preserved() {
        let s1 = make_section(vec![make_paragraph("A", 0, 0)], None);
        let s2 = make_section(vec![make_paragraph("B", 0, 0)], None);
        let s3 = make_section(vec![make_paragraph("C", 0, 0)], None);
        let (doc, _) = project_to_core(vec![s1, s2, s3]).unwrap();
        assert_eq!(doc.section_count(), 3);
    }

    #[test]
    fn empty_section_gets_fallback_paragraph() {
        let section = make_section(vec![], None);
        let (doc, _) = project_to_core(vec![section]).unwrap();
        assert_eq!(doc.sections()[0].paragraph_count(), 1);
    }

    #[test]
    fn warnings_are_collected() {
        let warn = Hwp5Warning::UnsupportedTag { tag_id: 0xAB, offset: 0 };
        let section = SectionResult {
            paragraphs: vec![make_paragraph("x", 0, 0)],
            page_def: None,
            section_def_properties: None,
            page_border_fills: Vec::new(),
            warnings: vec![warn],
        };
        let (_, warnings) = project_to_core(vec![section]).unwrap();
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn project_to_core_with_images_preserves_inline_order_and_populates_store() {
        let image = Hwp5Control::Image(Hwp5ImageControl {
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                x: 0,
                y: 0,
                width: 3_000,
                height: 2_000,
            },
            binary_data_id: 1,
        });
        let section = make_section(
            vec![Hwp5Paragraph {
                text: "앞\u{fffc}뒤".to_string(),
                text_segments: Vec::new(),
                para_shape_id: 3,
                style_id: 0,
                char_shape_runs: Vec::new(),
                line_segments: Vec::new(),
                controls: vec![image],
            }],
            None,
        );
        let image_assets = image_plan([(
            1,
            "BIN0001.png",
            Hwp5SemanticImageFormat::Png,
            vec![0x89, 0x50, 0x4E, 0x47],
        )]);

        let (document, image_store, warnings) =
            project_to_core_with_images(vec![section], &image_assets).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(image_store.len(), 1);
        assert_eq!(image_store.get("BIN0001.png"), Some(&[0x89, 0x50, 0x4E, 0x47][..]));

        let paragraph = &document.sections()[0].paragraphs[0];
        assert_eq!(paragraph.runs.len(), 3);
        assert_eq!(paragraph.runs[0].content.as_text(), Some("앞"));
        assert!(paragraph.runs[1].content.is_image());
        assert_eq!(paragraph.runs[2].content.as_text(), Some("뒤"));

        let image = paragraph.runs[1].content.as_image().expect("middle run should be image");
        assert_eq!(image.path, "BinData/BIN0001.png");
        assert_eq!(image.width, HwpUnit::new(3_000).unwrap());
        assert_eq!(image.height, HwpUnit::new(2_000).unwrap());
        let placement = image.placement.as_ref().expect("placement should be attached");
        assert!(placement.treat_as_char);
        assert_eq!(placement.text_wrap, ImageTextWrap::TopAndBottom);
        assert_eq!(placement.horz_rel_to, ImageRelativeTo::Para);
        assert_eq!(placement.vert_rel_to, ImageRelativeTo::Para);
        assert_eq!(document.sections()[0].content_counts().images, 1);
    }

    #[test]
    fn project_to_core_with_images_projects_header_and_footer_subtrees() {
        let header_image = Hwp5Control::Image(Hwp5ImageControl {
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                x: 0,
                y: 0,
                width: 1_200,
                height: 800,
            },
            binary_data_id: 7,
        });
        let section = make_section(
            vec![Hwp5Paragraph {
                text: "\u{fffc}\u{fffc}".to_string(),
                text_segments: Vec::new(),
                para_shape_id: 0,
                style_id: 0,
                char_shape_runs: Vec::new(),
                line_segments: Vec::new(),
                controls: vec![
                    Hwp5Control::Header(crate::decoder::section::Hwp5NestedSubtree {
                        ctrl_id: 0x6865_6164,
                        properties_raw: 0,
                        instance_id: 0,
                        paragraphs: vec![Hwp5Paragraph {
                            text: "\u{fffc}".to_string(),
                            text_segments: Vec::new(),
                            para_shape_id: 0,
                            style_id: 0,
                            char_shape_runs: Vec::new(),
                            line_segments: Vec::new(),
                            controls: vec![header_image],
                        }],
                    }),
                    Hwp5Control::Footer(crate::decoder::section::Hwp5NestedSubtree {
                        ctrl_id: 0x666F_6F74,
                        properties_raw: 0,
                        instance_id: 0,
                        paragraphs: vec![make_paragraph("꼬리말 테스트", 0, 0)],
                    }),
                ],
            }],
            None,
        );
        let image_assets =
            image_plan([(7, "BIN0007.png", Hwp5SemanticImageFormat::Png, vec![1, 2, 3, 4])]);

        let (document, image_store, _) =
            project_to_core_with_images(vec![section], &image_assets).unwrap();
        let section = &document.sections()[0];
        let header = section.headers.first().expect("header should be projected");
        let footer = section.footers.first().expect("footer should be projected");

        assert_eq!(image_store.get("BIN0007.png"), Some(&[1, 2, 3, 4][..]));
        assert_eq!(header.paragraphs.len(), 1);
        assert_eq!(footer.paragraphs.len(), 1);
        assert_eq!(header.paragraphs[0].runs.len(), 1);
        assert!(header.paragraphs[0].runs[0].content.is_image());
        assert_eq!(footer.paragraphs[0].text_content(), "꼬리말 테스트");
    }

    #[test]
    fn project_to_core_with_images_projects_textbox_with_nested_image() {
        let nested_image = Hwp5Control::Image(Hwp5ImageControl {
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                x: 0,
                y: 0,
                width: 1_500,
                height: 900,
            },
            binary_data_id: 3,
        });
        let textbox = Hwp5Control::TextBox(Hwp5TextBoxControl {
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                x: 50,
                y: 60,
                width: 8_000,
                height: 6_000,
            },
            paragraphs: vec![Hwp5Paragraph {
                text: "앞\u{fffc}뒤".to_string(),
                text_segments: Vec::new(),
                para_shape_id: 1,
                style_id: 0,
                char_shape_runs: Vec::new(),
                line_segments: Vec::new(),
                controls: vec![nested_image],
            }],
        });
        let section = make_section(
            vec![Hwp5Paragraph {
                text: "\u{fffc}".to_string(),
                text_segments: Vec::new(),
                para_shape_id: 0,
                style_id: 0,
                char_shape_runs: Vec::new(),
                line_segments: Vec::new(),
                controls: vec![textbox],
            }],
            None,
        );
        let image_assets =
            image_plan([(3, "BIN0003.png", Hwp5SemanticImageFormat::Png, vec![9, 8, 7])]);

        let (document, image_store, warnings) =
            project_to_core_with_images(vec![section], &image_assets).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(image_store.get("BIN0003.png"), Some(&[9, 8, 7][..]));

        let paragraph = &document.sections()[0].paragraphs[0];
        assert_eq!(paragraph.runs.len(), 1);
        let textbox_control =
            paragraph.runs[0].content.as_control().expect("textbox should project as control");
        match textbox_control {
            Control::TextBox { paragraphs, width, height, horz_offset, vert_offset, .. } => {
                assert_eq!(width, &HwpUnit::new(8_000).unwrap());
                assert_eq!(height, &HwpUnit::new(6_000).unwrap());
                assert_eq!(*horz_offset, 50);
                assert_eq!(*vert_offset, 60);
                assert_eq!(paragraphs.len(), 1);
                assert_eq!(paragraphs[0].runs.len(), 3);
                assert_eq!(paragraphs[0].runs[0].content.as_text(), Some("앞"));
                let nested_image =
                    paragraphs[0].runs[1].content.as_image().expect("middle run should be image");
                let placement =
                    nested_image.placement.as_ref().expect("textbox image should have placement");
                assert_eq!(placement.text_wrap, ImageTextWrap::Square);
                assert_eq!(placement.text_flow, ImageTextFlow::BothSides);
                assert!(!placement.treat_as_char);
                assert!(placement.flow_with_text);
                assert!(!placement.allow_overlap);
                assert_eq!(placement.horz_rel_to, ImageRelativeTo::Para);
                assert_eq!(placement.vert_rel_to, ImageRelativeTo::Para);
                assert_eq!(paragraphs[0].runs[2].content.as_text(), Some("뒤"));
            }
            other => panic!("expected TextBox control, got {:?}", other),
        }
    }

    #[test]
    fn project_to_core_with_images_warns_when_image_asset_join_is_missing() {
        let image = Hwp5Control::Image(Hwp5ImageControl {
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                x: 0,
                y: 0,
                width: 1_000,
                height: 800,
            },
            binary_data_id: 99,
        });
        let section = make_section(
            vec![Hwp5Paragraph {
                text: "\u{fffc}".to_string(),
                text_segments: Vec::new(),
                para_shape_id: 0,
                style_id: 0,
                char_shape_runs: Vec::new(),
                line_segments: Vec::new(),
                controls: vec![image],
            }],
            None,
        );

        let (document, image_store, warnings) =
            project_to_core_with_images(vec![section], &image_plan([])).unwrap();
        assert!(image_store.is_empty());
        assert_eq!(document.sections()[0].paragraphs[0].runs.len(), 1);
        assert_eq!(document.sections()[0].paragraphs[0].text_content(), "");
        assert!(warnings.iter().any(|warning| matches!(
            warning,
            Hwp5Warning::DroppedControl { control, reason }
                if *control == "image"
                    && reason == "missing_image_asset_for_binary_data_id=99"
        )));
    }

    #[test]
    fn project_to_core_with_images_falls_back_to_joined_asset_dimensions() {
        let image = Hwp5Control::Image(Hwp5ImageControl {
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            binary_data_id: 5,
        });
        let section = make_section(
            vec![Hwp5Paragraph {
                text: "\u{fffc}".to_string(),
                text_segments: Vec::new(),
                para_shape_id: 0,
                style_id: 0,
                char_shape_runs: Vec::new(),
                line_segments: Vec::new(),
                controls: vec![image],
            }],
            None,
        );
        let image_assets = image_plan_with_dimensions(
            5,
            "BIN0005.png",
            Hwp5SemanticImageFormat::Png,
            Some(3_210),
            Some(4_560),
            vec![0x89, 0x50, 0x4E, 0x47],
        );

        let (document, image_store, warnings) =
            project_to_core_with_images(vec![section], &image_assets).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(image_store.get("BIN0005.png"), Some(&[0x89, 0x50, 0x4E, 0x47][..]));

        let paragraph = &document.sections()[0].paragraphs[0];
        let image = paragraph.runs[0].content.as_image().expect("run should be image");
        assert_eq!(image.width, HwpUnit::new(3_210).unwrap());
        assert_eq!(image.height, HwpUnit::new(4_560).unwrap());
    }

    #[test]
    fn project_to_core_with_images_drops_zero_sized_image_without_fallback() {
        let image = Hwp5Control::Image(Hwp5ImageControl {
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            binary_data_id: 6,
        });
        let section = make_section(
            vec![Hwp5Paragraph {
                text: "\u{fffc}".to_string(),
                text_segments: Vec::new(),
                para_shape_id: 0,
                style_id: 0,
                char_shape_runs: Vec::new(),
                line_segments: Vec::new(),
                controls: vec![image],
            }],
            None,
        );

        let image_assets = image_plan([(6, "BIN0006.png", Hwp5SemanticImageFormat::Png, vec![1])]);

        let (document, image_store, warnings) =
            project_to_core_with_images(vec![section], &image_assets).unwrap();
        assert!(image_store.is_empty());
        assert_eq!(document.sections()[0].paragraphs[0].runs.len(), 1);
        assert_eq!(document.sections()[0].paragraphs[0].text_content(), "");
        assert!(warnings.iter().any(|warning| matches!(
            warning,
            Hwp5Warning::DroppedControl { control, reason }
                if *control == "image"
                    && reason == "image_zero_size_projection binary_data_id=6 width=0 height=0"
        )));
    }

    // ── page_def_to_settings ─────────────────────────────────────────────────

    #[test]
    fn page_def_dimensions_are_preserved() {
        let pd = Hwp5PageDef {
            width: 59535,
            height: 84183,
            margin_left: 5669,
            margin_right: 5669,
            margin_top: 5669,
            margin_bottom: 5669,
            header_margin: 2835,
            footer_margin: 2835,
            gutter: 0,
            landscape: false,
        };
        let ps = page_def_to_settings(&pd);
        assert_eq!(ps.width, HwpUnit::new(59535).unwrap());
        assert_eq!(ps.height, HwpUnit::new(84183).unwrap());
        assert_eq!(ps.margin_left, HwpUnit::new(5669).unwrap());
        assert!(!ps.landscape);
    }

    #[test]
    fn page_def_landscape_flag_propagated() {
        let pd = Hwp5PageDef {
            width: 59535,
            height: 84183,
            margin_left: 0,
            margin_right: 0,
            margin_top: 0,
            margin_bottom: 0,
            header_margin: 0,
            footer_margin: 0,
            gutter: 0,
            landscape: true,
        };
        let ps = page_def_to_settings(&pd);
        assert!(ps.landscape);
    }

    #[test]
    fn section_with_page_def_uses_it() {
        let pd = Hwp5PageDef {
            width: 59535,
            height: 84183,
            margin_left: 5669,
            margin_right: 5669,
            margin_top: 5669,
            margin_bottom: 5669,
            header_margin: 2835,
            footer_margin: 2835,
            gutter: 0,
            landscape: false,
        };
        let section = make_section(vec![make_paragraph("x", 0, 0)], Some(pd));
        let (doc, _) = project_to_core(vec![section]).unwrap();
        assert_eq!(doc.sections()[0].page_settings.width, HwpUnit::new(59535).unwrap());
    }

    #[test]
    fn section_without_page_def_defaults_to_a4() {
        let section = make_section(vec![make_paragraph("x", 0, 0)], None);
        let (doc, _) = project_to_core(vec![section]).unwrap();
        assert_eq!(doc.sections()[0].page_settings, PageSettings::a4());
    }

    // ── split_text_by_runs ────────────────────────────────────────────────────

    #[test]
    fn split_empty_text_empty_runs() {
        let result = split_text_by_runs("", &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn split_text_no_runs_returns_single_run() {
        let result = split_text_by_runs("Hello", &[]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content.as_text(), Some("Hello"));
        assert_eq!(result[0].char_shape_id, CharShapeIndex::new(0));
    }

    #[test]
    fn split_single_run_covers_all_text() {
        let runs = vec![hwp5_char_run(0, 7)];
        let result = split_text_by_runs("Hello", &runs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content.as_text(), Some("Hello"));
        assert_eq!(result[0].char_shape_id, CharShapeIndex::new(7));
    }

    #[test]
    fn split_two_runs() {
        // "HelloWorld" split at position 5
        let runs = vec![hwp5_char_run(0, 2), hwp5_char_run(5, 3)];
        let result = split_text_by_runs("HelloWorld", &runs);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].content.as_text(), Some("Hello"));
        assert_eq!(result[0].char_shape_id, CharShapeIndex::new(2));
        assert_eq!(result[1].content.as_text(), Some("World"));
        assert_eq!(result[1].char_shape_id, CharShapeIndex::new(3));
    }

    #[test]
    fn split_run_start_beyond_text_length_ignored() {
        // Run starting at position 100 in a 5-char string → ignored.
        let runs = vec![hwp5_char_run(0, 1), hwp5_char_run(100, 2)];
        let result = split_text_by_runs("Hello", &runs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content.as_text(), Some("Hello"));
        assert_eq!(result[0].char_shape_id, CharShapeIndex::new(1));
    }

    #[test]
    fn split_korean_text_by_runs() {
        // "안녕하세요" = 5 chars; split at char 2
        let runs = vec![hwp5_char_run(0, 10), hwp5_char_run(2, 11)];
        let result = split_text_by_runs("안녕하세요", &runs);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].content.as_text(), Some("안녕"));
        assert_eq!(result[1].content.as_text(), Some("하세요"));
    }

    #[test]
    fn split_text_by_utf16_code_units_handles_surrogate_pairs() {
        let runs = vec![hwp5_char_run(0, 1), hwp5_char_run(1, 2), hwp5_char_run(3, 3)];
        let result = split_text_by_runs("A😀B", &runs);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].content.as_text(), Some("A"));
        assert_eq!(result[1].content.as_text(), Some("😀"));
        assert_eq!(result[2].content.as_text(), Some("B"));
    }

    // ── table controls ────────────────────────────────────────────────────────

    #[test]
    fn table_control_becomes_run_table() {
        let para = Hwp5Paragraph {
            text: String::new(),
            text_segments: Vec::new(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            line_segments: Vec::new(),
            controls: vec![Hwp5Control::Table(Hwp5Table {
                rows: 2,
                cols: 3,
                page_break: Hwp5TablePageBreak::None,
                repeat_header: false,
                cell_spacing: 120,
                border_fill_id: Some(8),
                cells: vec![],
            })],
        };
        let section = make_section(vec![para], None);
        let (doc, _) = project_to_core(vec![section]).unwrap();
        let p = &doc.sections()[0].paragraphs[0];
        let table_run = p.runs.iter().find(|r| r.content.is_table());
        assert!(table_run.is_some(), "expected a table run");
        let table = table_run.unwrap().content.as_table().unwrap();
        assert_eq!(table.row_count(), 2);
        assert_eq!(table.col_count(), 3);
        assert_eq!(table.page_break, TablePageBreak::None);
        assert!(!table.repeat_header);
        assert_eq!(table.cell_spacing, Some(HwpUnit::new(120).unwrap()));
        assert_eq!(table.border_fill_id, Some(8));
    }

    #[test]
    fn table_cell_text_is_projected() {
        let para = Hwp5Paragraph {
            text: "\u{FFFC}".to_string(),
            text_segments: Vec::new(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            line_segments: Vec::new(),
            controls: vec![Hwp5Control::Table(Hwp5Table {
                rows: 1,
                cols: 1,
                page_break: Hwp5TablePageBreak::None,
                repeat_header: false,
                cell_spacing: 0,
                border_fill_id: None,
                cells: vec![Hwp5TableCell {
                    column: 0,
                    row: 0,
                    col_span: 1,
                    row_span: 1,
                    width: 4000,
                    height: 1000,
                    is_header: true,
                    margin: crate::decoder::section::Hwp5TableCellMargin {
                        left: 0,
                        right: 0,
                        top: 0,
                        bottom: 0,
                    },
                    vertical_align: crate::decoder::section::Hwp5TableCellVerticalAlign::Center,
                    border_fill_id: Some(3),
                    paragraphs: vec![Hwp5Paragraph {
                        text: "셀".to_string(),
                        text_segments: Vec::new(),
                        para_shape_id: 0,
                        style_id: 0,
                        char_shape_runs: vec![],
                        line_segments: Vec::new(),
                        controls: vec![],
                    }],
                }],
            })],
        };

        let section = make_section(vec![para], None);
        let (doc, _) = project_to_core(vec![section]).unwrap();
        let p = &doc.sections()[0].paragraphs[0];
        assert_eq!(p.text_content(), "", "control placeholder text should be stripped");

        let table =
            p.runs.iter().find_map(|run| run.content.as_table()).expect("expected table run");
        assert_eq!(table.rows[0].cells[0].paragraphs[0].text_content(), "셀");
        assert_eq!(table.rows[0].cells[0].height, Some(HwpUnit::new(1000).unwrap()));
        assert_eq!(table.rows[0].cells[0].border_fill_id, Some(3));
        assert_eq!(
            table.rows[0].cells[0].margin,
            Some(TableMargin {
                left: HwpUnit::new(0).unwrap(),
                right: HwpUnit::new(0).unwrap(),
                top: HwpUnit::new(0).unwrap(),
                bottom: HwpUnit::new(0).unwrap(),
            })
        );
        assert_eq!(
            table.rows[0].cells[0].vertical_align,
            Some(hwpforge_core::table::TableVerticalAlign::Center)
        );
    }

    #[test]
    fn unknown_table_cell_vertical_align_emits_projection_fallback_warning() {
        let para = Hwp5Paragraph {
            text: "\u{FFFC}".to_string(),
            text_segments: Vec::new(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            line_segments: Vec::new(),
            controls: vec![Hwp5Control::Table(Hwp5Table {
                rows: 1,
                cols: 1,
                page_break: Hwp5TablePageBreak::None,
                repeat_header: false,
                cell_spacing: 0,
                border_fill_id: None,
                cells: vec![Hwp5TableCell {
                    column: 0,
                    row: 0,
                    col_span: 1,
                    row_span: 1,
                    width: 4000,
                    height: 1000,
                    is_header: false,
                    margin: crate::decoder::section::Hwp5TableCellMargin {
                        left: 10,
                        right: 20,
                        top: 30,
                        bottom: 40,
                    },
                    vertical_align: crate::decoder::section::Hwp5TableCellVerticalAlign::Unknown(3),
                    border_fill_id: Some(3),
                    paragraphs: vec![Hwp5Paragraph {
                        text: "셀".to_string(),
                        text_segments: Vec::new(),
                        para_shape_id: 0,
                        style_id: 0,
                        char_shape_runs: vec![],
                        line_segments: Vec::new(),
                        controls: vec![],
                    }],
                }],
            })],
        };

        let section = make_section(vec![para], None);
        let (doc, warnings) = project_to_core(vec![section]).unwrap();
        let p = &doc.sections()[0].paragraphs[0];
        let table =
            p.runs.iter().find_map(|run| run.content.as_table()).expect("expected table run");
        assert_eq!(
            table.rows[0].cells[0].margin,
            Some(TableMargin {
                left: HwpUnit::new(10).unwrap(),
                right: HwpUnit::new(20).unwrap(),
                top: HwpUnit::new(30).unwrap(),
                bottom: HwpUnit::new(40).unwrap(),
            })
        );
        assert_eq!(table.rows[0].cells[0].vertical_align, None);
        assert!(warnings.iter().any(|warning| matches!(
            warning,
            Hwp5Warning::ProjectionFallback { subject, reason }
                if *subject == "table.cell.vertical_align"
                    && reason
                        == "row=0 col=0 unknown_hwp5_table_cell_vertical_align_raw=3; dropping_vertical_align"
        )));
    }

    #[test]
    fn mixed_table_header_cells_emit_warning_and_do_not_promote_header_row() {
        let para = Hwp5Paragraph {
            text: "\u{FFFC}".to_string(),
            text_segments: Vec::new(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            line_segments: Vec::new(),
            controls: vec![Hwp5Control::Table(Hwp5Table {
                rows: 1,
                cols: 2,
                page_break: Hwp5TablePageBreak::Cell,
                repeat_header: true,
                cell_spacing: 0,
                border_fill_id: None,
                cells: vec![
                    Hwp5TableCell {
                        column: 0,
                        row: 0,
                        col_span: 1,
                        row_span: 1,
                        width: 4000,
                        height: 1000,
                        is_header: true,
                        margin: crate::decoder::section::Hwp5TableCellMargin {
                            left: 0,
                            right: 0,
                            top: 0,
                            bottom: 0,
                        },
                        vertical_align: crate::decoder::section::Hwp5TableCellVerticalAlign::Center,
                        border_fill_id: Some(3),
                        paragraphs: vec![Hwp5Paragraph {
                            text: "head".to_string(),
                            text_segments: Vec::new(),
                            para_shape_id: 0,
                            style_id: 0,
                            char_shape_runs: vec![],
                            line_segments: Vec::new(),
                            controls: vec![],
                        }],
                    },
                    Hwp5TableCell {
                        column: 1,
                        row: 0,
                        col_span: 1,
                        row_span: 1,
                        width: 4000,
                        height: 1000,
                        is_header: false,
                        margin: crate::decoder::section::Hwp5TableCellMargin {
                            left: 0,
                            right: 0,
                            top: 0,
                            bottom: 0,
                        },
                        vertical_align: crate::decoder::section::Hwp5TableCellVerticalAlign::Center,
                        border_fill_id: Some(3),
                        paragraphs: vec![Hwp5Paragraph {
                            text: "body".to_string(),
                            text_segments: Vec::new(),
                            para_shape_id: 0,
                            style_id: 0,
                            char_shape_runs: vec![],
                            line_segments: Vec::new(),
                            controls: vec![],
                        }],
                    },
                ],
            })],
        };

        let section = make_section(vec![para], None);
        let (doc, warnings) = project_to_core(vec![section]).unwrap();
        let table = doc.sections()[0].paragraphs[0]
            .runs
            .iter()
            .find_map(|run| run.content.as_table())
            .expect("expected table run");
        assert!(!table.rows[0].is_header, "mixed header cells must not promote header row");
        assert!(warnings.iter().any(|warning| matches!(
            warning,
            Hwp5Warning::ProjectionFallback { subject, reason }
                if *subject == "table.header_row"
                    && reason == "mixed_hwp5_table_header_cells row=0 header_cells=1 total_cells=2; defaulting_to=non_header_row"
        )));
    }

    #[test]
    fn non_leading_header_row_is_demoted_and_warns() {
        // A real 한글 layout: header row (0), body row (1), then a *second*
        // header row (2). Core validation requires header rows to form a single
        // leading block, so the trailing header row must be demoted instead of
        // aborting the whole conversion.
        fn header_cell(row: u16, is_header: bool, text: &str) -> Hwp5TableCell {
            Hwp5TableCell {
                column: 0,
                row,
                col_span: 1,
                row_span: 1,
                width: 4000,
                height: 1000,
                is_header,
                margin: crate::decoder::section::Hwp5TableCellMargin {
                    left: 0,
                    right: 0,
                    top: 0,
                    bottom: 0,
                },
                vertical_align: crate::decoder::section::Hwp5TableCellVerticalAlign::Center,
                border_fill_id: Some(3),
                paragraphs: vec![Hwp5Paragraph {
                    text: text.to_string(),
                    text_segments: Vec::new(),
                    para_shape_id: 0,
                    style_id: 0,
                    char_shape_runs: vec![],
                    line_segments: Vec::new(),
                    controls: vec![],
                }],
            }
        }

        let para = Hwp5Paragraph {
            text: "\u{FFFC}".to_string(),
            text_segments: Vec::new(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            line_segments: Vec::new(),
            controls: vec![Hwp5Control::Table(Hwp5Table {
                rows: 3,
                cols: 1,
                page_break: Hwp5TablePageBreak::Cell,
                repeat_header: true,
                cell_spacing: 0,
                border_fill_id: None,
                cells: vec![
                    header_cell(0, true, "head"),
                    header_cell(1, false, "body"),
                    header_cell(2, true, "restated-head"),
                ],
            })],
        };

        let section = make_section(vec![para], None);
        let (doc, warnings) = project_to_core(vec![section]).unwrap();
        let table = doc.sections()[0].paragraphs[0]
            .runs
            .iter()
            .find_map(|run| run.content.as_table())
            .expect("expected table run");

        assert!(table.rows[0].is_header, "leading header row must be kept");
        assert!(!table.rows[1].is_header, "body row stays non-header");
        assert!(!table.rows[2].is_header, "trailing header row must be demoted");
        assert!(warnings.iter().any(|warning| matches!(
            warning,
            Hwp5Warning::ProjectionFallback { subject, reason }
                if *subject == "table.header_row"
                    && reason.starts_with("non_leading_hwp5_table_header_row row=2")
        )));

        // The demoted layout must now pass Core validation (previously aborted
        // with NonLeadingTableHeaderRow).
        assert!(doc.validate().is_ok(), "demoted table must satisfy Core validation");
    }

    #[test]
    fn page_number_inside_table_cell_is_carried_to_section() {
        // 한글 government layouts often put the page-number control (`pgnp`)
        // inside a layout table cell. The section-level scan must reach it;
        // a body-paragraph-only scan would drop it.
        fn pgnp_cell() -> Hwp5TableCell {
            Hwp5TableCell {
                column: 0,
                row: 0,
                col_span: 1,
                row_span: 1,
                width: 4000,
                height: 1000,
                is_header: false,
                margin: crate::decoder::section::Hwp5TableCellMargin {
                    left: 0,
                    right: 0,
                    top: 0,
                    bottom: 0,
                },
                vertical_align: crate::decoder::section::Hwp5TableCellVerticalAlign::Center,
                border_fill_id: None,
                paragraphs: vec![Hwp5Paragraph {
                    text: String::new(),
                    text_segments: Vec::new(),
                    para_shape_id: 0,
                    style_id: 0,
                    char_shape_runs: vec![],
                    line_segments: Vec::new(),
                    // pgnp control with a valid BOTTOM_CENTER (pos byte 5 = 5).
                    controls: vec![Hwp5Control::Unknown {
                        ctrl_id: CTRL_ID_PAGE_NUMBER,
                        header_data: vec![0, 0, 0, 0, 0, 5],
                    }],
                }],
            }
        }

        let para = Hwp5Paragraph {
            text: "\u{FFFC}".to_string(),
            text_segments: Vec::new(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            line_segments: Vec::new(),
            controls: vec![Hwp5Control::Table(Hwp5Table {
                rows: 1,
                cols: 1,
                page_break: Hwp5TablePageBreak::Cell,
                repeat_header: false,
                cell_spacing: 0,
                border_fill_id: None,
                cells: vec![pgnp_cell()],
            })],
        };

        let section = make_section(vec![para], None);
        let (doc, _) = project_to_core(vec![section]).unwrap();
        assert!(
            doc.sections()[0].page_number.is_some(),
            "page number inside a table cell must be carried to the section"
        );
    }

    #[test]
    fn page_number_resolves_in_document_order_across_table_and_body() {
        // When a page-number control sits both inside a table cell (earlier in
        // document order) and as a later top-level paragraph, the section-level
        // scan returns the first in document order — the table-cell one. This
        // locks the ordering the page_number SSOT refactor introduced (the old
        // body-only scan would have returned the later top-level control).
        fn pgnp_para(pos_byte: u8) -> Hwp5Paragraph {
            Hwp5Paragraph {
                text: "\u{FFFC}".to_string(),
                text_segments: Vec::new(),
                para_shape_id: 0,
                style_id: 0,
                char_shape_runs: vec![],
                line_segments: Vec::new(),
                controls: vec![Hwp5Control::Unknown {
                    ctrl_id: CTRL_ID_PAGE_NUMBER,
                    header_data: vec![0, 0, 0, 0, 0, pos_byte],
                }],
            }
        }
        fn table_cell_pgnp_para(pos_byte: u8) -> Hwp5Paragraph {
            let cell = Hwp5TableCell {
                column: 0,
                row: 0,
                col_span: 1,
                row_span: 1,
                width: 4000,
                height: 1000,
                is_header: false,
                margin: crate::decoder::section::Hwp5TableCellMargin {
                    left: 0,
                    right: 0,
                    top: 0,
                    bottom: 0,
                },
                vertical_align: crate::decoder::section::Hwp5TableCellVerticalAlign::Center,
                border_fill_id: None,
                paragraphs: vec![pgnp_para(pos_byte)],
            };
            Hwp5Paragraph {
                text: "\u{FFFC}".to_string(),
                text_segments: Vec::new(),
                para_shape_id: 0,
                style_id: 0,
                char_shape_runs: vec![],
                line_segments: Vec::new(),
                controls: vec![Hwp5Control::Table(Hwp5Table {
                    rows: 1,
                    cols: 1,
                    page_break: Hwp5TablePageBreak::Cell,
                    repeat_header: false,
                    cell_spacing: 0,
                    border_fill_id: None,
                    cells: vec![cell],
                })],
            }
        }

        // para 0 = table-cell pgnp at TopLeft (pos 1); para 1 = top-level pgnp
        // at BottomCenter (pos 5). Document order picks the table-cell one.
        let section = make_section(vec![table_cell_pgnp_para(1), pgnp_para(5)], None);
        let (doc, _) = project_to_core(vec![section]).unwrap();
        let page_number =
            doc.sections()[0].page_number.as_ref().expect("a page number must be resolved");
        assert_eq!(
            page_number.position,
            PageNumberPosition::TopLeft,
            "the first page number in document order (the table cell) must win"
        );
    }

    #[test]
    fn line_control_becomes_visible_core_line() {
        let para = Hwp5Paragraph {
            text: "\u{FFFC}".to_string(),
            text_segments: Vec::new(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            line_segments: Vec::new(),
            controls: vec![Hwp5Control::Line(Hwp5LineControl {
                ctrl_id: 0x6773_6F20,
                geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                    x: 9_884,
                    y: 11_980,
                    width: 29_360,
                    height: 0,
                },
                start: crate::schema::section::Hwp5ShapePoint { x: 0, y: 0 },
                end: crate::schema::section::Hwp5ShapePoint { x: 100, y: 100 },
            })],
        };
        let section = make_section(vec![para], None);
        let (doc, _) = project_to_core(vec![section]).unwrap();
        let paragraph = &doc.sections()[0].paragraphs[0];
        let control = paragraph.runs[0].content.as_control().expect("expected control run");
        match control {
            Control::Line { start, end, width, height, horz_offset, vert_offset, .. } => {
                assert_eq!(*start, hwpforge_core::control::ShapePoint { x: 0, y: 0 });
                assert_eq!(*end, hwpforge_core::control::ShapePoint { x: 29_360, y: 100 });
                assert_eq!(*width, HwpUnit::new(29_360).unwrap());
                assert_eq!(*height, HwpUnit::new(100).unwrap());
                assert_eq!(*horz_offset, 9_884);
                assert_eq!(*vert_offset, 11_980);
            }
            other => panic!("expected Line control, got {:?}", other),
        }
    }

    #[test]
    fn polygon_control_becomes_visible_core_polygon() {
        let para = Hwp5Paragraph {
            text: "\u{FFFC}".to_string(),
            text_segments: Vec::new(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            line_segments: Vec::new(),
            controls: vec![Hwp5Control::Polygon(Hwp5PolygonControl {
                ctrl_id: 0x6773_6F20,
                geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                    x: 17_804,
                    y: 13_900,
                    width: 12_560,
                    height: 13_040,
                },
                points: vec![
                    crate::schema::section::Hwp5ShapePoint { x: 1_882, y: 0 },
                    crate::schema::section::Hwp5ShapePoint { x: 0, y: 1_405 },
                    crate::schema::section::Hwp5ShapePoint { x: 732, y: 3_675 },
                    crate::schema::section::Hwp5ShapePoint { x: 3_032, y: 3_675 },
                    crate::schema::section::Hwp5ShapePoint { x: 3_765, y: 1_405 },
                    crate::schema::section::Hwp5ShapePoint { x: 1_882, y: 0 },
                ],
            })],
        };
        let section = make_section(vec![para], None);
        let (doc, _) = project_to_core(vec![section]).unwrap();
        let paragraph = &doc.sections()[0].paragraphs[0];
        let control = paragraph.runs[0].content.as_control().expect("expected control run");
        match control {
            Control::Polygon {
                vertices,
                width,
                height,
                horz_offset,
                vert_offset,
                paragraphs,
                ..
            } => {
                assert_eq!(vertices.len(), 6);
                assert_eq!(vertices[0], hwpforge_core::control::ShapePoint { x: 6_278, y: 0 });
                assert_eq!(vertices[5], hwpforge_core::control::ShapePoint { x: 6_278, y: 0 });
                assert_eq!(*width, HwpUnit::new(12_560).unwrap());
                assert_eq!(*height, HwpUnit::new(13_040).unwrap());
                assert_eq!(*horz_offset, 17_804);
                assert_eq!(*vert_offset, 13_900);
                assert!(paragraphs.is_empty());
            }
            other => panic!("expected Polygon control, got {:?}", other),
        }
    }

    #[test]
    fn rect_control_carries_into_core_rect_without_warning() {
        let para = Hwp5Paragraph {
            text: "\u{FFFC}".to_string(),
            text_segments: Vec::new(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            line_segments: Vec::new(),
            controls: vec![Hwp5Control::Rect(crate::decoder::section::Hwp5RectControl {
                ctrl_id: 0x6773_6F20,
                geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                    x: 13_200,
                    y: 14_280,
                    width: 10_020,
                    height: 8_000,
                },
            })],
        };
        let section = make_section(vec![para], None);
        let (doc, warnings) = project_to_core(vec![section]).unwrap();
        let paragraph = &doc.sections()[0].paragraphs[0];
        let control = paragraph.runs[0].content.as_control().expect("expected control run");
        match control {
            Control::Rect { width, height, horz_offset, vert_offset, .. } => {
                assert_eq!(*width, HwpUnit::new(10_020).unwrap());
                assert_eq!(*height, HwpUnit::new(8_000).unwrap());
                assert_eq!(*horz_offset, 13_200);
                assert_eq!(*vert_offset, 14_280);
            }
            other => panic!("expected Control::Rect, got {:?}", other),
        }
        assert!(
            !warnings.iter().any(|warning| matches!(
                warning,
                Hwp5Warning::DroppedControl { control, .. } if *control == "rect"
            )),
            "rect projection should no longer emit a DroppedControl warning"
        );
    }

    #[test]
    fn unknown_control_is_ignored() {
        let para = Hwp5Paragraph {
            text: "text".to_string(),
            text_segments: Vec::new(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            line_segments: Vec::new(),
            controls: vec![Hwp5Control::Unknown { ctrl_id: 0xDEAD_BEEF, header_data: Vec::new() }],
        };
        let section = make_section(vec![para], None);
        let (doc, _) = project_to_core(vec![section]).unwrap();
        let p = &doc.sections()[0].paragraphs[0];
        // Only one text run; no table run.
        assert!(p.runs.iter().all(|r| r.content.is_text()));
        assert_eq!(p.text_content(), "text");
    }

    // ── build_empty_table ─────────────────────────────────────────────────────

    #[test]
    fn build_empty_table_correct_dimensions() {
        let mut warnings: Vec<Hwp5Warning> = Vec::new();
        let t = build_empty_table(
            &Hwp5Table {
                rows: 3,
                cols: 4,
                page_break: Hwp5TablePageBreak::Cell,
                repeat_header: true,
                cell_spacing: 0,
                border_fill_id: None,
                cells: vec![],
            },
            &mut warnings,
        );
        assert_eq!(t.row_count(), 3);
        assert_eq!(t.col_count(), 4);
        assert_eq!(t.page_break, TablePageBreak::Cell);
        assert!(t.repeat_header);
        assert_eq!(t.cell_spacing, None);
        assert_eq!(t.border_fill_id, None);
        assert!(warnings.is_empty());
    }

    #[test]
    fn build_empty_table_zero_rows_clamps_to_one() {
        let mut warnings: Vec<Hwp5Warning> = Vec::new();
        let t = build_empty_table(
            &Hwp5Table {
                rows: 0,
                cols: 2,
                page_break: Hwp5TablePageBreak::None,
                repeat_header: false,
                cell_spacing: 0,
                border_fill_id: None,
                cells: vec![],
            },
            &mut warnings,
        );
        assert_eq!(t.row_count(), 1);
        assert_eq!(t.page_break, TablePageBreak::None);
        assert!(!t.repeat_header);
        assert_eq!(t.cell_spacing, None);
        assert!(warnings.is_empty());
    }

    #[test]
    fn build_empty_table_zero_cols_clamps_to_one() {
        let mut warnings: Vec<Hwp5Warning> = Vec::new();
        let t = build_empty_table(
            &Hwp5Table {
                rows: 2,
                cols: 0,
                page_break: Hwp5TablePageBreak::Table,
                repeat_header: false,
                cell_spacing: 0,
                border_fill_id: None,
                cells: vec![],
            },
            &mut warnings,
        );
        assert_eq!(t.col_count(), 1);
        assert_eq!(t.page_break, TablePageBreak::Table);
        assert_eq!(t.cell_spacing, None);
        assert!(warnings.is_empty());
    }

    #[test]
    fn build_empty_table_unknown_page_break_emits_projection_fallback_warning() {
        let mut warnings: Vec<Hwp5Warning> = Vec::new();
        let t = build_empty_table(
            &Hwp5Table {
                rows: 1,
                cols: 1,
                page_break: Hwp5TablePageBreak::Unknown(3),
                repeat_header: true,
                cell_spacing: 0,
                border_fill_id: None,
                cells: vec![],
            },
            &mut warnings,
        );
        assert_eq!(t.page_break, TablePageBreak::Cell);
        assert_eq!(warnings.len(), 1);
        assert!(warnings.iter().any(|warning| matches!(
            warning,
            Hwp5Warning::ProjectionFallback { subject, reason }
                if *subject == "table.page_break"
                    && reason == "unknown_hwp5_table_page_break_raw=3; defaulting_to=cell"
        )));
    }
}
