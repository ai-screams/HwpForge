//! 2단계 — 자산 파이프라인에서 **파일 I/O 가 일어나는 유일한 지점**.
//!
//! 디스크 조회는 신뢰 불가 입력이다: `canonicalize` + base_dir 포함 검사로
//! 경로 탈출(`../..`·절대 경로)을 차단하고, 정규 파일만, 상한 안에서만
//! 읽는다. 바이트가 실제 이미지인지(스니핑)는 3단계가 판정한다 — 이
//! 단계는 "무엇을 읽어도 되는가" 만 책임진다.
//!
//! [`AssetSource::DataUri`]·[`AssetSource::Remote`] 항목은 여기서 아예
//! 건드리지 않는다 (네트워크 접근 금지, 인라인 디코드는 3단계 소관).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::{
    AssetIdentity, AssetPlanEntry, AssetReject, AssetSource, Prepared, ProvidedAsset,
    MAX_IMAGE_BYTES,
};

/// 계획의 [`AssetSource::File`] 항목을 `base_dir` 기준으로 읽는다.
///
/// 돌려주는 목록은 계획의 File 항목마다 **정확히 하나**, 계획 순서다 —
/// [`super::finish_assets`] 의 입력 계약과 일치한다. `data:`·원격 항목은
/// 포함되지 않는다.
///
/// `base_dir` 이 `None` 이면(인라인 텍스트·stdin 입력) 모든 File 항목이
/// [`AssetReject::NoBaseDir`] 로 거부된다 — 단, 절대 경로는 존재 여부를
/// 먼저 확인하므로 없는 파일은 [`AssetReject::Missing`] 으로 나온다
/// (오늘의 판정 순서를 그대로 유지한다).
///
/// 같은 정규 경로는 **한 번만 읽는다** — 같은 파일을 가리키는 철자가
/// 여러 개여도 바이트가 갈라지지 않는다(정체 충돌 불가).
#[must_use]
pub fn resolve_files_from_dir(
    plan: &[AssetPlanEntry],
    base_dir: Option<&Path>,
) -> Vec<ProvidedAsset> {
    plan.iter()
        .zip(resolve_aligned(plan, base_dir))
        .filter_map(|(entry, ready)| match ready {
            Prepared::Provided { identity, bytes } => {
                Some(ProvidedAsset::Resolved { occurrence: entry.occurrence, identity, bytes })
            }
            Prepared::Rejected(reason) => {
                Some(ProvidedAsset::Rejected { occurrence: entry.occurrence, reason })
            }
            Prepared::DataUri(_) | Prepared::Remote => None,
        })
        .collect()
}

/// [`resolve_files_from_dir`] 의 계획 정렬판 — 호환 경로가 검증 단계를
/// 건너뛸 수 있게 하는 내부 형태다.
///
/// 계획의 모든 항목에 대해 하나씩, 계획 순서로 돌려준다.
pub(crate) fn resolve_aligned(plan: &[AssetPlanEntry], base_dir: Option<&Path>) -> Vec<Prepared> {
    // base_dir 은 한 번만 정규화한다 (부재·실패 = None → NoBaseDir 계열).
    let canonical_base = base_dir.and_then(|dir| dir.canonicalize().ok());
    let mut cache: HashMap<PathBuf, Vec<u8>> = HashMap::new();

    plan.iter()
        .map(|entry| match &entry.source {
            AssetSource::DataUri(src) => Prepared::DataUri(src.clone()),
            AssetSource::Remote(_) => Prepared::Remote,
            AssetSource::File(path) => {
                match read_contained(path, canonical_base.as_deref(), &mut cache) {
                    Ok((canonical, bytes)) => Prepared::Provided {
                        identity: AssetIdentity::CanonicalFile(canonical),
                        bytes,
                    },
                    Err(reason) => Prepared::Rejected(reason),
                }
            }
        })
        .collect()
}

