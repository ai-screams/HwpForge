//! The single Core nesting budget and consuming validation failure cleanup.
use crate::control::{Control, FieldBoundary, GroupChild};
use crate::image::Image;
use crate::{CoreError, CoreResult, Paragraph, Run, RunContent, Section, Table};

// 이것을 실패시키는 것: 상한 31/33 또는 >= 비교 (31/32/33 전 간선 시험).
const LIMIT: usize = 32;

// All semantic costs live here. Packaging edges cost zero.
// 이것을 실패시키는 것: 객체/캡션 비용 0 (every_edge_boundaries_and_cleanup).
enum Edge {
    Object,
    Caption,
    Packaging,
}
impl Edge {
    fn weight(self) -> usize {
        match self {
            Self::Object | Self::Caption => 1,
            Self::Packaging => 0,
        }
    }
}

enum Node<'a> {
    Paragraphs(&'a [Paragraph]),
    Runs(&'a [Run]),
    Table(&'a Table),
    Image(&'a Image),
    Control(&'a Control),
}

#[derive(Clone, Copy, Default)]
struct Maximum {
    #[cfg(test)]
    nodes: usize,
    depth: usize,
    section: usize,
    paragraph: usize,
    root: usize,
}

fn measure(sections: &[Section]) -> Maximum {
    let mut maximum = Maximum::default();
    #[cfg(test)]
    let mut nodes = 0;
    for (section_index, section) in sections.iter().enumerate() {
        let roots = std::iter::once(section.paragraphs.as_slice())
            .chain(section.headers.iter().chain(&section.footers).map(|h| h.paragraphs.as_slice()))
            .chain(section.master_pages.iter().flatten().map(|m| m.paragraphs.as_slice()));
        for (root, paragraphs) in roots.enumerate() {
            for (paragraph, p) in paragraphs.iter().enumerate() {
                #[cfg(test)]
                {
                    nodes += 1;
                }
                let mut stack = vec![(Node::Runs(&p.runs), 0)];
                while let Some((node, parent)) = stack.pop() {
                    #[cfg(test)]
                    {
                        nodes += match node {
                            Node::Paragraphs(p) => p.len(),
                            Node::Runs(r) => r.len(),
                            _ => 1,
                        };
                    }
                    let edge = match node {
                        Node::Table(_) | Node::Image(_) => Edge::Object,
                        Node::Control(c) => match c {
                            Control::Group { .. }
                            | Control::TextBox { .. }
                            | Control::Ellipse { .. }
                            | Control::Polygon { .. }
                            | Control::Line { .. }
                            | Control::Rect { .. }
                            | Control::Arc { .. }
                            | Control::Curve { .. }
                            | Control::ConnectLine { .. }
                            | Control::Footnote { .. }
                            | Control::Endnote { .. }
                            | Control::Memo { .. }
                            | Control::FieldBoundary { boundary: FieldBoundary::Start(_) } => {
                                Edge::Object
                            }
                            Control::FieldBoundary { boundary: FieldBoundary::End { .. } }
                            | Control::Hyperlink { .. }
                            | Control::EmbeddedChart { .. }
                            | Control::Equation { .. }
                            | Control::Chart { .. }
                            | Control::Dutmal { .. }
                            | Control::Compose { .. }
                            | Control::TextArt { .. }
                            | Control::Bookmark { .. }
                            | Control::CrossRef { .. }
                            | Control::Field { .. }
                            | Control::IndexMark { .. }
                            | Control::UnknownSummary { .. }
                            | Control::DateCodeField { .. }
                            | Control::PathField { .. }
                            | Control::PageNumber { .. }
                            | Control::InlinePageNumber { .. }
                            | Control::AutoNumber { .. }
                            | Control::NewNumber { .. }
                            | Control::PageHiding { .. }
                            | Control::Unknown { .. } => Edge::Packaging,
                        },
                        Node::Paragraphs(_) | Node::Runs(_) => Edge::Packaging,
                    };
                    let depth = parent + edge.weight();
                    if depth > maximum.depth {
                        maximum = Maximum {
                            #[cfg(test)]
                            nodes: 0,
                            depth,
                            section: section_index,
                            paragraph,
                            root,
                        };
                    }
                    match node {
                        Node::Paragraphs(ps) => {
                            stack.extend(ps.iter().map(|p| (Node::Runs(&p.runs), depth)))
                        }
                        Node::Runs(rs) => {
                            for r in rs {
                                match &r.content {
                                    RunContent::Table(t) => stack.push((Node::Table(t), depth)),
                                    RunContent::Image(i) => stack.push((Node::Image(i), depth)),
                                    RunContent::Control(c) => stack.push((Node::Control(c), depth)),
                                    RunContent::Text(_) | RunContent::InlineText(_) => {}
                                }
                            }
                        }
                        Node::Table(t) => {
                            stack.extend(
                                t.rows
                                    .iter()
                                    .flat_map(|r| &r.cells)
                                    .map(|c| (Node::Paragraphs(&c.paragraphs), depth)),
                            );
                            if let Some(c) = &t.caption {
                                stack.push((Node::Paragraphs(&c.paragraphs), depth));
                            }
                        }
                        Node::Image(i) => {
                            if let Some(c) = &i.caption {
                                stack.push((
                                    Node::Paragraphs(&c.paragraphs),
                                    depth + Edge::Caption.weight(),
                                ));
                            }
                        }
                        Node::Control(c) => match c {
                            Control::TextBox { paragraphs, caption, .. }
                            | Control::Ellipse { paragraphs, caption, .. }
                            | Control::Polygon { paragraphs, caption, .. } => {
                                stack.push((Node::Paragraphs(paragraphs), depth));
                                if let Some(c) = caption {
                                    stack.push((
                                        Node::Paragraphs(&c.paragraphs),
                                        depth + Edge::Caption.weight(),
                                    ));
                                }
                            }
                            Control::Line { caption, .. }
                            | Control::Rect { caption, .. }
                            | Control::Arc { caption, .. }
                            | Control::Curve { caption, .. }
                            | Control::ConnectLine { caption, .. } => {
                                if let Some(c) = caption {
                                    stack.push((
                                        Node::Paragraphs(&c.paragraphs),
                                        depth + Edge::Caption.weight(),
                                    ));
                                }
                            }
                            Control::Footnote { paragraphs, .. }
                            | Control::Endnote { paragraphs, .. } => {
                                stack.push((Node::Paragraphs(paragraphs), depth))
                            }
                            Control::Group { children, .. } => {
                                for child in children {
                                    match child {
                                        GroupChild::Picture(i) => {
                                            stack.push((Node::Image(i), depth))
                                        }
                                        GroupChild::Shape(c) => {
                                            stack.push((Node::Control(c), depth))
                                        }
                                    }
                                }
                            }
                            Control::Memo { content, anchor_runs, .. } => {
                                stack.push((Node::Paragraphs(content), depth));
                                stack.push((Node::Runs(anchor_runs), depth));
                            }
                            Control::FieldBoundary { boundary } => {
                                if let FieldBoundary::Start(s) = boundary {
                                    if let Some(ps) = &s.annotation {
                                        stack.push((Node::Paragraphs(ps), depth));
                                    }
                                }
                            }
                            Control::Hyperlink { .. }
                            | Control::EmbeddedChart { .. }
                            | Control::Equation { .. }
                            | Control::Chart { .. }
                            | Control::Dutmal { .. }
                            | Control::Compose { .. }
                            | Control::TextArt { .. }
                            | Control::Bookmark { .. }
                            | Control::CrossRef { .. }
                            | Control::Field { .. }
                            | Control::IndexMark { .. }
                            | Control::UnknownSummary { .. }
                            | Control::DateCodeField { .. }
                            | Control::PathField { .. }
                            | Control::PageNumber { .. }
                            | Control::InlinePageNumber { .. }
                            | Control::AutoNumber { .. }
                            | Control::NewNumber { .. }
                            | Control::PageHiding { .. }
                            | Control::Unknown { .. } => {}
                        },
                    }
                }
            }
        }
    }
    #[cfg(test)]
    {
        maximum.nodes = nodes;
    }
    maximum
}

pub(crate) fn check(sections: &[Section]) -> CoreResult<()> {
    let max = measure(sections);
    if max.depth > LIMIT {
        Err(CoreError::InvalidStructure {
            context: "nesting_depth".into(),
            reason: format!(
                "maximum {} exceeds limit {}; section={}, root={}, paragraph={} (zero-based)",
                max.depth, LIMIT, max.section, max.root, max.paragraph
            ),
        })
    } else {
        Ok(())
    }
}

// No Drop impl: preserve moves out of public Paragraph/Control fields.
// Empty each owning edge before its parent is automatically dropped.
pub(crate) fn dismantle(sections: &mut [Section]) -> usize {
    let mut nodes = 0;
    enum Owned {
        Paragraphs(Vec<Paragraph>),
        Runs(Vec<Run>),
        Control(Box<Control>),
        Image(Box<Image>),
        Table(Box<Table>),
    }
    let mut stack = Vec::new();
    for s in sections {
        stack.push(Owned::Paragraphs(std::mem::take(&mut s.paragraphs)));
        for h in s.headers.iter_mut().chain(&mut s.footers) {
            stack.push(Owned::Paragraphs(std::mem::take(&mut h.paragraphs)));
        }
        for m in s.master_pages.iter_mut().flatten() {
            stack.push(Owned::Paragraphs(std::mem::take(&mut m.paragraphs)));
        }
    }
    while let Some(node) = stack.pop() {
        nodes += match &node {
            Owned::Paragraphs(p) => p.len(),
            Owned::Runs(r) => r.len(),
            _ => 1,
        };
        match node {
            Owned::Paragraphs(ps) => {
                for mut p in ps {
                    stack.push(Owned::Runs(std::mem::take(&mut p.runs)));
                }
            }
            Owned::Runs(rs) => {
                for r in rs {
                    match r.content {
                        RunContent::Table(t) => stack.push(Owned::Table(t)),
                        RunContent::Image(i) => stack.push(Owned::Image(Box::new(i))),
                        RunContent::Control(c) => stack.push(Owned::Control(c)),
                        RunContent::Text(_) | RunContent::InlineText(_) => {}
                    }
                }
            }
            Owned::Table(mut t) => {
                for r in &mut t.rows {
                    for c in &mut r.cells {
                        stack.push(Owned::Paragraphs(std::mem::take(&mut c.paragraphs)));
                    }
                }
                if let Some(c) = t.caption.take() {
                    stack.push(Owned::Paragraphs(c.paragraphs));
                }
            }
            Owned::Image(mut i) => {
                if let Some(c) = i.caption.take() {
                    stack.push(Owned::Paragraphs(c.paragraphs));
                }
            }
            Owned::Control(mut c) => match c.as_mut() {
                Control::TextBox { paragraphs, caption, .. }
                | Control::Ellipse { paragraphs, caption, .. }
                | Control::Polygon { paragraphs, caption, .. } => {
                    stack.push(Owned::Paragraphs(std::mem::take(paragraphs)));
                    if let Some(c) = caption.take() {
                        stack.push(Owned::Paragraphs(c.paragraphs));
                    }
                }
                Control::Line { caption, .. }
                | Control::Rect { caption, .. }
                | Control::Arc { caption, .. }
                | Control::Curve { caption, .. }
                | Control::ConnectLine { caption, .. } => {
                    if let Some(c) = caption.take() {
                        stack.push(Owned::Paragraphs(c.paragraphs));
                    }
                }
                Control::Footnote { paragraphs, .. } | Control::Endnote { paragraphs, .. } => {
                    stack.push(Owned::Paragraphs(std::mem::take(paragraphs)))
                }
                Control::Group { children, .. } => {
                    for child in std::mem::take(children) {
                        match child {
                            GroupChild::Picture(i) => stack.push(Owned::Image(Box::new(i))),
                            GroupChild::Shape(c) => stack.push(Owned::Control(Box::new(c))),
                        }
                    }
                }
                Control::Memo { content, anchor_runs, .. } => {
                    stack.push(Owned::Paragraphs(std::mem::take(content)));
                    stack.push(Owned::Runs(std::mem::take(anchor_runs)));
                }
                Control::FieldBoundary { boundary } => {
                    if let FieldBoundary::Start(s) = boundary {
                        if let Some(ps) = s.annotation.take() {
                            stack.push(Owned::Paragraphs(ps));
                        }
                    }
                }
                Control::Hyperlink { .. }
                | Control::EmbeddedChart { .. }
                | Control::Equation { .. }
                | Control::Chart { .. }
                | Control::Dutmal { .. }
                | Control::Compose { .. }
                | Control::TextArt { .. }
                | Control::Bookmark { .. }
                | Control::CrossRef { .. }
                | Control::Field { .. }
                | Control::IndexMark { .. }
                | Control::UnknownSummary { .. }
                | Control::DateCodeField { .. }
                | Control::PathField { .. }
                | Control::PageNumber { .. }
                | Control::InlinePageNumber { .. }
                | Control::AutoNumber { .. }
                | Control::NewNumber { .. }
                | Control::PageHiding { .. }
                | Control::Unknown { .. } => {}
            },
        }
    }
    nodes
}

#[cfg(test)]
mod tests;
