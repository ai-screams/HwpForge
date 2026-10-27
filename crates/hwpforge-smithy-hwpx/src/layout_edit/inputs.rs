//! Package input policy (the single classification table for the proof).
//!
//! | Input | Dependency / disposition | Evidence and conservative boundary |
//! | --- | --- | --- |
//! | Opaque entries, regardless of path | Global raw-byte hash takes precedence | The classifications and exclusions below apply only to normal entries. |
//! | header font/char/para/style, numbering, bullets, tabs, borders | Typed transitive definitions | `schema::header`, `Definitions::reference`; font metrics and paragraph heads affect line width/height. |
//! | header outside refList, including layoutCompatibility | Global | `schema::header::HxHead`; compatibility can change layout despite equal text/styles. Only derived `head@secCnt` is removed. |
//! | section/page/column settings, headers/footers, master pages | Section/host graph | `schema::section`, `PackageAnalysis::read`; page and flow geometry. |
//! | content.hpf metadata | Document-field consumers | `Signals::scan`: fieldBegin, page fields, `$` stringParam, including `$lastsaveby`. |
//! | content.hpf spine | Ordered section paths; changed common prefix invalidates globally | Spine itemref order determines section order; manifest IDs resolve target paths; unsupported structures or extension attributes retain the entire content.hpf tree globally. |
//! | content.hpf manifest | Target resolution for supported content.hpf | OPF item ids/hrefs locate resources; referenced payload AND media-type remain inputs. Manifest order/spelling alone is not layout; unsupported extension attributes retain the entire tree globally. |
//! | META-INF/manifest.xml | Excluded package inventory | ODF manifest describes package entries; referenced payloads are still compared after decoding. |
//! | BinData, Chart | Referenced target bytes/canonical XML | `index_package_targets`; missing/changed targets cannot prove equality. |
//! | version.xml | Excluded packaging provenance | Hancom format guide: file-format/writer environment version, not document compatibility (which stays in header). |
//! | META-INF/container.xml | Excluded only after supported rootfiles are validated | Exactly one Contents/content.hpf package root; known preview/RDF roots allowed. Unsupported structures or extension attributes retain the entire container tree globally. |
//! | META-INF/container.rdf | Excluded package description | Resolved document parts remain inputs. |
//! | mimetype, Preview/* | Excluded signature/derived preview | Hancom format guide; not source content for line layout. |
//! | settings.xml ha:CaretPosition (listIDRef, paraIDRef, pos) | Excluded ONLY in the known application namespace | KS X 6101 table 14: saved insertion point, not paragraph properties. Unknown attributes/children/namespaces stay global. |
//! | settings.xml config-item PrintAutoFootNote, PrintAutoHeadNote | Global, retained | Hancom confirms PrintAutoFootNote affects printed output. No proof that all consumers lay out identically. |
//! | settings.xml config-item PrintMethod, PrintCropMark, BinderHoleType | Global, retained | Print/imposition settings; no demonstrated line-layout equivalence. |
//! | settings.xml config-item ZoomX, ZoomY, OverlapSize | Global, retained | Observed under PrintInfo, NOT evidence of a harmless view zoom. Print scaling/overlap cannot be inferred from names. |
//! | settings.xml other print/view/cursor config-item or config-item-set | Global, retained | Extensible ODF configuration. No wildcard exclusion based on names such as Zoom or Cursor. |
//! | Any other package part / unknown setting | Global, retained | Sound by default, even without a known field consumer. |
//!
//! Sources: [Hancom HWPX structure](https://tech.hancom.com/hwpxformat/),
//! [CaretPosition / KS X 6101 table 14](https://tech.hancom.com/python-hwpx-parsing-1/),
//! [Hancom print setting behavior](https://forum.developer.hancom.com/t/print/2499),
//! [OWPML writer](https://github.com/hancom-io/hwpx-owpml-model/blob/main/OWPMLApi/OWPMLSerialize.cpp).
//! These sources describe format roles; no Hancom rendering experiment is claimed.

