//! Visual showcase for checkable list semantics.
//!
//! Generates several HWPX documents under `temp/checkable_list_semantics_visual/`
//! so they can be opened in Hancom Office for manual inspection.
//!
//! Usage:
//!   cargo run -p hwpforge-smithy-hwpx --example checkable_list_semantics_visual

use std::fs;
use std::path::Path;

use hwpforge_blueprint::builtins::builtin_default;
use hwpforge_blueprint::registry::StyleRegistry;
use hwpforge_blueprint::style::{CharShape, ParaShape};
use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::metadata::Metadata;
use hwpforge_core::numbering::{BulletDef, NumberingDef, ParaHead, ParagraphListRef};
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::PageSettings;
use hwpforge_foundation::{
    Alignment, BreakType, BulletIndex, CharShapeIndex, Color, HwpUnit, NumberFormatType,
    NumberingIndex, ParaShapeIndex,
};
use hwpforge_smithy_hwpx::{HwpxEncoder, HwpxRegistryBridge};

const OUT_DIR: &str = "temp/checkable_list_semantics_visual";

#[derive(Clone, Copy)]
struct VisualStyles {
    body_cs: CharShapeIndex,
    title_cs: CharShapeIndex,
    note_cs: CharShapeIndex,
    body_ps: ParaShapeIndex,
    title_ps: ParaShapeIndex,
    note_ps: ParaShapeIndex,
    plain_bullet: [ParaShapeIndex; 3],
    check_unchecked: [ParaShapeIndex; 3],
    check_checked: [ParaShapeIndex; 3],
    numbered: [ParaShapeIndex; 3],
    outline: [ParaShapeIndex; 3],
}

#[derive(Clone)]
struct VisualRegistry {
    registry: StyleRegistry,
    styles: VisualStyles,
}

fn main() {
    let visual = build_visual_registry();
    let out_dir = Path::new(OUT_DIR);
    fs::create_dir_all(out_dir).expect("create output directory");

    let cases = vec![
        (
            "00_all_in_one.hwpx",
            "Checkable List Semantics - All In One",
            build_all_in_one_case(&visual.styles),
        ),
        (
            "01_basic_checkable.hwpx",
            "Checkable List Semantics - Basic",
            build_basic_case(&visual.styles),
        ),
        (
            "02_nested_checkable.hwpx",
            "Checkable List Semantics - Nested",
            build_nested_case(&visual.styles),
        ),
        (
            "03_mixed_transition.hwpx",
            "Checkable List Semantics - Transition",
            build_transition_case(&visual.styles),
        ),
    ];

    let image_store = ImageStore::new();
    for (file_name, title, paragraphs) in cases {
        let path = out_dir.join(file_name);
        write_case(&path, title, paragraphs, &visual.registry, &image_store);
        println!("generated {}", path.display());
    }

    write_manifest(out_dir);
    println!("manifest {}", out_dir.join("README.md").display());
}

