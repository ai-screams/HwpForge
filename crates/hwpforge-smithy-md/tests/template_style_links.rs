use hwpforge_blueprint::{
    builtins::{builtin_default, builtin_gov_proposal},
    inheritance::{resolve_template, TemplateProvider},
    markdown::{apply_markdown_heading_levels, resolve_markdown_style_names},
    registry::StyleRegistry,
    template::Template,
};
use hwpforge_core::{image::ImageStore, ParagraphListRef};
use hwpforge_smithy_hwpx::{
    HancomStyleSet, HwpxDecoder, HwpxEncoder, HwpxRegistryBridge, HwpxStyleLookup,
};
use hwpforge_smithy_md::{MdDecoder, MdEncoder};
use std::{
    collections::HashMap,
    io::{Cursor, Read},
};
#[path = "../../hwpforge-smithy-hwpx/tests/support/xml.rs"]
mod xml;
use xml::Element;

fn wire(md: &str, template: &Template, set: HancomStyleSet) -> Vec<u8> {
    let decoded = MdDecoder::decode(md, template).unwrap();
    let bridge = HwpxRegistryBridge::from_registry_with(&decoded.style_registry, set).unwrap();
    let doc = bridge.rebind_draft_document(decoded.document).unwrap().validate().unwrap();
    HwpxEncoder::encode(&doc, bridge.style_store(), &ImageStore::new()).unwrap()
}
fn part(bytes: &[u8], name: &str) -> String {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    archive.by_name(name).unwrap().read_to_string(&mut text).unwrap();
    text
}
fn nodes<'a>(node: &'a Element, name: &str) -> Vec<&'a Element> {
    let mut result = Vec::new();
    if node.name == name {
        result.push(node);
    }
    for child in &node.children {
        result.extend(nodes(child, name));
    }
    result
}

#[test]
fn selected_entries_join_wire_definitions_in_every_default_set() {
    // 이것을 실패시키는 것: decoder가 기본 개요 참조를 덧씌우거나 body 참조를 생략.
    for set in [HancomStyleSet::Classic, HancomStyleSet::Modern, HancomStyleSet::Latest] {
        let bytes = wire("# First\n\nBody\n\n## Second\n\nBody", &builtin_default().unwrap(), set);
        let header = Element::parse(&part(&bytes, "Contents/header.xml"));
        let section = Element::parse(&part(&bytes, "Contents/section0.xml"));
        let styles = nodes(&header, "hh:style");
        let shapes = nodes(&header, "hh:paraPr");
        let paragraphs = nodes(&section, "hp:p");
        assert_eq!(paragraphs.len(), 4);
        assert_eq!(nodes(&section, "hp:titleMark").len(), 2);
        for (i, (name, kind, level)) in [
            ("heading1", "OUTLINE", "0"),
            ("body", "NONE", "0"),
            ("heading2", "OUTLINE", "1"),
            ("body", "NONE", "0"),
        ]
        .into_iter()
        .enumerate()
        {
            let p = paragraphs[i];
            let style = styles[p.attr("styleIDRef").unwrap().parse::<usize>().unwrap()];
            assert_eq!(style.attr("name"), Some(name));
            assert_eq!(p.attr("paraPrIDRef"), style.attr("paraPrIDRef"));
            assert_eq!(p.find("hp:run").attr("charPrIDRef"), style.attr("charPrIDRef"));
            let shape = shapes[p.attr("paraPrIDRef").unwrap().parse::<usize>().unwrap()];
            assert_eq!(shape.find("hh:heading").attr("type"), Some(kind));
            assert_eq!(shape.find("hh:heading").attr("level"), Some(level));
        }
    }
}

