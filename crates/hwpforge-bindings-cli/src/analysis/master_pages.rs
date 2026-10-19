//! Scope-aware master-page evidence. Ownership comes from the existing decoders;
//! this analysis never assigns parts to sections by their ZIP file number.
use std::collections::BTreeMap;

use hwpforge_core::section::Section;
use hwpforge_smithy_hwp5::{Hwp5SemanticDocument, Hwp5SemanticMasterPageScope};
use hwpforge_smithy_hwpx::{HwpxDocument, HwpxResult, PackageReader};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct MasterPageEvidence {
    pub section_index: usize,
    pub index: usize,
    pub scope: Option<Hwp5SemanticMasterPageScope>,
    pub paragraphs: usize,
    pub deep_paragraphs: usize,
    pub objects: BTreeMap<String, usize>,
}

pub(crate) fn from_hwp5(document: &Hwp5SemanticDocument) -> Vec<MasterPageEvidence> {
    document
        .sections
        .iter()
        .flat_map(|section| {
            let summaries = super::deep_counts::summarize_hwp5_semantic_scopes(section);
            section.master_pages.iter().zip(summaries.into_iter().skip(1)).map(
                |(master, summary)| {
                    let objects = objects(&summary);
                    MasterPageEvidence {
                        section_index: section.index,
                        index: master.index,
                        scope: master.scope.clone(),
                        paragraphs: summary.paragraphs,
                        deep_paragraphs: summary.deep_paragraphs,
                        objects,
                    }
                },
            )
        })
        .collect()
}

pub(crate) fn from_hwpx(
    document: &HwpxDocument,
    bytes: &[u8],
) -> HwpxResult<Vec<MasterPageEvidence>> {
    let mut package = PackageReader::new(bytes)?;
    let inventory = super::hwpx_paths::collect_master_path_inventory(&mut package)?;
    Ok(document
        .document
        .sections()
        .iter()
        .enumerate()
        .flat_map(|(section_index, section)| {
            let inventory = &inventory;
            section.master_pages.iter().flatten().enumerate().map(move |(index, master)| {
                let mut host = Section::new(section.page_settings);
                host.paragraphs = master.paragraphs.clone();
                let occurrences: Vec<_> = inventory
                    .iter()
                    .filter(|p| p.section_index == section_index && p.index == index)
                    .flat_map(|p| &p.occurrences)
                    .collect();
                let summary =
                    super::deep_counts::summarize_hwpx_section(section_index, &host, &occurrences);
                MasterPageEvidence {
                    section_index,
                    index,
                    scope: Some(master.into()),
                    paragraphs: summary.paragraphs,
                    deep_paragraphs: summary.deep_paragraphs,
                    objects: objects(&summary),
                }
            })
        })
        .collect())
}

fn objects(summary: &super::deep_counts::DeepSectionSummary) -> BTreeMap<String, usize> {
    [
        ("table", summary.tables),
        ("image", summary.images),
        ("text_box", summary.text_boxes),
        ("rect", summary.rectangles),
        ("line", summary.lines),
        ("polygon", summary.polygons),
        ("ole", summary.ole_objects),
    ]
    .into_iter()
    .filter(|(_, count)| *count > 0)
    .map(|(name, count)| (name.into(), count))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    // 이것을 실패시키는 것: 한 번 분류할 때 master 항목을 Body 슬롯으로 넣거나 누락하기.
    #[test]
    fn master_scope_partition_keeps_body_and_each_master_evidence() {
        for name in ["f5c-odd-even", "f5f-shapes"] {
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/master_pages");
            let source = std::fs::read(root.join(format!("{name}.hwp"))).unwrap();
            let semantic = hwpforge_smithy_hwp5::build_hwp5_semantic(&source).unwrap();
            let (bytes, _) = hwpforge_convert::hwp5_to_hwpx_bytes(&source).unwrap();
            let decoded = hwpforge_smithy_hwpx::HwpxDecoder::decode(&bytes).unwrap();
            assert_eq!(from_hwp5(&semantic), from_hwpx(&decoded, &bytes).unwrap());
            for section in &semantic.sections {
                let scopes = super::super::deep_counts::summarize_hwp5_semantic_scopes(section);
                assert_eq!(scopes.len(), section.master_pages.len() + 1);
                assert_eq!(
                    scopes.iter().map(|s| s.deep_paragraphs).sum::<usize>(),
                    section.paragraphs.len()
                );
                assert!(scopes.iter().skip(1).all(|s| s.paragraphs > 0));
                if name == "f5f-shapes" {
                    assert_eq!(scopes[0].images, 0);
                    assert_eq!(scopes[1].images, 1);
                    assert_eq!(scopes[0].rectangles, 0);
                    assert_eq!(scopes[1].rectangles, 1);
                }
            }
        }
    }

    // 이것을 실패시키는 것: semantic master를 Body로 집계 / 출력의 master 범위를 비교에서 누락.
    #[test]
    fn master_audit_distinguishes_scope_from_total_counts() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/master_pages/f5c-odd-even.hwp");
        let source = std::fs::read(path).unwrap();
        let semantic = hwpforge_smithy_hwp5::build_hwp5_semantic(&source).unwrap();
        let (bytes, _) = hwpforge_convert::hwp5_to_hwpx_bytes(&source).unwrap();
        let mut output = hwpforge_smithy_hwpx::HwpxDecoder::decode(&bytes).unwrap();
        let expected = from_hwp5(&semantic);
        assert_eq!(expected, from_hwpx(&output, &bytes).unwrap());
        let body = super::super::deep_counts::summarize_hwp5_semantic(&semantic);
        assert_eq!(body.sections[0].paragraphs, output.document.sections()[0].paragraphs.len());
        output.document.sections_mut()[0].master_pages.as_mut().unwrap().swap(0, 1);
        assert_ne!(expected, from_hwpx(&output, &bytes).unwrap());
        output.document.sections_mut()[0].master_pages = None;
        assert_ne!(expected, from_hwpx(&output, &bytes).unwrap());
    }
}