fn build_visual_registry() -> VisualRegistry {
    let template = builtin_default().expect("builtin default template");
    let mut registry = StyleRegistry::from_template(&template).expect("default style registry");

    let body_entry = *registry.get_style("body").expect("body style");
    let heading_entry = *registry.get_style("heading1").expect("heading1 style");

    let body_cs = body_entry.char_shape_id;
    let title_cs = heading_entry.char_shape_id;
    let body_ps = body_entry.para_shape_id;

    let note_cs = {
        let mut note = registry.char_shape(body_cs).expect("body char shape").clone();
        note.size = HwpUnit::from_pt(9.0).expect("9pt");
        note.color = Color::from_rgb(90, 90, 90);
        note.italic = true;
        push_char_shape(&mut registry, note)
    };

    let title_ps = {
        let mut title = registry.para_shape(body_ps).expect("body para shape").clone();
        title.alignment = Alignment::Left;
        title.space_before = HwpUnit::from_mm(4.0).expect("4mm");
        title.space_after = HwpUnit::from_mm(2.5).expect("2.5mm");
        title.keep_with_next = true;
        push_para_shape(&mut registry, title)
    };

    let note_ps = {
        let mut note = registry.para_shape(body_ps).expect("body para shape").clone();
        note.space_after = HwpUnit::from_mm(1.5).expect("1.5mm");
        push_para_shape(&mut registry, note)
    };

    let numbering = push_numbering(&mut registry, numbering_def(10));
    let plain_bullet = push_bullet(&mut registry, bullet_def(1, ""));
    let check_bullet = push_bullet(&mut registry, checkable_bullet_def(2, "☐", "☑"));

    let plain_bullet_shapes = build_list_shape_array(&mut registry, body_ps, |level| {
        ParagraphListRef::Bullet { bullet_id: plain_bullet, level }
    });
    let check_unchecked_shapes = build_list_shape_array(&mut registry, body_ps, |level| {
        ParagraphListRef::CheckBullet { bullet_id: check_bullet, level, checked: false }
    });
    let check_checked_shapes = build_list_shape_array(&mut registry, body_ps, |level| {
        ParagraphListRef::CheckBullet { bullet_id: check_bullet, level, checked: true }
    });
    let numbered_shapes = build_list_shape_array(&mut registry, body_ps, |level| {
        ParagraphListRef::Number { numbering_id: numbering, level }
    });
    let outline_shapes =
        build_list_shape_array(&mut registry, body_ps, |level| ParagraphListRef::Outline { level });

    VisualRegistry {
        registry,
        styles: VisualStyles {
            body_cs,
            title_cs,
            note_cs,
            body_ps,
            title_ps,
            note_ps,
            plain_bullet: plain_bullet_shapes,
            check_unchecked: check_unchecked_shapes,
            check_checked: check_checked_shapes,
            numbered: numbered_shapes,
            outline: outline_shapes,
        },
    }
}

fn build_list_shape_array<const N: usize>(
    registry: &mut StyleRegistry,
    base_para: ParaShapeIndex,
    make_ref: impl Fn(u8) -> ParagraphListRef,
) -> [ParaShapeIndex; N] {
    std::array::from_fn(|idx| {
        let level = idx as u8;
        let base = registry.para_shape(base_para).expect("base para shape").clone();
        let list_shape = list_para_shape(base, make_ref(level), level);
        push_para_shape(registry, list_shape)
    })
}

fn list_para_shape(mut base: ParaShape, list: ParagraphListRef, level: u8) -> ParaShape {
    let left_mm = 8.0 + f64::from(level) * 6.5;
    base.indent_left = HwpUnit::from_mm(left_mm).expect("left indent");
    base.indent_first_line = HwpUnit::from_mm(-5.5).expect("hanging indent");
    base.space_before = HwpUnit::ZERO;
    base.space_after = HwpUnit::from_mm(0.8).expect("after spacing");
    base.break_type = BreakType::None;
    base.keep_with_next = false;
    base.keep_lines_together = false;
    base.widow_orphan = true;
    base.list = Some(list);
    base
}

fn push_char_shape(registry: &mut StyleRegistry, shape: CharShape) -> CharShapeIndex {
    let idx = CharShapeIndex::new(registry.char_shapes.len());
    registry.char_shapes.push(shape);
    idx
}

fn push_para_shape(registry: &mut StyleRegistry, shape: ParaShape) -> ParaShapeIndex {
    let idx = ParaShapeIndex::new(registry.para_shapes.len());
    registry.para_shapes.push(shape);
    idx
}

fn push_numbering(registry: &mut StyleRegistry, numbering: NumberingDef) -> NumberingIndex {
    let idx = NumberingIndex::new(registry.numberings.len());
    registry.numberings.push(numbering);
    idx
}

fn push_bullet(registry: &mut StyleRegistry, bullet: BulletDef) -> BulletIndex {
    let idx = BulletIndex::new(registry.bullets.len());
    registry.bullets.push(bullet);
    idx
}

fn bullet_def(id: u32, bullet_char: &str) -> BulletDef {
    BulletDef {
        id,
        bullet_char: bullet_char.to_string(),
        checked_char: None,
        use_image: false,
        para_head: ParaHead {
            start: 0,
            level: 1,
            num_format: NumberFormatType::Digit,
            text: String::new(),
            checkable: false,
        },
    }
}

