//! Memo (annotation) run/sublist builders (task #92 split from
//! `encoder/section.rs`; Wave 12e/f/g/h carry series).

use super::*;

/// Builds a `<hp:run>` XML string for a memo annotation.
///
/// `anchor_xml` is the inline `<hp:t>…</hp:t>` sequence representing the
/// visible body span the memo is attached to; it is placed *between*
/// `<hp:fieldBegin>` and `<hp:fieldEnd>` in the same `<hp:run>`. An empty
/// `anchor_xml` reproduces the pre-Wave-12f point-anchored layout, which
/// 한컴 renders as `[메모 시작][필드 끝]` (the memo end marker is
/// unpaired); see `.docs/algorithms/2026-06-01_memo_anchor_serialization.md`
/// for why we collapse anchor_runs to a single `<hp:t>` element here.
pub(super) fn build_memo_run_xml(
    sublist_xml: &str,
    anchor_xml: &str,
    metadata: &hwpforge_core::MemoMetadata,
    char_pr_id_ref: u32,
    field_id: usize,
) -> String {
    // Signed-32-bit-safe begin_id base; distinct from other field builders.
    let begin_id = 1_500_000_000_u64 + field_id as u64;
    // Non-zero 32-bit field instance id; `fieldid="0"` is invalid in Hancom.
    let field_uid = 2_028_000_000_u64 + field_id as u64;

    let id = metadata.hwpx_id();
    let command = if metadata.command.is_empty() {
        // HwpForge-authored memos synthesise a minimal Command string so
        // 한컴 still pairs the field markers correctly. Format mirrors what
        // 한컴 writes for a wire-less memo.
        format!("MEMO/{}/{}/0/0/{}/\\;;", metadata.shape_id_ref, metadata.number, metadata.author)
    } else {
        metadata.command.clone()
    };
    let create_datetime = if metadata.create_datetime.is_empty() {
        iso8601_utc_now()
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
            r#"{anchor}"#,
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
    let command_esc = escape_xml(command);
    let id_esc = escape_xml(id);
    let author_esc = escape_xml(author);
    let dt_esc = escape_xml(create_datetime);
    format!(
        concat!(
            r#"<hp:parameters cnt="7" name="">"#,
            r#"<hp:integerParam name="Prop">0</hp:integerParam>"#,
            r#"<hp:stringParam name="Command">{cmd}</hp:stringParam>"#,
            r#"<hp:stringParam name="ID">{id}</hp:stringParam>"#,
            r#"<hp:integerParam name="Number">{num}</hp:integerParam>"#,
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

/// Serializes a memo's `anchor_runs` into an inline `<hp:t>…</hp:t>` sequence
/// that lives between `<hp:fieldBegin type="MEMO">` and `<hp:fieldEnd>`.
///
/// Lossy by design: every `RunContent::Text` is concatenated into a single
/// `<hp:t>` element; non-text runs are skipped; the per-run `char_shape_id`
/// is *not* preserved (the surrounding `<hp:run>` already carries a single
/// `charPrIDRef`). 한컴's own HWPX output is the same shape — a memo's
/// anchor is a single `<hp:t>` per `<hp:run>` even when the source HWP5
/// stream split it across char_shape changes.
///
/// Returns `<hp:t/>` for an empty anchor; that path reproduces the
/// pre-Wave-12f point-anchored layout, which 한컴 mis-renders, so the
/// projection layer should always populate `anchor_runs` when a memo's
/// HWP5 `FieldBegin..FieldEnd` span contains text.
///
/// See `.docs/algorithms/2026-06-01_memo_anchor_serialization.md` for the
/// fidelity tradeoff and why we accept it.
pub(super) fn build_memo_anchor_xml(anchor_runs: &[hwpforge_core::run::Run]) -> String {
    use hwpforge_core::run::RunContent;
    let mut text = String::new();
    for run in anchor_runs {
        if let RunContent::Text(s) = &run.content {
            text.push_str(s);
        }
        // Non-text variants are dropped; a memo anchor cannot wrap
        // tables/images/nested controls in HWPX, and 한컴 does not produce
        // such anchors. If they ever appear, the lossy collapse here is
        // strictly preferable to emitting an empty anchor (the old buggy
        // path) — both bug 1 (wrong anchor position) and bug 2 (`[필드 끝]`
        // mis-label) regress if the anchor is empty.
    }
    if text.is_empty() {
        return "<hp:t/>".to_string();
    }
    build_text_element_xml(&text)
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
    sink: &mut EncodeSink,
) -> HwpxResult<String> {
    sink.enter(crate::decoder::PathSeg::Memo);
    let sublist_result =
        encode_paragraphs_to_sublist(paragraphs, depth, hyperlink_entries, options, sink);
    sink.leave();
    let sublist = sublist_result?;
    let xml = quick_xml::se::to_string(&sublist)
        .map_err(|e| HwpxError::InvalidStructure { detail: e.to_string() })?;
    // Fix root element: <HxSubList ...>...</HxSubList> → <hp:subList ...>...</hp:subList>
    let xml = xml.replacen("<HxSubList", "<hp:subList", 1);
    let xml = xml.replacen("</HxSubList>", "</hp:subList>", 1);
    Ok(xml)
}
