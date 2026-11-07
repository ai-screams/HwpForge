//! Exact preview deletion and descriptor preservation for raw editors.
use super::{
    integrity::{comparison_path, normalized_namespace},
    OutputChanges, RawPackage,
};
use crate::{EncodeWarning, HwpxError, HwpxResult};
use hwpforge_core::{source_xml::XmlSource, zip_package::open_package};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
};

use crate::decoder::package_order::{OCF, OPF};
use crate::encoder::package::{
    CONTAINER_PART, CONTAINER_RDF_PART, CONTENT_HPF_PART, MANIFEST_PART,
};
const PAYLOADS: [&str; 2] = ["Preview/PrvText.txt", "Preview/PrvImage.png"];

#[derive(Default)]
pub(crate) struct PreviewRemoval {
    pub(crate) deleted: Vec<String>,
    descriptors: Vec<DescriptorEdit>,
    ids: BTreeSet<String>,
    max_value_len: usize,
}
struct DescriptorEdit {
    path: String,
    original: Vec<u8>,
    removals: Vec<Range<usize>>,
}
struct Node {
    path: Vec<String>,
    ns: String,
    attrs: BTreeMap<String, String>,
}
/// Decodes `%XX` once; other bytes, including malformed escapes, stay literal.
/// Used only to refuse references spelled with percent escapes.
fn percent_decoded_once(value: &str) -> impl Iterator<Item = u8> + '_ {
    let mut bytes = value.bytes().peekable();
    std::iter::from_fn(move || {
        let byte = bytes.next()?;
        if byte == b'%' {
            let mut lookahead = bytes.clone();
            if let Some((high, low)) = lookahead
                .next()
                .and_then(|b| (b as char).to_digit(16))
                .zip(lookahead.next().and_then(|b| (b as char).to_digit(16)))
            {
                bytes = lookahead;
                return Some((high * 16 + low) as u8);
            }
        }
        Some(byte)
    })
}

fn refused(reason: impl std::fmt::Display) -> HwpxError {
    HwpxError::InvalidStructure {
        detail: format!("입력 정책 거부(지원하지 않는 Preview 구성): {reason}"),
    }
}
fn media(path: &str) -> &str {
    if path == PAYLOADS[0] {
        "text/plain"
    } else {
        "image/png"
    }
}

// OCF roots select packages or metadata; OPF items retain their content role.
fn discover_descriptors(
    package: &RawPackage,
    nodes: &[Node],
    paths: &mut BTreeSet<String>,
) -> HwpxResult<()> {
    for node in nodes {
        let root = node.ns == OCF && node.path == ["container", "rootfiles", "rootfile"];
        if !(root || (node.ns == OPF && node.path == ["package", "manifest", "item"])) {
            continue;
        }
        let Some(target) = node.attrs.get(if root { "full-path" } else { "href" }) else {
            continue;
        };
        let key = comparison_path(target);
        if !key.starts_with("meta-inf/") {
            if root && target != CONTENT_HPF_PART && !PAYLOADS.contains(&target.as_str()) {
                return Err(refused("unsupported rootfile target"));
            }
            continue;
        }
        let xml_media = node.attrs.get("media-type").is_some_and(|media| {
            let essence = media.split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
            matches!(essence.as_str(), "application/xml" | "text/xml") || essence.ends_with("+xml")
        });
        if root || key.ends_with(".xml") || key.ends_with(".rdf") || xml_media {
            if !package.index_by_path.contains_key(target) {
                return Err(refused("missing META-INF descriptor"));
            }
            paths.insert(target.clone());
        }
    }
    Ok(())
}

fn select_removals(
    raw: &[u8],
    nodes: &[Node],
    selected: Vec<usize>,
) -> HwpxResult<Vec<Range<usize>>> {
    let mut removals = Vec::new();
    if !selected.is_empty() {
        let original = std::str::from_utf8(raw).map_err(refused)?;
        let (_, source) = XmlSource::read(original).map_err(refused)?;
        for i in selected {
            let element = &source.elements()[i + 1];
            if !element.children().is_empty()
                || element.inner().is_some_and(|r| !original[r].trim().is_empty())
            {
                return Err(refused("preview element has content"));
            }
            // Namespace bindings on ancestors must agree with the selected role.
            let mut parent = element.parent();
            while parent != 0 {
                if nodes[parent - 1].ns != nodes[i].ns {
                    return Err(refused("preview ancestor namespace"));
                }
                parent = source.elements()[parent].parent();
            }
            removals.push(element.outer());
        }
    }
    Ok(removals)
}

impl PreviewRemoval {
    pub(crate) fn plan(package: &RawPackage) -> HwpxResult<Self> {
        let mut deleted = Vec::new();
        for entry in &package.entries {
            let key = comparison_path(&entry.path);
            if key == "preview" || key.starts_with("preview/") {
                if entry.opaque
                    || !(PAYLOADS.contains(&entry.path.as_str())
                        || (entry.path == "Preview/" && entry.bytes.is_empty()))
                {
                    return Err(refused("unsupported preview entry"));
                }
                deleted.push(entry.path.clone());
            }
        }
        deleted.sort();
        let mut plan = Self { deleted, ..Self::default() };
        if plan.deleted.is_empty() {
            return Ok(plan);
        }
        let mut paths: BTreeSet<String> =
            [CONTAINER_PART, MANIFEST_PART, CONTENT_HPF_PART, CONTAINER_RDF_PART]
                .into_iter()
                .filter(|path| package.index_by_path.contains_key(*path))
                .map(str::to_owned)
                .collect();
        let mut ids = BTreeSet::new();
        // Resolve deleted definitions before checking any descriptor values.
        if paths.contains(CONTENT_HPF_PART) {
            let xml = package.read_text_entry(CONTENT_HPF_PART)?;
            crate::decoder::package_order::visit(&xml, |path, ns, attrs| {
                if normalized_namespace(ns)?.as_ref() == OPF
                    && path == ["package", "manifest", "item"]
                    && attrs.get("href").is_some_and(|href| plan.deleted.contains(href))
                {
                    ids.insert(
                        attrs
                            .get("id")
                            .filter(|id| !id.is_empty())
                            .ok_or("missing preview id")?
                            .clone(),
                    );
                }
                Ok(())
            })
            .map_err(refused)?;
        }
        plan.ids = ids;
        plan.max_value_len = plan
            .deleted
            .iter()
            .chain(&plan.ids)
            .map(String::len)
            .max()
            .unwrap_or(0)
            .saturating_add(1); // A fragment reference can add '#'.
        let mut done = BTreeSet::new();
        while let Some(path) = paths.iter().find(|p| !done.contains(*p)).cloned() {
            done.insert(path.clone());
            let raw = package.read_entry(&path)?;
            let decoded = crate::package_xml::decode(
                raw.into(),
                hwpforge_core::zip_package::MAX_ENTRY_SIZE as usize,
            )
            .map_err(refused)?;
            let mut nodes = Vec::new();
            crate::decoder::package_order::visit(&decoded, |p, ns, attrs| {
                nodes.push(Node {
                    path: p.to_vec(),
                    ns: normalized_namespace(ns)?.into_owned(),
                    attrs: attrs.clone(),
                });
                Ok(())
            })
            .map_err(refused)?;
            discover_descriptors(package, &nodes, &mut paths)?;
            plan.validate_descriptor(&path, raw, &decoded, &nodes)?;
        }
        Ok(plan)
    }

