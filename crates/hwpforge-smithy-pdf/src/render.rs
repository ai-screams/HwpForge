//! 렌더 파이프라인 — replay(source) → shape/align(text) → Paint IR → backend.
//!
//! 좌표 흐름: source 는 HWPUNIT 정수, 셰이핑/배분은 분수 HWPUNIT, 이
//! 모듈의 Paint IR 방출 시점에 pt 로 **1회** 변환한다 (계획 §2).

use std::collections::{HashMap, HashSet};

use hwpforge_foundation::Color;

use crate::font::{embed_license, EmbedLicense, FaceStyle, FontResolver, ResolvedFont};
use crate::paint::{
    FontKey, GlyphRun, LineItem, Page, PaintItem, Point, PositionedGlyph, Pt, RectItem, Size,
};
use crate::source::replay_layout;
use crate::text::align::{place_line, NaturalLine};
use crate::text::shape::{shape_text, ShapedText};
use crate::{PdfError, PdfInput, PdfOptions, PdfOutput, PdfResult, PdfWarning, RenderFailureMode};

/// 렌더 컨텍스트 폰트 테이블 — (face 이름, 스타일) → [`FontKey`] 인터닝.
///
/// 물리 동일 face((경로, face 인덱스) 기준)는 [`FontKey`] 를 공유해 중복
/// 임베드를 막는다 (Degraded 강등이 (face, Bold) 와 (face, Regular) 를 같은
/// 실물로 해석하는 경우 등).
pub(crate) struct FontTable {
    resolver: FontResolver,
    keys: HashMap<(String, FaceStyle), FontKey>,
    by_identity: HashMap<(std::path::PathBuf, u32), FontKey>,
    fonts: Vec<ResolvedFont>,
}

impl FontTable {
    fn new(resolver: FontResolver) -> Self {
        Self { resolver, keys: HashMap::new(), by_identity: HashMap::new(), fonts: Vec::new() }
    }

    /// (face, style) 를 해석해 인터닝한다 (W4c 스타일 선택).
    ///
    /// - Regular = 현행 [`FontResolver::resolve`] (W2 계약).
    /// - 비-Regular = [`FontResolver::resolve_styled`]. miss 시
    ///   [`RenderFailureMode::Fatal`] 은 [`PdfError::FontStyleUnavailable`],
    ///   [`RenderFailureMode::Degraded`] 는 regular 강등 + 경고 1회
    ///   ((face, style) 캐시가 dedupe 를 겸한다).
    /// - [`PdfError::FontFaceAmbiguous`] 는 양 모드 모두 전파.
    fn key_for(
        &mut self,
        face: &str,
        style: FaceStyle,
        fallback: RenderFailureMode,
        location: &str,
        warnings: &mut Vec<PdfWarning>,
    ) -> PdfResult<FontKey> {
        let cache_key = (face.to_string(), style);
        if let Some(key) = self.keys.get(&cache_key) {
            return Ok(*key);
        }
        let resolved = if style == FaceStyle::Regular {
            self.resolver.resolve(face)?
        } else {
            match self.resolver.resolve_styled(face, style) {
                Ok(resolved) => resolved,
                Err(PdfError::FontUnresolved { .. }) => match fallback {
                    RenderFailureMode::Fatal => {
                        return Err(PdfError::FontStyleUnavailable {
                            face: face.to_string(),
                            style,
                            location: location.to_string(),
                        });
                    }
                    RenderFailureMode::Degraded => {
                        warnings.push(PdfWarning::FontStyleFallback {
                            face: face.to_string(),
                            requested: style,
                            location: location.to_string(),
                        });
                        self.resolver.resolve(face)?
                    }
                },
                Err(other) => return Err(other),
            }
        };
        let identity = (resolved.path.clone(), resolved.face_index);
        let key = if let Some(existing) = self.by_identity.get(&identity) {
            *existing // 동일 실물 = 이미 라이선스 판정 완료 (경고도 1회로 dedupe)
        } else {
            // 임베드 라이선스 게이트 (W4d) — 임베드할 그 바이트를 판정한다.
            match embed_license(&resolved.data, resolved.face_index) {
                EmbedLicense::Allowed => {}
                EmbedLicense::PreviewPrintOnly => {
                    let (_, hash) = crate::font::fingerprint(&resolved.data);
                    warnings.push(PdfWarning::FontEmbedPreviewPrint {
                        face: face.to_string(),
                        path: resolved.path.clone(),
                        fingerprint: format!("{hash:016x}"),
                    });
                }
                EmbedLicense::Denied(reason) => {
                    return Err(PdfError::FontEmbedRestricted {
                        face: face.to_string(),
                        path: resolved.path.clone(),
                        reason,
                    });
                }
            }
            let key = FontKey(self.fonts.len());
            self.fonts.push(resolved);
            self.by_identity.insert(identity, key);
            key
        };
        self.keys.insert(cache_key, key);
        Ok(key)
    }
}

