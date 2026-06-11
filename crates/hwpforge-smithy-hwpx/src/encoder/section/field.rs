//! Field run builders — hyperlink, bookmark span, ClickHere (누름틀),
//! SUMMERY auto-fields, date-code, path, autonum, and cross-reference
//! (task #92 split from `encoder/section.rs`; Wave 12l/12m/12n carry
//! series). Includes the date helpers the SUMMERY builders evaluate
//! and the cross-ref wire-code mappers (8-param Hancom-canonical
//! Command — see HWP5_WIRE_SPEC.md §14 / gotcha #23-27).

use super::*;

/// Builds a complete `<hp:run>` XML string for a hyperlink.
///
/// HWPX hyperlinks use a `fieldBegin`/`fieldEnd` pair inside `<hp:ctrl>`
/// elements, interleaved with text content within a single `<hp:run>`:
///
/// ```xml
/// <hp:run charPrIDRef="N">
///   <hp:ctrl>
///     <hp:fieldBegin type="HYPERLINK" ... fieldid="F" ...>
///       <hp:parameters cnt="4" name="">
///         <hp:stringParam name="Path">URL</hp:stringParam>
///         ...
///       </hp:parameters>
///     </hp:fieldBegin>
///   </hp:ctrl>
///   <hp:t>display text</hp:t>
///   <hp:ctrl>
///     <hp:fieldEnd beginIDRef="F" fieldid="F"/>
///   </hp:ctrl>
/// </hp:run>
/// ```
///
/// This interleaved ordering (ctrl → text → ctrl) cannot be expressed by
/// serde's field-order-based serialization, hence the manual XML generation.
pub(super) fn build_hyperlink_run_xml(
    text: &str,
    url: &str,
    char_pr_id_ref: u32,
    field_id: usize,
) -> String {
    let escaped_url = escape_xml(url);
    let text_xml = build_text_element_xml(text);
    // Unique begin_id per field instance (matches build_field_run_xml pattern).
    // beginIDRef must reference this id, NOT the fieldid.
    // Hancom reads `fieldBegin id` as a signed 32-bit int; this base + field_id
    // must stay well below i32::MAX (2_147_483_647). Distinct per builder.
    let begin_id = 1_100_000_000_u64 + field_id as u64;
    // `fieldid` is a Hancom field instance id and must be a non-zero 32-bit
    // value; `fieldid="0"` is treated as an invalid instance. Distinct base
    // keeps it unique vs other field types and stays under 2^31.
    let field_uid = 1_628_000_000_u64 + field_id as u64;
    // KS X 6101: mailto: → HWPHYPERLINK_TYPE_EMAIL, others → HWPHYPERLINK_TYPE_URL
    let category = if url.starts_with("mailto:") {
        "HWPHYPERLINK_TYPE_EMAIL"
    } else {
        "HWPHYPERLINK_TYPE_URL"
    };
    format!(
        concat!(
            r#"<hp:run charPrIDRef="{cpr}">"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldBegin id="{bid}" type="HYPERLINK" name="" editable="0" dirty="0" "#,
            r#"zorder="-1" fieldid="{fid}" metaTag="">"#,
            r#"<hp:parameters cnt="4" name="">"#,
            r#"<hp:stringParam name="Path">{url}</hp:stringParam>"#,
            r#"<hp:stringParam name="Category">{cat}</hp:stringParam>"#,
            r#"<hp:stringParam name="TargetType">HWPHYPERLINK_TARGET_DOCUMENT_DONTCARE</hp:stringParam>"#,
            r#"<hp:stringParam name="DocOpenType">HWPHYPERLINK_JUMP_NEWTAB</hp:stringParam>"#,
            r#"</hp:parameters>"#,
            r#"</hp:fieldBegin>"#,
            r#"</hp:ctrl>"#,
            r#"{txt}"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldEnd beginIDRef="{bid}" fieldid="{fid}"/>"#,
            r#"</hp:ctrl>"#,
            r#"</hp:run>"#,
        ),
        cpr = char_pr_id_ref,
        bid = begin_id,
        fid = field_uid,
        url = escaped_url,
        cat = category,
        txt = text_xml,
    )
}

