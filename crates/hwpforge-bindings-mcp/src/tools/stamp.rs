//! `hwpforge_stamp_plan` / `hwpforge_stamp` — 산문 placeholder 를 이름 붙은
//! 누름틀로 승격하는 템플릿 스탬핑 (E6, 2단계 plan/apply).
//!
//! plan 은 클래스-A 후보를 나열하고, 호출자가 후보 전량을 이름 또는 ignore
//! 로 분류한 spec 배열을 stamp 에 전달한다. stamp 는 fail-closed admission
//! 게이트(무손실 왕복 + ZIP closed-world) 뒤에서 all-or-nothing 으로
//! 적용하고 manifest 를 함께 기록한다.
//!
//! `stamp_plan` 과 `stamp` 모두 [`hwpforge::ops`] 로 온전히 이관됐다
//! (`ops::stamp_plan`, `ops::stamp`) — apply-phase 결과
//! (`stamped`/`stamped_cells`/`ignored`/`skipped_guarded`) 가 manifest 와
//! 별개로 `ops::StampOutput` 에 실린 이후.

use serde::Serialize;

use hwpforge::ops;
use hwpforge_smithy_hwpx::stamp::{
    CellStampCandidate, CellStampSpec, CellStampedField, SkippedTable, StampCandidate, StampMap,
    StampRequestV2, StampSpec, StampedField, STAMP_MAP_VERSION,
};

use crate::compat::{self, Tool};
use crate::output::{read_file_bytes, write_output_file, ToolErrorInfo, ToolWarningInfo};

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
    /// Decoder warnings for this document (`ops::StampPlanOutput::warnings`).
    /// Omitted when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<ToolWarningInfo>,
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
    /// What decoding the input reported, then the successful encode's
    /// non-semantic warnings (`ops::StampOutput::warnings`). Omitted when
    /// empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<ToolWarningInfo>,
}

