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
    fn documents(shapes: bool) -> (Hwp5SemanticDocument, Vec<u8>, HwpxDocument) {
        use hwpforge_core::section::MasterPage;
        use hwpforge_core::table::{Table, TableCell, TableRow};
        use hwpforge_core::{
            Control, Document, Image, ImageFormat, ImageStore, PageSettings, Paragraph, Run,
        };
        use hwpforge_foundation::{ApplyPageType, CharShapeIndex, HwpUnit, ParaShapeIndex};
        use hwpforge_smithy_hwp5::*;
        let u = |v| HwpUnit::new(v).unwrap();
        let p = |text: &str| {
            Paragraph::with_runs(
                vec![Run::text(text, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )
        };
        let mut section = Section::with_paragraphs(vec![p("body")], PageSettings::a4());
        let mut odd = p("odd");
        if shapes {
            odd.runs.push(Run::image(
                Image::new("", u(1000), u(1000), ImageFormat::Png),
                CharShapeIndex::new(0),
            ));
            odd.runs.push(Run::control(
                Control::rect(u(1000), u(1000)).unwrap(),
                CharShapeIndex::new(0),
            ));
        }
        odd.runs.push(Run::table(
            Table::new(vec![TableRow::new(vec![TableCell::new(vec![p("cell")], u(1000))])]),
            CharShapeIndex::new(0),
        ));
        section.master_pages = Some(vec![
            MasterPage::new(ApplyPageType::Odd, vec![odd]),
            MasterPage::new(ApplyPageType::Even, vec![p("even")]),
        ]);
        for master in section.master_pages.as_mut().unwrap() {
            master.text_width = u(10000);
            master.text_height = u(20000);
        }
        let mut semantic = Hwp5SemanticDocument::new(Hwp5SemanticPackageMeta {
            version: "5.0.2.5".into(),
            compressed: false,
            package_entries: vec![],
            bin_data_records: vec![],
            bin_data_streams: vec![],
        });
        let mut sem = Hwp5SemanticSection {
            section_id: Hwp5SemanticSectionId::new(0),
            index: 0,
            page_def: None,
            paragraphs: vec![],
            controls: vec![],
            master_pages: section
                .master_pages
                .as_ref()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(index, m)| Hwp5SemanticMasterPage {
                    index,
                    extended: false,
                    record_index: index,
                    raw: vec![],
                    scope: Some(m.into()),
                })
                .collect(),
        };
        for (i, (text, path)) in [
            ("body", vec![Hwp5SemanticContainerKind::Body]),
            ("odd", vec![Hwp5SemanticContainerKind::MasterPageSubList(0)]),
            (
                "cell",
                vec![
                    Hwp5SemanticContainerKind::MasterPageSubList(0),
                    Hwp5SemanticContainerKind::TableCellSubList,
                ],
            ),
            ("even", vec![Hwp5SemanticContainerKind::MasterPageSubList(1)]),
        ]
        .into_iter()
        .enumerate()
        {
            sem.paragraphs.push(Hwp5SemanticParagraph {
                paragraph_id: Hwp5SemanticParagraphId::new(i),
                paragraph_index: i,
                container: Hwp5SemanticContainerPath::new(path),
                owner_control_id: None,
                inline_items: vec![],
                text: text.into(),
                style_id: Some(0),
                char_shape_run_count: 1,
                control_ids: vec![],
            });
        }
        let mut kinds = vec![Hwp5SemanticControlKind::Table];
        if shapes {
            kinds.extend([Hwp5SemanticControlKind::Image, Hwp5SemanticControlKind::Rect]);
        }
        for (i, kind) in kinds.into_iter().enumerate() {
            sem.controls.push(Hwp5SemanticControlNode {
                node_id: Hwp5SemanticControlId::new(i),
                kind,
                payload: Hwp5SemanticControlPayload::None,
                container: Hwp5SemanticContainerPath::new(vec![
                    Hwp5SemanticContainerKind::MasterPageSubList(0),
                ]),
                literal_ctrl_id: None,
                anchor_paragraph_id: Some(Hwp5SemanticParagraphId::new(1)),
                confidence: Hwp5SemanticConfidence::High,
                notes: vec![],
            });
        }
        semantic.sections.push(sem);
        let mut doc = Document::new();
        doc.add_section(section);
        let bytes = hwpforge_smithy_hwpx::HwpxEncoder::encode(
            &doc.validate().unwrap(),
            &hwpforge_smithy_hwpx::style_store_for_preset("default").unwrap(),
            &ImageStore::new(),
        )
        .unwrap();
        let decoded = hwpforge_smithy_hwpx::HwpxDecoder::decode(&bytes).unwrap();
        (semantic, bytes, decoded)
    }

    // 이것을 실패시키는 것: 한 번 분류할 때 master 항목을 Body 슬롯으로 넣거나 누락하기.
    #[test]
    // 이것을 실패시키는 것: semantic master scope 누락 (실제 음성 대조 실행).
    fn master_scope_partition_keeps_body_and_each_master_evidence() {
        for shapes in [false, true] {
            let (semantic, bytes, decoded) = documents(shapes);
            assert_eq!(from_hwp5(&semantic), from_hwpx(&decoded, &bytes).unwrap());
            for section in &semantic.sections {
                let scopes = super::super::deep_counts::summarize_hwp5_semantic_scopes(section);
                assert_eq!(scopes.len(), section.master_pages.len() + 1);
                assert_eq!(
                    scopes.iter().map(|s| s.deep_paragraphs).sum::<usize>(),
                    section.paragraphs.len()
                );
                assert!(scopes.iter().skip(1).all(|s| s.paragraphs > 0));
                if shapes {
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
    // 이것을 실패시키는 것: semantic master scope 누락 (실제 음성 대조 실행).
    fn master_audit_distinguishes_scope_from_total_counts() {
        let (semantic, bytes, mut output) = documents(false);
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
