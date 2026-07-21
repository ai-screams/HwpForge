//! `hwpforge_stamp_plan` / `hwpforge_stamp` — 산문 placeholder 를 이름 붙은
//! 누름틀로 승격하는 템플릿 스탬핑 (E6, 2단계 plan/apply).
//!
//! plan 은 클래스-A 후보를 나열하고, 호출자가 후보 전량을 이름 또는 ignore
//! 로 분류한 spec 배열을 stamp 에 전달한다. stamp 는 fail-closed admission
//! 게이트(무손실 왕복 + ZIP closed-world) 뒤에서 all-or-nothing 으로
//! 적용하고 manifest 를 함께 기록한다.

use serde::Serialize;

use hwpforge_smithy_hwpx::stamp::{
    HwpxStamper, StampCandidate, StampError, StampSpec, StampedField, StamperError,
};

use crate::output::{read_file_bytes, write_output_file, ToolErrorInfo};

/// Output data from a successful stamp-plan operation.
#[derive(Debug, Serialize)]
pub struct StampPlanData {
    /// Discovered candidates (document order). Author one spec per
    /// candidate: unguarded candidates MUST be named or ignored.
    pub candidates: Vec<StampCandidate>,
}

/// Output data from a successful stamp operation.
#[derive(Debug, Serialize)]
pub struct StampData {
    /// Path to the stamped HWPX file.
    pub output_path: String,
    /// Path to the manifest JSON.
    pub manifest_path: String,
    /// Fields created by this stamp (spec order).
    pub stamped: Vec<StampedField>,
    /// Number of explicitly ignored candidates.
    pub ignored: usize,
    /// Guarded candidates skipped because no spec approved them.
    pub skipped_guarded: usize,
    /// Size of the output file in bytes.
    pub size_bytes: u64,
}

/// Discover class-A placeholder candidates.
pub fn run_stamp_plan(file_path: &str) -> Result<StampPlanData, ToolErrorInfo> {
    let bytes = read_file_bytes(file_path)?;
    let candidates = HwpxStamper::plan_bytes(&bytes).map_err(map_stamper_error)?;
    Ok(StampPlanData { candidates })
}

/// Apply the approved spec set behind the admission gate.
pub fn run_stamp(
    file_path: &str,
    specs: &[StampSpec],
    output_path: &str,
    manifest_path: Option<&str>,
) -> Result<StampData, ToolErrorInfo> {
    if !output_path.ends_with(".hwpx") {
        return Err(ToolErrorInfo::new(
            "INVALID_EXTENSION",
            format!("Output path must end with .hwpx: {output_path}"),
            "Use a .hwpx extension for the output file.",
        ));
    }

    let bytes = read_file_bytes(file_path)?;
    let result = HwpxStamper::stamp(&bytes, specs).map_err(map_stamper_error)?;

    // Review L1: serialize the manifest BEFORE writing anything, and remove
    // the .hwpx if the manifest write fails — a failed call must leave no
    // partial artifact behind (fail-closed).
    let manifest_file = manifest_path
        .map(str::to_string)
        .unwrap_or_else(|| format!("{}.manifest.json", output_path.trim_end_matches(".hwpx")));
    let manifest_json = serde_json::to_string_pretty(&result.manifest).map_err(|e| {
        ToolErrorInfo::new(
            "STAMP_MANIFEST_SERIALIZE",
            format!("manifest serialization failed: {e}"),
            "Report this as a bug.",
        )
    })?;
    write_output_file(output_path, &result.bytes)?;
    if let Err(e) = write_output_file(&manifest_file, manifest_json.as_bytes()) {
        let _ = std::fs::remove_file(output_path);
        return Err(e);
    }

    let size_bytes = result.bytes.len() as u64;
    Ok(StampData {
        output_path: output_path.to_string(),
        manifest_path: manifest_file,
        stamped: result.outcome.stamped,
        ignored: result.outcome.ignored,
        skipped_guarded: result.outcome.skipped_guarded.len(),
        size_bytes,
    })
}

