//! Helper for extracting chart payloads from HWP5 OLE-backed BinData entries.
//!
//! HWP5 charts arrive as DEFLATE-compressed bytes in `/BinData/BIN*.OLE`.
//! After DEFLATE inflation, the payload is a 4-byte little-endian length
//! prefix followed by an OLE2 compound file. The inner OLE2 carries:
//!
//! - `/Contents` — Hancom proprietary chart format
//! - `/OlePres000` — empty preview placeholder
//! - `/OOXMLChartContents` — full OOXML `<c:chartSpace>` document, ready
//!   for emission as `Chart/chartN.xml` in HWPX
//!
//! This module decompresses the outer DEFLATE stream, strips the prefix,
//! opens the inner OLE2, and returns both the OOXML chart XML and the
//! raw inner OLE2 bytes (used for the HWPX `<hp:ole>` fallback).
//!
//! Non-chart OLEs (those without `/OOXMLChartContents`) return
//! `Err(ChartOleError::NotChart)` so callers can fall back to a clean
//! drop warning.

use std::io::{Cursor, Read};

use cfb::CompoundFile;
use flate2::read::DeflateDecoder;

/// Successful extraction of a chart payload from a HWP5 OLE BinData entry.
#[derive(Debug, Clone)]
pub(crate) struct ExtractedChartPayload {
    /// Full OOXML chart XML (starts with `<?xml`, contains `<c:chartSpace>`).
    pub chart_xml: String,
    /// Raw OLE2 compound file bytes — the inner OLE2 with prefix stripped.
    /// Used for the `<hp:ole>` fallback rendering inside `<hp:switch>`.
    pub ole_bytes: Vec<u8>,
}

/// Error variants when extracting a chart from a HWP5 OLE BinData entry.
#[derive(Debug)]
pub(crate) enum ChartOleError {
    /// The DEFLATE-compressed outer stream could not be inflated.
    Inflate(String),
    /// The inflated payload was shorter than the 4-byte length prefix.
    TooShort,
    /// The inner bytes were not a valid OLE2 compound file.
    NotOle2(String),
    /// The inner OLE2 did not contain an `/OOXMLChartContents` stream
    /// (i.e. this is some other kind of OLE object — image preview, etc.).
    NotChart,
    /// I/O failure while reading `/OOXMLChartContents`.
    ReadStream(String),
    /// `/OOXMLChartContents` did not decode as UTF-8.
    InvalidUtf8(String),
}

impl std::fmt::Display for ChartOleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Inflate(detail) => write!(f, "ole_chart_inflate_failed: {detail}"),
            Self::TooShort => write!(f, "ole_chart_too_short_for_prefix"),
            Self::NotOle2(detail) => write!(f, "ole_chart_inner_not_ole2: {detail}"),
            Self::NotChart => write!(f, "ole_chart_no_ooxml_chart_contents_stream"),
            Self::ReadStream(detail) => write!(f, "ole_chart_read_stream_failed: {detail}"),
            Self::InvalidUtf8(detail) => write!(f, "ole_chart_xml_not_utf8: {detail}"),
        }
    }
}

