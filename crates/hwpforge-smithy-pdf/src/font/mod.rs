//! 폰트 해석 — face 축 분류 (family × style) + 정확 full-name 매칭.
//!
//! `header.xml` 의 fontface 이름(예: "한컴바탕")은 파일명이 아니다 —
//! name table 을 읽어 실물 파일에 매핑해야 한다 (W0 실측: 한컴바탕 =
//! `HBatang.TTF`). W4a 분류 계약 (Codex 적대 리뷰 H2 재설계):
//!
//! - **family** = nameID 16(typographic) 우선, 없으면 1 폴백. **subfamily**
//!   = 17 우선, 없으면 2. 로캘별 레코드 전부 등록한다 (한/영 병기 이름).
//! - **style 축** = face 플래그 (OS/2 bold·italic). subfamily 문자열이
//!   보수 집합(regular/보통/bold/italic/bold italic)의 명시 스타일인데
//!   플래그와 **모순**되면 그 face 는 후보에서 제외하고 문자열 쪽
//!   (family, style) 키를 ambiguous 로 등록한다 — 조용한 선택 금지.
//! - **vendor 접미사(B/M/L)는 style 신호로 쓰지 않는다** — 한컴 실측
//!   (HANBaek B/M/L 등)은 Bold 축이 아니라 별개 nominal family 다.
//! - 같은 (family, style) 후보 다수 = weight ranking (Bold 축 = 700,
//!   나머지 = 400 최근접), 동률 = ambiguous.
//!
//! W2 부터의 불변 계약:
//! - **명시 디렉터리 우선** — 자동 발견([`FontDiscovery`])은 낮은 우선순위
//!   tier 로만 참여하고, 명시 dirs 의 기존 해석을 바꾸지 못한다.
//! - 미해결 = [`PdfError::FontUnresolved`] — **fallback 금지** (다른 폰트로
//!   그리면 위치가 틀린 출력 — no-fake-support).
//! - 라이선스(fsType)/서브셋 게이트는 W4d (임베드 시점).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::{PdfError, PdfResult};

/// 폰트 face 의 스타일 축 (RIBBI 4축 — variable font 중간축은 비지원).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FaceStyle {
    /// 보통 (bold/italic 플래그 없음).
    Regular,
    /// 굵게.
    Bold,
    /// 기울임.
    Italic,
    /// 굵은 기울임.
    BoldItalic,
}

impl FaceStyle {
    /// face 플래그 쌍 → 스타일 축.
    pub fn from_flags(bold: bool, italic: bool) -> Self {
        match (bold, italic) {
            (false, false) => Self::Regular,
            (true, false) => Self::Bold,
            (false, true) => Self::Italic,
            (true, true) => Self::BoldItalic,
        }
    }

    /// weight ranking 목표값 (Bold 축 = 700, 나머지 = 400).
    fn target_weight(self) -> i32 {
        match self {
            Self::Bold | Self::BoldItalic => 700,
            Self::Regular | Self::Italic => 400,
        }
    }
}

/// subfamily 문자열이 명시하는 스타일 (보수 집합 — 미지 토큰 = `None`).
///
/// "SemiBold"·"Light"·vendor 접미사 등은 의도적으로 해석하지 않는다 —
/// 여기서 매칭되지 않은 face 는 플래그만으로 분류된다.
fn explicit_style_token(subfamily: &str) -> Option<FaceStyle> {
    let s = subfamily.trim();
    if s == "보통" {
        return Some(FaceStyle::Regular);
    }
    match s.to_ascii_lowercase().as_str() {
        "regular" => Some(FaceStyle::Regular),
        "bold" => Some(FaceStyle::Bold),
        "italic" => Some(FaceStyle::Italic),
        "bold italic" | "bolditalic" => Some(FaceStyle::BoldItalic),
        _ => None,
    }
}

