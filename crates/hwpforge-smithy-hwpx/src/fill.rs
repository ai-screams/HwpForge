//! E2 `fill` 델타 API — 이름 붙은 누름틀(ClickHere)을 값 맵으로 채운다.
//!
//! 에이전트가 섹션 JSON 전체를 왕복하지 않고 `이름 → 값` 델타만으로
//! 문서를 채우는 상위 API. 내부적으로 preserve-first 패처
//! ([`crate::HwpxPatcher`])를 사용하므로 채워진 섹션 XML 외의 모든 ZIP
//! 엔트리는 바이트 그대로 보존된다.
//!
//! # 정책
//!
//! - **전량 preflight 후 전량 적용** (all-or-nothing): 요청 값 중 하나라도
//!   검증에 실패하면 아무 것도 쓰지 않는다.
//! - **이름 중복 거부**: 같은 `name` 의 누름틀이 여러 개면
//!   [`FillError::DuplicateFieldName`] — 자동으로 전부 채우지 않는다.
//! - **빈 값 거부**: 빈 `display_text` 는 힌트-폴백·모호-다운그레이드
//!   sentinel 로 이미 과적되어 있으므로 [`FillError::EmptyValue`].
//! - **미채움/모호 필드 거부**: `display_text` 가 빈 필드는 patch 텍스트
//!   슬롯이 없어(슬롯 거울 불변식) 채울 수 없다 —
//!   [`FillError::UnfillableField`]. 병합-run(한컴 재저장) 필드가 여기
//!   해당한다. 다중 문단·중첩·구조화 범위와 미지원 흐름도 목록에
//!   남기되 같은 오류로 명시적으로 거부한다.

use std::collections::BTreeMap;

use hwpforge_core::field_inventory::{FieldEditability, FieldInventory};
use hwpforge_core::section::Section;

use crate::error::{CodecFailure, CodecStage, HwpxError, HwpxResult};
use crate::{HwpxDecoder, HwpxPatcher, PatchError};

/// 문서 안의 누름틀 한 개에 대한 발견가능성 정보 (`fields` CLI/MCP 표면).
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct FieldInfo {
    /// 필드 이름 (`fieldBegin name="…"`). 이름 없는 누름틀은 `None` —
    /// 이름으로 지정할 수 없으므로 fill 대상이 아니다.
    pub name: Option<String>,
    /// 힌트(placeholder) 텍스트 (`Direction` 파라미터).
    pub hint: Option<String>,
    /// 현재 본문 값. 미채움 네이티브 필드는 힌트와 동일한 문자열이다.
    pub current: String,
    /// 필드가 속한 섹션 인덱스.
    pub section: usize,
    /// 현재 값 편집 계약으로 채울 수 있는지. 빈 값·다중 문단·중첩·
    /// 구조화 본문·불완전 경계·미지원 흐름은 목록에 남기되 거부한다.
    pub fillable: bool,
}

/// `fill` 로 실제 채워진 필드 하나의 기록.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct FilledField {
    /// 필드 이름.
    pub name: String,
    /// 필드가 속한 섹션 인덱스.
    pub section: usize,
    /// 채우기 전 본문 (힌트 또는 이전 값) — 감사/롤백 참고용.
    pub previous: String,
}

/// [`HwpxFiller::fill`] 의 성공 결과.
#[derive(Debug, Clone)]
pub struct FillOutcome {
    /// 채워진 HWPX 패키지 바이트. 건드리지 않은 엔트리는 원본과 동일하다.
    pub bytes: Vec<u8>,
    /// 채워진 필드 목록 (요청 순서가 아니라 문서 순서).
    pub filled: Vec<FilledField>,
}

/// `fill` preflight/적용 실패.
#[derive(Debug)]
pub enum FillError {
    /// 빈 값은 채울 수 없다 — 빈 `display_text` 는 힌트-폴백 sentinel.
    EmptyValue {
        /// 문제의 필드 이름.
        name: String,
    },
    /// 요청한 이름의 누름틀이 문서에 없다.
    UnknownField {
        /// 요청한 이름.
        name: String,
        /// 문서에 존재하는 이름 붙은 누름틀 목록 (문서 순서, 중복 제거).
        available: Vec<String>,
    },
    /// 같은 이름의 누름틀이 여러 개 — 모호하므로 거부.
    DuplicateFieldName {
        /// 중복된 이름.
        name: String,
        /// 발견된 개수.
        count: usize,
    },
    /// 본문이나 범위가 현재 값 편집 계약으로 안전하게 치환되지 않는 필드.
    UnfillableField {
        /// 필드 이름.
        name: String,
        /// 필드가 속한 섹션.
        section: usize,
    },
    /// fill 고유 workflow 거부: 문서의 필드 목록 자체가 진단을 낸 경우
    /// (필드 경계가 깨져 이름 해석을 신뢰할 수 없다).
    Workflow(HwpxError),
    /// 입력 패키지를 읽거나 디코드하지 못했다. 단계는 [`CodecStage::Decode`].
    Codec(CodecFailure),
    /// 섹션 보존 메타데이터 export 또는 preserve-first 패치가 실패했다.
    /// [`PatchError`] 가 원인(코덱 단계·거부·보호 필드·불변식)을 보존한다.
    Patch(PatchError),
    /// preflight 로 확인한 사실이 적용 중에 성립하지 않았다(확인된 필드의
    /// 섹션 경로 부재, 적용 중 필드 목록 변화). 입력이 아니라 결함이다.
    Invariant {
        /// 성립하지 않은 사실.
        detail: String,
    },
}

impl std::fmt::Display for FillError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyValue { name } => {
                write!(f, "field '{name}': empty value is not fillable (빈 값 채우기 미지원 — 비우기는 별도 연산)")
            }
            Self::UnknownField { name, available } => {
                write!(f, "field '{name}' not found; available: [{}]", available.join(", "))
            }
            Self::DuplicateFieldName { name, count } => {
                write!(f, "field '{name}' appears {count} times; ambiguous target (이름 중복 — 문서에서 이름을 유일하게 하세요)")
            }
            Self::UnfillableField { name, section } => {
                write!(
                    f,
                    "field '{name}' in section {section} has no safely patchable body (빈 본문·구조화 범위·불완전 경계 또는 미지원 흐름)"
                )
            }
            Self::Workflow(error) => write!(f, "fill workflow error: {error}"),
            Self::Codec(failure) => write!(f, "fill workflow error: {}", failure.detail),
            Self::Patch(error) => write!(f, "fill workflow error: {error}"),
            Self::Invariant { detail } => {
                write!(f, "fill workflow error: internal invariant violated: {detail}")
            }
        }
    }
}

