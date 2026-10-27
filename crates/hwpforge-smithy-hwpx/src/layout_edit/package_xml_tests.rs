//! Synthetic package XML encoding × exclusion matrix. No corpus fixtures.
use super::*;
use crate::{EncodeOptions, HwpxDecoder, HwpxEncoder, HwpxPatcher};
use hwpforge_core::{
    image::ImageStore,
    run::{Run, RunContent},
    Document, PageSettings, Paragraph, Section,
};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
use std::io::{Cursor, Write};

fn base() -> Vec<u8> {
    let mut doc = Document::new();
    for text in ["alpha", "beta"] {
        doc.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::text(text, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        ));
    }
    HwpxEncoder::encode(
        &doc.validate().unwrap(),
        &crate::style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
    )
    .unwrap()
}

fn rewrite(bytes: &[u8], path: &str, value: &[u8], case: &str) -> Vec<u8> {
    let case = format!("{case}, rewrite part={path}");
    let pkg = RawPackage::read(bytes).unwrap_or_else(|error| panic!("{case}: {error}"));
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for name in pkg.entry_paths() {
        let case = format!("{case}, entry={name}");
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap_or_else(|error| panic!("{case}: {error}"));
        zip.write_all(if name == path {
            value
        } else {
            pkg.read_entry(name).unwrap_or_else(|error| panic!("{case}: {error}"))
        })
        .unwrap_or_else(|error| panic!("{case}: {error}"));
    }
    zip.finish().unwrap_or_else(|error| panic!("{case}: {error}")).into_inner()
}

fn encoded(xml: &str, encoding: usize) -> Vec<u8> {
    if encoding == 0 {
        return xml.as_bytes().to_vec();
    }
    let xml = xml.replace("UTF-8", "UTF-16");
    crate::package_xml::utf16(&xml, encoding == 1, true)
}

