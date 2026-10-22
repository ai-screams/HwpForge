//! Applies captured HWP5 layout hints onto generated HWPX bytes.
//!
//! The HWP5 decoder captures per-section [`SectionLayoutHints`] (paragraph line
//! segments + table heights) as a format-neutral value. This module replays
//! them onto the HWPX `section{N}.xml` streams so Hancom Office can open the
//! converted file without flagging a "low-security recovery". The capture side
//! lives in `hwpforge_smithy_hwp5::layout_hint_patch`; only the HWPX-byte
//! rewriting lives here.

use std::collections::{BTreeMap, VecDeque};
use std::io::{Cursor, Write};

use quick_xml::events::{BytesEnd, BytesStart, Event};
use quick_xml::{Reader, Writer};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::error::{LayoutPatchError, LayoutPatchResult};
use hwpforge_core::zip_package::{
    open_package, PackageLimits, MAX_ENTRIES, MAX_ENTRY_SIZE, MAX_TOTAL_SIZE,
};

fn package_error(error: hwpforge_core::CoreError) -> LayoutPatchError {
    let detail = match error {
        hwpforge_core::CoreError::InvalidStructure { reason, .. } => reason,
        other => other.to_string(),
    };
    LayoutPatchError::Package { detail }
}
use hwpforge_smithy_hwp5::layout_hint_patch::{
    ParagraphLayoutHint, SectionLayoutHints, TableLayoutHint,
};

#[derive(Debug, Clone)]
struct RawPackage {
    entries: Vec<RawPackageEntry>,
    index_by_path: BTreeMap<String, usize>,
}

#[derive(Debug, Clone)]
struct RawPackageEntry {
    opaque: bool,
    path: String,
    bytes: Vec<u8>,
    compression: CompressionMethod,
}

#[cfg(test)]
pub(crate) fn patch_hwpx_layout_hints(
    hwpx_bytes: &[u8],
    sections: &[SectionLayoutHints],
) -> LayoutPatchResult<Vec<u8>> {
    patch_hwpx_part_layout_hints(hwpx_bytes, sections, &[])
}

pub(crate) fn patch_hwpx_part_layout_hints(
    hwpx_bytes: &[u8],
    sections: &[SectionLayoutHints],
    masters: &[Vec<SectionLayoutHints>],
) -> LayoutPatchResult<Vec<u8>> {
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
    for (index, master) in masters.iter().flatten().enumerate() {
        if !master.has_payload() {
            continue;
        }
        let path = format!("Contents/masterpage{index}.xml");
        let xml = package.read_text_entry(&path)?;
        let patched = patch_section_xml(&xml, master.clone())?;
        package.replace_text_entry(&path, patched);
    }
    package.write()
}

impl RawPackage {
    fn read(bytes: &[u8]) -> LayoutPatchResult<Self> {
        Self::read_capped(bytes, MAX_ENTRY_SIZE, MAX_TOTAL_SIZE, MAX_ENTRIES)
    }

    /// Internal worker for [`read`] with explicit caps so tests can verify the
    /// bounds with small values instead of materializing multi-MiB entries.
    fn read_capped(
        bytes: &[u8],
        max_entry: u64,
        max_total: u64,
        max_entries: usize,
    ) -> LayoutPatchResult<Self> {
        let mut archive = open_package(
            bytes,
            PackageLimits { entry: max_entry, total: max_total, entries: max_entries },
        )
        .map_err(package_error)?;
        let mut entries = Vec::with_capacity(archive.entries().len());
        let mut index_by_path = BTreeMap::new();
        for index in 0..archive.entries().len() {
            let entry = archive.entries()[index].clone();
            let bytes = archive.read(index).map_err(package_error)?;
            index_by_path.insert(entry.path.clone(), entries.len());
            entries.push(RawPackageEntry {
                opaque: archive.is_opaque(index),
                path: entry.path,
                bytes,
                compression: entry.compression,
            });
        }

        Ok(Self { entries, index_by_path })
    }

    fn read_text_entry(&self, path: &str) -> LayoutPatchResult<String> {
        let index = self
            .index_by_path
            .get(path)
            .copied()
            .ok_or_else(|| LayoutPatchError::MissingEntry { name: path.to_string() })?;
        if self.entries[index].opaque {
            return Err(LayoutPatchError::Package {
                detail: format!("opaque ZIP entry selected semantically: '{path}'"),
            });
        }
        String::from_utf8(self.entries[index].bytes.clone()).map_err(|e| {
            LayoutPatchError::Package { detail: format!("entry '{path}' is not valid UTF-8: {e}") }
        })
    }

