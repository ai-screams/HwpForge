//! Shared preserve-first HWPX package storage.

use crate::decoder::package::PackageReader;
use crate::error::{HwpxError, HwpxResult};
use crate::package_path::package_error;
use hwpforge_core::zip_package::{open_package, PackageLimits, MAX_ENTRY_SIZE, MAX_TOTAL_SIZE};
use std::collections::BTreeMap;
use std::io::{Cursor, Write as _};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

#[derive(Debug, Clone)]
pub(crate) struct RawPackage {
    pub(crate) order: crate::decoder::package_order::PackageOrder,
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
thread_local! {
    // Full-package reads and writes on this test thread, excluding metadata opens.
    static PACKAGE_IO: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
}

impl RawPackage {
    /// Resolve a Core section index using the decoder's package reading order.
    /// 이것을 실패시키는 것: section index를 파일 번호로 사용하거나 범위 밖을 폴백하기.
    pub(crate) fn section_path(&self, section_idx: usize) -> HwpxResult<String> {
        self.order.sections.get(section_idx).cloned().ok_or_else(|| HwpxError::InvalidStructure {
            detail: format!(
                "section {section_idx} out of range (package has {} sections)",
                self.order.sections.len()
            ),
        })
    }

    pub(crate) fn read(bytes: &[u8]) -> HwpxResult<Self> {
        let reader = PackageReader::new(bytes)?;
        let mut package = Self::read_capped(bytes, MAX_ENTRY_SIZE, MAX_TOTAL_SIZE)?;
        package.order = reader.order;
        Ok(package)
    }

    /// Reads every entry, bounding decompression per entry and in total.
    ///
    /// The ZIP header size is attacker-controlled, so it is not used to size
    /// buffers at all: many entries declaring 50 MiB while holding a few
    /// bytes would otherwise reserve gigabytes that the cumulative budget
    /// (which counts real bytes) never sees. Buffers grow with the data, and
    /// the reader is limited with `take(max_entry + 1)` so a bomb stops one
    /// byte past the limit.
    fn read_capped(bytes: &[u8], max_entry: u64, max_total: u64) -> HwpxResult<Self> {
        #[cfg(test)]
        PACKAGE_IO.with(|count| count.set((count.get().0 + 1, count.get().1)));
        let mut archive = open_package(
            bytes,
            PackageLimits { entry: max_entry, total: max_total, ..Default::default() },
        )
        .map_err(package_error)?;
        let mut entries = Vec::with_capacity(archive.entries().len());
        let mut index_by_path = BTreeMap::new();
        for index in 0..archive.entries().len() {
            let entry = archive.entries()[index].clone();
            let data = archive.read(index).map_err(package_error)?;
            index_by_path.insert(entry.path.clone(), entries.len());
            entries.push(RawPackageEntry {
                opaque: archive.is_opaque(index),
                path: entry.path,
                bytes: data,
                compression: entry.compression,
            });
        }

        Ok(Self { entries, index_by_path, order: Default::default() })
    }

    pub(crate) fn entry_paths(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| e.path.as_str())
    }

    pub(crate) fn read_entry(&self, path: &str) -> HwpxResult<&[u8]> {
        let index = *self
            .index_by_path
            .get(path)
            .ok_or_else(|| HwpxError::MissingFile { path: path.into() })?;
        if self.entries[index].opaque {
            return Err(HwpxError::InvalidStructure {
                detail: format!("opaque ZIP entry selected semantically: '{path}'"),
            });
        }
        Ok(&self.entries[index].bytes)
    }

    /// Already bounded physical bytes, for unknown-input hashing only.
    pub(crate) fn preserved_entries(&self) -> impl Iterator<Item = (&str, &[u8], bool)> {
        self.entries.iter().map(|e| (e.path.as_str(), e.bytes.as_slice(), e.opaque))
    }

    /// Preserves opaque entries from `base` in a generated `output` package.
    /// If base metadata has no opaque entries, returns `output` unchanged,
    /// without validating or rewriting it. Otherwise both packages must be
    /// readable; read, duplicate-path, and final write/name-policy errors propagate.
    /// Failure discards the temporary package; `base` is never modified.
    pub(crate) fn preserve_opaque(base: &[u8], output: Vec<u8>) -> HwpxResult<Vec<u8>> {
        let package = open_package(base, PackageLimits::default()).map_err(package_error)?;
        // 이것을 실패시키는 것: opaque 없음 조기 반환 제거 — 전체 읽기·쓰기가 추가됨.
        if !(0..package.entries().len()).any(|index| package.is_opaque(index)) {
            return Ok(output);
        }
        let original = Self::read(base)?;
        let mut preserved = Self::read(&output)?;
        preserved.carry_opaque_from(&original)?;
        preserved.write()
    }