/// 폰트 자동 발견 정책 — 명시 디렉터리 외에 어디를 더 탐색할지.
///
/// 발견은 "이름이 실제로 일치하는 face 를 더 찾는 것"이다 — 미해결 이름을
/// 다른 폰트로 대체하는 fallback 이 아니다. 우선순위는 항상 명시
/// `font_dirs` 가 위다 (tier 순서 — [`FontResolver::with_discovery`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum FontDiscovery {
    /// 명시 `font_dirs` 만 (라이브러리 기본 — 머신 무관 결정적).
    #[default]
    ExplicitOnly,
    /// 명시 dirs + 한컴오피스 번들 폰트.
    ///
    /// macOS: `/Applications/Hancom Office HWP.app/Contents/Resources/Hnc/Shared/TTF`
    /// (W0 실측). 타 OS 설치 경로는 미실측 — 실측 후 추가한다 (no-fake-support).
    HancomBundle,
    /// 명시 dirs + 한컴 번들 + 플랫폼 시스템 폰트 디렉터리.
    ///
    /// 우선순위 순서 (사용자 > 로컬 > 시스템 — OS 폰트 캐스케이드 관례):
    /// - macOS: `~/Library/Fonts` → `/Library/Fonts` → `/System/Library/Fonts`
    ///   → `/System/Library/Fonts/Supplemental`
    /// - Linux: `~/.local/share/fonts` → `~/.fonts` → `/usr/local/share/fonts`
    ///   → `/usr/share/fonts`
    /// - Windows: `%LOCALAPPDATA%\Microsoft\Windows\Fonts` → `C:\Windows\Fonts`
    Platform,
}

/// 한컴오피스 번들 폰트 디렉터리 (알려진 설치 경로 — 미존재는 스캔에서 건너뜀).
fn hancom_bundle_dirs() -> Vec<PathBuf> {
    if cfg!(target_os = "macos") {
        vec![PathBuf::from("/Applications/Hancom Office HWP.app/Contents/Resources/Hnc/Shared/TTF")]
    } else {
        Vec::new() // 타 OS 설치 경로 미실측 — 실측 후 추가 (no-fake-support)
    }
}

/// 플랫폼 시스템 폰트 디렉터리 (우선순위 순 — [`FontDiscovery::Platform`] 문서).
fn platform_font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if cfg!(target_os = "macos") {
        if let Some(home) = std::env::var_os("HOME") {
            dirs.push(PathBuf::from(home).join("Library/Fonts"));
        }
        dirs.push(PathBuf::from("/Library/Fonts"));
        dirs.push(PathBuf::from("/System/Library/Fonts"));
        dirs.push(PathBuf::from("/System/Library/Fonts/Supplemental"));
    } else if cfg!(target_os = "linux") {
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            dirs.push(home.join(".local/share/fonts"));
            dirs.push(home.join(".fonts"));
        }
        dirs.push(PathBuf::from("/usr/local/share/fonts"));
        dirs.push(PathBuf::from("/usr/share/fonts"));
    } else if cfg!(target_os = "windows") {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join("Microsoft/Windows/Fonts"));
        }
        dirs.push(PathBuf::from("C:/Windows/Fonts"));
    }
    dirs
}

/// 파일 바이트 fingerprint — (길이, 64-bit 해시). 물리 동일성 판정용:
/// 동일 fingerprint + face 인덱스 = 같은 실물의 중복 배치 (충돌 아님).
fn fingerprint(data: &[u8]) -> (u64, u64) {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut h);
    (data.len() as u64, h.finish())
}

/// 해석된 폰트 실물.
#[derive(Debug, Clone)]
pub struct ResolvedFont {
    /// 요청했던 face 이름.
    pub face_name: String,
    /// 폰트 파일 경로.
    pub path: PathBuf,
    /// 파일 바이트 (컬렉션이면 전체 파일 — `face_index` 로 선택).
    pub data: Vec<u8>,
    /// 컬렉션(.ttc) 내 face 인덱스 (단일 폰트는 0).
    pub face_index: u32,
}

/// 정확 full-name 항목 — face 의 플래그 유래 스타일을 함께 기억한다.
///
/// 모순 face(`style = None`)는 [`FontResolver::resolve_styled`] 의 스타일
/// 매칭에서 제외된다 (이름 그대로의 [`FontResolver::resolve`] 는 허용).
#[derive(Debug)]
struct ExactFace {
    path: PathBuf,
    face_index: u32,
    style: Option<FaceStyle>,
}

/// face 이름 → 폰트 파일 resolver.
///
/// 생성 시 디렉터리를 1회 스캔해 name table 인덱스를 구축한다.
/// full name(nameID 4)은 먼저 발견된 항목이 이기고(경로 정렬 = 결정적),
/// (family, style) 키는 weight ranking 으로 승자를 정한다 — 모순/동률은
/// ambiguous 로 남겨 resolve 시 에러로 표면화한다.
#[derive(Debug)]
pub struct FontResolver {
    /// nameID 4 full name → face (정확 일치 — W2 계약 유지).
    exact: HashMap<String, ExactFace>,
    /// (family, style) → ranking 승자.
    styled: HashMap<(String, FaceStyle), (PathBuf, u32)>,
    /// 충돌/동률 키 → 진단 상세.
    ambiguous: HashMap<(String, FaceStyle), String>,
}