/// Builds a `<hp:run>` XML string for a span bookmark (fieldBegin/fieldEnd).
/// Builds a `<hp:run>` containing only `<hp:fieldBegin>` for bookmark span start.
///
/// The matching `<hp:fieldEnd>` is emitted by [`build_bookmark_span_end_run_xml`].
/// Text between them (in separate runs) is covered by the bookmark span.
pub(super) fn build_bookmark_span_start_run_xml(
    name: &str,
    char_pr_id_ref: u32,
    field_id: usize,
) -> String {
    let escaped_name = escape_xml(name);
    // Signed-32-bit-safe base; MUST match `build_bookmark_span_end_run_xml`
    // so the paired fieldEnd `beginIDRef` references this `id`.
    let begin_id = 1_200_000_000_u64 + field_id as u64;
    // Non-zero 32-bit field instance id; must match the paired fieldEnd.
    let field_uid = 1_728_000_000_u64 + field_id as u64;
    format!(
        concat!(
            r#"<hp:run charPrIDRef="{cpr}">"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldBegin id="{bid}" type="BOOKMARK" name="{name}" editable="0" dirty="0" "#,
            r#"zorder="-1" fieldid="{fid}" metaTag=""/>"#,
            r#"</hp:ctrl>"#,
            r#"</hp:run>"#,
        ),
        cpr = char_pr_id_ref,
        bid = begin_id,
        fid = field_uid,
        name = escaped_name,
    )
}

/// Builds a `<hp:run>` containing only `<hp:fieldEnd>` for bookmark span end.
pub(super) fn build_bookmark_span_end_run_xml(char_pr_id_ref: u32, field_id: usize) -> String {
    // Signed-32-bit-safe base; MUST match `build_bookmark_span_start_run_xml`.
    let begin_id = 1_200_000_000_u64 + field_id as u64;
    // Non-zero 32-bit field instance id; must match the paired fieldBegin.
    let field_uid = 1_728_000_000_u64 + field_id as u64;
    format!(
        concat!(
            r#"<hp:run charPrIDRef="{cpr}">"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldEnd beginIDRef="{bid}" fieldid="{fid}"/>"#,
            r#"</hp:ctrl>"#,
            r#"</hp:run>"#,
        ),
        cpr = char_pr_id_ref,
        bid = begin_id,
        fid = field_uid,
    )
}

/// Dispatches a `Control::Field` to the right HWPX `<hp:run>` builder
/// based on the field family.
///
/// # Field families
///
/// - **CLICK_HERE** (`build_clickhere_field_xml`): editable press-field
///   (누름틀). `type="CLICK_HERE"`, `fieldid=627272811`,
///   `Command=Clickhere:set:N:...`.
/// - **SUMMERY** (`build_summery_field_xml`): `$author`, `$lastsaveby`,
///   `$createtime`, `$modifiedtime`, `$title`. `type="SUMMERY"` (한글 typo),
///   `fieldid=628321650`.
pub(super) fn build_field_run_xml(
    field_type: &hwpforge_foundation::FieldType,
    hint: &str,
    help: &str,
    name: &str,
    char_pr_id_ref: u32,
    field_id: usize,
) -> String {
    use hwpforge_foundation::FieldType;
    let begin_id = 1_000_000_000_u64 + field_id as u64;
    match field_type {
        FieldType::ClickHere => {
            build_clickhere_field_xml(hint, help, name, char_pr_id_ref, begin_id)
        }
        FieldType::Author
        | FieldType::LastSavedBy
        | FieldType::CreatedTime
        | FieldType::ModifiedTime
        | FieldType::Title => {
            build_summery_field_xml(field_type, hint, name, char_pr_id_ref, begin_id)
        }
        // `FieldType` is `#[non_exhaustive]`. We intentionally do NOT collapse
        // future variants into ClickHere (Wave 12n architect review): silently
        // mis-encoding a future SUMMERY/auto-field token as CLICK_HERE would
        // create a stealth corruption path. New variants must explicitly extend
        // this match.
        _ => unreachable!(
            "FieldType variant added without an HWPX encoder branch — extend build_field_run_xml first"
        ),
    }
}