use super::{collect_named, Node, RawPackage};
use crate::HwpxResult;
use sha2::{Digest, Sha256};

const APP: &str = "{http://www.hancom.co.kr/hwpml/2011/app}";

/// Remove only a schema-known caret, never unfamiliar extension data. Prune
/// empty application wrappers so absent settings and caret-only settings agree.
fn settings_input(mut node: Node) -> Option<Node> {
    if node.namespace == APP
        && node.name == "CaretPosition"
        && node.attrs.keys().all(|k| matches!(k.as_str(), "listIDRef" | "paraIDRef" | "pos"))
        && node.children.is_empty()
        && node.text.iter().all(|t| t.trim().is_empty())
    {
        return None;
    }
    node.children = node.children.into_iter().filter_map(settings_input).collect();
    let wrapper =
        node.name.is_empty() || (node.namespace == APP && node.name == "HWPApplicationSetting");
    if wrapper
        && node.attrs.is_empty()
        && node.children.is_empty()
        && node.text.iter().all(|t| t.trim().is_empty())
    {
        None
    } else {
        Some(node)
    }
}

fn represented_part(path: &str) -> bool {
    matches!(path, "Contents/header.xml" | "Contents/content.hpf")
        || ["Contents/section", "Contents/masterpage"].iter().any(|prefix| {
            path.strip_prefix(prefix)
                .and_then(|p| p.strip_suffix(".xml"))
                .is_some_and(|p| p.parse::<usize>().is_ok())
        })
        || path.starts_with("BinData/")
        || path.starts_with("Chart/")
}

