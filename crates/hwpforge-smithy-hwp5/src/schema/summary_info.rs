//! OLE2 PropertySet parser for the `\x05HwpSummaryInformation` stream.
//!
//! Wave 12o Phase 3 — extracts Hancom-stored document metadata (title /
//! author / subject / description / lastsaveby / created / modified /
//! keywords) from the standard Office SummaryInformation PropertySet
//! that 한컴 emits alongside the body streams.
//!
//! # Wire format
//!
//! Standard Microsoft Office PropertySet layout:
//!
//! ```text
//!   0x00 .. 0x02   BOM (0xfffe little-endian)
//!   0x02 .. 0x04   format version
//!   0x04 .. 0x06   OS (kind)
//!   0x06 .. 0x08   OS version
//!   0x08 .. 0x18   FMTID (16-byte CLSID)
//!   0x18 .. 0x1c   reserved / section count (always 1 for Hancom)
//!   0x1c .. 0x2c   section FMTID
//!   0x2c .. 0x30   u32 offset of section start (typically 0x30)
//!
//!   At section start:
//!     +0    u32 section length (bytes)
//!     +4    u32 property count
//!     +8    property table (each entry: u32 pid, u32 offset within section)
//!
//!   Each property body:
//!     u32 VT type
//!     u32 length (chars for LPSTR/LPWSTR; high bits for FILETIME)
//!     payload (LPSTR = ASCII bytes; LPWSTR = UTF-16LE code units;
//!              FILETIME = 64-bit 100-ns units since 1601-01-01 UTC)
//! ```
//!
//! Architect-mandated security posture (Wave 12o §11.4 S2 / S7):
//!
//! - **S2 offset cycle detection**: the property table is required to be
//!   strictly monotonically increasing within the section bounds, and
//!   every offset must point inside the section. Re-entrant offsets are
//!   rejected, defeating the classic PropertySet payload-cycle DoS.
//! - **S7 UTF-16LE BOM strip**: VT_LPWSTR payloads sometimes carry an
//!   inline BOM (0xfeff) — strip it explicitly so it does not survive
//!   into [`Metadata`] values.
//!
//! Defense-in-depth caps mirror the HWPX decoder
//! ([`crate::decoder::package`] constants): per-property allocation
//! capped at 64 KiB, property count capped at 256.

use hwpforge_core::metadata::Metadata;

use crate::error::{Hwp5Error, Hwp5Result};

// ── Wire constants ──────────────────────────────────────────────────

/// PropertySet byte-order marker (little-endian).
const BOM_LE: u16 = 0xfffe;

/// Maximum per-property payload size we are willing to allocate.
const MAX_PROPERTY_BYTES: usize = 64 * 1024;

/// Maximum number of properties per section.
const MAX_PROPERTY_COUNT: u32 = 256;

/// 100-nanosecond intervals between FILETIME epoch (1601-01-01 UTC) and
/// Unix epoch (1970-01-01 UTC).
const FILETIME_UNIX_DELTA: u64 = 11_644_473_600_u64 * 10_000_000;

// VT (variant type) constants from MS Office PropertySet spec.
const VT_LPSTR: u32 = 0x001E;
const VT_LPWSTR: u32 = 0x001F;
const VT_FILETIME: u32 = 0x0040;

// Standard PIDs (Hancom mirrors these).
const PID_TITLE: u32 = 0x02;
const PID_SUBJECT: u32 = 0x03;
const PID_AUTHOR: u32 = 0x04;
const PID_KEYWORDS: u32 = 0x05;
const PID_COMMENTS: u32 = 0x06;
const PID_LAST_AUTHOR: u32 = 0x08;
const PID_LAST_SAVED_TIME: u32 = 0x0C;
const PID_CREATED_TIME: u32 = 0x0D;
// 한컴 custom — observed at offset 0x14 in fixtures: a formatted Korean
// locale date string. Carried verbatim through `extras` so a future
// HWP5 → HWPX round-trip preserves byte parity.
const PID_HANCOM_DATE_DISPLAY: u32 = 0x14;
// 한컴 custom — observed at offset 0x15 in fixtures: appname / version
// string ("12.30.0.6382 MAC64LE"). Also carried via `extras`.
const PID_HANCOM_APP_NAME: u32 = 0x15;