/// 이미지 원자의 바이트를 interner 경유로 해석한다 (§3 D1/D3).
///
/// `Ok(Some)` = 자산 확보, `Ok(None)` = Degraded 생략(경고 push 완료),
/// `Err` = Fatal.
fn resolve_image_asset(
    input: &PdfInput<'_>,
    assets: &mut std::collections::HashMap<String, std::sync::Arc<Vec<u8>>>,
    img: &crate::source::LineImage,
    location: &str,
    options: &PdfOptions,
    warnings: &mut Vec<PdfWarning>,
) -> PdfResult<Option<std::sync::Arc<Vec<u8>>>> {
    if let Some(existing) = assets.get(&img.canonical_key) {
        return Ok(Some(existing.clone()));
    }
    match input.styles.image_data(&img.canonical_key) {
        Some(bytes) => {
            let arc = std::sync::Arc::new(bytes.to_vec());
            assets.insert(img.canonical_key.clone(), arc.clone());
            Ok(Some(arc))
        }
        None => match options.failure_mode {
            RenderFailureMode::Fatal => Err(PdfError::ImageDataMissing {
                key: img.canonical_key.clone(),
                location: location.to_string(),
            }),
            RenderFailureMode::Degraded => {
                warnings.push(PdfWarning::ImageDataMissing {
                    key: img.canonical_key.clone(),
                    location: location.to_string(),
                });
                Ok(None)
            }
        },
    }
}

/// charPr 하나의 렌더 재료를 해석한다 — 언어축 검사(W4c) + face/크기/색 조회
/// + 폰트 인터닝. 본문 run 과 합성 쪽번호(W5-b)가 같은 경로를 공유한다.
#[allow(clippy::too_many_arguments)]
fn resolve_char_style(
    input: &PdfInput<'_>,
    table: &mut FontTable,
    options: &PdfOptions,
    cs: hwpforge_foundation::CharShapeIndex,
    location: &str,
    warned_axis: &mut HashSet<usize>,
    warnings: &mut Vec<PdfWarning>,
) -> PdfResult<(FontKey, i32, Color)> {
    // 언어축 검사 — 단일 폰트 렌더는 축 불일치 시 오글리프: 기본 fatal,
    // Degraded 옵트인만 한글 축([0])으로 강등 + 경고 (charPr 당 1회).
    let axis_names = input.styles.char_font_axis_names(cs);
    if axis_names.len() > 1 {
        match options.failure_mode {
            RenderFailureMode::Fatal => {
                return Err(PdfError::FontAxisMismatch {
                    location: location.to_string(),
                    fonts: axis_names.iter().map(|s| (*s).to_string()).collect(),
                });
            }
            RenderFailureMode::Degraded => {
                if warned_axis.insert(cs.get()) {
                    warnings.push(PdfWarning::FontAxisFallback {
                        fonts: axis_names.iter().map(|s| (*s).to_string()).collect(),
                        location: location.to_string(),
                    });
                }
            }
        }
    }
    let face = input
        .styles
        .char_font_name(cs)
        .ok_or_else(|| PdfError::StyleUnavailable {
            what: "font name",
            location: location.to_string(),
        })?
        .to_string();
    let size_hwpunit = input
        .styles
        .char_font_size(cs)
        .ok_or_else(|| PdfError::StyleUnavailable {
            what: "font size",
            location: location.to_string(),
        })?
        .as_i32();
    let color = input.styles.char_text_color(cs).unwrap_or(Color::BLACK);
    let style = FaceStyle::from_flags(
        input.styles.char_bold(cs).unwrap_or(false),
        input.styles.char_italic(cs).unwrap_or(false),
    );
    let key = table.key_for(&face, style, options.failure_mode, location, warnings)?;
    Ok((key, size_hwpunit, color))
}

