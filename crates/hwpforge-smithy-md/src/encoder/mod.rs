//! Core -> Markdown encoders.

pub(crate) mod guard;
#[cfg(test)]
mod guarded_tests;
mod list_format;
mod lossless;
mod lossy;
mod markup;
#[cfg(test)]
mod markup_safety_tests;
mod styled;

use hwpforge_blueprint::template::Template;
use hwpforge_core::{Document, StyleLookup, Validated};

use crate::error::MdResult;

pub use styled::MdOutput;

/// Markdown encoder entrypoint.
pub struct MdEncoder;

/// Warning emitted by the markdown bridge in **either direction** — lossy
/// markdown encoders (HWPX→MD) and the MD→HWPX image embed loader
/// ([`crate::embed`]). Warning-first: what the output can no longer
/// represent is reported, not silently dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MdWarning {
    /// A heading uses a separate paragraph shape to preserve an explicit list definition.
    HeadingListConflict {
        /// Markdown heading level.
        level: u8,
        /// Selected template style.
        style_name: String,
    },
    /// Markdown has no representation for the source paragraph measure.
    ParagraphMeasureLost {
        /// Paragraph definition index.
        definition: usize,
        /// Margin field or referenced tab position that cannot be represented.
        field: String,
        /// Original semantic value, including its unit.
        measure: hwpforge_core::ParagraphMeasure,
    },
    /// A table with merged cells was flattened into a plain GFM pipe grid
    /// (spans are not representable there). The style-aware encoder
    /// (`encode_styled`) renders such tables as HTML with colspan/rowspan
    /// instead and does not emit this warning.
    MergedCellsFlattened {
        /// Number of cells in the table with `col_span > 1` or `row_span > 1`.
        merged_cells: usize,
    },
    /// MD→HWPX: an image reference could not be embedded — the
    /// image run is dropped so no dangling `binaryItemIDRef` reaches the
    /// package, and the exclusion is declared here.
    ImageEmbedSkipped {
        /// The markdown source string (truncated for display; `data:` URIs
        /// are cut at 64 chars).
        src: String,
        /// Why the reference was excluded.
        reason: crate::embed::ImageEmbedSkipReason,
    },
    /// HWPX→MD (lossy view): a container nested deeper than the view draws
    /// was left out and a notice written in its place — past 8 tables nested
    /// in cells (`(중첩 표 생략)`). The styled view never reports it: Core
    /// validation keeps a validated document within the depth it draws.
    ContainerTooDeep {
        /// What was left out: `"table"`.
        container: &'static str,
        /// The nesting limit that was reached.
        limit: usize,
        /// Index of the section holding it.
        section: usize,
        /// Index of the section's top-level paragraph holding it.
        paragraph: usize,
    },
    /// HWPX→MD (styled view, Markdown text): an equation whose LaTeX cannot
    /// be written in Markdown safely — it would end its `$...$` early or
    /// start a link — was written as its script text instead.
    EquationAsText {
        /// Index of the section holding it.
        section: usize,
        /// Index of the section's top-level paragraph holding it.
        paragraph: usize,
    },
    /// HWPX→MD (readable views): the finished Markdown, parsed again, held
    /// markup the encoder does not write (raw HTML, a link or image it did
    /// not make, a link reference definition) in this top-level paragraph,
    /// so the paragraph was written as plain text, every punctuation mark
    /// escaped. The escaping rules are meant to make this never happen.
    MarkupWrittenAsText {
        /// Index of the section holding it.
        section: usize,
        /// Index of the section's top-level paragraph.
        paragraph: usize,
    },
    /// MD→HWPX: a comment the styled view writes in place of content it
    /// cannot show was left out — a chart (`<!-- chart -->`, its data is
    /// not in the Markdown) or a memo (`<!-- memo: … -->`, its body is not
    /// read back). One per comment, in source order.
    ContentLost {
        /// What was left out: `"chart"` or `"memo"`.
        kind: &'static str,
    },
}