#[test]
fn builtin_headings_keep_appearance_and_roundtrip_semantics() {
    // 이것을 실패시키는 것: 개요 적용 생략 또는 gov H4–H6 외형 override 제거.
    let md = "# One\n\n## Two\n\n### Three\n\n#### Four\n\n##### Five\n\n###### Six\n\nBody\n\n- [ ] task\n  - [x] nested";
    for (template, sizes) in [
        (builtin_default().unwrap(), [1600, 1400, 1200, 1200, 1200, 1200]),
        (builtin_gov_proposal().unwrap(), [1800, 1500, 1300, 1300, 1300, 1300]),
    ] {
        let decoded = MdDecoder::decode(md, &template).unwrap();
        for (i, p) in decoded.document.sections()[0].paragraphs.iter().take(6).enumerate() {
            let entry = decoded.style_registry.get_style(&format!("heading{}", i + 1)).unwrap();
            assert_eq!(p.style_ref, Some(entry.style_ref()));
            assert_eq!(p.para_shape_id, entry.para_shape_id);
            assert_eq!(
                decoded.style_registry.para_shape(p.para_shape_id).unwrap().list,
                Some(ParagraphListRef::Outline { level: i as u8 })
            );
            assert_eq!(
                decoded.style_registry.char_shape(entry.char_shape_id).unwrap().size.as_i32(),
                sizes[i]
            );
        }
        let lossy = MdEncoder::encode(&decoded.document.validate().unwrap(), &template).unwrap();
        assert_eq!(lossy.lines().find(|line| line.starts_with("######")), Some("###### Six"));
        assert_eq!(lossy.lines().find(|line| line.contains("nested")), Some("  - [x] nested"));
        let bytes = wire(md, &template, HancomStyleSet::Modern);
        let decoded = HwpxDecoder::decode(&bytes).unwrap();
        let lookup = HwpxStyleLookup::new(&decoded.style_store, &decoded.image_store);
        let styled = MdEncoder::encode_styled(&decoded.document.validate().unwrap(), &lookup);
        assert_eq!(
            styled.markdown.lines().find(|line| line.starts_with("######")),
            Some("###### **Six**")
        );
        assert_eq!(
            styled.markdown.lines().find(|line| line.contains("nested")),
            Some("  - [x] nested")
        );
        let again = wire(&styled.markdown, &template, HancomStyleSet::Modern);
        let decoded = HwpxDecoder::decode(&again).unwrap();
        let lookup = HwpxStyleLookup::new(&decoded.style_store, &decoded.image_store);
        assert_eq!(
            MdEncoder::encode_styled(&decoded.document.validate().unwrap(), &lookup)
                .markdown
                .lines()
                .find(|line| line.starts_with("######")),
            Some("###### **Six**")
        );
    }
}

struct Provider(HashMap<String, Template>);
impl TemplateProvider for Provider {
    fn get_template(&self, name: &str) -> Option<&Template> {
        self.0.get(name)
    }
    fn is_builtin_template(&self, name: &str) -> bool {
        matches!(name, "default" | "gov_proposal")
    }
}

#[test]
fn inheritance_shared_roles_and_legacy_matrix() {
    // 이것을 실패시키는 것: 공유 entry 갱신, legacy 목록 덮어쓰기, 내장 호환 후보 제한 제거.
    for inheritance in 0..4 {
        for role in 0..5 {
            for legacy in [false, true] {
                let extends = [
                    "",
                    "  extends: default\n",
                    "  extends: gov_proposal\n",
                    "  extends: parent\n",
                ][inheritance];
                let declaration = if legacy {
                    "      heading_type: Outline\n"
                } else {
                    "      alignment: Left\n"
                };
                let mapping = [
                    "",
                    "  body: heading3\n",
                    "  list_item: heading3\n",
                    "  heading4: heading3\n  heading5: heading3\n  heading6: heading3\n",
                    "",
                ][role];
                let yaml = format!("meta:\n  name: leaf\n  version: 1.0.0\n{extends}styles:\n  body:\n    char_shape: {{font: Test, size: 10pt}}\n    para_shape: {{alignment: Left}}\n  heading3:\n    char_shape:\n      font: Test\n      size: 19pt\n    para_shape:\n{declaration}  alias:\n    char_shape: {{font: Test, size: 19pt}}\n    para_shape:\n{declaration}markdown_mapping:\n  heading3: heading3\n{mapping}");
                let mut template = Template::from_yaml(&yaml).unwrap();
                if role != 3 {
                    let mut separate = template.styles["heading3"].clone();
                    separate.para_shape.as_mut().unwrap().heading_type = None;
                    for name in ["heading4", "heading5", "heading6"] {
                        template.styles.insert(name.into(), separate.clone());
                    }
                }
                let parent = Template::from_yaml("meta:\n  name: parent\n  version: 1.0.0\n  extends: default\nstyles:\n  heading3:\n    char_shape:\n      size: 17pt\n").unwrap();
                let provider = Provider(HashMap::from([
                    ("default".into(), builtin_default().unwrap()),
                    ("gov_proposal".into(), builtin_gov_proposal().unwrap()),
                    ("parent".into(), parent),
                ]));
                let resolved = resolve_template(&template, &provider).unwrap();
                assert_eq!(
                    resolved.markdown_mapping.as_ref().unwrap().heading6.as_deref(),
                    Some(if role == 3 { "heading3" } else { "heading6" })
                );
                let decoded =
                    MdDecoder::decode("### Three\n\n###### Six\n\nBody", &resolved).unwrap();
                let registry = &decoded.style_registry;
                let entry = registry.get_style("heading3").unwrap();
                for (i, level) in [2, 5].into_iter().enumerate() {
                    let p = &decoded.document.sections()[0].paragraphs[i];
                    assert_eq!(
                        p.style_ref,
                        Some(if i == 1 && role != 3 {
                            registry.get_style("heading6").unwrap().style_ref()
                        } else {
                            entry.style_ref()
                        })
                    );
                    assert_eq!(
                        registry.para_shape(p.para_shape_id).unwrap().list,
                        Some(ParagraphListRef::Outline { level })
                    );
                }
                assert_eq!(
                    registry.para_shape(entry.para_shape_id).unwrap().list,
                    if legacy {
                        Some(ParagraphListRef::Outline { level: 0 })
                    } else if role == 0 || role == 4 {
                        Some(ParagraphListRef::Outline { level: 2 })
                    } else {
                        None
                    }
                );
                let bytes = wire("### Three\n\n###### Six", &resolved, HancomStyleSet::Modern);
                let back = HwpxDecoder::decode(&bytes).unwrap();
                let lookup = HwpxStyleLookup::new(&back.style_store, &back.image_store);
                assert!(MdEncoder::encode_styled(&back.document.validate().unwrap(), &lookup)
                    .markdown
                    .replace("**", "")
                    .contains("###### Six"));
            }
        }
    }
}

