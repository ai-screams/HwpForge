//! The 5f3e214e acceptance path. Keep selection, work, open and verify unchanged.
//! Bounded ZIP framing before zip merges effective-name keys.
//!
//! Selection follows zip 9.0.4 `format/find_central_directory.rs`: EOCD
//! backwards (comment end <= EOF), ZIP32's first CDFH at/after its relative
//! offset, ZIP64 forwards from the locator's relative offset. We fail closed
//! on ambiguous EOCDs instead of allowing zip's recovery to switch footers.
//! `read/zip_archive.rs::get_metadata` uses the in-memory EOCD size for its
//! tail window (not the 22-byte on-disk header); see EOCD_WINDOW below.
//!
//! Public APIs expose start/base/merged count, but not the selected footer,
//! declared size, original names or unmerged count (`format` is private).
//! Keep only framing/original names here; zip decodes extras and contents.

use std::collections::BTreeSet;
use std::io::{Read, Seek, SeekFrom};

use zip::read::{ArchiveOffset, Config};
use zip::ZipArchive;

use super::{
    check_count, field, invalid, invalid_structure, u16_at, u32_at, zip_error, EOCD_WINDOW,
};
#[cfg(test)]
use crate::error::CoreError;
use crate::error::CoreResult;

fn u64_at(bytes: &[u8], offset: usize) -> CoreResult<u64> {
    Ok(u64::from_le_bytes(field(bytes, offset)?))
}
fn size(value: u64) -> CoreResult<usize> {
    usize::try_from(value).map_err(|_| invalid())
}

const MAX_END_CANDIDATES: usize = 64;

#[derive(Debug)]
pub(super) struct Directory {
    pub start: usize,
    pub size: usize,
    pub count: usize,
    pub base: usize,
    pub eocd: usize,
}

impl Directory {
    pub(super) fn open<R: Read + Seek + Clone>(&self, mut reader: R) -> CoreResult<ZipArchive<R>> {
        // 이것을 실패시키는 것: Detect로 되돌려 겹치는 ZIP64 후보를 다시 복사하기.
        let file_len = reader.seek(SeekFrom::End(0)).map_err(|e| zip_error(e.to_string()))?;
        let config = Config { archive_offset: ArchiveOffset::Known(self.base as u64) };
        let archive =
            ZipArchive::with_config(config, reader).map_err(|e| zip_error(e.to_string()))?;
        // zip 9.0.4 read_central_header finishes at the last physical record's
        // end. Cloning shares metadata (Arc), and only clones the reader here.
        // zip does not retain/use the declared CD size, so compare the consumed
        // span, not a reconstructed size from decoded names/extras.
        let consumed_end =
            archive.clone().into_inner().stream_position().map_err(|e| zip_error(e.to_string()))?;
        self.verify(&archive, consumed_end, file_len)?;
        Ok(archive)
    }

    fn verify<R: Read + Seek>(
        &self,
        archive: &ZipArchive<R>,
        consumed_end: u64,
        file_len: u64,
    ) -> CoreResult<()> {
        // 이것을 실패시키는 것: 시작/크기/base/EOCD 위치 중 하나의 대조를 삭제하기.
        if file_len.checked_sub(22 + archive.comment().len() as u64) != Some(self.eocd as u64)
            || archive.central_directory_start() != self.start as u64
            || archive.offset() != self.base as u64
            || (self.count != 0
                && consumed_end.checked_sub(archive.central_directory_start())
                    != Some(self.size as u64))
        {
            return Err(zip_error("central directory interpretation mismatch"));
        }
        // 이것을 실패시키는 것: Unicode extra 적용 후 병합된 개수를 허용하기.
        if archive.len() != self.count {
            return Err(invalid_structure("duplicate effective ZIP entry name"));
        }
        Ok(())
    }
}

