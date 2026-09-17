//! 자산(이미지) 임베드의 3단계 계약 — 계획 / 파일 읽기 / 완성.
//!
//! [`crate::embed::load_referenced_images`] 는 "문서를 걸으면서 디스크를
//! 읽고 그 자리에서 임베드한다" 를 한 함수에 묶고 있었다. Python 바인딩
//! 처럼 파일시스템이 없는(혹은 다른 곳에 있는) 호출자를 받으려면 **I/O 가
//! 일어나는 지점이 하나여야** 하므로, 같은 동작을 세 단계로 쪼갠다:
//!
//! 1. [`collect_asset_plan`] — 순수. 문서의 이미지 run 을 문서 순서로 훑어
//!    "무엇이 필요한지"([`AssetPlanEntry`])만 돌려준다. I/O 없음.
//! 2. [`fs::resolve_files_from_dir`] — **유일한 파일 I/O**. 계획의
//!    [`AssetSource::File`] 항목만 base_dir 포함 검사 후 읽어
//!    [`ProvidedAsset`] 로 돌려준다. `data:`·원격은 건드리지 않는다.
//! 3. [`finish_assets`] — 순수. 제공된 바이트로 문서를 재작성하고
//!    [`hwpforge_core::image::ImageStore`] 와 [`AssetOutcome`] 목록을 만든다.
//!
//! 파일시스템이 아닌 provider(메모리·오브젝트 스토어·FFI 건너편)는 2단계를
//! 자기 구현으로 갈아끼우고 [`AssetIdentity::Opaque`]·
//! [`AssetIdentity::ContentHash`] 로 정체를 선언하면 된다 — 3단계는
//! provider 를 모른다.
//!
//! # 주소 지정 계약 (중요)
//!
//! [`RunLocator`] 는 **문서 순서 방문 번호**다:
//! `paragraph` = [`hwpforge_core::document::Document::for_each_paragraph_mut`]
//! 가 방문하는 순서의 0-기반 일련번호(본문·표 셀·글상자·각주/미주·메모·
//! 머리말/꼬리말·바탕쪽 등 중첩 문단 전부 포함), `run` = 그 문단의
//! **변형 전** run 벡터 안 인덱스.
//!
//! 따라서 [`collect_asset_plan`] 과 [`finish_assets`] 사이에서 이미지 run
//! 구성을 **바꾸면 안 된다**. 주소만으로는 이를 알 수 없다 — 두 이미지의
//! `src` 가 서로 맞바뀌면 주소는 그대로인 채 내용만 뒤바뀌므로, 대조 없이는
//! A 의 바이트가 B 의 run 에 박힌다. 그래서 [`finish_assets`] 는 호출자가
//! 건네준 `plan` 을 믿지 않고 문서에서 계획을 **다시 수집해 전건 대조**한다
//! (길이·주소·출처·순서). 어긋나면 [`crate::MdError::AssetPlanMismatch`] 다.
//! 이미지 run 과 무관한 변경(텍스트 수정 등)은 계획에 나타나지 않으므로
//! 통과한다.
//!
//! 이미지 run 을 드롭해도 이 번호가 밀리지 않는다:
//! `Run::walk_paragraphs_mut` 는 `RunContent::Image` 안으로 재귀하지 않으므로
//! (캡션 문단은 방문 대상이 아니다) 이미지 run 제거가 이후 문단의 방문
//! 번호를 바꾸지 않는다.
//!
//! # 불변식
//!
//! `outcomes.len() == plan.len()` 이고 **i 번째 outcome 은 i 번째 계획
//! 항목의 결과**다 (dedup 으로 키를 재사용해도 [`AssetOutcome::Embedded`]
//! 가 하나 나온다). [`warnings_from`] 이 위치 짝짓기로 성립하는 근거이며,
//! 길이가 어긋나면 경고가 무음 드롭되므로 그쪽은 오류로 거부한다.

pub mod fs;

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::PathBuf;

use base64::Engine as _;
use hwpforge_core::document::Document;
use hwpforge_core::image::{ImageFormat, ImageStore};
use hwpforge_core::run::{Run, RunContent};
use serde::{Deserialize, Serialize};

use crate::embed::ImageEmbedSkipReason;
use crate::encoder::MdWarning;
use crate::error::{MdError, MdResult};

/// 이미지 1개의 적재 상한 (md 입력 자체의 50 MB 상한과 동일 계열).
pub(crate) const MAX_IMAGE_BYTES: u64 = 50 * 1024 * 1024;

/// 경고·오류 문자열에 원문을 통째로 싣지 않기 위한 표시 상한 (문자 수).
const MAX_SHOWN_CHARS: usize = 64;

// `AssetPlanMismatch.detail` 고정 문자열 — 할당 없는 안정 진단 문구.
const MISMATCH_NOT_IN_PLAN: &str =
    "occurrence is not in the asset plan collected from the document";
const MISMATCH_NOT_A_FILE: &str =
    "occurrence is not a File entry — data: and remote sources are resolved by finish_assets";
const MISMATCH_DUPLICATE: &str = "occurrence was provided more than once";
const MISMATCH_NOT_PROVIDED: &str = "File occurrence in the plan was not provided";
const MISMATCH_IDENTITY_IS_INLINE: &str =
    "provided identity collides with an inline data: URI occurrence in the same document";

// ---------------------------------------------------------------------------
// 계약 타입
// ---------------------------------------------------------------------------

/// 문서 안의 이미지 run 하나를 가리키는 주소.
///
/// 모듈 문서의 "주소 지정 계약" 절을 함께 읽을 것 — 두 필드 모두 **문서
/// 순서 방문 번호**이지 안정 ID 가 아니다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RunLocator {
    /// 문서 순서 문단 방문 번호 (0-기반, 중첩 문단 포함).
    pub paragraph: usize,
    /// 그 문단의 변형 전 run 벡터 인덱스 (0-기반).
    pub run: usize,
}

impl RunLocator {
    /// 주소를 만든다.
    #[must_use]
    pub fn new(paragraph: usize, run: usize) -> Self {
        Self { paragraph, run }
    }
}

impl fmt::Display for RunLocator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "paragraph {} run {}", self.paragraph, self.run)
    }
}

