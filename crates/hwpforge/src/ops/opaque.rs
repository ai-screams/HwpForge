//! Final opaque-entry preservation check for save operations.

use super::{HwpxError, OpsError};

/// Final save gate for every operation that produces HWPX from an input package.
/// Preservation stays in the codecs; this checks their actual output, including
/// opaque directory records. Ordinary inputs never cause the output to be opened.
pub(super) fn ensure_opaque_carried(input: &[u8], output: &[u8]) -> Result<(), OpsError> {
    use hwpforge_core::zip_package::open_package;
    use std::collections::BTreeMap;

    #[cfg(test)]
    opaque_preservation_tests::GATE_CALLS.with(|calls| calls.set(calls.get() + 1));

    let mut source =
        open_package(input, Default::default()).map_err(|error| OpsError::encode(error.into()))?;
    if !(0..source.entries().len()).any(|index| source.is_opaque(index)) {
        return Ok(());
    }
    let mut saved =
        open_package(output, Default::default()).map_err(|error| OpsError::encode(error.into()))?;
    let saved_indexes: BTreeMap<_, _> = saved
        .entries()
        .iter()
        .enumerate()
        .map(|(index, entry)| (entry.path.clone(), index))
        .collect();
    for index in 0..source.entries().len() {
        if !source.is_opaque(index) {
            continue;
        }
        let name = &source.entries()[index].path;
        let refusal = || {
            OpsError::encode(HwpxError::InvalidStructure {
                detail: format!(
                    "opaque entry '{name}' was not preserved (name, bytes or classification)"
                ),
            })
        };
        let saved_index = *saved_indexes.get(name).ok_or_else(refusal)?;
        if saved.is_opaque(saved_index) != source.is_opaque(index) {
            return Err(refusal());
        }
        let original = source.read(index).map_err(|error| OpsError::encode(error.into()))?;
        let carried = saved.read(saved_index).map_err(|error| OpsError::encode(error.into()))?;
        if original != carried {
            return Err(OpsError::encode(HwpxError::InvalidStructure {
                detail: format!("opaque entry '{}' payload changed", source.entries()[index].path),
            }));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "opaque_preservation_tests.rs"]
mod opaque_preservation_tests;