// ── Public API ──────────────────────────────────────────────────────

/// Parses a raw `\x05HwpSummaryInformation` OLE2 stream into a
/// populated [`Metadata`].
///
/// Returns [`Metadata::default()`] for streams that are too short to
/// hold a valid PropertySet header. Errors are returned for structural
/// violations (bad BOM, offset cycle, allocation cap exceeded, etc.).
pub fn parse_summary_information(bytes: &[u8]) -> Hwp5Result<Metadata> {
    if bytes.len() < 0x30 {
        return Ok(Metadata::default());
    }

    // 1. Verify byte-order marker.
    let bom = u16::from_le_bytes([bytes[0], bytes[1]]);
    if bom != BOM_LE {
        return Err(Hwp5Error::RecordParse {
            offset: 0,
            detail: format!("HwpSummaryInformation: bad BOM {bom:#06x}, expected {BOM_LE:#06x}"),
        });
    }

    // 2. Section count (typically 1 for Hancom; reject anything else).
    let section_count = u32::from_le_bytes([bytes[0x18], bytes[0x19], bytes[0x1a], bytes[0x1b]]);
    if section_count != 1 {
        return Err(Hwp5Error::RecordParse {
            offset: 0,
            detail: format!("HwpSummaryInformation: unsupported section count {section_count}"),
        });
    }

    // 3. Section start offset (typically 0x30).
    let sec_start =
        u32::from_le_bytes([bytes[0x2c], bytes[0x2d], bytes[0x2e], bytes[0x2f]]) as usize;
    if sec_start + 8 > bytes.len() {
        return Err(Hwp5Error::RecordParse {
            offset: 0,
            detail: format!(
                "HwpSummaryInformation: section start {sec_start:#x} past stream end {}",
                bytes.len()
            ),
        });
    }

    let section = &bytes[sec_start..];
    parse_section(section)
}

// ── Section parser ──────────────────────────────────────────────────

fn parse_section(section: &[u8]) -> Hwp5Result<Metadata> {
    let section_len = u32::from_le_bytes([section[0], section[1], section[2], section[3]]) as usize;
    let property_count = u32::from_le_bytes([section[4], section[5], section[6], section[7]]);

    if section_len > section.len() {
        return Err(Hwp5Error::RecordParse {
            offset: 0,
            detail: format!(
                "HwpSummaryInformation: section_len {section_len} > slice {}",
                section.len()
            ),
        });
    }
    if property_count > MAX_PROPERTY_COUNT {
        return Err(Hwp5Error::RecordParse {
            offset: 0,
            detail: format!(
                "HwpSummaryInformation: property_count {property_count} exceeds cap {MAX_PROPERTY_COUNT}"
            ),
        });
    }

    let table_bytes = property_count as usize * 8;
    if 8 + table_bytes > section_len {
        return Err(Hwp5Error::RecordParse {
            offset: 0,
            detail: "HwpSummaryInformation: property table overruns section".into(),
        });
    }

    // ── S2: parse property table and validate offset monotonicity ──
    let mut entries: Vec<(u32, u32)> = Vec::with_capacity(property_count as usize);
    let mut last_offset: i64 = -1;
    for i in 0..property_count as usize {
        let base = 8 + i * 8;
        let pid = u32::from_le_bytes([
            section[base],
            section[base + 1],
            section[base + 2],
            section[base + 3],
        ]);
        let offset = u32::from_le_bytes([
            section[base + 4],
            section[base + 5],
            section[base + 6],
            section[base + 7],
        ]);
        if (offset as usize) >= section_len {
            return Err(Hwp5Error::RecordParse { offset: 0,
                detail: format!(
                    "HwpSummaryInformation: property pid={pid} offset {offset:#x} past section_len {section_len}"
                ),
            });
        }
        if (offset as i64) <= last_offset {
            return Err(Hwp5Error::RecordParse { offset: 0,
                detail: format!(
                    "HwpSummaryInformation: property table not monotonic (pid={pid} offset={offset:#x} <= prev={last_offset:#x}) — cycle/DoS guard",
                ),
            });
        }
        last_offset = offset as i64;
        entries.push((pid, offset));
    }

    // ── Decode each property by VT type and route into Metadata ──
    let mut meta = Metadata::default();
    for (pid, offset) in &entries {
        let body = &section[*offset as usize..section_len];
        if body.len() < 8 {
            return Err(Hwp5Error::RecordParse {
                offset: 0,
                detail: format!("HwpSummaryInformation: pid={pid} body too short"),
            });
        }
        let vt = u32::from_le_bytes([body[0], body[1], body[2], body[3]]);
        match vt {
            VT_LPSTR => {
                let value = read_lpstr(&body[4..])?;
                route_string(&mut meta, *pid, value);
            }
            VT_LPWSTR => {
                let value = read_lpwstr(&body[4..])?;
                route_string(&mut meta, *pid, value);
            }
            VT_FILETIME => {
                if body.len() < 12 {
                    return Err(Hwp5Error::RecordParse {
                        offset: 0,
                        detail: format!("HwpSummaryInformation: pid={pid} FILETIME body too short"),
                    });
                }
                let ft = u64::from_le_bytes([
                    body[4], body[5], body[6], body[7], body[8], body[9], body[10], body[11],
                ]);
                let iso = filetime_to_iso8601(ft);
                route_datetime(&mut meta, *pid, iso);
            }
            // Other VT types (VT_I4 / VT_BSTR / VT_BLOB / …) — ignored
            // for Wave 12o scope. Future waves can extend.
            _ => {}
        }
    }

    Ok(meta)
}

