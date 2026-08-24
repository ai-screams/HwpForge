//! Paint IR — 좌표 확정·백엔드 중립 페인트 명령.
//!
//! 배치소스(source)가 캐시 재생을 끝낸 결과다. 이 층부터는 조판 정보가
//! 없다 — 좌표는 전부 **top-left 원점 pt(f64)** 로 확정돼 있고, 백엔드는
//! 그리기만 한다. W3(괘선)·W5(이미지) variant 확장을 위해 타입 계약을
//! 지금 고정한다 (Codex 리뷰 M1):
//!
//! - 백엔드 타입(krilla 등)을 **이 모듈로 새지 않게 한다** — 폰트는
//!   중립 [`FontKey`], 색은 foundation [`Color`].
//! - [`Page::items`] 의 **Vec 순서가 z-order** 다 (앞 = 아래).
//! - 새 variant 는 자기 geometry/style 을 스스로 소유한다.

use hwpforge_foundation::Color;

/// 포인트 단위 좌표값 (1pt = 1/72 inch).
///
/// source 층은 HWPUNIT(i32) 정수 산술로 규칙을 끝내고, paint 경계에서
/// `HwpUnit → Pt` 로 **한 번만** 변환한다 (1pt = 100 HWPUNIT).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pt(pub f64);

impl Pt {
    /// HWPUNIT 값을 pt 로 변환한다 (1pt = 100 HWPUNIT).
    pub fn from_hwpunit(value: i32) -> Self {
        Self(f64::from(value) / 100.0)
    }

    /// 분수 HWPUNIT(셰이핑 파생값)을 pt 로 변환한다.
    pub fn from_hwpunit_f64(value: f64) -> Self {
        Self(value / 100.0)
    }
}

/// top-left 원점 2D 좌표.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    /// 가로 (오른쪽 +).
    pub x: Pt,
    /// 세로 (아래쪽 + — top-left 원점).
    pub y: Pt,
}

/// 2D 크기.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Size {
    /// 폭.
    pub width: Pt,
    /// 높이.
    pub height: Pt,
}

/// 렌더 컨텍스트 폰트 테이블 인덱스 (백엔드 중립 폰트 식별자).
///
/// 실제 폰트 데이터([`crate::font::ResolvedFont`])는 렌더 컨텍스트가
/// 소유하고, Paint IR 은 인덱스만 나른다 — krilla 폰트 객체나 파일 경로가
/// 이 층에 유입되면 W4(resolver 확장)에서 재설계가 필요해진다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FontKey(pub usize);

/// baseline 원점 기준으로 위치가 확정된 글리프 하나.
#[derive(Debug, Clone, PartialEq)]
pub struct PositionedGlyph {
    /// 폰트 내 글리프 ID.
    pub glyph_id: u32,
    /// [`GlyphRun::baseline`] 원점으로부터의 가로 오프셋 (정렬 배분 반영 후).
    pub x_offset: Pt,
    /// 자연 어드밴스 (마지막 글리프의 pen 이동·advance 기반 백엔드용).
    pub advance: Pt,
    /// [`GlyphRun::text`] 안의 UTF-8 바이트 구간 (PDF 텍스트 추출 —
    /// ToUnicode/ActualText 정합. bbox-diff 게이트가 이것에 의존한다).
    pub text_range: std::ops::Range<usize>,
}

/// 같은 폰트·크기·색으로 그리는 글리프 묶음 (한 줄의 run 단위).
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphRun {
    /// 폰트 식별자.
    pub font: FontKey,
    /// 글자 크기 (pt).
    pub size: Pt,
    /// 글자 색.
    pub color: Color,
    /// baseline 원점 (top-left 좌표계 — y = baseline 의 세로 위치).
    pub baseline: Point,
    /// run 의 원문 텍스트 (글리프 [`PositionedGlyph::text_range`] 의 대상).
    pub text: String,
    /// 위치 확정 글리프들 (baseline 원점 상대).
    pub glyphs: Vec<PositionedGlyph>,
}

/// 채운 사각형 (W3 — 셀 배경).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct RectItem {
    /// 좌상단 x (pt).
    pub x: Pt,
    /// 좌상단 y (pt).
    pub y: Pt,
    /// 폭 (pt).
    pub width: Pt,
    /// 높이 (pt).
    pub height: Pt,
    /// 채움색.
    pub color: hwpforge_foundation::Color,
}

/// 선분 (W3 — 괘선, 경계선 중앙 기준 stroke).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct LineItem {
    /// 시작점 (pt).
    pub from: Point,
    /// 끝점 (pt).
    pub to: Point,
    /// 선 굵기 (pt).
    pub width: Pt,
    /// 선 색.
    pub color: hwpforge_foundation::Color,
}

