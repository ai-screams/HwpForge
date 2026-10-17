//! Scoped origin ledger: paragraph construction contributes a Core path;
//! final XML assembly determines its emitted ordinal. Private markers are
//! stripped before packaging. No public options or outcome fields change.
use crate::{HwpxError, HwpxResult, ParagraphPath};
use quick_xml::{events::Event, Reader};
use std::{cell::RefCell, collections::BTreeMap};

/// Core origins in XML paragraph-start order for each emitted package part.
/// `None` denotes a synthetic paragraph without a Core origin.
#[derive(Default, Debug, Clone)]
pub struct EmittedOrigins(pub BTreeMap<String, Vec<Option<ParagraphPath>>>);

#[derive(Default)]
struct Recording {
    paths: Vec<ParagraphPath>,
    output: EmittedOrigins,
}
thread_local! { static RECORDING: RefCell<Option<Recording>> = const { RefCell::new(None) }; }

pub(super) struct Scope;
impl Scope {
    pub fn enter() -> Self {
        RECORDING.with(|r| {
            assert!(r.borrow().is_none(), "nested origin recording");
            *r.borrow_mut() = Some(Recording::default());
        });
        Self
    }
    pub fn finish(self) -> EmittedOrigins {
        RECORDING.with(|r| r.borrow_mut().take().expect("origin scope").output)
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        RECORDING.with(|r| {
            r.borrow_mut().take();
        });
    }
}

pub(super) fn record(path: &[crate::PathSeg]) -> Option<usize> {
    RECORDING.with(|r| {
        let mut r = r.borrow_mut();
        let r = r.as_mut()?;
        let id = r.paths.len();
        r.paths.push(ParagraphPath(path.to_vec()));
        Some(id)
    })
}

pub(super) fn finish_part(part: &str, xml: &mut String) -> HwpxResult<()> {
    RECORDING.with(|r| {
        let mut r = r.borrow_mut();
        let Some(r) = r.as_mut() else {
            return Ok(());
        };
        let mut reader = Reader::from_str(xml);
        let mut origins = Vec::new();
        let mut removals = Vec::new();
        loop {
            let start = reader.buffer_position() as usize;
            match reader
                .read_event()
                .map_err(|e| HwpxError::XmlParse { file: part.into(), detail: e.to_string() })?
            {
                Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == "p" => {
                    let end = reader.buffer_position() as usize;
                    let tag = &xml[start..end];
                    let marker = " hwpforgeOrigin=\"";
                    let origin = if let Some(at) = tag.find(marker) {
                        let value_start = at + marker.len();
                        let value_end =
                            value_start + tag[value_start..].find('"').expect("serialized origin");
                        let id: usize =
                            tag[value_start..value_end].parse().expect("serialized origin ordinal");
                        removals.push(start + at..start + value_end + 1);
                        Some(r.paths[id].clone())
                    } else {
                        None
                    };
                    origins.push(origin);
                }
                Event::Eof => break,
                _ => {}
            }
        }
        for range in removals.into_iter().rev() {
            xml.replace_range(range, "");
        }
        r.output.0.insert(part.into(), origins);
        Ok(())
    })
}
