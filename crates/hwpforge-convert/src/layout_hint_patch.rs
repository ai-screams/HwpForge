//! Applies captured HWP5 layout hints onto generated HWPX bytes.
//!
//! The HWP5 decoder captures per-section [`SectionLayoutHints`] (paragraph line
//! segments + table heights) as a format-neutral value. This module replays
//! them onto the HWPX `section{N}.xml` streams so Hancom Office can open the
//! converted file without flagging a "low-security recovery". The capture side
//! lives in `hwpforge_smithy_hwp5::layout_hint_patch`; only the HWPX-byte
//! rewriting lives here.

use std::collections::BTreeMap;
use std::io::{Cursor, Read as _, Write};

use quick_xml::events::{BytesEnd, BytesStart, Event};
use quick_xml::{Reader, Writer};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use hwpforge_smithy_hwp5::layout_hint_patch::{
    ParagraphLayoutHint, SectionLayoutHints, TableLayoutHint,
};
use hwpforge_smithy_hwp5::schema::section::Hwp5ParaLineSeg;
use hwpforge_smithy_hwp5::{Hwp5Error, Hwp5Result};

#[derive(Debug, Clone)]
struct RawPackage {
    entries: Vec<RawPackageEntry>,
    index_by_path: BTreeMap<String, usize>,
}

#[derive(Debug, Clone)]
struct RawPackageEntry {
    path: String,
    bytes: Vec<u8>,
    compression: CompressionMethod,
}

pub(crate) fn patch_hwpx_layout_hints(
    hwpx_bytes: &[u8],
    sections: &[SectionLayoutHints],
) -> Hwp5Result<Vec<u8>> {
    let mut package = RawPackage::read(hwpx_bytes)?;
    for (section_idx, section) in sections.iter().enumerate() {
        if !section.has_payload() {
            continue;
        }

        let path = format!("Contents/section{section_idx}.xml");
        let xml = package.read_text_entry(&path)?;
        let patched = patch_section_xml(&xml, section.clone())?;
        package.replace_text_entry(&path, patched);
    }
    package.write()
}

/// Maximum decompressed size of a single ZIP entry (50 MiB).
///
/// Mirrors `smithy-hwpx::decoder::package::MAX_ENTRY_SIZE` so the secondary
/// reader used by the HWP5→HWPX layout-hint patcher enforces the same ZIP-bomb
/// defenses as the primary HWPX decoder.
const MAX_ENTRY_SIZE: u64 = 50 * 1024 * 1024;

/// Maximum total decompressed size across all entries (500 MiB).
const MAX_TOTAL_SIZE: u64 = 500 * 1024 * 1024;

/// Maximum number of entries in the archive.
const MAX_ENTRIES: usize = 10_000;

impl RawPackage {
    fn read(bytes: &[u8]) -> Hwp5Result<Self> {
        Self::read_capped(bytes, MAX_ENTRY_SIZE, MAX_TOTAL_SIZE, MAX_ENTRIES)
    }

    /// Internal worker for [`read`] with explicit caps so tests can verify the
    /// bounds with small values instead of materializing multi-MiB entries.
    fn read_capped(
        bytes: &[u8],
        max_entry: u64,
        max_total: u64,
        max_entries: usize,
    ) -> Hwp5Result<Self> {
        let cursor = Cursor::new(bytes);
        let mut archive = ZipArchive::new(cursor)
            .map_err(|e| Hwp5Error::Cfb { detail: format!("open hwpx package: {e}") })?;

        if archive.len() > max_entries {
            return Err(Hwp5Error::Cfb {
                detail: format!(
                    "hwpx package has {} entries, exceeds limit of {max_entries}",
                    archive.len()
                ),
            });
        }

        let mut entries = Vec::with_capacity(archive.len());
        let mut index_by_path = BTreeMap::new();
        let mut total: u64 = 0;

        for index in 0..archive.len() {
            let file = archive
                .by_index(index)
                .map_err(|e| Hwp5Error::Cfb { detail: format!("read hwpx entry #{index}: {e}") })?;
            let path = file.name().to_string();
            let compression = file.compression();
            // `file.size()` comes from the ZIP central directory and can be
            // spoofed, so cap the capacity hint and bound the reader itself
            // with `take(max_entry + 1)` to detect over-cap entries without
            // pre-allocating an attacker-controlled buffer.
            let hint = file.size().min(max_entry) as usize;
            let mut bytes = Vec::with_capacity(hint);
            file.take(max_entry + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| Hwp5Error::Cfb { detail: format!("read '{path}' bytes: {e}") })?;
            if bytes.len() as u64 > max_entry {
                return Err(Hwp5Error::Cfb {
                    detail: format!(
                        "hwpx entry '{path}' decompressed to {} bytes, exceeds limit of {max_entry}",
                        bytes.len()
                    ),
                });
            }
            total = total.saturating_add(bytes.len() as u64);
            if total > max_total {
                return Err(Hwp5Error::Cfb {
                    detail: format!(
                        "hwpx package total decompressed data ({total} bytes) exceeds limit of {max_total}"
                    ),
                });
            }
            index_by_path.insert(path.clone(), entries.len());
            entries.push(RawPackageEntry { path, bytes, compression });
        }

        Ok(Self { entries, index_by_path })
    }