/// Builds the CLICK_HERE (누름틀) `<hp:run>` XML.
///
/// Wire convention: `hint_len`/`help_len` are UTF-16 code unit counts of the
/// *decoded* strings. `Command N` is computed by `clickhere_command_string`
/// from the empirically-derived formula (see that function's doc comment).
pub(super) fn build_clickhere_field_xml(
    hint: &str,
    help: &str,
    name: &str,
    char_pr_id_ref: u32,
    begin_id: u64,
) -> String {
    let escaped_hint = escape_xml(hint);
    let escaped_name = escape_xml(name);
    let hint_len = hint.encode_utf16().count();
    let help_len = help.encode_utf16().count();
    let command = clickhere_command_string(hint, help, hint_len, help_len);
    format!(
        concat!(
            r#"<hp:run charPrIDRef="{cpr}">"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldBegin id="{bid}" type="CLICK_HERE" name="{name}" editable="1" dirty="0" "#,
            r#"zorder="-1" fieldid="627272811" metaTag="">"#,
            r#"<hp:parameters cnt="3" name="">"#,
            r#"<hp:integerParam name="Prop">9</hp:integerParam>"#,
            r#"<hp:stringParam name="Command" xml:space="preserve">{cmd}</hp:stringParam>"#,
            r#"<hp:stringParam name="Direction">{hint}</hp:stringParam>"#,
            r#"</hp:parameters>"#,
            r#"</hp:fieldBegin>"#,
            r#"</hp:ctrl>"#,
            r#"{display}"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldEnd beginIDRef="{bid}" fieldid="627272811"/>"#,
            r#"</hp:ctrl>"#,
            r#"</hp:run>"#,
        ),
        cpr = char_pr_id_ref,
        bid = begin_id,
        name = escaped_name,
        cmd = escape_xml(&command),
        hint = escaped_hint,
        display = build_text_element_xml(hint),
    )
}

/// Builds a SUMMERY (Author/LastSavedBy/CreatedTime/ModifiedTime/Title) `<hp:run>` XML.
///
/// Reference: `tests/fixtures/fields/date_field.hwpx`. The HWP5 ctrl_id `%smr`
/// is shared by all SUMMERY auto-fields; discrimination is via the `Command`
/// `$token`. Token mapping verified against 한컴 native fixtures in Wave 12n
/// (see `.docs/research/2026-06-02_auto_field_wire_dump.md`).
pub(super) fn build_summery_field_xml(
    field_type: &hwpforge_foundation::FieldType,
    hint: &str,
    name: &str,
    char_pr_id_ref: u32,
    begin_id: u64,
) -> String {
    use hwpforge_foundation::FieldType;
    let command = field_type.summery_token().expect("caller guards SUMMERY variants");
    // Wave 12n Step 6.6: emit empty body for typed SUMMERY fields.
    //
    // The previous implementation computed today's ISO date for
    // ModifiedTime and parked single-space placeholders for the rest.
    // Empirically (sample-field-docsummary 검증, 2026-06-06):
    // Hancom Office discards mismatched display text and rebuilds the
    // field on save anyway, while triggering the "low-security
    // recovery" warning on open because our locale-mismatched values
    // (ISO `2026-06-06` vs native Korean `2026년 6월 4일 …`) are
    // treated as corrupted content. Letting Hancom recompute from
    // metadata avoids the warning entirely.
    //
    // For Author/LastSavedBy/Title the hint string (if supplied)
    // still carries through — it's caller-provided display text
    // rather than a computed placeholder.
    let display_text = match field_type {
        FieldType::Author | FieldType::LastSavedBy | FieldType::Title if !hint.is_empty() => {
            hint.to_string()
        }
        FieldType::ClickHere => unreachable!("caller already routed ClickHere elsewhere"),
        _ => String::new(),
    };
    // Wave 12p task #124: editable depends on whether Hancom recomputes
    // the field value (Author/Title → "0" lock to authored value;
    // LastSavedBy/CreatedTime/ModifiedTime → "1" Hancom recomputes).
    build_summery_run_xml_raw(
        command,
        &display_text,
        name,
        char_pr_id_ref,
        begin_id,
        field_type.hwpx_editable(),
    )
}