pub(super) fn package_inputs(package: &RawPackage, content: &Node) -> HwpxResult<(Node, Node)> {
    let mut metadata = Vec::new();
    collect_named(content, &["metadata"], &mut metadata);
    let mut global = Node::default();
    for (path, bytes, opaque) in package.preserved_entries() {
        if opaque {
            global.children.push(Node {
                name: path.into(),
                attrs: std::collections::BTreeMap::from([(
                    "bytes".into(),
                    format!("{:x?}", Sha256::digest(bytes).as_slice()),
                )]),
                ..Node::default()
            });
            continue;
        }
        if path == crate::decoder::package_order::CONTENT && !package.order.content_excludable {
            global.children.push(Node {
                name: path.into(),
                children: vec![content.clone()],
                ..Node::default()
            });
        }
        if represented_part(path)
            || (path == "META-INF/container.xml" && package.order.container_excludable)
            || matches!(
                path,
                "version.xml" | "META-INF/container.rdf" | "META-INF/manifest.xml" | "mimetype"
            )
            || path.starts_with("Preview/")
        {
            continue;
        }
        let bytes = package.read_entry(path)?;
        let mut info = Node { name: path.into(), ..Node::default() };
        if path.ends_with(".xml") || path.ends_with(".hpf") {
            let xml = crate::package_xml::text_entry(path, bytes)?;
            let parsed = if path == crate::decoder::package_order::CONTAINER {
                Node::parse_package_xml(&xml)?
            } else {
                Node::parse(&xml)?
            };
            if path == "settings.xml" {
                let Some(settings) = settings_input(parsed) else { continue };
                info.children.push(settings);
            } else {
                info.children.push(parsed);
            }
        } else {
            info.attrs.insert("bytes".into(), format!("{:x?}", Sha256::digest(bytes).as_slice()));
        }
        global.children.push(info);
    }
    // ZIP entry order is packaging syntax, not an ordering of layout inputs.
    global.children.sort_by(|a, b| a.name.cmp(&b.name));
    Ok((global, Node { children: metadata, ..Node::default() }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // 이것을 실패시키는 것: RDF/manifest/version을 전역 입력으로 되돌리거나
    // Contents 아래의 미분류 파일까지 이미 분석된 것으로 간주해 누락하기.
    #[test]
    fn packaging_inventory_is_excluded_and_unknown_parts_are_global() {
        use std::io::{Cursor, Write};
        let analyze = |path: &str| {
            let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
            zip.start_file("mimetype", zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(b"application/hwp+zip").unwrap();
            if path != "mimetype" {
                zip.start_file(path, zip::write::SimpleFileOptions::default()).unwrap();
                zip.write_all(b"<unknown/>").unwrap();
            }
            let package = RawPackage::read(&zip.finish().unwrap().into_inner()).unwrap();
            package_inputs(&package, &Node::default()).unwrap().0.children.len()
        };
        for path in [
            "version.xml",
            "META-INF/container.rdf",
            "META-INF/manifest.xml",
            "mimetype",
            "Preview/PrvText.txt",
        ] {
            assert_eq!(analyze(path), 0, "{path}");
        }
        for path in [
            "META-INF/container.xml",
            "Future/layout.xml",
            "Contents/future.xml",
            "profile/data.bin",
            "settings.xml",
        ] {
            assert_eq!(analyze(path), 1, "{path}");
        }
    }

    // 이것을 실패시키는 것: settings 전체 또는 모든 config-item을 제외하기;
    // 반대로 알려진 CaretPosition의 pos를 전역 조판 입력으로 남기기.
    #[test]
    fn caret_only_is_ignored_but_every_print_or_unknown_setting_is_retained() {
        let wrap = |inner: &str| {
            format!("<ha:HWPApplicationSetting xmlns:ha='http://www.hancom.co.kr/hwpml/2011/app' xmlns:c='urn:oasis:names:tc:opendocument:xmlns:config:1.0'>{inner}</ha:HWPApplicationSetting>")
        };
        assert!(settings_input(
            Node::parse(&wrap("<ha:CaretPosition listIDRef='1' paraIDRef='9' pos='15'/>")).unwrap()
        )
        .is_none());
        for name in [
            "PrintAutoFootNote",
            "PrintAutoHeadNote",
            "PrintMethod",
            "PrintCropMark",
            "BinderHoleType",
            "ZoomX",
            "ZoomY",
            "OverlapSize",
            "FutureLayout",
            "Cursor",
        ] {
            let xml = wrap(&format!("<c:config-item-set c:name='PrintInfo'><c:config-item c:name='{name}' c:type='int'>1</c:config-item></c:config-item-set>"));
            assert!(settings_input(Node::parse(&xml).unwrap()).is_some(), "{name}");
        }
        for inner in [
            "<ha:CaretPosition future='1'/>",
            "<ha:CaretPosition><future/></ha:CaretPosition>",
            "<CaretPosition pos='1'/>",
            "<ha:FutureSetting/>",
        ] {
            assert!(settings_input(Node::parse(&wrap(inner)).unwrap()).is_some(), "{inner}");
        }
    }
}

#[cfg(test)]
mod opaque_input_tests {
    use super::*;
    // 이것을 실패시키는 것: opaque unknown payload를 해시에서 누락하거나 XML로 정준화하기.
    #[test]
    fn opaque_unknown_inputs_use_exact_bytes() {
        use std::io::Write;
        let input = |payload: &[u8]| {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            for (name, bytes) in
                [("mimetype", b"application/hwp+zip".as_slice()), ("Unused/./future.xml", payload)]
            {
                zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
                zip.write_all(bytes).unwrap();
            }
            let bytes = zip.finish().unwrap().into_inner();
            package_inputs(&RawPackage::read(&bytes).unwrap(), &Node::default())
                .unwrap()
                .0
                .children
                .into_iter()
                .map(|n| (n.name, n.attrs))
                .collect::<Vec<_>>()
        };
        assert_ne!(input(b"<x />"), input(b"<x/>"));
        assert_ne!(input(b"not xml"), input(b"other"));
    }
}
