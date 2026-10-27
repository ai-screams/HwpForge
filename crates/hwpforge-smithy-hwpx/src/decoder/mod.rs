//! HWPX decoding pipeline.
//!
//! Submodules handle individual stages:
//! - `package` — ZIP extraction and file access
//! - `header` — `header.xml` parsing → [`HwpxStyleStore`]
//! - `section` — `section*.xml` parsing → paragraphs + page settings

pub(crate) mod chart;
pub(crate) mod group_geometry;
pub(crate) mod header;
pub(crate) mod image_size;
pub(crate) mod metadata;
pub(crate) mod package;
pub(crate) mod package_order;
#[cfg(test)]
mod paragraph_measure_tests;
pub(crate) mod section;
pub(crate) mod shapes;

pub mod source_map;

use std::path::Path;

use hwpforge_core::document::{Document, Draft};
use hwpforge_core::image::ImageStore;
use hwpforge_core::section::{BeginNum, MasterPage, Section};
use hwpforge_core::PageSettings;

use crate::error::HwpxResult;
use crate::style_store::HwpxStyleStore;

// ── serde decode entry point ─────────────────────────────────────

/// Serde recursion budget for the HWPX parts this crate deserializes with
/// serde (`header.xml`, `section*.xml`, and the patch path's section
/// reparse; `content.hpf` and chart XML use the pull reader instead).
///
/// quick-xml 0.42 caps serde nesting at 128 by default, which rejects
/// tables nested 21 deep — well inside our own
/// [`hwpforge_core::MAX_NESTING_DEPTH`] numeric budget (32). The parser
/// counters differ from Core semantic depth. A nested table spans six XML
/// element levels and a text box five (measured on real fixtures), so seven per nesting level —
/// `7 * 32 = 224` — keeps even the 33rd level inside the budget: that level
/// is rejected by our structural nesting guards rather than by the serde
/// budget, the boundary quick-xml 0.41 had.
///
/// **Provisional:** the value may change in a minor release once more real
/// documents have been measured. Deep inputs still need stack; the measured
/// minimum per build profile is on [`HwpxDecoder::decode`].
pub(crate) const XML_RECURSION_LIMIT: usize = 7 * section::MAX_NESTING_DEPTH;

/// Upper bound on namespace bindings in scope while deserializing.
///
/// quick-xml 0.42 counts bindings in scope (not per element as 0.41 did) and
/// defaults to 128. The value is set explicitly so a future quick-xml bump
/// cannot move it silently. The largest count seen in real documents is 15.
pub(crate) const XML_MAX_NAMESPACE_BINDINGS: usize = 128;

/// Maximum events buffered while replaying interleaved serde lists.
///
/// On 2026-10-24, a SHA-256-deduplicated full corpus scan measured a maximum
/// of 90 events across 3,671 HWPX documents and 7,717 header/section/master-page
/// parts (3,999 paths, 3,757 unique files; 86 non-HWPX/invalid inputs excluded).
/// Binary search found each part's minimum accepted positive limit using the
/// production DTOs and whitespace preprocessing with quick-xml 0.42.0.
/// 65,536 is over 728x that maximum; empty elements consume two events.
///
/// quick-xml 0.42.0 buffers `DeEvent`, which is 40 bytes on this 64-bit target:
/// the capped write queue holds at most 65,536 * 40 = 2,621,440 bytes (2.5 MiB)
/// of live event slots. Tags borrow the input with `from_str`; owned/merged
/// text, replay-queue capacity and allocator overhead are additional. This is
/// an event-count limit, not a total-memory or input-byte limit.
pub(crate) const XML_EVENT_BUFFER_LIMIT: std::num::NonZeroUsize =
    std::num::NonZeroUsize::new(65_536).unwrap();

/// Deserializes one HWPX XML part with this crate's explicit limits.
///
/// Every serde decode in this crate goes through here so the recursion,
/// namespace and event-buffer limits apply uniformly; `quick_xml::de::from_str` is
/// rejected by the workspace `.clippy.toml` (`disallowed-methods`).
pub(crate) fn xml_from_str<'de, T: serde::Deserialize<'de>>(
    xml: &'de str,
) -> Result<T, quick_xml::DeError> {
    #[allow(clippy::disallowed_methods)] // the one place that sets the limits
    let mut de = quick_xml::de::Deserializer::from_str(xml);
    de.recursion_limit(XML_RECURSION_LIMIT);
    de.resolver_mut().set_max_namespace_bindings(XML_MAX_NAMESPACE_BINDINGS);
    de.event_buffer_size(Some(XML_EVENT_BUFFER_LIMIT));
    T::deserialize(&mut de)
}

/// The `detail` text for a failed [`xml_from_str`], as users see it in
/// `DECODE_FAILED`.
///
/// quick-xml's own text for the namespace limit tells the reader to call
/// `NamespaceResolver::set_max_namespace_bindings`, which no caller of the
/// CLI, MCP server or Python binding can do; it is replaced by a sentence
/// that states the limit. The replay-buffer limit likewise states HwpForge's
/// refusal policy, without exposing deserializer configuration advice.
/// Every other error keeps quick-xml's text.
pub(crate) fn xml_error_detail(error: &quick_xml::DeError) -> String {
    use quick_xml::name::NamespaceError;
    match error {
        quick_xml::DeError::TooManyEvents(limit) => format!(
            "more than {limit} buffered XML events; HwpForge does not read \
             documents that require more"
        ),
        quick_xml::DeError::InvalidXml(quick_xml::Error::Namespace(
            NamespaceError::TooManyBindings(limit),
        )) => format!(
            "more than {limit} namespace bindings in scope; HwpForge does not read \
             documents that declare more"
        ),
        other => other.to_string(),
    }
}

// ── HwpxDocument ─────────────────────────────────────────────────

/// The result of decoding an HWPX file.
///
/// Contains the Core document (structure), the HWPX-specific style
/// store (fonts, char shapes, para shapes from `header.xml`), and
/// binary image data extracted from `BinData/` entries.
#[derive(Debug)]
#[non_exhaustive]
pub struct HwpxDocument {
    pub(crate) source_basis: Option<std::sync::Arc<SourceBasis>>,
    pub(crate) source_maps: Vec<source_map::SourceMap>,
    /// The decoded document in Core's DOM.
    pub document: Document<Draft>,
    /// Style information parsed from `header.xml`.
    pub style_store: HwpxStyleStore,
    /// Binary image data extracted from `BinData/` ZIP entries.
    pub image_store: ImageStore,
    /// 디코드 중 표면화된 비치명 경고 (W5-α M4 — warning-first).
    pub warnings: Vec<DecodeWarning>,
}

/// Immutable decoded base for one editing operation. Sharing this context
/// avoids decoding the same package again for provenance and layout planning.
/// The original document, styles and images cannot be changed through it.
#[derive(Debug)]
pub struct HwpxEditBase {
    decoded: HwpxDocument,
}

impl HwpxEditBase {
    /// Decode and retain an independent original package.
    ///
    /// # Errors
    /// Returns the same errors as [`HwpxDecoder::decode`].
    pub fn read(bytes: &[u8]) -> HwpxResult<Self> {
        Ok(Self { decoded: HwpxDecoder::decode(bytes)? })
    }

    /// Read-only original model and source addresses.
    pub fn decoded(&self) -> &HwpxDocument {
        &self.decoded
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.decoded.source_basis.as_ref().expect("decoded edit base").package
    }
}

impl HwpxDocument {
    pub(crate) fn requires_header_normalization(&self) -> bool {
        self.source_basis.as_ref().is_none_or(|basis| basis.requires_header_normalization)
    }

    /// Decoder-owned original locations for a section's Core nodes.
    pub fn source_map(&self, section: usize) -> Option<&source_map::SourceMap> {
        self.source_maps.get(section)
    }
    pub(crate) fn require_source_table(&self, table: usize) -> HwpxResult<()> {
        let tables = crate::table_inventory::tables_in_document(&self.document);
        let Some(entry) = tables.get(table) else {
            return Ok(());
        };
        let path = crate::table_inventory::core_path(entry);
        if self.source_map(path.section()).and_then(|m| m.node(&path)).is_none() {
            return Err(crate::HwpxError::InvalidStructure {
                detail: format!("internal decoder source-map defect: no table {}", path.relative()),
            });
        }
        Ok(())
    }
    pub(crate) fn require_source_slot(&self, section: usize, path: &str) -> HwpxResult<()> {
        let Some(core) = self.document.sections().get(section) else {
            return Ok(());
        };
        if !crate::patch::collect_semantic_text_slots(core, section).iter().any(|s| s.path == path)
        {
            return Ok(());
        }
        let key = hwpforge_core::node_path::CorePath::new(section, path);
        if self.source_map(section).and_then(|m| m.node(&key)).is_none() {
            return Err(crate::HwpxError::InvalidStructure {
                detail: format!("internal decoder source-map defect: no slot {path}"),
            });
        }
        Ok(())
    }
}

/// 디코드 중 표면화된 비치명 경고.
///
/// 디코더가 미지 wire 값을 기본값으로 **조용히** 폴백하면 하류(렌더러
/// 등)의 warning-first 는 증거가 세탁된 뒤라 무력해진다 (W5-α M4) —
/// 폴백은 유지하되 사실을 표면화한다. 속성 결측(빈 문자열)은 기본값
/// 적용이 정상이므로 경고 대상이 아니다.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DecodeWarning {
    /// An unsupported definition/selection with no active content consumer.
    /// The omission is reported, but does not make a content edit lossy.
    InactiveHeaderReference {
        /// Original package part.
        part: String,
        /// Definition or reference field.
        kind: String,
        /// Original ID and omission policy.
        reason: String,
    },
    /// Selected paragraph semantics are preserved while wire branches are normalized.
    ParagraphMeasureNormalized {
        /// Header definition kind (paraPr or tabPr).
        definition: &'static str,
        /// Original definition ID.
        id: u32,
        /// Margin field, lineSpacing or tab ordinal.
        field: String,
        /// Discrepancy and selected interpretation.
        reason: String,
    },
    /// A source start has no end. The original boundary and body are retained;
    /// reading is allowed, while edits touching the remaining flow are refused.
    UnclosedFieldBoundary {
        /// Package part containing the range.
        part: String,
        /// Semantic location of the recovered start.
        reason: String,
    },
    /// A recovered field range has invalid boundaries. Its source content
    /// remains in Draft, but editing and validated export are refused.
    InvalidFieldBoundary {
        /// Package part containing the range.
        part: String,
        /// Semantic location and pairing failure.
        reason: String,
    },
    /// 알 수 없는 enum 문자열을 기본값으로 폴백함.
    UnknownEnumValue {
        /// 요소@속성 경로 (예: `hp:header@applyPageType`).
        attribute: &'static str,
        /// 원본 wire 문자열.
        raw: String,
        /// 폴백한 값의 wire 표기.
        fallback: &'static str,
    },
    /// 문단의 linesegarray 캐시가 Core 로 승격되지 않음 — wire→Core
    /// 좌표 ledger 구축 실패 또는 textpos 정규화 실패 (W1b fail-closed,
    /// 추측 좌표 승격 금지).
    LayoutCacheDropped {
        /// 경고가 난 문단의 중첩 경로.
        path: ParagraphPath,
        /// 구체 사유 (ledger 실패 원인 또는 문제 textpos).
        reason: String,
    },
    /// 입력에 있던 내용을 Core 가 담지 못해 디코드 결과에서 빠졌거나
    /// 근사됐다.
    ///
    /// 디코드는 계속되지만, 이 결과를 다시 인코드하면 손실이 굳는다. 그래서
    /// 입력을 재인코드해 돌려주는 편집기(stamp·set-cell·restyle)는 이 경고가
    /// 있는 입력을 거부한다 ([`DecodeWarning::is_content_loss`]).
    ContentLost {
        /// 손실이 난 패키지 part 이름 (예: `Contents/section0.xml`).
        part: String,
        /// part 안에서 손실이 난 자리의 중첩 경로.
        path: ParagraphPath,
        /// 무엇이 빠졌는지 — wire 요소 이름(`pic`, `ole` …) 또는 손실 분류
        /// (`group_child_transform`, `group_nested_switch` …).
        kind: String,
        /// 사람이 읽는 사유 (근사 오차 등).
        reason: String,
    },
    /// header `<hh:beginNum>` 의 시작 번호가 `0` 이라 다시 쓸 때 `1` 로 고친다.
    ///
    /// 문서 시작 번호는 XSD 상 1 이상(`positiveInteger`)이고 한컴은 언제나 1
    /// 을 쓴다. `0` 은 옛 HwpForge 재인코드 출력의 흔적이다(한컴은 그 파일을
    /// 다시 저장해도 0 을 둔다). 한컴은 header 쪽 번호 0 인 문서를 0 쪽부터
    /// 세고 홀짝 바탕쪽을 뒤바꾼다. 고침이지 손실이 아니므로 편집기는 이
    /// 입력을 거부하지 않는다 ([`DecodeWarning::is_content_loss`] 는 `false`).
    BeginNumRepaired {
        /// 0 이던 축 — `page`·`pic`·`tbl`·`equation` 중, 그 순서대로.
        axes: Vec<&'static str>,
    },
}

/// The header part every [`DecodeWarning::BeginNumRepaired`] points at.
const HEADER_PART: &str = "Contents/header.xml";