/// Lowest-level SUMMERY `<hp:run>` builder — emits a `type="SUMMERY"`
/// `fieldBegin`/`fieldEnd` pair with the caller-supplied `command` token
/// and `display` text. Used by [`build_summery_field_xml`] for typed
/// [`hwpforge_foundation::FieldType`] variants and by Wave 12n
/// `UnknownSummery` / `DateCodeField` fallback paths.
///
/// Wave 12n Step 6: `Control::PathField` no longer uses this builder.
/// See [`build_path_field_run_xml_raw`] for the native PATH wire shape.
pub(super) fn build_summery_run_xml_raw(
    command: &str,
    display: &str,
    name: &str,
    char_pr_id_ref: u32,
    begin_id: u64,
    editable: bool,
) -> String {
    let escaped_name = escape_xml(name);
    let escaped_cmd = escape_xml(command);
    let editable_bit = u8::from(editable);
    format!(
        concat!(
            r#"<hp:run charPrIDRef="{cpr}">"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldBegin id="{bid}" type="SUMMERY" name="{name}" editable="{ed}" dirty="0" "#,
            r#"zorder="-1" fieldid="628321650" metaTag="">"#,
            r#"<hp:parameters cnt="3" name="">"#,
            r#"<hp:integerParam name="Prop">8</hp:integerParam>"#,
            r#"<hp:stringParam name="Command">{cmd}</hp:stringParam>"#,
            r#"<hp:stringParam name="Property">{cmd}</hp:stringParam>"#,
            r#"</hp:parameters>"#,
            r#"</hp:fieldBegin>"#,
            r#"</hp:ctrl>"#,
            r#"{display}"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldEnd beginIDRef="{bid}" fieldid="628321650"/>"#,
            r#"</hp:ctrl>"#,
            // Wave 12n Step 6.5: trailing `<hp:t/>` matches Hancom native
            // wire shape — its absence triggers the "low-security recovery"
            // warning on open (verified against sample-field-docsummary
            // wire diff). Same structural requirement as PATH runs.
            r#"<hp:t/>"#,
            r#"</hp:run>"#,
        ),
        cpr = char_pr_id_ref,
        bid = begin_id,
        name = escaped_name,
        cmd = escaped_cmd,
        ed = editable_bit,
        display = build_text_element_xml(display),
    )
}

/// Lowest-level PATH `<hp:run>` builder — emits a `type="PATH"`
/// `fieldBegin`/`fieldEnd` pair carrying a `$P`/`$F`/`$P$F` format code
/// in the `Format` parameter. Wave 12n Step 6 — replaces the prior
/// SUMMERY surrogate for `Control::PathField`.
///
/// Wire shape (empirically derived from Hancom Office native
/// `sample-field-docsummary.hwp` → `.hwpx` conversion):
///
/// - `type="PATH"` (not SUMMERY — different field semantics)
/// - `fieldid="628121972"` (distinct from the SUMMERY `628321650`)
/// - `editable="0"` (PATH fields are read-only — Hancom recomputes)
/// - `<hp:parameters cnt="3">` with `Prop` / `Command` / **`Format`**
///   (NOT the SUMMERY `Property`)
/// - empty body (Hancom evaluates `$P$F` to the absolute path on save,
///   the same way `date` is recomputed)
pub(super) fn build_path_field_run_xml_raw(
    command: &str,
    char_pr_id_ref: u32,
    begin_id: u64,
) -> String {
    let escaped_cmd = escape_xml(command);
    // Wave 12n Step 6.5: Hancom-native PATH runs include a `<hp:t/>`
    // placeholder between fieldBegin/fieldEnd (recomputed to the
    // absolute path on save) AND a trailing `<hp:t/>` after fieldEnd.
    // Without the trailing element Hancom flags the file as
    // "low-security recovery" and rebuilds the run — verified against
    // `sample-field-docsummary.hwpx` wire diff after Step 6.
    format!(
        concat!(
            r#"<hp:run charPrIDRef="{cpr}">"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldBegin id="{bid}" type="PATH" name="" editable="0" dirty="0" "#,
            r#"zorder="-1" fieldid="628121972" metaTag="">"#,
            r#"<hp:parameters cnt="3" name="">"#,
            r#"<hp:integerParam name="Prop">8</hp:integerParam>"#,
            r#"<hp:stringParam name="Command">{cmd}</hp:stringParam>"#,
            r#"<hp:stringParam name="Format">{cmd}</hp:stringParam>"#,
            r#"</hp:parameters>"#,
            r#"</hp:fieldBegin>"#,
            r#"</hp:ctrl>"#,
            r#"<hp:t/>"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldEnd beginIDRef="{bid}" fieldid="628121972"/>"#,
            r#"</hp:ctrl>"#,
            r#"<hp:t/>"#,
            r#"</hp:run>"#,
        ),
        cpr = char_pr_id_ref,
        bid = begin_id,
        cmd = escaped_cmd,
    )
}

