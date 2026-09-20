//! The only file I/O in the operation layer.
//!
//! Every other function in [`crate::ops`] works on bytes the caller already
//! holds. Two steps here reach past that boundary to the filesystem, kept in
//! this one module so that a frontend without a filesystem — the Python
//! bindings fed from memory, an object store, an FFI peer — can replace
//! either and keep the rest of the pipeline untouched:
//!
//! - [`resolve_files_from_dir`] — asset (image) resolution: reads the `file:`
//!   entries of a plan a caller already made.
//! - [`read_bounded`] — W6b audit follow-up: reads a whole input document (or
//!   a stamp map / patch JSON file) from a path, capped at a caller-chosen
//!   size regardless of what the source's `metadata()` reports. CLI's
//!   `error::read_input`, MCP's `read_file_bytes` and the Python bindings'
//!   `Document.open` all read their input through one of this function or
//!   [`MAX_FILE_SIZE`], so the limit and its off-by-one behaviour cannot
//!   drift between frontends.
//!
//! The asset pipeline is three steps, and only the middle one is impure:
//!
//! 1. `collect_asset_plan` (pure) — what the document references.
//! 2. [`resolve_files_from_dir`] (**this module**) — read the files a plan
//!    asks for, with `base_dir` containment enforced by canonicalisation.
//! 3. `finish_assets` (pure) — embed the bytes and report the outcomes.

use std::io::Read;
use std::path::Path;

use hwpforge_foundation::diagnostics::OpsCode;

use super::OpsError;

/// Reads the `file:` entries of an asset plan, confined to `base_dir`.
///
/// Re-exported from `hwpforge_smithy_md::assets::fs` so that callers reach
/// the resolver through the operation layer they already use. `data:` and
/// remote entries are left untouched — this step never opens a network
/// connection.
pub use hwpforge_smithy_md::assets::fs::resolve_files_from_dir;

/// The frontend-shared input size limit: 100 MB.
///
/// CLI's `MAX_FILE_SIZE`, MCP's `MAX_FILE_SIZE` and the Python bindings'
/// `_hwpforge.MAX_FILE_SIZE` all name this constant (directly or through a
/// `pub use`), so raising or lowering the limit changes every frontend at
/// once instead of drifting between three copies of the same literal.
pub const MAX_FILE_SIZE: u64 = 100 * 1024 * 1024;

/// Reads `path` fully, refusing to read past `max` bytes.
///
/// Capping the read itself, not just a `metadata().len()` pre-check a caller
/// might run first, is the actual guarantee: a FIFO or process substitution
/// reports `0` from `metadata()` and would otherwise be read to completion by
/// a plain [`std::fs::read`] (measured: an unbounded read of a 200 MB FIFO
/// drove RSS to 2.8 GB in 20 s).
///
/// # Errors
///
/// [`OpsError::Io`] when opening or reading `path` fails — match on the
/// wrapped [`std::io::Error`]'s [`std::io::ErrorKind`] to special-case a
/// missing file, the way a caller already did before this reader was
/// shared. [`OpsError::Rejected`] (code [`OpsCode::InputTooLarge`]) when the
/// read exceeds `max`.
pub fn read_bounded(path: impl AsRef<Path>, max: u64) -> Result<Vec<u8>, OpsError> {
    let file = std::fs::File::open(path.as_ref()).map_err(OpsError::Io)?;
    let mut buf = Vec::new();
    // `saturating_add`: a caller passing `u64::MAX` must still get an
    // unbounded-but-safe read, not an overflow panic or a wrapped zero cap.
    file.take(max.saturating_add(1)).read_to_end(&mut buf).map_err(OpsError::Io)?;
    if buf.len() as u64 > max {
        return Err(OpsError::Rejected {
            code: OpsCode::InputTooLarge,
            reason: format!("input exceeds {} MB limit", max / 1024 / 1024),
        });
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempfile(tag: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hwpforge-ops-fs-{tag}-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("t").replace("::", "-"),
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("input");
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn within_limit_returns_all_bytes() {
        let path = tempfile("within", &[7u8; 5]);
        assert_eq!(read_bounded(&path, 10).unwrap(), vec![7u8; 5]);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn at_exact_limit_returns_all_bytes() {
        let path = tempfile("exact", &[7u8; 10]);
        assert_eq!(read_bounded(&path, 10).unwrap(), vec![7u8; 10]);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn over_limit_is_rejected_as_input_too_large() {
        // 11 bytes through a 10-byte cap: `take(max + 1)` lets the 11th byte
        // through so the length check can distinguish "exactly at the limit"
        // from "over it" — this exercises that off-by-one boundary.
        let path = tempfile("over", &[7u8; 11]);
        let err = read_bounded(&path, 10).unwrap_err();
        assert_eq!(err.code(), OpsCode::InputTooLarge);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_u64_max_bound_does_not_overflow() {
        // Review finding: `max + 1` overflowed for `u64::MAX` (panic with
        // overflow checks, wrapped to a zero cap in release). The bound must
        // saturate so the read simply behaves as unbounded.
        let path = tempfile("umax", &[7u8; 5]);
        assert_eq!(read_bounded(&path, u64::MAX).unwrap(), vec![7u8; 5]);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn missing_file_is_io_not_found() {
        let err = read_bounded(Path::new("/nonexistent/hwpforge-ops-fs-probe.hwpx"), MAX_FILE_SIZE)
            .unwrap_err();
        let OpsError::Io(io_err) = err else { panic!("expected OpsError::Io, got {err:?}") };
        assert_eq!(io_err.kind(), std::io::ErrorKind::NotFound);
    }
}