/// Discover both candidate classes (text markers + label-adjacent cells).
pub fn run_stamp_plan(file_path: &str) -> Result<StampPlanData, ToolErrorInfo> {
    let bytes = read_file_bytes(file_path)?;
    let out = ops::stamp_plan(&bytes).map_err(|e| compat::tool_error(Tool::StampPlan, e))?;
    let warnings: Vec<ToolWarningInfo> = out.warnings.iter().map(compat::warning).collect();
    Ok(StampPlanData {
        source_sha256: out.plan.source_sha256,
        candidates: out.plan.text,
        cells: out.plan.cells,
        skipped_tables: out.plan.skipped_tables,
        warnings,
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

    let request = if cells.is_empty() && source_sha256.is_none() {
        StampMap::Legacy(specs.to_vec())
    } else {
        let Some(sha) = source_sha256 else {
            return Err(ToolErrorInfo::new(
                "MISSING_SOURCE_SHA256",
                "cell specs require source_sha256 (drift pinning)",
                "hwpforge_stamp_plan 의 source_sha256 을 그대로 전달하세요.",
            ));
        };
        StampMap::V2(StampRequestV2 {
            schema_version: STAMP_MAP_VERSION,
            source_sha256: sha.to_string(),
            text: specs.to_vec(),
            cells: cells.to_vec(),
        })
    };

    let out = ops::stamp(&bytes, &request, &ops::StampOptions::default())
        .map_err(|e| compat::tool_error(Tool::Stamp, e))?;

    // `StampOptions::default()` always asks for the manifest, so this is
    // never `None` in practice; handled as an error rather than a panic
    // because it crosses an API boundary this tool does not own.
    let manifest = out.manifest.as_ref().ok_or_else(|| {
        ToolErrorInfo::new(
            "STAMP_MANIFEST_SERIALIZE",
            "manifest missing from a default-options stamp result",
            "Report this as a bug.",
        )
    })?;
    let manifest_json = serde_json::to_string_pretty(manifest).map_err(|e| {
        ToolErrorInfo::new(
            "STAMP_MANIFEST_SERIALIZE",
            format!("manifest serialization failed: {e}"),
            "Report this as a bug.",
        )
    })?;

    // Review L1: serialize the manifest BEFORE writing anything, and remove
    // the .hwpx if the manifest write fails — a failed call must leave no
    // partial artifact behind (fail-closed).
    //
    // W6b audit follow-up: the default path is now `ops::default_manifest_path`
    // — the CLI's own pre-migration rule, shared here instead of this file's
    // separate `trim_end_matches(".hwpx")` version (see that function's doc
    // for the one input the two disagreed on and why this migrates rather
    // than compat-maps it).
    let manifest_file = manifest_path.map(str::to_string).unwrap_or_else(|| {
        ops::default_manifest_path(std::path::Path::new(output_path)).to_string_lossy().into_owned()
    });
    // R2: identical paths would silently overwrite the stamped .hwpx with
    // the manifest JSON and still report success.
    if std::path::Path::new(output_path) == std::path::Path::new(&manifest_file) {
        return Err(ToolErrorInfo::new(
            "MANIFEST_PATH_CONFLICT",
            format!("manifest path equals output path: {output_path}"),
            "manifest_path 는 output_path 와 달라야 합니다.",
        ));
    }
    write_output_file(output_path, &out.bytes)?;
    if let Err(e) = write_output_file(&manifest_file, manifest_json.as_bytes()) {
        let _ = std::fs::remove_file(output_path);
        return Err(e);
    }

    let size_bytes = out.bytes.len() as u64;
    let warnings: Vec<ToolWarningInfo> = out.warnings.iter().map(compat::warning).collect();
    Ok(StampData {
        output_path: output_path.to_string(),
        manifest_path: manifest_file,
        stamped: out.stamped,
        stamped_cells: out.stamped_cells,
        ignored: out.ignored,
        skipped_guarded: out.skipped_guarded,
        size_bytes,
        warnings,
    })
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
        assert!(data.warnings.is_empty(), "a clean input must not warn: {:?}", data.warnings);

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
        assert!(data.warnings.is_empty(), "a clean template must not warn: {:?}", data.warnings);
    }

    fn fixture(rel: &str) -> String {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(rel)
            .to_str()
            .unwrap()
            .to_string()
    }

    /// 줄 조판 캐시가 낡은 fixture 를 plan 하면, 결과가 순수 디코드/투영이므로
    /// 그 디코드 경고(`LAYOUT_CACHE_DROPPED`)가 `warnings` 에 실려야 한다.
    #[test]
    fn stamp_plan_surfaces_decode_warnings() {
        let path = fixture("layout/stale-line-cache.hwpx");
        let data = run_stamp_plan(&path).unwrap();
        assert!(
            data.warnings.iter().any(|w| w.code == "LAYOUT_CACHE_DROPPED"),
            "stamp_plan must surface the decode warning: {:?}",
            data.warnings
        );

        let value = serde_json::to_value(&data).unwrap();
        let warning = value["warnings"]
            .as_array()
            .expect("warnings array")
            .iter()
            .find(|w| w["code"] == "LAYOUT_CACHE_DROPPED")
            .expect("LAYOUT_CACHE_DROPPED present in the serialized value");
        assert!(!warning["message"].as_str().unwrap_or_default().is_empty());
    }

    /// `stamp_plan_surfaces_decode_warnings` only exercises plan (pure
    /// decode/projection); this exercises apply — the admission-gated,
    /// re-encoding half — on the same fixture, so the decode warning must
    /// still reach the caller once a real edit and encode have happened in
    /// between. `LAYOUT_CACHE_DROPPED` is not a semantic-loss warning
    /// (`EncodeWarning::is_semantic_loss`), so it must not fail admission
    /// closed the way `NoteHeadSkipped` etc. would.
    #[test]
    fn stamp_apply_on_a_stale_fixture_surfaces_decode_warnings() {
        let path = fixture("layout/stale-line-cache.hwpx");
        let plan = run_stamp_plan(&path).unwrap();
        assert_eq!(plan.candidates.len(), 1, "{:?}", plan.candidates);
        let specs = vec![named(&plan.candidates[0], "성명")];

        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("stamped.hwpx");
        let data = run_stamp(&path, &specs, &[], None, out.to_str().unwrap(), None).unwrap();

        assert_eq!(data.stamped.len(), 1);
        assert!(
            data.warnings.iter().any(|w| w.code == "LAYOUT_CACHE_DROPPED"),
            "stamp apply must surface the decode warning too, not just stamp_plan: {:?}",
            data.warnings
        );

        let value = serde_json::to_value(&data).unwrap();
        assert_eq!(value["warnings"][0]["code"], "LAYOUT_CACHE_DROPPED");
        assert!(!value["warnings"][0]["message"].as_str().unwrap_or_default().is_empty());
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
        assert!(data.warnings.is_empty(), "a clean template must not warn: {:?}", data.warnings);

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