// ── Variant payload readers ──────────────────────────────────────────

/// Reads `VT_LPSTR`: u32 length (in bytes, includes null terminator)
/// followed by `length` bytes of CP-949 / ASCII text.
fn read_lpstr(body: &[u8]) -> Hwp5Result<Option<String>> {
    if body.len() < 4 {
        return Err(Hwp5Error::RecordParse {
            offset: 0,
            detail: "VT_LPSTR: header truncated".into(),
        });
    }
    let len_bytes = u32::from_le_bytes([body[0], body[1], body[2], body[3]]) as usize;
    if len_bytes > MAX_PROPERTY_BYTES {
        return Err(Hwp5Error::RecordParse {
            offset: 0,
            detail: format!(
                "VT_LPSTR: length {len_bytes} exceeds cap {MAX_PROPERTY_BYTES} (DoS guard)"
            ),
        });
    }
    if 4 + len_bytes > body.len() {
        return Err(Hwp5Error::RecordParse {
            offset: 0,
            detail: format!("VT_LPSTR: length {len_bytes} overruns property body"),
        });
    }
    let raw = &body[4..4 + len_bytes];
    let trimmed = strip_trailing_nuls(raw);
    if trimmed.is_empty() {
        return Ok(None);
    }
    // PropertySet LPSTR is technically the file system code page — for
    // Hancom Korean documents this is ASCII or CP-949. Use lossy UTF-8
    // since most Hancom values are ASCII (version strings, account
    // names) and full code-page decoding is out of scope.
    Ok(Some(String::from_utf8_lossy(trimmed).into_owned()))
}

