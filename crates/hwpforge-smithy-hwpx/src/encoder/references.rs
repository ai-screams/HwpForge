//! Package-level reference checks run on the manifest the writer is about to
//! write (C4).
//!
//! The checks read the exact `content.hpf` text
//! ([`render_content_hpf`](super::package::render_content_hpf)) that
//! [`PackageWriter::write_hwpx_with_content_hpf`](super::package::PackageWriter::write_hwpx_with_content_hpf)
//! writes, with an XML reader, so an id is compared as a consumer reads it
//! (entities resolved, whitespace normalized), not as the encoder held it.
//! They only report; the bytes are left as they are.
//!
//! Scope: only packages this encoder writes. The preserve-first editors that
//! write the original package back with some parts replaced (`fill`, `patch`,
//! structural insert/delete, through `RawPackage::write` in `patch.rs`) are
//! not checked — they add no reference and no manifest item, so they carry
//! over whatever the input had.

use quick_xml::events::Event;
use quick_xml::{Reader, XmlVersion};

use super::EncodeWarning;
use crate::error::{HwpxError, HwpxResult};

/// The CONTENT_HPF part name, for parse errors.
const CONTENT_HPF: &str = "Contents/content.hpf";

/// `(id, href)` of every `<opf:item>` in `content_hpf`, in document order,
/// as an XML reader reads the attribute values.
///
/// # Errors
///
/// [`HwpxError::XmlParse`] if the text is not well-formed.
pub(crate) fn read_manifest(content_hpf: &str) -> HwpxResult<Vec<(String, String)>> {
    let parse_error = |detail: String| HwpxError::XmlParse { file: CONTENT_HPF.into(), detail };
    let mut items = Vec::new();
    let mut reader = Reader::from_str(content_hpf);
    loop {
        let element = match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) => e,
            Ok(Event::Eof) => break,
            Ok(_) => continue,
            Err(e) => return Err(parse_error(e.to_string())),
        };
        if element.name().as_ref() != "opf:item" {
            continue;
        }
        let (mut id, mut href) = (String::new(), String::new());
        for attr in element.attributes() {
            let attr = attr.map_err(|e| parse_error(e.to_string()))?;
            let slot = match attr.key.as_ref() {
                "id" => &mut id,
                "href" => &mut href,
                _ => continue,
            };
            *slot = attr
                .normalized_value(XmlVersion::Explicit1_0)
                .map_err(|e| parse_error(e.to_string()))?
                .into_owned();
        }
        items.push((id, href));
    }
    Ok(items)
}

/// One [`EncodeWarning::PackageIdConflict`] per manifest id that two or more
/// items share, in the order the id first appears. `items` come from
/// [`read_manifest`].
///
/// The ids are indexed once, so the work grows with the item count (branch
/// review B-M1: a scan of the ids so far per item was quadratic).
/// 이것을 실패시키는 것: 항목마다 앞 id 를 훑기(`checks_stay_linear`).
pub(crate) fn manifest_id_conflicts(items: &[(String, String)]) -> Vec<EncodeWarning> {
    let mut by_id: Vec<(&str, Vec<String>)> = Vec::new();
    let mut index: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for (item_id, item_href) in items {
        crate::cache_flow::tick();
        match index.get(item_id.as_str()) {
            Some(&at) => by_id[at].1.push(item_href.clone()),
            None => {
                index.insert(item_id, by_id.len());
                by_id.push((item_id, vec![item_href.clone()]));
            }
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
/// images and OLE blobs) of `items` (from [`read_manifest`]).
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
    items: &[(String, String)],
    parts: &[(&str, &str)],
) -> HwpxResult<Vec<EncodeWarning>> {
    // Sets, so the work grows with the items and references (B-M1).
    // 이것을 실패시키는 것: 목록 훑기로 되돌리기(`checks_stay_linear`).
    let binary: std::collections::HashSet<&str> = items
        .iter()
        .filter(|(_, href)| href.starts_with("BinData/"))
        .map(|(id, _)| id.as_str())
        .collect();
    let mut warnings = Vec::new();
    for &(part, xml) in parts {
        let parse_error = |detail: String| HwpxError::XmlParse { file: part.to_string(), detail };
        let mut reported: std::collections::HashSet<String> = std::collections::HashSet::new();
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
                crate::cache_flow::tick();
                if reference.is_empty()
                    || binary.contains(reference.as_ref())
                    || !reported.insert(reference.to_string())
                {
                    continue;
                }
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
    use crate::encoder::package::{manifest_items, render_content_hpf};
    use hwpforge_core::metadata::Metadata;

    /// The manifest as written and read back.
    fn written(items: &[crate::encoder::package::ManifestItem]) -> Vec<(String, String)> {
        read_manifest(&render_content_hpf(&Metadata::default(), items)).unwrap()
    }

    fn strings(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|x| x.to_string()).collect()
    }

    /// Branch review B-M1: the manifest conflicts and the reference check
    /// do constant work per item and per reference.
    #[test]
    fn checks_stay_linear() {
        const N: usize = 3000;
        let items: Vec<(String, String)> =
            (0..N).map(|i| (format!("image{i}"), format!("BinData/image{i}.png"))).collect();
        let xml: String = std::iter::once("<r>".to_string())
            .chain((0..N).map(|i| format!(r#"<p binaryItemIDRef="missing{i}"/>"#)))
            .chain(std::iter::once("</r>".to_string()))
            .collect();
        crate::cache_flow::WORK.with(|w| w.set(0));
        assert!(manifest_id_conflicts(&items).is_empty());
        let unresolved = unresolved_references(&items, &[("Contents/section0.xml", &xml)]).unwrap();
        assert_eq!(unresolved.len(), N);
        let work = crate::cache_flow::WORK.with(std::cell::Cell::get);
        assert!(work <= 3 * N, "{work} work units for {N} items and references");
    }

    /// 이것을 실패시키는 것: 겹침 판정을 이미지·OLE 항목 안에서만 하는 것 —
    /// `header.png` 의 id `header` 는 그 집합 안에서는 하나뿐이다 (M12).
    #[test]
    fn an_image_named_like_a_fixed_part_conflicts_with_it() {
        let items = manifest_items(1, &strings(&["header.png"]), &[], &[]);
        assert_eq!(
            manifest_id_conflicts(&written(&items)),
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
            manifest_id_conflicts(&written(&items)),
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
        assert_eq!(manifest_id_conflicts(&written(&items)), []);
    }

    /// A key that would read back as `header` if written raw stays its own id
    /// once escaped.
    ///
    /// 이것을 실패시키는 것: `render_content_hpf` 가 `id` 를 escape 하지 않는 것
    /// (`he&#97;der` 가 `header` 로 읽혀 겹침이 보고된다).
    #[test]
    fn an_entity_lookalike_key_is_written_as_itself() {
        let items = manifest_items(1, &strings(&["he&#97;der.png"]), &[], &[]);
        let read = written(&items);
        assert!(read.contains(&("he&#97;der".to_string(), "BinData/he&#97;der.png".to_string())));
        assert_eq!(manifest_id_conflicts(&read), []);
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
    fn package() -> Vec<(String, String)> {
        written(&manifest_items(1, &strings(&["image1.png"]), &[], &strings(&["ole1"])))
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
