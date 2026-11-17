//! Resolve package reading order once, before Core paragraph paths are assigned.
//! HWPX hrefs are archive-relative (including `Contents/`), not OPF-relative.
use crate::encoder::package::{CONTAINER_RDF_PART, CONTENT_HPF_PART, HEADER_PART};
use std::collections::{BTreeMap, BTreeSet};

use quick_xml::{events::Event, name::ResolveResult, NsReader};

pub(crate) const OPF: &str = "http://www.idpf.org/2007/opf/";
pub(crate) const OCF: &str = "urn:oasis:names:tc:opendocument:xmlns:container";

#[derive(Debug, Clone, Default)]
pub(crate) struct PackageOrder {
    pub(crate) header_only_nonlinear: bool,
    pub(crate) script_auxiliary_order: bool,
    pub(crate) single_section_nonlinear_order: bool,
    /// Core section indices always index this list, also during cache analysis.
    pub sections: Vec<String>,
    /// Whether the entire container XML can be excluded from the global layout fingerprint.
    /// Unknown/missing rootfiles or unrepresented extensions prevent exclusion.
    pub container_excludable: bool,
    /// Whether the entire content XML can be excluded from the global layout fingerprint.
    /// Unrepresented extensions prevent exclusion even when reading order is resolved.
    pub content_excludable: bool,
    /// Decoder reports these as document-wide ContentLost; analysis fails closed.
    pub problems: Vec<String>,
    /// A content.hpf failure is already represented in package-order warnings.
    pub content_problem_reported: bool,
}

pub(crate) fn numbered_part(path: &str, prefix: &str) -> Option<usize> {
    path.strip_prefix(prefix)?.strip_suffix(".xml")?.parse().ok()
}

impl PackageOrder {
    pub fn resolve(paths: &[String], content: Option<&str>, container: Option<&str>) -> Self {
        let mut fallback: Vec<_> = paths
            .iter()
            .filter_map(|p| numbered_part(p, "Contents/section").map(|n| (n, p.clone())))
            .collect();
        fallback.sort();
        let mut result =
            Self { sections: fallback.into_iter().map(|(_, p)| p).collect(), ..Self::default() };
        match container
            .ok_or_else(|| "missing container.xml".to_string())
            .and_then(|xml| validate_container(xml, paths))
        {
            Ok(excludable) => result.container_excludable = excludable,
            Err(reason) => result.problems.push(format!("container: {reason}")),
        }
        let mut mode = SpineMode::default();
        match content
            .ok_or_else(|| "missing content.hpf".to_string())
            .and_then(|xml| resolve_spine_with_mode(xml, paths, &result.sections, &mut mode))
        {
            Ok((sections, excludable)) => {
                result.sections = sections;
                result.content_excludable = excludable;
            }
            Err(reason) => {
                // Retry the same resolver only for its typed failure cause. Public decode
                // still uses the original file-number fallback and warning.
                if mode.nonlinear || mode.known_script {
                    if let Some(xml) = content {
                        let mut narrow = SpineMode {
                            allow_header_nonlinear: true,
                            allow_known_scripts: true,
                            allow_single_section_nonlinear: true,
                            ..SpineMode::default()
                        };
                        if let Ok((sections, excludable)) =
                            resolve_spine_with_mode(xml, paths, &result.sections, &mut narrow)
                        {
                            if result.container_excludable
                                && excludable
                                && sections == result.sections
                            {
                                result.single_section_nonlinear_order =
                                    narrow.used_single_section_nonlinear;
                                result.script_auxiliary_order = !narrow
                                    .used_single_section_nonlinear
                                    && narrow.used_known_script;
                                result.header_only_nonlinear = !narrow
                                    .used_single_section_nonlinear
                                    && !narrow.used_known_script
                                    && narrow.used_header_nonlinear;
                            }
                        }
                    }
                }
                result.content_problem_reported = true;
                result.problems.push(format!("spine: {reason}; using file-number fallback"))
            }
        }
        result
    }
}

