//! Positional paragraph IDs cannot safely serve as persistent anchors.
//! Refuse their consumers before editing, including consumers in deleted content.
use super::{RawPackage, StructuralEditError};

fn refused() -> StructuralEditError {
    super::verification_error(crate::HwpxError::InvalidStructure {
        detail: "unresolved paragraph reference during structural edit".into(),
    })
}

/// Inspect the original, already materialized package without retaining XML ledgers.
pub(super) fn check(package: &RawPackage) -> Result<(), StructuralEditError> {
    for path in package.entry_paths().filter(|p| p.ends_with(".xml") || p.ends_with(".hpf")) {
        // Package XML has its own encoding contract. Other parts borrow their UTF-8
        // bytes; neither a full string copy nor a decoder ledger is needed.
        let owned;
        let xml = if matches!(path, "Contents/content.hpf" | "META-INF/container.xml") {
            owned = package.read_text_entry(path).map_err(super::verification_error)?;
            owned.as_str()
        } else {
            std::str::from_utf8(package.read_entry(path).map_err(super::verification_error)?)
                .map_err(|_| refused())?
        };
        let settings = path == "settings.xml";
        if settings
            && !crate::layout_edit::settings_paragraph_refs_are_safe(xml)
                .map_err(super::verification_error)?
        {
            return Err(refused());
        }
        // XML names cannot contain CDATA or character references. Filter on names,
        // never values: every spelling of a field value still enters the parser.
        // 이것을 실패시키는 것: fieldBegin 대신 원문 TARGET_OUTLINE 값으로 선검사.
        if xml.contains("fieldBegin") || xml.contains("paraIDRef") {
            check_xml(xml, settings)?;
        }
    }
    Ok(())
}

fn check_xml(xml: &str, settings: bool) -> Result<(), StructuralEditError> {
    super::xml_values::scan(
        xml,
        |key, _| {
            // 이것을 실패시키는 것: settings 밖 paraIDRef 거부 생략.
            if !settings && key.rsplit(':').next() == Some("paraIDRef") {
                return Err(refused());
            }
            Ok(())
        },
        |kind, values| {
            let text = |name| values.get(name).map(String::as_str);
            // 이것을 실패시키는 것: 개요 종류 거부 제거 또는 미지 CROSSREF 종류 승인.
            if text("RefType") == Some("TARGET_OUTLINE")
                || text("TargetType") == Some("HWPHYPERLINK_TARGET_OUTLINE")
                || (kind == "CROSSREF"
                    && !matches!(
                        text("RefType"),
                        Some(
                            "TARGET_BOOKMARK"
                                | "TARGET_TABLE"
                                | "TARGET_FIGURE"
                                | "TARGET_EQUATION"
                                | "TARGET_FOOTNOTE"
                                | "TARGET_ENDNOTE"
                        )
                    ))
            {
                return Err(refused());
            }
            Ok(())
        },
    )
}

#[cfg(test)]
#[path = "paragraph_refs_tests.rs"]
mod tests;