fn map_stamper_error(error: StamperError) -> ToolErrorInfo {
    match error {
        StamperError::NotRoundTripSafe { component, diff_path } => ToolErrorInfo::new(
            "INPUT_NOT_ROUNDTRIP_SAFE",
            format!("input is not round-trip-safe: {component} differs at {diff_path}"),
            "이 입력은 무손실 재인코드가 증명되지 않아 거부됩니다 (fail-closed). 코덱 갭 수정 전까지 스탬핑 불가.",
        ),
        StamperError::UncarriedZipEntries { entries } => ToolErrorInfo::new(
            "INPUT_ENTRIES_NOT_CARRIED",
            format!("encoder does not carry input entries: {}", entries.join(", ")),
            "재인코드 시 유실될 ZIP 엔트리가 있어 거부됩니다 (fail-closed).",
        ),
        StamperError::Stamp(inner) => map_stamp_error(inner),
        StamperError::ManifestInvariant { detail } => ToolErrorInfo::new(
            "STAMP_MANIFEST_INVARIANT",
            detail,
            "Report this as a bug — the output inventory violated an invariant.",
        ),
        StamperError::Codec(msg) => ToolErrorInfo::new(
            "STAMP_CODEC_FAILED",
            msg,
            "Check that the file is valid HWPX.",
        ),
        other => ToolErrorInfo::new("STAMP_FAILED", other.to_string(), "Unexpected failure."),
    }
}

fn map_stamp_error(error: StampError) -> ToolErrorInfo {
    match error {
        StampError::UncoveredCandidate { section, path, span, marker } => ToolErrorInfo::new(
            "STAMP_CANDIDATE_UNCOVERED",
            format!(
                "unguarded candidate {marker:?} (section {section}, {path} [{}..{}]) has no spec",
                span.start, span.end
            ),
            "모든 무가드 후보는 이름 또는 ignore 로 분류해야 합니다 — hwpforge_stamp_plan 출력을 빠짐없이 사용하세요.",
        ),
        StampError::UnknownSpec { section, path, span } => ToolErrorInfo::new(
            "STAMP_SPEC_STALE",
            format!(
                "spec matches no live candidate: section {section}, {path} [{}..{}]",
                span.start, span.end
            ),
            "문서가 변경됐거나 span 이 어긋났습니다 — hwpforge_stamp_plan 을 다시 실행하세요.",
        ),
        StampError::MarkerMismatch { path, expected, found } => ToolErrorInfo::new(
            "STAMP_MARKER_MISMATCH",
            format!("marker mismatch at {path}: spec {expected:?}, document {found:?}"),
            "spec 의 marker 는 문서의 현재 텍스트와 일치해야 합니다.",
        ),
        StampError::DuplicateSpec { path, span } => ToolErrorInfo::new(
            "STAMP_SPEC_DUPLICATE",
            format!("duplicate specs for {path} [{}..{}]", span.start, span.end),
            "같은 후보를 두 번 분류했습니다.",
        ),
        StampError::DuplicateName { name } => ToolErrorInfo::new(
            "STAMP_NAME_DUPLICATE",
            format!("duplicate field name {name:?}"),
            "필드 이름은 spec 전체에서 유일해야 합니다.",
        ),
        StampError::NameCollision { name } => ToolErrorInfo::new(
            "STAMP_NAME_COLLISION",
            format!("field name {name:?} already exists in the document"),
            "기존 누름틀과 이름이 겹칩니다 — hwpforge_fields 로 기존 이름을 확인하세요.",
        ),
        StampError::EmptyName => ToolErrorInfo::new(
            "STAMP_NAME_EMPTY",
            "field name must not be empty",
            "빈 이름은 허용되지 않습니다.",
        ),
        other => ToolErrorInfo::new("STAMP_FAILED", other.to_string(), "Unexpected failure."),
    }
}

#[cfg(test)]
mod tests {
    use hwpforge_smithy_hwpx::stamp::StampAction;

    use super::*;

    /// Markdown → HWPX: 무가드 괄호빈칸 2개 + 가드(※) 체크박스 1개.
    fn make_template(dir: &tempfile::TempDir) -> String {
        let path = dir.path().join("template.hwpx");
        crate::tools::convert::run_convert(
            "성명: (   )\n\n소속: (  )\n\n※ 해당하는 항목의 □에 표시",
            false,
            path.to_str().unwrap(),
            "default",
        )
        .unwrap();
        path.to_str().unwrap().to_string()
    }

