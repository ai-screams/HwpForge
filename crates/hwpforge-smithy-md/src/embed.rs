//! Markdown → HWPX 이미지 임베드 로더 (W6 §12b v2).
//!
//! md 디코더는 `![..](src)` 를 `src` 경로 참조만 담긴 [`hwpforge_core::image::Image`]
//! run 으로 만든다 — 참조된 바이트를 [`ImageStore`] 로 적재해야 인코더가
//! `BinData/` 로 포장한다 (미적재 = dangling `binaryItemIDRef`).
//!
//! **핵심 원칙 — 디스크 조회 경로와 패키지 키의 분리** (적대 리뷰 C1·H2·H3):
//!
//! - 디스크 조회는 신뢰 불가 입력이다: `canonicalize` + base_dir 포함
//!   검사로 경로 탈출(`../..`·절대경로)을 차단하고, **스니핑이 알려진
//!   이미지 포맷일 때만** 적재한다 (임의 파일 반입 차단).
//! - 문서에 박히는 키는 항상 **합성 정규명** `imageN.<ext>` 다 — 외부
//!   통제 문자열이 XML id/manifest 채널(`binaryItemIDRef`·OPF `id`)에
//!   들어가지 않는다. 동일 실파일 중복 참조는 동일 키로 dedup 된다.
//! - `data:` URI 는 네트워크 없이 로컬 base64 디코드로 지원한다 —
//!   base_dir 이 없는 인라인 입력(MCP 텍스트·CLI stdin)에서도 임베드가
//!   가능한 유일한 경로다. `http(s)` 원격은 경고 후 제외한다 (네트워크
//!   금지).
//! - 실패(부재·탈출·원격·미지 바이트 등)는 **이미지 run 을 드롭 + typed
//!   경고** — dangling 참조를 남기지 않는다 (no-fake-support).

use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use base64::Engine as _;
use hwpforge_core::document::Document;
use hwpforge_core::image::{ImageFormat, ImageStore};
use hwpforge_core::run::RunContent;

use crate::encoder::MdWarning;

/// 이미지 파일 1개의 적재 상한 (md 입력 자체의 50 MB 상한과 동일 계열).
const MAX_IMAGE_BYTES: u64 = 50 * 1024 * 1024;

/// [`load_referenced_images`] 가 이미지 참조를 제외한 사유 (typed —
/// warning-first, 무음 드롭 금지).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImageEmbedSkipReason {
    /// 파일이 존재하지 않거나 경로를 정규화할 수 없음.
    MissingFile,
    /// 파일 읽기 실패 (권한 등 I/O 오류).
    Unreadable,
    /// 정규화 결과가 base_dir 밖 — 경로 탈출 차단 (C1).
    PathEscapes,
    /// `http(s)`/프로토콜-상대 원격 URL — 네트워크 접근 금지.
    RemoteUrl,
    /// 상대 경로인데 base_dir 이 없음 (인라인 텍스트·stdin 입력).
    NoBaseDir,
    /// 읽은 바이트의 magic 이 알려진 이미지 포맷이 아님 — 임의 바이트
    /// 반입 차단 (C1).
    UnsupportedBytes,
    /// `data:` URI 파싱/base64 디코드 실패 (base64 아닌 payload 포함).
    InvalidDataUri,
    /// 파일이 적재 상한(50 MB)을 초과.
    TooLarge,
}

impl fmt::Display for ImageEmbedSkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::MissingFile => "file not found",
            Self::Unreadable => "file unreadable",
            Self::PathEscapes => "path escapes the document directory",
            Self::RemoteUrl => "remote URL (network access is not allowed)",
            Self::NoBaseDir => "relative path with no base directory (inline input)",
            Self::UnsupportedBytes => "bytes are not a recognized image format",
            Self::InvalidDataUri => "data: URI is not valid base64 image data",
            Self::TooLarge => "file exceeds the 50 MB embed ceiling",
        };
        f.write_str(s)
    }
}

/// [`load_referenced_images`] 결과 — 적재된 스토어 + typed 경고 목록.
#[derive(Debug, Default)]
pub struct EmbeddedImages {
    /// 인코더에 넘길 이미지 스토어 (키 = 합성 정규명 = `Image.path`).
    pub store: ImageStore,
    /// 제외된 참조들의 typed 경고 (원본 src 절단 포함).
    pub warnings: Vec<MdWarning>,
}