/// 이미지 run 의 `src` 분류 — 어디서 바이트를 구해야 하는지.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum AssetSource {
    /// 로컬 경로 참조 (상대·절대 모두). 2단계가 해석한다.
    File(PathBuf),
    /// `data:` URI 전문 — 바이트가 문서 안에 이미 있다.
    DataUri(String),
    /// `http(s)`·프로토콜 상대 등 원격 URL. 네트워크 접근 금지라 제외된다.
    Remote(String),
}

impl AssetSource {
    /// md `![..](src)` 의 `src` 를 분류한다.
    ///
    /// 검사 순서가 계약이다: `data:` → 원격 → 파일. `data:` 페이로드 안에
    /// `://` 가 들어갈 수 있으므로 `data:` 를 먼저 본다.
    #[must_use]
    pub fn classify(src: &str) -> Self {
        if src.starts_with("data:") {
            return Self::DataUri(src.to_string());
        }
        if src.contains("://") || src.starts_with("//") {
            return Self::Remote(src.to_string());
        }
        Self::File(PathBuf::from(src))
    }

    /// 원래 `src` 문자열을 돌려준다 (경고·진단 표시용).
    #[must_use]
    pub fn as_src(&self) -> Cow<'_, str> {
        match self {
            Self::File(path) => path.to_string_lossy(),
            Self::DataUri(src) | Self::Remote(src) => Cow::Borrowed(src),
        }
    }
}

/// 계획 1건 — "이 위치의 이미지는 이 출처에서 와야 한다".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetPlanEntry {
    /// 대상 이미지 run 주소.
    pub occurrence: RunLocator,
    /// 바이트를 구할 출처.
    pub source: AssetSource,
}

/// 자산의 정체 — 같은 정체면 같은 바이트이며 패키지 키를 공유한다(dedup).
///
/// 파일 provider 는 [`Self::CanonicalFile`] 을 쓴다. 나머지 두 변형은
/// 파일시스템 없는 provider 가 정체를 선언할 수 있게 열어 둔 것이다.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum AssetIdentity {
    /// 정규화된 절대 경로 (동일 파일의 다른 철자를 하나로 모은다).
    CanonicalFile(PathBuf),
    /// 내용 해시 (예: SHA-256) — 경로가 없는 provider 용.
    ContentHash([u8; 32]),
    /// provider 가 정한 불투명 키 (`data:` URI 전문 등).
    Opaque(String),
}

impl fmt::Display for AssetIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CanonicalFile(path) => {
                write!(f, "file:{}", truncate_for_display(&path.to_string_lossy()))
            }
            Self::ContentHash(hash) => {
                f.write_str("hash:")?;
                for byte in hash {
                    write!(f, "{byte:02x}")?;
                }
                Ok(())
            }
            Self::Opaque(key) => write!(f, "opaque:{}", truncate_for_display(key)),
        }
    }
}

/// 2단계(파일 읽기)가 자산을 거부한 사유.
///
/// 3단계에서만 판정되는 사유(바이트 스니핑 실패·`data:` 파싱 실패)는 여기
/// 없다 — 그쪽은 [`ImageEmbedSkipReason`] 으로 직접 나온다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AssetReject {
    /// 상대 경로인데 해석 가능한 base 디렉터리가 없다 (또는 절대 경로의
    /// 포함을 증명할 base 가 없다).
    NoBaseDir,
    /// 파일이 없거나 경로를 정규화할 수 없다.
    Missing,
    /// 정규화 결과가 base_dir 밖 — 경로 탈출 차단.
    Escapes,
    /// 읽기 실패 (권한·정규 파일 아님 등).
    Unreadable,
    /// 적재 상한(50 MB) 초과.
    TooLarge,
}

impl AssetReject {
    /// 사용자에게 보고되는 typed 제외 사유로 옮긴다.
    #[must_use]
    pub fn skip_reason(self) -> ImageEmbedSkipReason {
        match self {
            Self::NoBaseDir => ImageEmbedSkipReason::NoBaseDir,
            Self::Missing => ImageEmbedSkipReason::MissingFile,
            Self::Escapes => ImageEmbedSkipReason::PathEscapes,
            Self::Unreadable => ImageEmbedSkipReason::Unreadable,
            Self::TooLarge => ImageEmbedSkipReason::TooLarge,
        }
    }
}

/// provider 가 계획 1건에 대해 내놓은 답.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ProvidedAsset {
    /// 바이트를 구했다.
    Resolved {
        /// 대상 이미지 run 주소.
        occurrence: RunLocator,
        /// 이 바이트의 정체 (dedup 기준).
        identity: AssetIdentity,
        /// 원본 바이트 (포맷 스니핑은 3단계가 한다).
        bytes: Vec<u8>,
    },
    /// 바이트를 구하지 못했다.
    Rejected {
        /// 대상 이미지 run 주소.
        occurrence: RunLocator,
        /// 거부 사유.
        reason: AssetReject,
    },
}

impl ProvidedAsset {
    /// 이 답이 가리키는 이미지 run 주소.
    #[must_use]
    pub fn occurrence(&self) -> RunLocator {
        match self {
            Self::Resolved { occurrence, .. } | Self::Rejected { occurrence, .. } => *occurrence,
        }
    }
}

/// 계획 1건의 최종 처리 결과.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum AssetOutcome {
    /// 패키지에 적재됐다. run 의 `path`/`format` 이 여기 값으로 재작성된다.
    Embedded {
        /// 대상 이미지 run 주소.
        occurrence: RunLocator,
        /// 합성 정규명 패키지 키 (`imageN.ext`). dedup 시 기존 키 재사용.
        key: String,
        /// 스니핑으로 확정된 실포맷.
        format: ImageFormat,
    },
    /// 제외됐다 — run 은 드롭된다.
    Dropped {
        /// 대상 이미지 run 주소.
        occurrence: RunLocator,
        /// 제외 사유.
        reason: ImageEmbedSkipReason,
    },
    /// 원격 URL 이라 제외됐다 — run 은 드롭된다 (네트워크 접근 금지).
    Remote {
        /// 대상 이미지 run 주소.
        occurrence: RunLocator,
    },
}

impl AssetOutcome {
    /// 이 결과가 가리키는 이미지 run 주소.
    #[must_use]
    pub fn occurrence(&self) -> RunLocator {
        match self {
            Self::Embedded { occurrence, .. }
            | Self::Dropped { occurrence, .. }
            | Self::Remote { occurrence } => *occurrence,
        }
    }
}

