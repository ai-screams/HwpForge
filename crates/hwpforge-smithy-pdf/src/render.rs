//! 렌더 파이프라인 — replay(source) → shape/align(text) → Paint IR → backend.
//!
//! 좌표 흐름: source 는 HWPUNIT 정수, 셰이핑/배분은 분수 HWPUNIT, 이
//! 모듈의 Paint IR 방출 시점에 pt 로 **1회** 변환한다 (계획 §2).

use std::collections::HashMap;

use hwpforge_foundation::Color;

use crate::font::{FontResolver, ResolvedFont};
use crate::paint::{
    FontKey, GlyphRun, LineItem, Page, PaintItem, Point, PositionedGlyph, Pt, RectItem, Size,
};
use crate::source::replay_layout;
use crate::text::align::{place_line, NaturalLine};
use crate::text::shape::{shape_text, ShapedText};
use crate::{PdfError, PdfInput, PdfOptions, PdfOutput, PdfResult, PdfWarning};

/// 렌더 컨텍스트 폰트 테이블 — face 이름 → [`FontKey`] 인터닝.
struct FontTable {
    resolver: FontResolver,
    keys: HashMap<String, FontKey>,
    fonts: Vec<ResolvedFont>,
}

impl FontTable {
    fn new(resolver: FontResolver) -> Self {
        Self { resolver, keys: HashMap::new(), fonts: Vec::new() }
    }

    fn key_for(&mut self, face: &str) -> PdfResult<FontKey> {
        if let Some(key) = self.keys.get(face) {
            return Ok(*key);
        }
        let resolved = self.resolver.resolve(face)?;
        let key = FontKey(self.fonts.len());
        self.fonts.push(resolved);
        self.keys.insert(face.to_string(), key);
        Ok(key)
    }
}

/// 문서를 PDF 로 렌더한다 (조판 캐시 재생 — 계산이 아니라 재생).
///
/// # Errors
///
/// admission 행렬([`crate::source::replay_layout`]) + 스타일 결손
/// ([`PdfError::StyleUnavailable`]) + 폰트 미해결([`PdfError::FontUnresolved`],
/// fallback 금지) + 백엔드 실패([`PdfError::Backend`]).
pub fn render_document(input: &PdfInput<'_>, options: &PdfOptions) -> PdfResult<PdfOutput> {
    let layout = replay_layout(input, options)?;
    let mut warnings = layout.warnings;
    let mut table =
        FontTable::new(FontResolver::with_discovery(&options.font_dirs, options.discovery)?);

    struct PreparedRun {
        key: FontKey,
        size_hwpunit: i32,
        color: Color,
        shaped: ShapedText,
        text: String,
    }

    let mut pages = Vec::with_capacity(layout.pages.len());
    for page in &layout.pages {
        let mut items = Vec::new();
        // z-order 계약 (source): 셀 배경 → 괘선 → 글리프.
        for r in &page.rects {
            items.push(PaintItem::Rect(RectItem {
                x: Pt::from_hwpunit(r.x),
                y: Pt::from_hwpunit(r.y),
                width: Pt::from_hwpunit(r.width),
                height: Pt::from_hwpunit(r.height),
                color: r.color,
            }));
        }
        for b in &page.borders {
            items.push(PaintItem::Line(LineItem {
                from: Point { x: Pt::from_hwpunit(b.from.0), y: Pt::from_hwpunit(b.from.1) },
                to: Point { x: Pt::from_hwpunit(b.to.0), y: Pt::from_hwpunit(b.to.1) },
                width: Pt::from_hwpunit(b.width),
                color: b.color,
            }));
        }
        for line in &page.lines {
            if line.runs.is_empty() {
                continue; // 빈 줄 — 세로 공간은 캐시가 이미 소비했다.
            }
            // 1) run 별 스타일 조회 + 셰이핑.
            //
            // 줄 끝 공백은 셰이핑에서 제외한다: 한컴 캐시의 줄 텍스트는
            // 뒤따르는 공백을 앞 줄에 귀속시키지만, 렌더에서 그 공백은
            // 그리지도 JUSTIFY 분모에 넣지도 않는다 (W0 실측 — 한컴
            // 양쪽정렬 줄 끝 = 마지막 가시 글리프가 우변 밀착).
            let last_run_idx = line.runs.len() - 1;
            let mut prepared = Vec::with_capacity(line.runs.len());
            for (run_idx, run) in line.runs.iter().enumerate() {
                let text = if run_idx == last_run_idx {
                    run.text.trim_end_matches(' ')
                } else {
                    run.text.as_str()
                };
                if text.is_empty() {
                    continue;
                }
                let cs = run.char_shape;
                let face = input
                    .styles
                    .char_font_name(cs)
                    .ok_or_else(|| PdfError::StyleUnavailable {
                        what: "font name",
                        location: line.location.clone(),
                    })?
                    .to_string();
                let size_hwpunit = input
                    .styles
                    .char_font_size(cs)
                    .ok_or_else(|| PdfError::StyleUnavailable {
                        what: "font size",
                        location: line.location.clone(),
                    })?
                    .as_i32();
                let color = input.styles.char_text_color(cs).unwrap_or(Color::BLACK);
                let key = table.key_for(&face)?;
                let font = &table.fonts[key.0];
                let shaped = shape_text(&font.data, font.face_index, text, size_hwpunit)?;
                prepared.push(PreparedRun {
                    key,
                    size_hwpunit,
                    color,
                    shaped,
                    text: text.to_string(),
                });
            }

            // 2) 줄 단위 정렬 배분 (자연폭·공백 수는 run 합산).
            let natural = NaturalLine {
                width: prepared.iter().map(|p| p.shaped.natural_width()).sum(),
                space_count: prepared.iter().map(|p| p.shaped.space_count()).sum(),
            };
            let placement = place_line(line.alignment, line.line_box, natural, line.is_last_line);
            if placement.needs_warning {
                warnings
                    .push(PdfWarning::AlignmentApproximated { location: line.location.clone() });
            }

            // 3) 절대 배치 (분수 HWPUNIT) → run 별 GlyphRun (pt 변환 = 여기 1회).
            let baseline_y = Pt::from_hwpunit(line.baseline_y);
            let mut x = placement.origin_x;
            for p in prepared {
                let origin_x = x;
                let byte_len = p.text.len();
                let mut glyphs = Vec::with_capacity(p.shaped.glyphs.len());
                for (gi, g) in p.shaped.glyphs.iter().enumerate() {
                    let next_cluster = p.shaped.glyphs.get(gi + 1).map_or(byte_len, |n| n.cluster);
                    glyphs.push(PositionedGlyph {
                        glyph_id: g.glyph_id,
                        x_offset: Pt::from_hwpunit_f64(x - origin_x),
                        advance: Pt::from_hwpunit_f64(g.advance),
                        text_range: g.cluster..next_cluster,
                    });
                    x += g.advance;
                    if g.is_space {
                        x += placement.extra_per_space;
                    }
                }
                items.push(PaintItem::Glyphs(GlyphRun {
                    font: p.key,
                    size: Pt::from_hwpunit(p.size_hwpunit),
                    color: p.color,
                    baseline: Point { x: Pt::from_hwpunit_f64(origin_x), y: baseline_y },
                    text: p.text,
                    glyphs,
                }));
            }
        }
        pages.push(Page {
            size: Size {
                width: Pt::from_hwpunit(page.width),
                height: Pt::from_hwpunit(page.height),
            },
            items,
        });
    }

    let bytes = crate::backend::write_pdf(&pages, &table.fonts)?;
    Ok(PdfOutput { bytes, warnings })
}