/// Extracts a chart payload from a HWP5 OLE-backed `/BinData/BIN*.OLE` entry.
///
/// `raw_bytes` is the DEFLATE-compressed BinData stream as stored in the
/// HWP5 package (i.e. the bytes returned by `PackageReader::bin_data()`
/// when the corresponding `Hwp5BinDataRecordSummary::should_decompress`
/// is `true`).
///
/// Returns the extracted OOXML chart XML and inner OLE2 bytes, or an
/// error explaining why this entry is not a chart.
pub(crate) fn extract_chart_payload(
    raw_bytes: &[u8],
) -> Result<ExtractedChartPayload, ChartOleError> {
    // 1. Outer DEFLATE → inflated bytes.
    let mut decoder = DeflateDecoder::new(raw_bytes);
    let mut inflated = Vec::new();
    decoder.read_to_end(&mut inflated).map_err(|e| ChartOleError::Inflate(e.to_string()))?;

    // 2. Strip 4-byte little-endian length prefix.
    if inflated.len() < 4 {
        return Err(ChartOleError::TooShort);
    }
    let inner = &inflated[4..];

    // 3. Sanity-check OLE2 magic before handing to cfb.
    if inner.len() < 8 || &inner[..4] != b"\xD0\xCF\x11\xE0" {
        return Err(ChartOleError::NotOle2("missing D0CF11E0 magic".to_string()));
    }

    // 4. Open inner OLE2 and look for /OOXMLChartContents.
    let ole_bytes = inner.to_vec();
    let cursor = Cursor::new(ole_bytes.clone());
    let mut inner_cfb =
        CompoundFile::open(cursor).map_err(|e| ChartOleError::NotOle2(e.to_string()))?;

    let chart_path = "/OOXMLChartContents";
    let has_chart = inner_cfb.walk().any(|entry| entry.path().to_string_lossy() == chart_path);
    if !has_chart {
        return Err(ChartOleError::NotChart);
    }

    let mut stream =
        inner_cfb.open_stream(chart_path).map_err(|e| ChartOleError::ReadStream(e.to_string()))?;
    let mut xml_bytes = Vec::new();
    stream.read_to_end(&mut xml_bytes).map_err(|e| ChartOleError::ReadStream(e.to_string()))?;

    // OOXMLChartContents may begin with a UTF-8 BOM; strip it for a clean
    // round-trip into our `Chart/chartN.xml` file. quick-xml on the consumer
    // side tolerates either form, but truth output we compared against had
    // no BOM in the body.
    let xml_slice: &[u8] =
        if xml_bytes.starts_with(&[0xEF, 0xBB, 0xBF]) { &xml_bytes[3..] } else { &xml_bytes };

    let chart_xml = std::str::from_utf8(xml_slice)
        .map_err(|e| ChartOleError::InvalidUtf8(e.to_string()))?
        .to_string();

    Ok(ExtractedChartPayload { chart_xml, ole_bytes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn read_chart_fixture_bin0001() -> Option<Vec<u8>> {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/charts/chart_01_single_column.hwp");
        let bytes = fs::read(&fixture).ok()?;
        let cursor = Cursor::new(bytes);
        let mut cfb = CompoundFile::open(cursor).ok()?;
        let mut stream = cfb.open_stream("/BinData/BIN0001.OLE").ok()?;
        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).ok()?;
        Some(buf)
    }

    #[test]
    fn extract_chart_payload_from_real_fixture_returns_ooxml_and_ole_bytes() {
        let Some(raw) = read_chart_fixture_bin0001() else {
            // Fixture not available in this environment — skip without failing.
            return;
        };
        let payload = extract_chart_payload(&raw).expect("extraction should succeed");
        assert!(
            payload.chart_xml.contains("<c:chartSpace"),
            "chart_xml should contain <c:chartSpace> root, got prefix={:?}",
            &payload.chart_xml.chars().take(64).collect::<String>()
        );
        assert!(
            payload.ole_bytes.len() > 1024,
            "ole_bytes should carry inner OLE2 storage, got len={}",
            payload.ole_bytes.len()
        );
        assert_eq!(
            &payload.ole_bytes[..4],
            b"\xD0\xCF\x11\xE0",
            "ole_bytes must start with OLE2 magic"
        );
    }

    #[test]
    fn extract_chart_payload_rejects_non_deflate_garbage() {
        let garbage = b"not a deflate stream";
        let err = extract_chart_payload(garbage).unwrap_err();
        assert!(matches!(err, ChartOleError::Inflate(_)));
    }

    #[test]
    fn extract_chart_payload_rejects_too_short_payload() {
        // Empty DEFLATE inflates to empty, which is shorter than the 4-byte prefix.
        let mut buf = Vec::new();
        {
            use flate2::write::DeflateEncoder;
            use flate2::Compression;
            use std::io::Write;
            let mut encoder = DeflateEncoder::new(&mut buf, Compression::fast());
            encoder.write_all(&[]).unwrap();
            encoder.finish().unwrap();
        }
        let err = extract_chart_payload(&buf).unwrap_err();
        assert!(matches!(err, ChartOleError::TooShort));
    }
}