#[test]
fn shared_paragraph_position_does_not_mutate_alias() {
    // 이것을 실패시키는 것: 원본 P 제자리 수정.
    let template = builtin_default().unwrap();
    let names = resolve_markdown_style_names(&template).unwrap();
    let mut registry = StyleRegistry::from_template(&template).unwrap();
    let heading = *registry.get_style("heading3").unwrap();
    registry
        .register_style(
            "alias".into(),
            heading.char_shape_id,
            heading.para_shape_id,
            heading.font_id,
        )
        .unwrap();
    let alias = *registry.get_style("alias").unwrap();
    assert_eq!(heading.para_shape_id, alias.para_shape_id);
    assert_ne!(heading.style_ref(), alias.style_ref());
    apply_markdown_heading_levels(&mut registry, &names).unwrap();
    assert_eq!(registry.get_style("alias").unwrap().para_shape_id, alias.para_shape_id);
    assert_eq!(registry.para_shape(alias.para_shape_id).unwrap().list, None);
    assert_ne!(registry.get_style("heading3").unwrap().para_shape_id, alias.para_shape_id);
}

#[test]
fn nearest_declarations_and_first_entry_reach_public_decoder() {
    // 이것을 실패시키는 것: 상속 fold 방향 역전 또는 첫 entry 대신 임의 body 참조 선택.
    for field in [false, true] {
        for mapping in [false, true] {
            let mut root = builtin_default().unwrap();
            root.meta.name = "root".into();
            root.markdown_mapping = None;
            let original = root.styles["body"].clone();
            root.styles.clear();
            root.styles.insert("root_only".into(), original.clone());
            let mut grand = root.clone();
            grand.meta.name = "grand".into();
            grand.meta.extends = Some("root".into());
            grand.styles.clear();
            grand.styles.insert("parent_only".into(), original);
            let mut parent = grand.clone();
            parent.meta.name = "parent".into();
            parent.meta.extends = Some("grand".into());
            parent.styles.clear();
            if field {
                let mut style = root.styles["root_only"].clone();
                style.char_shape.as_mut().unwrap().size =
                    Some(hwpforge_foundation::HwpUnit::new(2300).unwrap());
                parent.styles.insert("root_only".into(), style);
            }
            if mapping {
                let md = hwpforge_blueprint::template::MarkdownMapping {
                    body: Some("parent_only".into()),
                    ..Default::default()
                };
                parent.markdown_mapping = Some(md);
            }
            let mut leaf = parent.clone();
            leaf.meta.name = "leaf".into();
            leaf.meta.extends = Some("parent".into());
            leaf.styles.clear();
            leaf.markdown_mapping = None;
            let provider = HashMap::from([
                ("root".into(), root),
                ("grand".into(), grand),
                ("parent".into(), parent),
            ]);
            let resolved = resolve_template(&leaf, &provider).unwrap();
            let decoded = MdDecoder::decode("Body\n\n###### Six", &resolved).unwrap();
            let name = if mapping { "parent_only" } else { "root_only" };
            let entry = decoded.style_registry.get_style(name).unwrap();
            assert_eq!(
                decoded.document.sections()[0].paragraphs[0].style_ref,
                Some(entry.style_ref())
            );
            assert_eq!(
                decoded.document.sections()[0].paragraphs[1].style_ref,
                Some(entry.style_ref())
            );
            let root_entry = decoded.style_registry.get_style("root_only").unwrap();
            assert_eq!(
                decoded.style_registry.char_shape(root_entry.char_shape_id).unwrap().size.as_i32(),
                if field { 2300 } else { 1000 }
            );
            assert_eq!(resolved.styles.keys().next().unwrap(), "root_only");
            let again = resolve_template(&resolved, &provider).unwrap();
            assert_eq!(again.markdown_mapping, resolved.markdown_mapping);
        }
    }
}

