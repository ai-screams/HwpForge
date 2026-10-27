//! Bounded ZIP framing before zip merges effective-name keys.
//!
//! First return the unchanged 5f3e214e path on success (see `legacy`).
//! Only its failures reach the bounded recovery below.
//! Selection follows zip 9.0.4 `format/find_central_directory.rs`: EOCD
//! backwards (comment end <= EOF), ZIP32's first CDFH at/after its relative
//! offset. ZIP64 recovery is deferred; existing ZIP64 acceptance stays in
//! `legacy`. We fail closed when the selected directory disagrees with our
//! framing policy.
//! `read/zip_archive.rs::get_metadata` uses the in-memory EOCD size for its
//! tail window (not the 22-byte on-disk header); see EOCD_WINDOW below.
//!
//! Public APIs expose start/base/merged count, but not the selected footer,
//! declared size, original names or unmerged count (`format` is private).
//! Keep only framing/original names here; zip decodes extras and contents.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::rc::Rc;

use zip::read::{ArchiveOffset, Config};
use zip::ZipArchive;

use super::{invalid_structure, zip_error};
use crate::error::{CoreError, CoreResult};

// zip 9.0.4: sizeof(Zip32CentralDirectoryEnd) + sizeof(Magic) + 65536.
// Its private struct has four u16s, two u32s and a Box<[u8]> (no padding).
// Thus 65572 bytes on 64-bit, 65564 on 32-bit; do not substitute disk size 22.
const EOCD_WINDOW: usize = 20 + std::mem::size_of::<Box<[u8]>>() + 65536;
// OWPML unzlocal_SearchCentralDir: uMaxBack = 0xffff (including the footer).
const REFERENCE_WINDOW: usize = 65_535;

mod legacy;

/// Preserve the complete 0.17.0 acceptance path before trying any new rules.
pub(super) fn open(bytes: &[u8], limit: usize) -> CoreResult<ZipArchive<Cursor<&[u8]>>> {
    let baseline =
        legacy::preflight(bytes, limit).and_then(|directory| directory.open(Cursor::new(bytes)));
    match baseline {
        // 이것을 실패시키는 것: 기존 성공 뒤 새 예산/위치 규칙 적용 또는 기존 경로 생략.
        Ok(archive) => Ok(archive),
        Err(error) => preflight(bytes, limit)
            .and_then(|directory| directory.open(Cursor::new(bytes)))
            // 이것을 실패시키는 것: 두 경로 실패 시 기준선 오류 대신 새 오류 반환.
            .map_err(|_| error),
    }
}

#[derive(Debug)]
pub(super) struct Directory {
    pub start: usize,
    pub size: usize,
    pub count: usize,
    pub base: usize,
    pub eocd: usize,
    comment: Vec<u8>,
    reference: Option<ReferenceEnd>,
    work: Rc<RefCell<MetadataWork>>,
}

/// Raw reference selection, independent of zip candidate eligibility.
#[derive(Debug)]
struct ReferenceEnd {
    eocd: usize,
}

impl Directory {
    pub(super) fn open<R: Read + Seek + Clone>(&self, mut reader: R) -> CoreResult<ZipArchive<R>> {
        let file_len = reader.seek(SeekFrom::End(0)).map_err(|e| zip_error(e.to_string()))?;
        let mut reader =
            MetadataReader { reader, end: file_len, footer: None, work: self.work.clone() };
        reader
            .seek(SeekFrom::Start((self.eocd + 20) as u64))
            .map_err(|e| zip_error(e.to_string()))?;
        let mut length = [0; 2];
        reader.read_exact(&mut length).map_err(|e| invalid_structure(e.to_string()))?;
        if usize::from(u16::from_le_bytes(length)) != self.comment.len() {
            return Err(invalid());
        }
        let reference = self.reference.as_ref().ok_or_else(invalid)?;
        if reference.eocd != self.eocd {
            return Err(invalid());
        }
        let config = Config { archive_offset: ArchiveOffset::Known(self.base as u64) };
        let result = ZipArchive::with_config(config, reader);
        self.work.borrow().check()?;
        let archive = result.map_err(|e| zip_error(e.to_string()))?;
        let consumed_end =
            archive.clone().into_inner().stream_position().map_err(|e| zip_error(e.to_string()))?;
        self.verify(&archive, consumed_end, file_len)?;
        // SAFETY: this is the very same original reader that produced metadata;
        // only the accounting wrapper is removed. A virtual footer is never used here.
        let metadata = archive.metadata();
        let reader = archive.into_inner().reader;
        #[allow(unsafe_code)]
        let archive = unsafe { ZipArchive::unsafe_new_with_metadata(reader, metadata) };
        Ok(archive)
    }

    fn verify<R: Read + Seek>(
        &self,
        archive: &ZipArchive<R>,
        consumed_end: u64,
        file_len: u64,
    ) -> CoreResult<()> {
        // 이것을 실패시키는 것: 시작/크기/base/EOCD 위치 중 하나의 대조를 삭제하기.
        if (self.eocd + 22 + self.comment.len()) as u64 > file_len
            || archive.comment() != self.comment
            || !self.matches_directory(archive, consumed_end)
        {
            return Err(zip_error("central directory interpretation mismatch"));
        }
        self.verify_count(archive.len())
    }

    // One comparison for candidate recovery and the final original-file open.
    fn matches_directory<R: Read + Seek>(
        &self,
        archive: &ZipArchive<R>,
        consumed_end: u64,
    ) -> bool {
        archive.central_directory_start() == self.start as u64
            && archive.offset() == self.base as u64
            && (self.count == 0
                || consumed_end.checked_sub(archive.central_directory_start())
                    == Some(self.size as u64))
    }

    fn verify_count(&self, count: usize) -> CoreResult<()> {
        // 이것을 실패시키는 것: Unicode extra 적용 후 병합된 개수를 허용하기.
        if count != self.count {
            return Err(invalid_structure("duplicate effective ZIP entry name"));
        }
        Ok(())
    }
}

fn invalid() -> CoreError {
    zip_error("invalid ZIP central directory framing")
}

fn field<const N: usize>(bytes: &[u8], offset: usize) -> CoreResult<[u8; N]> {
    bytes
        .get(offset..offset.checked_add(N).ok_or_else(invalid)?)
        .ok_or_else(invalid)?
        .try_into()
        .map_err(|_| invalid())
}
pub(super) fn u16_at(bytes: &[u8], offset: usize) -> CoreResult<u16> {
    Ok(u16::from_le_bytes(field(bytes, offset)?))
}
fn u32_at(bytes: &[u8], offset: usize) -> CoreResult<u32> {
    Ok(u32::from_le_bytes(field(bytes, offset)?))
}
fn check_count(count: u64, limit: usize) -> CoreResult<()> {
    // 이것을 실패시키는 것: 병합 후 len()만 검사하거나 >를 >=로 바꾸기.
    if count > limit as u64 {
        return Err(invalid_structure(format!(
            "archive has {count} entries, exceeds limit of {limit}"
        )));
    }
    Ok(())
}

