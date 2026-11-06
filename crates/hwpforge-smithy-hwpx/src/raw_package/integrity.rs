//! Editing refusal, not signature verification or decryption.
use super::RawPackage;
use crate::{HwpxError, HwpxResult};
use std::borrow::Cow;

// A refusal-only key: never used for package lookup, decoding or output names.
// 이것을 실패시키는 것: dot/빈 성분/부모 성분 또는 역슬래시 정규화 제거.
fn comparison_path(path: &str) -> String {
    let slashes = path.replace('\\', "/");
    let mut parts = Vec::new();
    for part in slashes.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    parts.join("/").to_ascii_lowercase()
}

// The shared read visitor exposes raw namespace URIs. Apply XML attribute
// normalization exactly once here, without changing its read-only contract.
// 이것을 실패시키는 것: namespace 정규화 제거/실패 허용/이중 정규화.
fn normalized_namespace(ns: &str) -> Result<Cow<'_, str>, String> {
    quick_xml::events::attributes::Attribute {
        key: quick_xml::name::QName("xmlns"),
        value: Cow::Borrowed(ns),
    }
    .normalized_value(quick_xml::XmlVersion::Explicit1_0)
    .map_err(|error| error.to_string())
}

// OWPML container profile and encryption/signature schemas; Hancom's
// OWPMLDocumentDef and NamespacePrefix use these paths and namespace URIs.
fn protected_path(path: &str) -> bool {
    // Core's ZIP name policy already accounts for Hancom's ASCII case folding.
    // This refusal does not rename a part or make it eligible for decoding.
    // 이것을 실패시키는 것: 경로 정규화/대소문자 접기 또는 보호 경로 판정 제거.
    let path = comparison_path(path);
    (path.starts_with("meta-inf/signatures") && path.ends_with(".xml"))
        || path == "meta-inf/encryption.xml"
}