#[test]
fn list_notes_tables_and_inline_runs_keep_selected_identity() {
    // 이것을 실패시키는 것: builder/table/note 참조 누락 또는 task/continuation 참조 혼동.
    let md = "Body **bold**[^1] and[^e1]\n\n1. [ ] task\n   - [x] nested\n\n     continuation\n\n| A | B |\n| - | - |\n| C | D |\n\n[^1]: note\n\n[^e1]: end";
    let decoded = MdDecoder::decode(md, &builtin_default().unwrap()).unwrap();
    decoded.document.for_each_paragraph(|p| {
        assert!(p.style_ref.is_some(), "{}", p.text_content());
        for (text, name) in [("note", "__hwpforge_md_footnote"), ("end", "__hwpforge_md_endnote")] {
            if p.text_content().ends_with(text) {
                assert_eq!(
                    p.style_ref,
                    Some(decoded.style_registry.get_style(name).unwrap().style_ref())
                );
            }
        }
    });
    let body = &decoded.document.sections()[0].paragraphs[0];
    let entry = decoded.style_registry.get_style("body").unwrap();
    assert_eq!(body.style_ref, Some(entry.style_ref()));
    assert!(body.runs.iter().any(|r| r.char_shape_id != entry.char_shape_id));
    let list = decoded.style_registry.get_style("list_item").unwrap();
    for p in &decoded.document.sections()[0].paragraphs[1..3] {
        assert_eq!(p.style_ref, Some(list.style_ref()));
    }
    let continuation = decoded.document.sections()[0]
        .paragraphs
        .iter()
        .find(|p| p.text_content() == "continuation")
        .unwrap();
    let shape = decoded.style_registry.para_shape(continuation.para_shape_id).unwrap();
    assert_eq!(shape.list, None);
    assert_eq!(shape.indent_first_line.as_i32(), 0);
    assert!(shape.indent_left.as_i32() > 0);
}

#[test]
fn invalid_names_and_mapping_still_fail() {
    // 이것을 실패시키는 것: 이름 검증 생략 또는 없는 명시 mapping을 폴백으로 은폐.
    for name in ["한국어", "with space"] {
        let mut template = builtin_default().unwrap();
        template.styles.insert(name.into(), template.styles["body"].clone());
        assert!(MdDecoder::decode("Body", &template).is_err());
    }
    let mut template = builtin_default().unwrap();
    template.markdown_mapping.as_mut().unwrap().heading4 = Some("missing".into());
    assert!(MdDecoder::decode("#### Four", &template).is_err());
    let mut template = builtin_default().unwrap();
    let p = template.styles.get_mut("heading3").unwrap().para_shape.as_mut().unwrap();
    p.heading_type = Some(hwpforge_foundation::HeadingType::Outline);
    p.list = Some(serde_yaml::from_str("kind: outline\nlevel: 2").unwrap());
    assert!(MdDecoder::decode("### Three", &template).is_err());
}