    fn read_text_entry(&self, path: &str) -> Hwp5Result<String> {
        let index = self
            .index_by_path
            .get(path)
            .copied()
            .ok_or_else(|| Hwp5Error::MissingStream { name: path.to_string() })?;
        String::from_utf8(self.entries[index].bytes.clone()).map_err(|e| Hwp5Error::Cfb {
            detail: format!("entry '{path}' is not valid UTF-8: {e}"),
        })
    }

    fn replace_text_entry(&mut self, path: &str, content: String) {
        if let Some(index) = self.index_by_path.get(path).copied() {
            self.entries[index].bytes = content.into_bytes();
        }
    }

    fn write(&self) -> Hwp5Result<Vec<u8>> {
        let cursor = Cursor::new(Vec::<u8>::new());
        let mut zip = ZipWriter::new(cursor);
        for entry in &self.entries {
            let options = SimpleFileOptions::default().compression_method(entry.compression);
            zip.start_file(&entry.path, options).map_err(|e| Hwp5Error::Cfb {
                detail: format!("zip start '{}': {e}", entry.path),
            })?;
            zip.write_all(&entry.bytes).map_err(|e| Hwp5Error::Cfb {
                detail: format!("zip write '{}': {e}", entry.path),
            })?;
        }
        let cursor = zip
            .finish()
            .map_err(|e| Hwp5Error::Cfb { detail: format!("finish hwpx patch package: {e}") })?;
        Ok(cursor.into_inner())
    }
}

#[derive(Debug)]
struct SectionXmlPatchState {
    hints: SectionLayoutHints,
    element_stack: Vec<Vec<u8>>,
    paragraph_stack: Vec<ParagraphLayoutHint>,
    table_stack: Vec<TableLayoutHint>,
}

impl SectionXmlPatchState {
    fn new(hints: SectionLayoutHints) -> Self {
        Self {
            hints,
            element_stack: Vec::new(),
            paragraph_stack: Vec::new(),
            table_stack: Vec::new(),
        }
    }

    fn handle_start<W: Write>(
        &mut self,
        event: BytesStart<'_>,
        writer: &mut Writer<W>,
    ) -> Hwp5Result<()> {
        let local = local_name(event.name().as_ref()).to_vec();
        if local.as_slice() == b"p" {
            self.push_paragraph_hint()?;
        } else if local.as_slice() == b"tbl" {
            self.push_table_hint()?;
        }

        let event = self.patch_table_size_event(local.as_slice(), event.into_owned())?;
        writer
            .write_event(Event::Start(event))
            .map_err(|e| Hwp5Error::Cfb { detail: format!("write patched section xml: {e}") })?;
        self.element_stack.push(local);
        Ok(())
    }

    fn handle_empty<W: Write>(
        &mut self,
        event: BytesStart<'_>,
        writer: &mut Writer<W>,
    ) -> Hwp5Result<()> {
        let local = local_name(event.name().as_ref()).to_vec();
        let event = self.patch_table_size_event(local.as_slice(), event.into_owned())?;
        writer
            .write_event(Event::Empty(event))
            .map_err(|e| Hwp5Error::Cfb { detail: format!("write patched section xml: {e}") })?;
        Ok(())
    }

