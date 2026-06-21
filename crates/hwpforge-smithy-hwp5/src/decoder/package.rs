//! OLE2/CFB package reader for HWP5 files.
//!
//! Wraps the `cfb` crate to open HWP5 compound files and expose
//! individual streams (FileHeader, DocInfo, BodyText/Section{N}, BinData).
//! Handles DEFLATE decompression via `flate2` for compressed streams.

use std::collections::HashMap;
use std::io::{Cursor, Read};

use cfb::CompoundFile;

use crate::error::{Hwp5Error, Hwp5Result};
use crate::schema::header::FileHeader;

/// Maximum decompressed size of any single stream (500 MB).
const MAX_STREAM_SIZE: u64 = 500 * 1024 * 1024;

/// Maximum allowed decompression ratio (100×).
const MAX_DECOMPRESSION_RATIO: u64 = 100;

/// Maximum number of BodyText sections to enumerate.
const MAX_SECTIONS: usize = 256;

/// Maximum cumulative decompressed size across every stream read from a single
/// package (2 GiB).
///
/// The per-stream `MAX_STREAM_SIZE` (500 MB) alone allows a malicious file with
/// `MAX_SECTIONS` (256) sections to expand to ~128 GiB. This global budget
/// bounds the aggregate while leaving generous headroom for large legitimate
/// government documents.
const MAX_TOTAL_DECOMPRESSED: u64 = 2 * 1024 * 1024 * 1024;

// ── PackageReader ─────────────────────────────────────────────────────────────

/// Opens an HWP5 OLE2/CFB container and exposes its streams.
///
/// Reads and decompresses all required streams at construction time so that
/// callers get plain `&[u8]` slices without owning a file handle.
#[derive(Debug)]
pub(crate) struct PackageReader {
    file_header: FileHeader,
    doc_info_data: Vec<u8>,
    sections_data: Vec<Vec<u8>>,
    #[allow(dead_code)]
    bin_data: HashMap<String, Vec<u8>>,
    /// Raw `\x05HwpSummaryInformation` stream bytes (Wave 12o Phase 3).
    /// `None` when the stream is absent (third-party authors may omit it);
    /// callers downgrade to `Metadata::default()` in that case.
    summary_info_data: Option<Vec<u8>>,
}

/// Detects common file signatures that prove the input is not an HWP5 OLE2/CFB
/// document, returning an actionable explanation.
///
/// HWP5 files are CFB containers whose first bytes are the magic
/// `D0 CF 11 E0 A1 B1 1A E1`. Government corpora frequently contain `.hwp`
/// files that are really HWPX (a ZIP) or a Hancom secured/DRM container; the
/// raw CFB error for those is just a byte dump, so we translate the most common
/// cases into guidance.
fn detect_non_hwp5_signature(bytes: &[u8]) -> Option<String> {
    // ZIP local-file ("PK\x03\x04") or empty-archive ("PK\x05\x06") header.
    if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") {
        return Some(
            "input has a ZIP signature (PK..), not an HWP5 OLE2/CFB container; \
             it looks like an HWPX file saved with a .hwp extension — open it through the HWPX path instead"
                .to_string(),
        );
    }
    // Hancom secured/DRM document container ("SCDS..").
    if bytes.starts_with(b"SCDS") {
        return Some(
            "input has a Hancom secured-document signature (SCDS..), not a plain HWP5 \
             OLE2/CFB container; remove the document protection in 한글 and re-save as HWP before converting"
                .to_string(),
        );
    }
    None
}

impl PackageReader {
    /// Open an HWP5 file from raw bytes.
    ///
    /// 1. Parses the OLE2/CFB container.
    /// 2. Reads `/FileHeader` → [`FileHeader::parse`].
    /// 3. Reads `/DocInfo` and decompresses if the `compressed` flag is set.
    /// 4. Enumerates `/BodyText/Section{N}` for N = 0..`MAX_SECTIONS`.
    /// 5. Reads all `/BinData/*` entries.
    pub(crate) fn open(bytes: &[u8]) -> Hwp5Result<Self> {
        Self::open_with_budget(bytes, MAX_TOTAL_DECOMPRESSED)
    }