impl core::fmt::Display for MdWarning {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::HeadingListConflict { level, style_name } => write!(f, "heading {level} conflicts with explicit list in style {style_name}; preserving entry definition"),
            Self::ParagraphMeasureLost { definition, field, measure } => write!(
                f, "paragraph definition {definition} {field} {measure:?} cannot be represented in Markdown"
            ),
            Self::MergedCellsFlattened { merged_cells } => write!(
                f,
                "table with {merged_cells} merged cell(s) flattened to a plain GFM grid \
                 (use styled mode to keep spans as HTML)"
            ),
            Self::ImageEmbedSkipped { src, reason } => {
                write!(f, "image \"{src}\" not embedded ({reason}) — image dropped")
            }
            Self::ContainerTooDeep { container, limit, section, paragraph } => write!(
                f,
                "{container} nested deeper than {limit} levels left out \
                 (section {section}, paragraph {paragraph})"
            ),
            Self::MarkupWrittenAsText { section, paragraph } => write!(
                f,
                "paragraph written as plain text: its Markdown held markup the encoder \
                 does not write (section {section}, paragraph {paragraph})"
            ),
            Self::ContentLost { kind } => write!(
                f,
                "{kind} comment left out: its content is not read back from Markdown"
            ),
            Self::EquationAsText { section, paragraph } => write!(
                f,
                "equation written as its script text, its LaTeX cannot be written \
                 in Markdown safely (section {section}, paragraph {paragraph})"
            ),
        }
    }
}

impl MdEncoder {
    /// Encodes a validated document into markdown with frontmatter.
    ///
    /// This method is mapping-aware and uses the provided template to map
    /// paragraph style IDs back into markdown semantics.
    pub fn encode(document: &Document<Validated>, template: &Template) -> MdResult<String> {
        lossy::encode_with_template(document, template)
    }

    /// Encodes a validated document into readable markdown without template mapping.
    pub fn encode_lossy(document: &Document<Validated>) -> MdResult<String> {
        lossy::encode_without_template(document)
    }

    /// Like [`MdEncoder::encode_lossy`], additionally reporting what the
    /// lossy rendering dropped (e.g. merged table cells flattened to GFM).
    pub fn encode_lossy_with_report(
        document: &Document<Validated>,
    ) -> MdResult<(String, Vec<MdWarning>)> {
        lossy::encode_without_template_report(document)
    }

    /// Like [`MdEncoder::encode`], additionally reporting what the lossy
    /// rendering dropped.
    pub fn encode_with_report(
        document: &Document<Validated>,
        template: &Template,
    ) -> MdResult<(String, Vec<MdWarning>)> {
        lossy::encode_with_template_report(document, template)
    }

    /// Encodes a validated document into lossless markdown (frontmatter + HTML-like body).
    ///
    /// The output is a machine-oriented view for checking what the
    /// preservation scope keeps. Its frontmatter always carries
    /// `metadata.format: lossless`, and [`crate::MdDecoder::decode`] refuses
    /// it; round-trip edits of a document go through JSON.
    pub fn encode_lossless(document: &Document<Validated>) -> MdResult<String> {
        lossless::encode_lossless(document)
    }

    /// Encodes a validated document into style-aware markdown.
    ///
    /// Queries the provided [`StyleLookup`] for character/paragraph/style
    /// properties to emit inline formatting (bold, italic, strikeout),
    /// heading markers, and extracted images.
    pub fn encode_styled(document: &Document<Validated>, styles: &dyn StyleLookup) -> MdOutput {
        styled::encode_styled(document, styles)
    }
}

/// A caption number as text: the number Hancom cached,
/// between its prefix and suffix characters. Markdown writes the number in
/// digits whatever its number shape (a lossy view; only DIGIT occurs in the
/// corpus).
pub(crate) fn auto_number_text(
    num: u32,
    format: &hwpforge_core::control::AutoNumberFormat,
) -> String {
    let mut out = String::new();
    out.extend(format.prefix);
    out.push_str(&num.to_string());
    out.extend(format.suffix);
    out
}

/// The parser options the Markdown decoder reads with. The styled view's
/// final check parses with the same ones, so what it lets through is what
/// the decoder reads (`guard`).
pub(crate) fn md_options() -> pulldown_cmark::Options {
    use pulldown_cmark::Options;
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_DEFINITION_LIST
        | Options::ENABLE_GFM
}