    fn handle_end<W: Write>(
        &mut self,
        event: BytesEnd<'_>,
        writer: &mut Writer<W>,
    ) -> Hwp5Result<()> {
        let local = local_name(event.name().as_ref()).to_vec();
        if local.as_slice() == b"p" {
            let hint = self.pop_paragraph_hint()?;
            write_linesegarray(writer, &hint.line_segments)?;
        }

        writer
            .write_event(Event::End(event.into_owned()))
            .map_err(|e| Hwp5Error::Cfb { detail: format!("write patched section xml: {e}") })?;

        self.pop_element(local.as_slice())?;
        if local.as_slice() == b"tbl" {
            self.table_stack.pop();
        }
        Ok(())
    }

    fn finish(self) -> Hwp5Result<()> {
        if !self.hints.paragraphs.is_empty()
            || !self.hints.tables.is_empty()
            || !self.paragraph_stack.is_empty()
        {
            return Err(Hwp5Error::Cfb {
                detail: "layout hint patch left unconsumed hints".into(),
            });
        }
        Ok(())
    }

    fn patch_table_size_event(
        &self,
        local: &[u8],
        event: BytesStart<'static>,
    ) -> Hwp5Result<BytesStart<'static>> {
        if !self.is_active_table_size_element(local) {
            return Ok(event);
        }

        let Some(height) = self.active_table_height()? else {
            return Ok(event);
        };
        rewrite_element_attr(event, "height", &height.to_string())
    }

    fn is_active_table_size_element(&self, local: &[u8]) -> bool {
        local == b"sz"
            && self.element_stack.last().is_some_and(|parent| parent.as_slice() == b"tbl")
    }

    fn active_table_height(&self) -> Hwp5Result<Option<i32>> {
        self.table_stack
            .last()
            .copied()
            .ok_or_else(|| Hwp5Error::Cfb {
                detail: "table size encountered without active table hint".into(),
            })
            .map(|hint| hint.height)
    }

    fn push_paragraph_hint(&mut self) -> Hwp5Result<()> {
        let hint = self.hints.paragraphs.pop_front().ok_or_else(|| Hwp5Error::Cfb {
            detail: "paragraph layout hint count underflow".into(),
        })?;
        self.paragraph_stack.push(hint);
        Ok(())
    }

    fn push_table_hint(&mut self) -> Hwp5Result<()> {
        let hint =
            self.hints.tables.pop_front().ok_or_else(|| Hwp5Error::Cfb {
                detail: "table layout hint count underflow".into(),
            })?;
        self.table_stack.push(hint);
        Ok(())
    }

    fn pop_paragraph_hint(&mut self) -> Hwp5Result<ParagraphLayoutHint> {
        self.paragraph_stack.pop().ok_or_else(|| Hwp5Error::Cfb {
            detail: "paragraph layout hint stack underflow".into(),
        })
    }

    fn pop_element(&mut self, local: &[u8]) -> Hwp5Result<()> {
        let popped = self
            .element_stack
            .pop()
            .ok_or_else(|| Hwp5Error::Cfb { detail: "xml element stack underflow".into() })?;
        if popped != local {
            return Err(Hwp5Error::Cfb {
                detail: format!(
                    "xml element stack mismatch: opened '{}' closed '{}'",
                    String::from_utf8_lossy(&popped),
                    String::from_utf8_lossy(local)
                ),
            });
        }
        Ok(())
    }
}