fn checkable_bullet_def(id: u32, bullet_char: &str, checked_char: &str) -> BulletDef {
    BulletDef {
        id,
        bullet_char: bullet_char.to_string(),
        checked_char: Some(checked_char.to_string()),
        use_image: false,
        para_head: ParaHead {
            start: 0,
            level: 1,
            num_format: NumberFormatType::Digit,
            text: String::new(),
            checkable: true,
        },
    }
}

fn numbering_def(id: u32) -> NumberingDef {
    NumberingDef {
        id,
        start: 1,
        levels: vec![
            para_head(1, 1, NumberFormatType::Digit, "^1."),
            para_head(1, 2, NumberFormatType::LatinCapital, "^2."),
            para_head(1, 3, NumberFormatType::RomanSmall, "^3)"),
        ],
    }
}

fn para_head(start: u32, level: u32, num_format: NumberFormatType, text: &str) -> ParaHead {
    ParaHead { start, level, num_format, text: text.to_string(), checkable: false }
}

fn title(text: &str, styles: &VisualStyles) -> Paragraph {
    para(text, styles.title_cs, styles.title_ps)
}

fn note(text: &str, styles: &VisualStyles) -> Paragraph {
    para(text, styles.note_cs, styles.note_ps)
}

fn body(text: &str, styles: &VisualStyles) -> Paragraph {
    para(text, styles.body_cs, styles.body_ps)
}

fn list_item(text: &str, styles: &VisualStyles, para_shape: ParaShapeIndex) -> Paragraph {
    para(text, styles.body_cs, para_shape)
}

fn para(text: &str, char_shape: CharShapeIndex, para_shape: ParaShapeIndex) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, char_shape)], para_shape)
}

fn blank(styles: &VisualStyles) -> Paragraph {
    body("", styles)
}

fn cover(title_text: &str, description: &str, styles: &VisualStyles) -> Vec<Paragraph> {
    vec![title(title_text, styles), note(description, styles), blank(styles)]
}

fn build_all_in_one_case(styles: &VisualStyles) -> Vec<Paragraph> {
    let mut paras = cover(
        "Checkable List Semantics",
        "plain bullet, checkable bullet, numbered, outline를 같은 IR 경로에서 생성한 종합본이다. checkable item state가 glyph와 paragraph checked state로 같이 내려가는지 눈으로 확인하면 된다.",
        styles,
    );
    paras.extend(build_basic_block(styles));
    paras.push(title("Nested checkable depth", styles).with_page_break());
    paras.extend(build_nested_block(styles));
    paras.push(title("Mixed transition", styles).with_page_break());
    paras.extend(build_transition_block(styles));
    paras
}

fn build_basic_case(styles: &VisualStyles) -> Vec<Paragraph> {
    let mut paras = cover(
        "Basic Checkable Bullet",
        "unchecked / checked item이 같은 checkable bullet definition을 공유하되, paragraph checked state만 다르게 내려가는지 보는 문서다.",
        styles,
    );
    paras.extend(build_basic_block(styles));
    paras
}

fn build_nested_case(styles: &VisualStyles) -> Vec<Paragraph> {
    let mut paras = cover(
        "Nested Checkable Bullet",
        "level 1..3 depth와 checked / unchecked 조합을 같이 배치했다. hanging indent와 glyph switching을 같이 확인하면 된다.",
        styles,
    );
    paras.extend(build_nested_block(styles));
    paras
}

fn build_transition_case(styles: &VisualStyles) -> Vec<Paragraph> {
    let mut paras = cover(
        "Checkable Transition",
        "plain bullet -> checkable -> numbered -> outline -> checkable resume 순서를 한 파일에 넣었다. plain paragraph interruption도 같이 본다.",
        styles,
    );
    paras.extend(build_transition_block(styles));
    paras
}

fn build_basic_block(styles: &VisualStyles) -> Vec<Paragraph> {
    vec![
        title("basic matrix", styles),
        list_item("plain bullet - control case", styles, styles.plain_bullet[0]),
        list_item("checkable unchecked item A", styles, styles.check_unchecked[0]),
        list_item("checkable checked item B", styles, styles.check_checked[0]),
        list_item("checkable unchecked item C", styles, styles.check_unchecked[0]),
        list_item("checkable checked item D", styles, styles.check_checked[0]),
        blank(styles),
        note(
            "plain bullet은 실제 bullet semantics지만 checked state는 없다. checkable bullet만 unchecked/checked glyph와 paragraph checked state가 동시에 들어간다.",
            styles,
        ),
    ]
}

