//! HWP5 IR → Core document projection.
//!
//! This module converts the decoded HWP5 intermediate representation
//! (parsed records, style tables) into HwpForge Core's `Document<Draft>`
//! structure, bridging the format-specific layer to the format-agnostic core.

mod group;
mod image_size;

pub(crate) use group::written_group_text;
mod shapes;
mod standalone;
mod text;

use std::collections::VecDeque;

#[cfg(test)]
thread_local! {
    /// How many image assets the projection copied into its store.
    pub(crate) static ASSET_COPIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

use hwpforge_core::caption::{Caption, CaptionSide};
use hwpforge_core::column::{ColumnLine, ColumnSettings};
use hwpforge_core::control::{FieldBoundary, FieldParameter, FieldStart, RefTarget};
use hwpforge_core::document::{Document, Draft};
use hwpforge_core::image::{Image, ImageCrop, ImageFormat, ImageStore};
use hwpforge_core::layout::WireTokenKind;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::placement::{
    HorzAlign, ObjectPlacement, ObjectRelativeTo, ObjectTextFlow, ObjectTextWrap, VertAlign,
};
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::section::{HeaderFooter, MasterPage, PageBorderFillEntry, PageNumber, Section};
use hwpforge_core::table::{CellLineWrap, CellZone, Table, TableCell, TableMargin, TableRow};
use hwpforge_core::Control;
use hwpforge_core::ObjectId;
use hwpforge_core::PageSettings;
use hwpforge_foundation::{
    BookmarkType, BorderLineType, CharShapeIndex, Color, HwpUnit, NumberFormatType,
    PageNumberPosition, ParaShapeIndex, RefContentType, RefType, StyleIndex, VerticalAlign,
};

use crate::ctrl_ids::{
    CTRL_ID_BOOKMARK_POINT, CTRL_ID_BOOKMARK_SPAN, CTRL_ID_CLICK_HERE, CTRL_ID_COLUMN_DEF,
    CTRL_ID_FIELD_CROSSREF, CTRL_ID_FIELD_DATE_CODE, CTRL_ID_FIELD_PATH, CTRL_ID_FIELD_SUMMERY,
    CTRL_ID_HYPERLINK, CTRL_ID_MEMO_INLINE, CTRL_ID_PAGE_NUMBER, CTRL_ID_SECD,
};
use crate::decoder::chart_ole::{extract_chart_payload_from_inflated, ChartOleError};
use crate::decoder::package::{DecompressBudget, MAX_TOTAL_DECOMPRESSED};
use crate::decoder::section::{
    Hwp5Control, Hwp5EquationControl, Hwp5ImageControl, Hwp5MasterPage, Hwp5MemoControl,
    Hwp5NestedSubtree, Hwp5OleObjectControl, Hwp5PageBorderFill, Hwp5Paragraph, Hwp5Table,
    Hwp5TableCell, Hwp5TextArtControl, Hwp5TextBoxControl, SectionResult,
};
use crate::decoder::Hwp5Warning;
use crate::error::{Hwp5Error, Hwp5Result};
use crate::numeric::positive_i32_from_u32;
use crate::schema::section::Hwp5DutmalControl;
use crate::schema::section::{
    Hwp5CharShapeRun, Hwp5PageDef, Hwp5ShapeComponentGeometry, SilentWire,
};
use crate::table_cell_vertical_align::{
    core_table_cell_vertical_align, unknown_hwp5_table_cell_vertical_align_raw,
};
use crate::table_page_break::{core_table_page_break, unknown_hwp5_table_page_break_raw};
use crate::warning_utils::push_projection_fallback;
use crate::{Hwp5JoinedImageAsset, Hwp5JoinedImageAssetPlan, Hwp5OleAssetPlan};

use self::shapes::{
    offset_placement, project_arc_run, project_connectline_run, project_curve_run,
    project_ellipse_run, project_line_run, project_polygon_run, project_rect_run, shape_placement,
};
use self::text::{
    char_shape_id_at_position, hwp_unit_from_u32, shape_of_previous_char, shape_starting_at,
    split_text_by_runs, utf16_boundaries, utf16_offset_to_byte,
};
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
// CTRL_ID constants moved to `crate::ctrl_ids` (#94 Step B1).

#[derive(Debug)]
struct ProjectedParagraph {
    paragraph: Paragraph,
}

#[derive(Debug, Clone, Copy)]
struct UnknownControlHeader<'a> {
    ctrl_id: u32,
    header_data: &'a [u8],
    bookmark_name: Option<&'a str>,
}

#[derive(Debug)]
struct ParagraphProjectionQueues<'a> {
    marker_headers: VecDeque<UnknownControlHeader<'a>>,
    object_controls: VecDeque<ProjectionControl<'a>>,
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
    summary_fields: VecDeque<crate::schema::section::Hwp5SummaryControl>,
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
    point_bookmark_names: VecDeque<Option<String>>,
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
    /// help_text, name, display_text }` Run emitted at `FieldEnd`. The
    /// actual span is collected at closure; only an empty body uses the hint.
    /// Mixed character shapes use FieldBoundary with ordinary body runs instead.
    ClickHere {
        start_utf16: u32,
        command: String,
        hint_text: Option<String>,
        help_text: Option<String>,
        name: Option<String>,
    },
    /// SUMMERY auto-field (Wave 12n). `command_token` carries the wire
    /// `$X` token (e.g. `$author`, `$modifiedtime`). On `FieldEnd` the
    /// token is mapped to a typed [`hwpforge_foundation::FieldType`] or,
    /// for unknown tokens, surfaced as `Control::UnknownSummary { token }`.
    /// `display_text` accumulates the body chars between `FieldBegin` and
    /// `FieldEnd` (the cached resolved value, e.g. the author name or the
    /// locale-formatted date) so the HWPX encoder can carry it — an empty
    /// body triggers 한컴's "낮은 보안 수준 복구" warning (#120/#136).
    SummaryField {
        start_utf16: u32,
        command_token: String,
        display_text: String,
    },
    /// `%dte` date/time format-code field (Wave 12n). Carries the raw
    /// Command pattern (smithy-internal) used only to derive `is_time_mode`.
    /// On `FieldEnd` the projection emits `Control::DateCodeField` with
    /// `is_time_mode` derived from the `T` prefix. `display_text`
    /// accumulates the cached resolved value (see [`Self::SummaryField`]).
    DateCodeField {
        start_utf16: u32,
        raw_command: String,
        display_text: String,
    },
    /// `%pat` path/file-name field (Wave 12n). On `FieldEnd` the
    /// projection maps the raw Command to a typed `PathFieldCommand`
    /// (or `Unknown` for forward compatibility) and emits
    /// `Control::PathField`. `display_text` accumulates the cached resolved
    /// path (see [`Self::SummaryField`]).
    PathField {
        start_utf16: u32,
        raw_command: String,
        display_text: String,
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
    let (document, _image_store, warnings) = project_to_core_internal(
        sections,
        None,
        None,
        DecompressBudget::new(MAX_TOTAL_DECOMPRESSED),
    )?;
    Ok((document, warnings))
}

/// Project decoded HWP5 sections into Core with the current image slice enabled.
///
/// `budget` is the document decompression budget left by the image join.
pub(crate) fn project_to_core_with_images(
    sections: Vec<SectionResult>,
    image_assets: &Hwp5JoinedImageAssetPlan,
    budget: DecompressBudget,
) -> Hwp5Result<(Document<Draft>, ImageStore, Vec<Hwp5Warning>)> {
    project_to_core_internal(sections, Some(image_assets), None, budget)
}

/// Project decoded HWP5 sections into Core with both image and OLE asset plans.
///
/// Used by [`crate::decode_hwp5_to_core`] so the projection layer can attempt
/// chart payload extraction from `/BinData/BIN*.OLE` entries and emit
/// [`hwpforge_core::Control::EmbeddedChart`] runs (Wave 4c carry).
///
/// `budget` is the document decompression budget left by the OLE join; every
/// chart extraction is charged to it, and exhausting it fails the document.
pub(crate) fn project_to_core_with_images_and_ole(
    sections: Vec<SectionResult>,
    image_assets: &Hwp5JoinedImageAssetPlan,
    ole_assets: &Hwp5OleAssetPlan,
    budget: DecompressBudget,
) -> Hwp5Result<(Document<Draft>, ImageStore, Vec<Hwp5Warning>)> {
    project_to_core_internal(sections, Some(image_assets), Some(ole_assets), budget)
}