    /// Copies already classified opaque entries from `base` into this package.
    /// An existing identical path returns `HwpxError::InvalidStructure`; callers
    /// must use `write()` to validate the merged package's complete name policy.
    /// This is not transactional: entries copied before a later conflict remain
    /// in `self` on failure. `base` is never modified.
    pub(crate) fn carry_opaque_from(&mut self, base: &Self) -> HwpxResult<()> {
        for entry in base.entries.iter().filter(|e| e.opaque) {
            if self.index_by_path.contains_key(&entry.path) {
                return Err(HwpxError::InvalidStructure {
                    detail: format!("generated part replaces opaque ZIP entry '{}'", entry.path),
                });
            }
            self.index_by_path.insert(entry.path.clone(), self.entries.len());
            self.entries.push(entry.clone());
        }
        Ok(())
    }

    pub(crate) fn read_text_entry(&self, path: &str) -> HwpxResult<String> {
        crate::package_xml::text_entry(path, self.read_entry(path)?)
    }

    pub(crate) fn replace_text_entry(&mut self, path: &str, content: String) {
        if let Some(index) = self.index_by_path.get(path).copied() {
            self.entries[index].bytes = content.into_bytes();
        }
    }

    pub(crate) fn write(&self) -> HwpxResult<Vec<u8>> {
        #[cfg(test)]
        PACKAGE_IO.with(|count| count.set((count.get().0, count.get().1 + 1)));
        let cursor = Cursor::new(Vec::<u8>::new());
        let mut zip = ZipWriter::new(cursor);

        for entry in &self.entries {
            // W0: fixed entry time (the ZIP epoch), never the clock.
            let options = SimpleFileOptions::default()
                .compression_method(entry.compression)
                .last_modified_time(zip::DateTime::default());
            zip.start_file(&entry.path, options).map_err(|e| HwpxError::Zip(e.to_string()))?;
            zip.write_all(&entry.bytes).map_err(|e| HwpxError::Zip(e.to_string()))?;
        }

        let cursor = zip.finish().map_err(|e| HwpxError::Zip(e.to_string()))?;
        let bytes = cursor.into_inner();
        // New bytes require new core name evidence (including collisions with
        // generated parts). Do not reuse the input's opaque flags as approval.
        open_package(&bytes, PackageLimits::default()).map_err(package_error)?;
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_core::Section;
    use zip::ZipArchive;

    // 이것을 실패시키는 것: opaque 없음 조기 반환 제거, 전체 읽기·쓰기 추가, 출력 바이트 변경.
    #[test]
    // 이것을 실패시키는 것: 비정규 이름으로 opaque-free 조건 제거 (실제 음성 대조 실행).
    fn opaque_free_carry_keeps_bytes_without_package_io() {
        let bytes = zeros_zip(&[("Contents/section0.xml", 16), ("Contents/header.xml", 8)]);
        let base = bytes.as_slice();
        let package = open_package(base, Default::default()).unwrap();
        assert!((0..package.entries().len()).all(|index| !package.is_opaque(index)));
        // A comment makes a redundant rewrite observable in the bytes as well.
        let mut output = base.to_vec();
        let end = output.len() - 22;
        output[end + 20..end + 22].copy_from_slice(&4u16.to_le_bytes());
        output.extend(b"keep");
        PACKAGE_IO.with(|count| count.set((0, 0)));
        let actual = RawPackage::preserve_opaque(base, output.clone()).unwrap();
        assert_eq!(PACKAGE_IO.with(|count| count.get()), (0, 0));
        assert_eq!(actual, output);
    }

    /// Builds an in-memory ZIP whose entries are `(name, size)` runs of zeros.
    fn zeros_zip(entries: &[(&str, usize)]) -> Vec<u8> {
        let mut zip = ZipWriter::new(Cursor::new(Vec::<u8>::new()));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, size) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(&vec![0u8; *size]).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    /// 이것을 실패시키는 것: patch의 두 번째 ZIP 읽기가 공용 이름 검사를 우회하기.
    #[test]
    fn zip9_fix3_patch_rejects_parent_part() {
        let bytes = zeros_zip(&[("Contents/../unused.xml", 1)]);
        // Call the raw reread directly: the preceding decoder must not mask a bypass.
        assert!(matches!(RawPackage::read_capped(&bytes, MAX_ENTRY_SIZE, MAX_TOTAL_SIZE),
            Err(HwpxError::InvalidStructure { detail })
            if detail == "invalid package part name 'Contents/../unused.xml'"));
    }

    /// Rewrites the declared uncompressed size in every central-directory
    /// record, leaving the compressed data (and its CRC) untouched.
    fn forge_declared_size(zip: &mut [u8], declared: u32) {
        let mut at = 0;
        while at + 28 <= zip.len() {
            if zip[at..at + 4] == [0x50, 0x4b, 0x01, 0x02] {
                zip[at + 24..at + 28].copy_from_slice(&declared.to_le_bytes());
                at += 46;
            } else {
                at += 1;
            }
        }
    }

    /// W0: a rewritten package keeps every entry at the fixed ZIP epoch.
    /// 이것을 실패시키는 것: 시각 고정 삭제 — `--features zip/time` 으로 돌릴 때
    /// (그 기능이 꺼진 빌드에서는 zip 기본값이 같은 epoch 라 통과한다).
    #[test]
    fn rewritten_entries_keep_a_fixed_time() {
        let bytes = zeros_zip(&[("a.bin", 8), ("b.bin", 8)]);
        let out = RawPackage::read_capped(&bytes, 1024, u64::MAX).unwrap().write().unwrap();
        let mut archive = ZipArchive::new(Cursor::new(out)).unwrap();
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

    // 이것을 실패시키는 것: 버퍼 용량을 헤더 크기로 예약 (`Vec::with_capacity(file.size().min(max_entry) as usize)`)
    #[test]
    fn read_capped_does_not_reserve_capacity_from_the_forged_header_size() {
        let mut bytes = zeros_zip(&[("a.bin", 8), ("b.bin", 8), ("c.bin", 8)]);
        forge_declared_size(&mut bytes, 10 * 1024 * 1024);
        // Guard the forgery itself: the archive must really declare 10 MiB.
        let mut archive = ZipArchive::new(Cursor::new(&bytes[..])).unwrap();
        assert_eq!(archive.by_index(0).unwrap().size(), 10 * 1024 * 1024);
        drop(archive);

        let package = RawPackage::read_capped(&bytes, 20 * 1024 * 1024, u64::MAX).unwrap();

        for entry in &package.entries {
            assert_eq!(entry.bytes.len(), 8);
            assert!(entry.bytes.capacity() < 4096, "capacity {}", entry.bytes.capacity());
        }
    }

    /// 이것을 실패시키는 것: 실제 해제량보다 작은 ZIP 헤더를 신뢰하여 읽기 오류를 무시하기.
    #[test]
    fn zip9_read_capped_rejects_understated_size() {
        let mut bytes = zeros_zip(&[("a.bin", 1024)]);
        forge_declared_size(&mut bytes, 1);
        let err = RawPackage::read_capped(&bytes, 2048, 4096).expect_err("forged size");
        assert!(matches!(err, HwpxError::Zip(detail) if detail.contains("read 'a.bin'")));
    }

    // 이것을 실패시키는 것: `file.take(max_entry + 1)` 제거 (해제량이 cap+1 이 아니라 cap*10 으로 보고됨)
    #[test]
    fn read_capped_stops_decompression_one_byte_past_the_entry_cap() {
        let cap: u64 = 1024;
        let bytes = zeros_zip(&[("bomb.bin", cap as usize * 10)]);

        let err = RawPackage::read_capped(&bytes, cap, u64::MAX).unwrap_err();

        match err {
            HwpxError::InvalidStructure { detail } => assert_eq!(
                detail,
                format!(
                    "entry 'bomb.bin' decompressed to {} bytes, exceeds limit of {cap}",
                    cap + 1
                )
            ),
            other => panic!("expected InvalidStructure, got {other:?}"),
        }
    }

    // 이것을 실패시키는 것: 누적 총량 검사(`total > max_total`) 제거
    #[test]
    fn read_capped_rejects_entries_that_are_small_alone_but_large_together() {
        let bytes = zeros_zip(&[("a.bin", 600), ("b.bin", 600)]);

        let err = RawPackage::read_capped(&bytes, 1024, 1000).unwrap_err();

        match err {
            HwpxError::InvalidStructure { detail } => {
                assert_eq!(detail, "total decompressed data (1001 bytes) exceeds limit of 1000")
            }
            other => panic!("expected InvalidStructure, got {other:?}"),
        }
    }

    // 이것을 실패시키는 것: 엔트리 검사 `>` 를 `>=` 로 바꿈 (정확히 cap 바이트가 거부됨)
    // 총량 검사 `>` → `>=` 도 같은 테스트(total == 상한)가 잡는다.
    #[test]
    fn read_capped_accepts_an_entry_of_exactly_the_cap() {
        let bytes = zeros_zip(&[("exact.bin", 1024)]);

        let package = RawPackage::read_capped(&bytes, 1024, 1024).unwrap();

        assert_eq!(package.entries[0].bytes.len(), 1024);
    }

    /// A decodable package can still hold a document Core rejects.
    ///
    /// This is the reachability question behind `ops::validate`'s `ok: false`
    /// branch (W1b review R2, finding 10). The encoder cannot produce such a
    /// package — `HwpxEncoder::encode` takes a `Document<Validated>`, so the
    /// type state makes an invalid document unencodable — but a package from
    /// another producer can carry a section with no paragraphs, and the
    /// decoder reports it faithfully rather than inventing a paragraph.
    ///
    /// The failed verdict is therefore a real branch, not dead code.
    ///
    /// The neighbouring case is the opposite: a `<hp:p>` stripped of its runs
    /// decodes with a synthesised run, so `EmptyParagraph` is **not**
    /// reachable this way. Both are asserted, so a decoder change that starts
    /// synthesising sections (or stops synthesising runs) is caught here.
    ///
    /// It lives in this module because `RawPackage` is the crate's only
    /// package writer and is `pub(crate)`.
    #[test]
    fn a_section_without_paragraphs_decodes_into_a_document_core_rejects() {
        use crate::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
        use crate::HwpxEncoder;
        use hwpforge_core::image::ImageStore;
        use hwpforge_core::run::Run;
        use hwpforge_core::{Document, PageSettings, Paragraph};
        use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};

        let para = |text: &str| {
            Paragraph::with_runs(
                vec![Run::text(text, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )
        };
        let mut document = Document::new();
        document.add_section(Section::with_paragraphs(
            vec![para("hello"), para("world")],
            PageSettings::a4(),
        ));
        let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
        styles.push_char_shape(HwpxCharShape::default());
        styles.push_para_shape(HwpxParaShape::default());
        let bytes = HwpxEncoder::encode(&document.validate().unwrap(), &styles, &ImageStore::new())
            .unwrap();
        let section_entry = RawPackage::read(&bytes).unwrap().section_path(0).unwrap();

        // ── a paragraph with no runs: the decoder fills one in ──────
        let mut pkg = RawPackage::read(&bytes).unwrap();
        let xml = pkg.read_text_entry(&section_entry).unwrap();
        let second_para = r#"<hp:p id="1" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0">"#;
        let runless = xml.replace(
            &format!("{second_para}<hp:run charPrIDRef=\"0\"><hp:t>world</hp:t></hp:run></hp:p>"),
            &format!("{second_para}</hp:p>"),
        );
        assert_ne!(runless, xml, "the fixture XML shape changed; update this test");
        pkg.replace_text_entry(&section_entry, runless);
        let decoded = crate::HwpxDecoder::decode(&pkg.write().unwrap()).unwrap();
        assert!(
            decoded.document.sections()[0].paragraphs.iter().all(|p| !p.runs.is_empty()),
            "the decoder synthesises a run, so EmptyParagraph is unreachable this way"
        );
        assert!(decoded.document.validate().is_ok());

        // ── a section with no paragraphs: Core rejects it ───────────
        let mut pkg = RawPackage::read(&bytes).unwrap();
        let xml = pkg.read_text_entry(&section_entry).unwrap();
        let first_para = xml.find("><hp:p").map(|i| i + 1).unwrap();
        let sec_close = xml.rfind("</hs:sec>").unwrap();
        pkg.replace_text_entry(
            &section_entry,
            format!("{}{}", &xml[..first_para], &xml[sec_close..]),
        );

        let decoded = crate::HwpxDecoder::decode(&pkg.write().unwrap())
            .expect("an empty section still decodes");

        assert_eq!(decoded.document.sections().len(), 1);
        assert!(decoded.document.sections()[0].paragraphs.is_empty());
        assert!(
            matches!(
                decoded.document.validate(),
                Err(hwpforge_core::CoreError::Validation(
                    hwpforge_core::ValidationError::EmptySection { section_index: 0 }
                ))
            ),
            "`ops::validate` reports ok:false for exactly this document"
        );
    }
}