    fn named(c: &StampCandidate, name: &str) -> StampSpec {
        StampSpec {
            section: c.section,
            path: c.path.clone(),
            span: c.span.clone(),
            marker: c.marker.clone(),
            action: StampAction::Field { name: name.to_string(), hint: None },
        }
    }

    #[test]
    fn stamp_plan_lists_candidates_with_guard() {
        let dir = tempfile::tempdir().unwrap();
        let src = make_template(&dir);
        let data = run_stamp_plan(&src).unwrap();
        assert_eq!(data.candidates.len(), 3, "{:?}", data.candidates);
        assert_eq!(data.candidates.iter().filter(|c| c.guard.is_some()).count(), 1);
    }

    #[test]
    fn stamp_happy_path_writes_output_and_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let src = make_template(&dir);
        let plan = run_stamp_plan(&src).unwrap();
        let unguarded: Vec<_> = plan.candidates.iter().filter(|c| c.guard.is_none()).collect();
        let specs = vec![named(unguarded[0], "성명"), named(unguarded[1], "소속")];
        let out = dir.path().join("stamped.hwpx");
        let data = run_stamp(&src, &specs, out.to_str().unwrap(), None).unwrap();
        assert_eq!(data.stamped.len(), 2);
        assert_eq!(data.skipped_guarded, 1);
        assert!(std::path::Path::new(&data.manifest_path).exists());

        // 스탬프 산출물은 즉시 fields 툴로 소비 가능해야 한다.
        let fields = crate::tools::fields::run_fields(out.to_str().unwrap()).unwrap();
        assert_eq!(fields.fields.len(), 2);
    }

    #[test]
    fn stamp_rejects_non_hwpx_extension() {
        let dir = tempfile::tempdir().unwrap();
        let src = make_template(&dir);
        let err = run_stamp(&src, &[], "out.zip", None).unwrap_err();
        assert_eq!(err.code, "INVALID_EXTENSION");
    }

    #[test]
    fn stamp_error_codes_reachable_via_real_calls() {
        let dir = tempfile::tempdir().unwrap();
        let src = make_template(&dir);
        let plan = run_stamp_plan(&src).unwrap();
        let unguarded: Vec<_> = plan.candidates.iter().filter(|c| c.guard.is_none()).collect();
        let (c1, c2) = (unguarded[0], unguarded[1]);
        let out = dir.path().join("never.hwpx");
        let out_s = out.to_str().unwrap();

        // 미커버 무가드 후보
        let err = run_stamp(&src, &[], out_s, None).unwrap_err();
        assert_eq!(err.code, "STAMP_CANDIDATE_UNCOVERED");

        // stale spec (span 어긋남)
        let mut stale = named(c1, "성명");
        stale.span = 0..1;
        let err = run_stamp(&src, &[stale, named(c2, "소속")], out_s, None).unwrap_err();
        assert_eq!(err.code, "STAMP_SPEC_STALE");

        // 마커 불일치
        let mut wrong = named(c1, "성명");
        wrong.marker = "(x)".to_string();
        let err = run_stamp(&src, &[wrong, named(c2, "소속")], out_s, None).unwrap_err();
        assert_eq!(err.code, "STAMP_MARKER_MISMATCH");

        // 같은 후보 이중 분류
        let err =
            run_stamp(&src, &[named(c1, "a"), named(c1, "b"), named(c2, "소속")], out_s, None)
                .unwrap_err();
        assert_eq!(err.code, "STAMP_SPEC_DUPLICATE");

        // 이름 중복
        let err =
            run_stamp(&src, &[named(c1, "같음"), named(c2, "같음")], out_s, None).unwrap_err();
        assert_eq!(err.code, "STAMP_NAME_DUPLICATE");

        // 빈 이름
        let err = run_stamp(&src, &[named(c1, ""), named(c2, "소속")], out_s, None).unwrap_err();
        assert_eq!(err.code, "STAMP_NAME_EMPTY");

        assert!(!out.exists(), "fail-closed: 거부 시 산출물이 없어야 한다");
    }
}