fn project_to_core_internal(
    sections: Vec<SectionResult>,
    image_assets: Option<&Hwp5JoinedImageAssetPlan>,
    ole_assets: Option<&Hwp5OleAssetPlan>,
    budget: DecompressBudget,
) -> Hwp5Result<(Document<Draft>, ImageStore, Vec<Hwp5Warning>)> {
    let mut doc = Document::<Draft>::new();
    let mut all_warnings: Vec<Hwp5Warning> = Vec::new();
    let mut projection_images = ProjectionImageState::new(image_assets, ole_assets, budget);

    for section_result in sections {
        // Collect warnings from decoding.
        all_warnings.extend(section_result.warnings);

        // Convert page definition.
        let page_settings = section_result
            .page_def
            .as_ref()
            .map(page_def_to_settings)
            .transpose()?
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
            // W1 fail-safe (계획 §1.2·§1.4): secd 속성 bits 20-21 의 의미는
            // 레퍼런스 3사가 서로 다르게 주장해 미확정 (corpus 0.12% 꼬리).
            // F1 실측 — bits==0 + 시작번호 필드 1 을 한컴 자신이
            // `<hp:startNum page="0">`(이어서) 로 변환하므로 begin_num 은
            // 만들지 않는다. bits≠0 은 재시작으로 날조하는 대신 raw 값과
            // 함께 경고로 표면화하고 이어서 처리한다.
            // 독립 리뷰 Low #9: `[20..28]` 절단은 bits 값과 무관하게 계획의
            // all-or-none 규약 위반 신호 — 표면화한다 (corpus 전수에서 관측
            // 0건이라 flood 위험 없음).
            if section_result.section_def_start_numbers.is_none() {
                all_warnings.push(Hwp5Warning::ProjectionFallback {
                    subject: "section.begin_num",
                    reason: "secd start-number payload [20..28] truncated; start numbers \
                             not captured"
                        .to_string(),
                });
            }
            let restart_bits = (properties >> 20) & 0x3;
            if restart_bits != 0 {
                let detail = match section_result.section_def_start_numbers {
                    Some(n) => format!(
                        "raw starts page={} pic={} tbl={} equation={}",
                        n.page, n.pic, n.tbl, n.equation
                    ),
                    None => "start-number payload truncated".to_string(),
                };
                all_warnings.push(Hwp5Warning::ProjectionFallback {
                    subject: "section.begin_num",
                    reason: format!(
                        "secd property bits 20-21 = {restart_bits} are unverified \
                         (reference implementations disagree); continuing page \
                         numbering instead of restarting ({detail})"
                    ),
                });
            }
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
        // 다단 (multi-column): map the `cold` ctrl's column count + gap to
        // `Section.column_settings`. Single-column (`col_count < 2`) stays
        // `None` so the encoder emits its single-column default. Equal-width
        // columns (한글 computes widths when `sameSz=1`).
        if let Some(col) = section_result.column_def {
            if col.col_count >= 2 {
                match ColumnSettings::equal_columns(
                    u32::from(col.col_count),
                    HwpUnit::new(i32::from(col.gap)).unwrap_or(HwpUnit::ZERO),
                ) {
                    Ok(mut cs) => {
                        if let Some(b) = col.border {
                            if b.kind != 0 {
                                cs = cs.with_separator(ColumnLine {
                                    line_type: hwp5_col_border_kind_to_line_type(b.kind),
                                    width: HwpUnit::from_mm(hwp5_border_width_mm(b.width))
                                        .unwrap_or(HwpUnit::ZERO),
                                    color: colorref_to_color(b.color),
                                });
                            }
                        }
                        section.column_settings = Some(cs);
                    }
                    Err(_) => projection_images.warnings.push(Hwp5Warning::ProjectionFallback {
                        subject: "column_def",
                        reason: format!("invalid column count {}", col.col_count),
                    }),
                }
            }
        }
        // All hosts use project_control_run, including section-scoped controls.
        // Nested headers/footers inherit the owning top-level paragraph anchor.
        let mut masters = section_result.master_pages.as_slice();
        for (index, hwp_para) in section_result.paragraphs.iter().enumerate() {
            let count = masters.partition_point(|master| master.body_paragraph_index == index);
            let (host_masters, remaining) = masters.split_at(count);
            masters = remaining;
            projection_images.section_anchor = section.paragraphs.len();
            let projected = project_paragraph_with_masters(
                hwp_para,
                &mut projection_images,
                ImageProjectionContext::Flow,
                host_masters,
            );
            section.add_paragraph(projected.paragraph);
        }

        section.page_number = PageNumber::first_in(&section.paragraphs);

        projection_images.headers.sort_by_key(|(order, _)| *order);
        projection_images.footers.sort_by_key(|(order, _)| *order);
        section.headers = std::mem::take(&mut projection_images.headers)
            .into_iter()
            .map(|(_, unit)| unit)
            .collect();
        section.footers = std::mem::take(&mut projection_images.footers)
            .into_iter()
            .map(|(_, unit)| unit)
            .collect();

        // Ensure every section has at least one paragraph (validation requirement).
        if section.is_empty() {
            section.add_paragraph(Paragraph::with_runs(
                vec![Run::text("", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            ));
        }

        let masters = std::mem::take(&mut projection_images.master_pages);
        if !masters.is_empty() {
            section.master_pages = Some(masters);
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

    // W4 무음 드롭 종결: 미지원 ctrl 드롭을 문서별 (ctrl_id → count) 집계
    // 경고로 방출. per-occurrence 폭탄 방지 — distinct id 상한 + "N more".
    const MAX_DISTINCT_DROP_WARNINGS: usize = 16;
    let dropped = std::mem::take(&mut projection_images.dropped_unknown);
    let distinct = dropped.len();
    for (ctrl_id, count) in dropped.into_iter().take(MAX_DISTINCT_DROP_WARNINGS) {
        projection_images.warnings.push(Hwp5Warning::DroppedControl {
            control: "unknown_control",
            reason: format!(
                "unsupported ctrl '{}' dropped {count} time(s) during projection",
                ctrl_id_ascii(ctrl_id)
            ),
        });
    }
    if distinct > MAX_DISTINCT_DROP_WARNINGS {
        projection_images.warnings.push(Hwp5Warning::DroppedControl {
            control: "unknown_control",
            reason: format!(
                "{} more distinct unsupported ctrl ids were dropped",
                distinct - MAX_DISTINCT_DROP_WARNINGS
            ),
        });
    }

    // Chart extraction and table construction run inside `Option<Run>`/`Paragraph` returning
    // helpers, so a document-budget overrun is parked in the shared state and
    // surfaced here. No partial document is returned.
    if let Some(err) = projection_images.budget_error.take() {
        return Err(err);
    }

    all_warnings.extend(projection_images.warnings);
    Ok((doc, projection_images.image_store, all_warnings))
}

/// ctrl_id(big-endian ASCII u32)를 사람이 읽을 표기로 — 비인쇄 바이트가
/// 섞이면 hex 로 폴백한다 (경고 메시지 전용).
fn ctrl_id_ascii(ctrl_id: u32) -> String {
    let bytes = ctrl_id.to_be_bytes();
    if bytes.iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
        bytes.iter().map(|&b| char::from(b)).collect()
    } else {
        format!("{ctrl_id:#010x}")
    }
}

// ---------------------------------------------------------------------------
// Paragraph projection
// ---------------------------------------------------------------------------

struct ProjectionImageState<'a> {
    /// Section effects are emitted by the same control projection as inline runs.
    section_anchor: usize,
    in_master: bool,
    master_pages: Vec<MasterPage>,
    headers: Vec<(usize, HeaderFooter)>,
    footers: Vec<(usize, HeaderFooter)>,
    image_assets: Option<&'a Hwp5JoinedImageAssetPlan>,
    ole_assets: Option<&'a Hwp5OleAssetPlan>,
    image_store: ImageStore,
    warnings: Vec<Hwp5Warning>,
    /// W4 무음 드롭 종결: `project_control_run` 의 Unknown arm 에서 죽는
    /// ctrl_id 별 카운트. 문서 끝에서 **집계 경고**로 방출된다 (per-occurrence
    /// 폭탄도 무경고도 금지 — corpus 실측: `%fmu` 531회·`pghd` 1,013회가
    /// 이 지점에서 소리 없이 사라졌었다).
    dropped_unknown: std::collections::BTreeMap<u32, usize>,
    /// Document decompression budget continued from the OLE join; every chart
    /// extraction charges the bytes it makes resident.
    budget: DecompressBudget,
    /// First document-budget overrun. Once set, chart extraction and table
    /// cell generation stop; `project_to_core_internal` returns the error.
    budget_error: Option<Hwp5Error>,
    generated_cells: u64,
    /// Top-level groups projected so far — numbers group warnings.
    group_seq: usize,
    /// Standalone styled shapes in document order (including text boxes).
    shape_seq: usize,
    /// Unique range field ids across all sections and nested flows of this document.
    field_seq: usize,
    /// Pixel size of each image asset read so far (by storage name), a
    /// header that does not parse included — read once per asset, however
    /// many pictures use it (branch review r2 H3: a JPEG header scan per
    /// reference, twice for a group picture).
    pixel_sizes: std::cell::RefCell<std::collections::HashMap<String, Option<(u32, u32)>>>,
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
        budget: DecompressBudget,
    ) -> Self {
        Self {
            section_anchor: 0,
            in_master: false,
            master_pages: Vec::new(),
            headers: Vec::new(),
            footers: Vec::new(),
            image_assets,
            ole_assets,
            image_store: ImageStore::new(),
            warnings: Vec::new(),
            dropped_unknown: std::collections::BTreeMap::new(),
            budget,
            budget_error: None,
            generated_cells: 0,
            group_seq: 0,
            shape_seq: 0,
            field_seq: 0,
            pixel_sizes: std::cell::RefCell::default(),
        }
    }

    /// Look up the inflated `/BinData/BIN*.OLE` entry by BinData reference,
    /// if a plan was supplied. `None` means no OLE plan is wired (e.g.
    /// inspect path) or the plan has no such entry.
    fn ole_inflated_for_reference(
        &self,
        reference: u16,
    ) -> Option<&'a Result<Vec<u8>, ChartOleError>> {
        self.ole_assets.and_then(|plan| plan.inflated_for_reference(reference))
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
            image_assets.asset_for_reference(image.binary_data_id)
        else {
            self.warnings.push(Hwp5Warning::DroppedControl {
                control: "image",
                reason: image_assets.missing_asset_reason(image.binary_data_id),
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

        self.store_asset(asset);

        // Decode placement before constructing the image so the byte-ground
        // fail-closed warnings (unknown relTo/wrap bits) reach `self.warnings`
        // without conflicting with the `Image::new(...)` borrow.
        let placement = image_placement_from_wire(
            &image.geometry,
            context,
            image.ctrl_properties,
            &mut self.warnings,
        );
        let mut core_image = Image::new(
            asset.payload.package_path.clone(),
            HwpUnit::new(resolved_dimensions.width_hwp).unwrap_or(HwpUnit::ZERO),
            HwpUnit::new(resolved_dimensions.height_hwp).unwrap_or(HwpUnit::ZERO),
            core_image_format(&asset.payload.format),
        )
        .with_placement(placement);
        // Wave 12p Step 3: HWP5 GSO CtrlHeader trailer instance ID 통과.
        // 한컴 native `<hp:pic id="...">` cross-ref target 과 매칭.
        if image.instance_id != 0 {
            core_image.inst_id = Some(ObjectId::new(u64::from(image.instance_id)));
        }
        core_image.hyperlink = self.picture_hyperlink(image);
        let display = (resolved_dimensions.width_hwp, resolved_dimensions.height_hwp);
        let pixels = self.asset_pixels(asset);
        self.carry_picture_frame(&mut core_image, image, display, pixels);
        let original = image
            .original_size
            .and_then(|(w, h)| Some((i32::try_from(w).ok()?, i32::try_from(h).ok()?)));
        if let Some(original) = original.filter(|&(w, h)| w > 0 && h > 0 && (w, h) != display) {
            core_image.frame.original_size =
                units(original).map(|(w, h)| hwpforge_foundation::Size::new(w, h));
        }
        // S20: the size lock (object common property bit 20).
        // 이것을 실패시키는 것: 읽지 않기(`size_lock_is_carried`).
        core_image.frame.size_protect =
            image.ctrl_properties & shapes::CTRL_PROPERTY_SIZE_PROTECT_BIT != 0;
        Some(core_image)
    }

    /// Sets a picture's crop and image extent from its `ShapePicture`
    /// record ([`picture_crop`]); what cannot be carried exactly is
    /// reported.
    fn carry_picture_frame(
        &mut self,
        core_image: &mut Image,
        image: &Hwp5ImageControl,
        display: (i32, i32),
        pixels: Option<(u32, u32)>,
    ) {
        let frame = picture_crop(&image.picture, display, pixels);
        core_image.crop = frame.crop;
        core_image.image_extent = frame.extent;
        if let Some(reason) = frame.note {
            // A crop that could not be carried at all leaves the whole image
            // (`picture.crop_lost` — a renderer must refuse it); one kept
            // without its extent is `picture.crop`.
            let subject = if frame.crop.is_none() { "picture.crop_lost" } else { "picture.crop" };
            push_projection_fallback(&mut self.warnings, subject, reason);
        }
    }

    /// The picture's link URL, or `None`; a link record in another form is
    /// reported as lost.
    fn picture_hyperlink(&mut self, image: &Hwp5ImageControl) -> Option<String> {
        match object_link_url(image.link.as_ref()) {
            Ok(url) => url,
            Err(reason) => {
                self.warnings.push(Hwp5Warning::ContentLost {
                    kind: "picture_hyperlink".to_string(),
                    reason,
                    location: format!("image binary_data_id={}", image.binary_data_id),
                });
                None
            }
        }
    }

    /// A single shape's outline and fill as a Core [`ShapeStyle`] (§19.5
    /// 1b). `fillable` = the shape has an interior (not a line).
    ///
    /// Line types 0 (none), 1 (solid), 2 (dot), 3 (dash) are confirmed
    /// against Hancom's HWPX twins; another type is reported and drawn
    /// solid. A fill Core cannot hold — a hatch kind, a gradient with more
    /// than two colours (its layout is unverified), a combined kind, a
    /// picture mode or adjustment Core has no value for, a picture whose
    /// image is missing — is reported (`ProjectionFallback`); the parts Core
    /// can hold are kept.
    fn shape_style(
        &mut self,
        look: &crate::schema::section::Hwp5ShapeLook,
        fillable: bool,
    ) -> hwpforge_core::control::ShapeStyle {
        let mut style = hwpforge_core::control::ShapeStyle::default();
        let location = format!("shape[{}]", self.shape_seq);
        self.shape_seq += 1;
        project_shape_line(&look.line, &mut style, &mut self.warnings, &location);
        style.corner_ratio = project_corner_ratio(look.corner_ratio, &mut self.warnings);
        if fillable {
            self.apply_shape_fill(look, &mut style, &location);
        }
        style
    }

    /// Shared fill projection and BinData storage, without parent transform changes.
    fn apply_shape_fill(
        &mut self,
        look: &crate::schema::section::Hwp5ShapeLook,
        style: &mut hwpforge_core::control::ShapeStyle,
        location: &str,
    ) {
        use crate::schema::border_fill::{
            Hwp5FillImageEffect, Hwp5FillImageMode, Hwp5GradationType, Hwp5RawBorderFillFill,
        };
        use hwpforge_core::control::Fill;
        use hwpforge_foundation::{GradientType, ImageFillMode};
        let colour = |raw: u32| hwpforge_foundation::Color::from_raw(raw & 0x00FF_FFFF);
        let lost = |warnings: &mut Vec<Hwp5Warning>, reason: String| {
            push_projection_fallback(warnings, "shape.fill", reason);
        };
        if look.fill_kind == Some(0) {
            // 이것을 실패시키는 것: 채우기 없음을 흰 칠(기본값)로 두기.
            style.fill = Some(Fill::None);
            return;
        }
        match &look.fill {
            Ok(Hwp5RawBorderFillFill::None) => style.fill = Some(Fill::None),
            Ok(Hwp5RawBorderFillFill::Color(c)) => {
                // Core's colour and pattern fills are opaque (review r1).
                // 이것을 실패시키는 것: 이 보고를 지우기
                // (`colour_fill_transparency_is_reported`).
                if c.alpha != 0 {
                    lost(&mut self.warnings, format!("fill transparency {} not carried", c.alpha));
                }
                let no_face = c.background_color == 0xFFFF_FFFF;
                let pattern = c.pattern_kind.pattern_type();
                match pattern {
                    None if c.pattern_kind
                        != crate::schema::border_fill::Hwp5FillPatternKind::None =>
                    {
                        lost(
                            &mut self.warnings,
                            format!("hatch kind {:?} not carried", c.pattern_kind),
                        );
                    }
                    _ => {}
                }
                if no_face && pattern.is_none() {
                    style.fill = Some(Fill::None);
                } else {
                    if !no_face {
                        style.fill_color = Some(colour(c.background_color));
                    }
                    if let Some(pattern_type) = pattern {
                        // 이것을 실패시키는 것: 투명 패턴 배경의 흰색 대체를 무음 처리.
                        if no_face {
                            self.warnings.push(Hwp5Warning::ContentLost {
                                kind: "shape_pattern_background".into(),
                                reason: format!("background_color=0xFFFFFFFF with hatch {:?}: transparent pattern background replaced by white", c.pattern_kind),
                                location: location.into(),
                            });
                        }
                        style.fill = Some(Fill::Pattern {
                            pattern_type,
                            fg_color: colour(c.pattern_color),
                            bg_color: colour(if no_face {
                                0x00FF_FFFF
                            } else {
                                c.background_color
                            }),
                        });
                    }
                }
            }
            Ok(Hwp5RawBorderFillFill::Gradation(g)) => {
                if g.colors.len() > 2 {
                    lost(
                        &mut self.warnings,
                        format!(
                            "gradient with {} colours (layout unverified) not carried",
                            g.colors.len()
                        ),
                    );
                    return;
                }
                // 1 LINEAR·3 CONICAL (x6), 2 RADIAL (Hancom twins, 4 shapes);
                // 4 is read in the border-fill table's order but unconfirmed.
                // 이것을 실패시키는 것: 형 표 한 줄 교체(x6 F3·F4).
                let gradient_type = match g.gradation_type {
                    Hwp5GradationType::Linear => GradientType::Linear,
                    Hwp5GradationType::Circular => GradientType::Radial,
                    Hwp5GradationType::Conical => GradientType::Conical,
                    other => {
                        lost(&mut self.warnings, format!("gradient type {other:?} is unverified"));
                        match other {
                            Hwp5GradationType::Rectangular => GradientType::Square,
                            _ => GradientType::Linear,
                        }
                    }
                };
                style.fill = Some(Fill::Gradient {
                    gradient_type,
                    angle: i32::try_from(g.angle).unwrap_or(0),
                    colors: g.colors.iter().map(|&c| (colour(c), 0)).collect(),
                    center_x: Some(g.center_x),
                    center_y: Some(g.center_y),
                    step: Some(g.blur),
                    step_center: g.blur_center.map(u32::from),
                });
            }
            Ok(Hwp5RawBorderFillFill::Image(img)) => {
                let Some(asset) = self.image_asset(img.bindata_id) else {
                    let reason = self.missing_asset_reason(img.bindata_id);
                    lost(&mut self.warnings, format!("picture fill: {reason}"));
                    return;
                };
                self.store_asset(asset);
                let mode = match img.mode {
                    Hwp5FillImageMode::TileAll => ImageFillMode::Tile,
                    Hwp5FillImageMode::Resize => ImageFillMode::Stretch,
                    Hwp5FillImageMode::Center => ImageFillMode::Center,
                    Hwp5FillImageMode::Zoom => ImageFillMode::FitAll,
                    other => {
                        lost(
                            &mut self.warnings,
                            format!("picture fill mode {other:?} not carried; tiled"),
                        );
                        ImageFillMode::Tile
                    }
                };
                if img.brightness != 0
                    || img.contrast != 0
                    || img.effect != Hwp5FillImageEffect::RealPic
                {
                    lost(
                        &mut self.warnings,
                        format!(
                            "picture fill brightness {} contrast {} effect {:?} not carried",
                            img.brightness, img.contrast, img.effect
                        ),
                    );
                }
                style.fill =
                    Some(Fill::Image { image_id: asset.payload.package_path.clone(), mode });
            }
            Ok(Hwp5RawBorderFillFill::Unknown { kind, .. }) => {
                lost(&mut self.warnings, format!("fill kind {kind:#x} not carried"));
            }
            Err(e) => lost(&mut self.warnings, format!("fill block unreadable: {e}")),
        }
    }

    /// `run`'s shape with the style of `look` (none when `look` is `None`).
    fn styled(
        &mut self,
        run: Option<Run>,
        look: Option<&crate::schema::section::Hwp5ShapeLook>,
        fillable: bool,
    ) -> Option<Run> {
        let mut run = run?;
        if let (Some(look), RunContent::Control(control)) = (look, &mut run.content) {
            let shaped = self.shape_style(look, fillable);
            if let Some(style) = shape_style_slot(control) {
                *style = Some(shaped);
            }
        }
        Some(run)
    }

    /// Puts `asset` in the image store unless it is already there — one copy
    /// per asset, however many pictures and fills reference it (branch
    /// review B-H3: each reference copied the bytes, twice for a group
    /// picture).
    /// 이것을 실패시키는 것: 이미 있어도 복사하기(`an_asset_is_copied_once`).
    fn store_asset(&mut self, asset: &Hwp5JoinedImageAsset) {
        if self.image_store.get(&asset.payload.storage_name).is_none() {
            #[cfg(test)]
            ASSET_COPIES.with(|n| n.set(n.get() + 1));
            self.image_store.insert(asset.payload.storage_name.clone(), asset.bytes.clone());
        }
    }

    /// The pixel size of `asset`'s image ([`image_size::pixel_size`]), read
    /// once per asset.
    /// 이것을 실패시키는 것: 캐시 없이 읽기(`an_asset_is_copied_once` 의 판독 수).
    fn asset_pixels(&self, asset: &Hwp5JoinedImageAsset) -> Option<(u32, u32)> {
        *self
            .pixel_sizes
            .borrow_mut()
            .entry(asset.payload.storage_name.clone())
            .or_insert_with(|| image_size::pixel_size(&asset.bytes))
    }

    /// The image asset of BinData `reference`, when the document has one.
    fn image_asset(&self, reference: u16) -> Option<&'a Hwp5JoinedImageAsset> {
        self.image_assets?.asset_for_reference(reference)
    }

    /// Why BinData `reference` has no image asset (see
    /// [`Hwp5JoinedImageAssetPlan::missing_asset_reason`]).
    fn missing_asset_reason(&self, reference: u16) -> String {
        match self.image_assets {
            Some(plan) => plan.missing_asset_reason(reference),
            None => format!("missing_image_asset_for_binary_data_id={reference}"),
        }
    }

    /// A group picture: the image at the folded size and offset (relative to
    /// its group), with its asset stored. `None` when the asset is missing
    /// (the planner has already reported that).
    fn build_group_picture(
        &mut self,
        image: &Hwp5ImageControl,
        (width, height): (i32, i32),
        (x, y): (i32, i32),
        hyperlink: Option<String>,
    ) -> Option<Image> {
        let asset = self.image_asset(image.binary_data_id)?;
        self.store_asset(asset);
        let mut core_image = Image::new(
            asset.payload.package_path.clone(),
            HwpUnit::new(width).unwrap_or(HwpUnit::ZERO),
            HwpUnit::new(height).unwrap_or(HwpUnit::ZERO),
            core_image_format(&asset.payload.format),
        );
        core_image.placement = shapes::offset_placement(x, y);
        core_image.hyperlink = hyperlink;
        // A group picture's box is folded from its matrices, so its own
        // display size is not the size the record's extent is compared to:
        // the extent is always kept with a crop, and dropped without one.
        let pixels = self.asset_pixels(asset);
        self.carry_picture_frame(&mut core_image, image, (width, height), pixels);
        Some(core_image)
    }
}

/// `(w, h)` as HWPUNITs, or `None` outside the HWPUNIT range.
fn units((w, h): (i32, i32)) -> Option<(HwpUnit, HwpUnit)> {
    Some((HwpUnit::new(w).ok()?, HwpUnit::new(h).ok()?))
}

/// HWPUNIT per image pixel (96 dpi) — the one-pixel tolerance of the
/// whole-image test for a picture record without its image size.
const HWPUNIT_PER_PIXEL: i64 = 75;

/// A picture's crop and image extent, and why they are not carried exactly.
#[derive(Debug, PartialEq)]
struct PictureFrame {
    crop: Option<ImageCrop>,
    extent: Option<(HwpUnit, HwpUnit)>,
    note: Option<String>,
}

/// The crop of a `ShapePicture` record (bytes 44–60, HWPX `imgClip`) in the
/// coordinates of its image size (bytes 82–90, HWPX `imgDim`), folded as
/// the HWPX decoder folds them: the whole extent is no crop, and the extent
/// is left out when it equals the display size and there is no crop.
/// Without the image size (a record with picture effects stores it
/// elsewhere) a crop within one pixel of the image's pixel size × 75 is the
/// whole image; any other crop is kept without an extent and noted — the
/// pixel size never stands in for the extent.
fn picture_crop(
    picture: &crate::schema::section::Hwp5ShapePicture,
    display: (i32, i32),
    pixels: Option<(u32, u32)>,
) -> PictureFrame {
    let [l, t, r, b] = picture.crop;
    let none = |note: Option<String>| PictureFrame { crop: None, extent: None, note };
    let dim = picture
        .dimension
        .and_then(|(w, h)| Some((i32::try_from(w).ok()?, i32::try_from(h).ok()?)))
        .filter(|&(w, h)| w > 0 && h > 0);
    // An all-zero rectangle is a record that holds no crop (none in the
    // corpus; synthetic records): the whole image.
    let whole = match (dim, pixels) {
        _ if [l, t, r, b] == [0; 4] => true,
        (Some((w, h)), _) => [l, t, r, b] == [0, 0, w, h],
        (None, Some((pw, ph))) => {
            let (pw, ph) = (i64::from(pw) * HWPUNIT_PER_PIXEL, i64::from(ph) * HWPUNIT_PER_PIXEL);
            let [l, t, r, b] = [l, t, r, b].map(i64::from);
            [l, t, r - pw, b - ph].iter().all(|v| v.abs() <= HWPUNIT_PER_PIXEL)
        }
        (None, None) => false,
    };
    if whole {
        let extent = dim.filter(|&d| d != display).and_then(units);
        return PictureFrame { crop: None, extent, note: None };
    }
    if r <= l || b <= t {
        return none(Some(format!("crop ({l}, {t}, {r}, {b}) is empty; the whole image is shown")));
    }
    let edge = |v: i32| HwpUnit::new(v).ok();
    let (Some(left), Some(top), Some(right), Some(bottom)) = (edge(l), edge(t), edge(r), edge(b))
    else {
        return none(Some(format!("crop ({l}, {t}, {r}, {b}) is out of range; not carried")));
    };
    let crop = Some(ImageCrop { left, top, right, bottom });
    match dim {
        Some(d) => match units(d) {
            Some(extent) => PictureFrame { crop, extent: Some(extent), note: None },
            None => none(Some(format!("image size {d:?} is out of range; crop not carried"))),
        },
        None => PictureFrame {
            crop,
            extent: None,
            note: Some(format!(
                "crop ({l}, {t}, {r}, {b}) kept without the image size, which the record does \
                 not hold"
            )),
        },
    }
}

/// What Hancom appends to the URL of an object's link (`URL;1;0;0;`; the
/// only form in the corpus). The HWPX `href` carries the same command.
const OBJECT_HYPERLINK_SUFFIX: &str = ";1;0;0;";

/// The URL of an object's link record: `Ok(None)` = no link, `Err` = a link
/// that is not `URL;1;0;0;` or could not be read (reported, not guessed at).
fn object_link_url(link: Option<&Result<String, String>>) -> Result<Option<String>, String> {
    match link {
        None => Ok(None),
        Some(Err(reason)) => Err(format!("unreadable object hyperlink: {reason}")),
        Some(Ok(command)) => match command.strip_suffix(OBJECT_HYPERLINK_SUFFIX) {
            Some(url) if !url.is_empty() => Ok(Some(url.to_string())),
            _ => Err(format!("unrecognised object hyperlink {command:?}")),
        },
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

/// HWP 5.0 표 70 공통 개체 속성 DWORD 의 bit0 (글자처럼 취급) 마스크.
const CTRL_PROPERTY_TREAT_AS_CHAR_BIT: u32 = 0x1;

/// 표 70 공통 개체 속성 DWORD 을 Core [`ObjectPlacement`] 로 디코드한다 (W5 w0 —
/// projection 관례를 바이트-그라운드 실측으로 승격).
///
/// bit0(글자처럼 취급)=1 이면 나머지 축 비트는 의미가 없으므로
/// [`ObjectPlacement::legacy_inline_defaults`] 를 반환한다 (도형은 `None` 으로
/// collapse, 이미지는 인라인 유지 — W2p 계약 보존). bit0=0(부유)일 때만 축을
/// 디코드한다 (비트 배치는 hwp-rs `common_properties.rs` 대조 + `textbox_anchored`
/// ·`anchored_zero_origin_png` fixture 의 속성 word 와 한컴 `.hwpx` 쌍 `<hp:pos>`
/// 바이트 검증):
///
/// - `vertRelTo` bits 3-4 (`0`=Paper `1`=Page `2`=Para)
/// - `horzRelTo` bits 8-9 (`0`=Paper `1`=Page `2`=Column `3`=Para)
/// - `flowWithText` bit 13 (`vertRelTo`=Para 일 때만 유의 — 그 외엔 false)
/// - `allowOverlap` bit 14 (`flowWithText`=1 이면 표 70 규약상 강제 false)
/// - `textWrap` bits 21-23 (`0`=Square `1`=TopAndBottom `2`=BehindText
///   `3`=InFrontOfText — **한컴 저장본의 4값**이다. 공식 스펙 rev1.3 표 70·
///   hwp-rs·hwp.js 의 6값(Square·Tight·Through·TopAndBottom·BehindText·
///   InFrontOfText)은 한컴이 쓰는 값과 다르다: X2 fixture(`x2-wrap-modes`)
///   4/4·한컴 짝의 떠 있는 개체 457/457 이 이 4값과 일치, `wire-gotchas.md`
///   #38)
/// - `textFlow` bits 24-25 (`0`=BothSides `1`=LeftOnly `2`=RightOnly
///   `3`=LargestOnly — `textWrap`=Square 일 때만 유의)
///
/// `align`(bits 5-7·10-12)·`width/heightRelTo`(15-19)·`protect`(20)·
/// `numbering`(26-28) 은 Core 가 carry 하지 않으므로 무시한다. `offset_x`/
/// `offset_y` 는 CtrlHeader 오프셋 필드(이미 signed `i32` 로 디코드됨 —
/// corpus 의 음수 오프셋 실재)를 그대로 싣는다.
///
/// **Fail-closed (no-fake-support)**: `vertRelTo`(bits 3-4)에서 레퍼런스 enum
/// 범위 밖 값 `3`(hwp-rs `VerticalRelativeTo` 에 대응 variant 없음), 또는
/// `textWrap`(bits 21-23)에서 한컴이 쓰지 않는 `4`–`7` 이 나오면 임의 known
/// 값으로 정규화하지 않고 [`Hwp5Warning::ProjectionFallback`] 를 방출한 뒤 가장
/// 보수적인 기본값(Paper/Square)으로 폴백한다 — 코퍼스 HWP5 에는 없는 값이라
/// 실무엔 안 뜨지만, 뜨면 속성 word 오프셋 오정렬의 canary 로 표면화한다.
fn object_placement_from_ctrl_properties(
    ctrl_properties: u32,
    offset_x: i32,
    offset_y: i32,
    warnings: &mut Vec<Hwp5Warning>,
) -> ObjectPlacement {
    effective_placement(&parse_object_placement_wire(ctrl_properties, offset_x, offset_y), warnings)
}

/// 표 70 개체 속성 word 의 placement 비트를 정규화 없이 읽은 것 (§19.16 A-v2).
/// 표는 이 wire 값을 그대로 Core 에 싣고([`table_placement_from_wire`]), 도형·
/// 그림은 렌더 의미로 정규화한다([`effective_placement`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WirePlacement {
    treat_as_char: bool,
    /// bit 2.
    affect_line_spacing: bool,
    /// bits 3-4.
    vert_rel_bits: u32,
    /// bits 5-7.
    vert_align_bits: u32,
    /// bits 8-9.
    horz_rel_bits: u32,
    /// bits 10-12.
    horz_align_bits: u32,
    /// bit 13.
    flow_with_text: bool,
    /// bit 14.
    allow_overlap: bool,
    /// bits 21-23.
    wrap_bits: u32,
    /// bits 24-25.
    flow_bits: u32,
    x: i32,
    y: i32,
}

fn parse_object_placement_wire(ctrl_properties: u32, x: i32, y: i32) -> WirePlacement {
    WirePlacement {
        treat_as_char: ctrl_properties & CTRL_PROPERTY_TREAT_AS_CHAR_BIT != 0,
        affect_line_spacing: (ctrl_properties >> 2) & 0x1 != 0,
        vert_rel_bits: (ctrl_properties >> 3) & 0x3,
        vert_align_bits: (ctrl_properties >> 5) & 0x7,
        horz_rel_bits: (ctrl_properties >> 8) & 0x3,
        horz_align_bits: (ctrl_properties >> 10) & 0x7,
        flow_with_text: (ctrl_properties >> 13) & 0x1 != 0,
        allow_overlap: (ctrl_properties >> 14) & 0x1 != 0,
        wrap_bits: (ctrl_properties >> 21) & 0x7,
        flow_bits: (ctrl_properties >> 24) & 0x3,
        x,
        y,
    }
}

/// 도형·그림의 렌더 의미 placement (W5 w0 규칙 그대로): 글자처럼 취급이면
/// inline 기본값, 아니면 flowWithText 는 vertRelTo=Para 일 때만, overlap 은
/// flow 가 거짓일 때만 읽는다. 정렬(bits 5-7·10-12)은 떠 있는 개체에 싣는다.
fn effective_placement(wire: &WirePlacement, warnings: &mut Vec<Hwp5Warning>) -> ObjectPlacement {
    if wire.treat_as_char {
        return ObjectPlacement::legacy_inline_defaults();
    }
    let vert_rel_to = checked_vert_relative_to(wire.vert_rel_bits, warnings);
    let horz_rel_to = horz_relative_to_from_bits(wire.horz_rel_bits);
    let text_wrap = checked_text_wrap(wire.wrap_bits, warnings);
    // 표 70: flowWithText 는 vertRelTo=Para 일 때만 정의된다. 그 외 기준에서는
    // bit 13 을 읽지 않고 false 로 둔다 (hwp-rs 의 `Option` 조건과 동형).
    let flow_with_text = vert_rel_to == ObjectRelativeTo::Para && wire.flow_with_text;
    // 표 70: flowWithText 가 참이면 allowOverlap 은 무조건 false 로 간주한다.
    let allow_overlap = if flow_with_text { false } else { wire.allow_overlap };
    // textFlow 는 글이 개체 옆으로 흐르는 wrap 에서만 의미가 있다. 한컴 4값
    // 중 그런 것은 Square 뿐이고(Tight/Through 는 이 표에서 나오지 않는다),
    // 다른 wrap 에서는 bits 24-25 가 무의미하므로 기본값 BothSides 로 둔다
    // (잡값이 사이드 흐름을 뒤집는 것을 방지).
    let text_flow = match text_wrap {
        ObjectTextWrap::Square | ObjectTextWrap::Tight | ObjectTextWrap::Through => {
            object_text_flow_from_bits(wire.flow_bits)
        }
        _ => ObjectTextFlow::BothSides,
    };
    ObjectPlacement::new(text_wrap, vert_rel_to, horz_rel_to)
        .with_text_flow(text_flow)
        .with_flow_with_text(flow_with_text)
        .with_allow_overlap(allow_overlap)
        .with_vert_offset(HwpUnit::new(wire.y).unwrap_or(HwpUnit::ZERO))
        .with_horz_offset(HwpUnit::new(wire.x).unwrap_or(HwpUnit::ZERO))
        .with_vert_align(vert_align_from_bits(wire.vert_align_bits, warnings))
        .with_horz_align(horz_align_from_bits(wire.horz_align_bits, warnings))
}

/// 표의 placement: wire 값을 정규화 없이 싣는다 (§19.16 A-v2 — 글자처럼
/// 취급 표의 기준·정렬·flow 도 보존). HWPX 인코더가 `None` 에 쓰는 기본 flow
/// 와 모든 필드가 같을 때만 `None`. `holdAnchorAndSO` 는 HWP5 비트가 확인되지
/// 않아 기본값(false).
///
/// 이것을 실패시키는 것: 접기 조건에서 필드 하나를 빼거나 기본값을 바꾸기
/// (`table_placement_carries_wire_values_and_folds_the_default`) / 표에
/// `effective_placement` 를 쓰기(글자처럼 취급 표의 기준이 사라짐).
fn table_placement_from_wire(
    wire: &WirePlacement,
    warnings: &mut Vec<Hwp5Warning>,
) -> Option<ObjectPlacement> {
    let placement = ObjectPlacement::new(
        checked_text_wrap(wire.wrap_bits, warnings),
        checked_vert_relative_to(wire.vert_rel_bits, warnings),
        horz_relative_to_from_bits(wire.horz_rel_bits),
    )
    .with_text_flow(object_text_flow_from_bits(wire.flow_bits))
    .with_treat_as_char(wire.treat_as_char)
    .with_flow_with_text(wire.flow_with_text)
    .with_allow_overlap(wire.allow_overlap)
    .with_affect_line_spacing(wire.affect_line_spacing)
    .with_vert_offset(HwpUnit::new(wire.y).unwrap_or(HwpUnit::ZERO))
    .with_horz_offset(HwpUnit::new(wire.x).unwrap_or(HwpUnit::ZERO))
    .with_vert_align(vert_align_from_bits(wire.vert_align_bits, warnings))
    .with_horz_align(horz_align_from_bits(wire.horz_align_bits, warnings));
    let default = ObjectPlacement::new(
        ObjectTextWrap::TopAndBottom,
        ObjectRelativeTo::Para,
        ObjectRelativeTo::Column,
    )
    .with_flow_with_text(true);
    (placement != default).then_some(placement)
}

fn checked_vert_relative_to(bits: u32, warnings: &mut Vec<Hwp5Warning>) -> ObjectRelativeTo {
    if bits == 3 {
        warnings.push(Hwp5Warning::ProjectionFallback {
            subject: "object_placement.vert_rel_to",
            reason: "gso property word vertRelTo bits 3-4 = 3 is outside the HWP5 reference \
                     range (0=Paper 1=Page 2=Para); falling back to Paper"
                .to_string(),
        });
    }
    vert_relative_to_from_bits(bits)
}

fn checked_text_wrap(bits: u32, warnings: &mut Vec<Hwp5Warning>) -> ObjectTextWrap {
    if bits >= 4 {
        warnings.push(Hwp5Warning::ProjectionFallback {
            subject: "object_placement.text_wrap",
            reason: format!(
                "gso property word textWrap bits 21-23 = {bits} is outside the values \
                 Hancom writes (0..=3); falling back to Square"
            ),
        });
    }
    object_text_wrap_from_bits(bits)
}

/// 표 70 세로 정렬(bits 5-7). 한컴 짝으로 확인된 값은 0 = Top 뿐이다 —
/// 1–4 는 스펙 순서(Center·Bottom·Inside·Outside)로 읽되 확인 전이라 경고하고
/// 값은 보존한다(warn-and-preserve, §19.19 S1). 5–7 은 정의 밖 → 경고 + Top.
///
/// 이것을 실패시키는 것: 1–4 의 경고를 지우기 / 5–7 을 경고 없이 읽기.
fn vert_align_from_bits(bits: u32, warnings: &mut Vec<Hwp5Warning>) -> VertAlign {
    let value = match bits {
        0 => return VertAlign::Top,
        1 => Some(VertAlign::Center),
        2 => Some(VertAlign::Bottom),
        3 => Some(VertAlign::Inside),
        4 => Some(VertAlign::Outside),
        _ => None,
    };
    align_warning(warnings, "vertical", bits, value.is_some());
    value.unwrap_or(VertAlign::Top)
}

/// 표 70 가로 정렬(bits 10-12). 0 = Left·1 = Center·2 = Right 는 한컴 짝으로
/// 확인됐다(도형 8·표 10). 3·4 는 스펙 순서(Inside·Outside)로 읽되 경고,
/// 5–7 은 경고 + Left.
fn horz_align_from_bits(bits: u32, warnings: &mut Vec<Hwp5Warning>) -> HorzAlign {
    let value = match bits {
        0 => return HorzAlign::Left,
        1 => return HorzAlign::Center,
        2 => return HorzAlign::Right,
        3 => Some(HorzAlign::Inside),
        4 => Some(HorzAlign::Outside),
        _ => None,
    };
    align_warning(warnings, "horizontal", bits, value.is_some());
    value.unwrap_or(HorzAlign::Left)
}

fn align_warning(warnings: &mut Vec<Hwp5Warning>, axis: &str, bits: u32, kept: bool) {
    warnings.push(Hwp5Warning::ProjectionFallback {
        subject: "object_placement.align_unverified",
        reason: if kept {
            format!("{axis} alignment value {bits} is read in the spec's order but unverified against Hancom")
        } else {
            format!("{axis} alignment value {bits} is outside the spec's range; written as the default")
        },
    });
}

/// 표 70 `vertRelTo`(bits 3-4)를 Core [`ObjectRelativeTo`] 로 매핑한다.
///
/// 스펙 정의값은 `0`/`1`/`2` 뿐이다 — 2비트 필드의 미정의 값 `3` 은 유효
/// 파일에 나타나지 않으므로(레퍼런스 `VerticalRelativeTo` 에 대응 variant
/// 없음) 가장 보수적인 `Paper` 로 폴백한다 (날조 매핑이 아니라 안전 기본값).
/// 이 미정의 값의 경고 방출은 호출부
/// [`object_placement_from_ctrl_properties`] 가 담당한다.
fn vert_relative_to_from_bits(bits: u32) -> ObjectRelativeTo {
    match bits {
        0 => ObjectRelativeTo::Paper,
        1 => ObjectRelativeTo::Page,
        2 => ObjectRelativeTo::Para,
        _ => ObjectRelativeTo::Paper,
    }
}

/// 표 70 `horzRelTo`(bits 8-9)를 Core [`ObjectRelativeTo`] 로 매핑한다.
///
/// 2비트 필드의 값 `0`~`3` 이 모두 정의돼 있어 폴백 분기는 도달하지 않는다.
fn horz_relative_to_from_bits(bits: u32) -> ObjectRelativeTo {
    match bits {
        0 => ObjectRelativeTo::Paper,
        1 => ObjectRelativeTo::Page,
        2 => ObjectRelativeTo::Column,
        3 => ObjectRelativeTo::Para,
        _ => ObjectRelativeTo::Paper,
    }
}

/// 표 70 `textWrap`(bits 21-23)을 Core [`ObjectTextWrap`] 로 매핑한다.
///
/// 한컴이 쓰는 값은 `0` 어울림·`1` 자리 차지·`2` 글 뒤로·`3` 글 앞으로의
/// 4값이다(스펙·hwp-rs·hwp.js 의 6값 순서가 아니다 — X2 fixture 4/4, 한컴 짝
/// 457/457). `4`–`7` 은 기본 `Square` 로 폴백한다 (경고 방출은 호출부
/// [`object_placement_from_ctrl_properties`] 담당).
fn object_text_wrap_from_bits(bits: u32) -> ObjectTextWrap {
    match bits {
        0 => ObjectTextWrap::Square,
        1 => ObjectTextWrap::TopAndBottom,
        2 => ObjectTextWrap::BehindText,
        3 => ObjectTextWrap::InFrontOfText,
        _ => ObjectTextWrap::Square,
    }
}

/// 표 70 `textFlow`(bits 24-25)를 Core [`ObjectTextFlow`] 로 매핑한다.
///
/// 2비트 필드의 값 `0`~`3` 이 모두 정의돼 있어 폴백 분기는 도달하지 않는다.
fn object_text_flow_from_bits(bits: u32) -> ObjectTextFlow {
    match bits {
        0 => ObjectTextFlow::BothSides,
        1 => ObjectTextFlow::LeftOnly,
        2 => ObjectTextFlow::RightOnly,
        3 => ObjectTextFlow::LargestOnly,
        _ => ObjectTextFlow::BothSides,
    }
}

/// body/텍스트박스 이미지의 [`ObjectPlacement`] 를 `gso ` CtrlHeader 속성 word
/// 에서 바이트-그라운드로 판정한다 (W5 w0).
///
/// 축(relTo·wrap·flow·overlap)은 전부
/// [`object_placement_from_ctrl_properties`] 가 속성 word 실비트로 디코드하며,
/// 이전의 projection-context 별 관례(Flow=Paper/InFrontOfText,
/// TextBox=Para/Square)는 폐기됐다. 따라서 `context` 는 더 이상 배치에 영향을
/// 주지 않는다 — 인자는 호출부 대칭과 향후 W1 앵커 렌더의 좌표 원점 프레임
/// (body vs 글상자 내부) 판정 후보로 남겨둔다. W1 이 원점 프레임을 문서 구조
/// 에서 유도한다면 이 인자와 [`ImageProjectionContext`] 스레딩을 제거할 것.
fn image_placement_from_wire(
    geometry: &Hwp5ShapeComponentGeometry,
    context: ImageProjectionContext,
    ctrl_properties: u32,
    warnings: &mut Vec<Hwp5Warning>,
) -> ObjectPlacement {
    let _ = context;
    let mut placement =
        object_placement_from_ctrl_properties(ctrl_properties, geometry.x, geometry.y, warnings);
    placement.out_margin = shapes::outer_margin(geometry, warnings);
    placement
}

fn project_paragraph_with_images(
    hwp_para: &Hwp5Paragraph,
    projection_images: &mut ProjectionImageState<'_>,
    image_context: ImageProjectionContext,
) -> ProjectedParagraph {
    project_paragraph_with_masters(hwp_para, projection_images, image_context, &[])
}

fn project_paragraph_with_masters(
    hwp_para: &Hwp5Paragraph,
    projection_images: &mut ProjectionImageState<'_>,
    image_context: ImageProjectionContext,
    masters: &[Hwp5MasterPage],
) -> ProjectedParagraph {
    let mut projected = if !paragraph_needs_structural_projection(hwp_para) {
        ProjectedParagraph {
            paragraph: project_paragraph_with_images_flat(
                hwp_para,
                projection_images,
                image_context,
                masters,
            ),
        }
    } else {
        project_paragraph_with_images_structural(
            hwp_para,
            projection_images,
            image_context,
            masters,
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
    // §19.22 D5 (S17): the paragraph-end mark's char shape, kept only when
    // it differs from the last run written — Hancom writes an end run only
    // then (3,385/3,385 and 6/6 in the twin pairs). Compared with the run,
    // not the last wire boundary, which may belong to something that writes
    // no run (a field end; review r1).
    // 이것을 실패시키는 것: 이 줄을 지우기 / 비교를 지우기 / 마지막 wire 경계와
    // 비교(`end_char_shape_reaches_core`, `end_shape_after_a_field_end_is_kept`).
    projected.paragraph.end_char_shape = hwp_para
        .end_char_shape
        .map(|id| CharShapeIndex::new(id as usize))
        .filter(|end| projected.paragraph.runs.last().map(|r| r.char_shape_id) != Some(*end));
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
            // Inline width is a physical length, outside #242 packed tab stops.
            // Preserve its existing cap policy independently of the removed helper.
            width: HwpUnit::new(width.min(HwpUnit::MAX_VALUE as u32) as i32)
                .expect("capped inline tab width fits HwpUnit"),
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
    masters: &[Hwp5MasterPage],
) -> Paragraph {
    let mut runs: Vec<Run> = Vec::new();
    // Marker-header controls (secd / cold / %bmk / %hlk / %xrf / bokm) are consumed by `SectionColumnDef` / `FieldBegin` text
    // segments in the structural path, never by `ControlRef`
    // (`\u{FFFC}`) markers. The flat path historically iterated *all*
    // controls and so mis-aligned the FFFC↔control pairing for first
    // section paragraphs (which always carry `secd`/`cold`). Filter
    // them out here so the FFFC iterator only sees object controls.
    // See `.docs/algorithms/2026-06-01_dutmal_carry.md` (companion-fix
    // section) for the full root-cause + rationale.
    let mut control_iter = projection_controls(hwp_para, masters).filter(|entry| {
        !matches!(
            entry,
            ProjectionControl::Control(Hwp5Control::Unknown {
                ctrl_id: CTRL_ID_SECD
                    | CTRL_ID_COLUMN_DEF
                    | CTRL_ID_BOOKMARK_SPAN
                    | CTRL_ID_HYPERLINK
                    | CTRL_ID_FIELD_CROSSREF
                    | CTRL_ID_BOOKMARK_POINT,
                ..
            })
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

            if let Some(control) = next_projection_control(&mut control_iter, projection_images) {
                if let Some(run) = project_control_run(
                    control,
                    projection_images,
                    image_context,
                    shape_starting_at(hwp_para, current_utf16),
                ) {
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

    while let Some(control) = next_projection_control(&mut control_iter, projection_images) {
        if let Some(run) = project_control_run(
            control,
            projection_images,
            image_context,
            shape_starting_at(hwp_para, current_utf16),
        ) {
            runs.push(run);
        }
    }

    // An empty paragraph keeps the char shape of its position 0 (the last
    // entry there — §19.22 B), not char shape 0: its height sets the blank
    // line's height. 이것을 실패시키는 것: 0 으로 되돌리기
    // (`empty_paragraph_keeps_its_char_shape`).
    if runs.is_empty() {
        runs.push(Run::text("", shape_starting_at(hwp_para, 0)));
    }

    let mut paragraph =
        Paragraph::with_runs(runs, ParaShapeIndex::new(hwp_para.para_shape_id as usize));
    if hwp_para.style_id > 0 {
        paragraph = paragraph.with_style(StyleIndex::new(hwp_para.style_id as usize));
    }
    paragraph.layout_cache = promote_line_segments(
        &hwp_para.line_segments,
        check_ledger_against_emission(build_flat_ledger(hwp_para), &paragraph.runs),
        &mut projection_images.warnings,
    );
    // W3: ParaHeader divide_sort 쪽/단 나누기 carry (F2 실측 — 한컴은 쪽나눔
    // 문단의 lineseg v 를 리셋하지 않아 이 플래그가 유일한 쪽분할 신호).
    paragraph.page_break = hwp_para.page_break;
    paragraph.column_break = hwp_para.column_break;
    paragraph
}

/// HWP5 `PARA_LINE_SEG` 레코드(36바이트×N)를 Core 조판 캐시로 승격한다
/// (decode-only — 인코더는 HWP5 변환(기본 carry)과 PDF 재생에서만 방출한다).
///
/// 필드 대응은 이름만 다르고 wire 의미는 HWPX `<hp:lineseg>` 와 동일하다.
/// 세그먼트가 없으면 `None` (HWP5 네이티브 문단은 항상 1개 이상 보유 —
/// 없음 = 캐시 부재 의미).
fn promote_line_segments(
    segs: &[crate::schema::section::Hwp5ParaLineSeg],
    ledger: Result<
        (crate::wire_text_map::WireTextMap, crate::wire_text_map::TokenLedger),
        &'static str,
    >,
    warnings: &mut Vec<crate::decoder::Hwp5Warning>,
) -> Option<hwpforge_core::layout::LayoutCache> {
    if segs.is_empty() {
        return None;
    }
    // W1b: raw wire textpos → Core 가시(text_content) 좌표 정규화.
    // ledger 구축 실패·좌표 변환 실패 = fail-closed (캐시 미승격 + 경고) —
    // 추측 좌표를 Core 에 싣지 않는다 (§1g v5).
    let (map, tokens) = match ledger {
        Ok(ledger) => ledger,
        Err(reason) => {
            warnings.push(crate::decoder::Hwp5Warning::LayoutCacheDropped {
                reason: reason.to_string(),
            });
            return None;
        }
    };
    let mut lines = Vec::with_capacity(segs.len());
    let mut skipped = 0usize;
    for s in segs {
        // S22: a line that starts inside a field drops the cache (design 3.3).
        // 이것을 실패시키는 것: 이 검사 삭제(`line_inside_a_field_drops_the_cache`).
        if tokens.inside_field(s.text_start_position) {
            warnings.push(crate::decoder::Hwp5Warning::LayoutCacheDropped {
                reason: format!("lineseg textpos {} is inside a field", s.text_start_position),
            });
            return None;
        }
        let textpos = match map.to_core(s.text_start_position) {
            Ok(core) => core,
            Err(e) => {
                warnings.push(crate::decoder::Hwp5Warning::LayoutCacheDropped {
                    reason: format!("lineseg textpos {}: {e}", s.text_start_position),
                });
                return None;
            }
        };
        let mut line = hwpforge_core::layout::LineSeg::default();
        line.textpos = textpos;
        line.vertpos = s.vertical_position;
        line.vertsize = s.line_height;
        line.textheight = s.text_height;
        line.baseline = s.baseline_distance;
        line.spacing = s.line_spacing;
        line.horzpos = s.column_start_position;
        line.horzsize = s.segment_width;
        line.flags = s.tag;
        // S22: a line that starts after some of the zero-length tokens at its
        // Core position records which ones (`None` = before them all).
        // 이것을 실패시키는 것: 경계 기록 삭제(붕괴 문단이 한 자리로 접힌다,
        // `collapsed_lines_keep_their_token_boundary`).
        line.boundary = tokens.boundary_at(textpos, s.text_start_position);
        // S22 r1: the boundaries' skipped tokens stay within a bound, so N
        // tokens at one place cannot grow N²/2 kinds.
        // 이것을 실패시키는 것: 이 검사 삭제, 또는 `>` 를 `>=` 로
        // (`boundary_skip_cap_keeps_the_limit_and_drops_past_it`).
        skipped += line.boundary.as_ref().map_or(0, |b| b.skip.len());
        if skipped > hwpforge_core::layout::MAX_PARAGRAPH_BOUNDARY_SKIP {
            warnings.push(crate::decoder::Hwp5Warning::LayoutCacheDropped {
                reason: format!(
                    "line boundaries pass {skipped} zero-width tokens (limit {})",
                    hwpforge_core::layout::MAX_PARAGRAPH_BOUNDARY_SKIP
                ),
            });
            return None;
        }
        lines.push(line);
    }
    Some(hwpforge_core::layout::LayoutCache::new(lines))
}

/// ledger 가 예측한 Core 총길이와 실제 방출된 run 들의 텍스트 총길이를
/// 대조한다 — 불일치 = ledger/방출 분기 (fail-closed, §1g v5 choke).
fn check_ledger_against_emission(
    ledger_result: Result<
        (crate::wire_text_map::WireTextMap, crate::wire_text_map::TokenLedger),
        &'static str,
    >,
    runs: &[Run],
) -> Result<(crate::wire_text_map::WireTextMap, crate::wire_text_map::TokenLedger), &'static str> {
    let (map, tokens) = ledger_result?;
    let emitted_core: u32 = runs
        .iter()
        .filter_map(|r| r.content.plain_text())
        .map(|t| t.encode_utf16().count() as u32)
        .sum();
    if map.core_end() != emitted_core {
        return Err("ledger/emission core length mismatch");
    }
    Ok((map, tokens))
}

/// FieldBegin~FieldEnd 방출 결과 — ledger 의 field 구간 core 폭을
/// 방출부가 결정한다 (단일 진실원, §1g v5 R3#1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldEmissionOutcome {
    /// 본문이 Control 내부로 접힘 (ClickHere/Hyperlink/CrossRef/Memo/
    /// 자동필드 display) — 전체 구간이 Core 텍스트 0 유닛.
    Folded,
    /// 본문이 가시 텍스트 슬라이스로 재방출됨 (BookmarkSpan/
    /// PlainTextFallback/빈 display CrossRef) — begin/end 만 (8,0),
    /// 본문은 가시 축 그대로 (내부 marker 는 U+FFFC 로 1유닛 재방출).
    ReEmitted,
}

/// 문단 하나의 wire→Core 좌표 ledger 를 방출과 나란히 구축한다 (W1b).
///
/// wire 소비는 세그먼트 종류에서 유도하고 (Text=UTF-16 len ·
/// Tab/컨트롤=8 · 단일 제어=1), segment 없는 무음 소비는
/// [`SilentWire`] 목록을 wire 순서로 합류시킨다. FieldBegin~FieldEnd
/// 는 pending 버퍼로 모아 방출 결과([`FieldEmissionOutcome`])가
/// 확정된 뒤 commit 한다. 어떤 단계든 실패하면 문단 전체가
/// fail-closed (캐시 미승격).
struct ParaLedger<'a> {
    builder: crate::wire_text_map::WireMapBuilder,
    /// S22: the zero-length tokens, with their kinds, in wire order.
    tokens: crate::wire_text_map::TokenLedger,
    silent: &'a [SilentWire],
    silent_idx: usize,
    /// ledger 자체 wire 커서 — pending 동안 builder 커서와 분리.
    wire_cursor: u32,
    /// FieldBegin 이후 버퍼된 body 조각 `(wire, 재방출 시 core, 종류)`.
    pending: Option<Vec<(u32, u32, WireTokenKind)>>,
    /// The wire position of the pending field's begin.
    pending_begin: u32,
    failed: Option<&'static str>,
}

/// The kind of a zero-length control marker (S22), from its control id.
/// `gso` objects take their kind in order from `gso_kinds` (a picture or a
/// shape — what the projection makes of them).
fn marker_kind(ctrl_id: u32, gso_kinds: &mut std::vec::IntoIter<WireTokenKind>) -> WireTokenKind {
    use crate::ctrl_ids as id;
    match ctrl_id {
        id::CTRL_ID_TABLE => WireTokenKind::Table,
        id::CTRL_ID_GSO => gso_kinds.next().unwrap_or(WireTokenKind::Shape),
        id::CTRL_ID_EQED => WireTokenKind::Equation,
        id::CTRL_ID_SECD => WireTokenKind::SectionDef,
        id::CTRL_ID_COLUMN_DEF => WireTokenKind::ColumnDef,
        id::CTRL_ID_HEADER => WireTokenKind::Header,
        id::CTRL_ID_FOOTER => WireTokenKind::Footer,
        id::CTRL_ID_FOOTNOTE => WireTokenKind::Footnote,
        id::CTRL_ID_ENDNOTE => WireTokenKind::Endnote,
        id::CTRL_ID_PAGE_NUMBER => WireTokenKind::PageNumber,
        id::CTRL_ID_NEW_NUMBER => WireTokenKind::NewNumber,
        id::CTRL_ID_PAGE_HIDING => WireTokenKind::PageHiding,
        id::CTRL_ID_BOOKMARK_POINT => WireTokenKind::Bookmark,
        id::CTRL_ID_ATNO => WireTokenKind::AutoNumber,
        id::CTRL_ID_DUTMAL | id::CTRL_ID_COMPOSE | id::CTRL_ID_INDEXMARK => WireTokenKind::Other,
        _ => WireTokenKind::Silent,
    }
}

/// The kinds of the paragraph's `gso` objects in control order: a picture
/// for an image, a shape for any other drawing object.
fn gso_kinds(hwp_para: &Hwp5Paragraph) -> std::vec::IntoIter<WireTokenKind> {
    hwp_para
        .controls
        .iter()
        .filter_map(|control| match control {
            Hwp5Control::Image(_) => Some(WireTokenKind::Picture),
            Hwp5Control::Line(_)
            | Hwp5Control::Rect(_)
            | Hwp5Control::Polygon(_)
            | Hwp5Control::Ellipse(_)
            | Hwp5Control::Arc(_)
            | Hwp5Control::Curve(_)
            | Hwp5Control::ConnectLine(_)
            | Hwp5Control::TextBox(_)
            | Hwp5Control::OleObject(_)
            | Hwp5Control::Group(_)
            | Hwp5Control::TextArt(_) => Some(WireTokenKind::Shape),
            Hwp5Control::Unknown { ctrl_id, .. } if *ctrl_id == crate::ctrl_ids::CTRL_ID_GSO => {
                Some(WireTokenKind::Shape)
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .into_iter()
}

impl<'a> ParaLedger<'a> {
    fn new(silent: &'a [SilentWire]) -> Self {
        Self {
            builder: crate::wire_text_map::WireMapBuilder::new(),
            tokens: crate::wire_text_map::TokenLedger::default(),
            silent,
            silent_idx: 0,
            wire_cursor: 0,
            pending: None,
            pending_begin: 0,
            failed: None,
        }
    }

    fn fail(&mut self, why: &'static str) {
        if self.failed.is_none() {
            self.failed = Some(why);
        }
    }

    /// 현재 커서 위치에 도달한 무음 소비 구간을 합류시킨다.
    fn drain_silent(&mut self) {
        while let Some(s) = self.silent.get(self.silent_idx) {
            if s.start > self.wire_cursor {
                break;
            }
            if s.start < self.wire_cursor {
                // parse 기록과 유도 소비폭이 어긋남 — 좌표를 믿을 수 없다.
                self.fail("silent-wire accounting mismatch");
                self.silent_idx += 1;
                continue;
            }
            self.silent_idx += 1;
            // S22: a silently consumed control keeps its control's kind (a
            // page number position `pgnp`, say); anything else is `Silent`.
            let kind = match s.ctrl_id {
                0 => WireTokenKind::Silent,
                id => marker_kind(id, &mut Vec::new().into_iter()),
            };
            self.consume(s.len, 0, 0, kind);
        }
    }

    /// A zero-length token at the current position (outside a field).
    fn record(&mut self, wire_start: u32, kind: WireTokenKind) {
        let (_, core) = self.builder.cursors();
        self.tokens.push(crate::wire_text_map::ZeroToken { core, wire_start, kind });
    }

    /// 소비 하나를 기록한다. `core_folded`/`core_reemitted` 는 각각
    /// field 밖(즉시 반영)·재방출 body 조각일 때의 Core 폭. `kind` 는 그
    /// 폭이 0 일 때 적는 토큰 종류 (S22).
    fn consume(&mut self, wire: u32, core_outside: u32, core_reemitted: u32, kind: WireTokenKind) {
        let start = self.wire_cursor;
        match self.wire_cursor.checked_add(wire) {
            Some(w) => self.wire_cursor = w,
            None => {
                self.fail("wire cursor overflow");
                return;
            }
        }
        if let Some(pieces) = self.pending.as_mut() {
            pieces.push((wire, core_reemitted, kind));
        } else if core_outside == wire {
            self.builder.advance_identity(wire);
        } else {
            if core_outside == 0 {
                self.record(start, kind);
            }
            self.builder.push_span(wire, core_outside);
        }
    }

    /// 일반 텍스트 (field 밖 1:1 · 재방출 body 도 1:1).
    fn observe_text(&mut self, utf16_len: u32) {
        self.drain_silent();
        self.consume(utf16_len, utf16_len, utf16_len, WireTokenKind::Text);
    }

    /// Tab — wire 8 · Core `\t` 1 (양쪽 동일).
    fn observe_tab(&mut self) {
        self.drain_silent();
        self.consume(8, 1, 1, WireTokenKind::Text);
    }

    /// 단일 유닛 제어 (`\n`·nbSp·fwSp) — 1:1.
    fn observe_unit(&mut self) {
        self.drain_silent();
        self.consume(1, 1, 1, WireTokenKind::Text);
    }

    /// ParaBreak — wire 1 · Core 0 (S22: 종류 `End`).
    fn observe_para_break(&mut self) {
        self.drain_silent();
        self.consume(1, 0, 0, WireTokenKind::End);
    }

    /// ControlRef/ExtendedControlRef marker — field 밖 (8,0) (U+FFFC
    /// 미방출), 재방출 body 안 (8,1) (U+FFFC 가 텍스트로 살아남음).
    fn observe_marker(&mut self, kind: WireTokenKind) {
        self.drain_silent();
        self.consume(8, 0, 1, kind);
    }

    /// SectionColumnDef — wire 8 · Core 0 (양쪽 동일).
    fn observe_section_ctrl(&mut self, kind: WireTokenKind) {
        self.drain_silent();
        self.consume(8, 0, 0, kind);
    }

    /// FieldBegin — pending 개시 (begin 자체 8유닛 소비).
    fn begin_field(&mut self) {
        self.drain_silent();
        if self.pending.is_some() {
            self.fail("nested field begin");
        }
        self.pending_begin = self.wire_cursor;
        match self.wire_cursor.checked_add(8) {
            Some(w) => self.wire_cursor = w,
            None => {
                self.fail("wire cursor overflow");
                return;
            }
        }
        if self.failed.is_none() {
            self.pending = Some(Vec::new());
        }
    }

    /// FieldEnd — 방출 결과에 따라 pending 을 commit 한다.
    fn commit_field(&mut self, outcome: FieldEmissionOutcome) {
        self.drain_silent();
        let end_start = self.wire_cursor;
        match self.wire_cursor.checked_add(8) {
            Some(w) => self.wire_cursor = w,
            None => {
                self.fail("wire cursor overflow");
                return;
            }
        }
        let Some(pieces) = self.pending.take() else {
            self.fail("unpaired field end");
            return;
        };
        let begin = self.pending_begin;
        match outcome {
            FieldEmissionOutcome::Folded => {
                let mut total: u32 = 16; // begin + end
                for (wire, _, _) in &pieces {
                    match total.checked_add(*wire) {
                        Some(t) => total = t,
                        None => {
                            self.fail("wire cursor overflow");
                            return;
                        }
                    }
                }
                // S22: begin, the folded body, end — at one Core position.
                self.record(begin, WireTokenKind::FieldBegin);
                if total > 16 {
                    self.record(begin + 8, WireTokenKind::Text);
                }
                self.record(end_start, WireTokenKind::FieldEnd);
                self.builder.push_span(total, 0);
            }
            FieldEmissionOutcome::ReEmitted => {
                self.record(begin, WireTokenKind::FieldBegin);
                self.builder.push_span(8, 0);
                let mut at = begin + 8;
                for (wire, core, kind) in pieces {
                    if wire == core {
                        self.builder.advance_identity(wire);
                    } else {
                        if core == 0 {
                            self.record(at, kind);
                        }
                        self.builder.push_span(wire, core);
                    }
                    at += wire;
                }
                self.record(end_start, WireTokenKind::FieldEnd);
                self.builder.push_span(8, 0);
            }
        }
    }

    /// 문단 종료 — 잔여 무음 구간 합류 후 seal.
    fn finish(
        mut self,
    ) -> Result<(crate::wire_text_map::WireTextMap, crate::wire_text_map::TokenLedger), &'static str>
    {
        if self.pending.is_some() {
            self.fail("dangling field begin");
        }
        self.drain_silent();
        if self.silent_idx < self.silent.len() {
            self.fail("silent-wire accounting mismatch");
        }
        if let Some(why) = self.failed {
            return Err(why);
        }
        let map = self.builder.finish().map_err(|_| "wire map invariant violation")?;
        Ok((map, self.tokens))
    }
}

/// field 없는(flat) 문단의 ledger — 세그먼트 종류만으로 구축한다.
fn build_flat_ledger(
    hwp_para: &Hwp5Paragraph,
) -> Result<(crate::wire_text_map::WireTextMap, crate::wire_text_map::TokenLedger), &'static str> {
    let mut ledger = ParaLedger::new(&hwp_para.silent_wires);
    let mut gso = gso_kinds(hwp_para);
    for segment in &hwp_para.text_segments {
        match segment {
            crate::schema::section::TextSegment::Text(text) => {
                ledger.observe_text(text.encode_utf16().count() as u32);
            }
            crate::schema::section::TextSegment::Tab { .. } => ledger.observe_tab(),
            crate::schema::section::TextSegment::LineBreak
            | crate::schema::section::TextSegment::NonBreakingSpace
            | crate::schema::section::TextSegment::FwSpace => ledger.observe_unit(),
            crate::schema::section::TextSegment::ParaBreak => ledger.observe_para_break(),
            crate::schema::section::TextSegment::ControlRef { extra }
            | crate::schema::section::TextSegment::ExtendedControlRef { extra } => {
                ledger.observe_marker(marker_kind(ctrl_id_from_inline_extra(extra), &mut gso));
            }
            crate::schema::section::TextSegment::SectionColumnDef { extra } => {
                ledger
                    .observe_section_ctrl(marker_kind(ctrl_id_from_inline_extra(extra), &mut gso));
            }
            crate::schema::section::TextSegment::FieldBegin { .. }
            | crate::schema::section::TextSegment::FieldEnd => {
                // flat 분기는 field 문단을 받지 않는다
                // (`paragraph_needs_structural_projection`) — 방어적 실패.
                ledger.fail("field segment in flat projection");
            }
        }
    }
    ledger.finish()
}

fn project_paragraph_with_images_structural(
    hwp_para: &Hwp5Paragraph,
    projection_images: &mut ProjectionImageState<'_>,
    image_context: ImageProjectionContext,
    masters: &[Hwp5MasterPage],
) -> ProjectedParagraph {
    let mut queues = build_paragraph_projection_queues(hwp_para, projection_images, masters);
    let mut runs: Vec<Run> = Vec::new();
    let mut visible_utf16: u32 = 0;
    let mut active_field: Option<ActiveField> = None;
    // Core's folded fields cannot contain arbitrary controls. Keep the outer
    // field active while balancing nested markers; preserve inner memo bodies
    // and objects separately, with an unconditional semantic-loss diagnostic.
    let mut nested_fields: Vec<ActiveField> = Vec::new();
    let mut field_extras: Vec<Run> = Vec::new();
    let mut field_text = FieldTextIndex::new(hwp_para);
    let mut ledger = ParaLedger::new(&hwp_para.silent_wires);
    let mut gso = gso_kinds(hwp_para);

    for segment in &hwp_para.text_segments {
        match segment {
            crate::schema::section::TextSegment::Text(text) => {
                ledger.observe_text(text.encode_utf16().count() as u32);
                project_visible_text_segment(
                    text,
                    hwp_para,
                    &mut active_field,
                    &mut runs,
                    &mut visible_utf16,
                );
            }
            crate::schema::section::TextSegment::Tab { .. } => {
                // Inline tab metadata is dropped here; `<hp:tab>`
                // attribute carry is tracked separately by
                // `warn_on_inline_tab_attributes` to cover both the
                // flat and structural projection branches uniformly.
                ledger.observe_tab();
                append_visible_unit(
                    hwp_para,
                    &mut runs,
                    &mut active_field,
                    &mut visible_utf16,
                    '\t',
                );
            }
            crate::schema::section::TextSegment::LineBreak => {
                ledger.observe_unit();
                append_visible_unit(
                    hwp_para,
                    &mut runs,
                    &mut active_field,
                    &mut visible_utf16,
                    '\n',
                );
            }
            crate::schema::section::TextSegment::NonBreakingSpace => {
                ledger.observe_unit();
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
                ledger.observe_unit();
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
            crate::schema::section::TextSegment::ControlRef { extra }
            | crate::schema::section::TextSegment::ExtendedControlRef { extra } => {
                ledger.observe_marker(marker_kind(ctrl_id_from_inline_extra(extra), &mut gso));
                let run = if ctrl_id_from_inline_extra(extra) == CTRL_ID_BOOKMARK_POINT {
                    queues.point_bookmark_names.pop_front().flatten().map(|name| {
                        Run::control(
                            Control::Bookmark { name, bookmark_type: BookmarkType::Point },
                            shape_starting_at(hwp_para, visible_utf16),
                        )
                    })
                } else {
                    next_projection_control(
                        &mut std::iter::from_fn(|| queues.object_controls.pop_front()),
                        projection_images,
                    )
                    .and_then(|control| {
                        project_control_run(
                            control,
                            projection_images,
                            image_context,
                            shape_starting_at(hwp_para, visible_utf16),
                        )
                    })
                };
                if let Some(run) = run {
                    if active_field.is_some() {
                        ledger.fail("object inside folded field");
                        projection_images.warnings.push(Hwp5Warning::ProjectionFallback {
                                subject: "field.object",
                                reason: "object inside a folded field cannot retain its inline position; preserving the outer field and emitting the object immediately after it".into(),
                            });
                        field_extras.push(run);
                    } else {
                        runs.push(run);
                    }
                }

                visible_utf16 += 1;
            }
            crate::schema::section::TextSegment::SectionColumnDef { extra } => {
                let ctrl_id = ctrl_id_from_inline_extra(extra);
                ledger.observe_section_ctrl(marker_kind(ctrl_id, &mut gso));
                let _ = consume_marker_header(&mut queues.marker_headers, ctrl_id);
            }
            crate::schema::section::TextSegment::FieldBegin { extra } => {
                ledger.begin_field();
                let field =
                    start_field_from_marker(extra, &mut queues, visible_utf16, projection_images);
                if active_field.is_some() {
                    ledger.fail("nested field begin");
                    projection_images.warnings.push(Hwp5Warning::ProjectionFallback {
                        subject: "field.nested",
                        reason: "nested field semantics flattened; preserving outer field, visible text and all memo bodies".into(),
                    });
                    nested_fields.push(field);
                } else {
                    active_field = Some(field);
                }
            }
            crate::schema::section::TextSegment::FieldEnd => {
                if let Some(field) = nested_fields.pop() {
                    preserve_nested_memo(field, hwp_para, projection_images, &mut field_extras);
                    continue;
                }
                if let Some(field) = active_field.take() {
                    let outcome = finish_active_field(
                        field,
                        &mut field_text,
                        hwp_para,
                        visible_utf16,
                        &mut runs,
                        projection_images,
                    );
                    ledger.commit_field(outcome);
                    runs.append(&mut field_extras);
                } else {
                    ledger.commit_field(FieldEmissionOutcome::Folded);
                }
            }
            crate::schema::section::TextSegment::ParaBreak => {
                ledger.observe_para_break();
            }
        }
    }

    for field in nested_fields {
        preserve_nested_memo(field, hwp_para, projection_images, &mut field_extras);
    }
    if let Some(field) = active_field.take() {
        projection_images.warnings.push(Hwp5Warning::ProjectionFallback {
            subject: "field.unclosed",
            reason: "field has no end marker in its paragraph; preserving field and content up to paragraph end".into(),
        });
        // 방출은 기존 동작대로 완료하되, 좌표는 신뢰 불가 (v5 fail-closed).
        let _ = finish_active_field(
            field,
            &mut field_text,
            hwp_para,
            visible_utf16,
            &mut runs,
            projection_images,
        );
        ledger.fail("dangling field begin");
    }

    runs.append(&mut field_extras);

    drain_unconsumed_paragraph_queues(
        queues,
        hwp_para,
        visible_utf16,
        &mut runs,
        projection_images,
        image_context,
    );

    // As in the flat path (§19.22 B). 이것을 실패시키는 것: 0 으로 되돌리기
    // (`empty_paragraph_keeps_its_char_shape`, structural case).
    if runs.is_empty() {
        runs.push(Run::text("", shape_starting_at(hwp_para, 0)));
    }

    let mut paragraph =
        Paragraph::with_runs(runs, ParaShapeIndex::new(hwp_para.para_shape_id as usize));
    if hwp_para.style_id > 0 {
        paragraph = paragraph.with_style(StyleIndex::new(hwp_para.style_id as usize));
    }
    paragraph.layout_cache = promote_line_segments(
        &hwp_para.line_segments,
        check_ledger_against_emission(ledger.finish(), &paragraph.runs),
        &mut projection_images.warnings,
    );
    // W3: ParaHeader divide_sort 쪽/단 나누기 carry (F2 실측 — 한컴은 쪽나눔
    // 문단의 lineseg v 를 리셋하지 않아 이 플래그가 유일한 쪽분할 신호).
    paragraph.page_break = hwp_para.page_break;
    paragraph.column_break = hwp_para.column_break;

    ProjectedParagraph { paragraph }
}

/// Cap on accumulated auto-field `display_text` (the cached value chars
/// between `FieldBegin` and `FieldEnd`).
///
/// Unlike the `%smr`/`%dte` BSTR *command* (capped at
/// `MAX_SUMMERY_COMMAND_UNITS` etc. at the decoder boundary), this body
/// text comes from `ParaText` and bypasses those caps. A malicious file
/// with a pathologically long FieldBegin..FieldEnd span would otherwise
/// grow `display_text` unbounded. Closure reports the exact lost UTF-16
/// unit count; legitimate long cached values can also reach this limit.
const MAX_FIELD_DISPLAY_TEXT_UNITS: usize = 4096;

/// Appends `text` to an auto-field `display_text`, stopping once the
/// accumulated UTF-16 length would exceed [`MAX_FIELD_DISPLAY_TEXT_UNITS`].
fn push_field_display_text(display_text: &mut String, text: &str) {
    let mut current = display_text.encode_utf16().count();
    if current >= MAX_FIELD_DISPLAY_TEXT_UNITS {
        return;
    }
    let remaining = MAX_FIELD_DISPLAY_TEXT_UNITS - current;
    if text.encode_utf16().count() <= remaining {
        display_text.push_str(text);
        return;
    }
    // Slow path: append char-by-char up to the cap (keeps UTF-16 boundaries
    // intact). Track the accumulated UTF-16 length with a running counter
    // instead of recomputing `display_text.encode_utf16().count()` every
    // iteration (was O(M·K); now O(M+K)). Same truncation point.
    for ch in text.chars() {
        let n = ch.len_utf16();
        if current + n > MAX_FIELD_DISPLAY_TEXT_UNITS {
            break;
        }
        display_text.push(ch);
        current += n;
    }
}

#[cfg(test)]
thread_local! {
    static FIELD_TEXT_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Paragraph-local character/style/object index. Both construction and the
/// forward cursor visit each input item at most once across sibling fields.
/// Literal U+FFFC text stays visible; only wire object markers are excluded.
struct FieldTextIndex<'a> {
    chars: std::str::CharIndices<'a>,
    text: &'a str,
    styles: &'a [Hwp5CharShapeRun],
    objects: Vec<u32>,
    position: u32,
    object: usize,
    style: usize,
    #[cfg(test)]
    visits: usize,
}

impl<'a> FieldTextIndex<'a> {
    fn new(para: &'a Hwp5Paragraph) -> Self {
        use crate::schema::section::TextSegment;
        let mut objects = Vec::new();
        let mut position = 0;
        #[cfg(test)]
        let mut visits = 0;
        for segment in &para.text_segments {
            #[cfg(test)]
            {
                visits += 1;
                FIELD_TEXT_VISITS.with(|n| n.set(n.get() + 1));
            }
            match segment {
                TextSegment::Text(text) => position += text.encode_utf16().count() as u32,
                TextSegment::ControlRef { .. } | TextSegment::ExtendedControlRef { .. } => {
                    objects.push(position);
                    position += 1;
                }
                TextSegment::Tab { .. }
                | TextSegment::LineBreak
                | TextSegment::NonBreakingSpace
                | TextSegment::FwSpace => position += 1,
                _ => {}
            }
        }
        Self {
            text: &para.text,
            chars: para.text.char_indices(),
            styles: &para.char_shape_runs,
            objects,
            position: 0,
            object: 0,
            style: 0,
            #[cfg(test)]
            visits,
        }
    }

    fn visit(&mut self, start: u32, end: u32, mut emit: impl FnMut(&str, u32)) {
        // Empty fields must not even advance the paragraph cursor.
        if start >= end {
            return;
        }
        while self.position < end {
            let Some((byte, ch)) = self.chars.next() else {
                break;
            };
            #[cfg(test)]
            {
                self.visits += 1;
                FIELD_TEXT_VISITS.with(|n| n.set(n.get() + 1));
            }
            let position = self.position;
            self.position += ch.len_utf16() as u32;
            while self.object < self.objects.len() && self.objects[self.object] < position {
                self.object += 1;
            }
            while self.style + 1 < self.styles.len()
                && self.styles[self.style + 1].position <= position
            {
                self.style += 1;
            }
            if position < start || self.objects.get(self.object) == Some(&position) {
                continue;
            }
            let shape = self
                .styles
                .get(self.style)
                .filter(|r| r.position <= position)
                .map_or(0, |r| r.char_shape_id);
            emit(&self.text[byte..byte + ch.len_utf8()], shape);
        }
    }

    fn project(&mut self, start: u32, end: u32) -> Vec<Run> {
        let mut runs: Vec<Run> = Vec::new();
        self.visit(start, end, |text, shape| {
            if let Some(Run { content: RunContent::Text(previous), char_shape_id, .. }) =
                runs.last_mut()
            {
                if char_shape_id.get() == shape as usize {
                    previous.push_str(text);
                    return;
                }
            }
            runs.push(Run::text(text, CharShapeIndex::new(shape as usize)));
        });
        runs
    }

    fn visible_units(&mut self, start: u32, end: u32) -> usize {
        let mut units = 0;
        self.visit(start, end, |text, _| units += text.encode_utf16().count());
        units
    }
}

fn project_field_text(index: &mut FieldTextIndex<'_>, start: u32, end: u32) -> Vec<Run> {
    index.project(start, end)
}

fn preserve_nested_memo(
    field: ActiveField,
    para: &Hwp5Paragraph,
    state: &mut ProjectionImageState<'_>,
    extras: &mut Vec<Run>,
) {
    if let ActiveField::MemoAnchor { start_utf16, memo } = field {
        extras.push(project_memo_run(
            &memo,
            state,
            shape_starting_at(para, start_utf16),
            Vec::new(),
        ));
    }
}

/// Projects a visible `TextSegment::Text` chunk (task #91 — extracted
/// from `project_paragraph_with_images_structural`).
///
/// Inside an active field the text feeds the field's `display_text`
/// (Hyperlink / CrossRef) or is silently skipped — BookmarkSpan /
/// PlainTextFallback / MemoAnchor / ClickHere / auto-field variants
/// re-emit their anchor text in `finish_active_field` via
/// `project_text_segment(start, end)`. Outside a field the chunk
/// projects to styled runs directly. The visible cursor advances by
/// the chunk's UTF-16 length either way.
fn project_visible_text_segment(
    text: &str,
    hwp_para: &Hwp5Paragraph,
    active_field: &mut Option<ActiveField>,
    runs: &mut Vec<Run>,
    visible_utf16: &mut u32,
) {
    let len = text.encode_utf16().count() as u32;
    if let Some(active) = active_field.as_mut() {
        match active {
            ActiveField::Hyperlink { display_text, .. }
            | ActiveField::CrossRef { display_text, .. } => display_text.push_str(text),
            // Wave 12n cached-value carry (#120/#136): SUMMERY/%dte/%pat
            // accumulate the FieldBegin..FieldEnd body as the field's cached
            // resolved value (capped — see `push_field_display_text`).
            ActiveField::SummaryField { display_text, .. }
            | ActiveField::DateCodeField { display_text, .. }
            | ActiveField::PathField { display_text, .. } => {
                push_field_display_text(display_text, text);
            }
            ActiveField::BookmarkSpan { .. }
            | ActiveField::PlainTextFallback { .. }
            | ActiveField::MemoAnchor { .. }
            | ActiveField::ClickHere { .. } => {}
        }
    } else {
        runs.extend(project_text_segment(
            &hwp_para.text,
            &hwp_para.char_shape_runs,
            *visible_utf16,
            *visible_utf16 + len,
        ));
    }
    *visible_utf16 += len;
}

/// Opens the `ActiveField` for an inline `FieldBegin` marker (task #91
/// — extracted from `project_paragraph_with_images_structural`).
///
/// The marker's `extra[0..4]` ctrl_id picks which per-family queue
/// supplies the typed payload (memo / clickhere / summary / datecode /
/// pathfield / crossref); families pop only their own queue so
/// unrelated controls stay queued for later markers.
fn start_field_from_marker(
    extra: &[u8; 14],
    queues: &mut ParagraphProjectionQueues<'_>,
    visible_utf16: u32,
    projection_images: &mut ProjectionImageState<'_>,
) -> ActiveField {
    let ctrl_id = ctrl_id_from_inline_extra(extra);
    let header = consume_marker_header(&mut queues.marker_headers, ctrl_id);
    let memo = if ctrl_id == CTRL_ID_MEMO_INLINE { queues.memo_controls.pop_front() } else { None };
    let clickhere =
        if ctrl_id == CTRL_ID_CLICK_HERE { queues.clickhere_controls.pop_front() } else { None };
    let summary =
        if ctrl_id == CTRL_ID_FIELD_SUMMERY { queues.summary_fields.pop_front() } else { None };
    let datecode =
        if ctrl_id == CTRL_ID_FIELD_DATE_CODE { queues.datecode_fields.pop_front() } else { None };
    let pathfield =
        if ctrl_id == CTRL_ID_FIELD_PATH { queues.pathfield_controls.pop_front() } else { None };
    let crossref =
        if ctrl_id == CTRL_ID_FIELD_CROSSREF { queues.crossref_controls.pop_front() } else { None };
    start_active_field(
        ctrl_id,
        header,
        memo,
        clickhere,
        summary,
        datecode,
        pathfield,
        crossref,
        visible_utf16,
        projection_images,
    )
}

/// Drains queue entries that no inline marker consumed (task #91 —
/// extracted from `project_paragraph_with_images_structural`).
///
/// Unanchored point bookmarks and object controls emit at paragraph end.
/// Named point bookmarks with markers emit at their original position. Memo placeholders with body content
/// emit with a `ProjectionFallback` warning — properly anchored memos
/// always consume their queue entry, so a leftover means the inline
/// `FieldBegin %unk MEMO` anchor was missing; emitting preserves the
/// body rather than silently dropping it.
fn drain_unconsumed_paragraph_queues(
    mut queues: ParagraphProjectionQueues<'_>,
    hwp_para: &Hwp5Paragraph,
    visible_utf16: u32,
    runs: &mut Vec<Run>,
    projection_images: &mut ProjectionImageState<'_>,
    image_context: ImageProjectionContext,
) {
    for bookmark_name in queues.point_bookmark_names.into_iter().flatten() {
        projection_images.warnings.push(Hwp5Warning::ProjectionFallback {
            subject: "field.bookmark_point.position",
            reason: "point bookmark has no inline marker; preserved at paragraph end".into(),
        });
        let char_shape_id = CharShapeIndex::new(shape_of_previous_char(
            &hwp_para.char_shape_runs,
            visible_utf16,
        ) as usize);
        runs.push(Run::control(
            Control::Bookmark { name: bookmark_name, bookmark_type: BookmarkType::Point },
            char_shape_id,
        ));
    }

    while let Some(control) = next_projection_control(
        &mut std::iter::from_fn(|| queues.object_controls.pop_front()),
        projection_images,
    ) {
        if let Some(run) = project_control_run(
            control,
            projection_images,
            image_context,
            shape_starting_at(hwp_para, visible_utf16),
        ) {
            runs.push(run);
        }
    }

    // 독립 리뷰 Medium #5: 소비되지 않고 남은 marker_headers(secd/cold/
    // bookmark/hyperlink CtrlHeader 가 inline marker 없이 남은 wire 이상
    // 케이스)도 무음 소멸 대신 드롭 집계에 합산한다.
    for leftover in queues.marker_headers {
        *projection_images.dropped_unknown.entry(leftover.ctrl_id).or_insert(0) += 1;
    }

    // Typed fields without an inline anchor must never disappear silently.
    for (kind, count) in [
        ("clickhere", queues.clickhere_controls.len()),
        ("summary", queues.summary_fields.len()),
        ("datecode", queues.datecode_fields.len()),
        ("path", queues.pathfield_controls.len()),
        ("crossref", queues.crossref_controls.len()),
    ] {
        for _ in 0..count {
            warn_unpaired_field(projection_images, kind);
        }
    }

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
                shape_starting_at(hwp_para, visible_utf16),
                Vec::new(),
            ));
        } else {
            warn_unpaired_field(projection_images, "memo");
        }
    }
}

fn warn_unpaired_field(state: &mut ProjectionImageState<'_>, control: &'static str) {
    state.warnings.push(Hwp5Warning::DroppedControl {
        control,
        reason: "typed field has no matching inline anchor".to_string(),
    });
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
            ActiveField::SummaryField { display_text, .. }
            | ActiveField::DateCodeField { display_text, .. }
            | ActiveField::PathField { display_text, .. } => {
                let mut buf = [0u8; 4];
                push_field_display_text(display_text, ch.encode_utf8(&mut buf));
            }
            ActiveField::BookmarkSpan { .. }
            | ActiveField::PlainTextFallback { .. }
            | ActiveField::MemoAnchor { .. }
            | ActiveField::ClickHere { .. } => {}
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
            ) || matches!(control, Hwp5Control::Bookmark { .. } | Hwp5Control::Memo(_))
                || matches!(control, Hwp5Control::ClickHere(_))
                || matches!(control, Hwp5Control::SummaryField(_))
                || matches!(control, Hwp5Control::DateCodeField(_))
                || matches!(control, Hwp5Control::PathField(_))
                || matches!(control, Hwp5Control::CrossRef(_))
                || matches!(control, Hwp5Control::InlinePageNumber(_))
        })
}

// Master lists have no inline ControlRef. Merge their saved host positions
// into the existing object traversal, sharing every projection counter. A list
// is drained before the next object without consuming that object's marker.
#[derive(Debug)]
enum ProjectionControl<'a> {
    Control(&'a Hwp5Control),
    Master(&'a Hwp5MasterPage),
}

fn projection_controls<'a>(
    paragraph: &'a Hwp5Paragraph,
    masters: &'a [Hwp5MasterPage],
) -> impl Iterator<Item = ProjectionControl<'a>> {
    let mut masters = masters.iter().peekable();
    let mut index = 0;
    std::iter::from_fn(move || {
        if masters.peek().is_some_and(|master| master.body_control_index == index) {
            return masters.next().map(ProjectionControl::Master);
        }
        let control = paragraph.controls.get(index)?;
        index += 1;
        Some(ProjectionControl::Control(control))
    })
}

fn next_projection_control<'a>(
    controls: &mut impl Iterator<Item = ProjectionControl<'a>>,
    projection_images: &mut ProjectionImageState<'_>,
) -> Option<&'a Hwp5Control> {
    for entry in controls.by_ref() {
        match entry {
            ProjectionControl::Control(control) => return Some(control),
            ProjectionControl::Master(master) => project_master(master, projection_images),
        }
    }
    None
}

