//! Pair markers, assign click metadata and choose representations once per flow.
//! No state is shared with another section, cell, caption or annotation flow.
use std::collections::BTreeMap;

use hwpforge_core::control::{FieldBoundary, FieldParameter, FieldStart};

use super::{ctrl_id_from_inline_extra, Hwp5Control, Hwp5Paragraph, CTRL_ID_CLICK_HERE};
use crate::schema::section::{Hwp5ClickHereControl, TextSegment};

type Address = (usize, usize);

#[derive(Clone)]
pub(super) enum PlannedMarker {
    Boundary(FieldBoundary),
    Begin { click: Option<Hwp5ClickHereControl>, nested: bool },
    Finish,
    NestedEnd { begin_segment: usize },
    UnmatchedEnd,
}

#[derive(Clone, Default)]
pub(super) struct ParagraphFieldSpans {
    pub markers: BTreeMap<usize, PlannedMarker>,
    pub unpaired_clicks: usize,
}

impl ParagraphFieldSpans {
    pub fn needs_structural_projection(&self) -> bool {
        // Preserve the flat fallback (including cache diagnostics) for orphan ends.
        self.markers.values().any(|m| !matches!(m, PlannedMarker::UnmatchedEnd))
    }
}

struct Field {
    begin: Address,
    end: Option<Address>,
    parent: Option<usize>,
    click: Option<Hwp5ClickHereControl>,
    boundary: bool,
}

pub(super) fn plan_field_spans(
    paragraphs: &[Hwp5Paragraph],
    next_id: &mut usize,
) -> Vec<ParagraphFieldSpans> {
    let mut plan = vec![ParagraphFieldSpans::default(); paragraphs.len()];
    let mut fields: Vec<Field> = Vec::new();
    let mut stack = Vec::new();
    for (p, paragraph) in paragraphs.iter().enumerate() {
        let mut clicks = paragraph.controls.iter().filter_map(|c| match c {
            Hwp5Control::ClickHere(c) => Some(c),
            _ => None,
        });
        for (s, segment) in paragraph.text_segments.iter().enumerate() {
            match segment {
                TextSegment::FieldBegin { extra } => {
                    let click = if ctrl_id_from_inline_extra(extra) == CTRL_ID_CLICK_HERE {
                        clicks.next().cloned()
                    } else {
                        None
                    };
                    fields.push(Field {
                        begin: (p, s),
                        end: None,
                        parent: stack.last().copied(),
                        click,
                        boundary: false,
                    });
                    stack.push(fields.len() - 1);
                }
                TextSegment::FieldEnd => {
                    if let Some(index) = stack.pop() {
                        fields[index].end = Some((p, s));
                    } else {
                        plan[p].markers.insert(s, PlannedMarker::UnmatchedEnd);
                    }
                }
                _ => {}
            }
        }
        plan[p].unpaired_clicks = clicks.count();
    }

    // Parents precede children: use the parent's actual representation, not
    // stack depth at the end marker. Folded ancestors still block promotion.
    for index in 0..fields.len() {
        let field = &fields[index];
        let (p, s) = field.begin;
        let parent_boundary = field.parent.is_none_or(|parent| fields[parent].boundary);
        let boundary = field.click.is_some()
            && field.end.is_some_and(|(end_p, _)| end_p != p)
            && parent_boundary;
        let nested = field
            .parent
            .is_some_and(|parent| fields[parent].begin.0 == p && !fields[parent].boundary);
        let click = fields[index].click.take();
        fields[index].boundary = boundary;
        if boundary {
            let click = click.expect("boundary requires click metadata");
            let (end_p, end_s) = fields[index].end.expect("boundary requires an end");
            *next_id += 1;
            let start =
                clickhere_start(next_id.to_string(), click.command, click.hint_text, click.name);
            let end =
                FieldBoundary::End { begin_id: start.id.clone(), field_id: start.field_id.clone() };
            plan[p]
                .markers
                .insert(s, PlannedMarker::Boundary(FieldBoundary::Start(Box::new(start))));
            plan[end_p].markers.insert(end_s, PlannedMarker::Boundary(end));
        } else {
            plan[p].markers.insert(s, PlannedMarker::Begin { click, nested });
            if let Some((end_p, end_s)) = fields[index].end {
                let end = if end_p != p {
                    PlannedMarker::UnmatchedEnd // folded fallback already ended at paragraph end
                } else if nested {
                    PlannedMarker::NestedEnd { begin_segment: s }
                } else {
                    PlannedMarker::Finish
                };
                plan[end_p].markers.insert(end_s, end);
            }
        }
    }
    plan
}

pub(super) fn clickhere_start(
    id: String,
    command: String,
    hint_text: Option<String>,
    name: Option<String>,
) -> FieldStart {
    let mut start = FieldStart::default();
    // Pairing key only; the HWPX document scope relocates collisions with objects.
    start.id = id;
    start.kind = "CLICK_HERE".into();
    start.name = name.unwrap_or_default();
    start.editable = "1".into();
    start.dirty = "0".into();
    start.z_order = "-1".into();
    start.field_id = CTRL_ID_CLICK_HERE.to_string();
    start.parameter_name = Some(String::new());
    start.parameters = vec![
        FieldParameter::Integer { name: "Prop".into(), value: "9".into() },
        FieldParameter::String {
            name: "Command".into(),
            value: command,
            xml_space: "preserve".into(),
        },
        FieldParameter::String {
            name: "Direction".into(),
            value: hint_text.unwrap_or_default(),
            xml_space: String::new(),
        },
    ];
    start
}
