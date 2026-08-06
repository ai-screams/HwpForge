//! 셰이핑 — rustybuzz + **한컴 공백 0.5em 오버라이드** (W0 R4).
//!
//! 한컴의 공백 어드밴스는 폰트 space 글리프 폭(예: HBatang 0.333em)이 아니라
//! **정확히 0.5em** 이다 — 3폰트 × 3크기 실측 일반 확증 (`HBatang OS/2
//! xAvgCharWidth = 512/1024` 부합). PoC 3차에서 이 교체 하나로 줄 끝
//! 오차가 0px 로 소멸했다.
//!
//! 산출 단위: **HWPUNIT (f64)** — `advance_em × size_hwpunit`. paint 경계
//! 전까지 pt 로 바꾸지 않는다.

use crate::{PdfError, PdfResult};

/// 한컴 공백 어드밴스 (em 비율 — W0 R4 실측).
pub const HANCOM_SPACE_ADVANCE_EM: f64 = 0.5;

/// 셰이핑된 글리프 하나.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapedGlyph {
    /// 폰트 내 글리프 ID.
    pub glyph_id: u32,
    /// 어드밴스 (HWPUNIT — 크기 반영, 공백 오버라이드 적용).
    pub advance: f64,
    /// 이 글리프가 공백(U+0020)에서 왔는지 (JUSTIFY 배분 대상).
    pub is_space: bool,
    /// 원문 텍스트 안의 시작 바이트 (rustybuzz cluster — 텍스트 range 파생용).
    pub cluster: usize,
}

/// 한 텍스트 조각의 셰이핑 결과.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapedText {
    /// 글리프 시퀀스 (시각 순서).
    pub glyphs: Vec<ShapedGlyph>,
}

impl ShapedText {
    /// 자연폭 (HWPUNIT — 어드밴스 합).
    pub fn natural_width(&self) -> f64 {
        self.glyphs.iter().map(|g| g.advance).sum()
    }

    /// 공백 글리프 수 (JUSTIFY 배분 분모).
    pub fn space_count(&self) -> usize {
        self.glyphs.iter().filter(|g| g.is_space).count()
    }
}

/// `text` 를 주어진 폰트 데이터/크기로 셰이핑한다.
///
/// - `font_size_hwpunit`: 글자 크기 (HWPUNIT — `HwpUnit::from_pt(10.0)` = 1000).
/// - 공백 글리프의 어드밴스는 폰트 값 대신 [`HANCOM_SPACE_ADVANCE_EM`] 을 쓴다.
///
/// # Errors
///
/// 파싱 불능 폰트 데이터는 [`PdfError::FontIo`] 로 보고한다
/// (정상 경로에서는 resolver 가 이미 걸렀어야 할 입력).
pub fn shape_text(
    font_data: &[u8],
    face_index: u32,
    text: &str,
    font_size_hwpunit: i32,
) -> PdfResult<ShapedText> {
    let face = rustybuzz::Face::from_slice(font_data, face_index).ok_or_else(|| {
        PdfError::FontIo(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "font data failed to parse for shaping",
        ))
    })?;
    let upem = f64::from(face.units_per_em());
    let size = f64::from(font_size_hwpunit);

    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(text);
    let shaped = rustybuzz::shape(&face, &[], buffer);

    let text_bytes = text.as_bytes();
    let infos = shaped.glyph_infos();
    let positions = shaped.glyph_positions();

    let mut glyphs = Vec::with_capacity(infos.len());
    for (info, pos) in infos.iter().zip(positions) {
        // cluster = UTF-8 바이트 인덱스 — 공백 판정·텍스트 range 파생에 사용.
        let cluster = info.cluster as usize;
        let is_space = text_bytes.get(cluster) == Some(&b' ');
        let advance_em =
            if is_space { HANCOM_SPACE_ADVANCE_EM } else { f64::from(pos.x_advance) / upem };
        glyphs.push(ShapedGlyph {
            glyph_id: info.glyph_id,
            advance: advance_em * size,
            is_space,
            cluster,
        });
    }
    Ok(ShapedText { glyphs })
}

#[cfg(test)]
mod tests {
    use super::*;

    const HANCOM_TTF_DIR: &str =
        "/Applications/Hancom Office HWP.app/Contents/Resources/Hnc/Shared/TTF";

    fn hbatang() -> Option<Vec<u8>> {
        let resolver =
            crate::font::FontResolver::new(&[std::path::PathBuf::from(HANCOM_TTF_DIR)]).ok()?;
        resolver.resolve("한컴바탕").ok().map(|f| f.data)
    }

