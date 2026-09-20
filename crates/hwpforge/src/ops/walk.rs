//! Section-scoped counting helpers for [`super::inspect`], plus the
//! caption-aware container-descent policy [`super::exchange`] shares with
//! them ([`ControlDescent`]/[`control_descent`]).
//!
//! Two traversal *policies* live here, deliberately kept apart rather than
//! unified into one generic walker — collapsing them is a silent value
//! change, not a simplification (see
//! `inspect_deep_counts_table_image_chart_nested_in_image_caption_and_master_page`
//! in `hwpforge-bindings-cli`'s `cli_integration.rs`, which pins the fixture
//! that would catch it):
//!
//! - [`object_counts`] — how many tables/images/text boxes/lines/
//!   rectangles/polygons a section's own HWPX XML part (`section-N.xml`)
//!   contains, at any nesting depth, generalizing the caption-aware walker
//!   `hwpforge::ops::exchange` uses for `LayoutCacheDropped` path discovery
//!   (`first_layout_cache_path` and friends) to *counting* instead of
//!   *finding the first one*. It descends into every kind of caption
//!   (table, image, textbox, line, rect, …), a shape group's children, and
//!   a memo's anchor run — everything the section's XML element tree
//!   physically contains — but not master pages, which HWPX stores in a
//!   separate part the section's own element tree never reaches.
//!
//!   **Charts are deliberately not one of the counted fields here**, even
//!   though `Control::Chart` exists and this module could match on it.
//!   `hwpforge-smithy-hwpx`'s decoder decodes `<hp:switch><hp:case>
//!   <hp:chart>` *only* inside the top-level section-paragraph loop
//!   (`decode_section`'s own `for switch in &hx_run.switches` block,
//!   `decoder/section.rs`) — chart is not one of the `HxRunChildKind`
//!   variants the generic, recursive `convert_paragraph`/run-content
//!   dispatch handles (unlike `Table`/`Picture`/`Rect`/`Line`/`Ellipse`/
//!   `Polygon`/`Curve`/`ConnectLine`/`Container`, which all are). A chart
//!   nested inside a table cell, a caption, a text box or any other
//!   non-top-level container therefore never becomes a `Control::Chart` at
//!   all on decode — confirmed by round-tripping (encode then re-decode)
//!   the exact document
//!   `inspect_deep_counts_table_image_chart_nested_in_image_caption_and_master_page`
//!   builds: the section XML genuinely contains the nested `<hp:chart>`
//!   element, but decoding it back produces zero `Control::Chart` runs.
//!   Counting `Control::Chart` here would silently undercount relative to
//!   the legacy raw-XML scan for exactly the documents that gap affects, so
//!   `charts` stays a raw scan in `hwpforge-bindings-cli`'s `inspect.rs`,
//!   the same way `ole_objects` already has to (Core has no HWPX decode
//!   support for `<hp:ole>` at all — see that file's doc).
//! - [`paragraph_counts`] — the legacy CLI scanner's narrower paragraph
//!   recursion (`analysis/deep_counts.rs`'s pre-migration
//!   `count_paragraphs_recursive`/`count_non_empty_paragraphs_recursive`,
//!   ported here unchanged in shape): body + header + footer paragraphs,
//!   recursing only into table cells and the paragraph content of
//!   `TextBox`/`Footnote`/`Endnote`/`Ellipse`/`Polygon`/`Memo` — **not**
//!   captions, **not** group children, **not** a memo's anchor run, **not**
//!   master pages. Every paragraph-bearing container this narrower policy
//!   skips is one [`object_counts`] still visits, which is exactly the
//!   asymmetry the pinned fixture exercises (a caption holding a table, an
//!   image and a chart, plus a master-page paragraph).
//!
//! [`ControlDescent`]/[`control_descent`] are a *third*, distinct thing: not
//! a traversal policy of their own, but the shared "which nested paragraph
//! lists does this `Control` expose" classification both [`object_counts`]
//! (via [`visit_control_for_objects`]) and `super::exchange`'s
//! `first_layout_cache_path` build their own, genuinely different walks on
//! top of — a table cell/caption or group-child recursion written down once
//! instead of twice, audit follow-up W6b finding 4 ("`exchange.rs` still has
//! its own layout-cache traversal while `walk.rs` reimplements the
//! overlapping policy"). What differs at each call site (a counting
//! increment here, an [`hwpforge_smithy_hwpx::PathSeg`] tag and an
//! encoder-mirroring group-child index there) stays local to that call
//! site; only the "what containers exist and what do they nest" fact is
//! shared.