    /// Internal worker for [`open`] with an explicit cumulative-decompression
    /// budget so tests can verify the global bound with a small value.
    fn open_with_budget(bytes: &[u8], max_total_decompressed: u64) -> Hwp5Result<Self> {
        // Surface a clear, actionable error for inputs that are obviously not
        // HWP5 OLE2/CFB containers before the raw CFB magic-number failure.
        if let Some(detail) = detect_non_hwp5_signature(bytes) {
            return Err(Hwp5Error::Cfb { detail });
        }

        let cursor = Cursor::new(bytes);
        let mut comp = CompoundFile::open(cursor)
            .map_err(|e| Hwp5Error::Cfb { detail: format!("open: {e}") })?;

        // Cumulative decompressed-size budget across every stream of this
        // package. Tracked locally to keep the helper-fn signatures clean.
        let mut total_decompressed: u64 = 0;
        let mut charge = |path: &str, len: usize| -> Hwp5Result<()> {
            total_decompressed = total_decompressed.saturating_add(len as u64);
            if total_decompressed > max_total_decompressed {
                return Err(Hwp5Error::Cfb {
                    detail: format!(
                        "total decompressed data ({total_decompressed} bytes) after '{path}' exceeds limit of {max_total_decompressed}"
                    ),
                });
            }
            Ok(())
        };

        // 1. FileHeader
        let header_bytes = read_stream(&mut comp, "/FileHeader")?;
        let file_header = FileHeader::parse(&header_bytes)?;

        // 2. DocInfo
        let doc_info_raw = read_stream(&mut comp, "/DocInfo")?;
        let doc_info_data = if file_header.flags.compressed {
            decompress_checked(&doc_info_raw, "/DocInfo")?
        } else {
            doc_info_raw
        };
        charge("/DocInfo", doc_info_data.len())?;

        // 3. BodyText sections
        let mut sections_data: Vec<Vec<u8>> = Vec::new();
        for n in 0..MAX_SECTIONS {
            let path = format!("/BodyText/Section{n}");
            match read_stream(&mut comp, &path) {
                Ok(raw) => {
                    let data = if file_header.flags.compressed {
                        decompress_checked(&raw, &path)?
                    } else {
                        raw
                    };
                    charge(&path, data.len())?;
                    sections_data.push(data);
                }
                Err(Hwp5Error::MissingStream { .. }) => break,
                Err(e) => return Err(e),
            }
        }

        // 4. BinData entries
        let mut bin_data: HashMap<String, Vec<u8>> = HashMap::new();
        let bin_entries: Vec<String> = comp
            .read_storage("/BinData")
            .map(|entries| {
                entries
                    .filter(|e| e.is_stream())
                    .map(|e| e.path().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();

        for path in bin_entries {
            match read_stream(&mut comp, &path) {
                Ok(data) => {
                    let name = path.trim_start_matches("/BinData/").to_string();
                    bin_data.insert(name, data);
                }
                Err(e) => return Err(e),
            }
        }

        // 5. \x05HwpSummaryInformation (Wave 12o Phase 3).
        // PropertySet streams are never document-compressed even when the
        // FileHeader's `compressed` flag is set — they are raw OLE2
        // property-set bytes per MS Office spec.
        let summary_path = "/\u{0005}HwpSummaryInformation";
        let summary_info_data = match read_stream(&mut comp, summary_path) {
            Ok(raw) => Some(raw),
            Err(Hwp5Error::MissingStream { .. }) => None,
            Err(e) => return Err(e),
        };

        Ok(Self { file_header, doc_info_data, sections_data, bin_data, summary_info_data })
    }

    /// Raw bytes of the `\x05HwpSummaryInformation` OLE2 PropertySet
    /// stream, when present (Wave 12o Phase 3).
    pub(crate) fn summary_info_data(&self) -> Option<&[u8]> {
        self.summary_info_data.as_deref()
    }

    /// The parsed [`FileHeader`].
    pub(crate) fn file_header(&self) -> &FileHeader {
        &self.file_header
    }

    /// Decompressed bytes of the `/DocInfo` stream.
    pub(crate) fn doc_info_data(&self) -> &[u8] {
        &self.doc_info_data
    }

    /// Decompressed bytes for each `/BodyText/Section{N}` stream.
    pub(crate) fn sections_data(&self) -> &[Vec<u8>] {
        &self.sections_data
    }

    /// Number of body-text sections found.
    #[allow(dead_code)]
    pub(crate) fn section_count(&self) -> usize {
        self.sections_data.len()
    }

    /// Raw bytes for each `/BinData/*` entry, keyed by entry name.
    #[allow(dead_code)]
    pub(crate) fn bin_data(&self) -> &HashMap<String, Vec<u8>> {
        &self.bin_data
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Read the full contents of an OLE2 stream into a buffer.
///
/// Returns [`Hwp5Error::MissingStream`] if the stream does not exist, and
/// [`Hwp5Error::Cfb`] for other I/O failures. Rejects streams that exceed
/// `MAX_STREAM_SIZE`.
fn read_stream(comp: &mut CompoundFile<Cursor<&[u8]>>, path: &str) -> Hwp5Result<Vec<u8>> {
    let stream =
        comp.open_stream(path).map_err(|_| Hwp5Error::MissingStream { name: path.to_string() })?;

    // Eagerly bound the read with `take(MAX_STREAM_SIZE + 1)` so a hostile
    // stream cannot drive an unbounded allocation before the post-hoc size
    // check runs — reading at most one byte past the cap is enough to detect
    // an over-cap stream.
    let mut buf = Vec::new();
    stream
        .take(MAX_STREAM_SIZE + 1)
        .read_to_end(&mut buf)
        .map_err(|e| Hwp5Error::Cfb { detail: format!("read '{path}': {e}") })?;

    if buf.len() as u64 > MAX_STREAM_SIZE {
        return Err(Hwp5Error::Cfb {
            detail: format!("stream '{path}' exceeds {MAX_STREAM_SIZE} bytes"),
        });
    }

    Ok(buf)
}

/// Decompress a stream and enforce the decompression-ratio safety limit.
fn decompress_checked(data: &[u8], path: &str) -> Hwp5Result<Vec<u8>> {
    let decompressed = decompress_stream(data)?;
    let ratio = if data.is_empty() { 0 } else { decompressed.len() as u64 / data.len() as u64 };
    if ratio > MAX_DECOMPRESSION_RATIO {
        return Err(Hwp5Error::Cfb {
            detail: format!(
                "stream '{path}' decompression ratio {ratio} exceeds limit {MAX_DECOMPRESSION_RATIO}"
            ),
        });
    }
    Ok(decompressed)
}

/// Decompress an HWP5 stream using raw DEFLATE (with zlib fallback).
///
/// HWP5 streams are almost always raw DEFLATE; a handful of older files use
/// zlib framing. We try DEFLATE first and fall back to zlib on failure.
pub(crate) fn decompress_stream(data: &[u8]) -> Hwp5Result<Vec<u8>> {
    decompress_stream_capped(data, MAX_STREAM_SIZE)
}

/// Decompress an HWP5 stream bounded by an explicit per-stream cap.
///
/// Exposing the cap as a parameter lets tests verify the eager `take()` bound
/// with a small value instead of materializing a half-gigabyte buffer.
fn decompress_stream_capped(data: &[u8], max_stream_size: u64) -> Hwp5Result<Vec<u8>> {
    if data.is_empty() {
        return Ok(Vec::new());
    }

    // Try raw DEFLATE first (most HWP5 files). Both decoders are wrapped in
    // `take(max_stream_size + 1)` so a decompression bomb cannot allocate an
    // unbounded buffer: we read at most one byte past the cap, then reject.
    use flate2::read::DeflateDecoder;
    let mut decoder = DeflateDecoder::new(data).take(max_stream_size + 1);
    let mut decompressed = Vec::new();
    match decoder.read_to_end(&mut decompressed) {
        Ok(_) => bound_decompressed(decompressed, max_stream_size),
        Err(_) => {
            // Fallback: try zlib (some files use this).
            use flate2::read::ZlibDecoder;
            let mut decoder = ZlibDecoder::new(data).take(max_stream_size + 1);
            let mut decompressed = Vec::new();
            match decoder.read_to_end(&mut decompressed) {
                Ok(_) => bound_decompressed(decompressed, max_stream_size),
                Err(e) => Err(Hwp5Error::RecordParse {
                    offset: 0,
                    detail: format!("decompression failed: {e}"),
                }),
            }
        }
    }
}

/// Rejects a decompressed buffer that exceeds the per-stream cap.
///
/// Used after a `take(max_stream_size + 1)`-bounded read so an over-cap stream
/// is caught with at most one byte of slack rather than after an unbounded
/// allocation.
fn bound_decompressed(buf: Vec<u8>, max_stream_size: u64) -> Hwp5Result<Vec<u8>> {
    if buf.len() as u64 > max_stream_size {
        return Err(Hwp5Error::Cfb {
            detail: format!("decompressed stream exceeds {max_stream_size} bytes"),
        });
    }
    Ok(buf)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use crate::schema::header::HwpVersion;

    #[test]
    fn zip_signature_gets_actionable_error() {
        // A real corpus case: an HWPX file saved with a .hwp extension.
        let zip = b"PK\x03\x04\x14\x00\x00\x00";
        let err = PackageReader::open(zip).expect_err("ZIP must be rejected");
        let msg = err.to_string();
        assert!(msg.contains("ZIP signature"), "got: {msg}");
        assert!(msg.contains("HWPX"), "error should point at the HWPX path, got: {msg}");
    }

    #[test]
    fn secured_document_signature_gets_actionable_error() {
        let scds = b"SCDSA004";
        let err = PackageReader::open(scds).expect_err("secured doc must be rejected");
        let msg = err.to_string();
        assert!(msg.contains("secured-document"), "got: {msg}");
    }

    #[test]
    fn plain_garbage_falls_through_to_cfb_error() {
        // Non-signature garbage still hits the underlying CFB magic check.
        let garbage = [0u8; 16];
        assert!(PackageReader::open(&garbage).is_err());
    }

    /// Build a minimal valid CFB file with FileHeader + DocInfo + Section0.
    fn make_test_cfb(version: u32, flags: u32, doc_info: &[u8], section0: &[u8]) -> Vec<u8> {
        let cursor = std::io::Cursor::new(Vec::new());
        let mut comp = cfb::CompoundFile::create(cursor).unwrap();

        // FileHeader (256 bytes)
        let mut header_buf = vec![0u8; 256];
        header_buf[..18].copy_from_slice(b"HWP Document File\0");
        header_buf[32..36].copy_from_slice(&version.to_le_bytes());
        header_buf[36..40].copy_from_slice(&flags.to_le_bytes());
        let mut stream = comp.create_stream("/FileHeader").unwrap();
        stream.write_all(&header_buf).unwrap();
        drop(stream);

        // DocInfo
        let mut stream = comp.create_stream("/DocInfo").unwrap();
        stream.write_all(doc_info).unwrap();
        drop(stream);

        // BodyText/Section0
        comp.create_storage("/BodyText").unwrap();
        let mut stream = comp.create_stream("/BodyText/Section0").unwrap();
        stream.write_all(section0).unwrap();
        drop(stream);

        comp.into_inner().into_inner()
    }

    fn make_version(major: u8, minor: u8, build: u8, rev: u8) -> u32 {
        (major as u32) << 24 | (minor as u32) << 16 | (build as u32) << 8 | rev as u32
    }

    #[test]
    fn open_uncompressed_cfb() {
        let doc_info = b"test doc info data";
        let section0 = b"test section data";
        let version = make_version(5, 0, 2, 5);
        let bytes = make_test_cfb(version, 0x00, doc_info, section0); // flags=0: uncompressed
        let pkg = PackageReader::open(&bytes).unwrap();
        assert_eq!(pkg.file_header().version, HwpVersion::new(5, 0, 2, 5));
        assert_eq!(pkg.section_count(), 1);
        assert_eq!(pkg.doc_info_data(), doc_info);
        assert_eq!(pkg.sections_data()[0], section0);
    }

    #[test]
    fn open_compressed_cfb() {
        use flate2::write::DeflateEncoder;
        use flate2::Compression;

        let original = b"Hello HWP5 World! This is some test data for compression.";
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(original).unwrap();
        let compressed = encoder.finish().unwrap();

        let version = make_version(5, 0, 2, 5);
        let bytes = make_test_cfb(version, 0x01, &compressed, &compressed); // flags=1: compressed
        let pkg = PackageReader::open(&bytes).unwrap();
        assert_eq!(pkg.doc_info_data(), original);
        assert_eq!(pkg.sections_data()[0], original);
    }

    #[test]
    fn reject_invalid_cfb() {
        let err = PackageReader::open(b"not a valid CFB file").unwrap_err();
        assert!(matches!(err, Hwp5Error::Cfb { .. }));
    }

    #[test]
    fn decompress_empty() {
        let result = decompress_stream(&[]).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn decompress_raw_deflate() {
        use flate2::write::DeflateEncoder;
        use flate2::Compression;

        let original = b"Test data for DEFLATE";
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(original).unwrap();
        let compressed = encoder.finish().unwrap();

        let decompressed = decompress_stream(&compressed).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn decompress_stream_rejects_output_over_cap() {
        // Compress 1000 bytes, then decompress under a 10-byte cap: the eager
        // `take()` bound must reject without returning the full output.
        use flate2::write::DeflateEncoder;
        use flate2::Compression;

        let original = vec![0u8; 1000];
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&original).unwrap();
        let compressed = encoder.finish().unwrap();

        let err = decompress_stream_capped(&compressed, 10).unwrap_err();
        assert!(err.to_string().contains("exceeds 10 bytes"), "got: {err}");
    }

    #[test]
    fn decompress_stream_capped_accepts_output_at_cap() {
        use flate2::write::DeflateEncoder;
        use flate2::Compression;

        let original = vec![7u8; 64];
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&original).unwrap();
        let compressed = encoder.finish().unwrap();

        let out = decompress_stream_capped(&compressed, 64).expect("output at cap is ok");
        assert_eq!(out, original);
    }

    #[test]
    fn open_rejects_cumulative_decompressed_over_budget() {
        // Uncompressed CFB: DocInfo (18 bytes) + Section0 (17 bytes) = 35 bytes
        // total. A 20-byte budget is exceeded only once Section0 is charged,
        // proving the budget accumulates across streams.
        let doc_info = b"test doc info data"; // 18 bytes
        let section0 = b"test section data"; // 17 bytes
        let version = make_version(5, 0, 2, 5);
        let bytes = make_test_cfb(version, 0x00, doc_info, section0);
        let err = PackageReader::open_with_budget(&bytes, 20).unwrap_err();
        assert!(err.to_string().contains("total decompressed data"), "got: {err}");
    }

    #[test]
    fn open_with_budget_accepts_within_budget() {
        let doc_info = b"test doc info data";
        let section0 = b"test section data";
        let version = make_version(5, 0, 2, 5);
        let bytes = make_test_cfb(version, 0x00, doc_info, section0);
        let pkg = PackageReader::open_with_budget(&bytes, 1024).expect("within budget");
        assert_eq!(pkg.section_count(), 1);
    }

    #[test]
    fn decompress_zlib_fallback() {
        use flate2::write::ZlibEncoder;
        use flate2::Compression;

        let original = b"Test data for zlib";
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(original).unwrap();
        let compressed = encoder.finish().unwrap();

        let decompressed = decompress_stream(&compressed).unwrap();
        assert_eq!(decompressed, original);
    }
}
