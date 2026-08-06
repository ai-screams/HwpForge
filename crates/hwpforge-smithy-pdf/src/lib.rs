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
//! # 웨이브 스테이징
//!
//! - **W2a (현재)**: Paint IR 타입 계약 + regular exact-face 폰트 resolver + 옵션/에러 표면
//! - W2b: 셰이핑(공백 0.5em)·정렬 배분(R2) — W2c: 배치소스·admission — W2d: krilla 백엔드
//!   + `render_document` 공개 (미구현 표면을 미리 노출하지 않는다)
//! - W2 스코프: **regular 텍스트 전용** — 표 포함 문서 거부(다쪽 표 page-ordinal 미보장),
//!   bold/italic run 경고, 머리말/꼬리말/쪽번호(W5)·폰트 스타일 선택/라이선스(W4) 제외
//!
//! # 입력 계약
//!
//! 렌더 입력은 [`PdfInput`] — Core 문서만으로는 폰트명·크기·정렬을 알 수 없어
//! [`StyleLookup`] 이 필수다 (Codex 리뷰 C1, smithy-md styled encoder 선례).

#![deny(missing_docs)]

pub mod font;
pub mod paint;
pub mod source;
pub mod text;

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

/// 렌더 동작 옵션.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct PdfOptions {
    /// 부분 캐시 처리 정책. 기본 [`PartialCachePolicy::WarnAndSkip`].
    pub partial_cache: PartialCachePolicy,
    /// 폰트 파일 탐색 디렉터리 (regular exact-face 만 — [`font::FontResolver`]).
    pub font_dirs: Vec<std::path::PathBuf>,
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
    /// bold/italic run — W2 는 regular 만 정합 보장 (폭 정합은 W4).
    NonRegularRun {
        /// 문서 내 위치.
        location: String,
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
    /// 폰트 파일 미해결 — fallback 하지 않는다 (위치가 틀린 출력 금지).
    #[error("font face {face:?} not resolved in provided font directories (no fallback)")]
    FontUnresolved {
        /// 요청 face 이름.
        face: String,
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