#[test]
fn explicit_lists_preserve_entry_and_isolate_heading_paragraph() {
    // 이것을 실패시키는 것: 명시 목록 충돌을 entry 갱신으로 처리.
    use hwpforge_blueprint::style::PartialParagraphListRef;
    for inheritance in 0..4 {
        for role in 0..5 {
            for list in [
                PartialParagraphListRef::Outline { level: 2 },
                PartialParagraphListRef::Outline { level: 0 },
                PartialParagraphListRef::Number { numbering_id: 1, level: 0 },
                PartialParagraphListRef::Bullet { bullet_id: 1, level: 0 },
            ] {
                let mut template = builtin_default().unwrap();
                template.numberings.push(hwpforge_core::NumberingDef::default_outline());
                template.bullets.push(hwpforge_core::BulletDef {
                    id: 1,
                    bullet_char: "•".into(),
                    checked_char: None,
                    use_image: false,
                    para_head: hwpforge_core::ParaHead {
                        start: 0,
                        level: 1,
                        num_format: hwpforge_foundation::NumberFormatType::Digit,
                        text: String::new(),
                        checkable: false,
                    },
                });
                template.styles.get_mut("heading3").unwrap().para_shape.as_mut().unwrap().list =
                    Some(list);
                let md = template.markdown_mapping.as_mut().unwrap();
                match role {
                    1 => md.body = Some("heading3".into()),
                    2 => md.list_item = Some("heading3".into()),
                    3 => md.heading4 = Some("heading3".into()),
                    _ => {}
                }
                if role == 4 {
                    template.styles.insert("alias".into(), template.styles["heading3"].clone());
                }
                template.meta.name = "leaf".into();
                template.meta.extends = [
                    None,
                    Some("default".into()),
                    Some("gov_proposal".into()),
                    Some("parent".into()),
                ][inheritance]
                    .clone();
                let mut parent = builtin_default().unwrap();
                parent.meta.name = "parent".into();
                parent.meta.extends = Some("default".into());
                let provider = Provider(HashMap::from([
                    ("default".into(), builtin_default().unwrap()),
                    ("gov_proposal".into(), builtin_gov_proposal().unwrap()),
                    ("parent".into(), parent),
                ]));
                let template = resolve_template(&template, &provider).unwrap();
                let before = StyleRegistry::from_template(&template).unwrap();
                let original = before
                    .para_shape(before.get_style("heading3").unwrap().para_shape_id)
                    .unwrap()
                    .list;
                let decoded = MdDecoder::decode("### Three", &template).unwrap();
                let entry = decoded.style_registry.get_style("heading3").unwrap();
                assert_eq!(
                    decoded.style_registry.para_shape(entry.para_shape_id).unwrap().list,
                    original
                );
                let p = &decoded.document.sections()[0].paragraphs[0];
                assert_eq!(p.style_ref, Some(entry.style_ref()));
                assert_eq!(
                    decoded.style_registry.para_shape(p.para_shape_id).unwrap().list,
                    Some(ParagraphListRef::Outline { level: 2 })
                );
            }
        }
    }
}

#[test]
fn builtin_candidates_respect_provider_and_user_declarations() {
    // 이것을 실패시키는 것: provider 표식을 무시하거나 중간 부모에서 mapping을 조기 구체화.
    let leaf = Template::from_yaml("meta:\n  name: leaf\n  version: 1.0.0\n  extends: default\nstyles:\n  heading3:\n    char_shape:\n      size: 19pt\n").unwrap();
    let decoded = MdDecoder::decode("###### Six", &leaf).unwrap();
    assert_eq!(
        decoded.document.sections()[0].paragraphs[0].style_ref,
        Some(decoded.style_registry.get_style("heading3").unwrap().style_ref())
    );
    let provider = HashMap::from([("default".into(), builtin_default().unwrap())]);
    assert!(!provider.is_builtin_template("default"));
    let resolved = resolve_template(&leaf, &provider).unwrap();
    assert_eq!(resolved.markdown_mapping.unwrap().heading6.as_deref(), Some("heading6"));
    let mut parent = leaf.clone();
    parent.meta.name = "parent".into();
    let mut child = leaf.clone();
    child.meta.extends = Some("parent".into());
    child.styles.clear();
    child.styles.insert("heading4".into(), parent.styles["heading3"].clone());
    let provider = Provider(HashMap::from([
        ("default".into(), builtin_default().unwrap()),
        ("parent".into(), parent),
    ]));
    let resolved = resolve_template(&child, &provider).unwrap();
    let decoded = MdDecoder::decode("###### Six", &resolved).unwrap();
    assert_eq!(
        decoded.document.sections()[0].paragraphs[0].style_ref,
        Some(decoded.style_registry.get_style("heading4").unwrap().style_ref())
    );
    child.markdown_mapping = Some(hwpforge_blueprint::template::MarkdownMapping {
        heading6: Some("heading6".into()),
        ..Default::default()
    });
    let resolved = resolve_template(&child, &provider).unwrap();
    assert_eq!(resolved.markdown_mapping.unwrap().heading6.as_deref(), Some("heading6"));
}

