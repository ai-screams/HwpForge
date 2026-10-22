//! One metadata-only classification for raw and effective names. No resolver
//! applies these transformations to a lookup or to an output name.
use std::collections::BTreeMap;
use std::io::Cursor;

use zip::ZipArchive;

use super::{invalid_structure, normalize_part_name, zip_error, PackageEntry};
use crate::CoreResult;

// T only removes prefixes/suffixes or replaces backslashes. Thus T(n) == n
// exactly when none of these operations applies; constructing a second path
// is unnecessary. ASCII folding below is K because every name is T-invariant.
fn invariant(name: &[u8]) -> bool {
    // The Unicode reference indexes WCHARs after MultiByteToWideChar, not scalars.
    !name.contains(&0)
        && String::from_utf8_lossy(name).encode_utf16().nth(1) != Some(u16::from(b':'))
        && !name.starts_with(b"/")
        && !name.contains(&b'\\')
        && !name.windows(4).any(|part| part == b"/../")
}

pub(super) fn classify(
    bytes: &[u8],
    archive: &ZipArchive<Cursor<&[u8]>>,
    entries: &[PackageEntry],
) -> CoreResult<Vec<bool>> {
    let metadata = archive.metadata();
    let mut names = Vec::with_capacity(entries.len());
    let mut opaque = vec![false; entries.len()];
    let mut stripped = BTreeMap::<&[u8], usize>::new();
    for (index, entry) in entries.iter().enumerate() {
        let meta = metadata.entry(index).map_err(|e| zip_error(e.to_string()))?;
        // The selected CD was already framed and checked by directory::open.
        // Join by the metadata's physical central-header offset, never by an
        // assumed equality of the zip iterator and physical record order.
        let start = meta.central_header_start() as usize;
        let len = u16::from_le_bytes(bytes[start + 28..start + 30].try_into().unwrap()) as usize;
        let raw = &bytes[start + 46..start + 46 + len];
        let effective = meta.name_raw();
        for name in [raw, effective] {
            if !invariant(name) {
                return Err(invalid_structure(format!(
                    "invalid package part name '{}'",
                    String::from_utf8_lossy(name)
                )));
            }
            opaque[index] |= name.ends_with(b"/") && entry.size != 0;
            let text = String::from_utf8_lossy(name);
            opaque[index] |= normalize_part_name(text.strip_suffix('/').unwrap_or(&text)).is_err();
        }
        // Keep the old decoded-name slash collision boundary, without
        // extending it to otherwise valid raw/effective aliases.
        let key = entry.path.strip_suffix('/').unwrap_or(&entry.path).as_bytes();
        if let Some(other) = stripped.insert(key, index) {
            opaque[index] = true;
            opaque[other] = true;
        }
        names.push(raw);
    }
    let mut keys = Vec::with_capacity(2 * entries.len());
    for (index, raw) in names.iter().enumerate() {
        // Borrow on demand rather than retaining a metadata handle per entry.
        let meta = metadata.entry(index).map_err(|e| zip_error(e.to_string()))?;
        let pair = [*raw, meta.name_raw()];
        // Writers retain the effective name only. Relaxation is lossless only
        // when it is also the raw name, including late slash-collision flags.
        if opaque[index] && pair[0] != pair[1] {
            return Err(invalid_structure("opaque ZIP entry has different raw/effective names"));
        }
        let distinct = if pair[0] == pair[1] { &pair[..1] } else { &pair[..] };
        for name in distinct {
            // Newly relaxed names need portable evidence. Existing unique
            // non-ASCII names keep their original encoding contract.
            if opaque[index] && (!name.is_ascii() || name.len() >= 260) {
                return Err(invalid_structure(
                    "opaque ZIP name exceeds portable reference evidence",
                ));
            }
            keys.push((name.to_ascii_lowercase(), index));
        }
    }
    #[cfg(test)]
    WORK.set((entries.len(), keys.len(), 0));
    #[cfg(not(test))]
    keys.sort_unstable();
    #[cfg(test)]
    keys.sort_unstable_by(|left, right| {
        #[cfg(test)]
        WORK.with(|work| {
            let (records, keys, compared) = work.get();
            let equal = left.0.iter().zip(&right.0).take_while(|(a, b)| a == b).count();
            work.set((records, keys, compared + (equal + 1).min(left.0.len().min(right.0.len()))));
        });
        left.cmp(right)
    });
    for pair in keys.windows(2) {
        let (left, a) = &pair[0];
        let (right, b) = &pair[1];
        if left == right && a != b && (opaque[*a] || opaque[*b]) {
            return Err(invalid_structure("opaque name collision in ZIP package"));
        }
    }
    Ok(opaque)
}

// Test-only work accounting: physical records, K keys, compared name bytes.
#[cfg(test)]
thread_local! {
    pub(super) static WORK: std::cell::Cell<(usize,usize,usize)> = const { std::cell::Cell::new((0,0,0)) };
}