fn patch_section_xml(xml: &str, hints: SectionLayoutHints) -> Hwp5Result<String> {
    let mut reader = Reader::from_str(xml);
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(xml.len() + 1024)));
    let mut buf = Vec::new();
    let mut state = SectionXmlPatchState::new(hints);

    loop {
        match reader
            .read_event_into(&mut buf)
            .map_err(|e| Hwp5Error::Cfb { detail: format!("parse generated section xml: {e}") })?
        {
            Event::Start(event) => state.handle_start(event, &mut writer)?,
            Event::Empty(event) => state.handle_empty(event, &mut writer)?,
            Event::End(event) => state.handle_end(event, &mut writer)?,
            Event::Eof => break,
            event => {
                writer.write_event(event.into_owned()).map_err(|e| Hwp5Error::Cfb {
                    detail: format!("write patched section xml: {e}"),
                })?;
            }
        }
        buf.clear();
    }

    state.finish()?;

    let bytes = writer.into_inner().into_inner();
    String::from_utf8(bytes).map_err(|e| Hwp5Error::Cfb {
        detail: format!("patched section xml is not valid UTF-8: {e}"),
    })
}

fn rewrite_element_attr(
    event: BytesStart<'static>,
    target_attr: &str,
    new_value: &str,
) -> Hwp5Result<BytesStart<'static>> {
    let name = String::from_utf8(event.name().as_ref().to_vec())
        .map_err(|e| Hwp5Error::Cfb { detail: format!("element name is not valid UTF-8: {e}") })?;
    let mut rebuilt = BytesStart::new(name);
    let mut replaced = false;

    for attr in event.attributes().with_checks(false) {
        let attr =
            attr.map_err(|e| Hwp5Error::Cfb { detail: format!("read xml attribute: {e}") })?;
        let key = std::str::from_utf8(attr.key.as_ref()).map_err(|e| Hwp5Error::Cfb {
            detail: format!("attribute key is not valid UTF-8: {e}"),
        })?;
        let value = if local_name(attr.key.as_ref()) == target_attr.as_bytes() {
            replaced = true;
            new_value
        } else {
            std::str::from_utf8(attr.value.as_ref()).map_err(|e| Hwp5Error::Cfb {
                detail: format!("attribute value is not valid UTF-8: {e}"),
            })?
        };
        rebuilt.push_attribute((key, value));
    }

    if !replaced {
        rebuilt.push_attribute((target_attr, new_value));
    }

    Ok(rebuilt)
}

/// Conservative default lineseg values for paragraphs whose HWP5 source
/// does not carry a `ParaLineSeg` (tag `0x45`) record (task #123).
///
/// Without an emitted `<hp:linesegarray>`, Hancom Office flags the file
/// as needing "low-security recovery" because it cannot pre-compute the
/// rendering cache. Emitting a single placeholder segment with native-
/// matching default attributes lets Hancom open the file silently — it
/// recomputes per-line metrics on first render anyway.
///
/// Values empirically derived from native `sample-field-docsummary.hwpx`:
/// - `vertsize/textheight = 1000` (10pt baseline, native default)
/// - `baseline = 850`, `spacing = 600`
/// - `horzsize = 42520` (A4 content width, 1cm L/R margin)
/// - `flags = 393216` (Hancom's default lineseg flag bitmask)
///
/// `vertpos` is intentionally left at `0` — Hancom recomputes the cumulative
/// vertical position from paragraph order on open.
fn write_default_lineseg<W: Write>(writer: &mut Writer<W>) -> Hwp5Result<()> {
    let mut line = BytesStart::new("hp:lineseg");
    line.push_attribute(("textpos", "0"));
    line.push_attribute(("vertpos", "0"));
    line.push_attribute(("vertsize", "1000"));
    line.push_attribute(("textheight", "1000"));
    line.push_attribute(("baseline", "850"));
    line.push_attribute(("spacing", "600"));
    line.push_attribute(("horzpos", "0"));
    line.push_attribute(("horzsize", "42520"));
    line.push_attribute(("flags", "393216"));
    writer
        .write_event(Event::Empty(line))
        .map_err(|e| Hwp5Error::Cfb { detail: format!("write default lineseg: {e}") })?;
    Ok(())
}

