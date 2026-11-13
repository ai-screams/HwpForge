//! The Document type with typestate pattern.
//!
//! [`Document<S>`] is the aggregate root of the Core DOM. It uses the
//! **typestate pattern** to enforce document lifecycle at compile time:
//!
//! - [`Draft`] -- mutable, can add/remove sections
//! - [`Validated`] -- immutable structure, safe for serialization/export
//!
//! The transition `Draft -> Validated` is one-way via [`Document::validate()`],
//! which consumes the draft (move semantics prevent reuse).
//!
//! # Design Decisions
//!
//! - **Typestate, not enum** -- invalid operations are compile errors
//!   (not runtime panics).
//! - **Deserialize always to Draft** -- serialized data may be modified
//!   externally; re-validation is mandatory.
//! - **No `Styled` state** -- styling is handled by
//!   Blueprint (StyleRegistry).
//!
//! # Examples
//!
//! ```
//! use hwpforge_core::document::{Document, Draft, Validated};
//! use hwpforge_core::section::Section;
//! use hwpforge_core::paragraph::Paragraph;
//! use hwpforge_core::run::Run;
//! use hwpforge_core::PageSettings;
//! use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
//!
//! let mut doc = Document::new();
//! doc.add_section(Section::with_paragraphs(
//!     vec![Paragraph::with_runs(
//!         vec![Run::text("Hello", CharShapeIndex::new(0))],
//!         ParaShapeIndex::new(0),
//!     )],
//!     PageSettings::a4(),
//! ));
//!
//! let validated: Document<Validated> = doc.validate().unwrap();
//! assert_eq!(validated.section_count(), 1);
//! ```
//!
//! ```compile_fail
//! // A validated document cannot add sections:
//! use hwpforge_core::document::{Document, Validated};
//! use hwpforge_core::section::Section;
//! use hwpforge_core::PageSettings;
//!
//! # fn get_validated() -> Document<Validated> { todo!() }
//! let mut validated = get_validated();
//! validated.add_section(Section::new(PageSettings::a4()));
//! // ERROR: no method named `add_section` found for `Document<Validated>`
//! ```

use std::marker::PhantomData;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::CoreResult;
use crate::metadata::Metadata;
use crate::paragraph::Paragraph;
use crate::section::Section;
use crate::validate::validate_sections;

/// 문서의 배치 수치를 해석하는 호환 프로필.
///
/// Word 조판 재현 보장 아님. 줄 간격 수치나 문단·표 구조를 자동 변환하지 않는다.
/// 파일 형식별 문자열과 호환 플래그는 각 Smithy의 책임이다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum LayoutProfile {
    /// 기존 한컴 문서의 해석 규칙을 유지하는 기본값.
    /// 백분율 줄 간격의 기준은 글자 크기(em)다.
    /// 글꼴 자연 줄 높이를 곱셈의 기준으로 다시 적용하지 않는다.
    /// 기존 첫 줄 배치와 컨테이너 간격 정책을 유지한다.
    #[default]
    Hancom,
    /// 백분율 줄 간격을 글꼴의 자연 줄 높이에 대한 배수로 해석한다.
    /// 100은 한 배, 115는 1.15배이며 em 기준으로 미리 환산하지 않는다.
    /// 측정한 표본에서는 첫 줄 위 여유가 포함되는 배치가 관찰됐다.
    /// 위 여유의 정확한 산식과 컨테이너 앞뒤 간격 규칙은 미확인이다.
    /// 표 anchor 이동과 셀 TOP 정렬은 가져오기 단계의 별도 처리다.
    WordCompatible,
}

/// Marker type: the document is a mutable draft.
///
/// A `Document<Draft>` can be modified (add sections, set metadata)
/// and then validated via [`Document::validate()`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Draft;