/// Builds the `Clickhere:set:N:...` command string.
///
/// `N` is **not** the total UTF-16 length of the command — empirically (verified
/// against five 한컴-authored fixtures including `basic`, `with-help`,
/// `empty-hint`, `multi`, and `named`) it equals the UTF-16 length of the
/// substring after `"Clickhere:set:N:"` minus one (one of the two trailing
/// spaces is excluded from `N`). The encoder can compute this directly
/// without iteration because the formula does not depend on `digits(N)`.
///
/// See `.docs/research/2026-06-02_clickhere_wire_dump.md` for the empirical
/// derivation.
pub(super) fn clickhere_command_string(
    hint: &str,
    help: &str,
    hint_len: usize,
    help_len: usize,
) -> String {
    let rest =
        format!("Direction:wstring:{hint_len}:{hint} HelpState:wstring:{help_len}:{help}  ",);
    let n = rest.encode_utf16().count().saturating_sub(1);
    format!("Clickhere:set:{n}:{rest}")
}

/// Builds a `<hp:run>` XML string for an inline page number (`<hp:autoNum>`).
///
/// Page numbers within body text use `<hp:autoNum numType="PAGE">` (current
/// page) or `numType="TOTAL_PAGE"` (total pages) — NOT fieldBegin/fieldEnd.
/// HWPX 스펙: `paralist.xsd` (`numType` enumeration includes
/// `PAGE`/`TOTAL_PAGE`/`FOOTNOTE`/...).
///
/// Returns `None` for [`hwpforge_core::control::InlinePageKind::Unknown`] —
/// the caller is expected to skip and emit a warning rather than fabricate
/// a `numType`. Wave 12n architect review CRITICAL: do not collapse
/// `TotalPages`/`Unknown` to `CurrentPage`.
pub(super) fn build_autonum_run_xml(
    char_pr_id_ref: u32,
    kind: hwpforge_core::control::InlinePageKind,
) -> Option<String> {
    let num_type = match kind {
        hwpforge_core::control::InlinePageKind::CurrentPage => "PAGE",
        hwpforge_core::control::InlinePageKind::TotalPages => "TOTAL_PAGE",
        hwpforge_core::control::InlinePageKind::Unknown => return None,
        // `InlinePageKind` is `#[non_exhaustive]`. Skip future kinds instead of
        // fabricating a numType — match the Unknown policy.
        _ => return None,
    };
    Some(format!(
        concat!(
            r#"<hp:run charPrIDRef="{cpr}">"#,
            r#"<hp:ctrl>"#,
            r#"<hp:autoNum num="1" numType="{nt}">"#,
            r#"<hp:autoNumFormat type="DIGIT" userChar="" prefixChar="" suffixChar="" supscript="0"/>"#,
            r#"</hp:autoNum>"#,
            r#"</hp:ctrl>"#,
            r#"</hp:run>"#,
        ),
        cpr = char_pr_id_ref,
        nt = num_type,
    ))
}