/// [`finish_assets`] 의 산출물.
#[derive(Debug)]
#[non_exhaustive]
pub struct FinishedAssets {
    /// 이미지 run 이 합성 키로 재작성되고 실패 run 이 드롭된 문서.
    pub document: Document,
    /// 인코더에 넘길 이미지 스토어 (키 = `Image.path`).
    pub image_store: ImageStore,
    /// 계획과 1:1·같은 순서인 처리 결과.
    pub outcomes: Vec<AssetOutcome>,
}

// ---------------------------------------------------------------------------
// 1단계: 계획
// ---------------------------------------------------------------------------

/// 문서의 이미지 run 을 문서 순서로 훑어 자산 계획을 만든다 (I/O 없음,
/// 복제 없음).
///
/// [`Document::for_each_paragraph`] 를 쓴다 — 완성 단계가 쓰는
/// [`Document::for_each_paragraph_mut`] 의 불변 쌍둥이이고 Core 가 두
/// 순서의 동일성을 테스트로 잠근다. 순회 순서를 smithy-md 가 따로 구현하지
/// 않는 이유: `Control` 은 `#[non_exhaustive]` 라 외부 미러는 와일드카드
/// arm 이 필요하고, 문단을 담는 variant 가 새로 생겨도 **조용히 놓친다**
/// (= dangling `binaryItemIDRef`).
#[must_use]
pub fn collect_asset_plan(document: &Document) -> Vec<AssetPlanEntry> {
    let mut plan = Vec::new();
    let mut paragraph = 0usize;
    document.for_each_paragraph(|para| {
        for (run, item) in para.runs.iter().enumerate() {
            if let RunContent::Image(img) = &item.content {
                plan.push(AssetPlanEntry {
                    occurrence: RunLocator::new(paragraph, run),
                    source: AssetSource::classify(&img.path),
                });
            }
        }
        paragraph += 1;
    });
    plan
}

// ---------------------------------------------------------------------------
// 3단계: 완성
// ---------------------------------------------------------------------------

/// 계획 1건에 대해 3단계가 실제로 수행할 일 — 계획과 1:1 정렬된다.
///
/// 출처와 제공 결과를 **미리 짝지어** 두므로 [`apply`] 는 불가능 조합을
/// 다룰 필요가 없다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Prepared {
    /// `data:` URI — 여기서 디코드한다 (인자는 URI 전문).
    DataUri(String),
    /// 원격 URL — 제외.
    Remote,
    /// provider 가 바이트를 줬다.
    Provided {
        /// dedup 기준 정체.
        identity: AssetIdentity,
        /// 원본 바이트.
        bytes: Vec<u8>,
    },
    /// provider 가 거부했다.
    Rejected(AssetReject),
}

/// 제공된 바이트로 문서를 완성한다 (I/O 없음).
///
/// `plan` 은 호출자가 [`collect_asset_plan`] 으로 얻어 `provided` 를 만들 때
/// 근거로 삼은 계획이다. 이 함수는 `document` 에서 계획을 **다시 수집해
/// `plan` 과 전건 대조**한 뒤에야 나머지 검사를 한다. 주소(
/// [`RunLocator`])는 문서 순서 방문 번호일 뿐이라, 계획 수집과 완성 사이에
/// 두 이미지의 `src` 가 서로 **맞바뀌면 주소는 그대로인 채 내용만 뒤바뀐다**
/// — 대조 없이는 A 의 바이트가 B 의 run 에 박힌다. 그래서 길이·주소·출처·
/// 순서가 하나라도 다르면 즉시 [`MdError::AssetPlanMismatch`] 다.
///
/// 나머지 계약:
///
/// - 계획의 [`AssetSource::File`] 항목마다 **정확히 하나**의
///   [`ProvidedAsset`] 이 있어야 한다.
/// - `data:`·원격 항목에 대한 [`ProvidedAsset`] 은 계약 위반이다 (이 두
///   출처는 이 단계가 직접 처리한다 — 원격을 바깥에서 채워 넣는 우회로를
///   만들지 않는다).
/// - 같은 [`AssetIdentity`] 에 서로 다른 바이트가 오면
///   [`MdError::AssetIdentityConflict`] 로 거부한다.
///
/// # Errors
///
/// 위 계약을 어기면 [`MdError::AssetPlanMismatch`] 또는
/// [`MdError::AssetIdentityConflict`] 를 돌려준다. 검증은 문서를 만지기
/// **전에** 끝나므로 부분 변형된 문서가 나오는 일은 없다 — 다만 `document`
/// 는 이 함수가 가져갔으므로 **오류일 때 돌려받지 못한다**. 소유권을
/// 넘기기 전에 확인하려면 [`validate_assets`] 를 먼저 부른다.
///
/// # 이력
///
/// `plan` 인자는 R1 리뷰(F1)에서 추가됐다. 이 API 는 아직 릴리스되지
/// 않았으므로(브랜치 `feat/python-bindings` 안에서만 존재) 시그니처 변경은
/// semver 사건이 아니다.
pub fn finish_assets(
    mut document: Document,
    plan: &[AssetPlanEntry],
    provided: Vec<ProvidedAsset>,
) -> MdResult<FinishedAssets> {
    check_plan_matches(plan, &document)?;
    let prepared = align(plan, provided)?;
    let (image_store, outcomes, _warnings) = apply(&mut document, plan, prepared);
    Ok(FinishedAssets { document, image_store, outcomes })
}

/// [`finish_assets`] 가 이 문서·계획·자산 조합을 받아들일지 **소유권을
/// 넘기지 않고** 미리 확인한다.
///
/// 검사 내용과 오류는 [`finish_assets`] 와 완전히 같다 (같은 구현을 쓴다).
/// `Ok(())` 면 같은 인자로 부른 [`finish_assets`] 는 계약 오류를 내지
/// 않는다 — 그 사이에 문서를 바꾸지 않는 한.
///
/// # Errors
///
/// [`MdError::AssetPlanMismatch`] · [`MdError::AssetIdentityConflict`].
pub fn validate_assets(
    document: &Document,
    plan: &[AssetPlanEntry],
    provided: &[ProvidedAsset],
) -> MdResult<()> {
    check_plan_matches(plan, document)?;
    check_contract(plan, provided)
}