/// The text the styled view shows for a control whose text is not its own
/// document text: a cross reference without display text shows its target
/// in brackets, a field its value, else its hint, else `____`, an unknown
/// control `[tag]`. The styled renderers and the styled plain-text fallback
/// (`guard::fallback_markdown`) both use it, so a fallback keeps what the view
/// showed.
/// 이것을 실패시키는 것: 갈래 하나씩 삭제(`fallback_text_shows_what_styled_shows`).
pub(crate) fn styled_display(
    control: &hwpforge_core::Control,
) -> Option<std::borrow::Cow<'_, str>> {
    use hwpforge_core::Control;
    match control {
        Control::CrossRef { target, display_text, .. } => Some(if display_text.is_empty() {
            format!("[{}]", target.as_display()).into()
        } else {
            display_text.as_str().into()
        }),
        Control::Field { hint_text, display_text, .. } => Some(if display_text.is_empty() {
            hint_text.as_deref().unwrap_or("____").into()
        } else {
            display_text.as_str().into()
        }),
        Control::Unknown { tag, .. } => Some(format!("[{tag}]").into()),
        _ => None,
    }
}

/// A memo's anchor runs as a paragraph: styled draws a memo control still
/// standing after `expand_memos` (one in another memo's anchor) this way,
/// one container deeper, and its plain-text fallback reads it the same way
/// (`guard::fallback_markdown`).
pub(crate) fn memo_anchor(anchor_runs: &[hwpforge_core::Run]) -> hwpforge_core::Paragraph {
    hwpforge_core::Paragraph::with_runs(
        anchor_runs.to_vec(),
        hwpforge_foundation::ParaShapeIndex::new(0),
    )
}