impl RawPackage {
    /// Check already bounded package bytes before editing, after input decode.
    /// Read-only decoders never call this. A marker is enough to refuse; its
    /// signature payload need not be valid or even present.
    pub(crate) fn ensure_editable(&self) -> HwpxResult<()> {
        let reject = |path: &str, reason: &str| HwpxError::InvalidStructure {
            detail: format!("package integrity: '{}': {reason}", super::shown_name(path)),
        };
        for entry in &self.entries {
            if protected_path(&entry.path) {
                return Err(reject(&entry.path, "signature/encryption entry forbids editing"));
            }
            // 이것을 실패시키는 것: descriptor 선택에서 한 경로 또는 경로 정규화/대소문자 접기 제거.
            if !matches!(
                comparison_path(&entry.path).as_str(),
                "meta-inf/manifest.xml" | "meta-inf/container.xml" | "contents/content.hpf"
            ) {
                continue;
            }
            // Reuse the package XML encoding rules, including UTF-16 and BOMs.
            // 이것을 실패시키는 것: UTF-16을 검사 없이 건너뛰거나 XML 파싱 실패를 허용.
            let xml = crate::package_xml::decode(
                entry.bytes.as_slice().into(),
                hwpforge_core::zip_package::MAX_ENTRY_SIZE as usize,
            )
            .map_err(|reason| reject(&entry.path, &reason))?;
            crate::decoder::package_order::visit(&xml, |path, ns, attrs| {
                let namespace = normalized_namespace(ns)?;
                let ns = namespace.as_ref();
                let name = path[path.len() - 1].as_str();
                // 이것을 실패시키는 것: namespace 판정 제거 또는 알려진 선언 하나 제거.
                if matches!(
                    (ns, name),
                    ("http://www.w3.org/2000/09/xmldsig#", "Signature")
                        | ("urn:oasis:names:tc:opendocument:xmlns:container", "signatures")
                        | ("urn:oasis:names:tc:opendocument:xmlns:manifest:1.0", "encryption-data")
                ) {
                    return Err("signature/encryption declaration forbids editing".into());
                }
                // OWPML OPF item@encryption is xs:boolean. Unknown values cannot
                // establish absence of encryption, so only explicit false is safe.
                // 이것을 실패시키는 것: encryption 속성 검사 제거 또는 true만 거부.
                // 이것을 실패시키는 것: 표준 OPF URI(끝 슬래시 없음) 비교 제거.
                if matches!(ns, "http://www.idpf.org/2007/opf/" | "http://www.idpf.org/2007/opf")
                    && name == "item"
                    // As with references below, attribute namespaces cannot
                    // exempt a protection marker: unknown qualifiers fail closed.
                    // 이것을 실패시키는 것: qualified encryption 속성을 건너뛰기.
                    && attrs.iter().any(|(key, value)| {
                        key.rsplit(':').next() == Some("encryption")
                            && !matches!(value.trim(), "false" | "0")
                    })
                {
                    return Err("encryption declaration forbids editing".into());
                }
                // Descriptor references still declare protection when their target
                // entry is missing. XML entity spellings have already been decoded.
                // 이것을 실패시키는 것: payload 없는 서명 경로 참조 검사 제거.
                if attrs.iter().any(|(key, value)| {
                    matches!(key.rsplit(':').next(), Some("href" | "full-path"))
                        && protected_path(value)
                }) {
                    return Err("signature/encryption reference forbids editing".into());
                }
                Ok(())
            })
            .map_err(|reason| reject(&entry.path, &reason))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    mod cases {
        include!("../../tests/support/integrity.rs");
    }

    // 이것을 실패시키는 것: namespace 정규화 제거/실패 허용/이중 정규화.
    #[test]
    fn integrity_namespace_attribute_normalization() {
        assert_eq!(normalized_namespace("urn:&amp;&lt;&gt;&apos;&quot;").unwrap(), "urn:&<>'\"");
        assert_eq!(normalized_namespace("urn:a\r\nb\tc&#10;d").unwrap(), "urn:a b c\nd");
        for xml in cases::REFUSED {
            assert!(check("META-INF/manifest.xml", xml.as_bytes()).is_err(), "{xml}");
        }
        for xml in cases::EDITABLE {
            check("META-INF/manifest.xml", xml.as_bytes()).unwrap();
        }
    }

    // 이것을 실패시키는 것: 보호 경로/descriptor의 dot/빈/부모 성분 또는 역슬래시 정규화 제거.
    #[test]
    fn integrity_equivalent_paths() {
        for path in
            ["./META-INF/signatures.xml", "META-INF/./encryption.xml", "META-INF//signatures.xml"]
        {
            assert!(check(path, b"").is_err(), "{path}");
        }
        for path in
            ["./META-INF/manifest.xml", "META-INF/./container.xml", "./Contents/content.hpf"]
        {
            assert!(
                check(path, b"<Signature xmlns='http://www.w3.org/2000/09/xmldsig#'/>").is_err(),
                "{path}"
            );
        }
        for xml in [
            "<r href='./META-INF/signatures.xml'/>",
            "<r full-path='META-INF/./encryption.xml'/>",
            "<r href='META-INF\\signatures.xml'/>",
            "<r href='META-INF/ignored/../signatures.xml'/>",
            "<r full-path='META-INF/signatures&amp;proof.xml'/>",
        ] {
            assert!(check("META-INF/manifest.xml", xml.as_bytes()).is_err(), "{xml}");
        }
        // ZIP backslashes are rejected by the existing Core name policy.
        assert!(check("META-INF\\signatures.xml", b"").is_err());
    }

    // 이것을 실패시키는 것: qualified encryption 속성을 건너뛰기.
    #[test]
    fn integrity_qualified_encryption() {
        for namespace in ["http://www.idpf.org/2007/op&#102;/", "urn:&amp;vendor"] {
            let xml = format!("<item xmlns='http://www.idpf.org/2007/opf/' xmlns:e='{namespace}' e:encryption='true'/>");
            assert!(check("META-INF/manifest.xml", xml.as_bytes()).is_err(), "{xml}");
        }
    }

    fn check(path: &str, bytes: &[u8]) -> HwpxResult<()> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file(path, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        RawPackage::read_capped(&bytes, 4096, 4096)?.ensure_editable()
    }

    // 이것을 실패시키는 것: 서명 경로의 접두/확장자 검사 제거, 서명 접미 번호를 허용하지 않기.
    #[test]
    fn integrity_path_boundaries() {
        for path in [
            "META-INF/signatures.xml",
            "META-INF/signatures42.xml",
            "META-INF/encryption.xml",
            "meta-inf/Signatures.XML",
            "META-INF/Encryption.XML",
        ] {
            assert!(check(path, b"").is_err(), "{path}");
        }
        for path in [
            "custom/signatures.xml",
            "META-INF/signatures.txt",
            "META-INF/signatures",
            "Contents/section0.xml",
        ] {
            check(path, b"not a descriptor").unwrap();
        }
    }

    // 이것을 실패시키는 것: XML 파싱 실패 무시, UTF-16 검사 생략, descriptor 제외/대소문자 구분.
    #[test]
    fn integrity_descriptors_and_encoding_fail_closed() {
        for path in [
            "META-INF/manifest.xml",
            "META-INF/Manifest.xml",
            "mEtA-InF/MaNiFeSt.XmL",
            "META-INF/container.xml",
            "Contents/content.hpf",
        ] {
            let xml = "<Signature xmlns='http://www.w3.org/2000/09/xmldsig#'/>";
            for bytes in [
                xml.as_bytes().to_vec(),
                crate::package_xml::utf16(xml, true, true),
                crate::package_xml::utf16(xml, false, true),
                b"<broken".to_vec(),
                vec![0xff],
            ] {
                assert!(check(path, &bytes).is_err(), "{path}");
            }
            check(path, b"<r/>").unwrap();
        }
    }

    // 이것을 실패시키는 것: namespace 무시, 빈/default namespace 요소와 주석/CDATA를 표지로 오인.
    #[test]
    fn integrity_namespaces_and_references() {
        for xml in [
            "<Signature xmlns='http://www.w3.org/2000/09/xmldsig#'/>",
            "<signatures xmlns='urn:oasis:names:tc:opendocument:xmlns:container'/>",
            "<encryption-data xmlns='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/>",
            "<r href='META-INF/signatures&#46;xml'/>",
            "<r full-path='META-INF/signatures.xml'/>",
            "<r xmlns:o='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' o:full-path='META-INF/signatures.xml'/>",
        ] { assert!(check("META-INF/manifest.xml", xml.as_bytes()).is_err(), "{xml}"); }
        for xml in [
            "<r xmlns:s='http://www.w3.org/2000/09/xmldsig#'/>",
            "<Signature/>",
            "<signatures/>",
            "<encryption-data/>",
            "<Signature xmlns='urn:other'/>",
            "<r><!-- <Signature/> --><![CDATA[encryption-data]]></r>",
            "<r title='META-INF/signatures.xml'/>",
            "<r href='custom/signatures.xml'/>",
        ] {
            check("META-INF/manifest.xml", xml.as_bytes()).unwrap();
        }
    }

    // 이것을 실패시키는 것: OPF encryption 거부 삭제, true만 검사, false/0 또는 공백을 거부.
    #[test]
    fn integrity_opf_encryption_boolean() {
        // 이것을 실패시키는 것: 표준 OPF URI(끝 슬래시 없음) 비교 제거.
        assert!(check(
            "Contents/content.hpf",
            b"<item xmlns='http://www.idpf.org/2007/opf' encryption='true'/>"
        )
        .is_err());
        for value in ["true", "1", "unknown", ""] {
            let xml = format!("<item xmlns='http://www.idpf.org/2007/opf/' encryption='{value}'/>");
            assert!(check("Contents/content.hpf", xml.as_bytes()).is_err(), "{value}");
        }
        for xml in [
            "<item xmlns='http://www.idpf.org/2007/opf/' encryption='false'/>",
            "<item xmlns='http://www.idpf.org/2007/opf/' encryption='0'/>",
            "<item xmlns='http://www.idpf.org/2007/opf/' encryption=' false '/>",
            "<item xmlns='http://www.idpf.org/2007/opf/'/>",
            "<item xmlns='urn:other' encryption='true'/>",
            "<other xmlns='http://www.idpf.org/2007/opf/' encryption='true'/>",
        ] {
            check("Contents/content.hpf", xml.as_bytes()).unwrap();
        }
    }
}