/// 페인트 항목. `#[non_exhaustive]` — W5 `Image` 확장 예정.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum PaintItem {
    /// 글리프 run.
    Glyphs(GlyphRun),
    /// 채운 사각형 (셀 배경 — 글리프보다 먼저 그린다).
    Rect(RectItem),
    /// 선분 (괘선 — 배경 뒤, 글리프 앞).
    Line(LineItem),
    /// 래스터 이미지 (W2a — §3 D3).
    Image(ImageItem),
    /// 사각 클립 그룹 (W4 w2 — 글상자 클리핑, 중첩 가능).
    Clipped(ClipGroup),
}

/// 래스터 이미지 paint 항목 — 자기완결 (backend 가 스토어를 모른다).
///
/// `data` 는 paint-build 스코프의 asset interner 가 canonical key 당
/// 1회만 복사한 공유 바이트다 (occurrence 별 복제 금지 — §3 D3).
/// 포맷은 backend 가 magic 스니핑으로 판별한다 (Core `ImageFormat` 은
/// 확장자 유래라 진단 힌트일 뿐).
#[derive(Debug, Clone, PartialEq)]
pub struct ImageItem {
    /// canonical 스토어 키 (디코드 캐시 키 — 같은 키 다른 바이트 =
    /// [`crate::PdfError::ImageAssetConflict`]).
    pub canonical_key: String,
    /// 원본 이미지 바이트 (공유).
    pub data: std::sync::Arc<Vec<u8>>,
    /// 좌상단 배치 원점 (pt).
    pub origin: Point,
    /// 표시 크기 (pt) — 0/음수/non-finite 는
    /// [`crate::PdfError::InvalidImageGeometry`].
    pub size: Size,
    /// 진단 위치 (경고/오류 payload).
    pub location: String,
}

/// 사각 클립 그룹 (W4 w2 — 글상자 렌더의 클리핑 기반).
///
/// `origin`+`size` 사각형으로 클립 영역을 세운 뒤 `items` 를 그리고 클립을
/// 되돌린다 (backend: krilla `push_clip_path` → 자식 그리기 → `pop`).
/// 좌표계는 다른 항목과 동일한 **top-left 원점 pt** 다.
///
/// `items` 는 [`PaintItem::Clipped`] 를 다시 담을 수 있다 (중첩 클립 —
/// 셀 안 글상자 대비). backend 는 재귀로 push/pop 을 쌓는다.
///
/// 넘친 자식(클립 밖)은 backend 가 렌더 시점에 잘라낸다 — Paint IR 은
/// 자식을 그대로 담고, 잘림은 클립 영역이 강제한다 (글상자 overflow
/// 실측 = 한컴은 글자 중간을 박스 경계로 절단, 설계 §8a).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ClipGroup {
    /// 클립 사각형 좌상단 원점 (pt).
    pub origin: Point,
    /// 클립 사각형 크기 (pt) — 0/음수/non-finite 는 backend 가 그룹째
    /// 생략한다 (빈 클립 = 아무것도 보이지 않음).
    pub size: Size,
    /// 클립 안에서 그릴 항목들 — **Vec 순서 = z-order**, [`PaintItem::Clipped`]
    /// 중첩 허용.
    pub items: Vec<PaintItem>,
}

/// PDF 한 쪽.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    /// 쪽 크기 (pt).
    pub size: Size,
    /// 페인트 항목 — **Vec 순서 = z-order** (앞 원소가 아래에 깔린다).
    pub items: Vec<PaintItem>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pt_from_hwpunit_is_1_to_100() {
        assert_eq!(Pt::from_hwpunit(100), Pt(1.0));
        assert_eq!(Pt::from_hwpunit(0), Pt(0.0));
        assert_eq!(Pt::from_hwpunit(-1600), Pt(-16.0));
        // A4 폭 59528 HWPUNIT = 595.28pt
        assert_eq!(Pt::from_hwpunit(59_528), Pt(595.28));
    }

    #[test]
    fn page_items_order_is_z_order() {
        // 계약 문서화 테스트: Vec 순서가 곧 그리기 순서다.
        let run = |x: f64| {
            PaintItem::Glyphs(GlyphRun {
                font: FontKey(0),
                size: Pt(10.0),
                color: Color::from_rgb(0, 0, 0),
                baseline: Point { x: Pt(x), y: Pt(100.0) },
                text: "가".to_string(),
                glyphs: vec![PositionedGlyph {
                    glyph_id: 1,
                    x_offset: Pt(0.0),
                    advance: Pt(10.0),
                    text_range: 0..3,
                }],
            })
        };
        let page = Page {
            size: Size { width: Pt(595.28), height: Pt(841.89) },
            items: vec![run(0.0), run(10.0)],
        };
        assert_eq!(page.items.len(), 2);
        let PaintItem::Glyphs(first) = &page.items[0] else {
            panic!("first item must be glyphs");
        };
        assert_eq!(first.baseline.x, Pt(0.0));
    }
}