/// Accounts for every four-byte signature probe, including unsuccessful ones.
/// The tail and all candidate forward scans share 4*(file_len+EOCD_WINDOW).
#[derive(Default, Debug)]
pub(super) struct Work {
    pub scanned_bytes: usize,
    #[cfg(test)]
    pub candidates: usize,
    #[cfg(test)]
    visited: Vec<usize>,
    metadata: Rc<RefCell<MetadataWork>>,
}
impl Work {
    fn signature(
        &mut self,
        bytes: &[u8],
        p: usize,
        magic: &[u8; 4],
        budget: usize,
    ) -> CoreResult<bool> {
        self.scanned_bytes = self.scanned_bytes.checked_add(4).ok_or_else(invalid)?;
        // 이것을 실패시키는 것: 스캔 바이트 예산을 제거하거나 반복 스캔을 추가하기.
        if self.scanned_bytes > budget {
            return Err(invalid_structure("ZIP end-record scan budget exceeded"));
        }
        Ok(bytes.get(p..p + 4) == Some(magic.as_slice()))
    }
}

/// Shared by clones, failed probes and the final original-file metadata pass.
#[derive(Debug)]
struct MetadataWork {
    read: usize,
    records: usize,
    read_limit: usize,
    record_limit: usize,
    exhausted: bool,
}
impl Default for MetadataWork {
    fn default() -> Self {
        Self {
            read: 0,
            records: 0,
            read_limit: usize::MAX,
            record_limit: 4 * super::MAX_ENTRIES,
            exhausted: false,
        }
    }
}
impl MetadataWork {
    fn charge(&mut self, bytes: usize, records: usize) -> io::Result<()> {
        self.read = self.read.saturating_add(bytes);
        self.records = self.records.saturating_add(records);
        self.exhausted |= self.read > self.read_limit || self.records > self.record_limit;
        if self.exhausted {
            return Err(io::Error::other("ZIP metadata work budget exceeded"));
        }
        Ok(())
    }
    fn check(&self) -> CoreResult<()> {
        if self.exhausted {
            return Err(invalid_structure("ZIP metadata work budget exceeded"));
        }
        Ok(())
    }
}

/// A metadata reader sharing a work budget across candidate probes and final opening.
/// Only with `footer: Some(_)` does it provide a candidate view through EOCD
/// with a zero comment length. With `footer: None`, bytes remain unchanged.
/// This is deliberately not a general overlay or a returned package.
#[derive(Clone)]
struct MetadataReader<R> {
    reader: R,
    end: u64,
    footer: Option<u64>,
    work: Rc<RefCell<MetadataWork>>,
}
impl<R: Read + Seek> Read for MetadataReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.work.borrow_mut().charge(0, 0)?;
        let position = self.reader.stream_position()?;
        let available = self.end.saturating_sub(position).min(buf.len() as u64) as usize;
        let n = self.reader.read(&mut buf[..available])?;
        self.work.borrow_mut().charge(n, 0)?;
        if let Some(footer) = self.footer {
            for (i, byte) in buf[..n].iter_mut().enumerate() {
                if (footer + 20..footer + 22).contains(&(position + i as u64)) {
                    *byte = 0;
                }
            }
        }
        Ok(n)
    }
    // Kept instead of the std default so that a short read keeps its internal
    // "unexpected end of file" text and does not retry on Interrupted.
    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        let mut rest = &mut *buf;
        while !rest.is_empty() {
            let n = self.read(rest)?;
            if n == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            rest = &mut rest[n..];
        }
        // A record is a complete 46-byte block that is a central header, by
        // content, not by the call pattern: zip 9.0.5 and 9.1.0 both read each
        // fixed header with one read_exact of exactly this size. This bounds
        // directories zip reaches through its own fallback, which our name
        // scan never saw. A 46-byte name, extra or comment that begins with
        // the signature also counts; that only makes the limit stricter.
        // 이것을 실패시키는 것: 이 과금 삭제 — zip 9.0.5가 주석 속 ZIP64 디렉터리를 상한 없이 끝까지 읽음.
        if buf.len() == 46 && buf.starts_with(b"PK\x01\x02") {
            self.work.borrow_mut().charge(0, 1)?;
        }
        Ok(())
    }
}
impl<R: Seek> Seek for MetadataReader<R> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        match pos {
            SeekFrom::End(offset) => {
                let at = self
                    .end
                    .checked_add_signed(offset)
                    .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
                self.reader.seek(SeekFrom::Start(at))
            }
            _ => self.reader.seek(pos),
        }
    }
}

pub(super) fn preflight(bytes: &[u8], limit: usize) -> CoreResult<Directory> {
    preflight_with_work(bytes, limit, &mut Work::default())
}

pub(super) fn preflight_with_work(
    bytes: &[u8],
    limit: usize,
    work: &mut Work,
) -> CoreResult<Directory> {
    let budget = bytes.len().saturating_add(EOCD_WINDOW).saturating_mul(4);
    let read_limit = work
        .metadata
        .borrow()
        .read_limit
        .min(bytes.len().saturating_add(EOCD_WINDOW).saturating_mul(8));
    work.metadata.borrow_mut().read_limit = read_limit;
    for p in (bytes.len().saturating_sub(EOCD_WINDOW)..bytes.len().saturating_sub(3)).rev() {
        if !work.signature(bytes, p, b"PK\x05\x06", budget)? {
            continue;
        }
        #[cfg(test)]
        {
            work.candidates += 1;
            work.visited.push(p);
        }
        let Ok(comment) = u16_at(bytes, p + 20) else { continue };
        let end = p + 22 + usize::from(comment);
        if end > bytes.len() {
            continue;
        }
        work.metadata.borrow_mut().charge(22, 0).map_err(|e| invalid_structure(e.to_string()))?;
        let Some(mut directory) = candidate(bytes, limit, work, p, budget)? else { continue };
        let reader = MetadataReader {
            reader: Cursor::new(bytes),
            end: (p + 22) as u64,
            footer: Some(p as u64),
            work: work.metadata.clone(),
        };
        let config = Config { archive_offset: ArchiveOffset::Known(directory.base as u64) };
        let probe = ZipArchive::with_config(config, reader);
        work.metadata.borrow().check()?;
        let Ok(probe) = probe else { continue };
        let consumed =
            probe.clone().into_inner().stream_position().map_err(|e| zip_error(e.to_string()))?;
        // A probe may recover to an earlier footer; that is not this candidate.
        if !directory.matches_directory(&probe, consumed) {
            continue;
        }
        directory.verify_count(probe.len())?;
        directory.reference = reference_end(bytes, work, budget)?;
        return Ok(directory);
    }
    Err(invalid())
}

/// Last raw signature in OWPML's 65,535 byte position window.
/// Only a complete fixed footer's position is retained: no CD read, comment
/// eligibility, disk/count validation or fallback can suppress that position.
fn reference_end(bytes: &[u8], work: &mut Work, budget: usize) -> CoreResult<Option<ReferenceEnd>> {
    for eocd in (bytes.len().saturating_sub(REFERENCE_WINDOW)..bytes.len().saturating_sub(3)).rev()
    {
        if !work.signature(bytes, eocd, b"PK\x05\x06", budget)? {
            continue;
        }
        let Ok(_) = field::<22>(bytes, eocd) else { return Ok(None) };
        work.metadata.borrow_mut().charge(22, 0).map_err(|e| invalid_structure(e.to_string()))?;
        return Ok(Some(ReferenceEnd { eocd }));
    }
    Ok(None)
}