/// 문서에서 계획을 다시 수집해 `plan` 과 전건 대조한다.
///
/// 이미지 run 이 아닌 곳(텍스트·표 구조 등)의 변경은 계획에 나타나지
/// 않으므로 통과한다 — 이 검사는 **계획**이 그대로인지를 묻지 문서가
/// 그대로인지를 묻지 않는다.
fn check_plan_matches(plan: &[AssetPlanEntry], document: &Document) -> MdResult<()> {
    let collected = collect_asset_plan(document);
    if collected == plan {
        return Ok(());
    }
    let index = plan
        .iter()
        .zip(&collected)
        .position(|(expected, actual)| expected != actual)
        .unwrap_or_else(|| plan.len().min(collected.len()));
    let occurrence = plan
        .get(index)
        .or_else(|| collected.get(index))
        .map_or_else(|| RunLocator::new(0, 0), |entry| entry.occurrence);
    Err(MdError::AssetPlanMismatch {
        occurrence,
        detail: format!(
            "the document drifted from the plan it was provisioned against \
             (plan has {} entries, the document now yields {}); first difference at index {}: \
             expected {}, found {}",
            plan.len(),
            collected.len(),
            index,
            render_entry(plan.get(index)),
            render_entry(collected.get(index)),
        ),
    })
}

/// 진단용 계획 항목 표시 — 출처 종류 + 절단된 src.
fn render_entry(entry: Option<&AssetPlanEntry>) -> String {
    match entry {
        None => "<none>".to_string(),
        Some(entry) => {
            let kind = match &entry.source {
                AssetSource::File(_) => "file",
                AssetSource::DataUri(_) => "data",
                AssetSource::Remote(_) => "remote",
            };
            format!("{} {kind}:{}", entry.occurrence, truncate_for_display(&entry.source.as_src()))
        }
    }
}

/// `provided` 를 계획에 맞춰 검증·정렬한다 — 모든 오류가 여기서 난다.
pub(crate) fn align(
    plan: &[AssetPlanEntry],
    provided: Vec<ProvidedAsset>,
) -> MdResult<Vec<Prepared>> {
    check_contract(plan, &provided)?;
    Ok(build_aligned(plan, provided))
}

/// 계약 검사 — 계획 형태(미지·비File·중복·미제공) + 정체(바이트 충돌·
/// `data:` 네임스페이스 침범). 검사 순서가 오류 우선순위다.
fn check_contract(plan: &[AssetPlanEntry], provided: &[ProvidedAsset]) -> MdResult<()> {
    let index = plan_index(plan);
    let mut filled = vec![false; plan.len()];
    for asset in provided {
        let occurrence = asset.occurrence();
        let Some(&i) = index.get(&occurrence) else {
            return Err(MdError::AssetPlanMismatch {
                occurrence,
                detail: MISMATCH_NOT_IN_PLAN.to_string(),
            });
        };
        match plan[i].source {
            AssetSource::File(_) => {}
            AssetSource::DataUri(_) | AssetSource::Remote(_) => {
                return Err(MdError::AssetPlanMismatch {
                    occurrence,
                    detail: MISMATCH_NOT_A_FILE.to_string(),
                });
            }
        }
        if filled[i] {
            return Err(MdError::AssetPlanMismatch {
                occurrence,
                detail: MISMATCH_DUPLICATE.to_string(),
            });
        }
        filled[i] = true;
    }
    for (entry, done) in plan.iter().zip(&filled) {
        match entry.source {
            AssetSource::File(_) if !done => {
                return Err(MdError::AssetPlanMismatch {
                    occurrence: entry.occurrence,
                    detail: MISMATCH_NOT_PROVIDED.to_string(),
                });
            }
            AssetSource::File(_) | AssetSource::DataUri(_) | AssetSource::Remote(_) => {}
        }
    }

    // `data:` 발생 건의 정체는 문서 소유다 — provider 가 같은 키를 주장하면
    // 바이트 충돌 이전에 네임스페이스 침범이다.
    let inline: HashSet<AssetIdentity> = plan
        .iter()
        .filter_map(|entry| match &entry.source {
            AssetSource::DataUri(src) => Some(AssetIdentity::Opaque(src.clone())),
            AssetSource::File(_) | AssetSource::Remote(_) => None,
        })
        .collect();
    let mut seen: HashMap<&AssetIdentity, &[u8]> = HashMap::new();
    for asset in provided {
        match asset {
            ProvidedAsset::Resolved { occurrence, identity, bytes } => {
                if inline.contains(identity) {
                    return Err(MdError::AssetPlanMismatch {
                        occurrence: *occurrence,
                        detail: MISMATCH_IDENTITY_IS_INLINE.to_string(),
                    });
                }
                if let Some(previous) = seen.insert(identity, bytes.as_slice()) {
                    if previous != bytes.as_slice() {
                        return Err(MdError::AssetIdentityConflict {
                            occurrence: *occurrence,
                            identity: identity.to_string(),
                        });
                    }
                }
            }
            ProvidedAsset::Rejected { .. } => {}
        }
    }
    Ok(())
}

/// 검증을 통과한 `provided` 를 계획 순서로 펼친다 — 무오류.
fn build_aligned(plan: &[AssetPlanEntry], provided: Vec<ProvidedAsset>) -> Vec<Prepared> {
    let index = plan_index(plan);
    let mut slots: Vec<Option<Prepared>> = plan.iter().map(|_| None).collect();
    for asset in provided {
        let Some(&i) = index.get(&asset.occurrence()) else {
            unreachable!("check_contract rejects occurrences outside the plan")
        };
        slots[i] = Some(match asset {
            ProvidedAsset::Resolved { identity, bytes, .. } => {
                Prepared::Provided { identity, bytes }
            }
            ProvidedAsset::Rejected { reason, .. } => Prepared::Rejected(reason),
        });
    }

    plan.iter()
        .zip(slots)
        .map(|(entry, slot)| match (&entry.source, slot) {
            (AssetSource::File(_), Some(ready)) => ready,
            (AssetSource::DataUri(src), None) => Prepared::DataUri(src.clone()),
            (AssetSource::Remote(_), None) => Prepared::Remote,
            (AssetSource::File(_), None)
            | (AssetSource::DataUri(_) | AssetSource::Remote(_), Some(_)) => {
                unreachable!("check_contract pairs File with exactly one provision and no other")
            }
        })
        .collect()
}

/// 발생 주소 → 계획 인덱스.
fn plan_index(plan: &[AssetPlanEntry]) -> HashMap<RunLocator, usize> {
    let mut index = HashMap::with_capacity(plan.len());
    for (i, entry) in plan.iter().enumerate() {
        index.insert(entry.occurrence, i);
    }
    index
}

