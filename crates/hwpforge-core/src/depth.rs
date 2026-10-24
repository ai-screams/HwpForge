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

// Empty each owning edge before its parent is automatically dropped.
// All consuming ownership edges live in Work below. Both Drop implementations
// and the test-only dismantle entry use these same shallow detach helpers.
enum Owned {
    Paragraphs(Vec<Paragraph>),
    Runs(Vec<Run>),
    GroupChildren(Vec<GroupChild>),
}

#[derive(Default)]
struct Work {
    stack: Vec<Owned>,
    #[cfg(test)]
    nodes: usize,
}

impl Work {
    #[inline(always)]
    fn count(&mut self) {
        #[cfg(test)]
        {
            self.nodes = self.nodes.saturating_add(1);
        }
    }

    fn paragraphs(&mut self, paragraphs: Vec<Paragraph>) {
        if !paragraphs.is_empty() {
            self.stack.push(Owned::Paragraphs(paragraphs));
        }
    }

    fn runs(&mut self, runs: Vec<Run>) {
        if !runs.is_empty() {
            self.stack.push(Owned::Runs(runs));
        }
    }

    fn take_paragraph_children(&mut self, paragraph: &mut Paragraph) {
        self.count();
        // Text-only roots never allocate a work stack or scan their runs twice.
        for run in std::mem::take(&mut paragraph.runs) {
            self.take_run_children(run);
        }
    }

    fn take_run_children(&mut self, run: Run) {
        self.count();
        match run.content {
            RunContent::Table(mut table) => self.take_table_children(&mut table),
            RunContent::Image(mut image) => self.take_image_children(&mut image),
            RunContent::Control(mut control) => self.take_control_children(&mut control),
            RunContent::Text(_) | RunContent::InlineText(_) => {}
        }
    }

    fn take_table_children(&mut self, table: &mut Table) {
        self.count();
        for row in &mut table.rows {
            for cell in &mut row.cells {
                self.paragraphs(std::mem::take(&mut cell.paragraphs));
            }
        }
        if let Some(caption) = table.caption.take() {
            self.paragraphs(caption.paragraphs);
        }
    }

    fn take_image_children(&mut self, image: &mut Image) {
        self.count();
        if let Some(caption) = image.caption.take() {
            self.paragraphs(caption.paragraphs);
        }
    }

    fn take_control_children(&mut self, control: &mut Control) {
        self.count();
        match control {
            Control::TextBox { paragraphs, caption, .. }
            | Control::Ellipse { paragraphs, caption, .. }
            | Control::Polygon { paragraphs, caption, .. } => {
                self.paragraphs(std::mem::take(paragraphs));
                if let Some(c) = caption.take() {
                    self.paragraphs(c.paragraphs);
                }
            }
            Control::Line { caption, .. }
            | Control::Rect { caption, .. }
            | Control::Arc { caption, .. }
            | Control::Curve { caption, .. }
            | Control::ConnectLine { caption, .. } => {
                if let Some(c) = caption.take() {
                    self.paragraphs(c.paragraphs);
                }
            }
            Control::Footnote { paragraphs, .. } | Control::Endnote { paragraphs, .. } => {
                self.paragraphs(std::mem::take(paragraphs))
            }
            Control::Group { children, .. } => {
                let children = std::mem::take(children);
                if !children.is_empty() {
                    self.stack.push(Owned::GroupChildren(children));
                }
            }
            Control::Memo { content, anchor_runs, .. } => {
                self.paragraphs(std::mem::take(content));
                self.runs(std::mem::take(anchor_runs));
            }
            Control::FieldBoundary { boundary } => {
                if let FieldBoundary::Start(s) = boundary {
                    if let Some(ps) = s.annotation.take() {
                        self.paragraphs(ps);
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
        }
    }

    fn drain(&mut self) {
        while let Some(node) = self.stack.pop() {
            match node {
                Owned::Paragraphs(paragraphs) => {
                    for mut paragraph in paragraphs {
                        self.take_paragraph_children(&mut paragraph);
                    }
                }
                Owned::Runs(runs) => {
                    for run in runs {
                        self.take_run_children(run);
                    }
                }
                Owned::GroupChildren(children) => {
                    for child in children {
                        match child {
                            GroupChild::Shape(mut control) => {
                                self.take_control_children(&mut control)
                            }
                            GroupChild::Picture(mut image) => self.take_image_children(&mut image),
                        }
                    }
                }
            }
        }
    }
}

pub(crate) fn drop_paragraph(paragraph: &mut Paragraph) {
    let mut work = Work::default();
    work.take_paragraph_children(paragraph);
    work.drain();
}

pub(crate) fn drop_control(control: &mut Control) {
    let mut work = Work::default();
    work.take_control_children(control);
    work.drain();
}

#[cfg(test)]
fn dismantle(sections: &mut [Section]) -> usize {
    let mut work = Work::default();
    for section in sections {
        work.paragraphs(std::mem::take(&mut section.paragraphs));
        for band in section.headers.iter_mut().chain(&mut section.footers) {
            work.paragraphs(std::mem::take(&mut band.paragraphs));
        }
        for master in section.master_pages.iter_mut().flatten() {
            work.paragraphs(std::mem::take(&mut master.paragraphs));
        }
    }
    work.drain();
    work.nodes
}

#[cfg(test)]
mod tests;