impl std::error::Error for FillError {}

/// 이름 기반 누름틀 채우기 상위 API.
#[derive(Debug, Clone, Copy)]
pub struct HwpxFiller;

impl HwpxFiller {
    /// 문서의 모든 누름틀을 문서 순서로 나열한다 (발견가능성 표면).
    ///
    /// # Errors
    ///
    /// 패키지 디코드에 실패하면 [`HwpxError`] 를 반환한다.
    pub fn list_fields(base: &[u8]) -> HwpxResult<Vec<FieldInfo>> {
        Self::list_fields_with_diagnostics(base)
            .map(crate::diagnostics::WithDecodeWarnings::into_value)
    }

    /// 모든 누름틀을 나열하되 **디코더 경고를 함께** 돌려준다.
    ///
    /// 투영 결과는 [`HwpxFiller::list_fields`] 와 동일하다 — 그쪽이 이
    /// 함수의 얇은 래퍼라 둘이 어긋날 수 없다.
    ///
    /// # Errors
    ///
    /// 패키지 디코드에 실패하면 [`HwpxError`] 를 반환한다.
    pub fn list_fields_with_diagnostics(
        base: &[u8],
    ) -> HwpxResult<crate::diagnostics::WithDecodeWarnings<Vec<FieldInfo>>> {
        let decoded = HwpxDecoder::decode(base)?;
        let warnings = decoded.warnings.clone();
        let inventory = FieldInventory::from_sections(decoded.document.sections());
        let fields = inventory
            .fields()
            .iter()
            .filter(|f| f.kind() == "CLICK_HERE")
            .map(|f| FieldInfo {
                name: f.name().clone(),
                hint: f.hint().clone(),
                current: f.current(),
                section: f.section(),
                fillable: f.name().is_some()
                    && f.editability() == FieldEditability::Editable
                    && !f.current().is_empty(),
            })
            .collect();
        Ok(crate::diagnostics::WithDecodeWarnings::new(fields, warnings))
    }

    /// 이름 → 값 맵으로 누름틀을 채운 새 패키지 바이트를 만든다.
    ///
    /// 전량 preflight 후 전량 적용 — 어느 하나라도 실패하면 아무 것도
    /// 쓰지 않고 [`FillError`] 를 반환한다.
    ///
    /// # Errors
    ///
    /// [`FillError`] 의 각 variant 문서를 참조. 패키지의 모든 엔트리(디코더가
    /// 읽지 않는 것 포함)를 해제하므로 엔트리 하나가 50 MiB, 전체가 500 MiB 를
    /// 넘으면 거부한다.
    pub fn fill(base: &[u8], values: &BTreeMap<String, String>) -> Result<FillOutcome, FillError> {
        Self::fill_with_diagnostics(base, values)
            .map(crate::diagnostics::WithDecodeWarnings::into_value)
    }

    /// 누름틀을 채우되 **디코더 경고를 함께** 돌려준다.
    ///
    /// 바이트·`filled` 목록은 [`HwpxFiller::fill`] 과 동일하다 — 그쪽이 이
    /// 함수의 얇은 래퍼라 둘이 어긋날 수 없다. 이 함수 역시
    /// [`HwpxFiller::fill_with_codec_diagnostics`] 의 얇은 래퍼로, 인코드
    /// 채널(값에서 지운 제어 문자 경고)만 버린다.
    ///
    /// # 어느 디코드인가
    ///
    /// 이름 해석용 **입력 디코드 한 번**의 경고만 싣는다. 뒤따르는
    /// preserve 패치는 섹션마다 같은 base 를 다시 디코드하지만 그것은 이미
    /// 보고한 디코드의 재실행이므로 합치면 한 경고가 섹션 수만큼 불어난다.
    /// 그래서 경고 개수는 편집이 건드린 섹션 수와 무관하게 일정하다.
    ///
    /// # Errors
    ///
    /// [`FillError`] 의 각 variant 문서를 참조. 패키지의 모든 엔트리(디코더가
    /// 읽지 않는 것 포함)를 해제하므로 엔트리 하나가 50 MiB, 전체가 500 MiB 를
    /// 넘으면 거부한다.
    pub fn fill_with_diagnostics(
        base: &[u8],
        values: &BTreeMap<String, String>,
    ) -> Result<crate::diagnostics::WithDecodeWarnings<FillOutcome>, FillError> {
        Self::fill_with_codec_diagnostics(base, values).map(|diagnosed| {
            crate::diagnostics::WithDecodeWarnings::new(diagnosed.value, diagnosed.decode_warnings)
        })
    }