/// Simple days-since-epoch to (year, month, day) conversion.
///
/// Wave 12n Step 6.6: no longer called by `build_summery_field_xml`
/// (SUMMERY body is now empty so Hancom recomputes from metadata).
/// Retained for the existing unit tests and possible future date
/// emit paths.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn days_to_ymd(days_since_epoch: u64) -> (u64, u64, u64) {
    // Simplified civil calendar calculation.
    let z = days_since_epoch + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Wave 12m Phase 2 Step 4 boundary: typed [`RefType`] → HWP5 `%xrf`
/// N1 wire code. Returns the canonical code for known variants and
/// `Unknown(code)` preserves the original byte.
pub(super) fn ref_type_wire_code(ref_type: &hwpforge_foundation::RefType) -> u8 {
    use hwpforge_foundation::RefType::*;
    match ref_type {
        Table => 0,
        Figure => 1,
        Equation => 2,
        Footnote => 3,
        Endnote => 4,
        Outline => 5,
        Bookmark => 6,
        Unknown(other) => *other,
        // Foundation RefType is `#[non_exhaustive]`; future variants
        // default to the Table wire code (the most common non-Bookmark
        // case) so Hancom still sees a parsable Command.
        _ => 0,
    }
}

/// Wave 12m Phase 2 Step 4 boundary: typed [`RefContentType`] → HWP5
/// `%xrf` N2 wire code, RefType-relative.
pub(super) fn ref_content_type_wire_code(
    ref_type: &hwpforge_foundation::RefType,
    content: &hwpforge_foundation::RefContentType,
) -> u8 {
    use hwpforge_foundation::RefContentType::*;
    let _ = ref_type;
    match content {
        Page => 0,
        UpDownPos => 3,
        Number => 1,
        // Wave 12p pre-fix: native wire 일치. 모든 RefType 에서
        // `Contents` 의 N2 wire code 는 2 (Figure/Table/Eq/Outline =
        // "캡션 내용"). 별도로 `BookmarkName` variant (한컴 "책갈피
        // 이름") 도 N2=2 로 emit — RefType=TARGET_BOOKMARK 컨텍스트에서
        // 한컴이 의미를 결정. (Bookmark+Number=N2=1 = 책갈피 본문/번호
        // 는 위 `Number => 1` arm 이 처리.)
        Contents | BookmarkName => 2,
        Unknown(other) => *other,
        // Foundation RefContentType is `#[non_exhaustive]`; future
        // variants default to Page (slot 0).
        _ => 0,
    }
}

/// Builds the `?<target>;` form used by Hancom for both `Command` and
/// `RefPath` parameters. Bookmark refs use the raw name; non-Bookmark
/// refs prepend `#` to a SystemId. `RefTarget::Raw` is passed through.
pub(super) fn crossref_target_for_command(
    target: &hwpforge_core::control::RefTarget,
    ref_type: &hwpforge_foundation::RefType,
) -> String {
    use hwpforge_core::control::RefTarget;
    match target {
        RefTarget::Name(name) => name.clone(),
        RefTarget::SystemId(id) => format!("#{id}"),
        RefTarget::Raw(raw) => {
            // Heuristic: Bookmark refs treat raw as a name; others treat
            // it as a pre-formatted `#<id>` style token. Preserves the
            // input verbatim.
            let _ = ref_type;
            raw.clone()
        }
        // Core `RefTarget` is `#[non_exhaustive]`; future variants
        // fall back to empty (caller can warn separately).
        _ => String::new(),
    }
}

