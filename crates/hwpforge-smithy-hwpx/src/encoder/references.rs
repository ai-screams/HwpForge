//! Package-level reference checks run on the manifest the writer is about to
//! write (C4).
//!
//! The checks look at the exact manifest list
//! ([`manifest_items`](super::package::manifest_items)) that
//! [`PackageWriter::write_hwpx_with_manifest`](super::package::PackageWriter::write_hwpx_with_manifest)
//! writes, so what is checked and what is written cannot drift apart. They
//! only report; the bytes are left as they are.

use super::package::ManifestItem;
use super::EncodeWarning;

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
}