#[test]
fn explicit_alias_mapping_survives_registration_order() {
    // 이것을 실패시키는 것: 선택 entry 대신 P/C 동등 entry 또는 등록 위치로 참조 결정.
    for reverse in [false, true] {
        let mut template = builtin_default().unwrap();
        let mut md = template.markdown_mapping.take().unwrap();
        md.body = Some("prose".into());
        md.heading1 = Some("title".into());
        md.list_item = Some("items".into());
        template.styles.insert("prose".into(), template.styles["body"].clone());
        template.styles.insert("title".into(), template.styles["heading1"].clone());
        template.styles.insert("items".into(), template.styles["list_item"].clone());
        if reverse {
            template.styles.reverse();
        }
        template.markdown_mapping = Some(md);
        let decoded = MdDecoder::decode("# Title\n\nBody\n\n- [x] task", &template).unwrap();
        for (p, name) in
            decoded.document.sections()[0].paragraphs.iter().zip(["title", "prose", "items"])
        {
            assert_eq!(
                p.style_ref,
                Some(decoded.style_registry.get_style(name).unwrap().style_ref())
            );
        }
    }
}

#[test]
fn same_field_and_mapping_overrides_reach_selected_reference() {
    // 이것을 실패시키는 것: 가까운 부모/leaf의 같은 필드 또는 mapping 덮어쓰기를 역전.
    for field in [false, true] {
        for mapping in [false, true] {
            let mut root = builtin_default().unwrap();
            root.meta.name = "root".into();
            for alias in ["root_alias", "parent_alias", "leaf_alias"] {
                root.styles.insert(alias.into(), root.styles["heading3"].clone());
            }
            root.markdown_mapping.as_mut().unwrap().heading3 = Some("root_alias".into());
            let mut parent = root.clone();
            parent.meta.name = "parent".into();
            parent.meta.extends = Some("root".into());
            parent.styles.clear();
            let mut partial = root.styles["heading3"].clone();
            partial.char_shape.as_mut().unwrap().size =
                Some(hwpforge_foundation::HwpUnit::new(2000).unwrap());
            parent.styles.insert("heading3".into(), partial.clone());
            parent.markdown_mapping.as_mut().unwrap().heading3 = Some("parent_alias".into());
            let mut leaf = parent.clone();
            leaf.meta.name = "leaf".into();
            leaf.meta.extends = Some("parent".into());
            leaf.styles.clear();
            leaf.markdown_mapping = None;
            if field {
                partial.char_shape.as_mut().unwrap().size =
                    Some(hwpforge_foundation::HwpUnit::new(2400).unwrap());
                leaf.styles.insert("heading3".into(), partial);
            }
            if mapping {
                leaf.markdown_mapping = Some(hwpforge_blueprint::template::MarkdownMapping {
                    heading3: Some("leaf_alias".into()),
                    ..Default::default()
                });
            }
            let resolved = resolve_template(&leaf, &vec![root, parent]).unwrap();
            let decoded = MdDecoder::decode("### Three", &resolved).unwrap();
            let entry = decoded
                .style_registry
                .get_style(if mapping { "leaf_alias" } else { "parent_alias" })
                .unwrap();
            assert_eq!(
                decoded.document.sections()[0].paragraphs[0].style_ref,
                Some(entry.style_ref())
            );
            let heading = decoded.style_registry.get_style("heading3").unwrap();
            assert_eq!(
                decoded.style_registry.char_shape(heading.char_shape_id).unwrap().size.as_i32(),
                if field { 2400 } else { 2000 }
            );
        }
    }
}

