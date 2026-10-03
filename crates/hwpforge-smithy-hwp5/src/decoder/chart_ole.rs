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
//! The outer DEFLATE stream is inflated by the OLE join
//! (`join_hwp5_ole_assets`) under the per-stream cap, the ratio limit and the
//! document budget. This module takes the inflated bytes, strips the prefix,
//! opens the inner OLE2, and returns both the OOXML chart XML and the raw
//! inner OLE2 bytes (used for the HWPX `<hp:ole>` fallback).
//!
//! Non-chart OLEs (those without `/OOXMLChartContents`) return
//! `Err(ChartOleError::NotChart)` so callers can fall back to a clean
//! drop warning.

use std::io::{Cursor, Read};

use cfb::CompoundFile;

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
#[derive(Debug, Clone)]
pub(crate) enum ChartOleError {
    /// The DEFLATE-compressed outer stream could not be inflated, or it
    /// exceeded the per-stream size cap or the decompression-ratio limit.
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

#[cfg(test)]
thread_local! {
    /// Test-only count of [`extract_chart_payload_from_inflated`] calls, so
    /// projection tests can prove extraction stops once the budget is spent.
    pub(crate) static EXTRACT_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Extracts a chart payload from an inflated HWP5 OLE BinData entry.
///
/// `inflated` is the outer DEFLATE stream already decompressed by the OLE
/// join: a 4-byte length prefix followed by an OLE2 compound file.
/// `/OOXMLChartContents` is read with `take(inner_cap + 1)`, so at most
/// `inner_cap + 1` bytes are read from it before an over-cap stream is
/// rejected; production passes
/// `inflated.len()`, since a stream inside the container cannot legitimately
/// be larger than the container.
///
/// Returns the extracted OOXML chart XML and inner OLE2 bytes, or an
/// error explaining why this entry is not a chart.
pub(crate) fn extract_chart_payload_from_inflated(
    inflated: &[u8],
    inner_cap: u64,
) -> Result<ExtractedChartPayload, ChartOleError> {
    #[cfg(test)]
    EXTRACT_CALLS.with(|calls| calls.set(calls.get() + 1));

    // 1. Strip 4-byte little-endian length prefix.
    if inflated.len() < 4 {
        return Err(ChartOleError::TooShort);
    }
    let inner = &inflated[4..];

    // 2. Sanity-check OLE2 magic before handing to cfb.
    if inner.len() < 8 || &inner[..4] != b"\xD0\xCF\x11\xE0" {
        return Err(ChartOleError::NotOle2("missing D0CF11E0 magic".to_string()));
    }

    // 3. Open inner OLE2 (borrowing the inflated bytes) and look for
    //    /OOXMLChartContents.
    let mut inner_cfb = CompoundFile::open(Cursor::new(inner))
        .map_err(|e| ChartOleError::NotOle2(e.to_string()))?;

    let chart_path = "/OOXMLChartContents";
    let has_chart = inner_cfb.walk().any(|entry| entry.path().to_string_lossy() == chart_path);
    if !has_chart {
        return Err(ChartOleError::NotChart);
    }

    let stream =
        inner_cfb.open_stream(chart_path).map_err(|e| ChartOleError::ReadStream(e.to_string()))?;
    let mut xml_bytes = Vec::new();
    stream
        .take(inner_cap.saturating_add(1))
        .read_to_end(&mut xml_bytes)
        .map_err(|e| ChartOleError::ReadStream(e.to_string()))?;
    if xml_bytes.len() as u64 > inner_cap {
        return Err(ChartOleError::ReadStream(format!(
            "{chart_path} read {} bytes, exceeds limit of {inner_cap}",
            xml_bytes.len()
        )));
    }

    // OOXMLChartContents may begin with a UTF-8 BOM; strip it for a clean
    // round-trip into our `Chart/chartN.xml` file. quick-xml on the consumer
    // side tolerates either form, but truth output we compared against had
    // no BOM in the body.
    let xml_slice: &[u8] =
        if xml_bytes.starts_with(&[0xEF, 0xBB, 0xBF]) { &xml_bytes[3..] } else { &xml_bytes };

    let chart_xml = std::str::from_utf8(xml_slice)
        .map_err(|e| ChartOleError::InvalidUtf8(e.to_string()))?
        .to_string();

    Ok(ExtractedChartPayload { chart_xml, ole_bytes: inner.to_vec() })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// Raw (still DEFLATE-compressed) `/BinData/BIN0001.OLE` of the
    /// single-column chart fixture.
    pub(crate) fn chart_fixture_raw_bin0001() -> Vec<u8> {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/charts/chart_01_single_column.hwp");
        let bytes = fs::read(&fixture).expect("chart fixture must exist");
        let mut cfb = CompoundFile::open(Cursor::new(bytes)).expect("fixture is CFB");
        let mut stream = cfb.open_stream("/BinData/BIN0001.OLE").expect("BIN0001.OLE");
        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).expect("read BIN0001.OLE");
        buf
    }

    /// Inflated `/BinData/BIN0001.OLE` of the single-column chart fixture.
    pub(crate) fn chart_fixture_inflated() -> Vec<u8> {
        crate::decoder::package::decompress_stream(&chart_fixture_raw_bin0001())
            .expect("fixture OLE inflates")
    }

    #[test]
    fn extract_chart_payload_from_real_fixture_returns_ooxml_and_ole_bytes() {
        let inflated = chart_fixture_inflated();
        let payload = extract_chart_payload_from_inflated(&inflated, inflated.len() as u64)
            .expect("extraction should succeed");
        assert!(
            payload.chart_xml.contains("<c:chartSpace"),
            "chart_xml should contain <c:chartSpace> root, got prefix={:?}",
            &payload.chart_xml.chars().take(64).collect::<String>()
        );
        assert_eq!(payload.ole_bytes, inflated[4..], "ole_bytes = inflated minus the prefix");
        assert_eq!(
            &payload.ole_bytes[..4],
            b"\xD0\xCF\x11\xE0",
            "ole_bytes must start with OLE2 magic"
        );
    }

    #[test]
    fn extract_chart_payload_rejects_too_short_payload() {
        let err = extract_chart_payload_from_inflated(&[1, 2, 3], 3).unwrap_err();
        assert!(matches!(err, ChartOleError::TooShort));
    }

    // 이것을 실패시키는 것 (따로 실행한 변이 둘):
    // - `.take(inner_cap + 1)` 제거 → 스트림 전체를 읽어 보고된 길이가 커진다.
    // - 읽은 뒤의 `> inner_cap` 비교 제거 → 오류가 사라지고 추출이 성공한다.
    #[test]
    fn inner_chart_stream_read_stops_one_byte_past_inner_cap() {
        let inflated = chart_fixture_inflated();
        let inner_cap = 16;
        let err = extract_chart_payload_from_inflated(&inflated, inner_cap).unwrap_err();
        let ChartOleError::ReadStream(detail) = err else {
            panic!("expected ReadStream, got {err:?}");
        };
        assert!(
            detail.contains("read 17 bytes, exceeds limit of 16"),
            "the bounded read must stop at inner_cap + 1, got: {detail}"
        );
    }
}