fn write_linesegarray<W: Write>(
    writer: &mut Writer<W>,
    line_segments: &[Hwp5ParaLineSeg],
) -> Hwp5Result<()> {
    // task #123: always emit linesegarray (even if HWP5 source omitted
    // the ParaLineSeg record). Hancom recomputes accurate metrics on
    // first render, but the element's *presence* is required to skip
    // the "low-security recovery" warning.
    writer
        .write_event(Event::Start(BytesStart::new("hp:linesegarray")))
        .map_err(|e| Hwp5Error::Cfb { detail: format!("write linesegarray start: {e}") })?;

    if line_segments.is_empty() {
        write_default_lineseg(writer)?;
    } else {
        for segment in line_segments {
            let textpos = segment.text_start_position.to_string();
            let vertpos = segment.vertical_position.to_string();
            let vertsize = segment.line_height.to_string();
            let textheight = segment.text_height.to_string();
            let baseline = segment.baseline_distance.to_string();
            let spacing = segment.line_spacing.to_string();
            let horzpos = segment.column_start_position.to_string();
            let horzsize = segment.segment_width.to_string();
            let flags = segment.tag.to_string();

            let mut line = BytesStart::new("hp:lineseg");
            line.push_attribute(("textpos", textpos.as_str()));
            line.push_attribute(("vertpos", vertpos.as_str()));
            line.push_attribute(("vertsize", vertsize.as_str()));
            line.push_attribute(("textheight", textheight.as_str()));
            line.push_attribute(("baseline", baseline.as_str()));
            line.push_attribute(("spacing", spacing.as_str()));
            line.push_attribute(("horzpos", horzpos.as_str()));
            line.push_attribute(("horzsize", horzsize.as_str()));
            line.push_attribute(("flags", flags.as_str()));

            writer
                .write_event(Event::Empty(line))
                .map_err(|e| Hwp5Error::Cfb { detail: format!("write lineseg: {e}") })?;
        }
    }

    writer
        .write_event(Event::End(BytesEnd::new("hp:linesegarray")))
        .map_err(|e| Hwp5Error::Cfb { detail: format!("write linesegarray end: {e}") })?;
    Ok(())
}

