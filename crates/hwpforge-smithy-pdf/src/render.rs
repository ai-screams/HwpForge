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
    canonical_key: &str,
    location: &str,
    options: &PdfOptions,
    warnings: &mut Vec<PdfWarning>,
) -> PdfResult<Option<std::sync::Arc<Vec<u8>>>> {
    if let Some(existing) = assets.get(canonical_key) {
        return Ok(Some(existing.clone()));
    }
    match input.styles.image_data(canonical_key) {
        Some(bytes) => {
            let arc = std::sync::Arc::new(bytes.to_vec());
            assets.insert(canonical_key.to_string(), arc.clone());
            Ok(Some(arc))
        }
        None => match options.failure_mode {
            RenderFailureMode::Fatal => Err(PdfError::ImageDataMissing {
                key: canonical_key.to_string(),
                location: location.to_string(),
            }),
            RenderFailureMode::Degraded => {
                warnings.push(PdfWarning::ImageDataMissing {
                    key: canonical_key.to_string(),
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
/// 원자별 준비 결과 — 텍스트 셰이핑 · 이미지 자산 (W2a §3 D4) · 인라인
/// 글상자 (W4 w3 — 폭만 소비하고 배치 단계에서 clip 그룹으로 낮춘다).
enum PreparedAtom<'a> {
    Run(PreparedRun),
    Image { canonical_key: String, data: std::sync::Arc<Vec<u8>>, width: i32, height: i32 },
    TextBox { tb: &'a crate::source::LineTextBox },
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
        // W5 w1b: 앵커 이미지 = 배경 (본문이 회피하므로 맨 아래에 깔린다 —
        // §9e z-order). 인라인 이미지와 같은 asset interner 를 공유한다.
        for ai in &page.anchored_images {
            let Some(data) = resolve_image_asset(
                input,
                &mut image_assets,
                &ai.canonical_key,
                &ai.location,
                options,
                warnings,
            )?
            else {
                continue; // Degraded: 경고 후 생략.
            };
            items.push(PaintItem::Image(crate::paint::ImageItem {
                canonical_key: ai.canonical_key.clone(),
                data,
                origin: Point { x: Pt::from_hwpunit(ai.x), y: Pt::from_hwpunit(ai.y) },
                size: Size {
                    width: Pt::from_hwpunit(ai.width),
                    height: Pt::from_hwpunit(ai.height),
                },
                location: ai.location.clone(),
            }));
        }
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
            let line_items = render_line(
                input,
                options,
                table,
                &mut image_assets,
                &mut warned_axis,
                line,
                warnings,
            )?;
            items.extend(line_items);
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

/// 배치가 끝난 한 줄([`crate::source::LaidLine`])을 Paint 항목들로 낮춘다.
///
/// 원자별로 텍스트는 셰이핑, 이미지는 자산 해석 후 폭 소비, 인라인 글상자는
/// clip 그룹([`build_textbox_clip_group`])으로 방출한다. 글상자 내부 줄은 이
/// 함수를 **재귀**로 호출한다. 종료 보장 = production producer
/// (`build_line_text_box`)가 내부 비텍스트 run 을 거부해 실제 깊이는 1
/// — 재귀 자체는 원자 종류에 일반적이라 테스트가 내부 이미지 원자를
/// 합성해도 동작한다 (리뷰 Low-3 문구 정정). 반환 Vec 순서 = z-order.
#[allow(clippy::too_many_arguments)]
/// sub-line 인라인 이미지가 baseline 아래로 얹히는 descent 비율 k 의 분자
/// (`descent = img_h × NUM / DEN`) — Hancom 내부 상수 (TEXTBOX_TEXT_MARGIN=283
/// 과 같은 계열의 매직 상수). 이미지 bottom = `baseline + k × img_h` 로, 이미지가
/// 자기 크기에 비례한 descent 를 가진 글리프처럼 baseline 에 정렬된다.
///
/// 실측 k ≈ 0.152 (subline_image_v2 body 4측점 0.1499~0.1523). **폰트 메트릭
/// 유도가 아니다** — hhea descent/em 0.23·OS/2 typo 0.170·win 0.23 어느 정의와도
/// 불일치(적대 리뷰 r2 가 TTF 직접 파싱으로 실증). lineseg 비율
/// ((vertsize−baseline)/vertsize = 0.150)과 이 상수(0.152)는 현 데이터(단일 글꼴
/// 패밀리·85:15 고정 줄 메트릭)로 비식별이며, 상수 vs 비율 판별은 **다른 글꼴
/// 패밀리 실측(w2 v3 fixture) 몫**이다 — 그때까지 폰트 스케일-불변 상수가 안전하다.
const SUBLINE_IMAGE_DESCENT_RATIO_NUM: i64 = 152;
/// [`SUBLINE_IMAGE_DESCENT_RATIO_NUM`] 의 분모 (per-mille 표현).
const SUBLINE_IMAGE_DESCENT_RATIO_DEN: i64 = 1000;

/// 인라인 이미지 원자의 세로 top(HWPUNIT)을 유도한다 (W1 §10c-2, r2 상수형).
///
/// 두 레짐을 **분기**로 처리한다 — 상수 k 로는 한 공식으로 일반화되지 않으므로
/// 이미지-지배 경로를 그대로 둔다 (r2 ②, 회귀 위험 0):
///
/// - **이미지-지배**(`img_h >= vertsize`): 이미지가 줄 높이를 결정 → 기존
///   계약대로 top = `line_top`.
/// - **sub-line**(`img_h < vertsize`, 텍스트-지배 줄): 이미지가 baseline 에 얹혀
///   자기 크기 비례 descent 를 가진다 —
///   `image_top = line_top + baseline − img_h + img_h × k`,
///   k = [`SUBLINE_IMAGE_DESCENT_RATIO_NUM`] / [`SUBLINE_IMAGE_DESCENT_RATIO_DEN`].
///
/// 정수(HWPUNIT) 산술 — descent 항은 절단 나눗셈(기존 관례), 중간 곱은 i64 로
/// 승격해 오버플로가 없다. `vertsize ≤ 0`(퇴화 줄)은 세로 보정 불가 → `line_top`.
fn inline_image_top(line_top: i32, baseline: i32, vertsize: i32, img_h: i32) -> i32 {
    // 이미지-지배(또는 퇴화 줄) = 기존 계약(top=line_top). 상수 k 로는 이 경로가
    // line_top 으로 환원되지 않으므로 명시 분기로 남긴다.
    if vertsize <= 0 || img_h >= vertsize {
        return line_top;
    }
    let descent =
        (i64::from(img_h) * SUBLINE_IMAGE_DESCENT_RATIO_NUM) / SUBLINE_IMAGE_DESCENT_RATIO_DEN;
    let top = i64::from(line_top) + i64::from(baseline) - i64::from(img_h) + descent;
    top.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

fn render_line(
    input: &PdfInput<'_>,
    options: &PdfOptions,
    table: &mut FontTable,
    image_assets: &mut HashMap<String, std::sync::Arc<Vec<u8>>>,
    warned_axis: &mut HashSet<usize>,
    line: &crate::source::LaidLine,
    warnings: &mut Vec<PdfWarning>,
) -> PdfResult<Vec<PaintItem>> {
    let mut out = Vec::new();
    if line.atoms.is_empty() {
        return Ok(out); // 빈 줄 — 세로 공간은 캐시가 이미 소비했다.
    }
    // 1) 원자별 준비 — 텍스트는 스타일 조회 + 셰이핑, 이미지는 바이트
    //    해석(interner) 후 폭만 기여 (W2a §3 D4), 글상자는 폭만 기여 (W4 w3).
    //
    // 줄 끝 공백은 셰이핑에서 제외한다: 한컴 캐시의 줄 텍스트는 뒤따르는
    // 공백을 앞 줄에 귀속시키지만, 렌더에서 그 공백은 그리지도 JUSTIFY
    // 분모에 넣지도 않는다 (W0 실측). trim 은 **마지막 가시 원자가
    // 텍스트일 때만** 적용한다.
    let last_atom_idx = line.atoms.len() - 1;
    let mut prepared: Vec<PreparedAtom<'_>> = Vec::with_capacity(line.atoms.len());
    for (atom_idx, atom) in line.atoms.iter().enumerate() {
        let run = match atom {
            crate::source::LineAtom::Image(img) => {
                let data = match resolve_image_asset(
                    input,
                    image_assets,
                    &img.canonical_key,
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
            crate::source::LineAtom::TextBox(tb) => {
                prepared.push(PreparedAtom::TextBox { tb });
                continue;
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
            warned_axis,
            warnings,
        )?;
        let font = &table.fonts[key.0];
        let shaped = shape_text(&font.data, font.face_index, text, size_hwpunit)?;
        // tofu 게이트 (W6 §5f): 폰트에 없는 글리프는 조용히 □ 로 찍힌다 —
        // 기본 fatal, Degraded 만 경고 후 렌더.
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
                PreparedAtom::TextBox { tb } => f64::from(tb.width),
            })
            .sum(),
        // JUSTIFY 분모는 텍스트 공백만 (이미지·글상자는 신축 불가 원자).
        space_count: prepared
            .iter()
            .map(|a| match a {
                PreparedAtom::Run(p) => p.shaped.space_count(),
                PreparedAtom::Image { .. } | PreparedAtom::TextBox { .. } => 0,
            })
            .sum(),
    };
    // 줄 넘침 표면화 (W6 §5f): 자간/장평 미carry 로 우리 자연폭이 캐시 줄
    // 상자를 넘으면 우측으로 삐져나간다 — 무음 금지.
    let overflow = natural.width - f64::from(line.line_box.horzsize);
    if overflow > 10.0 {
        warnings.push(PdfWarning::LineOverflow {
            location: line.location.clone(),
            excess: overflow.round() as i32,
        });
    }
    let placement = place_line(line.alignment, line.line_box, natural, line.is_last_line);
    if placement.needs_warning {
        warnings.push(PdfWarning::AlignmentApproximated { location: line.location.clone() });
    }

    // 3) 절대 배치 (분수 HWPUNIT) → run 별 GlyphRun (pt 변환 = 여기 1회).
    let baseline_y = Pt::from_hwpunit(line.baseline_y);
    let mut x = placement.origin_x;
    for atom in prepared {
        let p = match atom {
            PreparedAtom::Image { canonical_key, data, width, height } => {
                // 세로 배치 = §10c-2 분기 (이미지-지배는 line_top, sub-line 은
                // baseline + 상수 k descent). baseline = baseline_y − top_y.
                let image_top = inline_image_top(
                    line.top_y,
                    line.baseline_y - line.top_y,
                    line.vertsize,
                    height,
                );
                out.push(PaintItem::Image(crate::paint::ImageItem {
                    canonical_key,
                    data,
                    origin: Point { x: Pt::from_hwpunit_f64(x), y: Pt::from_hwpunit(image_top) },
                    size: Size { width: Pt::from_hwpunit(width), height: Pt::from_hwpunit(height) },
                    location: line.location.clone(),
                }));
                x += f64::from(width);
                continue;
            }
            PreparedAtom::TextBox { tb } => {
                // 박스 원점 = (전진 전 x, 호스트 줄 top). 내부 줄은 clip 그룹
                // 안에서 절대 배치된다. z-order 는 셀 계약과 동일 계열:
                // 채움 → 내용(clip) → 테두리 (테두리가 경계 글자 위에 —
                // 한컴 박스 외곽선 관례).
                let box_x = x.round() as i32;
                if let Some(fill) = tb.style.as_ref().and_then(|s| s.fill_color) {
                    out.push(PaintItem::Rect(RectItem {
                        x: Pt::from_hwpunit(box_x),
                        y: Pt::from_hwpunit(line.top_y),
                        width: Pt::from_hwpunit(tb.width),
                        height: Pt::from_hwpunit(tb.height),
                        color: fill,
                    }));
                }
                let group = build_textbox_clip_group(
                    input,
                    options,
                    table,
                    image_assets,
                    warned_axis,
                    tb,
                    box_x,
                    line.top_y,
                    &line.location,
                    warnings,
                )?;
                out.push(PaintItem::Clipped(group));
                if let Some(style) = tb.style.as_ref() {
                    if let Some(border) = style.line_color {
                        out.extend(textbox_border_lines(
                            box_x,
                            line.top_y,
                            tb.width,
                            tb.height,
                            style.line_width,
                            border,
                        ));
                    }
                }
                x += f64::from(tb.width);
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
        out.push(PaintItem::Glyphs(GlyphRun {
            font: p.key,
            size: Pt::from_hwpunit(p.size_hwpunit),
            color: p.color,
            baseline: Point { x: Pt::from_hwpunit_f64(origin_x), y: baseline_y },
            text: p.text,
            glyphs,
        }));
    }
    Ok(out)
}

/// 글상자 테두리 4변을 [`PaintItem::Line`] 으로 방출한다 (W4 w4).
///
/// 좌표는 박스 사각형 모서리 정확치 (표 괘선과 동일 관례 — cap 연장
/// 없음). `line_width` 0 이하는 방출하지 않는다 (무의미 스트로크 방지).
fn textbox_border_lines(
    box_x: i32,
    box_top: i32,
    width: i32,
    height: i32,
    line_width: i32,
    color: hwpforge_foundation::Color,
) -> Vec<PaintItem> {
    if line_width <= 0 {
        return Vec::new();
    }
    let (x0, y0) = (Pt::from_hwpunit(box_x), Pt::from_hwpunit(box_top));
    let (x1, y1) = (Pt::from_hwpunit(box_x + width), Pt::from_hwpunit(box_top + height));
    let w = Pt::from_hwpunit(line_width);
    let edge = |from: Point, to: Point| PaintItem::Line(LineItem { from, to, width: w, color });
    vec![
        edge(Point { x: x0, y: y0 }, Point { x: x1, y: y0 }),
        edge(Point { x: x0, y: y1 }, Point { x: x1, y: y1 }),
        edge(Point { x: x0, y: y0 }, Point { x: x0, y: y1 }),
        edge(Point { x: x1, y: y0 }, Point { x: x1, y: y1 }),
    ]
}

/// 인라인 글상자를 clip 그룹([`crate::paint::ClipGroup`])으로 낮춘다 (W4 w3).
///
/// 내부 줄을 **박스 원점 + textMargin(기본 283) + vertAlign 시프트**로 절대
/// 배치해 [`render_line`] 로 재귀 렌더하고, 전체를 **박스 사각형**(width ×
/// `LineTextBox::height`)의 clip 으로 감싼다. clip 높이는 Core 선언 박스
/// 높이지 host 줄 vertsize(넘침 시 더 큼)가 아니다 — 그래야 overflow 가
/// 실제로 잘린다 (§8f ③). 박스 채움/테두리 페인트는 호출부(atom arm)가
/// clip 그룹 바깥에서 감싼다 (채움 → 내용 → 테두리).
#[allow(clippy::too_many_arguments)]
fn build_textbox_clip_group(
    input: &PdfInput<'_>,
    options: &PdfOptions,
    table: &mut FontTable,
    image_assets: &mut HashMap<String, std::sync::Arc<Vec<u8>>>,
    warned_axis: &mut HashSet<usize>,
    tb: &crate::source::LineTextBox,
    box_x: i32,
    box_top: i32,
    host_location: &str,
    warnings: &mut Vec<PdfWarning>,
) -> PdfResult<crate::paint::ClipGroup> {
    let margin = crate::source::TEXTBOX_TEXT_MARGIN;
    // vertAlign 시프트 — 내부 캐시는 vertAlign 미반영(§8f 실측: 3종 모두
    // 내부 vertpos=0), 렌더가 시프트한다. interior = box.height − 상하
    // textMargin, content = 내부 content-extent (admission 과 동일 산식).
    // Center/Bottom 은 여유가 음수(overflow)면 0 (상단 밀착).
    let content_extent = crate::source::textbox_content_extent(&tb.inner_lines, host_location)?;
    let interior = tb.height - 2 * margin;
    let valign_shift = match tb.vert_align {
        hwpforge_foundation::VerticalAlign::Top => 0,
        hwpforge_foundation::VerticalAlign::Center => ((interior - content_extent) / 2).max(0),
        hwpforge_foundation::VerticalAlign::Bottom => (interior - content_extent).max(0),
        // non_exhaustive 미래 variant — 보수적으로 상단 밀착 (시프트 없음).
        _ => 0,
    };
    // 내부 줄을 박스-상대 → 페이지 절대 좌표로 옮겨 재귀 렌더한다.
    let mut group_items = Vec::new();
    for (li, inner) in tb.inner_lines.iter().enumerate() {
        let abs_top = box_top + margin + valign_shift + inner.seg.vertpos;
        let laid = crate::source::LaidLine {
            location: format!("{host_location}/tb/l{li}"),
            atoms: inner.atoms.clone(),
            top_y: abs_top,
            baseline_y: abs_top + inner.seg.baseline,
            vertsize: inner.seg.vertsize,
            line_box: crate::text::align::LineBox {
                horzpos: box_x + margin + inner.seg.horzpos,
                horzsize: inner.seg.horzsize,
            },
            is_last_line: inner.is_last_line,
            alignment: inner.alignment,
        };
        let inner_items =
            render_line(input, options, table, image_assets, warned_axis, &laid, warnings)?;
        group_items.extend(inner_items);
    }
    Ok(crate::paint::ClipGroup {
        origin: Point { x: Pt::from_hwpunit(box_x), y: Pt::from_hwpunit(box_top) },
        size: Size { width: Pt::from_hwpunit(tb.width), height: Pt::from_hwpunit(tb.height) },
        items: group_items,
    })
}

#[cfg(test)]
mod atom_tests {
    use super::*;
    use crate::source::{LaidAnchoredImage, LaidLine, LaidRect, LineAtom, LineImage, PageLayout};
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
            anchored_images: Vec::new(),
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
                vertsize: 3000, // 이미지-지배(img_h==vertsize) → top==line_top.
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

    /// W5 w1b: 앵커 이미지는 `PaintItem::Image` 로 절대 배치되고, **배경**
    /// (셀 배경 rect 보다도 먼저 = z-order 맨 아래)에 온다. origin/size 는
    /// source 층 HWPUNIT 을 pt 로 1:1 환산한다.
    #[test]
    fn anchored_image_paints_as_background_before_rects() {
        let layout = PageLayout {
            width: 59528,
            height: 84188,
            anchored_images: vec![LaidAnchoredImage {
                location: "s0/p0".into(),
                canonical_key: "img1.png".into(),
                x: 10764,
                y: 11020,
                width: 4000,
                height: 3000,
            }],
            rects: vec![LaidRect {
                location: "s0/p0/cell".into(),
                x: 0,
                y: 0,
                width: 1000,
                height: 1000,
                color: Color::from_rgb(200, 200, 200),
            }],
            borders: Vec::new(),
            lines: Vec::new(),
            page_number: None,
        };
        let doc = minimal_doc();
        let input = PdfInput { document: &doc, styles: &ImgStyles };
        let options = PdfOptions::default();
        let mut table = FontTable::new(
            FontResolver::with_discovery(&[], crate::font::FontDiscovery::ExplicitOnly)
                .expect("resolver"),
        );
        let mut warnings = Vec::new();
        let pages = build_paint_pages(&input, &options, &[layout], &mut table, &mut warnings)
            .expect("paint");
        assert!(warnings.is_empty(), "{warnings:?}");
        // 첫 항목 = 앵커 이미지 (배경), 그 다음이 셀 배경 rect.
        assert!(
            matches!(&pages[0].items[0], PaintItem::Image(_)),
            "앵커 이미지가 맨 아래 배경이어야 함: {:?}",
            pages[0].items[0]
        );
        assert!(matches!(&pages[0].items[1], PaintItem::Rect(_)), "rect 는 이미지 뒤");
        let PaintItem::Image(img) = &pages[0].items[0] else { unreachable!() };
        assert_eq!(img.canonical_key, "img1.png");
        assert_eq!(*img.data, vec![1, 2, 3]);
        assert!((img.origin.x.0 - Pt::from_hwpunit(10764).0).abs() < 1e-9);
        assert!((img.origin.y.0 - Pt::from_hwpunit(11020).0).abs() < 1e-9);
        assert!((img.size.width.0 - Pt::from_hwpunit(4000).0).abs() < 1e-9);
        assert!((img.size.height.0 - Pt::from_hwpunit(3000).0).abs() < 1e-9);
    }

    /// 앵커 이미지 자산 누락도 폰트/인라인 이미지와 같은 failure_mode 계약.
    #[test]
    fn anchored_image_missing_asset_follows_failure_mode() {
        struct NoImg;
        impl StyleLookup for NoImg {}
        let layout = PageLayout {
            width: 59528,
            height: 84188,
            anchored_images: vec![LaidAnchoredImage {
                location: "s0/p0".into(),
                canonical_key: "gone.png".into(),
                x: 0,
                y: 0,
                width: 4000,
                height: 3000,
            }],
            rects: Vec::new(),
            borders: Vec::new(),
            lines: Vec::new(),
            page_number: None,
        };
        let doc = minimal_doc();
        let input = PdfInput { document: &doc, styles: &NoImg };
        let mut table = FontTable::new(
            FontResolver::with_discovery(&[], crate::font::FontDiscovery::ExplicitOnly)
                .expect("resolver"),
        );
        // Fatal.
        let mut warnings = Vec::new();
        let err = build_paint_pages(
            &input,
            &PdfOptions::default(),
            std::slice::from_ref(&layout),
            &mut table,
            &mut warnings,
        )
        .expect_err("fatal");
        assert!(matches!(err, PdfError::ImageDataMissing { .. }), "{err:?}");
        // Degraded: 경고 후 이미지 생략.
        let options =
            PdfOptions { failure_mode: RenderFailureMode::Degraded, ..PdfOptions::default() };
        let mut warnings = Vec::new();
        let pages = build_paint_pages(&input, &options, &[layout], &mut table, &mut warnings)
            .expect("degraded");
        assert!(pages[0].items.iter().all(|i| !matches!(i, PaintItem::Image(_))));
        assert!(warnings.iter().any(|w| matches!(w, PdfWarning::ImageDataMissing { .. })));
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

    // ── W4 w3: 인라인 글상자 clip 높이·vertAlign 시프트 (폰트 불요 —
    //    내부 줄을 이미지 원자로 세워 clip/배치 산술만 잠근다) ──────────

    use crate::source::{LineTextBox, TextBoxLine};
    use hwpforge_core::layout::LineSeg;
    use hwpforge_foundation::VerticalAlign;

    /// 내부 이미지 원자 한 줄 (vertpos 0 · 지정 vertsize) — content-extent =
    /// vertsize 를 준다. 이미지는 **이미지-지배**(height == vertsize)로 둬서
    /// 세로 배치가 줄 top 에 놓이게 한다 (valign 시프트 검증이 목적이라 세로
    /// 배치는 image-dominated 분기로 고정 — sub-line 공식은 별도 테스트).
    fn inner_image_line(vertsize: i32) -> TextBoxLine {
        TextBoxLine {
            seg: LineSeg {
                textpos: 0,
                vertpos: 0,
                vertsize,
                textheight: vertsize,
                baseline: 0,
                spacing: 0,
                horzpos: 0,
                horzsize: 16440,
                flags: 0,
            },
            atoms: vec![LineAtom::Image(LineImage {
                canonical_key: "img1.png".into(),
                width: vertsize,
                height: vertsize,
            })],
            alignment: Alignment::Left,
            is_last_line: true,
        }
    }

    fn textbox_layout(height: i32, valign: VerticalAlign, inner_extent: i32) -> PageLayout {
        let tb = LineTextBox {
            width: 17008,
            height,
            style: None,
            vert_align: valign,
            inner_lines: vec![inner_image_line(inner_extent)],
        };
        PageLayout {
            width: 59528,
            height: 84188,
            anchored_images: Vec::new(),
            rects: Vec::new(),
            borders: Vec::new(),
            lines: vec![LaidLine {
                location: "s0/p0/l0".into(),
                atoms: vec![LineAtom::TextBox(tb)],
                top_y: 5000,
                baseline_y: 5000,
                vertsize: height, // 글상자 host 줄 높이 (이미지 원자 없음).
                line_box: LineBox { horzpos: 8504, horzsize: 42520 },
                is_last_line: true,
                alignment: Alignment::Left,
            }],
            page_number: None,
        }
    }

    fn paint_textbox(layout: PageLayout) -> crate::paint::Page {
        let doc = minimal_doc();
        let input = PdfInput { document: &doc, styles: &ImgStyles };
        let options = PdfOptions::default();
        let mut table = FontTable::new(
            FontResolver::with_discovery(&[], crate::font::FontDiscovery::ExplicitOnly)
                .expect("resolver"),
        );
        let mut warnings = Vec::new();
        let mut pages = build_paint_pages(&input, &options, &[layout], &mut table, &mut warnings)
            .expect("paint");
        pages.remove(0)
    }

    /// clip 사각형 높이 = **박스 선언 높이**(4252)지 host 줄 vertsize(넘침
    /// 시 더 큼)가 아니다 — 그래야 overflow 가 실제로 잘린다 (§8f ③). 이
    /// 게이트를 놓치면 w3 는 통과하고 w4 시각 대조에서만 터진다.
    #[test]
    fn textbox_clip_height_is_box_height_not_content_extent() {
        // box 4252, 내부 content-extent 12200 (>>box) — overflow 프로파일.
        let page = paint_textbox(textbox_layout(4252, VerticalAlign::Top, 12200));
        let clips: Vec<_> = page
            .items
            .iter()
            .filter_map(|i| match i {
                PaintItem::Clipped(g) => Some(g),
                _ => None,
            })
            .collect();
        assert_eq!(clips.len(), 1, "글상자 = clip 그룹 1개");
        let g = clips[0];
        assert!(
            (g.size.height.0 - Pt::from_hwpunit(4252).0).abs() < 1e-9,
            "clip 높이 = 박스 높이 4252 (content-extent 12200 아님): {:?}",
            g.size.height
        );
        assert!((g.size.width.0 - Pt::from_hwpunit(17008).0).abs() < 1e-9);
        // 박스 원점 = (호스트 줄 좌변, 호스트 줄 top).
        assert!((g.origin.x.0 - Pt::from_hwpunit(8504).0).abs() < 1e-9);
        assert!((g.origin.y.0 - Pt::from_hwpunit(5000).0).abs() < 1e-9);
        // 내부 이미지가 clip 안에 있다 (넘쳐도 방출 — 잘림은 clip 이 강제).
        assert!(g.items.iter().any(|i| matches!(i, PaintItem::Image(_))));
    }

    /// vertAlign TOP/CENTER/BOTTOM 이 내부 콘텐츠를 박스 여백 안에서
    /// 시프트한다 — 내부 이미지 top = box_top + textMargin(283) + shift.
    /// box 7087, interior = 7087 − 566 = 6521, content 1000.
    #[test]
    fn textbox_vertalign_shifts_inner_content() {
        for (valign, shift) in [
            (VerticalAlign::Top, 0),
            (VerticalAlign::Center, (6521 - 1000) / 2), // 2760
            (VerticalAlign::Bottom, 6521 - 1000),       // 5521
        ] {
            let page = paint_textbox(textbox_layout(7087, valign, 1000));
            let g = page
                .items
                .iter()
                .find_map(|i| match i {
                    PaintItem::Clipped(g) => Some(g),
                    _ => None,
                })
                .expect("clip group");
            let img = g
                .items
                .iter()
                .find_map(|i| match i {
                    PaintItem::Image(im) => Some(im),
                    _ => None,
                })
                .expect("inner image");
            let expected_top = 5000 + 283 + shift; // box_top + margin + shift + vertpos(0)
            assert!(
                (img.origin.y.0 - Pt::from_hwpunit(expected_top).0).abs() < 1e-9,
                "{valign:?}: 내부 top {:?} != box_top+283+{shift}",
                img.origin.y
            );
        }
    }
}

/// sub-line 이미지 세로 배치([`inline_image_top`], 상수 k=152/1000) 산술 잠금
/// (W1 §10c-2, r2 상수형).
///
/// 값은 subline_image_v2 fixture 의 실측 lineseg(vertsize/baseline)와 이미지
/// 높이로부터 상수 k 로 유도한 것이다 — 한컴 PDF CTM 대조 잔차 body 4측점
/// ≤0.04pt, 글상자 E 는 border-gap 계통차로 ≤0.08pt (e2e 가 대조).
#[cfg(test)]
mod subline_image_top_tests {
    use super::inline_image_top;

    /// 5측점(A~E) 실측 입력에 대한 Δtop(= image_top − line_top) 하드 잠금.
    /// descent = img_h × 152 / 1000 (절단 나눗셈).
    #[test]
    fn five_measured_points_lock_delta_top() {
        // (baseline, vertsize, img_h, 기대 Δtop) — descent 절단:
        // A 86.184→86 · B 129.2→129 · C 129.2→129 · D 215.384→215
        for (baseline, vertsize, img_h, expected) in [
            (850, 1000, 567, 369),   // A: 10pt 줄 × 2mm  (850−567+86)
            (850, 1000, 850, 129),   // B: 10pt 줄 × 3mm  (850−850+129)
            (1700, 2000, 850, 979),  // C: 20pt 줄 × 3mm  (1700−850+129)
            (1700, 2000, 1417, 498), // D: 20pt 줄 × 5mm  (1700−1417+215)
            (850, 1000, 850, 129),   // E: 글상자 내부 3mm (내부-줄 상대, B 와 동일)
        ] {
            assert_eq!(
                inline_image_top(0, baseline, vertsize, img_h),
                expected,
                "baseline={baseline} vertsize={vertsize} img_h={img_h}"
            );
        }
    }

    /// line_top 오프셋은 선형으로 더해진다 (쪽 절대 좌표).
    #[test]
    fn line_top_offset_is_additive() {
        assert_eq!(inline_image_top(10_104, 850, 1000, 567), 10_104 + 369);
    }

    /// 이미지-지배(img_h ≥ vertsize)는 기존 계약대로 top == line_top (명시
    /// 분기 — 상수 k 로는 일반화 안 됨). img_h > vertsize 방어 케이스 포함.
    #[test]
    fn image_dominated_places_at_line_top() {
        for (line_top, baseline, vertsize, img_h) in [
            (0, 850, 1000, 1000),      // img_h == vertsize
            (5000, 2550, 3000, 3000),  // img_h == vertsize
            (8504, 1700, 2000, 2000),  // img_h == vertsize
            (12345, 6024, 7087, 7087), // img_h == vertsize
            (500, 850, 1000, 1200),    // 방어: img_h > vertsize (admission 밖)
        ] {
            assert_eq!(
                inline_image_top(line_top, baseline, vertsize, img_h),
                line_top,
                "img_h={img_h} >= vertsize={vertsize} 는 top==line_top 이어야 함"
            );
        }
    }

    /// 퇴화 줄(vertsize ≤ 0)은 세로 보정 불가 → line_top 폴백.
    #[test]
    fn nonpositive_vertsize_falls_back_to_line_top() {
        assert_eq!(inline_image_top(1234, 850, 0, 500), 1234);
        assert_eq!(inline_image_top(1234, 0, -5, 500), 1234);
    }

    /// 극단 크기에서도 오버플로 없이 계산된다 (i64 중간·clamp).
    #[test]
    fn large_inputs_do_not_overflow() {
        // sub-line (img_h < vertsize): baseline − img_h + img_h×152/1000
        // = 1_000_000 − 1_500_000 + 1_500_000×152/1000 = −272_000
        let top = inline_image_top(100_000_000, 1_000_000, 2_000_000, 1_500_000);
        assert_eq!(top, 100_000_000 - 272_000);
    }
}