    fn replace_text_entry(&mut self, path: &str, content: String) {
        if let Some(index) = self.index_by_path.get(path).copied() {
            self.entries[index].bytes = content.into_bytes();
        }
    }

    fn write(&self) -> LayoutPatchResult<Vec<u8>> {
        let cursor = Cursor::new(Vec::<u8>::new());
        let mut zip = ZipWriter::new(cursor);
        for entry in &self.entries {
            // W0: fixed entry time (the ZIP epoch), never the clock.
            let options = SimpleFileOptions::default()
                .compression_method(entry.compression)
                .last_modified_time(zip::DateTime::default());
            zip.start_file(&entry.path, options).map_err(|e| LayoutPatchError::Package {
                detail: format!("zip start '{}': {e}", entry.path),
            })?;
            zip.write_all(&entry.bytes).map_err(|e| LayoutPatchError::Package {
                detail: format!("zip write '{}': {e}", entry.path),
            })?;
        }
        let cursor = zip.finish().map_err(|e| LayoutPatchError::Package {
            detail: format!("finish hwpx patch package: {e}"),
        })?;
        Ok(cursor.into_inner())
    }
}

#[derive(Debug)]
struct SectionXmlPatchState {
    hints: SectionLayoutHints,
    /// Header and footer table hints, split off the end of `hints.tables`
    /// ([`Self::split_table_scopes`]); `hints.tables` keeps the body's.
    header_tables: VecDeque<TableLayoutHint>,
    footer_tables: VecDeque<TableLayoutHint>,
    element_stack: Vec<String>,
    paragraph_stack: Vec<ParagraphLayoutHint>,
    table_stack: Vec<TableLayoutHint>,
}

impl SectionXmlPatchState {
    fn new(hints: SectionLayoutHints) -> Self {
        Self {
            hints,
            header_tables: VecDeque::new(),
            footer_tables: VecDeque::new(),
            element_stack: Vec::new(),
            paragraph_stack: Vec::new(),
            table_stack: Vec::new(),
        }
    }

    /// Splits the table hints (body, then headers, then footers) by the
    /// number of tables of each scope in `xml`: the encoder writes a
    /// header/footer into its anchor paragraph, so the scopes' tables
    /// interleave in the XML while each scope's own order is fixed.
    ///
    /// 이것을 실패시키는 것: 나누지 않고 모든 표를 한 큐에서 꺼내기
    /// (`patch_section_xml_takes_header_tables_from_the_header_queue`).
    fn split_table_scopes(&mut self, xml: &str) -> LayoutPatchResult<()> {
        let (mut headers, mut footers) = (0usize, 0usize);
        let mut scopes: Vec<String> = Vec::new();
        let mut reader = Reader::from_str(xml);
        loop {
            match reader.read_event().map_err(|e| LayoutPatchError::Package {
                detail: format!("parse generated section xml: {e}"),
            })? {
                Event::Start(event) => {
                    let local = local_name(event.name().as_ref()).to_string();
                    if local == "tbl" {
                        match scopes
                            .iter()
                            .rev()
                            .find(|n| matches!(n.as_str(), "header" | "footer"))
                        {
                            Some(scope) if scope == "header" => headers += 1,
                            Some(_) => footers += 1,
                            None => {}
                        }
                    }
                    scopes.push(local);
                }
                Event::End(_) => {
                    scopes.pop();
                }
                Event::Eof => break,
                _ => {}
            }
        }
        let scoped = headers + footers;
        let body = self.hints.tables.len().checked_sub(scoped).ok_or_else(|| {
            LayoutPatchError::Package { detail: "table layout hint count underflow".into() }
        })?;
        let mut rest = self.hints.tables.split_off(body);
        self.footer_tables = rest.split_off(headers);
        self.header_tables = rest;
        Ok(())
    }

    fn handle_start<W: Write>(
        &mut self,
        event: BytesStart<'_>,
        writer: &mut Writer<W>,
    ) -> LayoutPatchResult<()> {
        let local = local_name(event.name().as_ref()).to_string();
        if local == "p" {
            self.push_paragraph_hint()?;
        } else if local == "tbl" {
            self.push_table_hint()?;
        }

        let event = self.patch_table_size_event(&local, event.into_owned())?;
        writer.write_event(Event::Start(event)).map_err(|e| LayoutPatchError::Package {
            detail: format!("write patched section xml: {e}"),
        })?;
        self.element_stack.push(local);
        Ok(())
    }