    /// 누름틀을 채우되 **디코더 경고와 인코드 경고를 함께** 돌려준다.
    ///
    /// 바이트·`filled` 목록은 [`HwpxFiller::fill`] 과 동일하다.
    /// `decode_warnings` 는 [`HwpxFiller::fill_with_diagnostics`] 와 같은 입력
    /// 디코드 한 번의 경고다.
    ///
    /// 채우기는 preserve-first 라 패키지를 재인코드하지 않는다.
    /// `encode_warnings` 는 값 자체에 대한 것뿐이다: XML 1.0 이 담을 수 없는
    /// 문자(TAB·LF·CR 을 뺀 C0 제어 문자 — 단 `<hp:fwSpace/>` 로 쓰는
    /// U+001F 는 남긴다 — 와 U+FFFE·U+FFFF)는 값에서 지우고, 지운 값마다
    /// [`EncodeWarning::XmlForbiddenCharsRemoved`](crate::EncodeWarning::XmlForbiddenCharsRemoved)
    /// 하나(`location` = `field 'NAME'`)를 싣는다. 지운 뒤의 값이 쓰기·"값이
    /// 바뀌었나" 비교·`filled` 보고에 모두 쓰이므로, 지운 뒤 현재 값과 같으면
    /// 그 필드는 채우지 않은 것으로 남는다(경고만, 바이트 그대로). 본문 CR/CRLF도
    /// 공용 인코드 규칙으로 정규화한 뒤 비교한다. 지운 뒤
    /// 비면 [`FillError::EmptyValue`]. 현재 값과 원문 그대로 같은 요청은
    /// 정리하지 않는다 — 원본이 이미 그런 문자를 담고 있어도 경고 없는 no-op 이다.
    ///
    /// # Errors
    ///
    /// [`FillError`] 의 각 variant 문서를 참조. 패키지의 모든 엔트리(디코더가
    /// 읽지 않는 것 포함)를 해제하므로 엔트리 하나가 50 MiB, 전체가 500 MiB 를
    /// 넘으면 거부한다.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::collections::BTreeMap;
    /// use hwpforge_smithy_hwpx::HwpxFiller;
    ///
    /// let base = std::fs::read("form.hwpx")?;
    /// let values = BTreeMap::from([("성명".to_string(), "홍길동".to_string())]);
    /// let diagnosed = HwpxFiller::fill_with_codec_diagnostics(&base, &values)?;
    /// for warning in &diagnosed.encode_warnings {
    ///     eprintln!("{warning}");
    /// }
    /// std::fs::write("filled.hwpx", &diagnosed.value.bytes)?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn fill_with_codec_diagnostics(
        base: &[u8],
        values: &BTreeMap<String, String>,
    ) -> Result<crate::diagnostics::WithCodecWarnings<FillOutcome>, FillError> {
        // ── preflight 1: 값 자체 검증 (XML 이 담을 수 없는 문자를 지운 뒤) ──
        for (name, value) in values {
            if value.chars().all(crate::wire_xml::is_edit_unrepresentable_char) {
                return Err(FillError::EmptyValue { name: name.clone() });
            }
        }

        let mut decoded = HwpxDecoder::decode(base)
            .map_err(|error| FillError::Codec(CodecFailure::hwpx(CodecStage::Decode, &error)))?;
        let decode_warnings = decoded.warnings.clone();

        let original_sections: Vec<Section> = decoded.document.sections().to_vec();
        let inventory = FieldInventory::from_sections(&original_sections);
        if let Some(error) = inventory.diagnostics().first() {
            return Err(FillError::Workflow(HwpxError::InvalidStructure {
                detail: error.to_string(),
            }));
        }
        let fields: Vec<_> =
            inventory.fields().iter().filter(|f| f.kind() == "CLICK_HERE").collect();
        for name in values.keys() {
            let matches: Vec<_> =
                fields.iter().filter(|f| f.name().as_ref() == Some(name)).collect();
            match matches.as_slice() {
                [] => {
                    return Err(FillError::UnknownField {
                        name: name.clone(),
                        available: {
                            let mut seen = std::collections::HashSet::new();
                            fields
                                .iter()
                                .filter_map(|f| f.name().as_ref())
                                .filter(|name| seen.insert(name.as_str()))
                                .cloned()
                                .collect()
                        },
                    })
                }
                [field]
                    if field.editability() != FieldEditability::Editable
                        || field.current().is_empty() =>
                {
                    return Err(FillError::UnfillableField {
                        name: name.clone(),
                        section: field.section(),
                    })
                }
                [_] => {}
                _ => {
                    return Err(FillError::DuplicateFieldName {
                        name: name.clone(),
                        count: matches.len(),
                    })
                }
            }
        }
        let mut encode_warnings = Vec::new();
        let package = crate::patch::RawPackage::read(base)
            .map_err(|error| FillError::Codec(CodecFailure::hwpx(CodecStage::Decode, &error)))?;
        let mut filled = Vec::new();
        let mut touched = std::collections::BTreeSet::new();
        let mut pending = Vec::new();
        for field in fields {
            let Some(name) = &field.name() else {
                continue;
            };
            let Some(value) = values.get(name) else {
                continue;
            };
            let previous = field.current();
            if value == &previous {
                continue;
            }
            // The field came from decoding these bytes, so its section exists.
            let section_path = package
                .section_path(field.section())
                .map_err(|error| FillError::Invariant { detail: error.to_string() })?;
            let value = crate::wire_xml::sanitize_edit_text(
                value,
                &section_path,
                &format!("field '{name}'"),
                &mut encode_warnings,
            );
            let value = crate::wire_xml::body_text_line_breaks(&value, true).into_owned();
            if value == previous {
                continue;
            }
            pending.push((field, value));
            touched.insert(field.section());
            filled.push(FilledField { name: name.clone(), section: field.section(), previous });
        }

        let edits: Vec<_> = pending.iter().map(|(f, value)| (*f, value.as_str())).collect();
        if !FieldInventory::replace_values(decoded.document.sections_mut(), &edits) {
            return Err(FillError::Invariant {
                detail: "field inventory changed during fill preflight".into(),
            });
        }
        let mut bytes = base.to_vec();
        for section_idx in touched {
            // preservation 은 원본 섹션 상태로 export 해야 sha/슬롯이 맞는다.
            // 앞선 patch 는 다른 섹션 파일만 바꾸므로 현재 bytes 의 이 섹션
            // XML 은 base 와 동일하다 — 체이닝이 안전한 근거.
            let preservation = HwpxPatcher::export_section_preservation(
                &bytes,
                section_idx,
                &original_sections[section_idx],
            )
            .map_err(FillError::Patch)?;
            bytes = HwpxPatcher::patch_section_preserving(
                &bytes,
                section_idx,
                &decoded.document.sections()[section_idx],
                None,
                Some(&preservation),
            )
            .map_err(FillError::Patch)?;
        }

        // The patcher invalidates caches from the actual changed text locators.
        // 이것을 실패시키는 것: 별도 wire 필드 개수로 캐시를 다시 해석하기.
        Ok(crate::diagnostics::WithCodecWarnings::new(
            FillOutcome { bytes, filled },
            decode_warnings,
            encode_warnings,
        ))
    }
}

#[cfg(test)]
mod tests {
    //! 채운 누름틀의 문단 캐시를 wire 에서 찾는 경로의 테스트.
    //!
    //! 캐시 표지: [`add_caches`] 가 모든 `</hp:p>` 앞에 `vertpos` = 9000 + (닫히는
    //! 순서) 인 `linesegarray` 를 넣는다. 안쪽 문단이 먼저 닫히므로 번호는
    //! 구조에서 정해진다 — 각 테스트 주석에 어느 번호가 어느 문단인지 적었다.