impl FontResolver {
    /// 주어진 디렉터리들만 스캔해 resolver 를 만든다
    /// ([`FontDiscovery::ExplicitOnly`] 과 동일).
    ///
    /// # Errors
    ///
    /// 디렉터리가 존재하지 않거나 읽을 수 없으면 [`PdfError::FontIo`].
    /// (개별 파일의 폰트 파싱 실패는 조용히 건너뛴다 — 폰트가 아닌 파일.)
    pub fn new(dirs: &[PathBuf]) -> PdfResult<Self> {
        Self::from_tiers(&[(dirs.to_vec(), true)])
    }

    /// 명시 디렉터리 + 자동 발견 경로를 스캔해 resolver 를 만든다.
    ///
    /// 우선순위 tier: 명시 `dirs`(최상위) → 한컴 번들 → (Platform) 사용자
    /// → 로컬 → 시스템 폰트 디렉터리. 어떤 (family, style) 키든 **가장
    /// 낮은 tier 에 등장한 face 들만** 해석에 참여한다 — 발견이 명시 dirs
    /// 의 기존 해석을 바꾸지 못한다. 발견 tier 의 미존재/읽기 불가 경로는
    /// 조용히 건너뛴다.
    ///
    /// # Errors
    ///
    /// 명시 `dirs` 가 존재하지 않거나 읽을 수 없으면 [`PdfError::FontIo`].
    pub fn with_discovery(dirs: &[PathBuf], discovery: FontDiscovery) -> PdfResult<Self> {
        let mut tiers: Vec<(Vec<PathBuf>, bool)> = vec![(dirs.to_vec(), true)];
        match discovery {
            FontDiscovery::ExplicitOnly => {}
            FontDiscovery::HancomBundle => tiers.push((hancom_bundle_dirs(), false)),
            FontDiscovery::Platform => {
                tiers.push((hancom_bundle_dirs(), false));
                for dir in platform_font_dirs() {
                    tiers.push((vec![dir], false));
                }
            }
        }
        Self::from_tiers(&tiers)
    }