/// Marker type: the document has passed structural validation.
///
/// A `Document<Validated>` is guaranteed to have:
/// - At least 1 section
/// - Every section has at least 1 paragraph
/// - Every paragraph has at least 1 run
/// - All table/control structural invariants hold
/// - Semantic nesting depth is at most 32 (see [`Section::check_nesting_depth`])
///
/// The only way to obtain a `Document<Validated>` is through
/// [`Document<Draft>::validate()`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Validated;

/// The document aggregate root with compile-time state tracking.
///
/// The generic parameter `S` determines which operations are available:
///
/// | State | Mutable | Serializable | Exportable |
/// |-------|---------|-------------|-----------|
/// | [`Draft`] | Yes | Yes | No (must validate first) |
/// | [`Validated`] | No | Yes | Yes |
///
/// # Typestate Safety
///
/// The `_state` field is private and zero-sized. There is no way to
/// construct a `Document<Validated>` except through `validate()`.
///
/// # Examples
///
/// ```
/// use hwpforge_core::document::Document;
/// use hwpforge_core::Metadata;
///
/// let doc = Document::with_metadata(Metadata::new().with_title("Report"));
/// assert!(doc.is_empty());
/// ```
pub struct Document<S = Draft> {
    sections: Vec<Section>,
    metadata: Metadata,
    layout_profile: LayoutProfile,
    _state: PhantomData<S>,
    format_source: Option<
        std::sync::Arc<
            dyn std::any::Any + Send + Sync + std::panic::RefUnwindSafe + std::panic::UnwindSafe,
        >,
    >,
}

// ---------------------------------------------------------------------------
// Shared methods (any state)
// ---------------------------------------------------------------------------

impl<S> Document<S> {
    /// 문서의 배치 수치 해석 프로필을 반환한다.
    pub fn layout_profile(&self) -> LayoutProfile {
        self.layout_profile
    }

    /// Immutable, non-serialized decoder context, independent of submitted nodes.
    /// Only the owning codec can interpret its private context type.
    #[doc(hidden)]
    pub fn format_source<T: std::any::Any + Send + Sync>(&self) -> Option<std::sync::Arc<T>> {
        let source: std::sync::Arc<dyn std::any::Any + Send + Sync> = self.format_source.clone()?;
        source.downcast().ok()
    }

    /// Visits every table in the document, in every section (see
    /// [`crate::section::Section::for_each_table`]).
    pub fn for_each_table<F: FnMut(&crate::table::Table)>(&self, mut f: F) {
        for section in &self.sections {
            section.for_each_table(&mut f);
        }
    }

    /// Returns a slice of all sections.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::document::Document;
    ///
    /// let doc = Document::new();
    /// assert!(doc.sections().is_empty());
    /// ```
    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// Returns a reference to the document metadata.
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// Returns the number of sections.
    pub fn section_count(&self) -> usize {
        self.sections.len()
    }

    /// Returns `true` if the document has no sections.
    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Draft-only methods
// ---------------------------------------------------------------------------

impl Document<Draft> {
    /// 배치 수치 해석 프로필을 설정한다. 스타일 수치나 문단·표 구조는 변환하지 않는다.
    ///
    /// 일반 출력에서 변경하면 기존 배치 캐시를 재사용하지 않는다. 호출자가 새 의미에
    /// 맞는 스타일과 구조를 책임진다. base 포함 보존 편집은 base와 다른 프로필을 거부한다.
    pub fn set_layout_profile(&mut self, profile: LayoutProfile) {
        self.layout_profile = profile;
    }

    /// Bind an immutable decoder context independently of node provenance claims.
    #[doc(hidden)]
    pub fn set_format_source<
        T: std::any::Any + Send + Sync + std::panic::RefUnwindSafe + std::panic::UnwindSafe,
    >(
        &mut self,
        source: std::sync::Arc<T>,
    ) {
        self.format_source = Some(source);
    }

    /// Creates a new empty draft document with default metadata.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::document::Document;
    ///
    /// let doc = Document::new();
    /// assert!(doc.is_empty());
    /// ```
    pub fn new() -> Self {
        Self {
            sections: Vec::new(),
            metadata: Metadata::default(),
            layout_profile: LayoutProfile::default(),
            format_source: None,
            _state: PhantomData,
        }
    }