/// 적재 상태 — dedup 표 + 합성 키 카운터 + 스토어.
struct Embedder {
    store: ImageStore,
    seen: HashMap<AssetIdentity, (String, ImageFormat)>,
    counter: usize,
}

impl Embedder {
    fn new() -> Self {
        Self { store: ImageStore::new(), seen: HashMap::new(), counter: 0 }
    }

    /// 바이트를 적재하고 `(패키지 키, 실포맷)` 을 돌려준다.
    ///
    /// 순서가 계약이다: dedup 조회 → 스니핑 → 카운터 증가. 스니핑 실패는
    /// 카운터를 올리지 않고 `seen` 에도 남기지 않는다 (같은 정체가 다시
    /// 나오면 다시 실패하고 다시 경고한다).
    fn embed(
        &mut self,
        identity: AssetIdentity,
        bytes: Vec<u8>,
    ) -> Result<(String, ImageFormat), ImageEmbedSkipReason> {
        if let Some((key, format)) = self.seen.get(&identity) {
            return Ok((key.clone(), format.clone()));
        }
        let Some(format) = ImageFormat::sniff(&bytes) else {
            return Err(ImageEmbedSkipReason::UnsupportedBytes);
        };
        let ext = format.canonical_extension().expect("sniff never returns Unknown");
        self.counter += 1;
        let key = format!("image{}.{ext}", self.counter);
        self.store.insert(key.clone(), bytes);
        self.seen.insert(identity, (key.clone(), format.clone()));
        Ok((key, format))
    }
}

/// 준비된 결정을 문서에 적용한다 — 무오류.
///
/// `plan` 과 `prepared` 는 [`collect_asset_plan`] 이 만든 계획과 1:1
/// 정렬이므로, 같은 순회를 다시 돌며 이미지 run 마다 하나씩 소비한다.
///
/// 경고는 결과를 만드는 **바로 그 자리에서** 함께 만든다 — 길이가 어긋날
/// 여지가 없으므로 호출자 쪽에 패닉 경로가 생기지 않는다. 결과→경고 사상
/// 자체는 [`warning_for`] 하나뿐이고 [`warnings_from`] 도 그것을 쓴다.
pub(crate) fn apply(
    document: &mut Document,
    plan: &[AssetPlanEntry],
    prepared: Vec<Prepared>,
) -> (ImageStore, Vec<AssetOutcome>, Vec<MdWarning>) {
    let mut embedder = Embedder::new();
    let mut outcomes = Vec::with_capacity(prepared.len());
    let mut warnings = Vec::new();
    let mut queue = plan.iter().zip(prepared);
    let mut paragraph = 0usize;

    document.for_each_paragraph_mut(|para| {
        let mut dropped_char_shape = None;
        let mut next_run = 0usize;
        para.runs.retain_mut(|item| {
            // retain_mut 은 원본 순서로 정확히 한 번씩 방문하므로 이 인덱스가
            // "변형 전 run 인덱스" 다 (생존 벡터 인덱스가 아니다).
            let run = next_run;
            next_run += 1;
            let char_shape_id = item.char_shape_id;
            let RunContent::Image(img) = &mut item.content else { return true };
            let occurrence = RunLocator::new(paragraph, run);
            let (entry, ready) =
                queue.next().expect("prepared is aligned 1:1 with the document's plan");
            let mut record = |outcome: AssetOutcome| {
                if let Some(warning) = warning_for(&entry.source, &outcome) {
                    warnings.push(warning);
                }
                outcomes.push(outcome);
            };
            let embedded = match ready {
                Prepared::Remote => {
                    record(AssetOutcome::Remote { occurrence });
                    dropped_char_shape = Some(char_shape_id);
                    return false;
                }
                Prepared::Rejected(reason) => Err(reason.skip_reason()),
                Prepared::Provided { identity, bytes } => embedder.embed(identity, bytes),
                Prepared::DataUri(src) => decode_data_uri(&src)
                    .and_then(|bytes| embedder.embed(AssetIdentity::Opaque(src), bytes)),
            };
            match embedded {
                Ok((key, format)) => {
                    img.path.clone_from(&key);
                    img.format = format.clone();
                    record(AssetOutcome::Embedded { occurrence, key, format });
                    true
                }
                Err(reason) => {
                    record(AssetOutcome::Dropped { occurrence, reason });
                    dropped_char_shape = Some(char_shape_id);
                    false
                }
            }
        });
        // 이미지 단독 문단이 드롭으로 비면 빈 텍스트 run 으로 문단 유효성을
        // 보존한다 (validate 는 run 0 문단을 거부한다).
        if para.runs.is_empty() {
            if let Some(cs) = dropped_char_shape {
                para.runs.push(Run::text("", cs));
            }
        }
        paragraph += 1;
    });

    (embedder.store, outcomes, warnings)
}

/// `data:` URI 전문을 디코드한다 — base64 payload 만 지원한다 (이미지의
/// 비-base64 `data:` URI 는 비현실적).
fn decode_data_uri(src: &str) -> Result<Vec<u8>, ImageEmbedSkipReason> {
    let rest = src.strip_prefix("data:").ok_or(ImageEmbedSkipReason::InvalidDataUri)?;
    let comma = rest.find(',').ok_or(ImageEmbedSkipReason::InvalidDataUri)?;
    let (meta, payload) = rest.split_at(comma);
    let payload = &payload[1..];
    if !meta.ends_with(";base64") {
        return Err(ImageEmbedSkipReason::InvalidDataUri);
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload.trim())
        .map_err(|_| ImageEmbedSkipReason::InvalidDataUri)?;
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err(ImageEmbedSkipReason::TooLarge);
    }
    Ok(bytes)
}

// ---------------------------------------------------------------------------
// 경고 파생
// ---------------------------------------------------------------------------