    fn validate_descriptor(
        &mut self,
        path: &str,
        raw: &[u8],
        decoded: &str,
        nodes: &[Node],
    ) -> HwpxResult<()> {
        let mut permitted: BTreeSet<(usize, String)> = BTreeSet::new();
        let mut selected = Vec::new();
        let mut all_ids = BTreeSet::new();
        let mut spine_ids = BTreeSet::new();
        let mut roots = BTreeSet::new();
        let mut main_roots = 0;
        for (i, node) in nodes.iter().enumerate() {
            let a = &node.attrs;
            let item = path == CONTENT_HPF_PART
                && node.ns == OPF
                && node.path == ["package", "manifest", "item"];
            if item {
                if let Some(id) = a.get("id") {
                    if !all_ids.insert(id) {
                        return Err(refused("duplicate manifest id"));
                    }
                }
            }
            let root = path == CONTAINER_PART
                && node.ns == OCF
                && node.path == ["container", "rootfiles", "rootfile"];
            let reference = if item {
                a.get("href")
            } else if root {
                a.get("full-path")
            } else {
                None
            };
            if root && a.get("full-path").map(String::as_str) == Some(CONTENT_HPF_PART) {
                main_roots += 1;
            }
            if let Some(target) = reference {
                let undecorated = target.split(['?', '#']).next().unwrap_or(target);
                if undecorated != target && self.matches_removed_value(undecorated)? {
                    return Err(refused("unresolved preview address"));
                }
                if self.deleted.contains(target) && PAYLOADS.contains(&target.as_str()) {
                    if a.get("media-type").map(String::as_str) != Some(media(target)) {
                        return Err(refused("preview media type"));
                    }
                    let allowed: &[&str] = if item {
                        &["id", "href", "media-type", "isEmbeded"]
                    } else {
                        &["full-path", "media-type"]
                    };
                    if a.keys().any(|key| !allowed.contains(&key.as_str()))
                        || a.get("isEmbeded").is_some_and(|value| value != "1")
                    {
                        return Err(refused("conditional preview selection"));
                    }
                    if root && !roots.insert(target) {
                        return Err(refused("duplicate preview rootfile"));
                    }
                    permitted.insert((i, if item { "href" } else { "full-path" }.into()));
                    if item {
                        permitted.insert((i, "id".into()));
                    }
                    selected.push(i);
                }
            }
            if path == CONTENT_HPF_PART
                && node.ns == OPF
                && node.path == ["package", "spine", "itemref"]
                && a.get("idref").is_some_and(|id| self.ids.contains(id))
            {
                if a.keys().any(|key| !["idref", "linear"].contains(&key.as_str()))
                    || a.get("linear").is_some_and(|v| v != "yes")
                {
                    return Err(refused("conditional preview spine"));
                }
                if !spine_ids.insert(a.get("idref").unwrap()) {
                    return Err(refused("duplicate preview itemref"));
                }
                permitted.insert((i, "idref".into()));
                selected.push(i);
            }
        }
        if path == CONTAINER_PART && main_roots != 1 {
            return Err(refused("main rootfile must survive"));
        }
        let matches = |value: &str| self.matches_removed_value(value);
        crate::structural::xml_values::scan_values(
            decoded,
            false,
            |e| refused(format!("{path}: {e}")),
            |i, key, value| {
                if key == "xml:base"
                    || (key.starts_with("xmlns")
                        && value == "http://www.w3.org/XML/1998/namespace"
                        && key != "xmlns:xml")
                    || (matches(value)? && !permitted.contains(&(i, key.into())))
                {
                    return Err(refused("unresolved descriptor attribute"));
                }
                Ok(())
            },
            |_, value| {
                if matches(value)? {
                    Err(refused("surviving descriptor text"))
                } else {
                    Ok(())
                }
            },
            |_, _| Ok(()),
        )?;
        let removals = select_removals(raw, nodes, selected)?;
        self.descriptors.push(DescriptorEdit {
            path: path.into(),
            original: raw.to_vec(),
            removals,
        });
        Ok(())
    }

    fn matches_removed_value(&self, value: &str) -> HwpxResult<bool> {
        // Percent spelling is decoded once for refusal only. Exact deletion
        // admission and ZIP lookup continue to use the original attribute value.
        let key = if value.contains('%') {
            crate::structural::xml_values::comparison_bytes(
                percent_decoded_once(value),
                self.max_value_len,
            )
        } else {
            crate::structural::xml_values::comparison_value(value, self.max_value_len)
        }
        .map_err(refused)?;
        let Some(key) = key else {
            return Ok(false);
        };
        Ok(self.deleted.iter().any(|p| comparison_path(p) == key)
            || self.ids.contains(value)
            || value.strip_prefix('#').is_some_and(|id| {
                self.ids.contains(id)
                    || (id.contains('%') && {
                        // Compare decoded bytes exactly; a lossy text conversion
                        // would let unrelated invalid escapes match.
                        let decoded: Vec<u8> = percent_decoded_once(id).collect();
                        self.ids.iter().any(|known| known.as_bytes() == decoded.as_slice())
                    })
            }))
    }

    fn verify_values(&self, path: &str, raw: &[u8]) -> HwpxResult<()> {
        let xml = crate::package_xml::decode(
            raw.into(),
            hwpforge_core::zip_package::MAX_ENTRY_SIZE as usize,
        )
        .map_err(refused)?;
        let check = |value: &str| {
            if self.matches_removed_value(value)? {
                Err(refused("surviving final descriptor value"))
            } else {
                Ok(())
            }
        };
        crate::structural::xml_values::scan_values(
            &xml,
            false,
            |e| refused(format!("{path}: {e}")),
            |_, _, value| check(value),
            |_, value| check(value),
            |_, _| Ok(()),
        )
    }

    pub(crate) fn apply(
        &self,
        package: &mut RawPackage,
        changes: &mut OutputChanges,
    ) -> HwpxResult<()> {
        for edit in &self.descriptors {
            if !edit.removals.is_empty() {
                let replacements =
                    edit.removals.iter().cloned().map(|r| (r, String::new())).collect();
                let output = crate::cache_flow::splice(
                    std::str::from_utf8(&edit.original).map_err(refused)?,
                    replacements,
                )?;
                package.replace_text_entry(&edit.path, output);
                changes.rewritten.push(edit.path.clone());
            }
        }
        package.remove_entries_exact(&self.deleted);
        changes.deleted.extend(self.deleted.clone());
        Ok(())
    }

