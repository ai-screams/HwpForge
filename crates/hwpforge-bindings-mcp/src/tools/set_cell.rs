//! `hwpforge_set_cell` — 논리 격자 주소 기반 표 셀 편집 (E3).
//!
//! 셀은 표 서수(to-json export 순서) + 좌표(`at`, 피병합 좌표는 앵커로
//! resolve) 또는 라벨 상대(`right_of`/`below`, 정규화 exact match)로
//! 지정한다. stamp 와 동일한 fail-closed admission 게이트 뒤에서
//! all-or-nothing 으로 적용된다. 빈 문자열은 정당한 clear.

use serde::Serialize;

use hwpforge::ops;
use hwpforge_smithy_hwpx::{CellSpec, SetCellResult};

use crate::compat::{self, Tool};
use crate::output::{read_file_bytes, write_output_file, ToolErrorInfo, ToolWarningInfo};

/// Output data from a successful set-cell operation.
#[derive(Debug, Serialize)]
pub struct SetCellData {
    /// Path to the edited HWPX file.
    pub output_path: String,
    /// Applied edits (spec order) with requested/anchor/resolution.
    pub cells: Vec<SetCellResult>,
    /// Size of the output file in bytes.
    pub size_bytes: u64,
    /// What the admission decode reported, then the successful encode's
    /// non-semantic warnings (`ops::SetCellOutput::warnings`). Omitted when
    /// empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<ToolWarningInfo>,
}

/// Apply a batch of cell edits behind the admission gate.
pub fn run_set_cell(
    file_path: &str,
    specs: &[CellSpec],
    output_path: &str,
) -> Result<SetCellData, ToolErrorInfo> {
    if !output_path.ends_with(".hwpx") {
        return Err(ToolErrorInfo::new(
            "INVALID_EXTENSION",
            format!("Output path must end with .hwpx: {output_path}"),
            "Use a .hwpx extension for the output file.",
        ));
    }
    // Kept local (not delegated to `ops::set_cell`'s own empty-batch check)
    // so a missing input file plus an empty batch still reports
    // INVALID_SET_CELL_MAP rather than FILE_NOT_FOUND — the pre-migration
    // check order.
    if specs.is_empty() {
        return Err(ToolErrorInfo::new(
            "INVALID_SET_CELL_MAP",
            "specs is empty",
            "Pass at least one CellSpec: {table, at|right_of|below, text}.",
        ));
    }

    let bytes = read_file_bytes(file_path)?;
    let opts = ops::SetCellOptions::default().with_specs(specs.to_vec());
    let outcome = ops::set_cell(&bytes, &opts).map_err(|e| compat::tool_error(Tool::SetCell, e))?;

    write_output_file(output_path, &outcome.bytes)?;

    let warnings: Vec<ToolWarningInfo> = outcome.warnings.iter().map(compat::warning).collect();
    Ok(SetCellData {
        output_path: output_path.to_string(),
        cells: outcome.results,
        size_bytes: outcome.bytes.len() as u64,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_core::table::grid::GridCoord;
    use hwpforge_smithy_hwpx::CellTarget;

    fn spec(table: usize, target: CellTarget, text: &str) -> CellSpec {
        CellSpec { table, target, text: text.to_string() }
    }

    /// 성명/주소 2×2 라벨 서식을 hwpx 로 만들어 경로를 돌려준다.
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
        let path = dir.join("label_form.hwpx");
        std::fs::write(&path, bytes).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("hwpforge_mcp_set_cell_{}", std::process::id()))
            .join(name);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn set_cell_edits_by_label_and_reports_resolution() {
        let dir = temp_dir("edit");
        let input = label_form_hwpx(&dir);
        let output = dir.join("edited.hwpx").to_string_lossy().into_owned();

        let data = run_set_cell(
            &input,
            &[
                spec(0, CellTarget::RightOf("성명".into()), "홍길동"),
                spec(0, CellTarget::At(GridCoord::new(1, 1)), "서울"),
            ],
            &output,
        )
        .unwrap();
        assert_eq!(data.cells.len(), 2);
        assert!(std::path::Path::new(&output).exists());
        assert!(data.warnings.is_empty(), "a clean input must not warn: {:?}", data.warnings);

        // 편집 결과가 fill/fields 계열과 동일한 디코드 표면으로 확인 가능.
        let decoded =
            hwpforge_smithy_hwpx::HwpxDecoder::decode(&std::fs::read(&output).unwrap()).unwrap();
        let table = decoded.document.sections()[0].paragraphs[0]
            .runs
            .iter()
            .find_map(|r| r.content.as_table())
            .unwrap();
        assert_eq!(table.rows[0].cells[1].paragraphs[0].text_content(), "홍길동");
        assert_eq!(table.rows[1].cells[1].paragraphs[0].text_content(), "서울");
    }

    fn fixture(rel: &str) -> String {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(rel)
            .to_str()
            .unwrap()
            .to_string()
    }

    /// 줄 조판 캐시가 낡은 fixture 를 편집하면, 편집이 규정한 admission 디코드
    /// 경고(`LAYOUT_CACHE_DROPPED`)가 `warnings` 에 실려야 한다.
    #[test]
    fn set_cell_surfaces_decode_warnings() {
        let path = fixture("layout/stale-line-cache.hwpx");
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out.hwpx").to_string_lossy().into_owned();

        let data = run_set_cell(&path, &[spec(0, CellTarget::At(GridCoord::new(0, 0)), "x")], &out)
            .unwrap();
        assert!(
            data.warnings.iter().any(|w| w.code == "LAYOUT_CACHE_DROPPED"),
            "set_cell must surface the admission decode warning: {:?}",
            data.warnings
        );
    }

    #[test]
    fn set_cell_rejects_bad_targets_with_codes() {
        let dir = temp_dir("reject");
        let input = label_form_hwpx(&dir);
        let output = dir.join("never.hwpx").to_string_lossy().into_owned();

        let err =
            run_set_cell(&input, &[spec(9, CellTarget::At(GridCoord::new(0, 0)), "x")], &output)
                .unwrap_err();
        assert_eq!(err.code, "TABLE_NOT_FOUND");

        let err =
            run_set_cell(&input, &[spec(0, CellTarget::RightOf("연락처".into()), "x")], &output)
                .unwrap_err();
        assert_eq!(err.code, "CELL_NOT_FOUND");

        let err = run_set_cell(&input, &[], &output).unwrap_err();
        assert_eq!(err.code, "INVALID_SET_CELL_MAP");
        assert!(!std::path::Path::new(&output).exists(), "no output on rejection");
    }
}