    fn handle_empty<W: Write>(
        &mut self,
        event: BytesStart<'_>,
        writer: &mut Writer<W>,
    ) -> LayoutPatchResult<()> {
        let local = local_name(event.name().as_ref()).to_string();
        let event = self.patch_table_size_event(&local, event.into_owned())?;
        writer.write_event(Event::Empty(event)).map_err(|e| LayoutPatchError::Package {
            detail: format!("write patched section xml: {e}"),
        })?;
        Ok(())
    }

    fn handle_end<W: Write>(
        &mut self,
        event: BytesEnd<'_>,
        writer: &mut Writer<W>,
    ) -> LayoutPatchResult<()> {
        let local = local_name(event.name().as_ref()).to_string();
        if local == "p" {
            self.pop_paragraph_hint()?;
        }

        writer.write_event(Event::End(event.into_owned())).map_err(|e| {
            LayoutPatchError::Package { detail: format!("write patched section xml: {e}") }
        })?;

        self.pop_element(&local)?;
        if local == "tbl" {
            self.table_stack.pop();
        }
        Ok(())
    }

    fn finish(self) -> LayoutPatchResult<()> {
        if !self.hints.paragraphs.is_empty()
            || !self.hints.tables.is_empty()
            || !self.header_tables.is_empty()
            || !self.footer_tables.is_empty()
            || !self.paragraph_stack.is_empty()
        {
            return Err(LayoutPatchError::Package {
                detail: "layout hint patch left unconsumed hints".into(),
            });
        }
        Ok(())
    }

    fn patch_table_size_event(
        &self,
        local: &str,
        event: BytesStart<'static>,
    ) -> LayoutPatchResult<BytesStart<'static>> {
        if !self.is_active_table_size_element(local) {
            return Ok(event);
        }

        let Some(height) = self.active_table_height()? else {
            return Ok(event);
        };
        rewrite_element_attr(event, "height", &height.to_string())
    }

    fn is_active_table_size_element(&self, local: &str) -> bool {
        local == "sz" && self.element_stack.last().is_some_and(|parent| parent == "tbl")
    }

    fn active_table_height(&self) -> LayoutPatchResult<Option<i32>> {
        self.table_stack
            .last()
            .copied()
            .ok_or_else(|| LayoutPatchError::Package {
                detail: "table size encountered without active table hint".into(),
            })
            .map(|hint| hint.height)
    }

    fn push_paragraph_hint(&mut self) -> LayoutPatchResult<()> {
        let hint = self.hints.paragraphs.pop_front().ok_or_else(|| LayoutPatchError::Package {
            detail: "paragraph layout hint count underflow".into(),
        })?;
        self.paragraph_stack.push(hint);
        Ok(())
    }

    /// Takes the next table hint of the scope being streamed: the innermost
    /// enclosing `<hp:header>`/`<hp:footer>`, else the body. The encoder
    /// writes a header/footer into its anchor paragraph, so header and body
    /// tables interleave in the XML; each scope's own order is fixed.
    ///
    /// 이것을 실패시키는 것: 모든 표를 본문 큐에서 꺼내기
    /// (`patch_section_xml_takes_header_tables_from_the_header_queue`).
    fn push_table_hint(&mut self) -> LayoutPatchResult<()> {
        let scope = self
            .element_stack
            .iter()
            .rev()
            .find(|name| matches!(name.as_str(), "header" | "footer"))
            .map(String::as_str);
        let queue = match scope {
            Some("header") => &mut self.header_tables,
            Some(_) => &mut self.footer_tables,
            None => &mut self.hints.tables,
        };
        let hint = queue.pop_front().ok_or_else(|| LayoutPatchError::Package {
            detail: "table layout hint count underflow".into(),
        })?;
        self.table_stack.push(hint);
        Ok(())
    }

    fn pop_paragraph_hint(&mut self) -> LayoutPatchResult<ParagraphLayoutHint> {
        self.paragraph_stack.pop().ok_or_else(|| LayoutPatchError::Package {
            detail: "paragraph layout hint stack underflow".into(),
        })
    }

    fn pop_element(&mut self, local: &str) -> LayoutPatchResult<()> {
        let popped = self.element_stack.pop().ok_or_else(|| LayoutPatchError::Package {
            detail: "xml element stack underflow".into(),
        })?;
        if popped != local {
            return Err(LayoutPatchError::Package {
                detail: format!("xml element stack mismatch: opened '{popped}' closed '{local}'"),
            });
        }
        Ok(())
    }
}

