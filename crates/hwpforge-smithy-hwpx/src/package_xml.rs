//! Byte decoding for the two package XML parts; source-map parts stay UTF-8.
use std::borrow::Cow;

use crate::{HwpxError, HwpxResult};

/// Decodes entry bytes already selected by their exact package path.
/// Only `META-INF/container.xml` and `Contents/content.hpf` accept UTF-16
/// (BOM or a compatible declaration required), strip a BOM, and normalize an
/// existing XML declaration to UTF-8. All other paths require UTF-8.
/// Invalid encoding/declarations or an oversized decoded package XML return
/// `HwpxError::Zip`; XML structure validation belongs to the caller.
/// No package state is mutated, including on failure.
pub(crate) fn text_entry<'a>(path: &str, bytes: impl Into<Cow<'a, [u8]>>) -> HwpxResult<String> {
    let bytes = bytes.into();
    let result = match path {
        crate::decoder::package_order::CONTENT | crate::decoder::package_order::CONTAINER => {
            decode(bytes, hwpforge_core::zip_package::MAX_ENTRY_SIZE as usize)
        }
        _ => utf8(bytes),
    };
    result.map_err(|detail| HwpxError::Zip(format!("read '{path}': {detail}")))
}

fn utf8(bytes: Cow<'_, [u8]>) -> Result<String, String> {
    match bytes {
        Cow::Owned(bytes) => String::from_utf8(bytes).map_err(|e| e.utf8_error().to_string()),
        Cow::Borrowed(bytes) => {
            std::str::from_utf8(bytes).map(str::to_owned).map_err(|e| e.to_string())
        }
    }
}

fn decode(bytes: Cow<'_, [u8]>, limit: usize) -> Result<String, String> {
    use quick_xml::{
        events::{BytesDecl, BytesStart, Event},
        Reader,
    };
    // (endianness, BOM length, declaration required). No charset guessing.
    let (endian, skip, required) = if bytes.starts_with(&[0xff, 0xfe]) {
        (Some(true), 2, false)
    } else if bytes.starts_with(&[0xfe, 0xff]) {
        (Some(false), 2, false)
    } else if bytes.starts_with(&[0x3c, 0, 0x3f, 0]) {
        (Some(true), 0, true)
    } else if bytes.starts_with(&[0, 0x3c, 0, 0x3f]) {
        (Some(false), 0, true)
    } else {
        (None, usize::from(bytes.starts_with(&[0xef, 0xbb, 0xbf])) * 3, false)
    };
    let mut xml = if let Some(le) = endian {
        let bytes = &bytes[skip..];
        if !bytes.len().is_multiple_of(2) {
            return Err("odd UTF-16 byte count".into());
        }
        let units = bytes.chunks_exact(2).map(|b| {
            let pair = [b[0], b[1]];
            if le {
                u16::from_le_bytes(pair)
            } else {
                u16::from_be_bytes(pair)
            }
        });
        let mut text = String::new();
        for ch in char::decode_utf16(units) {
            let ch = ch.map_err(|e| e.to_string())?;
            if text.len() + ch.len_utf8() > limit {
                return Err("XML UTF-8 output limit".into());
            }
            text.push(ch);
        }
        text
    } else {
        if bytes.len() - skip > limit {
            return Err("XML UTF-8 output limit".into());
        }
        let bytes = match bytes {
            Cow::Borrowed(bytes) => Cow::Borrowed(&bytes[skip..]),
            Cow::Owned(mut bytes) => {
                // Drop the UTF-8 BOM in place, retaining the decompression allocation.
                bytes.drain(..skip);
                Cow::Owned(bytes)
            }
        };
        utf8(bytes)?
    };
    // NUL is not an XML character; also rejects UTF-32 signatures/payloads
    // rather than accepting their ASCII-looking bytes as UTF-8 or UTF-16.
    if xml.contains('\0') {
        return Err("NUL / unsupported UTF-32 XML encoding".into());
    }
    // Inspect only the declaration here. Structure validation/fallback belongs
    // to the existing package parsers, including malformed declaration-free XML.
    if !xml.starts_with("<?xml") {
        if required {
            return Err("BOM-less UTF-16 requires an encoding declaration".into());
        }
        return Ok(xml);
    }
    let mut reader = Reader::from_str(&xml);
    let event = reader.read_event().map_err(|e| e.to_string())?;
    if let Event::Decl(decl) = event {
        let start = BytesStart::from_content(decl.as_ref(), 3);
        for attr in start.attributes() {
            attr.map_err(|e| e.to_string())?;
        }
        let encoding = decl.encoding().transpose().map_err(|e| e.to_string())?;
        let encoding = encoding.as_deref().map(str::to_ascii_uppercase);
        let compatible = match (endian, encoding.as_deref()) {
            (None, None | Some("UTF-8")) => true,
            (Some(_), Some("UTF-16")) => true,
            (Some(true), Some("UTF-16LE")) | (Some(false), Some("UTF-16BE")) => true,
            (Some(_), None) => !required,
            _ => false,
        };
        if !compatible {
            return Err("unsupported or contradictory XML encoding".into());
        }
        let version = decl.version().map_err(|e| e.to_string())?;
        let standalone = decl.standalone().transpose().map_err(|e| e.to_string())?;
        let normalized = BytesDecl::new(&version, Some("UTF-8"), standalone.as_deref());
        let end = reader.buffer_position() as usize;
        let replacement = format!("<?{}?>", normalized.as_ref());
        if xml.len() - end + replacement.len() > limit {
            return Err("XML UTF-8 output limit".into());
        }
        xml.replace_range(..end, &replacement);
    } else if required {
        return Err("BOM-less UTF-16 requires an encoding declaration".into());
    }
    Ok(xml)
}