// 이것을 실패시키는 것: 세 byte→XML 소비처 중 하나를 UTF-8 전용으로 복원,
// container/content 전체 Node 누락, 순서 번호 fallback, 미수정 XML 재인코딩,
// canonical mimetype 접미부 거부(합성 20도 전체 decode/edit 경로로 통과).
#[test]
fn package_xml_cross_matrix() {
    let case = "package_xml_cross_matrix setup".to_owned();
    let original = rewrite(&base(), "mimetype", b"application/hwp+zip;x=1", &case);
    let pkg = RawPackage::read(&original).unwrap_or_else(|error| panic!("{case}: {error}"));
    let content = pkg
        .read_text_entry("Contents/content.hpf")
        .unwrap_or_else(|error| panic!("{case}: {error}"));
    let (head, spine) =
        content.split_once("<opf:spine>").unwrap_or_else(|| panic!("{case}: missing spine"));
    let content = format!(
        "{head}<opf:spine>{}",
        spine
            .replace("section0", "SWAP")
            .replace("section1", "section0")
            .replace("SWAP", "section1")
    );
    let original = rewrite(&original, "Contents/content.hpf", content.as_bytes(), &case);
    let container = pkg
        .read_text_entry("META-INF/container.xml")
        .unwrap_or_else(|error| panic!("{case}: {error}"));
    let container = container.replace("<ocf:container ", "<ocf:container version=\"1.0\" ");
    // The generated writer uses this prefix; guard the synthetic premise.
    assert!(container.contains("version=\"1.0\""), "{case}");
    for part in ["META-INF/container.xml", "Contents/content.hpf"] {
        for extended in [false, true] {
            let mut reference: Option<PackageAnalysis> = None;
            for (encoding, encoding_name) in
                ["UTF-8", "UTF-16LE+BOM", "UTF-16BE+BOM"].into_iter().enumerate()
            {
                let case = format!("part={part}, extended={extended}, encoding={encoding_name}, output=input-analysis");
                let mut c = container.clone();
                let mut h = content.clone();
                if extended {
                    if part == "META-INF/container.xml" {
                        c = c.replace("<ocf:container ", "<ocf:container future=\"1\" ");
                    } else {
                        h = h.replace("<opf:itemref ", "<opf:itemref future=\"1\" ");
                    }
                }
                let raw_c = encoded(&c, encoding);
                let raw_h = encoded(&h, encoding);
                let bytes = rewrite(&original, "META-INF/container.xml", &raw_c, &case);
                let bytes = rewrite(&bytes, "Contents/content.hpf", &raw_h, &case);
                let package =
                    RawPackage::read(&bytes).unwrap_or_else(|error| panic!("{case}: {error}"));
                assert_eq!(
                    package.order.container_excludable,
                    !(extended && part == "META-INF/container.xml"),
                    "{case}"
                );
                assert_eq!(
                    package.order.content_excludable,
                    !(extended && part == "Contents/content.hpf"),
                    "{case}"
                );
                let node = Node::parse_package_xml(
                    &package
                        .read_text_entry("Contents/content.hpf")
                        .unwrap_or_else(|error| panic!("{case}: {error}")),
                )
                .unwrap_or_else(|error| panic!("{case}: {error}"));
                let (global, _) = inputs::package_inputs(&package, &node)
                    .unwrap_or_else(|error| panic!("{case}: {error}"));
                for path in ["META-INF/container.xml", "Contents/content.hpf"] {
                    let case = format!("{case}, fingerprint part={path}");
                    assert_eq!(
                        global.children.iter().any(|n| n.name == path),
                        extended && part == path,
                        "{case}"
                    );
                }
                let decoded =
                    HwpxDecoder::decode(&bytes).unwrap_or_else(|error| panic!("{case}: {error}"));
                assert!(decoded.warnings.is_empty(), "{case}: {:?}", decoded.warnings);
                assert_eq!(
                    decoded
                        .document
                        .sections()
                        .iter()
                        .map(|s| s.paragraphs[0].text_content())
                        .collect::<Vec<_>>(),
                    ["beta", "alpha"],
                    "{case}"
                );
                let analysis =
                    PackageAnalysis::read(&bytes).unwrap_or_else(|error| panic!("{case}: {error}"));
                assert_eq!(
                    analysis.section_paths,
                    ["Contents/section1.xml", "Contents/section0.xml"],
                    "{case}"
                );
                if let Some(ref prior) = reference {
                    assert_eq!(prior.global.hash, analysis.global.hash, "{case}");
                    assert_eq!(
                        prior.paths().collect::<Vec<_>>(),
                        analysis.paths().collect::<Vec<_>>(),
                        "{case}"
                    );
                    assert!(prior.section_same(&analysis, 0), "{case}");
                    assert!(prior.section_same(&analysis, 1), "{case}");
                }
                let case = format!("part={part}, extended={extended}, encoding={encoding_name}, output=encode_with_base");
                let output = HwpxEncoder::encode_with_base(
                    &bytes,
                    &decoded.document.validate().unwrap_or_else(|error| panic!("{case}: {error}")),
                    &decoded.style_store,
                    &decoded.image_store,
                    EncodeOptions::default(),
                )
                .unwrap_or_else(|error| panic!("{case}: {error}"));
                assert_eq!(output.bytes, bytes, "{case}");
                let case = format!("part={part}, extended={extended}, encoding={encoding_name}, output=export_section_for_edit");
                let mut exported = HwpxPatcher::export_section_for_edit(&bytes, 0, true)
                    .unwrap_or_else(|error| panic!("{case}: {error}"))
                    .exported;
                assert_eq!(
                    exported
                        .preservation
                        .as_ref()
                        .unwrap_or_else(|| panic!("{case}: missing preservation"))
                        .section_path,
                    "Contents/section1.xml",
                    "{case}"
                );
                exported.section.paragraphs[0].runs[0].content = RunContent::Text("edited".into());
                let case = format!("part={part}, extended={extended}, encoding={encoding_name}, output=patch_exported_section");
                let edited = HwpxPatcher::patch_exported_section(&bytes, 0, &exported)
                    .unwrap_or_else(|error| panic!("{case}: {error}"))
                    .bytes;
                let edited_pkg =
                    RawPackage::read(&edited).unwrap_or_else(|error| panic!("{case}: {error}"));
                assert_eq!(
                    edited_pkg
                        .read_entry("META-INF/container.xml")
                        .unwrap_or_else(|error| panic!("{case}: {error}")),
                    raw_c,
                    "{case}"
                );
                assert_eq!(
                    edited_pkg
                        .read_entry("Contents/content.hpf")
                        .unwrap_or_else(|error| panic!("{case}: {error}")),
                    raw_h,
                    "{case}"
                );
                let edited_doc =
                    HwpxDecoder::decode(&edited).unwrap_or_else(|error| panic!("{case}: {error}"));
                assert_eq!(
                    edited_doc.document.sections()[0].paragraphs[0].text_content(),
                    "edited",
                    "{case}"
                );
                assert_eq!(
                    edited_doc.document.sections()[1].paragraphs[0].text_content(),
                    "alpha",
                    "{case}"
                );
                if extended {
                    let case = format!("part={part}, extended={extended}, encoding={encoding_name}, output=changed-extension-analysis");
                    let xml = if part == "META-INF/container.xml" { &c } else { &h };
                    let changed = rewrite(
                        &bytes,
                        part,
                        &encoded(&xml.replace("future=\"1\"", "future=\"2\""), encoding),
                        &case,
                    );
                    let changed = PackageAnalysis::read(&changed)
                        .unwrap_or_else(|error| panic!("{case}: {error}"));
                    assert_eq!(analysis.section_paths, changed.section_paths, "{case}");
                    assert_ne!(analysis.global.hash, changed.global.hash, "{case}");
                    assert!(!analysis.section_same(&changed, 0), "{case}");
                    assert!(!analysis.section_same(&changed, 1), "{case}");
                }
                reference = Some(analysis);
            }
        }
    }
}