/// 계획과 결과를 짝지어 사용자 경고를 만든다.
///
/// 경고는 결과에서 **파생**된다 — 두 목록이 갈라질 수 없게 하려는 것이다.
/// 위치로 짝지으므로 (모듈 문서의 불변식) 두 목록의 길이는 반드시 같아야
/// 한다.
///
/// # Errors
///
/// 길이가 다르면 [`MdError::AssetPlanMismatch`]. 짧은 쪽에서 조용히 멈추면
/// **경고가 소리 없이 사라지므로**(= 무음 드롭) 거부한다 — R1 리뷰 F7.
pub fn warnings_from(
    plan: &[AssetPlanEntry],
    outcomes: &[AssetOutcome],
) -> MdResult<Vec<MdWarning>> {
    if plan.len() != outcomes.len() {
        let occurrence = plan
            .first()
            .map(|entry| entry.occurrence)
            .or_else(|| outcomes.first().map(AssetOutcome::occurrence))
            .unwrap_or_else(|| RunLocator::new(0, 0));
        return Err(MdError::AssetPlanMismatch {
            occurrence,
            detail: format!(
                "cannot derive warnings: the plan has {} entries but {} outcomes were given",
                plan.len(),
                outcomes.len(),
            ),
        });
    }
    Ok(plan
        .iter()
        .zip(outcomes)
        .filter_map(|(entry, outcome)| warning_for(&entry.source, outcome))
        .collect())
}

/// 결과 1건 → 경고 0..1건. 결과에서 경고를 뽑는 **유일한** 사상이다
/// ([`apply`] 와 [`warnings_from`] 이 함께 쓴다).
fn warning_for(source: &AssetSource, outcome: &AssetOutcome) -> Option<MdWarning> {
    match outcome {
        AssetOutcome::Embedded { .. } => None,
        AssetOutcome::Dropped { reason, .. } => {
            Some(skip_warning(&source.as_src(), reason.clone()))
        }
        AssetOutcome::Remote { .. } => {
            Some(skip_warning(&source.as_src(), ImageEmbedSkipReason::RemoteUrl))
        }
    }
}

/// src 를 표시용으로 절단해 경고를 만든다 (`data:` URI 전문 방지).
fn skip_warning(src: &str, reason: ImageEmbedSkipReason) -> MdWarning {
    MdWarning::ImageEmbedSkipped { src: truncate_for_display(src), reason }
}

/// 표시 상한을 넘는 문자열을 잘라 말줄임을 붙인다.
fn truncate_for_display(src: &str) -> String {
    if src.chars().count() > MAX_SHOWN_CHARS {
        let head: String = src.chars().take(MAX_SHOWN_CHARS).collect();
        format!("{head}…")
    } else {
        src.to_string()
    }
}