    use super::*;
    use crate::patch::RawPackage;
    use hwpforge_core::caption::{Caption, CaptionSide};
    use hwpforge_core::document::Document;
    use hwpforge_core::image::{Image, ImageFormat, ImageStore};
    use hwpforge_core::run::Run;
    use hwpforge_core::table::{Table, TableCell, TableRow};
    use hwpforge_core::PageSettings;
    use hwpforge_core::{Control, Paragraph, RunContent};
    use hwpforge_foundation::FieldType;
    use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};

    /// 한컴 저장 누름틀 하나(`user_email`, 캐시 `vertpos="0"`)가 있는 문서.
    const NAMED: &[u8] = include_bytes!("../../../tests/fixtures/fields/clickhere_named.hwpx");

    fn values(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn field(name: &str, current: &str) -> RunContent {
        RunContent::Control(Box::new(Control::Field {
            field_type: FieldType::ClickHere,
            hint_text: Some(current.to_string()),
            help_text: None,
            name: Some(name.to_string()),
            display_text: current.to_string(),
        }))
    }

    fn text(value: &str) -> RunContent {
        RunContent::Text(value.to_string())
    }

    fn para(contents: Vec<RunContent>) -> Paragraph {
        let mut paragraph = Paragraph::new(ParaShapeIndex::new(0));
        for content in contents {
            paragraph.runs.push(Run { content, char_shape_id: CharShapeIndex::new(0) });
        }
        paragraph
    }

    fn control(control: Control) -> RunContent {
        RunContent::Control(Box::new(control))
    }

    fn size(value: i32) -> HwpUnit {
        HwpUnit::new(value).expect("unit")
    }

    fn encode(paragraphs: Vec<Paragraph>, images: &ImageStore) -> Vec<u8> {
        let mut doc = Document::new();
        let mut section = Section::new(PageSettings::default());
        section.paragraphs = paragraphs;
        doc.add_section(section);
        let validated = doc.validate().expect("validate");
        let styles = crate::style_store_for_preset("default").expect("preset");
        crate::HwpxEncoder::encode(&validated, &styles, images).expect("encode")
    }

    fn section_xml(bytes: &[u8]) -> String {
        RawPackage::read(bytes)
            .expect("read")
            .read_text_entry("Contents/section0.xml")
            .expect("xml")
    }

    fn with_section_xml(bytes: &[u8], xml: String) -> Vec<u8> {
        let mut pkg = RawPackage::read(bytes).expect("read");
        pkg.replace_text_entry("Contents/section0.xml", xml);
        pkg.write().expect("write")
    }

    /// 모든 `</hp:p>` 앞에 닫히는 순서 번호를 `vertpos` 로 단 캐시를 넣는다.
    fn add_caches(xml: &str) -> String {
        let mut out = String::with_capacity(xml.len());
        for (k, piece) in xml.split("</hp:p>").enumerate() {
            if k > 0 {
                out.push_str("</hp:p>");
            }
            out.push_str(piece);
            if xml[..].matches("</hp:p>").count() > k {
                out.push_str(&format!(
                    "<hp:linesegarray><hp:lineseg textpos=\"0\" vertpos=\"{}\" vertsize=\"1000\" \
                     textheight=\"1000\" baseline=\"850\" spacing=\"600\" horzpos=\"0\" \
                     horzsize=\"42520\" flags=\"393216\"/></hp:linesegarray>",
                    9000 + k
                ));
            }
        }
        out
    }

    /// 남아 있는 캐시 표지 번호 (9000 기준 오프셋).
    fn caches(xml: &str) -> Vec<usize> {
        (0..64).filter(|k| xml.contains(&format!("vertpos=\"{}\"", 9000 + k))).collect()
    }

    fn fill_xml(bytes: &[u8], pairs: &[(&str, &str)]) -> Result<String, FillError> {
        let outcome = HwpxFiller::fill(bytes, &values(pairs))?;
        HwpxDecoder::decode(&outcome.bytes).expect("filled output decodes");
        Ok(section_xml(&outcome.bytes))
    }

    fn named_with(xml_edit: impl Fn(&str) -> String) -> Vec<u8> {
        let xml = section_xml(NAMED);
        let edited = xml_edit(&xml);
        assert_ne!(edited, xml, "test edit must change the fixture");
        with_section_xml(NAMED, edited)
    }

    #[test]
    fn equivalent_name_spellings_reach_the_filled_paragraph() {
        // 이것을 실패시키는 것: 이름을 문자열 needle(`name="…"`)로 찾기 —
        // 엔티티·작은따옴표·`=` 공백·속성 안 `>` 를 못 찾아 캐시가 남는다.
        // 이름을 정규화 없이 raw 로 비교 → 엔티티 행이 위치 불일치 오류.
        // `normalized_value` 대신 엔티티만 푸는 `quick_xml::escape::unescape`(속성값
        // 공백 정규화 없음) → 날 탭 행이 위치 불일치 오류(디코더는 날 탭을
        // 공백으로 읽는다).
        let cases: [(&str, &str); 10] = [
            ("name=\"user_email\"", "user_email"),
            ("name=\"a&amp;b\"", "a&b"),
            ("name=\"a&#38;b\"", "a&b"),
            ("name=\"a&quot;b\"", "a\"b"),
            ("name=\"a&lt;b\"", "a<b"),
            ("name=\"a>b\"", "a>b"),
            ("name='user_email'", "user_email"),
            ("name = \"user_email\"", "user_email"),
            ("name=\"a&#9;b\"", "a\tb"),
            ("name=\"a\tb\"", "a b"),
        ];
        for (wire, decoded) in cases {
            let bytes = if wire == "name=\"user_email\"" {
                NAMED.to_vec()
            } else {
                named_with(|xml| xml.replacen("name=\"user_email\"", wire, 1))
            };
            let listed = HwpxFiller::list_fields(&bytes).expect("list");
            assert_eq!(listed[0].name.as_deref(), Some(decoded), "fixture premise for {wire}");
            let xml = fill_xml(&bytes, &[(decoded, "채움")])
                .unwrap_or_else(|e| panic!("{wire}: fill failed: {e}"));
            assert!(!xml.contains("<hp:linesegarray"), "{wire}: filled paragraph cache must go");
        }
    }

    #[test]
    fn same_named_hyperlink_before_the_field_keeps_its_cache() {
        // 앞 문단 = 같은 이름의 HYPERLINK(캐시 vertpos 0), 뒤 문단 = 실제
        // 누름틀(캐시 vertpos 777).
        // 이것을 실패시키는 것: `type="CLICK_HERE"` 필터 제거 → wire 쪽 후보가
        // 둘이 되어 위치 불일치 오류.
        let bytes = named_with(|xml| {
            let start = xml.find("<hp:run charPrIDRef=\"0\"><hp:ctrl><hp:fieldBegin").unwrap();
            let end = xml[start..].find("</hp:run>").unwrap() + start + "</hp:run>".len();
            let run = xml[start..end].replace("1000000000", "1000000001");
            let new_para = format!(
                "<hp:p id=\"1\" paraPrIDRef=\"0\" styleIDRef=\"0\" pageBreak=\"0\" \
                 columnBreak=\"0\" merged=\"0\">{run}<hp:linesegarray><hp:lineseg textpos=\"0\" \
                 vertpos=\"777\" vertsize=\"1000\" textheight=\"1000\" baseline=\"850\" \
                 spacing=\"600\" horzpos=\"0\" horzsize=\"42520\" flags=\"393216\"/>\
                 </hp:linesegarray></hp:p>"
            );
            xml.replacen(
                "type=\"CLICK_HERE\" name=\"user_email\"",
                "type=\"HYPERLINK\" name=\"user_email\"",
                1,
            )
            .replace("</hs:sec>", &format!("{new_para}</hs:sec>"))
        });
        let xml = fill_xml(&bytes, &[("user_email", "x@y.z")]).expect("fill");
        assert!(xml.contains("vertpos=\"0\""), "hyperlink paragraph cache must stay");
        assert!(!xml.contains("vertpos=\"777\""), "filled paragraph cache must go");
    }

    #[test]
    fn cell_cache_inside_the_filled_paragraph_is_left_alone() {
        // 9000 = 셀 문단(먼저 닫힘), 9001 = 필드와 표가 든 바깥 문단.
        // 이것을 실패시키는 것: 캐시를 그것의 부모 문단이 아니라 가장 바깥
        // 문단에 귀속 → 셀 캐시 9000 이 지워진다.
        let cell = TableCell::new(vec![para(vec![text("셀")])], size(4000));
        let table = Table::new(vec![TableRow::new(vec![cell])]);
        let doc = encode(
            vec![para(vec![field("f", "입력"), RunContent::Table(Box::new(table))])],
            &ImageStore::default(),
        );
        let bytes = with_section_xml(&doc, add_caches(&section_xml(&doc)));
        let xml = fill_xml(&bytes, &[("f", "값")]).expect("fill");
        assert_eq!(caches(&xml), vec![0]);
    }

    #[test]
    fn two_filled_fields_in_one_paragraph_remove_its_cache_once() {
        // 9000 = 필드 둘이 든 문단, 9001 = 다음 문단.
        // 이것을 실패시키는 것: 같은 구간을 두 번 모으기(`stale_caches` 가 구간을
        // 겹쳐 내면 두 번째 제거가 다른 바이트를 깎는다).
        let doc = encode(
            vec![para(vec![field("a", "가"), field("b", "나")]), para(vec![text("다음")])],
            &ImageStore::default(),
        );
        let bytes = with_section_xml(&doc, add_caches(&section_xml(&doc)));
        let xml = fill_xml(&bytes, &[("a", "1"), ("b", "2")]).expect("fill");
        // W2: 다음 문단(9001)도 채운 문단 뒤 같은 흐름이라 누적 vertpos 가
        // 낡는다 — 전에는 남겼던 것이 틀렸다.
        assert_eq!(caches(&xml), Vec::<usize>::new());
        assert!(xml.contains("<hp:t>다음</hp:t>"), "the next paragraph's text is intact");
    }

    /// W2 (design 3.5 tests 1, 3): filling a field in the middle of three
    /// cell paragraphs drops that paragraph's cache, the cell paragraph after
    /// it, the table's host paragraph and the body paragraph after the host;
    /// the first cell paragraph and the body paragraph before keep theirs.
    /// A value of the same length does the same (line counts are not judged
    /// — that would need layout).
    /// 이것을 실패시키는 것: 꼬리 대신 문단 하나만(9003 이 남는다) · host 사슬
    /// 생략(9004·9005 가 남는다) · 목록 처음부터(9001 이 간다).
    #[test]
    fn cell_fill_drops_the_flow_tail_and_the_host_chain() {
        let cell = TableCell::new(
            vec![para(vec![text("c0")]), para(vec![field("mid", "칸")]), para(vec![text("c2")])],
            size(4000),
        );
        let table = Table::new(vec![TableRow::new(vec![cell])]);
        let doc = encode(
            vec![
                para(vec![text("앞")]),
                para(vec![RunContent::Table(Box::new(table))]),
                para(vec![text("뒤")]),
            ],
            &ImageStore::default(),
        );
        // close order: 앞 9000, c0 9001, mid 9002, c2 9003, host 9004, 뒤 9005.
        let bytes = with_section_xml(&doc, add_caches(&section_xml(&doc)));
        for value in ["여러 줄로 길어진 값 여러 줄로 길어진 값 여러 줄로 길어진 값", "값"]
        {
            let xml = fill_xml(&bytes, &[("mid", value)]).expect("fill");
            assert_eq!(caches(&xml), vec![0, 1], "{value}");
        }
    }

    /// Per container, filling the middle of three paragraphs
    /// drops exactly that paragraph, the one after it, the host paragraph
    /// and the body paragraph after the host — every other cache stays.
    /// 이것을 실패시키는 것: host 사슬 생략 · 꼬리 대신 문단 하나만 · 목록
    /// 처음부터 — 컨테이너마다.
    #[test]
    fn each_container_drops_its_flow_tail_and_host_chain() {
        let inner =
            || vec![para(vec![text("x0")]), para(vec![field("mid", "칸")]), para(vec![text("x2")])];
        let containers: Vec<(&str, RunContent)> = vec![
            (
                "cell",
                RunContent::Table(Box::new(Table::new(vec![TableRow::new(vec![TableCell::new(
                    inner(),
                    size(4000),
                )])]))),
            ),
            ("text box", control(Control::text_box(inner(), size(4000), size(2000)))),
            (
                "table caption",
                RunContent::Table(Box::new(
                    Table::new(vec![TableRow::new(vec![TableCell::new(
                        vec![para(vec![text("칸")])],
                        size(4000),
                    )])])
                    .with_caption(Caption::new(inner(), CaptionSide::Bottom)),
                )),
            ),
            ("footnote", control(Control::footnote(inner()))),
        ];
        for (name, content) in containers {
            let doc = encode(
                vec![para(vec![text("앞")]), para(vec![content]), para(vec![text("뒤")])],
                &ImageStore::default(),
            );
            let xml = add_caches(&section_xml(&doc));
            // Close order of a paragraph = `</hp:p>` count before it closes.
            let leaf =
                |needle: &str| xml[..xml.find(needle).expect(needle)].matches("</hp:p>").count();
            let host = leaf("<hp:t>뒤</hp:t>") - 1;
            let dropped =
                [leaf("name=\"mid\""), leaf("<hp:t>x2</hp:t>"), host, leaf("<hp:t>뒤</hp:t>")];
            let all = caches(&xml);
            let expected: Vec<usize> =
                all.iter().copied().filter(|k| !dropped.contains(k)).collect();
            let bytes = with_section_xml(&doc, xml.clone());
            let filled = fill_xml(&bytes, &[("mid", "여러 줄로 길어진 값")])
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(caches(&filled), expected, "{name}");
            assert!(expected.contains(&leaf("<hp:t>x0</hp:t>")), "{name}: x0 keeps its cache");
        }
    }

    #[test]
    fn filled_paragraph_without_cache_is_a_no_op_for_the_cache() {
        // 이것을 실패시키는 것: "캐시 없음"(직접 자식 캐시 0개)을 오류로 처리.
        let doc = encode(vec![para(vec![field("f", "입력")])], &ImageStore::default());
        let xml = fill_xml(&doc, &[("f", "값")]).expect("fill");
        assert!(!xml.contains("<hp:linesegarray"));
    }

    #[test]
    fn same_named_field_inside_a_group_is_visible_and_ambiguous() {
        // 이것을 실패시키는 것: 편집 미지원 흐름의 필드를 이름 해석에서 숨기기.
        let inner = Control::text_box(vec![para(vec![field("dup", "안")])], size(4000), size(2000));
        let group = Control::Group {
            frame: hwpforge_core::ObjectFrame::new(),
            children: vec![hwpforge_core::control::GroupChild::Shape(inner)],
            width: size(4000),
            height: size(2000),
            placement: None,
            inst_id: None,
        };
        let doc = encode(
            vec![para(vec![control(group)]), para(vec![field("dup", "밖")])],
            &ImageStore::default(),
        );
        let bytes = with_section_xml(&doc, add_caches(&section_xml(&doc)));
        let listed = HwpxFiller::list_fields(&bytes).expect("list");
        assert_eq!(listed.len(), 2);
        assert!(listed.iter().any(|f| !f.fillable));
        // 이것을 실패시키는 것: 같은 이름을 available 목록에 중복 보고하기.
        let Err(FillError::UnknownField { available, .. }) =
            fill_xml(&bytes, &[("missing", "value")])
        else {
            panic!("unknown field");
        };
        assert_eq!(available, ["dup"]);

        assert!(matches!(
            fill_xml(&bytes, &[("dup", "값")]),
            Err(FillError::DuplicateFieldName { .. })
        ));
    }

    #[test]
    fn paragraph_attributes_split_across_lines_are_found() {
        // 이것을 실패시키는 것: 문단 시작을 `<hp:p` + 공백/`>` 문자열로 찾던
        // 옛 스캐너 — 개행으로 나뉜 속성(`<hp:p\n id=…>`)을 문단으로 보지 못해
        // 캐시가 남는다.
        let bytes = named_with(|xml| xml.replace("<hp:p id=", "<hp:p\n id="));
        let xml = fill_xml(&bytes, &[("user_email", "x@y.z")]).expect("fill");
        assert!(!xml.contains("<hp:linesegarray"));
    }

    #[test]
    fn ellipse_inner_and_caption_fields_match_by_structure_not_order() {
        // 인벤토리는 타원 내부 문단(A)을 캡션(B)보다 먼저 방문하고, XML 은
        // 캡션을 먼저 쓴다 — 문서 순서로는 [B, A], 인벤토리 순서로는 [A, B].
        // 이것을 실패시키는 것: 구조 키 대신 문서 순서 목록으로 짝짓기 →
        // 이름이 엇갈려 정상 문서를 위치 불일치로 거부한다.
        let mut ellipse =
            Control::ellipse_with_text(size(6000), size(3000), vec![para(vec![field("A", "가")])]);
        if let Control::Ellipse { caption, .. } = &mut ellipse {
            *caption = Some(Caption::new(vec![para(vec![field("B", "나")])], CaptionSide::Bottom));
        }
        let doc = encode(vec![para(vec![control(ellipse)])], &ImageStore::default());
        let xml = section_xml(&doc);
        let caption_at = xml.find("<hp:caption").expect("caption");
        let inner_at = xml.find("<hp:drawText").expect("drawText");
        assert!(caption_at < inner_at, "premise: the caption is written before the inner text");
        // 9000 = 캡션 문단, 9001 = 내부 문단, 9002 = 타원이 든 본문 문단.
        let bytes = with_section_xml(&doc, add_caches(&xml));
        let only_a = fill_xml(&bytes, &[("A", "1")]).expect("fill A");
        // W2: 내부 문단(9001)의 host 는 타원이 든 본문 문단(9002) — 상자가 글에
        // 맞춰 자랄 수 있으므로 host 사슬로 함께 지운다(전에는 남겼다). 캡션(9000)은
        // 다른 흐름이라 남는다.
        assert_eq!(caches(&only_a), vec![0]);
        let filled = HwpxFiller::fill(&bytes, &values(&[("A", "1"), ("B", "2")])).expect("fill");
        assert_eq!(caches(&section_xml(&filled.bytes)), Vec::<usize>::new());
        let listed = HwpxFiller::list_fields(&filled.bytes).expect("list");
        let current: Vec<(Option<&str>, &str)> =
            listed.iter().map(|f| (f.name.as_deref(), f.current.as_str())).collect();
        assert_eq!(current, vec![(Some("A"), "1"), (Some("B"), "2")]);
    }

    #[test]
    fn unclosed_click_here_remains_listed_but_cannot_be_filled() {
        // The earlier closed field stays editable. A later source start with
        // the same name is listed as protected, so name-based fill is refused.
        let bytes = named_with(|xml| {
            let open = xml.find("<hp:fieldBegin").unwrap();
            let close =
                xml[open..].find("</hp:fieldBegin>").unwrap() + open + "</hp:fieldBegin>".len();
            let begin = xml[open..close].replace("1000000000", "1000000009");
            let new_para = format!(
                "<hp:p id=\"1\" paraPrIDRef=\"0\" styleIDRef=\"0\" pageBreak=\"0\" \
                 columnBreak=\"0\" merged=\"0\"><hp:run charPrIDRef=\"0\"><hp:ctrl>{begin}\
                 </hp:ctrl></hp:run><hp:run charPrIDRef=\"0\"><hp:t>본문</hp:t></hp:run>\
                 <hp:linesegarray><hp:lineseg textpos=\"0\" vertpos=\"777\" vertsize=\"1000\" \
                 textheight=\"1000\" baseline=\"850\" spacing=\"600\" horzpos=\"0\" \
                 horzsize=\"42520\" flags=\"393216\"/></hp:linesegarray></hp:p>"
            );
            xml.replace("</hs:sec>", &format!("{new_para}</hs:sec>"))
        });
        let listed = HwpxFiller::list_fields(&bytes).expect("list");
        assert_eq!(listed.len(), 2, "recovery ranges stay discoverable");
        assert!(listed[0].fillable);
        assert!(!listed[1].fillable);
        assert!(fill_xml(&bytes, &[("user_email", "x@y.z")]).is_err());
    }

    #[test]
    fn a_second_begin_in_the_run_replaces_the_pending_one() {
        // 같은 run 에서 `fieldBegin`(ghost) 다음 `fieldBegin`(user_email) 이 오면
        // 디코더는 뒤의 것과 `fieldEnd` 를 짝짓는다.
        // 이것을 실패시키는 것: 짝을 기다리는 begin 이 있으면 새 begin 을 무시
        // → wire 이름이 ghost 가 되어 위치 불일치 오류.
        let bytes = named_with(|xml| {
            let open = xml.find("<hp:ctrl><hp:fieldBegin").unwrap();
            let ghost = "<hp:ctrl><hp:fieldBegin id=\"1000000007\" type=\"CLICK_HERE\" \
                         name=\"ghost\" editable=\"1\" dirty=\"0\" zorder=\"-1\" \
                         fieldid=\"627272811\"/></hp:ctrl>";
            format!("{}{ghost}{}", &xml[..open], &xml[open..])
        });
        let listed = HwpxFiller::list_fields(&bytes).expect("list");
        assert_eq!(listed.len(), 2);
        assert!(fill_xml(&bytes, &[("user_email", "x@y.z")]).is_err());
    }

    #[test]
    fn picture_caption_fields_survive_an_empty_image_reference() {
        // 그림 캡션 문단 = 9000, 그림이 든 문단 = 9001, 본문 필드 문단 = 9002.
        // 이것을 실패시키는 것: 빈 참조 그림을 버리기(캡션 필드 cap 소실).
        // 자산이 없어도 그림 틀과 캡션은 남아 필드 채우기가 가능하다.
        let mut images = ImageStore::new();
        images.insert("image1.png", vec![0x89, b'P', b'N', b'G']);
        let image = Image::new("BinData/image1.png", size(2000), size(2000), ImageFormat::Png)
            .with_caption(Caption::new(vec![para(vec![field("cap", "캡")])], CaptionSide::Bottom));
        let doc = encode(
            vec![para(vec![RunContent::Image(image)]), para(vec![field("body", "본")])],
            &images,
        );
        let xml = add_caches(&section_xml(&doc));
        let bytes = with_section_xml(&doc, xml.clone());
        let filled = fill_xml(&bytes, &[("cap", "1")]).expect("fill caption field");
        // W2: 캡션이 자라면 그림이 든 문단(9001, host)과 그 뒤 본문 문단(9002)이
        // 밀린다 — 전에는 둘 다 남겼다.
        assert_eq!(caches(&filled), Vec::<usize>::new());

        let start = xml.find("binaryItemIDRef=\"").expect("img ref") + "binaryItemIDRef=\"".len();
        let end = xml[start..].find('"').unwrap() + start;
        let assetless = with_section_xml(&doc, format!("{}{}", &xml[..start], &xml[end..]));
        let listed = HwpxFiller::list_fields(&assetless).expect("list");
        assert_eq!(listed.len(), 2, "assetless frame keeps its caption field");
        let filled = fill_xml(&assetless, &[("cap", "1")]).expect("fill assetless caption");
        assert_eq!(caches(&filled), Vec::<usize>::new());
        let filled = fill_xml(&assetless, &[("body", "1")]).expect("fill body field");
        assert_eq!(caches(&filled), vec![0, 1]);
    }

    #[test]
    fn arc_draw_text_is_out_of_scope() {
        // `hasArcPr="1"` 타원은 호(Control::Arc)라 디코더가 내부 문단을 버린다.
        // 이것을 실패시키는 것: `hasArcPr` 를 보지 않고 타원 `drawText` 를 늘
        // 범위 안으로 두기 → wire 후보가 하나 많아 위치 불일치 오류.
        let ellipse =
            Control::ellipse_with_text(size(6000), size(3000), vec![para(vec![field("in", "가")])]);
        let doc = encode(
            vec![para(vec![control(ellipse)]), para(vec![field("body", "본")])],
            &ImageStore::default(),
        );
        let xml = section_xml(&doc);
        assert!(xml.contains("hasArcPr=\"0\""), "premise: the encoder writes hasArcPr");
        let bytes = with_section_xml(
            &doc,
            add_caches(&xml.replacen("hasArcPr=\"0\"", "hasArcPr=\"1\"", 1)),
        );
        let listed = HwpxFiller::list_fields(&bytes).expect("list");
        assert_eq!(listed.len(), 1, "premise: the arc's inner text is not in the inventory");
        let filled = fill_xml(&bytes, &[("body", "1")]).expect("fill");
        // 9000 = 호 내부 문단, 9001 = 호가 든 문단, 9002 = 본문 필드 문단.
        assert_eq!(caches(&filled), vec![0, 1]);
    }

    #[test]
    fn every_container_fill_visits_maps_to_its_own_paragraph() {
        // fill 인벤토리가 방문하는 컨테이너마다 필드 하나를 두고 하나씩 채운다.
        // 기대 캐시(독립 판정): 각 필드 문단에는 필드만 있으므로 그 문단은
        // `fieldBegin` 뒤 첫 `</hp:p>` 에서 닫힌다 — 표지 번호 = 그 앞의
        // `</hp:p>` 개수.
        // 이것을 실패시키는 것: 범위 표의 갈래 하나를 범위 밖으로 바꾸기
        // (`tc`·표 `caption`·`footNote`·`endNote`·`rect`/`polygon` 의 `drawText`·
        // 도형 `caption`·`subList`) → 그 필드가 wire 에서 빠져 위치 불일치 오류.
        let with_caption = |mut control: Control, name: &str| {
            let caption = Caption::new(vec![para(vec![field(name, "캡")])], CaptionSide::Bottom);
            match &mut control {
                Control::TextBox { caption: c, .. }
                | Control::Polygon { caption: c, .. }
                | Control::Rect { caption: c, .. }
                | Control::Line { caption: c, .. }
                | Control::Curve { caption: c, .. } => *c = Some(caption),
                _ => panic!("captionable shape"),
            }
            control
        };
        let point = hwpforge_core::control::ShapePoint::new;
        let mut polygon =
            Control::polygon(vec![point(0, 0), point(3000, 0), point(0, 3000)]).expect("polygon");
        if let Control::Polygon { paragraphs, .. } = &mut polygon {
            *paragraphs = vec![para(vec![field("poly", "다")])];
        }
        let cell = |name: &str| TableCell::new(vec![para(vec![field(name, "칸")])], size(4000));
        let inner = Table::new(vec![TableRow::new(vec![cell("inner_cell")])]);
        let outer_cell =
            TableCell::new(vec![para(vec![RunContent::Table(Box::new(inner))])], size(4000));
        let table = Table::new(vec![TableRow::new(vec![cell("cell"), outer_cell])]).with_caption(
            Caption::new(vec![para(vec![field("table_caption", "표")])], CaptionSide::Bottom),
        );
        let doc = encode(
            vec![
                para(vec![field("body", "본")]),
                para(vec![RunContent::Table(Box::new(table))]),
                para(vec![control(Control::footnote(vec![para(vec![field("foot", "각")])]))]),
                para(vec![control(Control::endnote(vec![para(vec![field("end", "미")])]))]),
                para(vec![control(with_caption(
                    Control::text_box(vec![para(vec![field("box", "글")])], size(4000), size(2000)),
                    "box_caption",
                ))]),
                para(vec![control(with_caption(polygon, "poly_caption"))]),
                para(vec![control(with_caption(
                    Control::line(point(0, 0), point(3000, 0)).expect("line"),
                    "line_caption",
                ))]),
                para(vec![control(with_caption(
                    Control::curve(vec![point(0, 0), point(1500, 1500), point(3000, 0)])
                        .expect("curve"),
                    "curve_caption",
                ))]),
            ],
            &ImageStore::default(),
        );
        let xml = add_caches(&section_xml(&doc));
        let bytes = with_section_xml(&doc, xml.clone());
        let listed = HwpxFiller::list_fields(&bytes).expect("list");
        let names: Vec<String> = listed.iter().filter_map(|f| f.name.clone()).collect();
        assert_eq!(names.len(), 12, "premise: every field is in the inventory: {names:?}");
        let all: Vec<usize> = caches(&xml);
        for name in &names {
            let at = xml.find(&format!("name=\"{name}\"")).expect("field on the wire");
            let own = xml[..at].matches("</hp:p>").count();
            let filled = fill_xml(&bytes, &[(name.as_str(), "값")])
                .unwrap_or_else(|e| panic!("{name}: fill failed: {e}"));
            // W2: 흐름 꼬리와 host 사슬도 가므로 정확한 남은 집합은 그 규칙의
            // 전용 시험(`cell_fill_drops_the_flow_tail_and_the_host_chain`)이 맡고,
            // 여기서는 짝지은 문단의 캐시가 가고 첫 본문 문단(9000, `body`)은
            // 다른 필드일 때 남는지만 본다.
            let left = caches(&filled);
            assert!(!left.contains(&own), "{name}: its own paragraph cache goes: {left:?}");
            assert!(left.iter().all(|k| all.contains(k)), "{name}");
            if name != "body" {
                assert!(left.contains(&0), "{name}: the body paragraph before it keeps its cache");
            }
        }
    }

    #[test]
    fn cache_with_another_prefix_is_still_the_paragraph_cache() {
        // 디코더(serde)는 접두어 없는 이름으로 읽으므로 `p:linesegarray` 도
        // 문단 캐시다.
        // 이것을 실패시키는 것: 캐시를 접두어 포함 이름(`hp:linesegarray`)으로만
        // 판정 → `p:` 캐시가 남는다.
        let bytes = named_with(|xml| {
            xml.replacen(
                "<hs:sec ",
                "<hs:sec xmlns:p=\"http://www.hancom.co.kr/hwpml/2011/paragraph\" ",
                1,
            )
            .replace("hp:linesegarray", "p:linesegarray")
            .replace("<hp:lineseg ", "<p:lineseg ")
        });
        let decoded = HwpxDecoder::decode(&bytes).expect("decode");
        assert!(
            decoded.document.sections()[0].paragraphs.iter().any(|p| p.layout_cache.is_some()),
            "premise: the decoder reads the prefixed cache"
        );
        let xml = fill_xml(&bytes, &[("user_email", "x@y.z")]).expect("fill");
        assert!(!xml.contains("linesegarray"), "prefixed cache must go: {xml}");
    }

    #[test]
    fn self_closing_field_begin_is_a_fill_target() {
        // 이것을 실패시키는 것: `Empty` 이벤트의 `fieldBegin` 을 무시(Start 만
        // 처리) → wire 에서 필드가 빠져 위치 불일치 오류.
        let bytes = named_with(|xml| {
            let open = xml.find("<hp:fieldBegin").unwrap();
            let head_end = xml[open..].find('>').unwrap() + open;
            let close = xml[open..].find("</hp:fieldBegin>").unwrap() + open;
            format!("{}/>{}", &xml[..head_end], &xml[close + "</hp:fieldBegin>".len()..])
        });
        assert!(
            section_xml(&bytes).contains("metaTag=\"\"/>"),
            "premise: fieldBegin is self-closing"
        );
        let listed = HwpxFiller::list_fields(&bytes).expect("list");
        assert_eq!(listed.len(), 1, "premise: the decoder pairs the self-closing begin");
        let xml = fill_xml(&bytes, &[("user_email", "x@y.z")]).expect("fill");
        assert!(!xml.contains("<hp:linesegarray"));
    }

    #[test]
    fn named_field_after_an_unnamed_one_keeps_its_ordinal() {
        // 9000 = 이름 없는 필드와 `b` 가 든 문단, 9001 = 다음 문단.
        // 이것을 실패시키는 것: 이름 없는 ClickHere 를 wire 쪽(또는 인벤토리 쪽)
        // 목록에서 빼기 → 같은 경로 안 순번이 어긋나 위치 불일치 오류.
        let unnamed = RunContent::Control(Box::new(Control::Field {
            field_type: FieldType::ClickHere,
            hint_text: Some("무명".to_string()),
            help_text: None,
            name: None,
            display_text: "무명".to_string(),
        }));
        let doc = encode(
            vec![para(vec![unnamed, field("b", "나")]), para(vec![text("다음")])],
            &ImageStore::default(),
        );
        let bytes = with_section_xml(&doc, add_caches(&section_xml(&doc)));
        let listed = HwpxFiller::list_fields(&bytes).expect("list");
        assert_eq!(listed.len(), 2, "premise: the unnamed field is listed");
        let xml = fill_xml(&bytes, &[("b", "값")]).expect("fill");
        // W2: 다음 문단(9001)은 흐름 꼬리라 함께 간다(전에는 남겼다). 순번 시험의
        // 이빨은 위치 불일치 오류(`expect`)가 진다.
        assert_eq!(caches(&xml), Vec::<usize>::new());
    }
}