/// 문서의 이미지 참조를 해석해 [`ImageStore`] 로 적재하고, 성공한 run 의
/// `Image.path`/`format` 을 합성 키·스니핑 실포맷으로 재작성한다.
///
/// `base_dir` = md 파일의 부모 디렉터리 (파일 입력일 때). 인라인 텍스트/
/// stdin 입력은 `None` — 이 경우 상대 경로 참조는
/// [`ImageEmbedSkipReason::NoBaseDir`] 로 제외되고 `data:` URI 만
/// 임베드된다.
///
/// 실패한 참조는 run 이 **드롭**되고 경고로 선언된다 — 결과 문서에는
/// dangling `binaryItemIDRef` 가 남지 않는다.
pub fn load_referenced_images(document: &mut Document, base_dir: Option<&Path>) -> EmbeddedImages {
    let mut out = EmbeddedImages::default();
    // dedup: 해석된 정체(canonical 경로 문자열 또는 data URI 전문) → 합성 키.
    let mut seen: HashMap<String, String> = HashMap::new();
    let mut counter: usize = 0;
    // base_dir 은 한 번만 정규화 (부재/실패 = None → NoBaseDir 계열).
    let canonical_base = base_dir.and_then(|d| d.canonicalize().ok());

    document.for_each_paragraph_mut(|para| {
        para.runs.retain_mut(|run| {
            let RunContent::Image(img) = &mut run.content else { return true };
            let src = img.path.clone();
            match resolve_source(&src, canonical_base.as_deref()) {
                Ok((identity, bytes)) => {
                    if let Some(key) = seen.get(&identity) {
                        img.path.clone_from(key);
                        return true;
                    }
                    let Some(format) = ImageFormat::sniff(&bytes) else {
                        out.warnings
                            .push(skip_warning(&src, ImageEmbedSkipReason::UnsupportedBytes));
                        return false;
                    };
                    let ext = format.canonical_extension().expect("sniff never returns Unknown");
                    counter += 1;
                    let key = format!("image{counter}.{ext}");
                    out.store.insert(key.clone(), bytes);
                    seen.insert(identity, key.clone());
                    img.path = key;
                    img.format = format;
                    true
                }
                Err(reason) => {
                    out.warnings.push(skip_warning(&src, reason));
                    false
                }
            }
        });
    });
    out
}

/// src 를 분류·해석해 (dedup 정체, 바이트) 를 돌려준다.
fn resolve_source(
    src: &str,
    canonical_base: Option<&Path>,
) -> Result<(String, Vec<u8>), ImageEmbedSkipReason> {
    if let Some(rest) = src.strip_prefix("data:") {
        let bytes = decode_data_uri(rest).ok_or(ImageEmbedSkipReason::InvalidDataUri)?;
        if bytes.len() as u64 > MAX_IMAGE_BYTES {
            return Err(ImageEmbedSkipReason::TooLarge);
        }
        return Ok((src.to_string(), bytes));
    }
    if src.contains("://") || src.starts_with("//") {
        return Err(ImageEmbedSkipReason::RemoteUrl);
    }
    let path = Path::new(src);
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        let Some(base) = canonical_base else {
            return Err(ImageEmbedSkipReason::NoBaseDir);
        };
        base.join(path)
    };
    let canonical = candidate.canonicalize().map_err(|_| ImageEmbedSkipReason::MissingFile)?;
    // 포함 검사 (C1): 절대경로 저작 포함 — 정규화 결과가 base 밖이면 차단.
    // base 자체가 없으면(절대 src + 인라인 입력) 포함을 증명할 수 없으므로
    // 동일하게 차단한다.
    let Some(base) = canonical_base else {
        return Err(ImageEmbedSkipReason::NoBaseDir);
    };
    if !canonical.starts_with(base) {
        return Err(ImageEmbedSkipReason::PathEscapes);
    }
    let meta = std::fs::metadata(&canonical).map_err(|_| ImageEmbedSkipReason::Unreadable)?;
    if meta.len() > MAX_IMAGE_BYTES {
        return Err(ImageEmbedSkipReason::TooLarge);
    }
    let bytes = std::fs::read(&canonical).map_err(|_| ImageEmbedSkipReason::Unreadable)?;
    Ok((canonical.to_string_lossy().into_owned(), bytes))
}

/// `data:` 접두 이후(`<mime>[;base64],<payload>`)를 디코드한다 —
/// base64 payload 만 지원 (이미지의 비-base64 data URI 는 비현실적).
fn decode_data_uri(rest: &str) -> Option<Vec<u8>> {
    let comma = rest.find(',')?;
    let (meta, payload) = rest.split_at(comma);
    let payload = &payload[1..];
    if !meta.ends_with(";base64") {
        return None;
    }
    base64::engine::general_purpose::STANDARD.decode(payload.trim()).ok()
}

/// src 를 표시용으로 절단해 경고를 만든다 (data URI 전문 방지).
fn skip_warning(src: &str, reason: ImageEmbedSkipReason) -> MdWarning {
    const MAX_SRC: usize = 64;
    let shown = if src.chars().count() > MAX_SRC {
        let head: String = src.chars().take(MAX_SRC).collect();
        format!("{head}…")
    } else {
        src.to_string()
    };
    MdWarning::ImageEmbedSkipped { src: shown, reason }
}

