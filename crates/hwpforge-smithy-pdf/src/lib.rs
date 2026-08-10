//! PDF renderer for HwpForge — 한컴 조판 캐시 재생 (layout-cache replay).
//!
//! 이 크레이트는 문서를 **조판하지 않는다**. 한컴이 저장한 줄 조판 캐시
//! ([`hwpforge_core::layout::LayoutCache`], W1 에서 decode-only 승격)를
//! 재생해 "한컴과 같은 PDF" 를 만든다. 캐시에 없는 것만 실측 확정 규칙
//! (`.docs/algorithms/pdf-cache-replay-rules.md`, W0)으로 계산한다.
//!
//! # 3계층 구조 (에픽 §4)
//!
//! ```text
//! source/   캐시 재생 배치소스 (HWPUNIT i32 정수 산술 — 쪽분할·줄분할·baseline)
//!    ↓ (source→paint 경계에서 f64 pt 로 1회 변환)
//! paint/    Paint IR — 좌표 확정·백엔드 중립 (top-left pt, Vec 순서 = z-order)
//!    ↓
//! backend/  krilla PDF 쓰기
//! ```
//!
//! # 지원 범위 (W2~W5 출하 기준)
//!
//! - 텍스트: 셰이핑(공백 0.5em)·정렬 배분(R2)·bold/italic face 선택·언어축 검사·
//!   임베드 라이선스 게이트(fsType fail-closed)
//! - 표: 열폭/행높이(R1)·쪽걸침 분할·제목행 반복·셀 배경/괘선
//! - 머리말/꼬리말: 밴드-상대 캐시 재생(R6) · ODD/EVEN parity · 무클립 overflow
//! - 쪽번호: BOTTOM_CENTER+DIGIT 합성 (전용 "쪽 번호" 스타일 출처)
//! - 미지원 (경고 or fail-closed 거부): 이미지·GSO 도형·각주/미주/메모·차트/OLE·
//!   무캐시 문서(자체 조판 없음 — 한컴 재저장 필요)
//!
//! # 입력 계약
//!
//! 렌더 입력은 [`PdfInput`] — Core 문서만으로는 폰트명·크기·정렬을 알 수 없어
//! [`StyleLookup`] 이 필수다 (Codex 리뷰 C1, smithy-md styled encoder 선례).

#![deny(missing_docs)]

mod backend;
pub mod font;
pub mod paint;
mod render;
pub mod source;
pub mod text;

pub use render::render_document;

use hwpforge_core::document::{Document, Validated};
use hwpforge_core::StyleLookup;

/// 렌더 입력: 검증된 문서 + 스타일 컨텍스트.
///
/// `styles` 는 포맷 중립 trait 객체다 — 구체 스토어(`HwpxStyleStore` 등)를
/// 직접 받지 않으므로 smithy-hwpx/hwp5 에 의존하지 않는다.
pub struct PdfInput<'a> {
    /// 렌더 대상 문서 (조판 캐시 보유 — admission 이 검사).
    pub document: &'a Document<Validated>,
    /// 문자/문단 스타일 조회 (폰트명·크기·bold/italic·색·정렬).
    pub styles: &'a dyn StyleLookup,
}

/// 부분 캐시(편집본) 처리 정책.
///
/// fill/set-cell/stamp 편집본은 `layout_carry` 가 미편집 문단 캐시만 보존해
/// 편집 문단이 캐시 결손이 된다. 에픽 §4 표는 이런 문서를 "지원(경고)" 로
/// 명시하므로 기본값은 [`PartialCachePolicy::WarnAndSkip`] 이다.
///
/// 한계: `Document` 에는 편집 이력이 없어 국소 결손(fill)과 구조 편집(E4)
/// 결손을 캐시 상태만으로 구분할 수 없다. 렌더 가능 캐시가 0 인 섹션은
/// 정책과 무관하게 거부한다 (하한).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PartialCachePolicy {
    /// 캐시 결손 문단이 하나라도 있으면 거부.
    Reject,
    /// 결손 문단은 경고를 남기고 건너뛴다 (기본).
    #[default]
    WarnAndSkip,
}

