//! Package-level reference checks run on the manifest the writer is about to
//! write (C4).
//!
//! The checks look at the exact manifest list
//! ([`manifest_items`](super::package::manifest_items)) that
//! [`PackageWriter::write_hwpx_with_manifest`](super::package::PackageWriter::write_hwpx_with_manifest)
//! writes, so what is checked and what is written cannot drift apart. They
//! only report; the bytes are left as they are.
//!
//! Scope: only packages this encoder writes. The preserve-first editors that
//! write the original package back with some parts replaced (`fill`, `patch`,
//! structural insert/delete, through `RawPackage::write` in `patch.rs`) are
//! not checked — they add no reference and no manifest item, so they carry
//! over whatever the input had.

use quick_xml::events::Event;
use quick_xml::{Reader, XmlVersion};

use super::package::ManifestItem;
use super::EncodeWarning;
use crate::error::{HwpxError, HwpxResult};

/// One [`EncodeWarning::PackageIdConflict`] per manifest id that two or more
/// items share, in the order the id first appears.
pub(crate) fn manifest_id_conflicts(items: &[ManifestItem]) -> Vec<EncodeWarning> {
    let mut by_id: Vec<(&str, Vec<String>)> = Vec::new();
    for item in items {
        match by_id.iter_mut().find(|(id, _)| *id == item.id) {
            Some((_, hrefs)) => hrefs.push(item.href.clone()),
            None => by_id.push((&item.id, vec![item.href.clone()])),
        }
    }
    by_id
        .into_iter()
        .filter(|(_, hrefs)| hrefs.len() > 1)
        .map(|(id, hrefs)| EncodeWarning::PackageIdConflict { id: id.to_string(), hrefs })
        .collect()
}