/// Reads `VT_LPWSTR`: u32 length (in code units, includes null
/// terminator) followed by `length * 2` bytes of UTF-16LE code units.
fn read_lpwstr(body: &[u8]) -> Hwp5Result<Option<String>> {
    if body.len() < 4 {
        return Err(Hwp5Error::RecordParse {
            offset: 0,
            detail: "VT_LPWSTR: header truncated".into(),
        });
    }
    let units = u32::from_le_bytes([body[0], body[1], body[2], body[3]]) as usize;
    let bytes_needed = units.checked_mul(2).ok_or_else(|| Hwp5Error::RecordParse {
        offset: 0,
        detail: "VT_LPWSTR: length overflow".into(),
    })?;
    if bytes_needed > MAX_PROPERTY_BYTES {
        return Err(Hwp5Error::RecordParse {
            offset: 0,
            detail: format!(
                "VT_LPWSTR: byte length {bytes_needed} exceeds cap {MAX_PROPERTY_BYTES} (DoS guard)"
            ),
        });
    }
    if 4 + bytes_needed > body.len() {
        return Err(Hwp5Error::RecordParse {
            offset: 0,
            detail: format!("VT_LPWSTR: length {units} ({bytes_needed} bytes) overruns body"),
        });
    }
    let raw = &body[4..4 + bytes_needed];
    let mut code_units: Vec<u16> =
        raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    // ── S7: strip UTF-16LE BOM if present ──
    if matches!(code_units.first(), Some(&0xfeff)) {
        code_units.remove(0);
    }
    // Trim trailing null code units (length field includes the terminator).
    while matches!(code_units.last(), Some(&0)) {
        code_units.pop();
    }
    if code_units.is_empty() {
        return Ok(None);
    }
    let s = String::from_utf16_lossy(&code_units);
    if s.is_empty() {
        Ok(None)
    } else {
        Ok(Some(s))
    }
}

fn strip_trailing_nuls(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1] == 0 {
        end -= 1;
    }
    &bytes[..end]
}

// ── FILETIME → ISO 8601 hand-roll (no chrono) ───────────────────────

/// Converts a Windows FILETIME (100-ns intervals since 1601-01-01 UTC)
/// into an ISO 8601 string truncated to seconds:
/// `"YYYY-MM-DDTHH:MM:SSZ"`. Sub-second precision is discarded
/// intentionally — Hancom's content.hpf wire only carries seconds
/// (Wave 12o §11.2 M4).
///
/// Returns `None` for FILETIME = 0 (meaning "unset") and for years
/// outside the ISO 8601 four-digit range (1..=9999).
pub(crate) fn filetime_to_iso8601(ft: u64) -> Option<String> {
    if ft == 0 {
        return None;
    }
    if ft < FILETIME_UNIX_DELTA {
        // Pre-1970 timestamps — Hancom never emits these. Reject for
        // safety (callers see `None` which surfaces as absent metadata).
        return None;
    }
    let unix_100ns = ft - FILETIME_UNIX_DELTA;
    let unix_secs: i64 = (unix_100ns / 10_000_000) as i64;

    // Convert Unix seconds → date+time using fundamental algorithm
    // (Howard Hinnant 'date.h' style — works for proleptic Gregorian
    // calendar without depending on `chrono`).
    let (year, month, day, hour, min, sec) = unix_seconds_to_ymd_hms(unix_secs)?;
    if !(1..=9999).contains(&year) {
        return None;
    }
    Some(format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z"))
}

/// Decomposes a Unix-epoch second count into UTC (Y, M, D, h, m, s).
fn unix_seconds_to_ymd_hms(secs: i64) -> Option<(i32, u32, u32, u32, u32, u32)> {
    let day_secs = 86_400_i64;
    let days = secs.div_euclid(day_secs);
    let time_of_day = secs.rem_euclid(day_secs) as u32;
    let (y, m, d) = days_to_ymd(days)?;
    let hour = time_of_day / 3600;
    let min = (time_of_day % 3600) / 60;
    let sec = time_of_day % 60;
    Some((y, m, d, hour, min, sec))
}

/// Civil date algorithm — turn Unix days into (year, month, day).
/// Reference: Hinnant H. (2013) "chrono-Compatible Low-Level Date
/// Algorithms".
fn days_to_ymd(days_from_unix: i64) -> Option<(i32, u32, u32)> {
    // Shift to algorithm's epoch (0000-03-01).
    let days_from_epoch = days_from_unix.checked_add(719_468)?;
    let era = if days_from_epoch >= 0 {
        days_from_epoch / 146_097
    } else {
        (days_from_epoch - 146_096) / 146_097
    };
    let doe = (days_from_epoch - era * 146_097) as u64; // [0, 146097)
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let mut y: i64 = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 366)
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { (mp + 3) as u32 } else { (mp - 9) as u32 };
    if m <= 2 {
        y += 1;
    }
    Some((y as i32, m, d))
}