/// 폰트 강등 정책 — 스타일 face 결손·언어축 불일치의 공통 처리 (W4c).
///
/// 실측 (blank-HPC): 한컴 문서의 charPr 언어축 불일치는 예외가 아니라
/// 상례(렌더 run 30%)이고, bold 요청 face 미보유도 흔하다. 기본값은
/// 조용한 오글리프 출력을 금지하는 [`Fatal`](Self::Fatal) — 실용 렌더가
/// 필요하면 [`Degraded`](Self::Degraded) 를 명시 옵트인한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum FontFallbackMode {
    /// 강등 없이 에러 (기본 — [`PdfError::FontStyleUnavailable`] /
    /// [`PdfError::FontAxisMismatch`]).
    #[default]
    Fatal,
    /// regular face·한글 축으로 강등하고 경고를 표면화한다 (옵트인 —
    /// [`PdfWarning::FontStyleFallback`] / [`PdfWarning::FontAxisFallback`]).
    /// 신호 모순([`PdfError::FontFaceAmbiguous`])은 이 모드에서도 에러다.
    Degraded,
}

/// 렌더 동작 옵션.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct PdfOptions {
    /// 부분 캐시 처리 정책. 기본 [`PartialCachePolicy::WarnAndSkip`].
    pub partial_cache: PartialCachePolicy,
    /// 폰트 파일 탐색 디렉터리 (face 축 분류 — [`font::FontResolver`]).
    pub font_dirs: Vec<std::path::PathBuf>,
    /// 폰트 자동 발견 정책. 기본 [`font::FontDiscovery::ExplicitOnly`]
    /// (명시 dirs 만 — 머신 무관 결정적).
    pub discovery: font::FontDiscovery,
    /// 폰트 강등 정책. 기본 [`FontFallbackMode::Fatal`].
    pub font_fallback: FontFallbackMode,
}

/// 렌더 산출물.
#[derive(Debug)]
#[non_exhaustive]
pub struct PdfOutput {
    /// PDF 바이트.
    pub bytes: Vec<u8>,
    /// 비치명 경고 (스킵된 문단·regular 외 run 등 — no-fake-support).
    pub warnings: Vec<PdfWarning>,
}

/// 비치명 경고 (warning-first 원칙).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PdfWarning {
    /// 캐시 결손 문단을 건너뜀 ([`PartialCachePolicy::WarnAndSkip`]).
    ParagraphSkipped {
        /// 문서 내 위치 (사람이 읽는 경로 — 섹션/문단 인덱스).
        location: String,
    },
    /// Degraded 모드: 요청 스타일 face 가 없어 regular 로 강등함
    /// ((face, style) 당 1회 — 첫 발생 위치 기록).
    FontStyleFallback {
        /// face/family 이름.
        face: String,
        /// 요청했던 스타일 축.
        requested: font::FaceStyle,
        /// 첫 발생 위치.
        location: String,
    },
    /// Degraded 모드: charPr 언어축(7축)이 서로 다른 폰트를 참조하는데
    /// 한글 축 폰트 단일로 렌더함 (charPr 당 1회 — 첫 발생 위치 기록).
    ///
    /// 축별 폰트 선택 + run 분할은 후속 슬라이스 (issue #129).
    FontAxisFallback {
        /// 축별 distinct 폰트 이름 (첫 원소 = 렌더에 쓴 한글 축).
        fonts: Vec<String>,
        /// 첫 발생 위치.
        location: String,
    },
    /// Preview & Print 전용 폰트를 임베드함 (W4d — physical face 당 1회).
    ///
    /// PDF 뷰/인쇄 임베드는 관례상 허용 범위지만 라이선스 상태를
    /// 표면화한다 (권한·출처·fingerprint).
    FontEmbedPreviewPrint {
        /// face 이름.
        face: String,
        /// 실물 경로 (출처).
        path: std::path::PathBuf,
        /// 파일 바이트 fingerprint (hex — 물리 동일성 진단).
        fingerprint: String,
    },
    /// 정렬 배분이 근사임 (배분 정렬 2종·공백 0 JUSTIFY — W0 미실측,
    /// 위치는 정확하고 스트레치만 생략).
    AlignmentApproximated {
        /// 문서 내 위치.
        location: String,
    },
    /// 문단 안의 비텍스트 run(컨트롤·이미지)을 렌더에서 생략함 (W5 전
    /// 미지원 — 문단 자체는 렌더됨).
    NonTextRunDropped {
        /// 문서 내 위치.
        location: String,
    },
    /// 분할 표의 중간 쪽 경계는 캐시에 신호가 없어 **계산**으로 배치함
    /// (W3 — 캐시 앵커 이중 검산을 통과한 출력에만 딸려 나옴).
    TablePaginationComputed {
        /// 표 host 문단 위치.
        location: String,
    },
    /// 병합 셀 부족분을 행들에 재배분함 (규칙 = 마지막 스팬 행 몰빵 —
    /// 실측 fixture 로 확정됐지만 내부 기하는 검산 사각이라 표면화).
    TableDeficitDistributed {
        /// 표 host 문단 위치.
        location: String,
    },
    /// 미지원 표 스타일(채움 종류·괘선 종류)을 경고 후 생략함.
    UnsupportedTableStyle {
        /// 셀 위치.
        location: String,
        /// 생략한 속성 (예: `cell fill`, `border line style`).
        what: &'static str,
    },
    /// 머리말/꼬리말 캐시가 밴드 높이를 넘음 — 한컴 실측(rules-header-overflow)
    /// 대로 **무클립 재생**한다 (본문 리플로 없음, 잉크 겹침 가능성만 표면화).
    BandOverflow {
        /// 밴드 종류 (`header`/`footer`).
        kind: &'static str,
        /// 넘친 머리말/꼬리말 위치.
        location: String,
    },
    /// `pageStartsOn != BOTH` 는 미실측 — BOTH 거동으로 렌더하고 표면화.
    PageStartsOnFallback {
        /// 섹션 인덱스.
        section: usize,
    },
    /// 머리말/꼬리말 `vertAlign != TOP` 은 미실측 (실측 fixture 전부
    /// textHeight==밴드 높이라 TOP 과 구분 불가) — TOP 거동으로 렌더하고 표면화.
    VertAlignFallback {
        /// 머리말/꼬리말 위치.
        location: String,
    },
    /// 쪽번호를 생략함 — BOTTOM_CENTER + DIGIT 조합만 실측(rules-pagenum),
    /// 그 외 position/포맷은 좌표 근거가 없어 그리지 않는다 (본문은 정상).
    PageNumberSkipped {
        /// 섹션 인덱스.
        section: usize,
        /// 생략 사유 (`position` / `format`).
        what: &'static str,
    },
    /// "쪽 번호"(Page Number) CHAR 스타일이 스타일 테이블에 없어 문서 기본
    /// charPr(0) 로 폴백함 — 한컴 실측 출처는 전용 스타일이다 (§8c).
    PageNumberStyleFallback {
        /// 섹션 인덱스.
        section: usize,
    },
}