/// Physical framing of the selected candidate only.
fn directory_names<'a>(
    bytes: &'a [u8],
    start: usize,
    directory_end: usize,
    count: u64,
    limit: usize,
    work: &mut Work,
) -> CoreResult<Vec<&'a [u8]>> {
    let directory = bytes.get(start..directory_end).ok_or_else(invalid)?;
    let mut position = 0;
    let mut names = Vec::new();
    while position < directory.len() {
        work.metadata.borrow_mut().charge(46, 1).map_err(|e| invalid_structure(e.to_string()))?;
        let header = directory
            .get(position..position.checked_add(46).ok_or_else(invalid)?)
            .ok_or_else(invalid)?;
        if !header.starts_with(b"PK\x01\x02") {
            return Err(invalid());
        }
        let name_len = usize::from(u16_at(header, 28)?);
        let extra_len = usize::from(u16_at(header, 30)?);
        let comment_len = usize::from(u16_at(header, 32)?);
        let name_start = position + 46;
        position =
            name_start.checked_add(name_len + extra_len + comment_len).ok_or_else(invalid)?;
        if position > directory.len() {
            return Err(invalid());
        }
        work.metadata
            .borrow_mut()
            .charge(name_len, 0)
            .map_err(|e| invalid_structure(e.to_string()))?;
        names.push(&directory[name_start..name_start + name_len]);
        // Count the actual records too: a forged EOCD count must not hide them.
        check_count(names.len() as u64, limit)?;
    }
    if names.len() as u64 != count {
        return Err(invalid());
    }
    Ok(names)
}

fn candidate(
    bytes: &[u8],
    limit: usize,
    work: &mut Work,
    eocd: usize,
    budget: usize,
) -> CoreResult<Option<Directory>> {
    if u16_at(bytes, eocd + 4)? != 0 || u16_at(bytes, eocd + 6)? != 0 {
        return Err(invalid());
    }
    let count32 = u16_at(bytes, eocd + 10)?;
    if u16_at(bytes, eocd + 8)? != count32 {
        return Err(invalid());
    }
    let size32 = u32_at(bytes, eocd + 12)?;
    let offset32 = u32_at(bytes, eocd + 16)?;
    // ZIP64 recovery is deferred, including a locator without ZIP32 sentinels.
    // 이것을 실패시키는 것: sentinel/locator 조기 거부 제거 — 보류 후보를 더 읽게 됨.
    if count32 == u16::MAX
        || size32 == u32::MAX
        || offset32 == u32::MAX
        || eocd.checked_sub(20).is_some_and(|p| bytes[p..].starts_with(b"PK\x06\x07"))
    {
        return Err(invalid());
    }
    let relative = offset32 as usize;
    let start = if count32 == 0 {
        if relative > eocd {
            return Ok(None);
        }
        eocd
    } else {
        let mut first = None;
        for p in relative..eocd.saturating_sub(3) {
            if work.signature(bytes, p, b"PK\x01\x02", budget)? {
                first = Some(p);
                break;
            }
        }
        let Some(first) = first else { return Ok(None) };
        first
    };
    let count = u64::from(count32);
    let directory_size = size32 as usize;
    let base = start - relative;
    let directory_end = eocd;
    check_count(count, limit)?;
    // 이것을 실패시키는 것: zip이 고른 첫 CDFH 대신 끝에서 size만 빼서 읽기.
    if start.checked_add(directory_size) != Some(directory_end) {
        return Err(invalid());
    }
    let names = directory_names(bytes, start, directory_end, count, limit, work)?;
    let mut raw_names = BTreeSet::new();
    for name in &names {
        // 이것을 실패시키는 것: Unicode extra 적용 후 이름만 검사하기.
        if !raw_names.insert(*name) {
            return Err(invalid_structure("duplicate original ZIP entry name"));
        }
    }
    let comment = &bytes[eocd + 22..eocd + 22 + usize::from(u16_at(bytes, eocd + 20)?)];
    work.metadata
        .borrow_mut()
        .charge(comment.len(), 0)
        .map_err(|e| invalid_structure(e.to_string()))?;
    Ok(Some(Directory {
        start,
        size: directory_size,
        count: names.len(),
        base,
        eocd,
        comment: comment.to_vec(),
        reference: None,
        work: work.metadata.clone(),
    }))
}