/// Whether a link may be written as a link by the readable views. The same
/// schemes the Markdown decoder accepts — `http://`, `https://`, `mailto:`
/// (any case), and the empty URL as it does — and nothing that a browser
/// would strip before reading the
/// scheme: a URL holding whitespace or a control character anywhere is
/// refused, so `" javascript:…"` or `"java\tscript:…"` cannot pass as
/// another scheme. A refused link keeps only its visible text.
/// 이것을 실패시키는 것: 공백·제어 문자 검사 삭제 · 허용 목록을 금지 목록으로
/// (`link_urls_follow_the_allow_list`).
pub(crate) fn link_url_allowed(url: &str) -> bool {
    if url.is_empty() {
        return true;
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    let lower = url.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:")
}

/// One drawn piece of a paragraph, borrowed from the document: a run's
/// content, or a group child standing in its group's place.
#[derive(Clone, Copy)]
pub(crate) enum Part<'a> {
    /// A text run (`Text` or `InlineText`).
    Text(&'a hwpforge_core::RunContent),
    /// A picture: a picture run or a group's picture child.
    Image(&'a hwpforge_core::image::Image),
    /// A table run.
    Table(&'a hwpforge_core::Table),
    /// A control run or a group's shape child.
    Control(&'a hwpforge_core::Control),
}

/// The runs of a paragraph as the readable views draw them: a group run
/// (묶음) stands for its children in z-order — a picture child as a
/// picture, a shape child as that control, a nested group recursively —
/// each with the group run's char shape. Everything is borrowed (no
/// subtree is copied) and nested groups are walked with an explicit stack:
/// no recursion, and an auxiliary stack of one entry per open group level.
/// 이것을 실패시키는 것: 그림 갈래 삭제 · 중첩 재귀 삭제 · 자식 순서 뒤집기 ·
/// 복사로 되돌리기(`lossy_group_children_in_order`, `styled_group_children_in_order`,
/// `group_children_are_borrowed`).
pub(crate) fn spread_groups(
    runs: &[hwpforge_core::Run],
) -> impl Iterator<Item = (Part<'_>, hwpforge_foundation::CharShapeIndex)> {
    use hwpforge_core::control::GroupChild;
    use hwpforge_core::{Control, Run, RunContent};
    use hwpforge_foundation::CharShapeIndex;
    struct Spread<'a> {
        runs: std::slice::Iter<'a, Run>,
        groups: Vec<(std::slice::Iter<'a, GroupChild>, CharShapeIndex)>,
    }
    impl<'a> Iterator for Spread<'a> {
        type Item = (Part<'a>, CharShapeIndex);
        fn next(&mut self) -> Option<Self::Item> {
            loop {
                if let Some((children, cs)) = self.groups.last_mut() {
                    let cs = *cs;
                    match children.next() {
                        None => {
                            self.groups.pop();
                        }
                        Some(GroupChild::Picture(image)) => return Some((Part::Image(image), cs)),
                        Some(GroupChild::Shape(Control::Group { children, .. })) => {
                            self.groups.push((children.iter(), cs));
                        }
                        Some(GroupChild::Shape(shape)) => return Some((Part::Control(shape), cs)),
                        // A future child kind is not drawn.
                        Some(_) => {}
                    }
                    continue;
                }
                let run = self.runs.next()?;
                let cs = run.char_shape_id;
                match &run.content {
                    RunContent::Control(c) => match c.as_ref() {
                        Control::Group { children, .. } => self.groups.push((children.iter(), cs)),
                        other => return Some((Part::Control(other), cs)),
                    },
                    RunContent::Image(image) => return Some((Part::Image(image), cs)),
                    RunContent::Table(table) => return Some((Part::Table(table), cs)),
                    content => return Some((Part::Text(content), cs)),
                }
            }
        }
    }
    Spread { runs: runs.iter(), groups: Vec::new() }
}

#[cfg(test)]
mod spread_tests {
    use super::*;
    use hwpforge_core::control::{Control, GroupChild};
    use hwpforge_core::image::{Image, ImageFormat};
    use hwpforge_core::{ObjectFrame, Run};
    use hwpforge_foundation::{CharShapeIndex, HwpUnit};

    /// Group children come out in z-order, each borrowed from the tree
    /// itself (the same address — nothing is copied), nested groups walked
    /// without recursion; a run that is not a group comes out as it is.
    ///
    /// 이것을 실패시키는 것: 자식을 복사해 내보내기 · 자식 순서 뒤집기 · 중첩 묶음 건너뛰기.
    #[test]
    fn group_children_are_borrowed() {
        let u = |v| HwpUnit::new(v).unwrap();
        let group = |children| Control::Group {
            children,
            width: u(10),
            height: u(10),
            frame: ObjectFrame::new(),
            placement: None,
            inst_id: None,
        };
        let mut inner =
            group(vec![GroupChild::Picture(Image::new("b", u(1), u(1), ImageFormat::Png))]);
        for _ in 0..200 {
            inner = group(vec![GroupChild::Shape(inner)]);
        }
        let runs = vec![
            Run::text("t", CharShapeIndex::new(1)),
            Run::control(
                group(vec![
                    GroupChild::Picture(Image::new("a", u(1), u(1), ImageFormat::Png)),
                    GroupChild::Shape(inner),
                    GroupChild::Shape(Control::equation("x")),
                ]),
                CharShapeIndex::new(7),
            ),
        ];
        let parts: Vec<_> = spread_groups(&runs).collect();
        assert_eq!(parts.len(), 4);
        assert!(
            matches!(parts[0], (Part::Text(c), cs) if std::ptr::eq(c, &runs[0].content) && cs == CharShapeIndex::new(1))
        );
        let hwpforge_core::RunContent::Control(top) = &runs[1].content else { panic!() };
        let Control::Group { children, .. } = top.as_ref() else { panic!() };
        let GroupChild::Picture(a) = &children[0] else { panic!() };
        assert!(
            matches!(parts[1], (Part::Image(i), cs) if std::ptr::eq(i, a) && cs == CharShapeIndex::new(7))
        );
        let mut node = &children[1];
        let deepest = loop {
            match node {
                GroupChild::Shape(Control::Group { children, .. }) => node = &children[0],
                GroupChild::Picture(image) => break image,
                _ => panic!(),
            }
        };
        assert!(matches!(parts[2], (Part::Image(i), _) if std::ptr::eq(i, deepest)));
        let GroupChild::Shape(eq) = &children[2] else { panic!() };
        assert!(matches!(parts[3], (Part::Control(c), _) if std::ptr::eq(c, eq)));
    }
}