    #[test]
    fn garbage_font_data_is_io_error() {
        let err = shape_text(b"not a font", 0, "가", 1000).unwrap_err();
        assert!(matches!(err, PdfError::FontIo(_)));
    }

    #[test]
    fn space_advance_is_half_em_regardless_of_font_metrics() {
        // W0 R4: 공백 = 0.5em (폰트 space 글리프 0.333em 아님).
        let Some(data) = hbatang() else { return }; // fixture-optional
        let shaped = shape_text(&data, 0, "가 나", 1000).expect("shape");
        let spaces: Vec<_> = shaped.glyphs.iter().filter(|g| g.is_space).collect();
        assert_eq!(spaces.len(), 1);
        // 10pt(1000HU) × 0.5em = 500HU = 5.0pt
        assert_eq!(spaces[0].advance, 500.0);
        assert_eq!(shaped.space_count(), 1);
    }

    #[test]
    fn natural_width_scales_linearly_with_size() {
        // W0 R4 일반성: 10pt→5.04pt gap, 20pt→9.96pt gap (선형).
        let Some(data) = hbatang() else { return };
        let at10 = shape_text(&data, 0, "가나 다라", 1000).expect("shape@10");
        let at20 = shape_text(&data, 0, "가나 다라", 2000).expect("shape@20");
        let ratio = at20.natural_width() / at10.natural_width();
        assert!((ratio - 2.0).abs() < 1e-9, "ratio={ratio}");
    }

    #[test]
    fn hangul_glyph_advance_comes_from_font_not_assumption() {
        // 글리프 폭은 폰트 실측값 (PoC 2차: 단어폭 Δ≤0.08pt) — 1em 가정 금지.
        let Some(data) = hbatang() else { return };
        let shaped = shape_text(&data, 0, "가", 1000).expect("shape");
        assert_eq!(shaped.glyphs.len(), 1);
        let g = &shaped.glyphs[0];
        assert!(!g.is_space);
        // HBatang 한글은 1em 전각 — 폰트에서 읽은 값이 1000HU(=1em×10pt)와 일치해야 한다.
        // (폰트가 다른 값을 갖는다면 그 값이 정답 — 이 단언은 HBatang 실측 고정.)
        assert_eq!(g.advance, 1000.0);
    }

    // ── 커밋된 테스트 폰트 (tests/fonts/generate_test_fonts.py) — CI 포함 전 환경 실행.
    // 메트릭이 생성기에 고정돼 있어 (space 0.3em / Latin 0.6em / 한글 1.0em)
    // 단언을 정확값으로 걸 수 있다.

    fn test_font() -> Vec<u8> {
        std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fonts/HwpForgeTest-Regular.ttf"))
            .expect("committed test font")
    }

    #[test]
    fn space_override_beats_font_space_metric() {
        // 테스트 폰트의 space 어드밴스는 의도적으로 0.3em — 오버라이드(0.5em)가 이겨야 한다.
        let shaped = shape_text(&test_font(), 0, "가 나", 1000).expect("shape");
        let spaces: Vec<_> = shaped.glyphs.iter().filter(|g| g.is_space).collect();
        assert_eq!(spaces.len(), 1);
        assert_eq!(spaces[0].advance, 500.0, "폰트값 300HU 이 아니라 0.5em=500HU");
        assert_eq!(shaped.space_count(), 1);
    }

    #[test]
    fn glyph_advance_comes_from_hmtx() {
        // 한글 1.0em·Latin 0.6em — 생성기 고정 메트릭이 그대로 나와야 한다 (1em 가정 금지).
        let data = test_font();
        let hangul = shape_text(&data, 0, "가", 1000).expect("shape hangul");
        assert_eq!(hangul.glyphs.len(), 1);
        assert_eq!(hangul.glyphs[0].advance, 1000.0);
        let latin = shape_text(&data, 0, "A", 1000).expect("shape latin");
        assert_eq!(latin.glyphs.len(), 1);
        assert_eq!(latin.glyphs[0].advance, 600.0);
    }

    #[test]
    fn natural_width_is_exact_and_linear_with_test_font() {
        // 자연폭 = 어드밴스 합: 가나(2000) + 공백(500) + 다라(2000) = 4500HU @10pt.
        let data = test_font();
        let at10 = shape_text(&data, 0, "가나 다라", 1000).expect("shape@10");
        assert_eq!(at10.natural_width(), 4500.0);
        let at20 = shape_text(&data, 0, "가나 다라", 2000).expect("shape@20");
        assert_eq!(at20.natural_width(), 9000.0, "크기 선형성");
    }
}