impl DecodeWarning {
    /// 이 경고의 안정 코드.
    ///
    /// 하류(ops·convert·CLI)가 자기 매핑표에 없는 변형을 만나면 이 값을
    /// 그대로 보고한다. 기존 변형의 하류 출력 코드는 각 하류의 표가 정한다.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_smithy_hwpx::{DecodeWarning, ParagraphPath};
    ///
    /// let w = DecodeWarning::ContentLost {
    ///     part: "Contents/section0.xml".into(),
    ///     path: ParagraphPath::default(),
    ///     kind: "pic".into(),
    ///     reason: "unsupported group child".into(),
    /// };
    /// assert_eq!(w.code(), "CONTENT_LOST");
    /// ```
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::InactiveHeaderReference { .. } => "INACTIVE_HEADER_REFERENCE",
            Self::UnknownEnumValue { .. } => "UNKNOWN_ENUM_VALUE",
            Self::LayoutCacheDropped { .. } => "LAYOUT_CACHE_DROPPED",
            Self::ContentLost { .. } => "CONTENT_LOST",
            Self::InvalidFieldBoundary { .. } => "INVALID_FIELD_BOUNDARY",
            Self::UnclosedFieldBoundary { .. } => "UNCLOSED_FIELD_BOUNDARY",
            Self::BeginNumRepaired { .. } => "BEGIN_NUM_REPAIRED",
            Self::ParagraphMeasureNormalized { .. } => "PARAGRAPH_MEASURE_NORMALIZED",
        }
    }

    /// 경고가 가리키는 패키지 part 이름. part 를 싣지 않는 변형은 `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_smithy_hwpx::{DecodeWarning, ParagraphPath};
    ///
    /// let lost = DecodeWarning::ContentLost {
    ///     part: "Contents/section0.xml".into(),
    ///     path: ParagraphPath::default(),
    ///     kind: "pic".into(),
    ///     reason: "unsupported group child".into(),
    /// };
    /// assert_eq!(lost.part(), Some("Contents/section0.xml"));
    /// let dropped = DecodeWarning::LayoutCacheDropped {
    ///     path: ParagraphPath::default(),
    ///     reason: "textpos".into(),
    /// };
    /// assert_eq!(dropped.part(), None);
    /// ```
    #[must_use]
    pub fn part(&self) -> Option<&str> {
        match self {
            Self::UnknownEnumValue { .. } | Self::LayoutCacheDropped { .. } => None,
            Self::ContentLost { part, .. }
            | Self::InactiveHeaderReference { part, .. }
            | Self::InvalidFieldBoundary { part, .. }
            | Self::UnclosedFieldBoundary { part, .. } => Some(part),
            Self::BeginNumRepaired { .. } | Self::ParagraphMeasureNormalized { .. } => {
                Some(HEADER_PART)
            }
        }
    }

    /// 경고가 가리키는 자리를 사람이 읽는 한 줄로.
    ///
    /// `UnknownEnumValue` 는 `요소@속성`, `LayoutCacheDropped` 는 문단 경로,
    /// `ContentLost` 는 `part:경로`.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_smithy_hwpx::{DecodeWarning, ParagraphPath, PathSeg};
    ///
    /// let w = DecodeWarning::ContentLost {
    ///     part: "Contents/section0.xml".into(),
    ///     path: ParagraphPath(vec![PathSeg::Section(0), PathSeg::GroupChild(1)]),
    ///     kind: "pic".into(),
    ///     reason: "unsupported group child".into(),
    /// };
    /// assert_eq!(w.location().as_deref(), Some("Contents/section0.xml:section[0].group[1]"));
    /// ```
    #[must_use]
    pub fn location(&self) -> Option<String> {
        match self {
            Self::InactiveHeaderReference { part, kind, .. } => Some(format!("{part}:{kind}")),
            Self::UnknownEnumValue { attribute, .. } => Some((*attribute).to_string()),
            Self::LayoutCacheDropped { path, .. } => Some(path.to_string()),
            Self::ContentLost { part, path, .. } => Some(format!("{part}:{path}")),
            Self::InvalidFieldBoundary { part, .. } | Self::UnclosedFieldBoundary { part, .. } => {
                Some(part.clone())
            }
            Self::BeginNumRepaired { .. } => Some("hh:beginNum".to_string()),
            Self::ParagraphMeasureNormalized { definition, id, field, .. } => {
                Some(format!("{definition}[{id}].{field}"))
            }
        }
    }

    /// 자리([`Self::location`])를 뺀 사람용 문구.
    ///
    /// 자리를 따로 싣는 하류 표(CLI `to-pdf` 의 `location` 열 등)가 문구에
    /// 자리를 두 번 쓰지 않도록 쓴다. 자리까지 한 줄로 필요하면 `Display`.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_smithy_hwpx::{DecodeWarning, ParagraphPath};
    ///
    /// let w = DecodeWarning::ContentLost {
    ///     part: "Contents/section0.xml".into(),
    ///     path: ParagraphPath::default(),
    ///     kind: "pic".into(),
    ///     reason: "unsupported group child".into(),
    /// };
    /// assert_eq!(w.message(), "pic lost: unsupported group child");
    /// ```
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::UnknownEnumValue { raw, fallback, .. } => {
                format!("\"{raw}\" unknown — fell back to {fallback}")
            }
            Self::InactiveHeaderReference { kind, reason, .. } => {
                format!("inactive {kind}: {reason}")
            }
            Self::ParagraphMeasureNormalized { reason, .. } => reason.clone(),
            Self::LayoutCacheDropped { reason, .. } => format!("layout cache dropped: {reason}"),
            Self::ContentLost { kind, reason, .. } => format!("{kind} lost: {reason}"),
            Self::InvalidFieldBoundary { reason, .. }
            | Self::UnclosedFieldBoundary { reason, .. } => reason.clone(),
            Self::BeginNumRepaired { axes } => {
                format!("start number 0 on {} is not valid; a re-encode writes 1", axes.join(", "))
            }
        }
    }

    /// 입력 내용의 소실·근사 또는 불완전 경계 때문에 편집을 거부해야 하는지.
    ///
    /// `true` 인 경고가 있는 입력을 재인코드하면 손실이 굳으므로, 입력을
    /// 다시 써서 돌려주는 편집기는 그 입력을 거부한다. 판정 정의는 이
    /// 메서드 하나다.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_smithy_hwpx::{DecodeWarning, ParagraphPath};
    ///
    /// let lost = DecodeWarning::ContentLost {
    ///     part: "Contents/section0.xml".into(),
    ///     path: ParagraphPath::default(),
    ///     kind: "pic".into(),
    ///     reason: "unsupported group child".into(),
    /// };
    /// assert!(lost.is_content_loss());
    /// let dropped = DecodeWarning::LayoutCacheDropped {
    ///     path: ParagraphPath::default(),
    ///     reason: "textpos".into(),
    /// };
    /// assert!(!dropped.is_content_loss());
    /// ```
    #[must_use]
    pub fn is_content_loss(&self) -> bool {
        match self {
            Self::InactiveHeaderReference { .. }
            | Self::UnknownEnumValue { .. }
            | Self::LayoutCacheDropped { .. }
            | Self::ParagraphMeasureNormalized { .. }
            | Self::BeginNumRepaired { .. }
            | Self::UnclosedFieldBoundary { .. } => false,
            Self::ContentLost { .. } | Self::InvalidFieldBoundary { .. } => true,
        }
    }
}

impl std::fmt::Display for DecodeWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InactiveHeaderReference { part, kind, reason } => {
                write!(f, "inactive {kind} at {part}: {reason}")
            }
            Self::UnclosedFieldBoundary { part, reason } => {
                write!(f, "unclosed source field at {part}: {reason}")
            }
            Self::InvalidFieldBoundary { part, reason } => {
                write!(f, "invalid field boundary at {part}: {reason}")
            }
            Self::UnknownEnumValue { attribute, raw, fallback } => {
                write!(f, "{attribute}: \"{raw}\" unknown — fell back to {fallback}")
            }
            Self::LayoutCacheDropped { path, reason } => {
                write!(f, "layout cache dropped at {path}: {reason}")
            }
            Self::ContentLost { part, path, kind, reason } => {
                write!(f, "{kind} lost at {part}:{path}: {reason}")
            }
            Self::ParagraphMeasureNormalized { definition, id, field, reason } => {
                write!(f, "{definition}[{id}].{field}: {reason}")
            }
            Self::BeginNumRepaired { axes } => write!(
                f,
                "hh:beginNum: start number 0 on {} is not valid; a re-encode writes 1",
                axes.join(", ")
            ),
        }
    }
}

/// 경고 변형마다 표본 하나 (테스트 전용, 안정성 보장 없음).
///
/// 하류 크레이트의 테스트가 "새 변형이 폴백 코드로 보고되지 않는다"를
/// 확인하는 데 쓴다. 표본 목록 옆의 와일드카드 없는 `match` 가 변형 추가 시
/// 컴파일을 깨뜨려 목록 갱신을 강제한다. 디코드(`decode`)와
/// 인코드(`encode`) 경고를 함께 둔다.
#[cfg(any(test, feature = "__test-support"))]
#[doc(hidden)]
pub mod warning_samples {
    use super::{DecodeWarning, ParagraphPath, PathSeg};
    use crate::EncodeWarning;

    /// 각 [`EncodeWarning`] 변형의 표본 하나씩, 선언 순서대로.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_smithy_hwpx::decoder::warning_samples;
    ///
    /// let samples = warning_samples::encode();
    /// assert!(samples.iter().any(|w| w.code() == "LAYOUT_CACHE_DROPPED"));
    /// ```
    #[must_use]
    pub fn encode() -> Vec<EncodeWarning> {
        let path = || ParagraphPath(vec![PathSeg::Section(0), PathSeg::BodyParagraph(0)]);
        let samples = vec![
            EncodeWarning::UnclosedFieldBoundary {
                part: "Contents/section0.xml".into(),
                reason: "sample".into(),
            },
            EncodeWarning::LayoutCacheDropped { path: path(), reason: "sample".to_string() },
            EncodeWarning::NoteHeadSkipped { path: path(), reason: "sample".to_string() },
            EncodeWarning::TitleMarkSkipped { path: path(), reason: "sample".to_string() },
            EncodeWarning::NoteRestartIgnored { path: path(), reason: "sample".to_string() },
            EncodeWarning::XmlForbiddenCharsRemoved {
                part: "Contents/section0.xml".to_string(),
                location: "hp:t".to_string(),
                count: 1,
            },
            EncodeWarning::ContentLost {
                part: "Contents/section0.xml".to_string(),
                path: path(),
                kind: "unknown".to_string(),
                reason: "sample".to_string(),
            },
            EncodeWarning::PackageIdConflict {
                id: "header".to_string(),
                hrefs: vec!["Contents/header.xml".to_string(), "BinData/header.png".to_string()],
            },
            EncodeWarning::ImageReferenceUnresolved {
                part: "Contents/header.xml".to_string(),
                location: "hc:img@binaryItemIDRef".to_string(),
                reference: "image1".to_string(),
            },
            EncodeWarning::ObjectIdConflict {
                id: 7,
                kinds: vec!["table".to_string(), "picture".to_string()],
            },
        ];
        for sample in &samples {
            // 와일드카드 금지 — 새 변형은 여기서 컴파일 오류가 나고,
            // 그때 위 표본에도 하나를 더한다.
            match sample {
                EncodeWarning::UnclosedFieldBoundary { .. }
                | EncodeWarning::LayoutCacheDropped { .. }
                | EncodeWarning::NoteHeadSkipped { .. }
                | EncodeWarning::TitleMarkSkipped { .. }
                | EncodeWarning::NoteRestartIgnored { .. }
                | EncodeWarning::XmlForbiddenCharsRemoved { .. }
                | EncodeWarning::ContentLost { .. }
                | EncodeWarning::PackageIdConflict { .. }
                | EncodeWarning::ImageReferenceUnresolved { .. }
                | EncodeWarning::ObjectIdConflict { .. } => {}
            }
        }
        samples
    }

    /// 각 [`DecodeWarning`] 변형의 표본 하나씩, 선언 순서대로.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_smithy_hwpx::decoder::warning_samples;
    ///
    /// let samples = warning_samples::decode();
    /// assert!(samples.iter().any(|w| w.code() == "CONTENT_LOST"));
    /// ```
    #[must_use]
    pub fn decode() -> Vec<DecodeWarning> {
        let path = || ParagraphPath(vec![PathSeg::Section(0), PathSeg::BodyParagraph(0)]);
        let samples = vec![
            DecodeWarning::UnknownEnumValue {
                attribute: "hp:header@applyPageType",
                raw: "SOMETIMES".to_string(),
                fallback: "BOTH",
            },
            DecodeWarning::LayoutCacheDropped { path: path(), reason: "sample".to_string() },
            DecodeWarning::ContentLost {
                part: "Contents/section0.xml".to_string(),
                path: path(),
                kind: "pic".to_string(),
                reason: "sample".to_string(),
            },
            DecodeWarning::InactiveHeaderReference {
                part: "Contents/header.xml".into(),
                kind: "memoPr".into(),
                reason: "unused".into(),
            },
            DecodeWarning::BeginNumRepaired { axes: vec!["page", "pic"] },
            DecodeWarning::ParagraphMeasureNormalized {
                definition: "paraPr",
                id: 7,
                field: "left".into(),
                reason: "default differs".into(),
            },
            DecodeWarning::UnclosedFieldBoundary {
                part: "Contents/section0.xml".into(),
                reason: "unpaired source start".into(),
            },
            DecodeWarning::InvalidFieldBoundary {
                part: "Contents/section0.xml".into(),
                reason: "unpaired end".into(),
            },
        ];
        for sample in &samples {
            // 와일드카드 금지 — 새 변형은 여기서 컴파일 오류가 나고,
            // 그때 위 표본에도 하나를 더한다.
            match sample {
                DecodeWarning::UnknownEnumValue { .. }
                | DecodeWarning::LayoutCacheDropped { .. }
                | DecodeWarning::ContentLost { .. }
                | DecodeWarning::InactiveHeaderReference { .. }
                | DecodeWarning::ParagraphMeasureNormalized { .. }
                | DecodeWarning::BeginNumRepaired { .. }
                | DecodeWarning::InvalidFieldBoundary { .. }
                | DecodeWarning::UnclosedFieldBoundary { .. } => {}
            }
        }
        samples
    }
}

/// 디코드 경고가 가리키는 문단의 중첩 경로.
///
/// `(section, paragraph)` 튜플로는 셀/머리말/각주 내부 문단을 표현할 수
/// 없어 ordered segment vector 로 기록한다. `Section` 으로 시작해 최종
/// 문단 segment 로 끝난다 — sublist 진입 시 인덱스를 리셋하거나 depth
/// 만 전달하지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParagraphPath(pub Vec<PathSeg>);

/// [`ParagraphPath`] 의 segment 하나.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PathSeg {
    /// 구역 인덱스.
    Section(usize),
    /// 본문 문단 인덱스.
    BodyParagraph(usize),
    /// 머리말 인덱스.
    Header(usize),
    /// 꼬리말 인덱스.
    Footer(usize),
    /// run 인덱스.
    Run(usize),
    /// 표 셀 (행, 셀).
    TableCell {
        /// 행 인덱스.
        row: usize,
        /// 셀 인덱스.
        cell: usize,
    },
    /// 캡션 내부.
    Caption,
    /// 각주 내부.
    Footnote,
    /// 미주 내부.
    Endnote,
    /// 글상자 내부.
    TextBox,
    /// 메모 내부.
    Memo,
    /// 묶음 객체 자식 인덱스.
    GroupChild(usize),
    /// 중첩 sublist 문단 인덱스 (컨테이너 내부 문단).
    NestedParagraph(usize),
    /// 바탕쪽 인덱스 (패키지 전역 번호).
    MasterPage(usize),
}

impl std::fmt::Display for ParagraphPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, seg) in self.0.iter().enumerate() {
            if i > 0 {
                write!(f, ".")?;
            }
            match seg {
                PathSeg::Section(n) => write!(f, "section[{n}]")?,
                PathSeg::BodyParagraph(n) => write!(f, "para[{n}]")?,
                PathSeg::Header(n) => write!(f, "header[{n}]")?,
                PathSeg::Footer(n) => write!(f, "footer[{n}]")?,
                PathSeg::Run(n) => write!(f, "run[{n}]")?,
                PathSeg::TableCell { row, cell } => write!(f, "cell[{row}][{cell}]")?,
                PathSeg::Caption => write!(f, "caption")?,
                PathSeg::Footnote => write!(f, "footnote")?,
                PathSeg::Endnote => write!(f, "endnote")?,
                PathSeg::TextBox => write!(f, "textbox")?,
                PathSeg::Memo => write!(f, "memo")?,
                PathSeg::GroupChild(n) => write!(f, "group[{n}]")?,
                PathSeg::NestedParagraph(n) => write!(f, "npara[{n}]")?,
                PathSeg::MasterPage(n) => write!(f, "masterpage[{n}]")?,
            }
        }
        Ok(())
    }
}

// ── HwpxDecoder ──────────────────────────────────────────────────

/// Decodes HWPX files (ZIP + XML) into Core's `Document<Draft>`.
///
/// # Examples
///
/// ```no_run
/// use hwpforge_smithy_hwpx::HwpxDecoder;
///
/// let bytes = std::fs::read("document.hwpx").unwrap();
/// let result = HwpxDecoder::decode(&bytes).unwrap();
/// println!("Sections: {}", result.document.sections().len());
/// ```
pub struct HwpxDecoder;

/// Decoder-only context: callers cannot construct this type or derive it from a claim.
#[derive(Debug)]
pub(crate) struct SourceBasis {
    // Header-read fact belongs to the original package, not editable JSON styles.
    requires_header_normalization: bool,
    package: std::sync::Arc<[u8]>,
    origins: std::collections::HashMap<
        hwpforge_core::node_path::CorePath,
        hwpforge_core::source_xml::UnclosedOrigin,
    >,
}

