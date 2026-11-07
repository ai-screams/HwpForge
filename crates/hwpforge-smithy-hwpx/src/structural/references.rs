//! Reference closure for deleted source definitions. Unknown reference syntax
//! is refused; this check does not relax the existing admission or preflight.
use super::StructuralEditError;
use crate::decoder::section::{decode_crossref_target, reference_object_id};
use hwpforge_core::control::RefTarget;
use hwpforge_core::source_xml::{XmlElement, XmlSource};
use std::collections::{BTreeMap, BTreeSet};

type Attributes = BTreeMap<String, String>;

#[derive(Default)]
pub(super) struct Removed {
    ids: BTreeSet<String>,
    object_ids: BTreeSet<u64>,
    names: BTreeSet<String>,
}

fn invalid(detail: impl Into<String>) -> StructuralEditError {
    super::verification_error(crate::HwpxError::InvalidStructure { detail: detail.into() })
}

pub(super) fn source(xml: &str) -> Result<XmlSource, StructuralEditError> {
    // 이것을 실패시키는 것: 원장 생성 전 제한 스트리밍 검사 생략.
    super::xml_values::scan(xml, |_, _| Ok(()), |_, _| Ok(()))?;
    XmlSource::read(xml).map(|(_, source)| source).map_err(invalid)
}

pub(super) fn attributes(
    xml: &str,
    element: &XmlElement,
) -> Result<Attributes, StructuralEditError> {
    let start = crate::cache_flow::source_start(xml, element)
        .ok_or_else(|| invalid("unreadable reference start tag"))?;
    start
        .attributes()
        .map(|attribute| {
            let attribute = attribute.map_err(|e| invalid(e.to_string()))?;
            let key = attribute.key.as_ref();
            let value = attribute
                .normalized_value(quick_xml::XmlVersion::Explicit1_0)
                .map_err(|e| invalid(e.to_string()))?;
            Ok((key.to_owned(), value.into_owned()))
        })
        .collect()
}

impl Removed {
    pub(super) fn record(&mut self, xml: &str) -> Result<(), StructuralEditError> {
        let source = source(xml)?;
        for element in source.elements().iter().skip(1) {
            let attrs = attributes(xml, element)?;
            for (key, value) in attrs {
                // Paragraph references are refused before editing; header/footer IDs identify flows.
                // Binary items are package definitions: deleting a picture keeps its asset.
                // 이것을 실패시키는 것: 삭제 subtree의 ID 또는 이름 수집 생략.
                match key.as_str() {
                    // 이것을 실패시키는 것: header/footer의 local ID를 전역 대상으로 수집.
                    "id" if !matches!(element.name(), "p" | "header" | "footer") => {
                        self.record_id(value, element.name() != "fieldBegin")?;
                    }
                    "instid" | "instId" => {
                        self.record_id(value, true)?;
                    }
                    "name" if matches!(element.name(), "bookmark" | "fieldBegin") => {
                        self.names.insert(value);
                    }
                    _ => {}
                }
            }
        }
        self.ids.remove("");
        self.names.remove("");
        Ok(())
    }

    fn record_id(&mut self, value: String, object: bool) -> Result<(), StructuralEditError> {
        // String IDs (e.g. field begin/end) remain exact; object consumers use u64.
        // 이것을 실패시키는 것: 정의 쪽 숫자 ID 수집 생략 또는 정규 표기만 수집.
        if let Some(id) = reference_object_id(&value).filter(|_| object) {
            self.object_ids.insert(id);
        } else if object && !value.is_empty() {
            // 이것을 실패시키는 것: 숫자로 해석할 수 없는 객체 정의를 조용히 무시.
            return Err(invalid("unresolved numeric object definition"));
        }
        self.ids.insert(value);
        Ok(())
    }

    // Untyped links/attributes can refer to either ID space. Keep exact string
    // identity and conservatively check numeric aliases as well, without parsing names.
    // 이것을 실패시키는 것: href/Path/속성의 숫자 별칭 비교 생략.
    fn targets_removed(&self, target: &str) -> bool {
        self.ids.contains(target)
            || self.names.contains(target)
            || reference_object_id(target).is_some_and(|id| self.object_ids.contains(&id))
    }