use hwpforge_core::caption::Caption;
use hwpforge_core::control::Control;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::section::Section;
use hwpforge_core::table::Table;

// ── object counts (caption-aware, master-page-blind) ──────────────

/// Structural object counts for one section, matching the legacy CLI raw-XML
/// scanner's *scope* (captions included, master pages excluded) — see the
/// module doc.
///
/// "Matching scope" is not "matching values": this walks the *decoded* Core
/// tree, so an element the decoder accepts but cannot represent — an
/// `<hp:pic>` with no usable `binaryItemIDRef`, which `convert_picture`
/// (`hwpforge-smithy-hwpx/src/decoder/section.rs`) turns into `Ok(None)`
/// rather than an error, for example — silently undercounts here relative to
/// a genuine raw-XML element scan over the same bytes. `hwpforge-bindings-cli`
/// keeps its own `--json` `tables`/`images`/`text_boxes`/`lines`/
/// `rectangles`/`polygons` fields sourced from exactly such a raw scan
/// instead of from this module for that reason (see its `commands/
/// inspect.rs` doc) — this module's counts feed `InspectSection`'s
/// `tables_all`/`images_all`/… fields, a deliberately distinct scope
/// (see that struct's doc).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ObjectCounts {
    /// Tables, at any nesting depth (cells, captions, shape groups, memos).
    pub(crate) tables: usize,
    /// Images, at any nesting depth.
    pub(crate) images: usize,
    /// Text boxes ([`Control::TextBox`] — HWPX `<hp:rect>` with a nested
    /// `<hp:drawText>`).
    pub(crate) text_boxes: usize,
    /// Line drawing objects ([`Control::Line`]).
    pub(crate) lines: usize,
    /// Pure rectangles ([`Control::Rect`] — HWPX `<hp:rect>` *without* a
    /// nested `<hp:drawText>`; a text-bearing `<hp:rect>` decodes to
    /// [`Control::TextBox`] instead and counts under [`Self::text_boxes`],
    /// never both).
    pub(crate) rectangles: usize,
    /// Polygon drawing objects ([`Control::Polygon`]).
    pub(crate) polygons: usize,
}

/// Computes [`ObjectCounts`] for `section`.
///
/// Visits body, header and footer paragraphs; does not visit
/// `section.master_pages` (see the module doc — a separate HWPX XML part,
/// invisible to the scan this mirrors).
pub(crate) fn object_counts(section: &Section) -> ObjectCounts {
    let mut counts = ObjectCounts::default();
    visit_paragraphs_for_objects(&section.paragraphs, &mut counts);
    for header_or_footer in section.headers.iter().chain(section.footers.iter()) {
        visit_paragraphs_for_objects(&header_or_footer.paragraphs, &mut counts);
    }
    counts
}

fn visit_paragraphs_for_objects(paragraphs: &[Paragraph], counts: &mut ObjectCounts) {
    for paragraph in paragraphs {
        for run in &paragraph.runs {
            visit_run_for_objects(run, counts);
        }
    }
}

