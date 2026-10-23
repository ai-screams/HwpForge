//! Shared presentation of folded and boundary fields.
//! Renderers use `display_runs`; lossless writers use `expand_memos` so a
//! folded annotation has the same boundary grammar as an unfolded annotation.
use crate::control::{FieldBoundary, FieldParameter, FieldStart};
use crate::{Control, Paragraph, Run, RunContent};
use std::borrow::Cow;

/// Lazy visible-run traversal, preserving each Memo anchor's character style.
#[derive(Debug)]
#[non_exhaustive]
pub struct DisplayRuns<'a> {
    stack: Vec<std::slice::Iter<'a, Run>>,
}
impl<'a> Iterator for DisplayRuns<'a> {
    type Item = &'a Run;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let iter = self.stack.last_mut()?;
            match iter.next() {
                Some(run) => {
                    if let Some(Control::Memo { anchor_runs, .. }) = run.content.as_control() {
                        self.stack.push(anchor_runs.iter());
                    } else {
                        return Some(run);
                    }
                }
                None => {
                    self.stack.pop();
                }
            }
        }
    }
}
/// Visit visible runs without copying text or collapsing styles.
pub fn display_runs(runs: &[Run]) -> DisplayRuns<'_> {
    DisplayRuns { stack: vec![runs.iter()] }
}

/// Expand folded Memo controls into equivalent boundary/anchor runs.
/// The returned paragraph borrows the input when no conversion is needed.
pub fn expand_memos(paragraph: &Paragraph) -> Cow<'_, Paragraph> {
    let mut next = 0;
    expand_with_ids(paragraph, &mut || {
        next += 1;
        format!("memo-view-{next}")
    })
}

/// Expand a complete document view with collision-free deterministic IDs.
pub fn expand_sections(sections: &[crate::Section]) -> Vec<crate::Section> {
    let inventory = crate::field_inventory::FieldInventory::from_sections(sections);
    let mut used: std::collections::HashSet<String> =
        inventory.fields().iter().filter_map(|f| f.id().clone()).collect();
    let mut next = 0usize;
    let mut allocate = || loop {
        next += 1;
        let id = format!("memo-view-{next}");
        if used.insert(id.clone()) {
            break id;
        }
    };
    let mut expanded = sections.to_vec();
    for section in &mut expanded {
        crate::field_inventory::walk_section_mut(section, &mut |paragraphs, _| {
            for paragraph in paragraphs {
                if let Cow::Owned(value) = expand_with_ids(paragraph, &mut allocate) {
                    *paragraph = value;
                }
            }
        });
    }
    expanded
}
fn expand_with_ids<'a>(
    paragraph: &'a Paragraph,
    allocate: &mut impl FnMut() -> String,
) -> Cow<'a, Paragraph> {
    if !paragraph.runs.iter().any(|r| matches!(r.content.as_control(), Some(Control::Memo { .. })))
    {
        return Cow::Borrowed(paragraph);
    }
    let mut result = paragraph.clone();
    result.runs.clear();
    for run in &paragraph.runs {
        if let Some(Control::Memo { content, anchor_runs, metadata }) = run.content.as_control() {
            let id = allocate();
            let mut start = FieldStart {
                id: id.clone(),
                kind: "MEMO".into(),
                editable: "1".into(),
                dirty: "1".into(),
                z_order: "1".into(),
                parameter_name: Some(String::new()),
                parameters: vec![
                    FieldParameter::Integer { name: "Prop".into(), value: "0".into() },
                    FieldParameter::Integer {
                        name: "Number".into(),
                        value: metadata.number.to_string(),
                    },
                ],
                ..FieldStart::default()
            };
            for (name, value) in [
                ("Command", metadata.command.clone()),
                ("ID", metadata.id.clone()),
                ("Author", metadata.author.clone()),
                ("MemoShapeIDRef", metadata.shape_id_ref.to_string()),
                ("CreateDateTime", metadata.create_datetime.clone()),
            ] {
                start.parameters.push(FieldParameter::String {
                    name: name.into(),
                    value,
                    xml_space: String::new(),
                });
            }
            start.annotation = Some(content.clone());
            result.runs.push(Run::control(
                Control::FieldBoundary { boundary: FieldBoundary::Start(Box::new(start)) },
                run.char_shape_id,
            ));
            result.runs.extend(anchor_runs.iter().cloned());
            result.runs.push(Run::control(
                Control::FieldBoundary {
                    boundary: FieldBoundary::End { begin_id: id, field_id: String::new() },
                },
                run.char_shape_id,
            ));
        } else {
            result.runs.push(run.clone());
        }
    }
    Cow::Owned(result)
}
/// Whether a boundary has no drawable characters of its own.
pub fn is_boundary(content: &RunContent) -> bool {
    matches!(content.as_control(), Some(Control::FieldBoundary { .. }))
}