impl HwpxDecoder {
    /// Decodes an HWPX file from raw bytes.
    ///
    /// Pipeline:
    /// 1. Open ZIP archive, validate mimetype
    /// 2. Parse `Contents/header.xml` → `HwpxStyleStore`
    /// 3. Parse `Contents/section*.xml` → paragraphs + page settings
    /// 4. Assemble `Document<Draft>` with sections
    ///
    /// # Limits
    ///
    /// `header.xml` and `section*.xml` are deserialized with a nesting budget
    /// of 224 XML levels (provisional: it may change in a minor release once
    /// more real documents have been measured), enough for tables, text boxes or
    /// table/text-box mixes nested 32 deep. The 33rd level is rejected as
    /// [`HwpxError::InvalidStructure`](crate::HwpxError::InvalidStructure) by
    /// our structural nesting guards. A part deeper than the budget, or one
    /// with more than 128 namespace bindings in scope, fails with
    /// [`HwpxError::XmlParse`](crate::HwpxError::XmlParse). Other parts
    /// (`content.hpf`, charts) are read without these two limits.
    ///
    /// Successful decode guarantees that every section, including headers,
    /// footers and master pages, is within [`hwpforge_core::MAX_NESTING_DEPTH`].
    /// Semantic overflow returns [`HwpxError::Core`](crate::HwpxError::Core)
    /// (E4008), with `nesting_depth` context and the actual section index.
    /// This does not perform other Core structural validation.
    /// Parser guard messages (E4006) report the one-based level the decoder
    /// tried to open and its part; that level need not equal Core depth.
    ///
    /// # Stack
    ///
    /// Deeply nested input recurses on the calling thread's stack, and an
    /// overflow aborts the process instead of returning an error. Measured
    /// through this function on macOS arm64, with packages holding nested
    /// tables and text boxes up to and past the budget:
    ///
    /// | Build | Stack that decoded every input | Overflowed at |
    /// | -- | -- | -- |
    /// | release (optimized) | 1 MiB | 512 KiB |
    /// | `opt-level = 1` (the `cargo test` profile) | 1 MiB | 768 KiB |
    /// | `opt-level = 0` (unoptimized dev build) | 4 MiB | 3 MiB |
    ///
    /// So in an optimized build give the calling thread 2 MiB of stack.
    /// 1 MiB is the measured minimum, but how much a build needs depends on
    /// how the compiler inlines the recursive decode: a test binary that
    /// compiled extra code into the same crate needed about twice as much
    /// for the same input. Rust's default 2 MiB spawned threads qualify, and
    /// so does the main thread on Linux and macOS (typically 8 MiB). On
    /// Windows the main thread defaults to 1 MiB in total, part of it
    /// already used by the caller, so decode deeply nested input on a
    /// spawned thread there. An unoptimized build needs about 4 MiB, which
    /// Rust's default spawned threads do not have. The 1 MiB release figure
    /// is also checked by a test on Linux x86_64 in CI; other platforms are
    /// not measured.
    pub fn decode(bytes: &[u8]) -> HwpxResult<HwpxDocument> {
        // Step 1: Open package
        let mut pkg = package::PackageReader::new(bytes)?;

        // Step 2: Parse header (style store + begin_num)
        let header_xml = pkg.read_header_xml()?;
        let header_result = header::parse_header(&header_xml)?;
        let style_store = header_result.style_store;
        let mut references = crate::header_references::HeaderReferences::build(&style_store)?;
        references.memos = header_result.memo_count;
        references.track_changes = header_result.track_change_count;
        references.track_change_authors = header_result.track_change_author_count;
        references.validate(&style_store, false)?;
        let begin_num = header_result.begin_num;

        // Step 2b (Wave 12o): Parse Contents/content.hpf metadata.
        // Missing or malformed metadata downgrades to default rather
        // than failing the whole decode — Hancom-emitted XML always
        // has a metadata block, but third-party authoring tools may not.
        let metadata = match pkg.read_text_entry("Contents/content.hpf") {
            Ok(xml) => metadata::parse_content_hpf_metadata(&xml).unwrap_or_default(),
            Err(_) => hwpforge_core::metadata::Metadata::default(),
        };

        // Step 3: Extract chart XMLs from ZIP
        let chart_xmls = pkg.read_chart_xmls()?;

        // Step 4: Extract masterpage XMLs from ZIP, by file number, and the
        // manifest ids that name them (`secPr/masterPage@idRef` resolves
        // through `content.hpf`).
        // Ordered by file number, so the unreferenced-part warnings come in a
        // fixed order.
        let masterpage_xmls = pkg.read_masterpage_parts()?;
        let manifest = pkg
            .read_text_entry("Contents/content.hpf")
            .map(|xml| manifest_hrefs(&xml))
            .unwrap_or_default();

        // Step 5: Parse sections
        let mut document = Document::<Draft>::with_metadata(metadata);
        let mut has_memo = false;
        let section_count = pkg.section_count();
        // Where each master page part parsed so far was first kept (section,
        // position in its master pages) — copies are taken from there, so no
        // further copy is held — and the budget every kept master page, first
        // or copy, is charged against.
        let mut parsed_masterpages: std::collections::BTreeMap<usize, (usize, usize)> =
            std::collections::BTreeMap::new();
        let mut master_budget = CopyBudget::new(MASTER_PAGE_BUDGET);
        let mut named_masterpages = std::collections::BTreeSet::new();
        let mut warnings: Vec<DecodeWarning> = header_begin_num_warnings(begin_num.as_ref());
        warnings.extend(header_result.warnings);
        warnings.extend(references.validate_auxiliary_xml(&header_xml, "Contents/header.xml")?);
        warnings.extend(pkg.order.problems.iter().map(|reason| DecodeWarning::ContentLost {
            part: "Contents/content.hpf / META-INF/container.xml".into(),
            path: ParagraphPath(Vec::new()),
            kind: "package_reading_order".into(),
            reason: reason.clone(),
        }));

        // BinData is read before the sections: a picture without an
        // `<hp:imgDim>` is judged whole against its image's own size.
        let image_store = pkg.read_all_bindata()?;
        let image_pixels = image_size::pixel_sizes_by_reference(&image_store);

        let mut source_maps = Vec::with_capacity(section_count);
        for i in 0..section_count {
            let section_xml = pkg.read_section_xml(i)?;
            let mut result = section::parse_section_with_references(
                &section_xml,
                i,
                &pkg.section_paths()[i],
                &chart_xmls,
                &image_pixels,
                Some(&references),
            )?;
            warnings.append(&mut result.warnings);

            let page_settings = result.page_settings.unwrap_or_else(PageSettings::a4);

            // The master pages this section owns are the parts its
            // `masterPage@idRef`s name — not the next ones in file order
            // (two sections whose references were swapped
            // read each other's master page, and a re-encode made it
            // permanent). A reference that names no master page part is lost
            // content; `masterPageCnt` only checks the count.
            // 이것을 실패시키는 것: 파일 순서로 배정(`swapped_master_page_references_follow_the_ids`),
            // 미해결 참조 무시(`an_unresolved_master_page_reference_is_lost_content`).
            let section_part = pkg.section_paths()[i].clone();
            let lost = |reason: String| DecodeWarning::ContentLost {
                part: section_part.clone(),
                path: ParagraphPath(vec![PathSeg::Section(i)]),
                kind: "master_page".into(),
                reason,
            };
            let refs = master_page_refs(&section_xml);
            let mp_cnt = result.master_page_cnt;
            if refs.is_empty() && mp_cnt > 0 {
                warnings
                    .push(lost(format!("masterPageCnt {mp_cnt} without a masterPage reference")));
            }
            let mut owned: Vec<MasterPage> = Vec::with_capacity(refs.len());
            for id in &refs {
                let (number, _part) = match resolve_master_page_ref(id, &manifest, &masterpage_xmls)
                {
                    Ok(number) => number,
                    Err(reason) => {
                        warnings.push(lost(reason));
                        continue;
                    }
                };
                let xml = &masterpage_xmls[&number].1;
                // A part is parsed once; every reference to it (sections may
                // share a master page: `masterPage@idRef` has no owner) keeps
                // a master page, first and copies alike charged the part's XML
                // size against one budget (`MASTER_PAGE_BUDGET`), checked
                // before parsing or copying; only an admitted reference is
                // charged, one past the budget is lost content. Copies come from the first kept
                // master page, so nothing else is held.
                // 이것을 실패시키는 것: 공유를 손실로 보기(`shared_master_pages_are_kept`),
                // 예산 없이 복제하기 · Core 일부만 세기(`a_master_page_part_is_read_once`),
                // 거절도 과금하기(`copy_budget_charges_admitted_copies_only`).
                // Named by a section, kept or not (a part refused by
                // the budget is not also "referenced by no section").
                // 이것을 실패시키는 것: 보관 맵으로 무참조를 판정하기
                // (`a_refused_master_page_is_not_reported_unreferenced`).
                named_masterpages.insert(number);
                let cost = xml.len();
                if !master_budget.admit(cost) {
                    warnings.push(lost(format!(
                        "masterPage idRef {id:?}: master page {number} ({cost} bytes) does not fit \
                         the master page budget ({MASTER_PAGE_BUDGET} bytes, {} used)",
                        master_budget.spent
                    )));
                    continue;
                }
                let master = match parsed_masterpages.get(&number) {
                    Some(&(section, at)) if section == i => owned[at].clone(),
                    Some(&(section, at)) => document
                        .sections()
                        .get(section)
                        .and_then(|s| s.master_pages.as_ref())
                        .and_then(|pages| pages.get(at))
                        .cloned()
                        .ok_or_else(|| crate::error::HwpxError::InvalidStructure {
                            detail: format!(
                                "master page {number} kept by section {section} is gone"
                            ),
                        })?,
                    None => {
                        let (master, mut mp_warnings, master_has_memo) =
                            section::parse_master_page_with_references(
                                xml,
                                i,
                                number,
                                &image_pixels,
                                Some(&references),
                            )?;
                        has_memo |= master_has_memo;
                        warnings.append(&mut mp_warnings);
                        parsed_masterpages.insert(number, (i, owned.len()));
                        master
                    }
                };
                owned.push(master);
            }
            has_memo |= result.has_memo;
            let section_master_pages =
                if owned.is_empty() { result.master_pages } else { Some(owned) };

            let section = Section {
                paragraphs: result.paragraphs,
                page_settings,
                // ADR-002 cardinality — 파서가 wire 순서 그대로의 Vec 을
                // 돌려준다 (W5-α C1: ODD/EVEN 다중 머리말 보존).
                headers: result.headers,
                footers: result.footers,
                page_number: result.page_number,
                column_settings: result.column_settings,
                visibility: result.visibility,
                line_number_shape: result.line_number_shape,
                page_border_fills: result.page_border_fills,
                master_pages: section_master_pages,
                // Per-section startNum from secPr; merge footnote/endnote
                // from header.xml for the first section. Only those two: the
                // header's page/pic/tbl/equation are the document's start
                // numbers, which `Section::begin_num` (a section restart, 0 =
                // continue) cannot hold — without a startNum the section
                // continues.
                begin_num: {
                    let mut bn = result.begin_num;
                    if i == 0 {
                        if let Some(header_bn) = &begin_num {
                            let section_bn = bn.get_or_insert(BeginNum {
                                page: 0,
                                pic: 0,
                                tbl: 0,
                                equation: 0,
                                ..BeginNum::default()
                            });
                            section_bn.footnote = header_bn.footnote;
                            section_bn.endnote = header_bn.endnote;
                        }
                    }
                    bn
                },
                text_direction: result.text_direction,
            };

            // 이것을 실패시키는 것: 관문 삭제/본문만 검사/section 색인 보정 삭제
            // (tests/decode_depth.rs). Keep this before source.finish.
            crate::depth::check_section_depth(&section, i)?;

            source_maps.push(result.source.finish(&section, i, &section_part));
            document.add_section(section);
        }
        for number in masterpage_xmls.keys().filter(|n| !named_masterpages.contains(*n)) {
            warnings.push(DecodeWarning::ContentLost {
                part: format!("Contents/masterpage{number}.xml"),
                path: ParagraphPath::default(),
                kind: "master_page".into(),
                reason: "no section references this master page".into(),
            });
        }
        crate::header_references::HeaderReferences::validate_body_borders(
            &document,
            1..=style_store.border_fill_count() as u32,
        )?;
        drop_cell_zones_with_unknown_border_fill(
            &mut document,
            &style_store,
            &mut warnings,
            pkg.section_paths(),
        );
        // Native files keep a default memo definition even without any memo.
        // Warn about unused metadata without rejecting unrelated content edits.
        if !has_memo {
            for warning in &mut warnings {
                if let DecodeWarning::ContentLost { part, kind, reason, .. } = warning {
                    if matches!(kind.as_str(), "memoPr" | "secPr.memoShapeIDRef") {
                        *warning = DecodeWarning::InactiveHeaderReference {
                            part: part.clone(),
                            kind: kind.clone(),
                            reason: reason.clone(),
                        };
                    }
                }
            }
        }
        let origins: Vec<_> = source_maps.iter().flat_map(|m| m.unclosed.iter().cloned()).collect();
        let package: std::sync::Arc<[u8]> = std::sync::Arc::from(bytes);
        hwpforge_core::field_inventory::FieldInventory::recover_shared_source_ranges(
            document.sections_mut(),
            &origins,
            package.clone(),
        );
        let source_basis = std::sync::Arc::new(SourceBasis {
            requires_header_normalization: header_result.requires_header_normalization,
            package,
            origins: origins.into_iter().map(|origin| (origin.path().clone(), origin)).collect(),
        });
        document.set_format_source(source_basis.clone());
        let inventory =
            hwpforge_core::field_inventory::FieldInventory::from_sections(document.sections());
        for diagnostic in inventory.warnings() {
            if let hwpforge_core::ValidationError::InvalidFieldBoundary { section_index, .. } =
                &diagnostic
            {
                warnings.push(DecodeWarning::UnclosedFieldBoundary {
                    part: pkg.section_paths()[*section_index].clone(),
                    reason: diagnostic.to_string(),
                });
            }
        }
        for diagnostic in inventory.diagnostics() {
            if let hwpforge_core::ValidationError::InvalidFieldBoundary { section_index, .. } =
                &diagnostic
            {
                warnings.push(DecodeWarning::InvalidFieldBoundary {
                    part: pkg.section_paths()[*section_index].clone(),
                    reason: diagnostic.to_string(),
                });
            }
        }

        Ok(HwpxDocument {
            document,
            style_store,
            image_store,
            warnings,
            source_maps,
            source_basis: Some(source_basis),
        })
    }

    /// Verify claims only against the separately supplied, decoded base package.
    /// Its SourceMap locates claims; original package bytes are re-read on every call.
    /// Submitted claim package bytes never select the verification base.
    ///
    /// # Errors
    /// Rejects excessive section nesting, missing/stale claims and any mismatch
    /// with the base SourceMap.
    pub fn verify_source_provenance(sections: &[Section], base: &HwpxDocument) -> HwpxResult<()> {
        for section in sections {
            section.check_nesting_depth().map_err(|error| crate::HwpxError::InvalidStructure {
                detail: error.to_string(),
            })?;
        }
        Self::verify_against_basis(sections, base.source_basis.as_deref())
    }

    /// Replace submitted claims and bind the caller's base independently of those claims.
    ///
    /// # Errors
    /// Rejects excessive nesting in the submitted document or base document,
    /// and claims that cannot be restored from this base.
    pub fn restore_source_provenance(
        document: &mut Document<Draft>,
        base: &HwpxDocument,
    ) -> HwpxResult<()> {
        for section in document.sections().iter().chain(base.document.sections()) {
            section.check_nesting_depth().map_err(|error| crate::HwpxError::InvalidStructure {
                detail: error.to_string(),
            })?;
        }
        hwpforge_core::field_inventory::FieldInventory::restore_source_provenance(
            document.sections_mut(),
            base.document.sections(),
        );
        Self::verify_source_provenance(document.sections(), base)?;
        if let Some(basis) = &base.source_basis {
            document.set_format_source(basis.clone());
        }
        Ok(())
    }

