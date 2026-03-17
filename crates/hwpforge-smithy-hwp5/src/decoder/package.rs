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
        let cursor = Cursor::new(bytes);
        let mut comp = CompoundFile::open(cursor)
            .map_err(|e| Hwp5Error::Cfb { detail: format!("open: {e}") })?;

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

        Ok(Self { file_header, doc_info_data, sections_data, bin_data })
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
    let mut stream =
        comp.open_stream(path).map_err(|_| Hwp5Error::MissingStream { name: path.to_string() })?;

    let mut buf = Vec::new();
    stream
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
    if data.is_empty() {
        return Ok(Vec::new());
    }

    // Try raw DEFLATE first (most HWP5 files).
    use flate2::read::DeflateDecoder;
    let mut decoder = DeflateDecoder::new(data);
    let mut decompressed = Vec::new();
    match decoder.read_to_end(&mut decompressed) {
        Ok(_) => Ok(decompressed),
        Err(_) => {
            // Fallback: try zlib (some files use this).
            use flate2::read::ZlibDecoder;
            let mut decoder = ZlibDecoder::new(data);
            let mut decompressed = Vec::new();
            match decoder.read_to_end(&mut decompressed) {
                Ok(_) => Ok(decompressed),
                Err(e) => Err(Hwp5Error::RecordParse {
                    offset: 0,
                    detail: format!("decompression failed: {e}"),
                }),
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use crate::schema::header::HwpVersion;

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
