//! `hwpforge_restyle` — Apply a different style preset to an existing HWPX document.

use serde::Serialize;

use hwpforge_smithy_hwpx::presets::builtin_presets;
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder};

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
    // 1. Validate output extension
    if !output_path.ends_with(".hwpx") {
        return Err(ToolErrorInfo::new(
            "INVALID_EXTENSION",
            format!("Output path must end with .hwpx: {output_path}"),
            "Use a .hwpx extension for the output file.",
        ));
    }

    // 2. Look up preset font
    let presets = builtin_presets();
    let preset_info = presets.iter().find(|p| p.name == preset).ok_or_else(|| {
        ToolErrorInfo::new(
            "PRESET_NOT_FOUND",
            format!("Preset '{preset}' not found"),
            "Use hwpforge_templates to see available presets.",
        )
    })?;
    let preset_font = preset_info.font.clone();

    // 3. Read and decode source HWPX
    let bytes = read_file_bytes(file_path)?;

    let hwpx_doc = HwpxDecoder::decode(&bytes).map_err(|e| {
        ToolErrorInfo::new(
            "DECODE_ERROR",
            format!("HWPX decode failed: {e}"),
            "Check that the file is a valid HWPX document.",
        )
    })?;

    // 4. Replace base font in the decoded style store.
    //    Instead of creating a new preset store (which would lose char/para shape
    //    definitions the document references), we keep the original style store
    //    intact and only swap font face names. This preserves all shape indices
    //    while applying the new font.
    let mut style_store = hwpx_doc.style_store;
    // The first font in the store is the base/body font by encoder contract
    // (HwpxStyleStore::push_font writes base font first). Third-party HWPX
    // files may have a different ordering — a future improvement could resolve
    // the base font from the default paragraph style instead.
    let original_base: Option<String> =
        style_store.iter_fonts().next().map(|f| f.face_name.clone());
    match original_base {
        Some(ref base) => style_store.replace_font(base, &preset_font),
        None => {
            return Err(ToolErrorInfo::new(
                "NO_FONTS",
                "Document has no fonts to restyle",
                "The HWPX file may be malformed. Use hwpforge_validate to check.",
            ));
        }
    }

    let validated = hwpx_doc.document.validate().map_err(|e| {
        ToolErrorInfo::new(
            "VALIDATION_ERROR",
            format!("Document validation failed: {e}"),
            "Check document structure.",
        )
    })?;

    let section_count = validated.section_count();

    let outcome = HwpxEncoder::encode_with_diagnostics(
        &validated,
        &style_store,
        &hwpx_doc.image_store,
        hwpforge_smithy_hwpx::EncodeOptions::default(),
    )
    .map_err(|e| {
        ToolErrorInfo::new(
            "ENCODE_ERROR",
            format!("HWPX encoding failed: {e}"),
            "This may be a bug. Please report at https://github.com/ai-screams/HwpForge/issues",
        )
    })?;
    // 5. 의미 손상 fail-closed (R1 F2).
    //
    // restyle 은 preserve-first 재인코드 편집기다 — 원본을 다시 만들어 내는
    // 경로라, 인코더가 "각주 번호 머리를 못 넣었다"(`NoteHeadSkipped`) 같은
    // 의미 손상을 보고하면 그 산출물은 **원본과 뜻이 다르다**. 과거에는 이
    // 경고를 `warnings` 문자열로만 싣고 파일을 그대로 썼기 때문에, 각주 번호가
    // 사라진 문서가 조용히 배포됐다 (stamper·cell-edit 는 이미 거부하던 조건).
    //
    // 분류의 정의는 `EncodeWarning::is_semantic_loss` 하나뿐이다.
    let semantic: Vec<String> = outcome
        .warnings
        .iter()
        .filter(|w| w.is_semantic_loss())
        .map(std::string::ToString::to_string)
        .collect();
    if !semantic.is_empty() {
        return Err(ToolErrorInfo::new(
            "ENCODE_SEMANTIC_LOSS",
            semantic.join("; "),
            "The restyled document would lose footnote/endnote numbering or TOC marks; fix the \
             source document or restyle a document without those constructs.",
        ));
    }

    let output_bytes = outcome.bytes;
    // 비-의미 경고(줄 조판 캐시 드롭 등)는 지금까지처럼 결과에 실어 보낸다.
    let warnings: Vec<String> =
        outcome.warnings.iter().map(std::string::ToString::to_string).collect();

    // 6. Write output
    write_output_file(output_path, &output_bytes)?;

    let size_bytes = output_bytes.len() as u64;

    Ok(RestyleData {
        output_path: output_path.to_string(),
        applied_preset: preset.to_string(),
        size_bytes,
        sections: section_count,
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
        let restyled_bytes = std::fs::read(&out_path).unwrap();
        let restyled_doc = HwpxDecoder::decode(&restyled_bytes).unwrap();
        assert!(!restyled_doc.document.sections().is_empty());
    }
}