/// 경로 하나를 포함 검사 후 읽는다.
///
/// 검사 순서가 계약이다 — 상대 경로는 base 가 없으면 **즉시**
/// [`AssetReject::NoBaseDir`], 절대 경로는 **정규화를 먼저** 하므로 없는
/// 파일이 [`AssetReject::Missing`] 으로 나온 뒤에야 base 부재를 본다.
fn read_contained(
    path: &Path,
    canonical_base: Option<&Path>,
    cache: &mut HashMap<PathBuf, Vec<u8>>,
) -> Result<(PathBuf, Vec<u8>), AssetReject> {
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        let Some(base) = canonical_base else { return Err(AssetReject::NoBaseDir) };
        base.join(path)
    };
    let canonical = candidate.canonicalize().map_err(|_| AssetReject::Missing)?;
    // 포함 검사: 절대 경로 저작 포함 — 정규화 결과가 base 밖이면 차단한다.
    // base 자체가 없으면(절대 src + 인라인 입력) 포함을 증명할 수 없으므로
    // 동일하게 차단한다.
    let Some(base) = canonical_base else { return Err(AssetReject::NoBaseDir) };
    if !canonical.starts_with(base) {
        return Err(AssetReject::Escapes);
    }
    if let Some(bytes) = cache.get(&canonical) {
        return Ok((canonical, bytes.clone()));
    }
    let meta = std::fs::metadata(&canonical).map_err(|_| AssetReject::Unreadable)?;
    // 정규 파일만 — FIFO 는 read 가 무한 블록되고 len=0 이라 상한도 통과한다.
    if !meta.is_file() {
        return Err(AssetReject::Unreadable);
    }
    if meta.len() > MAX_IMAGE_BYTES {
        return Err(AssetReject::TooLarge);
    }
    let bytes = std::fs::read(&canonical).map_err(|_| AssetReject::Unreadable)?;
    cache.insert(canonical.clone(), bytes.clone());
    Ok((canonical, bytes))
}