/// Bounded, namespace-aware pull traversal. No DTDs, unbound prefixes, multiple
/// roots or truncated XML can establish a supported package structure.
pub(crate) fn visit(
    xml: &str,
    mut element: impl FnMut(&[String], &str, &BTreeMap<String, String>) -> Result<(), String>,
) -> Result<(), String> {
    let mut reader = NsReader::from_str(xml);
    let mut path = Vec::new();
    let mut roots = 0;
    loop {
        let event = reader.read_event().map_err(|e| e.to_string())?;
        let empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(e) | Event::Empty(e) => {
                if path.is_empty() {
                    roots += 1;
                }
                if path.len() >= 64 || roots != 1 {
                    return Err("invalid XML depth/root count".into());
                }
                let (ns, name) = reader.resolver().resolve_element(e.name());
                let namespace = match ns {
                    ResolveResult::Bound(ns) => ns.as_ref().to_string(),
                    ResolveResult::Unbound => String::new(),
                    ResolveResult::Unknown(_) => return Err("unbound XML prefix".into()),
                };
                path.push(name.as_ref().to_string());
                let mut attrs = BTreeMap::new();
                for a in e.attributes() {
                    let a = a.map_err(|e| e.to_string())?;
                    let key = a.key.as_ref();
                    if key == "xmlns" || key.starts_with("xmlns:") {
                        continue;
                    }
                    attrs.insert(
                        key.to_string(),
                        a.normalized_value(quick_xml::XmlVersion::Explicit1_0)
                            .map_err(|e| e.to_string())?
                            .into_owned(),
                    );
                }
                element(&path, &namespace, &attrs)?;
                if empty {
                    path.pop();
                }
            }
            Event::End(_) => {
                path.pop();
            }
            Event::DocType(_) => return Err("DTD is unsupported".into()),
            Event::Eof => break,
            _ => {}
        }
    }
    if roots != 1 || !path.is_empty() {
        return Err("missing/unclosed XML root".into());
    }
    Ok(())
}

/// Qualified/conditional attributes can change selection. Only unqualified
/// additive attributes are ignored for order; their layout meaning stays unknown.
fn selection_attributes(attrs: &BTreeMap<String, String>) -> Result<(), String> {
    if attrs.keys().any(|k| {
        k.contains(':')
            || matches!(
                k.as_str(),
                "required-namespace"
                    | "required-modules"
                    | "fallback"
                    | "fallback-style"
                    | "requiredFeatures"
                    | "requiredExtensions"
                    | "systemLanguage"
            )
    }) {
        return Err("unsupported selection attributes".into());
    }
    Ok(())
}

fn validate_container(xml: &str, paths: &[String]) -> Result<bool, String> {
    let mut excludable = true;
    let mut packages = 0;
    let mut seen = BTreeSet::new();
    visit(xml, |path, ns, attrs| {
        if ns != OCF {
            return Err("unsupported container namespace".into());
        }
        selection_attributes(attrs)?;
        let names: Vec<_> = path.iter().map(String::as_str).collect();
        match names.as_slice() {
            ["container"] => {
                if attrs.get("version").is_some_and(|v| v != "1.0") {
                    return Err("unsupported container version".into());
                }
                excludable &= attrs.keys().all(|k| k == "version");
                Ok(())
            }
            ["container", "rootfiles"] => {
                excludable &= attrs.is_empty();
                Ok(())
            }
            ["container", "rootfiles", "rootfile"] => {
                let target = attrs.get("full-path").map(String::as_str).unwrap_or("");
                let media = attrs.get("media-type").map(String::as_str).unwrap_or("");
                excludable &=
                    attrs.keys().all(|k| matches!(k.as_str(), "full-path" | "media-type"));
                if !seen.insert(target.to_string()) || !paths.iter().any(|p| p == target) {
                    return Err(format!("invalid/missing rootfile {target}"));
                }
                match (target, media) {
                    (CONTENT_HPF_PART, "application/hwpml-package+xml") => packages += 1,
                    ("Preview/PrvText.txt", "text/plain")
                    | (CONTAINER_RDF_PART, "application/rdf+xml") => {}
                    _ => return Err(format!("unsupported rootfile {target} ({media})")),
                }
                Ok(())
            }
            _ => Err("unsupported container structure/attributes".into()),
        }
    })?;
    if packages != 1 {
        return Err("expected exactly one supported package rootfile".into());
    }
    Ok(excludable)
}

#[derive(Default)]
struct SpineMode {
    allow_header_nonlinear: bool,
    allow_known_scripts: bool,
    allow_single_section_nonlinear: bool,
    nonlinear: bool,
    known_script: bool,
    used_header_nonlinear: bool,
    used_known_script: bool,
    used_single_section_nonlinear: bool,
}

// XML 속성 정규화 뒤 값으로 비교한다(문자 참조로 쓴 공백도 같은 값).
fn is_known_script_auxiliary(target: &str, media: &str) -> bool {
    matches!(target, "Scripts/headerScripts.js" | "Scripts/sourceScripts.js")
        && media == "application/x-javascript ;charset=utf-16"
}

