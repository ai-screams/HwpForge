//! krilla 백엔드 — Paint IR → PDF 바이트.
//!
//! 이 층은 그리기만 한다. 좌표는 이미 top-left pt 로 확정돼 있고,
//! 글리프의 절대 오프셋을 krilla 의 advance 시퀀스로 되돌려 방출한다
//! (adv_i = x_{i+1} − x_i, 마지막 = 자연 advance — 위치 재현 정확).
//! 폰트 서브셋/임베드는 krilla 가 수행한다.

use std::collections::HashMap;
use std::sync::Arc;

use crate::font::ResolvedFont;
use crate::image_sniff::{sniff_image_format, Sniffed};
use crate::paint::{GlyphRun, ImageItem, Page, PaintItem};
use crate::{PdfError, PdfResult, PdfWarning, RenderFailureMode};

/// canonical key 당 1회 생성·preflight 된 krilla 이미지 (§3 D2).
struct CachedImage {
    /// 캐시 정합 검증용 원본 바이트 (같은 키 다른 바이트 = fatal).
    data: Arc<Vec<u8>>,
    /// preflight 통과한 krilla 이미지 — cheap clone 으로 재사용.
    image: krilla::image::Image,
}

/// Paint IR 페이지들을 PDF 바이트로 쓴다.
///
/// `failure_mode` 는 이미지 실패의 Fatal/Degraded 정책 (§3 D5 — 폰트와
/// 공통). Degraded 실패는 `warnings` 로 표면화하고 해당 항목만 생략한다.
/// [`PdfError::ImageAssetConflict`] 는 모드 무관 항상 에러다.
pub(crate) fn write_pdf(
    pages: &[Page],
    fonts: &[ResolvedFont],
    failure_mode: RenderFailureMode,
    warnings: &mut Vec<PdfWarning>,
) -> PdfResult<Vec<u8>> {
    let mut krilla_fonts: Vec<Option<krilla::text::Font>> = vec![None; fonts.len()];
    let mut image_cache: HashMap<String, CachedImage> = HashMap::new();
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
                PaintItem::Image(item) => {
                    // 기하 검증 — 무음 생략 금지 (§3 D5).
                    let w = item.size.width.0;
                    let h = item.size.height.0;
                    if !(w.is_finite() && h.is_finite()) || w <= 0.0 || h <= 0.0 {
                        let detail = format!("display size {w}x{h}pt");
                        match failure_mode {
                            RenderFailureMode::Fatal => {
                                return Err(PdfError::InvalidImageGeometry {
                                    key: item.canonical_key.clone(),
                                    detail,
                                    location: item.location.clone(),
                                });
                            }
                            RenderFailureMode::Degraded => {
                                warnings.push(PdfWarning::InvalidImageGeometry {
                                    key: item.canonical_key.clone(),
                                    detail,
                                    location: item.location.clone(),
                                });
                                continue;
                            }
                        }
                    }
                    let image = match cached_image(&mut image_cache, item) {
                        Ok(image) => image,
                        // 자산 충돌은 모드 무관 fatal (§3 D2).
                        Err(e @ PdfError::ImageAssetConflict { .. }) => return Err(e),
                        Err(e) => match failure_mode {
                            RenderFailureMode::Fatal => return Err(e),
                            RenderFailureMode::Degraded => {
                                warnings.push(image_error_to_warning(e));
                                continue;
                            }
                        },
                    };
                    let Some(size) = krilla::geom::Size::from_wh(w as f32, h as f32) else {
                        // from_wh 실패는 위 기하 검증이 이미 걸렀어야 한다.
                        return Err(PdfError::Backend(format!(
                            "krilla rejected image size {w}x{h}"
                        )));
                    };
                    surface.push_transform(&krilla::geom::Transform::from_translate(
                        item.origin.x.0 as f32,
                        item.origin.y.0 as f32,
                    ));
                    surface.draw_image(image, size);
                    surface.pop();
                }
            }
        }

        surface.finish();
        pdf_page.finish();
    }

    doc.finish().map_err(|e| PdfError::Backend(format!("{e:?}")))
}