    /// Creates a new draft document with the given metadata.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::document::Document;
    /// use hwpforge_core::Metadata;
    ///
    /// let doc = Document::with_metadata(Metadata::new().with_title("Test"));
    /// assert_eq!(doc.metadata().title.as_deref(), Some("Test"));
    /// ```
    pub fn with_metadata(metadata: Metadata) -> Self {
        Self {
            sections: Vec::new(),
            metadata,
            layout_profile: LayoutProfile::default(),
            format_source: None,
            _state: PhantomData,
        }
    }

    /// 문서의 문단을 문서 순서(pre-order)로 방문한다.
    ///
    /// 캐시 정규화([`Self::strip_layout_caches`]) 등 전 문단 일괄 변환의
    /// 진입점이다. `Draft` 전용 — `Validated` 는 typestate 상 불변이므로
    /// 비교가 필요하면 검증 전 사본에서 수행한다.
    ///
    /// # 재귀 대상 (정확한 목록)
    ///
    /// 섹션마다 본문 → 머리말 → 꼬리말 → 바탕쪽 순으로 돌고, 각 문단은
    /// **자신을 먼저** 방문한 뒤 run 안으로 내려간다:
    ///
    /// - 표: 행 → 셀 → 셀 문단(중첩 표 포함), 그다음 표 캡션
    /// - 도형/글상자: 본문 문단 + 캡션 (타원·다각형 동일; 선·사각형·호·
    ///   곡선·연결선은 캡션만)
    /// - 각주·미주: 본문 문단
    /// - 메모: 본문 문단 + 앵커 run 안의 중첩
    /// - 묶음 객체: 자식 컨트롤로 재귀
    ///
    /// # 방문하지 않는 것 (알려진 갭)
    ///
    /// **[`crate::image::Image::caption`] 안의 문단은 방문하지 않는다.**
    /// [`crate::run::RunContent::Image`] 는 재귀 대상이 아니기 때문이다.
    /// 표·글상자 캡션은 방문되므로 캡션 처리가 비대칭이다 — 의도된 설계가
    /// 아니라 갭이며, 재귀 대상을 넓히면 캐시 정규화·편집 파이프라인 등
    /// **모든 기존 호출자의 동작이 바뀌므로** 픽스처를 갖춘 별도 작업으로
    /// 다룬다.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::document::Document;
    /// use hwpforge_core::page::PageSettings;
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_core::section::Section;
    /// use hwpforge_foundation::ParaShapeIndex;
    ///
    /// let mut doc = Document::new();
    /// doc.add_section(Section::with_paragraphs(
    ///     vec![Paragraph::new(ParaShapeIndex::new(0))],
    ///     PageSettings::a4(),
    /// ));
    /// let mut count = 0;
    /// doc.for_each_paragraph_mut(|_| count += 1);
    /// assert_eq!(count, 1);
    /// ```
    pub fn for_each_paragraph_mut<F: FnMut(&mut Paragraph)>(&mut self, mut f: F) {
        for section in &mut self.sections {
            section.walk_paragraphs_mut(&mut f);
        }
    }

    /// [`Self::for_each_table`] 의 가변판 — 방문 대상·순서 동일.
    pub fn for_each_table_mut<F: FnMut(&mut crate::table::Table)>(&mut self, mut f: F) {
        for section in &mut self.sections {
            section.for_each_table_mut(&mut f);
        }
    }