/// Accounts for every four-byte signature probe, including unsuccessful ones.
/// One bounded tail pass and at most one forward pass: <= 4*(file_len+EOCD_WINDOW).
#[derive(Default, Debug)]
pub(super) struct Work {
    pub scanned_bytes: usize,
    pub candidates: usize,
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
            return Err(zip_error("ZIP end-record scan budget exceeded"));
        }
        Ok(bytes.get(p..p + 4) == Some(magic.as_slice()))
    }
    fn candidate(&mut self) -> CoreResult<()> {
        self.candidates += 1;
        // 이것을 실패시키는 것: 후보 상한 삭제 또는 경계 >를 >=로 바꾸기.
        if self.candidates > MAX_END_CANDIDATES {
            return Err(zip_error("too many ZIP end-record candidates"));
        }
        Ok(())
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
    // Same window/order/comment inequality as zip 9.0.4. Do not prefer a
    // terminal outer EOCD over a parseable EOCD in its comment (r2 M1).
    let mut selected = None;
    for p in (bytes.len().saturating_sub(EOCD_WINDOW)..bytes.len().saturating_sub(3)).rev() {
        if !work.signature(bytes, p, b"PK\x05\x06", budget)? {
            continue;
        }
        work.candidate()?;
        let Ok(comment) = u16_at(bytes, p + 20) else { continue };
        let end = p + 22 + usize::from(comment);
        if end > bytes.len() {
            continue;
        }
        // 이것을 실패시키는 것: 두 EOCD 중 터미널 후보를 우선해 다른 base를 허용하기.
        if selected.replace(p).is_some() {
            return Err(zip_error("ambiguous ZIP end records"));
        }
    }
    let eocd = selected.ok_or_else(invalid)?;
    // This is an acceptance check AFTER selection, never a candidate filter.
    // 이것을 실패시키는 것: 후행 쓰레기를 허용해 주석 속 EOCD를 열기.
    if eocd + 22 + usize::from(u16_at(bytes, eocd + 20)?) != bytes.len() {
        return Err(invalid());
    }
    if u16_at(bytes, eocd + 4)? != 0 || u16_at(bytes, eocd + 6)? != 0 {
        return Err(invalid());
    }
    let count32 = u16_at(bytes, eocd + 10)?;
    if u16_at(bytes, eocd + 8)? != count32 {
        return Err(invalid());
    }
    let size32 = u32_at(bytes, eocd + 12)?;
    let offset32 = u32_at(bytes, eocd + 16)?;
    let locator = eocd.checked_sub(20).filter(|&p| bytes[p..].starts_with(b"PK\x06\x07"));
    let may_be_zip64 = count32 == u16::MAX || size32 == u32::MAX || offset32 == u32::MAX;
    let (count, directory_size, start, base, directory_end) = if let Some(locator) = locator {
        // zip consults a locator only when may_be_zip64 is true. Never silently
        // choose ZIP64 ourselves when zip would choose ZIP32.
        // 이것을 실패시키는 것: sentinel 없는 ZIP64 locator를 별도 규칙으로 선택하기.
        if !may_be_zip64 || u32_at(bytes, locator + 4)? != 0 || u32_at(bytes, locator + 16)? != 1 {
            return Err(invalid());
        }
        let relative = size(u64_at(bytes, locator + 8)?)?;
        if relative >= locator {
            return Err(invalid());
        }
        let mut selected64 = None;
        // zip's Detect order, but inspect fixed fields BEFORE extensible data
        // allocation. Known(base) below prevents zip from repeating this scan.
        for p in relative..locator.saturating_sub(3) {
            if !work.signature(bytes, p, b"PK\x06\x06", budget)? {
                continue;
            }
            work.candidate()?;
            let Ok(header) = field::<56>(bytes, p) else { continue };
            let record_size = u64_at(&header, 4)?;
            let count = u64_at(&header, 32)?;
            let offset = u64_at(&header, 48)?;
            // Matches try_candidate/try_read_eocd64 and blocks::parse in 9.0.4.
            // zip's parser admits 40..44; our framing check below requires 44.
            if record_size < 40
                || record_size.checked_add(12) != Some((locator - p) as u64)
                || u32_at(&header, 20)? != u32_at(bytes, locator + 4)?
                || (p as u64) < count.saturating_mul(46).saturating_add(offset)
            {
                continue;
            }
            selected64 = Some(p);
            break;
        }
        let zip64 = selected64.ok_or_else(invalid)?;
        if u64_at(bytes, zip64 + 4)? < 44
            || u32_at(bytes, zip64 + 16)? != 0
            || u32_at(bytes, zip64 + 20)? != 0
        {
            return Err(invalid());
        }
        let count = u64_at(bytes, zip64 + 32)?;
        if u64_at(bytes, zip64 + 24)? != count {
            return Err(invalid());
        }
        let size64 = u64_at(bytes, zip64 + 40)?;
        let offset64 = u64_at(bytes, zip64 + 48)?;
        // 이것을 실패시키는 것: ZIP32의 non-sentinel count/size/offset 대조 중 하나 삭제.
        if (count32 != u16::MAX && u64::from(count32) != count)
            || (size32 != u32::MAX && u64::from(size32) != size64)
            || (offset32 != u32::MAX && u64::from(offset32) != offset64)
        {
            return Err(invalid());
        }
        let base = zip64 - relative;
        let start = size(offset64)?.checked_add(base).ok_or_else(invalid)?;
        (count, size(size64)?, start, base, zip64)
    } else {
        if may_be_zip64 {
            return Err(invalid());
        }
        let relative = offset32 as usize;
        let start = if count32 == 0 {
            if relative > eocd {
                return Err(invalid());
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
            first.ok_or_else(invalid)?
        };
        (u64::from(count32), size32 as usize, start, start - relative, eocd)
    };
    check_count(count, limit)?;
    // 이것을 실패시키는 것: zip이 고른 첫 CDFH 대신 끝에서 size만 빼서 읽기.
    if start.checked_add(directory_size) != Some(directory_end) {
        return Err(invalid());
    }
    let directory = &bytes[start..directory_end];
    let mut position = 0;
    let mut names = Vec::new();
    while position < directory.len() {
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
        names.push(&directory[name_start..name_start + name_len]);
        // Count the actual records too: a forged EOCD count must not hide them.
        check_count(names.len() as u64, limit)?;
    }
    if names.len() as u64 != count {
        return Err(invalid());
    }
    let mut raw_names = BTreeSet::new();
    for name in &names {
        // 이것을 실패시키는 것: Unicode extra 적용 후 이름만 검사하기.
        if !raw_names.insert(*name) {
            return Err(invalid_structure("duplicate original ZIP entry name"));
        }
    }
    Ok(Directory { start, size: directory_size, count: names.len(), base, eocd })
}

#[cfg(test)]
mod tests {
    use super::super::super::{open_package, PackageLimits};
    use super::*;
    fn open(bytes: &[u8]) -> CoreResult<super::super::super::Package<'_>> {
        open_package(bytes, PackageLimits::default())
    }
    use std::cell::Cell;
    use std::io::{self, Cursor};
    use std::rc::Rc;
    use std::time::Instant;

    const FIXTURE: &[u8] =
        include_bytes!("../../../../../tests/fixtures/user_samples/text/sample-empty.hwpx");
    const LIMIT: usize = 10_000;

    fn zip64() -> Vec<u8> {
        let end = FIXTURE.len() - 22;
        let mut bytes = FIXTURE[..end].to_vec();
        let mut footer = FIXTURE[end..].to_vec();
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

    fn overlapping_candidates(n: usize) -> Vec<u8> {
        let original = zip64();
        let prefix = n * 56;
        let locator = prefix + original.len() - 42;
        let mut bytes = vec![0; prefix];
        for p in (0..prefix).step_by(56) {
            bytes[p..p + 4].copy_from_slice(b"PK\x06\x06");
            bytes[p + 4..p + 12].copy_from_slice(&((locator - p - 12) as u64).to_le_bytes());
            bytes[p + 24..p + 40].fill(0xff);
        }
        bytes.extend(original);
        bytes
    }

    #[derive(Clone)]
    struct Counted<'a> {
        cursor: Cursor<&'a [u8]>,
        read: Rc<Cell<usize>>,
    }
    impl Read for Counted<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = self.cursor.read(buf)?;
            self.read.set(self.read.get() + n);
            Ok(n)
        }
    }
    impl Seek for Counted<'_> {
        fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
            self.cursor.seek(pos)
        }
    }

    /// 이것을 실패시키는 것: 후보 상한/고정 base를 없애 zip이 겹친 확장 데이터를 반복 복사하기.
    #[test]
    fn zip9_fix2_overlapping_zip64_candidates_have_linear_work() {
        for n in [0, 450, 480, 32_000, 64_000, 128_000] {
            let bytes = overlapping_candidates(n);
            let mut work = Work::default();
            let timer = Instant::now();
            let result = preflight_with_work(&bytes, LIMIT, &mut work);
            let read = Rc::new(Cell::new(0));
            if n < 500 {
                let directory = result.unwrap();
                directory
                    .open(Counted { cursor: Cursor::new(&bytes), read: read.clone() })
                    .unwrap();
                assert!(open(&bytes).is_ok());
                // Fixed ZIP metadata I/O independent of the number/size of the
                // rejected ZIP64 candidates. Detect mutation exceeds this.
                assert!(read.get() < 8_192, "n={n} read={}", read.get());
            } else {
                assert!(
                    matches!(result, Err(CoreError::InvalidStructure { ref reason, .. }) if reason == "too many ZIP end-record candidates")
                );
                assert!(matches!(open(&bytes), Err(CoreError::InvalidStructure { .. })));
                assert_eq!(read.get(), 0);
            }
            assert!(work.candidates <= MAX_END_CANDIDATES + 1);
            assert!(work.scanned_bytes <= 4 * (bytes.len() + EOCD_WINDOW));
            eprintln!(
                "ZIP9_WORK n={n} len={} probes_bytes={} candidates={} zip_read={} elapsed_us={}",
                bytes.len(),
                work.scanned_bytes,
                work.candidates,
                read.get(),
                timer.elapsed().as_micros()
            );
        }
        // Accepted, increasingly long prefixes force the full forward pass.
        let mut previous = None;
        for n in [32_000, 64_000, 128_000] {
            let bytes = [vec![0; n * 56], zip64()].concat();
            let timer = Instant::now();
            let mut work = Work::default();
            let directory = preflight_with_work(&bytes, LIMIT, &mut work).unwrap();
            let read = Rc::new(Cell::new(0));
            directory.open(Counted { cursor: Cursor::new(&bytes), read: read.clone() }).unwrap();
            assert_eq!(directory.base, n * 56);
            if let Some((old_n, old_bytes)) = previous {
                assert_eq!(work.scanned_bytes - old_bytes, 4 * 56 * (n - old_n));
            }
            previous = Some((n, work.scanned_bytes));
            assert!(work.scanned_bytes <= 4 * (bytes.len() + EOCD_WINDOW));
            assert!(read.get() < 8_192);
            eprintln!(
                "ZIP9_LINEAR n={n} len={} probes_bytes={} zip_read={} elapsed_us={}",
                bytes.len(),
                work.scanned_bytes,
                read.get(),
                timer.elapsed().as_micros()
            );
        }
    }

    /// 이것을 실패시키는 것: 후보/바이트 상한 삭제 또는 허용 경계에서 >=로 거부하기.
    #[test]
    fn zip9_fix2_work_budget_boundaries() {
        let mut work = Work::default();
        for _ in 0..MAX_END_CANDIDATES {
            work.candidate().unwrap();
        }
        assert!(work.candidate().is_err());
        assert!(work.signature(b"PK\x05\x06", 0, b"PK\x05\x06", 4).unwrap());
        assert!(work.signature(b"PK\x05\x06", 0, b"PK\x05\x06", 4).is_err());
    }

    /// 이것을 실패시키는 것: 터미널 EOCD 우선 선택 또는 archive base 대조 생략.
    #[test]
    fn zip9_fix2_comment_eocd_cannot_redirect_to_prefix() {
        let end = FIXTURE.len() - 22;
        let start = u32_at(FIXTURE, end + 16).unwrap() as usize;
        assert_eq!(start, 23_630);
        let mut bytes = [FIXTURE[..start].to_vec(), FIXTURE.to_vec()].concat();
        bytes[start..start + 4].copy_from_slice(b"BAD!");
        // Opening is lazy; the damaged first local header fails on content access.
        assert!(open(&bytes).unwrap().read(0).is_err());
        let outer = start + end;
        bytes[outer + 20..outer + 22].copy_from_slice(&23u16.to_le_bytes());
        let mut fake = FIXTURE[end..].to_vec();
        fake[16..20].copy_from_slice(&((start * 2) as u32).to_le_bytes());
        bytes.extend(fake);
        bytes.push(0);
        let zip = ZipArchive::new(Cursor::new(&bytes)).unwrap();
        assert_eq!(zip.central_directory_start(), (start * 2) as u64);
        assert_eq!(zip.offset(), 0);
        assert_eq!(zip.len(), 11);
        assert!(
            matches!(open(&bytes), Err(CoreError::InvalidStructure { ref reason, .. }) if reason == "ambiguous ZIP end records")
        );
        // Even if the embedded footer ends at EOF, two parseable EOCDs are ambiguous.
        *bytes.get_mut(outer + 20).unwrap() = 22;
        bytes.pop();
        assert!(
            matches!(preflight(&bytes, LIMIT), Err(CoreError::InvalidStructure { ref reason, .. }) if reason == "ambiguous ZIP end records")
        );
    }

    /// 이것을 실패시키는 것: ZIP32 non-sentinel count/size/offset 중 하나의 대조 삭제.
    #[test]
    fn zip9_fix2_zip32_zip64_conflicts_are_rejected() {
        let valid = zip64();
        let end = valid.len() - 22;
        for (field, value) in [(8, 10_001u32), (10, 10_001), (12, 1), (16, 1)] {
            let mut bytes = valid.clone();
            if field < 12 {
                // Both disk and total counts: do not let their equality check
                // accidentally cover up a missing ZIP32-vs-ZIP64 check.
                bytes[end + 8..end + 10].copy_from_slice(&(value as u16).to_le_bytes());
                bytes[end + 10..end + 12].copy_from_slice(&(value as u16).to_le_bytes());
            } else {
                bytes[end + field..end + field + 4].copy_from_slice(&value.to_le_bytes());
            }
            assert!(ZipArchive::new(Cursor::new(&bytes)).is_ok());
            assert!(
                matches!(open(&bytes), Err(CoreError::InvalidStructure { .. })),
                "field {field}"
            );
        }
    }

    /// 이것을 실패시키는 것: ZIP64/일치하는 non-sentinel 값/확장 데이터/접두부를 일괄 거부하기.
    #[test]
    fn zip9_fix2_normal_zip64_and_matching_zip32_values() {
        for field in [8, 12, 16] {
            let mut bytes = zip64();
            let end = bytes.len() - 22;
            let old = FIXTURE.len() - 22;
            let length = 4;
            bytes[end + field..end + field + length]
                .copy_from_slice(&FIXTURE[old + field..old + field + length]);
            // Also exercise a legitimate extensible data sector.
            let zip64_pos = end - 76;
            bytes[zip64_pos + 4..zip64_pos + 12].copy_from_slice(&52u64.to_le_bytes());
            bytes.splice(end - 20..end - 20, *b"ext-data");
            let bytes = [b"prefix".as_slice(), bytes.as_slice()].concat();
            let directory = preflight(&bytes, LIMIT).unwrap();
            let archive = directory.open(Cursor::new(&bytes)).unwrap();
            assert_eq!(archive.raw_zip64_extensible_data_sector(), Some(b"ext-data".as_slice()));
            assert_eq!(archive.offset(), 6);
            assert!(open(&bytes).is_ok());
        }
    }

    /// 이것을 실패시키는 것: 시작/소비 크기/개수/base/EOCD 위치 대조를 개별 삭제하기.
    #[test]
    fn zip9_fix2_all_opened_directory_values_must_match() {
        let archive = ZipArchive::new(Cursor::new(FIXTURE)).unwrap();
        let end = archive.clone().into_inner().position();
        for field in ["start", "size", "count", "base", "eocd"] {
            let mut directory = preflight(FIXTURE, LIMIT).unwrap();
            directory.verify(&archive, end, FIXTURE.len() as u64).unwrap();
            match field {
                "start" => directory.start += 1,
                "size" => directory.size += 1,
                "count" => directory.count += 1,
                "base" => directory.base += 1,
                "eocd" => directory.eocd += 1,
                _ => unreachable!(),
            }
            assert!(directory.verify(&archive, end, FIXTURE.len() as u64).is_err(), "{field}");
        }
    }

    /// 이것을 실패시키는 것: zip이 무시할 sentinel 없는 locator를 별도로 신뢰하기.
    #[test]
    fn zip9_fix2_locator_requires_zip64_selection() {
        let mut bytes = zip64();
        let end = bytes.len() - 22;
        let old = FIXTURE.len() - 22;
        bytes[end + 8..end + 20].copy_from_slice(&FIXTURE[old + 8..old + 20]);
        assert!(matches!(preflight(&bytes, LIMIT), Err(CoreError::InvalidStructure { .. })));
    }

    /// 이것을 실패시키는 것: 후보 선택 후 terminal 검사 삭제, 또는 ZIP64 역방향 선택.
    #[test]
    fn zip9_fix2_selection_order_and_trailing_data() {
        let mut trailing = FIXTURE.to_vec();
        trailing.push(0);
        assert!(ZipArchive::new(Cursor::new(&trailing)).is_ok());
        assert!(preflight(&trailing, LIMIT).is_err());
        // An earlier ZIP64 candidate passes zip's fixed-header checks but has
        // a conflicting count. Do not skip it in favour of the final footer.
        let mut bytes = overlapping_candidates(480);
        let p = 450 * 56;
        bytes[p + 24..p + 32].copy_from_slice(&10u64.to_le_bytes());
        bytes[p + 32..p + 40].copy_from_slice(&11u64.to_le_bytes());
        assert!(preflight(&bytes, LIMIT).is_err());
    }

    /// 이것을 실패시키는 것: EOCD 탐색 창 축소 또는 잘린 후보를 상한 계산에서 제외하기.
    #[test]
    fn zip9_fix2_eocd_window_and_candidate_limits() {
        // zip's in-memory window includes signatures just BEFORE the maximum
        // on-disk comment window. A 22-byte-based scan misses this ambiguity.
        let mut edge = FIXTURE.to_vec();
        let end = edge.len() - 22;
        edge[end - 4..end].copy_from_slice(b"PK\x05\x06");
        edge[end + 20..end + 22].copy_from_slice(&u16::MAX.to_le_bytes());
        edge.extend(vec![b'x'; u16::MAX as usize]);
        assert!(
            matches!(preflight(&edge, LIMIT), Err(CoreError::InvalidStructure { ref reason, .. }) if reason == "ambiguous ZIP end records")
        );
        for original in [FIXTURE.to_vec(), zip64()] {
            let mut bytes = original.clone();
            let end = bytes.len() - 22;
            bytes[end + 20..end + 22].copy_from_slice(&u16::MAX.to_le_bytes());
            bytes.extend(vec![b'x'; u16::MAX as usize]);
            assert!(open(&bytes).is_ok());
            for n in [63, 64] {
                let mut bytes = original.clone();
                bytes[end + 20..end + 22].copy_from_slice(&((n * 22) as u16).to_le_bytes());
                for _ in 0..n {
                    let mut fake = [0u8; 22];
                    fake[..4].copy_from_slice(b"PK\x05\x06");
                    fake[20..].fill(0xff);
                    bytes.extend(fake);
                }
                // ZIP64 contributes one additional candidate after the tail scan.
                let expected = n == 63 && original.len() == FIXTURE.len();
                assert_eq!(open(&bytes).is_ok(), expected);
            }
        }
    }

    /// 이것을 실패시키는 것: zip의 길이/디스크/포화 개수 판정을 생략해 잘못된 앞 후보를 선택하기.
    #[test]
    fn zip9_fix2_invalid_earlier_zip64_candidates_are_skipped() {
        for reason in ["length", "disk", "count", "overflow"] {
            let mut bytes = overlapping_candidates(450);
            let p = 440 * 56;
            bytes[p + 24..p + 32].copy_from_slice(&11u64.to_le_bytes());
            bytes[p + 32..p + 40].copy_from_slice(&11u64.to_le_bytes());
            match reason {
                "length" => bytes[p + 4..p + 12].copy_from_slice(&44u64.to_le_bytes()),
                "disk" => bytes[p + 20..p + 24].copy_from_slice(&1u32.to_le_bytes()),
                "count" => bytes[p + 32..p + 40].copy_from_slice(&u64::MAX.to_le_bytes()),
                "overflow" => {
                    bytes[p + 32..p + 40].copy_from_slice(&((1u64 << 63) + 1).to_le_bytes())
                }
                _ => unreachable!(),
            }
            let directory = preflight(&bytes, LIMIT).unwrap();
            assert_eq!(directory.base, 450 * 56, "{reason}");
            assert!(open(&bytes).is_ok(), "{reason}");
        }
    }
}