    pub(crate) fn verify_document_source<S>(document: &Document<S>) -> HwpxResult<()> {
        Self::verify_against_basis(
            document.sections(),
            document.format_source::<SourceBasis>().as_deref(),
        )
    }

    fn verify_against_basis(sections: &[Section], basis: Option<&SourceBasis>) -> HwpxResult<()> {
        use hwpforge_core::field_inventory::FieldInventory;
        let mut originals = Vec::new();
        for (current, start) in FieldInventory::source_claims(sections) {
            hwpforge_core::source_xml::record_provenance_work(1);
            let invalid = || crate::HwpxError::InvalidStructure {
                detail: format!("unverified source field at {}: {}", current.relative(), start.id),
            };
            let claim = start.source_claim().ok_or_else(invalid)?;
            if !start.unclosed || claim.path() != &current || claim.id() != start.id {
                return Err(invalid());
            }
            let basis = basis.ok_or_else(invalid)?;
            let source = basis.origins.get(claim.source_path()).ok_or_else(invalid)?;
            if source.id() != claim.id()
                || source.part() != claim.part()
                || source.range() != claim.range()
                || source.digest() != claim.digest()
            {
                return Err(invalid());
            }
            originals.push(source.clone());
        }
        // SourceMap is an address index, not authority. Re-read the independent
        // package on every acceptance/output, sharing parses only within this call.
        // 이것을 실패시키는 것: raw 검증 생략 또는 작업 간 파싱 캐시 유지.
        if !originals.is_empty()
            && !hwpforge_core::source_xml::UnclosedOrigin::verify_package_batch(
                &originals,
                &basis.expect("claims require an independent basis").package,
            )
        {
            return Err(crate::HwpxError::InvalidStructure {
                detail: "unverified original source field bytes".into(),
            });
        }
        Ok(())
    }

    /// Decodes an HWPX file from a filesystem path.
    ///
    /// The nesting and namespace limits of [`decode`](Self::decode) apply,
    /// including its provisional 224-level budget and its stack advice for
    /// the calling thread (2 MiB recommended, 1 MiB measured).
    pub fn decode_file(path: impl AsRef<Path>) -> HwpxResult<HwpxDocument> {
        let bytes = std::fs::read(path.as_ref()).map_err(crate::error::HwpxError::Io)?;
        Self::decode(&bytes)
    }
}

/// A table cell zone whose `borderFillIDRef` names no
/// border fill of the header (looked up by id, not by count) is dropped and
/// reported as lost content — a renderer would draw nothing for it.
/// 이것을 실패시키는 것: 이 검사 삭제(`cell_zone_with_an_unknown_border_fill_is_lost`).
fn drop_cell_zones_with_unknown_border_fill(
    document: &mut Document<hwpforge_core::document::Draft>,
    style_store: &HwpxStyleStore,
    warnings: &mut Vec<DecodeWarning>,
    section_parts: &[String],
) {
    for (i, section) in document.sections_mut().iter_mut().enumerate() {
        section.for_each_table_mut(|table| {
            table.cell_zones.retain(|zone| {
                let known = crate::header_references::HeaderReferences::border_fill(
                    style_store,
                    zone.border_fill_id,
                )
                .is_ok();
                if !known {
                    warnings.push(DecodeWarning::ContentLost {
                        part: section_parts[i].clone(),
                        path: ParagraphPath::default(),
                        kind: "cellzone".to_string(),
                        reason: format!(
                            "cell zone border fill {} is not in the header",
                            zone.border_fill_id
                        ),
                    });
                }
                known
            });
        });
    }
}

/// What re-encoding does to the header's start numbers (`<hh:beginNum>`):
/// page/pic/tbl/equation are written as Hancom's `1`, because Core keeps
/// only the first section's restart numbers (footnote and endnote are
/// merged into it and kept). A `0` there is a schema violation that this
/// fixes ([`DecodeWarning::BeginNumRepaired`]); a valid value other than
/// `1` has nowhere to go in Core and is lost
/// ([`DecodeWarning::ContentLost`] `begin_num`, which editors refuse).
fn header_begin_num_warnings(header: Option<&BeginNum>) -> Vec<DecodeWarning> {
    let Some(bn) = header else {
        return Vec::new();
    };
    let axes = [("page", bn.page), ("pic", bn.pic), ("tbl", bn.tbl), ("equation", bn.equation)];
    let mut warnings = Vec::new();
    let zero: Vec<&'static str> =
        axes.iter().filter(|(_, value)| *value == 0).map(|(name, _)| *name).collect();
    if !zero.is_empty() {
        warnings.push(DecodeWarning::BeginNumRepaired { axes: zero });
    }
    let lost: Vec<String> = axes
        .iter()
        .filter(|(_, value)| *value > 1)
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    if !lost.is_empty() {
        warnings.push(DecodeWarning::ContentLost {
            part: HEADER_PART.to_string(),
            path: ParagraphPath::default(),
            kind: "begin_num".to_string(),
            reason: format!(
                "header beginNum {} is not carried (a re-encode writes 1)",
                lost.join(" ")
            ),
        });
    }
    warnings
}

// ── Masterpage helpers ────────────────────────────────────────────

/// Extracts `masterPageCnt` from the `<hp:secPr>` element in a section XML string.
///
/// Scans the raw XML for `masterPageCnt="N"` without re-parsing the full XML.
/// Returns 0 if the attribute is absent or unparseable.
/// The master page XML (decompressed part bytes) every section's kept
/// master pages together may come from — each kept master page, first or
/// copy, is charged its part's size. The decoded master pages are built from
/// that XML, so they are as large as decoding that much XML directly would
/// make them: sharing cannot make the package decode to more than a package
/// of this size could (an entry is capped at 50 MiB, the package at
/// 500 MiB). It lets a large master page be shared by two sections and a
/// small one by many (64 sections of 1 MiB), and charges every Core form a
/// part becomes — text, tab-mixed text, captions, objects — without a model
/// of each.
const MASTER_PAGE_BUDGET: usize = 64 << 20;

/// A running total checked before each charge: a charge that does not fit
/// is refused and leaves the total as it was.
#[derive(Debug)]
struct CopyBudget {
    limit: usize,
    spent: usize,
}

impl CopyBudget {
    fn new(limit: usize) -> Self {
        Self { limit, spent: 0 }
    }

    /// Charges `cost` if it fits, and says whether it did.
    fn admit(&mut self, cost: usize) -> bool {
        match self.spent.checked_add(cost) {
            Some(total) if total <= self.limit => {
                self.spent = total;
                true
            }
            _ => false,
        }
    }
}

/// Resolves each section's master parts through the same manifest lookup used
/// by the decoder. Section and list order are preserved; shared parts may recur.
/// Analysis fails on unresolved references instead of guessing file order.
///
/// # Errors
/// Returns an error if the package cannot be read or a master reference is
/// missing, ambiguous, or names a missing part.
pub fn master_page_part_paths(
    pkg: &mut package::PackageReader<'_>,
) -> HwpxResult<Vec<Vec<String>>> {
    let parts = pkg.read_masterpage_parts()?;
    let manifest = pkg
        .read_text_entry("Contents/content.hpf")
        .map(|xml| manifest_hrefs(&xml))
        .unwrap_or_default();
    (0..pkg.section_count())
        .map(|index| {
            let xml = pkg.read_section_xml(index)?;
            master_page_refs(&xml)
                .iter()
                .map(|id| {
                    resolve_master_page_ref(id, &manifest, &parts)
                        .map(|(_, path)| path.to_owned())
                        .map_err(|detail| crate::HwpxError::XmlParse {
                            file: pkg.section_paths()[index].clone(),
                            detail,
                        })
                })
                .collect()
        })
        .collect()
}

fn resolve_master_page_ref<'a>(
    id: &str,
    manifest: &std::collections::HashMap<String, Option<String>>,
    parts: &'a std::collections::BTreeMap<usize, (String, String)>,
) -> Result<(usize, &'a str), String> {
    let href = match manifest.get(id) {
        Some(Some(href)) => href,
        Some(None) => {
            return Err(format!("masterPage idRef {id:?} names more than one manifest item"))
        }
        None => return Err(format!("masterPage idRef {id:?} names no master page part")),
    };
    masterpage_number(href)
        .and_then(|n| parts.get(&n).map(|(path, _)| (n, path.as_str())))
        .ok_or_else(|| format!("masterPage idRef {id:?} names no master page part"))
}

