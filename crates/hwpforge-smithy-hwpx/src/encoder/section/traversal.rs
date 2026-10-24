//! Pre-serialization control traversal, in emitted order. Never traverses XML
//! fragments: every host is still Core here, including late-injected headers.
//! Core validation bounds document depth before this traversal.
use super::*;
use hwpforge_core::control::GroupChild;

fn caption(c: &Option<Caption>, f: &mut impl FnMut(&Control) -> HwpxResult<()>) -> HwpxResult<()> {
    if let Some(c) = c {
        paragraphs(&c.paragraphs, f)?;
    }
    Ok(())
}

fn paragraphs(ps: &[Paragraph], f: &mut impl FnMut(&Control) -> HwpxResult<()>) -> HwpxResult<()> {
    for p in ps {
        for r in &p.runs {
            match &r.content {
                RunContent::Control(c) => control(c, f)?,
                RunContent::Table(t) => {
                    caption(&t.caption, f)?;
                    for row in &t.rows {
                        for cell in &row.cells {
                            paragraphs(&cell.paragraphs, f)?;
                        }
                    }
                }
                RunContent::Image(i) => caption(&i.caption, f)?,
                RunContent::Text(_) | RunContent::InlineText(_) => {}
                _ => {
                    return Err(HwpxError::InvalidStructure {
                        detail: "unknown run content".into(),
                    })
                }
            }
        }
    }
    Ok(())
}

fn control(c: &Control, f: &mut impl FnMut(&Control) -> HwpxResult<()>) -> HwpxResult<()> {
    f(c)?;
    match c {
        Control::TextBox { paragraphs: ps, caption: cap, .. } => {
            paragraphs(ps, f)?;
            caption(cap, f)?;
        }
        Control::Ellipse { paragraphs: ps, caption: cap, .. }
        | Control::Polygon { paragraphs: ps, caption: cap, .. } => {
            // Shape schema emits caption before drawText.
            caption(cap, f)?;
            paragraphs(ps, f)?;
        }
        Control::Footnote { paragraphs: ps, .. } | Control::Endnote { paragraphs: ps, .. } => {
            paragraphs(ps, f)?
        }
        Control::Line { caption: cap, .. }
        | Control::Rect { caption: cap, .. }
        | Control::Arc { caption: cap, .. }
        | Control::Curve { caption: cap, .. }
        | Control::ConnectLine { caption: cap, .. } => caption(cap, f)?,
        Control::Group { children, .. } => {
            for child in children {
                match child {
                    GroupChild::Picture(i) => caption(&i.caption, f)?,
                    GroupChild::Shape(c) if super::super::shapes::group_child_is_encodable(c) => {
                        control(c, f)?
                    }
                    // Encoder reports unsupported group children; they do not
                    // consume numbers of controls that actually get emitted.
                    GroupChild::Shape(_) => {}
                    _ => {
                        return Err(HwpxError::InvalidStructure {
                            detail: "unknown group child".into(),
                        })
                    }
                }
            }
        }
        // Non-text memo anchors are explicitly reported by the encoder.
        Control::Memo { content, .. } => paragraphs(content, f)?,
        Control::FieldBoundary { boundary } => {
            if let hwpforge_core::control::FieldBoundary::Start(start) = boundary {
                if let Some(annotation) = &start.annotation {
                    paragraphs(annotation, f)?;
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
        _ => {
            return Err(HwpxError::InvalidStructure {
                detail: "unrecognized control in pre-serialization traversal".into(),
            })
        }
    }
    Ok(())
}

pub(super) fn plan_section_notes(
    section: &Section,
    numbering: &mut NoteNumbering,
    sink: &mut DocumentEncodeContext,
) -> HwpxResult<()> {
    sink.note_numbers.clear();
    let mut visit = |c: &Control| {
        let num = match c {
            Control::Footnote { .. } => Some(numbering.footnote.take("footnote")?),
            Control::Endnote { .. } => Some(numbering.endnote.take("endnote")?),
            Control::NewNumber { kind, number } => {
                match kind {
                    NewNumberKind::Footnote => numbering.footnote.restart(*number),
                    NewNumberKind::Endnote => numbering.endnote.restart(*number),
                    _ => {}
                }
                None
            }
            _ => None,
        };
        if let Some(num) = num {
            sink.note_numbers.insert(std::ptr::from_ref(c) as usize, num);
        }
        Ok(())
    };
    // An anchored header/footer precedes its body paragraph on the wire.
    let mut anchored = vec![Vec::new(); section.paragraphs.len()];
    for h in section.headers.iter().chain(&section.footers) {
        let Some(slot) = anchored.get_mut(header_footer_anchor(h)) else {
            return Err(HwpxError::InvalidStructure {
                detail: "header/footer anchor outside section".into(),
            });
        };
        slot.push(h);
    }
    for (i, p) in section.paragraphs.iter().enumerate() {
        for h in &anchored[i] {
            paragraphs(&h.paragraphs, &mut visit)?;
        }
        paragraphs(std::slice::from_ref(p), &mut visit)?;
    }
    for m in section.master_pages.iter().flatten() {
        paragraphs(&m.paragraphs, &mut visit)?;
    }
    Ok(())
}

/// Geometry fallback clones a group. Rebind identity keys to that clone using
/// exactly the same visitor; no sequence is consumed twice and no FIFO joins.
pub(crate) fn rebind_group_notes(
    original: &Control,
    clone: &Control,
    sink: &mut DocumentEncodeContext,
) -> HwpxResult<()> {
    let mut ids = Vec::new();
    control(original, &mut |c| {
        ids.push(sink.note_numbers.get(&(std::ptr::from_ref(c) as usize)).copied());
        Ok(())
    })?;
    let mut ids = ids.into_iter();
    control(clone, &mut |c| {
        if let Some(Some(num)) = ids.next() {
            sink.note_numbers.insert(std::ptr::from_ref(c) as usize, num);
        }
        Ok(())
    })
}