    /// [`Self::for_each_paragraph_mut`] 의 불변 쌍둥이 — **방문 순서와 재귀
    /// 대상이 완전히 같다**. 재귀 대상 목록과 이미지 캡션 갭은 그쪽 문서에
    /// 있다.
    ///
    /// 문서를 바꾸지 않고 훑기만 하는 호출자(자산 계획 수집·통계·검증)를
    /// 위한 것이다. 순서가 같아야 두 순회를 같은 문단 번호로 짝지을 수
    /// 있으므로, 한쪽만 고치면 안 된다 —
    /// `layout::tests::immutable_walker_matches_the_mutable_one` 이 잠근다.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::document::Document;
    /// use hwpforge_core::page::PageSettings;
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_core::section::Section;
    /// use hwpforge_foundation::ParaShapeIndex;
    ///
    /// let mut doc = Document::new();
    /// doc.add_section(Section::with_paragraphs(
    ///     vec![Paragraph::new(ParaShapeIndex::new(0))],
    ///     PageSettings::a4(),
    /// ));
    /// let mut count = 0;
    /// doc.for_each_paragraph(|_| count += 1);
    /// assert_eq!(count, 1);
    /// ```
    pub fn for_each_paragraph<F: FnMut(&Paragraph)>(&self, mut f: F) {
        for section in &self.sections {
            section.walk_paragraphs(&mut f);
        }
    }

    /// 모든 문단의 줄 조판 캐시([`Paragraph::layout_cache`])를 제거한다.
    ///
    /// 문서 동등성 비교(admission/golden)의 정규화 단계: 네이티브 입력은
    /// 캐시를 보유하고 우리 재인코드 산출물은 미보유가 정상이므로, 비교
    /// 전 양쪽 사본에서 이 함수를 호출해 캐시 차이를 제거한다.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::document::Document;
    /// use hwpforge_core::layout::LayoutCache;
    /// use hwpforge_core::page::PageSettings;
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_core::section::Section;
    /// use hwpforge_foundation::ParaShapeIndex;
    ///
    /// let mut para = Paragraph::new(ParaShapeIndex::new(0));
    /// para.layout_cache = Some(LayoutCache::default());
    /// let mut doc = Document::new();
    /// doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));
    /// doc.strip_layout_caches();
    /// assert!(doc.sections()[0].paragraphs[0].layout_cache.is_none());
    /// ```
    pub fn strip_layout_caches(&mut self) {
        self.for_each_paragraph_mut(|p| {
            p.layout_cache = None;
            // 표 수준 decode-only 캐시도 함께 제거 — 방문 문단의 직계 run 만
            // 보면 충분하다 (중첩 표의 host 는 셀 문단이고, walker 가 셀
            // 문단도 방문한다).
            for run in &mut p.runs {
                if let crate::run::RunContent::Table(table) = &mut run.content {
                    table.layout_cache = None;
                }
            }
        });
    }

    /// Appends a section to the draft document.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::document::Document;
    /// use hwpforge_core::section::Section;
    /// use hwpforge_core::PageSettings;
    ///
    /// let mut doc = Document::new();
    /// doc.add_section(Section::new(PageSettings::a4()));
    /// assert_eq!(doc.section_count(), 1);
    /// ```
    pub fn add_section(&mut self, section: Section) {
        self.sections.push(section);
    }

    /// Sets the document metadata.
    pub fn set_metadata(&mut self, metadata: Metadata) {
        self.metadata = metadata;
    }

    /// Returns a mutable reference to the metadata.
    pub fn metadata_mut(&mut self) -> &mut Metadata {
        &mut self.metadata
    }

    /// Returns a mutable slice of sections.
    pub fn sections_mut(&mut self) -> &mut [Section] {
        &mut self.sections
    }