// 이것을 실패시키는 것: content의 package/manifest/spine/item 확장 속성도
// metadata 요약에만 맡겨 global input에서 지우기.
#[test]
fn content_extension_positions() {
    let case = "content_extension_positions setup".to_owned();
    let original = base();
    let package = RawPackage::read(&original).unwrap_or_else(|error| panic!("{case}: {error}"));
    let content = package
        .read_text_entry("Contents/content.hpf")
        .unwrap_or_else(|error| panic!("{case}: {error}"));
    for tag in ["<opf:package ", "<opf:manifest>", "<opf:spine>", "<opf:item "] {
        let case = format!("part=Contents/content.hpf, extended=true, encoding=UTF-8, tag={tag}, output=extension-analysis");
        let replacement = format!(
            "{} future=\"1\"{}",
            tag.trim_end_matches([' ', '>']),
            if tag.ends_with('>') { ">" } else { " " }
        );
        let xml = content.replace(tag, &replacement);
        let bytes = rewrite(&original, "Contents/content.hpf", xml.as_bytes(), &case);
        let package = RawPackage::read(&bytes).unwrap_or_else(|error| panic!("{case}: {error}"));
        assert!(package.order.problems.is_empty(), "{case}: {tag}");
        assert!(!package.order.content_excludable, "{case}: {tag}");
        let analysis =
            PackageAnalysis::read(&bytes).unwrap_or_else(|error| panic!("{case}: {error}"));
        let changed = rewrite(
            &bytes,
            "Contents/content.hpf",
            xml.replace("future=\"1\"", "future=\"2\"").as_bytes(),
            &case,
        );
        let changed =
            PackageAnalysis::read(&changed).unwrap_or_else(|error| panic!("{case}: {error}"));
        assert_ne!(analysis.global.hash, changed.global.hash, "{case}: {tag}");
        assert!(!analysis.section_same(&changed, 0), "{case}");
        assert!(!analysis.section_same(&changed, 1), "{case}");
    }
}

