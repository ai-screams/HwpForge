//! krilla 백엔드 — Paint IR → PDF 바이트.
//!
//! 이 층은 그리기만 한다. 좌표는 이미 top-left pt 로 확정돼 있고,
//! 글리프의 절대 오프셋을 krilla 의 advance 시퀀스로 되돌려 방출한다
//! (adv_i = x_{i+1} − x_i, 마지막 = 자연 advance — 위치 재현 정확).
//! 폰트 서브셋/임베드는 krilla 가 수행한다.

use crate::font::ResolvedFont;
use crate::paint::{GlyphRun, Page, PaintItem};
use crate::{PdfError, PdfResult};

/// Paint IR 페이지들을 PDF 바이트로 쓴다.
pub(crate) fn write_pdf(pages: &[Page], fonts: &[ResolvedFont]) -> PdfResult<Vec<u8>> {
    let mut krilla_fonts: Vec<Option<krilla::text::Font>> = vec![None; fonts.len()];
    let mut doc = krilla::Document::new();

    for page in pages {
        let size = krilla::geom::Size::from_wh(page.size.width.0 as f32, page.size.height.0 as f32)
            .ok_or_else(|| PdfError::Backend("invalid page size".to_string()))?;
        let mut pdf_page = doc.start_page_with(krilla::page::PageSettings::new(size));
        let mut surface = pdf_page.surface();

        for item in &page.items {
            match item {
                PaintItem::Glyphs(run) => {
                    let font = resolve_krilla_font(&mut krilla_fonts, fonts, run.font.0)?;
                    let (r, g, b) = run.color.to_rgb();
                    surface.set_stroke(None);
                    surface.set_fill(Some(krilla::paint::Fill {
                        paint: krilla::color::rgb::Color::new(r, g, b).into(),
                        ..Default::default()
                    }));
                    let glyphs = to_krilla_glyphs(run);
                    surface.draw_glyphs(
                        krilla::geom::Point::from_xy(
                            run.baseline.x.0 as f32,
                            run.baseline.y.0 as f32,
                        ),
                        &glyphs,
                        font,
                        &run.text,
                        run.size.0 as f32,
                        false,
                    );
                }
                PaintItem::Rect(rect) => {
                    let Some(kr) = krilla::geom::Rect::from_xywh(
                        rect.x.0 as f32,
                        rect.y.0 as f32,
                        rect.width.0 as f32,
                        rect.height.0 as f32,
                    ) else {
                        continue; // 0/음수 크기 — 그릴 것 없음.
                    };
                    let mut pb = krilla::geom::PathBuilder::new();
                    pb.push_rect(kr);
                    let Some(path) = pb.finish() else { continue };
                    let (r, g, b) = rect.color.to_rgb();
                    surface.set_stroke(None);
                    surface.set_fill(Some(krilla::paint::Fill {
                        paint: krilla::color::rgb::Color::new(r, g, b).into(),
                        ..Default::default()
                    }));
                    surface.draw_path(&path);
                }
                PaintItem::Line(line) => {
                    let mut pb = krilla::geom::PathBuilder::new();
                    pb.move_to(line.from.x.0 as f32, line.from.y.0 as f32);
                    pb.line_to(line.to.x.0 as f32, line.to.y.0 as f32);
                    let Some(path) = pb.finish() else { continue };
                    let (r, g, b) = line.color.to_rgb();
                    surface.set_fill(None);
                    surface.set_stroke(Some(krilla::paint::Stroke {
                        paint: krilla::color::rgb::Color::new(r, g, b).into(),
                        width: line.width.0 as f32,
                        ..Default::default()
                    }));
                    surface.draw_path(&path);
                }
            }
        }

        surface.finish();
        pdf_page.finish();
    }

    doc.finish().map_err(|e| PdfError::Backend(format!("{e:?}")))
}

fn resolve_krilla_font(
    cache: &mut [Option<krilla::text::Font>],
    fonts: &[ResolvedFont],
    key: usize,
) -> PdfResult<krilla::text::Font> {
    if let Some(font) = &cache[key] {
        return Ok(font.clone());
    }
    let resolved = &fonts[key];
    let font = krilla::text::Font::new(resolved.data.clone().into(), resolved.face_index)
        .ok_or_else(|| {
            PdfError::Backend(format!("krilla failed to parse font {:?}", resolved.path))
        })?;
    cache[key] = Some(font.clone());
    Ok(font)
}

/// 절대 오프셋 글리프를 krilla advance 시퀀스로 변환한다 (em 단위).
fn to_krilla_glyphs(run: &GlyphRun) -> Vec<krilla::text::KrillaGlyph> {
    let size = run.size.0;
    run.glyphs
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let advance_pt = match run.glyphs.get(i + 1) {
                Some(next) => next.x_offset.0 - g.x_offset.0,
                None => g.advance.0,
            };
            krilla::text::KrillaGlyph::new(
                krilla::text::GlyphId::new(g.glyph_id),
                (advance_pt / size) as f32,
                0.0,
                0.0,
                0.0,
                g.text_range.clone(),
                None,
            )
        })
        .collect()
}