/// 렌더 실패 (fail-closed — 출력 바이트 없음).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PdfError {
    /// 렌더 가능한 조판 캐시가 없는 섹션 (완전 무캐시 산출물 포함).
    ///
    /// 힌트: 한컴에서 열어 저장하면 캐시가 재생성된다.
    #[error(
        "section {section} has no renderable layout cache — open and re-save in Hancom to \
         regenerate (구조 편집본/순수 생성물은 캐시가 없다)"
    )]
    NoRenderableCache {
        /// 섹션 인덱스.
        section: usize,
    },
    /// [`PartialCachePolicy::Reject`] 하에서 캐시 결손 문단 발견.
    #[error("{count} paragraph(s) missing layout cache (first: {first}) under Reject policy")]
    MissingLayoutCache {
        /// 결손 문단 수.
        count: usize,
        /// 첫 결손 위치.
        first: String,
    },
    /// W2 미지원 콘텐츠 (표 등 — 다쪽 표는 후속 문단 page-ordinal 을 보장할 수 없다).
    #[error("unsupported content in W2 scope: {kind} at {location}")]
    UnsupportedContent {
        /// 콘텐츠 종류 (예: `table`).
        kind: &'static str,
        /// 문서 내 위치.
        location: String,
    },
    /// 내부 불변식 위반 — 코드 결함의 안전망 (W6-α: corpus 대량 실행에서
    /// panic 대신 문서 단위 fail-closed 오류로 표면화한다).
    #[error("internal invariant violated: {detail}")]
    InternalInvariant {
        /// 위반 상세 (위치 포함).
        detail: String,
    },
    /// 같은 쪽에 머리말/꼬리말 후보가 2개 이상 매치 (BOTH+ODD 중복 등) —
    /// 한컴 우선순위 미실측이라 첫 항목 선택 대신 거부한다 (게이트2 H2).
    /// 다중 섹션 + ODD/EVEN parity 조합도 동일 사유로 거부 (물리/구역
    /// 서수 중 어느 쪽 홀짝인지 미실측).
    #[error("ambiguous {kind} selection: {detail}")]
    AmbiguousHeaderFooter {
        /// 밴드 종류 (`header`/`footer`).
        kind: &'static str,
        /// 충돌 상세 (섹션/쪽/매치 수).
        detail: String,
    },
    /// 폰트 파일 미해결 — fallback 하지 않는다 (위치가 틀린 출력 금지).
    #[error("font face {face:?} not resolved in provided font directories (no fallback)")]
    FontUnresolved {
        /// 요청 face 이름.
        face: String,
    },
    /// 요청 스타일 face 미보유 — 기본([`FontFallbackMode::Fatal`]) 모드
    /// (조용한 regular 강등 출력 금지 — [`FontFallbackMode::Degraded`] 로
    /// 옵트인하면 regular + 경고로 렌더된다).
    #[error(
        "font {face:?} has no {style:?} face at {location} (opt in to \
         FontFallbackMode::Degraded to render regular with a warning)"
    )]
    FontStyleUnavailable {
        /// face/family 이름.
        face: String,
        /// 요청 스타일 축.
        style: font::FaceStyle,
        /// 문서 내 위치.
        location: String,
    },
    /// charPr 언어축(7축)이 서로 다른 폰트를 참조 — 기본
    /// ([`FontFallbackMode::Fatal`]) 모드 (단일 폰트 렌더는 오글리프 —
    /// 축별 선택/분할 전까지 [`FontFallbackMode::Degraded`] 로 옵트인).
    #[error("char shape references different fonts per language axis at {location}: {fonts:?}")]
    FontAxisMismatch {
        /// 문서 내 위치.
        location: String,
        /// 축별 distinct 폰트 이름 (첫 원소 = 한글 축).
        fonts: Vec<String>,
    },
    /// 폰트 임베드 라이선스 거부 (W4d — `fsType` fail-closed).
    ///
    /// Restricted·No-subsetting(bit8)·Bitmap-only(bit9)·권한 검증 불가
    /// (OS/2 결측/malformed) 폰트는 임베드 전에 거부한다.
    /// [`FontFallbackMode::Degraded`] 는 강등 정책이지 라이선스 우회가
    /// 아니다 — 양 모드 모두 에러.
    #[error("font {face:?} at {path:?} cannot be embedded: {reason}")]
    FontEmbedRestricted {
        /// face 이름.
        face: String,
        /// 실물 경로 (출처 진단).
        path: std::path::PathBuf,
        /// 거부 사유 ([`font::embed_license`] 판정).
        reason: String,
    },
    /// 폰트 face 신호 충돌 — (family, style) 후보가 모순/동률이라 결정 불가.
    ///
    /// 조용한 선택 금지 (no-fake-support): 어느 실물 face 를 의미하는지
    /// 확정할 수 없으면 에러로 표면화한다 (W4a 분류기 계약).
    #[error("font face {face:?} style {style:?} is ambiguous: {detail}")]
    FontFaceAmbiguous {
        /// 요청 face/family 이름.
        face: String,
        /// 요청 스타일 축.
        style: crate::font::FaceStyle,
        /// 충돌 상세 (모순 face 경로 / 동률 후보 목록).
        detail: String,
    },
    /// 캐시 형식 정합 위반 (textpos 비단조/범위 초과 등).
    ///
    /// 이 검사는 형식 정합이지 스테일 캐시 방어가 아니다 — 동일 길이 치환은
    /// 통과한다 (스테일 방어는 편집 표면의 캐시 드롭 불변식이 담당).
    #[error("layout cache failed structural validation: {detail}")]
    InvalidCache {
        /// 위반 내용.
        detail: String,
    },
    /// 폰트 파일 IO 실패.
    #[error("font io: {0}")]
    FontIo(#[from] std::io::Error),
    /// 스타일 조회 실패 — [`StyleLookup`] 이 렌더 필수 속성을 제공하지 않음.
    #[error("style lookup missing {what} at {location}")]
    StyleUnavailable {
        /// 결손 속성 (예: `font name`).
        what: &'static str,
        /// 문서 내 위치.
        location: String,
    },
    /// PDF 백엔드(krilla) 실패.
    #[error("pdf backend: {0}")]
    Backend(String),
}

/// 이 크레이트의 `Result`.
pub type PdfResult<T> = Result<T, PdfError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_cache_policy_defaults_to_warn_and_skip() {
        // 에픽 §4 표: fill/set-cell 편집본 = "지원(경고)" — 옵션 뒤에 숨기지 않는다.
        assert_eq!(PartialCachePolicy::default(), PartialCachePolicy::WarnAndSkip);
        assert_eq!(PdfOptions::default().partial_cache, PartialCachePolicy::WarnAndSkip);
    }
}