/// Whether the public Memo model can reproduce all authored scalar metadata.
/// IDs identify the paired object; the HWPX encoder allocates them anew.
pub fn memo_is_representable(start: &FieldStart) -> bool {
    if start.kind != "MEMO"
        || !start.name.is_empty()
        || !start.meta_tag.is_empty()
        || start.editable != "1"
        || start.dirty != "1"
        || start.z_order != "1"
        || start.parameter_name.as_deref() != Some("")
        || start.parameters.len() != 7
        || start.annotation.is_none()
    {
        return false;
    }
    let expected =
        ["Prop", "Number", "Command", "ID", "Author", "MemoShapeIDRef", "CreateDateTime"];
    if start.parameters.iter().zip(expected).any(|(p, expected)| {
        let name = match p {
            FieldParameter::Integer { name, .. }
            | FieldParameter::String { name, .. }
            | FieldParameter::Boolean { name, .. } => name,
        };
        name != expected
    }) {
        return false;
    }
    let mut seen = std::collections::HashSet::new();
    start.parameters.iter().all(|p| match p {
        FieldParameter::Integer { name, value } if name == "Prop" => {
            seen.insert(name) && value == "0"
        }
        FieldParameter::Integer { name, value } if name == "Number" => {
            seen.insert(name) && value.parse::<u32>().is_ok_and(|n| n.to_string() == *value)
        }
        FieldParameter::String { name, value, xml_space }
            if xml_space.is_empty() && name == "MemoShapeIDRef" =>
        {
            seen.insert(name) && value.parse::<u32>().is_ok_and(|n| n.to_string() == *value)
        }
        FieldParameter::String { name, xml_space, .. }
            if xml_space.is_empty()
                && ["Command", "ID", "Author", "CreateDateTime"].contains(&name.as_str()) =>
        {
            seen.insert(name)
        }
        _ => false,
    })
}

/// Scalar field display value shared by readable consumers and inventory.
/// Returns `None` for containers, annotations and non-field controls.
pub fn scalar_text(control: &Control) -> Option<&str> {
    match control {
        Control::Field { display_text, hint_text, .. } => Some(if display_text.is_empty() {
            hint_text.as_deref().unwrap_or("")
        } else {
            display_text
        }),
        Control::CrossRef { display_text, .. }
        | Control::UnknownSummary { display_text, .. }
        | Control::PathField { display_text, .. }
        | Control::DateCodeField { display_text, .. } => Some(display_text),
        _ => None,
    }
}

/// Annotation paragraphs independent of the folded/boundary representation.
pub fn annotation(control: &Control) -> Option<&[Paragraph]> {
    match control {
        Control::Memo { content, .. } => Some(content),
        Control::FieldBoundary { boundary: FieldBoundary::Start(start) } => {
            start.annotation.as_deref()
        }
        _ => None,
    }
}

/// Folded anchor runs; boundary anchors already belong to the parent flow.
pub fn anchor_runs(control: &Control) -> &[Run] {
    match control {
        Control::Memo { anchor_runs, .. } => anchor_runs,
        _ => &[],
    }
}