#[cfg(test)]
mod tests {
    use hwpforge_core::image::Image;
    use hwpforge_core::paragraph::Paragraph;
    use hwpforge_core::section::Section;
    use hwpforge_core::table::{Table, TableCell, TableRow};
    use hwpforge_core::PageSettings;
    use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};

    use super::*;

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3];
    const PNG_OTHER: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 9, 9, 9];

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
        for section in doc.sections() {
            for para in &section.paragraphs {
                for run in &para.runs {
                    if let RunContent::Image(img) = &run.content {
                        out.push(img.path.clone());
                    }
                }
            }
        }
        out
    }

    fn data_uri(mime: &str, bytes: &[u8]) -> String {
        format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
    }

    #[test]
    fn plan_order_matches_document_order_for_mixed_sources() {
        let inline = data_uri("image/png", PNG);
        let doc = doc_with_srcs(&["a.png", &inline, "https://example.com/b.png"]);
        let plan = collect_asset_plan(&doc);

        assert_eq!(
            plan.iter().map(|e| e.occurrence).collect::<Vec<_>>(),
            vec![RunLocator::new(0, 0), RunLocator::new(0, 1), RunLocator::new(0, 2)]
        );
        assert_eq!(
            plan.iter().map(|e| e.source.clone()).collect::<Vec<_>>(),
            vec![
                AssetSource::File(PathBuf::from("a.png")),
                AssetSource::DataUri(inline),
                AssetSource::Remote("https://example.com/b.png".to_string()),
            ]
        );
    }

    #[test]
    fn plan_covers_images_nested_in_table_cells() {
        // 표 셀 문단은 Core 순회기가 방문한다 — md 디코더도 셀 이미지를 만든다.
        let cell = TableCell::new(
            vec![Paragraph::with_runs(vec![image_run("cell.png")], ParaShapeIndex::new(0))],
            HwpUnit::from_mm(40.0).expect("cell width"),
        );
        let table = Table::new(vec![TableRow::new(vec![cell])]);
        let host = Paragraph::with_runs(
            vec![Run::table(table, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(vec![host], PageSettings::a4()));

        let plan = collect_asset_plan(&doc);
        assert_eq!(plan.len(), 1, "셀 안 이미지도 계획에 잡혀야 한다: {plan:?}");
        // host 문단이 0번, 셀 문단이 1번 (pre-order).
        assert_eq!(plan[0].occurrence, RunLocator::new(1, 0));
        assert_eq!(plan[0].source, AssetSource::File(PathBuf::from("cell.png")));
    }

    #[test]
    fn outcomes_are_one_per_plan_entry_in_plan_order() {
        let inline = data_uri("image/png", PNG);
        let doc = doc_with_srcs(&[&inline, "https://example.com/b.png", &inline]);
        let plan = collect_asset_plan(&doc);
        let finished = finish_assets(doc, &plan, Vec::new()).expect("no File entries to provide");

        assert_eq!(finished.outcomes.len(), plan.len());
        assert_eq!(
            finished.outcomes.iter().map(AssetOutcome::occurrence).collect::<Vec<_>>(),
            plan.iter().map(|e| e.occurrence).collect::<Vec<_>>()
        );
        // 세 번째는 첫 번째와 같은 data: URI — dedup 으로 같은 키를 재사용하되
        // 결과는 여전히 Embedded 하나다.
        assert!(
            matches!(finished.outcomes[2], AssetOutcome::Embedded { ref key, .. } if key == "image1.png")
        );
    }

    #[test]
    fn finish_rejects_provided_occurrence_outside_the_plan() {
        let doc = doc_with_srcs(&["a.png"]);
        let plan = collect_asset_plan(&doc);
        let err = finish_assets(
            doc,
            &plan,
            vec![ProvidedAsset::Rejected {
                occurrence: RunLocator::new(9, 9),
                reason: AssetReject::Missing,
            }],
        )
        .expect_err("locator is not in the plan");
        assert!(
            matches!(&err, MdError::AssetPlanMismatch { detail, .. } if detail.as_str() == MISMATCH_NOT_IN_PLAN),
            "{err:?}"
        );
    }

    #[test]
    fn finish_rejects_duplicate_occurrence() {
        let doc = doc_with_srcs(&["a.png"]);
        let plan = collect_asset_plan(&doc);
        let at = RunLocator::new(0, 0);
        let err = finish_assets(
            doc,
            &plan,
            vec![
                ProvidedAsset::Resolved {
                    occurrence: at,
                    identity: AssetIdentity::Opaque("mem:a".to_string()),
                    bytes: PNG.to_vec(),
                },
                ProvidedAsset::Rejected { occurrence: at, reason: AssetReject::Missing },
            ],
        )
        .expect_err("same occurrence provided twice");
        assert!(
            matches!(&err, MdError::AssetPlanMismatch { detail, .. } if detail.as_str() == MISMATCH_DUPLICATE),
            "{err:?}"
        );
    }

    #[test]
    fn finish_rejects_missing_file_occurrence() {
        let doc = doc_with_srcs(&["a.png"]);
        let plan = collect_asset_plan(&doc);
        let err = finish_assets(doc, &plan, Vec::new()).expect_err("File entry was not provided");
        assert!(
            matches!(&err, MdError::AssetPlanMismatch { detail, .. } if detail.as_str() == MISMATCH_NOT_PROVIDED),
            "{err:?}"
        );
    }

    #[test]
    fn finish_rejects_provided_entry_for_a_non_file_occurrence() {
        // 원격을 바깥에서 채워 넣는 우회로를 열지 않는다.
        let doc = doc_with_srcs(&["https://example.com/a.png"]);
        let plan = collect_asset_plan(&doc);
        let err = finish_assets(
            doc,
            &plan,
            vec![ProvidedAsset::Resolved {
                occurrence: RunLocator::new(0, 0),
                identity: AssetIdentity::Opaque("fetched".to_string()),
                bytes: PNG.to_vec(),
            }],
        )
        .expect_err("remote occurrences are finish-owned");
        assert!(
            matches!(&err, MdError::AssetPlanMismatch { detail, .. } if detail.as_str() == MISMATCH_NOT_A_FILE),
            "{err:?}"
        );
    }

    #[test]
    fn same_identity_with_different_bytes_is_a_conflict() {
        let doc = doc_with_srcs(&["a.png", "b.png"]);
        let plan = collect_asset_plan(&doc);
        let shared = AssetIdentity::Opaque("mem:shared".to_string());
        let err = finish_assets(
            doc,
            &plan,
            vec![
                ProvidedAsset::Resolved {
                    occurrence: RunLocator::new(0, 0),
                    identity: shared.clone(),
                    bytes: PNG.to_vec(),
                },
                ProvidedAsset::Resolved {
                    occurrence: RunLocator::new(0, 1),
                    identity: shared,
                    bytes: PNG_OTHER.to_vec(),
                },
            ],
        )
        .expect_err("one identity cannot carry two byte sequences");
        assert!(matches!(err, MdError::AssetIdentityConflict { .. }), "{err:?}");
    }

    #[test]
    fn provided_identity_may_not_squat_an_inline_data_uri() {
        let inline = data_uri("image/png", PNG);
        let doc = doc_with_srcs(&[&inline, "a.png"]);
        let plan = collect_asset_plan(&doc);
        let err = finish_assets(
            doc,
            &plan,
            vec![ProvidedAsset::Resolved {
                occurrence: RunLocator::new(0, 1),
                identity: AssetIdentity::Opaque(inline),
                bytes: PNG_OTHER.to_vec(),
            }],
        )
        .expect_err("data: URI identities are owned by the document");
        assert!(
            matches!(&err, MdError::AssetPlanMismatch { detail, .. } if detail.as_str() == MISMATCH_IDENTITY_IS_INLINE),
            "{err:?}"
        );
    }

    #[test]
    fn identical_bytes_via_two_data_uri_encodings_are_not_deduped() {
        // 오늘의 동작 잠금: dedup 기준은 URI 전문이라 mime 이 다르면 별개다.
        let a = data_uri("image/png", PNG);
        let b = data_uri("image/x-png", PNG);
        assert_ne!(a, b);
        let doc = doc_with_srcs(&[&a, &b]);
        let plan = collect_asset_plan(&doc);
        let finished = finish_assets(doc, &plan, Vec::new()).expect("inline only");

        assert_eq!(image_paths(&finished.document), vec!["image1.png", "image2.png"]);
        assert_eq!(finished.image_store.len(), 2);
    }

    #[test]
    fn in_memory_provider_embeds_without_touching_the_filesystem() {
        // provider 비의존 증명: 디스크에 없는 상대 경로를 Opaque·ContentHash
        // 정체의 메모리 바이트로 채운다.
        let doc = doc_with_srcs(&["does/not/exist.png", "also/missing.png"]);
        let plan = collect_asset_plan(&doc);
        let finished = finish_assets(
            doc,
            &plan,
            vec![
                ProvidedAsset::Resolved {
                    occurrence: RunLocator::new(0, 0),
                    identity: AssetIdentity::Opaque("mem://first".to_string()),
                    bytes: PNG.to_vec(),
                },
                ProvidedAsset::Resolved {
                    occurrence: RunLocator::new(0, 1),
                    identity: AssetIdentity::ContentHash([7u8; 32]),
                    bytes: PNG_OTHER.to_vec(),
                },
            ],
        )
        .expect("memory provider satisfies the plan");

        assert_eq!(image_paths(&finished.document), vec!["image1.png", "image2.png"]);
        assert_eq!(finished.image_store.get("image1.png"), Some(PNG));
        assert_eq!(finished.image_store.get("image2.png"), Some(PNG_OTHER));
        assert!(finished.outcomes.iter().all(|o| matches!(o, AssetOutcome::Embedded { .. })));
    }

    #[test]
    fn rejected_provision_drops_the_run_with_the_mapped_reason() {
        let doc = doc_with_srcs(&["gone.png"]);
        let plan = collect_asset_plan(&doc);
        let finished = finish_assets(
            doc,
            &plan,
            vec![ProvidedAsset::Rejected {
                occurrence: RunLocator::new(0, 0),
                reason: AssetReject::Escapes,
            }],
        )
        .expect("rejection is a normal outcome");

        assert!(image_paths(&finished.document).is_empty());
        assert!(matches!(
            finished.outcomes[0],
            AssetOutcome::Dropped { reason: ImageEmbedSkipReason::PathEscapes, .. }
        ));
        let warnings = warnings_from(&plan, &finished.outcomes).expect("aligned");
        assert!(matches!(
            &warnings[..],
            [MdWarning::ImageEmbedSkipped { reason: ImageEmbedSkipReason::PathEscapes, src }]
                if src == "gone.png"
        ));
    }

    #[test]
    fn validate_assets_checks_without_taking_ownership() {
        // 소유권을 넘기기 전에 거절을 알 수 있어야 한다 — finish_assets 는
        // 오류일 때 문서를 돌려주지 않는다.
        let doc = doc_with_srcs(&["a.png"]);
        let bad = vec![ProvidedAsset::Rejected {
            occurrence: RunLocator::new(9, 9),
            reason: AssetReject::Missing,
        }];
        let plan = collect_asset_plan(&doc);
        let err = validate_assets(&doc, &plan, &bad).expect_err("locator is not in the plan");
        assert!(
            matches!(&err, MdError::AssetPlanMismatch { detail, .. } if detail.as_str() == MISMATCH_NOT_IN_PLAN),
            "{err:?}"
        );

        // 거절당한 뒤에도 문서는 호출자 것이다 — 고쳐서 다시 시도한다.
        let good = vec![ProvidedAsset::Resolved {
            occurrence: RunLocator::new(0, 0),
            identity: AssetIdentity::Opaque("mem:a".to_string()),
            bytes: PNG.to_vec(),
        }];
        validate_assets(&doc, &plan, &good).expect("contract is satisfied");
        let finished = finish_assets(doc, &plan, good).expect("validated input finishes");
        assert_eq!(image_paths(&finished.document), vec!["image1.png"]);
    }

    #[test]
    fn warnings_from_rejects_length_mismatch() {
        // 짧은 쪽에서 멈추면 경고가 무음 드롭된다 — 거부해야 한다 (R1 F7).
        let doc = doc_with_srcs(&["https://example.com/a.png"]);
        let plan = collect_asset_plan(&doc);
        let err = warnings_from(&plan, &[]).expect_err("1 plan entry vs 0 outcomes");
        assert!(
            matches!(&err, MdError::AssetPlanMismatch { detail, .. }
                if detail.contains("1 entries but 0 outcomes")),
            "{err:?}"
        );
        let err = warnings_from(&[], &[AssetOutcome::Remote { occurrence: RunLocator::new(0, 0) }])
            .expect_err("0 plan entries vs 1 outcome");
        assert!(matches!(err, MdError::AssetPlanMismatch { .. }), "{err:?}");
    }

    #[test]
    fn finish_rejects_sources_swapped_behind_unchanged_locators() {
        // R1 F1: 주소는 그대로인데 두 이미지의 src 가 맞바뀌면, 대조가 없으면
        // a.png 의 바이트가 b.png 의 run 에 박힌다.
        let mut doc = doc_with_srcs(&["a.png", "b.png"]);
        let plan = collect_asset_plan(&doc);
        let provided = vec![
            ProvidedAsset::Resolved {
                occurrence: RunLocator::new(0, 0),
                identity: AssetIdentity::Opaque("mem:a".to_string()),
                bytes: PNG.to_vec(),
            },
            ProvidedAsset::Resolved {
                occurrence: RunLocator::new(0, 1),
                identity: AssetIdentity::Opaque("mem:b".to_string()),
                bytes: PNG_OTHER.to_vec(),
            },
        ];

        // 주소는 건드리지 않고 src 만 맞바꾼다.
        doc.for_each_paragraph_mut(|para| {
            for run in &mut para.runs {
                if let RunContent::Image(img) = &mut run.content {
                    img.path = if img.path == "a.png" { "b.png" } else { "a.png" }.to_string();
                }
            }
        });
        assert_eq!(collect_asset_plan(&doc).len(), plan.len(), "주소 수는 그대로");

        let err = validate_assets(&doc, &plan, &provided).expect_err("the plan drifted");
        assert!(
            matches!(&err, MdError::AssetPlanMismatch { detail, .. }
                if detail.contains("first difference at index 0")
                    && detail.contains("expected paragraph 0 run 0 file:a.png")
                    && detail.contains("found paragraph 0 run 0 file:b.png")),
            "{err:?}"
        );
        let err = finish_assets(doc, &plan, provided).expect_err("the plan drifted");
        assert!(matches!(err, MdError::AssetPlanMismatch { .. }), "{err:?}");
    }

    #[test]
    fn finish_tolerates_edits_that_do_not_touch_the_plan() {
        // 대조 대상은 **계획**이지 문서 전체가 아니다 — 텍스트 run 수정은
        // 계획에 나타나지 않으므로 통과해야 한다.
        let para = Paragraph::with_runs(
            vec![Run::text("before", CharShapeIndex::new(0)), image_run("a.png")],
            ParaShapeIndex::new(0),
        );
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));
        let plan = collect_asset_plan(&doc);
        let provided = vec![ProvidedAsset::Resolved {
            occurrence: plan[0].occurrence,
            identity: AssetIdentity::Opaque("mem:a".to_string()),
            bytes: PNG.to_vec(),
        }];

        doc.for_each_paragraph_mut(|para| {
            for run in &mut para.runs {
                if let RunContent::Text(text) = &mut run.content {
                    *text = "after the caller edited unrelated prose".to_string();
                }
            }
        });

        validate_assets(&doc, &plan, &provided).expect("text edits do not move the plan");
        let finished =
            finish_assets(doc, &plan, provided).expect("text edits do not move the plan");
        assert_eq!(image_paths(&finished.document), vec!["image1.png"]);
    }

    #[test]
    fn finish_rejects_a_plan_whose_length_no_longer_matches() {
        let mut doc = doc_with_srcs(&["a.png", "b.png"]);
        let plan = collect_asset_plan(&doc);
        doc.for_each_paragraph_mut(|para| para.runs.truncate(1));

        let err = validate_assets(&doc, &plan, &[]).expect_err("an image run disappeared");
        assert!(
            matches!(&err, MdError::AssetPlanMismatch { detail, .. }
                if detail.contains("plan has 2 entries, the document now yields 1")
                    && detail.contains("first difference at index 1")
                    && detail.contains("found <none>")),
            "{err:?}"
        );
    }

    #[test]
    fn identity_display_is_truncated_and_tagged() {
        let long = "x".repeat(500);
        let shown = AssetIdentity::Opaque(long).to_string();
        assert!(shown.starts_with("opaque:"));
        assert!(shown.chars().count() <= "opaque:".len() + MAX_SHOWN_CHARS + 1, "{shown}");
        assert_eq!(
            AssetIdentity::ContentHash([0xAB; 32]).to_string(),
            format!("hash:{}", "ab".repeat(32))
        );
    }
}