fn visit_run_for_objects(run: &Run, counts: &mut ObjectCounts) {
    match &run.content {
        RunContent::Table(table) => {
            counts.tables += 1;
            visit_table_for_objects(table, counts);
        }
        // The one gap `Section::for_each_paragraph` has on purpose (Core's
        // `image_caption_paragraphs_are_skipped_documents_known_gap` test) —
        // closing it here, for counting, is this module's reason to exist.
        RunContent::Image(image) => {
            counts.images += 1;
            visit_caption_for_objects(image.caption.as_ref(), counts);
        }
        RunContent::Control(control) => visit_control_for_objects(control, counts),
        // `RunContent` is `#[non_exhaustive]`, so a wildcard is required
        // regardless of Core's own match coverage.
        RunContent::Text(_) | RunContent::InlineText(_) => {}
        _ => {}
    }
}

fn visit_table_for_objects(table: &Table, counts: &mut ObjectCounts) {
    for row in &table.rows {
        for cell in &row.cells {
            visit_paragraphs_for_objects(&cell.paragraphs, counts);
        }
    }
    visit_caption_for_objects(table.caption.as_ref(), counts);
}

fn visit_caption_for_objects(caption: Option<&Caption>, counts: &mut ObjectCounts) {
    if let Some(caption) = caption {
        visit_paragraphs_for_objects(&caption.paragraphs, counts);
    }
}

fn visit_control_for_objects(control: &Control, counts: &mut ObjectCounts) {
    // Counting still needs the concrete variant — [`control_descent`]
    // describes only the shared *recursion*, not which counter (if any)
    // each variant bumps, so that part stays a direct match here.
    // `Control::Chart` is deliberately not counted at all — see the module
    // doc's "Charts are deliberately not one of the counted fields here"
    // paragraph.
    match control {
        Control::TextBox { .. } => counts.text_boxes += 1,
        Control::Polygon { .. } => counts.polygons += 1,
        Control::Line { .. } => counts.lines += 1,
        Control::Rect { .. } => counts.rectangles += 1,
        _ => {}
    }
    match control_descent(control) {
        ControlDescent::Body { paragraphs, caption } => {
            visit_paragraphs_for_objects(paragraphs, counts);
            visit_caption_for_objects(caption, counts);
        }
        ControlDescent::CaptionOnly(caption) => visit_caption_for_objects(caption, counts),
        ControlDescent::Footnote(paragraphs) | ControlDescent::Endnote(paragraphs) => {
            visit_paragraphs_for_objects(paragraphs, counts);
        }
        ControlDescent::Group(children) => {
            // Unlike `exchange.rs`'s `first_in_control` (which mirrors the
            // *encoder*'s `emitted_idx` to build a path the encoder would
            // recognise), counting has no path to build and no encoder
            // round-trip to mirror — every decoded child is really in the
            // input file, so every child is visited unconditionally. This is
            // exactly the policy [`ControlDescent::Group`]'s own doc says a
            // caller must apply for itself.
            for child in children {
                visit_control_for_objects(child, counts);
            }
        }
        ControlDescent::Memo { content, anchor_runs } => {
            visit_paragraphs_for_objects(content, counts);
            for run in anchor_runs {
                visit_run_for_objects(run, counts);
            }
        }
        ControlDescent::None => {}
    }
}

// ── shared container-descent policy (also used by `exchange::from_json`) ──