fn patch_section_xml(xml: &str, hints: SectionLayoutHints) -> LayoutPatchResult<String> {
    let mut reader = Reader::from_str(xml);
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(xml.len() + 1024)));
    let mut buf = Vec::new();
    let mut state = SectionXmlPatchState::new(hints);
    state.split_table_scopes(xml)?;

    loop {
        match reader.read_event_into(&mut buf).map_err(|e| LayoutPatchError::Package {
            detail: format!("parse generated section xml: {e}"),
        })? {
            Event::Start(event) => state.handle_start(event, &mut writer)?,
            Event::Empty(event) => state.handle_empty(event, &mut writer)?,
            Event::End(event) => state.handle_end(event, &mut writer)?,
            Event::Eof => break,
            event => {
                writer.write_event(event.into_owned()).map_err(|e| LayoutPatchError::Package {
                    detail: format!("write patched section xml: {e}"),
                })?;
            }
        }
        buf.clear();
    }

    state.finish()?;

    let bytes = writer.into_inner().into_inner();
    String::from_utf8(bytes).map_err(|e| LayoutPatchError::Package {
        detail: format!("patched section xml is not valid UTF-8: {e}"),
    })
}

fn rewrite_element_attr(
    event: BytesStart<'static>,
    target_attr: &str,
    new_value: &str,
) -> LayoutPatchResult<BytesStart<'static>> {
    let mut rebuilt = BytesStart::new(event.name().as_ref().to_string());
    let mut replaced = false;

    for attr in event.attributes().with_checks(false) {
        let attr = attr.map_err(|e| LayoutPatchError::Package {
            detail: format!("read xml attribute: {e}"),
        })?;
        let key: &str = attr.key.as_ref();
        if local_name(key) == target_attr {
            replaced = true;
            rebuilt.push_attribute((key, new_value));
        } else {
            // `attr.value` is already escaped as written; pass the Attribute itself so it is not escaped again.
            rebuilt.push_attribute(attr);
        }
    }

    if !replaced {
        rebuilt.push_attribute((target_attr, new_value));
    }

    Ok(rebuilt)
}