#[cfg(test)]
mod tests {
    use hwpforge_core::document::Document;
    use hwpforge_core::image::{Image, ImageFormat};
    use hwpforge_core::paragraph::Paragraph;
    use hwpforge_core::run::{Run, RunContent};
    use hwpforge_core::section::Section;
    use hwpforge_core::PageSettings;
    use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};

    use super::super::{collect_asset_plan, finish_assets, warnings_from, AssetOutcome};
    use super::*;

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3];

    /// 테스트 전용 임시 디렉터리 (신규 dev-dep 없이 — 유일명 + 자동 삭제).
    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let p = std::env::temp_dir().join(format!(
                "hwpforge-assets-{tag}-{}-{}",
                std::process::id(),
                std::thread::current().name().unwrap_or("t").replace("::", "-"),
            ));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).expect("mkdir");
            Self(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
        fn write(&self, rel: &str, bytes: &[u8]) {
            let p = self.0.join(rel);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).expect("mkdir parents");
            }
            std::fs::write(p, bytes).expect("write");
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn doc_with_srcs(srcs: &[&str]) -> Document {
        let runs = srcs
            .iter()
            .map(|src| {
                let img = Image::new(
                    *src,
                    HwpUnit::from_mm(10.0).expect("w"),
                    HwpUnit::from_mm(10.0).expect("h"),
                    ImageFormat::from_extension(src),
                );
                Run::image(img, CharShapeIndex::new(0))
            })
            .collect();
        let para = Paragraph::with_runs(runs, ParaShapeIndex::new(0));
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));
        doc
    }

    #[test]
    fn relative_file_entry_without_base_dir_is_no_base_dir() {
        // 상대 경로는 정규화를 시도하지 않고 즉시 NoBaseDir 로 거부된다.
        let doc = doc_with_srcs(&["rel.png"]);
        let plan = collect_asset_plan(&doc);
        let provided = resolve_files_from_dir(&plan, None);
        assert_eq!(
            provided,
            vec![ProvidedAsset::Rejected {
                occurrence: plan[0].occurrence,
                reason: AssetReject::NoBaseDir
            }]
        );
    }

    #[test]
    fn only_file_entries_are_resolved_here() {
        let dir = TempDir::new("only-files");
        dir.write("a.png", PNG);
        let doc = doc_with_srcs(&["a.png", "data:image/png;base64,AA==", "https://e.test/x.png"]);
        let plan = collect_asset_plan(&doc);
        let provided = resolve_files_from_dir(&plan, Some(dir.path()));

        assert_eq!(provided.len(), 1, "data:·원격은 이 단계가 만지지 않는다: {provided:?}");
        assert_eq!(provided[0].occurrence(), plan[0].occurrence);
        assert!(matches!(
            &provided[0],
            ProvidedAsset::Resolved { identity: AssetIdentity::CanonicalFile(_), bytes, .. }
                if bytes == PNG
        ));
    }

    #[test]
    fn duplicate_spellings_share_one_canonical_identity() {
        let dir = TempDir::new("one-read");
        dir.write("x.png", PNG);
        let doc = doc_with_srcs(&["x.png", "./x.png"]);
        let plan = collect_asset_plan(&doc);
        let provided = resolve_files_from_dir(&plan, Some(dir.path()));

        assert_eq!(provided.len(), 2, "발생 건마다 하나씩 나온다");
        let identities: Vec<_> = provided
            .iter()
            .map(|p| match p {
                ProvidedAsset::Resolved { identity, .. } => identity.clone(),
                ProvidedAsset::Rejected { reason, .. } => panic!("unexpected reject: {reason:?}"),
            })
            .collect();
        assert_eq!(identities[0], identities[1], "같은 파일 = 같은 정체");
    }

    #[test]
    fn staged_route_preserves_an_image_only_paragraph() {
        // B1 회귀: `![..](url)` 단독 문단이 드롭으로 비면 validate 가 문서
        // 전체를 거부한다. embed 쪽 테스트는 호환 진입점만 덮으므로 3단계
        // 조립 경로에서도 직접 잠근다.
        let doc = doc_with_srcs(&["https://e.test/logo.png"]);
        let plan = collect_asset_plan(&doc);
        let provided = resolve_files_from_dir(&plan, None);
        assert!(provided.is_empty(), "원격은 2단계가 만지지 않는다");
        let finished = finish_assets(doc, provided).expect("remote-only plan needs no provision");

        assert!(matches!(finished.outcomes[0], AssetOutcome::Remote { .. }));
        let runs = &finished.document.sections()[0].paragraphs[0].runs;
        assert_eq!(runs.len(), 1, "빈 텍스트 run 으로 문단 보존");
        assert_eq!(runs[0].content.as_text(), Some(""));
        assert!(finished.document.validate().is_ok(), "드롭 후에도 문서는 유효");
    }

    #[test]
    fn staged_route_dedups_two_spellings_of_one_file() {
        // 파일 정체 dedup 이 finish_assets 를 통과하는 경로 — 발생 건마다
        // Embedded 가 하나씩 나오되 키는 하나다.
        let dir = TempDir::new("staged-dedup");
        dir.write("x.png", PNG);
        let doc = doc_with_srcs(&["x.png", "./x.png"]);
        let plan = collect_asset_plan(&doc);
        let provided = resolve_files_from_dir(&plan, Some(dir.path()));
        let finished = finish_assets(doc, provided).expect("both spellings resolve");

        assert_eq!(finished.outcomes.len(), plan.len(), "계획 1건당 결과 1건");
        let keys: Vec<_> = finished
            .outcomes
            .iter()
            .map(|o| match o {
                AssetOutcome::Embedded { key, .. } => key.clone(),
                other => panic!("unexpected outcome: {other:?}"),
            })
            .collect();
        assert_eq!(keys, vec!["image1.png".to_string(), "image1.png".to_string()]);
        assert_eq!(finished.image_store.len(), 1);
        assert!(warnings_from(&plan, &finished.outcomes).is_empty());
    }

    #[test]
    fn three_stage_pipeline_matches_the_compatibility_entry_point() {
        let dir = TempDir::new("pipeline");
        dir.write("a.png", PNG);
        let staged = doc_with_srcs(&["a.png", "missing.png", "https://e.test/x.png"]);
        let plan = collect_asset_plan(&staged);
        let provided = resolve_files_from_dir(&plan, Some(dir.path()));
        let finished = finish_assets(staged, provided).expect("plan is satisfied");
        let staged_warnings = warnings_from(&plan, &finished.outcomes);

        let mut direct = doc_with_srcs(&["a.png", "missing.png", "https://e.test/x.png"]);
        let embedded = crate::embed::load_referenced_images(&mut direct, Some(dir.path()));

        assert_eq!(staged_warnings, embedded.warnings);
        assert_eq!(finished.image_store, embedded.store);
        let staged_paths: Vec<_> = finished
            .document
            .sections()
            .iter()
            .flat_map(|s| &s.paragraphs)
            .flat_map(|p| &p.runs)
            .filter_map(|r| match &r.content {
                RunContent::Image(img) => Some(img.path.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(staged_paths, vec!["image1.png".to_string()]);
        assert!(matches!(finished.outcomes[1], AssetOutcome::Dropped { .. }));
        assert!(matches!(finished.outcomes[2], AssetOutcome::Remote { .. }));
    }
}