#[cfg(test)]
mod tests {
    use super::super::{open_package, PackageLimits};
    use super::*;
    fn open(bytes: &[u8]) -> CoreResult<super::super::Package<'_>> {
        open_package(bytes, PackageLimits::default())
    }
    use std::io::Cursor;
    use std::rc::Rc;

    fn fixture() -> Vec<u8> {
        use std::io::Write;
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..11 {
            writer
                .start_file(
                    format!("part{i}.xml"),
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Stored),
                )
                .unwrap();
            writer.write_all(&vec![b'x'; if i == 0 { 23_000 } else { 26 }]).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }
    const LIMIT: usize = 10_000;

    fn synthetic() -> Vec<u8> {
        use std::io::Write;
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer.start_file("mimetype", zip::write::SimpleFileOptions::default()).unwrap();
        writer.write_all(b"application/hwp+zip").unwrap();
        writer.finish().unwrap().into_inner()
    }

    /// 이것을 실패시키는 것: 원시 위치에 주석·CD 적격성 추가 / 잘린 서명에서 앞 후보로 복구 / 창 경계 변경.
    #[test]
    fn reference_position_and_budget() {
        let bytes = synthetic();
        let eocd = bytes.len() - 22;
        let inspect =
            |bytes: &[u8]| reference_end(bytes, &mut Work::default(), usize::MAX).unwrap();
        for comment in [0u16, 1, u16::MAX] {
            let mut input = bytes.clone();
            input[eocd + 20..eocd + 22].copy_from_slice(&comment.to_le_bytes());
            assert_eq!(inspect(&input).unwrap().eocd, eocd);
        }
        // Even garbage fixed fields must retain the last complete raw position.
        let mut input = bytes.clone();
        input.extend(b"PK\x05\x06");
        input.extend([255; 18]);
        assert_eq!(inspect(&input).unwrap().eocd, bytes.len());
        for len in [4, 21] {
            let mut input = bytes.clone();
            input.extend(b"PK\x05\x06");
            input.resize(bytes.len() + len, 0);
            assert!(inspect(&input).is_none());
        }
        for n in [65_513, 65_514] {
            let input = [bytes.clone(), vec![0; n]].concat();
            assert_eq!(inspect(&input).is_some(), n == 65_513);
        }
        let mut work = Work::default();
        assert_eq!(reference_end(&bytes, &mut work, usize::MAX).unwrap().unwrap().eocd, eocd);
        assert_eq!(work.scanned_bytes, 19 * 4);
        assert_eq!(work.metadata.borrow().records, 0);
        assert_eq!(work.metadata.borrow().read, 22);
        let mut work = Work::default();
        work.metadata.borrow_mut().read_limit = 21;
        assert!(reference_end(&bytes, &mut work, usize::MAX).is_err());
        assert!(reference_end(&bytes, &mut Work::default(), 0).is_err());
    }

    /// 이것을 실패시키는 것: 기존 경로에도 위치 규칙 적용 / 주석 적격성을 참조 탐색에 추가.
    #[test]
    // 이것을 실패시키는 것: 외부 EOCD comment 길이 축소 (실제 음성 대조 실행).
    fn legacy_comment_footer_cross_product() {
        for garbage in [false, true] {
            for comment in [0u16, 1, u16::MAX] {
                let mut bytes = fixture().to_vec();
                let outer = bytes.len() - 22;
                let mut footer = bytes[outer..].to_vec();
                footer[20..22].copy_from_slice(&comment.to_le_bytes());
                if garbage {
                    footer[8..20].fill(0xff);
                }
                bytes[outer + 20..outer + 22].copy_from_slice(&22u16.to_le_bytes());
                bytes.extend(footer);
                let reference =
                    reference_end(&bytes, &mut Work::default(), usize::MAX).unwrap().unwrap();
                assert_eq!(reference.eocd, outer + 22);
                assert_eq!(
                    open(&bytes).is_ok(),
                    comment != 0,
                    "garbage={garbage}, comment={comment}"
                );
                if comment != 0 {
                    let selected = legacy::preflight(&bytes, LIMIT).unwrap();
                    assert_eq!(selected.eocd, outer);
                    assert_eq!(selected.open(Cursor::new(&bytes)).unwrap().len(), 11);
                }
            }
        }
    }

    /// 이것을 실패시키는 것: 기존 성공 뒤 새 읽기 예산 적용 / 기존 경로 생략.
    #[test]
    // 이것을 실패시키는 것: 후보 수 상한을 유발하는 서명 제거 (실제 음성 대조 실행).
    fn legacy_max_comment_invalid_signatures() {
        for (original, max_fakes) in [(fixture().to_vec(), 63), (zip64(), 62)] {
            for n in [0, 1, 12, 13, 62, 63, 64] {
                let mut bytes = original.clone();
                let end = bytes.len() - 22;
                bytes[end + 20..end + 22].copy_from_slice(&u16::MAX.to_le_bytes());
                for _ in 0..n {
                    bytes.extend(b"PK\x05\x06");
                    bytes.extend([0; 16]);
                    bytes.extend([255; 2]);
                }
                bytes.resize(original.len() + usize::from(u16::MAX), b'x');
                let baseline = legacy::preflight(&bytes, LIMIT)
                    .and_then(|directory| directory.open(Cursor::new(&bytes)));
                assert_eq!(baseline.is_ok(), n <= max_fakes);
                let result = super::open(&bytes, LIMIT);
                assert_eq!(result.is_ok(), n <= max_fakes, "max_fakes={max_fakes}, n={n}");
                match (baseline, result) {
                    (Ok(before), Ok(after)) => {
                        assert_eq!(before.len(), after.len());
                        assert_eq!(before.offset(), after.offset());
                        assert_eq!(
                            before.central_directory_start(),
                            after.central_directory_start()
                        );
                        assert_eq!(before.comment(), after.comment());
                    }
                    (Err(before), Err(after)) => assert_eq!(before.to_string(), after.to_string()),
                    _ => unreachable!(),
                }
            }
        }
    }

    /// 이것을 실패시키는 것: ZIP64 조기 거부 삭제 / 기존 ZIP64에도 새 경계 적용 /
    /// 두 경로 실패 시 legacy 오류 대신 새 오류 반환.
    #[test]
    // 이것을 실패시키는 것: 정상 ZIP64 선택의 sentinel 제거 (실제 음성 대조 실행).
    fn zip64_sentinel_tail_cross_product() {
        let old = fixture().len() - 22;
        for sentinel in [8, 12, 16] {
            let mut bytes = zip64();
            let end = bytes.len() - 22;
            bytes[end + 8..end + 20].copy_from_slice(&fixture()[old + 8..old + 20]);
            bytes[end + sentinel..end + sentinel + 4].fill(0xff);
            assert!(open(&bytes).is_ok());
            let mut fake = b"PK\x05\x06".to_vec();
            fake.extend([0; 16]);
            fake.extend([255; 2]);
            // An over-limit candidate tail distinguishes legacy's error from
            // the new ZIP64 boundary error, even though both reject the file.
            for tail in [vec![0; 11], fake.repeat(65)] {
                let input = [bytes.as_slice(), tail.as_slice()].concat();
                assert!(preflight(&input, LIMIT).is_err());
                let expected = legacy::preflight(&input, LIMIT).unwrap_err();
                let actual = open(&input).unwrap_err();
                assert_eq!(std::mem::discriminant(&actual), std::mem::discriminant(&expected));
                assert_eq!(actual.to_string(), expected.to_string());
            }
        }
    }

    /// 이것을 실패시키는 것: count/size/offset sentinel 또는 locator 조기 거부 제거.
    #[test]
    // 이것을 실패시키는 것: ZIP64 sentinel 제거 (실제 음성 대조 실행).
    fn zip64_recovery_stops_before_directory_scan() {
        for field in [8, 12, 16, 20] {
            let mut bytes = fixture().to_vec();
            let end = bytes.len() - 22;
            if field == 20 {
                bytes[end - 20..end - 16].copy_from_slice(b"PK\x06\x07");
            } else {
                bytes[end + field..end + field + 4].fill(0xff);
            }
            let mut work = Work::default();
            assert!(candidate(&bytes, LIMIT, &mut work, end, usize::MAX).is_err());
            assert_eq!(work.scanned_bytes, 0);
            assert_eq!(work.metadata.borrow().read, 0);
        }
    }

    /// 이것을 실패시키는 것: 거부 전에 참조 디렉터리를 읽을 수 있어야 한다는 조건 추가.
    #[test]
    fn complete_comment_footer_positions() {
        let original = synthetic();
        let outer = original.len() - 22;
        for same_cd in [false, true] {
            for at_eof in [false, true] {
                let mut bytes = original.clone();
                let mut footer = original[outer..].to_vec();
                if !same_cd {
                    footer[8..12].copy_from_slice(&[7, 0, 7, 0]);
                    footer[12..16].copy_from_slice(&1000u32.to_le_bytes());
                    footer[16..20].copy_from_slice(&5u32.to_le_bytes());
                }
                bytes[outer + 20..outer + 22]
                    .copy_from_slice(&(if at_eof { 22u16 } else { 24u16 }).to_le_bytes());
                bytes.extend(footer);
                if !at_eof {
                    bytes.extend(b"xx");
                }
                let zip = ZipArchive::new(Cursor::new(&bytes)).unwrap();
                assert_eq!(
                    zip.central_directory_start(),
                    u64::from(u32_at(&original, outer + 16).unwrap())
                );
                // Empty comment identifies the inner footer; the outer has 22/24 bytes.
                assert_eq!(zip.comment().is_empty(), same_cd);
                // These exact duplicate-CD fixtures retain the old size: even if
                // zip chooses the inner footer, its physical span/base disagrees.
                assert!(open(&bytes).is_err(), "same_cd={same_cd}, at_eof={at_eof}");
            }
        }
        // A structurally valid same-CD inner footer: the outer EOCD occupies the
        // last CDFH's comment. Both name the same start; the inner span includes it.
        for at_eof in [false, true] {
            let mut bytes = original.clone();
            let cd = u32_at(&bytes, outer + 16).unwrap() as usize;
            bytes[cd + 32..cd + 34].copy_from_slice(&22u16.to_le_bytes());
            bytes[outer + 20..outer + 22]
                .copy_from_slice(&(if at_eof { 22u16 } else { 24u16 }).to_le_bytes());
            let mut footer = original[outer..].to_vec();
            let size = u32_at(&footer, 12).unwrap() + 22;
            footer[12..16].copy_from_slice(&size.to_le_bytes());
            bytes.extend(footer);
            if !at_eof {
                bytes.extend(b"xx");
            }
            let zip = ZipArchive::new(Cursor::new(&bytes)).unwrap();
            assert!(zip.comment().is_empty());
            let selected = preflight(&bytes, LIMIT).unwrap();
            assert_eq!(selected.eocd, original.len());
            assert_eq!(selected.start, cd);
            selected.open(Cursor::new(&bytes)).unwrap();
            assert!(open(&bytes).is_ok());
        }
    }

    /// 이것을 실패시키는 것: EOCD 끝=EOF 강제 / 앞 payload 후보 중복·64 상한 복원.
    #[test]
    fn short_tails_and_unused_candidates() {
        let original = synthetic();
        for tail in [b"abcdefghijk".as_slice(), &[0; 11], b"PK\x06\x06", b"PK\x06\x07"] {
            let bytes = [original.as_slice(), tail].concat();
            assert!(open(&bytes).is_ok(), "tail={tail:?}");
        }
        for n in [1, 65] {
            let mut prefix = Vec::new();
            for _ in 0..n {
                prefix.extend(b"PK\x05\x06");
                prefix.extend([0; 16]);
                prefix.extend(if n == 65 { [255; 2] } else { [0; 2] });
            }
            use std::io::Write;
            let mut writer = zip::ZipWriter::new_append(Cursor::new(original.clone())).unwrap();
            writer
                .start_file(
                    "Unused/probe.bin",
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Stored),
                )
                .unwrap();
            writer.write_all(&prefix).unwrap();
            let bytes = writer.finish().unwrap().into_inner();
            assert!(open(&bytes).is_ok(), "payload={n}");
        }
    }

    /// 이것을 실패시키는 것: 조기 종료만 추가하고 후보 64 상한을 유지하기.
    #[test]
    fn failed_candidate_visits() {
        for n in [63, 64, 65, 128] {
            let mut bytes = synthetic();
            let end = bytes.len() - 22;
            for _ in 0..n {
                let at = bytes.len();
                bytes.extend(b"PK\x05\x06");
                bytes.extend([0; 4]);
                bytes.extend([1, 0, 1, 0]);
                bytes.extend(46u32.to_le_bytes());
                bytes.extend(((at + 1) as u32).to_le_bytes());
                bytes.extend([0; 2]);
            }
            let mut work = Work::default();
            let selected = preflight_with_work(&bytes, LIMIT, &mut work).unwrap();
            assert_eq!(selected.eocd, end);
            assert_eq!(work.candidates, n + 1);
            assert_eq!(
                work.visited,
                (0..n)
                    .rev()
                    .map(|i| end + 22 + i * 22)
                    .chain(std::iter::once(end))
                    .collect::<Vec<_>>()
            );
            assert!(!work.metadata.borrow().exhausted);
            assert!(open(&bytes).is_err());
        }
    }

    /// 이것을 실패시키는 것: 꼬리에만 서명을 찾기 / 참조 창 경계를 한 바이트 늘리기 / 기존 주석에도 참조 제한 적용.
    /// 이것을 실패시키는 것: 잘린 마지막 서명을 완전한 EOCD 위치로 취급하기.
    #[test]
    fn reference_tail_boundary() {
        for n in [65_513, 65_514] {
            let bytes = [synthetic(), vec![0; n]].concat();
            assert_eq!(open(&bytes).is_ok(), n == 65_513);
        }
        let mut bytes = synthetic();
        let end = bytes.len() - 22;
        bytes[end + 20..end + 22].copy_from_slice(&3u16.to_le_bytes());
        bytes.extend(b"PK\x05");
        assert!(open(&bytes).is_ok());
        bytes.push(6);
        assert!(open(&bytes).is_err());
        bytes[end + 20..end + 22].copy_from_slice(&4u16.to_le_bytes());
        assert!(open(&bytes).is_ok());
    }

    /// 이것을 실패시키는 것: 예산 >= 변이 / read·record 누적·clone 공유 삭제 / 일반 필드를 레코드로 과금.
    #[test]
    fn metadata_budget_edges_and_clones() {
        for (bytes, records) in [(8, 0), (0, 4)] {
            let mut budget = MetadataWork { read_limit: 8, record_limit: 4, ..Default::default() };
            budget.charge(bytes, records).unwrap();
            assert!(budget.charge(usize::from(bytes != 0), usize::from(records != 0)).is_err());
            assert!(budget.charge(0, 0).is_err());
            assert!(budget.check().is_err());
        }
        let work = Rc::new(RefCell::new(MetadataWork { read_limit: 4, ..Default::default() }));
        let mut reader = MetadataReader {
            reader: Cursor::new(b"12345678"),
            end: 8,
            footer: None,
            work: work.clone(),
        };
        reader.read_exact(&mut [0; 4]).unwrap();
        let mut clone = reader.clone();
        assert!(clone.read_exact(&mut [0; 1]).is_err());
        assert!(reader.read_exact(&mut [0; 1]).is_err());
        assert_eq!(work.borrow().read, 5);
        let mut reader = MetadataReader {
            reader: Cursor::new(b"1234"),
            end: 3,
            footer: Some(0),
            work: Rc::new(RefCell::new(MetadataWork::default())),
        };
        assert_eq!(reader.seek(SeekFrom::End(-1)).unwrap(), 2);
        assert!(reader.seek(SeekFrom::End(-4)).is_err());
        assert!(reader.read_exact(&mut [0; 2]).is_err());
        // A record is a complete 46-byte central header by content, whatever
        // call pattern precedes it (zip 9.0 queries the position, 9.1 does not).
        // 이것을 실패시키는 것: 서명 대조 삭제(모든 46바이트 읽기 과금) / 길이 조건을 >= 46으로 / 위치 질의에 기대기.
        for (len, magic, probe, expected) in [
            (46, b"PK\x01\x02", false, 1),
            (46, b"PK\x01\x02", true, 1),
            (46, b"PK\x05\x06", true, 0),
            (47, b"PK\x01\x02", true, 0),
        ] {
            let mut bytes = vec![0; len];
            bytes[..4].copy_from_slice(magic);
            let work = Rc::new(RefCell::new(MetadataWork::default()));
            let mut reader = MetadataReader {
                reader: Cursor::new(bytes),
                end: len as u64,
                footer: None,
                work: work.clone(),
            };
            if probe {
                reader.stream_position().unwrap();
            }
            reader.read_exact(&mut vec![0; len]).unwrap();
            assert_eq!(work.borrow().records, expected, "{len} {magic:?} {probe}");
            assert_eq!(work.borrow().read, len);
        }
        // A short fixed-header read spends its bytes but no record.
        // 이것을 실패시키는 것: 읽기 전에(내용 확인 없이) 레코드를 과금하기.
        let work = Rc::new(RefCell::new(MetadataWork::default()));
        let mut reader =
            MetadataReader { reader: Cursor::new(b"PK"), end: 2, footer: None, work: work.clone() };
        reader.stream_position().unwrap();
        assert!(reader.read_exact(&mut [0; 46]).is_err());
        assert_eq!(work.borrow().records, 0);
        assert_eq!(work.borrow().read, 2);
    }

    /// 이것을 실패시키는 것: 최종 metadata 읽기 과금 / 고정 필드·레코드 방문 과금 삭제, 정상 예산 축소.
    #[test]
    fn normal_metadata_budget_and_final_charge() {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..LIMIT {
            writer
                .start_file(format!("entry{i}"), zip::write::SimpleFileOptions::default())
                .unwrap();
        }
        let many = writer.finish().unwrap().into_inner();
        for original in [synthetic(), many] {
            let mut bytes = original;
            let end = bytes.len() - 22;
            bytes[end + 20..end + 22].copy_from_slice(&100u16.to_le_bytes());
            bytes.extend(vec![b'x'; 100]);
            let mut work = Work::default();
            let directory = preflight_with_work(&bytes, LIMIT, &mut work).unwrap();
            // One record per header for our name scan and one for each zip
            // pass that reads the header block (probe, then open): the same on
            // zip 9.0.5 and 9.1.0 because both read each block once per pass.
            // 이것을 실패시키는 것: 이름 스캔 과금 삭제·두 배로 / 46바이트 내용 기준 과금 삭제.
            assert_eq!(work.metadata.borrow().records, 2 * directory.count);
            let before = work.metadata.borrow().read;
            directory.open(Cursor::new(&bytes)).unwrap();
            let budget = work.metadata.borrow();
            assert_eq!(budget.records, 3 * directory.count);
            assert!(budget.read > before);
            assert!(budget.read <= 8 * (bytes.len() + EOCD_WINDOW));
            eprintln!(
                "W1_NORMAL len={} count={} read={} records={}",
                bytes.len(),
                directory.count,
                budget.read,
                budget.records
            );
        }
        let bytes = synthetic();
        let directory = preflight(&bytes, LIMIT).unwrap();
        let used = directory.work.borrow().read;
        directory.work.borrow_mut().read_limit = used + 2;
        assert!(
            matches!(directory.open(Cursor::new(&bytes)), Err(CoreError::InvalidStructure { context, reason })
            if context == "ZIP package" && reason == "ZIP metadata work budget exceeded")
        );
        let mut work = Work::default();
        work.metadata.borrow_mut().read_limit = 80;
        assert!(
            matches!(preflight_with_work(&bytes, LIMIT, &mut work), Err(CoreError::InvalidStructure { context, reason })
            if context == "ZIP package" && reason == "ZIP metadata work budget exceeded")
        );
    }

    // Every candidate has one physically complete CDFH with overlapping, large
    // variable data. zip reads that data, then rejects its invalid extra field.
    fn repeated_variable_reads(n: usize) -> Vec<u8> {
        let mut bytes = vec![0; 60_000];
        let original = synthetic();
        let original_end = original.len() - 22;
        let cd = u32_at(&original, original_end + 16).unwrap() as usize;
        for i in 0..n {
            let start = i * 100;
            let end = bytes.len();
            bytes[start..start + 46].copy_from_slice(&original[cd..cd + 46]);
            bytes[start + 28..start + 34].fill(0);
            bytes[start + 30..start + 32]
                .copy_from_slice(&((end - start - 46) as u16).to_le_bytes());
            bytes[start + 46..start + 50].copy_from_slice(&[0x75, 0x70, 0, 0]);
            bytes.extend(b"PK\x05\x06");
            bytes.extend([0; 4]);
            bytes.extend(1u16.to_le_bytes());
            bytes.extend(1u16.to_le_bytes());
            bytes.extend(((end - start) as u32).to_le_bytes());
            bytes.extend((start as u32).to_le_bytes());
            bytes.extend([0; 2]);
        }
        bytes
    }

    /// 이것을 실패시키는 것: 실패 프로브 읽기 비용을 초기화 / 누적 읽기 상한 삭제 / 자원 오류를 fallback으로 삼키기.
    // 64 candidates exhaust the read budget with zip 9.0 and 9.1 alike. Each
    // candidate's directory abuts its end record, and zip 9.1 stops falling
    // back after such a header fails, so 16 candidates no longer reach the
    // limit there (the input is still rejected).
    #[test]
    fn repeated_large_variable_reads_exhaust_budget() {
        let bytes = repeated_variable_reads(64);
        let mut work = Work::default();
        let result = preflight_with_work(&bytes, LIMIT, &mut work);
        assert!(
            matches!(&result, Err(CoreError::InvalidStructure { context, reason })
            if context == "ZIP package" && reason == "ZIP metadata work budget exceeded"),
            "{result:?} {work:?}"
        );
        assert!(work.metadata.borrow().read > 8 * (bytes.len() + EOCD_WINDOW));
        // The read budget trips long before the record limit (20 records on
        // zip 9.0.5, which falls back inside one probe; 36 on 9.1.0).
        eprintln!(
            "W1_EXHAUST read={} candidates={} records={}",
            work.metadata.borrow().read,
            work.candidates,
            work.metadata.borrow().records
        );
    }

    // `chains` candidates whose directories each pass our framing scan with
    // `rounds * (block + 1)` records, then fail in zip on an invalid extra.
    // Directories interleave: every round each chain writes `block` headers
    // and one jumper whose comment skips the other chains' bytes, so all end
    // records still fit in the tail window.
    fn interleaved_directories(chains: usize, rounds: usize, block: usize) -> Vec<u8> {
        let original = synthetic();
        let original_end = original.len() - 22;
        let cd = u32_at(&original, original_end + 16).unwrap() as usize;
        let mut template = original[cd..cd + 46].to_vec();
        template[28..34].fill(0);
        template[28..30].copy_from_slice(&2u16.to_le_bytes());
        let mut bytes = Vec::new();
        let mut starts = vec![0; chains];
        let mut pending: Vec<Option<(usize, usize)>> = vec![None; chains];
        let close = |bytes: &mut Vec<u8>, jumper: Option<(usize, usize)>| {
            if let Some((field, from)) = jumper {
                let skip = u16::try_from(bytes.len() - from).unwrap();
                bytes[field..field + 2].copy_from_slice(&skip.to_le_bytes());
            }
        };
        for round in 0..rounds {
            for chain in 0..chains {
                close(&mut bytes, pending[chain]);
                if round == 0 {
                    starts[chain] = bytes.len();
                }
                for i in 0..=block {
                    let at = bytes.len();
                    bytes.extend(&template);
                    bytes.extend(((round * (block + 1) + i) as u16).to_le_bytes());
                    if i == block {
                        bytes[at + 30..at + 32].copy_from_slice(&4u16.to_le_bytes());
                        bytes.extend([0x75, 0x70, 0, 0]);
                        pending[chain] = Some((at + 32, bytes.len()));
                    }
                }
            }
        }
        let count = u16::try_from(rounds * (block + 1)).unwrap();
        for chain in 0..chains {
            close(&mut bytes, pending[chain]);
            let size = bytes.len() - starts[chain];
            bytes.extend(b"PK\x05\x06");
            bytes.extend([0; 4]);
            bytes.extend(count.to_le_bytes());
            bytes.extend(count.to_le_bytes());
            bytes.extend((size as u32).to_le_bytes());
            bytes.extend((starts[chain] as u32).to_le_bytes());
            bytes.extend([0; 2]);
        }
        bytes
    }

    /// 이것을 실패시키는 것: 레코드 상한 비교(`records > record_limit`) 삭제 — 모든 후보 실패 뒤 framing 오류로 바뀜.
    /// 이것을 실패시키는 것: 기본 상한을 4 * MAX_ENTRIES 아닌 값으로(예: 45,000) / `>`를 `>=`로.
    // Five candidates of 9,860 records each (2.4 MB): the record limit trips
    // while the read budget, which grows with the input (19.5 MB here), is not
    // spent. Each candidate alone stays under the per-candidate entry limit, so
    // only the cumulative record limit bounds this work in absolute terms. zip
    // adds its own header reads (9.0.5 all of a chain, 9.1.0 up to the bad
    // extra), so the candidate that trips differs by version.
    #[test]
    fn record_limit_binds_before_read_budget() {
        let bytes = interleaved_directories(5, 29, 339);
        let mut work = Work::default();
        let result = preflight_with_work(&bytes, LIMIT, &mut work);
        assert!(
            matches!(&result, Err(CoreError::InvalidStructure { context, reason })
            if context == "ZIP package" && reason == "ZIP metadata work budget exceeded"),
            "{result:?}"
        );
        let metadata = work.metadata.borrow();
        assert_eq!(metadata.record_limit, 4 * super::super::MAX_ENTRIES);
        assert_eq!(metadata.records, 40_001);
        // The record limit is reached before the read budget.
        assert!(metadata.read <= metadata.read_limit, "{} {}", metadata.read, metadata.read_limit);
        eprintln!(
            "W1_RECORDS len={} read={} read_limit={} records={}",
            bytes.len(),
            metadata.read,
            metadata.read_limit,
            metadata.records
        );
    }

    // `n` valid headers, then one outer header that our scan accepts (one
    // record) but zip rejects (invalid extra). Its comment hides a ZIP64 end
    // record, locator and inner end record that describe the `n` headers, so
    // zip 9.0.5 falls back to a directory our name scan never visited.
    fn zip64_fallback_behind_bad_extra(n: usize) -> Vec<u8> {
        let original = synthetic();
        let original_end = original.len() - 22;
        let cd = u32_at(&original, original_end + 16).unwrap() as usize;
        let mut template = original[cd..cd + 46].to_vec();
        template[28..34].fill(0);
        let mut bytes = Vec::new();
        for i in 0..n {
            let at = bytes.len();
            bytes.extend(&template);
            bytes[at + 28..at + 30].copy_from_slice(&8u16.to_le_bytes());
            bytes.extend((i as u64).to_le_bytes());
        }
        let size = bytes.len() as u64;
        let outer = bytes.len();
        bytes.extend(&template);
        bytes[outer + 28..outer + 34].copy_from_slice(&[1, 0, 4, 0, 98, 0]);
        bytes.extend(b"o");
        bytes.extend([0x75, 0x70, 0, 0]);
        let zip64 = bytes.len() as u64;
        bytes.extend(b"PK\x06\x06");
        bytes.extend(44u64.to_le_bytes());
        bytes.extend([45, 0, 45, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        for value in [n as u64, n as u64, size, 0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(b"PK\x06\x07");
        bytes.extend(0u32.to_le_bytes());
        bytes.extend(zip64.to_le_bytes());
        bytes.extend(1u32.to_le_bytes());
        bytes.extend(b"PK\x05\x06");
        bytes.extend([0; 4]);
        bytes.extend([0xff; 12]);
        bytes.extend([0; 2]);
        let directory = bytes.len() - outer;
        assert_eq!(directory, 149);
        bytes.extend(b"PK\x05\x06");
        bytes.extend([0; 4]);
        bytes.extend([1, 0, 1, 0]);
        bytes.extend((directory as u32).to_le_bytes());
        bytes.extend((outer as u32).to_le_bytes());
        bytes.extend([0; 2]);
        bytes
    }

    /// 이것을 실패시키는 것: 46바이트 내용 기준 레코드 과금 삭제 — zip 9.0.5가 내부 ZIP64 디렉터리의 헤더를 전부 읽음.
    /// 이것을 실패시키는 것(9.0.5): 레코드 상한 비교 삭제 / 기본 상한 완화 / `>`를 `>=`로.
    /// (zip 9.1.0은 EOCD에 붙은 디렉터리 실패 뒤 fallback하지 않으므로 위 변이들을 9.0.5에서만 잡는다.)
    // 45,000 inner headers: just above the 40,000 record limit so that zip
    // cannot finish them, and still a short test (2.4 MB).
    #[test]
    fn zip_fallback_directory_is_record_bounded() {
        let n = 45_000;
        let bytes = zip64_fallback_behind_bad_extra(n);
        let mut work = Work::default();
        let result = preflight_with_work(&bytes, LIMIT, &mut work);
        assert!(result.is_err());
        let metadata = work.metadata.borrow();
        eprintln!(
            "W1_FALLBACK len={} read={} read_limit={} records={} result={result:?}",
            bytes.len(),
            metadata.read,
            metadata.read_limit,
            metadata.records
        );
        // zip 9.0.5 stops at the record limit; 9.1.0 never falls back and the
        // inner ZIP64 candidate is refused by our own framing.
        if matches!(&result, Err(CoreError::InvalidStructure { reason, .. })
            if reason == "ZIP metadata work budget exceeded")
        {
            assert_eq!(metadata.records, 40_001);
        }
        assert!(metadata.records <= 40_001);
        // Neither version reads every inner fixed header.
        assert!(metadata.read < 46 * n, "{}", metadata.read);
        assert!(metadata.read <= metadata.read_limit);
        assert!(open(&bytes).is_err());
    }

    // `n` empty entries with distinct ASCII names, each with a 46-byte comment
    // `PK\x01\x02` + 42 `x`, and an exact directory and end record.
    fn signature_comments(n: usize) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..n {
            writer.start_file(format!("e{i}"), zip::write::SimpleFileOptions::default()).unwrap();
        }
        let plain = writer.finish().unwrap().into_inner();
        let end = plain.len() - 22;
        let cd = u32_at(&plain, end + 16).unwrap() as usize;
        let comment = [b"PK\x01\x02".as_slice(), &[b'x'; 42]].concat();
        let mut bytes = plain[..cd].to_vec();
        let mut p = cd;
        while p < end {
            let len = 46
                + usize::from(u16_at(&plain, p + 28).unwrap())
                + usize::from(u16_at(&plain, p + 30).unwrap());
            let at = bytes.len();
            bytes.extend(&plain[p..p + len]);
            bytes[at + 32..at + 34].copy_from_slice(&46u16.to_le_bytes());
            bytes.extend(&comment);
            p += len + usize::from(u16_at(&plain, p + 32).unwrap());
        }
        let size = (bytes.len() - cd) as u32;
        let footer = bytes.len();
        bytes.extend(&plain[end..]);
        bytes[footer + 12..footer + 16].copy_from_slice(&size.to_le_bytes());
        bytes
    }

    /// 이것을 실패시키는 것: 46바이트 내용 판별 삭제 — 레코드가 40,001에 닿지 않아 복구 경로가 열림.
    // A 46-byte variable field that begins with the central header signature
    // counts as a record, which makes the limit stricter. This rejection is
    // intended; normal HWPX packages do not have this shape. Each entry costs
    // our scan 1 and each zip pass (probe, open) 2 (header and comment): 5N =
    // 40,005 for N = 8,001, so the recovery path stops at 40,001 on both zip
    // versions. Without the trailing byte the legacy path opens the same file.
    #[test]
    fn signature_comments_tighten_the_record_limit() {
        let n = 8_001;
        let exact = signature_comments(n);
        assert_eq!(open(&exact).unwrap().entries().len(), n);
        let bytes = [exact.as_slice(), b"x"].concat();
        // The public opener returns the legacy error once recovery also fails.
        let legacy = legacy::preflight(&bytes, LIMIT).unwrap_err();
        let public = open(&bytes).unwrap_err();
        assert!(matches!(&public, CoreError::InvalidStructure { context, reason }
            if context == "ZIP" && reason == "invalid ZIP central directory framing"));
        assert_eq!(public.to_string(), legacy.to_string());
        // Measured on zip 9.0.5 and 9.1.0 alike: 3N after the probe, then the
        // final open trips the limit.
        let mut work = Work::default();
        let directory = preflight_with_work(&bytes, LIMIT, &mut work).unwrap();
        assert_eq!(work.metadata.borrow().records, 3 * n);
        let opened = directory.open(Cursor::new(&bytes));
        assert!(matches!(&opened, Err(CoreError::InvalidStructure { context, reason })
            if context == "ZIP package" && reason == "ZIP metadata work budget exceeded"));
        assert_eq!(work.metadata.borrow().records, 40_001);
    }

    /// 이것을 실패시키는 것: extra 파싱 전에 선택 완료 / 프로브 fallback을 현재 후보 성공으로 취급 / 정책 오류를 건너뛰기.
    #[test]
    fn extra_failure_recovers_but_policy_does_not() {
        for prefix in [0, 6] {
            let base = [vec![0; prefix], synthetic()].concat();
            let good_end = base.len() - 22;
            let mut bytes = base.clone();
            let cd = u32_at(&base, good_end + 16).unwrap() as usize + prefix;
            let start = bytes.len();
            bytes.extend(&base[cd..good_end]);
            let extra_at = bytes.len();
            bytes[start + 30..start + 32].copy_from_slice(&4u16.to_le_bytes());
            bytes.extend([0x75, 0x70, 0, 0]);
            let bad_end = bytes.len();
            bytes.extend(&base[good_end..]);
            bytes[bad_end + 12..bad_end + 16]
                .copy_from_slice(&((bad_end - start) as u32).to_le_bytes());
            bytes[bad_end + 16..bad_end + 20].copy_from_slice(&(start as u32).to_le_bytes());
            let mut work = Work::default();
            let selected = preflight_with_work(&bytes, LIMIT, &mut work).unwrap();
            assert_eq!(selected.eocd, good_end);
            assert_eq!(work.visited, [bad_end, good_end]);
            assert!(open(&bytes).is_err());
            // Fix the extra, but lie about CD size. A readable last CD's policy
            // failure must never fall back to the earlier good archive.
            bytes[extra_at..extra_at + 4].copy_from_slice(&[0, 0, 0, 0]);
            bytes[bad_end + 12..bad_end + 16].copy_from_slice(&1u32.to_le_bytes());
            assert!(ZipArchive::new(Cursor::new(&bytes)).is_ok());
            assert!(preflight(&bytes, LIMIT).is_err());
        }
    }

    /// 이것을 실패시키는 것: 선택 위치의 주석 길이/바이트 대조 생략 / 빈 CD 소비 위치까지 강제.
    #[test]
    fn selected_comment_and_empty_directory() {
        let mut bytes = synthetic();
        let end = bytes.len() - 22;
        bytes[end + 20..end + 22].copy_from_slice(&3u16.to_le_bytes());
        bytes.extend(b"abc");
        bytes.extend([0; 11]);
        let mut directory = preflight(&bytes, LIMIT).unwrap();
        let archive = directory.open(Cursor::new(&bytes)).unwrap();
        let consumed = archive.clone().into_inner().position();
        directory.comment[0] = b'z';
        assert!(directory.verify(&archive, consumed, bytes.len() as u64).is_err());
        directory.comment[0] = b'a';
        directory.eocd += 1;
        directory.reference.as_mut().unwrap().eocd += 1;
        assert!(directory.open(Cursor::new(&bytes)).is_err());
        let empty = zip::ZipWriter::new(Cursor::new(Vec::new())).finish().unwrap().into_inner();
        assert!(open(&empty).is_ok());
    }

    /// 이것을 실패시키는 것: 빈 ZIP32 후보의 fallback 제거 / ZIP64 보류 뒤 앞 후보 복구.
    #[test]
    fn unreadable_fixed_candidates() {
        for zip64 in [false, true] {
            let mut bytes = synthetic();
            let selected = bytes.len() - 22;
            if zip64 {
                let mut broken = as_zip64(&synthetic());
                let fixed = broken.len() - 98;
                broken[fixed..fixed + 4].fill(0);
                bytes.extend(broken);
            } else {
                let at = bytes.len();
                bytes.extend(b"PK\x05\x06");
                bytes.extend([0; 12]);
                bytes.extend(((at + 1) as u32).to_le_bytes());
                bytes.extend([0; 2]);
            }
            let mut work = Work::default();
            let result = preflight_with_work(&bytes, LIMIT, &mut work);
            if zip64 {
                assert!(result.is_err());
                assert_eq!(work.candidates, 1);
            } else {
                assert_eq!(result.unwrap().eocd, selected);
                assert_eq!(work.candidates, 2);
            }
            assert!(open(&bytes).is_err());
        }
    }

    fn zip64() -> Vec<u8> {
        as_zip64(&fixture())
    }

    fn as_zip64(original: &[u8]) -> Vec<u8> {
        let end = original.len() - 22;
        let mut bytes = original[..end].to_vec();
        let mut footer = original[end..].to_vec();
        bytes.extend_from_slice(b"PK\x06\x06");
        bytes.extend_from_slice(&44u64.to_le_bytes());
        bytes.extend_from_slice(&[45, 0, 45, 0]);
        bytes.extend_from_slice(&[0; 8]);
        for value in [
            u16_at(&footer, 8).unwrap() as u64,
            u16_at(&footer, 10).unwrap() as u64,
            u32_at(&footer, 12).unwrap() as u64,
            u32_at(&footer, 16).unwrap() as u64,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(b"PK\x06\x07");
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&(end as u64).to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        footer[8..20].fill(0xff);
        bytes.extend(footer);
        bytes
    }
}