fn local_name(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_smithy_hwp5::schema::section::Hwp5ParaLineSeg;
    use std::collections::VecDeque;
    use zip::ZipArchive;

    fn rewrite_empty_element(xml: &str, target: &str, value: &str) -> String {
        let mut reader = Reader::from_str(xml);
        let event = match reader.read_event().unwrap() {
            Event::Empty(e) => e.into_owned(),
            other => panic!("expected empty element, got {other:?}"),
        };
        let rebuilt = rewrite_element_attr(event, target, value).unwrap();
        let mut writer = Writer::new(Cursor::new(Vec::new()));
        writer.write_event(Event::Empty(rebuilt)).unwrap();
        String::from_utf8(writer.into_inner().into_inner()).unwrap()
    }

    #[test]
    fn rewrite_element_attr_keeps_untouched_attributes_as_written() {
        // 이것을 실패시키는 것: 비대상 속성을 `push_attribute((key, &str))` 로 다시 쓰는 변경
        // (`&amp;` 가 `&amp;amp;` 로 이중 escape 됨)
        let out = rewrite_empty_element(
            r#"<hp:sz width="10" note="a&amp;b&lt;c" height="5"/>"#,
            "height",
            "9999",
        );
        assert_eq!(out, r#"<hp:sz width="10" note="a&amp;b&lt;c" height="9999"/>"#);
    }

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

    /// 이것을 실패시키는 것: layout-hint 읽기가 공용 ZIP 이름 검사를 우회하기.
    #[test]
    fn zip9_fix3_layout_rejects_parent_part() {
        let bytes = make_zip(&[("Contents/../unused.xml", b"x")]);
        assert!(matches!(patch_hwpx_layout_hints(&bytes, &[]),
            Err(LayoutPatchError::Package { detail })
            if detail == "invalid package part name 'Contents/../unused.xml'"));
    }

    // 이것을 실패시키는 것: convert RawPackage에서 core opaque 표시 전달 또는 의미 읽기 검사를 제거.
    // 이것을 실패시키는 것: convert 복사 목록에서 빈 opaque 폴더를 제외하기.
    // 이번 보강을 실패시키는 것: opaque payload 대신 빈 바이트를 저장하기.
    #[test]
    fn w2_convert_raw_opaque_semantic_gate_and_copy() {
        for names in
            [vec!["Unused/./a.bin"], vec!["Unused/./"], vec!["Unused", "Unused/"], vec!["Unused/"]]
        {
            let payload = if names[0] == "Unused/./" { b"".as_slice() } else { b"x".as_slice() };
            let specs: Vec<_> = names.iter().map(|name| (*name, payload)).collect();
            let bytes = make_zip(&specs);
            let package = RawPackage::read(&bytes).unwrap();
            for name in &names {
                assert!(package
                    .read_text_entry(name)
                    .unwrap_err()
                    .to_string()
                    .contains("opaque ZIP"));
            }
            let output = patch_hwpx_layout_hints(&bytes, &[]).unwrap();
            let mut copy = open_package(&output, Default::default()).unwrap();
            assert_eq!(copy.entries().len(), names.len());
            for (index, name) in names.iter().enumerate() {
                assert_eq!(copy.entries()[index].path, *name);
                assert_eq!(copy.read(index).unwrap(), payload);
                assert!(copy.is_opaque(index));
            }
        }
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

    /// W0: a rewritten package keeps every entry at the fixed ZIP epoch.
    /// 이것을 실패시키는 것: 시각 고정 삭제 — `--features zip/time` 으로 돌릴 때
    /// (그 기능이 꺼진 빌드에서는 zip 기본값이 같은 epoch 라 통과한다).
    #[test]
    fn rewritten_entries_keep_a_fixed_time() {
        let zip = make_zip(&[("Contents/section0.xml", b"<sec/>"), ("mimetype", b"x")]);
        let out = RawPackage::read(&zip).unwrap().write().unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(out)).unwrap();
        for i in 0..archive.len() {
            let entry = archive.by_index(i).unwrap();
            assert_eq!(
                entry.last_modified(),
                Some(zip::DateTime::default()),
                "{}",
                entry.name().expect("ZIP entry name")
            );
        }
    }

    #[test]
    fn read_capped_accepts_normal_small_package() {
        let zip = make_zip(&[("Contents/section0.xml", b"<sec/>"), ("mimetype", b"x")]);
        let pkg = RawPackage::read_capped(&zip, MAX_ENTRY_SIZE, MAX_TOTAL_SIZE, MAX_ENTRIES)
            .expect("normal small package must parse");
        assert_eq!(pkg.entries.len(), 2);
        assert_eq!(pkg.read_text_entry("Contents/section0.xml").unwrap(), "<sec/>");
    }

    /// 이것을 실패시키는 것: 디코딩 이름 중복 검사 제거 또는 raw 이름만 비교하기.
    #[test]
    fn zip9_layout_rejects_ambiguous_decoded_names() {
        let mut bytes = make_zip(&[("BinData/cafX.png", b"first"), ("BinData/cafYZ.png", b"last")]);
        let offsets: Vec<_> = {
            let mut archive = ZipArchive::new(Cursor::new(&bytes)).expect("archive");
            (0..archive.len())
                .map(|index| {
                    let entry = archive.by_index(index).expect("entry");
                    (entry.header_start() as usize, entry.central_header_start() as usize)
                })
                .collect()
        };
        for (name, (local, central)) in
            [b"BinData/caf\x82.png".as_slice(), "BinData/café.png".as_bytes()]
                .into_iter()
                .zip(offsets)
        {
            bytes[local + 30..local + 30 + name.len()].copy_from_slice(name);
            bytes[central + 46..central + 46 + name.len()].copy_from_slice(name);
        }
        let err = RawPackage::read(&bytes).expect_err("ambiguous name");
        assert!(matches!(err, LayoutPatchError::Package { detail }
            if detail == "duplicate decoded ZIP entry name 'BinData/café.png'"));
    }

    /// A header anchored to paragraph 0 comes before the body table in the
    /// XML; its table takes the header queue's hint and the body table the
    /// body's. 이것을 실패시키는 것: 모든 표를 본문 큐에서 꺼내기(높이가
    /// 뒤바뀌거나 underflow).
    #[test]
    fn patch_section_xml_takes_header_tables_from_the_header_queue() {
        let table = |id: u32| {
            format!(
                r#"<hp:tbl id="{id}" rowCnt="1" colCnt="1"><hp:sz width="1000" height="0"/><hp:tr><hp:tc><hp:subList><hp:p id="9"><hp:run charPrIDRef="0"><hp:t>c</hp:t></hp:run></hp:p></hp:subList></hp:tc></hp:tr></hp:tbl>"#
            )
        };
        let xml = format!(
            r#"<hs:sec xmlns:hp="p" xmlns:hs="s"><hp:p id="0"><hp:run charPrIDRef="0"><hp:ctrl><hp:header><hp:subList><hp:p id="1"><hp:run charPrIDRef="0">{}</hp:run></hp:p></hp:subList></hp:header></hp:ctrl></hp:run><hp:run charPrIDRef="0">{}</hp:run></hp:p></hs:sec>"#,
            table(1),
            table(2)
        );
        let hint = || ParagraphLayoutHint { line_segments: Vec::new() };
        let hints = SectionLayoutHints {
            paragraphs: VecDeque::from(vec![hint(), hint(), hint(), hint()]),
            // Body first, then the header's: the scopes' order in the queue.
            tables: VecDeque::from(vec![
                TableLayoutHint { height: Some(111) },
                TableLayoutHint { height: Some(222) },
            ]),
        };
        let patched = patch_section_xml(&xml, hints).expect("patch");
        let header_h = patched.find(r#"height="222""#).expect("header table height");
        let body_h = patched.find(r#"height="111""#).expect("body table height");
        assert!(header_h < body_h, "{patched}");
    }

    #[test]
    fn patch_section_xml_patches_table_height_without_linesegarray() {
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
        // Table height must still be patched.
        assert!(patched.contains(r#"height="4482""#), "table height must be patched: {patched}");
        // No linesegarray must be emitted at all.
        assert!(
            !patched.contains("hp:linesegarray"),
            "linesegarray must NOT be emitted: {patched}"
        );
        assert!(!patched.contains("hp:lineseg"), "lineseg must NOT be emitted: {patched}");
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

    // ── has_payload = false → patch_hwpx_layout_hints skips the section ────────

    #[test]
    fn patch_hwpx_layout_hints_skips_section_with_no_payload() {
        // A section with empty paragraphs and tables has has_payload()=false.
        // The patcher must skip it entirely — no "missing stream" error even
        // when the ZIP does not contain that section file.
        let zip = make_zip(&[("Contents/section0.xml", b"<sec/>")]);

        let sections = vec![
            SectionLayoutHints { paragraphs: VecDeque::new(), tables: VecDeque::new() }, // no payload → skip
        ];

        // Must not error even though there is no "Contents/section0.xml" to
        // read — has_payload=false means we never attempt to read it.
        let result = patch_hwpx_layout_hints(&zip, &sections);
        assert!(result.is_ok(), "no-payload section must be skipped: {result:?}");
    }

    // ── read_text_entry errors on missing path ─────────────────────────────────

    #[test]
    fn read_text_entry_returns_error_for_missing_path() {
        let zip = make_zip(&[("Contents/section0.xml", b"<sec/>")]);
        let pkg = RawPackage::read(&zip).expect("parse zip");
        let err = pkg.read_text_entry("Contents/section99.xml").unwrap_err();
        assert!(
            err.to_string().contains("section99"),
            "error must mention the missing path: {err}"
        );
    }

    // ── replace_text_entry is a no-op for an absent path ─────────────────────

    #[test]
    fn replace_text_entry_is_noop_for_absent_path() {
        let zip = make_zip(&[("Contents/section0.xml", b"<sec/>")]);
        let mut pkg = RawPackage::read(&zip).expect("parse zip");
        let before = pkg.entries.len();
        // This path does not exist — must not panic or add an entry.
        pkg.replace_text_entry("Contents/section99.xml", "<new/>".into());
        assert_eq!(pkg.entries.len(), before, "entry count must be unchanged");
    }

    // ── handle_empty exercises the Event::Empty branch ───────────────────────

    #[test]
    fn patch_section_xml_processes_self_closing_elements() {
        // A self-closing element (not <hp:p> or <hp:tbl>) must pass through
        // unchanged, exercising the handle_empty code path.
        let xml = concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?>"#,
            r#"<hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph" xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section">"#,
            r#"<hp:p id="0" paraPrIDRef="0" styleIDRef="0"><hp:run charPrIDRef="0"><hp:foo/><hp:t>hi</hp:t></hp:run></hp:p>"#,
            r#"</hs:sec>"#
        );

        let hints = SectionLayoutHints {
            paragraphs: VecDeque::from(vec![ParagraphLayoutHint {
                line_segments: vec![line_segment(0, 0, 1000)],
            }]),
            tables: VecDeque::new(),
        };

        let patched = patch_section_xml(xml, hints).expect("must patch");
        // The self-closing <hp:foo/> element must be preserved.
        assert!(patched.contains("hp:foo"), "self-closing element must survive patching");
    }

    // ── finish errors when unconsumed paragraph hints remain ─────────────────

    #[test]
    fn finish_errors_when_paragraph_hints_remain_unconsumed() {
        // Two hints for a section with only one <hp:p> → finish() must error.
        let xml = concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?>"#,
            r#"<hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph" xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section">"#,
            r#"<hp:p id="0" paraPrIDRef="0" styleIDRef="0"><hp:run charPrIDRef="0"><hp:t>X</hp:t></hp:run></hp:p>"#,
            r#"</hs:sec>"#
        );

        let hints = SectionLayoutHints {
            paragraphs: VecDeque::from(vec![
                ParagraphLayoutHint { line_segments: vec![line_segment(0, 0, 1000)] },
                ParagraphLayoutHint { line_segments: vec![line_segment(0, 0, 1000)] }, // surplus
            ]),
            tables: VecDeque::new(),
        };

        let err = patch_section_xml(xml, hints).unwrap_err();
        assert!(
            err.to_string().contains("unconsumed hints"),
            "must error on unconsumed hints: {err}"
        );
    }

    // ── paragraph hint underflow ──────────────────────────────────────────────

    #[test]
    fn patch_section_xml_errors_on_paragraph_hint_underflow() {
        // Zero hints for a section with one <hp:p> → push_paragraph_hint underflow.
        let xml = concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?>"#,
            r#"<hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph" xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section">"#,
            r#"<hp:p id="0" paraPrIDRef="0" styleIDRef="0"><hp:run charPrIDRef="0"><hp:t>X</hp:t></hp:run></hp:p>"#,
            r#"</hs:sec>"#
        );

        let hints = SectionLayoutHints { paragraphs: VecDeque::new(), tables: VecDeque::new() };

        let err = patch_section_xml(xml, hints).unwrap_err();
        assert!(
            err.to_string().contains("underflow"),
            "must error on paragraph hint underflow: {err}"
        );
    }

    // ── table hint underflow ──────────────────────────────────────────────────

    #[test]
    fn patch_section_xml_errors_on_table_hint_underflow() {
        // A section with a table but zero table hints → push_table_hint underflow.
        let xml = concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?>"#,
            r#"<hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph" xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section">"#,
            r#"<hp:p id="0" paraPrIDRef="0" styleIDRef="0"><hp:run charPrIDRef="0">"#,
            r#"<hp:tbl id="1" rowCnt="1" colCnt="1" cellSpacing="0" borderFillIDRef="2">"#,
            r#"<hp:sz width="1000" widthRelTo="ABSOLUTE" height="0" heightRelTo="ABSOLUTE" protect="0"/>"#,
            r#"<hp:tr><hp:tc borderFillIDRef="2"><hp:subList>"#,
            r#"<hp:p id="1" paraPrIDRef="0" styleIDRef="0"><hp:run charPrIDRef="0"><hp:t>C</hp:t></hp:run></hp:p>"#,
            r#"</hp:subList></hp:tc></hp:tr></hp:tbl></hp:run></hp:p>"#,
            r#"</hs:sec>"#
        );

        let hints = SectionLayoutHints {
            paragraphs: VecDeque::from(vec![
                ParagraphLayoutHint { line_segments: vec![line_segment(0, 0, 1000)] },
                ParagraphLayoutHint { line_segments: vec![line_segment(0, 0, 1000)] },
            ]),
            tables: VecDeque::new(), // no table hints → underflow
        };

        let err = patch_section_xml(xml, hints).unwrap_err();
        assert!(err.to_string().contains("underflow"), "must error on table hint underflow: {err}");
    }

    // ── pop_element stack underflow ───────────────────────────────────────────

    #[test]
    fn patch_section_xml_errors_on_element_stack_underflow() {
        // A malformed closing tag with no matching open exercises the
        // element_stack.pop() underflow path.
        // We directly call pop_element on an empty state to trigger it.
        let mut state = SectionXmlPatchState::new(SectionLayoutHints {
            paragraphs: VecDeque::new(),
            tables: VecDeque::new(),
        });
        let err = state.pop_element("run").unwrap_err();
        assert!(
            err.to_string().contains("underflow"),
            "empty element stack pop must give underflow error: {err}"
        );
    }

    // ── pop_element mismatch ──────────────────────────────────────────────────

    #[test]
    fn patch_section_xml_errors_on_element_stack_mismatch() {
        // Push "p", then try to pop "run" → mismatch.
        let mut state = SectionXmlPatchState::new(SectionLayoutHints {
            paragraphs: VecDeque::new(),
            tables: VecDeque::new(),
        });
        state.element_stack.push("p".to_string());
        let err = state.pop_element("run").unwrap_err();
        assert!(
            err.to_string().contains("mismatch"),
            "mismatched element names must give mismatch error: {err}"
        );
    }

    // ── rewrite_element_attr inserts attribute when absent ────────────────────

    #[test]
    fn rewrite_element_attr_inserts_attribute_when_absent() {
        // An element without a "height" attribute gets one injected.
        let event = BytesStart::new("hp:sz").into_owned();
        let result = rewrite_element_attr(event, "height", "9999").expect("rewrite must succeed");
        let name = result.name();
        let xml = format!("<{} />", name.as_ref());
        // Attribute must be present in the rebuilt element.
        let attrs: Vec<_> = result.attributes().with_checks(false).filter_map(|a| a.ok()).collect();
        assert!(
            attrs.iter().any(|a| { a.key.as_ref() == "height" && a.value == "9999" }),
            "injected 'height' attribute must be present; xml={xml}"
        );
    }

    // ── no linesegarray emitted even when line_segments is empty ─────────────

    #[test]
    fn patch_section_xml_emits_no_linesegarray_for_empty_line_segments() {
        // A ParagraphLayoutHint with no line_segments must NOT emit any
        // lineseg or linesegarray — Hancom recomputes layout itself.
        let xml = concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?>"#,
            r#"<hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph" xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section">"#,
            r#"<hp:p id="0" paraPrIDRef="0" styleIDRef="0"><hp:run charPrIDRef="0"><hp:t>X</hp:t></hp:run></hp:p>"#,
            r#"</hs:sec>"#
        );

        let hints = SectionLayoutHints {
            paragraphs: VecDeque::from(vec![ParagraphLayoutHint { line_segments: vec![] }]),
            tables: VecDeque::new(),
        };

        let patched = patch_section_xml(xml, hints).expect("must patch");
        assert!(
            !patched.contains("hp:linesegarray"),
            "linesegarray must NOT be emitted for empty line_segments: {patched}"
        );
        assert!(
            !patched.contains("hp:lineseg"),
            "lineseg must NOT be emitted for empty line_segments: {patched}"
        );
    }

    // ── regression: non-empty line_segments also produce no linesegarray ─────

    #[test]
    fn patch_section_xml_emits_no_linesegarray_for_non_empty_line_segments() {
        // Even when the ParagraphLayoutHint carries multiple line segments from
        // HWP5, no <hp:linesegarray> must be written — stale HWP5 positions
        // would cause Hancom to overlap text on a single line.
        let xml = concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?>"#,
            r#"<hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph" xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section">"#,
            r#"<hp:p id="0" paraPrIDRef="0" styleIDRef="0"><hp:run charPrIDRef="0"><hp:t>X</hp:t></hp:run></hp:p>"#,
            r#"</hs:sec>"#
        );

        let hints = SectionLayoutHints {
            paragraphs: VecDeque::from(vec![ParagraphLayoutHint {
                line_segments: vec![
                    line_segment(0, 0, 1000),
                    line_segment(20, 1600, 1000),
                    line_segment(48, 3200, 1000),
                ],
            }]),
            tables: VecDeque::new(),
        };

        let patched = patch_section_xml(xml, hints).expect("must patch");
        assert!(
            !patched.contains("hp:linesegarray"),
            "linesegarray must NOT be emitted even with non-empty line_segments: {patched}"
        );
        assert!(
            !patched.contains("hp:lineseg"),
            "lineseg must NOT be emitted even with non-empty line_segments: {patched}"
        );
    }
    // 이것을 실패시키는 것: master 글로벌 번호를 구역마다 초기화 / section과 master 표 큐 혼합 / 빈 hint에서 번호 미소비.
    #[test]
    fn master_table_hints_are_part_local_across_sections() {
        let xml = br#"<hm:masterPage><hp:p><hp:run><hp:tbl><hp:sz height="1"/></hp:tbl></hp:run></hp:p></hm:masterPage>"#;
        let zip = make_zip(&[
            ("Contents/section0.xml", xml),
            ("Contents/masterpage0.xml", b"<master/>"),
            ("Contents/masterpage1.xml", xml),
            ("Contents/masterpage2.xml", xml),
        ]);
        let hints = |height| SectionLayoutHints {
            paragraphs: VecDeque::from([ParagraphLayoutHint { line_segments: vec![] }]),
            tables: VecDeque::from([TableLayoutHint { height: Some(height) }]),
        };
        let empty = SectionLayoutHints { paragraphs: VecDeque::new(), tables: VecDeque::new() };
        let bytes = patch_hwpx_part_layout_hints(
            &zip,
            &[hints(111)],
            &[vec![empty, hints(222)], vec![hints(333)]],
        )
        .unwrap();
        let package = RawPackage::read(&bytes).unwrap();
        for (part, height) in [("section0", 111), ("masterpage1", 222), ("masterpage2", 333)] {
            assert!(package
                .read_text_entry(&format!("Contents/{part}.xml"))
                .unwrap()
                .contains(&format!("height=\"{height}\"")));
        }
        assert_eq!(package.read_text_entry("Contents/masterpage0.xml").unwrap(), "<master/>");
    }
}
