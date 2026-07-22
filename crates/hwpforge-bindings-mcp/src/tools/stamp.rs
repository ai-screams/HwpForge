//! `hwpforge_stamp_plan` / `hwpforge_stamp` — 산문 placeholder 를 이름 붙은
//! 누름틀로 승격하는 템플릿 스탬핑 (E6, 2단계 plan/apply).
//!
//! plan 은 클래스-A 후보를 나열하고, 호출자가 후보 전량을 이름 또는 ignore
//! 로 분류한 spec 배열을 stamp 에 전달한다. stamp 는 fail-closed admission
//! 게이트(무손실 왕복 + ZIP closed-world) 뒤에서 all-or-nothing 으로
//! 적용하고 manifest 를 함께 기록한다.

use serde::Serialize;

use hwpforge_smithy_hwpx::stamp::{
    CellStampCandidate, CellStampError, CellStampSpec, CellStampedField, HwpxStamper, SkippedTable,
    StampCandidate, StampError, StampRequestV2, StampSpec, StampedField, StamperError,
    STAMP_MAP_VERSION,
};

use crate::output::{read_file_bytes, write_output_file, ToolErrorInfo};

/// Output data from a successful stamp-plan operation.
#[derive(Debug, Serialize)]
pub struct StampPlanData {
    /// SHA-256 (hex) of the input — pass back verbatim as `source_sha256`
    /// when the stamp request carries cell specs.
    pub source_sha256: String,
    /// Discovered class-A text candidates (document order). Author one spec
    /// per candidate: unguarded candidates MUST be named or ignored.
    pub candidates: Vec<StampCandidate>,
    /// Discovered class-B cell candidates (label-adjacent empty cells).
    pub cells: Vec<CellStampCandidate>,
    /// Tables excluded from cell detection (invalid grid) — explicit
    /// incomplete-coverage diagnostics.
    pub skipped_tables: Vec<SkippedTable>,
}

/// Output data from a successful stamp operation.
#[derive(Debug, Serialize)]
pub struct StampData {
    /// Path to the stamped HWPX file.
    pub output_path: String,
    /// Path to the manifest JSON.
    pub manifest_path: String,
    /// Text fields created by this stamp (spec order).
    pub stamped: Vec<StampedField>,
    /// Cell fields created by this stamp (document order; empty for
    /// text-only legacy requests).
    pub stamped_cells: Vec<CellStampedField>,
    /// Number of explicitly ignored candidates (both classes).
    pub ignored: usize,
    /// Guarded candidates skipped because no spec approved them.
    pub skipped_guarded: usize,
    /// Size of the output file in bytes.
    pub size_bytes: u64,
}

/// Discover both candidate classes (text markers + label-adjacent cells).
pub fn run_stamp_plan(file_path: &str) -> Result<StampPlanData, ToolErrorInfo> {
    let bytes = read_file_bytes(file_path)?;
    let plan = HwpxStamper::plan_bytes_v2(&bytes).map_err(map_stamper_error)?;
    Ok(StampPlanData {
        source_sha256: plan.source_sha256,
        candidates: plan.text,
        cells: plan.cells,
        skipped_tables: plan.skipped_tables,
    })
}