// 이것을 실패시키는 것: 패키지 container를 HWP 도형 container로 취급해 id 소거.
#[test]
fn package_container_identity_is_literal() {
    package_attribute_changes("META-INF/container.xml", "<ocf:container ", "id", ["1", "2"]);
}

// 이것을 실패시키는 것: OPF itemref 속성 값을 같은 HWP 정의의 지문으로 치환.
#[test]
fn package_itemref_references_are_literal() {
    for attr in ["charPrIDRef", "prIDRef"] {
        package_attribute_changes("Contents/content.hpf", "<opf:itemref ", attr, ["0", "1"]);
    }
}

fn package_attribute_changes(part: &str, tag: &str, attr: &str, values: [&str; 2]) {
    let case = format!("part={part}, tag={tag}, attr={attr}, values={values:?}");
    let original = base();
    let package = RawPackage::read(&original).unwrap_or_else(|error| panic!("{case}: {error}"));
    // Two different positional IDs deliberately resolve to identical definitions.
    let header = package
        .read_text_entry("Contents/header.xml")
        .unwrap_or_else(|error| panic!("{case}: {error}"));
    let start = header
        .find("<hh:charProperties ")
        .unwrap_or_else(|| panic!("{case}: missing charProperties start"));
    let end = header[start..]
        .find("</hh:charProperties>")
        .unwrap_or_else(|| panic!("{case}: missing charProperties end"))
        + start;
    let header = format!(
        "{}<hh:charProperties itemCnt=\"2\"><hh:charPr id=\"0\"/><hh:charPr id=\"1\"/>{}",
        &header[..start],
        &header[end..]
    );
    let original = rewrite(&original, "Contents/header.xml", header.as_bytes(), &case);
    let mut definitions =
        Definitions::new(&Node::parse(&header).unwrap_or_else(|error| panic!("{case}: {error}")));
    let a = definitions.resolve(("charPr".into(), "0".into())).0;
    let b = definitions.resolve(("charPr".into(), "1".into())).0;
    assert!(a.resolved && b.resolved, "{case}");
    assert_eq!(a.hash, b.hash, "{case}");
    let xml = package.read_text_entry(part).unwrap_or_else(|error| panic!("{case}: {error}"));
    let xml = if part == "META-INF/container.xml" {
        xml.replace(tag, &format!("{tag}version=\"1.0\" "))
    } else {
        xml
    };
    let mut reference: Option<PackageAnalysis> = None;
    for (encoding, encoding_name) in
        ["UTF-8", "UTF-16LE+BOM", "UTF-16BE+BOM"].into_iter().enumerate()
    {
        let case =
            format!("{case}, extended=true, encoding={encoding_name}, output=attribute-analysis");
        let analyses: Vec<_> = values
            .iter()
            .map(|value| {
                let case = format!("{case}, value={value}");
                let xml = xml.replace(tag, &format!("{tag}{attr}=\"{value}\" "));
                let bytes = rewrite(&original, part, &encoded(&xml, encoding), &case);
                let package =
                    RawPackage::read(&bytes).unwrap_or_else(|error| panic!("{case}: {error}"));
                assert!(package.order.problems.is_empty(), "{case}");
                assert!(
                    !(if part == "META-INF/container.xml" {
                        package.order.container_excludable
                    } else {
                        package.order.content_excludable
                    }),
                    "{case}"
                );
                PackageAnalysis::read(&bytes).unwrap_or_else(|error| panic!("{case}: {error}"))
            })
            .collect();
        let before = &analyses[0];
        let after = &analyses[1];
        assert!(before.global.resolved && after.global.resolved, "{case}");
        assert_eq!(before.section_paths, after.section_paths, "{case}");
        assert_eq!(before.section_paths.len(), 2, "{case}");
        assert_ne!(
            before.global.hash, after.global.hash,
            "{case}: {part}@{attr}, encoding={encoding}"
        );
        let plan = crate::layout_edit::plan_invalidation(
            before,
            after,
            &crate::layout_edit::ParagraphCorrespondence::identity(before),
            &crate::layout_edit::ChangeSet::default(),
        );
        assert_eq!(plan.paragraphs.len(), 2, "{case}");
        assert!(plan.paragraphs.iter().all(|reasons| !reasons.is_empty()), "{case}");
        for s in 0..2 {
            let case = format!("{case}, section={s}");
            assert!(before.section_same(before, s), "{case}");
            assert!(!before.section_same(after, s), "{case}");
            if let Some(reference) = &reference {
                assert_eq!(reference.global.hash, before.global.hash, "{case}");
                assert!(reference.section_same(before, s), "{case}");
            }
        }
        reference = Some(
            analyses.into_iter().next().unwrap_or_else(|| panic!("{case}: missing first analysis")),
        );
    }
}

