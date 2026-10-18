//! Memo (annotation) run/sublist builders (task #92 split from
//! `encoder/section.rs`; Wave 12e/f/g/h carry series).

use super::*;

/// Builds a `<hp:run>` XML string for a memo annotation.
///
/// `anchor_xml` contains complete runs between the begin and end markers.
/// Each anchor keeps its character shape and inline text attributes.
pub(super) fn build_memo_run_xml(
    sublist_xml: &str,
    anchor_xml: &str,
    metadata: &hwpforge_core::MemoMetadata,
    char_pr_id_ref: u32,
    field_id: usize,
    memo_create_time: Option<u64>,
) -> String {
    // Signed-32-bit-safe begin_id base; distinct from other field builders.
    let begin_id = super::super::object_ids::field_id(1_500_000_000_u64 + field_id as u64);
    // Non-zero 32-bit field instance id; `fieldid="0"` is invalid in Hancom.
    let field_uid = 2_028_000_000_u64 + field_id as u64;

    let id = metadata.hwpx_id();
    let command = if metadata.command.is_empty() && !metadata.preserve_empty {
        // HwpForge-authored memos synthesise a minimal Command string so
        // 한컴 still pairs the field markers correctly. Format mirrors what
        // 한컴 writes for a wire-less memo.
        format!("MEMO/{}/{}/0/0/{}/\\;;", metadata.shape_id_ref, metadata.number, metadata.author)
    } else {
        metadata.command.clone()
    };
    // W0: no clock read — the caller's time or the epoch.
    let create_datetime = if metadata.create_datetime.is_empty() && !metadata.preserve_empty {
        memo_default_datetime(memo_create_time)
    } else {
        metadata.create_datetime.clone()
    };

    let parameters = build_memo_parameters_xml(
        metadata.shape_id_ref,
        &command,
        &id,
        metadata.number,
        &metadata.author,
        &create_datetime,
    );

    format!(
        concat!(
            r#"<hp:run charPrIDRef="{cpr}">"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldBegin id="{bid}" type="MEMO" name="" editable="1" dirty="1" "#,
            r#"zorder="1" fieldid="{fid}" metaTag="">"#,
            r#"{params}"#,
            r#"{sublist}"#,
            r#"</hp:fieldBegin>"#,
            r#"</hp:ctrl>"#,
            r#"</hp:run>{anchor}<hp:run charPrIDRef="{cpr}">"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldEnd beginIDRef="{bid}" fieldid="{fid}"/>"#,
            r#"</hp:ctrl>"#,
            r#"</hp:run>"#,
        ),
        cpr = char_pr_id_ref,
        bid = begin_id,
        fid = field_uid,
        params = parameters,
        sublist = sublist_xml,
        anchor = anchor_xml,
    )
}

/// Builds the 7-parameter `<hp:parameters>` block 한컴 writes for a memo
/// fieldBegin. Extracted from `build_memo_run_xml` so the same structure
/// is easy to find when other field types need similar parameter blocks
/// (hyperlink/crossref already use a 4-parameter analogue inside
/// `build_hyperlink_run_xml`; that one can converge on this helper when
/// it gains parity with 한컴 truth).
pub(super) fn build_memo_parameters_xml(
    shape_id_ref: u32,
    command: &str,
    id: &str,
    number: u32,
    author: &str,
    create_datetime: &str,
) -> String {
    let command_esc = super::field::escape_field_value(command);
    let id_esc = super::field::escape_field_value(id);
    let author_esc = super::field::escape_field_value(author);
    let dt_esc = super::field::escape_field_value(create_datetime);
    format!(
        concat!(
            r#"<hp:parameters cnt="7" name="">"#,
            r#"<hp:integerParam name="Prop">0</hp:integerParam>"#,
            r#"<hp:integerParam name="Number">{num}</hp:integerParam>"#,
            r#"<hp:stringParam name="Command">{cmd}</hp:stringParam>"#,
            r#"<hp:stringParam name="ID">{id}</hp:stringParam>"#,
            r#"<hp:stringParam name="Author">{author}</hp:stringParam>"#,
            r#"<hp:stringParam name="MemoShapeIDRef">{shape}</hp:stringParam>"#,
            r#"<hp:stringParam name="CreateDateTime">{dt}</hp:stringParam>"#,
            r#"</hp:parameters>"#,
        ),
        cmd = command_esc,
        id = id_esc,
        num = number,
        author = author_esc,
        shape = shape_id_ref,
        dt = dt_esc,
    )
}

/// Serializes each anchor run with its character shape and inline attributes.
/// As with the existing memo writer, only text-bearing anchor runs are emitted.
pub(super) fn build_memo_anchor_xml(
    anchor_runs: &[hwpforge_core::run::Run],
    sink: &DocumentEncodeContext,
) -> HwpxResult<String> {
    let mut xml = String::new();
    for run in anchor_runs {
        let text = match &run.content {
            RunContent::Text(s) => build_text_element_xml(s),
            RunContent::InlineText(t) => build_inline_text_element_xml(t),
            _ => continue,
        };
        xml.push_str(&format!(
            r#"<hp:run charPrIDRef="{}">{text}</hp:run>"#,
            sink.output_id("charPr", run.char_shape_id.get())?
        ));
    }
    Ok(xml)
}

/// Encodes memo body paragraphs as an XML string for embedding inside fieldBegin.
///
/// `quick_xml::se::to_string` uses the Rust struct name `HxSubList` as the root
/// element because `HxSubList` has no struct-level serde rename (the `hp:subList`
/// rename lives on parent struct fields). We must fix the root tag manually.
pub(super) fn encode_memo_sublist(
    paragraphs: &[Paragraph],
    depth: usize,
    hyperlink_entries: &mut Vec<(String, String)>,
    options: EncodeOptions,
    sink: &mut DocumentEncodeContext,
) -> HwpxResult<String> {
    sink.enter(crate::decoder::PathSeg::Memo);
    let sublist_result =
        encode_paragraphs_to_sublist(paragraphs, depth, hyperlink_entries, options, sink);
    sink.leave();
    let sublist = sublist_result?;
    #[allow(clippy::disallowed_methods)] // normalized on the next line
    let xml = quick_xml::se::to_string(&sublist)
        .map_err(|e| HwpxError::InvalidStructure { detail: e.to_string() })?;
    let xml = crate::wire_xml::normalize_attr_control_whitespace(&xml);
    // Fix root element: <HxSubList ...>...</HxSubList> → <hp:subList ...>...</hp:subList>
    let xml = xml.replacen("<HxSubList", "<hp:subList", 1);
    let xml = xml.replacen("</HxSubList>", "</hp:subList>", 1);
    Ok(xml)
}