/// What one [`Control`] variant offers to the caption/paragraph-aware
/// container walk [`object_counts`] and `exchange::first_layout_cache_path`
/// both need — the *recursion structure* only, never what either walk does
/// once it gets there (a counting increment on this module's side, an
/// [`hwpforge_smithy_hwpx::PathSeg`] tag on `exchange`'s). Before this
/// existed, both files hand-wrote the same "which nested paragraph lists
/// does this control expose" policy independently — this is the one place
/// that policy is written down, so the two cannot drift apart silently the
/// way the module doc's opening paragraph warns two *different* policies
/// must not be collapsed into one.
pub(crate) enum ControlDescent<'a> {
    /// Paragraph content, then a caption if the control has one —
    /// `TextBox`/`Ellipse`/`Polygon`. All three share this shape (and, on
    /// `exchange`'s side, the same `PathSeg::TextBox` tag) even though only
    /// `TextBox`/`Polygon` bump a counter here.
    Body { paragraphs: &'a [Paragraph], caption: Option<&'a Caption> },
    /// A caption only, no body content of its own —
    /// `Line`/`Rect`/`Arc`/`Curve`/`ConnectLine`.
    CaptionOnly(Option<&'a Caption>),
    /// A footnote's body.
    Footnote(&'a [Paragraph]),
    /// An endnote's body — kept apart from [`Self::Footnote`] only because
    /// `exchange` tags the two with different `PathSeg` variants; this
    /// module's own counting treats them identically.
    Endnote(&'a [Paragraph]),
    /// A shape group's children, in source order. Unlike every other arm
    /// here, a caller must apply its own group-child-index policy
    /// (`object_counts` visits every child; `exchange` skips an index for a
    /// child the encoder never emits — see `exchange::group_child_is_emitted`)
    /// rather than getting one from this enum, because the two callers'
    /// policies genuinely differ and neither is "more correct" for the
    /// other's purpose.
    Group(&'a [Control]),
    /// A memo's visible content, plus its `anchor_runs` — the latter is a
    /// run list, not a paragraph list, because the encoder flattens it to
    /// plain text and never recurses into a non-text run there (see
    /// `exchange::first_in_control`'s memo arm); a caller that cares must
    /// walk `anchor_runs` itself.
    Memo { content: &'a [Paragraph], anchor_runs: &'a [Run] },
    /// Nothing this walk needs to see (`Chart`, `Equation`, `Field`, …).
    None,
}

/// Classifies `control` for [`ControlDescent`] — the shared recursion
/// policy [`object_counts`] and `exchange::first_layout_cache_path` both
/// build on. See [`ControlDescent`]'s own doc for what each arm means.
pub(crate) fn control_descent(control: &Control) -> ControlDescent<'_> {
    match control {
        Control::TextBox { paragraphs, caption, .. }
        | Control::Ellipse { paragraphs, caption, .. }
        | Control::Polygon { paragraphs, caption, .. } => {
            ControlDescent::Body { paragraphs, caption: caption.as_ref() }
        }
        Control::Line { caption, .. }
        | Control::Rect { caption, .. }
        | Control::Arc { caption, .. }
        | Control::Curve { caption, .. }
        | Control::ConnectLine { caption, .. } => ControlDescent::CaptionOnly(caption.as_ref()),
        Control::Footnote { paragraphs, .. } => ControlDescent::Footnote(paragraphs),
        Control::Endnote { paragraphs, .. } => ControlDescent::Endnote(paragraphs),
        Control::Group { children, .. } => ControlDescent::Group(children),
        Control::Memo { content, anchor_runs, .. } => ControlDescent::Memo { content, anchor_runs },
        // `Control` is `#[non_exhaustive]`; every caller needs a wildcard
        // regardless of Core's own match coverage.
        _ => ControlDescent::None,
    }
}

// ── paragraph counts (legacy scanner's narrower recursion) ────────

/// Paragraph counts for one section, matching the legacy CLI scanner's
/// narrower recursion — see the module doc.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ParagraphCounts {
    /// Body-flow paragraphs (`section.paragraphs`, no recursion) with
    /// visible text, per [`paragraph_has_visible_text`].
    pub(crate) non_empty_paragraphs: usize,
    /// Body + header + footer paragraphs, recursing into table cells and
    /// `TextBox`/`Footnote`/`Endnote`/`Ellipse`/`Polygon`/`Memo` content.
    pub(crate) deep_paragraphs: usize,
    /// Same recursion as [`Self::deep_paragraphs`], counting only the
    /// paragraphs with visible text.
    pub(crate) deep_non_empty_paragraphs: usize,
}

/// Computes [`ParagraphCounts`] for `section`.
pub(crate) fn paragraph_counts(section: &Section) -> ParagraphCounts {
    let non_empty_paragraphs: usize =
        section.paragraphs.iter().filter(|paragraph| paragraph_has_visible_text(paragraph)).count();

    let deep_paragraphs: usize = count_paragraphs_narrow(&section.paragraphs)
        + section
            .headers
            .iter()
            .map(|header| count_paragraphs_narrow(&header.paragraphs))
            .sum::<usize>()
        + section
            .footers
            .iter()
            .map(|footer| count_paragraphs_narrow(&footer.paragraphs))
            .sum::<usize>();

    let deep_non_empty_paragraphs: usize = count_non_empty_paragraphs_narrow(&section.paragraphs)
        + section
            .headers
            .iter()
            .map(|header| count_non_empty_paragraphs_narrow(&header.paragraphs))
            .sum::<usize>()
        + section
            .footers
            .iter()
            .map(|footer| count_non_empty_paragraphs_narrow(&footer.paragraphs))
            .sum::<usize>();

    ParagraphCounts { non_empty_paragraphs, deep_paragraphs, deep_non_empty_paragraphs }
}

/// Whether `paragraph` (or content nested one recursion step inside it —
/// e.g. a table cell) carries visible text. Ported from
/// `analysis/deep_counts.rs`'s `paragraph_has_visible_text_deep` /
/// `first_visible_text_in_*_deep` family (bool instead of the first-match
/// text preview, which `inspect` has no field for).
pub(crate) fn paragraph_has_visible_text(paragraph: &Paragraph) -> bool {
    paragraph.runs.iter().any(run_has_visible_text)
}

fn run_has_visible_text(run: &Run) -> bool {
    match &run.content {
        RunContent::Text(_) | RunContent::InlineText(_) => {
            run.content.plain_text().is_some_and(|text| !text.trim().is_empty())
        }
        RunContent::Image(_) => false,
        RunContent::Table(table) => table
            .rows
            .iter()
            .flat_map(|row| row.cells.iter())
            .any(|cell| cell.paragraphs.iter().any(paragraph_has_visible_text)),
        RunContent::Control(control) => control_has_visible_text(control),
        _ => false,
    }
}

fn control_has_visible_text(control: &Control) -> bool {
    match control {
        Control::TextBox { paragraphs, .. }
        | Control::Footnote { paragraphs, .. }
        | Control::Endnote { paragraphs, .. }
        | Control::Ellipse { paragraphs, .. }
        | Control::Polygon { paragraphs, .. } => paragraphs.iter().any(paragraph_has_visible_text),
        Control::Memo { content, .. } => content.iter().any(paragraph_has_visible_text),
        Control::Hyperlink { text, .. } => !text.trim().is_empty(),
        _ => false,
    }
}

fn count_paragraphs_narrow(paragraphs: &[Paragraph]) -> usize {
    paragraphs
        .iter()
        .map(|paragraph| 1 + paragraph.runs.iter().map(count_runs_paragraphs_narrow).sum::<usize>())
        .sum()
}

fn count_non_empty_paragraphs_narrow(paragraphs: &[Paragraph]) -> usize {
    paragraphs
        .iter()
        .map(|paragraph| {
            let current: usize = usize::from(paragraph_has_visible_text(paragraph));
            current
                + paragraph.runs.iter().map(count_runs_non_empty_paragraphs_narrow).sum::<usize>()
        })
        .sum()
}

fn count_runs_paragraphs_narrow(run: &Run) -> usize {
    match &run.content {
        RunContent::Text(_) | RunContent::InlineText(_) | RunContent::Image(_) => 0,
        RunContent::Table(table) => table
            .rows
            .iter()
            .flat_map(|row| row.cells.iter())
            .map(|cell| count_paragraphs_narrow(&cell.paragraphs))
            .sum(),
        RunContent::Control(control) => count_control_paragraphs_narrow(control),
        _ => 0,
    }
}

fn count_runs_non_empty_paragraphs_narrow(run: &Run) -> usize {
    match &run.content {
        RunContent::Text(_) | RunContent::InlineText(_) | RunContent::Image(_) => 0,
        RunContent::Table(table) => table
            .rows
            .iter()
            .flat_map(|row| row.cells.iter())
            .map(|cell| count_non_empty_paragraphs_narrow(&cell.paragraphs))
            .sum(),
        RunContent::Control(control) => count_control_non_empty_paragraphs_narrow(control),
        _ => 0,
    }
}

fn count_control_paragraphs_narrow(control: &Control) -> usize {
    match control {
        Control::TextBox { paragraphs, .. }
        | Control::Footnote { paragraphs, .. }
        | Control::Endnote { paragraphs, .. }
        | Control::Ellipse { paragraphs, .. }
        | Control::Polygon { paragraphs, .. } => count_paragraphs_narrow(paragraphs),
        Control::Memo { content, .. } => count_paragraphs_narrow(content),
        _ => 0,
    }
}

fn count_control_non_empty_paragraphs_narrow(control: &Control) -> usize {
    match control {
        Control::TextBox { paragraphs, .. }
        | Control::Footnote { paragraphs, .. }
        | Control::Endnote { paragraphs, .. }
        | Control::Ellipse { paragraphs, .. }
        | Control::Polygon { paragraphs, .. } => count_non_empty_paragraphs_narrow(paragraphs),
        Control::Memo { content, .. } => count_non_empty_paragraphs_narrow(content),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_core::caption::{Caption, CaptionSide};
    use hwpforge_core::chart::{ChartData, ChartGrouping, ChartType, LegendPosition};
    use hwpforge_core::image::Image;
    use hwpforge_core::page::PageSettings;
    use hwpforge_core::run::Run;
    use hwpforge_core::section::MasterPage;
    use hwpforge_core::table::{Table, TableCell, TableRow};
    use hwpforge_foundation::{ApplyPageType, CharShapeIndex, HwpUnit, ParaShapeIndex};

    fn text_para(text: &str) -> Paragraph {
        Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
    }

    /// Regression lock for the module's whole reason to exist: a table, an
    /// image and a chart nested inside an *image's* caption are visible to
    /// [`object_counts`] (unlike `Section::for_each_paragraph`), and a
    /// master-page paragraph is invisible to it (unlike
    /// `Section::for_each_paragraph`) — mirrors
    /// `inspect_deep_counts_table_image_chart_nested_in_image_caption_and_master_page`
    /// in `hwpforge-bindings-cli`'s integration tests, at the `ops` layer.
    #[test]
    fn object_counts_sees_image_caption_but_not_master_pages() {
        use hwpforge_core::control::Control;

        let nested_table = Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![text_para("caption-table-cell")],
            HwpUnit::from_pt(100.0).unwrap(),
        )])]);
        let nested_image = Image::new(
            "BinData/nested.png",
            HwpUnit::from_pt(10.0).unwrap(),
            HwpUnit::from_pt(10.0).unwrap(),
            hwpforge_core::image::ImageFormat::Png,
        );
        let nested_chart = Control::Chart {
            chart_type: ChartType::Bar,
            data: ChartData::category(&["A", "B"], &[("Series1", [1.0, 2.0].as_slice())]),
            width: HwpUnit::new(10000).unwrap(),
            height: HwpUnit::new(8000).unwrap(),
            title: None,
            legend: LegendPosition::default(),
            grouping: ChartGrouping::Clustered,
            bar_shape: None,
            explosion: None,
            of_pie_type: None,
            radar_style: None,
            wireframe: None,
            bubble_3d: None,
            scatter_style: None,
            show_markers: None,
            stock_variant: None,
        };
        let mut caption_table_para = text_para("caption-table-host");
        caption_table_para.add_run(Run::table(nested_table, CharShapeIndex::new(0)));
        let mut caption_image_para = text_para("caption-image-host");
        caption_image_para.add_run(Run::image(nested_image, CharShapeIndex::new(0)));
        let mut caption_chart_para = text_para("caption-chart-host");
        caption_chart_para.add_run(Run::control(nested_chart, CharShapeIndex::new(0)));

        let mut host_image = Image::new(
            "BinData/host.png",
            HwpUnit::from_pt(10.0).unwrap(),
            HwpUnit::from_pt(10.0).unwrap(),
            hwpforge_core::image::ImageFormat::Png,
        );
        host_image.caption = Some(Caption::new(
            vec![
                text_para("caption text"),
                caption_table_para,
                caption_image_para,
                caption_chart_para,
            ],
            CaptionSide::Bottom,
        ));

        let mut host = text_para("host");
        host.add_run(Run::image(host_image, CharShapeIndex::new(0)));

        let mut section = Section::with_paragraphs(vec![host], PageSettings::a4());
        section.master_pages =
            Some(vec![MasterPage::new(ApplyPageType::Both, vec![text_para("master-page-para")])]);

        // `nested_chart`/`caption_chart_para` are still nested into the same
        // caption as the table and image (matching the CLI integration
        // fixture this test mirrors) to prove `object_counts` tolerates a
        // `Control::Chart` in its input without crashing or double-counting
        // anything else — it is just silently not one of the counted
        // fields (see the module doc's chart paragraph, and
        // [`chart_nested_in_a_caption_is_a_documented_decoder_gap`] for why
        // a decode-based chart count would be unsafe to offer at all).
        let counts = object_counts(&section);
        assert_eq!(counts.images, 2, "host image + caption-nested image");
        assert_eq!(counts.tables, 1);

        let paragraphs = paragraph_counts(&section);
        // The master-page paragraph never enters this scope (headers/footers
        // + body only), and the caption's nested paragraphs never enter it
        // either (captions are outside the narrow recursion) — only the
        // one body paragraph counts.
        assert_eq!(paragraphs.deep_paragraphs, 1);
    }

    /// Documents *why* [`ObjectCounts`] has no `charts` field, with a
    /// reproduction rather than just a claim: a `Control::Chart` nested
    /// inside an image's caption really does reach the encoded HWPX bytes
    /// (`<hp:switch><hp:case><hp:chart>` is genuinely in the section XML —
    /// asserted below, not assumed), but re-decoding those same bytes
    /// produces zero `Control::Chart` runs anywhere in the section.
    /// `hwpforge-smithy-hwpx`'s `decode_section` only extracts
    /// `<hp:switch>`/`<hp:chart>` in its own top-level-paragraph loop —
    /// chart is not one of the `HxRunChildKind` variants the recursive
    /// `convert_paragraph`/run-content dispatch (used for every non-
    /// top-level container: cells, captions, text boxes, …) handles. If a
    /// future decoder change closes this gap, this test starts failing at
    /// the `assert!(!has_chart, …)` below, which is the signal to add
    /// `charts`/`charts_all` back to [`ObjectCounts`]/`InspectSection`.
    #[test]
    fn chart_nested_in_a_caption_is_a_documented_decoder_gap() {
        use hwpforge_core::control::Control;
        use hwpforge_core::image::{Image, ImageFormat, ImageStore};
        use hwpforge_core::run::RunContent;
        use hwpforge_core::Document;
        use hwpforge_smithy_hwpx::style_store::{
            HwpxCharShape, HwpxFont, HwpxParaShape, HwpxStyleStore,
        };
        use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder, PackageReader};

        let mut store = HwpxStyleStore::new();
        for &lang in &["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"] {
            store.push_font(HwpxFont::new(0, "함초롬돋움", lang));
        }
        store.push_char_shape(HwpxCharShape::default());
        store.push_para_shape(HwpxParaShape::default());

        let nested_chart = Control::Chart {
            chart_type: ChartType::Bar,
            data: ChartData::category(&["A", "B"], &[("Series1", [1.0, 2.0].as_slice())]),
            width: HwpUnit::new(10000).unwrap(),
            height: HwpUnit::new(8000).unwrap(),
            title: None,
            legend: LegendPosition::default(),
            grouping: ChartGrouping::Clustered,
            bar_shape: None,
            explosion: None,
            of_pie_type: None,
            radar_style: None,
            wireframe: None,
            bubble_3d: None,
            scatter_style: None,
            show_markers: None,
            stock_variant: None,
        };
        let mut caption_chart_para = text_para("caption-chart-host");
        caption_chart_para.add_run(Run::control(nested_chart, CharShapeIndex::new(0)));

        let mut host_image = Image::new(
            "BinData/host.png",
            HwpUnit::from_pt(10.0).unwrap(),
            HwpUnit::from_pt(10.0).unwrap(),
            ImageFormat::Png,
        );
        host_image.caption = Some(Caption::new(
            vec![text_para("caption text"), caption_chart_para],
            CaptionSide::Bottom,
        ));

        let mut host = text_para("host");
        host.add_run(Run::image(host_image, CharShapeIndex::new(0)));
        let section = Section::with_paragraphs(vec![host], PageSettings::a4());

        let mut doc = Document::new();
        doc.add_section(section);
        let validated = doc.validate().expect("validate");
        let bytes = HwpxEncoder::encode(&validated, &store, &ImageStore::new()).expect("encode");

        let mut package_reader = PackageReader::new(&bytes).expect("open package");
        let section_xml = package_reader.read_section_xml(0).expect("read section xml");
        assert!(
            section_xml.contains("<hp:chart"),
            "the encoder really did write the nested chart into the section XML: {section_xml}"
        );

        let decoded = HwpxDecoder::decode(&bytes).expect("decode");
        let has_chart = decoded.document.sections()[0].paragraphs.iter().any(|p| {
            p.runs.iter().any(|run| {
                matches!(&run.content, RunContent::Image(image) if image.caption.as_ref().is_some_and(|c| {
                    c.paragraphs.iter().any(|cp| cp.runs.iter().any(|r| matches!(&r.content, RunContent::Control(c) if matches!(**c, Control::Chart { .. }))))
                }))
            })
        });
        assert!(
            !has_chart,
            "decoder now reconstructs a nested chart — see this test's doc: \
             remove `charts` from ObjectCounts's excluded-fields note and add it back as a counted field"
        );
    }

    #[test]
    fn rect_and_textbox_never_double_count() {
        use hwpforge_core::control::Control;

        let mut para = text_para("host");
        para.add_run(Run::control(
            Control::Rect {
                width: HwpUnit::new(1000).unwrap(),
                height: HwpUnit::new(1000).unwrap(),
                placement: None,
                caption: None,
                style: None,
            },
            CharShapeIndex::new(0),
        ));
        para.add_run(Run::control(
            Control::TextBox {
                paragraphs: vec![text_para("inside box")],
                width: HwpUnit::new(1000).unwrap(),
                height: HwpUnit::new(1000).unwrap(),
                placement: None,
                caption: None,
                style: None,
                text_vertical_align: hwpforge_foundation::VerticalAlign::Top,
            },
            CharShapeIndex::new(0),
        ));

        let section = Section::with_paragraphs(vec![para], PageSettings::a4());
        let counts = object_counts(&section);
        assert_eq!(counts.rectangles, 1);
        assert_eq!(counts.text_boxes, 1);
    }

    #[test]
    fn non_empty_paragraphs_is_shallow_but_probes_deep() {
        let mut host = Paragraph::new(ParaShapeIndex::new(0));
        let table = Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![text_para("cell text")],
            HwpUnit::from_pt(100.0).unwrap(),
        )])]);
        host.add_run(Run::table(table, CharShapeIndex::new(0)));
        let empty = Paragraph::new(ParaShapeIndex::new(0));

        let section = Section::with_paragraphs(vec![host, empty], PageSettings::a4());
        let counts = paragraph_counts(&section);
        // The host paragraph has no direct text but its cell does — the
        // deep probe finds it without recursing into the second paragraph.
        assert_eq!(counts.non_empty_paragraphs, 1);
    }
}