/// `id → href` of every `<opf:item>` in a `content.hpf` manifest.
fn manifest_hrefs(xml: &str) -> std::collections::HashMap<String, Option<String>> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut out: std::collections::HashMap<String, Option<String>> =
        std::collections::HashMap::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) if e.local_name().as_ref() == "item" => {
                let attr = |key: &str| {
                    e.attributes().flatten().find(|a| a.key.local_name().as_ref() == key).and_then(
                        |a| {
                            a.normalized_value(quick_xml::XmlVersion::Explicit1_0)
                                .ok()
                                .map(|v| v.into_owned())
                        },
                    )
                };
                if let (Some(id), Some(href)) = (attr("id"), attr("href")) {
                    // An id given to two different parts names neither: keep
                    // that as `None` rather than letting the last item win
                    // 이것을 실패시키는 것: 마지막 항목으로 덮어쓰기
                    // (`an_ambiguous_manifest_id_is_lost_content`).
                    out.entry(id)
                        .and_modify(|seen| {
                            if seen.as_deref() != Some(href.as_str()) {
                                *seen = None;
                            }
                        })
                        .or_insert(Some(href));
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// The `idRef` of every `<masterPage>` in a section XML, in order.
fn master_page_refs(section_xml: &str) -> Vec<String> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(section_xml);
    let mut out = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) if e.local_name().as_ref() == "masterPage" => {
                if let Some(id) = e
                    .attributes()
                    .flatten()
                    .find(|a| a.key.local_name().as_ref() == "idRef")
                    .and_then(|a| a.normalized_value(quick_xml::XmlVersion::Explicit1_0).ok())
                {
                    out.push(id.into_owned());
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// The file number of a `Contents/masterpageN.xml` href.
fn masterpage_number(href: &str) -> Option<usize> {
    href.strip_prefix("Contents/masterpage")?.strip_suffix(".xml")?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 이것을 실패시키는 것: 주소 인덱스만 신뢰하거나 원본 검증에 claim 패키지 사용.
    #[test]
    fn acceptance_and_output_reject_stale_index_against_actual_package() {
        use hwpforge_core::source_xml::source_work;
        use std::sync::Arc;
        let mut package = crate::patch::RawPackage::read(&master_pages_doc(&[None, None])).unwrap();
        for part in ["Contents/section0.xml", "Contents/section1.xml"] {
            let xml = package.read_text_entry(part).unwrap();
            let field = r#"<hp:run charPrIDRef="0"><hp:ctrl><hp:fieldBegin id="77" type="FORMULA" name="" editable="1" dirty="0" zorder="0" fieldid="1"/></hp:ctrl><hp:t>PROTECTED</hp:t></hp:run></hp:p>"#;
            package.replace_text_entry(part, xml.replacen("</hp:p>", field, 1));
        }
        let bytes = package.write().unwrap();
        let mut base = HwpxDecoder::decode(&bytes).unwrap();
        let basis = base.source_basis.clone().unwrap();
        assert_eq!(basis.origins.len(), 2);
        // 이것을 실패시키는 것: 여러 파트를 하나로 취급하거나 작업 간 raw 캐시 유지.
        let before = source_work();
        for k in 1..=3 {
            HwpxDecoder::verify_source_provenance(base.document.sections(), &base).unwrap();
            assert_eq!(source_work().0 - before.0, k * 2);
        }
        for change in ["missing", "begin", "id", "pair"] {
            let mut altered = crate::patch::RawPackage::read(&bytes).unwrap();
            let part = "Contents/section1.xml";
            let xml = altered.read_text_entry(part).unwrap();
            let xml = match change {
                "begin" => xml.replace("fieldBegin", "otherBegin"),
                "id" => xml.replace(r#"id="77""#, r#"id="88""#),
                "pair" => xml.replacen("</hp:p>", r#"<hp:run charPrIDRef="0"><hp:ctrl><hp:fieldEnd beginIDRef="77"/></hp:ctrl></hp:run></hp:p>"#, 1),
                _ => xml,
            };
            altered.replace_text_entry(part, xml);
            let package: Arc<[u8]> = if change == "missing" {
                Arc::from([])
            } else {
                Arc::from(altered.write().unwrap())
            };
            let stale = Arc::new(SourceBasis {
                package,
                origins: basis.origins.clone(),
                requires_header_normalization: basis.requires_header_normalization,
            });
            base.source_basis = Some(stale.clone());
            let mut submitted = base.document.clone();
            submitted.set_format_source(stale);
            assert!(
                HwpxDecoder::verify_source_provenance(submitted.sections(), &base).is_err(),
                "{change}"
            );
            assert!(
                crate::HwpxEncoder::encode(
                    &submitted.validate().unwrap(),
                    &base.style_store,
                    &base.image_store
                )
                .is_err(),
                "{change}"
            );
        }
    }

    /// Two sections owning master pages "A" and "B", encoded.
    fn two_master_pages() -> Vec<u8> {
        master_pages_doc(&[Some("A"), Some("B")])
    }

    /// One section per entry, owning a master page with that text (`None`:
    /// no master page), encoded.
    fn master_pages_doc(names: &[Option<&str>]) -> Vec<u8> {
        use hwpforge_core::run::Run;
        use hwpforge_core::section::MasterPage;
        use hwpforge_core::{PageSettings, Paragraph};
        use hwpforge_foundation::{ApplyPageType, CharShapeIndex, ParaShapeIndex};
        let para = |t: &str| {
            Paragraph::with_runs(vec![Run::text(t, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
        };
        let mut doc = Document::new();
        for name in names {
            let mut section = Section::with_paragraphs(vec![para("본문")], PageSettings::a4());
            section.master_pages =
                name.map(|name| vec![MasterPage::new(ApplyPageType::Both, vec![para(name)])]);
            doc.add_section(section);
        }
        let store = crate::style_store_for_preset("default").unwrap();
        crate::HwpxEncoder::encode(
            &doc.validate().unwrap(),
            &store,
            &hwpforge_core::image::ImageStore::new(),
        )
        .unwrap()
    }

    /// `bytes` with section `i`'s `masterPage@idRef` set to `ids[i]`, or a
    /// reference added where the section has none.
    fn with_master_page_refs(bytes: &[u8], ids: &[&str]) -> Vec<u8> {
        let mut pkg = crate::patch::RawPackage::read(bytes).unwrap();
        let mut part = 0;
        for (i, id) in ids.iter().enumerate() {
            let path = pkg.section_path(i).unwrap();
            let xml = pkg.read_text_entry(&path).unwrap();
            let own = format!(r#"<hp:masterPage idRef="masterpage{part}"/>"#);
            let new = format!(r#"<hp:masterPage idRef="{id}"/>"#);
            let xml = if xml.contains(&own) {
                part += 1;
                xml.replace(&own, &new)
            } else {
                let at = xml.find("</hp:secPr>").expect("secPr");
                format!("{}{new}{}", &xml[..at], &xml[at..])
            };
            pkg.replace_text_entry(&path, xml);
        }
        pkg.write().unwrap()
    }

    fn master_page_texts(doc: &HwpxDocument) -> Vec<String> {
        doc.document
            .sections()
            .iter()
            .map(|s| {
                s.master_pages.as_ref().map_or(String::new(), |mps| {
                    mps.iter()
                        .flat_map(|mp| &mp.paragraphs)
                        .flat_map(|p| &p.runs)
                        .filter_map(|r| r.content.as_text().map(str::to_string))
                        .collect()
                })
            })
            .collect()
    }

    /// Each section owns the master page its `idRef`
    /// names. With the two references swapped, section 0 owns B and section
    /// 1 owns A — and a re-encode keeps it that way.
    /// 이것을 실패시키는 것: 파일 순서로 배정(A·B 로 읽힌다).
    #[test]
    fn swapped_master_page_references_follow_the_ids() {
        let plain = HwpxDecoder::decode(&two_master_pages()).unwrap();
        assert_eq!(master_page_texts(&plain), ["A", "B"]);
        let swapped = with_master_page_refs(&two_master_pages(), &["masterpage1", "masterpage0"]);
        let decoded = HwpxDecoder::decode(&swapped).unwrap();
        assert!(decoded.warnings.is_empty(), "{:?}", decoded.warnings);
        assert_eq!(master_page_texts(&decoded), ["B", "A"]);
        let validated = decoded.document.clone().validate().unwrap();
        let again =
            crate::HwpxEncoder::encode(&validated, &decoded.style_store, &decoded.image_store)
                .unwrap();
        assert_eq!(master_page_texts(&HwpxDecoder::decode(&again).unwrap()), ["B", "A"]);
    }

    /// `bytes` with `edit` applied to section 1 and to the manifest.
    fn with_section1_and_manifest(
        bytes: &[u8],
        section1: impl Fn(&str) -> String,
        manifest: impl Fn(&str) -> String,
    ) -> Vec<u8> {
        let mut pkg = crate::patch::RawPackage::read(bytes).unwrap();
        let xml = pkg.read_text_entry("Contents/section1.xml").unwrap();
        pkg.replace_text_entry("Contents/section1.xml", section1(&xml));
        let hpf = pkg.read_text_entry("Contents/content.hpf").unwrap();
        pkg.replace_text_entry("Contents/content.hpf", manifest(&hpf));
        pkg.write().unwrap()
    }

    fn lost_messages(doc: &HwpxDocument) -> Vec<String> {
        doc.warnings.iter().filter(|w| w.is_content_loss()).map(|w| w.message()).collect()
    }

    /// An id the manifest gives two parts names
    /// neither. Both parts are also used by unambiguous ids, so no
    /// unreferenced-part report hides the case.
    /// 이것을 실패시키는 것: 마지막 항목으로 덮어쓰기(구역 1 이 B 를 하나 더 받는다).
    #[test]
    fn an_ambiguous_manifest_id_is_lost_content() {
        let own = r#"<hp:masterPage idRef="masterpage1"/>"#;
        let bytes = with_section1_and_manifest(
            &two_master_pages(),
            |xml| xml.replace(own, &format!(r#"{own}<hp:masterPage idRef="x"/>"#)),
            |hpf| {
                let at = hpf.find("<opf:item ").unwrap();
                format!(
                    r#"{}<opf:item id="x" href="Contents/masterpage0.xml" media-type="application/xml"/><opf:item id="x" href="Contents/masterpage1.xml" media-type="application/xml"/>{}"#,
                    &hpf[..at],
                    &hpf[at..]
                )
            },
        );
        let decoded = HwpxDecoder::decode(&bytes).unwrap();
        assert_eq!(master_page_texts(&decoded), ["A", "B"]);
        let lost = lost_messages(&decoded);
        assert_eq!(lost.len(), 1, "{lost:?}");
        assert!(lost[0].contains("more than one manifest item"), "{lost:?}");
    }

    /// References to one large part are
    /// admitted while their decoded size fits the master page budget; the
    /// rest are each reported, so repeated references cannot multiply it.
    /// 이것을 실패시키는 것: 예산 없이 참조마다 복제하기(31 개 모두 들어간다).
    #[test]
    fn a_master_page_part_is_read_once() {
        let big = "가".repeat(1 << 20); // about 3 MiB of text
        let bytes = master_pages_doc(&[Some(&big), Some("B")]);
        let own = r#"<hp:masterPage idRef="masterpage1"/>"#;
        let more = r#"<hp:masterPage idRef="masterpage0"/>"#.repeat(30);
        let bytes = with_section1_and_manifest(
            &bytes,
            |xml| xml.replace(own, &format!("{own}{more}")),
            str::to_string,
        );
        let decoded = HwpxDecoder::decode(&bytes).unwrap();
        let kept = decoded.document.sections()[1].master_pages.as_ref().unwrap().len() - 1;
        let lost = lost_messages(&decoded);
        // Every kept master page is charged its part's XML size, the first
        // one too.
        let (a, b) = (part_len(&bytes, 0), part_len(&bytes, 1));
        assert_eq!(kept, (MASTER_PAGE_BUDGET - a - b) / a, "{kept}");
        assert!((2..30).contains(&kept), "{kept}");
        assert_eq!(lost.len(), 30 - kept, "{lost:?}");
        assert!(lost.iter().all(|m| m.contains("does not fit the master page budget")));
    }

    fn part_len(bytes: &[u8], number: usize) -> usize {
        crate::patch::RawPackage::read(bytes)
            .unwrap()
            .read_text_entry(&format!("Contents/masterpage{number}.xml"))
            .unwrap()
            .len()
    }

    /// The charge does not depend on the Core form the
    /// part becomes — a 3 MiB text with a tab (tab-mixed text) and a 3 MiB
    /// picture caption are charged their XML like plain text, so 30 more
    /// references are cut at the budget.
    /// 이것을 실패시키는 것: Core 의 일반 글자만 세기(탭 섞인 글자·캡션은 0 에 가깝게 과금).
    #[test]
    fn every_form_of_master_page_content_is_charged() {
        use hwpforge_core::caption::{Caption, CaptionSide};
        use hwpforge_core::image::{Image, ImageFormat};
        use hwpforge_core::run::Run;
        use hwpforge_core::section::MasterPage;
        use hwpforge_core::{PageSettings, Paragraph};
        use hwpforge_foundation::{ApplyPageType, CharShapeIndex, HwpUnit, ParaShapeIndex};
        let big = "가".repeat(1 << 20);
        let para = |runs| Paragraph::with_runs(runs, ParaShapeIndex::new(0));
        let text = |t: &str| Run::text(t, CharShapeIndex::new(0));
        // A tab with a width is kept as tab-mixed text (a bare tab would
        // decode as plain text).
        let tabbed = Run {
            content: hwpforge_core::run::RunContent::InlineText(
                hwpforge_core::inline::InlineText::from_segments([
                    hwpforge_core::inline::InlineSegment::Plain(big.clone()),
                    hwpforge_core::inline::InlineSegment::Tab(
                        hwpforge_core::inline::InlineTabAttr {
                            width: HwpUnit::new(1).unwrap(),
                            leader: 0,
                            tab_type: 0,
                        },
                    ),
                    hwpforge_core::inline::InlineSegment::Plain("끝".into()),
                ]),
            ),
            char_shape_id: CharShapeIndex::new(0),
        };
        let w = HwpUnit::new(1000).unwrap();
        let captioned = Run::image(
            Image::new("BinData/image1.png", w, w, ImageFormat::Png)
                .with_caption(Caption::new(vec![para(vec![text(&big)])], CaptionSide::Bottom)),
            CharShapeIndex::new(0),
        );
        for run in [tabbed, captioned] {
            let mut doc = Document::new();
            for page in [Some(run.clone()), None] {
                let mut section =
                    Section::with_paragraphs(vec![para(vec![text("본문")])], PageSettings::a4());
                section.master_pages =
                    page.map(|r| vec![MasterPage::new(ApplyPageType::Both, vec![para(vec![r])])]);
                doc.add_section(section);
            }
            let store = crate::style_store_for_preset("default").unwrap();
            let mut images = hwpforge_core::image::ImageStore::new();
            images.insert("image1.png", vec![0x89, b'P', b'N', b'G']);
            let bytes =
                crate::HwpxEncoder::encode(&doc.validate().unwrap(), &store, &images).unwrap();
            let refs = r#"<hp:masterPage idRef="masterpage0"/>"#.repeat(30);
            let bytes = with_section1_and_manifest(
                &bytes,
                |xml| {
                    let at = xml.find("</hp:secPr>").unwrap();
                    format!("{}{refs}{}", &xml[..at], &xml[at..])
                },
                str::to_string,
            );
            let len = part_len(&bytes, 0);
            assert!(len > 3 << 20, "{len}");
            let decoded = HwpxDecoder::decode(&bytes).unwrap();
            // The content decodes in the form it was written, whole.
            let first = &decoded.document.sections()[0].master_pages.as_ref().unwrap()[0];
            match (&run.content, &first.paragraphs[0].runs[0].content) {
                (
                    hwpforge_core::run::RunContent::InlineText(_),
                    hwpforge_core::run::RunContent::InlineText(got),
                ) => assert!(got.plain_text().starts_with(&big)),
                (
                    hwpforge_core::run::RunContent::Image(_),
                    hwpforge_core::run::RunContent::Image(got),
                ) => {
                    let caption = got.caption.as_ref().unwrap();
                    assert_eq!(caption.paragraphs[0].runs[0].content.as_text(), Some(big.as_str()));
                }
                (want, got) => panic!("{want:?} decoded as {got:?}"),
            }
            let kept = decoded.document.sections()[1].master_pages.as_ref().map_or(0, Vec::len);
            assert_eq!(kept, (MASTER_PAGE_BUDGET - len) / len, "{len}");
            let lost = lost_messages(&decoded);
            assert_eq!(lost.len(), 30 - kept, "{lost:?}");
            assert!(lost.iter().all(|m| m.contains("does not fit the master page budget")));
        }
    }

    /// A part its section names but the budget refuses
    /// is reported once, as over the budget — not also as named by no
    /// section.
    /// 이것을 실패시키는 것: 무참조를 보관 맵으로 판정하기.
    #[test]
    fn a_refused_master_page_is_not_reported_unreferenced() {
        let big = "가".repeat(1 << 20);
        let bytes = master_pages_doc(&[Some(&big), Some(&big)]);
        let own = r#"<hp:masterPage idRef="masterpage0"/>"#;
        let mut pkg = crate::patch::RawPackage::read(&bytes).unwrap();
        let xml = pkg.read_text_entry("Contents/section0.xml").unwrap();
        // Section 0 fills the budget with its own part; section 1's part,
        // named once, does not fit.
        let copies = MASTER_PAGE_BUDGET / part_len(&bytes, 0);
        pkg.replace_text_entry("Contents/section0.xml", xml.replace(own, &own.repeat(copies)));
        let decoded = HwpxDecoder::decode(&pkg.write().unwrap()).unwrap();
        assert!(decoded.document.sections()[1].master_pages.is_none());
        let lost = lost_messages(&decoded);
        assert_eq!(lost.len(), 1, "{lost:?}");
        assert!(lost[0].contains("does not fit the master page budget"), "{lost:?}");
    }

    /// A part named by three or five sections,
    /// once each, is kept by all — the schema gives a part no owner — and
    /// whether the package also holds a part nobody names changes nothing.
    /// 이것을 실패시키는 것: 공유를 손실로 보기 · 예산을 part 크기 합으로 잡기.
    #[test]
    fn shared_master_pages_are_kept() {
        for sections in [3, 5] {
            let refs = vec!["masterpage0"; sections];
            let mut names = vec![None; sections];
            names[0] = Some("A");
            let only = with_master_page_refs(&master_pages_doc(&names), &refs);
            names[1] = Some("unused");
            let with_unused = with_master_page_refs(&master_pages_doc(&names), &refs);
            for (bytes, orphans) in [(only, 0), (with_unused, 1)] {
                let decoded = HwpxDecoder::decode(&bytes).unwrap();
                assert_eq!(master_page_texts(&decoded), vec!["A"; sections]);
                assert_eq!(lost_messages(&decoded).len(), orphans, "{sections}");
            }
        }
        // One section naming its own part twice keeps two copies.
        let own = r#"<hp:masterPage idRef="masterpage0"/>"#;
        let mut pkg = crate::patch::RawPackage::read(&two_master_pages()).unwrap();
        let xml = pkg.read_text_entry("Contents/section0.xml").unwrap();
        pkg.replace_text_entry("Contents/section0.xml", xml.replace(own, &own.repeat(2)));
        let decoded = HwpxDecoder::decode(&pkg.write().unwrap()).unwrap();
        assert_eq!(master_page_texts(&decoded), ["AA", "B"]);
    }

    /// The budget refuses a charge that does not fit without spending it,
    /// so a smaller one after it still fits.
    /// 이것을 실패시키는 것: 거절한 비용도 누적하기.
    #[test]
    fn copy_budget_charges_admitted_copies_only() {
        let mut budget = CopyBudget::new(100);
        assert!(budget.admit(60));
        assert!(!budget.admit(50));
        assert!(budget.admit(30));
        assert_eq!(budget.spent, 90);
        assert!(!budget.admit(11));
        assert!(budget.admit(10));
    }

    /// A reference that names no master page part, and the part no section
    /// references, are lost content (editors refuse the input).
    /// 이것을 실패시키는 것: 미해결 참조 무시 · 참조 없는 바탕쪽 무시.
    #[test]
    fn fix6_master_page_count_uses_deserialized_value() {
        // 이것을 실패시키는 것: 원시 masterPageCnt 문자열 검색 복원, count 전달/경고 생략.
        let base = master_pages_doc(&[None]);
        for attribute in [
            r#"masterPageCnt="1""#,
            "masterPageCnt='1'",
            r#"masterPageCnt="&#49;""#,
            r#"masterPageCnt = "1""#,
            r#"masterPageCnt="0""#,
        ] {
            let mut pkg = crate::patch::RawPackage::read(&base).unwrap();
            let xml = pkg.read_text_entry("Contents/section0.xml").unwrap();
            assert!(xml.contains(r#"masterPageCnt="0""#));
            pkg.replace_text_entry(
                "Contents/section0.xml",
                xml.replace(r#"masterPageCnt="0""#, attribute),
            );
            let decoded = HwpxDecoder::decode(&pkg.write().unwrap()).unwrap();
            let losses: Vec<_> = decoded
                .warnings
                .iter()
                .filter_map(|warning| match warning {
                    DecodeWarning::ContentLost { part, kind, reason, .. }
                        if kind == "master_page" =>
                    {
                        Some((part, reason))
                    }
                    _ => None,
                })
                .collect();
            if attribute == r#"masterPageCnt="0""# {
                assert!(losses.is_empty());
            } else {
                assert_eq!(losses.len(), 1, "{attribute}: {losses:?}");
                assert_eq!(losses[0].0, "Contents/section0.xml");
                assert_eq!(losses[0].1, "masterPageCnt 1 without a masterPage reference");
            }
        }
    }

    #[test]
    fn an_unresolved_master_page_reference_is_lost_content() {
        let bytes = with_master_page_refs(&two_master_pages(), &["masterpage0", "nosuch"]);
        let decoded = HwpxDecoder::decode(&bytes).unwrap();
        assert_eq!(master_page_texts(&decoded), ["A", ""]);
        let lost: Vec<String> = decoded
            .warnings
            .iter()
            .filter(|w| w.is_content_loss())
            .map(|w| format!("{} {}", w.location().unwrap_or_default(), w.message()))
            .collect();
        assert_eq!(lost.len(), 2, "{lost:?}");
        assert!(lost[0].contains("section1") && lost[0].contains("nosuch"), "{lost:?}");
        assert!(lost[1].contains("masterpage1"), "{lost:?}");

        // A count without any reference is lost too.
        let mut pkg = crate::patch::RawPackage::read(&two_master_pages()).unwrap();
        let xml = pkg.read_text_entry("Contents/section1.xml").unwrap();
        pkg.replace_text_entry(
            "Contents/section1.xml",
            xml.replace(r#"<hp:masterPage idRef="masterpage1"/>"#, ""),
        );
        let decoded = HwpxDecoder::decode(&pkg.write().unwrap()).unwrap();
        let lost: Vec<String> =
            decoded.warnings.iter().filter(|w| w.is_content_loss()).map(|w| w.message()).collect();
        assert!(lost.iter().any(|m| m.contains("without a masterPage reference")), "{lost:?}");
    }

    #[test]
    fn fix6_later_section_master_page_count_uses_deserialized_value() {
        // 이것을 실패시키는 것: masterPageCnt 원시 문자열 검색 복원,
        // 구역 결과의 count 전달 누락, 0에도 누락 경고 생성.
        for attribute in [
            r#"masterPageCnt="1""#,
            "masterPageCnt='1'",
            r#"masterPageCnt="&#49;""#,
            r#"masterPageCnt = "1""#,
            r#"masterPageCnt="0""#,
        ] {
            let mut pkg = crate::patch::RawPackage::read(&two_master_pages()).unwrap();
            let part = "Contents/section1.xml";
            let xml = pkg.read_text_entry(part).unwrap();
            assert!(xml.contains(r#"masterPageCnt="1""#));
            pkg.replace_text_entry(
                part,
                xml.replace(r#"<hp:masterPage idRef="masterpage1"/>"#, "")
                    .replace(r#"masterPageCnt="1""#, attribute),
            );
            let decoded = HwpxDecoder::decode(&pkg.write().unwrap()).unwrap();
            let count = decoded
                .warnings
                .iter()
                .filter(|w| {
                    w.is_content_loss() && w.message().contains("without a masterPage reference")
                })
                .count();
            assert_eq!(count, usize::from(!attribute.contains('0')), "{attribute}");
        }
    }

    /// A package whose picture-caption table names a border
    /// fill the header lacks decodes with that zone dropped and reported.
    /// The package is encoded with border fill 7, then its header renames 7.
    /// 이것을 실패시키는 것: 디코드 순회가 그림 캡션에 들어가지 않기.
    #[test]
    fn picture_caption_table_zone_with_an_unknown_fill_is_lost() {
        use crate::style_store::{HwpxBorderFill, HwpxCharShape, HwpxParaShape};
        use hwpforge_core::caption::{Caption, CaptionSide};
        use hwpforge_core::image::{Image, ImageFormat};
        use hwpforge_core::run::{Run, RunContent};
        use hwpforge_core::table::{CellZone, Table, TableCell, TableRow};
        use hwpforge_core::Paragraph;
        use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
        let mut store = HwpxStyleStore::with_default_fonts("함초롬바탕");
        store.push_char_shape(HwpxCharShape::default());
        store.push_para_shape(HwpxParaShape::default());
        for id in 1..=7 {
            let mut bf = HwpxBorderFill::default_page_border();
            bf.id = id;
            store.push_border_fill(bf);
        }
        let text = || {
            Paragraph::with_runs(
                vec![Run::text("x", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )
        };
        let table = Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![text()],
            HwpUnit::new(1000).unwrap(),
        )])])
        .with_cell_zones(vec![CellZone::new(0, 0, 0, 0, 7)]);
        let unit = HwpUnit::new(1000).unwrap();
        let mut image = Image::new("BinData/a.png", unit, unit, ImageFormat::Png);
        image.caption = Some(Caption::new(
            vec![Paragraph::with_runs(
                vec![Run::table(table, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            CaptionSide::Bottom,
        ));
        let mut doc = Document::<Draft>::new();
        doc.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::image(image, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        ));
        let mut images = ImageStore::new();
        images.insert("a.png", vec![0x89, b'P', b'N', b'G']);
        let bytes = crate::HwpxEncoder::encode(&doc.validate().unwrap(), &store, &images).unwrap();
        let caption_zones = |bytes: &[u8]| {
            let decoded = crate::HwpxDecoder::decode(bytes).unwrap();
            let RunContent::Image(img) =
                &decoded.document.sections()[0].paragraphs[0].runs[0].content
            else {
                panic!()
            };
            let RunContent::Table(t) = &img.caption.as_ref().unwrap().paragraphs[0].runs[0].content
            else {
                panic!()
            };
            (t.cell_zones.len(), decoded.warnings.iter().filter(|w| w.is_content_loss()).count())
        };
        assert_eq!(caption_zones(&bytes), (1, 0));
        // Rename border fill 7 in the header.
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes.clone())).unwrap();
        let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).unwrap();
            let name = entry.name().expect("ZIP entry name").to_string();
            let mut data = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut data).unwrap();
            if name == "Contents/header.xml" {
                data = String::from_utf8(data)
                    .unwrap()
                    .replace(r#"borderFill id="7""#, r#"borderFill id="70""#)
                    .into_bytes();
            }
            out.start_file(name, SimpleFileOptions::default()).unwrap();
            out.write_all(&data).unwrap();
        }
        let renamed = out.finish().unwrap().into_inner();
        // 한컴 실측: id label changes do not change position 7.
        assert_eq!(caption_zones(&renamed), (1, 0));
        let mut package = crate::patch::RawPackage::read(&bytes).unwrap();
        let section = package
            .read_text_entry("Contents/section0.xml")
            .unwrap()
            .replace("borderFillIDRef=\"7\"", "borderFillIDRef=\"99\"");
        package.replace_text_entry("Contents/section0.xml", section);
        assert_eq!(caption_zones(&package.write().unwrap()), (0, 1));
    }

    /// A cell zone naming a border fill the header does
    /// not have (by id — the store holds 1 and 5, the zone names 3) is
    /// dropped and reported; known ids stay, in nested tables too.
    /// 이것을 실패시키는 것: 검사 삭제 · 개수 비교로 바꾸기(3 ≤ 2 개라 통과) ·
    /// 중첩 표 미방문.
    #[test]
    fn cell_zone_with_an_unknown_border_fill_is_lost() {
        use crate::style_store::HwpxBorderFill;
        use hwpforge_core::run::{Run, RunContent};
        use hwpforge_core::table::{CellZone, Table, TableCell, TableRow};
        use hwpforge_core::Paragraph;
        use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
        let mut store = HwpxStyleStore::new();
        for id in [1, 5] {
            let mut bf = HwpxBorderFill::default_page_border();
            bf.id = id;
            store.push_border_fill(bf);
        }
        let zones = vec![CellZone::new(0, 0, 0, 0, 1), CellZone::new(0, 0, 0, 0, 3)];
        let text = || {
            Paragraph::with_runs(
                vec![Run::text("x", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )
        };
        let cell = |p| TableCell::new(vec![p], HwpUnit::new(1000).unwrap());
        let inner =
            Table::new(vec![TableRow::new(vec![cell(text())])]).with_cell_zones(zones.clone());
        let outer = Table::new(vec![TableRow::new(vec![cell(Paragraph::with_runs(
            vec![Run::table(inner, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        ))])])
        .with_cell_zones(zones);
        let mut document = Document::<Draft>::new();
        document.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::table(outer, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        ));
        let mut warnings = Vec::new();
        drop_cell_zones_with_unknown_border_fill(
            &mut document,
            &store,
            &mut warnings,
            &["Contents/section0.xml".into()],
        );
        let RunContent::Table(outer) = &document.sections()[0].paragraphs[0].runs[0].content else {
            panic!()
        };
        assert_eq!(outer.cell_zones, [CellZone::new(0, 0, 0, 0, 1)]);
        let RunContent::Table(inner) = &outer.rows[0].cells[0].paragraphs[0].runs[0].content else {
            panic!()
        };
        assert_eq!(inner.cell_zones, [CellZone::new(0, 0, 0, 0, 1)]);
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(warnings.iter().all(|w| w.is_content_loss()));
    }
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    /// Creates a complete minimal HWPX for testing.
    fn make_test_hwpx(header_xml: &str, section_xmls: &[&str]) -> Vec<u8> {
        let buf = Vec::new();
        let mut zip = ZipWriter::new(Cursor::new(buf));

        let stored =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let deflate = SimpleFileOptions::default();

        zip.start_file("mimetype", stored).unwrap();
        zip.write_all(b"application/hwp+zip").unwrap();

        zip.start_file("Contents/header.xml", deflate).unwrap();
        zip.write_all(header_xml.as_bytes()).unwrap();

        for (i, xml) in section_xmls.iter().enumerate() {
            let path = format!("Contents/section{}.xml", i);
            zip.start_file(&path, deflate).unwrap();
            zip.write_all(xml.as_bytes()).unwrap();
        }

        zip.finish().unwrap().into_inner()
    }

    const HEADER: &str = r##"<head version="1.4" secCnt="1">
        <refList>
            <fontfaces itemCnt="1">
                <fontface lang="HANGUL" fontCnt="1">
                    <font id="0" face="함초롬돋움" type="TTF" isEmbedded="0"/>
                </fontface>
            <fontface lang="LATIN" fontCnt="1"><font id="0" face="함초롬돋움" type="TTF"/></fontface><fontface lang="HANJA" fontCnt="1"><font id="0" face="함초롬돋움" type="TTF"/></fontface><fontface lang="JAPANESE" fontCnt="1"><font id="0" face="함초롬돋움" type="TTF"/></fontface><fontface lang="OTHER" fontCnt="1"><font id="0" face="함초롬돋움" type="TTF"/></fontface><fontface lang="SYMBOL" fontCnt="1"><font id="0" face="함초롬돋움" type="TTF"/></fontface><fontface lang="USER" fontCnt="1"><font id="0" face="함초롬돋움" type="TTF"/></fontface></fontfaces>
            <charProperties itemCnt="1">
                <charPr id="0" height="1000" textColor="#000000" shadeColor="none"
                        useFontSpace="0" useKerning="0" symMark="NONE" borderFillIDRef="0">
                    <fontRef hangul="0" latin="0" hanja="0" japanese="0" other="0" symbol="0" user="0"/>
                </charPr>
            </charProperties>
            <paraProperties itemCnt="1">
                <paraPr id="0">
                    <align horizontal="LEFT" vertical="BASELINE"/>
                    <switch><default>
                        <lineSpacing type="PERCENT" value="160" unit="HWPUNIT"/>
                    </default></switch>
                </paraPr>
            </paraProperties>
        <styles><style id="0" type="PARA" name="base" paraPrIDRef="0" charPrIDRef="0" nextStyleIDRef="0"/></styles></refList>
    </head>"##;

    const SECTION_TEXT: &str = r#"<sec>
        <p paraPrIDRef="0">
            <run charPrIDRef="0">
                <secPr textDirection="HORIZONTAL">
                    <pagePr landscape="WIDELY" width="59528" height="84188">
                        <margin header="4252" footer="4252" gutter="0"
                                left="8504" right="8504" top="5668" bottom="4252"/>
                    </pagePr>
                </secPr>
                <t>안녕하세요</t>
            </run>
        </p>
    </sec>"#;

    // ── Full pipeline tests ──────────────────────────────────────

    #[test]
    fn fix5_memo_loss_is_active_in_body_and_image_caption() {
        // 이것을 실패시키는 것: Core for_each_paragraph로 메모를 판정해 그림 캡션 누락,
        // 디코딩 관측/구역 집계 누락, 뒤의 메모 없는 구역으로 존재 사실 덮어쓰기.
        let header = HEADER.replace(
            "</refList>",
            r#"<memoProperties><memoPr id="7"/></memoProperties></refList>"#,
        );
        for caption in [false, true] {
            for active in [false, true] {
                let field = if active {
                    r#"<ctrl><fieldBegin id="1" type="MEMO"><parameters><stringParam name="MemoShapeIDRef">65535</stringParam></parameters></fieldBegin></ctrl><t>x</t><ctrl><fieldEnd beginIDRef="1"/></ctrl>"#
                } else {
                    "<t>x</t>"
                };
                let payload = if caption {
                    format!(
                        r#"<pic><sz width="1000" height="1000"/><img binaryItemIDRef="a"/><caption><subList><p><run>{field}</run></p></subList></caption></pic>"#
                    )
                } else {
                    field.into()
                };
                let section =
                    format!(r#"<sec><p><run><secPr memoShapeIDRef="7"/>{payload}</run></p></sec>"#);
                let decoded =
                    HwpxDecoder::decode(&make_test_hwpx(&header, &[&section, SECTION_TEXT]))
                        .unwrap();
                if caption {
                    assert!(decoded.document.sections()[0].paragraphs[0]
                        .runs
                        .iter()
                        .any(|r| matches!(&r.content,
                        hwpforge_core::RunContent::Image(image) if image.caption.is_some())));
                }
                for expected in ["memoPr", "secPr.memoShapeIDRef"] {
                    let warning = decoded
                        .warnings
                        .iter()
                        .find(|w| match w {
                            DecodeWarning::ContentLost { kind, .. }
                            | DecodeWarning::InactiveHeaderReference { kind, .. } => {
                                kind == expected
                            }
                            _ => false,
                        })
                        .expect(expected);
                    assert_eq!(warning.is_content_loss(), active, "caption={caption}: {warning:?}");
                }
            }
        }
    }

    #[test]
    fn fix5_master_page_memo_observation_reaches_document() {
        // 이것을 실패시키는 것: 바탕쪽의 메모 관측을 반환/문서 집계에서 누락.
        let mut package = crate::patch::RawPackage::read(&two_master_pages()).unwrap();
        let header = package.read_text_entry("Contents/header.xml").unwrap().replace(
            "</hh:refList>",
            r#"<hh:memoProperties><hh:memoPr id="7"/></hh:memoProperties></hh:refList>"#,
        );
        package.replace_text_entry("Contents/header.xml", header);
        let xml = package.read_text_entry("Contents/masterpage0.xml").unwrap()
            .replace("<hp:t>A</hp:t>", r#"<hp:ctrl><hp:fieldBegin id="1" type="MEMO"/></hp:ctrl><hp:t>A</hp:t><hp:ctrl><hp:fieldEnd beginIDRef="1"/></hp:ctrl>"#);
        package.replace_text_entry("Contents/masterpage0.xml", xml);
        let decoded = HwpxDecoder::decode(&package.write().unwrap()).unwrap();
        assert!(decoded.warnings.iter().any(|w| matches!(w,
            DecodeWarning::ContentLost { kind, .. } if kind == "memoPr")));
    }

    #[test]
    fn fix5_memo_parameter_entities_share_deserialized_validation() {
        // 이것을 실패시키는 것: name 원시 바이트 비교로 &#82;/&#x52; 우회,
        // 파라미터 조회/손실 경고 생략, 65535 상속 오거부, 속성 오류 무시.
        for name in ["MemoShapeIDRef", "MemoShapeID&#82;ef", "MemoShapeID&#x52;ef"] {
            for (value, expected) in [
                ("99", "broken memoPr reference ID 99"),
                ("&#49;", "validated memoPr 1"),
                ("65535", ""),
            ] {
                let header = HEADER.replace(
                    "</refList>",
                    r#"<memoProperties><memoPr id="7"/></memoProperties></refList>"#,
                );
                let section = format!(
                    r#"<sec><p><run><ctrl><fieldBegin id="1" type="MEMO"><parameters><stringParam name="Author">tester</stringParam><stringParam name="{name}">{value}</stringParam></parameters></fieldBegin></ctrl><t>x</t><ctrl><fieldEnd beginIDRef="1"/></ctrl></run></p></sec>"#
                );
                let decoded = HwpxDecoder::decode(&make_test_hwpx(&header, &[&section])).unwrap();
                let reasons: Vec<_> = decoded
                    .warnings
                    .iter()
                    .filter_map(|w| match w {
                        DecodeWarning::ContentLost { kind, reason, .. }
                            if kind == "MemoShapeIDRef" =>
                        {
                            Some(reason)
                        }
                        _ => None,
                    })
                    .collect();
                if expected.is_empty() {
                    assert!(reasons.is_empty());
                } else {
                    assert_eq!(reasons.len(), 1);
                    assert!(reasons[0].contains(expected), "{reasons:?}");
                }
            }
        }
        let invalid = r#"<sec><p><run><ctrl><fieldBegin type="MEMO"><parameters><stringParam name="MemoShapeIDRef">invalid</stringParam></parameters></fieldBegin></ctrl></run></p></sec>"#;
        assert!(HwpxDecoder::decode(&make_test_hwpx(HEADER, &[invalid]))
            .unwrap_err()
            .to_string()
            .contains("MemoShapeIDRef invalid ID"));
        for attributes in [
            r#"name="MemoShapeIDRef" name="ignored""#,
            r#"name="MemoShapeID&unknown;Ref""#,
            r#"name"#,
        ] {
            let section = format!(
                r#"<sec><p><run><ctrl><fieldBegin type="MEMO"><parameters><stringParam {attributes}>99</stringParam></parameters></fieldBegin></ctrl></run></p></sec>"#
            );
            assert!(
                HwpxDecoder::decode(&make_test_hwpx(HEADER, &[&section])).is_err(),
                "{attributes}"
            );
        }
    }

    #[test]
    fn fix4_original_border_ids_and_dutmal_are_checked_in_packages() {
        // 이것을 실패시키는 것: 문자 2→None, 표/셀/쪽 검증 누락,
        // 덧말 styleIDRef를 조회 전에 0으로 폐기, 첫 위치 1의 오거부(한컴 실측).
        for id in [2, 4, 99] {
            let header =
                HEADER.replace("borderFillIDRef=\"0\"", &format!("borderFillIDRef=\"{id}\""));
            for definitions in [String::new(), "<borderFills itemCnt=\"1\"><borderFill id=\"7\"><slash/><backSlash/><leftBorder/><rightBorder/><topBorder/><bottomBorder/></borderFill></borderFills>".into()] {
                let header = header.replace("</refList>", &format!("{definitions}</refList>"));
                let error = HwpxDecoder::decode(&make_test_hwpx(&header, &[SECTION_TEXT])).unwrap_err();
                assert!(error.to_string().contains("borderFill"), "{error}");
            }
        }
        let header = HEADER.replace("</refList>", "<borderFills itemCnt=\"1\"><borderFill id=\"7\"><slash/><backSlash/><leftBorder/><rightBorder/><topBorder/><bottomBorder/></borderFill></borderFills></refList>")
            .replace("borderFillIDRef=\"0\"", "borderFillIDRef=\"1\"");
        let decoded = HwpxDecoder::decode(&make_test_hwpx(&header, &[SECTION_TEXT])).unwrap();
        assert_eq!(decoded.style_store.iter_char_shapes().next().unwrap().border_fill_id, Some(1));
        let bytes = crate::HwpxEncoder::encode(
            &decoded.document.validate().unwrap(),
            &decoded.style_store,
            &decoded.image_store,
        )
        .unwrap();
        assert_eq!(
            HwpxDecoder::decode(&bytes)
                .unwrap()
                .style_store
                .iter_char_shapes()
                .next()
                .unwrap()
                .border_fill_id,
            Some(1)
        );
        for (kind, payload) in [
            (
                "table",
                r#"<tbl borderFillIDRef="99" rowCnt="1" colCnt="1"><tr><tc><cellAddr colAddr="0" rowAddr="0"/><cellSpan colSpan="1" rowSpan="1"/><cellSz width="1000" height="1000"/><subList><p><run><t>x</t></run></p></subList></tc></tr></tbl>"#,
            ),
            (
                "cell",
                r#"<tbl rowCnt="1" colCnt="1"><tr><tc borderFillIDRef="99"><cellAddr colAddr="0" rowAddr="0"/><cellSpan colSpan="1" rowSpan="1"/><cellSz width="1000" height="1000"/><subList><p><run><t>x</t></run></p></subList></tc></tr></tbl>"#,
            ),
            ("page", r#"<secPr><pageBorderFill type="BOTH" borderFillIDRef="99"/></secPr>"#),
            (
                "style",
                r#"<dutmal styleIDRef="99"><mainText>x</mainText><subText>y</subText></dutmal>"#,
            ),
        ] {
            let section = format!("<sec><p><run>{payload}</run></p></sec>");
            let error = HwpxDecoder::decode(&make_test_hwpx(HEADER, &[&section])).unwrap_err();
            assert!(error.to_string().contains(kind), "{kind}: {error}");
        }
    }

    #[test]
    fn fix4_unsupported_definitions_and_dutmal_loss_are_explicit() {
        // 이것을 실패시키는 것: 미지원 정의/덧말 스타일의 무음 삭제.
        for kind in ["memoPr", "trackChange", "trackChangeAuthor"] {
            let header = HEADER.replace("</refList>", &format!("<{kind} id=\"7\"/></refList>"));
            let doc = HwpxDecoder::decode(&make_test_hwpx(&header, &[SECTION_TEXT])).unwrap();
            assert!(doc
                .warnings
                .iter()
                .any(|w| matches!(w, DecodeWarning::ContentLost { kind: k, .. } | DecodeWarning::InactiveHeaderReference { kind: k, .. } if k == kind)));
        }
        let header = HEADER.replace("</styles>", r#"<style id="7" type="PARA" name="ruby" paraPrIDRef="0" charPrIDRef="0" nextStyleIDRef="0"/></styles>"#);
        let section = r#"<sec><p><run><dutmal styleIDRef="1"><mainText>x</mainText><subText>y</subText></dutmal></run></p></sec>"#;
        let doc = HwpxDecoder::decode(&make_test_hwpx(&header, &[section])).unwrap();
        assert!(doc
            .warnings
            .iter()
            .any(|w| matches!(w, DecodeWarning::ContentLost { kind, reason, .. }
            if kind == "dutmal.styleIDRef" && reason.contains("1"))));
    }

    #[test]
    fn fix4_schema_omitted_references_are_resolved_before_loss() {
        // 이것을 실패시키는 것: 스키마가 무시하는 secPr/inline/form/메모 참조 조회 및 경고 생략.
        let validate = |refs: &crate::header_references::HeaderReferences, xml: &str| {
            if xml.starts_with("<stringParam") {
                let parameters = xml_from_str(&format!("<parameters>{xml}</parameters>")).unwrap();
                refs.validate_memo_parameters(&parameters, "section")
            } else {
                refs.validate_auxiliary_xml(xml, "section")
            }
        };
        let refs = crate::header_references::HeaderReferences::build(
            &super::header::parse_header(HEADER).unwrap().style_store,
        )
        .unwrap();
        for (xml, kind, missing) in [
            (r#"<secPr outlineShapeIDRef="99"/>"#, "outlineShapeIDRef", true),
            (r#"<secPr memoShapeIDRef="99"/>"#, "memoShapeIDRef", true),
            (r#"<t charStyleIDRef="0">x</t>"#, "charStyleIDRef", false),
            (r#"<formCharPr charPrIDRef="0"/>"#, "charPrIDRef", false),
            (r#"<stringParam name="MemoShapeIDRef">99</stringParam>"#, "MemoShapeIDRef", true),
        ] {
            let warnings = validate(&refs, xml).unwrap();
            assert!(warnings
                .iter()
                .any(|w| matches!(w, DecodeWarning::ContentLost { kind: k, reason, .. }
                if k.contains(kind) && (!missing || reason.contains("broken")))));
        }
        for xml in [r#"<t charStyleIDRef="99">x</t>"#, r#"<formCharPr charPrIDRef="99"/>"#] {
            assert!(refs
                .validate_auxiliary_xml(xml, "section")
                .unwrap_err()
                .to_string()
                .contains("99"));
        }
        let header = HEADER.replace(
            "</refList>",
            r#"<memoProperties><memoPr id="7"/></memoProperties></refList>"#,
        );
        let parsed = super::header::parse_header(&header).unwrap();
        assert_eq!(parsed.memo_count, 1);
        let mut refs =
            crate::header_references::HeaderReferences::build(&parsed.style_store).unwrap();
        refs.memos = parsed.memo_count;
        for xml in [
            r#"<secPr memoShapeIDRef="1"/>"#,
            r#"<stringParam name="MemoShapeIDRef">1</stringParam>"#,
        ] {
            let warnings = validate(&refs, xml).unwrap();
            assert!(warnings.iter().any(|w| matches!(w, DecodeWarning::ContentLost { reason, .. } | DecodeWarning::InactiveHeaderReference { reason, .. } if reason.contains("validated memoPr 1"))));
        }
        for xml in [
            r#"<secPr outlineShapeIDRef="0" memoShapeIDRef="0"/>"#,
            r#"<stringParam name="MemoShapeIDRef">65535</stringParam>"#,
        ] {
            assert!(validate(&refs, xml).unwrap().is_empty());
        }
        let section = r#"<sec><p><run><secPr memoShapeIDRef="1"/><t>x</t></run></p></sec>"#;
        let decoded = HwpxDecoder::decode(&make_test_hwpx(&header, &[section])).unwrap();
        assert!(decoded.warnings.iter().any(|w| matches!(w, DecodeWarning::ContentLost { reason, .. } | DecodeWarning::InactiveHeaderReference { reason, .. } if reason.contains("validated memoPr 1"))));
        for payload in [r#"<t charStyleIDRef="99">x</t>"#, r#"<formCharPr charPrIDRef="99"/>"#] {
            let section = format!("<sec><p><run>{payload}</run></p></sec>");
            assert!(HwpxDecoder::decode(&make_test_hwpx(HEADER, &[&section])).is_err());
        }
        let mut store = parsed.style_store;
        store.push_numbering(hwpforge_core::NumberingDef::default_outline());
        let refs = crate::header_references::HeaderReferences::build(&store).unwrap();
        assert!(refs
            .validate_auxiliary_xml(r#"<secPr outlineShapeIDRef="1"/>"#, "section")
            .unwrap()
            .is_empty());
        store.push_para_shape(crate::HwpxParaShape {
            heading_type: hwpforge_foundation::HeadingType::Outline,
            heading_id_ref: 99,
            ..Default::default()
        });
        let refs = crate::header_references::HeaderReferences::build(&store).unwrap();
        assert!(refs.validate(&store, false).unwrap_err().to_string().contains("numbering"));
    }

    #[test]
    fn fix4_change_tracking_references_are_checked_before_loss() {
        // 이것을 실패시키는 것: IDRef가 아닌 charTcId/paraTcId/authorID/
        // charshapeID/parashapeID 조회를 생략하거나 header 교차 조회를 빼기.
        for (owner, field, valid) in [
            ("trackChange", "authorID", 1),
            ("trackChange", "charshapeID", 0),
            ("trackChange", "parashapeID", 0),
            ("run", "charTcId", 1),
            ("p", "paraTcId", 1),
        ] {
            for id in [valid, 99] {
                let attr = format!(r#"{field}="{id}""#);
                let change_attr = if owner == "trackChange" { attr.as_str() } else { "" };
                let header = HEADER.replace("</refList>", &format!(r#"<trackChanges><trackChange id="7" {change_attr}/></trackChanges><trackChangeAuthors><trackChangeAuthor id="8"/></trackChangeAuthors></refList>"#));
                let run_attr = if owner == "run" { attr.as_str() } else { "" };
                let para_attr = if owner == "p" { attr.as_str() } else { "" };
                let section =
                    format!(r#"<sec><p {para_attr}><run {run_attr}><t>x</t></run></p></sec>"#);
                let decoded = HwpxDecoder::decode(&make_test_hwpx(&header, &[&section])).unwrap();
                let reason = decoded
                    .warnings
                    .iter()
                    .find_map(|w| match w {
                        DecodeWarning::ContentLost { kind, reason, .. }
                            if *kind == format!("{owner}.{field}") =>
                        {
                            Some(reason)
                        }
                        _ => None,
                    })
                    .expect("explicit change tracking reference loss");
                assert_eq!(
                    reason.contains("validated"),
                    id == valid,
                    "{owner}.{field}={id}: {reason}"
                );
                assert!(reason.contains(&id.to_string()));
            }
        }
        let section = r#"<sec><p paraTcId="0"><run charTcId="0"><t>x</t></run></p></sec>"#;
        let decoded = HwpxDecoder::decode(&make_test_hwpx(HEADER, &[section])).unwrap();
        assert!(!decoded.warnings.iter().any(
            |w| matches!(w, DecodeWarning::ContentLost { kind, .. } if kind.ends_with("TcId"))
        ));
    }

    #[test]
    fn fix4_unused_memo_definitions_do_not_block_content_edits() {
        // 이것을 실패시키는 것: 미사용 기본 정의를 content loss로 취급하거나,
        // 실제 메모가 있는 문서의 손실까지 비활성 경고로 낮추기.
        let header = HEADER.replace(
            "</refList>",
            r#"<memoProperties><memoPr id="7"/></memoProperties></refList>"#,
        );
        for (body, active) in [
            ("<t>x</t>", false),
            (
                r#"<ctrl><fieldBegin id="1" type="MEMO"/></ctrl><t>x</t><ctrl><fieldEnd beginIDRef="1"/></ctrl>"#,
                true,
            ),
        ] {
            let section =
                format!(r#"<sec><p><run><secPr memoShapeIDRef="7"/>{body}</run></p></sec>"#);
            let decoded = HwpxDecoder::decode(&make_test_hwpx(&header, &[&section])).unwrap();
            let warning = decoded
                .warnings
                .iter()
                .find(|w| match w {
                    DecodeWarning::ContentLost { kind, .. }
                    | DecodeWarning::InactiveHeaderReference { kind, .. } => kind == "memoPr",
                    _ => false,
                })
                .unwrap();
            assert_eq!(warning.is_content_loss(), active);
            assert_eq!(warning.part(), Some("Contents/header.xml"));
            assert!(warning.location().unwrap().contains("Contents/header.xml"));
            assert!(!warning.message().is_empty());
            assert!(!warning.to_string().is_empty());
            if !active {
                assert_eq!(warning.code(), "INACTIVE_HEADER_REFERENCE");
            }
        }
    }

    #[test]
    fn fix3_unused_empty_tab_case_survives_package_roundtrip() {
        // 이것을 실패시키는 것: 미사용 빈 case를 default 내용에 따라 거부/대체하거나 경고 누락.
        for default in ["", r#"<tabItem pos="2000"/>"#] {
            let tab = format!(
                r#"<tabProperties><tabPr id="99"><switch><case required-namespace="http://www.hancom.co.kr/hwpml/2016/HwpUnitChar"/><default>{default}</default></switch></tabPr></tabProperties>"#
            );
            let header = HEADER.replace("</refList>", &format!("{tab}</refList>"));
            let decoded = HwpxDecoder::decode(&make_test_hwpx(&header, &[SECTION_TEXT])).unwrap();
            assert!(decoded.style_store.iter_tabs().find(|t| t.id == 0).unwrap().stops.is_empty());
            assert_eq!(
                decoded
                    .warnings
                    .iter()
                    .filter(|w| matches!(w,
                DecodeWarning::ParagraphMeasureNormalized { definition: "tabPr", id: 0, field, .. }
                if field == "stops"))
                    .count(),
                usize::from(!default.is_empty())
            );
            let output = crate::HwpxEncoder::encode(
                &decoded.document.validate().unwrap(),
                &decoded.style_store,
                &decoded.image_store,
            )
            .unwrap();
            let after = HwpxDecoder::decode(&output).unwrap();
            assert!(after.style_store.iter_tabs().find(|t| t.id == 0).unwrap().stops.is_empty());
        }
    }

    #[test]
    fn empty_definitions_zero_decode_gate() {
        // 이것을 실패시키는 것: 빈 원본 charPr/paraPr 표의 명시적 0 참조를 디코드가 허용함.
        for kind in ["char", "para", "both"] {
            let mut header = HEADER.to_string();
            let start = header.find("<styles>").unwrap();
            let end = header.find("</styles>").unwrap() + "</styles>".len();
            header.replace_range(start..end, "");
            for (name, remove) in
                [("charProperties", kind != "para"), ("paraProperties", kind != "char")]
            {
                if remove {
                    let start = header.find(&format!("<{name}")).unwrap();
                    let end_tag = format!("</{name}>");
                    let end = header.find(&end_tag).unwrap() + end_tag.len();
                    header.replace_range(start..end, &format!("<{name} itemCnt=\"0\"/>"));
                }
            }
            let error = HwpxDecoder::decode(&make_test_hwpx(&header, &[SECTION_TEXT])).unwrap_err();
            eprintln!("HWPX {kind} empty: REJECTED {error}");
            let expected = if kind == "char" { "charPr" } else { "paraPr" };
            assert!(error.to_string().contains(expected), "{error}");
        }
    }

    #[test]
    fn decode_minimal_hwpx() {
        let bytes = make_test_hwpx(HEADER, &[SECTION_TEXT]);
        let result = HwpxDecoder::decode(&bytes).unwrap();

        // Document structure
        assert_eq!(result.document.sections().len(), 1);
        let section = &result.document.sections()[0];
        assert_eq!(section.paragraphs.len(), 1);

        // Text content
        let text = section.paragraphs[0].runs[0].content.as_text();
        assert_eq!(text, Some("안녕하세요"));

        // Page settings
        assert_eq!(section.page_settings.width.as_i32(), 59528);
        assert_eq!(section.page_settings.height.as_i32(), 84188);

        // Style store
        assert_eq!(result.style_store.font_count(), 7);
        assert_eq!(result.style_store.char_shape_count(), 1);
        assert_eq!(result.style_store.para_shape_count(), 1);
    }

    #[test]
    fn decode_multiple_sections() {
        let s0 = r#"<sec><p paraPrIDRef="0"><run charPrIDRef="0"><t>Section 0</t></run></p></sec>"#;
        let s1 = r#"<sec><p paraPrIDRef="0"><run charPrIDRef="0"><t>Section 1</t></run></p></sec>"#;
        let bytes = make_test_hwpx(HEADER, &[s0, s1]);
        let result = HwpxDecoder::decode(&bytes).unwrap();
        assert_eq!(result.document.sections().len(), 2);
    }

    #[test]
    fn decode_with_table() {
        let section = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <tbl rowCnt="1" colCnt="1">
                        <tr>
                            <tc name="A1">
                                <cellSz width="5000" height="1000"/>
                                <subList><p paraPrIDRef="0"><run charPrIDRef="0"><t>Cell</t></run></p></subList>
                            </tc>
                        </tr>
                    </tbl>
                </run>
            </p>
        </sec>"#;
        let bytes = make_test_hwpx(HEADER, &[section]);
        let result = HwpxDecoder::decode(&bytes).unwrap();
        let run = &result.document.sections()[0].paragraphs[0].runs[0];
        assert!(run.content.is_table());
    }

    #[test]
    fn decode_section_without_secpr_uses_a4_defaults() {
        let section = r#"<sec><p paraPrIDRef="0"><run charPrIDRef="0"><t>Text</t></run></p></sec>"#;
        let bytes = make_test_hwpx(HEADER, &[section]);
        let result = HwpxDecoder::decode(&bytes).unwrap();
        let ps = &result.document.sections()[0].page_settings;
        assert_eq!(*ps, PageSettings::a4());
    }

    #[test]
    fn decode_not_a_zip() {
        let err = HwpxDecoder::decode(b"not a zip").unwrap_err();
        assert!(matches!(err, crate::error::HwpxError::Zip(_)));
    }

    #[test]
    fn decode_file_nonexistent() {
        let err = HwpxDecoder::decode_file("/nonexistent/path.hwpx").unwrap_err();
        assert!(matches!(err, crate::error::HwpxError::Io(_)));
    }

    // ── Header / Footer / PageNum decode tests ──────────────────

    #[test]
    fn decode_section_with_header_ctrl() {
        let section = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <ctrl>
                        <header id="0" applyPageType="BOTH">
                            <subList id="0" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP"
                                     linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0">
                                <p paraPrIDRef="0">
                                    <run charPrIDRef="0"><t>Page Header</t></run>
                                </p>
                            </subList>
                        </header>
                    </ctrl>
                    <t>Body text</t>
                </run>
            </p>
        </sec>"#;
        let bytes = make_test_hwpx(HEADER, &[section]);
        let result = HwpxDecoder::decode(&bytes).unwrap();

        let sec = &result.document.sections()[0];
        let header = sec.headers.first().expect("section should have header");
        assert_eq!(header.apply_page_type, hwpforge_foundation::ApplyPageType::Both);
        assert_eq!(header.paragraphs.len(), 1);
        assert_eq!(header.paragraphs[0].runs[0].content.as_text(), Some("Page Header"));
    }

    #[test]
    fn decode_section_with_footer_and_pagenum() {
        let section = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <ctrl>
                        <footer id="0" applyPageType="ODD">
                            <subList id="0" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP"
                                     linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0">
                                <p paraPrIDRef="0">
                                    <run charPrIDRef="0"><t>Footer</t></run>
                                </p>
                            </subList>
                        </footer>
                    </ctrl>
                    <ctrl>
                        <pageNum pos="BOTTOM_CENTER" formatType="DIGIT" sideChar="- "/>
                    </ctrl>
                    <t>Body</t>
                </run>
            </p>
        </sec>"#;
        let bytes = make_test_hwpx(HEADER, &[section]);
        let result = HwpxDecoder::decode(&bytes).unwrap();

        let sec = &result.document.sections()[0];
        let footer = sec.footers.first().expect("section should have footer");
        assert_eq!(footer.apply_page_type, hwpforge_foundation::ApplyPageType::Odd);
        assert_eq!(footer.paragraphs[0].runs[0].content.as_text(), Some("Footer"));

        let pn = sec.page_number.as_ref().expect("section should have page number");
        assert_eq!(pn.position, hwpforge_foundation::PageNumberPosition::BottomCenter);
        assert_eq!(pn.number_format, hwpforge_foundation::NumberFormatType::Digit);
        assert_eq!(pn.decoration, "- ");
    }

    // ── Image binary roundtrip test ─────────────────────────────

    #[test]
    fn decode_extracts_bindata_images() {
        let buf = Vec::new();
        let mut zip = ZipWriter::new(Cursor::new(buf));
        let stored =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let deflate = SimpleFileOptions::default();

        zip.start_file("mimetype", stored).unwrap();
        zip.write_all(b"application/hwp+zip").unwrap();

        zip.start_file("Contents/header.xml", deflate).unwrap();
        zip.write_all(HEADER.as_bytes()).unwrap();

        let section = r#"<sec><p paraPrIDRef="0"><run charPrIDRef="0"><t>Body</t></run></p></sec>"#;
        zip.start_file("Contents/section0.xml", deflate).unwrap();
        zip.write_all(section.as_bytes()).unwrap();

        // Add a BinData image
        let fake_png = vec![0x89, 0x50, 0x4E, 0x47]; // PNG magic bytes
        zip.start_file("BinData/logo.png", stored).unwrap();
        zip.write_all(&fake_png).unwrap();

        let bytes = zip.finish().unwrap().into_inner();
        let result = HwpxDecoder::decode(&bytes).unwrap();

        assert!(!result.image_store.is_empty(), "image store should contain extracted images");
        let data = result.image_store.get("logo.png").expect("should find logo.png");
        assert_eq!(data, &fake_png);
    }
}

/// Limits of the shared serde entry point [`xml_from_str`]: our structural
/// nesting guards must stay the error that fires at 33 levels (not the serde
/// recursion budget), and the namespace-binding cap is 128 in scope.
///
/// Boundary cases run on a 64 MiB thread so they hold in every build
/// profile (an unoptimized build overflows 2 MiB at 32 nested tables, and an
/// overflow aborts the test process). The documented 1 MiB minimum for
/// optimized builds is checked by `tests/decode_stack.rs`, which links the
/// library as it ships.
#[cfg(test)]
mod warning_accessor_tests {
    use super::{warning_samples, DecodeWarning, ParagraphPath, PathSeg};

    fn content_lost() -> DecodeWarning {
        DecodeWarning::ContentLost {
            part: "Contents/section1.xml".into(),
            path: ParagraphPath(vec![PathSeg::Section(1), PathSeg::GroupChild(2)]),
            kind: "pic".into(),
            reason: "unsupported group child".into(),
        }
    }

    /// 이것을 실패시키는 것: `is_content_loss` 의 `ContentLost` 갈래를 `false` 로
    /// 바꾸는 것 — 입력 경계 편집기가 손실 입력을 통과시킨다.
    #[test]
    fn semantic_recovery_warnings_block_editing() {
        for w in warning_samples::decode() {
            assert_eq!(
                w.is_content_loss(),
                matches!(
                    w,
                    DecodeWarning::ContentLost { .. } | DecodeWarning::InvalidFieldBoundary { .. }
                ),
                "{w:?}"
            );
        }
    }

    /// 이것을 실패시키는 것: 표본 목록에서 한 변형을 빼는 것 — 고유 코드 수가
    /// 갈래 수와 어긋난다.
    #[test]
    fn samples_cover_every_code_once() {
        let codes: std::collections::BTreeSet<&str> =
            warning_samples::decode().iter().map(DecodeWarning::code).collect();
        assert_eq!(
            codes.into_iter().collect::<Vec<_>>(),
            [
                "BEGIN_NUM_REPAIRED",
                "CONTENT_LOST",
                "INACTIVE_HEADER_REFERENCE",
                "INVALID_FIELD_BOUNDARY",
                "LAYOUT_CACHE_DROPPED",
                "PARAGRAPH_MEASURE_NORMALIZED",
                "UNCLOSED_FIELD_BOUNDARY",
                "UNKNOWN_ENUM_VALUE"
            ]
        );
    }

    #[test]
    fn content_lost_reports_part_and_place() {
        let w = content_lost();
        assert_eq!(w.code(), "CONTENT_LOST");
        assert_eq!(w.part(), Some("Contents/section1.xml"));
        assert_eq!(w.location().as_deref(), Some("Contents/section1.xml:section[1].group[2]"));
        assert_eq!(
            w.to_string(),
            "pic lost at Contents/section1.xml:section[1].group[2]: unsupported group child"
        );
    }

    /// 이것을 실패시키는 것: `message` 가 자리를 다시 넣는 것 (자리 없는 문구가
    /// 깨진다).
    #[test]
    fn message_leaves_the_location_out() {
        for w in warning_samples::decode() {
            let loc = w.location().unwrap();
            assert!(!w.message().contains(&loc), "{w:?}: {}", w.message());
        }
    }

    #[test]
    fn master_page_segment_displays() {
        let path = ParagraphPath(vec![PathSeg::MasterPage(3), PathSeg::NestedParagraph(0)]);
        assert_eq!(path.to_string(), "masterpage[3].npara[0]");
    }
}

#[cfg(test)]
mod xml_limit_tests {
    use std::collections::HashMap;

    use super::section::parse_section;
    use crate::error::HwpxError;

    const DEEPEST: &str = "XML_LIMIT_DEEPEST";

    fn group_section(levels: usize) -> String {
        let mut fragment = format!(
            r#"<hp:rect id="1001"><hp:orgSz width="1000" height="1000"/><hp:curSz width="1000" height="1000"/><hp:sz width="1000" height="1000"/><hp:drawText><hp:subList><hp:p paraPrIDRef="0"><hp:run charPrIDRef="0"><hp:t>{DEEPEST}</hp:t></hp:run></hp:p></hp:subList></hp:drawText></hp:rect>"#
        );
        for _ in 0..levels {
            fragment = format!(
                r#"<hp:container id="1000"><hp:orgSz width="15000" height="13000"/><hp:curSz width="0" height="0"/><hp:sz width="15000" height="13000"/>{fragment}</hp:container>"#
            );
        }
        format!(
            r#"<hs:sec xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section" xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph"><hp:p paraPrIDRef="0"><hp:run charPrIDRef="0">{fragment}</hp:run></hp:p></hs:sec>"#
        )
    }

    /// Decodes on a 64 MiB thread; returns the paragraphs' debug text.
    fn decode_on_big_stack(xml: String) -> Result<String, HwpxError> {
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn(move || {
                parse_section(&xml, 0, &HashMap::new()).map(|r| format!("{:?}", r.paragraphs))
            })
            .expect("spawn decode thread")
            .join()
            .expect("decode thread panicked")
    }

    // 묶음 도형도 표와 같은 한도: 32겹은 가장 안쪽 글자까지 살아 있다.
    // 이것을 실패시키는 것: `decode_container` 가드를 `depth >= MAX_NESTING_DEPTH`
    // 에서 `depth >= 31` 로 낮추기 (32겹이 오류가 된다).
    #[test]
    // 이것을 실패시키는 것: 그룹 깊이 guard 초과 (실제 음성 대조 실행).
    fn xml_limit_group_nesting_32_decodes() {
        match decode_on_big_stack(group_section(32)) {
            Ok(debug) => {
                assert!(debug.contains(DEEPEST), "group x32: deepest text lost");
                assert_eq!(debug.matches("Group {").count(), 32, "group x32: Group count");
            }
            Err(err) => panic!("group x32 must decode, got: {err:?}"),
        }
    }

    /// `n` namespace declarations with distinct prefixes starting `tag`.
    fn namespace_decls(tag: &str, n: usize) -> String {
        (0..n).map(|k| format!(r#" xmlns:{tag}{k}="urn:hwpforge-test:{tag}{k}""#)).collect()
    }

    /// A minimal section whose `sec`, `p` and `run` elements declare the
    /// given numbers of namespace bindings, all in scope at the run.
    fn namespace_section(on_sec: usize, on_p: usize, on_run: usize) -> String {
        format!(
            r#"<sec{}><p paraPrIDRef="0"{}><run charPrIDRef="0"{}><t>ns</t></run></p></sec>"#,
            namespace_decls("s", on_sec),
            namespace_decls("p", on_p),
            namespace_decls("r", on_run),
        )
    }

    fn assert_namespace_limit_rejects(xml: &str) {
        match parse_section(xml, 0, &HashMap::new()) {
            // 이것을 실패시키는 것: `xml_error_detail` 없이 quick-xml 문구를 그대로 쓰는 것 —
            // 사용자가 부를 수 없는 `NamespaceResolver` API 를 안내한다.
            Err(HwpxError::XmlParse { detail, .. })
                if detail.contains("more than 128 namespace bindings")
                    && !detail.contains("NamespaceResolver") => {}
            other => panic!("129 bindings in scope must be rejected, got: {other:?}"),
        }
    }

    // quick-xml 0.42 의 기본값도 128 이라 `set_max_namespace_bindings` 호출을
    // 빼는 변이는 오늘 이 테스트들을 빨갛게 만들지 못한다(no-op). 호출은 다음
    // bump 에서 기본값이 움직이는 것을 막으려고 둔다.
    // 이것을 실패시키는 것: `XML_MAX_NAMESPACE_BINDINGS` 를 127 로 낮추기.
    #[test]
    fn xml_limit_namespace_bindings_128_on_root_decode() {
        parse_section(&namespace_section(128, 0, 0), 0, &HashMap::new()).expect("128 bindings");
    }

    // 이것을 실패시키는 것: `XML_MAX_NAMESPACE_BINDINGS` 를 256 (또는 usize::MAX)
    // 으로 올리기.
    #[test]
    fn xml_limit_namespace_bindings_129_on_root_rejected() {
        assert_namespace_limit_rejects(&namespace_section(129, 0, 0));
    }

    // 요소 하나당이 아니라 범위 안 합계를 센다(0.41 은 요소당 256).
    // 이것을 실패시키는 것: `XML_MAX_NAMESPACE_BINDINGS` 를 127 로 낮추기.
    #[test]
    fn xml_limit_namespace_bindings_128_spread_decode() {
        parse_section(&namespace_section(43, 43, 42), 0, &HashMap::new())
            .expect("128 bindings spread over sec/p/run");
    }

    // 이것을 실패시키는 것: `XML_MAX_NAMESPACE_BINDINGS` 를 256 으로 올리기.
    #[test]
    fn xml_limit_namespace_bindings_129_spread_rejected() {
        assert_namespace_limit_rejects(&namespace_section(43, 43, 43));
    }
}