/// One [`EncodeWarning::ImageReferenceUnresolved`] per `(part, reference)`
/// whose `binaryItemIDRef` names no binary item (`href` under `BinData/` —
/// images and OLE blobs) of `items`.
///
/// `parts` are `(part name, XML)` in package order. Every element's
/// `binaryItemIDRef` attribute is read with an XML reader (either quote
/// style), whatever the element. An empty value is not a reference: Hancom
/// writes one for a picture whose asset it lost, and its font substitutes
/// (`hh:substFont`) carry one too.
///
/// # Errors
///
/// [`HwpxError::XmlParse`] if a part is not well-formed — the encoder wrote
/// it, so this is an encoder defect, and a part that cannot be read cannot be
/// checked.
pub(crate) fn unresolved_references(
    items: &[ManifestItem],
    parts: &[(&str, &str)],
) -> HwpxResult<Vec<EncodeWarning>> {
    let binary: Vec<&str> = items
        .iter()
        .filter(|item| item.href.starts_with("BinData/"))
        .map(|item| item.id.as_str())
        .collect();
    let mut warnings = Vec::new();
    for &(part, xml) in parts {
        let parse_error = |detail: String| HwpxError::XmlParse { file: part.to_string(), detail };
        let mut reported: Vec<String> = Vec::new();
        let mut reader = Reader::from_str(xml);
        loop {
            let element = match reader.read_event() {
                Ok(Event::Start(e) | Event::Empty(e)) => e,
                Ok(Event::Eof) => break,
                Ok(_) => continue,
                Err(e) => return Err(parse_error(e.to_string())),
            };
            for attr in element.attributes() {
                let attr = attr.map_err(|e| parse_error(e.to_string()))?;
                if attr.key.as_ref() != "binaryItemIDRef" {
                    continue;
                }
                let reference = attr
                    .normalized_value(XmlVersion::Explicit1_0)
                    .map_err(|e| parse_error(e.to_string()))?;
                if reference.is_empty()
                    || binary.contains(&reference.as_ref())
                    || reported.iter().any(|r| *r == reference)
                {
                    continue;
                }
                reported.push(reference.to_string());
                warnings.push(EncodeWarning::ImageReferenceUnresolved {
                    part: part.to_string(),
                    location: format!("{}@binaryItemIDRef", element.name().as_ref()),
                    reference: reference.into_owned(),
                });
            }
        }
    }
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoder::package::manifest_items;

    fn strings(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|x| x.to_string()).collect()
    }

    /// 이것을 실패시키는 것: 겹침 판정을 이미지·OLE 항목 안에서만 하는 것 —
    /// `header.png` 의 id `header` 는 그 집합 안에서는 하나뿐이다 (M12).
    #[test]
    fn an_image_named_like_a_fixed_part_conflicts_with_it() {
        let items = manifest_items(1, &strings(&["header.png"]), &[], &[]);
        assert_eq!(
            manifest_id_conflicts(&items),
            [EncodeWarning::PackageIdConflict {
                id: "header".into(),
                hrefs: strings(&["Contents/header.xml", "BinData/header.png"]),
            }]
        );
    }

    /// 이것을 실패시키는 것: 겹침을 `id` 가 아니라 `href`(확장자 포함)로 판정하는 것.
    #[test]
    fn two_images_with_one_stem_conflict() {
        let items =
            manifest_items(1, &strings(&["a.png", "b.png", "a.jpg"]), &[], &["ole1".into()]);
        assert_eq!(
            manifest_id_conflicts(&items),
            [EncodeWarning::PackageIdConflict {
                id: "a".into(),
                hrefs: strings(&["BinData/a.png", "BinData/a.jpg"]),
            }]
        );
    }

    /// 이것을 실패시키는 것: 항목이 하나뿐인 id 도 보고하는 것 (`> 1` → `>= 1`).
    #[test]
    fn distinct_ids_report_nothing() {
        let items = manifest_items(
            2,
            &strings(&["image1.png", "image2.jpg"]),
            &strings(&["Contents/masterpage0.xml"]),
            &strings(&["ole1"]),
        );
        assert_eq!(manifest_id_conflicts(&items), []);
    }

    fn unresolved(warnings: &[EncodeWarning]) -> Vec<(&str, &str, &str)> {
        warnings
            .iter()
            .map(|w| match w {
                EncodeWarning::ImageReferenceUnresolved { part, location, reference } => {
                    (part.as_str(), location.as_str(), reference.as_str())
                }
                other => panic!("unexpected {other:?}"),
            })
            .collect()
    }

    /// A package with one image (`image1`) and one OLE blob (`ole1`).
    fn package() -> Vec<ManifestItem> {
        manifest_items(1, &strings(&["image1.png"]), &[], &strings(&["ole1"]))
    }

    /// 이것을 실패시키는 것: 해석 집합에서 OLE 항목을 빼는 것, 또는 이미지
    /// id 대신 `href` 의 파일 이름(`image1.png`)으로 대조하는 것.
    #[test]
    fn image_and_ole_items_resolve() {
        let section = r#"<hs:sec><hp:pic><hc:img binaryItemIDRef="image1"/></hp:pic><hp:ole binaryItemIDRef="ole1"/></hs:sec>"#;
        let found = unresolved_references(&package(), &[("Contents/section0.xml", section)]);
        assert_eq!(unresolved(&found.unwrap()), []);
    }

    /// 이것을 실패시키는 것: 해석 집합을 manifest 전체 id 로 넓히는 것 —
    /// `header`·`section0` 은 XML part 라 그림이 될 수 없다.
    #[test]
    fn a_reference_to_an_xml_part_id_is_unresolved() {
        let header = r#"<hh:head><hc:img binaryItemIDRef="section0"/></hh:head>"#;
        let found = unresolved_references(&package(), &[("Contents/header.xml", header)]);
        assert_eq!(
            unresolved(&found.unwrap()),
            [("Contents/header.xml", "hc:img@binaryItemIDRef", "section0")]
        );
    }

    /// 이것을 실패시키는 것: XML 리더 대신 `binaryItemIDRef="` 문자열 needle 로
    /// 찾는 것.
    #[test]
    fn a_single_quoted_reference_is_read() {
        let section = "<hs:sec><hc:img binaryItemIDRef='x'/></hs:sec>";
        let found = unresolved_references(&package(), &[("Contents/section0.xml", section)]);
        assert_eq!(
            unresolved(&found.unwrap()),
            [("Contents/section0.xml", "hc:img@binaryItemIDRef", "x")]
        );
    }

    /// 한컴 header 의 `hh:substFont binaryItemIDRef=""` (코퍼스 3,256 문서)와
    /// 한컴이 다시 저장한 깨진 그림의 빈 참조는 참조가 아니다.
    ///
    /// 이것을 실패시키는 것: 빈 값 건너뛰기를 지우는 것.
    #[test]
    fn an_empty_value_is_not_a_reference() {
        let header = r#"<hh:head><hh:substFont face="x" type="TTF" isEmbedded="0" binaryItemIDRef=""/></hh:head>"#;
        let found = unresolved_references(&package(), &[("Contents/header.xml", header)]);
        assert_eq!(unresolved(&found.unwrap()), []);
    }

    /// 같은 part 의 같은 참조는 한 번(처음 본 자리), 다른 part 는 따로.
    ///
    /// 이것을 실패시키는 것: part 안 합치기를 지우는 것, 또는 합친 목록을
    /// part 를 넘어 유지하는 것.
    #[test]
    fn reports_merge_per_part_and_reference() {
        let section = r#"<hs:sec><hp:ole binaryItemIDRef="a"/><hc:img binaryItemIDRef="a"/><hc:img binaryItemIDRef="b"/></hs:sec>"#;
        let master = r#"<hm:masterPage><hc:img binaryItemIDRef="a"/></hm:masterPage>"#;
        let found = unresolved_references(
            &package(),
            &[("Contents/section0.xml", section), ("Contents/masterpage0.xml", master)],
        );
        assert_eq!(
            unresolved(&found.unwrap()),
            [
                ("Contents/section0.xml", "hp:ole@binaryItemIDRef", "a"),
                ("Contents/section0.xml", "hc:img@binaryItemIDRef", "b"),
                ("Contents/masterpage0.xml", "hc:img@binaryItemIDRef", "a"),
            ]
        );
    }

    /// 이것을 실패시키는 것: 읽기 오류에서 그 part 검사를 조용히 멈추는 것.
    #[test]
    fn a_part_that_is_not_well_formed_is_an_error() {
        let broken = r#"<hs:sec><hc:img binaryItemIDRef="x"></hs:sec>"#;
        let err = unresolved_references(&package(), &[("Contents/section0.xml", broken)])
            .expect_err("mismatched end tag");
        assert!(
            matches!(&err, HwpxError::XmlParse { file, .. } if file == "Contents/section0.xml"),
            "{err:?}"
        );
    }
}
