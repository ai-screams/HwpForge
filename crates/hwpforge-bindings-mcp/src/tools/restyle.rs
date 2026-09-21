//! `hwpforge_restyle` — Apply a different style preset to an existing HWPX document.

use serde::Serialize;

use hwpforge::ops::{restyle as ops_restyle, OpsError, RestyleOptions};
use hwpforge_smithy_hwpx::presets::builtin_presets;

use crate::compat::{self, Tool};
use crate::output::{read_file_bytes, write_output_file, ToolErrorInfo};

/// Output data from a successful restyle operation.
#[derive(Debug, Serialize)]
pub struct RestyleData {
    /// Path to the generated HWPX file.
    pub output_path: String,
    /// Applied preset name.
    pub applied_preset: String,
    /// Size of the output file in bytes.
    pub size_bytes: u64,
    /// Number of sections.
    pub sections: usize,
    /// 인코드 경고 (각주 번호 머리 생략 등 — 무음 폐기 금지).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Apply a style preset to an existing HWPX document.
pub fn run_restyle(
    file_path: &str,
    preset: &str,
    output_path: &str,
) -> Result<RestyleData, ToolErrorInfo> {
    // 1. Validate output extension (MCP-local — stays outside `ops`).
    if !output_path.ends_with(".hwpx") {
        return Err(ToolErrorInfo::new(
            "INVALID_EXTENSION",
            format!("Output path must end with .hwpx: {output_path}"),
            "Use a .hwpx extension for the output file.",
        ));
    }

    // 2. Preset existence, checked before touching the filesystem: this
    //    preserves the legacy ordering (`restyle_unknown_preset` must not
    //    depend on `file_path` existing). `ops::restyle` checks the same
    //    thing again, authoritatively, once it has bytes to work with.
    if !builtin_presets().iter().any(|p| p.name == preset) {
        return Err(compat::tool_error(
            Tool::Restyle,
            OpsError::PresetNotFound { name: preset.to_string() },
        ));
    }

    // 3. Read source HWPX bytes.
    let bytes = read_file_bytes(file_path)?;

    // 4. Delegate decode → font swap → validate → encode → semantic-loss
    //    fail-closed to `ops::restyle`.
    let out = ops_restyle(&bytes, &RestyleOptions::default().with_preset(preset))
        .map_err(|e| compat::tool_error(Tool::Restyle, e))?;

    // 비-의미 경고(줄 조판 캐시 드롭 등)는 지금까지처럼 결과에 실어 보낸다.
    // (의미 손상 경고는 `ops::restyle` 이 이미 fail-closed 로 거부했다.)
    let warnings: Vec<String> = out.warnings.iter().map(|w| compat::warning(w).message).collect();

    // 5. Write output.
    write_output_file(output_path, &out.bytes)?;

    let size_bytes = out.bytes.len() as u64;

    Ok(RestyleData {
        output_path: output_path.to_string(),
        applied_preset: preset.to_string(),
        size_bytes,
        sections: out.sections,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restyle_invalid_extension() {
        let err = run_restyle("/tmp/doc.hwpx", "default", "/tmp/out.txt").unwrap_err();
        assert_eq!(err.code, "INVALID_EXTENSION");
    }

    #[test]
    fn restyle_unknown_preset() {
        let err = run_restyle("/tmp/doc.hwpx", "unknown", "/tmp/out.hwpx").unwrap_err();
        assert_eq!(err.code, "PRESET_NOT_FOUND");
    }

    #[test]
    fn restyle_missing_file() {
        let err = run_restyle("/nonexistent/file.hwpx", "modern", "/tmp/out.hwpx").unwrap_err();
        assert_eq!(err.code, "FILE_NOT_FOUND");
    }

    #[test]
    fn restyle_happy_path() {
        // 1. Create a valid HWPX via convert (default preset = 함초롬돋움)
        let dir = tempfile::tempdir().unwrap();
        let hwpx_path = dir.path().join("source.hwpx");
        crate::tools::convert::run_convert(
            "# Test\n\nSome content.",
            false,
            hwpx_path.to_str().unwrap(),
            "default",
        )
        .unwrap();

        // 2. Restyle with "modern" preset (맑은 고딕)
        let out_path = dir.path().join("restyled.hwpx");
        let data =
            run_restyle(hwpx_path.to_str().unwrap(), "modern", out_path.to_str().unwrap()).unwrap();

        assert!(out_path.exists());
        assert_eq!(data.applied_preset, "modern");
        assert!(data.size_bytes > 0);
        assert!(data.sections >= 1);
    }

    /// titleMark 각주 문서는 재인코드에서 번호 머리가 생략된다
    /// (`NoteHeadSkipped`). restyle 도 preserve-first 재인코드 경로이므로
    /// **파일을 쓰지 않고 거부**해야 한다 (R1 F2 — 과거엔 경고만 싣고 썼다).
    ///
    /// fixture 는 `hwpforge-smithy-hwpx/tests/note_numbering_roundtrip.rs`
    /// 의 `stamper_fails_closed_on_note_head_skip` 과 같은 구성이다:
    /// 첫 문단이 heading 인 각주 본문 → 안전한 번호 머리 삽입 지점 없음.
    #[test]
    fn restyle_fails_closed_on_note_head_skip() {
        use hwpforge_core::control::Control;
        use hwpforge_core::image::ImageStore;
        use hwpforge_core::page::PageSettings;
        use hwpforge_core::run::Run;
        use hwpforge_core::section::Section;
        use hwpforge_core::{Document, Paragraph};
        use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
        use hwpforge_smithy_hwpx::style_store::{
            HwpxCharShape, HwpxFont, HwpxParaShape, HwpxStyleStore,
        };
        use hwpforge_smithy_hwpx::HwpxEncoder;

        let mut store = HwpxStyleStore::new();
        for &lang in &["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"] {
            store.push_font(HwpxFont::new(0, "함초롬돋움", lang));
        }
        store.push_char_shape(HwpxCharShape::default());
        store.push_para_shape(HwpxParaShape::default());

        let mut heading_body = Paragraph::with_runs(
            vec![Run::text("제목 각주", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        heading_body.heading_level = Some(1);
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(Control::footnote(vec![heading_body]), CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        ));
        let validated = doc.validate().expect("validate");
        let base = HwpxEncoder::encode(&validated, &store, &ImageStore::new()).expect("encode");

        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("titlemark-note.hwpx");
        std::fs::write(&src, &base).unwrap();
        let out = dir.path().join("restyled.hwpx");

        let err = run_restyle(src.to_str().unwrap(), "modern", out.to_str().unwrap())
            .expect_err("의미 손상(번호 머리 생략)을 무음 통과시키면 안 된다");

        assert_eq!(err.code, "ENCODE_SEMANTIC_LOSS", "{err:?}");
        assert!(
            err.message.contains("note number head skipped"),
            "메시지에 경고 Display 가 실려야 한다: {}",
            err.message
        );
        assert!(!out.exists(), "fail-closed 인데 산출 파일이 생성됐다");
    }

    #[test]
    fn restyle_preserves_all_shape_indices_with_complex_doc() {
        // Regression test: documents with code blocks reference higher char/para
        // shape indices (7+/20+). The old implementation created a preset store
        // with only 7+20 default shapes, causing index mismatch on encode.
        let dir = tempfile::tempdir().unwrap();
        let hwpx_path = dir.path().join("complex.hwpx");
        let md = "# Heading\n\nBody text.\n\n```rust\nfn main() {}\n```\n\n> Blockquote\n";
        crate::tools::convert::run_convert(md, false, hwpx_path.to_str().unwrap(), "default")
            .unwrap();

        // Restyle must not panic or produce corrupt output
        let out_path = dir.path().join("restyled.hwpx");
        let data = run_restyle(hwpx_path.to_str().unwrap(), "classic", out_path.to_str().unwrap())
            .unwrap();

        assert!(out_path.exists());
        assert_eq!(data.applied_preset, "classic");
        assert!(data.size_bytes > 0);

        // Verify the restyled file can be decoded back (not corrupted)
        use hwpforge_smithy_hwpx::HwpxDecoder;
        let restyled_bytes = std::fs::read(&out_path).unwrap();
        let restyled_doc = HwpxDecoder::decode(&restyled_bytes).unwrap();
        assert!(!restyled_doc.document.sections().is_empty());
    }
}
