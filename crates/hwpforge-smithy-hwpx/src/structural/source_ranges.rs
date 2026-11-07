//! Check that the planned replacements were applied exactly, including untouched gaps.
//! The replacement plan is shared with the writer, not independently derived here.
//! `self_verify` and the existing structural tests check the plan's correctness.
use super::{paragraph_replacements, verification_error, HwpxDocument, StructuralEditError};
use hwpforge_core::zip_package::{open_package, Package};
use std::ops::Range;

#[cfg(test)]
thread_local! {
    static XML_READS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub(super) struct SectionChange {
    pub section: usize,
    pub path: String,
    pub original: String,
    pub deleted: Vec<usize>,
    pub inserted: Option<(usize, usize, String)>,
}

pub(super) fn verify_output(
    bytes: &[u8],
    decoded: &HwpxDocument,
    changes: &[SectionChange],
) -> Result<(), StructuralEditError> {
    let mut output = open_package(bytes, Default::default())
        .map_err(crate::package_path::package_error)
        .map_err(verification_error)?;
    let mut removed = super::references::Removed::default();
    for change in changes {
        let xml = &change.original;
        let index = output
            .entries()
            .iter()
            .position(|entry| entry.path == change.path)
            .ok_or_else(mismatch)?;
        let final_xml = read_xml(&mut output, index)?;
        let map = &decoded.source_maps[change.section];
        let allowed = change.replacements(map)?;
        verify_section(xml, &final_xml, allowed)?;
        for &index in &change.deleted {
            removed.record(&xml[map.paragraphs[index].outer.clone()])?;
        }
    }
    // Existing conservative preflight remains first, preserving its diagnostics.
    if let Some(change) = changes.iter().find(|change| !change.deleted.is_empty()) {
        if !removed.surviving_references_are_safe(&mut output)? {
            return Err(StructuralEditError::ReferenceStranded {
                section: change.section,
                index: change.deleted[0],
            });
        }
    }
    Ok(())
}

impl SectionChange {
    pub(super) fn replacements(
        &self,
        map: &crate::decoder::source_map::SourceMap,
    ) -> Result<Vec<(Range<usize>, String)>, StructuralEditError> {
        let inserted = self.inserted.as_ref().map(|(at, count, text)| (*at, *count, text.as_str()));
        paragraph_replacements(map, &self.deleted, inserted)
    }
}

pub(super) fn read_xml(
    package: &mut Package<'_>,
    index: usize,
) -> Result<String, StructuralEditError> {
    let path = package.entries()[index].path.clone();
    // 이것을 실패시키는 것: 보존 전용 opaque XML을 참조 검사 입력으로 승인.
    if package.is_opaque(index) {
        return Err(verification_error(crate::HwpxError::InvalidStructure {
            detail: format!("opaque ZIP entry selected semantically: '{path}'"),
        }));
    }
    #[cfg(test)]
    XML_READS.with(|reads| reads.borrow_mut().push(path.clone()));
    let bytes = package
        .read(index)
        .map_err(crate::package_path::package_error)
        .map_err(verification_error)?;
    crate::package_xml::text_entry(&path, bytes).map_err(verification_error)
}

fn mismatch() -> StructuralEditError {
    verification_error(crate::HwpxError::InvalidStructure {
        detail: "section bytes differ outside the declared structural replacements".into(),
    })
}

/// Compare untouched gaps and the exact authorized replacements at final offsets.
/// This does not trust offsets or lengths supplied by the writer.
fn verify_section(
    original: &str,
    output: &str,
    mut allowed: Vec<(Range<usize>, String)>,
) -> Result<(), StructuralEditError> {
    allowed.sort_unstable_by_key(|(range, _)| (range.start, range.end));
    let (mut source_at, mut output_at) = (0, 0);
    for (range, replacement) in allowed {
        let gap = original.get(source_at..range.start).ok_or_else(mismatch)?;
        for expected in [gap, replacement.as_str()] {
            // 이것을 실패시키는 것: 허용 구간 밖 같은 길이 변조 또는 추가 치환 허용.
            if output.as_bytes().get(output_at..output_at + expected.len())
                != Some(expected.as_bytes())
            {
                return Err(mismatch());
            }
            output_at += expected.len();
        }
        source_at = range.end;
    }
    // 이것을 실패시키는 것: 마지막 허용 구간 뒤의 변조/추가 바이트 무시.
    if original.get(source_at..) != output.get(output_at..) {
        return Err(mismatch());
    }
    Ok(())
}

#[cfg(test)]
#[path = "source_ranges_tests.rs"]
mod tests;