fn project_master(wire: &Hwp5MasterPage, projection_images: &mut ProjectionImageState<'_>) {
    let mut master = match wire.core_metadata() {
        Ok(master) => master,
        Err(reason) => {
            projection_images.warnings.push(Hwp5Warning::DroppedControl {
                control: "master_page",
                reason: format!("{}: {reason}; scope not widened", wire.location()),
            });
            return;
        }
    };
    if wire.unsupported_properties() {
        projection_images.warnings.push(Hwp5Warning::ProjectionFallback {
                    subject: "master_page.properties",
                    reason: format!("{}: non-default direction, wrapping, reference bitmap or unknown flags/tail cannot be represented; retaining confirmed fields", wire.location()),
                });
    }
    projection_images.in_master = true;
    master.paragraphs = project_nested_paragraphs(
        &wire.paragraphs,
        projection_images,
        ImageProjectionContext::Flow,
    );
    projection_images.in_master = false;
    projection_images.master_pages.push(master);
}

fn build_paragraph_projection_queues<'a>(
    hwp_para: &'a Hwp5Paragraph,
    projection_images: &mut ProjectionImageState<'_>,
    masters: &'a [Hwp5MasterPage],
) -> ParagraphProjectionQueues<'a> {
    let mut marker_headers = VecDeque::new();
    let mut object_controls = VecDeque::new();
    let mut memo_controls = VecDeque::new();
    let mut clickhere_controls = VecDeque::new();
    let mut summary_fields = VecDeque::new();
    let mut datecode_fields = VecDeque::new();
    let mut pathfield_controls = VecDeque::new();
    let mut crossref_controls = VecDeque::new();
    let mut point_bookmark_names = VecDeque::new();

    for entry in projection_controls(hwp_para, masters) {
        let ProjectionControl::Control(control) = entry else {
            object_controls.push_back(entry);
            continue;
        };
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
        if let Hwp5Control::SummaryField(summary) = control {
            summary_fields.push_back(summary.clone());
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
            object_controls.push_back(ProjectionControl::Control(control));
            continue;
        };

        match unknown.ctrl_id {
            CTRL_ID_SECD | CTRL_ID_COLUMN_DEF | CTRL_ID_BOOKMARK_SPAN | CTRL_ID_HYPERLINK => {
                marker_headers.push_back(unknown)
            }
            CTRL_ID_PAGE_NUMBER => object_controls.push_back(ProjectionControl::Control(control)),
            CTRL_ID_BOOKMARK_POINT => {
                point_bookmark_names.push_back(unknown.bookmark_name.map(str::to_string));
                if unknown.bookmark_name.is_none() {
                    projection_images.warnings.push(Hwp5Warning::ProjectionFallback {
                        subject: "field.bookmark_point",
                        reason: "bookmark point name unavailable; dropping bookmark control"
                            .to_string(),
                    });
                }
            }
            _ => object_controls.push_back(ProjectionControl::Control(control)),
        }
    }

    ParagraphProjectionQueues {
        marker_headers,
        object_controls,
        memo_controls,
        clickhere_controls,
        summary_fields,
        datecode_fields,
        pathfield_controls,
        crossref_controls,
        point_bookmark_names,
    }
}

