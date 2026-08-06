//! 폰트 해석 — W2 스코프: **regular exact-face** 매칭만.
//!
//! `header.xml` 의 fontface 이름(예: "한컴바탕")은 파일명이 아니다 —
//! name table (nameID 1 family / 4 full name, 한국어 로캘 포함) 을 읽어
//! 실물 파일에 매핑해야 한다 (W0 실측: 한컴바탕 = `HBatang.TTF`).
//!
//! W2 경계 (Codex 리뷰 H2·논점 5):
//! - **명시 디렉터리만** 탐색한다 — 한컴 번들 자동 발견·시스템 폴백은 W4.
//! - 정확한 이름 일치 실패 = [`PdfError::FontUnresolved`] — **fallback 금지**
//!   (다른 폰트로 그리면 위치가 틀린 출력 — no-fake-support).
//! - bold/italic face 선택·synthetic style·라이선스/서브셋 게이트는 W4.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::{PdfError, PdfResult};

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

/// face 이름 → 폰트 파일 resolver.
///
/// 생성 시 디렉터리를 1회 스캔해 name table 인덱스를 구축한다.
/// 같은 이름이 여러 파일에서 나오면 먼저 발견된 항목이 이긴다
/// (디렉터리 순서 = 우선순위 — 호출자가 순서로 제어).
#[derive(Debug)]
pub struct FontResolver {
    index: HashMap<String, (PathBuf, u32)>,
}

impl FontResolver {
    /// 주어진 디렉터리들을 스캔해 resolver 를 만든다.
    ///
    /// # Errors
    ///
    /// 디렉터리가 존재하지 않거나 읽을 수 없으면 [`PdfError::FontIo`].
    /// (개별 파일의 폰트 파싱 실패는 조용히 건너뛴다 — 폰트가 아닌 파일.)
    pub fn new(dirs: &[PathBuf]) -> PdfResult<Self> {
        let mut index = HashMap::new();
        for dir in dirs {
            // 재귀 수집 후 전체 경로 정렬 — 순회 순서를 결정적으로 고정.
            let mut files = Vec::new();
            collect_font_files(dir, &mut files)?;
            files.sort();
            for path in files {
                let Ok(data) = std::fs::read(&path) else {
                    continue;
                };
                for (name, face_index) in regular_face_names(&data) {
                    index.entry(name).or_insert_with(|| (path.clone(), face_index));
                }
            }
        }
        Ok(Self { index })
    }

    /// 인덱스에 등록된 face 이름 수 (진단용).
    pub fn face_count(&self) -> usize {
        self.index.len()
    }

    /// face 이름을 정확 일치로 해석한다. 실패 시 fallback 없이 에러.
    ///
    /// # Errors
    ///
    /// 이름 미등록 = [`PdfError::FontUnresolved`], 파일 재독 실패 = [`PdfError::FontIo`].
    pub fn resolve(&self, face_name: &str) -> PdfResult<ResolvedFont> {
        let (path, face_index) = self
            .index
            .get(face_name.trim())
            .ok_or_else(|| PdfError::FontUnresolved { face: face_name.to_string() })?;
        let data = std::fs::read(path)?;
        Ok(ResolvedFont {
            face_name: face_name.trim().to_string(),
            path: path.clone(),
            data,
            face_index: *face_index,
        })
    }
}

fn is_font_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc"))
}

fn collect_font_files(dir: &Path, out: &mut Vec<PathBuf>) -> PdfResult<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_font_files(&path, out)?;
        } else if is_font_file(&path) {
            out.push(path);
        }
    }
    Ok(())
}

/// 파일 안 **regular face** 의 (family/full 이름, face 인덱스) 를 수집한다.
///
/// family 이름(nameID 1)은 bold/변형 face 도 공유한다 — subfamily(nameID 2)가
/// Regular 계열일 때만 등록해 비-regular 파일이 family 이름을 선점하지
/// 못하게 한다 (실측: 함초롬돋움 family 충돌로 잉크 오프셋 2.3pt 오염).
/// full name(nameID 4)은 face 고유값이라 그대로 등록한다.
fn regular_face_names(data: &[u8]) -> Vec<(String, u32)> {
    use rustybuzz::ttf_parser;

    let face_count = ttf_parser::fonts_in_collection(data).unwrap_or(1);
    let mut out = Vec::new();
    for face_index in 0..face_count {
        let Ok(face) = ttf_parser::Face::parse(data, face_index) else {
            continue;
        };
        let mut families = Vec::new();
        let mut subfamilies = Vec::new();
        for name in face.names() {
            let Some(value) = name.to_string() else { continue };
            let value = value.trim().to_string();
            if value.is_empty() {
                continue;
            }
            match name.name_id {
                ttf_parser::name_id::FAMILY => families.push(value),
                ttf_parser::name_id::SUBFAMILY => subfamilies.push(value),
                ttf_parser::name_id::FULL_NAME => out.push((value, face_index)),
                _ => {}
            }
        }
        // subfamily 미기재 = 단일 face 파일 → regular 취급.
        let is_regular = subfamilies.is_empty()
            || subfamilies.iter().any(|s| s.eq_ignore_ascii_case("regular") || s == "보통");
        if is_regular {
            for family in families {
                out.push((family, face_index));
            }
        }
    }
    out
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