fn build_nested_block(styles: &VisualStyles) -> Vec<Paragraph> {
    vec![
        title("depth 1..3", styles),
        list_item("level 1 unchecked", styles, styles.check_unchecked[0]),
        list_item("level 2 checked", styles, styles.check_checked[1]),
        list_item("level 3 unchecked", styles, styles.check_unchecked[2]),
        list_item("level 2 unchecked sibling", styles, styles.check_unchecked[1]),
        list_item("level 1 checked sibling", styles, styles.check_checked[0]),
        blank(styles),
        note(
            "여기서는 들여쓰기 depth와 checked state를 함께 본다. 같은 bullet definition을 쓰되 para shape별 checked bit만 다르다.",
            styles,
        ),
    ]
}

fn build_transition_block(styles: &VisualStyles) -> Vec<Paragraph> {
    vec![
        title("plain bullet -> checkable -> numbered -> outline", styles),
        list_item("plain bullet item", styles, styles.plain_bullet[0]),
        list_item("checkable unchecked", styles, styles.check_unchecked[0]),
        list_item("checkable checked", styles, styles.check_checked[0]),
        list_item("numbered item 1", styles, styles.numbered[0]),
        list_item("numbered item 2", styles, styles.numbered[1]),
        list_item("outline item 1", styles, styles.outline[0]),
        list_item("outline item 2", styles, styles.outline[1]),
        body("이 문단은 list가 아닌 일반 본문이다.", styles),
        list_item("checkable unchecked resume", styles, styles.check_unchecked[0]),
        list_item("checkable checked resume", styles, styles.check_checked[0]),
        blank(styles),
        note(
            "transition block은 heading type과 idRef 전환을 보기 위한 문서다. outline은 OUTLINE, numbered는 NUMBER, plain/checkable은 모두 BULLET 계열이어야 한다.",
            styles,
        ),
    ]
}

fn write_case(
    path: &Path,
    title: &str,
    paragraphs: Vec<Paragraph>,
    registry: &StyleRegistry,
    image_store: &ImageStore,
) {
    let mut doc =
        Document::with_metadata(Metadata { title: Some(title.to_string()), ..Metadata::default() });
    doc.add_section(Section::with_paragraphs(paragraphs, PageSettings::a4()));
    let bridge = HwpxRegistryBridge::from_registry(registry).expect("build registry bridge");
    let rebound = bridge.rebind_draft_document(doc).expect("rebind generated document");
    let validated = rebound.validate().expect("validate generated document");
    let bytes =
        HwpxEncoder::encode(&validated, bridge.style_store(), image_store).expect("encode hwpx");
    fs::write(path, bytes).expect("write hwpx");
}

fn write_manifest(out_dir: &Path) {
    let mut text = String::new();
    text.push_str("# Checkable List Semantics Visual Checklist\n\n");
    text.push_str("Generated files:\n");
    text.push_str("- 00_all_in_one.hwpx: plain bullet / checkable / numbered / outline 종합본\n");
    text.push_str("- 01_basic_checkable.hwpx: checked / unchecked 기본 비교\n");
    text.push_str("- 02_nested_checkable.hwpx: level 1..3 nested checkable\n");
    text.push_str(
        "- 03_mixed_transition.hwpx: plain bullet -> checkable -> numbered -> outline 전환\n\n",
    );
    text.push_str("Visual checks:\n");
    text.push_str("- plain bullet은 checkbox glyph가 아니라 일반 bullet glyph로 보이는지\n");
    text.push_str("- checkable unchecked는 빈 체크박스, checked는 체크된 박스로 보이는지\n");
    text.push_str("- nested level이 깊어질수록 들여쓰기가 유지되는지\n");
    text.push_str("- numbered와 outline 전환 후에도 checkable resume이 다시 checkbox로 보이는지\n");
    fs::write(out_dir.join("README.md"), text).expect("write manifest");
}