/// Builds a `<hp:run>` XML string for a cross-reference (상호참조).
///
/// Wave 12m Phase 2 Step 4: unified Hancom-canonical 8-parameter form
/// (`Fiexde`/`Prop`/`Command`/`RefPath`/`RefType`/`RefContentType`/
/// `RefHyperLink`/`RefOpenType=HWPHYPERLINK_JUMP_CURRENTTAB`). The
/// pre-Step-4 5-param form did not round-trip through 한컴; the 8-param
/// form matches what 한컴 itself emits when authoring CROSSREF.
pub(super) fn build_crossref_run_xml(
    target: &hwpforge_core::control::RefTarget,
    display_text: &str,
    ref_type: &hwpforge_foundation::RefType,
    content_type: &hwpforge_foundation::RefContentType,
    as_hyperlink: bool,
    char_pr_id_ref: u32,
    field_id: usize,
) -> String {
    let target_token = escape_xml(&crossref_target_for_command(target, ref_type));
    let ref_type_str = ref_type.to_string();
    let content_type_str = content_type.to_string();
    let n1 = ref_type_wire_code(ref_type);
    let n2 = ref_content_type_wire_code(ref_type, content_type);
    let n3: u8 = if as_hyperlink { 1 } else { 0 };
    let hyperlink_val = if as_hyperlink { "true" } else { "false" };
    // Signed-32-bit-safe begin_id base; distinct from other field builders.
    // Hancom reads `fieldBegin id` as i32; a base >= 2^31 wraps negative
    // and the field is no longer recognized.
    let begin_id = 1_300_000_000_u64 + field_id as u64;
    // Wave 12m Phase 2 Step 4 fixup: `fieldid` is a Hancom **type tag**,
    // not an instance id. Native Hancom HWPX emits the ctrl_id's ASCII
    // big-endian u32 — same convention as ClickHere (`%clk`=0x25636C6B),
    // SummeryField (`%smr`=0x25736D72), PathField (`%pat`=0x25706174).
    // For CROSSREF the constant is `%xrf` = 0x25787266 = 628_650_598.
    // All CROSSREF fields in a document share this fieldid; per-instance
    // identity is carried by `id` (begin_id) above. Verified against
    // n=11 native Hancom-authored .hwpx samples (re-research stage 2).
    let field_uid: u64 = 0x25787266;
    let _ = field_id; // kept in signature for symmetry; not used for fieldid.
    let display_text_xml = build_text_element_xml(display_text);
    format!(
        concat!(
            r#"<hp:run charPrIDRef="{cpr}">"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldBegin id="{bid}" type="CROSSREF" name="" editable="0" dirty="0" "#,
            r#"zorder="-1" fieldid="{fid}" metaTag="">"#,
            r#"<hp:parameters cnt="8" name="">"#,
            r#"<hp:booleanParam name="Fiexde">1</hp:booleanParam>"#,
            r#"<hp:integerParam name="Prop">0</hp:integerParam>"#,
            r#"<hp:stringParam name="Command">?{target};{n1};{n2};{n3};0;</hp:stringParam>"#,
            r#"<hp:stringParam name="RefPath">?{target};</hp:stringParam>"#,
            r#"<hp:stringParam name="RefType">{ref_type}</hp:stringParam>"#,
            r#"<hp:stringParam name="RefContentType">{content_type}</hp:stringParam>"#,
            r#"<hp:booleanParam name="RefHyperLink">{hyperlink}</hp:booleanParam>"#,
            r#"<hp:stringParam name="RefOpenType">HWPHYPERLINK_JUMP_CURRENTTAB</hp:stringParam>"#,
            r#"</hp:parameters>"#,
            r#"</hp:fieldBegin>"#,
            r#"</hp:ctrl>"#,
            r#"{display_text_xml}"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldEnd beginIDRef="{bid}" fieldid="{fid}"/>"#,
            r#"</hp:ctrl>"#,
            r#"</hp:run>"#,
        ),
        cpr = char_pr_id_ref,
        bid = begin_id,
        fid = field_uid,
        target = target_token,
        n1 = n1,
        n2 = n2,
        n3 = n3,
        ref_type = ref_type_str,
        content_type = content_type_str,
        hyperlink = hyperlink_val,
        display_text_xml = display_text_xml,
    )
}