#[cfg(test)]
thread_local! { static RESOLVES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

#[cfg(test)]
fn resolve_spine(
    xml: &str,
    paths: &[String],
    fallback: &[String],
) -> Result<(Vec<String>, bool), String> {
    resolve_spine_with_mode(xml, paths, fallback, &mut SpineMode::default())
}

fn resolve_spine_with_mode(
    xml: &str,
    paths: &[String],
    fallback: &[String],
    mode: &mut SpineMode,
) -> Result<(Vec<String>, bool), String> {
    #[cfg(test)]
    RESOLVES.with(|count| count.set(count.get() + 1));
    let mut excludable = true;
    let mut manifest: BTreeMap<String, Option<(String, String)>> = BTreeMap::new();
    let mut script_hrefs = BTreeMap::<String, usize>::new();
    let mut refs = Vec::new();
    let mut spines = 0;
    let mut manifests = 0;
    visit(xml, |path, ns, attrs| {
        let names: Vec<_> = path.iter().map(String::as_str).collect();
        let get = |key| attrs.get(key).cloned().ok_or_else(|| format!("missing {key}"));
        match names.as_slice() {
            ["package"]
            | ["package", "manifest"]
            | ["package", "spine"]
            | ["package", "manifest", "item"]
            | ["package", "spine", "itemref"] => selection_attributes(attrs)?,
            _ => {}
        }
        match names.as_slice() {
            ["package"]
            | ["package", "manifest"]
            | ["package", "spine"]
            | ["package", "manifest", "item"]
            | ["package", "spine", "itemref"]
                if ns != OPF =>
            {
                return Err("unsupported OPF namespace".into())
            }
            ["package"] => {
                excludable &= attrs
                    .keys()
                    .all(|k| matches!(k.as_str(), "version" | "unique-identifier" | "id"));
            }
            ["package", "manifest"] => {
                excludable &= attrs.is_empty();
                manifests += 1;
            }
            ["package", "spine"] => {
                excludable &= attrs.is_empty();
                spines += 1;
            }
            ["package", "manifest", "item"] => {
                excludable &= attrs
                    .keys()
                    .all(|k| matches!(k.as_str(), "id" | "href" | "media-type" | "isEmbeded"));
                let id = get("id")?;
                let value = (get("href")?, get("media-type")?);
                if matches!(
                    value.0.as_str(),
                    "Scripts/headerScripts.js" | "Scripts/sourceScripts.js"
                ) {
                    *script_hrefs.entry(value.0.clone()).or_default() += 1;
                }
                manifest.entry(id).and_modify(|v| *v = None).or_insert(Some(value));
            }
            ["package", "spine", "itemref"] => {
                excludable &= attrs.keys().all(|k| matches!(k.as_str(), "idref" | "linear"));
                let nonlinear = attrs.get("linear").is_some_and(|v| v != "yes");
                if nonlinear {
                    mode.nonlinear = true;
                    if !(mode.allow_header_nonlinear
                        || mode.allow_known_scripts
                        || mode.allow_single_section_nonlinear)
                    {
                        return Err("unsupported spine itemref attributes".into());
                    }
                    if attrs.get("linear").map(String::as_str) != Some("no") {
                        return Err("unsupported spine itemref attributes".into());
                    }
                }
                refs.push((get("idref")?, nonlinear));
            }
            ["package", "spine", ..] => return Err("unsupported spine child".into()),
            _ => {}
        }
        Ok(())
    })?;
    if spines != 1 || manifests != 1 {
        return Err("expected exactly one manifest and spine".into());
    }
    let mut sections = Vec::new();
    let mut seen = BTreeSet::new();
    for (id, nonlinear) in refs {
        let (target, media) = manifest
            .get(&id)
            .and_then(Option::as_ref)
            .ok_or_else(|| format!("missing/ambiguous manifest item {id}"))?;
        let known_script = is_known_script_auxiliary(target, media);
        let is_section = numbered_part(target, "Contents/section").is_some();
        if nonlinear
            && !(target == HEADER_PART && mode.allow_header_nonlinear)
            && !(known_script && mode.allow_known_scripts)
            && !(is_section && fallback.len() == 1 && mode.allow_single_section_nonlinear)
        {
            return Err("unsupported spine itemref attributes".into());
        }
        if !paths.contains(target) || !seen.insert(target.clone()) {
            return Err(format!("missing/duplicate spine target {target}"));
        }
        if known_script && mode.allow_known_scripts {
            mode.used_known_script = true;
            continue;
        }
        if nonlinear && target == HEADER_PART {
            mode.used_header_nonlinear = true;
        }
        if media != "application/xml" {
            mode.known_script = known_script;
            return Err(format!("unsupported spine media-type {media} ({target})"));
        }
        if is_section {
            mode.used_single_section_nonlinear |= nonlinear;
            sections.push(target.clone());
        } else if target != HEADER_PART && numbered_part(target, "Contents/masterpage").is_none() {
            return Err(format!("non-section spine target {target}"));
        }
        // Header and masterpage are known structural entries in Hancom's spine;
        // their content is decoded separately. Only proven script auxiliaries are skipped.
    }
    // Unreferenced ambiguous IDs remain compatible with the ordinary/header resolver,
    // but cannot establish the stronger proof that script auxiliaries are the only loss.
    if mode.used_known_script
        && (manifest.values().any(Option::is_none)
            || script_hrefs.values().any(|&count| count != 1))
    {
        return Err("ambiguous manifest item in script package".into());
    }
    if sections.len() != fallback.len() || fallback.iter().any(|p| !sections.contains(p)) {
        return Err("spine does not cover each section exactly once".into());
    }
    Ok((sections, excludable))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoder::package::CONTAINER_PART;

    const CONTAINER_XML: &str = "<c:container xmlns:c='urn:oasis:names:tc:opendocument:xmlns:container'><c:rootfiles><c:rootfile full-path='Contents/content.hpf' media-type='application/hwpml-package+xml'/></c:rootfiles></c:container>";
    const CONTENT_XML: &str = "<p:package xmlns:p='http://www.idpf.org/2007/opf/'><p:manifest><p:item id='head' href='Contents/header.xml' media-type='application/xml'/><p:item id='alpha' href='Contents/section0.xml' media-type='application/xml'/><p:item id='beta' href='Contents/section1.xml' media-type='application/xml'/></p:manifest><p:spine><p:itemref idref='head'/><p:itemref idref='beta'/><p:itemref idref='alpha'/></p:spine></p:package>";

    fn paths() -> Vec<String> {
        [
            CONTENT_HPF_PART,
            CONTAINER_PART,
            "Contents/header.xml",
            "Contents/section0.xml",
            "Contents/section1.xml",
            "Preview/PrvText.txt",
            "META-INF/container.rdf",
            "settings.xml",
        ]
        .map(str::to_owned)
        .to_vec()
    }

    fn script_package() -> (String, Vec<String>) {
        let xml = CONTENT_XML.replace("<p:itemref idref='beta'/><p:itemref idref='alpha'/>", "<p:itemref idref='alpha'/><p:itemref idref='beta'/>")
            .replace("</p:manifest>", "<p:item id='aux' href='Scripts/headerScripts.js' media-type='application/x-javascript ;charset=utf-16'/></p:manifest>")
            .replace("<p:spine>", "<p:spine><p:itemref idref='aux'/>");
        let mut paths = paths();
        paths.push("Scripts/headerScripts.js".into());
        (xml, paths)
    }

    // 이것을 실패시키는 것: header 실패 후 별도 script 재시도, 실제 사용 flag 누락, 최초 오류 변경.
    #[test]
    fn script_header_no_uses_one_retry() {
        let (xml, paths) = script_package();
        for xml in [
            xml.clone(),
            xml.replace("idref='head'", "idref='head' linear='no'"),
            xml.replace("<p:itemref idref='aux'/>", "")
                .replace("</p:spine>", "<p:itemref idref='aux' linear='no'/></p:spine>")
                .replace("idref='head'", "idref='head' linear='no'"),
        ] {
            RESOLVES.with(|c| c.set(0));
            let order = PackageOrder::resolve(&paths, Some(&xml), Some(CONTAINER_XML));
            assert_eq!(RESOLVES.with(std::cell::Cell::get), 2);
            assert!(order.script_auxiliary_order);
            assert!(!order.header_only_nonlinear);
            assert!(!order.content_excludable);
            assert_eq!(order.sections, ["Contents/section0.xml", "Contents/section1.xml"]);
            let mut strict = SpineMode::default();
            let error =
                resolve_spine_with_mode(&xml, &paths, &order.sections, &mut strict).unwrap_err();
            let expected = match strict.nonlinear {
                true => "unsupported spine itemref attributes",
                false => "unsupported spine media-type application/x-javascript ;charset=utf-16 (Scripts/headerScripts.js)",
            };
            assert_eq!(error, expected);
            assert_eq!(order.problems, [format!("spine: {expected}; using file-number fallback")]);
            let mut narrow = SpineMode {
                allow_header_nonlinear: true,
                allow_known_scripts: true,
                ..Default::default()
            };
            resolve_spine_with_mode(&xml, &paths, &order.sections, &mut narrow).unwrap();
            assert!(narrow.used_known_script);
            assert_eq!(narrow.used_header_nonlinear, strict.nonlinear);
        }
        RESOLVES.with(|c| c.set(0));
        PackageOrder::resolve(&paths, Some(CONTENT_XML), Some(CONTAINER_XML));
        assert_eq!(RESOLVES.with(std::cell::Cell::get), 1);
        RESOLVES.with(|c| c.set(0));
        let other = xml.replace("Scripts/headerScripts.js", "Scripts/other.js");
        PackageOrder::resolve(&paths, Some(&other), Some(CONTAINER_XML));
        assert_eq!(RESOLVES.with(std::cell::Cell::get), 1);
    }

    // 이것을 실패시키는 것: 방문 단계에서 script 전용 no 거부, loop에서 header 비트 조건 제거.
    #[test]
    fn script_mode_bits_are_independent() {
        let (xml, paths) = script_package();
        let fallback = vec!["Contents/section0.xml".into(), "Contents/section1.xml".into()];
        let mut mode = SpineMode { allow_header_nonlinear: true, ..Default::default() };
        assert!(resolve_spine_with_mode(&xml, &paths, &fallback, &mut mode).is_err());
        let script_no = xml.replace("idref='aux'", "idref='aux' linear='no'");
        let mut mode = SpineMode { allow_known_scripts: true, ..Default::default() };
        resolve_spine_with_mode(&script_no, &paths, &fallback, &mut mode).unwrap();
        assert!(mode.used_known_script);
        assert!(!mode.used_header_nonlinear);
        let header_no = xml.replace("idref='head'", "idref='head' linear='no'");
        let mut mode = SpineMode { allow_known_scripts: true, ..Default::default() };
        assert!(resolve_spine_with_mode(&header_no, &paths, &fallback, &mut mode).is_err());
        let section_no = xml.replace("idref='alpha'", "idref='alpha' linear='no'");
        let mut mode = SpineMode {
            allow_header_nonlinear: true,
            allow_known_scripts: true,
            ..Default::default()
        };
        assert!(resolve_spine_with_mode(&section_no, &paths, &fallback, &mut mode).is_err());
    }

    // 이것을 실패시키는 것: version/future 때문에 순서를 버리거나 레이아웃에서 제외하기.
    #[test]
    fn order_attributes() {
        for extra in ["version='1.0'", "version='1.0' future='1'"] {
            let xml = CONTAINER_XML.replace("<c:container ", &format!("<c:container {extra} "));
            let content = CONTENT_XML.replace("idref='beta'", "idref='beta' future='1'");
            let result = PackageOrder::resolve(&paths(), Some(&content), Some(&xml));
            assert!(result.problems.is_empty(), "{:?}", result.problems);
            assert_eq!(result.sections, ["Contents/section1.xml", "Contents/section0.xml"]);
            assert_eq!(result.container_excludable, !extra.contains("future"));
        }
    }

    // 이것을 실패시키는 것: rootfiles/rootfile 부가 속성을 레이아웃에서 제외,
    // 알려진 선택 필드를 누락/접두사/중복이어도 유일한 값으로 간주.
    #[test]
    fn container_attribute_positions() {
        for tag in ["<c:rootfiles>", "<c:rootfile "] {
            let replacement = if tag.ends_with('>') {
                "<c:rootfiles future='1'>"
            } else {
                "<c:rootfile future='1' "
            };
            let xml = CONTAINER_XML.replace(tag, replacement);
            let result = PackageOrder::resolve(&paths(), Some(CONTENT_XML), Some(&xml));
            assert!(result.problems.is_empty());
            assert!(!result.container_excludable);
        }
        for (from, to) in [
            ("full-path=", "x:full-path="),
            ("media-type=", "x:media-type="),
            ("<c:rootfile ", "<c:rootfile full-path='Contents/content.hpf' "),
            ("<c:rootfiles>", "<c:rootfiles xml:base='x'>"),
        ] {
            assert!(validate_container(&CONTAINER_XML.replace(from, to), &paths()).is_err());
        }
    }

    // 이것을 실패시키는 것: 모든 부가 속성을 무조건 무시하여 조건부/경로/접두사 의미를 버리기.
    #[test]
    fn order_semantic_attributes_fallback() {
        for extra in [
            "xml:base='x'",
            "required-namespace='x'",
            "required-modules='x'",
            "fallback='x'",
            "fallback-style='x'",
            "requiredFeatures='x'",
            "requiredExtensions='x'",
            "systemLanguage='x'",
            "xmlns:x='urn:x' x:idref='beta'",
            "x:idref='beta'",
            "idref='beta'",
            "linear='no'",
        ] {
            let content = CONTENT_XML.replace("idref='beta'", &format!("idref='beta' {extra}"));
            let result = PackageOrder::resolve(&paths(), Some(&content), Some(CONTAINER_XML));
            assert!(!result.problems.is_empty(), "{extra}");
            assert_eq!(result.sections, ["Contents/section0.xml", "Contents/section1.xml"]);
        }
        for extra in [
            "version='2.0'",
            "xml:base='x'",
            "required-namespace='x'",
            "xmlns:x='urn:x' x:idref='x'",
        ] {
            let xml = CONTAINER_XML.replace("<c:container ", &format!("<c:container {extra} "));
            let result = PackageOrder::resolve(&paths(), Some(CONTENT_XML), Some(&xml));
            assert!(!result.problems.is_empty(), "{extra}");
            assert!(!result.container_excludable);
        }
    }

    // 이것을 실패시키는 것: manifest id를 section 파일 번호로 오인하기,
    // ZIP/manifest 순서로 읽기, 알려진 preview/RDF rootfile도 무조건 거부하기.
    #[test]
    fn resolves_ids_not_names_and_accepts_known_auxiliary_roots() {
        let container = CONTAINER_XML.replace("</c:rootfiles>", "<c:rootfile full-path='Preview/PrvText.txt' media-type='text/plain'/><c:rootfile full-path='META-INF/container.rdf' media-type='application/rdf+xml'/></c:rootfiles>");
        let result = PackageOrder::resolve(&paths(), Some(CONTENT_XML), Some(&container));
        assert!(result.problems.is_empty(), "{:?}", result.problems);
        assert!(result.container_excludable);
        assert_eq!(result.sections, ["Contents/section1.xml", "Contents/section0.xml"]);
        let encoded = CONTENT_XML
            .replace("id='beta'", "id='b&#101;ta'")
            .replace("idref='beta'", "idref='beta' linear='yes'");
        assert_eq!(resolve_spine(&encoded, &paths(), &result.sections).unwrap().0, result.sections);
    }

    // 이것을 실패시키는 것: 없는/중복/비구역 대상, namespace/scope/속성/구역 누락
    // 검증을 생략하고 지원된 spine으로 승격하기. 각 행의 고유 사유도 확인한다.
    #[test]
    fn unsupported_spine_boundaries_are_explicit() {
        let sections = vec!["Contents/section0.xml".into(), "Contents/section1.xml".into()];
        let cases = [
            ("idref='beta'", "idref='missing'", "missing/ambiguous manifest"),
            ("id='beta'", "id='alpha'", "missing/ambiguous manifest"),
            (
                "href='Contents/section1.xml'",
                "href='missing.xml'",
                "missing/duplicate spine target",
            ),
            ("idref='alpha'", "idref='beta'", "missing/duplicate spine target"),
            ("href='Contents/section1.xml'", "href='settings.xml'", "non-section spine target"),
            ("application/xml", "image/png", "unsupported spine media-type"),
            ("http://www.idpf.org/2007/opf/", "urn:fake", "unsupported OPF namespace"),
            ("<p:itemref idref='beta'/>", "", "cover each section"),
            ("<p:itemref idref='beta'/>", "<p:itemref/>", "missing idref"),
            ("idref='beta'", "idref='beta' linear='no'", "itemref attributes"),
            ("</p:spine>", "<p:future/></p:spine>", "unsupported spine child"),
            ("</p:spine>", "</p:spine><p:spine/>", "exactly one manifest and spine"),
            ("</p:manifest>", "</p:manifest><p:manifest/>", "exactly one manifest and spine"),
            ("<p:spine>", "<p:wrapper><p:spine>", "ill-formed"),
            ("id='beta'", "other='beta'", "missing id"),
        ];
        for (from, to, reason) in cases {
            let xml = CONTENT_XML.replace(from, to);
            let error = resolve_spine(&xml, &paths(), &sections).unwrap_err();
            assert!(error.contains(reason), "{from} -> {to}: {error}");
        }
        // An ambiguous id referenced by the spine must not let last-write win.
        let ambiguous = CONTENT_XML.replace("</p:manifest>", "<p:item id='beta' href='Contents/section1.xml' media-type='application/xml'/></p:manifest>");
        assert!(resolve_spine(&ambiguous, &paths(), &sections).unwrap_err().contains("ambiguous"));
        let missing_file: Vec<_> =
            paths().into_iter().filter(|p| p != "Contents/section1.xml").collect();
        assert!(resolve_spine(CONTENT_XML, &missing_file, &sections)
            .unwrap_err()
            .contains("missing/duplicate"));
        let duplicate_header =
            CONTENT_XML.replace("</p:spine>", "<p:itemref idref='head'/></p:spine>");
        assert!(resolve_spine(&duplicate_header, &paths(), &sections)
            .unwrap_err()
            .contains("missing/duplicate"));
        let non_section = CONTENT_XML.replace("</p:manifest>", "<p:item id='other' href='settings.xml' media-type='application/xml'/></p:manifest>")
            .replace("</p:spine>", "<p:itemref idref='other'/></p:spine>");
        assert!(resolve_spine(&non_section, &paths(), &sections)
            .unwrap_err()
            .contains("non-section"));
        let missing = PackageOrder::resolve(&paths(), None, None);
        assert_eq!(missing.sections, sections);
        assert_eq!(missing.problems.len(), 2);
        assert!(!missing.container_excludable);
    }

    // 이것을 실패시키는 것: rootfile을 실제로 해석하지 않고 container.xml을
    // 제외하기, 알려지지 않은 대상/타입/속성/namespace나 중복 루트를 허용하기.
    #[test]
    fn container_requires_a_supported_existing_package_root() {
        for (from, to, reason) in [
            (CONTENT_HPF_PART, "other.hpf", "missing rootfile"),
            (CONTENT_HPF_PART, "settings.xml", "unsupported rootfile"),
            ("application/hwpml-package+xml", "application/xml", "unsupported rootfile"),
            ("media-type=", "other-type=", "unsupported rootfile"),
            ("<c:rootfile ", "<c:rootfile xml:base='x' ", "selection attributes"),
            ("urn:oasis:names:tc:opendocument:xmlns:container", "urn:fake", "container namespace"),
            ("</c:rootfiles>", "<c:future/></c:rootfiles>", "container structure"),
            ("</c:rootfiles>", "<c:rootfile full-path='Contents/content.hpf' media-type='application/hwpml-package+xml'/></c:rootfiles>", "invalid/missing rootfile"),
        ] {
            let error = validate_container(&CONTAINER_XML.replace(from, to), &paths()).unwrap_err();
            assert!(error.contains(reason), "{to}: {error}");
        }
        let missing_file: Vec<_> = paths().into_iter().filter(|p| p != CONTENT_HPF_PART).collect();
        assert!(validate_container(CONTAINER_XML, &missing_file)
            .unwrap_err()
            .contains("missing rootfile"));
        let duplicate_preview = CONTAINER_XML.replace(
            "</c:rootfiles>",
            &format!(
                "{}</c:rootfiles>",
                "<c:rootfile full-path='Preview/PrvText.txt' media-type='text/plain'/>".repeat(2)
            ),
        );
        assert!(validate_container(&duplicate_preview, &paths())
            .unwrap_err()
            .contains("invalid/missing"));
        let no_package = CONTAINER_XML.replace("<c:rootfile full-path='Contents/content.hpf' media-type='application/hwpml-package+xml'/>", "");
        assert!(validate_container(&no_package, &paths()).unwrap_err().contains("exactly one"));
    }

    // 이것을 실패시키는 것: DTD/잘린 XML/복수 루트/과도한 깊이를 허용하거나
    // XML 오류를 성공으로 바꿔 패키지 구조가 지원됐다고 간주하기.
    #[test]
    fn malformed_package_xml_cannot_prove_order() {
        for xml in [
            "".to_string(),
            "<root>".into(),
            "<root/><root/>".into(),
            "<!DOCTYPE root><root/>".into(),
            "<unknown:root/>".into(),
            "<root a='1' a='2'/>".into(),
            "<root></other>".into(),
            format!("{}{}", "<root>".repeat(65), "</root>".repeat(65)),
        ] {
            assert!(visit(&xml, |_, _, _| Ok(())).is_err(), "{xml}");
        }
    }

    fn encoded_package() -> (crate::raw_package::RawPackage, String, Vec<String>, String) {
        use hwpforge_core::{Document, ImageStore, PageSettings, Paragraph, Run, Section};
        use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            ["처음", "삭제", "생존"]
                .map(|text| {
                    Paragraph::with_runs(
                        vec![Run::text(text, CharShapeIndex::new(0))],
                        ParaShapeIndex::new(0),
                    )
                })
                .to_vec(),
            PageSettings::a4(),
        ));
        let bytes = crate::HwpxEncoder::encode(
            &doc.validate().unwrap(),
            &crate::style_store_for_preset("default").unwrap(),
            &ImageStore::new(),
        )
        .unwrap();
        use std::io::{Cursor, Read, Write};
        let mut input = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
        let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..input.len() {
            let mut entry = input.by_index(i).unwrap();
            let mut payload = Vec::new();
            entry.read_to_end(&mut payload).unwrap();
            out.start_file(
                entry.name().unwrap(),
                zip::write::SimpleFileOptions::default().compression_method(entry.compression()),
            )
            .unwrap();
            out.write_all(&payload).unwrap();
        }
        out.start_file("Scripts/headerScripts.js", zip::write::SimpleFileOptions::default())
            .unwrap();
        out.write_all(b"/\0/\0").unwrap();
        let p = crate::raw_package::RawPackage::read(&out.finish().unwrap().into_inner()).unwrap();
        let content =
            std::str::from_utf8(p.read_entry("Contents/content.hpf").unwrap()).unwrap().to_owned();
        let container = std::str::from_utf8(p.read_entry("META-INF/container.xml").unwrap())
            .unwrap()
            .to_owned();
        let paths = p.entry_paths().map(str::to_owned).collect();
        (p, content, paths, container)
    }

    // 이것을 실패시키는 것: section 사용 비트에 의한 script 결과 억제 제거.
    #[test]
    fn combined_nonlinear_order_uses_one_retry() {
        use crate::decoder::preserved_loss::LossCause;
        let (mut p, content, paths, container) = encoded_package();
        RESOLVES.with(|c| c.set(0));
        let ordinary = PackageOrder::resolve(&paths, Some(&content), Some(&container));
        assert_eq!(RESOLVES.with(std::cell::Cell::get), 1);
        assert!(ordinary.problems.is_empty());
        assert!(!ordinary.single_section_nonlinear_order);
        let content = content.replace("</opf:manifest>", "<opf:item id='aux' href='Scripts/headerScripts.js' media-type='application/x-javascript ;charset=utf-16'/></opf:manifest>");
        let start = content.find("<opf:spine>").unwrap() + "<opf:spine>".len();
        let end = content.find("</opf:spine>").unwrap();
        for (refs, cause) in [
            ("<opf:itemref idref='header' linear='no'/><opf:itemref idref='section0' linear='no'/><opf:itemref idref='aux' linear='no'/>", LossCause::SingleSectionNonlinearOrder),
            ("<opf:itemref idref='section0' linear='no'/><opf:itemref idref='aux' linear='no'/><opf:itemref idref='header' linear='no'/>", LossCause::SingleSectionNonlinearOrder),
            ("<opf:itemref idref='aux'/><opf:itemref idref='header' linear='no'/><opf:itemref idref='section0' linear='no'/>", LossCause::SingleSectionNonlinearOrder),
            ("<opf:itemref idref='header' linear='no'/><opf:itemref idref='section0'/>", LossCause::HeaderOnlyNonlinearOrder),
            ("<opf:itemref idref='aux'/><opf:itemref idref='header'/><opf:itemref idref='section0'/>", LossCause::ScriptAuxiliaryOrder),
            ("<opf:itemref idref='header' linear='no'/><opf:itemref idref='aux' linear='no'/><opf:itemref idref='section0'/>", LossCause::ScriptAuxiliaryOrder),
        ] {
            let xml = format!("{}{refs}{}", &content[..start], &content[end..]);
            RESOLVES.with(|c| c.set(0));
            let order = PackageOrder::resolve(&paths, Some(&xml), Some(&container));
            assert_eq!(RESOLVES.with(std::cell::Cell::get), 2);
            assert!(!order.content_excludable);
            assert!(order.container_excludable);
            assert!(order.content_problem_reported);
            assert_eq!(order.sections, ["Contents/section0.xml"]);
            let mut strict = SpineMode::default();
            let error = resolve_spine_with_mode(&xml, &paths, &order.sections, &mut strict).unwrap_err();
            let expected = if refs.contains("linear='no'") { "unsupported spine itemref attributes" } else { "unsupported spine media-type application/x-javascript ;charset=utf-16 (Scripts/headerScripts.js)" };
            assert_eq!(error, expected);
            assert_eq!(order.problems, [format!("spine: {expected}; using file-number fallback")]);
            assert_eq!((order.single_section_nonlinear_order, order.script_auxiliary_order, order.header_only_nonlinear), (cause == LossCause::SingleSectionNonlinearOrder, cause == LossCause::ScriptAuxiliaryOrder, cause == LossCause::HeaderOnlyNonlinearOrder));
            p.replace_text_entry("Contents/content.hpf", xml);
            let d = crate::HwpxDecoder::decode(&p.write().unwrap()).unwrap();
            let losses = &d.source_basis.as_ref().unwrap().losses;
            assert_eq!(losses.len(), 1);
            assert_eq!(losses[0].cause, cause);
        }
    }

    // 이것을 실패시키는 것: header 사용 비트 기록에서 header target 조건 제거.
    #[test]
    fn single_section_mode_bits_are_independent() {
        let (_, content, paths, _) = encoded_package();
        let xml = content
            .replace("idref=\"section0\" linear=\"yes\"", "idref=\"section0\" linear=\"no\"");
        let fallback = vec!["Contents/section0.xml".into()];
        let mut mode = SpineMode { allow_single_section_nonlinear: true, ..Default::default() };
        let (sections, excludable) =
            resolve_spine_with_mode(&xml, &paths, &fallback, &mut mode).unwrap();
        assert_eq!(sections, fallback);
        assert!(excludable);
        assert!(mode.used_single_section_nonlinear);
        assert!(!mode.used_header_nonlinear);
        assert!(!mode.used_known_script);
        let mut strict = SpineMode::default();
        assert!(resolve_spine_with_mode(&xml, &paths, &fallback, &mut strict).is_err());
        assert!(strict.nonlinear);
        assert!(!strict.used_single_section_nonlinear);
        let mut header_only = SpineMode {
            allow_header_nonlinear: true,
            allow_known_scripts: true,
            ..Default::default()
        };
        assert!(resolve_spine_with_mode(&xml, &paths, &fallback, &mut header_only).is_err());
        let mut normal = SpineMode { allow_single_section_nonlinear: true, ..Default::default() };
        resolve_spine_with_mode(&content, &paths, &fallback, &mut normal).unwrap();
        assert!(!normal.used_single_section_nonlinear);
    }
}