// 이것을 실패시키는 것: package parser에 field link 정준화를 복원하거나,
// 패키지 하위 Node의 출처를 누락하거나, linesegarray/shell 생략을 다시 적용.
// 요소의 namespace도 경계가 아니다: HWP namespace여도 입력 출처가 우선한다.
#[test]
fn package_nodes_keep_hwp_shaped_data() {
    for inner in [
        "<fieldBegin id='1'/><fieldEnd beginIDRef='1'/>",
        "<linesegarray><lineseg value='1'/></linesegarray>",
        "<p id='1'/>",
        "<header id='1'/>",
        "<footer id='1'/>",
    ] {
        let xml = format!(
            "<metadata xmlns='http://www.hancom.co.kr/hwpml/2011/paragraph'>{inner}</metadata>"
        );
        let before = Node::parse_package_xml(&xml).unwrap();
        let after = Node::parse_package_xml(&xml.replace("'1'", "'2'")).unwrap();
        let mut definitions = Definitions::new(&Node::default());
        for shell in [false, true] {
            let a = definitions.stamp(&before, shell).0;
            let b = definitions.stamp(&after, shell).0;
            assert!(a.resolved && b.resolved, "{inner}");
            assert_ne!(a.hash, b.hash, "{inner}, shell={shell}");
        }
    }
}

// 이것을 실패시키는 것: 패키지 속성에 HWP 참조 해석/신호 스캔을 재적용하거나
// fontRef의 속성 값을 생략하기. 원시 값은 header 정의 변경과 독립적이어야 한다.
#[test]
fn package_nodes_have_no_hwp_dependencies() {
    for inner in [
        "<itemref charPrIDRef='0' futureIDRef='8'/>",
        "<fontRef latin='0'/>",
        "<fieldBegin id='1' beginIDRef='1'/>",
    ] {
        let xml = format!("<package>{inner}</package>");
        let node = Node::parse_package_xml(&xml).unwrap();
        let (a, signals) = Definitions::new(&Node::default()).stamp(&node, false);
        assert!(a.resolved, "{inner}");
        assert!(a.dependencies.is_empty());
        assert!(!signals.unresolved && !signals.document_input && !signals.backwards);
        let header = Node::parse(
            "<head><charPr id='0'/><fontface lang='LATIN'><font id='0'/></fontface></head>",
        )
        .unwrap();
        let (b, _) = Definitions::new(&header).stamp(&node, false);
        assert_eq!(a.hash, b.hash);
        let changed =
            Node::parse_package_xml(&xml.replace("'0'", "'2'").replace("'1'", "'2'")).unwrap();
        assert_ne!(a.hash, Definitions::new(&header).stamp(&changed, false).0.hash);
    }
}