fn local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|byte| *byte == b':').next().unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// Build a small in-memory ZIP whose entries hold the given byte payloads.
    fn make_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let cursor = Cursor::new(Vec::<u8>::new());
        let mut zip = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, payload) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(payload).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn line_segment(
        text_start_position: u32,
        vertical_position: i32,
        line_height: i32,
    ) -> Hwp5ParaLineSeg {
        Hwp5ParaLineSeg {
            text_start_position,
            vertical_position,
            line_height,
            text_height: 1000,
            baseline_distance: 850,
            line_spacing: 600,
            column_start_position: 0,
            segment_width: 20272,
            tag: 393216,
        }
    }

    #[test]
    fn read_capped_rejects_entry_count_over_cap() {
        let entries: Vec<(String, Vec<u8>)> =
            (0..5).map(|i| (format!("f{i}.xml"), b"x".to_vec())).collect();
        let refs: Vec<(&str, &[u8])> =
            entries.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect();
        let zip = make_zip(&refs);
        // Cap of 3 entries < 5 actual → reject before reading payloads.
        let err = RawPackage::read_capped(&zip, MAX_ENTRY_SIZE, MAX_TOTAL_SIZE, 3).unwrap_err();
        assert!(err.to_string().contains("exceeds limit of 3"), "got: {err}");
    }

    #[test]
    fn read_capped_rejects_entry_size_over_cap() {
        // One 100-byte entry against a 10-byte per-entry cap → reject without
        // pre-allocating the central-directory-advertised size.
        let zip = make_zip(&[("big.xml", &[b'a'; 100])]);
        let err = RawPackage::read_capped(&zip, 10, MAX_TOTAL_SIZE, MAX_ENTRIES).unwrap_err();
        assert!(err.to_string().contains("exceeds limit of 10"), "got: {err}");
    }

    #[test]
    fn read_capped_rejects_cumulative_total_over_cap() {
        // Three 40-byte entries (120 bytes total) against a 100-byte total cap,
        // each under a 50-byte per-entry cap → only the cumulative budget trips.
        let zip =
            make_zip(&[("a.xml", &[b'a'; 40]), ("b.xml", &[b'b'; 40]), ("c.xml", &[b'c'; 40])]);
        let err = RawPackage::read_capped(&zip, 50, 100, MAX_ENTRIES).unwrap_err();
        assert!(err.to_string().contains("total decompressed data"), "got: {err}");
    }

    #[test]
    fn read_capped_accepts_normal_small_package() {
        let zip = make_zip(&[("Contents/section0.xml", b"<sec/>"), ("mimetype", b"x")]);
        let pkg = RawPackage::read_capped(&zip, MAX_ENTRY_SIZE, MAX_TOTAL_SIZE, MAX_ENTRIES)
            .expect("normal small package must parse");
        assert_eq!(pkg.entries.len(), 2);
        assert_eq!(pkg.read_text_entry("Contents/section0.xml").unwrap(), "<sec/>");
    }

    #[test]
    fn patch_section_xml_injects_linesegarray_and_table_height() {
        let xml = concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?>"#,
            r#"<hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph" xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section">"#,
            r#"<hp:p id="0" paraPrIDRef="0" styleIDRef="0"><hp:run charPrIDRef="0"><hp:t>P0</hp:t></hp:run></hp:p>"#,
            r#"<hp:p id="1" paraPrIDRef="0" styleIDRef="0"><hp:run charPrIDRef="0"><hp:tbl id="1" rowCnt="1" colCnt="1" cellSpacing="0" borderFillIDRef="2"><hp:sz width="1000" widthRelTo="ABSOLUTE" height="0" heightRelTo="ABSOLUTE" protect="0"/><hp:tr><hp:tc borderFillIDRef="2"><hp:subList><hp:p id="2" paraPrIDRef="0" styleIDRef="0"><hp:run charPrIDRef="0"><hp:t>CELL</hp:t></hp:run></hp:p></hp:subList></hp:tc></hp:tr></hp:tbl></hp:run></hp:p>"#,
            r#"</hs:sec>"#
        );

        let hints = SectionLayoutHints {
            paragraphs: VecDeque::from(vec![
                ParagraphLayoutHint { line_segments: vec![line_segment(0, 0, 1000)] },
                ParagraphLayoutHint { line_segments: vec![line_segment(0, 0, 1000)] },
                ParagraphLayoutHint {
                    line_segments: vec![
                        line_segment(0, 0, 1000),
                        line_segment(20, 1600, 1000),
                        line_segment(48, 3200, 1000),
                    ],
                },
            ]),
            tables: VecDeque::from(vec![TableLayoutHint { height: Some(4482) }]),
        };

        let patched = patch_section_xml(xml, hints).expect("section xml should patch");
        assert!(patched
            .contains(r#"<hp:linesegarray><hp:lineseg textpos="0" vertpos="0" vertsize="1000""#));
        assert!(patched.contains(r#"height="4482""#));
    }

    #[test]
    fn patch_section_xml_skips_table_height_when_hint_is_unknown() {
        let xml = concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?>"#,
            r#"<hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph" xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section">"#,
            r#"<hp:p id="0" paraPrIDRef="0" styleIDRef="0"><hp:run charPrIDRef="0"><hp:tbl id="1" rowCnt="2" colCnt="1" cellSpacing="0" borderFillIDRef="2"><hp:sz width="1000" widthRelTo="ABSOLUTE" height="7777" heightRelTo="ABSOLUTE" protect="0"/><hp:tr><hp:tc borderFillIDRef="2"><hp:subList><hp:p id="1" paraPrIDRef="0" styleIDRef="0"><hp:run charPrIDRef="0"><hp:t>CELL</hp:t></hp:run></hp:p></hp:subList></hp:tc></hp:tr></hp:tbl></hp:run></hp:p>"#,
            r#"</hs:sec>"#
        );

        let hints = SectionLayoutHints {
            paragraphs: VecDeque::from(vec![
                ParagraphLayoutHint { line_segments: vec![line_segment(0, 0, 1000)] },
                ParagraphLayoutHint { line_segments: vec![line_segment(0, 0, 1000)] },
            ]),
            tables: VecDeque::from(vec![TableLayoutHint { height: None }]),
        };

        let patched = patch_section_xml(xml, hints).expect("section xml should patch");
        assert!(patched.contains(r#"height="7777""#));
        assert!(!patched.contains(r#"height="0""#));
    }
}