/// 문서를 PDF 로 렌더한다 (조판 캐시 재생 — 계산이 아니라 재생).
///
/// # Errors
///
/// admission 행렬([`crate::source::replay_layout`]) + 스타일 결손
/// ([`PdfError::StyleUnavailable`]) + 폰트 미해결([`PdfError::FontUnresolved`],
/// fallback 금지) + 백엔드 실패([`PdfError::Backend`]).
/// 원자별 준비 결과 — 텍스트 셰이핑 or 이미지 자산 (W2a §3 D4).
enum PreparedAtom {
    Run(PreparedRun),
    Image { canonical_key: String, data: std::sync::Arc<Vec<u8>>, width: i32, height: i32 },
}

struct PreparedRun {
    key: FontKey,
    size_hwpunit: i32,
    color: Color,
    shaped: ShapedText,
    text: String,
}

/// 문서를 조판 캐시 재생으로 PDF 바이트로 렌더한다 (파이프라인 진입점).
pub fn render_document(input: &PdfInput<'_>, options: &PdfOptions) -> PdfResult<PdfOutput> {
    let layout = replay_layout(input, options)?;
    let mut warnings = layout.warnings;
    let mut table =
        FontTable::new(FontResolver::with_discovery(&options.font_dirs, options.discovery)?);
    let pages = build_paint_pages(input, options, &layout.pages, &mut table, &mut warnings)?;
    let bytes =
        crate::backend::write_pdf(&pages, &table.fonts, options.failure_mode, &mut warnings)?;
    Ok(PdfOutput { bytes, warnings })
}