#[cfg(test)]
mod tests {
    use hwpforge_core::image::Image;
    use hwpforge_core::paragraph::Paragraph;
    use hwpforge_core::run::Run;
    use hwpforge_core::section::Section;
    use hwpforge_core::PageSettings;
    use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};

    use super::*;

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3];

    /// 테스트 전용 임시 디렉터리 (신규 dev-dep 없이 — 유일명 + 자동 삭제).
    struct TempDir(std::path::PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let p = std::env::temp_dir().join(format!(
                "hwpforge-embed-{tag}-{}-{}",
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

    fn image_run(src: &str) -> Run {
        let img = Image::new(
            src,
            HwpUnit::from_mm(10.0).expect("w"),
            HwpUnit::from_mm(10.0).expect("h"),
            ImageFormat::from_extension(src),
        );
        Run::image(img, CharShapeIndex::new(0))
    }

    fn doc_with_srcs(srcs: &[&str]) -> Document {
        let runs = srcs.iter().map(|s| image_run(s)).collect();
        let para = Paragraph::with_runs(runs, ParaShapeIndex::new(0));
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));
        doc
    }

    fn image_paths(doc: &Document) -> Vec<String> {
        let mut out = Vec::new();
        let sections = doc.sections();
        for s in sections {
            for p in &s.paragraphs {
                for r in &p.runs {
                    if let RunContent::Image(img) = &r.content {
                        out.push(img.path.clone());
                    }
                }
            }
        }
        out
    }

    #[test]
    fn relative_path_embeds_with_synthetic_key() {
        let dir = TempDir::new("rel");
        dir.write("photo-원본.png", PNG);
        let mut doc = doc_with_srcs(&["photo-원본.png"]);
        let out = load_referenced_images(&mut doc, Some(dir.path()));
        assert!(out.warnings.is_empty(), "{:?}", out.warnings);
        assert_eq!(image_paths(&doc), vec!["image1.png"]);
        assert_eq!(out.store.get("image1.png"), Some(PNG));
    }

    #[test]
    fn subdirectory_src_never_leaks_into_key() {
        // H2: 서브디렉터리 src 가 그대로 키가 되면 binaryItemIDRef 가 깨짐.
        let dir = TempDir::new("subdir");
        dir.write("images/a.png", PNG);
        let mut doc = doc_with_srcs(&["images/a.png"]);
        let out = load_referenced_images(&mut doc, Some(dir.path()));
        assert_eq!(image_paths(&doc), vec!["image1.png"]);
        assert!(out.store.get("image1.png").is_some());
    }

    #[test]
    fn duplicate_spellings_dedup_to_one_entry() {
        let dir = TempDir::new("dedup");
        dir.write("x.png", PNG);
        let mut doc = doc_with_srcs(&["x.png", "./x.png"]);
        let out = load_referenced_images(&mut doc, Some(dir.path()));
        assert_eq!(image_paths(&doc), vec!["image1.png", "image1.png"]);
        assert_eq!(out.store.iter().count(), 1);
    }

    #[test]
    fn path_escape_is_dropped_with_typed_warning() {
        // C1: `..` 탈출 — 실제 존재하는 밖 파일이어도 차단.
        let outer = TempDir::new("outer");
        outer.write("secret.png", PNG);
        let inner = outer.path().join("inner");
        std::fs::create_dir_all(&inner).expect("mkdir");
        let mut doc = doc_with_srcs(&["../secret.png"]);
        let out = load_referenced_images(&mut doc, Some(&inner));
        assert!(image_paths(&doc).is_empty(), "run 드롭");
        assert!(matches!(
            &out.warnings[..],
            [MdWarning::ImageEmbedSkipped { reason: ImageEmbedSkipReason::PathEscapes, .. }]
        ));
        assert_eq!(out.store.iter().count(), 0);
    }

    #[test]
    fn absolute_path_outside_base_is_blocked() {
        let outside = TempDir::new("abs-out");
        outside.write("f.png", PNG);
        let base = TempDir::new("abs-base");
        let abs = outside.path().join("f.png");
        let mut doc = doc_with_srcs(&[abs.to_str().expect("utf8")]);
        let out = load_referenced_images(&mut doc, Some(base.path()));
        assert!(image_paths(&doc).is_empty());
        assert!(matches!(
            &out.warnings[..],
            [MdWarning::ImageEmbedSkipped { reason: ImageEmbedSkipReason::PathEscapes, .. }]
        ));
    }

    #[test]
    fn non_image_bytes_are_rejected() {
        // C1: 임의 파일 반입 차단 — magic 미달 바이트는 적재 금지.
        let dir = TempDir::new("bytes");
        dir.write("fake.png", b"ssh-rsa AAAA fake key material");
        let mut doc = doc_with_srcs(&["fake.png"]);
        let out = load_referenced_images(&mut doc, Some(dir.path()));
        assert!(image_paths(&doc).is_empty());
        assert!(matches!(
            &out.warnings[..],
            [MdWarning::ImageEmbedSkipped { reason: ImageEmbedSkipReason::UnsupportedBytes, .. }]
        ));
        assert_eq!(out.store.iter().count(), 0);
    }

    #[test]
    fn missing_file_and_remote_and_nobase() {
        let dir = TempDir::new("misc");
        let mut doc =
            doc_with_srcs(&["nope.png", "https://example.com/a.png", "//cdn.example.com/b.png"]);
        let out = load_referenced_images(&mut doc, Some(dir.path()));
        assert!(image_paths(&doc).is_empty());
        let reasons: Vec<_> = out
            .warnings
            .iter()
            .map(|w| match w {
                MdWarning::ImageEmbedSkipped { reason, .. } => reason.clone(),
                other => panic!("unexpected: {other:?}"),
            })
            .collect();
        assert_eq!(
            reasons,
            vec![
                ImageEmbedSkipReason::MissingFile,
                ImageEmbedSkipReason::RemoteUrl,
                ImageEmbedSkipReason::RemoteUrl,
            ]
        );

        // 인라인 입력 (base_dir 없음): 상대 경로 = NoBaseDir.
        let mut doc2 = doc_with_srcs(&["rel.png"]);
        let out2 = load_referenced_images(&mut doc2, None);
        assert!(matches!(
            &out2.warnings[..],
            [MdWarning::ImageEmbedSkipped { reason: ImageEmbedSkipReason::NoBaseDir, .. }]
        ));
    }

    #[test]
    fn data_uri_embeds_without_base_dir() {
        // H1: 인라인 입력의 유일한 임베드 경로 — 네트워크 없이 로컬 디코드.
        let b64 = base64::engine::general_purpose::STANDARD.encode(PNG);
        let src = format!("data:image/png;base64,{b64}");
        let mut doc = doc_with_srcs(&[&src]);
        let out = load_referenced_images(&mut doc, None);
        assert!(out.warnings.is_empty(), "{:?}", out.warnings);
        assert_eq!(image_paths(&doc), vec!["image1.png"]);
        assert_eq!(out.store.get("image1.png"), Some(PNG));
    }

    #[test]
    fn invalid_data_uri_variants_are_dropped() {
        for src in
            ["data:image/png;base64,%%%not-base64%%%", "data:image/png,plainpayload", "data:,x"]
        {
            let mut doc = doc_with_srcs(&[src]);
            let out = load_referenced_images(&mut doc, None);
            assert!(
                matches!(
                    &out.warnings[..],
                    [MdWarning::ImageEmbedSkipped {
                        reason: ImageEmbedSkipReason::InvalidDataUri,
                        ..
                    }]
                ),
                "src={src} → {:?}",
                out.warnings
            );
        }
    }

    #[test]
    fn long_data_uri_src_is_truncated_in_warning() {
        let src = format!("data:image/png;base64,{}", "A".repeat(500));
        let mut doc = doc_with_srcs(&[&src]);
        let out = load_referenced_images(&mut doc, None);
        let MdWarning::ImageEmbedSkipped { src: shown, .. } = &out.warnings[0] else {
            panic!("expected skip warning");
        };
        assert!(shown.chars().count() <= 65, "절단됨: {}", shown.len());
    }

    #[test]
    fn text_runs_and_format_hint_untouched() {
        let dir = TempDir::new("mixed");
        dir.write("a.jpg", &[0xFF, 0xD8, 0xFF, 0xE0, 1]);
        let para = Paragraph::with_runs(
            vec![
                Run::text("앞 ", CharShapeIndex::new(0)),
                image_run("a.jpg"),
                Run::text(" 뒤", CharShapeIndex::new(0)),
            ],
            ParaShapeIndex::new(0),
        );
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));
        let out = load_referenced_images(&mut doc, Some(dir.path()));
        assert!(out.warnings.is_empty());
        let sections = doc.sections();
        let runs = &sections[0].paragraphs[0].runs;
        assert_eq!(runs.len(), 3, "텍스트 run 보존");
        let RunContent::Image(img) = &runs[1].content else { panic!("image run") };
        assert_eq!(img.path, "image1.jpg");
        assert_eq!(img.format, ImageFormat::Jpeg, "스니핑 실포맷으로 갱신");
    }
}