    /// Only descriptor payloads are read here; package preservation is separate.
    pub(crate) fn verify_final(&self, bytes: &[u8]) -> HwpxResult<Option<EncodeWarning>> {
        let mut package =
            open_package(bytes, Default::default()).map_err(crate::package_path::package_error)?;
        for edit in &self.descriptors {
            let index = package
                .entries()
                .iter()
                .position(|e| e.path == edit.path)
                .ok_or_else(|| refused("missing descriptor"))?;
            let output = package.read(index).map_err(crate::package_path::package_error)?;
            self.verify_values(&edit.path, &output)?;
            if edit.removals.is_empty() {
                if output != edit.original {
                    return Err(refused("descriptor bytes changed"));
                }
            } else {
                let replacements =
                    edit.removals.iter().cloned().map(|r| (r, String::new())).collect();
                crate::cache_flow::verify_replacements(
                    std::str::from_utf8(&edit.original).map_err(refused)?,
                    std::str::from_utf8(&output).map_err(refused)?,
                    replacements,
                )?;
            }
        }
        Ok((!self.deleted.is_empty())
            .then(|| EncodeWarning::PreviewRemoved { parts: self.deleted.clone() }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raw_package::RawPackageEntry;
    use hwpforge_core::{Control, Document, PageSettings, Paragraph, Run, RunContent, Section};
    use hwpforge_foundation::{CharShapeIndex, FieldType, ParaShapeIndex};
    use zip::CompressionMethod;

    fn generated() -> RawPackage {
        let mut document = Document::new();
        for name in ["first", "second"] {
            let run = Run {
                char_shape_id: CharShapeIndex::new(0),
                content: RunContent::Control(Box::new(Control::Field {
                    field_type: FieldType::ClickHere,
                    hint_text: Some("old".into()),
                    help_text: None,
                    name: Some(name.into()),
                    display_text: "old".into(),
                })),
            };
            document.add_section(Section::with_paragraphs(
                vec![Paragraph::with_runs(vec![run], ParaShapeIndex::new(0))],
                PageSettings::a4(),
            ));
        }
        let bytes = crate::HwpxEncoder::encode(
            &document.validate().unwrap(),
            &crate::style_store_for_preset("default").unwrap(),
            &Default::default(),
        )
        .unwrap();
        RawPackage::read(&bytes).unwrap()
    }
    fn add(package: &mut RawPackage, path: &str, bytes: &[u8]) {
        package.index_by_path.insert(path.into(), package.entries.len());
        package.entries.push(RawPackageEntry {
            path: path.into(),
            bytes: bytes.into(),
            opaque: false,
            compression: CompressionMethod::Deflated,
        });
    }
    fn preview() -> RawPackage {
        let mut p = generated();
        for path in PAYLOADS {
            add(&mut p, path, b"derived");
        }
        let xml = p.read_text_entry("META-INF/container.xml").unwrap();
        p.replace_text_entry("META-INF/container.xml", format!("\u{feff}{}", xml.replace("</ocf:rootfiles>", "<ocf:rootfile full-path='Preview/PrvText.txt' media-type='text/plain'/></ocf:rootfiles>")));
        p
    }

    // 이것을 실패시키는 것: percent 접두사/구분자를 거부 비교에서 해석하지 않거나 삭제 허가에 사용.
    #[test]
    fn percent_encoded_manifest_addresses_are_refused_by_public_fill() {
        let mut accepted = Vec::new();
        for target in ["Preview%2FPrvText.txt", "%50review/PrvText.txt"] {
            for media in ["text/plain", "image/png"] {
                let mut p = preview();
                let xml = p.read_text_entry(CONTENT_HPF_PART).unwrap();
                p.replace_text_entry(CONTENT_HPF_PART, xml.replace("</opf:manifest>", &format!("<opf:item id='alias' href='{target}' media-type='{media}'/></opf:manifest>")));
                let result = crate::HwpxFiller::fill(
                    &p.write().unwrap(),
                    &[("first".into(), "new".into())].into_iter().collect(),
                );
                if result.is_ok() {
                    accepted.push((target, media));
                }
            }
        }
        assert!(accepted.is_empty(), "surviving references: {accepted:?}");
    }

    // 이것을 실패시키는 것: 거부용 percent 비교를 속성/본문/최종 검증 중 한 곳에만 적용.
    #[test]
    fn encoded_deletion_keys_share_descriptor_comparison() {
        let p = preview();
        let plan = PreviewRemoval::plan(&p).unwrap();
        let xml = p.read_text_entry(CONTENT_HPF_PART).unwrap();
        for payload in PAYLOADS {
            let mut aliases = vec![
                payload.to_ascii_lowercase(),
                payload.replace("Preview", "&#x50;review"),
                payload.replace('/', "&#47;"),
                payload.replace("Preview", "&#37;50review"),
                format!("./{payload}"),
                payload.replace('/', "//"),
                payload.replace('/', "\\"),
                format!("Other/../{payload}"),
                payload.bytes().map(|b| format!("%{b:02X}")).collect(),
                format!("%2e%2f{payload}"),
                format!("Other%2f%2e%2e%5c{payload}"),
                format!("%FF%2f%2e%2e%2f{payload}"),
            ];
            for (i, byte) in payload.bytes().enumerate() {
                for hex in [format!("%{byte:02X}"), format!("%{byte:02x}")] {
                    aliases.push(format!("{}{hex}{}", &payload[..i], &payload[i + 1..]));
                }
            }
            for alias in aliases {
                for extra in
                    [format!("<extra value='{alias}'/>"), format!("<extra>{alias}</extra>")]
                {
                    let mut bad = p.clone();
                    bad.replace_text_entry(
                        CONTENT_HPF_PART,
                        xml.replace("</opf:package>", &format!("{extra}</opf:package>")),
                    );
                    assert!(PreviewRemoval::plan(&bad).is_err(), "input: {extra}");
                    assert!(
                        plan.verify_values(
                            CONTENT_HPF_PART,
                            bad.read_entry(CONTENT_HPF_PART).unwrap()
                        )
                        .is_err(),
                        "final: {extra}"
                    );
                }
            }
        }
        assert!(!plan.matches_removed_value("Preview%FFPrvText.txt").unwrap());
        for alias in [
            format!("{}Preview%2FPrvText.txt", "%2e%2f".repeat(100)),
            format!("{}Preview%2FPrvText.txt", "Other%2f%2e%2e%2f".repeat(100)),
        ] {
            assert!(plan.matches_removed_value(&alias).is_err());
        }
        assert!(!plan
            .matches_removed_value(&format!("https://example.org/%20{}", "x".repeat(10000)))
            .unwrap());
        for value in [
            "PreviewExtra%2FPrvText.txt",
            "Preview%2Fother.txt",
            "%2550review/PrvText.txt",
            "Preview%2GPrvText.txt",
            "https://example.org/Preview%2FPrvText.txt",
        ] {
            assert!(!plan.matches_removed_value(value).unwrap(), "{value}");
        }
    }

    // 이것을 실패시키는 것: 삭제 경로 비교용 percent 해석으로 무관한 값이나 원래 manifest id를 거부/누락.
    #[test]
    fn percent_values_keep_literal_ids_and_unrelated_metadata() {
        let mut p = preview();
        let xml = p.read_text_entry(CONTENT_HPF_PART).unwrap();
        p.replace_text_entry(CONTENT_HPF_PART, xml.replace("</opf:manifest>", "<opf:item id='literal%FF' href='Preview/PrvText.txt' media-type='text/plain'/></opf:manifest>")
            .replace("</opf:package>", "<extra value='unrelated%FF'>unrelated%FF</extra></opf:package>"));
        let plan = PreviewRemoval::plan(&p).unwrap();
        let mut bad = p.clone();
        let xml = bad.read_text_entry(CONTENT_HPF_PART).unwrap();
        bad.replace_text_entry(CONTENT_HPF_PART, xml.replace("unrelated%FF", "literal%FF"));
        assert!(PreviewRemoval::plan(&bad).is_err());
        assert!(plan
            .verify_values(CONTENT_HPF_PART, bad.read_entry(CONTENT_HPF_PART).unwrap())
            .is_err());
        plan.apply(&mut p, &mut OutputChanges::default()).unwrap();
        assert!(p.read_text_entry(CONTENT_HPF_PART).unwrap().contains("unrelated%FF"));
        plan.verify_final(&p.write().unwrap()).unwrap();
    }

    // 이것을 실패시키는 것: `#` fragment 의 percent 해석을 거부 비교에서 빼는 것, 해석 바이트를 lossy 문자열로 비교하는 것.
    #[test]
    fn percent_fragment_naming_a_removed_id_is_refused() {
        let mut p = preview();
        let xml = p.read_text_entry(CONTENT_HPF_PART).unwrap();
        p.replace_text_entry(CONTENT_HPF_PART, xml.replace("</opf:manifest>", "<opf:item id='pv' href='Preview/PrvText.txt' media-type='text/plain'/></opf:manifest>")
            .replace("</opf:package>", "<opf:meta refines='#%70v'>x</opf:meta></opf:package>"));
        let mut control = p.clone();
        let xml = control.read_text_entry(CONTENT_HPF_PART).unwrap();
        control.replace_text_entry(CONTENT_HPF_PART, xml.replace("#%70v", "#%71v"));
        PreviewRemoval::plan(&control).unwrap();
        // An invalid escape must not match an id that holds U+FFFD.
        let mut lossy = preview();
        let xml = lossy.read_text_entry(CONTENT_HPF_PART).unwrap();
        lossy.replace_text_entry(CONTENT_HPF_PART, xml.replace("</opf:manifest>", "<opf:item id='p\u{FFFD}v' href='Preview/PrvText.txt' media-type='text/plain'/></opf:manifest>")
            .replace("</opf:package>", "<opf:meta refines='#p%FFv'>x</opf:meta></opf:package>"));
        PreviewRemoval::plan(&lossy).unwrap();
        assert!(PreviewRemoval::plan(&p).is_err());
    }

    // 이것을 실패시키는 것: percent 디코드를 ZIP 조회나 실제 삭제 키로 사용.
    #[test]
    fn percent_spelling_does_not_authorize_zip_lookup_or_deletion() {
        for path in ["Preview%2FPrvText.txt", "%50review/PrvText.txt"] {
            let mut p = preview();
            add(&mut p, path, b"literal percent entry");
            let plan = PreviewRemoval::plan(&p).unwrap();
            assert!(!plan.deleted.iter().any(|p| p == path));
            plan.apply(&mut p, &mut OutputChanges::default()).unwrap();
            assert_eq!(p.read_entry(path).unwrap(), b"literal percent entry");
            assert!(PAYLOADS.iter().all(|p| plan.deleted.iter().any(|d| d == p)));
            assert!(PAYLOADS.iter().all(|path| p.read_entry(path).is_err()));
            plan.verify_final(&p.write().unwrap()).unwrap();
        }
    }

    // 이것을 실패시키는 것: header 정규화 분기 고정 또는 뒤 구역의 raw 편집/실제 값 변경 제거.
    #[test]
    fn fill_normalizes_header_then_patches_the_next_section_raw() {
        let mut p = preview();
        let header = p.read_text_entry("Contents/header.xml").unwrap();
        p.replace_text_entry(
            "Contents/header.xml",
            header.replace("<hh:charPr id=\"0\"", "<hh:charPr id=\"7\""),
        );
        let base = p.write().unwrap();
        assert!(crate::HwpxDecoder::decode(&base).unwrap().requires_header_normalization());
        super::super::OUTPUT_READS.with(|count| count.set(0));
        let filled = crate::HwpxFiller::fill_with_codec_diagnostics(
            &base,
            &[("first".into(), "normalized".into()), ("second".into(), "raw".into())]
                .into_iter()
                .collect(),
        )
        .unwrap();
        let after = crate::HwpxDecoder::decode(&filled.value.bytes).unwrap();
        assert!(!after.requires_header_normalization());
        assert_eq!(filled.value.filled.len(), 2);
        assert!(filled.encode_warnings.iter().all(|w| w.code() != "PREVIEW_REMOVED"));
        assert_eq!(
            crate::patch::PRESERVING_BRANCHES.with(|branches| branches.borrow().clone()),
            ["reencode", "raw"]
        );
        // Re-encoding already dropped Preview, so the later raw edit owes no
        // Preview-removal warning or additional final Preview verification.
        assert_eq!(super::super::OUTPUT_READS.with(|count| count.get()), 0);
        let fields = hwpforge_core::field_inventory::FieldInventory::from_sections(
            after.document.sections(),
        );
        assert_eq!(fields.fields()[0].current(), "normalized");
        assert_eq!(fields.fields()[1].current(), "raw");
    }

    // 이것을 실패시키는 것: OPF 내용 item에 파일명/media-type 허용 목록을 다시 적용.
    #[test]
    fn content_items_keep_their_role_for_arbitrary_names_and_media() {
        for path in ["Contents/memoExtended.xml", "Contents/unknown.rdf", "Other/data.part"] {
            for media in ["application/xml", "TEXT/XML", "application/custom+xml", "unknown/type"] {
                let mut p = preview();
                let xml = p.read_text_entry(CONTENT_HPF_PART).unwrap();
                p.replace_text_entry(CONTENT_HPF_PART, xml.replace("</opf:manifest>", &format!("<opf:item id='memoextended' href='{path}' media-type='{media}'/></opf:manifest>")));
                add(&mut p, path, b"<content/>");
                let base = p.write().unwrap();
                let filled = crate::HwpxFiller::fill_with_codec_diagnostics(
                    &base,
                    &[("first".into(), "new".into())].into_iter().collect(),
                )
                .unwrap();
                assert_eq!(
                    filled.encode_warnings.iter().filter(|w| w.code() == "PREVIEW_REMOVED").count(),
                    1
                );
                let after = RawPackage::read(&filled.value.bytes).unwrap();
                assert_eq!(after.read_entry(path).unwrap(), b"<content/>");
                assert!(PAYLOADS.iter().all(|p| after.read_entry(p).is_err()));
            }
        }
        let mut p = preview();
        let xml = p.read_text_entry(CONTENT_HPF_PART).unwrap();
        p.replace_text_entry(CONTENT_HPF_PART, xml.replace("</opf:manifest>", "<opf:item id='unknown' href='Preview/PrvText.txt' media-type='unknown/type'/></opf:manifest>"));
        assert!(PreviewRemoval::plan(&p).is_err());
    }

    // 이것을 실패시키는 것: META-INF 연결을 경로 비교 대신 대소문자 그대로 판정.
    #[test]
    fn linked_metadata_uses_package_path_comparison() {
        for path in ["META-INF/extra.part", "meta-inf/extra.part"] {
            let mut p = preview();
            let xml = p.read_text_entry(CONTENT_HPF_PART).unwrap();
            p.replace_text_entry(CONTENT_HPF_PART, xml.replace("</opf:manifest>", &format!("<opf:item id='metadata' href='{path}' media-type='application/xml'/></opf:manifest>")));
            add(&mut p, path, b"<extra>Preview/PrvText.txt</extra>");
            assert!(PreviewRemoval::plan(&p).is_err());
            p.replace_text_entry(path, "<extra>unrelated</extra>".into());
            let plan = PreviewRemoval::plan(&p).unwrap();
            plan.apply(&mut p, &mut OutputChanges::default()).unwrap();
            plan.verify_final(&p.write().unwrap()).unwrap();
        }
    }

    // 이것을 실패시키는 것: OCF rootfile에 OPF 내용 item의 허용 규칙을 적용하거나 META-INF rootfile 스캔 생략.
    #[test]
    fn rootfiles_distinguish_content_preview_and_metadata() {
        for path in ["Other/alt.opf", "META-INF/container.rdf", "META-INF/extra.part"] {
            let mut p = preview();
            let container = p.read_text_entry(CONTAINER_PART).unwrap();
            let media = if path.ends_with(".part") {
                "unknown/type"
            } else if path.ends_with(".rdf") {
                "application/rdf+xml"
            } else {
                "application/oebps-package+xml"
            };
            p.replace_text_entry(
                CONTAINER_PART,
                container.replace(
                    "</ocf:rootfiles>",
                    &format!(
                        "<ocf:rootfile full-path='{path}' media-type='{media}'/></ocf:rootfiles>"
                    ),
                ),
            );
            add(&mut p, path, b"<extra>unrelated</extra>");
            if path.starts_with("META-INF/") {
                let base = p.write().unwrap();
                let filled = crate::HwpxFiller::fill(
                    &base,
                    &[("first".into(), "new".into())].into_iter().collect(),
                )
                .unwrap();
                let after = RawPackage::read(&filled.bytes).unwrap();
                assert_eq!(after.read_entry(path).unwrap(), b"<extra>unrelated</extra>");
                assert!(PAYLOADS.iter().all(|payload| after.read_entry(payload).is_err()));
            }
            p.replace_text_entry(path, format!("<package xmlns='{OPF}'><manifest><item href='Preview/PrvText.txt' media-type='text/plain'/></manifest></package>"));
            let base = p.write().unwrap();
            let error = crate::HwpxFiller::fill(
                &base,
                &[("first".into(), "new".into())].into_iter().collect(),
            )
            .unwrap_err()
            .to_string();
            assert!(
                error.contains(if path.starts_with("META-INF/") {
                    "surviving descriptor"
                } else {
                    "unsupported rootfile target"
                }) || error.contains("unresolved descriptor attribute"),
                "{error}"
            );
        }
    }

    // 이것을 실패시키는 것: 긴 descriptor 경로의 거부를 입력 또는 최종 검증에서 누락.
    #[test]
    fn overlong_descriptor_paths_are_refused_before_and_after_edit() {
        let mut p = preview();
        let plan = PreviewRemoval::plan(&p).unwrap();
        let long = format!("{}Preview/PrvText.txt", "a/../".repeat(100));
        let xml = p.read_text_entry(CONTENT_HPF_PART).unwrap();
        p.replace_text_entry(
            CONTENT_HPF_PART,
            xml.replace("</opf:package>", &format!("<extra>{long}</extra></opf:package>")),
        );
        assert!(PreviewRemoval::plan(&p).is_err());
        assert!(plan
            .verify_values(CONTENT_HPF_PART, p.read_entry(CONTENT_HPF_PART).unwrap())
            .is_err());
    }

    // 이것을 실패시키는 것: 비교 길이 상한에서 삭제 item의 긴 id를 제외.
    #[test]
    fn comparison_bound_includes_manifest_ids_and_fragments() {
        let mut p = preview();
        let id = "p".repeat(512);
        let xml = p.read_text_entry(CONTENT_HPF_PART).unwrap();
        p.replace_text_entry(CONTENT_HPF_PART, xml.replace("</opf:manifest>", &format!("<opf:item id='{id}' href='Preview/PrvText.txt' media-type='text/plain'/></opf:manifest>")));
        let plan = PreviewRemoval::plan(&p).unwrap();
        assert!(plan.matches_removed_value(&id).unwrap());
        assert!(plan.matches_removed_value(&format!("#{id}")).unwrap());
        let xml = p.read_text_entry(CONTENT_HPF_PART).unwrap();
        p.replace_text_entry(
            CONTENT_HPF_PART,
            xml.replace("</opf:package>", &format!("<extra>#{id}</extra></opf:package>")),
        );
        assert!(PreviewRemoval::plan(&p).is_err());
    }

    // 이것을 실패시키는 것: 자식 경계 검사 생략, 오류 맥락 변경 또는 정책 오류 이중 감싸기.
    #[test]
    fn mixed_content_segments_and_descriptor_errors_are_checked() {
        for text in [
            "Preview/PrvText.txt<x/>tail",
            "head<x/>Preview/PrvText.txt",
            "Preview/Prv<!-- comment -->Text.txt",
            "Preview/Prv<?split value?>Text.txt",
            "&undefined;",
        ] {
            let mut p = preview();
            let xml = p.read_text_entry(CONTENT_HPF_PART).unwrap();
            p.replace_text_entry(
                CONTENT_HPF_PART,
                xml.replace("</opf:package>", &format!("<extra>{text}</extra></opf:package>")),
            );
            let error = PreviewRemoval::plan(&p).err().unwrap().to_string();
            assert!(error.contains("입력 정책 거부(지원하지 않는 Preview 구성)"), "{error}");
            assert_eq!(error.matches("입력 정책 거부").count(), 1, "{error}");
            assert!(!error.contains("paragraph reference"), "{error}");
            if text == "&undefined;" {
                assert!(error.contains(CONTENT_HPF_PART), "{error}");
            }
        }
        let mut p = preview();
        let plan = PreviewRemoval::plan(&p).unwrap();
        plan.apply(&mut p, &mut OutputChanges::default()).unwrap();
        let xml = p.read_text_entry(CONTENT_HPF_PART).unwrap();
        p.replace_text_entry(
            CONTENT_HPF_PART,
            xml.replace("</opf:package>", "<extra>&undefined;</extra></opf:package>"),
        );
        let error = plan.verify_final(&p.write().unwrap()).unwrap_err().to_string();
        assert!(error.contains(CONTENT_HPF_PART), "{error}");
        assert_eq!(error.matches("입력 정책 거부").count(), 1, "{error}");
        assert!(!error.contains("paragraph reference"), "{error}");
    }

    // 이것을 실패시키는 것: 최종 descriptor 대조 생략 또는 재인코드된 descriptor 전체를 허가.
    #[test]
    fn fill_rejects_descriptor_changes_after_preview_removal() {
        let original = preview();
        let base = original.write().unwrap();
        let plan = PreviewRemoval::plan(&original).unwrap();
        let first_decoded = crate::HwpxDecoder::decode(&base).unwrap();
        let mut first_document = first_decoded.document.clone();
        let first_fields = hwpforge_core::field_inventory::FieldInventory::from_sections(
            first_decoded.document.sections(),
        );
        assert!(hwpforge_core::field_inventory::FieldInventory::replace_values(
            first_document.sections_mut(),
            &[(&first_fields.fields()[0], "raw")]
        ));
        let preservation = crate::HwpxPatcher::export_section_preservation(
            &base,
            0,
            &first_decoded.document.sections()[0],
        )
        .unwrap();
        let mut changes = OutputChanges::default();
        let first = crate::HwpxPatcher::patch_section_preserving_with_changes(
            &base,
            0,
            &first_document.sections()[0],
            None,
            Some(&preservation),
            Some(&mut changes),
        )
        .unwrap();
        assert_eq!(
            first.encode_warnings.iter().filter(|w| w.code() == "PREVIEW_REMOVED").count(),
            1
        );
        let bytes = first.value;
        let raw_result = RawPackage::read(&bytes).unwrap();
        assert!(PAYLOADS.iter().all(|path| raw_result.read_entry(path).is_err()));
        let decoded = crate::HwpxDecoder::decode(&bytes).unwrap();
        let mut document = decoded.document.clone();
        let fields = hwpforge_core::field_inventory::FieldInventory::from_sections(
            decoded.document.sections(),
        );
        assert!(hwpforge_core::field_inventory::FieldInventory::replace_values(
            document.sections_mut(),
            &[(&fields.fields()[1], "fallback")]
        ));
        let fallback = crate::HwpxEncoder::encode_with_base(
            &bytes,
            &document.clone().validate().unwrap(),
            &decoded.style_store,
            &decoded.image_store,
            Default::default(),
        )
        .unwrap();
        changes.rewritten.extend(OutputChanges::reencoded(document.sections()).rewritten);
        changes.rewritten.sort();
        changes.rewritten.dedup();
        // General ZIP declarations allow the rewrite; the descriptor byte
        // contract is the remaining barrier before fill can return these bytes.
        original.verify_output_with(&base, &fallback.bytes, &changes).unwrap();
        let outcome = crate::WithCodecWarnings::new(
            crate::FillOutcome { bytes: fallback.bytes, filled: Vec::new() },
            Vec::new(),
            Vec::new(),
        );
        assert!(
            crate::fill::finish_fill(&original, &base, &mut changes, Some(&plan), outcome).is_err()
        );
    }

    // 이것을 실패시키는 것: delete의 codec wrapper에서 bytes 또는 encode 경고를 잘못 전달.
    #[test]
    fn generated_delete_keeps_existing_bytes_and_empty_encode_warnings() {
        let mut document = Document::new();
        document.add_section(Section::with_paragraphs(
            (0..3)
                .map(|i| {
                    Paragraph::with_runs(
                        vec![Run::text(format!("paragraph {i}"), CharShapeIndex::new(0))],
                        ParaShapeIndex::new(0),
                    )
                })
                .collect(),
            PageSettings::a4(),
        ));
        let base = crate::HwpxEncoder::encode(
            &document.validate().unwrap(),
            &crate::style_store_for_preset("default").unwrap(),
            &Default::default(),
        )
        .unwrap();
        let targets = [crate::ParagraphLocator { section: 0, index: 1 }];
        let old = crate::HwpxStructuralEditor::delete_paragraphs(&base, &targets).unwrap();
        let new =
            crate::HwpxStructuralEditor::delete_paragraphs_with_codec_diagnostics(&base, &targets)
                .unwrap();
        assert_eq!(new.value, old);
        assert!(new.encode_warnings.is_empty());
    }

    // 이것을 실패시키는 것: 삭제 잔존·선언 검사 제거, 빈 디렉터리 거부 또는 내용 있는 디렉터리 허가.
    #[test]
    fn deletion_declarations_are_exact() {
        let p = preview();
        let base = p.write().unwrap();
        let mut c = OutputChanges { deleted: vec![PAYLOADS[0].into()], ..Default::default() };
        assert!(p.verify_output_with(&base, &base, &c).is_err());
        let mut after = p.clone();
        after.remove_entries_exact(&c.deleted);
        let output = after.write().unwrap();
        p.verify_output_with(&base, &output, &c).unwrap();
        c.deleted.push(PAYLOADS[0].into());
        assert!(p.verify_output_with(&base, &output, &c).is_err());
        c.deleted = vec!["missing".into()];
        assert!(p.verify_output_with(&base, &base, &c).is_err());
        c.deleted = vec![PAYLOADS[0].into()];
        c.rewritten = c.deleted.clone();
        assert!(p.verify_output_with(&base, &output, &c).is_err());
        let mut directory = p.clone();
        add(&mut directory, "Preview/", b"");
        let plan = PreviewRemoval::plan(&directory).unwrap();
        assert!(plan.deleted.contains(&"Preview/".into()));
        plan.apply(&mut directory, &mut OutputChanges::default()).unwrap();
        assert!(directory.read_entry("Preview/").is_err());
        let mut directory = p.clone();
        add(&mut directory, "Preview/", b"content");
        assert!(PreviewRemoval::plan(&directory).is_err());
        let mut opaque = generated();
        add(&mut opaque, "asset", b"x");
        add(&mut opaque, "asset/", b"");
        let base = opaque.write().unwrap();
        let original = RawPackage::read(&base).unwrap();
        let deleted = vec!["asset".into(), "asset/".into()];
        let mut output = original.clone();
        output.remove_entries_exact(&deleted);
        assert!(original
            .verify_output_with(
                &base,
                &output.write().unwrap(),
                &OutputChanges { deleted, ..Default::default() }
            )
            .is_err());
    }

    // 이것을 실패시키는 것: 적용·최종 값/원문 대조 생략, BOM offset 이동, UTF-16 수정 허가, 경고 분류/정렬 변경.
    #[test]
    fn removal_preserves_descriptor_bytes_and_warns_once() {
        let mut p = preview();
        let original = p.clone();
        let plan = PreviewRemoval::plan(&p).unwrap();
        let xml = original.read_entry("META-INF/container.xml").unwrap();
        let edit = plan.descriptors.iter().find(|e| e.path == "META-INF/container.xml").unwrap();
        assert_eq!(edit.removals.len(), 1);
        assert_eq!(
            edit.removals[0].start,
            std::str::from_utf8(xml).unwrap().find("<ocf:rootfile full-path='Preview").unwrap()
        );
        let mut changes = OutputChanges::default();
        plan.apply(&mut p, &mut changes).unwrap();
        let bytes = p.write().unwrap();
        original.verify_output_with(&original.write().unwrap(), &bytes, &changes).unwrap();
        let warning = plan.verify_final(&bytes).unwrap().unwrap();
        assert_eq!(warning.code(), "PREVIEW_REMOVED");
        assert!(!warning.is_semantic_loss());
        assert_eq!(warning.part(), None);
        assert_eq!(warning.location(), None);
        assert_eq!(
            warning.message(),
            "removed preview parts after editing: Preview/PrvImage.png, Preview/PrvText.txt"
        );
        assert_eq!(warning.to_string(), warning.message());
        for (from, to) in [("\u{feff}", ""), ("</ocf:container>", " </ocf:container>")] {
            let mut corrupt = p.clone();
            // read_text_entry normalizes BOM, so use the physical bytes here.
            let raw = std::str::from_utf8(p.read_entry("META-INF/container.xml").unwrap()).unwrap();
            corrupt.replace_text_entry("META-INF/container.xml", raw.replace(from, to));
            assert!(plan.verify_final(&corrupt.write().unwrap()).is_err());
        }
        let mut corrupt = p.clone();
        corrupt.replace_text_entry(
            "Contents/content.hpf",
            format!("{} ", p.read_text_entry("Contents/content.hpf").unwrap()),
        );
        assert!(plan.verify_final(&corrupt.write().unwrap()).is_err());
        fn utf16(bytes: &[u8]) -> Vec<u8> {
            let text = std::str::from_utf8(bytes)
                .unwrap()
                .trim_start_matches('\u{feff}')
                .replace("UTF-8", "UTF-16");
            [vec![0xff, 0xfe], text.encode_utf16().flat_map(u16::to_le_bytes).collect()].concat()
        }
        let mut wide = original.clone();
        let i = wide.index_by_path["Contents/content.hpf"];
        wide.entries[i].bytes = utf16(&wide.entries[i].bytes);
        let wide_plan = PreviewRemoval::plan(&wide).unwrap();
        wide_plan.apply(&mut wide, &mut OutputChanges::default()).unwrap();
        wide_plan.verify_final(&wide.write().unwrap()).unwrap();
        let mut wide = original.clone();
        let i = wide.index_by_path["META-INF/container.xml"];
        wide.entries[i].bytes = utf16(&wide.entries[i].bytes);
        assert!(PreviewRemoval::plan(&wide).is_err());
        // A faulty plan must not certify its own surviving descriptor reference.
        let mut faulty = plan;
        let edit =
            faulty.descriptors.iter_mut().find(|e| e.path == "Contents/content.hpf").unwrap();
        let xml = String::from_utf8(edit.original.clone())
            .unwrap()
            .replace("</opf:package>", "<extra>Preview/PrvText.txt</extra></opf:package>");
        edit.original = xml.as_bytes().to_vec();
        p.replace_text_entry("Contents/content.hpf", xml);
        assert!(faulty.verify_final(&p.write().unwrap()).is_err());
    }

    // 이것을 실패시키는 것: 값 채널·namespace/확장 속성 검사·혼합 문자 데이터 합류 제거, XML 이중 해석.
    // 이것을 실패시키는 것: XML media-type 연결 누락, 무관한 URI/자산 오인, Preview 없는 입력까지 검사.
    #[test]
    fn descriptor_values_refuse_only_exact_references() {
        for value in [
            "Preview/PrvText.txt",
            "Preview/Prv&#84;ext.txt",
            "preview/./PrvText.txt",
            "Preview\\PrvText.txt",
            "Preview/Prv<![CDATA[Text]]>.txt",
            "Preview/Prv<!-- split -->Text.txt",
            "Preview/<child>PrvText</child>.txt",
        ] {
            let mut p = preview();
            let xml = p.read_text_entry("Contents/content.hpf").unwrap();
            p.replace_text_entry(
                "Contents/content.hpf",
                xml.replace("</opf:package>", &format!("<extra>{value}</extra></opf:package>")),
            );
            assert!(PreviewRemoval::plan(&p).is_err(), "{value}");
        }
        for suffix in [
            "<extra value='Preview/PrvText.txt'/>",
            "<extra xml:base='any'/>",
            "<extra>&undefined;</extra>",
            "<extra xmlns:a='urn:same' xmlns:b='urn:same' a:value='first' b:value='second'/>",
        ] {
            let mut p = preview();
            let xml = p.read_text_entry("Contents/content.hpf").unwrap();
            p.replace_text_entry(
                "Contents/content.hpf",
                xml.replace("</opf:package>", &format!("{suffix}</opf:package>")),
            );
            assert!(PreviewRemoval::plan(&p).is_err(), "{suffix}");
        }
        for path in ["Preview/other.bin", "preview/PrvText.txt", "Preview/PrvImage.jpg"] {
            let mut p = preview();
            add(&mut p, path, b"x");
            assert!(PreviewRemoval::plan(&p).is_err());
        }
        let mut chart = preview();
        let xml = chart.read_text_entry("Contents/content.hpf").unwrap();
        chart.replace_text_entry("Contents/content.hpf", xml.replace("</opf:manifest>", "<opf:item id='chart-data' href='Chart/chart.xml' media-type='application/xml'/></opf:manifest>"));
        add(&mut chart, "Chart/chart.xml", b"<asset/>");
        assert!(PreviewRemoval::plan(&chart).is_ok());
        for media_type in ["application/xml; charset=UTF-8", "TEXT/XML", "application/rdf+xml"] {
            let mut linked = preview();
            let xml = linked.read_text_entry("Contents/content.hpf").unwrap();
            linked.replace_text_entry("Contents/content.hpf", xml.replace("</opf:manifest>", &format!("<opf:item id='metadata' href='META-INF/extra.part' media-type='{media_type}'/></opf:manifest>")));
            add(&mut linked, "META-INF/extra.part", b"<extra>Preview/PrvText.txt</extra>");
            assert!(PreviewRemoval::plan(&linked).is_err());
            linked.replace_text_entry("META-INF/extra.part", "<extra>unrelated</extra>".into());
            let plan = PreviewRemoval::plan(&linked).unwrap();
            plan.apply(&mut linked, &mut OutputChanges::default()).unwrap();
            plan.verify_final(&linked.write().unwrap()).unwrap();
        }
        let mut dangling = preview();
        dangling.remove_entries_exact(&[PAYLOADS[1].into()]);
        let xml = dangling.read_text_entry("META-INF/container.xml").unwrap();
        dangling.replace_text_entry("META-INF/container.xml", xml.replace("</ocf:rootfiles>", "<ocf:rootfile full-path='Preview/PrvImage.png' media-type='image/png'/></ocf:rootfiles>"));
        let plan = PreviewRemoval::plan(&dangling).unwrap();
        assert_eq!(plan.deleted, [PAYLOADS[0]]);
        let mut without = generated();
        add(&mut without, "META-INF/container.rdf", b"unparsed");
        assert!(PreviewRemoval::plan(&without).unwrap().deleted.is_empty());
        let mut p = preview();
        add(&mut p, "PreviewExtra/keep.bin", b"keep");
        let xml = p.read_text_entry("Contents/content.hpf").unwrap();
        p.replace_text_entry("Contents/content.hpf", xml.replace("</opf:package>", "<fieldBegin><fieldBegin/></fieldBegin><extra value='Preview/Prv&amp;#84;ext.txt' empty='' uri='https://example.org' href='https://example.org/description.xml'>Preview/PrvText.txt description</extra></opf:package>"));
        assert!(PreviewRemoval::plan(&p).is_ok());
        let mut p = preview();
        let xml = p.read_text_entry("Contents/content.hpf").unwrap();
        p.replace_text_entry("Contents/content.hpf", xml.replace("</opf:manifest>", "<opf:item id='chart-data' href='BinData/chart.xml' media-type='application/xml'/></opf:manifest>"));
        add(&mut p, "BinData/chart.xml", b"<asset/>");
        assert!(PreviewRemoval::plan(&p).is_ok());
        let xml = p.read_text_entry("Contents/content.hpf").unwrap();
        p.replace_text_entry(
            "Contents/content.hpf",
            xml.replace("</opf:package>", "<extra unbound:value='unrelated'/></opf:package>"),
        );
        assert!(PreviewRemoval::plan(&p).is_err());
    }

    // 이것을 실패시키는 것: 삭제 요소 선택·ID 참조 검증 또는 주 rootfile 생존 검사 제거.
    #[test]
    fn selected_definitions_and_spine_are_unambiguous() {
        let item = "<opf:item id='preview' href='Preview/PrvText.txt' media-type='text/plain'/>";
        let spine = "<opf:itemref idref='preview'/>";
        let mut p = preview();
        let xml = p.read_text_entry("Contents/content.hpf").unwrap();
        let xml = xml
            .replace("</opf:manifest>", &format!("{item}</opf:manifest>"))
            .replace("</opf:spine>", &format!("{spine}</opf:spine>"));
        p.replace_text_entry("Contents/content.hpf", xml.clone());
        let plan = PreviewRemoval::plan(&p).unwrap();
        let mut changed = p.clone();
        plan.apply(&mut changed, &mut OutputChanges::default()).unwrap();
        assert!(!changed.read_text_entry("Contents/content.hpf").unwrap().contains("'preview'"));
        for (from, to) in [
            (item.to_string(), item.replace("text/plain", "image/png")),
            (item.to_string(), item.replace("/>", " fallback='other'/>")),
            (item.to_string(), item.replace("/>", " isEmbeded='0'/>")),
            (item.to_string(), item.replace("/>", "><!--keep--></opf:item>")),
            (item.to_string(), item.replace("/>", "><extra/></opf:item>")),
            (item.to_string(), format!("{item}{item}")),
            (item.to_string(), item.replace("PrvText.txt", "PrvText.txt?query")),
            (item.to_string(), item.replace("PrvText.txt", "Prv%54ext.txt")),
            (spine.to_string(), spine.replace("/>", " linear='no'/>")),
            (spine.to_string(), format!("{spine}{spine}")),
            ("</opf:package>".into(), "<extra value='#preview'/></opf:package>".into()),
            ("</opf:package>".into(), "<extra>preview</extra></opf:package>".into()),
        ] {
            let mut bad = p.clone();
            bad.replace_text_entry("Contents/content.hpf", xml.replace(&from, &to));
            assert!(PreviewRemoval::plan(&bad).is_err(), "{to}");
        }
        let mut bad = p.clone();
        let container = bad.read_text_entry("META-INF/container.xml").unwrap();
        bad.replace_text_entry(
            "META-INF/container.xml",
            container.replace("Contents/content.hpf", "missing.hpf"),
        );
        assert!(PreviewRemoval::plan(&bad).is_err());
        let (_, source) = XmlSource::read("\u{feff}<root/>").unwrap();
        assert_eq!(source.elements()[1].outer(), 3..10);
    }

    // 이것을 실패시키는 것: fallback no-op 원본 반환 또는 변경 선언 제거, Preview 없는 결과에도 제거 경고 생성.
    #[test]
    fn fallback_before_raw_distinguishes_retained_and_lost_preview() {
        let base = preview().write().unwrap();
        let decoded = crate::HwpxDecoder::decode(&base).unwrap();
        for changed in [false, true] {
            let mut document = decoded.document.clone();
            if changed {
                let inventory = hwpforge_core::field_inventory::FieldInventory::from_sections(
                    decoded.document.sections(),
                );
                assert!(hwpforge_core::field_inventory::FieldInventory::replace_values(
                    document.sections_mut(),
                    &[(&inventory.fields()[0], "fallback")]
                ));
            }
            let fallback = crate::HwpxEncoder::encode_with_base(
                &base,
                &document.validate().unwrap(),
                &decoded.style_store,
                &decoded.image_store,
                Default::default(),
            )
            .unwrap();
            let package = RawPackage::read(&fallback.bytes).unwrap();
            assert_eq!(package.read_entry(PAYLOADS[0]).is_ok(), !changed);
            let result = crate::HwpxFiller::fill_with_codec_diagnostics(
                &fallback.bytes,
                &[("second".into(), "raw".into())].into_iter().collect(),
            )
            .unwrap();
            assert_eq!(
                result.encode_warnings.iter().filter(|w| w.code() == "PREVIEW_REMOVED").count(),
                usize::from(!changed)
            );
        }
        let mut legacy = preview();
        let header = legacy.read_text_entry("Contents/header.xml").unwrap();
        legacy.replace_text_entry(
            "Contents/header.xml",
            header.replace("<hh:charPr id=\"0\"", "<hh:charPr id=\"7\""),
        );
        let base = legacy.write().unwrap();
        let decoded = crate::HwpxDecoder::decode(&base).unwrap();
        assert!(decoded.requires_header_normalization());
        let mut document = decoded.document.clone();
        let inventory = hwpforge_core::field_inventory::FieldInventory::from_sections(
            decoded.document.sections(),
        );
        assert!(hwpforge_core::field_inventory::FieldInventory::replace_values(
            document.sections_mut(),
            &[(&inventory.fields()[0], "fallback")]
        ));
        let preservation = crate::HwpxPatcher::export_section_preservation(
            &base,
            0,
            &decoded.document.sections()[0],
        )
        .unwrap();
        let mut changes = OutputChanges::default();
        let first = crate::HwpxPatcher::patch_section_preserving_with_changes(
            &base,
            0,
            &document.sections()[0],
            None,
            Some(&preservation),
            Some(&mut changes),
        )
        .unwrap();
        assert!(changes.rewritten.contains(&"Contents/header.xml".into()));
        assert!(changes.rewritten.contains(&"Contents/section1.xml".into()));
        let final_result = crate::HwpxFiller::fill_with_codec_diagnostics(
            &first.value,
            &[("second".into(), "raw".into())].into_iter().collect(),
        )
        .unwrap();
        assert!(final_result.encode_warnings.iter().all(|w| w.code() != "PREVIEW_REMOVED"));
    }

    // 이것을 실패시키는 것: raw no-op 뒤 정리 순서 이동, fill 경고 합류 제거 또는 구역마다 전체 payload 복제.
    #[test]
    fn raw_patch_and_multisection_fill_remove_preview() {
        let mut p = preview();
        add(&mut p, "PreviewExtra/keep.bin", b"keep");
        add(&mut p, "BinData/large.bin", &vec![42; 1024 * 1024]);
        let base = p.write().unwrap();
        let decoded = crate::HwpxDecoder::decode(&base).unwrap();
        let exported = crate::HwpxPatcher::export_section_for_edit(&base, 0, false).unwrap();
        let noop = crate::HwpxPatcher::patch_section_preserving_with_codec_diagnostics(
            &base,
            0,
            &decoded.document.sections()[0],
            None,
            exported.exported.preservation.as_ref(),
        )
        .unwrap();
        assert_eq!(noop.value, base);
        assert!(noop.encode_warnings.is_empty());
        let values = [("first".into(), "new".into()), ("second".into(), "changed".into())]
            .into_iter()
            .collect();
        super::super::OUTPUT_READS.with(|count| count.set(0));
        super::super::PACKAGE_CLONES.with(|count| count.set(0));
        super::super::PACKAGE_IO.with(|count| count.set((0, 0)));
        let filled = crate::HwpxFiller::fill_with_codec_diagnostics(&base, &values).unwrap();
        assert_eq!(super::super::OUTPUT_READS.with(|count| count.get()), 1);
        assert_eq!(super::super::PACKAGE_CLONES.with(|count| count.get()), 0);
        assert_eq!(super::super::PACKAGE_IO.with(|count| count.get()), (5, 2));
        assert_eq!(
            filled.encode_warnings.iter().filter(|w| w.code() == "PREVIEW_REMOVED").count(),
            1
        );
        let result = RawPackage::read(&filled.value.bytes).unwrap();
        assert!(PAYLOADS.iter().all(|path| result.read_entry(path).is_err()));
        assert_eq!(result.read_entry("PreviewExtra/keep.bin").unwrap(), b"keep");
        let edited = crate::HwpxDecoder::decode(&filled.value.bytes).unwrap();
        super::super::OUTPUT_READS.with(|count| count.set(0));
        super::super::PACKAGE_CLONES.with(|count| count.set(0));
        super::super::PACKAGE_IO.with(|count| count.set((0, 0)));
        let patched = crate::HwpxPatcher::patch_section_preserving_with_codec_diagnostics(
            &base,
            0,
            &edited.document.sections()[0],
            None,
            exported.exported.preservation.as_ref(),
        )
        .unwrap();
        assert_eq!(super::super::OUTPUT_READS.with(|count| count.get()), 1);
        assert_eq!(super::super::PACKAGE_CLONES.with(|count| count.get()), 1);
        assert_eq!(super::super::PACKAGE_IO.with(|count| count.get()), (1, 1));
        assert_eq!(
            patched.encode_warnings.iter().filter(|w| w.code() == "PREVIEW_REMOVED").count(),
            1
        );
    }
}
