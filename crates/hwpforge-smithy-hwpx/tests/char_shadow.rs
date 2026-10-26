#[path = "support/char_shadow.rs"]
mod shared;
use shared::*;

use std::io::{Cursor, Read};

use hwpforge_core::image::ImageStore;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::{Document, PageSettings, Paragraph};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex, ShadowType};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::HwpxEncoder;

/// A document built in code writes the new-document default for every
/// `charPr` — the value Hancom itself writes, NONE `#C0C0C0` 10 10.
///
/// 이것을 실패시키는 것: 인코더가 DEFAULT 대신 리터럴(옛 `#B2B2B2`)을 쓰는 것,
/// `HwpxCharShape::default()` 가 DEFAULT 아닌 값을 갖는 것.
#[test]
fn a_new_document_writes_the_hancom_default_shadow() {
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::text("가", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    let bytes = HwpxEncoder::encode(&doc.validate().unwrap(), &styles, &ImageStore::new()).unwrap();
    let all = shadows(&header(&bytes));
    assert!(!all.is_empty());
    for s in all {
        assert_eq!(s, r##"<hh:shadow type="NONE" color="#C0C0C0" offsetX="10" offsetY="10"/>"##);
    }
}

/// A Blueprint document: the built-in template with `shadow: drop` on
/// heading1 (kind only). Every `charPr` the registry writes is Hancom's
/// new-document value, and the heading's shadow takes the default color and
/// offsets — the visible change from `#B2B2B2` that the release notes name.
///
/// 이것을 실패시키는 것: 기본 charPr 묶음(`HwpxStyleStore` 의 Hancom 기본 글자 모양)을
/// DEFAULT 아닌 값으로 두는 것, `PartialCharShadow::resolve` 의 기본 색을 바꾸는 것.
#[test]
fn a_blueprint_document_writes_the_default_shadow_and_a_kind_only_drop() {
    use hwpforge_blueprint::builtins::builtin_default;
    use hwpforge_blueprint::registry::StyleRegistry;
    use hwpforge_blueprint::style::PartialCharShadow;

    let mut template = builtin_default().unwrap();
    let mut drop = PartialCharShadow::default();
    drop.kind = Some(ShadowType::Drop);
    template.styles["heading1"].char_shape.as_mut().expect("heading1 char shape").shadow =
        Some(drop);
    let registry = StyleRegistry::from_template(&template).unwrap();
    let styles = HwpxStyleStore::from_registry(&registry).unwrap();

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::text("가", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    let bytes = HwpxEncoder::encode(&doc.validate().unwrap(), &styles, &ImageStore::new()).unwrap();
    let all = shadows(&header(&bytes));
    let (drops, rest): (Vec<_>, Vec<_>) = all.iter().partition(|s| s.contains("DROP"));
    assert_eq!(drops, [r##"<hh:shadow type="DROP" color="#C0C0C0" offsetX="10" offsetY="10"/>"##]);
    assert!(rest.len() >= 7, "{all:?}");
    for s in rest {
        assert_eq!(s, r##"<hh:shadow type="NONE" color="#C0C0C0" offsetX="10" offsetY="10"/>"##);
    }
}