/// Legacy Hancom-format CROSSREF builder retained for fieldid regression
/// gates. The production encoder routes everything through
/// [`build_crossref_run_xml`] after Wave 12m Step 4; this stays only as
/// a stable target for guarantees about fieldid range and pairing
/// invariants.
#[cfg(test)]
pub(super) fn build_hwp5_crossref_run_xml(
    target_name: &str,
    display_text: &str,
    ref_type: hwpforge_foundation::RefType,
    content_type: hwpforge_foundation::RefContentType,
    as_hyperlink: bool,
    char_pr_id_ref: u32,
    field_id: usize,
) -> String {
    let escaped_target_name = escape_xml(target_name);
    let escaped_display_text = build_text_element_xml(display_text);
    let ref_type_str = ref_type.to_string();
    let content_type_str = content_type.to_string();
    let hyperlink_val = if as_hyperlink { "true" } else { "false" };
    // Signed-32-bit-safe begin_id base; distinct from other field builders.
    // Hancom reads `fieldBegin id` as i32; a base >= 2^31 wraps negative and
    // the field is no longer recognized (click / F9 / Ctrl+click jump fail).
    let begin_id = 1_400_000_000_u64 + field_id as u64;
    // `fieldid` is a Hancom field instance id and must be a non-zero 32-bit
    // value. A raw 0-based `field_id` would emit `fieldid="0"`, which Hancom
    // treats as an invalid instance (F9 refresh / Ctrl+click jump break).
    // Distinct base keeps it unique vs other field types' fieldid id-space
    // and stays under 2^31 (truth fixtures use values < 2^31).
    let field_uid = 1_928_000_000_u64 + field_id as u64;
    format!(
        concat!(
            r#"<hp:run charPrIDRef="{cpr}">"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldBegin id="{bid}" type="CROSSREF" name="" editable="0" dirty="0" "#,
            r#"zorder="-1" fieldid="{fid}" metaTag="">"#,
            r#"<hp:parameters cnt="8" name="">"#,
            r#"<hp:booleanParam name="Fiexde">1</hp:booleanParam>"#,
            r#"<hp:integerParam name="Prop">0</hp:integerParam>"#,
            r#"<hp:stringParam name="Command">?{target};6;0;0;0;</hp:stringParam>"#,
            r#"<hp:stringParam name="RefPath">?{target};</hp:stringParam>"#,
            r#"<hp:stringParam name="RefType">{ref_type}</hp:stringParam>"#,
            r#"<hp:stringParam name="RefContentType">{content_type}</hp:stringParam>"#,
            r#"<hp:booleanParam name="RefHyperLink">{hyperlink}</hp:booleanParam>"#,
            r#"<hp:stringParam name="RefOpenType">HWPHYPERLINK_JUMP_CURRENTTAB</hp:stringParam>"#,
            r#"</hp:parameters>"#,
            r#"</hp:fieldBegin>"#,
            r#"</hp:ctrl>"#,
            r#"{display_text}"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldEnd beginIDRef="{bid}" fieldid="{fid}"/>"#,
            r#"</hp:ctrl>"#,
            r#"</hp:run>"#,
        ),
        cpr = char_pr_id_ref,
        bid = begin_id,
        fid = field_uid,
        target = escaped_target_name,
        ref_type = ref_type_str,
        content_type = content_type_str,
        hyperlink = hyperlink_val,
        display_text = escaped_display_text,
    )
}

pub(super) fn unix_to_ymdhms(secs: u64) -> (i64, u32, u32, u32, u32, u32) {
    let total_days = (secs / 86_400) as i64;
    let tod = secs % 86_400;
    let hour = (tod / 3_600) as u32;
    let minute = ((tod % 3_600) / 60) as u32;
    let second = (tod % 60) as u32;
    // 1970-01-01 → days since 0000-03-01 (era anchor) = 719468.
    let z = total_days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146097)
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 400)
    let y0 = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 366)
    let mp = (5 * doy + 2) / 153; // [0, 12)
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month_civil = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year_civil = if month_civil <= 2 { y0 + 1 } else { y0 };
    (year_civil, month_civil, day, hour, minute, second)
}