// ── PID routing ─────────────────────────────────────────────────────

fn route_string(meta: &mut Metadata, pid: u32, value: Option<String>) {
    let Some(value) = value else { return };
    match pid {
        PID_TITLE => meta.title = Some(value),
        PID_SUBJECT => meta.subject = Some(value),
        PID_AUTHOR => meta.author = Some(value),
        PID_KEYWORDS => {
            meta.keywords =
                value.split(';').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        }
        PID_COMMENTS => meta.description = Some(value),
        PID_LAST_AUTHOR => meta.last_saved_by = Some(value),
        PID_HANCOM_DATE_DISPLAY => {
            meta.extras.insert("date".into(), value);
        }
        PID_HANCOM_APP_NAME => {
            meta.extras.insert("appname".into(), value);
        }
        other => {
            // Unknown PID: carry as extras with `pid_<hex>` key so the
            // wire bytes are preserved without colliding with future
            // typed promotions.
            meta.extras.insert(format!("pid_{other:#x}"), value);
        }
    }
}

fn route_datetime(meta: &mut Metadata, pid: u32, iso: Option<String>) {
    let Some(iso) = iso else { return };
    match pid {
        PID_CREATED_TIME => meta.created = Some(iso),
        PID_LAST_SAVED_TIME => meta.modified = Some(iso),
        other => {
            meta.extras.insert(format!("pid_{other:#x}_time"), iso);
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // FILETIME for 2026-06-04T11:20:00Z:
    //   unix seconds = 1780572000
    //   filetime = (1780572000 + 11644473600) * 10_000_000
    //          = 13_425_045_600 * 10_000_000
    //          = 134_250_456_000_000_000
    const FT_2026_06_04_T_11_20: u64 = 134_250_456_000_000_000;

    #[test]
    fn filetime_unset_yields_none() {
        assert_eq!(filetime_to_iso8601(0), None);
    }

    #[test]
    fn filetime_to_iso8601_known_value() {
        let iso = filetime_to_iso8601(FT_2026_06_04_T_11_20).expect("non-null filetime");
        assert_eq!(iso, "2026-06-04T11:20:00Z");
    }

    #[test]
    fn filetime_below_unix_epoch_rejected() {
        // 1969-12-31T23:59:59Z would be (-1) Unix seconds — pre-Unix.
        let ft = FILETIME_UNIX_DELTA - 10_000_000;
        assert_eq!(filetime_to_iso8601(ft), None);
    }

    #[test]
    fn empty_stream_returns_default_metadata() {
        let m = parse_summary_information(&[]).unwrap();
        assert_eq!(m, Metadata::default());
    }

    #[test]
    fn bad_bom_rejected() {
        let mut buf = vec![0u8; 0x30];
        buf[0] = 0xAA;
        buf[1] = 0xBB;
        let err = parse_summary_information(&buf).unwrap_err();
        assert!(format!("{err}").contains("BOM"));
    }

    /// Builds a minimal valid PropertySet stream carrying the supplied
    /// properties. Each property entry is `(pid, vt, payload)`.
    /// Payloads must already be in wire form (i.e. include LPWSTR length
    /// prefix, FILETIME 8-byte field, etc.).
    fn build_property_set(props: &[(u32, u32, Vec<u8>)]) -> Vec<u8> {
        // Header
        let mut buf = vec![];
        buf.extend_from_slice(&BOM_LE.to_le_bytes()); // BOM
        buf.extend_from_slice(&[0u8; 2]); // version
        buf.extend_from_slice(&[0u8; 4]); // OS / OS version
        buf.extend_from_slice(&[0u8; 16]); // class FMTID (dummy)
        buf.extend_from_slice(&1u32.to_le_bytes()); // section count
        buf.extend_from_slice(&[0u8; 16]); // section FMTID (dummy)
        buf.extend_from_slice(&0x30u32.to_le_bytes()); // section offset

        // Section
        let mut section = vec![];
        // Layout each property body laid out *after* the property table.
        let property_table_size = 8 + props.len() * 8;
        let mut bodies: Vec<(u32, Vec<u8>)> = Vec::new();
        let mut cursor = property_table_size;
        let mut entries: Vec<(u32, u32)> = Vec::new();
        for (pid, vt, payload) in props {
            entries.push((*pid, cursor as u32));
            let mut body = Vec::with_capacity(4 + payload.len());
            body.extend_from_slice(&vt.to_le_bytes());
            body.extend_from_slice(payload);
            cursor += body.len();
            bodies.push((*pid, body));
        }
        let section_len = cursor as u32;
        section.extend_from_slice(&section_len.to_le_bytes());
        section.extend_from_slice(&(props.len() as u32).to_le_bytes());
        for (pid, off) in &entries {
            section.extend_from_slice(&pid.to_le_bytes());
            section.extend_from_slice(&off.to_le_bytes());
        }
        for (_pid, body) in &bodies {
            section.extend_from_slice(body);
        }
        buf.extend_from_slice(&section);
        buf
    }

    fn lpwstr_payload(s: &str) -> Vec<u8> {
        // length (units, includes null) + UTF-16LE code units + null
        let mut units: Vec<u16> = s.encode_utf16().collect();
        units.push(0);
        let mut out = Vec::new();
        out.extend_from_slice(&(units.len() as u32).to_le_bytes());
        for u in units {
            out.extend_from_slice(&u.to_le_bytes());
        }
        out
    }

    fn filetime_payload(ft: u64) -> Vec<u8> {
        ft.to_le_bytes().to_vec()
    }

    #[test]
    fn lpwstr_title_decoded() {
        let bytes = build_property_set(&[(PID_TITLE, VT_LPWSTR, lpwstr_payload("Wave 12o 데모"))]);
        let m = parse_summary_information(&bytes).unwrap();
        assert_eq!(m.title.as_deref(), Some("Wave 12o 데모"));
    }

    #[test]
    fn typed_pids_route_correctly() {
        let bytes = build_property_set(&[
            (PID_TITLE, VT_LPWSTR, lpwstr_payload("Title")),
            (PID_SUBJECT, VT_LPWSTR, lpwstr_payload("Subject")),
            (PID_AUTHOR, VT_LPWSTR, lpwstr_payload("Author")),
            (PID_COMMENTS, VT_LPWSTR, lpwstr_payload("Description")),
            (PID_LAST_AUTHOR, VT_LPWSTR, lpwstr_payload("Editor")),
            (PID_CREATED_TIME, VT_FILETIME, filetime_payload(FT_2026_06_04_T_11_20)),
            (PID_LAST_SAVED_TIME, VT_FILETIME, filetime_payload(FT_2026_06_04_T_11_20)),
        ]);
        let m = parse_summary_information(&bytes).unwrap();
        assert_eq!(m.title.as_deref(), Some("Title"));
        assert_eq!(m.subject.as_deref(), Some("Subject"));
        assert_eq!(m.author.as_deref(), Some("Author"));
        assert_eq!(m.description.as_deref(), Some("Description"));
        assert_eq!(m.last_saved_by.as_deref(), Some("Editor"));
        assert_eq!(m.created.as_deref(), Some("2026-06-04T11:20:00Z"));
        assert_eq!(m.modified.as_deref(), Some("2026-06-04T11:20:00Z"));
    }

    #[test]
    fn keywords_semicolon_split() {
        let bytes =
            build_property_set(&[(PID_KEYWORDS, VT_LPWSTR, lpwstr_payload("alpha;beta;gamma"))]);
        let m = parse_summary_information(&bytes).unwrap();
        assert_eq!(m.keywords, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn unknown_pid_falls_back_to_extras_with_typed_key() {
        let bytes = build_property_set(&[(0xABCD, VT_LPWSTR, lpwstr_payload("hancom-specific"))]);
        let m = parse_summary_information(&bytes).unwrap();
        assert_eq!(m.extras.get("pid_0xabcd").map(String::as_str), Some("hancom-specific"));
    }

    #[test]
    fn hancom_date_display_promotes_to_extras_date() {
        let bytes = build_property_set(&[(
            PID_HANCOM_DATE_DISPLAY,
            VT_LPWSTR,
            lpwstr_payload("2026년 6월 4일"),
        )]);
        let m = parse_summary_information(&bytes).unwrap();
        assert_eq!(m.extras.get("date").map(String::as_str), Some("2026년 6월 4일"));
    }

    // ── Security gates ────────────────────────────────────────────

    #[test]
    fn non_monotonic_offset_rejected() {
        // Hand-craft a PropertySet where property table offsets are
        // duplicated — the parser must reject this to prevent
        // re-entrancy attacks on the property body.
        let mut buf = vec![];
        buf.extend_from_slice(&BOM_LE.to_le_bytes());
        buf.extend_from_slice(&[0u8; 2]);
        buf.extend_from_slice(&[0u8; 4]);
        buf.extend_from_slice(&[0u8; 16]);
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&[0u8; 16]);
        buf.extend_from_slice(&0x30u32.to_le_bytes());

        // Section: two entries pointing at the same offset.
        let mut section = vec![];
        section.extend_from_slice(&80u32.to_le_bytes()); // section_len
        section.extend_from_slice(&2u32.to_le_bytes()); // property_count
        section.extend_from_slice(&PID_TITLE.to_le_bytes());
        section.extend_from_slice(&0x20u32.to_le_bytes()); // offset
        section.extend_from_slice(&PID_SUBJECT.to_le_bytes());
        section.extend_from_slice(&0x20u32.to_le_bytes()); // same offset!
                                                           // Pad so the section_len is honoured.
        section.resize(80, 0);
        buf.extend_from_slice(&section);

        let err = parse_summary_information(&buf).unwrap_err();
        assert!(format!("{err}").to_lowercase().contains("monotonic"));
    }

    #[test]
    fn property_count_cap_enforced() {
        // section_len just needs to be plausible; property_count past cap.
        let mut buf = vec![];
        buf.extend_from_slice(&BOM_LE.to_le_bytes());
        buf.extend_from_slice(&[0u8; 2]);
        buf.extend_from_slice(&[0u8; 4]);
        buf.extend_from_slice(&[0u8; 16]);
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&[0u8; 16]);
        buf.extend_from_slice(&0x30u32.to_le_bytes());

        let mut section = vec![];
        section.extend_from_slice(&16u32.to_le_bytes()); // section_len
        section.extend_from_slice(&(MAX_PROPERTY_COUNT + 1).to_le_bytes()); // too many
        section.resize(16, 0);
        buf.extend_from_slice(&section);

        let err = parse_summary_information(&buf).unwrap_err();
        assert!(format!("{err}").contains("property_count"));
    }

    #[test]
    fn lpwstr_payload_cap_enforced() {
        // length field claims more than MAX_PROPERTY_BYTES.
        let huge_units = (MAX_PROPERTY_BYTES / 2 + 1) as u32;
        let mut payload = vec![];
        payload.extend_from_slice(&huge_units.to_le_bytes());
        // (don't bother actually writing the bytes — parser must reject
        //  before allocating)
        let bytes = build_property_set(&[(PID_TITLE, VT_LPWSTR, payload)]);
        let err = parse_summary_information(&bytes).unwrap_err();
        let msg = format!("{err}").to_lowercase();
        assert!(msg.contains("cap") || msg.contains("overrun"));
    }

    /// S7 — inline UTF-16LE BOM (0xfeff) at the start of a VT_LPWSTR
    /// payload must be stripped, not surfaced in the metadata value.
    #[test]
    fn s7_utf16_bom_stripped_from_lpwstr() {
        let inner: Vec<u16> = std::iter::once(0xfeffu16) // inline BOM
            .chain("Title".encode_utf16())
            .chain(std::iter::once(0)) // null terminator
            .collect();
        let mut payload = vec![];
        payload.extend_from_slice(&(inner.len() as u32).to_le_bytes());
        for u in inner {
            payload.extend_from_slice(&u.to_le_bytes());
        }
        let bytes = build_property_set(&[(PID_TITLE, VT_LPWSTR, payload)]);
        let m = parse_summary_information(&bytes).unwrap();
        assert_eq!(m.title.as_deref(), Some("Title"));
    }
}