    /// Validates the document structure and transitions to `Validated`.
    ///
    /// Consumes `self` (move semantics). On success, returns a
    /// `Document<Validated>` with semantic nesting depth at most 32.
    /// Every section receives the same check as [`Section::check_nesting_depth`].
    /// On failure, returns a `CoreError`.
    ///
    /// Normalizes table header-row convenience flags from effective cell
    /// markers, preserving their meaning (including non-leading title cells).
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Validation`](crate::error::CoreError::Validation) if the document violates any
    /// structural invariant (empty sections, empty paragraphs, etc.).
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::document::Document;
    /// use hwpforge_core::section::Section;
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_core::run::Run;
    /// use hwpforge_core::PageSettings;
    /// use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
    ///
    /// let mut doc = Document::new();
    /// doc.add_section(Section::with_paragraphs(
    ///     vec![Paragraph::with_runs(
    ///         vec![Run::text("Hello", CharShapeIndex::new(0))],
    ///         ParaShapeIndex::new(0),
    ///     )],
    ///     PageSettings::a4(),
    /// ));
    ///
    /// let validated = doc.validate().unwrap();
    /// assert_eq!(validated.section_count(), 1);
    /// ```
    ///
    /// ```
    /// use hwpforge_core::document::Document;
    ///
    /// let doc = Document::new(); // empty
    /// assert!(doc.validate().is_err());
    /// ```
    pub fn validate(mut self) -> CoreResult<Document<Validated>> {
        // 실패 시 해제는 Paragraph·Control 의 Drop 이 반복형으로 한다.
        // 검사는 정규화보다 먼저. 이것을 실패시키는 것: 정규화 뒤로 이동
        // (consuming_deep_validation_small_stack, 2 MiB / 100,000).
        crate::depth::check(&self.sections)?;
        // Cell meaning is canonical. Normalize even tables in nested containers
        // and bands; the shared walk includes picture captions and memo anchors.
        // 이것을 실패시키는 것: 정규화 삭제 · 본문 표만 순회
        // (`cell_header_validation_normalizes_every_table`).
        for section in &mut self.sections {
            section.for_each_table_mut(|table| {
                crate::table::TableRow::derive_header_rows(&mut table.rows);
            });
        }
        validate_sections(&self.sections)?;
        Ok(Document {
            sections: self.sections,
            metadata: self.metadata,
            layout_profile: self.layout_profile,
            format_source: self.format_source,
            _state: PhantomData,
        })
    }
}

impl Default for Document<Draft> {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Manual trait impls (avoid T: Trait bounds on phantom type S)
// ---------------------------------------------------------------------------

impl<S> std::fmt::Debug for Document<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Document")
            .field("sections", &self.sections)
            .field("metadata", &self.metadata)
            .field("layout_profile", &self.layout_profile)
            .finish()
    }
}

impl<S> Clone for Document<S> {
    fn clone(&self) -> Self {
        Self {
            sections: self.sections.clone(),
            metadata: self.metadata.clone(),
            layout_profile: self.layout_profile,
            format_source: self.format_source.clone(),
            _state: PhantomData,
        }
    }
}

impl<S> PartialEq for Document<S> {
    fn eq(&self, other: &Self) -> bool {
        self.sections == other.sections
            && self.metadata == other.metadata
            && self.layout_profile == other.layout_profile
    }
}

impl<S> Eq for Document<S> {}

impl<S> std::fmt::Display for Document<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Document({} sections)", self.sections.len())
    }
}

// ---------------------------------------------------------------------------
// Serde: serialize any state, deserialize only to Draft
// ---------------------------------------------------------------------------

impl<S> Serialize for Document<S> {
    fn serialize<Ser: serde::Serializer>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error> {
        use serde::ser::SerializeStruct;
        let include_profile = self.layout_profile != LayoutProfile::Hancom;
        let mut state =
            serializer.serialize_struct("Document", 2 + usize::from(include_profile))?;
        state.serialize_field("sections", &self.sections)?;
        state.serialize_field("metadata", &self.metadata)?;
        if include_profile {
            state.serialize_field("layout_profile", &self.layout_profile)?;
        }
        state.end()
    }
}

impl<'de> Deserialize<'de> for Document<Draft> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct DocumentData {
            sections: Vec<Section>,
            metadata: Metadata,
            #[serde(default)]
            layout_profile: LayoutProfile,
        }

        let data = DocumentData::deserialize(deserializer)?;
        Ok(Document {
            sections: data.sections,
            metadata: data.metadata,
            layout_profile: data.layout_profile,
            format_source: None,
            _state: PhantomData,
        })
    }
}

// ---------------------------------------------------------------------------
// JsonSchema: hide PhantomData
// ---------------------------------------------------------------------------