    /// tier 목록으로 resolver 를 만든다 (앞 tier = 높은 우선순위).
    ///
    /// `required = false` tier 는 미존재/읽기 실패 디렉터리를 건너뛴다
    /// (자동 발견 경로 — 머신마다 설치 여부가 다르다).
    fn from_tiers(tiers: &[(Vec<PathBuf>, bool)]) -> PdfResult<Self> {
        struct Candidate {
            path: PathBuf,
            face_index: u32,
            weight: i32,
            tier: usize,
            fingerprint: (u64, u64),
        }
        let mut exact: HashMap<String, ExactFace> = HashMap::new();
        let mut candidates: HashMap<(String, FaceStyle), Vec<Candidate>> = HashMap::new();
        let mut contradictions: HashMap<(String, FaceStyle), Vec<(usize, String)>> = HashMap::new();
        for (tier, (dirs, required)) in tiers.iter().enumerate() {
            for dir in dirs {
                // 재귀 수집 후 전체 경로 정렬 — 순회 순서를 결정적으로 고정.
                let mut files = Vec::new();
                if *required {
                    collect_font_files(dir, 0, &mut files)?;
                } else if collect_font_files(dir, 0, &mut files).is_err() {
                    continue;
                }
                files.sort();
                for path in files {
                    let Ok(data) = std::fs::read(&path) else {
                        continue;
                    };
                    let fp = fingerprint(&data);
                    for face in classify_faces(&data) {
                        let exact_style =
                            if face.contradiction.is_some() { None } else { Some(face.style) };
                        for full in &face.full_names {
                            exact.entry(full.clone()).or_insert_with(|| ExactFace {
                                path: path.clone(),
                                face_index: face.face_index,
                                style: exact_style,
                            });
                        }
                        if let Some((token_style, detail)) = &face.contradiction {
                            // 모순 face: 후보 등록 대신 문자열 쪽 키를 오염 표시.
                            for family in &face.families {
                                contradictions
                                    .entry((family.clone(), *token_style))
                                    .or_default()
                                    .push((
                                        tier,
                                        format!(
                                            "{detail} at {}#{}",
                                            path.display(),
                                            face.face_index
                                        ),
                                    ));
                            }
                        } else {
                            for family in &face.families {
                                candidates.entry((family.clone(), face.style)).or_default().push(
                                    Candidate {
                                        path: path.clone(),
                                        face_index: face.face_index,
                                        weight: face.weight,
                                        tier,
                                        fingerprint: fp,
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }
        // 키별 해석: 최저 tier 만 참여 → 모순 우선 → weight ranking →
        // 동일 실물(fingerprint) 중복 dedupe → 잔여 동률 = ambiguous.
        let mut styled = HashMap::new();
        let mut ambiguous: HashMap<(String, FaceStyle), String> = HashMap::new();
        let keys: HashSet<(String, FaceStyle)> =
            candidates.keys().chain(contradictions.keys()).cloned().collect();
        for key in keys {
            let cands = candidates.get(&key).map_or(&[][..], Vec::as_slice);
            let contras = contradictions.get(&key).map_or(&[][..], Vec::as_slice);
            let min_tier = cands
                .iter()
                .map(|c| c.tier)
                .chain(contras.iter().map(|(t, _)| *t))
                .min()
                .expect("key exists in at least one map");
            if let Some((_, detail)) = contras.iter().find(|(t, _)| *t == min_tier) {
                ambiguous.insert(key, detail.clone());
                continue;
            }
            let target = key.1.target_weight();
            let tier_cands: Vec<&Candidate> = cands.iter().filter(|c| c.tier == min_tier).collect();
            let best = tier_cands
                .iter()
                .map(|c| (c.weight - target).abs())
                .min()
                .expect("non-empty bucket");
            // 동일 실물(같은 fingerprint + face 인덱스)의 중복 배치는 충돌이
            // 아니다 — 먼저 스캔된 경로가 canonical 로 남는다.
            let mut distinct: Vec<&Candidate> = Vec::new();
            for c in tier_cands.iter().filter(|c| (c.weight - target).abs() == best) {
                if !distinct
                    .iter()
                    .any(|d| d.fingerprint == c.fingerprint && d.face_index == c.face_index)
                {
                    distinct.push(c);
                }
            }
            if let [winner] = distinct.as_slice() {
                styled.insert(key, (winner.path.clone(), winner.face_index));
            } else {
                let list = distinct
                    .iter()
                    .map(|c| {
                        format!(
                            "{}#{} (weight {}, fingerprint {:016x})",
                            c.path.display(),
                            c.face_index,
                            c.weight,
                            c.fingerprint.1
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                ambiguous.insert(
                    key,
                    format!("weight tie at distance {best} from target {target}: {list}"),
                );
            }
        }
        Ok(Self { exact, styled, ambiguous })
    }

    /// 등록된 이름 수 (진단용 — 정확 full name + (family, style) 키 합).
    pub fn face_count(&self) -> usize {
        self.exact.len() + self.styled.len()
    }

    /// face 이름을 그대로 해석한다 (full name 정확 일치 → (이름, Regular)).
    ///
    /// W2 계약 유지 — `header.xml` 의 fontface 이름 경로. 실패 시 fallback
    /// 없이 에러.
    ///
    /// # Errors
    ///
    /// 미등록 = [`PdfError::FontUnresolved`], (이름, Regular) 충돌 =
    /// [`PdfError::FontFaceAmbiguous`], 파일 재독 실패 = [`PdfError::FontIo`].
    pub fn resolve(&self, face_name: &str) -> PdfResult<ResolvedFont> {
        let name = face_name.trim();
        if let Some(hit) = self.exact.get(name) {
            // 이름 그대로 요청 — face 플래그와 무관하게 그 실물을 반환한다.
            return load(name, &hit.path, hit.face_index);
        }
        self.resolve_family(name, FaceStyle::Regular)
    }

    /// (face 이름, 스타일 축) 으로 해석한다.
    ///
    /// full name 정확 일치는 face 플래그가 요청 스타일과 **일치할 때만**
    /// 매칭된다 (모순 face 의 조용한 강등 방지 — 예: full name 이 family
    /// 이름과 같은 regular face 에 Bold 를 요청하면 미해결로 남겨 W4c 의
    /// fail-closed 처리에 넘긴다).
    ///
    /// # Errors
    ///
    /// 미등록 = [`PdfError::FontUnresolved`], 신호 충돌/동률 =
    /// [`PdfError::FontFaceAmbiguous`], 파일 재독 실패 = [`PdfError::FontIo`].
    pub fn resolve_styled(&self, face_name: &str, style: FaceStyle) -> PdfResult<ResolvedFont> {
        let name = face_name.trim();
        if let Some(hit) = self.exact.get(name) {
            if hit.style == Some(style) {
                return load(name, &hit.path, hit.face_index);
            }
            // 스타일 불일치/모순 face — family 경로로 폴스루.
        }
        self.resolve_family(name, style)
    }

    fn resolve_family(&self, name: &str, style: FaceStyle) -> PdfResult<ResolvedFont> {
        let key = (name.to_string(), style);
        if let Some((path, face_index)) = self.styled.get(&key) {
            return load(name, path, *face_index);
        }
        if let Some(detail) = self.ambiguous.get(&key) {
            return Err(PdfError::FontFaceAmbiguous {
                face: name.to_string(),
                style,
                detail: detail.clone(),
            });
        }
        Err(PdfError::FontUnresolved { face: name.to_string() })
    }
}

fn load(name: &str, path: &Path, face_index: u32) -> PdfResult<ResolvedFont> {
    let data = std::fs::read(path)?;
    Ok(ResolvedFont { face_name: name.to_string(), path: path.to_path_buf(), data, face_index })
}

fn is_font_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc"))
}

/// 폰트 스캔 가드 — 재귀 깊이 상한 (병적 트리/시스템 디렉터리 방어).
const MAX_SCAN_DEPTH: usize = 8;
/// 폰트 스캔 가드 — 단일 파일 크기 상한 (실물 폰트 최대 ~60MB 의 여유 상한).
const MAX_FONT_FILE_BYTES: u64 = 256 * 1024 * 1024;

fn collect_font_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) -> PdfResult<()> {
    if depth > MAX_SCAN_DEPTH {
        return Ok(()); // 깊이 상한 초과분은 조용히 무시 (가드 — 에러 아님).
    }
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            // 심볼릭 링크 디렉터리는 따라가지 않는다 (사이클 가드). 링크
            // 파일은 대상이 정상 크기의 파일일 때만 수집한다 (Linux 시스템
            // 폰트 디렉터리의 관례적 링크 지원).
            if is_font_file(&path) {
                if let Ok(target) = std::fs::metadata(&path) {
                    if target.is_file() && target.len() <= MAX_FONT_FILE_BYTES {
                        out.push(path);
                    }
                }
            }
            continue;
        }
        if meta.is_dir() {
            collect_font_files(&path, depth + 1, out)?;
        } else if is_font_file(&path) && meta.len() <= MAX_FONT_FILE_BYTES {
            out.push(path);
        }
    }
    Ok(())
}

/// 파일 안 face 하나의 분류 결과.
struct ClassifiedFace {
    face_index: u32,
    /// nameID 4 full name (로캘별 전부, 중복 제거).
    full_names: Vec<String>,
    /// family 이름 (nameID 16 우선 → 1 폴백, 중복 제거).
    families: Vec<String>,
    /// face 플래그 유래 스타일.
    style: FaceStyle,
    /// OS/2 usWeightClass (결측 = 400 취급).
    weight: i32,
    /// subfamily 명시 토큰이 플래그와 모순 → (토큰 스타일, 사유).
    contradiction: Option<(FaceStyle, String)>,
}

/// 파일 안 모든 face 를 name table + 플래그로 분류한다.
fn classify_faces(data: &[u8]) -> Vec<ClassifiedFace> {
    use rustybuzz::ttf_parser;

    let face_count = ttf_parser::fonts_in_collection(data).unwrap_or(1);
    let mut out = Vec::new();
    for face_index in 0..face_count {
        let Ok(face) = ttf_parser::Face::parse(data, face_index) else {
            continue;
        };
        let mut id1 = Vec::new();
        let mut id2 = Vec::new();
        let mut id16 = Vec::new();
        let mut id17 = Vec::new();
        let mut full_names = Vec::new();
        for name in face.names() {
            let Some(value) = name.to_string() else { continue };
            let value = value.trim().to_string();
            if value.is_empty() {
                continue;
            }
            match name.name_id {
                ttf_parser::name_id::FAMILY => id1.push(value),
                ttf_parser::name_id::SUBFAMILY => id2.push(value),
                ttf_parser::name_id::FULL_NAME => full_names.push(value),
                ttf_parser::name_id::TYPOGRAPHIC_FAMILY => id16.push(value),
                ttf_parser::name_id::TYPOGRAPHIC_SUBFAMILY => id17.push(value),
                _ => {}
            }
        }
        let mut families = if id16.is_empty() { id1 } else { id16 };
        dedupe_preserving_order(&mut families);
        dedupe_preserving_order(&mut full_names);
        let subfamilies = if id17.is_empty() { id2 } else { id17 };
        let (bold, italic) = (face.is_bold(), face.is_italic());
        let style = FaceStyle::from_flags(bold, italic);
        let weight = i32::from(face.weight().to_number());
        let mut contradiction = None;
        for sf in &subfamilies {
            if let Some(token) = explicit_style_token(sf) {
                if token != style {
                    contradiction = Some((
                        token,
                        format!(
                            "subfamily {sf:?} contradicts face flags (bold={bold}, \
                             italic={italic})"
                        ),
                    ));
                    break;
                }
            }
        }
        out.push(ClassifiedFace { face_index, full_names, families, style, weight, contradiction });
    }
    out
}

fn dedupe_preserving_order(values: &mut Vec<String>) {
    let mut seen = HashSet::new();
    values.retain(|v| seen.insert(v.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 한컴 번들 경로 (fixture-optional — 설치 머신에서만 실행).
    const HANCOM_TTF_DIR: &str =
        "/Applications/Hancom Office HWP.app/Contents/Resources/Hnc/Shared/TTF";

    #[test]
    fn empty_dirs_resolve_nothing() {
        let resolver = FontResolver::new(&[]).expect("empty resolver");
        assert_eq!(resolver.face_count(), 0);
        let err = resolver.resolve("한컴바탕").unwrap_err();
        assert!(matches!(err, PdfError::FontUnresolved { .. }));
    }

    #[test]
    fn missing_dir_is_io_error() {
        let err =
            FontResolver::new(&[PathBuf::from("/nonexistent-hwpforge-font-dir")]).unwrap_err();
        assert!(matches!(err, PdfError::FontIo(_)));
    }

    #[test]
    fn committed_test_fonts_resolve_family_to_regular_only() {
        // 커밋된 자체 제작 폰트 (tests/fonts/generate_test_fonts.py) — CI 포함 전 환경 실행.
        let dir = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fonts"));
        let resolver = FontResolver::new(&[dir]).expect("scan committed fonts");
        // Bold 파일("...-Bold.ttf")이 사전순으로 먼저 스캔되지만 subfamily 필터가
        // family 이름 선점을 막는다 (함초롬돋움 잉크 오프셋 2.3pt 오염의 회귀 잠금).
        let family = resolver.resolve("HwpForge Test").expect("family name");
        assert!(
            family.path.file_name().unwrap().to_string_lossy().contains("Regular"),
            "family 는 Regular 파일이어야 한다: {:?}",
            family.path
        );
        // full name(nameID 4)은 face 고유값 — Bold 도 자기 full name 으로는 해석된다.
        let bold = resolver.resolve("HwpForge Test Bold").expect("bold full name");
        assert!(
            bold.path.file_name().unwrap().to_string_lossy().contains("Bold"),
            "unexpected file: {:?}",
            bold.path
        );
        // fallback 금지 — 미등록 이름은 에러.
        assert!(matches!(
            resolver.resolve("HwpForge Test Black"),
            Err(PdfError::FontUnresolved { .. })
        ));
    }

    /// 커밋 폰트 디렉터리 (tests/fonts — 생성기 산출물).
    fn committed_fonts_dir() -> PathBuf {
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fonts"))
    }

    #[test]
    fn w4_pair_resolves_by_typographic_family_and_style() {
        let resolver = FontResolver::new(&[committed_fonts_dir()]).expect("scan");
        let regular =
            resolver.resolve_styled("HwpForge W4", FaceStyle::Regular).expect("regular face");
        let bold = resolver.resolve_styled("HwpForge W4", FaceStyle::Bold).expect("bold face");
        assert!(
            regular.path.file_name().unwrap().to_string_lossy().contains("Regular"),
            "unexpected regular file: {:?}",
            regular.path
        );
        assert!(
            bold.path.file_name().unwrap().to_string_lossy().contains("Bold"),
            "unexpected bold file: {:?}",
            bold.path
        );
        // nameID 16 이 있으면 nameID 1 은 family 로 등록하지 않는다 (16 우선 — 병기 아님).
        assert!(matches!(
            resolver.resolve("HwpForgeW4 Legacy"),
            Err(PdfError::FontUnresolved { .. })
        ));
        // Bold 는 실제 폭이 다르다 (Latin 0.7em vs 0.6em) — bbox 게이트 유효성의 근거.
        let shaped_r =
            crate::text::shape::shape_text(&regular.data, regular.face_index, "AB", 1000)
                .expect("shape regular");
        let shaped_b = crate::text::shape::shape_text(&bold.data, bold.face_index, "AB", 1000)
            .expect("shape bold");
        assert_eq!(shaped_r.natural_width().round() as i64, 1200);
        assert_eq!(shaped_b.natural_width().round() as i64, 1400);
    }

    #[test]
    fn contradictory_subfamily_vs_flags_is_ambiguous_not_silent() {
        let resolver = FontResolver::new(&[committed_fonts_dir()]).expect("scan");
        // 충돌 전용 폰트: subfamily "Bold" + regular 플래그 → (family, Bold) = ambiguous.
        assert!(matches!(
            resolver.resolve_styled("HwpForge Conflict", FaceStyle::Bold),
            Err(PdfError::FontFaceAmbiguous { .. })
        ));
        // 모순 face 는 후보로도 등록하지 않는다 — Regular face 가 없으니 미해결.
        assert!(matches!(
            resolver.resolve("HwpForge Conflict"),
            Err(PdfError::FontUnresolved { .. })
        ));
        // 레거시 W2 Bold(fsSelection 미설정)도 같은 모순 케이스로 표면화된다.
        assert!(matches!(
            resolver.resolve_styled("HwpForge Test", FaceStyle::Bold),
            Err(PdfError::FontFaceAmbiguous { .. })
        ));
    }

    #[test]
    fn weight_ranking_nearest_wins_and_tie_is_ambiguous() {
        let resolver = FontResolver::new(&[committed_fonts_dir()]).expect("scan");
        // Regular 목표 400: weight 400 이 500 을 이긴다.
        let rank = resolver.resolve("HwpForge Rank").expect("rank family");
        assert!(
            rank.path.file_name().unwrap().to_string_lossy().contains("R400"),
            "unexpected winner: {:?}",
            rank.path
        );
        // 350 vs 450 = 목표 400 에서 동거리 → 조용한 선택 금지.
        assert!(matches!(
            resolver.resolve("HwpForge RankTie"),
            Err(PdfError::FontFaceAmbiguous { .. })
        ));
    }

    #[test]
    fn exact_full_name_carries_its_own_style() {
        let resolver = FontResolver::new(&[committed_fonts_dir()]).expect("scan");
        // full name 은 스타일 내재 — 요청 스타일과 face 플래그가 일치할 때만 styled 매칭.
        let bold =
            resolver.resolve_styled("HwpForge W4 Bold", FaceStyle::Bold).expect("bold full name");
        assert!(bold.path.file_name().unwrap().to_string_lossy().contains("Bold"));
        // 모순 face (레거시 lying Bold): full name 이라도 styled 요청은 신뢰하지 않는다.
        assert!(matches!(
            resolver.resolve_styled("HwpForge Test Bold", FaceStyle::Bold),
            Err(PdfError::FontUnresolved { .. })
        ));
        // resolve() (이름 그대로 — W2 계약) 는 여전히 성공.
        resolver.resolve("HwpForge Test Bold").expect("legacy exact name");
    }

    #[test]
    fn fs_type_fixtures_classify_normally_in_w4a() {
        // 라이선스 게이트는 W4d (임베드 시점) — 분류기는 fsType 과 무관하게 해석한다.
        let resolver = FontResolver::new(&[committed_fonts_dir()]).expect("scan");
        resolver.resolve("HwpForge FsV0Restricted").expect("v0 restricted");
        // OS/2 결측 = 플래그 없음 → Regular/weight 400 취급 (분류 실패 아님).
        resolver.resolve("HwpForge FsNoOs2").expect("no os2");
    }

    /// 테스트 전용 임시 디렉터리 (재실행 멱등 — 기존 잔재 제거 후 생성).
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hwpforge-w4b-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn copy_font(src_name: &str, dst_dir: &Path, dst_name: &str) {
        std::fs::copy(committed_fonts_dir().join(src_name), dst_dir.join(dst_name))
            .expect("copy fixture font");
    }

    #[test]
    fn discovery_default_is_explicit_only() {
        assert_eq!(FontDiscovery::default(), FontDiscovery::ExplicitOnly);
        assert_eq!(crate::PdfOptions::default().discovery, FontDiscovery::ExplicitOnly);
    }

    #[test]
    fn explicit_tier_shadows_discovered_tier() {
        let explicit = temp_dir("shadow-explicit");
        let discovered = temp_dir("shadow-discovered");
        // 명시 tier 에 weight 500, 발견 tier 에 weight 400 — ranking 만이라면
        // 400 이 이기지만 tier 우선이 먼저다 (명시 dirs 가 발견 경로를 가린다).
        copy_font("HwpForgeRank-R500.ttf", &explicit, "HwpForgeRank-R500.ttf");
        copy_font("HwpForgeRank-R400.ttf", &discovered, "HwpForgeRank-R400.ttf");
        let resolver =
            FontResolver::from_tiers(&[(vec![explicit.clone()], true), (vec![discovered], false)])
                .expect("tiered scan");
        let hit = resolver.resolve("HwpForge Rank").expect("family");
        assert!(hit.path.starts_with(&explicit), "explicit tier must win: {:?}", hit.path);
    }

    #[test]
    fn identical_duplicate_across_dirs_is_not_ambiguous() {
        let a = temp_dir("dup-a");
        let b = temp_dir("dup-b");
        copy_font("HwpForgeTest-Regular.ttf", &a, "Copy-A.ttf");
        copy_font("HwpForgeTest-Regular.ttf", &b, "Copy-B.ttf");
        // 같은 실물(동일 fingerprint)의 중복 배치는 충돌이 아니다 — 첫 경로 canonical.
        let resolver = FontResolver::new(&[a.clone(), b]).expect("scan");
        let hit = resolver.resolve("HwpForge Test").expect("deduped family");
        assert!(hit.path.starts_with(&a), "first path must be canonical: {:?}", hit.path);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_directory_cycles_are_skipped() {
        let dir = temp_dir("symlink-cycle");
        copy_font("HwpForgeTest-Regular.ttf", &dir, "Font.ttf");
        std::os::unix::fs::symlink(&dir, dir.join("loop")).expect("create symlink cycle");
        // 디렉터리 심볼릭 링크는 따라가지 않는다 — 사이클에서도 스캔이 끝난다.
        let resolver = FontResolver::new(&[dir]).expect("scan terminates");
        resolver.resolve("HwpForge Test").expect("font still found");
    }

    #[test]
    fn optional_discovery_tier_missing_is_silently_skipped() {
        // 발견 tier 의 미존재 경로는 조용히 건너뛴다 (명시 tier 는 FontIo 계약 유지).
        let resolver = FontResolver::from_tiers(&[
            (Vec::new(), true),
            (vec![PathBuf::from("/nonexistent-hwpforge-discovery-dir")], false),
        ])
        .expect("optional tier missing is not an error");
        assert_eq!(resolver.face_count(), 0);
    }

    #[test]
    fn hancom_bundle_discovery_resolves_without_explicit_dirs() {
        if !PathBuf::from(HANCOM_TTF_DIR).exists() {
            return; // fixture-optional (한컴 미설치 머신)
        }
        let resolver =
            FontResolver::with_discovery(&[], FontDiscovery::HancomBundle).expect("scan bundle");
        resolver.resolve("한컴바탕").expect("한컴바탕 via discovery");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn platform_discovery_finds_system_font() {
        if !Path::new("/System/Library/Fonts").exists() {
            return;
        }
        let resolver =
            FontResolver::with_discovery(&[], FontDiscovery::Platform).expect("scan platform");
        // Apple SD Gothic Neo = macOS 기본 한글 시스템 폰트 (weight ranking 이
        // 다face .ttc 에서 Regular 를 골라내는 실물 검증).
        resolver.resolve("Apple SD Gothic Neo").expect("system font");
    }

    #[test]
    fn hancom_bundle_resolves_korean_face_names() {
        let dir = PathBuf::from(HANCOM_TTF_DIR);
        if !dir.exists() {
            return; // fixture-optional (CI 에는 한컴 미설치)
        }
        let resolver = FontResolver::new(&[dir]).expect("scan bundle");
        // W0 실측: 한컴바탕 = HBatang.TTF (Haansoft Batang)
        let batang = resolver.resolve("한컴바탕").expect("한컴바탕");
        assert!(
            batang.path.file_name().unwrap().to_string_lossy().eq_ignore_ascii_case("HBatang.TTF"),
            "unexpected file: {:?}",
            batang.path
        );
        // 함초롬바탕도 이름으로 해석돼야 한다 (HANBatang)
        let hcr = resolver.resolve("함초롬바탕").expect("함초롬바탕");
        assert!(
            hcr.path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_ascii_lowercase()
                .contains("hanbatang"),
            "unexpected file: {:?}",
            hcr.path
        );
        // fallback 금지: 없는 이름은 에러
        assert!(matches!(
            resolver.resolve("존재하지않는서체"),
            Err(PdfError::FontUnresolved { .. })
        ));
    }
}