#[test]
fn checked_depth_and_ordered_task_roundtrip() {
    // 이것을 실패시키는 것: task checked/level 값을 고정하거나 선택 목록 ref를 생략.
    for ordered in [false, true] {
        for checked in [false, true] {
            let mark = if checked { "x" } else { " " };
            let prefix = if ordered { "1." } else { "-" };
            let indent = if ordered { "   " } else { "  " };
            let md = format!("{prefix} [{mark}] outer\n{indent}- [{mark}] inner");
            let template = builtin_default().unwrap();
            let decoded = MdDecoder::decode(&md, &template).unwrap();
            for (i, p) in decoded.document.sections()[0].paragraphs.iter().enumerate() {
                assert_eq!(
                    p.style_ref,
                    Some(decoded.style_registry.get_style("list_item").unwrap().style_ref())
                );
                assert!(
                    matches!(decoded.style_registry.para_shape(p.para_shape_id).unwrap().list, Some(ParagraphListRef::CheckBullet { level, checked: state, .. }) if usize::from(level) == i && state == checked)
                );
            }
            let bytes = wire(&md, &template, HancomStyleSet::Modern);
            let back = HwpxDecoder::decode(&bytes).unwrap();
            let lookup = HwpxStyleLookup::new(&back.style_store, &back.image_store);
            let styled = MdEncoder::encode_styled(&back.document.validate().unwrap(), &lookup);
            assert_eq!(
                styled.markdown.lines().find(|line| line.contains("outer")),
                Some(format!("- [{mark}] outer").as_str())
            );
            assert_eq!(
                styled.markdown.lines().find(|line| line.contains("inner")),
                Some(format!("  - [{mark}] inner").as_str())
            );
        }
    }
    let decoded = MdDecoder::decode("", &builtin_default().unwrap()).unwrap();
    assert_eq!(
        decoded.document.sections()[0].paragraphs[0].style_ref,
        Some(decoded.style_registry.get_style("body").unwrap().style_ref())
    );
}

#[test]
fn heading_conflicts_are_returned_as_decoder_warnings() {
    // 이것을 실패시키는 것: 충돌 수집 또는 decoder 경고 전달 삭제.
    let mut template = builtin_default().unwrap();
    template.styles.get_mut("heading1").unwrap().para_shape.as_mut().unwrap().heading_type =
        Some(hwpforge_foundation::HeadingType::Outline);
    template.markdown_mapping.get_or_insert_with(Default::default).heading3 =
        Some("heading1".into());
    let decoded = MdDecoder::decode("# Title", &template).unwrap();
    assert!(decoded.warnings.iter().any(|warning| matches!(warning,
        hwpforge_smithy_md::MdWarning::HeadingListConflict { level: 3, style_name }
        if style_name == "heading1")));
}

#[test]
fn unused_original_heading_shapes_have_no_wire_references() {
    // 이것을 실패시키는 것: 개요 적용 전 P로 스타일 또는 문단 참조를 되돌리기.
    let template = builtin_default().unwrap();
    let before = StyleRegistry::from_template(&template).unwrap();
    let decoded = MdDecoder::decode("# One", &template).unwrap();
    let bridge =
        HwpxRegistryBridge::from_registry_with(&decoded.style_registry, HancomStyleSet::Modern)
            .unwrap();
    let original = before.get_style("heading1").unwrap().para_shape_id;
    let old_shape = decoded.style_registry.para_shape(original).unwrap();
    assert_eq!(old_shape.list, None);
    let bytes = wire("# One", &template, HancomStyleSet::Modern);
    let header = Element::parse(&part(&bytes, "Contents/header.xml"));
    let section = Element::parse(&part(&bytes, "Contents/section0.xml"));
    let mut probe = decoded.document;
    probe.sections_mut()[0].paragraphs[0].para_shape_id = original;
    let old_position = bridge.rebind_draft_document(probe).unwrap().sections()[0].paragraphs[0]
        .para_shape_id
        .get()
        .to_string();
    assert_eq!(
        nodes(&header, "hh:paraPr")[old_position.parse::<usize>().unwrap()]
            .find("hh:heading")
            .attr("type"),
        Some("NONE")
    );
    assert_eq!(
        nodes(&header, "hh:style")
            .iter()
            .filter(|s| s.attr("paraPrIDRef") == Some(old_position.as_str()))
            .count(),
        0
    );
    assert_eq!(
        nodes(&section, "hp:p")
            .iter()
            .filter(|p| p.attr("paraPrIDRef") == Some(old_position.as_str()))
            .count(),
        0
    );
}