/// Apply the approved spec set behind the admission gate.
///
/// Text-only requests without `source_sha256` run the legacy v1 path;
/// any cell spec (or an explicit `source_sha256`) selects the v2 path
/// with source-hash pinning and post-encode delta verification.
pub fn run_stamp(
    file_path: &str,
    specs: &[StampSpec],
    cells: &[CellStampSpec],
    source_sha256: Option<&str>,
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
    let serialize_err = |e: serde_json::Error| {
        ToolErrorInfo::new(
            "STAMP_MANIFEST_SERIALIZE",
            format!("manifest serialization failed: {e}"),
            "Report this as a bug.",
        )
    };
    let (out_bytes, manifest_json, stamped, stamped_cells, ignored, skipped_guarded) =
        if cells.is_empty() && source_sha256.is_none() {
            let result = HwpxStamper::stamp(&bytes, specs).map_err(map_stamper_error)?;
            let manifest_json =
                serde_json::to_string_pretty(&result.manifest).map_err(serialize_err)?;
            (
                result.bytes,
                manifest_json,
                result.outcome.stamped,
                Vec::new(),
                result.outcome.ignored,
                result.outcome.skipped_guarded.len(),
            )
        } else {
            let Some(sha) = source_sha256 else {
                return Err(ToolErrorInfo::new(
                    "MISSING_SOURCE_SHA256",
                    "cell specs require source_sha256 (drift pinning)",
                    "hwpforge_stamp_plan 의 source_sha256 을 그대로 전달하세요.",
                ));
            };
            let request = StampRequestV2 {
                schema_version: STAMP_MAP_VERSION,
                source_sha256: sha.to_string(),
                text: specs.to_vec(),
                cells: cells.to_vec(),
            };
            let result = HwpxStamper::stamp_v2(&bytes, &request).map_err(map_stamper_error)?;
            let manifest_json =
                serde_json::to_string_pretty(&result.manifest).map_err(serialize_err)?;
            (
                result.bytes,
                manifest_json,
                result.outcome.text.stamped,
                result.outcome.cells.stamped,
                result.outcome.text.ignored + result.outcome.cells.ignored,
                result.outcome.text.skipped_guarded.len()
                    + result.outcome.cells.skipped_guarded.len(),
            )
        };

    // Review L1: serialize the manifest BEFORE writing anything, and remove
    // the .hwpx if the manifest write fails — a failed call must leave no
    // partial artifact behind (fail-closed).
    let manifest_file = manifest_path
        .map(str::to_string)
        .unwrap_or_else(|| format!("{}.manifest.json", output_path.trim_end_matches(".hwpx")));
    // R2: identical paths would silently overwrite the stamped .hwpx with
    // the manifest JSON and still report success.
    if std::path::Path::new(output_path) == std::path::Path::new(&manifest_file) {
        return Err(ToolErrorInfo::new(
            "MANIFEST_PATH_CONFLICT",
            format!("manifest path equals output path: {output_path}"),
            "manifest_path 는 output_path 와 달라야 합니다.",
        ));
    }
    write_output_file(output_path, &out_bytes)?;
    if let Err(e) = write_output_file(&manifest_file, manifest_json.as_bytes()) {
        let _ = std::fs::remove_file(output_path);
        return Err(e);
    }

    let size_bytes = out_bytes.len() as u64;
    Ok(StampData {
        output_path: output_path.to_string(),
        manifest_path: manifest_file,
        stamped,
        stamped_cells,
        ignored,
        skipped_guarded,
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
        // R1: entry names are untrusted — {:?} escapes control chars even
        // if a client prints the parsed JSON string raw.
        StamperError::UncarriedZipEntries { entries } => ToolErrorInfo::new(
            "INPUT_ENTRIES_NOT_CARRIED",
            format!("encoder does not carry input entries: {entries:?}"),
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
        StamperError::SourceHashMismatch { expected, actual } => ToolErrorInfo::new(
            "STAMP_SOURCE_HASH_MISMATCH",
            format!("request is pinned to {expected}, input is {actual}"),
            "문서가 변경됐습니다 — hwpforge_stamp_plan 을 다시 실행해 source_sha256 을 갱신하세요.",
        ),
        StamperError::CellStamp(inner) => map_cell_stamp_error(inner),
        StamperError::DeltaMismatch { stage, detail } => ToolErrorInfo::new(
            "STAMP_DELTA_MISMATCH",
            format!("post-encode verification failed at {stage}: {detail}"),
            "산출물 검증 실패 — 코덱 버그 가능성이 있어 무출력으로 거부했습니다.",
        ),
        other => ToolErrorInfo::new("STAMP_FAILED", other.to_string(), "Unexpected failure."),
    }
}

fn map_cell_stamp_error(error: CellStampError) -> ToolErrorInfo {
    match error {
        CellStampError::TableNotFound { table } => ToolErrorInfo::new(
            "TABLE_NOT_FOUND",
            format!("table ordinal {table} does not exist"),
            "hwpforge_stamp_plan 의 cells[].table 서수를 사용하세요.",
        ),
        CellStampError::TableGridInvalid { table, detail } => ToolErrorInfo::new(
            "TABLE_GRID_INVALID",
            format!("table {table}: {detail}"),
            "이 표는 논리 격자를 만들 수 없어 셀 스탬핑 대상이 아닙니다.",
        ),
        CellStampError::NotAnAnchor { table, requested, anchor } => ToolErrorInfo::new(
            "STAMP_CELL_NOT_ANCHOR",
            format!("table {table}: ({},{}) is not an anchor", requested.row, requested.col),
            match anchor {
                Some(a) => {
                    format!("병합 피복 위치입니다 — anchor ({},{}) 를 지정하세요.", a.row, a.col)
                }
                None => "격자 범위 밖 좌표입니다.".to_string(),
            },
        ),
        CellStampError::TargetNotStampable { table, at } => ToolErrorInfo::new(
            "STAMP_CELL_NOT_EMPTY",
            format!("table {table}: cell ({},{}) has authored content", at.row, at.col),
            "클래스-B 대상은 whitespace-only 빈 셀이어야 합니다.",
        ),
        CellStampError::LabelDrift { table, at, claimed, found } => ToolErrorInfo::new(
            "STAMP_LABEL_DRIFT",
            format!(
                "table {table} ({},{}): claimed label {claimed:?}, live {found:?}",
                at.row, at.col
            ),
            "문서가 변경됐습니다 — hwpforge_stamp_plan 을 다시 실행하세요.",
        ),
        CellStampError::UnknownCandidate { table, at } => ToolErrorInfo::new(
            "STAMP_CELL_NOT_CANDIDATE",
            format!("table {table}: ({},{}) is not a live candidate", at.row, at.col),
            "ignore 는 live 후보에만 가능합니다.",
        ),
        CellStampError::DuplicateTarget { table, at } => ToolErrorInfo::new(
            "STAMP_CELL_TARGET_DUPLICATE",
            format!("table {table}: cell ({},{}) targeted twice", at.row, at.col),
            "같은 셀을 두 번 분류했습니다.",
        ),
        CellStampError::EmptyName => ToolErrorInfo::new(
            "STAMP_NAME_EMPTY",
            "field name must not be empty",
            "빈 이름은 허용되지 않습니다.",
        ),
        CellStampError::BlankHint { name } => ToolErrorInfo::new(
            "STAMP_HINT_BLANK",
            format!("cell spec {name:?}: hint must not be blank"),
            "빈 셀엔 마커가 없어 hint 가 필수입니다 — plan 의 suggested_hint 를 참고하세요.",
        ),
        CellStampError::DuplicateName { name } => ToolErrorInfo::new(
            "STAMP_NAME_DUPLICATE",
            format!("duplicate field name {name:?}"),
            "필드 이름은 text+cells 전체에서 유일해야 합니다.",
        ),
        CellStampError::NameCollision { name } => ToolErrorInfo::new(
            "STAMP_NAME_COLLISION",
            format!("field name {name:?} already exists in the document"),
            "기존 누름틀과 이름이 겹칩니다 — hwpforge_fields 로 확인하세요.",
        ),
        CellStampError::UncoveredCandidate { table, at } => ToolErrorInfo::new(
            "STAMP_CANDIDATE_UNCOVERED",
            format!(
                "unguarded cell candidate at table {table} ({},{}) has no spec",
                at.row, at.col
            ),
            "모든 무가드 셀 후보는 이름 또는 ignore 로 분류해야 합니다.",
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

    /// 성명/주소 2×2 라벨 서식 (set_cell 테스트와 동일 형태).
    fn label_form_hwpx(dir: &std::path::Path) -> String {
        use hwpforge_core::page::PageSettings;
        use hwpforge_core::run::Run;
        use hwpforge_core::table::{Table, TableCell, TableRow};
        use hwpforge_core::{Document, Paragraph, Section};
        use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
        use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
        use hwpforge_smithy_hwpx::HwpxEncoder;

        let text_para = |t: &str| {
            Paragraph::with_runs(vec![Run::text(t, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
        };
        let cell = |t: &str| TableCell::new(vec![text_para(t)], HwpUnit::new(8000).unwrap());
        let table = Table::new(vec![
            TableRow::new(vec![cell("성명"), cell("")]),
            TableRow::new(vec![cell("주소"), cell("")]),
        ]);
        let mut host = Paragraph::new(ParaShapeIndex::new(0));
        host.add_run(Run::table(table, CharShapeIndex::new(0)));
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(vec![host], PageSettings::default()));

        let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
        styles.push_char_shape(HwpxCharShape::default());
        styles.push_para_shape(HwpxParaShape::default());
        let bytes = HwpxEncoder::encode(
            &doc.validate().unwrap(),
            &styles,
            &hwpforge_core::image::ImageStore::new(),
        )
        .unwrap();
        let path = dir.join("label-form.hwpx");
        std::fs::write(&path, bytes).unwrap();
        path.to_str().unwrap().to_string()
    }

    #[test]
    fn stamp_v2_cells_via_mcp_surface() {
        use hwpforge_core::table::grid::GridCoord;
        use hwpforge_smithy_hwpx::stamp::{CellLabelClaim, CellStampAction};

        let dir = tempfile::tempdir().unwrap();
        let src = label_form_hwpx(dir.path());
        let plan = run_stamp_plan(&src).unwrap();
        assert_eq!(plan.cells.len(), 2, "{:?}", plan.cells);
        assert!(plan.skipped_tables.is_empty());

        let cell_specs = vec![
            CellStampSpec {
                table: 0,
                at: GridCoord::new(0, 1),
                label: Some(CellLabelClaim { at: GridCoord::new(0, 0), text: "성명".into() }),
                action: CellStampAction::Field {
                    name: "성명".into(), hint: "성명 입력".into()
                },
            },
            CellStampSpec {
                table: 0,
                at: GridCoord::new(1, 1),
                label: None,
                action: CellStampAction::Ignore,
            },
        ];

        // cells 만 있고 source_sha256 이 없으면 거부 (드리프트 핀 필수).
        let out = dir.path().join("cells.hwpx");
        let err = run_stamp(&src, &[], &cell_specs, None, out.to_str().unwrap(), None).unwrap_err();
        assert_eq!(err.code, "MISSING_SOURCE_SHA256");

        let data = run_stamp(
            &src,
            &[],
            &cell_specs,
            Some(&plan.source_sha256),
            out.to_str().unwrap(),
            None,
        )
        .unwrap();
        assert_eq!(data.stamped_cells.len(), 1);
        assert_eq!(data.stamped_cells[0].name, "성명");
        assert_eq!(data.ignored, 1);

        // 산출물은 즉시 fields 로 발견 가능.
        let fields = crate::tools::fields::run_fields(out.to_str().unwrap()).unwrap();
        assert_eq!(fields.fields.len(), 1);
        assert_eq!(fields.fields[0].name.as_deref(), Some("성명"));

        // 틀린 sha 는 STAMP_SOURCE_HASH_MISMATCH.
        let err =
            run_stamp(&src, &[], &cell_specs, Some(&"0".repeat(64)), out.to_str().unwrap(), None)
                .unwrap_err();
        assert_eq!(err.code, "STAMP_SOURCE_HASH_MISMATCH");
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
        let data = run_stamp(&src, &specs, &[], None, out.to_str().unwrap(), None).unwrap();
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
        let err = run_stamp(&src, &[], &[], None, "out.zip", None).unwrap_err();
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
        let err = run_stamp(&src, &[], &[], None, out_s, None).unwrap_err();
        assert_eq!(err.code, "STAMP_CANDIDATE_UNCOVERED");

        // stale spec (span 어긋남)
        let mut stale = named(c1, "성명");
        stale.span = 0..1;
        let err = run_stamp(&src, &[stale, named(c2, "소속")], &[], None, out_s, None).unwrap_err();
        assert_eq!(err.code, "STAMP_SPEC_STALE");

        // 마커 불일치
        let mut wrong = named(c1, "성명");
        wrong.marker = "(x)".to_string();
        let err = run_stamp(&src, &[wrong, named(c2, "소속")], &[], None, out_s, None).unwrap_err();
        assert_eq!(err.code, "STAMP_MARKER_MISMATCH");

        // 같은 후보 이중 분류
        let err = run_stamp(
            &src,
            &[named(c1, "a"), named(c1, "b"), named(c2, "소속")],
            &[],
            None,
            out_s,
            None,
        )
        .unwrap_err();
        assert_eq!(err.code, "STAMP_SPEC_DUPLICATE");

        // 이름 중복
        let err = run_stamp(&src, &[named(c1, "같음"), named(c2, "같음")], &[], None, out_s, None)
            .unwrap_err();
        assert_eq!(err.code, "STAMP_NAME_DUPLICATE");

        // 빈 이름
        let err = run_stamp(&src, &[named(c1, ""), named(c2, "소속")], &[], None, out_s, None)
            .unwrap_err();
        assert_eq!(err.code, "STAMP_NAME_EMPTY");

        assert!(!out.exists(), "fail-closed: 거부 시 산출물이 없어야 한다");
    }

    #[test]
    fn stamp_manifest_write_failure_removes_output() {
        // Review L1: manifest 기록 실패 시 .hwpx 산출물도 제거되어야 한다
        // (fail-closed — 부분 산출물 금지).
        let dir = tempfile::tempdir().unwrap();
        let src = make_template(&dir);
        let plan = run_stamp_plan(&src).unwrap();
        let unguarded: Vec<_> = plan.candidates.iter().filter(|c| c.guard.is_none()).collect();
        let specs = vec![named(unguarded[0], "성명"), named(unguarded[1], "소속")];
        let out = dir.path().join("orphan.hwpx");
        let err = run_stamp(
            &src,
            &specs,
            &[],
            None,
            out.to_str().unwrap(),
            Some("/nonexistent-dir/never.manifest.json"),
        )
        .unwrap_err();
        assert_eq!(err.code, "WRITE_ERROR");
        assert!(!out.exists(), "manifest 실패 시 산출물이 제거되어야 한다");
    }
}