/// canonical key 로 캐시 조회/생성한다 — 미스 시 스니핑 → 생성자 →
/// **preflight**(1×1 임시 문서 finish 로 deferred decode 강제, §3 D2).
fn cached_image(
    cache: &mut HashMap<String, CachedImage>,
    item: &ImageItem,
) -> PdfResult<krilla::image::Image> {
    if let Some(cached) = cache.get(&item.canonical_key) {
        // 같은 키 = 같은 바이트여야 캐시가 정합하다 (Arc 동일성 우선,
        // 다른 Arc 면 바이트 비교).
        if Arc::ptr_eq(&cached.data, &item.data) || *cached.data == *item.data {
            return Ok(cached.image.clone());
        }
        return Err(PdfError::ImageAssetConflict {
            key: item.canonical_key.clone(),
            location: item.location.clone(),
        });
    }
    let data: krilla::Data = (item.data.clone() as Arc<dyn AsRef<[u8]> + Send + Sync>).into();
    // interpolate=true: 확대 시 보간 (한컴 뷰어 동작에 근접).
    let constructed = match sniff_image_format(&item.data) {
        Sniffed::Png => krilla::image::Image::from_png(data, true),
        Sniffed::Jpeg => krilla::image::Image::from_jpeg(data, true),
        Sniffed::Gif => krilla::image::Image::from_gif(data, true),
        Sniffed::Webp => krilla::image::Image::from_webp(data, true),
        Sniffed::KnownUnsupported(format) => {
            return Err(PdfError::UnsupportedImageFormat {
                key: item.canonical_key.clone(),
                format,
                location: item.location.clone(),
            });
        }
        Sniffed::Unknown => {
            return Err(PdfError::UnsupportedImageFormat {
                key: item.canonical_key.clone(),
                format: "unknown",
                location: item.location.clone(),
            });
        }
    };
    let image = constructed.map_err(|detail| PdfError::ImageDecodeFailed {
        key: item.canonical_key.clone(),
        detail,
        location: item.location.clone(),
    })?;
    preflight_decode(&image).map_err(|detail| PdfError::ImageDecodeFailed {
        key: item.canonical_key.clone(),
        detail,
        location: item.location.clone(),
    })?;
    cache.insert(
        item.canonical_key.clone(),
        CachedImage { data: item.data.clone(), image: image.clone() },
    );
    Ok(image)
}

/// krilla 는 실제 디코드를 `Document::finish()` 까지 미룬다 (생성자는
/// metadata 만 읽음) — valid-header/corrupt-body 를 Degraded 로 걸러내려면
/// 임시 1×1 문서로 디코드를 강제해야 한다 (§3 disposition H1).
fn preflight_decode(image: &krilla::image::Image) -> Result<(), String> {
    let mut doc = krilla::Document::new();
    let size = krilla::geom::Size::from_wh(1.0, 1.0)
        .ok_or_else(|| "internal: 1x1 preflight size rejected".to_string())?;
    let mut page = doc.start_page_with(krilla::page::PageSettings::new(size));
    let mut surface = page.surface();
    surface.draw_image(image.clone(), size);
    surface.finish();
    page.finish();
    doc.finish().map(|_| ()).map_err(|e| format!("{e:?}"))
}

