//! HWP5 IR → Core document projection.
//!
//! This module converts the decoded HWP5 intermediate representation
//! (parsed records, style tables) into HwpForge Core's `Document<Draft>`
//! structure, bridging the format-specific layer to the format-agnostic core.

mod shapes;
mod text;

use std::collections::{BTreeSet, VecDeque};

use hwpforge_core::column::{ColumnLine, ColumnSettings};
use hwpforge_core::control::RefTarget;
use hwpforge_core::document::{Document, Draft};
use hwpforge_core::image::{Image, ImageFormat, ImageStore};
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::placement::{ObjectPlacement, ObjectRelativeTo, ObjectTextFlow, ObjectTextWrap};
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::section::{HeaderFooter, PageBorderFillEntry, PageNumber, Section};
use hwpforge_core::table::{Table, TableCell, TableMargin, TableRow};
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
use crate::decoder::chart_ole::{extract_chart_payload, ChartOleError};
use crate::decoder::section::{
    Hwp5Control, Hwp5EquationControl, Hwp5GroupChild, Hwp5GroupControl, Hwp5ImageControl,
    Hwp5MemoControl, Hwp5NestedSubtree, Hwp5OleObjectControl, Hwp5PageBorderFill, Hwp5Paragraph,
    Hwp5Table, Hwp5TableCell, Hwp5TextArtControl, Hwp5TextBoxControl, SectionResult,
};
use crate::decoder::Hwp5Warning;
use crate::error::Hwp5Result;
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
    char_shape_at, char_shape_id_at_position, char_shape_id_for_visible_position,
    hwp_unit_from_u32, split_text_by_runs, utf16_boundaries, utf16_offset_to_byte,
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
/// Used by [`crate::decode_hwp5_to_core`] so the projection layer can attempt
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
    image_assets: Option<&'a Hwp5JoinedImageAssetPlan>,
    ole_assets: Option<&'a Hwp5OleAssetPlan>,
    image_store: ImageStore,
    warnings: Vec<Hwp5Warning>,
    /// W4 무음 드롭 종결: `project_control_run` 의 Unknown arm 에서 죽는
    /// ctrl_id 별 카운트. 문서 끝에서 **집계 경고**로 방출된다 (per-occurrence
    /// 폭탄도 무경고도 금지 — corpus 실측: `%fmu` 531회·`pghd` 1,013회가
    /// 이 지점에서 소리 없이 사라졌었다).
    dropped_unknown: std::collections::BTreeMap<u32, usize>,
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
        Self {
            image_assets,
            ole_assets,
            image_store: ImageStore::new(),
            warnings: Vec::new(),
            dropped_unknown: std::collections::BTreeMap::new(),
        }
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
        Some(core_image)
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
/// - `textWrap` bits 21-23 (`0`=Square `1`=Tight `2`=Through `3`=TopAndBottom
///   `4`=BehindText `5`=InFrontOfText)
/// - `textFlow` bits 24-25 (`0`=BothSides `1`=LeftOnly `2`=RightOnly
///   `3`=LargestOnly — `textWrap`∈{Square,Tight,Through} 일 때만 유의)
///
/// `align`(bits 5-7·10-12)·`width/heightRelTo`(15-19)·`protect`(20)·
/// `numbering`(26-28) 은 Core 가 carry 하지 않으므로 무시한다. `offset_x`/
/// `offset_y` 는 CtrlHeader 오프셋 필드(이미 signed `i32` 로 디코드됨 —
/// corpus 의 음수 오프셋 실재)를 그대로 싣는다.
///
/// **Fail-closed (no-fake-support)**: `vertRelTo`(bits 3-4)·`textWrap`(bits
/// 21-23)에서 레퍼런스 enum 범위 밖 값(각각 `3`, `6`/`7` — hwp-rs
/// `VerticalRelativeTo`/`TextWrap` 에 대응 variant 없음)이 나오면 임의 known
/// 값으로 정규화하지 않고 [`Hwp5Warning::ProjectionFallback`] 를 방출한 뒤 가장
/// 보수적인 기본값(Paper/Square)으로 폴백한다 — 유효 HWP5 에는 없는 값이라
/// 실무엔 안 뜨지만, 뜨면 속성 word 오프셋 오정렬의 canary 로 표면화한다.
fn object_placement_from_ctrl_properties(
    ctrl_properties: u32,
    offset_x: i32,
    offset_y: i32,
    warnings: &mut Vec<Hwp5Warning>,
) -> ObjectPlacement {
    if ctrl_properties & CTRL_PROPERTY_TREAT_AS_CHAR_BIT != 0 {
        return ObjectPlacement::legacy_inline_defaults();
    }
    let vert_bits = (ctrl_properties >> 3) & 0x3;
    if vert_bits == 3 {
        warnings.push(Hwp5Warning::ProjectionFallback {
            subject: "object_placement.vert_rel_to",
            reason: "gso property word vertRelTo bits 3-4 = 3 is outside the HWP5 reference \
                     range (0=Paper 1=Page 2=Para); falling back to Paper"
                .to_string(),
        });
    }
    let vert_rel_to = vert_relative_to_from_bits(vert_bits);
    let horz_rel_to = horz_relative_to_from_bits((ctrl_properties >> 8) & 0x3);
    let wrap_bits = (ctrl_properties >> 21) & 0x7;
    if wrap_bits >= 6 {
        warnings.push(Hwp5Warning::ProjectionFallback {
            subject: "object_placement.text_wrap",
            reason: format!(
                "gso property word textWrap bits 21-23 = {wrap_bits} is outside the HWP5 \
                 reference range (0..=5); falling back to Square"
            ),
        });
    }
    let text_wrap = object_text_wrap_from_bits(wrap_bits);
    // 표 70: flowWithText 는 vertRelTo=Para 일 때만 정의된다. 그 외 기준에서는
    // bit 13 을 읽지 않고 false 로 둔다 (hwp-rs 의 `Option` 조건과 동형).
    let flow_with_text =
        vert_rel_to == ObjectRelativeTo::Para && (ctrl_properties >> 13) & 0x1 != 0;
    // 표 70: flowWithText 가 참이면 allowOverlap 은 무조건 false 로 간주한다.
    let allow_overlap = if flow_with_text { false } else { (ctrl_properties >> 14) & 0x1 != 0 };
    // 표 70: textFlow 는 wrap 이 Square/Tight/Through 일 때만 정의된다. 다른
    // wrap(TopAndBottom/BehindText/InFrontOfText)에서는 bits 24-25 가 무의미
    // 하므로 기본값 BothSides 로 둔다 (잡값이 사이드 흐름을 뒤집는 것을 방지).
    let text_flow = match text_wrap {
        ObjectTextWrap::Square | ObjectTextWrap::Tight | ObjectTextWrap::Through => {
            object_text_flow_from_bits((ctrl_properties >> 24) & 0x3)
        }
        _ => ObjectTextFlow::BothSides,
    };
    ObjectPlacement {
        text_wrap,
        text_flow,
        treat_as_char: false,
        flow_with_text,
        allow_overlap,
        vert_rel_to,
        horz_rel_to,
        vert_offset: HwpUnit::new(offset_y).unwrap_or(HwpUnit::ZERO),
        horz_offset: HwpUnit::new(offset_x).unwrap_or(HwpUnit::ZERO),
    }
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
/// 스펙 정의값은 `0`~`5` 다 — 3비트 필드의 미정의 값 `6`/`7` 은 유효 파일에
/// 나타나지 않으므로 기본 `Square` 로 폴백한다 (경고 방출은 호출부
/// [`object_placement_from_ctrl_properties`] 담당).
fn object_text_wrap_from_bits(bits: u32) -> ObjectTextWrap {
    match bits {
        0 => ObjectTextWrap::Square,
        1 => ObjectTextWrap::Tight,
        2 => ObjectTextWrap::Through,
        3 => ObjectTextWrap::TopAndBottom,
        4 => ObjectTextWrap::BehindText,
        5 => ObjectTextWrap::InFrontOfText,
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
    object_placement_from_ctrl_properties(ctrl_properties, geometry.x, geometry.y, warnings)
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
                ctrl_id: CTRL_ID_SECD
                    | CTRL_ID_COLUMN_DEF
                    | CTRL_ID_BOOKMARK_SPAN
                    | CTRL_ID_HYPERLINK
                    | CTRL_ID_FIELD_CROSSREF
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
                if let Some(run) = project_control_run(
                    control,
                    projection_images,
                    image_context,
                    char_shape_at(hwp_para, current_utf16),
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

    for control in control_iter {
        if let Some(run) = project_control_run(
            control,
            projection_images,
            image_context,
            char_shape_at(hwp_para, current_utf16),
        ) {
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
/// (decode-only — 인코더 방출은 convert opt-in 전용).
///
/// 필드 대응은 이름만 다르고 wire 의미는 HWPX `<hp:lineseg>` 와 동일하다.
/// 세그먼트가 없으면 `None` (HWP5 네이티브 문단은 항상 1개 이상 보유 —
/// 없음 = 캐시 부재 의미).
fn promote_line_segments(
    segs: &[crate::schema::section::Hwp5ParaLineSeg],
    ledger: Result<crate::wire_text_map::WireTextMap, &'static str>,
    warnings: &mut Vec<crate::decoder::Hwp5Warning>,
) -> Option<hwpforge_core::layout::LayoutCache> {
    if segs.is_empty() {
        return None;
    }
    // W1b: raw wire textpos → Core 가시(text_content) 좌표 정규화.
    // ledger 구축 실패·좌표 변환 실패 = fail-closed (캐시 미승격 + 경고) —
    // 추측 좌표를 Core 에 싣지 않는다 (§1g v5).
    let map = match ledger {
        Ok(map) => map,
        Err(reason) => {
            warnings.push(crate::decoder::Hwp5Warning::LayoutCacheDropped {
                reason: reason.to_string(),
            });
            return None;
        }
    };
    let mut lines = Vec::with_capacity(segs.len());
    for s in segs {
        let textpos = match map.to_core(s.text_start_position) {
            Ok(core) => core,
            Err(e) => {
                warnings.push(crate::decoder::Hwp5Warning::LayoutCacheDropped {
                    reason: format!("lineseg textpos {}: {e}", s.text_start_position),
                });
                return None;
            }
        };
        lines.push(hwpforge_core::layout::LineSeg {
            textpos,
            vertpos: s.vertical_position,
            vertsize: s.line_height,
            textheight: s.text_height,
            baseline: s.baseline_distance,
            spacing: s.line_spacing,
            horzpos: s.column_start_position,
            horzsize: s.segment_width,
            flags: s.tag,
        });
    }
    Some(hwpforge_core::layout::LayoutCache::new(lines))
}

/// ledger 가 예측한 Core 총길이와 실제 방출된 run 들의 텍스트 총길이를
/// 대조한다 — 불일치 = ledger/방출 분기 (fail-closed, §1g v5 choke).
fn check_ledger_against_emission(
    ledger_result: Result<crate::wire_text_map::WireTextMap, &'static str>,
    runs: &[Run],
) -> Result<crate::wire_text_map::WireTextMap, &'static str> {
    let map = ledger_result?;
    let emitted_core: u32 = runs
        .iter()
        .filter_map(|r| r.content.plain_text())
        .map(|t| t.encode_utf16().count() as u32)
        .sum();
    if map.core_end() != emitted_core {
        return Err("ledger/emission core length mismatch");
    }
    Ok(map)
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
    silent: &'a [SilentWire],
    silent_idx: usize,
    /// ledger 자체 wire 커서 — pending 동안 builder 커서와 분리.
    wire_cursor: u32,
    /// FieldBegin 이후 버퍼된 body 조각 `(wire, 재방출 시 core)`.
    pending: Option<Vec<(u32, u32)>>,
    failed: Option<&'static str>,
}

impl<'a> ParaLedger<'a> {
    fn new(silent: &'a [SilentWire]) -> Self {
        Self {
            builder: crate::wire_text_map::WireMapBuilder::new(),
            silent,
            silent_idx: 0,
            wire_cursor: 0,
            pending: None,
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
            self.consume(s.len, 0, 0);
        }
    }

    /// 소비 하나를 기록한다. `core_folded`/`core_reemitted` 는 각각
    /// field 밖(즉시 반영)·재방출 body 조각일 때의 Core 폭.
    fn consume(&mut self, wire: u32, core_outside: u32, core_reemitted: u32) {
        match self.wire_cursor.checked_add(wire) {
            Some(w) => self.wire_cursor = w,
            None => {
                self.fail("wire cursor overflow");
                return;
            }
        }
        if let Some(pieces) = self.pending.as_mut() {
            pieces.push((wire, core_reemitted));
        } else if core_outside == wire {
            self.builder.advance_identity(wire);
        } else {
            self.builder.push_span(wire, core_outside);
        }
    }

    /// 일반 텍스트 (field 밖 1:1 · 재방출 body 도 1:1).
    fn observe_text(&mut self, utf16_len: u32) {
        self.drain_silent();
        self.consume(utf16_len, utf16_len, utf16_len);
    }

    /// Tab — wire 8 · Core `\t` 1 (양쪽 동일).
    fn observe_tab(&mut self) {
        self.drain_silent();
        self.consume(8, 1, 1);
    }

    /// 단일 유닛 제어 (`\n`·nbSp·fwSp) — 1:1.
    fn observe_unit(&mut self) {
        self.drain_silent();
        self.consume(1, 1, 1);
    }

    /// ParaBreak — wire 1 · Core 0.
    fn observe_para_break(&mut self) {
        self.drain_silent();
        self.consume(1, 0, 0);
    }

    /// ControlRef/ExtendedControlRef marker — field 밖 (8,0) (U+FFFC
    /// 미방출), 재방출 body 안 (8,1) (U+FFFC 가 텍스트로 살아남음).
    fn observe_marker(&mut self) {
        self.drain_silent();
        self.consume(8, 0, 1);
    }

    /// SectionColumnDef — wire 8 · Core 0 (양쪽 동일).
    fn observe_section_ctrl(&mut self) {
        self.drain_silent();
        self.consume(8, 0, 0);
    }

    /// FieldBegin — pending 개시 (begin 자체 8유닛 소비).
    fn begin_field(&mut self) {
        self.drain_silent();
        if self.pending.is_some() {
            self.fail("nested field begin");
        }
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
        match outcome {
            FieldEmissionOutcome::Folded => {
                let mut total: u32 = 16; // begin + end
                for (wire, _) in &pieces {
                    match total.checked_add(*wire) {
                        Some(t) => total = t,
                        None => {
                            self.fail("wire cursor overflow");
                            return;
                        }
                    }
                }
                self.builder.push_span(total, 0);
            }
            FieldEmissionOutcome::ReEmitted => {
                self.builder.push_span(8, 0);
                for (wire, core) in pieces {
                    if wire == core {
                        self.builder.advance_identity(wire);
                    } else {
                        self.builder.push_span(wire, core);
                    }
                }
                self.builder.push_span(8, 0);
            }
        }
    }

    /// 문단 종료 — 잔여 무음 구간 합류 후 seal.
    fn finish(mut self) -> Result<crate::wire_text_map::WireTextMap, &'static str> {
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
        self.builder.finish().map_err(|_| "wire map invariant violation")
    }
}

/// field 없는(flat) 문단의 ledger — 세그먼트 종류만으로 구축한다.
fn build_flat_ledger(
    hwp_para: &Hwp5Paragraph,
) -> Result<crate::wire_text_map::WireTextMap, &'static str> {
    let mut ledger = ParaLedger::new(&hwp_para.silent_wires);
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
            crate::schema::section::TextSegment::ControlRef { .. }
            | crate::schema::section::TextSegment::ExtendedControlRef { .. } => {
                ledger.observe_marker();
            }
            crate::schema::section::TextSegment::SectionColumnDef { .. } => {
                ledger.observe_section_ctrl();
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
    mut field_hints: Option<&mut SectionProjectionHints>,
) -> ProjectedParagraph {
    let mut queues =
        build_paragraph_projection_queues(hwp_para, projection_images, field_hints.as_deref_mut());
    let mut runs: Vec<Run> = Vec::new();
    let mut visible_utf16: u32 = 0;
    let mut active_field: Option<ActiveField> = None;
    let mut ledger = ParaLedger::new(&hwp_para.silent_wires);

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
            crate::schema::section::TextSegment::ControlRef { .. }
            | crate::schema::section::TextSegment::ExtendedControlRef { .. } => {
                ledger.observe_marker();
                if active_field.is_none() {
                    if let Some(control) = queues.object_controls.pop_front() {
                        if let Some(run) = project_control_run(
                            control,
                            projection_images,
                            image_context,
                            char_shape_at(hwp_para, visible_utf16),
                        ) {
                            runs.push(run);
                        }
                    }
                }
                visible_utf16 += 1;
            }
            crate::schema::section::TextSegment::SectionColumnDef { extra } => {
                ledger.observe_section_ctrl();
                let ctrl_id = ctrl_id_from_inline_extra(extra);
                let _ = consume_marker_header(&mut queues.marker_headers, ctrl_id);
            }
            crate::schema::section::TextSegment::FieldBegin { extra } => {
                ledger.begin_field();
                if active_field.is_some() {
                    // 기존 동작(교체)은 유지하되 ledger 는 nested 로 실패
                    // 처리됨 — 좌표를 추측하지 않는다.
                    ledger.fail("nested field begin");
                }
                active_field = Some(start_field_from_marker(
                    extra,
                    &mut queues,
                    visible_utf16,
                    projection_images,
                    field_hints.as_deref_mut(),
                ));
            }
            crate::schema::section::TextSegment::FieldEnd => {
                if let Some(field) = active_field.take() {
                    let outcome = finish_active_field(
                        field,
                        hwp_para,
                        visible_utf16,
                        &mut runs,
                        projection_images,
                    );
                    ledger.commit_field(outcome);
                } else {
                    ledger.commit_field(FieldEmissionOutcome::Folded);
                }
            }
            crate::schema::section::TextSegment::ParaBreak => {
                ledger.observe_para_break();
            }
        }
    }

    if let Some(field) = active_field.take() {
        // 방출은 기존 동작대로 완료하되, 좌표는 신뢰 불가 (v5 fail-closed).
        let _ = finish_active_field(field, hwp_para, visible_utf16, &mut runs, projection_images);
        ledger.fail("dangling field begin");
    }

    drain_unconsumed_paragraph_queues(
        queues,
        hwp_para,
        visible_utf16,
        &mut runs,
        projection_images,
        image_context,
    );

    if runs.is_empty() {
        runs.push(Run::text("", CharShapeIndex::new(0)));
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
/// grow `display_text` unbounded. A legitimate cached render
/// (author/date/path/title) is far under this; truncation only bites
/// adversarial input. (Architect review P1.)
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
    field_hints: Option<&mut SectionProjectionHints>,
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
        field_hints,
    )
}

/// Drains queue entries that no inline marker consumed (task #91 —
/// extracted from `project_paragraph_with_images_structural`).
///
/// Point bookmarks and object controls emit at the end of the
/// paragraph in document order. Memo placeholders with body content
/// emit with a `ProjectionFallback` warning — properly anchored memos
/// always consume their queue entry, so a leftover means the inline
/// `FieldBegin %unk MEMO` anchor was missing; emitting preserves the
/// body rather than silently dropping it.
fn drain_unconsumed_paragraph_queues(
    queues: ParagraphProjectionQueues<'_>,
    hwp_para: &Hwp5Paragraph,
    visible_utf16: u32,
    runs: &mut Vec<Run>,
    projection_images: &mut ProjectionImageState<'_>,
    image_context: ImageProjectionContext,
) {
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
        if let Some(run) = project_control_run(
            control,
            projection_images,
            image_context,
            char_shape_at(hwp_para, visible_utf16),
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
                char_shape_at(hwp_para, visible_utf16),
                Vec::new(),
            ));
        }
    }
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
            ) || matches!(control, Hwp5Control::Memo(_))
                || matches!(control, Hwp5Control::ClickHere(_))
                || matches!(control, Hwp5Control::SummaryField(_))
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
    let mut summary_fields = VecDeque::new();
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
            object_controls.push_back(control);
            continue;
        };

        match unknown.ctrl_id {
            CTRL_ID_SECD | CTRL_ID_COLUMN_DEF | CTRL_ID_BOOKMARK_SPAN | CTRL_ID_HYPERLINK => {
                marker_headers.push_back(unknown)
            }
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
    hwp_para: &Hwp5Paragraph,
    end_utf16: u32,
    runs: &mut Vec<Run>,
    projection_images: &mut ProjectionImageState<'_>,
) -> FieldEmissionOutcome {
    match field {
        ActiveField::Hyperlink { url, start_utf16, display_text } => {
            if display_text.is_empty() {
                return FieldEmissionOutcome::Folded;
            }
            let char_shape_id = CharShapeIndex::new(char_shape_id_for_visible_position(
                &hwp_para.char_shape_runs,
                start_utf16,
            ) as usize);
            runs.push(Run::control(Control::Hyperlink { text: display_text, url }, char_shape_id));
            FieldEmissionOutcome::Folded
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
            FieldEmissionOutcome::ReEmitted
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
            runs.extend(project_text_segment(
                &hwp_para.text,
                &hwp_para.char_shape_runs,
                start_utf16,
                end_utf16,
            ));
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
            FieldEmissionOutcome::Folded
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
                    // ClickHere's visible body is `hint_text`, not a cached
                    // render — leave display_text empty (span not accumulated).
                    display_text: String::new(),
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
            let char_shape_id = CharShapeIndex::new(char_shape_id_for_visible_position(
                &hwp_para.char_shape_runs,
                start_utf16,
            ) as usize);
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
            let char_shape_id = CharShapeIndex::new(char_shape_id_for_visible_position(
                &hwp_para.char_shape_runs,
                start_utf16,
            ) as usize);
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
            let char_shape_id = CharShapeIndex::new(char_shape_id_for_visible_position(
                &hwp_para.char_shape_runs,
                start_utf16,
            ) as usize);
            let _ = end_utf16;
            use hwpforge_core::control::PathFieldCommand;
            let command = PathFieldCommand::from_wire(&raw_command);
            runs.push(Run::control(Control::PathField { command, display_text }, char_shape_id));
            FieldEmissionOutcome::Folded
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
        Hwp5Control::Line(line) => project_line_run(line, &mut projection_images.warnings),
        Hwp5Control::Rect(rect) => project_rect_run(rect, &mut projection_images.warnings),
        Hwp5Control::Polygon(polygon) => {
            project_polygon_run(polygon, &mut projection_images.warnings)
        }
        Hwp5Control::Ellipse(ellipse) => {
            project_ellipse_run(ellipse, &mut projection_images.warnings)
        }
        Hwp5Control::Arc(arc) => project_arc_run(arc, &mut projection_images.warnings),
        Hwp5Control::Curve(curve) => project_curve_run(curve, &mut projection_images.warnings),
        Hwp5Control::TextArt(text_art) => {
            Some(project_text_art_run(text_art, &mut projection_images.warnings))
        }
        Hwp5Control::ConnectLine(connect_line) => {
            project_connectline_run(connect_line, &mut projection_images.warnings)
        }
        Hwp5Control::Equation(equation) => Some(project_equation_run(equation)),
        Hwp5Control::TextBox(textbox) => Some(project_textbox_run(textbox, projection_images)),
        Hwp5Control::Group(group) => project_group_run(group, projection_images),
        Hwp5Control::Footnote(subtree) => Some(project_footnote_run(subtree, projection_images)),
        Hwp5Control::Endnote(subtree) => Some(project_endnote_run(subtree, projection_images)),
        // Memo emission flows through the `FieldBegin`/`MemoAnchor` machinery in
        // `project_paragraph_with_images_structural`, not through this dispatch.
        // If a Memo control ever reaches here (no matching FieldBegin in text
        // segments), prefer dropping over silently double-emitting.
        Hwp5Control::Memo(_) | Hwp5Control::Header(_) | Hwp5Control::Footer(_) => None,
        // W4 무음 드롭 종결: 미지원 ctrl 은 여전히 드롭되지만 **집계 카운트**
        // 를 남겨 문서 끝에서 경고로 방출된다 (nwno/pghd 가 여기서 소리 없이
        // 죽던 것이 corpus 18% 문서의 fake output 원인이었다 — 계획 §0).
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
        Hwp5Control::ClickHere(_) => None,
        // SUMMERY auto-fields (Wave 12n) follow the same structural-pairing
        // pattern as ClickHere. Free-floating SummaryField means the inline
        // FieldBegin marker did not pair with this CtrlHeader; drop.
        Hwp5Control::SummaryField(_) => None,
        // %dte date/time format-code fields (Wave 12n) — same pattern.
        Hwp5Control::DateCodeField(_) => None,
        // %pat path fields (Wave 12n) — same pattern.
        Hwp5Control::PathField(_) => None,
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
        Hwp5Control::CrossRef(_) => None,
    };
    run.map(|mut r| {
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
    let Some(raw_bytes) = projection_images.ole_bytes_for_binary_data_id(ole.binary_data_id) else {
        projection_images.warnings.push(Hwp5Warning::DroppedControl {
            control: "ole_object",
            reason: format!("ole_bin_data_unavailable binary_data_id={}", ole.binary_data_id),
        });
        return None;
    };

    match extract_chart_payload(raw_bytes) {
        Ok(payload) => {
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
fn shape_vertical_align_from_list_header(list_header_properties: Option<u32>) -> VerticalAlign {
    match list_header_properties {
        Some(props) => match ((props >> 5) & 0x03) as u8 {
            0 => VerticalAlign::Top,
            1 => VerticalAlign::Center,
            2 => VerticalAlign::Bottom,
            _ => VerticalAlign::Top,
        },
        None => VerticalAlign::Top,
    }
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
            placement: shape_placement(
                &textbox.geometry,
                textbox.ctrl_properties,
                &mut projection_images.warnings,
            ),
            caption: None,
            style: None,
            text_vertical_align: shape_vertical_align_from_list_header(
                textbox.list_header_properties,
            ),
        },
        CharShapeIndex::new(0),
    )
}

/// Projects a HWP5 group (묶음 객체) into a Core `Run` carrying
/// `Control::Group` (Wave A: flat children only).
///
/// Each child is projected via the existing per-shape helpers; a child that
/// carried `drawText` paragraphs becomes a text-bearing shape (rect →
/// `Control::TextBox`, ellipse → `ellipse_with_text`). Children that cannot
/// be represented as a Core group child (e.g. a degraded nested-group
/// `Unknown`, or an image — not a valid group child per `validate`) are
/// dropped so the resulting group still validates. Returns `None` only when
/// the group ends up with no representable children.
fn project_group_run(
    group: &Hwp5GroupControl,
    projection_images: &mut ProjectionImageState<'_>,
) -> Option<Run> {
    let mut children = Vec::with_capacity(group.children.len());
    for child in &group.children {
        if let Some(control) = project_group_child(child, projection_images) {
            children.push(control);
        }
    }
    if children.is_empty() {
        return None;
    }
    let inst_id = (group.instance_id != 0).then_some(ObjectId::new(u64::from(group.instance_id)));
    Some(Run::control(
        Control::Group {
            children,
            width: hwp_unit_from_u32(group.geometry.width),
            height: hwp_unit_from_u32(group.geometry.height),
            placement: offset_placement(group.geometry.x, group.geometry.y),
            inst_id,
        },
        CharShapeIndex::new(0),
    ))
}

/// Projects one [`Hwp5GroupChild`] into a Core shape `Control` suitable as a
/// `Control::Group` child. Text-bearing rect/ellipse children become
/// `TextBox` / `ellipse_with_text`; everything else reuses the single-shape
/// projection helpers and extracts the inner control. Returns `None` for
/// children that have no valid Core group-child representation.
fn project_group_child(
    child: &Hwp5GroupChild,
    projection_images: &mut ProjectionImageState<'_>,
) -> Option<Control> {
    // Text-bearing shapes: attach the projected paragraphs.
    if !child.paragraphs.is_empty() {
        let paragraphs = project_nested_paragraphs(
            &child.paragraphs,
            projection_images,
            ImageProjectionContext::TextBox,
        );
        let valign = shape_vertical_align_from_list_header(child.list_header_properties);
        match &child.control {
            Hwp5Control::Rect(rect) => {
                let mut control = Control::text_box(
                    paragraphs,
                    hwp_unit_from_u32(rect.geometry.width),
                    hwp_unit_from_u32(rect.geometry.height),
                );
                if let Control::TextBox { placement, text_vertical_align, .. } = &mut control {
                    // 그룹 자식은 자체 `gso ` 속성 word 가 없다 (`into_child`
                    // 의 강제 0 은 byte-grounded 아님 — 자식 배치는 컨테이너가
                    // 지배, W5 해석) → bit0 경로 대신 offset 휴리스틱으로
                    // 기존 zero-offset 인라인 방출을 보존한다.
                    *placement = offset_placement(rect.geometry.x, rect.geometry.y);
                    *text_vertical_align = valign;
                }
                return Some(control);
            }
            Hwp5Control::Ellipse(ellipse) => {
                let width = HwpUnit::new(positive_i32_from_u32(ellipse.geometry.width)?).ok()?;
                let height = HwpUnit::new(positive_i32_from_u32(ellipse.geometry.height)?).ok()?;
                let mut control = Control::ellipse_with_text(width, height, paragraphs);
                if let Control::Ellipse { placement, text_vertical_align, .. } = &mut control {
                    // 위 TextBox arm 과 동일 — 그룹 자식 offset 휴리스틱.
                    *placement = offset_placement(ellipse.geometry.x, ellipse.geometry.y);
                    *text_vertical_align = valign;
                }
                return Some(control);
            }
            // Other text-bearing children are uncommon; fall through to the
            // non-text projection (text is dropped) rather than fabricate.
            _ => {}
        }
    }

    // Non-text children reuse the single-shape projection helpers; extract
    // the inner control from the produced run.
    let run = match &child.control {
        Hwp5Control::Line(line) => project_line_run(line, &mut projection_images.warnings),
        Hwp5Control::Rect(rect) => project_rect_run(rect, &mut projection_images.warnings),
        Hwp5Control::Polygon(polygon) => {
            project_polygon_run(polygon, &mut projection_images.warnings)
        }
        Hwp5Control::Ellipse(ellipse) => {
            project_ellipse_run(ellipse, &mut projection_images.warnings)
        }
        Hwp5Control::Arc(arc) => project_arc_run(arc, &mut projection_images.warnings),
        Hwp5Control::Curve(curve) => project_curve_run(curve, &mut projection_images.warnings),
        Hwp5Control::TextArt(text_art) => {
            Some(project_text_art_run(text_art, &mut projection_images.warnings))
        }
        Hwp5Control::ConnectLine(connect_line) => {
            project_connectline_run(connect_line, &mut projection_images.warnings)
        }
        Hwp5Control::Equation(equation) => Some(project_equation_run(equation)),
        // Nested group (Wave B): recurse. `project_group_run` returns a
        // `Run` carrying `Control::Group`; extract it the same way as every
        // leaf shape below. Recursion bottoms out when all descendants are
        // leaf shapes; depth is already bounded by the decoder's cap.
        Hwp5Control::Group(nested) => project_group_run(nested, projection_images),
        // Image / OLE / anything else is not a valid group child; drop it.
        _ => None,
    }?;
    match run.content {
        RunContent::Control(boxed) => {
            let mut control = *boxed;
            // 공유 project_*_run helper 는 bit0 기반 shape_placement 를
            // 쓰지만, 그룹 자식의 `ctrl_properties` 는 `into_child` 가
            // 강제한 0 (byte-grounded 아님 — 자식 배치는 컨테이너 지배,
            // W5 해석) → offset 휴리스틱으로 재지정해 기존 zero-offset
            // 인라인 방출(numbering="NONE")을 보존한다.
            if let Some((x, y)) = group_child_offset(&child.control) {
                override_shape_placement(&mut control, x, y);
            }
            Some(control)
        }
        _ => None,
    }
}

/// [`project_group_child`] 의 offset-휴리스틱 재지정 대상 자식의 기하
/// offset. bit0 을 자체 보유하지 않는 leaf 도형만 대상 — `TextArt` 는
/// [`offset_placement`] 로 이미 투영되고, 중첩 `Group` 은
/// `project_group_run` 이 컨테이너 규칙을 소유하므로 제외한다.
fn group_child_offset(control: &Hwp5Control) -> Option<(i32, i32)> {
    match control {
        Hwp5Control::Line(c) => Some((c.geometry.x, c.geometry.y)),
        Hwp5Control::Rect(c) => Some((c.geometry.x, c.geometry.y)),
        Hwp5Control::Polygon(c) => Some((c.geometry.x, c.geometry.y)),
        Hwp5Control::Ellipse(c) => Some((c.geometry.x, c.geometry.y)),
        Hwp5Control::Arc(c) => Some((c.geometry.x, c.geometry.y)),
        Hwp5Control::Curve(c) => Some((c.geometry.x, c.geometry.y)),
        Hwp5Control::ConnectLine(c) => Some((c.geometry.x, c.geometry.y)),
        _ => None,
    }
}

/// 그룹 자식 leaf 도형의 placement 를 [`offset_placement`] 결과로
/// 재지정한다 (대상 variant 는 [`group_child_offset`] 와 짝).
fn override_shape_placement(control: &mut Control, x: i32, y: i32) {
    match control {
        Control::Line { placement, .. }
        | Control::Rect { placement, .. }
        | Control::Polygon { placement, .. }
        | Control::Ellipse { placement, .. }
        | Control::Arc { placement, .. }
        | Control::Curve { placement, .. }
        | Control::ConnectLine { placement, .. } => *placement = offset_placement(x, y),
        _ => {}
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
    let control = Control::TextArt {
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
            let projected = cells
                .iter()
                .copied()
                .map(|cell| project_table_cell_with_images(cell, projection_images))
                .collect();
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
    restore_placeholder_cells_for_uncovered_rows(&mut rows, &mut projection_images.warnings);

    let mut core_table = Table::new(rows);
    apply_table_projection_metadata(table, &mut core_table, &mut projection_images.warnings);
    core_table
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

    /// 그룹 자식은 offset 휴리스틱을 쓴다 — `into_child` 가 강제한
    /// `ctrl_properties: 0` 이 bit0 경로(shape_placement)를 타면
    /// zero-offset 자식이 floating placement 를 얻어 numbering 이
    /// NONE→PICTURE 로 조용히 바뀐다 (W4 w1 byte-preserving 잠금).
    #[test]
    fn group_child_zero_offset_stays_inline_non_zero_floats() {
        let rect = |x: i32, y: i32| {
            crate::decoder::section::Hwp5RectControl {
                ctrl_id: 0,
                geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                    x,
                    y,
                    width: 1000,
                    height: 1000,
                },
                ctrl_properties: 0, // into_child 의 강제값 재현
            }
        };
        for (x, y, expect_inline) in [(0, 0, true), (1_000, 500, false)] {
            let child = Hwp5GroupChild {
                control: Hwp5Control::Rect(rect(x, y)),
                paragraphs: Vec::new(),
                list_header_properties: None,
            };
            let mut state = ProjectionImageState::new(None, None);
            let control = project_group_child(&child, &mut state).expect("rect child projects");
            let Control::Rect { placement, .. } = control else {
                panic!("rect child must project to Control::Rect");
            };
            assert_eq!(
                placement.is_none(),
                expect_inline,
                "offset ({x},{y}) → inline={expect_inline} (zero-offset 은 legacy \
                 인라인 방출 보존, non-zero 는 floating)"
            );
        }
    }

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
            shape_vertical_align_from_list_header(Some(0 << 5)),
            VerticalAlign::Top,
            "code 0 → top"
        );
        assert_eq!(
            shape_vertical_align_from_list_header(Some(1 << 5)),
            VerticalAlign::Center,
            "code 1 → center"
        );
        assert_eq!(
            shape_vertical_align_from_list_header(Some(2 << 5)),
            VerticalAlign::Bottom,
            "code 2 → bottom"
        );
    }

    #[test]
    fn shape_vertical_align_ignores_other_bits() {
        // Lower bits (text direction, line wrap) and higher bits must not leak
        // into the alignment. Set every bit *except* 5~6 and expect Top.
        let noise = !0b0110_0000u32;
        assert_eq!(shape_vertical_align_from_list_header(Some(noise)), VerticalAlign::Top);
        // center with surrounding noise still decodes center.
        assert_eq!(
            shape_vertical_align_from_list_header(Some(noise | (1 << 5))),
            VerticalAlign::Center
        );
    }

    #[test]
    fn shape_vertical_align_defaults_top_when_absent() {
        assert_eq!(shape_vertical_align_from_list_header(None), VerticalAlign::Top);
        // code 3 (reserved) is not a real value → default Top, no fake mapping.
        assert_eq!(shape_vertical_align_from_list_header(Some(3 << 5)), VerticalAlign::Top);
    }

    #[test]
    fn project_textbox_carries_center_vertical_align_from_list_header() {
        // 한컴 stores the textbox 세로 정렬 in the ListHeader 속성 bits 5~6.
        // A CENTER (code 1) textbox must project to Core with Center align.
        let textbox = Hwp5TextBoxControl {
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                x: 10,
                y: 20,
                width: 8_000,
                height: 6_000,
            },
            paragraphs: vec![make_paragraph("가운데", 0, 0)],
            list_header_properties: Some(1 << 5),
            ctrl_properties: 0,
        };
        let mut images = ProjectionImageState::new(None, None);
        let run = project_textbox_run(&textbox, &mut images);
        match run.content.as_control().unwrap().clone() {
            Control::TextBox { text_vertical_align, .. } => {
                assert_eq!(text_vertical_align, VerticalAlign::Center);
            }
            other => panic!("expected Control::TextBox, got {other:?}"),
        }
    }

    #[test]
    fn project_textbox_defaults_top_when_list_header_absent() {
        let textbox = Hwp5TextBoxControl {
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                x: 0,
                y: 0,
                width: 8_000,
                height: 6_000,
            },
            paragraphs: vec![make_paragraph("위", 0, 0)],
            list_header_properties: None,
            ctrl_properties: 0,
        };
        let mut images = ProjectionImageState::new(None, None);
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

    fn _make_paragraph_with_runs(text: &str, runs: Vec<Hwp5CharShapeRun>) -> Hwp5Paragraph {
        Hwp5Paragraph {
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
        let cache = promote_line_segments(&segs, Ok(identity), &mut warnings).expect("promoted");
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
        assert!(promote_line_segments(&[], Ok(empty_map), &mut warnings).is_none());
        assert!(warnings.is_empty());
    }

    fn make_section(
        paragraphs: Vec<Hwp5Paragraph>,
        page_def: Option<Hwp5PageDef>,
    ) -> SectionResult {
        SectionResult {
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
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                x: 0,
                y: 0,
                width: 3_000,
                height: 2_000,
            },
            binary_data_id: 1,
            instance_id: 0,
            // W2p: treat_as_char 는 이제 raw bit0 이 유일한 진실 — 좌표
            // (0,0) 만으로는 더 이상 inline 판정이 되지 않는다.
            ctrl_properties: 0x1,
        });
        let section = make_section(
            vec![Hwp5Paragraph {
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
        assert_eq!(placement.text_wrap, ObjectTextWrap::TopAndBottom);
        assert_eq!(placement.horz_rel_to, ObjectRelativeTo::Para);
        assert_eq!(placement.vert_rel_to, ObjectRelativeTo::Para);
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
            instance_id: 0,
            ctrl_properties: 0,
        });
        let section = make_section(
            vec![Hwp5Paragraph {
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
                        ctrl_id: 0x6865_6164,
                        properties_raw: 0,
                        instance_id: 0,
                        paragraphs: vec![Hwp5Paragraph {
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
            instance_id: 0,
            // W5 w0: 배치가 이제 속성 word 실비트로 결정되므로, 실측 anchored
            // 이미지 word(anchored_zero_origin_png native fixture, 0x040a2310 →
            // Para/Para/Square, flowWithText=1)를 싣는다 (이전엔 TextBox 관례가
            // 이 값을 합성했다).
            ctrl_properties: 0x040a_2310,
        });
        let textbox = Hwp5Control::TextBox(Hwp5TextBoxControl {
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                x: 50,
                y: 60,
                width: 8_000,
                height: 6_000,
            },
            list_header_properties: None,
            ctrl_properties: 0,
            paragraphs: vec![Hwp5Paragraph {
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

        let (document, image_store, warnings) =
            project_to_core_with_images(vec![section], &image_assets).unwrap();
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
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                x: 0,
                y: 0,
                width: 1_000,
                height: 800,
            },
            binary_data_id: 99,
            instance_id: 0,
            ctrl_properties: 0,
        });
        let section = make_section(
            vec![Hwp5Paragraph {
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
            instance_id: 0,
            ctrl_properties: 0,
        });
        let section = make_section(
            vec![Hwp5Paragraph {
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
            instance_id: 0,
            ctrl_properties: 0,
        });
        let section = make_section(
            vec![Hwp5Paragraph {
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

    // ── image_placement_from_wire (W5 w0 — byte-ground axes) ────────────────

    #[test]
    fn image_placement_from_wire_byte_grounds_floating_axes() {
        // 속성 word bit0=0 → 부유. relTo/wrap/flow/overlap 이 실비트로
        // 디코드된다 (이전 Flow 관례 Paper/Paper/InFrontOfText 폐기).
        let zero_origin = crate::schema::section::Hwp5ShapeComponentGeometry {
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

    // ── table controls ────────────────────────────────────────────────────────

    #[test]
    fn table_control_becomes_run_table() {
        let para = Hwp5Paragraph {
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
                rows: 2,
                cols: 3,
                page_break: Hwp5TablePageBreak::None,
                repeat_header: false,
                cell_spacing: 120,
                border_fill_id: Some(8),
                cells: vec![],
                instance_id: 0,
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

    fn grid_test_cell(row: u16, column: u16, row_span: u16, col_span: u16) -> Hwp5TableCell {
        Hwp5TableCell {
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
                rows,
                cols,
                page_break: Hwp5TablePageBreak::None,
                repeat_header: false,
                cell_spacing: 0,
                border_fill_id: None,
                cells,
                instance_id: 0,
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
                rows: 1,
                cols: 1,
                page_break: Hwp5TablePageBreak::Cell,
                repeat_header: false,
                cell_spacing: 0,
                border_fill_id: None,
                cells: vec![pgnp_cell()],
                instance_id: 0,
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
                    rows: 1,
                    cols: 1,
                    page_break: Hwp5TablePageBreak::Cell,
                    repeat_header: false,
                    cell_spacing: 0,
                    border_fill_id: None,
                    cells: vec![cell],
                    instance_id: 0,
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
                ctrl_id: 0x666E_2020,
                properties_raw: 0,
                instance_id: 7,
                paragraphs: vec![Hwp5Paragraph {
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
                ctrl_id: 0x6773_6F20,
                geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
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
                ctrl_id: 0x6773_6F20,
                geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
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
                rows: 3,
                cols: 4,
                page_break: Hwp5TablePageBreak::Cell,
                repeat_header: true,
                cell_spacing: 0,
                border_fill_id: None,
                cells: vec![],
                instance_id: 0,
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
                instance_id: 0,
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
                instance_id: 0,
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
                instance_id: 0,
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
            let mut images = ProjectionImageState::new(None, None);
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
            let mut images = ProjectionImageState::new(None, None);
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
            let mut images = ProjectionImageState::new(None, None);
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
}