impl<S> JsonSchema for Document<S> {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Document".into()
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let mut profile = gen.subschema_for::<LayoutProfile>();
        profile.insert("default".into(), "hancom".into());
        schemars::json_schema!({
            "type": "object",
            "properties": {
                "sections": gen.subschema_for::<Vec<Section>>(),
                "metadata": gen.subschema_for::<crate::metadata::Metadata>(),
                "layout_profile": profile,
            },
            "required": ["sections", "metadata"]
        })
    }
}

// ---------------------------------------------------------------------------
// Send + Sync verification
// ---------------------------------------------------------------------------

const _: () = {
    #[allow(dead_code)]
    fn assert_send<T: Send>() {}
    #[allow(dead_code)]
    fn assert_sync<T: Sync>() {}
    #[allow(dead_code)]
    fn verify() {
        assert_send::<Document<Draft>>();
        assert_sync::<Document<Draft>>();
        assert_send::<Document<Validated>>();
        assert_sync::<Document<Validated>>();
    }
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CoreError;
    use crate::page::PageSettings;
    use crate::paragraph::Paragraph;
    use crate::run::Run;
    use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};

    fn valid_section() -> Section {
        Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::text("Hello", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        )
    }

    #[test]
    fn layout_profile_survives_document_operations() {
        // 이것을 실패시키는 것: 기본값 변경, 수동 복제·동등성·Debug·검증의 프로필 누락.
        for doc in
            [Document::new(), Document::default(), Document::with_metadata(Metadata::default())]
        {
            assert_eq!(doc.layout_profile(), LayoutProfile::Hancom);
        }
        let mut hancom = Document::new();
        hancom.add_section(valid_section());
        let mut word = hancom.clone();
        word.set_layout_profile(LayoutProfile::WordCompatible);
        assert_ne!(word, hancom);
        assert_eq!(word.clone().layout_profile(), LayoutProfile::WordCompatible);
        assert!(format!("{word:?}").contains("layout_profile: WordCompatible"));
        word.set_metadata(Metadata::default());
        word.metadata_mut().title = Some("title".into());
        assert_eq!(word.layout_profile(), LayoutProfile::WordCompatible);
        word.set_format_source(std::sync::Arc::new(37_u32));
        let validated = word.clone().validate().unwrap();
        assert_eq!(validated.layout_profile(), LayoutProfile::WordCompatible);
        assert_eq!(*validated.format_source::<u32>().unwrap(), 37);
    }

    #[test]
    fn layout_profile_json_is_optional_and_rejects_unknown_values() {
        // 이것을 실패시키는 것: Hancom 출력, Word 출력·복원 누락, 미지 값 허용.
        let hancom = serde_json::to_value(Document::new()).unwrap();
        assert!(hancom.get("layout_profile").is_none());
        assert_eq!(
            serde_json::from_value::<Document>(hancom.clone()).unwrap().layout_profile(),
            LayoutProfile::Hancom
        );
        let mut word = Document::new();
        word.set_layout_profile(LayoutProfile::WordCompatible);
        let value = serde_json::to_value(&word).unwrap();
        assert_eq!(value["layout_profile"], "word_compatible");
        assert_eq!(serde_json::from_value::<Document>(value).unwrap(), word);
        for name in ["hancom", "word_compatible", "word", "unknown"] {
            let mut value = hancom.clone();
            value["layout_profile"] = name.into();
            let result = serde_json::from_value::<Document>(value);
            assert_eq!(result.is_ok(), matches!(name, "hancom" | "word_compatible"));
        }
        let schema = serde_json::to_value(schemars::schema_for!(Document)).unwrap();
        assert_eq!(schema["properties"]["layout_profile"]["default"], "hancom");
        assert_eq!(
            schema["$defs"]["LayoutProfile"]["oneOf"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v["const"].clone())
                .collect::<Vec<_>>(),
            vec![serde_json::json!("hancom"), serde_json::json!("word_compatible")]
        );
        assert_eq!(schema["required"], serde_json::json!(["sections", "metadata"]));
    }

    // === Construction ===

    #[test]
    fn new_creates_empty_draft() {
        let doc = Document::new();
        assert!(doc.is_empty());
        assert_eq!(doc.section_count(), 0);
        assert!(doc.metadata().title.is_none());
    }

    #[test]
    fn with_metadata() {
        let meta = Metadata { title: Some("Test".to_string()), ..Metadata::default() };
        let doc = Document::with_metadata(meta);
        assert_eq!(doc.metadata().title.as_deref(), Some("Test"));
    }

    #[test]
    fn default_is_new() {
        let a = Document::new();
        let b = Document::default();
        assert_eq!(a, b);
    }

    // === Draft mutations ===

    #[test]
    fn add_section() {
        let mut doc = Document::new();
        doc.add_section(valid_section());
        assert_eq!(doc.section_count(), 1);
        assert!(!doc.is_empty());
    }

    #[test]
    fn add_multiple_sections() {
        let mut doc = Document::new();
        doc.add_section(valid_section());
        doc.add_section(valid_section());
        doc.add_section(valid_section());
        assert_eq!(doc.section_count(), 3);
    }

    #[test]
    fn set_metadata() {
        let mut doc = Document::new();
        doc.set_metadata(Metadata { title: Some("New".to_string()), ..Metadata::default() });
        assert_eq!(doc.metadata().title.as_deref(), Some("New"));
    }

    #[test]
    fn metadata_mut() {
        let mut doc = Document::new();
        doc.metadata_mut().title = Some("Mutated".to_string());
        assert_eq!(doc.metadata().title.as_deref(), Some("Mutated"));
    }

    #[test]
    fn sections_mut() {
        let mut doc = Document::new();
        doc.add_section(valid_section());
        doc.add_section(valid_section());
        assert_eq!(doc.sections_mut().len(), 2);
    }

    // === Validation (Draft -> Validated) ===

    #[test]
    fn validate_success() {
        let mut doc = Document::new();
        doc.add_section(valid_section());
        let validated = doc.validate().unwrap();
        assert_eq!(validated.section_count(), 1);
    }

    #[test]
    fn validate_empty_document_fails() {
        let doc = Document::new();
        let err = doc.validate().unwrap_err();
        assert!(matches!(err, CoreError::Validation(_)));
    }

    #[test]
    fn validate_empty_section_fails() {
        let mut doc = Document::new();
        doc.add_section(Section::new(PageSettings::a4()));
        assert!(doc.validate().is_err());
    }

    #[test]
    fn validate_consumes_draft() {
        let mut doc = Document::new();
        doc.add_section(valid_section());
        let _validated = doc.validate().unwrap();
        // doc is moved -- attempting to use it would be a compile error
    }

    // === Validated state ===

    #[test]
    fn validated_has_read_methods() {
        let mut doc = Document::new();
        doc.add_section(valid_section());
        let validated = doc.validate().unwrap();

        assert_eq!(validated.section_count(), 1);
        assert!(!validated.is_empty());
        assert_eq!(validated.sections().len(), 1);
        assert!(validated.metadata().title.is_none());
    }

    // === Display ===

    #[test]
    fn display_draft() {
        let doc = Document::new();
        assert_eq!(doc.to_string(), "Document(0 sections)");
    }

    #[test]
    fn display_validated() {
        let mut doc = Document::new();
        doc.add_section(valid_section());
        let validated = doc.validate().unwrap();
        assert_eq!(validated.to_string(), "Document(1 sections)");
    }

    // === Equality ===

    #[test]
    fn equality_draft() {
        let mut a = Document::new();
        a.add_section(valid_section());
        let mut b = Document::new();
        b.add_section(valid_section());
        assert_eq!(a, b);
    }

    #[test]
    fn equality_validated() {
        let mut a = Document::new();
        a.add_section(valid_section());
        let mut b = Document::new();
        b.add_section(valid_section());
        let va = a.validate().unwrap();
        let vb = b.validate().unwrap();
        assert_eq!(va, vb);
    }

    // === Clone ===

    #[test]
    fn clone_draft() {
        let mut doc = Document::new();
        doc.add_section(valid_section());
        let cloned = doc.clone();
        assert_eq!(doc, cloned);
    }

    #[test]
    fn clone_validated() {
        let mut doc = Document::new();
        doc.add_section(valid_section());
        let validated = doc.validate().unwrap();
        let cloned = validated.clone();
        assert_eq!(validated, cloned);
    }

    // === Serde ===

    #[test]
    fn serde_roundtrip_draft() {
        let mut doc = Document::new();
        doc.add_section(valid_section());
        doc.set_metadata(Metadata { title: Some("Test".to_string()), ..Metadata::default() });

        let json = serde_json::to_string(&doc).unwrap();
        let back: Document<Draft> = serde_json::from_str(&json).unwrap();
        assert_eq!(doc, back);
    }

    #[test]
    fn serde_roundtrip_validated_deserializes_to_draft() {
        let mut doc = Document::new();
        doc.add_section(valid_section());
        let validated = doc.validate().unwrap();

        let json = serde_json::to_string(&validated).unwrap();
        // Deserialize always produces Draft
        let back: Document<Draft> = serde_json::from_str(&json).unwrap();
        // Must re-validate
        let re_validated = back.validate().unwrap();
        assert_eq!(validated, re_validated);
    }

    #[test]
    fn serde_empty_document() {
        let doc = Document::new();
        let json = serde_json::to_string(&doc).unwrap();
        let back: Document<Draft> = serde_json::from_str(&json).unwrap();
        assert_eq!(doc, back);
    }

    // === Complex document ===

    #[test]
    fn complex_document_roundtrip() {
        use crate::control::Control;
        use crate::image::{Image, ImageFormat};
        use crate::table::{Table, TableCell, TableRow};
        use hwpforge_foundation::HwpUnit;

        let cell = TableCell::new(
            vec![Paragraph::with_runs(
                vec![Run::text("cell", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            HwpUnit::from_mm(50.0).unwrap(),
        );
        let table = Table::new(vec![TableRow::new(vec![cell])]);

        let link = Control::Hyperlink {
            text: "click".to_string(),
            url: "https://example.com".to_string(),
        };

        let img = Image::new(
            "test.png",
            HwpUnit::from_mm(10.0).unwrap(),
            HwpUnit::from_mm(10.0).unwrap(),
            ImageFormat::Png,
        );

        let section = Section::with_paragraphs(
            vec![
                Paragraph::with_runs(
                    vec![
                        Run::text("Hello ", CharShapeIndex::new(0)),
                        Run::text("world", CharShapeIndex::new(1)),
                    ],
                    ParaShapeIndex::new(0),
                ),
                Paragraph::with_runs(
                    vec![Run::table(table, CharShapeIndex::new(0))],
                    ParaShapeIndex::new(1),
                ),
                Paragraph::with_runs(
                    vec![Run::control(link, CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                ),
                Paragraph::with_runs(
                    vec![Run::image(img, CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                ),
            ],
            PageSettings::a4(),
        );

        let mut doc = Document::with_metadata(Metadata {
            title: Some("Complex Doc".to_string()),
            author: Some("Author".to_string()),
            keywords: vec!["test".to_string()],
            ..Metadata::default()
        });
        doc.add_section(section);

        let validated = doc.validate().unwrap();
        let json = serde_json::to_string_pretty(&validated).unwrap();
        let back: Document<Draft> = serde_json::from_str(&json).unwrap();
        let re_validated = back.validate().unwrap();
        assert_eq!(validated, re_validated);
    }

    // === Debug ===

    #[test]
    fn debug_output() {
        let doc = Document::new();
        let s = format!("{doc:?}");
        assert!(s.contains("Document"), "debug: {s}");
        assert!(s.contains("sections"), "debug: {s}");
    }
}