/// Degraded 모드용 오류→경고 변환 (충돌은 호출부가 fatal 로 선처리).
fn image_error_to_warning(e: PdfError) -> PdfWarning {
    match e {
        PdfError::ImageDataMissing { key, location } => {
            PdfWarning::ImageDataMissing { key, location }
        }
        PdfError::UnsupportedImageFormat { key, format, location } => {
            PdfWarning::UnsupportedImageFormat { key, format, location }
        }
        PdfError::ImageDecodeFailed { key, detail, location } => {
            PdfWarning::ImageDecodeFailed { key, detail, location }
        }
        PdfError::InvalidImageGeometry { key, detail, location } => {
            PdfWarning::InvalidImageGeometry { key, detail, location }
        }
        other => PdfWarning::ImageDecodeFailed {
            key: String::new(),
            detail: format!("unexpected image failure: {other}"),
            location: String::new(),
        },
    }
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

#[cfg(test)]
mod image_tests {
    use super::*;
    use crate::paint::Pt;
    use crate::paint::{ImageItem, Page, Point, Size};

    /// 유효한 1×1 RGB PNG (표준 청크 + CRC — 생성 스크립트 검증).
    const TINY_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8,
        0xCF, 0xC0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0xC9, 0xFE, 0x92, 0xEF, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    fn image_item(key: &str, bytes: &[u8]) -> ImageItem {
        ImageItem {
            canonical_key: key.to_string(),
            data: Arc::new(bytes.to_vec()),
            origin: Point { x: Pt(10.0), y: Pt(20.0) },
            size: Size { width: Pt(50.0), height: Pt(40.0) },
            location: "s0/p0".to_string(),
        }
    }

    fn one_page(items: Vec<PaintItem>) -> Vec<Page> {
        vec![Page { size: Size { width: Pt(595.0), height: Pt(842.0) }, items }]
    }

    /// PDF parser-lite: 압축 스트림을 인플레이트해 원문과 합친 뒤 검사한다
    /// (§3 수용 기준 — byte grep 은 krilla 의 압축/객체 스트림에 취약).
    fn inflated_pdf_text(bytes: &[u8]) -> Vec<u8> {
        use std::io::Read as _;
        let mut all = bytes.to_vec();
        let mut i = 0usize;
        while let Some(off) = find(&bytes[i..], b"stream") {
            let start = i + off + b"stream".len();
            // EOL 스킵 (CRLF/LF).
            let mut s = start;
            while bytes.get(s) == Some(&b'\r') || bytes.get(s) == Some(&b'\n') {
                s += 1;
            }
            let end = find(&bytes[s..], b"endstream").map(|e| s + e).unwrap_or(bytes.len());
            let mut out = Vec::new();
            let mut dec = flate2::read::ZlibDecoder::new(&bytes[s..end]);
            if dec.read_to_end(&mut out).is_ok() {
                all.extend_from_slice(&out);
            }
            i = (end + b"endstream".len()).min(bytes.len());
            if i >= bytes.len() {
                break;
            }
        }
        all
    }

    fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|w| w == needle)
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        find(haystack, needle).is_some()
    }

    #[test]
    fn image_paint_item_reaches_pdf_as_xobject_with_do() {
        let pages = one_page(vec![PaintItem::Image(image_item("img1.png", TINY_PNG))]);
        let mut warnings = Vec::new();
        let bytes =
            write_pdf(&pages, &[], RenderFailureMode::Fatal, &mut warnings).expect("render");
        assert!(warnings.is_empty(), "{warnings:?}");
        let text = inflated_pdf_text(&bytes);
        assert!(
            contains(&text, b"/Subtype/Image") || contains(&text, b"/Subtype /Image"),
            "XObject 존재"
        );
        assert!(contains(&text, b" Do"), "content stream 에서 Do 사용");
    }

    #[test]
    fn same_key_same_bytes_embeds_once() {
        // 같은 키 2회 = 캐시 재사용 — XObject 이미지가 1개만 임베드된다.
        let a = image_item("dup.png", TINY_PNG);
        let b = a.clone();
        let pages = one_page(vec![PaintItem::Image(a), PaintItem::Image(b)]);
        let mut warnings = Vec::new();
        let bytes =
            write_pdf(&pages, &[], RenderFailureMode::Fatal, &mut warnings).expect("render");
        let text = inflated_pdf_text(&bytes);
        let occurrences =
            text.windows(b"/Subtype/Image".len()).filter(|w| *w == b"/Subtype/Image").count();
        assert_eq!(occurrences, 1, "단일 임베드 (cheap-clone 재사용)");
    }

    #[test]
    fn same_key_different_bytes_is_always_fatal_conflict() {
        let a = image_item("k.png", TINY_PNG);
        let mut other = TINY_PNG.to_vec();
        other[50] ^= 0xFF; // 내용만 다른 같은 키
        let b = ImageItem { data: Arc::new(other), ..a.clone() };
        let pages = one_page(vec![PaintItem::Image(a), PaintItem::Image(b)]);
        let mut warnings = Vec::new();
        // Degraded 에서도 fatal (§3 D2).
        let err = write_pdf(&pages, &[], RenderFailureMode::Degraded, &mut warnings)
            .expect_err("conflict");
        assert!(matches!(err, PdfError::ImageAssetConflict { .. }), "{err:?}");
    }

    #[test]
    fn corrupt_body_fails_at_preflight_not_finish() {
        // valid PNG 헤더 + 손상 본문: krilla 생성자(metadata)는 통과하고
        // deferred decode 에서 죽는다 — preflight 가 잡아야 한다 (H1).
        let mut corrupt = TINY_PNG.to_vec();
        for b in &mut corrupt[41..53] {
            *b = 0xAA; // IDAT 본문 파괴 (헤더/IHDR 유지)
        }
        let item = image_item("corrupt.png", &corrupt);
        let mut warnings = Vec::new();
        let err = write_pdf(
            &one_page(vec![PaintItem::Image(item.clone())]),
            &[],
            RenderFailureMode::Fatal,
            &mut warnings,
        )
        .expect_err("must fail");
        assert!(matches!(err, PdfError::ImageDecodeFailed { .. }), "{err:?}");

        // Degraded: 경고 + 항목 생략, 문서는 성공.
        let mut warnings = Vec::new();
        let bytes = write_pdf(
            &one_page(vec![PaintItem::Image(item)]),
            &[],
            RenderFailureMode::Degraded,
            &mut warnings,
        )
        .expect("degraded render");
        assert!(warnings.iter().any(|w| matches!(w, PdfWarning::ImageDecodeFailed { .. })));
        assert!(!contains(&inflated_pdf_text(&bytes), b"/Subtype/Image"));
    }

    #[test]
    fn unsupported_and_unknown_formats_follow_failure_mode() {
        let mut bmp = Vec::from(&b"BM"[..]);
        bmp.extend_from_slice(&[0u8; 20]);
        let item = image_item("logo.bmp", &bmp);
        let mut warnings = Vec::new();
        let err = write_pdf(
            &one_page(vec![PaintItem::Image(item.clone())]),
            &[],
            RenderFailureMode::Fatal,
            &mut warnings,
        )
        .expect_err("bmp fatal");
        assert!(
            matches!(&err, PdfError::UnsupportedImageFormat { format, .. } if *format == "bmp")
        );

        let mut warnings = Vec::new();
        write_pdf(
            &one_page(vec![PaintItem::Image(item)]),
            &[],
            RenderFailureMode::Degraded,
            &mut warnings,
        )
        .expect("degraded ok");
        assert!(warnings
            .iter()
            .any(|w| matches!(w, PdfWarning::UnsupportedImageFormat { format: "bmp", .. })));
    }

    #[test]
    fn zero_and_negative_geometry_follow_failure_mode() {
        for (w, h) in [(0.0, 40.0), (-1.0, 40.0), (50.0, 0.0), (f64::NAN, 40.0)] {
            let mut item = image_item("geom.png", TINY_PNG);
            item.size = Size { width: Pt(w), height: Pt(h) };
            let mut warnings = Vec::new();
            let err = write_pdf(
                &one_page(vec![PaintItem::Image(item.clone())]),
                &[],
                RenderFailureMode::Fatal,
                &mut warnings,
            )
            .expect_err("geometry fatal");
            assert!(matches!(err, PdfError::InvalidImageGeometry { .. }), "{w}x{h}: {err:?}");

            let mut warnings = Vec::new();
            write_pdf(
                &one_page(vec![PaintItem::Image(item)]),
                &[],
                RenderFailureMode::Degraded,
                &mut warnings,
            )
            .expect("degraded ok");
            assert!(
                warnings.iter().any(|w| matches!(w, PdfWarning::InvalidImageGeometry { .. })),
                "{w}x{h}"
            );
        }
    }

    // ── W2b c3: krilla `q → composed cm → Do → Q` 구조 잠금 ─────────
    //
    // 이 테스트는 `tests/support/mod.rs` 의 범용 bbox 추출기가 의존하는
    // 구조 가정(단일 합성 cm, 별도 Y-flip cm 없음)을 `write_pdf` 산출물
    // 에 직접 대고 검증한다. `write_pdf` 는 크레이트 내부 전용
    // (`pub(crate)`)이라 `tests/` 통합 테스트에서 호출할 수 없으므로
    // 이 락은 여기(단위 테스트)에만 존재할 수 있다 — 범용 파서는
    // `tests/*.rs` e2e 가 실사용으로 검증한다.

    /// 콘텐츠 스트림(비-이미지 FlateDecode 스트림)만 인플레이트해 반환.
    fn content_stream_bytes(pdf: &[u8]) -> Vec<u8> {
        let mut i = 0usize;
        loop {
            let off = find(&pdf[i..], b"stream").expect("content stream not found");
            let dict_end = i + off;
            let dict_start = dict_end.saturating_sub(256);
            let is_image = contains(&pdf[dict_start..dict_end], b"/Subtype/Image");
            let mut s = dict_end + b"stream".len();
            while pdf.get(s) == Some(&b'\r') || pdf.get(s) == Some(&b'\n') {
                s += 1;
            }
            let end = find(&pdf[s..], b"endstream").map(|e| s + e).expect("endstream not found");
            if !is_image {
                use std::io::Read as _;
                let mut out = Vec::new();
                let mut dec = flate2::read::ZlibDecoder::new(&pdf[s..end]);
                dec.read_to_end(&mut out).expect("inflate content stream");
                return out;
            }
            i = end + b"endstream".len();
        }
    }

    /// 인플레이트된 콘텐츠 텍스트에서 `<6 numbers> cm` 피연산자를 읽는다.
    fn find_single_cm_operands(content: &[u8]) -> [f64; 6] {
        let text = String::from_utf8_lossy(content);
        let idx = text.find(" cm").expect("cm operator not found");
        let before = &text[..idx];
        let nums: Vec<f64> = before
            .split_whitespace()
            .rev()
            .take(6)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|s| s.parse().expect("numeric cm operand"))
            .collect();
        nums.try_into().unwrap_or_else(|v: Vec<f64>| {
            panic!("expected exactly 6 cm operands, got {}: {v:?}", v.len())
        })
    }

    #[test]
    fn image_ctm_is_single_composed_cm_matching_page_yflip_translate_scale() {
        // 원본 origin=(10,20)pt size=(50,40)pt, 페이지 595x842pt (image_item/one_page 기본값).
        let pages = one_page(vec![PaintItem::Image(image_item("geom.png", TINY_PNG))]);
        let mut warnings = Vec::new();
        let bytes =
            write_pdf(&pages, &[], RenderFailureMode::Fatal, &mut warnings).expect("render");
        assert!(warnings.is_empty(), "{warnings:?}");

        let content = content_stream_bytes(&bytes);
        // 구조 잠금: q/Q 블록 정확히 1쌍, cm 정확히 1회, Form XObject 없음
        // (별도 Y-flip cm 도 없음 — krilla 가 translate+scale+flip 을 이미
        // 하나의 cm 으로 합성해 방출한다는 가정).
        let text = String::from_utf8_lossy(&content);
        assert_eq!(text.matches('q').count(), 1, "q 정확히 1개: {text}");
        assert_eq!(text.matches('Q').count(), 1, "Q 정확히 1개: {text}");
        assert_eq!(text.matches("cm").count(), 1, "cm 정확히 1개: {text}");
        assert!(!text.contains("/Form"), "Form XObject 없어야 함: {text}");

        let [a, b, c, d, e, f] = find_single_cm_operands(&content);
        let page_height = 842.0;
        let (origin_x, origin_y, w, h) = (10.0, 20.0, 50.0, 40.0);
        // 기대 합성: scale(w,h) + translate(x, page_height - y - h).
        assert!(approx(a, w), "a(scale-x)={a}");
        assert!(approx(b, 0.0), "b={b}");
        assert!(approx(c, 0.0), "c={c}");
        assert!(approx(d, h), "d(scale-y)={d}");
        assert!(approx(e, origin_x), "e(translate-x)={e}");
        assert!(approx(f, page_height - origin_y - h), "f(translate-y)={f}");

        // 실측 잠금의 본체: unit square 네 꼭짓점을 이 행렬로 변환하면
        // top-left bbox == (10, 20, 50, 40) ±0.01pt (§4 D3 검증 방법과 동일 산술).
        let corners: [(f64, f64); 4] = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]
            .map(|(x, y)| (a * x + c * y + e, b * x + d * y + f));
        let min_x = corners.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
        let max_x = corners.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
        let min_y = corners.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
        let max_y = corners.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
        let bbox_x = min_x;
        let bbox_y = page_height - max_y; // top-left 변환.
        let bbox_w = max_x - min_x;
        let bbox_h = max_y - min_y;
        assert!(approx(bbox_x, 10.0), "bbox.x={bbox_x}");
        assert!(approx(bbox_y, 20.0), "bbox.y={bbox_y}");
        assert!(approx(bbox_w, 50.0), "bbox.width={bbox_w}");
        assert!(approx(bbox_h, 40.0), "bbox.height={bbox_h}");
    }

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() <= 0.01
    }
}
