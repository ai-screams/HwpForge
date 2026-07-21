//! `hwpforge_set_cell` — 논리 격자 주소 기반 표 셀 편집 (E3).
//!
//! 셀은 표 서수(to-json export 순서) + 좌표(`at`, 피병합 좌표는 앵커로
//! resolve) 또는 라벨 상대(`right_of`/`below`, 정규화 exact match)로
//! 지정한다. stamp 와 동일한 fail-closed admission 게이트 뒤에서
//! all-or-nothing 으로 적용된다. 빈 문자열은 정당한 clear.

use serde::Serialize;

use hwpforge_smithy_hwpx::{CellEditError, CellSpec, HwpxCellEditor, SetCellResult};

use crate::output::{read_file_bytes, write_output_file, ToolErrorInfo};

/// Output data from a successful set-cell operation.
#[derive(Debug, Serialize)]
pub struct SetCellData {
    /// Path to the edited HWPX file.
    pub output_path: String,
    /// Applied edits (spec order) with requested/anchor/resolution.
    pub cells: Vec<SetCellResult>,
    /// Size of the output file in bytes.
    pub size_bytes: u64,
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
    if specs.is_empty() {
        return Err(ToolErrorInfo::new(
            "INVALID_SET_CELL_MAP",
            "specs is empty",
            "Pass at least one CellSpec: {table, at|right_of|below, text}.",
        ));
    }

    let bytes = read_file_bytes(file_path)?;
    let result = HwpxCellEditor::set_cells(&bytes, specs).map_err(map_cell_edit_error)?;

    write_output_file(output_path, &result.bytes)?;

    Ok(SetCellData {
        output_path: output_path.to_string(),
        cells: result.outcome.cells,
        size_bytes: result.bytes.len() as u64,
    })
}

fn map_cell_edit_error(error: CellEditError) -> ToolErrorInfo {
    let (code, hint): (&str, &str) = match &error {
        CellEditError::TableNotFound { .. } => {
            ("TABLE_NOT_FOUND", "표 서수는 hwpforge_to_json export 의 문서 순서 0-base 입니다.")
        }
        CellEditError::TableGridInvalid { .. } => (
            "TABLE_GRID_INVALID",
            "이 표는 셀 span 이 well-formed 격자를 이루지 않아 주소 지정이 불가합니다.",
        ),
        CellEditError::CellNotFound { .. } => {
            ("CELL_NOT_FOUND", "좌표는 병합 전 논리 격자 0-base — export 의 addr 값을 쓰세요.")
        }
        CellEditError::LabelAmbiguous { .. } => {
            ("CELL_LABEL_AMBIGUOUS", "라벨이 여러 셀과 일치합니다 — at 좌표로 직접 지정하세요.")
        }
        CellEditError::NonTextContent { .. } => (
            "CELL_HAS_NON_TEXT_CONTENT",
            "표/이미지/컨트롤이 든 셀은 파괴 방지를 위해 교체를 거부합니다.",
        ),
        CellEditError::TargetDuplicate { .. } => {
            ("CELL_TARGET_DUPLICATE", "두 편집이 같은 앵커 셀로 resolve 됐습니다.")
        }
        CellEditError::TargetConflict { .. } => {
            ("CELL_TARGET_CONFLICT", "바깥 셀 교체가 다른 편집이 노리는 중첩 표를 파괴합니다.")
        }
        CellEditError::NotRoundTripSafe { .. } => (
            "INPUT_NOT_ROUNDTRIP_SAFE",
            "이 입력은 무손실 재인코드가 증명되지 않아 편집을 거부합니다 (fail-closed).",
        ),
        CellEditError::UncarriedZipEntries { .. } => (
            "INPUT_ENTRIES_NOT_CARRIED",
            "인코더가 carry 하지 않는 ZIP entry 가 있어 편집을 거부합니다 (fail-closed).",
        ),
        CellEditError::Codec(_) => ("SET_CELL_CODEC_FAILED", "Report this as a bug."),
        _ => ("SET_CELL_FAILED", "Report this as a bug."),
    };
    ToolErrorInfo::new(code, error.to_string(), hint)
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
