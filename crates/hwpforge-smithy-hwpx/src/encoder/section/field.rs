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
/// - **SUMMERY** (`build_summary_field_xml`): `$author`, `$lastsaveby`,
///   `$createtime`, `$modifiedtime`, `$title`. `type="SUMMERY"` (한글 typo),
///   `fieldid=628321650`.
pub(super) fn build_field_run_xml(
    field_type: &hwpforge_foundation::FieldType,
    hint: &str,
    help: &str,
    name: &str,
    display_text: &str,
    char_pr_id_ref: u32,
    field_id: usize,
) -> String {
    use hwpforge_foundation::FieldType;
    let begin_id = 1_000_000_000_u64 + field_id as u64;
    match field_type {
        FieldType::ClickHere => {
            build_clickhere_field_xml(hint, help, name, char_pr_id_ref, begin_id, display_text)
        }
        FieldType::Author
        | FieldType::LastSavedBy
        | FieldType::CreatedTime
        | FieldType::ModifiedTime
        | FieldType::Title => {
            build_summary_field_xml(field_type, hint, name, display_text, char_pr_id_ref, begin_id)
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
///
/// `body` is the filled field value rendered between `fieldBegin` and
/// `fieldEnd`. Empty `body` = unfilled → the hint placeholder is emitted as
/// the body (한컴 native convention; byte-neutral for HWP5→HWPX carry where
/// the ClickHere span is always empty).
pub(super) fn build_clickhere_field_xml(
    hint: &str,
    help: &str,
    name: &str,
    char_pr_id_ref: u32,
    begin_id: u64,
    body: &str,
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
        display = build_text_element_xml(if body.is_empty() { hint } else { body }),
    )
}

/// Builds a SUMMERY (Author/LastSavedBy/CreatedTime/ModifiedTime/Title) `<hp:run>` XML.
///
/// Reference: `tests/fixtures/fields/date_field.hwpx`. The HWP5 ctrl_id `%smr`
/// is shared by all SUMMERY auto-fields; discrimination is via the `Command`
/// `$token`. Token mapping verified against 한컴 native fixtures in Wave 12n
/// (see `.docs/research/2026-06-02_auto_field_wire_dump.md`).
pub(super) fn build_summary_field_xml(
    field_type: &hwpforge_foundation::FieldType,
    hint: &str,
    name: &str,
    cached_value: &str,
    char_pr_id_ref: u32,
    begin_id: u64,
) -> String {
    use hwpforge_foundation::FieldType;
    let command = field_type.summary_token().expect("caller guards SUMMERY variants");
    // #120/#136 (supersedes Wave 12n Step 6.6): carry the cached resolved
    // value in the body.
    //
    // History: Step 6.6 emitted an EMPTY body after observing that a
    // *synthesized* ISO date (`2026-06-06`) — locale-mismatched against
    // 한컴's `2026년 6월 …` — was treated as corrupted content and
    // triggered the "낮은 보안 수준 복구" warning. But an empty body ALSO
    // triggers it (#120 stayed open). Byte-diff + 한컴 실측 (2026-06-13)
    // proved native 한컴 carries the verbatim locale value in the body and
    // opens cleanly; carrying the HWP5 source's own cached render (NOT a
    // synthesized value) reproduces that and closes the warning.
    //
    // Precedence: prefer the carried `cached_value` (from the HWP5
    // FieldBegin..FieldEnd span). Fall back to the caller-provided `hint`
    // for Author/LastSavedBy/Title built via the native HwpForge API, which
    // has no source span. Empty = none (Hancom recomputes editable fields).
    let display_text = if !cached_value.is_empty() {
        cached_value.to_string()
    } else {
        match field_type {
            FieldType::Author | FieldType::LastSavedBy | FieldType::Title if !hint.is_empty() => {
                hint.to_string()
            }
            FieldType::ClickHere => unreachable!("caller already routed ClickHere elsewhere"),
            _ => String::new(),
        }
    };
    // Wave 12p task #124: editable depends on whether Hancom recomputes
    // the field value (Author/Title → "0" lock to authored value;
    // LastSavedBy/CreatedTime/ModifiedTime → "1" Hancom recomputes).
    build_summary_run_xml_raw(
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
/// and `display` text. Used by [`build_summary_field_xml`] for typed
/// [`hwpforge_foundation::FieldType`] variants and by Wave 12n
/// `UnknownSummary` / `DateCodeField` fallback paths.
///
/// Wave 12n Step 6: `Control::PathField` no longer uses this builder.
/// See [`build_path_field_run_xml_raw`] for the native PATH wire shape.
pub(super) fn build_summary_run_xml_raw(
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
/// - cached body value (`cached_value`): the absolute path/file name 한컴
///   last evaluated. #120/#136 proved an empty body triggers the recovery
///   warning; carrying the verbatim source value closes it. 한컴 still
///   recomputes `$P$F` against the file's on-disk path on save.
pub(super) fn build_path_field_run_xml_raw(
    command: &str,
    cached_value: &str,
    char_pr_id_ref: u32,
    begin_id: u64,
) -> String {
    let escaped_cmd = escape_xml(command);
    // Wave 12n Step 6.5 / #120: the body between fieldBegin/fieldEnd carries
    // the cached resolved path (was a bare `<hp:t/>`; an empty body triggers
    // the "낮은 보안 수준 복구" warning — verified against
    // `sample-field-docsummary.hwpx` wire diff + 한컴 실측 2026-06-13).
    // A trailing `<hp:t/>` after fieldEnd is also required (its absence is a
    // separate trigger).
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
            r#"{display}"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldEnd beginIDRef="{bid}" fieldid="628121972"/>"#,
            r#"</hp:ctrl>"#,
            r#"<hp:t/>"#,
            r#"</hp:run>"#,
        ),
        cpr = char_pr_id_ref,
        bid = begin_id,
        cmd = escaped_cmd,
        display = build_text_element_xml(cached_value),
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
        // `Contents` → N2 wire code 2. Figure/Table/Eq/Outline 의 "캡션
        // 내용" 과 Bookmark 의 "책갈피 이름" 둘 다 N2=2 로 emit — 의미
        // 구분은 동반 RefType 가 carry (gotcha #27). E6 슬라이스 B 에서
        // `BookmarkName` variant 를 `Contents` 로 흡수. (Bookmark+N2=1 =
        // 책갈피 본문/번호 는 위 `Number => 1` arm 이 처리.)
        Contents => 2,
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
        RefTarget::Object(id) => format!("#{id}"),
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
    // SummaryField (`%smr`=0x25736D72), PathField (`%pat`=0x25706174).
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

#[cfg(test)]
mod clickhere_body_tests {
    use super::*;

    /// 채워진 누름틀은 `display_text` 를 본문 `<hp:t>` 로 방출한다.
    /// `Direction` 힌트 파라미터는 값과 별개 축이므로 그대로 남는다
    /// (native fixture: fieldBegin 파라미터에 힌트, 본문에 값).
    #[test]
    fn clickhere_body_prefers_display_text_when_filled() {
        let xml = build_clickhere_field_xml(
            "회사 이메일을 입력하세요",
            "",
            "user_email",
            0,
            1_000_000_000,
            "hanyul@example.com",
        );
        assert!(
            xml.contains("<hp:t>hanyul@example.com</hp:t>"),
            "본문은 채워진 값이어야 한다: {xml}"
        );
        assert!(
            xml.contains(
                r#"<hp:stringParam name="Direction">회사 이메일을 입력하세요</hp:stringParam>"#
            ),
            "Direction 힌트는 그대로 남아야 한다: {xml}"
        );
    }

    /// 미채움(`display_text` 빈 문자열)은 기존과 동일하게 힌트를 본문으로
    /// 방출한다 — HWP5→HWPX 변환 산출물 byte-중립 게이트
    /// (HWP5 wire 의 ClickHere span 은 항상 비어 있음, projection/mod.rs).
    #[test]
    fn clickhere_body_falls_back_to_hint_when_unfilled() {
        let xml = build_clickhere_field_xml(
            "회사 이메일을 입력하세요",
            "",
            "user_email",
            0,
            1_000_000_000,
            "",
        );
        assert!(
            xml.contains("<hp:t>회사 이메일을 입력하세요</hp:t>"),
            "미채움은 힌트로 폴백해야 한다: {xml}"
        );
    }
}

#[cfg(test)]
mod bookmarkname_collapse_tests {
    use super::*;
    use hwpforge_foundation::{RefContentType, RefType};

    /// E6 슬라이스 B byte-불변 게이트: `BookmarkName` 흡수 후에도 Bookmark
    /// 의 "책갈피 이름"(`Contents`) 은 N2 wire code `2` 로 emit — 이전
    /// 분리됐던 variant 와 동일. golden 12-fixture 매트릭스는 content-type
    /// 출력을 단언하지 않으므로 이 직접 단언이 진짜 byte-중립 게이트다.
    #[test]
    fn bookmark_contents_emits_n2_wire_code_2() {
        assert_eq!(
            ref_content_type_wire_code(&RefType::Bookmark, &RefContentType::Contents),
            2,
            "Bookmark Contents (책갈피 이름) must emit N2=2"
        );
        // caption-content (Figure/Table/Eq/Outline) 도 동일 N2=2 — 의미
        // 구분은 RefType 가 carry (gotcha #27), wire 는 동일.
        assert_eq!(
            ref_content_type_wire_code(&RefType::Figure, &RefContentType::Contents),
            2,
            "Figure caption Contents must also emit N2=2"
        );
        // Display 불변도 함께 잠금.
        assert_eq!(RefContentType::Contents.to_string(), "OBJECT_TYPE_CONTENTS");
    }
}
