//! Lossless output is a machine view for checking the preservation scope
//! (#281): general decoding refuses it by its frontmatter marker, while
//! `decode_lossless` still reads it.
#[path = "support/document.rs"]
mod document;
use document::{doc, text};
use hwpforge_smithy_md::{MdDecoder, MdEncoder, MdError};

fn titled_lossless_output() -> String {
    let mut draft = doc(vec![text("본문")]);
    draft.metadata_mut().title = Some("보고서".into());
    MdEncoder::encode_lossless(&draft.validate().unwrap()).unwrap()
}

// 이것을 실패시키는 것: `reject_lossless_view` 호출 삭제(본문의 raw HTML 오류로 바뀌어
// 안내 문구 단언이 실패), 비교 값 `"lossless"` 를 다른 문자열로 바꾸기.
#[test]
fn general_decode_refuses_lossless_output_with_guidance() {
    let err = MdDecoder::decode_with_default(&titled_lossless_output()).unwrap_err();

    let MdError::UnsupportedStructure { detail } = &err else { panic!("{err:?}") };
    assert!(detail.starts_with("lossless Markdown is a machine-oriented view"), "{detail}");
    assert!(detail.contains("use to-json/from-json for round-trip edits"), "{detail}");
}

// 이것을 실패시키는 것: 표식 검사를 파싱된 metadata 대신 원문 문자열
// `markdown.contains("format: lossless")` 로 바꾸기.
#[test]
fn a_body_mentioning_the_marker_still_decodes() {
    let markdown =
        "---\ntitle: 안내\n---\n\n설정 예: `metadata.format: lossless`\n\nformat: lossless\n";

    let decoded = MdDecoder::decode_with_default(markdown).expect("plain Markdown decodes");

    assert_eq!(decoded.document.metadata().title.as_deref(), Some("안내"));
}

// 이것을 실패시키는 것: `reject_lossless_view` 를 `decode_lossless` 에도 넣기.
#[test]
fn decode_lossless_still_reads_lossless_output() {
    let document = MdDecoder::decode_lossless(&titled_lossless_output()).expect("lossless reads");

    assert_eq!(document.metadata().title.as_deref(), Some("보고서"));
    assert_eq!(document.sections()[0].paragraphs[0].runs[0], text("본문"));
}

// Only the exact string value of `metadata.format` marks the view; case,
// quoted spaces, other values, a top-level key and a fenced lookalike all
// decode as ordinary Markdown.
// 이것을 실패시키는 것: 표식 비교를 대소문자 무시(`eq_ignore_ascii_case`)로 바꾸기
// (`Lossless` 행), `trim()` 뒤 비교로 바꾸기(`" lossless "` 행).
#[test]
fn only_the_exact_marker_value_is_refused() {
    let cases = [
        ("case", "---\nmetadata:\n  format: Lossless\n---\n\n본문\n"),
        ("quoted spaces", "---\nmetadata:\n  format: \" lossless \"\n---\n\n본문\n"),
        ("other value", "---\nmetadata:\n  format: styled\n---\n\n본문\n"),
        ("top-level key", "---\nformat: lossless\n---\n\n본문\n"),
        ("fenced", "본문\n\n```yaml\n---\nmetadata:\n  format: lossless\n---\n```\n"),
    ];
    for (name, markdown) in cases {
        if let Err(err) = MdDecoder::decode_with_default(markdown) {
            panic!("{name}: {err:?}");
        }
    }
}

// The marker is checked before the template is resolved, so the guidance
// wins over a template that cannot be resolved.
// 이것을 실패시키는 것: `reject_lossless_view` 호출을 `resolve_mapping` 뒤로 옮기기.
#[test]
fn the_marker_is_refused_before_style_mapping() {
    let template = hwpforge_blueprint::template::Template::from_yaml(
        "meta:\n  name: broken\n  version: 1.0.0\n  extends: no_such_template\n",
    )
    .unwrap();

    let plain = MdDecoder::decode("본문\n", &template).unwrap_err();
    assert!(matches!(plain, MdError::TemplateResolution { .. }), "{plain:?}");
    let err = MdDecoder::decode(&titled_lossless_output(), &template).unwrap_err();
    let MdError::UnsupportedStructure { detail } = &err else { panic!("{err:?}") };
    assert!(detail.starts_with("lossless Markdown is a machine-oriented view"), "{detail}");
}
