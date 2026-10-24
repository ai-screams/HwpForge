//! Private, ordered borrowing stacks for the existing Core walkers.
use crate::control::{FieldBoundary, GroupChild};
use crate::{Control, Paragraph, Run, RunContent, Table};

// Frames borrow disjoint fields only. Children are read after the callback.
pub(crate) enum Node<'a> {
    Paragraph(&'a Paragraph),
    Paragraphs(&'a [Paragraph]),
    Run(&'a Run),
    Table(&'a Table),
    Control(&'a Control),
}

fn children<'a>(node: Node<'a>, stack: &mut Vec<Node<'a>>) {
    match node {
        Node::Paragraph(p) => stack.extend(
            p.runs
                .iter()
                .rev()
                .filter(|r| matches!(r.content, RunContent::Table(_) | RunContent::Control(_)))
                .map(Node::Run),
        ),
        Node::Paragraphs(ps) => stack.extend(ps.iter().rev().map(Node::Paragraph)),
        Node::Run(r) => match &r.content {
            RunContent::Table(t) => stack.push(Node::Table(t)),
            RunContent::Control(c) => stack.push(Node::Control(c)),
            RunContent::Text(_) | RunContent::InlineText(_) | RunContent::Image(_) => {}
        },
        Node::Table(t) => {
            if let Some(c) = &t.caption {
                stack.push(Node::Paragraphs(&c.paragraphs));
            }
            for row in t.rows.iter().rev() {
                for cell in row.cells.iter().rev() {
                    stack.push(Node::Paragraphs(&cell.paragraphs));
                }
            }
        }
        Node::Control(c) => match c {
            Control::TextBox { paragraphs, caption, .. }
            | Control::Ellipse { paragraphs, caption, .. }
            | Control::Polygon { paragraphs, caption, .. } => {
                if let Some(c) = caption {
                    stack.push(Node::Paragraphs(&c.paragraphs));
                }
                stack.push(Node::Paragraphs(paragraphs));
            }
            Control::Footnote { paragraphs, .. } | Control::Endnote { paragraphs, .. } => {
                stack.push(Node::Paragraphs(paragraphs))
            }
            Control::Line { caption, .. }
            | Control::Rect { caption, .. }
            | Control::Arc { caption, .. }
            | Control::Curve { caption, .. }
            | Control::ConnectLine { caption, .. } => {
                if let Some(c) = caption {
                    stack.push(Node::Paragraphs(&c.paragraphs));
                }
            }
            Control::Group { children, .. } => {
                for child in children.iter().rev() {
                    match child {
                        GroupChild::Shape(c) => stack.push(Node::Control(c)),
                        GroupChild::Picture(i) => {
                            if let Some(c) = &i.caption {
                                stack.push(Node::Paragraphs(&c.paragraphs));
                            }
                        }
                    }
                }
            }
            Control::Memo { content, anchor_runs, .. } => {
                stack.extend(anchor_runs.iter().rev().map(Node::Run));
                stack.push(Node::Paragraphs(content));
            }
            Control::FieldBoundary { boundary } => {
                if let FieldBoundary::Start(s) = boundary {
                    if let Some(ps) = &s.annotation {
                        stack.push(Node::Paragraphs(ps));
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

pub(crate) fn paragraphs(root: Node<'_>, f: &mut dyn FnMut(&Paragraph)) {
    paragraph_roots(std::iter::once(root), f);
}

pub(crate) fn paragraph_roots<'a>(
    roots: impl Iterator<Item = Node<'a>>,
    f: &mut dyn FnMut(&Paragraph),
) {
    let mut stack = Vec::new();
    for root in roots {
        stack.push(root);
        while let Some(node) = stack.pop() {
            match node {
                Node::Paragraph(p) => {
                    f(p);
                    children(Node::Paragraph(p), &mut stack);
                }
                other => children(other, &mut stack),
            }
        }
    }
}

enum Tables<'a> {
    Descend(Node<'a>),
    Root(&'a Run),
    Own(&'a Run),
    Flush(usize),
}

pub(crate) fn tables<'a>(runs: impl Iterator<Item = &'a Run>, f: &mut dyn FnMut(&Table)) {
    let mut stack = Vec::new();
    let mut deferred = Vec::new();
    let mut pending = Vec::new();
    // Reuse the three work buffers across independent roots in a section.
    for run in runs {
        if matches!(run.content, RunContent::Text(_) | RunContent::InlineText(_)) {
            continue;
        }
        stack.push(Tables::Root(run));
        while let Some(frame) = stack.pop() {
            match frame {
                Tables::Root(r) => {
                    stack.push(Tables::Flush(deferred.len()));
                    stack.push(Tables::Own(r));
                }
                Tables::Flush(start) => {
                    stack.extend(deferred.drain(start..).rev().map(Tables::Descend));
                }
                Tables::Own(r) => match &r.content {
                    RunContent::Table(t) => {
                        f(t);
                        deferred.push(Node::Table(t));
                    }
                    RunContent::Image(i) => {
                        for p in i.caption.iter().flat_map(|c| c.paragraphs.iter()).rev() {
                            stack.extend(p.runs.iter().rev().map(Tables::Root));
                        }
                    }
                    RunContent::Control(c) => match c.as_ref() {
                        Control::Memo { content, anchor_runs, .. } => {
                            deferred.push(Node::Paragraphs(content));
                            stack.extend(anchor_runs.iter().rev().map(Tables::Own));
                        }
                        c => deferred.push(Node::Control(c)),
                    },
                    RunContent::Text(_) | RunContent::InlineText(_) => {}
                },
                Tables::Descend(Node::Paragraph(p)) => {
                    // All own runs precede all descendants of a nested paragraph.
                    stack.push(Tables::Flush(deferred.len()));
                    stack.extend(p.runs.iter().rev().map(Tables::Own));
                }
                Tables::Descend(node) => {
                    children(node, &mut pending);
                    stack.extend(pending.drain(..).map(Tables::Descend));
                }
            }
        }
    }
}

// Frames borrow disjoint fields only. Children are read after the callback.
pub(crate) enum NodeMut<'a> {
    Paragraph(&'a mut Paragraph),
    Paragraphs(&'a mut [Paragraph]),
    Run(&'a mut Run),
    Table(&'a mut Table),
    Control(&'a mut Control),
}

fn children_mut<'a>(node: NodeMut<'a>, stack: &mut Vec<NodeMut<'a>>) {
    match node {
        NodeMut::Paragraph(p) => stack.extend(
            p.runs
                .iter_mut()
                .rev()
                .filter(|r| matches!(r.content, RunContent::Table(_) | RunContent::Control(_)))
                .map(NodeMut::Run),
        ),
        NodeMut::Paragraphs(ps) => stack.extend(ps.iter_mut().rev().map(NodeMut::Paragraph)),
        NodeMut::Run(r) => match &mut r.content {
            RunContent::Table(t) => stack.push(NodeMut::Table(t)),
            RunContent::Control(c) => stack.push(NodeMut::Control(c)),
            RunContent::Text(_) | RunContent::InlineText(_) | RunContent::Image(_) => {}
        },
        NodeMut::Table(t) => {
            if let Some(c) = &mut t.caption {
                stack.push(NodeMut::Paragraphs(&mut c.paragraphs));
            }
            for row in t.rows.iter_mut().rev() {
                for cell in row.cells.iter_mut().rev() {
                    stack.push(NodeMut::Paragraphs(&mut cell.paragraphs));
                }
            }
        }
        NodeMut::Control(c) => match c {
            Control::TextBox { paragraphs, caption, .. }
            | Control::Ellipse { paragraphs, caption, .. }
            | Control::Polygon { paragraphs, caption, .. } => {
                if let Some(c) = caption {
                    stack.push(NodeMut::Paragraphs(&mut c.paragraphs));
                }
                stack.push(NodeMut::Paragraphs(paragraphs));
            }
            Control::Footnote { paragraphs, .. } | Control::Endnote { paragraphs, .. } => {
                stack.push(NodeMut::Paragraphs(paragraphs))
            }
            Control::Line { caption, .. }
            | Control::Rect { caption, .. }
            | Control::Arc { caption, .. }
            | Control::Curve { caption, .. }
            | Control::ConnectLine { caption, .. } => {
                if let Some(c) = caption {
                    stack.push(NodeMut::Paragraphs(&mut c.paragraphs));
                }
            }
            Control::Group { children, .. } => {
                for child in children.iter_mut().rev() {
                    match child {
                        GroupChild::Shape(c) => stack.push(NodeMut::Control(c)),
                        GroupChild::Picture(i) => {
                            if let Some(c) = &mut i.caption {
                                stack.push(NodeMut::Paragraphs(&mut c.paragraphs));
                            }
                        }
                    }
                }
            }
            Control::Memo { content, anchor_runs, .. } => {
                stack.extend(anchor_runs.iter_mut().rev().map(NodeMut::Run));
                stack.push(NodeMut::Paragraphs(content));
            }
            Control::FieldBoundary { boundary } => {
                if let FieldBoundary::Start(s) = boundary {
                    if let Some(ps) = &mut s.annotation {
                        stack.push(NodeMut::Paragraphs(ps));
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

pub(crate) fn paragraphs_mut(root: NodeMut<'_>, f: &mut dyn FnMut(&mut Paragraph)) {
    paragraph_roots_mut(std::iter::once(root), f);
}

pub(crate) fn paragraph_roots_mut<'a>(
    roots: impl Iterator<Item = NodeMut<'a>>,
    f: &mut dyn FnMut(&mut Paragraph),
) {
    let mut stack = Vec::new();
    for root in roots {
        stack.push(root);
        while let Some(node) = stack.pop() {
            match node {
                NodeMut::Paragraph(p) => {
                    f(p);
                    children_mut(NodeMut::Paragraph(p), &mut stack);
                }
                other => children_mut(other, &mut stack),
            }
        }
    }
}

enum TablesMut<'a> {
    Descend(NodeMut<'a>),
    Root(&'a mut Run),
    Own(&'a mut Run),
    Flush(usize),
}

pub(crate) fn tables_mut<'a>(
    runs: impl Iterator<Item = &'a mut Run>,
    f: &mut dyn FnMut(&mut Table),
) {
    let mut stack = Vec::new();
    let mut deferred = Vec::new();
    let mut pending = Vec::new();
    // Reuse the three work buffers across independent roots in a section.
    for run in runs {
        if matches!(run.content, RunContent::Text(_) | RunContent::InlineText(_)) {
            continue;
        }
        stack.push(TablesMut::Root(run));
        while let Some(frame) = stack.pop() {
            match frame {
                TablesMut::Root(r) => {
                    stack.push(TablesMut::Flush(deferred.len()));
                    stack.push(TablesMut::Own(r));
                }
                TablesMut::Flush(start) => {
                    stack.extend(deferred.drain(start..).rev().map(TablesMut::Descend));
                }
                TablesMut::Own(r) => match &mut r.content {
                    RunContent::Table(t) => {
                        f(t);
                        deferred.push(NodeMut::Table(t));
                    }
                    RunContent::Image(i) => {
                        for p in i.caption.iter_mut().flat_map(|c| c.paragraphs.iter_mut()).rev() {
                            stack.extend(p.runs.iter_mut().rev().map(TablesMut::Root));
                        }
                    }
                    RunContent::Control(c) => match c.as_mut() {
                        Control::Memo { content, anchor_runs, .. } => {
                            deferred.push(NodeMut::Paragraphs(content));
                            stack.extend(anchor_runs.iter_mut().rev().map(TablesMut::Own));
                        }
                        c => deferred.push(NodeMut::Control(c)),
                    },
                    RunContent::Text(_) | RunContent::InlineText(_) => {}
                },
                TablesMut::Descend(NodeMut::Paragraph(p)) => {
                    // All own runs precede all descendants of a nested paragraph.
                    stack.push(TablesMut::Flush(deferred.len()));
                    stack.extend(p.runs.iter_mut().rev().map(TablesMut::Own));
                }
                TablesMut::Descend(node) => {
                    children_mut(node, &mut pending);
                    stack.extend(pending.drain(..).map(TablesMut::Descend));
                }
            }
        }
    }
}