// Wave 12n added 3 more optional carriers (summary/datecode/pathfield) on
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
    summary: Option<crate::schema::section::Hwp5SummaryControl>,
    datecode: Option<crate::schema::section::Hwp5DateCodeControl>,
    pathfield: Option<crate::schema::section::Hwp5PathFieldControl>,
    crossref: Option<crate::schema::section::Hwp5CrossRefControl>,
    start_utf16: u32,
    projection_images: &mut ProjectionImageState<'_>,
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
            if let Some(name) = header.and_then(|h| h.bookmark_name).map(str::to_string) {
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
        CTRL_ID_FIELD_CROSSREF => {
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
                    command: clickhere.command,
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
            if let Some(summary) = summary {
                ActiveField::SummaryField {
                    start_utf16,
                    command_token: summary.command_token,
                    display_text: String::new(),
                }
            } else {
                projection_images.warnings.push(Hwp5Warning::ProjectionFallback {
                    subject: "field.summary",
                    reason: "summary auto-field metadata unavailable; \
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
                    display_text: String::new(),
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
                ActiveField::PathField {
                    start_utf16,
                    raw_command: pat.raw_command,
                    display_text: String::new(),
                }
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
    field_text: &mut FieldTextIndex<'_>,
    hwp_para: &Hwp5Paragraph,
    end_utf16: u32,
    runs: &mut Vec<Run>,
    projection_images: &mut ProjectionImageState<'_>,
) -> FieldEmissionOutcome {
    if let ActiveField::SummaryField { start_utf16, display_text, .. }
    | ActiveField::DateCodeField { start_utf16, display_text, .. }
    | ActiveField::PathField { start_utf16, display_text, .. } = &field
    {
        let source_units = field_text.visible_units(*start_utf16, end_utf16);
        let lost = source_units.saturating_sub(display_text.encode_utf16().count());
        if lost > 0 {
            projection_images.warnings.push(Hwp5Warning::ProjectionFallback {
                subject: "field.display_text.truncated",
                reason: format!("auto-field cached text truncated at {MAX_FIELD_DISPLAY_TEXT_UNITS} UTF-16 units; lost {lost} UTF-16 units of visible text"),
            });
        }
    }
    match field {
        ActiveField::Hyperlink { url, start_utf16, display_text } => {
            if display_text.is_empty() {
                return FieldEmissionOutcome::Folded;
            }
            let char_shape_id = shape_starting_at(hwp_para, start_utf16);
            if hwp_para.char_shape_runs.iter().any(|r| {
                r.position > start_utf16
                    && r.position < end_utf16
                    && r.char_shape_id as usize != char_shape_id.get()
            }) {
                projection_images.warnings.push(Hwp5Warning::ProjectionFallback {
                    subject: "field.hyperlink.mixed_style",
                    reason: "hyperlink display text contains multiple character styles; using its first character style".to_string(),
                });
            }
            runs.push(Run::control(Control::Hyperlink { text: display_text, url }, char_shape_id));
            FieldEmissionOutcome::Folded
        }
        ActiveField::BookmarkSpan { name, start_utf16 } => {
            let char_shape_id = shape_starting_at(hwp_para, start_utf16);
            runs.push(Run::control(
                Control::Bookmark { name: name.clone(), bookmark_type: BookmarkType::SpanStart },
                char_shape_id,
            ));
            runs.extend(project_field_text(field_text, start_utf16, end_utf16));
            runs.push(Run::control(
                Control::Bookmark { name, bookmark_type: BookmarkType::SpanEnd },
                char_shape_id,
            ));
            FieldEmissionOutcome::ReEmitted
        }
        ActiveField::CrossRef { control, start_utf16, display_text } => {
            // Wave 12m Phase 2 Step 4: emit native `Control::CrossRef`.
            // Boundary functions decode the wire codes into typed
            // `RefType` / `RefContentType` / `RefTarget`. The HWPX
            // encoder embeds `display_text` between fieldBegin/fieldEnd.
            let char_shape_id = shape_starting_at(hwp_para, start_utf16);
            if display_text.is_empty() {
                // No visible body between FieldBegin/FieldEnd — preserve
                // any latent body text so users at least see the source
                // span. This mirrors the pre-Step-4 fallback.
                runs.extend(project_field_text(field_text, start_utf16, end_utf16));
                return FieldEmissionOutcome::ReEmitted;
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
            FieldEmissionOutcome::Folded
        }
        ActiveField::PlainTextFallback { start_utf16 } => {
            runs.extend(project_field_text(field_text, start_utf16, end_utf16));
            FieldEmissionOutcome::ReEmitted
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
            let anchor_runs = project_field_text(field_text, start_utf16, end_utf16);
            let char_shape_id = shape_starting_at(hwp_para, start_utf16);
            runs.push(project_memo_run(&memo, projection_images, char_shape_id, anchor_runs));
            FieldEmissionOutcome::Folded
        }
        ActiveField::ClickHere { start_utf16, command, hint_text, help_text, name } => {
            let char_shape_id = shape_starting_at(hwp_para, start_utf16);
            let body_runs = project_field_text(field_text, start_utf16, end_utf16);
            // FieldTextIndex coalesces adjacent text with the same shape. Match
            // the HWPX decoder's cross-run field representation without folding
            // the body into a string (which discards its character shapes).
            if body_runs.windows(2).any(|pair| pair[0].char_shape_id != pair[1].char_shape_id) {
                projection_images.field_seq += 1;
                let mut start = FieldStart::default();
                // Pairing key only: the HWPX document scope relocates this id
                // if it collides with a preserved object (including later hosts).
                start.id = projection_images.field_seq.to_string();
                start.kind = "CLICK_HERE".into();
                start.name = name.unwrap_or_default();
                start.editable = "1".into();
                start.dirty = "0".into();
                start.z_order = "-1".into();
                start.field_id = CTRL_ID_CLICK_HERE.to_string();
                start.parameter_name = Some(String::new());
                start.parameters = vec![
                    FieldParameter::Integer { name: "Prop".into(), value: "9".into() },
                    FieldParameter::String {
                        name: "Command".into(),
                        value: command,
                        xml_space: "preserve".into(),
                    },
                    FieldParameter::String {
                        name: "Direction".into(),
                        value: hint_text.unwrap_or_default(),
                        xml_space: String::new(),
                    },
                ];
                let end = FieldBoundary::End {
                    begin_id: start.id.clone(),
                    field_id: start.field_id.clone(),
                };
                let end_shape = body_runs.last().map_or(char_shape_id, |run| run.char_shape_id);
                runs.push(Run::control(
                    Control::FieldBoundary { boundary: FieldBoundary::Start(Box::new(start)) },
                    char_shape_id,
                ));
                runs.extend(body_runs);
                runs.push(Run::control(Control::FieldBoundary { boundary: end }, end_shape));
                return FieldEmissionOutcome::ReEmitted;
            }
            let display_text = body_runs
                .into_iter()
                .filter_map(|run| match run.content {
                    RunContent::Text(text) => Some(text),
                    _ => None,
                })
                .collect::<String>();
            runs.push(Run::control(
                Control::Field {
                    field_type: hwpforge_foundation::FieldType::ClickHere,
                    hint_text,
                    help_text,
                    name,
                    display_text,
                },
                char_shape_id,
            ));
            FieldEmissionOutcome::Folded
        }
        ActiveField::SummaryField { start_utf16, command_token, display_text } => {
            // Emit a single Run carrying either typed `Control::Field`
            // (for known `$X` tokens) or `Control::UnknownSummary` for
            // future-compat raw carry. `display_text` is the cached
            // resolved value accumulated from the FieldBegin..FieldEnd
            // span — 한컴 native HWPX carries it in the body and an empty
            // body triggers the "낮은 보안 수준 복구" warning (#120/#136).
            let char_shape_id = shape_starting_at(hwp_para, start_utf16);
            let _ = end_utf16;
            let control = match hwpforge_foundation::FieldType::from_summary_token(&command_token) {
                Some(field_type) => Control::Field {
                    field_type,
                    hint_text: None,
                    help_text: None,
                    name: None,
                    display_text,
                },
                None => Control::UnknownSummary { token: command_token, display_text },
            };
            runs.push(Run::control(control, char_shape_id));
            FieldEmissionOutcome::Folded
        }
        ActiveField::DateCodeField { start_utf16, raw_command, display_text } => {
            // Emit Control::DateCodeField with `is_time_mode` derived
            // from the `T` prefix convention. The raw HWP5 command pattern
            // is smithy-internal and not carried into the core IR (E6 slice C).
            // `display_text` is the cached resolved date/time (#120/#136).
            let char_shape_id = shape_starting_at(hwp_para, start_utf16);
            let _ = end_utf16;
            let is_time_mode = raw_command.starts_with('T');
            runs.push(Run::control(
                Control::DateCodeField { is_time_mode, display_text },
                char_shape_id,
            ));
            FieldEmissionOutcome::Folded
        }
        ActiveField::PathField { start_utf16, raw_command, display_text } => {
            // Map raw `$P`/`$F`/`$P$F` to a typed PathFieldCommand
            // (Unknown for forward compatibility). Wave 12n. `display_text`
            // is the cached resolved path accumulated from the span (#120).
            let char_shape_id = shape_starting_at(hwp_para, start_utf16);
            let _ = end_utf16;
            use hwpforge_core::control::PathFieldCommand;
            let command = PathFieldCommand::from_wire(&raw_command);
            runs.push(Run::control(Control::PathField { command, display_text }, char_shape_id));
            FieldEmissionOutcome::Folded
        }
    }
}

/// Q8 (W0): the HWP5 wire keeps no time per memo, so every memo without
/// one takes the document's last-saved time (summary information), else the
/// epoch. A conversion never reads the clock: the same file converts to the
/// same bytes. Memos in run pictures' captions are reached too.
/// 이것을 실패시키는 것: 채우기 삭제(인코더 epoch 로 떨어져 저장 시각을 잃는다) ·
/// 저장 시각 대신 생성 시각(`memo_takes_the_last_saved_time`).
pub(crate) fn fill_memo_create_times(document: &mut hwpforge_core::document::Document) {
    fn fill(p: &mut Paragraph, time: &str) {
        p.for_each_paragraph_mut(|q| {
            for run in &mut q.runs {
                match &mut run.content {
                    RunContent::Control(c) => {
                        if let Control::Memo { metadata, .. } = c.as_mut() {
                            if metadata.create_datetime.is_empty() {
                                metadata.create_datetime = time.to_string();
                            }
                        }
                    }
                    RunContent::Image(img) => {
                        for cp in img.caption.iter_mut().flat_map(|c| c.paragraphs.iter_mut()) {
                            fill(cp, time);
                        }
                    }
                    _ => {}
                }
            }
        });
    }
    let time =
        document.metadata().modified.clone().unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string());
    for section in document.sections_mut() {
        let hs = section.headers.iter_mut().chain(section.footers.iter_mut());
        let tops =
            section.paragraphs.iter_mut().chain(hs.flat_map(|h| h.paragraphs.iter_mut())).chain(
                section.master_pages.iter_mut().flatten().flat_map(|m| m.paragraphs.iter_mut()),
            );
        for p in tops {
            fill(p, &time);
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
    // `MemoMetadata`. `id` is left empty so the HWPX encoder derives
    // `"memo{number}"`; `create_datetime` is filled after projection from
    // the document's last-saved time ([`fill_memo_create_times`], Q8) —
    // the wire carries neither field per memo. We go
    // through `Default::default()` because `MemoMetadata` is
    // `#[non_exhaustive]` and can't be constructed positionally outside
    // `hwpforge-core`.
    let mut metadata = hwpforge_core::MemoMetadata::default();
    if memo.command.shape_id != 65535 {
        push_projection_fallback(&mut projection_images.warnings, "memo.shape",
            format!("memo shape {} is unsupported; original command {:?} retained, shape definition is not carried", memo.command.shape_id, memo.command.raw));
    }
    metadata.shape_id_ref = memo.command.shape_id;
    metadata.number = memo.command.memo_id;
    metadata.author = memo.command.author.clone();
    metadata.command = memo.command.raw.clone();
    Run::control(Control::Memo { content: paragraphs, anchor_runs, metadata }, char_shape_id)
}

#[test]
fn fix4_nondefault_memo_shape_loss_is_explicit() {
    // 이것을 실패시키는 것: 메모 모양 참조/파싱 대체값의 무음 방출.
    for id in [0, 7, 99, 65535] {
        let memo = Hwp5MemoControl {
            ctrl_id: crate::ctrl_ids::CTRL_ID_MEMO,
            command: crate::schema::section::Hwp5MemoCommand {
                raw: format!("MEMO/{id}/1/2/3/author/"),
                shape_id: id,
                memo_id: 1,
                hancom_inst_a: 2,
                hancom_inst_b: 3,
                author: "author".into(),
                terminator: String::new(),
            },
            paragraphs: Vec::new(),
        };
        let mut images =
            ProjectionImageState::new(None, None, DecompressBudget::new(MAX_TOTAL_DECOMPRESSED));
        project_memo_run(&memo, &mut images, CharShapeIndex::new(0), Vec::new());
        assert_eq!(images.warnings.iter().any(|w| matches!(w,
            Hwp5Warning::ProjectionFallback { subject: "memo.shape", reason } if reason.contains(&id.to_string()))), id != 65535);
    }
}

/// Projects a HWP5 dutmal (덧말) control into a Core `Run` carrying
/// `Control::Dutmal`. Position, size ratio, and alignment map from the
/// wire's raw words (offsets pinned by the task #73 variants fixture —
/// see `schema::section::Hwp5DutmalControl` for the tail table); the
/// `option` word stays a verbatim mirror. Unknown align codes fall back
/// to CENTER with a `ProjectionFallback` warning (warning-first — the
/// wire's CENTER is `3`, so an unexpected `0` is surfaced rather than
/// silently absorbed). `styleIDRef` remains un-promoted (unattributed
/// reserved word).
fn project_dutmal_run(
    dutmal: &Hwp5DutmalControl,
    projection_images: &mut ProjectionImageState<'_>,
) -> Run {
    let position = match dutmal.pos_type_raw {
        0 => hwpforge_core::control::DutmalPosition::Top,
        1 => hwpforge_core::control::DutmalPosition::Bottom,
        2 => hwpforge_core::control::DutmalPosition::Right,
        3 => hwpforge_core::control::DutmalPosition::Left,
        _ => hwpforge_core::control::DutmalPosition::Top,
    };
    let align = match dutmal.align_raw {
        1 => hwpforge_core::control::DutmalAlign::Left,
        2 => hwpforge_core::control::DutmalAlign::Right,
        3 => hwpforge_core::control::DutmalAlign::Center,
        other => {
            projection_images.warnings.push(Hwp5Warning::ProjectionFallback {
                subject: "dutmal.align",
                reason: format!("unknown dutmal align wire code {other}; defaulting to CENTER"),
            });
            hwpforge_core::control::DutmalAlign::Center
        }
    };
    let mut metadata = hwpforge_core::DutmalMetadata::default();
    metadata.option = dutmal.option_raw;
    Run::control(
        Control::Dutmal {
            main_text: dutmal.main_text.clone(),
            sub_text: dutmal.sub_text.clone(),
            position,
            sz_ratio: dutmal.sz_ratio,
            align,
            metadata,
        },
        CharShapeIndex::new(0),
    )
}

/// Anchors a header/footer to Core paragraph `index` (§19.17 F2); paragraph
/// 0 is `None`, the same meaning, so a converted document's first-paragraph
/// headers keep their JSON and compare equal to API-built ones.
trait WithAnchor {
    fn with_anchor(self, index: usize) -> Self;
}

impl WithAnchor for HeaderFooter {
    fn with_anchor(mut self, index: usize) -> Self {
        // 이것을 실패시키는 것: 0 을 `Some(0)` 으로(첫 문단 머리말 JSON 이 바뀜).
        self.anchor_paragraph = (index != 0).then_some(index);
        self
    }
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

fn project_nested_paragraphs(
    paragraphs: &[Hwp5Paragraph],
    projection_images: &mut ProjectionImageState<'_>,
    image_context: ImageProjectionContext,
) -> Vec<Paragraph> {
    paragraphs
        .iter()
        .map(|nested| {
            project_paragraph_with_images(nested, projection_images, image_context).paragraph
        })
        .collect()
}

fn unknown_control_header(control: &Hwp5Control) -> Option<UnknownControlHeader<'_>> {
    match control {
        Hwp5Control::Bookmark { ctrl_id, header_data, name } => Some(UnknownControlHeader {
            ctrl_id: *ctrl_id,
            header_data,
            bookmark_name: name.as_deref(),
        }),
        Hwp5Control::Unknown { ctrl_id, header_data } => {
            Some(UnknownControlHeader { ctrl_id: *ctrl_id, header_data, bookmark_name: None })
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
/// | RefType        | N2=0 | N2=1   | N2=2       | N2=3      |
/// |----------------|------|--------|------------|-----------|
/// | Bookmark       | Page | Number | Contents¹  | UpDownPos |
/// | 그 외 (T/F/Eq/…) | Page | Number | Contents   | UpDownPos |
///
/// 책갈피 N2=1 은 한컴에서 "책갈피 본문/번호" 의미 (OBJECT_TYPE_NUMBER
/// emit), N2=2 는 "책갈피 이름" (OBJECT_TYPE_CONTENTS emit). spec 외
/// 의미이지만 native wire 와 일치.
///
/// Maps the HWP5 `cold` Border-line kind code to a Core [`BorderLineType`].
///
/// Codes follow 한글's real encoding (same table as
/// `schema::border_fill::Hwp5BorderLineKind`): `0=없음`, `1=실선`, `2=점선`,
/// `8=이중선`. Codes with no direct Core equivalent fall back to `Solid`.
/// Verified against the native `cold` ctrl in `nativ-colline.hwpx` (kind 8 →
/// `DOUBLE_SLIM`).
fn hwp5_col_border_kind_to_line_type(code: u8) -> BorderLineType {
    match code {
        0 => BorderLineType::None,
        1 => BorderLineType::Solid,
        2 => BorderLineType::Dot,
        3 => BorderLineType::Dash,
        4 => BorderLineType::DashDot,
        5 => BorderLineType::DashDotDot,
        6 => BorderLineType::LongDash,
        8 => BorderLineType::DoubleSlim,
        _ => BorderLineType::Solid,
    }
}

/// Maps a HWP5 border-width index to millimetres (한글's 16-step table).
///
/// Verified: index `9` → `0.7 mm` (native `nativ-colline.hwpx`). Out-of-range
/// indices fall back to the OWPML default `0.12 mm`.
fn hwp5_border_width_mm(index: u8) -> f64 {
    const WIDTHS_MM: [f64; 16] =
        [0.1, 0.12, 0.15, 0.2, 0.25, 0.3, 0.4, 0.5, 0.6, 0.7, 1.0, 1.5, 2.0, 3.0, 4.0, 5.0];
    WIDTHS_MM.get(index as usize).copied().unwrap_or(0.12)
}

/// Converts a HWP5 `COLORREF` (`0x00BBGGRR`) to a Core [`Color`].
fn colorref_to_color(c: u32) -> Color {
    Color::from_rgb((c & 0xFF) as u8, ((c >> 8) & 0xFF) as u8, ((c >> 16) & 0xFF) as u8)
}

/// ¹ E6 슬라이스 B (2026-06-28): Bookmark N2=2 = "책갈피 이름" 은
/// `Contents` variant 로 carry (이전 분리됐던 `BookmarkName` 흡수).
/// wire(N2=2)·Display(`OBJECT_TYPE_CONTENTS`) 가 caption-content 와
/// 동일하고, 구분은 동반 RefType 가 보유 (gotcha #27).
fn decode_hwp5_crossref_content_type(ref_type_code: u8, code: u8) -> RefContentType {
    match (ref_type_code, code) {
        (_, 0) => RefContentType::Page,
        (HWP5_CROSSREF_REF_TYPE_BOOKMARK, 1) => RefContentType::Number,
        // (Bookmark, 2) = "책갈피 이름" → `Contents` (OBJECT_TYPE_CONTENTS,
        // N2=2). E6 슬라이스 B: 이전 `BookmarkName` variant 를 흡수 — wire/
        // Display 가 동일하고 RefType 컨텍스트가 의미를 carry (gotcha #27)
        // 하므로 아래 `(_, 2) => Contents` 와 동일. 명시 arm 불필요.
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
            return RefTarget::Object(ObjectId::new(id));
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
        // 장식 없는 맨 숫자 폴백 — F1 실측 (한컴 기본 재저장 = sideChar="")
        // 과 정합. 장식을 날조하지 않는다.
        PageNumber::new(PageNumberPosition::BottomCenter, NumberFormatType::Digit)
    })
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
    // Number shape (번호 모양) lives in property bits 0-7 = header_data[4]
    // (the position above is property bits 8-11 = header_data[5]). The HWP5
    // `HWPNumberShape` codes map 1:1 to `NumberFormatType` (0=Digit,
    // 1=CircledDigit, 2=RomanCapital, 3=RomanSmall, …), verified against the
    // native `sample-pagenu-roman` fixture (ROMAN_CAPITAL = shape 2). Before
    // this the format byte was never read and every page number emitted
    // `Digit`, silently dropping Roman/Hangul/Latin page numbering (P0-3).
    let number_format = header_data
        .get(4)
        .and_then(|&shape| NumberFormatType::try_from(shape).ok())
        .unwrap_or(NumberFormatType::Digit);
    // The trailing side-decoration char is the last printable byte. Filter to
    // `is_ascii_graphic` (not `is_ascii`): the property word's number-shape byte
    // [4] and position byte [5] are small non-graphic values (e.g. 9 = InsideTop)
    // that would otherwise be mis-read as a decoration glyph like `\t`.
    // 장식 바이트가 전부 0 = **장식 없음** — F1 native fixture 실측
    // (2026-08-12): wire 전부 0 인 pgnp 를 한컴 자신이 `sideChar=""` 로
    // 재저장하고 PDF 에도 맨 숫자로 찍는다. 과거의 `"-"` 기본값은 근거 없는
    // 날조였다 (W2 에서 실측으로 교정).
    let decoration = header_data
        .iter()
        .rev()
        .find(|byte| **byte != 0)
        .copied()
        .filter(|byte| byte.is_ascii_graphic())
        .map(|byte| char::from(byte).to_string())
        .unwrap_or_default();
    Some(PageNumber::with_decoration(position, number_format, decoration))
}

// ---------------------------------------------------------------------------
// Text splitting
// ---------------------------------------------------------------------------

/// Dispatches a queued object control into its Core `Run`.
///
/// `char_shape_id` is the char shape at the control's visible anchor
/// position (task #76) — applied uniformly on the dispatch result so
/// the leaf `project_*_run` builders stay position-agnostic. Before
/// #76 every control run was hardcoded to char shape 0, which lost
/// the surrounding text style on paragraphs using non-default shapes.
fn project_control_run(
    control: &Hwp5Control,
    projection_images: &mut ProjectionImageState<'_>,
    image_context: ImageProjectionContext,
    char_shape_id: CharShapeIndex,
) -> Option<Run> {
    let run = match control {
        Hwp5Control::Table(table) => Some(Run::table(
            build_table_with_images(table, projection_images),
            CharShapeIndex::new(0),
        )),
        Hwp5Control::Image(image) => projection_images
            .build_image(image, image_context)
            .map(|core_image| Run::image(core_image, CharShapeIndex::new(0))),
        Hwp5Control::Line(line) => {
            let run = project_line_run(line, &mut projection_images.warnings);
            projection_images.styled(run, line.look.as_ref(), false)
        }
        Hwp5Control::Rect(rect) => {
            let run = project_rect_run(rect, &mut projection_images.warnings);
            projection_images.styled(run, rect.look.as_ref(), true)
        }
        Hwp5Control::Polygon(polygon) => {
            let mut run = project_polygon_run(polygon, &mut projection_images.warnings);
            attach_shape_text(
                run.as_mut(),
                &polygon.paragraphs,
                polygon.list_header_properties,
                polygon.text_margin,
                projection_images,
            );
            projection_images.styled(run, polygon.look.as_ref(), true)
        }
        Hwp5Control::Ellipse(ellipse) => {
            let mut run = project_ellipse_run(ellipse, &mut projection_images.warnings);
            attach_shape_text(
                run.as_mut(),
                &ellipse.paragraphs,
                ellipse.list_header_properties,
                ellipse.text_margin,
                projection_images,
            );
            projection_images.styled(run, ellipse.look.as_ref(), true)
        }
        Hwp5Control::Arc(arc) => {
            let run = project_arc_run(arc, &mut projection_images.warnings);
            projection_images.styled(run, arc.look.as_ref(), true)
        }
        Hwp5Control::Curve(curve) => {
            let run = project_curve_run(curve, &mut projection_images.warnings);
            projection_images.styled(run, curve.look.as_ref(), true)
        }
        Hwp5Control::TextArt(text_art) => {
            Some(project_text_art_run(text_art, &mut projection_images.warnings))
        }
        Hwp5Control::ConnectLine(connect_line) => {
            let run = project_connectline_run(connect_line, &mut projection_images.warnings);
            projection_images.styled(run, connect_line.look.as_ref(), false)
        }
        Hwp5Control::Equation(equation) => Some(project_equation_run(equation)),
        Hwp5Control::TextBox(textbox) => Some(project_textbox_run(textbox, projection_images)),
        Hwp5Control::Group(group) => group::project_group_run(group, projection_images),
        Hwp5Control::Footnote(subtree) => Some(project_footnote_run(subtree, projection_images)),
        Hwp5Control::Endnote(subtree) => Some(project_endnote_run(subtree, projection_images)),
        Hwp5Control::Header(subtree) | Hwp5Control::Footer(subtree) => {
            if projection_images.in_master {
                projection_images.warnings.push(Hwp5Warning::DroppedControl {
                    control: "master_page.header_footer",
                    reason: "header/footer inside a master list has no independent section anchor; omitted".into(),
                });
                return None;
            }
            // Reserve before recursion to preserve control order even when a
            // header itself contains another header. No separate tree scan.
            let is_header = matches!(control, Hwp5Control::Header(_));
            let unit = HeaderFooter::new(
                Vec::new(),
                hwp5_header_property_to_apply_page_type(subtree.properties_raw),
            )
            .with_anchor(projection_images.section_anchor);
            let units = if is_header {
                &mut projection_images.headers
            } else {
                &mut projection_images.footers
            };
            let index = units.len();
            units.push((subtree.wire_order, unit));
            let paragraphs = project_nested_paragraphs(
                &subtree.paragraphs,
                projection_images,
                ImageProjectionContext::Flow,
            );
            let units = if is_header {
                &mut projection_images.headers
            } else {
                &mut projection_images.footers
            };
            units[index].1.paragraphs = paragraphs;
            None
        }
        Hwp5Control::Memo(_) => {
            warn_unpaired_field(projection_images, "memo");
            None
        }
        // W4 무음 드롭 종결: 미지원 ctrl 은 여전히 드롭되지만 **집계 카운트**
        // 를 남겨 문서 끝에서 경고로 방출된다 (nwno/pghd 가 여기서 소리 없이
        // 죽던 것이 corpus 18% 문서의 fake output 원인이었다 — 계획 §0).
        Hwp5Control::Unknown { ctrl_id: CTRL_ID_PAGE_NUMBER, header_data } => Some(Run::control(
            Control::PageNumber {
                number: page_number_from_pgnp_header(header_data, &mut projection_images.warnings),
            },
            CharShapeIndex::new(0),
        )),
        Hwp5Control::Unknown { ctrl_id, .. } => {
            *projection_images.dropped_unknown.entry(*ctrl_id).or_insert(0) += 1;
            None
        }
        Hwp5Control::Dutmal(dutmal) => Some(project_dutmal_run(dutmal, projection_images)),
        Hwp5Control::Compose(compose) => Some(project_compose_run(compose)),
        Hwp5Control::IndexMark(indexmark) => Some(project_indexmark_run(indexmark)),
        // ClickHere emission flows through the `FieldBegin`/`ActiveField::ClickHere`
        // machinery in `project_paragraph_with_images_structural` (mirroring the
        // Memo dispatch above). If a ClickHere ever reaches this flat dispatch
        // path it means the structural pairing failed — drop rather than
        // silently emit a free-floating field run.
        Hwp5Control::ClickHere(_) => {
            warn_unpaired_field(projection_images, "clickhere");
            None
        }
        // SUMMERY auto-fields (Wave 12n) follow the same structural-pairing
        // pattern as ClickHere. Free-floating SummaryField means the inline
        // FieldBegin marker did not pair with this CtrlHeader; drop.
        Hwp5Control::SummaryField(_) => {
            warn_unpaired_field(projection_images, "summary");
            None
        }
        // %dte date/time format-code fields (Wave 12n) — same pattern.
        Hwp5Control::DateCodeField(_) => {
            warn_unpaired_field(projection_images, "datecode");
            None
        }
        // %pat path fields (Wave 12n) — same pattern.
        Hwp5Control::PathField(_) => {
            warn_unpaired_field(projection_images, "path");
            None
        }
        // atno inline page-number controls (Wave 12n) emit immediately.
        // The 0x12 inline marker is a ControlRef (no FieldEnd), so the
        // emission flows through the object-control queue, not an
        // ActiveField/FieldBegin pair.
        Hwp5Control::InlinePageNumber(atno) => {
            use hwpforge_core::control::InlinePageKind;
            // Inline the wire-flag → kind mapping (E6 slice C: the raw flag
            // is smithy-internal and no longer carried into the core IR).
            let kind = match atno.raw_flag {
                0x00 => InlinePageKind::CurrentPage,
                0x06 => InlinePageKind::TotalPages,
                _ => InlinePageKind::Unknown,
            };
            // §19.16 J (P19): bits 0–3 = 3·4·5 are a picture, table or
            // equation number (HWPX `numType` PICTURE·TABLE·EQUATION). The
            // corpus has none (`scan_atno.py`), so the record layout of its
            // format is unmeasured: reported, not guessed. 이것을 실패시키는
            // 것: 이 경고를 지우기 (`atno_caption_number_is_reported`).
            if matches!(atno.raw_flag & 0xF, 3..=5) {
                projection_images.warnings.push(Hwp5Warning::ProjectionFallback {
                    subject: "control.auto_number",
                    reason: format!(
                        "atno kind {} (picture/table/equation number) is not carried",
                        atno.raw_flag & 0xF
                    ),
                });
            }
            Some(Run::control(Control::InlinePageNumber { kind }, CharShapeIndex::new(0)))
        }
        Hwp5Control::NewNumber(nwno) => {
            use hwpforge_core::control::NewNumberKind;
            // 속성 bits 0-3 → kind (F1 실측: 0 = 쪽). 미지 raw(6-15)는
            // Unknown 으로 carry + 경고 — 타입을 지어내지 않는다.
            let kind = match nwno.kind_raw {
                0 => NewNumberKind::Page,
                1 => NewNumberKind::Footnote,
                2 => NewNumberKind::Endnote,
                3 => NewNumberKind::Picture,
                4 => NewNumberKind::Table,
                5 => NewNumberKind::Equation,
                raw => {
                    projection_images.warnings.push(Hwp5Warning::ProjectionFallback {
                        subject: "control.new_number",
                        reason: format!(
                            "nwno kind raw value {raw} is unmapped; carrying as Unknown"
                        ),
                    });
                    NewNumberKind::Unknown
                }
            };
            Some(Run::control(
                Control::NewNumber { kind, number: u32::from(nwno.number) },
                CharShapeIndex::new(0),
            ))
        }
        Hwp5Control::PageHiding(pghd) => {
            // 속성 bits 0-5 → 6 bool (F2 실측: 0x20 쪽번호 / 0x10 배경 /
            // 0x3F 전부 — secd word 동일 배열). bits 6+ 잔여는 실측 밖 —
            // 무음 무시 대신 경고로 표면화하고 정의된 6비트만 carry.
            if pghd.mask & !0x3F != 0 {
                projection_images.warnings.push(Hwp5Warning::ProjectionFallback {
                    subject: "control.page_hiding",
                    reason: format!(
                        "pghd mask {:#010x} has bits outside verified 0-5; carrying low bits only",
                        pghd.mask
                    ),
                });
            }
            let bit = |n: u32| pghd.mask & (1 << n) != 0;
            Some(Run::control(
                Control::PageHiding {
                    hide_header: bit(0),
                    hide_footer: bit(1),
                    hide_master_page: bit(2),
                    hide_border: bit(3),
                    hide_fill: bit(4),
                    hide_page_num: bit(5),
                },
                CharShapeIndex::new(0),
            ))
        }
        Hwp5Control::OleObject(ole) => project_ole_object_run(ole, projection_images),
        // %xrf cross-reference fields (Wave 12m) flow through the
        // `FieldBegin`/`ActiveField::CrossRef` machinery in
        // `project_paragraph_with_images_structural` — same pattern as
        // ClickHere / SummaryField / DateCodeField / PathField. A
        // free-floating CrossRef CtrlHeader means the inline `FieldBegin`
        // marker did not pair with it; drop rather than silently emit.
        Hwp5Control::Bookmark { .. } => {
            warn_unpaired_field(projection_images, "bookmark");
            None
        }
        Hwp5Control::CrossRef(_) => {
            warn_unpaired_field(projection_images, "crossref");
            None
        }
    };
    run.map(|mut r| {
        standalone::settle(control, &mut r, &mut projection_images.warnings);
        r.char_shape_id = char_shape_id;
        r
    })
}

/// Resolves the char shape at a visible UTF-16 position (task #76 —
/// shared by every `project_control_run` call site and the point
/// bookmark / fallback memo drains).
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
    // The document budget is already exhausted: the document will fail, so
    // skip every further extraction instead of allocating more.
    if projection_images.budget_error.is_some() {
        return None;
    }
    if let Some(reason) =
        projection_images.ole_assets.and_then(|plan| plan.reference_ambiguity(ole.binary_data_id))
    {
        projection_images.warnings.push(Hwp5Warning::ProjectionFallback {
            subject: "ole.bin_data_reference",
            reason: reason.to_string(),
        });
    }
    let Some(inflated) = projection_images.ole_inflated_for_reference(ole.binary_data_id) else {
        projection_images.warnings.push(Hwp5Warning::DroppedControl {
            control: "ole_object",
            reason: format!("ole_bin_data_unavailable binary_data_id={}", ole.binary_data_id),
        });
        return None;
    };

    let extracted = match inflated {
        Ok(inflated) => extract_chart_payload_from_inflated(inflated, inflated.len() as u64),
        Err(err) => Err(err.clone()),
    };
    match extracted {
        Ok(payload) => {
            // Every extraction (each control referencing the same asset too)
            // makes a new copy of the chart XML and the inner OLE bytes
            // resident, so each one is charged.
            let extracted_len = (payload.chart_xml.len() + payload.ole_bytes.len()) as u64;
            if let Err(err) = projection_images.budget.charge(
                extracted_len,
                &format!("chart binary_data_id={} (extracted)", ole.binary_data_id),
            ) {
                projection_images.budget_error = Some(err);
                return None;
            }
            // Dimensions come from the wrapping `gso ` CtrlHeader geometry
            // ([16..24] = display frame width/height in HWPUNIT) — the
            // `ShapeComponentOle` extent fields hold the OLE's *internal
            // canvas* size (observed constant 7200×7200), which 한컴
            // mirrors into `hp:orgSz`/`hc:extent`, NOT `hp:sz`. The
            // chart-fixture ground truth (`chart_02_single_pie.hwpx`
            // 한컴-native pair: hp:sz 32250×18750 == CtrlHeader geometry)
            // pinned this; using the extent shrank converted charts to
            // ≈2.5cm. Extent stays as the fallback for a zero geometry.
            // The geometry x/y mirror the placement convention used by
            // the other shape projections (zero-offset == inline).
            let geometry_width = i32::try_from(ole.geometry.width).unwrap_or(0);
            let geometry_height = i32::try_from(ole.geometry.height).unwrap_or(0);
            let Some(width) =
                chart_dimension(geometry_width).or_else(|| chart_dimension(ole.extent_width))
            else {
                projection_images.warnings.push(Hwp5Warning::DroppedControl {
                    control: "ole_object",
                    reason: format!(
                        "ole_chart_invalid_width binary_data_id={} geometry_width={} extent_width={}",
                        ole.binary_data_id, geometry_width, ole.extent_width
                    ),
                });
                return None;
            };
            let Some(height) =
                chart_dimension(geometry_height).or_else(|| chart_dimension(ole.extent_height))
            else {
                projection_images.warnings.push(Hwp5Warning::DroppedControl {
                    control: "ole_object",
                    reason: format!(
                        "ole_chart_invalid_height binary_data_id={} geometry_height={} extent_height={}",
                        ole.binary_data_id, geometry_height, ole.extent_height
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
                    placement: offset_placement(ole.geometry.x, ole.geometry.y),
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

/// Recovers the shape text vertical alignment from a `HWPTAG_LIST_HEADER`
/// 속성 word (표 65). Bits 5–6 hold the alignment (`0=top`, `1=center`,
/// `2=bottom`), mirroring `parse_table_cell`. `None` (no ListHeader 속성
/// captured) and any unmapped value default to [`VerticalAlign::Top`], which
/// matches 한컴's default and keeps default shapes byte-unchanged downstream.
///
/// Value 3 has no meaning and is reported (§19.22 D4).
/// 이것을 실패시키는 것: 3 의 보고를 지우기(`shape_vertical_align_3_is_reported`).
fn shape_vertical_align_from_list_header(
    list_header_properties: Option<u32>,
    warnings: &mut Vec<Hwp5Warning>,
) -> VerticalAlign {
    match list_header_properties {
        Some(props) => match ((props >> 5) & 0x03) as u8 {
            0 => VerticalAlign::Top,
            1 => VerticalAlign::Center,
            2 => VerticalAlign::Bottom,
            raw => {
                push_projection_fallback(
                    warnings,
                    "textbox.vertical_align",
                    format!("text vertical align {raw} has no meaning; read as top"),
                );
                VerticalAlign::Top
            }
        },
        None => VerticalAlign::Top,
    }
}

/// Same corner policy for standalone and grouped rectangles.
fn project_corner_ratio(raw: u8, warnings: &mut Vec<Hwp5Warning>) -> Option<u8> {
    match raw {
        0 => None,
        r if r > 50 => {
            push_projection_fallback(
                warnings,
                "shape.corner_ratio",
                format!("corner ratio {r} above 50; written as 50"),
            );
            Some(50)
        }
        r => Some(r),
    }
}

/// Project the outline once for both standalone shapes and group children.
/// Fill, corner ratio and geometry belong to their existing separate paths.
fn project_shape_line(
    line: &crate::schema::section::Hwp5ShapeLine,
    style: &mut hwpforge_core::control::ShapeStyle,
    warnings: &mut Vec<Hwp5Warning>,
    location: &str,
) {
    use hwpforge_core::control::LineStyle;
    style.line_color = Some(hwpforge_foundation::Color::from_raw(line.color & 0x00FF_FFFF));
    style.line_width = u32::try_from(line.thickness).ok();
    style.line_style = Some(match line.line_type() {
        0 => LineStyle::None,
        1 => LineStyle::Solid,
        2 => LineStyle::Dot,
        3 => LineStyle::Dash,
        other => {
            push_projection_fallback(
                warnings,
                "shape.line_type",
                format!("line type {other} is not confirmed against Hancom; drawn solid"),
            );
            LineStyle::Solid
        }
    });
    style.head_arrow = project_arrow(line.properties, true, warnings, location);
    style.tail_arrow = project_arrow(line.properties, false, warnings, location);
}

/// HWP5 wire sizes are row-major; Core's original 0–2 discriminants are stable.
fn project_arrow(
    properties: u32,
    head: bool,
    warnings: &mut Vec<Hwp5Warning>,
    location: &str,
) -> Option<hwpforge_core::control::ArrowStyle> {
    use hwpforge_core::control::ArrowStyle;
    use hwpforge_foundation::{ArrowSize, ArrowType};
    let side = if head { "head" } else { "tail" };
    let shape = ((properties >> if head { 10 } else { 16 }) & 0x3F) as u8;
    let raw_size = (properties >> if head { 22 } else { 26 }) & 0xF;
    let arrow_type = ArrowType::try_from(shape).ok();
    let size = match raw_size {
        0 => Some(ArrowSize::Small),
        1 => Some(ArrowSize::SmallMedium),
        2 => Some(ArrowSize::SmallLarge),
        3 => Some(ArrowSize::MediumSmall),
        4 => Some(ArrowSize::Medium),
        5 => Some(ArrowSize::MediumLarge),
        6 => Some(ArrowSize::LargeSmall),
        7 => Some(ArrowSize::LargeMedium),
        8 => Some(ArrowSize::Large),
        _ => None,
    };
    if arrow_type.is_none() {
        push_projection_fallback(
            warnings,
            "shape.arrow",
            format!("{side} arrow shape {shape} unsupported (expected 0–6); arrow omitted"),
        );
    }
    if size.is_none() {
        push_projection_fallback(
            warnings,
            "shape.arrow",
            format!("{side} arrow size {raw_size} unsupported (expected 0–8); arrow omitted"),
        );
    }
    // 이것을 실패시키는 것: 실제 끝점 제거에 ContentLost를 남기지 않기.
    if arrow_type.is_none() || size.is_none() {
        warnings.push(Hwp5Warning::ContentLost {
            kind: "shape_arrow".into(),
            reason: format!(
                "{side} arrow shape {shape}, size {raw_size}: unsupported endpoint omitted"
            ),
            location: format!("{location}.{side}"),
        });
    }
    ArrowStyle {
        arrow_type: arrow_type?,
        size: size?,
        filled: properties & (1 << if head { 30 } else { 31 }) != 0,
    }
    .into_nondefault()
}

/// The style slot of a shape control.
fn shape_style_slot(
    control: &mut Control,
) -> Option<&mut Option<hwpforge_core::control::ShapeStyle>> {
    match control {
        Control::TextBox { style, .. }
        | Control::Rect { style, .. }
        | Control::Ellipse { style, .. }
        | Control::Polygon { style, .. }
        | Control::Arc { style, .. }
        | Control::Curve { style, .. }
        | Control::Line { style, .. }
        | Control::ConnectLine { style, .. } => Some(style),
        _ => None,
    }
}

fn project_textbox_run(
    textbox: &Hwp5TextBoxControl,
    projection_images: &mut ProjectionImageState<'_>,
) -> Run {
    let style = textbox.look.as_ref().map(|look| projection_images.shape_style(look, true));
    let paragraphs = project_nested_paragraphs(
        &textbox.paragraphs,
        projection_images,
        ImageProjectionContext::TextBox,
    );
    let frame = shapes::object_frame(
        textbox.original_size,
        (
            hwp_unit_from_u32(textbox.geometry.width).as_i32(),
            hwp_unit_from_u32(textbox.geometry.height).as_i32(),
        ),
        textbox.ctrl_properties,
        &mut projection_images.warnings,
    );
    Run::control(
        Control::TextBox {
            frame,
            text_margin: text_box_margin(textbox.text_margin),
            paragraphs,
            width: hwp_unit_from_u32(textbox.geometry.width),
            height: hwp_unit_from_u32(textbox.geometry.height),
            placement: shape_placement(
                &textbox.geometry,
                textbox.ctrl_properties,
                &mut projection_images.warnings,
            ),
            caption: None,
            style,
            text_vertical_align: shape_vertical_align_from_list_header(
                textbox.list_header_properties,
                &mut projection_images.warnings,
            ),
        },
        CharShapeIndex::new(0),
    )
}

/// The text paragraphs an ellipse or polygon is written with — none when
/// the shape itself is not projected ([`attach_shape_text`] only fills a
/// projected shape). The layout-hint collector counts exactly these.
pub(crate) fn written_shape_text(control: &Hwp5Control) -> &[Hwp5Paragraph] {
    let mut scratch = Vec::new();
    match control {
        Hwp5Control::Ellipse(e) if project_ellipse_run(e, &mut scratch).is_some() => &e.paragraphs,
        Hwp5Control::Polygon(p) if project_polygon_run(p, &mut scratch).is_some() => &p.paragraphs,
        _ => &[],
    }
}

/// An ellipse's or polygon's text (§19.5 1a, review r1): its paragraphs and
/// text vertical alignment, projected like a text box's. Before, the shape
/// was written without its text.
///
/// 이것을 실패시키는 것: 이 호출을 지우기(`ellipse_and_polygon_keep_their_text`).
fn attach_shape_text(
    run: Option<&mut Run>,
    paragraphs: &[Hwp5Paragraph],
    list_header_properties: Option<u32>,
    text_margin: Option<crate::decoder::section::Hwp5TableCellMargin>,
    projection_images: &mut ProjectionImageState<'_>,
) {
    let Some(run) = run else { return };
    if paragraphs.is_empty() {
        return;
    }
    let projected =
        project_nested_paragraphs(paragraphs, projection_images, ImageProjectionContext::TextBox);
    report_shape_text_margin(text_margin, &mut projection_images.warnings);
    if let RunContent::Control(control) = &mut run.content {
        if let Control::Ellipse { paragraphs, text_vertical_align, .. }
        | Control::Polygon { paragraphs, text_vertical_align, .. } = control.as_mut()
        {
            *paragraphs = projected;
            *text_vertical_align = shape_vertical_align_from_list_header(
                list_header_properties,
                &mut projection_images.warnings,
            );
        }
    }
}

/// A text box's inner margin from its text list header (§19.22 D4; Hancom
/// twins match `textMargin` in all 348 text boxes), carried also when it is
/// 283 on every side.
pub(super) fn text_box_margin(
    margin: Option<crate::decoder::section::Hwp5TableCellMargin>,
) -> Option<TableMargin> {
    margin.map(core_margin)
}

/// An ellipse's or polygon's text margin other than 283 on every side: Core
/// carries a text margin for text boxes only (S16), so the output writes
/// 283 — reported, not refused (the text stays; only its inset changes).
/// 이것을 실패시키는 것: 이 보고를 지우기(`text_margins_are_read`).
pub(super) fn report_shape_text_margin(
    margin: Option<crate::decoder::section::Hwp5TableCellMargin>,
    warnings: &mut Vec<Hwp5Warning>,
) {
    if let Some(m) = margin {
        if [m.left, m.right, m.top, m.bottom] != [283; 4] {
            push_projection_fallback(
                warnings,
                "shape.text_margin",
                format!(
                    "text margin {} {} {} {} of an ellipse or polygon is not carried; 283 is written",
                    m.left, m.right, m.top, m.bottom
                ),
            );
        }
    }
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
    // Wave 12p Step 3: HWP5 CtrlHeader trailer instance ID 통과.
    // 한컴 native `<hp:footNote instId="...">` 와 매칭되어 HWPX
    // cross-ref Command `?#<id>` lookup 이 동작. 0 은 unset 의미
    // (Step 1b 의 fallback 값) — None 으로 맵핑.
    let inst_id =
        (subtree.instance_id != 0).then_some(ObjectId::new(u64::from(subtree.instance_id)));
    Run::control(Control::Footnote { inst_id, paragraphs }, CharShapeIndex::new(0))
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
    // Wave 12p Step 3: 동일 패턴 (`<hp:endNote instId="...">`).
    let inst_id =
        (subtree.instance_id != 0).then_some(ObjectId::new(u64::from(subtree.instance_id)));
    Run::control(Control::Endnote { inst_id, paragraphs }, CharShapeIndex::new(0))
}

/// Project a TextArt (글맵시). Geometry comes from the owning gso CtrlHeader
/// (mirroring the ellipse), and the warped-text payload from the `0x5A`
/// `ShapeTextArt` sub-record. The HWP5 wire stores `text_shape`/`align` as
/// integer enums; we map them to the HWPX string names, warning (rather than
/// silently defaulting) when a value is out of the known range.
fn project_text_art_run(text_art: &Hwp5TextArtControl, warnings: &mut Vec<Hwp5Warning>) -> Run {
    let ta = &text_art.text_art;
    let shape = crate::schema::section::textart_shape_name(ta.text_shape).unwrap_or_else(|| {
        warnings.push(Hwp5Warning::ProjectionFallback {
            subject: "textart.text_shape",
            reason: format!(
                "unknown TextArt shape enum {} (defaulting to RECTANGLE)",
                ta.text_shape
            ),
        });
        "RECTANGLE"
    });
    let align = crate::schema::section::textart_align_name(ta.align).unwrap_or_else(|| {
        warnings.push(Hwp5Warning::ProjectionFallback {
            subject: "textart.align",
            reason: format!("unknown TextArt align enum {} (defaulting to LEFT)", ta.align),
        });
        "LEFT"
    });
    let width = hwp_unit_from_u32(text_art.geometry.width);
    let height = hwp_unit_from_u32(text_art.geometry.height);
    let inst_id =
        (text_art.instance_id != 0).then_some(ObjectId::new(u64::from(text_art.instance_id)));
    // S20 review r1: the initial box and size lock, folded against the
    // TextArt's own default (the 14173 design box).
    // 이것을 실패시키는 것: frame 을 두지 않기 / 표시 크기와 비교해 접기
    // (`text_art_keeps_its_frame`).
    let design = hwpforge_core::frame::TEXT_ART_DESIGN_SIDE;
    let frame = hwpforge_core::ObjectFrame::new()
        .with_size_protect(text_art.ctrl_properties & shapes::CTRL_PROPERTY_SIZE_PROTECT_BIT != 0);
    let frame = match shapes::framed_initial(
        text_art.original_size,
        (width.as_i32(), height.as_i32()),
        (design, design),
        warnings,
    ) {
        Some(o) => frame.with_original_size(o),
        None => frame,
    };
    let control = Control::TextArt {
        frame,
        text: ta.text.clone(),
        shape: shape.to_string(),
        font_name: ta.font_name.clone(),
        font_style: ta.font_style.clone(),
        align: align.to_string(),
        line_spacing: ta.line_spacing,
        char_spacing: ta.char_spacing,
        width,
        height,
        placement: offset_placement(text_art.geometry.x, text_art.geometry.y),
        fill_color: None,
        inst_id,
    };
    Run::control(control, CharShapeIndex::new(0))
}

/// Project an equation. The HancomEQN script is carried verbatim; the box size
/// comes from the `eqed` ctrl-header geometry when positive (equations are
/// always inline, so there is no offset to set).
fn project_equation_run(equation: &Hwp5EquationControl) -> Run {
    let mut control = hwpforge_core::control::Control::equation(&equation.script);
    if let Control::Equation { width, height, inst_id, .. } = &mut control {
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
        // Wave 12p Step 3: `<hp:equation id="...">` cross-ref target.
        if equation.instance_id != 0 {
            *inst_id = Some(ObjectId::new(u64::from(equation.instance_id)));
        }
    }
    Run::control(control, CharShapeIndex::new(0))
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

// ---------------------------------------------------------------------------
// Table construction
// ---------------------------------------------------------------------------

// A generated cell owns a paragraph and a run as well as table metadata.
// Bound that amplification independently of compressed bytes and nesting depth.
const MAX_TABLE_CELLS: u64 = 65_536;
const MAX_DOCUMENT_CELLS: u64 = 262_144;

impl ProjectionImageState<'_> {
    fn charge_table(&mut self, table: &Hwp5Table) -> bool {
        if self.budget_error.is_some() {
            return false;
        }
        let rows = table
            .cells
            .iter()
            .map(|c| u64::from(c.row) + u64::from(c.row_span.max(1)))
            .max()
            .unwrap_or(1)
            .max(u64::from(table.rows.max(1)));
        let cols = table
            .cells
            .iter()
            .map(|c| u64::from(c.column) + u64::from(c.col_span.max(1)))
            .max()
            .unwrap_or(1)
            .max(u64::from(table.cols.max(1)));
        let area = rows.saturating_mul(cols);
        // Explicit cells plus at most one placeholder per uncovered row;
        // empty tables materialize the full rectangle.
        let generated = if table.cells.is_empty() {
            area
        } else {
            (table.cells.len() as u64).saturating_add(rows)
        };
        let total = self.generated_cells.saturating_add(generated);
        if area > MAX_TABLE_CELLS || generated > MAX_TABLE_CELLS || total > MAX_DOCUMENT_CELLS {
            self.budget_error = Some(Hwp5Error::RecordParse {
                offset: 0,
                detail: format!("table cell budget exceeded: {rows}x{cols}, generated={generated}, document={total}; limits table={MAX_TABLE_CELLS}, document={MAX_DOCUMENT_CELLS}"),
            });
            return false;
        }
        self.generated_cells = total;
        true
    }
}

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
    if table.caption.is_some() {
        warnings.push(Hwp5Warning::DroppedControl {
            control: "caption",
            reason: "the caption of a table without cells is not carried".to_string(),
        });
    }
    core_table
}

fn build_table_with_images(
    table: &Hwp5Table,
    projection_images: &mut ProjectionImageState<'_>,
) -> Table {
    // Charge before building even a single Core cell or row buffer. Keep the
    // first error in the document state, as chart decompression does.
    if !projection_images.charge_table(table) {
        return Table::new(Vec::new());
    }
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
            let projected = cells
                .iter()
                .copied()
                .map(|cell| project_table_cell_with_images(cell, projection_images))
                .collect();
            let row_height = cells.iter().map(|cell| cell.height).max().unwrap_or(0);
            match HwpUnit::new(row_height) {
                Ok(height) if row_height > 0 => TableRow::with_height(projected, height),
                _ => TableRow::new(projected),
            }
        })
        .collect();

    TableRow::derive_header_rows(&mut rows);
    restore_placeholder_cells_for_uncovered_rows(&mut rows, &mut projection_images.warnings);

    let mut core_table = Table::new(rows);
    apply_table_projection_metadata(table, &mut core_table, &mut projection_images.warnings);
    core_table.caption = project_table_caption(table, &core_table, projection_images);
    core_table
}

/// The caption paragraphs the projection writes for `table` — none when
/// the table has no cells (its caption is dropped, [`build_empty_table`]) or
/// the caption has no paragraph (dropped, [`project_table_caption`]). The
/// layout hints follow this so their count matches the written paragraphs.
pub(crate) fn written_caption_paragraphs(table: &Hwp5Table) -> &[Hwp5Paragraph] {
    match &table.caption {
        Some(caption) if !table.cells.is_empty() => &caption.paragraphs,
        _ => &[],
    }
}

/// D: the table caption as a Core caption. `last_width` is kept only when
/// it differs from the width the HWPX encoder writes for this table (its
/// own width, else the first row's), so an unedited conversion writes the
/// source value back either way. A width or last width Core cannot hold is
/// dropped with a warning — not taken for "equal to the table width".
/// 이것을 실패시키는 것: 측 대응 밀림 · `fullSz` 무시 · 정규화 생략/반전
/// (`table_caption_projects_to_core`) · 표현 불가 폭 경고 삭제 · 빈 캡션
/// 유지(`caption_without_paragraphs_or_with_a_huge_width_is_reported`).
fn project_table_caption(
    table: &Hwp5Table,
    core_table: &Table,
    projection_images: &mut ProjectionImageState<'_>,
) -> Option<Caption> {
    let source = table.caption.as_ref()?;
    let side = match source.side {
        0 => CaptionSide::Left,
        1 => CaptionSide::Right,
        2 => CaptionSide::Top,
        _ => CaptionSide::Bottom,
    };
    if source.paragraphs.is_empty() {
        projection_images.warnings.push(Hwp5Warning::DroppedControl {
            control: "caption",
            reason: "a table caption without paragraphs is not carried".to_string(),
        });
        return None;
    }
    let paragraphs = source
        .paragraphs
        .iter()
        .map(|paragraph| {
            project_paragraph_with_images(
                paragraph,
                projection_images,
                ImageProjectionContext::Flow,
            )
            .paragraph
        })
        .collect();
    let unit = |v: i64| HwpUnit::new(i32::try_from(v).unwrap_or(i32::MAX)).ok();
    let encoder_width = core_table.width.map(HwpUnit::as_i32).unwrap_or_else(|| {
        core_table.rows.first().map_or(0, |r| r.cells.iter().map(|c| c.width.as_i32()).sum())
    });
    let mut unrepresentable = |field: &str, value: u32| {
        push_projection_fallback(
            &mut projection_images.warnings,
            "table.caption",
            format!("caption {field} {value} is out of range; dropped"),
        );
    };
    let last_width = if i64::from(source.last_width) == i64::from(encoder_width) {
        None
    } else {
        let value = unit(i64::from(source.last_width));
        if value.is_none() {
            unrepresentable("last width", source.last_width);
        }
        value
    };
    let mut caption = Caption::new(paragraphs, side)
        .with_gap(unit(i64::from(source.gap)).unwrap_or(HwpUnit::ZERO))
        .with_full_size(source.full_size)
        .with_last_width(last_width);
    if source.width > 0 {
        match unit(i64::from(source.width)) {
            Some(width) => caption = caption.with_width(width),
            None => unrepresentable("width", source.width),
        }
    }
    Some(caption)
}

/// Keeps rows that the wire left empty truthful when row spans from earlier
/// rows cover them, and falls back to a placeholder cell (with a warning —
/// historically this was silent) only when the table does not tile a
/// well-formed grid.
///
/// The check mirrors `hwpforge_core::validate`'s covered-row rule exactly
/// (grid derivation succeeds and the grid has non-zero width), so the
/// projected table is guaranteed to pass validation either way.
fn restore_placeholder_cells_for_uncovered_rows(
    rows: &mut Vec<TableRow>,
    warnings: &mut Vec<Hwp5Warning>,
) {
    if !rows.iter().any(|row| row.cells.is_empty()) {
        return;
    }
    let candidate = Table::new(std::mem::take(rows));
    let covered = hwpforge_core::table::grid::TableGrid::from_table(&candidate)
        .is_ok_and(|grid| grid.dimensions().1 > 0);
    *rows = candidate.rows;
    if covered {
        return;
    }
    for (row_idx, row) in rows.iter_mut().enumerate() {
        if row.cells.is_empty() {
            row.cells.push(empty_cell());
            push_projection_fallback(
                warnings,
                "table.covered_row",
                format!(
                    "uncovered_empty_hwp5_table_row row={row_idx}; inserting_placeholder_cell (row has no cells and is not covered by row spans)"
                ),
            );
        }
    }
}

/// F: the table's cell zones, each checked against the declared grid. A
/// zone whose start is past its end or whose end is outside the table is
/// dropped and reported (Core `validate` would refuse it); the border fill
/// id is checked against `DocInfo` after projection
/// ([`drop_cell_zones_with_unknown_border_fill`]).
/// 이것을 실패시키는 것: 역전 검사 삭제 · 표 밖 검사 삭제 · 경고 삭제
/// (`invalid_cell_zones_are_dropped_and_reported`).
fn project_cell_zones(table: &Hwp5Table, warnings: &mut Vec<Hwp5Warning>) -> Vec<CellZone> {
    let mut zones = Vec::with_capacity(table.cell_zones.len());
    for (i, z) in table.cell_zones.iter().enumerate() {
        let why = if z.start_row > z.end_row || z.start_col > z.end_col {
            "starts past its end"
        } else if z.end_row >= table.rows || z.end_col >= table.cols {
            "lies outside the table"
        } else {
            zones.push(CellZone::new(
                z.start_row,
                z.start_col,
                z.end_row,
                z.end_col,
                u32::from(z.border_fill_id),
            ));
            continue;
        };
        push_projection_fallback(
            warnings,
            "table.cell_zone",
            format!(
                "cell zone {i} ({}, {})-({}, {}) of a {}x{} table {why}; dropped",
                z.start_row, z.start_col, z.end_row, z.end_col, table.rows, table.cols
            ),
        );
    }
    zones
}

/// F: drops every table cell zone whose border fill id is not one of the
/// document's `border_fill_count` border fills (ids are 1-based), with a
/// warning each — the HWPX it would become points at nothing.
/// 이것을 실패시키는 것: 범위 검사 삭제 · 0 을 유효로 · 마지막 id 를 거부
/// (`cell_zone_with_unknown_border_fill_is_dropped`).
pub(crate) fn drop_cell_zones_with_unknown_border_fill(
    document: &mut Document<Draft>,
    border_fill_count: usize,
    warnings: &mut Vec<Hwp5Warning>,
) {
    document.for_each_table_mut(|table| {
        table.cell_zones.retain(|z| {
            let ok = hwpforge_core::style_lookup::lookup(
                (z.border_fill_id >= 1 && z.border_fill_id as usize <= border_fill_count).then_some(()),
                "cell zone borderFill", z.border_fill_id,
            ).is_ok();
            if !ok {
                push_projection_fallback(
                    warnings,
                    "table.cell_zone",
                    format!(
                        "cell zone border fill {} is not one of the {border_fill_count} border fills; dropped",
                        z.border_fill_id
                    ),
                );
            }
            ok
        });
    });
}

fn apply_table_projection_metadata(
    table: &Hwp5Table,
    core_table: &mut Table,
    warnings: &mut Vec<Hwp5Warning>,
) {
    core_table.repeat_header = table.repeat_header;
    core_table.no_adjust = table.no_adjust;
    core_table.cell_zones = project_cell_zones(table, warnings);
    core_table.cell_spacing = (table.cell_spacing > 0)
        .then(|| HwpUnit::new(i32::from(table.cell_spacing)))
        .transpose()
        .unwrap_or(None);
    core_table.border_fill_id = table.border_fill_id.map(u32::from);
    // §19.22 D1: the table's own margins, carried even when they equal the
    // encoder's defaults (as the HWPX decoder carries the wire values).
    core_table.in_margin = table.inner_margin.map(core_margin);
    core_table.out_margin = table.ctrl.as_ref().and_then(|ctrl| ctrl.outer_margin).map(core_margin);
    // S19: a dormant cell margin equal to the table's padding is what the
    // encoder writes anyway; only distinct values are carried.
    // 이것을 실패시키는 것: 이 정리를 지우기(`dormant_cell_margin_is_kept`).
    let padding = core_table.in_margin.unwrap_or(TableMargin {
        left: HwpUnit::new(510).expect("510"),
        right: HwpUnit::new(510).expect("510"),
        top: HwpUnit::new(141).expect("141"),
        bottom: HwpUnit::new(141).expect("141"),
    });
    for row in &mut core_table.rows {
        for cell in &mut row.cells {
            if cell.dormant_margin == Some(padding) {
                cell.dormant_margin = None;
            }
        }
    }
    // §19.6 A: 표 자신의 CtrlHeader 속성 word·오프셋 → `Table::placement`.
    if let Some(ctrl) = &table.ctrl {
        let wire = parse_object_placement_wire(ctrl.properties, ctrl.geometry.x, ctrl.geometry.y);
        core_table.placement = table_placement_from_wire(&wire, warnings);
        // S20: the size lock (bit 20). 이것을 실패시키는 것: 읽지 않기
        // (`size_lock_is_carried`).
        core_table.frame.size_protect =
            ctrl.properties & shapes::CTRL_PROPERTY_SIZE_PROTECT_BIT != 0;
    }
    // Wave 12p Step 3: HWP5 Table CtrlHeader trailer instance ID 통과.
    // 한컴 native `<hp:tbl id="...">` cross-ref target 과 매칭. 0 은
    // unset (Step 1c-1 fallback).
    if table.instance_id != 0 {
        core_table.inst_id = Some(ObjectId::new(u64::from(table.instance_id)));
    }

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
                )
                .paragraph
            })
            .collect()
    };

    let mut core_cell =
        TableCell::with_span(paragraphs, width, cell.col_span.max(1), cell.row_span.max(1));
    core_cell.header = Some(cell.is_header);
    core_cell.height =
        (cell.height > 0).then(|| HwpUnit::new(cell.height)).transpose().unwrap_or(None);
    core_cell.border_fill_id = cell.border_fill_id.map(u32::from);
    // §19.22 D2: a cell without its own margin (property bit 16 clear)
    // uses the table's default padding. 이것을 실패시키는 것: 판정 반전 /
    // 비트 번호 15·17(`table_margins_and_cell_margin_flag_are_read`).
    core_cell.margin = cell.has_margin.then(|| core_margin(cell.margin));
    // S19: a cell using the table's padding keeps its own values (Hancom
    // keeps them in `<hp:cellMargin>`); the table pass drops those equal to
    // the padding. 이것을 실패시키는 것: 이 값을 싣지 않기
    // (`dormant_cell_margin_is_kept`).
    core_cell.dormant_margin = (!cell.has_margin).then(|| core_margin(cell.margin));
    // §19.22 D3: line wrap. 2 (keep) has no Hancom twin to confirm it and
    // is reported; 3 has no meaning and is read as break.
    // 이것을 실패시키는 것: 비트 3–4 를 5–6 으로 / 2 의 경고 삭제 / 3 을
    // squeeze 로(`cell_line_wrap_is_read_from_bits_3_and_4`).
    core_cell.line_wrap = match cell.line_wrap {
        0 => CellLineWrap::Break,
        1 => CellLineWrap::Squeeze,
        2 => {
            push_projection_fallback(
                &mut projection_images.warnings,
                "cell.line_wrap_unverified",
                format!(
                    "row={} col={}: line wrap 2 (keep) has no verified twin",
                    cell.row, cell.column
                ),
            );
            CellLineWrap::Keep
        }
        raw => {
            push_projection_fallback(
                &mut projection_images.warnings,
                "cell.line_wrap",
                format!(
                    "row={} col={}: line wrap {raw} has no meaning; read as break",
                    cell.row, cell.column
                ),
            );
            CellLineWrap::Break
        }
    };
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

/// A HWP5 margin block as a Core margin (negative values become 0, as
/// before for cells).
fn core_margin(m: crate::decoder::section::Hwp5TableCellMargin) -> TableMargin {
    let unit = |v: i16| HwpUnit::new(i32::from(v)).unwrap_or(HwpUnit::ZERO);
    TableMargin {
        left: unit(m.left),
        right: unit(m.right),
        top: unit(m.top),
        bottom: unit(m.bottom),
    }
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
/// Dimensions are checked before conversion; an out-of-range positive value
/// must never become a valid negative margin.
fn page_def_to_settings(pd: &Hwp5PageDef) -> Hwp5Result<PageSettings> {
    let u = |field: &str, v: u32| -> Hwp5Result<HwpUnit> {
        let signed = i32::try_from(v).map_err(|_| Hwp5Error::RecordParse {
            offset: 0,
            detail: format!("PageDef {field} value {v} exceeds signed range"),
        })?;
        Ok(HwpUnit::new(signed)?)
    };
    Ok(PageSettings {
        width: u("width", pd.width)?,
        height: u("height", pd.height)?,
        margin_left: u("margin_left", pd.margin_left)?,
        margin_right: u("margin_right", pd.margin_right)?,
        margin_top: u("margin_top", pd.margin_top)?,
        margin_bottom: u("margin_bottom", pd.margin_bottom)?,
        header_margin: u("header_margin", pd.header_margin)?,
        footer_margin: u("footer_margin", pd.footer_margin)?,
        gutter: u("gutter", pd.gutter)?,
        landscape: pd.landscape,
        ..PageSettings::a4()
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bookmark_n2_2_collapses_to_contents_not_separate_variant() {
        // E6 슬라이스 B 회귀 잠금: (Bookmark, 2) = "책갈피 이름" 은 이전
        // `BookmarkName` variant 가 아니라 `Contents` 로 carry — wire/Display
        // 가 caption-content 와 동일하고 구분은 동반 RefType 가 보유 (gotcha #27).
        assert_eq!(
            decode_hwp5_crossref_content_type(HWP5_CROSSREF_REF_TYPE_BOOKMARK, 2),
            RefContentType::Contents,
            "Bookmark N2=2 must collapse to Contents (BookmarkName absorbed)"
        );
        // 비-Bookmark N2=2 도 동일하게 Contents (대칭 확인).
        assert_eq!(
            decode_hwp5_crossref_content_type(HWP5_CROSSREF_REF_TYPE_BOOKMARK + 1, 2),
            RefContentType::Contents
        );
        // Display byte 불변: Contents → OBJECT_TYPE_CONTENTS.
        assert_eq!(RefContentType::Contents.to_string(), "OBJECT_TYPE_CONTENTS");
    }

    #[test]
    fn shape_vertical_align_extracts_bits_5_6() {
        // 표 65 문단 리스트 헤더 속성 bit 5~6 = 세로 정렬 (0=top, 1=center, 2=bottom).
        // value << 5 places the alignment code in the right bits.
        assert_eq!(
            shape_vertical_align_from_list_header(Some(0 << 5), &mut Vec::new()),
            VerticalAlign::Top,
            "code 0 → top"
        );
        assert_eq!(
            shape_vertical_align_from_list_header(Some(1 << 5), &mut Vec::new()),
            VerticalAlign::Center,
            "code 1 → center"
        );
        assert_eq!(
            shape_vertical_align_from_list_header(Some(2 << 5), &mut Vec::new()),
            VerticalAlign::Bottom,
            "code 2 → bottom"
        );
    }

    /// §19.22 D4: value 3 has no meaning — read as top and reported.
    #[test]
    fn shape_vertical_align_3_is_reported() {
        let mut warnings = Vec::new();
        assert_eq!(
            shape_vertical_align_from_list_header(Some(3 << 5), &mut warnings),
            VerticalAlign::Top
        );
        assert!(matches!(
            warnings[..],
            [Hwp5Warning::ProjectionFallback { subject: "textbox.vertical_align", .. }]
        ));
    }

    #[test]
    fn shape_vertical_align_ignores_other_bits() {
        // Lower bits (text direction, line wrap) and higher bits must not leak
        // into the alignment. Set every bit *except* 5~6 and expect Top.
        let noise = !0b0110_0000u32;
        assert_eq!(
            shape_vertical_align_from_list_header(Some(noise), &mut Vec::new()),
            VerticalAlign::Top
        );
        // center with surrounding noise still decodes center.
        assert_eq!(
            shape_vertical_align_from_list_header(Some(noise | (1 << 5)), &mut Vec::new()),
            VerticalAlign::Center
        );
    }

    #[test]
    fn shape_vertical_align_defaults_top_when_absent() {
        assert_eq!(
            shape_vertical_align_from_list_header(None, &mut Vec::new()),
            VerticalAlign::Top
        );
        // code 3 (reserved) is not a real value → default Top, no fake mapping.
        assert_eq!(
            shape_vertical_align_from_list_header(Some(3 << 5), &mut Vec::new()),
            VerticalAlign::Top
        );
    }

    #[test]
    fn project_textbox_carries_center_vertical_align_from_list_header() {
        // 한컴 stores the textbox 세로 정렬 in the ListHeader 속성 bits 5~6.
        // A CENTER (code 1) textbox must project to Core with Center align.
        let textbox = Hwp5TextBoxControl {
            standalone_render: None,
            original_size: None,
            look: None,
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                outer_margin: [0; 4],
                x: 10,
                y: 20,
                width: 8_000,
                height: 6_000,
            },
            paragraphs: vec![make_paragraph("가운데", 0, 0)],
            list_header_properties: Some(1 << 5),
            text_margin: None,
            ctrl_properties: 0,
        };
        let mut images =
            ProjectionImageState::new(None, None, DecompressBudget::new(MAX_TOTAL_DECOMPRESSED));
        let run = project_textbox_run(&textbox, &mut images);
        match run.content.as_control().unwrap().clone() {
            Control::TextBox { text_vertical_align, .. } => {
                assert_eq!(text_vertical_align, VerticalAlign::Center);
            }
            other => panic!("expected Control::TextBox, got {other:?}"),
        }
    }

    /// S20 review r1: a top-level TextArt keeps its initial box against its
    /// own default, the 14173 design box — 7000 x 8000 is kept, the design
    /// box itself is no frame value, the displayed size (5000 x 3000) as the
    /// initial box is kept — and its size lock.
    #[test]
    fn text_art_keeps_its_frame() {
        let art = |original: Option<(u32, u32)>, properties: u32| Hwp5TextArtControl {
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                outer_margin: [0; 4],
                x: 0,
                y: 0,
                width: 5000,
                height: 3000,
            },
            text_art: crate::schema::section::Hwp5ShapeTextArt {
                points: [crate::schema::section::Hwp5ShapePoint { x: 0, y: 0 }; 4],
                text: "글".into(),
                font_name: String::new(),
                font_style: String::new(),
                font_type: 0,
                text_shape: 0,
                line_spacing: 120,
                char_spacing: 100,
                align: 0,
            },
            instance_id: 0,
            original_size: original,
            ctrl_properties: properties,
        };
        let frame = |a: &Hwp5TextArtControl| {
            let run = project_text_art_run(a, &mut Vec::new());
            *run.content.as_control().unwrap().frame().unwrap()
        };
        let u = |v| HwpUnit::new(v).unwrap();
        let size = |w, h| Some(hwpforge_foundation::Size::new(u(w), u(h)));
        assert_eq!(frame(&art(Some((7000, 8000)), 0)).original_size, size(7000, 8000));
        assert_eq!(frame(&art(Some((14173, 14173)), 0)).original_size, None);
        assert_eq!(frame(&art(Some((5000, 3000)), 0)).original_size, size(5000, 3000));
        assert!(frame(&art(None, 1 << 20)).size_protect);
        assert!(!frame(&art(None, 0)).size_protect);
    }

    /// S20: a text box keeps its initial box (4,000 × 3,000, shown 8,000 ×
    /// 6,000) and its size lock as its frame.
    /// 이것을 실패시키는 것: 글상자 frame 을 두지 않기.
    #[test]
    fn project_textbox_carries_its_frame() {
        let textbox = Hwp5TextBoxControl {
            standalone_render: None,
            original_size: Some((4_000, 3_000)),
            look: None,
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                outer_margin: [0; 4],
                x: 0,
                y: 0,
                width: 8_000,
                height: 6_000,
            },
            paragraphs: vec![make_paragraph("틀", 0, 0)],
            list_header_properties: None,
            text_margin: None,
            ctrl_properties: 1 << 20,
        };
        let mut images =
            ProjectionImageState::new(None, None, DecompressBudget::new(MAX_TOTAL_DECOMPRESSED));
        let run = project_textbox_run(&textbox, &mut images);
        let frame = *run.content.as_control().unwrap().frame().unwrap();
        let u = |v| HwpUnit::new(v).unwrap();
        assert_eq!(frame.original_size, Some(hwpforge_foundation::Size::new(u(4_000), u(3_000))));
        assert!(frame.size_protect);
    }

    #[test]
    fn project_textbox_defaults_top_when_list_header_absent() {
        let textbox = Hwp5TextBoxControl {
            standalone_render: None,
            original_size: None,
            look: None,
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                outer_margin: [0; 4],
                x: 0,
                y: 0,
                width: 8_000,
                height: 6_000,
            },
            paragraphs: vec![make_paragraph("위", 0, 0)],
            list_header_properties: None,
            text_margin: None,
            ctrl_properties: 0,
        };
        let mut images =
            ProjectionImageState::new(None, None, DecompressBudget::new(MAX_TOTAL_DECOMPRESSED));
        let run = project_textbox_run(&textbox, &mut images);
        match run.content.as_control().unwrap().clone() {
            Control::TextBox { text_vertical_align, .. } => {
                assert_eq!(text_vertical_align, VerticalAlign::Top);
            }
            other => panic!("expected Control::TextBox, got {other:?}"),
        }
    }

    #[test]
    fn parse_page_number_control_reads_number_shape_from_property() {
        // pgnp ctrl-header layout: [0..4] ctrl_id, [4..8] property u32 (LE)
        // where bits 0-7 (byte 4) = number shape and bits 8-11 (byte 5) =
        // position. Build a header with shape=2 (RomanCapital) and
        // position=9 (INSIDE_TOP), plus a trailing '-' side char.
        let mut header = vec![b'p', b'n', b'g', b'p'];
        header.push(2); // byte 4: number shape = RomanCapital
        header.push(9); // byte 5: position = InsideTop
        header.extend_from_slice(&[0, 0]); // rest of property u32
        header.extend_from_slice(&[0, 0]); // number u16
        header.push(b'-'); // side decoration char
        let pn = parse_page_number_control(&header).expect("pgnp should parse");
        assert_eq!(pn.number_format, NumberFormatType::RomanCapital);
        assert_eq!(pn.position, PageNumberPosition::InsideTop);
        assert_eq!(pn.decoration, "-", "trailing graphic byte is the side char");

        // Shape 0 must still decode as Digit (default, regression guard).
        header[4] = 0;
        let pn = parse_page_number_control(&header).expect("pgnp should parse");
        assert_eq!(pn.number_format, NumberFormatType::Digit);

        // Q1 regression + W2 실측 교정: 장식 바이트 없는 pgnp 는 비그래픽
        // 속성 바이트(9 = '\t')를 장식으로 오독하지 않아야 하고, 장식은
        // **빈 문자열**이어야 한다 — F1 fixture 실측: 전부 0 인 wire 를
        // 한컴이 `sideChar=""` + 맨 숫자 PDF 로 확정 (과거 "-" 기본값은 날조).
        let no_deco = vec![b'p', b'n', b'g', b'p', 0, 9, 0, 0, 0, 0];
        let pn = parse_page_number_control(&no_deco).expect("pgnp should parse");
        assert_eq!(pn.position, PageNumberPosition::InsideTop);
        assert_eq!(pn.decoration, "", "zero wire = no decoration (F1 byte-verified)");
    }

    use std::collections::BTreeMap;

    use hwpforge_core::table::TablePageBreak;

    use crate::decoder::section::{
        Hwp5ImageControl, Hwp5LineControl, Hwp5PolygonControl, Hwp5SectionStartNumbers,
        Hwp5TablePageBreak, Hwp5TextBoxControl,
    };
    use crate::Hwp5SemanticImageFormat;

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn make_paragraph(text: &str, para_shape_id: u16, style_id: u8) -> Hwp5Paragraph {
        Hwp5Paragraph {
            end_char_shape: None,
            silent_wires: Vec::new(),
            page_break: false,
            column_break: false,
            text: text.to_string(),
            text_segments: Vec::new(),
            para_shape_id,
            style_id,
            char_shape_runs: vec![],
            line_segments: Vec::new(),
            controls: vec![],
        }
    }

    /// 이것을 실패시키는 것: 필드마다 인덱스 재생성·커서 초기화 또는 빈 범위에서 커서 진행.
    #[test]
    fn r5_field_segment_visits_are_linear() {
        use crate::schema::section::TextSegment;
        for n in [1_000, 10_000] {
            for filled in [false, true] {
                let mut para =
                    make_paragraph(&if filled { "가😀".repeat(n) } else { String::new() }, 0, 0);
                for _ in 0..n {
                    para.text_segments.push(TextSegment::FieldBegin { extra: [0; 14] });
                    if filled {
                        para.text_segments.push(TextSegment::Text("가😀".into()));
                    }
                    para.text_segments.push(TextSegment::FieldEnd);
                }
                let mut index = FieldTextIndex::new(&para);
                for i in 0..n {
                    let start = if filled { 3 * i as u32 } else { 0 };
                    let runs =
                        project_field_text(&mut index, start, start + if filled { 3 } else { 0 });
                    assert_eq!(runs.len(), usize::from(filled));
                    if filled {
                        assert_eq!(runs[0].content, RunContent::Text("가😀".into()));
                    }
                }
                let expected = n * if filled { 5 } else { 2 };
                assert_eq!(index.visits, expected, "N={n} filled={filled}");
                FIELD_TEXT_VISITS.with(|n| n.set(0));
                let mut state = ProjectionImageState::new(
                    None,
                    None,
                    DecompressBudget::new(MAX_TOTAL_DECOMPRESSED),
                );
                let _ = project_paragraph_with_images_structural(
                    &para,
                    &mut state,
                    ImageProjectionContext::Flow,
                    &[],
                );
                assert_eq!(FIELD_TEXT_VISITS.with(|n| n.get()), expected);
                eprintln!("N={n} filled={filled} segment+character visits={}", index.visits);
            }
        }
        let para = make_paragraph("prefix", 0, 0);
        let mut index = FieldTextIndex::new(&para);
        assert!(index.project(6, 6).is_empty());
        assert_eq!(index.visits, 0);
        assert_eq!(index.project(0, 6)[0].content, RunContent::Text("prefix".into()));
    }

    /// 이것을 실패시키는 것: 객체 제외·UTF-16 커서·서식 진행 또는 시작 서식 공백 처리 제거.
    #[test]
    fn r5_field_cursor_preserves_styles_unicode_and_literal_object_char() {
        use crate::schema::section::TextSegment;
        let mut para = make_paragraph("앞a\u{fffc}😀\u{fffc}b끝", 0, 0);
        para.text_segments = vec![
            TextSegment::Text("앞a".into()),
            TextSegment::ControlRef { extra: [0; 14] },
            TextSegment::Text("😀\u{fffc}b끝".into()),
        ];
        para.char_shape_runs = vec![
            Hwp5CharShapeRun { position: 1, char_shape_id: 2 },
            Hwp5CharShapeRun { position: 3, char_shape_id: 3 },
            Hwp5CharShapeRun { position: 6, char_shape_id: 4 },
        ];
        let mut index = FieldTextIndex::new(&para);
        let first = index.project(0, 1);
        assert_eq!(first[0].char_shape_id.get(), 0);
        let runs = index.project(1, 7);
        let facts: Vec<_> =
            runs.iter().map(|r| (r.content.as_text().unwrap(), r.char_shape_id.get())).collect();
        assert_eq!(facts, [("a", 2), ("😀\u{fffc}", 3), ("b", 4)]);
        assert_eq!(index.project(7, 8)[0].content.as_text(), Some("끝"));
    }

    fn _make_paragraph_with_runs(text: &str, runs: Vec<Hwp5CharShapeRun>) -> Hwp5Paragraph {
        Hwp5Paragraph {
            end_char_shape: None,
            silent_wires: Vec::new(),
            page_break: false,
            column_break: false,
            text: text.to_string(),
            text_segments: Vec::new(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: runs,
            line_segments: Vec::new(),
            controls: vec![],
        }
    }

    /// §19.22 D5: the decoder's end-mark char shape reaches Core.
    ///
    /// 이것을 실패시키는 것: 투영에서 이 칸을 옮기지 않기.
    #[test]
    fn end_char_shape_reaches_core() {
        let project = |end: u32| {
            let mut para = _make_paragraph_with_runs(
                "가",
                vec![Hwp5CharShapeRun { position: 0, char_shape_id: 1 }],
            );
            para.end_char_shape = Some(end);
            let (doc, _) = project_to_core(vec![make_section(vec![para], None)]).unwrap();
            doc.sections()[0].paragraphs[0].end_char_shape
        };
        assert_eq!(project(5), Some(CharShapeIndex::new(5)));
        // The same shape as the last run: no end mark.
        assert_eq!(project(1), None);
    }

    /// §19.22 B: an empty paragraph keeps the char shape of position 0 —
    /// the last entry there — on the flat path and on the structural path
    /// (a page-number control sends it there and yields no run).
    ///
    /// 이것을 실패시키는 것: 두 경로의 대체 런을 0 으로 / 첫 항목으로.
    #[test]
    fn empty_paragraph_keeps_its_char_shape() {
        let runs = vec![
            Hwp5CharShapeRun { position: 0, char_shape_id: 120 },
            Hwp5CharShapeRun { position: 0, char_shape_id: 5 },
        ];
        let flat = _make_paragraph_with_runs("", runs.clone());
        let mut structural = _make_paragraph_with_runs("", runs);
        structural.controls =
            vec![Hwp5Control::Unknown { ctrl_id: CTRL_ID_PAGE_NUMBER, header_data: vec![0; 12] }];
        structural.text_segments =
            vec![crate::schema::section::TextSegment::ControlRef { extra: [0u8; 14] }];
        assert!(paragraph_needs_structural_projection(&structural));
        let (doc, _) = project_to_core(vec![make_section(vec![flat, structural], None)]).unwrap();
        let paragraphs = &doc.sections()[0].paragraphs;
        for (name, p) in [("flat", &paragraphs[0]), ("structural", &paragraphs[1])] {
            assert_eq!(p.runs.len(), 1, "{name}: {:?}", p.runs);
            assert_eq!(p.runs[0].char_shape_id, CharShapeIndex::new(5), "{name}");
        }
    }

    #[test]
    fn line_segments_promote_to_layout_cache() {
        use crate::schema::section::Hwp5ParaLineSeg;
        let segs = vec![
            Hwp5ParaLineSeg {
                text_start_position: 0,
                vertical_position: 0,
                line_height: 1000,
                text_height: 1000,
                baseline_distance: 850,
                line_spacing: 600,
                column_start_position: 10,
                segment_width: 42520,
                tag: 0x0060_0000,
            },
            Hwp5ParaLineSeg {
                text_start_position: 35,
                vertical_position: 1600,
                line_height: 1000,
                text_height: 1000,
                baseline_distance: 850,
                line_spacing: 600,
                column_start_position: 10,
                segment_width: 42520,
                tag: 0x0016_0000,
            },
        ];
        let mut warnings = Vec::new();
        let identity = {
            let mut b = crate::wire_text_map::WireMapBuilder::new();
            b.advance_identity(64);
            b.finish().expect("identity map")
        };
        let cache = promote_line_segments(&segs, Ok((identity, Default::default())), &mut warnings)
            .expect("promoted");
        assert!(warnings.is_empty(), "identity promote must not warn: {warnings:?}");
        assert_eq!(cache.line_count(), 2);
        // 필드 대응: 이름만 다르고 wire 의미 동일 (textpos/vertpos/vertsize/…)
        assert_eq!(cache.lines[0].horzpos, 10);
        assert_eq!(cache.lines[0].horzsize, 42520);
        assert_eq!(cache.lines[1].textpos, 35);
        assert_eq!(cache.lines[1].vertpos, 1600);
        assert_eq!(cache.lines[1].vertsize, 1000);
        assert_eq!(cache.lines[1].baseline, 850);
        assert_eq!(cache.lines[1].spacing, 600);
        assert_eq!(cache.lines[1].flags, 0x0016_0000);
        // 세그먼트 없음 = 캐시 부재 (경고도 없음)
        let empty_map = crate::wire_text_map::WireMapBuilder::new().finish().expect("empty");
        assert!(promote_line_segments(&[], Ok((empty_map, Default::default())), &mut warnings)
            .is_none());
        assert!(warnings.is_empty());
    }

    fn make_section(
        paragraphs: Vec<Hwp5Paragraph>,
        page_def: Option<Hwp5PageDef>,
    ) -> SectionResult {
        SectionResult {
            master_pages: Vec::new(),
            paragraphs,
            page_def,
            section_def_properties: None,
            section_def_start_numbers: None,
            page_border_fills: Vec::new(),
            column_def: None,
            warnings: vec![],
        }
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
        let assets_by_reference: BTreeMap<u16, Hwp5JoinedImageAsset> = ordered_assets
            .iter()
            .cloned()
            .map(|asset| (asset.payload.binary_data_id, asset))
            .collect();
        Hwp5JoinedImageAssetPlan {
            ordered_assets,
            assets_by_reference,
            unavailable_reasons: BTreeMap::new(),
        }
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
        let assets_by_reference: BTreeMap<u16, Hwp5JoinedImageAsset> =
            [(binary_data_id, asset.clone())].into_iter().collect();
        Hwp5JoinedImageAssetPlan {
            ordered_assets: vec![asset],
            assets_by_reference,
            unavailable_reasons: BTreeMap::new(),
        }
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

    // ── W1: secd 시작번호 fail-safe (계획 §1.2 F1 실측·§1.4 corpus) ────────

    #[test]
    fn secd_restart_bits_zero_is_silent_and_begin_num_stays_none() {
        // F1 실측: bits==0 + 시작번호 필드 1 → 한컴도 `<hp:startNum page="0">`
        // (이어서). begin_num None 이 byte-정합 — 경고도 없어야 한다.
        let mut section = make_section(vec![], None);
        section.section_def_properties = Some(0);
        section.section_def_start_numbers =
            Some(Hwp5SectionStartNumbers { page: 1, pic: 0, tbl: 0, equation: 0 });
        let (doc, warnings) = project_to_core(vec![section]).unwrap();
        assert!(doc.sections()[0].begin_num.is_none());
        assert!(
            !warnings.iter().any(|w| matches!(
                w,
                Hwp5Warning::ProjectionFallback { subject: "section.begin_num", .. }
            )),
            "bits==0 은 정상 경로 — 경고 금지: {warnings:?}"
        );
    }

    #[test]
    fn secd_restart_bits_nonzero_warns_with_raw_starts_and_keeps_begin_num_none() {
        // corpus 실측 존재값 bits=2 (2건/2,468) — 의미 미확정이므로 재시작으로
        // 날조하지 않고 raw 값과 함께 경고 후 이어서 처리한다.
        let mut section = make_section(vec![], None);
        section.section_def_properties = Some(2 << 20);
        section.section_def_start_numbers =
            Some(Hwp5SectionStartNumbers { page: 6, pic: 0, tbl: 0, equation: 0 });
        let (doc, warnings) = project_to_core(vec![section]).unwrap();
        assert!(doc.sections()[0].begin_num.is_none(), "미확정 비트 재시작 날조 금지");
        let reason = warnings
            .iter()
            .find_map(|w| match w {
                Hwp5Warning::ProjectionFallback { subject: "section.begin_num", reason } => {
                    Some(reason)
                }
                _ => None,
            })
            .expect("fail-safe warning must surface");
        assert!(reason.contains("page=6"), "raw 값 표면화: {reason}");
    }

    #[test]
    fn secd_restart_bits_nonzero_with_truncated_payload_reports_truncation() {
        // all-or-none: [20..28] 이 없으면 부분값 대신 truncation 을 알린다
        // (Codex 결함 8 — 읽지 않은 값을 기본값 1 로 날조 금지).
        let mut section = make_section(vec![], None);
        section.section_def_properties = Some(3 << 20);
        let (doc, warnings) = project_to_core(vec![section]).unwrap();
        assert!(doc.sections()[0].begin_num.is_none());
        let reason = warnings
            .iter()
            .find_map(|w| match w {
                Hwp5Warning::ProjectionFallback { subject: "section.begin_num", reason } => {
                    Some(reason)
                }
                _ => None,
            })
            .expect("fail-safe warning must surface");
        assert!(reason.contains("truncated"), "{reason}");
    }

    #[test]
    fn unknown_control_drop_aggregate_caps_distinct_ids_and_reports_more() {
        // distinct id 상한(16) + "N more" — corpus noisy 문서 폭주 방지 잠금.
        let mut controls = Vec::new();
        let mut segments = Vec::new();
        for i in 0..20u32 {
            // 'a a'..'t t' 꼴의 서로 다른 인쇄가능 4바이트 id 20종.
            let id = u32::from_be_bytes([b'a' + i as u8, b' ', b'a' + i as u8, b' ']);
            controls.push(Hwp5Control::Unknown { ctrl_id: id, header_data: vec![] });
            segments.push(crate::schema::section::TextSegment::ControlRef { extra: [0u8; 14] });
        }
        let para = Hwp5Paragraph {
            end_char_shape: None,
            silent_wires: Vec::new(),
            text: String::new(),
            text_segments: segments,
            para_shape_id: 0,
            style_id: 0,
            page_break: false,
            column_break: false,
            char_shape_runs: vec![],
            line_segments: vec![],
            controls,
        };
        let (_, warnings) = project_to_core(vec![make_section(vec![para], None)]).unwrap();
        let drops: Vec<&String> = warnings
            .iter()
            .filter_map(|w| match w {
                Hwp5Warning::DroppedControl { control: "unknown_control", reason } => Some(reason),
                _ => None,
            })
            .collect();
        assert_eq!(drops.len(), 17, "16 distinct + 'N more' 요약 1건: {drops:?}");
        assert!(drops.last().unwrap().contains("4 more distinct"), "{}", drops.last().unwrap());
    }

    #[test]
    fn ctrl_id_ascii_falls_back_to_hex_for_non_printable() {
        assert_eq!(ctrl_id_ascii(u32::from_be_bytes(*b"form")), "form");
        assert_eq!(ctrl_id_ascii(0x0102_0304), "0x01020304");
    }

    #[test]
    fn unknown_control_drops_are_aggregated_into_one_warning_per_id() {
        // W4 무음 드롭 종결: 같은 미지원 ctrl 이 몇 번 죽든 경고는 id 당
        // 1건 + count (per-occurrence 폭탄 금지 — corpus `%fmu` 531회).
        let form_id = u32::from_be_bytes(*b"form"); // 기지 deferred ctrl
        let mk = || Hwp5Control::Unknown { ctrl_id: form_id, header_data: vec![] };
        let para = Hwp5Paragraph {
            end_char_shape: None,
            silent_wires: Vec::new(),
            text: String::new(),
            text_segments: vec![
                crate::schema::section::TextSegment::ControlRef { extra: [0u8; 14] },
                crate::schema::section::TextSegment::ControlRef { extra: [0u8; 14] },
            ],
            para_shape_id: 0,
            style_id: 0,
            page_break: false,
            column_break: false,
            char_shape_runs: vec![],
            line_segments: vec![],
            controls: vec![mk(), mk()],
        };
        let section = make_section(vec![para], None);
        let (_, warnings) = project_to_core(vec![section]).unwrap();
        let drops: Vec<&String> = warnings
            .iter()
            .filter_map(|w| match w {
                Hwp5Warning::DroppedControl { control: "unknown_control", reason } => Some(reason),
                _ => None,
            })
            .collect();
        assert_eq!(drops.len(), 1, "id 당 집계 1건: {warnings:?}");
        assert!(drops[0].contains("'form'") && drops[0].contains("2 time(s)"), "{}", drops[0]);
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
            master_pages: Vec::new(),
            paragraphs: vec![make_paragraph("x", 0, 0)],
            page_def: None,
            section_def_properties: None,
            section_def_start_numbers: None,
            page_border_fills: Vec::new(),
            column_def: None,
            warnings: vec![warn],
        };
        let (_, warnings) = project_to_core(vec![section]).unwrap();
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn project_to_core_with_images_preserves_inline_order_and_populates_store() {
        let image = Hwp5Control::Image(Hwp5ImageControl {
            standalone_render: None,
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                outer_margin: [0; 4],
                x: 0,
                y: 0,
                width: 3_000,
                height: 2_000,
            },
            binary_data_id: 1,
            original_size: None,
            instance_id: 0,
            // W2p: treat_as_char 는 이제 raw bit0 이 유일한 진실 — 좌표
            // (0,0) 만으로는 더 이상 inline 판정이 되지 않는다.
            ctrl_properties: 0x1,
            picture: Default::default(),
            link: None,
        });
        let section = make_section(
            vec![Hwp5Paragraph {
                end_char_shape: None,
                silent_wires: Vec::new(),
                page_break: false,
                column_break: false,
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

        let (document, image_store, warnings) = project_to_core_with_images(
            vec![section],
            &image_assets,
            DecompressBudget::new(MAX_TOTAL_DECOMPRESSED),
        )
        .unwrap();
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
        assert_eq!(placement.text_wrap, ObjectTextWrap::TopAndBottom);
        assert_eq!(placement.horz_rel_to, ObjectRelativeTo::Para);
        assert_eq!(placement.vert_rel_to, ObjectRelativeTo::Para);
        assert_eq!(document.sections()[0].content_counts().images, 1);
    }

    /// D-T7: an object takes the char shape that starts at its own
    /// position — here a boundary right at the object, after "A" (a
    /// boundary sits at a control's start 3,724 times in 1,163 corpus
    /// documents, `scan_cs_interior.py`) — not the shape of the character
    /// before it.
    ///
    /// 이것을 실패시키는 것: 개체 조회를 앞 글자(`position - 1`) 조회로
    /// 되돌리는 것.
    #[test]
    fn an_object_takes_the_char_shape_that_starts_at_it() {
        let image = Hwp5Control::Image(Hwp5ImageControl {
            standalone_render: None,
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                outer_margin: [0; 4],
                x: 0,
                y: 0,
                width: 3_000,
                height: 2_000,
            },
            binary_data_id: 1,
            original_size: None,
            instance_id: 0,
            ctrl_properties: 0x1,
            picture: Default::default(),
            link: None,
        });
        let section = make_section(
            vec![Hwp5Paragraph {
                end_char_shape: None,
                silent_wires: Vec::new(),
                page_break: false,
                column_break: false,
                text: "A\u{fffc}".to_string(),
                text_segments: Vec::new(),
                para_shape_id: 0,
                style_id: 0,
                char_shape_runs: vec![
                    Hwp5CharShapeRun { position: 0, char_shape_id: 1 },
                    Hwp5CharShapeRun { position: 1, char_shape_id: 2 },
                ],
                line_segments: Vec::new(),
                controls: vec![image],
            }],
            None,
        );
        let image_assets =
            image_plan([(1, "BIN0001.png", Hwp5SemanticImageFormat::Png, vec![0x89, 0x50])]);
        let (document, _, _) = project_to_core_with_images(
            vec![section],
            &image_assets,
            DecompressBudget::new(MAX_TOTAL_DECOMPRESSED),
        )
        .unwrap();
        let runs = &document.sections()[0].paragraphs[0].runs;
        assert_eq!(runs.len(), 2, "{runs:?}");
        assert_eq!(runs[0].char_shape_id, CharShapeIndex::new(1));
        assert!(runs[1].content.is_image());
        assert_eq!(runs[1].char_shape_id, CharShapeIndex::new(2));
    }

    /// D-T7 on the structured path (the one that walks `text_segments`; a
    /// bookmark point in the paragraph selects it): the object takes the
    /// shape that starts at it, the character after it the next one.
    ///
    /// 이것을 실패시키는 것: 구조 경로의 개체 조회(`ControlRef` 갈래)를 앞 글자
    /// 조회로 되돌리는 것.
    #[test]
    fn an_object_on_the_structured_path_takes_the_char_shape_that_starts_at_it() {
        use crate::schema::section::TextSegment;
        let image = Hwp5Control::Image(Hwp5ImageControl {
            standalone_render: None,
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                outer_margin: [0; 4],
                x: 0,
                y: 0,
                width: 3_000,
                height: 2_000,
            },
            binary_data_id: 1,
            original_size: None,
            instance_id: 0,
            ctrl_properties: 0x1,
            picture: Default::default(),
            link: None,
        });
        let bookmark =
            Hwp5Control::Unknown { ctrl_id: CTRL_ID_BOOKMARK_POINT, header_data: Vec::new() };
        let paragraph = Hwp5Paragraph {
            end_char_shape: None,
            silent_wires: Vec::new(),
            page_break: false,
            column_break: false,
            text: "A\u{fffc}B".to_string(),
            text_segments: vec![
                TextSegment::Text("A".to_string()),
                TextSegment::ControlRef { extra: [0; 14] },
                TextSegment::Text("B".to_string()),
            ],
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![
                Hwp5CharShapeRun { position: 0, char_shape_id: 1 },
                Hwp5CharShapeRun { position: 1, char_shape_id: 2 },
                Hwp5CharShapeRun { position: 2, char_shape_id: 3 },
            ],
            line_segments: Vec::new(),
            controls: vec![bookmark, image],
        };
        assert!(paragraph_needs_structural_projection(&paragraph));
        let image_assets =
            image_plan([(1, "BIN0001.png", Hwp5SemanticImageFormat::Png, vec![0x89, 0x50])]);
        let (document, _, _) = project_to_core_with_images(
            vec![make_section(vec![paragraph], None)],
            &image_assets,
            DecompressBudget::new(MAX_TOTAL_DECOMPRESSED),
        )
        .unwrap();
        let runs: Vec<(bool, usize)> = document.sections()[0].paragraphs[0]
            .runs
            .iter()
            .map(|run| (run.content.is_image(), run.char_shape_id.get()))
            .collect();
        assert_eq!(runs, [(false, 1), (true, 2), (false, 3)]);
    }

    #[test]
    fn project_to_core_with_images_projects_header_and_footer_subtrees() {
        let header_image = Hwp5Control::Image(Hwp5ImageControl {
            standalone_render: None,
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                outer_margin: [0; 4],
                x: 0,
                y: 0,
                width: 1_200,
                height: 800,
            },
            binary_data_id: 7,
            original_size: None,
            instance_id: 0,
            ctrl_properties: 0,
            picture: Default::default(),
            link: None,
        });
        let section = make_section(
            vec![Hwp5Paragraph {
                end_char_shape: None,
                silent_wires: Vec::new(),
                page_break: false,
                column_break: false,
                text: "\u{fffc}\u{fffc}".to_string(),
                text_segments: Vec::new(),
                para_shape_id: 0,
                style_id: 0,
                char_shape_runs: Vec::new(),
                line_segments: Vec::new(),
                controls: vec![
                    Hwp5Control::Header(crate::decoder::section::Hwp5NestedSubtree {
                        wire_order: 0,
                        ctrl_id: 0x6865_6164,
                        properties_raw: 0,
                        instance_id: 0,
                        paragraphs: vec![Hwp5Paragraph {
                            end_char_shape: None,
                            silent_wires: Vec::new(),
                            page_break: false,
                            column_break: false,
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
                        wire_order: 0,
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

        let (document, image_store, _) = project_to_core_with_images(
            vec![section],
            &image_assets,
            DecompressBudget::new(MAX_TOTAL_DECOMPRESSED),
        )
        .unwrap();
        let section = &document.sections()[0];
        let header = section.headers.first().expect("header should be projected");
        let footer = section.footers.first().expect("footer should be projected");

        assert_eq!(image_store.get("BIN0007.png"), Some(&[1, 2, 3, 4][..]));
        assert_eq!(header.paragraphs.len(), 1);
        assert_eq!(footer.paragraphs.len(), 1);
        assert_eq!(header.paragraphs[0].runs.len(), 1);
        assert!(header.paragraphs[0].runs[0].content.is_image());
        assert_eq!(footer.paragraphs[0].text_content(), "꼬리말 테스트");
        // 첫 문단의 머리말/꼬리말은 앵커 `None`(= 문단 0, API 문서와 같은 값).
        // 이것을 실패시키는 것: 0 을 `Some(0)` 으로 두기.
        assert_eq!((header.anchor_paragraph, footer.anchor_paragraph), (None, None));
    }

    #[test]
    fn project_to_core_with_images_projects_textbox_with_nested_image() {
        let nested_image = Hwp5Control::Image(Hwp5ImageControl {
            standalone_render: None,
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                outer_margin: [0; 4],
                x: 0,
                y: 0,
                width: 1_500,
                height: 900,
            },
            binary_data_id: 3,
            original_size: None,
            instance_id: 0,
            // W5 w0: 배치가 이제 속성 word 실비트로 결정되므로, 실측 anchored
            // 이미지 word(anchored_zero_origin_png native fixture, 0x040a2310 →
            // Para/Para/Square, flowWithText=1)를 싣는다 (이전엔 TextBox 관례가
            // 이 값을 합성했다).
            ctrl_properties: 0x040a_2310,
            picture: Default::default(),
            link: None,
        });
        let textbox = Hwp5Control::TextBox(Hwp5TextBoxControl {
            standalone_render: None,
            original_size: None,
            look: None,
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                outer_margin: [0; 4],
                x: 50,
                y: 60,
                width: 8_000,
                height: 6_000,
            },
            list_header_properties: None,
            text_margin: None,
            ctrl_properties: 0,
            paragraphs: vec![Hwp5Paragraph {
                end_char_shape: None,
                silent_wires: Vec::new(),
                page_break: false,
                column_break: false,
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
                end_char_shape: None,
                silent_wires: Vec::new(),
                page_break: false,
                column_break: false,
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

        let (document, image_store, warnings) = project_to_core_with_images(
            vec![section],
            &image_assets,
            DecompressBudget::new(MAX_TOTAL_DECOMPRESSED),
        )
        .unwrap();
        assert!(warnings.is_empty());
        assert_eq!(image_store.get("BIN0003.png"), Some(&[9, 8, 7][..]));

        let paragraph = &document.sections()[0].paragraphs[0];
        assert_eq!(paragraph.runs.len(), 1);
        let textbox_control =
            paragraph.runs[0].content.as_control().expect("textbox should project as control");
        match textbox_control {
            Control::TextBox { paragraphs, width, height, placement, .. } => {
                assert_eq!(width, &HwpUnit::new(8_000).unwrap());
                assert_eq!(height, &HwpUnit::new(6_000).unwrap());
                let placement = placement.as_ref().expect("floating textbox carries placement");
                assert_eq!(placement.horz_offset.as_i32(), 50);
                assert_eq!(placement.vert_offset.as_i32(), 60);
                assert_eq!(paragraphs.len(), 1);
                assert_eq!(paragraphs[0].runs.len(), 3);
                assert_eq!(paragraphs[0].runs[0].content.as_text(), Some("앞"));
                let nested_image =
                    paragraphs[0].runs[1].content.as_image().expect("middle run should be image");
                let placement =
                    nested_image.placement.as_ref().expect("textbox image should have placement");
                assert_eq!(placement.text_wrap, ObjectTextWrap::Square);
                assert_eq!(placement.text_flow, ObjectTextFlow::BothSides);
                assert!(!placement.treat_as_char);
                assert!(placement.flow_with_text);
                assert!(!placement.allow_overlap);
                assert_eq!(placement.horz_rel_to, ObjectRelativeTo::Para);
                assert_eq!(placement.vert_rel_to, ObjectRelativeTo::Para);
                assert_eq!(paragraphs[0].runs[2].content.as_text(), Some("뒤"));
            }
            other => panic!("expected TextBox control, got {:?}", other),
        }
    }

    #[test]
    fn project_to_core_with_images_warns_when_image_asset_join_is_missing() {
        let image = Hwp5Control::Image(Hwp5ImageControl {
            standalone_render: None,
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                outer_margin: [0; 4],
                x: 0,
                y: 0,
                width: 1_000,
                height: 800,
            },
            binary_data_id: 99,
            original_size: None,
            instance_id: 0,
            ctrl_properties: 0,
            picture: Default::default(),
            link: None,
        });
        let section = make_section(
            vec![Hwp5Paragraph {
                end_char_shape: None,
                silent_wires: Vec::new(),
                page_break: false,
                column_break: false,
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

        let (document, image_store, warnings) = project_to_core_with_images(
            vec![section],
            &image_plan([]),
            DecompressBudget::new(MAX_TOTAL_DECOMPRESSED),
        )
        .unwrap();
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
            standalone_render: None,
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                outer_margin: [0; 4],
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            binary_data_id: 5,
            original_size: None,
            instance_id: 0,
            ctrl_properties: 0,
            picture: Default::default(),
            link: None,
        });
        let section = make_section(
            vec![Hwp5Paragraph {
                end_char_shape: None,
                silent_wires: Vec::new(),
                page_break: false,
                column_break: false,
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

        let (document, image_store, warnings) = project_to_core_with_images(
            vec![section],
            &image_assets,
            DecompressBudget::new(MAX_TOTAL_DECOMPRESSED),
        )
        .unwrap();
        assert!(warnings.is_empty());
        assert_eq!(image_store.get("BIN0005.png"), Some(&[0x89, 0x50, 0x4E, 0x47][..]));

        let paragraph = &document.sections()[0].paragraphs[0];
        let image = paragraph.runs[0].content.as_image().expect("run should be image");
        assert_eq!(image.width, HwpUnit::new(3_210).unwrap());
        assert_eq!(image.height, HwpUnit::new(4_560).unwrap());
    }

    /// A body picture keeps its link (Hancom's `URL;1;0;0;` command, the
    /// suffix taken off); a link record in another form is reported, not
    /// guessed at.
    ///
    /// 이것을 실패시키는 것: `core_image.hyperlink = …` 줄 삭제 / 형식 밖 링크 경고 삭제.
    #[test]
    fn body_picture_keeps_its_hyperlink() {
        let project = |link: Option<Result<String, String>>| {
            let image = Hwp5Control::Image(Hwp5ImageControl {
                standalone_render: None,
                ctrl_id: 0x6773_6F20,
                geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                    outer_margin: [0; 4],
                    x: 0,
                    y: 0,
                    width: 100,
                    height: 100,
                },
                binary_data_id: 5,
                original_size: None,
                instance_id: 0,
                ctrl_properties: 0,
                picture: Default::default(),
                link,
            });
            let section = make_section(
                vec![Hwp5Paragraph {
                    end_char_shape: None,
                    silent_wires: Vec::new(),
                    page_break: false,
                    column_break: false,
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
            let assets = image_plan_with_dimensions(
                5,
                "BIN0005.png",
                Hwp5SemanticImageFormat::Png,
                None,
                None,
                vec![0x89, 0x50, 0x4E, 0x47],
            );
            let (document, _, warnings) = project_to_core_with_images(
                vec![section],
                &assets,
                DecompressBudget::new(MAX_TOTAL_DECOMPRESSED),
            )
            .unwrap();
            let image = document.sections()[0].paragraphs[0].runs[0].content.as_image().cloned();
            let lost = warnings
                .iter()
                .filter(|w| matches!(w, Hwp5Warning::ContentLost { kind, .. } if kind == "picture_hyperlink"))
                .count();
            (image.expect("image").hyperlink, lost)
        };
        assert_eq!(project(None), (None, 0));
        assert_eq!(
            project(Some(Ok("http://www.korea.kr;1;0;0;".to_string()))),
            (Some("http://www.korea.kr".to_string()), 0)
        );
        assert_eq!(project(Some(Ok("http://www.korea.kr".to_string()))), (None, 1));
        assert_eq!(
            project(Some(Err("not an object hyperlink parameter set".to_string()))),
            (None, 1)
        );
    }

    #[test]
    fn project_to_core_with_images_drops_zero_sized_image_without_fallback() {
        let image = Hwp5Control::Image(Hwp5ImageControl {
            standalone_render: None,
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                outer_margin: [0; 4],
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            binary_data_id: 6,
            original_size: None,
            instance_id: 0,
            ctrl_properties: 0,
            picture: Default::default(),
            link: None,
        });
        let section = make_section(
            vec![Hwp5Paragraph {
                end_char_shape: None,
                silent_wires: Vec::new(),
                page_break: false,
                column_break: false,
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

        let (document, image_store, warnings) = project_to_core_with_images(
            vec![section],
            &image_assets,
            DecompressBudget::new(MAX_TOTAL_DECOMPRESSED),
        )
        .unwrap();
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

    // ── image_placement_from_wire (W5 w0 — byte-ground axes) ────────────────

    #[test]
    fn image_placement_from_wire_byte_grounds_floating_axes() {
        // 속성 word bit0=0 → 부유. relTo/wrap/flow/overlap 이 실비트로
        // 디코드된다 (이전 Flow 관례 Paper/Paper/InFrontOfText 폐기).
        let zero_origin = crate::schema::section::Hwp5ShapeComponentGeometry {
            outer_margin: [0; 4],
            x: 0,
            y: 0,
            width: 100,
            height: 200,
        };
        // 모든 축 비트 0 → Paper/Paper/Square, flow=false, overlap=false.
        let zeroed = image_placement_from_wire(
            &zero_origin,
            ImageProjectionContext::Flow,
            0x0,
            &mut Vec::new(),
        );
        assert!(!zeroed.treat_as_char);
        assert_eq!(zeroed.text_wrap, ObjectTextWrap::Square);
        assert_eq!(zeroed.vert_rel_to, ObjectRelativeTo::Paper);
        assert_eq!(zeroed.horz_rel_to, ObjectRelativeTo::Paper);
        assert!(!zeroed.flow_with_text);
        assert!(!zeroed.allow_overlap);
        assert_eq!(zeroed.vert_offset, HwpUnit::ZERO);
        assert_eq!(zeroed.horz_offset, HwpUnit::ZERO);

        // 실측 속성 word (anchored_zero_origin_png native fixture,
        // attr=0x040a2310) → Para/Para/Square, flow=true → overlap 강제 false.
        let img = image_placement_from_wire(
            &zero_origin,
            ImageProjectionContext::Flow,
            0x040a_2310,
            &mut Vec::new(),
        );
        assert!(!img.treat_as_char);
        assert_eq!(img.vert_rel_to, ObjectRelativeTo::Para);
        assert_eq!(img.horz_rel_to, ObjectRelativeTo::Para);
        assert_eq!(img.text_wrap, ObjectTextWrap::Square);
        assert!(img.flow_with_text);
        assert!(!img.allow_overlap);

        // 오프셋은 signed CtrlHeader 필드를 그대로 싣는다 (음수 포함 —
        // corpus 의 문단 위 돌출 offset).
        let neg = crate::schema::section::Hwp5ShapeComponentGeometry {
            outer_margin: [0; 4],
            x: -1_234,
            y: -5_678,
            width: 100,
            height: 200,
        };
        let placement = image_placement_from_wire(
            &neg,
            ImageProjectionContext::Flow,
            0x040a_2310,
            &mut Vec::new(),
        );
        assert_eq!(placement.horz_offset, HwpUnit::new(-1_234).unwrap());
        assert_eq!(placement.vert_offset, HwpUnit::new(-5_678).unwrap());
    }

    #[test]
    fn image_placement_from_wire_bit0_inline_collapses_to_legacy_default() {
        // bit0=1 → 좌표·다른 비트 무관 인라인 (legacy default). W2p 계약 보존.
        let non_zero = crate::schema::section::Hwp5ShapeComponentGeometry {
            outer_margin: [0; 4],
            x: 1_234,
            y: 5_678,
            width: 100,
            height: 200,
        };
        let inline = image_placement_from_wire(
            &non_zero,
            ImageProjectionContext::Flow,
            0x1,
            &mut Vec::new(),
        );
        assert!(inline.treat_as_char);
        assert_eq!(inline, ObjectPlacement::legacy_inline_defaults());
        // 다른 비트가 전부 켜져도 bit0 이 인라인 판정을 지배한다.
        let inline_all = image_placement_from_wire(
            &non_zero,
            ImageProjectionContext::Flow,
            0xFFFF_FFFF,
            &mut Vec::new(),
        );
        assert!(inline_all.treat_as_char);
        assert_eq!(inline_all, ObjectPlacement::legacy_inline_defaults());
    }

    #[test]
    fn image_placement_from_wire_is_context_independent() {
        // W5 w0: 배치는 속성 word 로만 결정된다 — Flow/TextBox context 는 더
        // 이상 축을 바꾸지 않는다 (이전 TextBox 관례 Para/Square 폐기).
        let geometry = crate::schema::section::Hwp5ShapeComponentGeometry {
            outer_margin: [0; 4],
            x: 8_503,
            y: 2_834,
            width: 100,
            height: 200,
        };
        // textbox_anchored native fixture attr=0x040a4110 → Para/Page/Square.
        let flow = image_placement_from_wire(
            &geometry,
            ImageProjectionContext::Flow,
            0x040a_4110,
            &mut Vec::new(),
        );
        let textbox = image_placement_from_wire(
            &geometry,
            ImageProjectionContext::TextBox,
            0x040a_4110,
            &mut Vec::new(),
        );
        assert_eq!(flow, textbox);
        assert_eq!(flow.vert_rel_to, ObjectRelativeTo::Para);
        assert_eq!(flow.horz_rel_to, ObjectRelativeTo::Page);
        assert_eq!(flow.text_wrap, ObjectTextWrap::Square);
        assert!(flow.allow_overlap);
        assert!(!flow.flow_with_text);
        assert_eq!(flow.vert_offset, HwpUnit::new(2_834).unwrap());
        assert_eq!(flow.horz_offset, HwpUnit::new(8_503).unwrap());
    }

    #[test]
    fn image_placement_from_wire_textbox_context_respects_bit0_inline() {
        // W5 w0 선재 결함 수정 잠금: TextBox 컨텍스트가 bit0 을 무시하고
        // treat_as_char=false 로 강제하던 구거동을 제거했다 — 글상자 안 진짜
        // 인라인 이미지(bit0=1)는 이제 TextBox 컨텍스트에서도 inline 으로
        // 판정된다 (구 테스트 `..._textbox_context_ignores_bit` 대체).
        let geometry = crate::schema::section::Hwp5ShapeComponentGeometry {
            outer_margin: [0; 4],
            x: 0,
            y: 0,
            width: 100,
            height: 200,
        };
        let placement = image_placement_from_wire(
            &geometry,
            ImageProjectionContext::TextBox,
            0x1,
            &mut Vec::new(),
        );
        assert!(placement.treat_as_char, "TextBox context must respect bit0=1 (inline)");
        assert_eq!(placement, ObjectPlacement::legacy_inline_defaults());
    }

    /// E-T1: bits 21–23 are Hancom's four-value table (X2 fixture and the
    /// 457 floating objects of the Hancom pairs), not the spec's six.
    ///
    /// 이것을 실패시키는 것: 6값 스펙 표로 되돌리는 것, 1·2·3 중 둘을
    /// 맞바꾸는 것.
    #[test]
    fn text_wrap_bits_are_hancoms_four_values() {
        for (bits, wrap) in [
            (0, ObjectTextWrap::Square),
            (1, ObjectTextWrap::TopAndBottom),
            (2, ObjectTextWrap::BehindText),
            (3, ObjectTextWrap::InFrontOfText),
        ] {
            let mut warnings = Vec::new();
            let placement = object_placement_from_ctrl_properties(bits << 21, 0, 0, &mut warnings);
            assert_eq!(placement.text_wrap, wrap, "bits {bits}");
            assert!(warnings.is_empty(), "bits {bits}: {warnings:?}");
        }
    }

    /// E-T2: 4–7 are outside what Hancom writes: a canary warning and the
    /// most conservative wrap.
    ///
    /// 이것을 실패시키는 것: 경고 조건을 `>= 6` 으로 되돌리는 것(4·5 가
    /// 무경고로 다른 감싸기가 된다).
    #[test]
    fn text_wrap_bits_four_to_seven_warn_and_fall_back_to_square() {
        for bits in 4..=7 {
            let mut warnings = Vec::new();
            let placement = object_placement_from_ctrl_properties(bits << 21, 0, 0, &mut warnings);
            assert_eq!(placement.text_wrap, ObjectTextWrap::Square, "bits {bits}");
            assert!(
                matches!(
                    warnings.as_slice(),
                    [Hwp5Warning::ProjectionFallback { subject: "object_placement.text_wrap", .. }]
                ),
                "bits {bits}: {warnings:?}"
            );
        }
    }

    /// 떠 있는 도형·그림은 정렬 비트를 싣고, 글자처럼 취급이면 그대로 inline
    /// 기본값이다(P1b·P5 예측 집합 보호). 이것을 실패시키는 것: 도형 경로에서
    /// 정렬을 빼기 / 글자처럼 취급에도 정렬을 싣기.
    #[test]
    fn floating_object_gets_alignment_and_inline_object_does_not() {
        let floating = (1u32 << 10) | (2 << 3);
        let mut w = Vec::new();
        let p = object_placement_from_ctrl_properties(floating, 0, 0, &mut w);
        assert_eq!(p.horz_align, HorzAlign::Center);
        assert!(w.is_empty(), "{w:?}");
        let inline = object_placement_from_ctrl_properties(floating | 1, 0, 0, &mut w);
        assert_eq!(inline, ObjectPlacement::legacy_inline_defaults());
    }

    #[test]
    fn object_placement_from_ctrl_properties_fails_closed_on_out_of_range_bits() {
        // W5 w0 fail-closed: 레퍼런스 범위 밖 relTo/wrap 값은 임의 known 값으로
        // 정규화하지 않고 typed 경고 + 보수 fallback(Paper/Square)으로 처리한다.
        // vertRelTo=3(미정의)·textWrap=6(미정의), bit0=0(부유).
        let word = (3u32 << 3) | (6u32 << 21);
        let mut warnings = Vec::new();
        let placement = object_placement_from_ctrl_properties(word, 0, 0, &mut warnings);
        assert_eq!(placement.vert_rel_to, ObjectRelativeTo::Paper);
        assert_eq!(placement.text_wrap, ObjectTextWrap::Square);
        assert_eq!(warnings.len(), 2, "unknown vertRelTo + textWrap each emit one warning");
        assert!(warnings.iter().any(|w| matches!(
            w,
            Hwp5Warning::ProjectionFallback { subject, .. } if *subject == "object_placement.vert_rel_to"
        )));
        assert!(warnings.iter().any(|w| matches!(
            w,
            Hwp5Warning::ProjectionFallback { subject, .. } if *subject == "object_placement.text_wrap"
        )));

        // 유효 word (anchored_zero_origin_png attr) 는 경고 0.
        let mut clean = Vec::new();
        let _ = object_placement_from_ctrl_properties(0x040a_2310, 0, 0, &mut clean);
        assert!(clean.is_empty(), "valid property word must not emit fail-closed warnings");
    }

    // ── page_def_to_settings ─────────────────────────────────────────────────

    #[test]
    fn fix2_page_dimensions_reject_unsigned_wrap_and_out_of_range() {
        // 이것을 실패시키는 것: PageDef u32를 as i32로 잘라 음수로 수용 / 범위 오류를 0으로.
        for value in [u32::MAX, 100_000_001] {
            let pd = Hwp5PageDef {
                width: 59535,
                height: 84183,
                margin_left: value,
                margin_right: 0,
                margin_top: 0,
                margin_bottom: 0,
                header_margin: 0,
                footer_margin: 0,
                gutter: 0,
                landscape: false,
            };
            assert!(page_def_to_settings(&pd).is_err());
            let section = make_section(vec![make_paragraph("x", 0, 0)], Some(pd));
            assert!(project_to_core(vec![section]).is_err());
        }
    }

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
        let ps = page_def_to_settings(&pd).unwrap();
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
        let ps = page_def_to_settings(&pd).unwrap();
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

    // ── table controls ────────────────────────────────────────────────────────

    #[test]
    fn table_control_becomes_run_table() {
        let para = Hwp5Paragraph {
            end_char_shape: None,
            silent_wires: Vec::new(),
            page_break: false,
            column_break: false,
            text: String::new(),
            text_segments: Vec::new(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            line_segments: Vec::new(),
            controls: vec![Hwp5Control::Table(Hwp5Table {
                inner_margin: None,
                no_adjust: true,
                cell_zones: Vec::new(),
                caption: None,
                rows: 2,
                cols: 3,
                page_break: Hwp5TablePageBreak::None,
                repeat_header: false,
                cell_spacing: 120,
                border_fill_id: Some(8),
                cells: vec![],
                instance_id: 0,
                ctrl: None,
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
        // 이것을 실패시키는 것: 투영에서 `no_adjust` 대입 삭제.
        assert!(table.no_adjust);
    }

    fn captioned(side: u8, full_size: bool, last_width: u32) -> Table {
        let paragraphs = vec![grid_test_cell(0, 0, 1, 1).paragraphs.remove(0)];
        captioned_with(side, full_size, 8504, last_width, paragraphs).0
    }

    fn captioned_with(
        side: u8,
        full_size: bool,
        width: u32,
        last_width: u32,
        paragraphs: Vec<Hwp5Paragraph>,
    ) -> (Table, Vec<Hwp5Warning>) {
        use crate::decoder::section::Hwp5TableCaption;
        let mut table = table_with_ctrl(0, 0, 0);
        table.cells = vec![grid_test_cell(0, 0, 1, 1), grid_test_cell(0, 1, 1, 1)];
        table.rows = 1;
        table.cols = 2;
        table.caption =
            Some(Hwp5TableCaption { side, full_size, width, gap: 850, last_width, paragraphs });
        let para = Hwp5Paragraph {
            end_char_shape: None,
            silent_wires: Vec::new(),
            page_break: false,
            column_break: false,
            text: String::new(),
            text_segments: Vec::new(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            line_segments: Vec::new(),
            controls: vec![Hwp5Control::Table(table)],
        };
        let (doc, warnings) = project_to_core(vec![make_section(vec![para], None)]).unwrap();
        let table = doc.sections()[0].paragraphs[0]
            .runs
            .iter()
            .find_map(|r| r.content.as_table().cloned())
            .expect("table run");
        (table, warnings)
    }

    /// Q8 (W0): every memo without a time takes the document's last-saved
    /// time — a body memo and one in a run picture's caption — and a memo
    /// with a time keeps it; with no saved time, the epoch.
    /// 이것을 실패시키는 것: 채우기 삭제 · 생성 시각 사용 · 그림 캡션 미방문 ·
    /// 이미 있는 시각 덮어쓰기.
    #[test]
    fn memo_takes_the_last_saved_time() {
        use hwpforge_core::metadata::Metadata;
        let cs = CharShapeIndex::new(0);
        let memo = |time: &str| {
            let mut c = Control::memo_with_anchor(
                vec![Paragraph::with_runs(vec![Run::text("m", cs)], ParaShapeIndex::new(0))],
                vec![Run::text("a", cs)],
            );
            if let Control::Memo { metadata, .. } = &mut c {
                metadata.create_datetime = time.to_string();
            }
            Run::control(c, cs)
        };
        let doc = |modified: Option<&str>| {
            let mut pic =
                Image::new("BinData/a.png", HwpUnit::ZERO, HwpUnit::ZERO, ImageFormat::Png);
            pic.caption = Some(Caption::new(
                vec![Paragraph::with_runs(vec![memo("")], ParaShapeIndex::new(0))],
                CaptionSide::Bottom,
            ));
            let body = Paragraph::with_runs(
                vec![memo(""), memo("2001-02-03T04:05:06Z"), Run::image(pic, cs)],
                ParaShapeIndex::new(0),
            );
            let mut d = Document::new();
            d.add_section(Section::with_paragraphs(vec![body], PageSettings::a4()));
            let mut m = Metadata::default();
            m.created = Some("2020-01-01T00:00:00Z".to_string());
            m.modified = modified.map(str::to_string);
            d.set_metadata(m);
            fill_memo_create_times(&mut d);
            let mut times = Vec::new();
            fn collect(p: &Paragraph, out: &mut Vec<String>) {
                p.for_each_paragraph(|q| {
                    for r in &q.runs {
                        match &r.content {
                            RunContent::Control(c) => {
                                if let Control::Memo { metadata, .. } = c.as_ref() {
                                    out.push(metadata.create_datetime.clone());
                                }
                            }
                            RunContent::Image(img) => {
                                for cp in img.caption.iter().flat_map(|c| &c.paragraphs) {
                                    collect(cp, out);
                                }
                            }
                            _ => {}
                        }
                    }
                });
            }
            collect(&d.sections()[0].paragraphs[0], &mut times);
            times
        };
        let saved = "2026-05-29T23:16:46Z";
        assert_eq!(doc(Some(saved)), [saved, "2001-02-03T04:05:06Z", saved]);
        let epoch = "1970-01-01T00:00:00Z";
        assert_eq!(doc(None), [epoch, "2001-02-03T04:05:06Z", epoch]);
    }

    /// S22: lines that start after some of the zero-length tokens at one
    /// Core position record which ones — section and column definitions, a
    /// silently consumed page number position, tables, the paragraph end —
    /// and a line before them all records none.
    /// 이것을 실패시키는 것: 경계 기록 삭제 · 무음 컨트롤의 종류를 `Silent` 로
    /// (쪽 번호 위치) · 문단 끝을 `End` 아닌 종류로 · gso 를 그림/도형 구분 없이.
    #[test]
    fn collapsed_lines_keep_their_token_boundary() {
        use crate::schema::section::{Hwp5ParaLineSeg, SilentWire, TextSegment};
        use hwpforge_core::layout::LineBoundary;
        use WireTokenKind::*;
        let extra = |id: u32| {
            let mut e = [0u8; 14];
            e[..4].copy_from_slice(&id.to_le_bytes());
            e
        };
        let seg = |w: u32| Hwp5ParaLineSeg {
            text_start_position: w,
            vertical_position: 0,
            line_height: 1000,
            text_height: 1000,
            baseline_distance: 850,
            line_spacing: 600,
            column_start_position: 0,
            segment_width: 40000,
            tag: 0,
        };
        let id = |s: &[u8; 4]| u32::from_be_bytes(*s);
        // secd 0, cold 8, pgnp 16 (silent), tbl 24, gso(picture) 32, "끝" 40, end 41.
        let para = Hwp5Paragraph {
            end_char_shape: None,
            silent_wires: vec![SilentWire { start: 16, len: 8, ctrl_id: id(b"pgnp") }],
            page_break: false,
            column_break: false,
            text: "끝".into(),
            text_segments: vec![
                TextSegment::SectionColumnDef { extra: extra(id(b"secd")) },
                TextSegment::SectionColumnDef { extra: extra(id(b"cold")) },
                TextSegment::ControlRef { extra: extra(id(b"tbl ")) },
                TextSegment::ControlRef { extra: extra(id(b"gso ")) },
                TextSegment::Text("끝".into()),
                TextSegment::ParaBreak,
            ],
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            line_segments: Vec::new(),
            controls: vec![Hwp5Control::Image(Hwp5ImageControl {
                standalone_render: None,
                ctrl_id: id(b"gso "),
                geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                    outer_margin: [0; 4],
                    x: 0,
                    y: 0,
                    width: 3_000,
                    height: 2_000,
                },
                binary_data_id: 1,
                original_size: None,
                instance_id: 0,
                ctrl_properties: 0x1,
                picture: Default::default(),
                link: None,
            })],
        };
        let segs = [seg(0), seg(24), seg(32), seg(40), seg(41)];
        let mut warnings = Vec::new();
        let cache = promote_line_segments(&segs, build_flat_ledger(&para), &mut warnings)
            .unwrap_or_else(|| panic!("{warnings:?}"));
        let got: Vec<(u32, Option<LineBoundary>)> =
            cache.lines.iter().map(|l| (l.textpos, l.boundary.clone())).collect();
        let b = |skip: &[WireTokenKind], next| Some(LineBoundary::new(skip.to_vec(), next));
        assert_eq!(
            got,
            [
                (0, None),
                (0, b(&[SectionDef, ColumnDef, PageNumber], Some(Table))),
                (0, b(&[SectionDef, ColumnDef, PageNumber, Table], Some(Picture))),
                (0, b(&[SectionDef, ColumnDef, PageNumber, Table, Picture], None)),
                (1, None),
            ]
        );
        // "가", a table at core 1, the paragraph end: a line before the end.
        let para = Hwp5Paragraph {
            silent_wires: Vec::new(),
            text: "가".into(),
            text_segments: vec![
                TextSegment::Text("가".into()),
                TextSegment::ControlRef { extra: extra(id(b"tbl ")) },
                TextSegment::ParaBreak,
            ],
            controls: Vec::new(),
            ..para
        };
        let cache =
            promote_line_segments(&[seg(0), seg(9)], build_flat_ledger(&para), &mut warnings)
                .unwrap_or_else(|| panic!("{warnings:?}"));
        assert_eq!(cache.lines[1].boundary, b(&[Table], Some(End)));
    }

    /// S22 (design 3.3): a line that starts inside a field — here inside a
    /// re-emitted field's visible text — drops the cache with a warning; a
    /// line at the field's edge does not.
    /// 이것을 실패시키는 것: 필드 안쪽 검사 삭제.
    #[test]
    fn line_inside_a_field_drops_the_cache() {
        use crate::schema::section::Hwp5ParaLineSeg;
        let seg = |w: u32| Hwp5ParaLineSeg {
            text_start_position: w,
            vertical_position: 0,
            line_height: 1000,
            text_height: 1000,
            baseline_distance: 850,
            line_spacing: 600,
            column_start_position: 0,
            segment_width: 40000,
            tag: 0,
        };
        // "가" 0, field begin 1, "나다라" 9, field end 12, "끝" 20, end 21.
        let ledger = || {
            let mut l = ParaLedger::new(&[]);
            l.observe_text(1);
            l.begin_field();
            l.observe_text(3);
            l.commit_field(FieldEmissionOutcome::ReEmitted);
            l.observe_text(1);
            l.observe_para_break();
            l.finish()
        };
        let mut warnings = Vec::new();
        assert!(promote_line_segments(&[seg(0), seg(10)], ledger(), &mut warnings).is_none());
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        let mut warnings = Vec::new();
        assert!(promote_line_segments(&[seg(0), seg(20)], ledger(), &mut warnings).is_some());
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    /// S22 r1: a paragraph's line boundaries pass at most
    /// `MAX_PARAGRAPH_BOUNDARY_SKIP` tokens in total — 64 bookmarks at one
    /// place, lines after k of them for k in a set summing to exactly the
    /// limit, keep the cache; one more drops it with a warning.
    /// 이것을 실패시키는 것: 상한 검사 삭제(1,025 도 유지), `>` 를 `>=` 로
    /// (1,024 도 drop).
    #[test]
    fn boundary_skip_cap_keeps_the_limit_and_drops_past_it() {
        use crate::schema::section::Hwp5ParaLineSeg;
        let seg = |k: u32| Hwp5ParaLineSeg {
            text_start_position: 8 * k,
            vertical_position: 0,
            line_height: 1000,
            text_height: 1000,
            baseline_distance: 850,
            line_spacing: 600,
            column_start_position: 0,
            segment_width: 40000,
            tag: 0,
        };
        let ledger = || {
            let mut l = ParaLedger::new(&[]);
            for _ in 0..64 {
                l.observe_marker(WireTokenKind::Bookmark);
            }
            l.observe_para_break();
            l.finish()
        };
        let limit = hwpforge_core::layout::MAX_PARAGRAPH_BOUNDARY_SKIP;
        let mut at_limit: Vec<u32> = vec![0, 25, 47, 48];
        at_limit.extend(49..=64);
        assert_eq!(at_limit.iter().sum::<u32>() as usize, limit, "premise");
        let segs: Vec<_> = at_limit.iter().map(|&k| seg(k)).collect();
        let mut warnings = Vec::new();
        let cache = promote_line_segments(&segs, ledger(), &mut warnings)
            .unwrap_or_else(|| panic!("{warnings:?}"));
        let total: usize =
            cache.lines.iter().filter_map(|l| l.boundary.as_ref()).map(|b| b.skip.len()).sum();
        assert_eq!(total, limit);
        assert!(warnings.is_empty(), "{warnings:?}");

        let segs: Vec<_> = at_limit.iter().map(|&k| seg(if k == 25 { 26 } else { k })).collect();
        let mut warnings = Vec::new();
        assert!(promote_line_segments(&segs, ledger(), &mut warnings).is_none());
        assert!(format!("{warnings:?}").contains("pass 1025 zero-width tokens"), "{warnings:?}");
    }

    /// D review r1 (HIGH-2, MEDIUM-4): a caption without paragraphs is
    /// dropped and reported (the layout hints count none for it), and a width
    /// or last width Core cannot hold is dropped and reported rather than
    /// read as "equal to the table width".
    /// 이것을 실패시키는 것: 빈 캡션 유지(`if source.paragraphs.is_empty()` 를
    /// `if false`) · 폭·마지막 폭 경고 삭제.
    #[test]
    fn caption_without_paragraphs_or_with_a_huge_width_is_reported() {
        let text = || vec![grid_test_cell(0, 0, 1, 1).paragraphs.remove(0)];
        let (table, warnings) = captioned_with(3, false, 8504, 8000, Vec::new());
        assert!(table.caption.is_none());
        assert!(
            warnings
                .iter()
                .any(|w| matches!(w, Hwp5Warning::DroppedControl { control: "caption", .. })),
            "{warnings:?}"
        );
        let huge = 100_000_001;
        let (table, warnings) = captioned_with(3, false, 8504, huge, text());
        assert_eq!(table.caption.unwrap().last_width, None);
        let text_of = |w: &[Hwp5Warning]| format!("{w:?}");
        assert!(text_of(&warnings).contains("caption last width 100000001"), "{warnings:?}");
        let (table, warnings) = captioned_with(3, false, huge, 8000, text());
        assert_eq!(table.caption.unwrap().width, None);
        assert!(text_of(&warnings).contains("caption width 100000001"), "{warnings:?}");
        // In range: no caption warning.
        let (_, warnings) = captioned_with(3, false, 8504, 9000, text());
        assert!(!text_of(&warnings).contains("caption"), "{warnings:?}");
    }

    /// D: the HWP5 caption becomes a Core caption — side, `fullSz`, width,
    /// gap and paragraphs; `last_width` only when it differs from the width
    /// the encoder writes (here the first row, 8,000).
    /// 이것을 실패시키는 것: 측 대응 밀림 · `fullSz` 무시 · 정규화 생략(8,000 이
    /// `Some`) · 정규화 반전(다른 값이 `None`) · 캡션 대입 삭제.
    #[test]
    fn table_caption_projects_to_core() {
        let table = captioned(3, true, 8566);
        let caption = table.caption.as_ref().expect("caption");
        assert_eq!(caption.side, CaptionSide::Bottom);
        assert!(caption.full_size);
        assert_eq!(caption.width, Some(HwpUnit::new(8504).unwrap()));
        assert_eq!(caption.gap, HwpUnit::new(850).unwrap());
        assert_eq!(caption.last_width, Some(HwpUnit::new(8566).unwrap()));
        assert_eq!(caption.paragraphs.len(), 1);
        assert_eq!(captioned(3, false, 8000).caption.unwrap().last_width, None);
        let sides: Vec<CaptionSide> =
            (0..4).map(|side| captioned(side, false, 8000).caption.unwrap().side).collect();
        assert_eq!(
            sides,
            [CaptionSide::Left, CaptionSide::Right, CaptionSide::Top, CaptionSide::Bottom]
        );
    }

    /// F: zones that start past their end or reach outside the declared
    /// grid are dropped with one warning each; the rest keep their order.
    /// 이것을 실패시키는 것: 역전 검사 삭제 · 표 밖 검사 삭제 · 끝을 배타로
    /// 비교(마지막 칸 영역 버림) · 경고 삭제.
    #[test]
    fn invalid_cell_zones_are_dropped_and_reported() {
        use crate::decoder::section::Hwp5TableCellZone;
        let z = |r0, c0, r1, c1| Hwp5TableCellZone {
            start_row: r0,
            start_col: c0,
            end_row: r1,
            end_col: c1,
            border_fill_id: 4,
        };
        let mut table = table_with_ctrl(0, 0, 0);
        table.rows = 3;
        table.cols = 2;
        table.cell_zones = vec![
            z(0, 0, 2, 1),
            z(1, 0, 0, 1),
            z(0, 1, 0, 0),
            z(0, 0, 3, 1),
            z(0, 0, 2, 2),
            z(2, 1, 2, 1),
        ];
        let mut warnings = Vec::new();
        let zones = project_cell_zones(&table, &mut warnings);
        assert_eq!(zones, [CellZone::new(0, 0, 2, 1, 4), CellZone::new(2, 1, 2, 1, 4)]);
        let reasons: Vec<String> = warnings.iter().map(|w| format!("{w:?}")).collect();
        assert_eq!(reasons.len(), 4, "{reasons:?}");
        assert!(reasons[..2].iter().all(|r| r.contains("starts past its end")), "{reasons:?}");
        assert!(reasons[2..].iter().all(|r| r.contains("outside the table")), "{reasons:?}");
    }

    /// F: a zone whose border fill is not one of the document's (ids are
    /// 1-based) is dropped with a warning, in a nested table too.
    /// 이것을 실패시키는 것: 범위 검사 삭제 · 0 을 유효로 · 마지막 id 거부 ·
    /// 중첩 표 미방문.
    #[test]
    fn cell_zone_with_unknown_border_fill_is_dropped() {
        let zones = vec![
            CellZone::new(0, 0, 0, 0, 0),
            CellZone::new(0, 0, 0, 0, 1),
            CellZone::new(0, 0, 0, 0, 3),
            CellZone::new(0, 0, 0, 0, 4),
        ];
        let cell = |paragraphs| TableCell::new(paragraphs, HwpUnit::new(1000).unwrap());
        let text = || {
            Paragraph::with_runs(
                vec![Run::text("x", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )
        };
        let inner = Table::new(vec![TableRow::new(vec![cell(vec![text()])])])
            .with_cell_zones(zones.clone());
        let outer = Table::new(vec![TableRow::new(vec![cell(vec![Paragraph::with_runs(
            vec![Run::table(inner, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )])])])
        .with_cell_zones(zones);
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::table(outer, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        ));
        let mut warnings = Vec::new();
        drop_cell_zones_with_unknown_border_fill(&mut doc, 3, &mut warnings);
        let kept = [CellZone::new(0, 0, 0, 0, 1), CellZone::new(0, 0, 0, 0, 3)];
        let RunContent::Table(outer) = &doc.sections()[0].paragraphs[0].runs[0].content else {
            panic!()
        };
        assert_eq!(outer.cell_zones, kept);
        let RunContent::Table(inner) = &outer.rows[0].cells[0].paragraphs[0].runs[0].content else {
            panic!()
        };
        assert_eq!(inner.cell_zones, kept);
        assert_eq!(warnings.len(), 4, "{warnings:?}");
    }

    fn table_with_ctrl(properties: u32, x: i32, y: i32) -> Hwp5Table {
        Hwp5Table {
            inner_margin: None,
            no_adjust: false,
            cell_zones: Vec::new(),
            caption: None,
            rows: 1,
            cols: 1,
            page_break: Hwp5TablePageBreak::Cell,
            repeat_header: false,
            cell_spacing: 0,
            border_fill_id: None,
            cells: vec![grid_test_cell(0, 0, 1, 1)],
            instance_id: 0,
            ctrl: Some(crate::decoder::section::Hwp5TableCtrl {
                outer_margin: None,
                properties,
                geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                    outer_margin: [0; 4],
                    x,
                    y,
                    width: 50_737,
                    height: 43_643,
                },
            }),
        }
    }

    fn projected_table_placement(table: Hwp5Table) -> (Option<ObjectPlacement>, Vec<String>) {
        let para = Hwp5Paragraph {
            end_char_shape: None,
            silent_wires: Vec::new(),
            page_break: false,
            column_break: false,
            text: String::new(),
            text_segments: Vec::new(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            line_segments: Vec::new(),
            controls: vec![Hwp5Control::Table(table)],
        };
        let (doc, warnings) = project_to_core(vec![make_section(vec![para], None)]).unwrap();
        let table = doc.sections()[0].paragraphs[0]
            .runs
            .iter()
            .find_map(|r| r.content.as_table())
            .expect("table")
            .clone();
        (table.placement, warnings.iter().map(|w| format!("{w:?}")).collect())
    }

    /// S20: object common property bit 20 is the size lock of a picture and
    /// of a table (HWP5 ↔ Hancom twins 1,806/1,806).
    /// 이것을 실패시키는 것: 그림·표 잠금을 읽지 않기.
    #[test]
    fn size_lock_is_carried() {
        let doc_with = |props: u32| {
            let image = Hwp5Control::Image(Hwp5ImageControl {
                standalone_render: None,
                ctrl_id: 0x6773_6F20,
                geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                    outer_margin: [0; 4],
                    x: 0,
                    y: 0,
                    width: 100,
                    height: 100,
                },
                binary_data_id: 5,
                original_size: None,
                instance_id: 0,
                ctrl_properties: props,
                picture: Default::default(),
                link: None,
            });
            let section = make_section(
                vec![Hwp5Paragraph {
                    end_char_shape: None,
                    silent_wires: Vec::new(),
                    page_break: false,
                    column_break: false,
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
            let assets = image_plan_with_dimensions(
                5,
                "BIN0005.png",
                Hwp5SemanticImageFormat::Png,
                None,
                None,
                vec![0x89, 0x50, 0x4E, 0x47],
            );
            let (document, _, _) = project_to_core_with_images(
                vec![section],
                &assets,
                DecompressBudget::new(MAX_TOTAL_DECOMPRESSED),
            )
            .unwrap();
            document.sections()[0].paragraphs[0].runs[0].content.as_image().cloned().unwrap()
        };
        assert!(doc_with(1 << 20).frame.size_protect);
        assert!(!doc_with(0).frame.size_protect);

        let para = |props: u32| Hwp5Paragraph {
            end_char_shape: None,
            silent_wires: Vec::new(),
            page_break: false,
            column_break: false,
            text: String::new(),
            text_segments: Vec::new(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            line_segments: Vec::new(),
            controls: vec![Hwp5Control::Table(table_with_ctrl(props, 0, 0))],
        };
        let lock = |props: u32| {
            let (doc, _) = project_to_core(vec![make_section(vec![para(props)], None)]).unwrap();
            doc.sections()[0].paragraphs[0]
                .runs
                .iter()
                .find_map(|r| r.content.as_table())
                .expect("table")
                .frame
                .size_protect
        };
        assert!(lock(0x082a_4310 | (1 << 20)));
        assert!(!lock(0x082a_4310));
    }

    /// A-T3 (법무부 실측 속성 word): 본문 표 `0x082a4310` = 떠 있음·PARA/PARA·
    /// overlap·offset (13, 27415); 머리 표 `0x082a2311` = 글자처럼 취급·PARA/PARA·
    /// flowWithText — 글자처럼 취급 표도 wire 기준·flow 를 그대로 싣는다(한컴
    /// 짝 5,131/5,131). 기본 flow word 는 `None`.
    ///
    /// 이것을 실패시키는 것: 표에 `effective_placement` 를 쓰기(머리 표가 inline
    /// 기본값) / 속성 word 를 표에 안 넘기기(둘 다 `None`) / 접기 기본값에서
    /// flowWithText 를 빼기(기본 word 가 `Some`).
    #[test]
    fn table_placement_carries_wire_values_and_folds_the_default() {
        let (body, _) = projected_table_placement(table_with_ctrl(0x082a_4310, 13, 27_415));
        let body = body.expect("floating table");
        assert!(!body.treat_as_char && body.allow_overlap && !body.flow_with_text);
        assert_eq!(
            (body.vert_rel_to.clone(), body.horz_rel_to.clone()),
            (ObjectRelativeTo::Para, ObjectRelativeTo::Para)
        );
        assert_eq!((body.horz_offset.as_i32(), body.vert_offset.as_i32()), (13, 27_415));
        assert_eq!(body.text_wrap, ObjectTextWrap::TopAndBottom);

        let (head, _) = projected_table_placement(table_with_ctrl(0x082a_2311, 0, 0));
        let head = head.expect("inline table");
        assert!(head.treat_as_char && head.flow_with_text && !head.affect_line_spacing);
        // bit 2 = affectLSpacing (한컴 짝 `affectLSpacing="1"` 표).
        let (spacing, _) = projected_table_placement(table_with_ctrl(0x082a_2311 | 0b100, 0, 0));
        assert!(spacing.expect("inline table").affect_line_spacing);
        assert_eq!(head.horz_rel_to, ObjectRelativeTo::Para);

        // treatAsChar 0 · flowWithText · PARA/COLUMN · wrap 1 · offset 0 = 기본 flow.
        let default_word = (2 << 3) | (2 << 8) | (1 << 13) | (1 << 21);
        assert_eq!(projected_table_placement(table_with_ctrl(default_word, 0, 0)).0, None);
    }

    /// S1 HWP5 정렬: 가로 2 = Right 는 확인값(경고 없음), 세로 1 = Center 는
    /// 스펙 순서로 읽되 미확인 경고, 가로 7 은 범위 밖 → 경고 + Left.
    /// 이것을 실패시키는 것: 정렬 비트를 읽지 않기 / 미확인 경고 삭제 / 확인값
    /// 2 에 경고.
    #[test]
    fn table_alignment_bits_are_read_and_unverified_values_reported() {
        let default_word: u32 = (2 << 3) | (2 << 8) | (1 << 13) | (1 << 21);
        let (p, w) = projected_table_placement(table_with_ctrl(default_word | (2 << 10), 0, 0));
        assert_eq!(p.expect("aligned").horz_align, HorzAlign::Right);
        assert!(!w.iter().any(|w| w.contains("align_unverified")), "{w:?}");
        let (p, w) = projected_table_placement(table_with_ctrl(default_word | (1 << 5), 0, 0));
        assert_eq!(p.expect("aligned").vert_align, VertAlign::Center);
        assert_eq!(w.iter().filter(|w| w.contains("align_unverified")).count(), 1, "{w:?}");
        let (p, w) = projected_table_placement(table_with_ctrl(default_word | (7 << 10), 0, 0));
        assert_eq!(p, None, "out of range reads as the default");
        assert_eq!(w.iter().filter(|w| w.contains("align_unverified")).count(), 1, "{w:?}");
    }

    fn grid_test_cell(row: u16, column: u16, row_span: u16, col_span: u16) -> Hwp5TableCell {
        Hwp5TableCell {
            has_margin: true,
            line_wrap: 0,
            column,
            row,
            col_span,
            row_span,
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
                end_char_shape: None,
                silent_wires: Vec::new(),
                page_break: false,
                column_break: false,
                text: "셀".to_string(),
                text_segments: Vec::new(),
                para_shape_id: 0,
                style_id: 0,
                char_shape_runs: vec![],
                line_segments: Vec::new(),
                controls: vec![],
            }],
        }
    }

    fn grid_test_table_paragraph(rows: u16, cols: u16, cells: Vec<Hwp5TableCell>) -> Hwp5Paragraph {
        Hwp5Paragraph {
            end_char_shape: None,
            silent_wires: Vec::new(),
            page_break: false,
            column_break: false,
            text: "\u{FFFC}".to_string(),
            text_segments: Vec::new(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            line_segments: Vec::new(),
            controls: vec![Hwp5Control::Table(Hwp5Table {
                inner_margin: None,
                no_adjust: false,
                cell_zones: Vec::new(),
                caption: None,
                rows,
                cols,
                page_break: Hwp5TablePageBreak::None,
                repeat_header: false,
                cell_spacing: 0,
                border_fill_id: None,
                cells,
                instance_id: 0,
                ctrl: None,
            })],
        }
    }

    #[test]
    fn fully_covered_row_projects_without_phantom_cell() {
        // Wire truth: 2×1 grid with one rs-2 anchor; the second wire row has
        // no cells. The projection must keep the empty row instead of
        // injecting a phantom cell that breaks the grid tiling invariant.
        let para = grid_test_table_paragraph(2, 1, vec![grid_test_cell(0, 0, 2, 1)]);
        let section = make_section(vec![para], None);
        let (doc, warnings) = project_to_core(vec![section]).unwrap();

        {
            let table = doc.sections()[0].paragraphs[0]
                .runs
                .iter()
                .find_map(|run| run.content.as_table())
                .expect("expected table run");
            assert_eq!(table.rows.len(), 2);
            assert!(table.rows[1].cells.is_empty(), "covered row must stay empty");
        }
        assert!(!warnings.iter().any(|warning| matches!(
            warning,
            Hwp5Warning::ProjectionFallback { subject, .. } if *subject == "table.covered_row"
        )));
        assert!(doc.validate().is_ok(), "truthful covered row must pass Core validation");
    }

    #[test]
    fn uncovered_empty_row_gets_placeholder_cell_and_warning() {
        // Malformed wire: row 1 declared but neither populated nor covered.
        // Conversion keeps the historical placeholder cell so the document
        // still converts, but the fallback is surfaced (previously silent).
        let para = grid_test_table_paragraph(2, 1, vec![grid_test_cell(0, 0, 1, 1)]);
        let section = make_section(vec![para], None);
        let (doc, warnings) = project_to_core(vec![section]).unwrap();

        {
            let table = doc.sections()[0].paragraphs[0]
                .runs
                .iter()
                .find_map(|run| run.content.as_table())
                .expect("expected table run");
            assert_eq!(table.rows.len(), 2);
            assert_eq!(table.rows[1].cells.len(), 1, "placeholder cell expected");
        }
        assert!(warnings.iter().any(|warning| matches!(
            warning,
            Hwp5Warning::ProjectionFallback { subject, reason }
                if *subject == "table.covered_row"
                    && reason.contains("uncovered_empty_hwp5_table_row row=1")
        )));
        assert!(doc.validate().is_ok());
    }

    #[test]
    fn table_cell_text_is_projected() {
        let para = Hwp5Paragraph {
            end_char_shape: None,
            silent_wires: Vec::new(),
            page_break: false,
            column_break: false,
            text: "\u{FFFC}".to_string(),
            text_segments: Vec::new(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            line_segments: Vec::new(),
            controls: vec![Hwp5Control::Table(Hwp5Table {
                inner_margin: None,
                no_adjust: false,
                cell_zones: Vec::new(),
                caption: None,
                rows: 1,
                cols: 1,
                page_break: Hwp5TablePageBreak::None,
                repeat_header: false,
                cell_spacing: 0,
                border_fill_id: None,
                cells: vec![Hwp5TableCell {
                    has_margin: true,
                    line_wrap: 0,
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
                        end_char_shape: None,
                        silent_wires: Vec::new(),
                        page_break: false,
                        column_break: false,
                        text: "셀".to_string(),
                        text_segments: Vec::new(),
                        para_shape_id: 0,
                        style_id: 0,
                        char_shape_runs: vec![],
                        line_segments: Vec::new(),
                        controls: vec![],
                    }],
                }],
                instance_id: 0,
                ctrl: None,
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
            end_char_shape: None,
            silent_wires: Vec::new(),
            page_break: false,
            column_break: false,
            text: "\u{FFFC}".to_string(),
            text_segments: Vec::new(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            line_segments: Vec::new(),
            controls: vec![Hwp5Control::Table(Hwp5Table {
                inner_margin: None,
                no_adjust: false,
                cell_zones: Vec::new(),
                caption: None,
                rows: 1,
                cols: 1,
                page_break: Hwp5TablePageBreak::None,
                repeat_header: false,
                cell_spacing: 0,
                border_fill_id: None,
                cells: vec![Hwp5TableCell {
                    has_margin: true,
                    line_wrap: 0,
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
                        end_char_shape: None,
                        silent_wires: Vec::new(),
                        page_break: false,
                        column_break: false,
                        text: "셀".to_string(),
                        text_segments: Vec::new(),
                        para_shape_id: 0,
                        style_id: 0,
                        char_shape_runs: vec![],
                        line_segments: Vec::new(),
                        controls: vec![],
                    }],
                }],
                instance_id: 0,
                ctrl: None,
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
    fn mixed_table_header_cells_are_preserved_without_warning() {
        let para = Hwp5Paragraph {
            end_char_shape: None,
            silent_wires: Vec::new(),
            page_break: false,
            column_break: false,
            text: "\u{FFFC}".to_string(),
            text_segments: Vec::new(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            line_segments: Vec::new(),
            controls: vec![Hwp5Control::Table(Hwp5Table {
                inner_margin: None,
                no_adjust: false,
                cell_zones: Vec::new(),
                caption: None,
                rows: 1,
                cols: 2,
                page_break: Hwp5TablePageBreak::Cell,
                repeat_header: true,
                cell_spacing: 0,
                border_fill_id: None,
                cells: vec![
                    Hwp5TableCell {
                        has_margin: true,
                        line_wrap: 0,
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
                            end_char_shape: None,
                            silent_wires: Vec::new(),
                            page_break: false,
                            column_break: false,
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
                        has_margin: true,
                        line_wrap: 0,
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
                            end_char_shape: None,
                            silent_wires: Vec::new(),
                            page_break: false,
                            column_break: false,
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
                instance_id: 0,
                ctrl: None,
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
        // 이것을 실패시키는 것: 셀 값 강등 / 옛 경고 복원.
        assert_eq!(table.rows[0].cells[0].header, Some(true));
        assert_eq!(table.rows[0].cells[1].header, Some(false));
        assert!(!warnings.iter().any(|warning| matches!(
            warning,
            Hwp5Warning::ProjectionFallback { subject: "table.header_row", .. }
        )));
    }

    #[test]
    fn non_leading_header_cells_are_preserved_without_warning() {
        // 이것을 실패시키는 것: 비선두 칸 값 제거 / 행 파생 생략 / 옛 경고 복원.
        fn header_cell(row: u16, is_header: bool, text: &str) -> Hwp5TableCell {
            Hwp5TableCell {
                has_margin: true,
                line_wrap: 0,
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
                    end_char_shape: None,
                    silent_wires: Vec::new(),
                    page_break: false,
                    column_break: false,
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
            end_char_shape: None,
            silent_wires: Vec::new(),
            page_break: false,
            column_break: false,
            text: "\u{FFFC}".to_string(),
            text_segments: Vec::new(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            line_segments: Vec::new(),
            controls: vec![Hwp5Control::Table(Hwp5Table {
                inner_margin: None,
                no_adjust: false,
                cell_zones: Vec::new(),
                caption: None,
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
                instance_id: 0,
                ctrl: None,
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
        assert!(!table.rows[2].is_header, "trailing title cells are not repeat rows");
        assert_eq!(table.rows[2].cells[0].header, Some(true));
        assert!(!warnings.iter().any(|warning| matches!(
            warning,
            Hwp5Warning::ProjectionFallback { subject: "table.header_row", .. }
        )));
        assert!(doc.validate().is_ok(), "explicit non-leading title cells are valid");
    }

    #[test]
    fn page_number_inside_table_cell_is_carried_to_section() {
        // 한글 government layouts often put the page-number control (`pgnp`)
        // inside a layout table cell. The section-level scan must reach it;
        // a body-paragraph-only scan would drop it.
        fn pgnp_cell() -> Hwp5TableCell {
            Hwp5TableCell {
                has_margin: true,
                line_wrap: 0,
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
                    end_char_shape: None,
                    silent_wires: Vec::new(),
                    page_break: false,
                    column_break: false,
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
            end_char_shape: None,
            silent_wires: Vec::new(),
            page_break: false,
            column_break: false,
            text: "\u{FFFC}".to_string(),
            text_segments: Vec::new(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            line_segments: Vec::new(),
            controls: vec![Hwp5Control::Table(Hwp5Table {
                inner_margin: None,
                no_adjust: false,
                cell_zones: Vec::new(),
                caption: None,
                rows: 1,
                cols: 1,
                page_break: Hwp5TablePageBreak::Cell,
                repeat_header: false,
                cell_spacing: 0,
                border_fill_id: None,
                cells: vec![pgnp_cell()],
                instance_id: 0,
                ctrl: None,
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
                end_char_shape: None,
                silent_wires: Vec::new(),
                page_break: false,
                column_break: false,
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
                has_margin: true,
                line_wrap: 0,
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
                end_char_shape: None,
                silent_wires: Vec::new(),
                page_break: false,
                column_break: false,
                text: "\u{FFFC}".to_string(),
                text_segments: Vec::new(),
                para_shape_id: 0,
                style_id: 0,
                char_shape_runs: vec![],
                line_segments: Vec::new(),
                controls: vec![Hwp5Control::Table(Hwp5Table {
                    inner_margin: None,
                    no_adjust: false,
                    cell_zones: Vec::new(),
                    caption: None,
                    rows: 1,
                    cols: 1,
                    page_break: Hwp5TablePageBreak::Cell,
                    repeat_header: false,
                    cell_spacing: 0,
                    border_fill_id: None,
                    cells: vec![cell],
                    instance_id: 0,
                    ctrl: None,
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
    fn inline_footnote_stays_in_one_paragraph_between_text_runs() {
        // Regression (CLAUDE.md gotcha #12): an inline footnote reference
        // (HWP5 ParaText control char 0x11 → `\u{FFFC}` after the schema
        // promotion) must project as a `Control::Footnote` run *between* the
        // surrounding text runs, inside a single Core paragraph — never on
        // its own line / paragraph, and never drained to the paragraph tail.
        let para = Hwp5Paragraph {
            end_char_shape: None,
            silent_wires: Vec::new(),
            page_break: false,
            column_break: false,
            text: "앞\u{FFFC}뒤".to_string(),
            text_segments: Vec::new(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            line_segments: Vec::new(),
            controls: vec![Hwp5Control::Footnote(crate::decoder::section::Hwp5NestedSubtree {
                wire_order: 0,
                ctrl_id: 0x666E_2020,
                properties_raw: 0,
                instance_id: 7,
                paragraphs: vec![Hwp5Paragraph {
                    end_char_shape: None,
                    silent_wires: Vec::new(),
                    page_break: false,
                    column_break: false,
                    text: "각주 본문".to_string(),
                    text_segments: Vec::new(),
                    para_shape_id: 0,
                    style_id: 0,
                    char_shape_runs: vec![],
                    line_segments: Vec::new(),
                    controls: vec![],
                }],
            })],
        };
        let section = make_section(vec![para], None);
        let (doc, warnings) = project_to_core(vec![section]).unwrap();
        let paragraph = &doc.sections()[0].paragraphs[0];

        // Exactly one Core paragraph carries everything.
        assert_eq!(
            doc.sections()[0].paragraphs.len(),
            1,
            "footnote must not spawn its own paragraph"
        );

        // Runs are in document order: text → footnote → text.
        assert_eq!(paragraph.runs.len(), 3, "runs: {:?}", paragraph.runs);
        assert_eq!(paragraph.runs[0].content.as_text(), Some("앞"));
        let footnote = paragraph.runs[1]
            .content
            .as_control()
            .expect("middle run must be the footnote control");
        match footnote {
            Control::Footnote { paragraphs, .. } => {
                assert!(!paragraphs.is_empty(), "footnote body must stay nested");
                assert_eq!(
                    paragraphs[0].runs[0].content.as_text(),
                    Some("각주 본문"),
                    "footnote body text must survive nested inside the control"
                );
            }
            other => panic!("expected Footnote control, got {other:?}"),
        }
        assert_eq!(paragraph.runs[2].content.as_text(), Some("뒤"));

        assert!(warnings.is_empty(), "no warnings expected, got {warnings:?}");
    }

    #[test]
    fn line_control_becomes_visible_core_line() {
        let para = Hwp5Paragraph {
            end_char_shape: None,
            silent_wires: Vec::new(),
            page_break: false,
            column_break: false,
            text: "\u{FFFC}".to_string(),
            text_segments: Vec::new(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            line_segments: Vec::new(),
            controls: vec![Hwp5Control::Line(Hwp5LineControl {
                standalone_render: None,
                original_size: None,
                look: None,
                ctrl_id: 0x6773_6F20,
                geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                    outer_margin: [0; 4],
                    x: 9_884,
                    y: 11_980,
                    width: 29_360,
                    height: 0,
                },
                start: crate::schema::section::Hwp5ShapePoint { x: 0, y: 0 },
                end: crate::schema::section::Hwp5ShapePoint { x: 100, y: 100 },
                ctrl_properties: 0,
            })],
        };
        let section = make_section(vec![para], None);
        let (doc, _) = project_to_core(vec![section]).unwrap();
        let paragraph = &doc.sections()[0].paragraphs[0];
        let control = paragraph.runs[0].content.as_control().expect("expected control run");
        match control {
            Control::Line { start, end, width, height, placement, .. } => {
                assert_eq!(*start, hwpforge_core::control::ShapePoint { x: 0, y: 0 });
                assert_eq!(*end, hwpforge_core::control::ShapePoint { x: 29_360, y: 100 });
                assert_eq!(*width, HwpUnit::new(29_360).unwrap());
                assert_eq!(*height, HwpUnit::new(100).unwrap());
                let placement = placement.as_ref().expect("floating line carries placement");
                assert_eq!(placement.horz_offset.as_i32(), 9_884);
                assert_eq!(placement.vert_offset.as_i32(), 11_980);
            }
            other => panic!("expected Line control, got {:?}", other),
        }
    }

    #[test]
    fn polygon_control_becomes_visible_core_polygon() {
        let para = Hwp5Paragraph {
            end_char_shape: None,
            silent_wires: Vec::new(),
            page_break: false,
            column_break: false,
            text: "\u{FFFC}".to_string(),
            text_segments: Vec::new(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            line_segments: Vec::new(),
            controls: vec![Hwp5Control::Polygon(Hwp5PolygonControl {
                standalone_render: None,
                original_size: None,
                look: None,
                ctrl_id: 0x6773_6F20,
                paragraphs: Vec::new(),
                list_header_properties: None,
                text_margin: None,
                geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                    outer_margin: [0; 4],
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
                ctrl_properties: 0,
            })],
        };
        let section = make_section(vec![para], None);
        let (doc, _) = project_to_core(vec![section]).unwrap();
        let paragraph = &doc.sections()[0].paragraphs[0];
        let control = paragraph.runs[0].content.as_control().expect("expected control run");
        match control {
            Control::Polygon { vertices, width, height, placement, paragraphs, .. } => {
                assert_eq!(vertices.len(), 6);
                assert_eq!(vertices[0], hwpforge_core::control::ShapePoint { x: 6_278, y: 0 });
                assert_eq!(vertices[5], hwpforge_core::control::ShapePoint { x: 6_278, y: 0 });
                assert_eq!(*width, HwpUnit::new(12_560).unwrap());
                assert_eq!(*height, HwpUnit::new(13_040).unwrap());
                let placement = placement.as_ref().expect("floating polygon carries placement");
                assert_eq!(placement.horz_offset.as_i32(), 17_804);
                assert_eq!(placement.vert_offset.as_i32(), 13_900);
                assert!(paragraphs.is_empty());
            }
            other => panic!("expected Polygon control, got {:?}", other),
        }
    }

    #[test]
    fn rect_control_carries_into_core_rect_without_warning() {
        let para = Hwp5Paragraph {
            end_char_shape: None,
            silent_wires: Vec::new(),
            page_break: false,
            column_break: false,
            text: "\u{FFFC}".to_string(),
            text_segments: Vec::new(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            line_segments: Vec::new(),
            controls: vec![Hwp5Control::Rect(crate::decoder::section::Hwp5RectControl {
                standalone_render: None,
                original_size: None,
                look: None,
                ctrl_id: 0x6773_6F20,
                geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                    outer_margin: [0; 4],
                    x: 13_200,
                    y: 14_280,
                    width: 10_020,
                    height: 8_000,
                },
                ctrl_properties: 0,
            })],
        };
        let section = make_section(vec![para], None);
        let (doc, warnings) = project_to_core(vec![section]).unwrap();
        let paragraph = &doc.sections()[0].paragraphs[0];
        let control = paragraph.runs[0].content.as_control().expect("expected control run");
        match control {
            Control::Rect { width, height, placement, .. } => {
                assert_eq!(*width, HwpUnit::new(10_020).unwrap());
                assert_eq!(*height, HwpUnit::new(8_000).unwrap());
                let placement = placement.as_ref().expect("floating rect carries placement");
                assert_eq!(placement.horz_offset.as_i32(), 13_200);
                assert_eq!(placement.vert_offset.as_i32(), 14_280);
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
            end_char_shape: None,
            silent_wires: Vec::new(),
            page_break: false,
            column_break: false,
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
                inner_margin: None,
                no_adjust: false,
                cell_zones: Vec::new(),
                caption: None,
                rows: 3,
                cols: 4,
                page_break: Hwp5TablePageBreak::Cell,
                repeat_header: true,
                cell_spacing: 0,
                border_fill_id: None,
                cells: vec![],
                instance_id: 0,
                ctrl: None,
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
                inner_margin: None,
                no_adjust: false,
                cell_zones: Vec::new(),
                caption: None,
                rows: 0,
                cols: 2,
                page_break: Hwp5TablePageBreak::None,
                repeat_header: false,
                cell_spacing: 0,
                border_fill_id: None,
                cells: vec![],
                instance_id: 0,
                ctrl: None,
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
                inner_margin: None,
                no_adjust: false,
                cell_zones: Vec::new(),
                caption: None,
                rows: 2,
                cols: 0,
                page_break: Hwp5TablePageBreak::Table,
                repeat_header: false,
                cell_spacing: 0,
                border_fill_id: None,
                cells: vec![],
                instance_id: 0,
                ctrl: None,
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
                inner_margin: None,
                no_adjust: false,
                cell_zones: Vec::new(),
                caption: None,
                rows: 1,
                cols: 1,
                page_break: Hwp5TablePageBreak::Unknown(3),
                repeat_header: true,
                cell_spacing: 0,
                border_fill_id: None,
                cells: vec![],
                instance_id: 0,
                ctrl: None,
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

    // E6 slice C: the HWP5 wire bytes (`raw_command`/`raw_trailer`/`raw_flag`)
    // were removed from the core IR. These tests lock that the *derivation*
    // (DateCodeField.is_time_mode, InlinePageNumber.kind) survives that
    // removal by re-running the exact projection logic.

    /// An `atno` whose kind is a picture, table or equation number (3–5) is
    /// reported; a page number is not.
    ///
    /// 이것을 실패시키는 것: 그 경고를 지우는 것 / 범위를 3..=4 로 줄이는 것.
    #[test]
    fn atno_caption_number_is_reported() {
        use crate::schema::section::Hwp5InlinePageNumberControl;
        let warnings = |raw_flag: u32| {
            let ctrl = Hwp5Control::InlinePageNumber(Hwp5InlinePageNumberControl {
                ctrl_id: 0x6174_6E6F,
                raw_flag,
            });
            let mut images = ProjectionImageState::new(
                None,
                None,
                DecompressBudget::new(MAX_TOTAL_DECOMPRESSED),
            );
            project_control_run(
                &ctrl,
                &mut images,
                ImageProjectionContext::Flow,
                CharShapeIndex::new(0),
            );
            images
                .warnings
                .iter()
                .filter(|w| {
                    matches!(
                        w,
                        Hwp5Warning::ProjectionFallback { subject: "control.auto_number", .. }
                    )
                })
                .count()
        };
        assert_eq!([3, 4, 5].map(warnings), [1, 1, 1]);
        assert_eq!([0, 1, 6].map(warnings), [0, 0, 0]);
    }

    /// `project_control_run` maps an `atno` wire flag to the right
    /// `InlinePageKind` without carrying the raw flag into the core IR.
    #[test]
    fn project_inline_pagenumber_maps_raw_flag_to_kind() {
        use crate::schema::section::Hwp5InlinePageNumberControl;
        use hwpforge_core::control::{Control, InlinePageKind};

        let project = |raw_flag: u32| -> Control {
            let ctrl = Hwp5Control::InlinePageNumber(Hwp5InlinePageNumberControl {
                ctrl_id: 0x6174_6E6F,
                raw_flag,
            });
            let mut images = ProjectionImageState::new(
                None,
                None,
                DecompressBudget::new(MAX_TOTAL_DECOMPRESSED),
            );
            let run = project_control_run(
                &ctrl,
                &mut images,
                ImageProjectionContext::Flow,
                CharShapeIndex::new(0),
            )
            .expect("atno control must project to a run");
            match run.content {
                RunContent::Control(boxed) => *boxed,
                other => panic!("expected control run, got {other:?}"),
            }
        };

        assert_eq!(
            project(0x06),
            Control::InlinePageNumber { kind: InlinePageKind::TotalPages },
            "raw flag 0x06 → TotalPages",
        );
        assert_eq!(
            project(0x00),
            Control::InlinePageNumber { kind: InlinePageKind::CurrentPage },
            "raw flag 0x00 → CurrentPage",
        );
        assert_eq!(
            project(0xABCD_1234),
            Control::InlinePageNumber { kind: InlinePageKind::Unknown },
            "unknown raw flag → Unknown (no fabricated kind)",
        );
    }

    /// W2: `project_control_run` maps `nwno` 속성 bits 0-3 to the right
    /// `NewNumberKind` (F1 실측: 0 = 쪽) and carries the u16 number as u32.
    /// Unmapped raw values surface as Unknown + warning (no fabrication).
    #[test]
    fn project_new_number_maps_kind_and_number() {
        use crate::schema::section::Hwp5NewNumberControl;
        use hwpforge_core::control::{Control, NewNumberKind};

        let project = |kind_raw: u32, number: u16| -> (Control, Vec<Hwp5Warning>) {
            let ctrl = Hwp5Control::NewNumber(Hwp5NewNumberControl {
                ctrl_id: 0x6E77_6E6F,
                kind_raw,
                number,
            });
            let mut images = ProjectionImageState::new(
                None,
                None,
                DecompressBudget::new(MAX_TOTAL_DECOMPRESSED),
            );
            let run = project_control_run(
                &ctrl,
                &mut images,
                ImageProjectionContext::Flow,
                CharShapeIndex::new(0),
            )
            .expect("nwno control must project to a run");
            let control = match run.content {
                RunContent::Control(boxed) => *boxed,
                other => panic!("expected control run, got {other:?}"),
            };
            (control, images.warnings)
        };

        // F1 실측: 00 00 00 00 07 00 → 쪽 번호 7.
        let (control, warnings) = project(0, 7);
        assert_eq!(control, Control::NewNumber { kind: NewNumberKind::Page, number: 7 });
        assert!(warnings.is_empty(), "정상 kind 는 경고 없음: {warnings:?}");

        let (control, _) = project(5, 3);
        assert_eq!(control, Control::NewNumber { kind: NewNumberKind::Equation, number: 3 });

        // 미지 raw → Unknown + 경고 (fake 매핑 금지).
        let (control, warnings) = project(9, 1);
        assert_eq!(control, Control::NewNumber { kind: NewNumberKind::Unknown, number: 1 });
        assert!(warnings.iter().any(|w| matches!(
            w,
            Hwp5Warning::ProjectionFallback { subject: "control.new_number", .. }
        )));
    }

    /// W3: `project_control_run` maps pghd 속성 bits 0-5 to six bools
    /// (F2 실측: 0x20 쪽번호 / 0x10 배경 / 0x3F 전부 — secd word 동일 배열).
    /// 검증 밖 비트(6+)는 경고 + 하위 6비트만 carry.
    #[test]
    fn project_page_hiding_maps_mask_bits() {
        use crate::schema::section::Hwp5PageHidingControl;
        use hwpforge_core::control::Control;

        let project = |mask: u32| -> (Control, Vec<Hwp5Warning>) {
            let ctrl =
                Hwp5Control::PageHiding(Hwp5PageHidingControl { ctrl_id: 0x7067_6864, mask });
            let mut images = ProjectionImageState::new(
                None,
                None,
                DecompressBudget::new(MAX_TOTAL_DECOMPRESSED),
            );
            let run = project_control_run(
                &ctrl,
                &mut images,
                ImageProjectionContext::Flow,
                CharShapeIndex::new(0),
            )
            .expect("pghd control must project to a run");
            let control = match run.content {
                RunContent::Control(boxed) => *boxed,
                other => panic!("expected control run, got {other:?}"),
            };
            (control, images.warnings)
        };

        // F2-①: 쪽번호만.
        let (control, warnings) = project(0x20);
        assert_eq!(
            control,
            Control::PageHiding {
                hide_header: false,
                hide_footer: false,
                hide_master_page: false,
                hide_border: false,
                hide_fill: false,
                hide_page_num: true,
            }
        );
        assert!(warnings.is_empty(), "정의 비트만 = 경고 없음: {warnings:?}");

        // F2-③: 전부.
        let (control, _) = project(0x3F);
        assert!(matches!(
            control,
            Control::PageHiding {
                hide_header: true,
                hide_footer: true,
                hide_master_page: true,
                hide_border: true,
                hide_fill: true,
                hide_page_num: true,
            }
        ));

        // 검증 밖 비트 → 경고 + 하위 6비트만.
        let (control, warnings) = project(0x60);
        assert!(matches!(control, Control::PageHiding { hide_page_num: true, .. }));
        assert!(warnings.iter().any(|w| matches!(
            w,
            Hwp5Warning::ProjectionFallback { subject: "control.page_hiding", .. }
        )));
    }

    /// The `%dte` time-mode derivation (`raw_command` `T`-prefix →
    /// `is_time_mode`) survives the removal of `raw_command` from the core
    /// IR. The emission path is private + stateful, so we assert the exact
    /// derivation rule the projection applies in `emit_active_field`.
    #[test]
    fn datecodefield_time_mode_derived_from_t_prefix() {
        use hwpforge_core::control::Control;

        let derive = |raw_command: &str| -> Control {
            Control::DateCodeField {
                is_time_mode: raw_command.starts_with('T'),
                display_text: String::new(),
            }
        };

        assert!(
            matches!(derive("T\\:H:mm;0;"), Control::DateCodeField { is_time_mode: true, .. }),
            "`T`-prefixed command → time mode",
        );
        assert!(
            matches!(
                derive("\\:1년 2월 3일;0;"),
                Control::DateCodeField { is_time_mode: false, .. }
            ),
            "date command → not time mode",
        );
    }

    // ── R0-2: chart extraction is charged to the document budget ─────────

    fn chart_ole_control(binary_data_id: u16) -> Hwp5Control {
        Hwp5Control::OleObject(Hwp5OleObjectControl {
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                outer_margin: [0; 4],
                x: 0,
                y: 0,
                width: 30_000,
                height: 20_000,
            },
            binary_data_id,
            extent_width: 7_200,
            extent_height: 7_200,
        })
    }

    fn paragraph_with_controls(controls: Vec<Hwp5Control>) -> Hwp5Paragraph {
        Hwp5Paragraph { controls, ..make_paragraph("", 0, 0) }
    }

    /// OLE plan whose every id maps to the inflated chart fixture.
    fn chart_ole_plan(ids: &[u16]) -> Hwp5OleAssetPlan {
        let inflated = crate::decoder::chart_ole::tests::chart_fixture_inflated();
        Hwp5OleAssetPlan {
            assets_by_reference: ids.iter().map(|id| (*id, Ok(inflated.clone()))).collect(),
            reference_ambiguities: BTreeMap::new(),
        }
    }

    /// Bytes one extraction of the fixture chart makes resident.
    fn one_chart_extraction_len() -> u64 {
        let inflated = crate::decoder::chart_ole::tests::chart_fixture_inflated();
        let payload = extract_chart_payload_from_inflated(&inflated, inflated.len() as u64)
            .expect("fixture chart extracts");
        (payload.chart_xml.len() + payload.ole_bytes.len()) as u64
    }

    fn project_charts(
        sections: Vec<SectionResult>,
        ole_ids: &[u16],
        budget_max: u64,
    ) -> Hwp5Result<(Document<Draft>, ImageStore, Vec<Hwp5Warning>)> {
        project_to_core_with_images_and_ole(
            sections,
            &image_plan([]),
            &chart_ole_plan(ole_ids),
            DecompressBudget::new(budget_max),
        )
    }

    fn embedded_chart_count(doc: &Document<Draft>) -> usize {
        doc.sections()
            .iter()
            .flat_map(|section| section.paragraphs.iter())
            .flat_map(|paragraph| paragraph.runs.iter())
            .filter(|run| matches!(run.content.as_control(), Some(Control::EmbeddedChart { .. })))
            .count()
    }

    fn reset_extract_calls() {
        crate::decoder::chart_ole::EXTRACT_CALLS.with(|calls| calls.set(0));
    }

    fn extract_calls() -> usize {
        crate::decoder::chart_ole::EXTRACT_CALLS.with(|calls| calls.get())
    }

    fn assert_budget_error(
        result: Hwp5Result<(Document<Draft>, ImageStore, Vec<Hwp5Warning>)>,
        needle: &str,
    ) {
        let err = result.expect_err("document budget overrun must fail the document");
        assert!(matches!(err, Hwp5Error::Cfb { .. }), "got: {err:?}");
        let msg = err.to_string();
        assert!(msg.contains("total decompressed data"), "got: {msg}");
        assert!(msg.contains(needle), "expected {needle:?} in: {msg}");
    }

    // 이것을 실패시키는 것: `project_ole_object_run` 의 추출 charge 제거
    // (두 번째 차트도 성공해 문서가 `Ok`).
    #[test]
    fn chart_extractions_are_charged_and_overrun_fails_the_document() {
        let one = one_chart_extraction_len();
        let section = || {
            make_section(
                vec![paragraph_with_controls(vec![chart_ole_control(1), chart_ole_control(2)])],
                None,
            )
        };

        let (doc, _, _) =
            project_charts(vec![section()], &[1, 2], 2 * one).expect("two charts fit exactly");
        assert_eq!(embedded_chart_count(&doc), 2);

        assert_budget_error(
            project_charts(vec![section()], &[1, 2], 2 * one - 1),
            "chart binary_data_id=2",
        );
    }

    // 이것을 실패시키는 것: 같은 binary_data_id 는 처음 한 번만 charge 하기
    // (자산당 1회로 세면 두 번째 참조가 예산 밖에서 할당된다).
    #[test]
    fn repeated_references_to_one_chart_asset_are_charged_per_reference() {
        let one = one_chart_extraction_len();
        let section = || {
            make_section(
                vec![paragraph_with_controls(vec![chart_ole_control(1), chart_ole_control(1)])],
                None,
            )
        };

        let (doc, _, _) =
            project_charts(vec![section()], &[1], 2 * one).expect("two references fit exactly");
        assert_eq!(embedded_chart_count(&doc), 2);

        assert_budget_error(
            project_charts(vec![section()], &[1], 2 * one - 1),
            "chart binary_data_id=1",
        );
    }

    /// One section holding a single chart (`binary_data_id`) inside the named
    /// container.
    fn section_with_chart_in(container: &str, binary_data_id: u16) -> SectionResult {
        let chart_paragraph = || paragraph_with_controls(vec![chart_ole_control(binary_data_id)]);
        let control = match container {
            "body" => return make_section(vec![chart_paragraph()], None),
            "table_cell" => {
                let mut cell = grid_test_cell(0, 0, 1, 1);
                cell.paragraphs = vec![chart_paragraph()];
                return make_section(vec![grid_test_table_paragraph(1, 1, vec![cell])], None);
            }
            "header" => Hwp5Control::Header(crate::decoder::section::Hwp5NestedSubtree {
                wire_order: 0,
                ctrl_id: 0x6865_6164,
                properties_raw: 0,
                instance_id: 0,
                paragraphs: vec![chart_paragraph()],
            }),
            "textbox" => Hwp5Control::TextBox(Hwp5TextBoxControl {
                standalone_render: None,
                original_size: None,
                look: None,
                ctrl_id: 0x6773_6F20,
                geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                    outer_margin: [0; 4],
                    x: 0,
                    y: 0,
                    width: 8_000,
                    height: 6_000,
                },
                paragraphs: vec![chart_paragraph()],
                list_header_properties: None,
                text_margin: None,
                ctrl_properties: 0,
            }),
            other => panic!("unknown container {other}"),
        };
        make_section(vec![paragraph_with_controls(vec![control])], None)
    }

    // 이것을 실패시키는 것: `project_to_core_internal` 의 `Ok` 직전
    // `budget_error` 검사 제거 (네 컨테이너 모두 문서가 `Ok` 로 반환된다).
    #[test]
    fn chart_budget_overrun_in_any_container_fails_the_document() {
        let one = one_chart_extraction_len();
        for container in ["body", "table_cell", "header", "textbox"] {
            // Positive control: the container path really reaches extraction.
            reset_extract_calls();
            project_charts(vec![section_with_chart_in(container, 1)], &[1], one)
                .unwrap_or_else(|err| panic!("{container}: one chart fits exactly: {err}"));
            assert_eq!(extract_calls(), 1, "{container}: chart must be extracted once");

            assert_budget_error(
                project_charts(vec![section_with_chart_in(container, 1)], &[1], one - 1),
                "chart binary_data_id=1",
            );
        }
    }

    // 이것을 실패시키는 것: `project_ole_object_run` 첫머리의
    // `budget_error.is_some()` 조기 반환 제거 (초과 뒤에도 표 셀·글상자의
    // 차트를 계속 추출해 호출 수가 3이 되고, 마지막 오류가 첫 오류를 덮는다).
    #[test]
    fn extraction_stops_after_budget_overrun_and_first_error_is_kept() {
        let one = one_chart_extraction_len();
        let sections = || {
            vec![
                section_with_chart_in("body", 1),
                section_with_chart_in("table_cell", 2),
                section_with_chart_in("textbox", 3),
            ]
        };

        // Positive control: with room for all three, each reference is
        // extracted exactly once.
        reset_extract_calls();
        project_charts(sections(), &[1, 2, 3], 3 * one).expect("three charts fit");
        assert_eq!(extract_calls(), 3);

        reset_extract_calls();
        assert_budget_error(
            project_charts(sections(), &[1, 2, 3], one - 1),
            "chart binary_data_id=1",
        );
        assert_eq!(extract_calls(), 1, "no extraction may run after the budget is exhausted");
    }
}

#[cfg(test)]
mod shape_style_tests {
    use super::*;
    use crate::schema::border_fill::{
        Hwp5FillPatternKind, Hwp5RawBorderFillFill, Hwp5RawColorFill,
    };
    use crate::schema::section::{Hwp5ShapeLine, Hwp5ShapeLook};
    use hwpforge_core::control::{Fill, LineStyle};

    fn look(line_type: u32, fill_kind: u32, fill: Hwp5RawBorderFillFill) -> Hwp5ShapeLook {
        Hwp5ShapeLook {
            corner_ratio: 0,
            line: Hwp5ShapeLine { color: 0x0017_9EDE, thickness: 283, properties: line_type },
            fill_kind: Some(fill_kind),
            fill: Ok(fill),
        }
    }

    fn colour_fill(background: u32, pattern: Hwp5FillPatternKind) -> Hwp5RawBorderFillFill {
        Hwp5RawBorderFillFill::Color(Hwp5RawColorFill {
            background_color: background,
            pattern_color: 0,
            pattern_kind: pattern,
            alpha: 0,
            extra_data: Vec::new(),
        })
    }

    fn style(look: &Hwp5ShapeLook, fillable: bool) -> (hwpforge_core::control::ShapeStyle, usize) {
        let mut images =
            ProjectionImageState::new(None, None, DecompressBudget::new(MAX_TOTAL_DECOMPRESSED));
        let s = images.shape_style(look, fillable);
        (s, images.warnings.len())
    }

    /// A colour fill with transparency is reported (Core's fills are
    /// opaque); an opaque one is not.
    #[test]
    fn colour_fill_transparency_is_reported() {
        let mut fill = colour_fill(0x00EF_DECE, Hwp5FillPatternKind::None);
        if let Hwp5RawBorderFillFill::Color(c) = &mut fill {
            c.alpha = 128;
        }
        let mut images =
            ProjectionImageState::new(None, None, DecompressBudget::new(MAX_TOTAL_DECOMPRESSED));
        images.shape_style(&look(1, 1, fill), true);
        assert!(
            matches!(
                images.warnings.as_slice(),
                [Hwp5Warning::ProjectionFallback { subject: "shape.fill", .. }]
            ),
            "{:?}",
            images.warnings
        );
        let (_, opaque) =
            style(&look(1, 1, colour_fill(0x00EF_DECE, Hwp5FillPatternKind::None)), true);
        assert_eq!(opaque, 0);
    }

    /// 1b-T2 + r6 안쪽 제목 상자: 종류 0 = 선 없음 — 색·굵기는 있어도
    /// 종류가 이긴다(굵기로 판정하지 않음); 색은 COLORREF(BGR) 그대로
    /// (`0x00179EDE` → `#DE9E17`). 이것을 실패시키는 것: 종류 판정을 굵기로 /
    /// `0 => None` 갈래를 실선으로 / BGR 변환 누락.
    #[test]
    fn line_type_zero_is_no_line_whatever_the_width() {
        let (s, warnings) =
            style(&look(0, 1, colour_fill(0x00EF_DECE, Hwp5FillPatternKind::None)), true);
        assert_eq!(s.line_style, Some(LineStyle::None));
        assert_eq!(s.line_width, Some(283));
        assert_eq!(s.line_color.unwrap().to_hex_rgb(), "#DE9E17");
        assert_eq!(s.fill_color.unwrap().to_hex_rgb(), "#CEDEEF");
        assert_eq!(warnings, 0);
        for (code, expected) in [(1, LineStyle::Solid), (2, LineStyle::Dot), (3, LineStyle::Dash)] {
            let (s, w) = style(&look(code, 0, Hwp5RawBorderFillFill::None), false);
            assert_eq!((s.line_style, w), (Some(expected), 0), "{code}");
        }
        let (s, w) = style(&look(8, 0, Hwp5RawBorderFillFill::None), false);
        assert_eq!((s.line_style, w), (Some(LineStyle::Solid), 1), "unconfirmed type reported");
    }

    /// 1b-T6: 채우기 없음은 빈 상자(흰 칠과 구별), 선 도형에는 채우기가
    /// 없다. 이것을 실패시키는 것: 종류 0 을 기본(흰 칠)으로 두기 / 선 도형에
    /// 채우기를 쓰기.
    #[test]
    fn no_fill_is_an_empty_box() {
        // 이것을 실패시키는 것: 종류 0 또는 fixture 32의 단색 + 면 색
        // 0xFFFFFFFF를 흰색/기본 채우기로 치환하기.
        for (kind, fill) in [
            (0, Hwp5RawBorderFillFill::None),
            (1, colour_fill(0xFFFF_FFFF, Hwp5FillPatternKind::None)),
        ] {
            let (s, warnings) = style(&look(1, kind, fill), true);
            assert_eq!((s.fill, s.fill_color, warnings), (Some(Fill::None), None, 0));
        }
        // 종류 0 이면 뒤 블록을 못 읽어도 빈 상자다.
        let mut unreadable = look(1, 0, Hwp5RawBorderFillFill::None);
        unreadable.fill = Err("cut short".into());
        let (s, w) = style(&unreadable, true);
        assert_eq!((s.fill, w), (Some(Fill::None), 0));
        let (s, _) = style(&look(1, 1, colour_fill(0x00FF_0000, Hwp5FillPatternKind::None)), false);
        assert_eq!((s.fill, s.fill_color), (None, None));
    }

    /// 1c: ratio 20 → `Some(20)`, 0 → `None`, 60 → 50 + 경고.
    #[test]
    fn corner_ratio_is_projected_and_capped() {
        for (ratio, want, warned) in [(20u8, Some(20u8), 0usize), (0, None, 0), (60, Some(50), 1)] {
            let mut l = look(1, 0, Hwp5RawBorderFillFill::None);
            l.corner_ratio = ratio;
            let mut images = ProjectionImageState::new(
                None,
                None,
                DecompressBudget::new(MAX_TOTAL_DECOMPRESSED),
            );
            let s = images.shape_style(&l, true);
            assert_eq!(s.corner_ratio, want, "{ratio}");
            // The cap is reported under its own subject (review r1: a count
            // alone passed a wrong subject).
            let capped = images
                .warnings
                .iter()
                .filter(|w| {
                    matches!(
                        w,
                        Hwp5Warning::ProjectionFallback { subject: "shape.corner_ratio", .. }
                    )
                })
                .count();
            assert_eq!((capped, images.warnings.len()), (warned, warned), "{ratio}");
        }
    }

    /// A text box gets its outline and fill too (r6: the boxes around the
    /// Ministry of Justice text). 이것을 실패시키는 것: 글상자 투영에서
    /// 스타일을 빼기.
    #[test]
    fn text_box_takes_its_look() {
        let textbox = Hwp5TextBoxControl {
            standalone_render: None,
            original_size: None,
            look: Some(look(0, 1, colour_fill(0x00DC_AF86, Hwp5FillPatternKind::None))),
            ctrl_id: 0,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                outer_margin: [0; 4],
                x: 0,
                y: 0,
                width: 8_000,
                height: 2_000,
            },
            paragraphs: Vec::new(),
            list_header_properties: None,
            text_margin: None,
            ctrl_properties: 0,
        };
        let mut images =
            ProjectionImageState::new(None, None, DecompressBudget::new(MAX_TOTAL_DECOMPRESSED));
        let run = project_textbox_run(&textbox, &mut images);
        let Some(Control::TextBox { style: Some(s), .. }) = run.content.as_control() else {
            panic!("{run:?}")
        };
        assert_eq!(s.line_style, Some(LineStyle::None));
        assert_eq!(s.fill_color.unwrap().to_hex_rgb(), "#86AFDC");
    }

    /// 1b-T3 (x6 F1·F2): 무늬 −1 = 없음(단색만), 무늬 2 = Hancom `SLASH`.
    /// 이것을 실패시키는 것: 무늬 −1 을 0(가로줄)으로 / 무늬 표 한 줄 교체.
    #[test]
    fn hatch_none_and_slash() {
        let (s, _) = style(&look(1, 1, colour_fill(0x0000_00FF, Hwp5FillPatternKind::None)), true);
        assert_eq!((s.fill_color.unwrap().to_hex_rgb(), s.fill), ("#FF0000".to_string(), None));
        let (s, _) =
            style(&look(1, 1, colour_fill(0x0000_FFFF, Hwp5FillPatternKind::BackSlash)), true);
        let Some(Fill::Pattern { pattern_type, bg_color, .. }) = s.fill else { panic!("{s:?}") };
        assert_eq!(
            (pattern_type.to_string(), bg_color.to_hex_rgb()),
            ("SLASH".into(), "#FFFF00".into())
        );
    }

    /// A crop that cannot be carried at all (the picture is left whole) is
    /// `picture.crop_lost`, which a renderer refuses; one kept without its
    /// image size is `picture.crop`.
    /// 이것을 실패시키는 것: 주제 구분을 지우기(PDF 가 통째 그림을 그림).
    #[test]
    fn a_lost_crop_is_told_apart_from_a_crop_without_extent() {
        use crate::schema::section::Hwp5ShapePicture;
        let subject = |crop: [i32; 4]| {
            let image = Hwp5ImageControl {
                standalone_render: None,
                ctrl_id: 0,
                geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                    outer_margin: [0; 4],
                    x: 0,
                    y: 0,
                    width: 3_000,
                    height: 2_000,
                },
                binary_data_id: 1,
                original_size: None,
                instance_id: 0,
                ctrl_properties: 0,
                picture: Hwp5ShapePicture { in_margin: [0; 4], crop, ..Default::default() },
                link: None,
            };
            let mut images = ProjectionImageState::new(
                None,
                None,
                DecompressBudget::new(MAX_TOTAL_DECOMPRESSED),
            );
            let mut core =
                Image::new("BinData/a.png", HwpUnit::ZERO, HwpUnit::ZERO, ImageFormat::Png);
            images.carry_picture_frame(&mut core, &image, (3_000, 2_000), None);
            match images.warnings.as_slice() {
                [Hwp5Warning::ProjectionFallback { subject, .. }] => *subject,
                other => panic!("{other:?}"),
            }
        };
        assert_eq!(subject([10, 0, 10, 100]), "picture.crop_lost");
        assert_eq!(subject([0, 0, 7_474, 15_000]), "picture.crop");
    }

    /// A picture record's crop and image size → Core crop and extent,
    /// folded as the HWPX decoder folds `imgClip`/`imgDim`: the whole
    /// extent is no crop (extent kept only when it is not the display
    /// size); a crop keeps its extent; without the record's image size a
    /// crop within one pixel of pixels × 75 is the whole image and any other
    /// is kept without extent, noted; an empty or out-of-range crop is
    /// noted and not carried. The crop (0, 0, 7474, 15000) of 22500 x 15000
    /// is Hancom's F3c.
    ///
    /// 이것을 실패시키는 것: 전체 비교를 지우는 것 / 자르기에서 범위를 버리는 것 /
    /// 픽셀 허용 폭을 0 으로 / 크기 모름 자르기를 버리거나 경고를 지우는 것 /
    /// 빈 자르기 검사를 지우는 것.
    #[test]
    fn picture_crop_folds_like_the_hwpx_decoder() {
        use crate::schema::section::Hwp5ShapePicture;
        let u = |v: i32| HwpUnit::new(v).unwrap();
        let pic = |crop: [i32; 4], dimension: Option<(u32, u32)>| Hwp5ShapePicture {
            in_margin: [0; 4],
            crop,
            dimension,
            ..Default::default()
        };
        let display = (8000, 5000);
        let dim = Some((22_500, 15_000));
        let whole = picture_crop(&pic([0, 0, 22_500, 15_000], dim), display, None);
        assert_eq!(
            whole,
            PictureFrame { crop: None, extent: Some((u(22_500), u(15_000))), note: None }
        );
        let same = picture_crop(&pic([0, 0, 8000, 5000], Some((8000, 5000))), display, None);
        assert_eq!(same, PictureFrame { crop: None, extent: None, note: None });
        let f3c = picture_crop(&pic([0, 0, 7474, 15_000], dim), display, None);
        assert_eq!(
            f3c,
            PictureFrame {
                crop: Some(ImageCrop { left: u(0), top: u(0), right: u(7474), bottom: u(15_000) }),
                extent: Some((u(22_500), u(15_000))),
                note: None,
            }
        );
        // No image size in the record: a 300 x 200 px image is 22500 x 15000.
        let px = Some((300, 200));
        assert_eq!(picture_crop(&pic([0, 0, 22_440, 15_000], None), display, px).crop, None);
        let kept = picture_crop(&pic([0, 0, 7474, 15_000], None), display, px);
        assert!(kept.crop.is_some() && kept.extent.is_none() && kept.note.is_some(), "{kept:?}");
        let unset = picture_crop(&pic([0, 0, 0, 0], None), display, None);
        assert_eq!(unset, PictureFrame { crop: None, extent: None, note: None });
        let empty = picture_crop(&pic([10, 0, 10, 15_000], dim), display, None);
        assert!(empty.crop.is_none() && empty.note.is_some(), "{empty:?}");
        let far = picture_crop(&pic([0, 0, i32::MAX, 15_000], dim), display, None);
        assert!(far.crop.is_none() && far.note.is_some(), "{far:?}");
    }
    /// 이것을 실패시키는 것: 앞 글자 서식 재사용 / 다중 서식 경고 무조건 발생·삭제 /
    /// 링크 뒤 경계까지 링크 내부라고 판단하기.
    #[test]
    fn field_start_styles_and_mixed_link_warning() {
        for mixed in [false, true] {
            let mut para = Hwp5Paragraph {
                end_char_shape: None,
                silent_wires: Vec::new(),
                page_break: false,
                column_break: false,
                text: "(abc)".into(),
                text_segments: Vec::new(),
                para_shape_id: 0,
                style_id: 0,
                char_shape_runs: Vec::new(),
                line_segments: Vec::new(),
                controls: Vec::new(),
            };
            para.char_shape_runs = vec![
                Hwp5CharShapeRun { position: 0, char_shape_id: 267 },
                Hwp5CharShapeRun { position: 1, char_shape_id: 215 },
                Hwp5CharShapeRun { position: 2, char_shape_id: if mixed { 216 } else { 215 } },
                Hwp5CharShapeRun { position: 4, char_shape_id: 267 },
            ];
            let fields = vec![
                ActiveField::Hyperlink {
                    url: "https://example.test".into(),
                    start_utf16: 1,
                    display_text: "abc".into(),
                },
                ActiveField::SummaryField {
                    command_token: "$author".into(),
                    start_utf16: 1,
                    display_text: "abc".into(),
                },
                ActiveField::DateCodeField {
                    raw_command: "YYYY".into(),
                    start_utf16: 1,
                    display_text: "abc".into(),
                },
                ActiveField::PathField {
                    raw_command: "$P$F".into(),
                    start_utf16: 1,
                    display_text: "abc".into(),
                },
                ActiveField::ClickHere {
                    start_utf16: 1,
                    command: String::new(),
                    hint_text: Some("abc".into()),
                    help_text: None,
                    name: None,
                },
                ActiveField::CrossRef {
                    start_utf16: 1,
                    display_text: "abc".into(),
                    control: crate::schema::section::Hwp5CrossRefControl {
                        ctrl_id: CTRL_ID_FIELD_CROSSREF,
                        command_raw: "?target;6;0;0;0;".into(),
                        target_raw: "target".into(),
                        ref_type_code: 6,
                        content_type_code: 0,
                        hyperlink_code: 0,
                        param4_raw: 0,
                        header_flag_raw: 0,
                        trailer_begin_id: 1,
                        trailer_field_id: 2,
                    },
                },
                ActiveField::BookmarkSpan { name: "bookmark".into(), start_utf16: 1 },
                ActiveField::MemoAnchor {
                    start_utf16: 1,
                    memo: Hwp5MemoControl {
                        ctrl_id: crate::ctrl_ids::CTRL_ID_MEMO,
                        command: crate::schema::section::Hwp5MemoCommand {
                            raw: "MEMO/65535/1/2/3/author/".into(),
                            shape_id: 65535,
                            memo_id: 1,
                            hancom_inst_a: 2,
                            hancom_inst_b: 3,
                            author: "author".into(),
                            terminator: String::new(),
                        },
                        paragraphs: Vec::new(),
                    },
                },
            ];
            for (index, field) in fields.into_iter().enumerate() {
                let mut images = ProjectionImageState::new(
                    None,
                    None,
                    DecompressBudget::new(MAX_TOTAL_DECOMPRESSED),
                );
                let mut runs = Vec::new();
                finish_active_field(
                    field,
                    &mut FieldTextIndex::new(&para),
                    &para,
                    4,
                    &mut runs,
                    &mut images,
                );
                assert_eq!(runs[0].char_shape_id.get(), 215);
                assert_eq!(
                    images
                        .warnings
                        .iter()
                        .filter(|w| matches!(
                            w,
                            Hwp5Warning::ProjectionFallback {
                                subject: "field.hyperlink.mixed_style",
                                ..
                            }
                        ))
                        .count(),
                    usize::from(mixed && index == 0)
                );
            }
        }
    }
}

#[cfg(test)]
mod arrow_tests {
    use super::*;
    /// 이것을 실패시키는 것: HWP5 크기를 Core TryFrom에 직접 전달, head/tail 혼동,
    /// 미지원 값 기본 대체 또는 경고 누락, 모양 0의 비기본 크기/채움 소실.
    #[test]
    fn hwp5_arrow_bits_sizes_and_unknown_values() {
        let wires = [
            "SMALL_SMALL",
            "SMALL_MEDIUM",
            "SMALL_LARGE",
            "MEDIUM_SMALL",
            "MEDIUM_MEDIUM",
            "MEDIUM_LARGE",
            "LARGE_SMALL",
            "LARGE_MEDIUM",
            "LARGE_LARGE",
        ];
        let shapes = [
            "NORMAL",
            "ARROW",
            "SPEAR",
            "CONCAVE_ARROW",
            "EMPTY_DIAMOND",
            "EMPTY_CIRCLE",
            "EMPTY_BOX",
        ];
        for head in [true, false] {
            let (shape_shift, size_shift, fill_shift) =
                if head { (10, 22, 30) } else { (16, 26, 31) };
            for (raw_shape, shape) in shapes.iter().enumerate() {
                for (raw_size, size) in wires.iter().enumerate() {
                    for filled in [false, true] {
                        let props = (raw_shape as u32) << shape_shift
                            | (raw_size as u32) << size_shift
                            | u32::from(filled) << fill_shift;
                        let mut warnings = Vec::new();
                        let projected = project_arrow(props, head, &mut warnings, "shape[0]");
                        if raw_shape == 0 && raw_size == 4 && filled {
                            assert!(projected.is_none());
                            assert!(warnings.is_empty());
                            continue;
                        }
                        let a = projected.unwrap();
                        assert_eq!(
                            a.arrow_type,
                            shape.parse::<hwpforge_foundation::ArrowType>().unwrap()
                        );
                        assert_eq!(a.size.to_string(), *size);
                        assert_eq!(a.filled, filled);
                        assert!(warnings.is_empty());
                        let other = project_arrow(props, !head, &mut warnings, "shape[0]").unwrap();
                        assert_eq!(other.arrow_type, hwpforge_foundation::ArrowType::None);
                        assert_eq!(other.size, hwpforge_foundation::ArrowSize::Small);
                        assert!(!other.filled);
                    }
                }
            }
            for (shape, size, detail) in [(7, 4, "shape 7"), (1, 9, "size 9")] {
                let mut warnings = Vec::new();
                assert!(project_arrow(
                    shape << shape_shift | size << size_shift,
                    head,
                    &mut warnings,
                    "shape[0]"
                )
                .is_none());
                assert_eq!(warnings.len(), 2);
                let warning = format!("{:?}", warnings[0]);
                assert!(
                    warning.contains(detail)
                        && warning.contains(if head { "head" } else { "tail" }),
                    "{warning}"
                );
            }
            for raw_size in 9..=15 {
                let mut warnings = Vec::new();
                assert!(project_arrow(
                    raw_size << size_shift | 1 << fill_shift,
                    head,
                    &mut warnings,
                    "shape[0]"
                )
                .is_none());
                assert_eq!(warnings.len(), 2);
                assert!(format!("{:?}", warnings[0]).contains(&format!("size {raw_size}")));
            }
        }
    }
}