#[cfg(test)]
pub(crate) fn utf16(xml: &str, le: bool, bom: bool) -> Vec<u8> {
    let mut bytes = if bom {
        if le {
            vec![0xff, 0xfe]
        } else {
            vec![0xfe, 0xff]
        }
    } else {
        vec![]
    };
    for unit in xml.encode_utf16() {
        bytes.extend(if le { unit.to_le_bytes() } else { unit.to_be_bytes() });
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_and_borrowed_text_keep_encoding_and_errors() {
        for path in ["Contents/header.xml", "Contents/content.hpf", "META-INF/container.xml"] {
            for bytes in [
                b"<r>UTF-8</r>".to_vec(),
                b"\xef\xbb\xbf<r/>".to_vec(),
                b"<?xml version='1.0' encoding='utf-8'?><r/>".to_vec(),
                utf16("<?xml version='1.0' encoding='UTF-16'?><r>가😀</r>", true, true),
                utf16("<?xml version='1.0' encoding='UTF-16BE'?><r/>", false, false),
                vec![0xff, 0xfe, 0x3c],
                vec![0xff, 0xfe, 0, 0xd8],
                vec![0xf0, 0x9f],
                b"\0<r/>".to_vec(),
                b"<?xml version='1.0' encoding='UTF-16'?><r/>".to_vec(),
            ] {
                let borrowed = text_entry(path, bytes.as_slice()).map_err(|e| e.to_string());
                let owned = text_entry(path, bytes).map_err(|e| e.to_string());
                assert_eq!(owned, borrowed, "{path}");
            }
            let bytes = vec![b'x', 0xff];
            let detail = std::str::from_utf8(&bytes).unwrap_err().to_string();
            let expected = HwpxError::Zip(format!("read '{path}': {detail}")).to_string();
            assert_eq!(text_entry(path, bytes).unwrap_err().to_string(), expected);
        }
    }

    // BOM stripping must retain an owned UTF-8 allocation, too.
    #[test]
    fn owned_utf8_bom_reuses_allocation() {
        for path in ["Contents/content.hpf", "META-INF/container.xml"] {
            let mut bytes = Vec::with_capacity(128);
            bytes.extend_from_slice(b"\xef\xbb\xbf<r/>");
            let allocation = (bytes.as_ptr(), bytes.capacity());
            let text = text_entry(path, bytes).unwrap();
            assert_eq!(text, "<r/>");
            assert_eq!((text.as_ptr(), text.capacity()), allocation);
        }
    }

    // 이것을 실패시키는 것: UTF-16/BOM 분기 제거, 선언을 UTF-8로 정규화하지 않기.
    #[test]
    fn encoding_acceptance() {
        for le in [true, false] {
            for bom in [true, false] {
                for enc in ["UTF-16", if le { "UTF-16LE" } else { "UTF-16BE" }] {
                    let xml = format!(
                        "<?xml version='1.0' encoding='{enc}' standalone='yes'?><r>가😀UTF-16</r>"
                    );
                    let result = decode(utf16(&xml, le, bom).into(), 1000).unwrap();
                    assert!(result.contains("encoding=\"UTF-8\""), "{result}");
                    assert!(result.ends_with("<r>가😀UTF-16</r>"));
                    assert!(result.contains("standalone=\"yes\""));
                }
            }
            assert_eq!(decode(utf16("<r/>", le, true).into(), 100).unwrap(), "<r/>");
            assert!(decode(utf16("<?xml version='1.0'?><r/>", le, true).into(), 100).is_ok());
        }
        assert_eq!(decode(b"\xef\xbb\xbf<r/>".as_slice().into(), 100).unwrap(), "<r/>");
        assert_eq!(decode(b"<r/>".as_slice().into(), 4).unwrap(), "<r/>");
        // 이것을 실패시키는 것: 선언 없는 본문까지 선행 파싱하여 기존 fallback을 오류로 변경.
        assert_eq!(decode(b"<broken".as_slice().into(), 100).unwrap(), "<broken");
        assert!(
            decode(b"<?xml version='1.0' encoding='utf-8'?><r/>".as_slice().into(), 100).is_ok()
        );
    }

    // 이것을 실패시키는 것: lossy 변환, 선언/BOM 일치 검사 또는 선언 필요 검사 제거.
    #[test]
    fn encoding_rejections() {
        let mut cases = vec![
            vec![0xff],
            vec![0xff, 0xfe, 0x3c],
            vec![0xff, 0xfe, 0x00, 0xd8],
            vec![0xfe, 0xff, 0xdc, 0x00],
            vec![0xff, 0xfe, 0, 0, 0x3c, 0, 0, 0],
            vec![0, 0, 0xfe, 0xff],
            vec![0, 0, 0, 0x3c],
            vec![0x3c, 0, 0, 0],
        ];
        for enc in ["UTF-16", "UTF-16LE", "UTF-16BE", "UTF-32", "ISO-8859-1"] {
            let xml = format!("<?xml version='1.0' encoding='{enc}'?><r/>");
            cases.push(xml.as_bytes().to_vec());
            cases.push([b"\xef\xbb\xbf".as_slice(), xml.as_bytes()].concat());
        }
        for le in [true, false] {
            for enc in ["UTF-8", if le { "UTF-16BE" } else { "UTF-16LE" }, "UTF-32", "latin1"] {
                let xml = format!("<?xml version='1.0' encoding='{enc}'?><r/>");
                cases.push(utf16(&xml, le, true));
                cases.push(utf16(&xml, le, false));
            }
            cases.push(utf16("<?xml version='1.0'?><r/>", le, false));
            cases.push(utf16("<r/>", le, false));
            cases.push(utf16("<?process x?><r/>", le, false));
            cases.push(utf16("<?xml-stylesheet href='x'?><r/>", le, false));
        }
        cases.push(b"<?xml version='1.0' encoding='UTF-8' encoding='UTF-16'?><r/>".to_vec());
        for bytes in cases {
            assert!(decode(bytes.as_slice().into(), 1000).is_err(), "{bytes:?}");
        }
    }

    // 이것을 실패시키는 것: UTF-8 출력 예산 검사 제거 또는 >= 로 경계 축소.
    #[test]
    fn encoding_output_budget() {
        for bytes in ["가가".as_bytes().to_vec(), utf16("가가", true, true)] {
            assert_eq!(decode(bytes.as_slice().into(), 6).unwrap(), "가가");
            assert!(decode(bytes.as_slice().into(), 5).unwrap_err().contains("limit"));
        }
        let declaration = b"<?xml version='1.0'?><r/>";
        let normalized = decode(declaration.as_slice().into(), 100).unwrap();
        assert!(decode(declaration.as_slice().into(), normalized.len()).is_ok());
        assert!(decode(declaration.as_slice().into(), normalized.len() - 1)
            .unwrap_err()
            .contains("limit"));
        let limit = 50 * 1024 * 1024;
        let xml = format!("<r>{}x</r>", "가".repeat((limit - 7) / 3));
        assert_eq!(xml.len(), limit);
        let bytes = utf16(&xml, true, true);
        assert_eq!(text_entry("META-INF/container.xml", bytes.as_slice()).unwrap().len(), limit);
        let overflow = utf16(&xml.replace("</r>", "x</r>"), true, true);
        assert!(text_entry("Contents/content.hpf", overflow.as_slice())
            .unwrap_err()
            .to_string()
            .contains("limit"));
        assert!(text_entry("Contents/section0.xml", utf16("<r/>", true, true)).is_err());
    }
}