    fn crossref_is_safe(&self, target: &RefTarget) -> bool {
        // 이것을 실패시키는 것: 객체/책갈피 공간 혼합 또는 Raw 대상 승인.
        match target {
            RefTarget::Object(id) => !self.object_ids.contains(&id.value()),
            RefTarget::Name(name) => !self.names.contains(name),
            _ => false,
        }
    }

    pub(super) fn surviving_references_are_safe(
        &self,
        package: &mut hwpforge_core::zip_package::Package<'_>,
    ) -> Result<bool, StructuralEditError> {
        // 이것을 실패시키는 것: 삭제 정의가 없어도 XML part를 읽기.
        if self.ids.is_empty() && self.names.is_empty() {
            return Ok(true);
        }
        // Scan all XML parts, including untouched sections, headers and masters.
        for index in 0..package.entries().len() {
            let path = &package.entries()[index].path;
            if !(path.ends_with(".xml") || path.ends_with(".hpf")) {
                continue;
            }
            let xml = super::source_ranges::read_xml(package, index)?;
            if !self.xml_is_safe(&xml)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn xml_is_safe(&self, xml: &str) -> Result<bool, StructuralEditError> {
        let mut attributes_safe = true;
        let mut fields_safe = true;
        super::xml_values::scan(
            xml,
            |key, value| {
                attributes_safe &= self.attribute_is_safe(key, value);
                Ok(())
            },
            |kind, params| {
                fields_safe &= self.field_is_safe(kind, params);
                Ok(())
            },
        )?;
        Ok(attributes_safe && fields_safe)
    }

    fn attribute_is_safe(&self, key: &str, value: &str) -> bool {
        if value.is_empty() || key == "xmlns" || key.starts_with("xmlns:") {
            return true;
        }
        let local = key.rsplit(':').next().unwrap_or(key);
        // 이것을 실패시키는 것: 미해석 namespace의 참조를 같은 local name으로 승인.
        if key.contains(':') && (local == "href" || local.to_ascii_lowercase().ends_with("ref")) {
            return false;
        }
        match local {
            // Flags carry no target; linked-list zero is the encoder's absent value.
            // 이것을 실패시키는 것: 플래그를 참조로 분류하거나 목록 기본값을 거부/무조건 승인.
            "hasTextRef" | "hasNumRef" => true,
            "linkListIDRef" | "linkListNextIDRef" => value == "0",
            // Zero means an unconnected shape, not object zero.
            // 이것을 실패시키는 것: 연결점의 0 기본값 거부 또는 비영 대상까지 면제.
            "subjectIDRef" if value == "0" => true,
            "href" => self.link_is_safe(value),
            // The decoder pairs field boundaries by exact String identity;
            // OPF identifiers are also strings, not object numbers.
            // 이것을 실패시키는 것: 문자열 ID 참조를 숫자 별칭으로 비교.
            "idref" | "beginIDRef" => !self.ids.contains(value),
            "idRef" | "subjectIDRef" => !self.targets_removed(value),
            // Style/assets are outside deleted subtrees. Paragraph/list
            // addresses have already passed the paragraph-anchor check.
            "binaryItemIDRef" | "chartIDRef" | "paraPrIDRef" | "charPrIDRef" | "styleIDRef"
            | "borderFillIDRef" | "tabPrIDRef" | "nextStyleIDRef" | "prIDRef" | "listIDRef"
            | "paraIDRef" | "memoShapeIDRef" | "outlineShapeIDRef" | "charStyleIDRef" => true,
            // Unknown *Ref attributes may be harmless, but allowing them
            // could hide a surviving link to a deleted definition.
            _ => !local.to_ascii_lowercase().ends_with("ref"),
        }
    }

    fn link_is_safe(&self, value: &str) -> bool {
        let value =
            value.strip_suffix(crate::decoder::section::OBJECT_HYPERLINK_SUFFIX).unwrap_or(value);
        if let Some(address) =
            value.strip_prefix("https://").or_else(|| value.strip_prefix("http://"))
        {
            // 이것을 실패시키는 것: http://#name 같은 빈 host를 외부 링크로 승인.
            let authority = address.split(['/', '?', '#']).next().unwrap_or("");
            return !authority.is_empty() && !authority.chars().any(char::is_whitespace);
        }
        if let Some(address) = value.strip_prefix("mailto:") {
            return !address.is_empty() && !address.starts_with('#');
        }
        if let Some((_, target)) = value.split_once('#') {
            return !target.is_empty()
                && !target.contains(['%', '#', '?', ';'])
                // A numeric-looking fragment must obey the same u64 grammar.
                // Other fragments remain literal bookmark names, as in the decoder.
                // 이것을 실패시키는 것: 음수/소수/overflow/공백 숫자 fragment 승인.
                && (!target.starts_with(|c: char| c.is_ascii_digit() || c == '+' || c == '-' || c.is_whitespace())
                    || reference_object_id(target).is_some())
                && !self.targets_removed(target);
        }
        // Manifest hrefs address retained package entries, not paragraph targets.
        value.starts_with("Contents/")
            || value.starts_with("BinData/")
            || value.starts_with("Chart/")
            || value == "settings.xml"
    }

    fn field_is_safe(&self, kind: &str, params: &BTreeMap<String, String>) -> bool {
        if matches!(
            kind,
            "CLICK_HERE" | "BOOKMARK" | "MEMO" | "SUMMERY" | "DATE" | "PATH" | "PAGE_NUM"
        ) {
            return true;
        }
        if !matches!(kind, "CROSSREF" | "HYPERLINK") {
            return false;
        }
        // 이것을 실패시키는 것: 미해석 참조 parameter를 이름만 보존하고 승인.
        let known_parameters: &[&str] = if kind == "HYPERLINK" {
            &["Path", "Category", "TargetType", "DocOpenType"]
        } else {
            &[
                "Fiexde",
                "Prop",
                "Command",
                "RefPath",
                "RefType",
                "RefContentType",
                "RefHyperLink",
                "RefOpenType",
            ]
        };
        if params.keys().any(|name| !known_parameters.contains(&name.as_str())) {
            return false;
        }
        // 이것을 실패시키는 것: 필드 RefPath/Command/Path의 삭제 대상 검사 생략.
        if kind == "HYPERLINK" {
            return params.get("Path").is_some_and(|value| self.link_is_safe(value));
        }
        let Some(path) = params.get("RefPath") else { return false };
        let target = path.strip_prefix('?').unwrap_or(path);
        let target = target.strip_suffix(';').unwrap_or(target);
        let target = target.strip_prefix('#').unwrap_or(target);
        if target.is_empty() || target.contains(['?', ';', '%', '#']) {
            return false;
        }
        let ref_type = params
            .get("RefType")
            .and_then(|value| value.parse::<hwpforge_foundation::RefType>().ok())
            .unwrap_or_default();
        let decoded_target = decode_crossref_target(path, &ref_type);
        if !self.crossref_is_safe(&decoded_target) {
            return false;
        }
        let Some(command) = params.get("Command") else { return false };
        let Some((_, suffix)) = command.split_once(';') else { return false };
        let fields: Vec<_> = suffix.split(';').collect();
        // 이것을 실패시키는 것: 미해석 Command 인수 또는 RefPath와 다른 대상 승인.
        if !matches!(
            fields.as_slice(),
            ["0" | "1" | "2" | "3" | "4" | "5" | "6", "0" | "1" | "2" | "3", "0" | "1", "0", ""]
        ) {
            return false;
        }
        // 이것을 실패시키는 것: RefPath/Command의 의미 비교를 문자열 비교로 되돌리거나 불일치 승인.
        let command_target = command.split_once(';').filter(|(target, _)| target.starts_with('?'));
        command_target
            .is_some_and(|(target, _)| decode_crossref_target(target, &ref_type) == decoded_target)
    }
}

#[cfg(test)]
#[path = "references_tests.rs"]
mod tests;