/// PageLayout 들을 Paint IR 페이지로 변환한다 (W2a 시임 — synthetic
/// 혼합-atom 테스트가 직접 호출한다).
pub(crate) fn build_paint_pages(
    input: &PdfInput<'_>,
    options: &PdfOptions,
    layout_pages: &[crate::source::PageLayout],
    table: &mut FontTable,
    warnings: &mut Vec<PdfWarning>,
) -> PdfResult<Vec<crate::paint::Page>> {
    // W2a §3 D3: canonical key 당 원본 바이트 1회 복사 (asset interner).
    let mut image_assets: std::collections::HashMap<String, std::sync::Arc<Vec<u8>>> =
        std::collections::HashMap::new();
    // 언어축 불일치 경고는 charPr 당 1회 (Degraded 모드).
    let mut warned_axis: HashSet<usize> = HashSet::new();

    let mut pages = Vec::with_capacity(layout_pages.len());
    for page in layout_pages {
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
            if line.atoms.is_empty() {
                continue; // 빈 줄 — 세로 공간은 캐시가 이미 소비했다.
            }
            // 1) 원자별 준비 — 텍스트는 스타일 조회 + 셰이핑, 이미지는
            //    바이트 해석(interner) 후 폭만 기여 (W2a §3 D4).
            //
            // 줄 끝 공백은 셰이핑에서 제외한다: 한컴 캐시의 줄 텍스트는
            // 뒤따르는 공백을 앞 줄에 귀속시키지만, 렌더에서 그 공백은
            // 그리지도 JUSTIFY 분모에 넣지도 않는다 (W0 실측 — 한컴
            // 양쪽정렬 줄 끝 = 마지막 가시 글리프가 우변 밀착).
            // trim 은 **마지막 가시 원자가 텍스트일 때만** 적용한다.
            let last_atom_idx = line.atoms.len() - 1;
            let mut prepared: Vec<PreparedAtom> = Vec::with_capacity(line.atoms.len());
            for (atom_idx, atom) in line.atoms.iter().enumerate() {
                let run = match atom {
                    crate::source::LineAtom::Image(img) => {
                        let data = match resolve_image_asset(
                            input,
                            &mut image_assets,
                            img,
                            &line.location,
                            options,
                            warnings,
                        )? {
                            Some(data) => data,
                            None => continue, // Degraded: 경고 후 생략
                        };
                        prepared.push(PreparedAtom::Image {
                            canonical_key: img.canonical_key.clone(),
                            data,
                            width: img.width,
                            height: img.height,
                        });
                        continue;
                    }
                    crate::source::LineAtom::TextBox(_) => {
                        // W4 w2 는 타입([`LineTextBox`])만 정의한다 — source→paint
                        // 배선(내부 replay·clip·박스 페인트)은 w3/w4 몫이다.
                        // production admission 이 아직 글상자를 방출하지 않아
                        // 도달 불가지만, 무음 드롭 금지 원칙에 따라 fail-closed
                        // (w3 이 이 arm 을 실제 렌더로 대체한다).
                        return Err(PdfError::UnsupportedContent {
                            kind: "inline text box",
                            location: line.location.clone(),
                        });
                    }
                    crate::source::LineAtom::Text(run) => run,
                };
                let text = if atom_idx == last_atom_idx {
                    run.text.trim_end_matches(' ')
                } else {
                    run.text.as_str()
                };
                if text.is_empty() {
                    continue;
                }
                let (key, size_hwpunit, color) = resolve_char_style(
                    input,
                    table,
                    options,
                    run.char_shape,
                    &line.location,
                    &mut warned_axis,
                    warnings,
                )?;
                let font = &table.fonts[key.0];
                let shaped = shape_text(&font.data, font.face_index, text, size_hwpunit)?;
                // tofu 게이트 (W6 §5f): 폰트에 없는 글리프는 조용히 □ 로
                // 찍힌다 — 기본 fatal, Degraded 만 경고 후 렌더.
                if shaped.missing_glyphs > 0 {
                    match options.failure_mode {
                        RenderFailureMode::Fatal => {
                            return Err(PdfError::GlyphsUnavailable {
                                face: font.face_name.clone(),
                                count: shaped.missing_glyphs,
                                location: line.location.clone(),
                            });
                        }
                        RenderFailureMode::Degraded => {
                            warnings.push(PdfWarning::MissingGlyphs {
                                face: font.face_name.clone(),
                                count: shaped.missing_glyphs,
                                location: line.location.clone(),
                            });
                        }
                    }
                }
                prepared.push(PreparedAtom::Run(PreparedRun {
                    key,
                    size_hwpunit,
                    color,
                    shaped,
                    text: text.to_string(),
                }));
            }

            // 2) 줄 단위 정렬 배분 (자연폭·공백 수는 run 합산).
            let natural = NaturalLine {
                width: prepared
                    .iter()
                    .map(|a| match a {
                        PreparedAtom::Run(p) => p.shaped.natural_width(),
                        PreparedAtom::Image { width, .. } => f64::from(*width),
                    })
                    .sum(),
                // JUSTIFY 분모는 텍스트 공백만 (이미지는 신축 불가 원자).
                space_count: prepared
                    .iter()
                    .map(|a| match a {
                        PreparedAtom::Run(p) => p.shaped.space_count(),
                        PreparedAtom::Image { .. } => 0,
                    })
                    .sum(),
            };
            // 줄 넘침 표면화 (W6 §5f): 자간/장평 미carry 로 우리 자연폭이
            // 캐시 줄 상자를 넘으면 우측으로 삐져나간다 — 무음 금지.
            let overflow = natural.width - f64::from(line.line_box.horzsize);
            if overflow > 10.0 {
                warnings.push(PdfWarning::LineOverflow {
                    location: line.location.clone(),
                    excess: overflow.round() as i32,
                });
            }
            let placement = place_line(line.alignment, line.line_box, natural, line.is_last_line);
            if placement.needs_warning {
                warnings
                    .push(PdfWarning::AlignmentApproximated { location: line.location.clone() });
            }

            // 3) 절대 배치 (분수 HWPUNIT) → run 별 GlyphRun (pt 변환 = 여기 1회).
            let baseline_y = Pt::from_hwpunit(line.baseline_y);
            let mut x = placement.origin_x;
            for atom in prepared {
                let p = match atom {
                    PreparedAtom::Image { canonical_key, data, width, height } => {
                        items.push(PaintItem::Image(crate::paint::ImageItem {
                            canonical_key,
                            data,
                            origin: Point {
                                x: Pt::from_hwpunit_f64(x),
                                // W0a 실측 계약: 이미지 top = 줄 top.
                                y: Pt::from_hwpunit(line.top_y),
                            },
                            size: Size {
                                width: Pt::from_hwpunit(width),
                                height: Pt::from_hwpunit(height),
                            },
                            location: line.location.clone(),
                        }));
                        x += f64::from(width);
                        continue;
                    }
                    PreparedAtom::Run(p) => p,
                };
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
        // W5-b: 합성 쪽번호 — 전용 스타일 charPr 로 셰이핑해 페이지 폭 중앙 ·
        // em 하단 앵커에 배치한다 (§8c 실측: baseline = 앵커 − hhea descent).
        if let Some(pn) = &page.page_number {
            let (key, size_hwpunit, color) = resolve_char_style(
                input,
                table,
                options,
                pn.char_shape,
                &pn.location,
                &mut warned_axis,
                warnings,
            )?;
            let font = &table.fonts[key.0];
            let shaped = shape_text(&font.data, font.face_index, &pn.text, size_hwpunit)?;
            if shaped.missing_glyphs > 0 {
                match options.failure_mode {
                    RenderFailureMode::Fatal => {
                        return Err(PdfError::GlyphsUnavailable {
                            face: font.face_name.clone(),
                            count: shaped.missing_glyphs,
                            location: pn.location.clone(),
                        });
                    }
                    RenderFailureMode::Degraded => {
                        warnings.push(PdfWarning::MissingGlyphs {
                            face: font.face_name.clone(),
                            count: shaped.missing_glyphs,
                            location: pn.location.clone(),
                        });
                    }
                }
            }
            // 가로 = 페이지 폭 중앙 − 자연폭/2 (여백 무관 — R6 실측 Δ0.08pt).
            let origin_x = (f64::from(page.width) - shaped.natural_width()) / 2.0;
            let baseline_y = f64::from(pn.anchor_bottom) - shaped.descent;
            let byte_len = pn.text.len();
            let mut x = origin_x;
            let mut glyphs = Vec::with_capacity(shaped.glyphs.len());
            for (gi, g) in shaped.glyphs.iter().enumerate() {
                let next_cluster = shaped.glyphs.get(gi + 1).map_or(byte_len, |n| n.cluster);
                glyphs.push(PositionedGlyph {
                    glyph_id: g.glyph_id,
                    x_offset: Pt::from_hwpunit_f64(x - origin_x),
                    advance: Pt::from_hwpunit_f64(g.advance),
                    text_range: g.cluster..next_cluster,
                });
                x += g.advance;
            }
            items.push(PaintItem::Glyphs(GlyphRun {
                font: key,
                size: Pt::from_hwpunit(size_hwpunit),
                color,
                baseline: Point {
                    x: Pt::from_hwpunit_f64(origin_x),
                    y: Pt::from_hwpunit_f64(baseline_y),
                },
                text: pn.text.clone(),
                glyphs,
            }));
        }

        pages.push(Page {
            size: Size {
                width: Pt::from_hwpunit(page.width),
                height: Pt::from_hwpunit(page.height),
            },
            items,
        });
    }

    Ok(pages)
}

#[cfg(test)]
mod atom_tests {
    use super::*;
    use crate::source::{LaidLine, LineAtom, LineImage, PageLayout};
    use crate::text::align::LineBox;
    use crate::StyleLookup;
    use hwpforge_foundation::Alignment;

    /// synthetic PageLayout 테스트용 최소 유효 문서 — build_paint_pages 는
    /// document 를 직접 읽지 않으므로 내용은 무관하다 (validate 통과용).
    fn minimal_doc() -> hwpforge_core::document::Document<hwpforge_core::document::Validated> {
        let mut doc = hwpforge_core::document::Document::new();
        doc.add_section(hwpforge_core::section::Section::with_paragraphs(
            vec![hwpforge_core::paragraph::Paragraph::with_runs(
                vec![hwpforge_core::run::Run::text(
                    "x",
                    hwpforge_foundation::CharShapeIndex::new(0),
                )],
                hwpforge_foundation::ParaShapeIndex::new(0),
            )],
            hwpforge_core::page::PageSettings::a4(),
        ));
        doc.validate().expect("minimal doc")
    }

    /// 이미지 바이트를 공급하는 테스트 lookup (브리지 대역).
    struct ImgStyles;
    impl StyleLookup for ImgStyles {
        fn image_data(&self, key: &str) -> Option<&[u8]> {
            (key == "img1.png").then_some(&[1, 2, 3][..])
        }
    }

    fn image_only_layout() -> PageLayout {
        PageLayout {
            width: 59528,
            height: 84188,
            rects: Vec::new(),
            borders: Vec::new(),
            lines: vec![LaidLine {
                location: "s0/p0/l0".into(),
                atoms: vec![LineAtom::Image(LineImage {
                    canonical_key: "img1.png".into(),
                    width: 2000,
                    height: 3000,
                })],
                top_y: 5000,
                baseline_y: 5000 + 2550,
                line_box: LineBox { horzpos: 8504, horzsize: 42520 },
                is_last_line: true,
                alignment: Alignment::Left,
            }],
            page_number: None,
        }
    }

    #[test]
    fn image_atom_paints_at_line_top_with_hwpunit_size() {
        // 폰트 불요 (이미지 단독 줄) — atom → PaintItem::Image 변환과
        // W0a 계약(이미지 top = 줄 top)·x 원점(LEFT = line_box.horzpos)을
        // 잠근다.
        let doc = minimal_doc();
        let input = PdfInput { document: &doc, styles: &ImgStyles };
        let options = PdfOptions::default();
        let mut table = FontTable::new(
            FontResolver::with_discovery(&[], crate::font::FontDiscovery::ExplicitOnly)
                .expect("resolver"),
        );
        let mut warnings = Vec::new();
        let pages =
            build_paint_pages(&input, &options, &[image_only_layout()], &mut table, &mut warnings)
                .expect("paint");
        assert!(warnings.is_empty(), "{warnings:?}");
        let images: Vec<_> = pages[0]
            .items
            .iter()
            .filter_map(|i| match i {
                PaintItem::Image(item) => Some(item),
                _ => None,
            })
            .collect();
        assert_eq!(images.len(), 1);
        let img = images[0];
        assert_eq!(img.canonical_key, "img1.png");
        assert_eq!(*img.data, vec![1, 2, 3]);
        // LEFT 정렬 원점 = line_box.horzpos, top = top_y (baseline 아님).
        assert!((img.origin.x.0 - Pt::from_hwpunit(8504).0).abs() < 1e-9);
        assert!((img.origin.y.0 - Pt::from_hwpunit(5000).0).abs() < 1e-9);
        assert!((img.size.width.0 - Pt::from_hwpunit(2000).0).abs() < 1e-9);
        assert!((img.size.height.0 - Pt::from_hwpunit(3000).0).abs() < 1e-9);
    }

    #[test]
    fn missing_image_asset_follows_failure_mode() {
        struct NoImg;
        impl StyleLookup for NoImg {}
        let doc = minimal_doc();
        let input = PdfInput { document: &doc, styles: &NoImg };
        let mut table = FontTable::new(
            FontResolver::with_discovery(&[], crate::font::FontDiscovery::ExplicitOnly)
                .expect("resolver"),
        );

        let options = PdfOptions::default();
        let mut warnings = Vec::new();
        let err =
            build_paint_pages(&input, &options, &[image_only_layout()], &mut table, &mut warnings)
                .expect_err("fatal");
        assert!(matches!(err, PdfError::ImageDataMissing { .. }), "{err:?}");

        let options =
            PdfOptions { failure_mode: RenderFailureMode::Degraded, ..PdfOptions::default() };
        let mut warnings = Vec::new();
        let pages =
            build_paint_pages(&input, &options, &[image_only_layout()], &mut table, &mut warnings)
                .expect("degraded ok");
        assert!(warnings.iter().any(|w| matches!(w, PdfWarning::ImageDataMissing { .. })));
        assert!(pages[0].items.iter().all(|i| !matches!(i, PaintItem::Image(_))));
    }
}
