//! Acceptance visual pack for list semantics.
//!
//! Generates a compact set of HWPX files under `temp/list_acceptance_visual/`
//! for manual inspection in Hancom Office.
//!
//! Usage:
//!   cargo run -p hwpforge-smithy-hwpx --example list_acceptance_visual

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

const OUT_DIR: &str = "temp/list_acceptance_visual";
const LEVELS: usize = 4;

#[derive(Clone, Copy)]
struct VisualStyles {
    body_cs: CharShapeIndex,
    title_cs: CharShapeIndex,
    note_cs: CharShapeIndex,
    body_ps: ParaShapeIndex,
    title_ps: ParaShapeIndex,
    note_ps: ParaShapeIndex,
    bullet: [ParaShapeIndex; LEVELS],
    numbered: [ParaShapeIndex; LEVELS],
    outline: [ParaShapeIndex; LEVELS],
    check_unchecked: [ParaShapeIndex; LEVELS],
    check_checked: [ParaShapeIndex; LEVELS],
    continuation: [ParaShapeIndex; LEVELS],
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
            "List Acceptance Visual - All In One",
            build_all_in_one_case(&visual.styles),
        ),
        ("01_bullet.hwpx", "List Acceptance Visual - Bullet", build_bullet_case(&visual.styles)),
        (
            "02_numbered_outline.hwpx",
            "List Acceptance Visual - Numbered And Outline",
            build_numbered_outline_case(&visual.styles),
        ),
        (
            "03_checkable.hwpx",
            "List Acceptance Visual - Checkable",
            build_checkable_case(&visual.styles),
        ),
        (
            "04_checkable_continuation.hwpx",
            "List Acceptance Visual - Checkable Continuation",
            build_continuation_case(&visual.styles),
        ),
        (
            "05_mixed_transition.hwpx",
            "List Acceptance Visual - Mixed Transition",
            build_mixed_case(&visual.styles),
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

    let numbering = push_numbering(&mut registry, numbering_def(20));
    let plain_bullet = push_bullet(&mut registry, bullet_def(1, ""));
    let check_bullet = push_bullet(&mut registry, checkable_bullet_def(2, "☐", "☑"));

    let bullet_shapes = build_list_shape_array(&mut registry, body_ps, |level| {
        ParagraphListRef::Bullet { bullet_id: plain_bullet, level }
    });
    let numbered_shapes = build_list_shape_array(&mut registry, body_ps, |level| {
        ParagraphListRef::Number { numbering_id: numbering, level }
    });
    let outline_shapes =
        build_list_shape_array(&mut registry, body_ps, |level| ParagraphListRef::Outline { level });
    let check_unchecked_shapes = build_list_shape_array(&mut registry, body_ps, |level| {
        ParagraphListRef::CheckBullet { bullet_id: check_bullet, level, checked: false }
    });
    let check_checked_shapes = build_list_shape_array(&mut registry, body_ps, |level| {
        ParagraphListRef::CheckBullet { bullet_id: check_bullet, level, checked: true }
    });
    let continuation_shapes = build_continuation_shape_array(&mut registry, body_ps);

    VisualRegistry {
        registry,
        styles: VisualStyles {
            body_cs,
            title_cs,
            note_cs,
            body_ps,
            title_ps,
            note_ps,
            bullet: bullet_shapes,
            numbered: numbered_shapes,
            outline: outline_shapes,
            check_unchecked: check_unchecked_shapes,
            check_checked: check_checked_shapes,
            continuation: continuation_shapes,
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
        push_para_shape(registry, list_para_shape(base, make_ref(level), level))
    })
}

fn build_continuation_shape_array<const N: usize>(
    registry: &mut StyleRegistry,
    base_para: ParaShapeIndex,
) -> [ParaShapeIndex; N] {
    std::array::from_fn(|idx| {
        let level = idx as u8;
        let mut base = registry.para_shape(base_para).expect("base para shape").clone();
        let left_mm = 8.0 + f64::from(level) * 6.5;
        base.indent_left = HwpUnit::from_mm(left_mm).expect("left indent");
        base.indent_first_line = HwpUnit::ZERO;
        base.space_before = HwpUnit::ZERO;
        base.space_after = HwpUnit::from_mm(0.8).expect("after spacing");
        base.break_type = BreakType::None;
        base.keep_with_next = false;
        base.keep_lines_together = false;
        base.widow_orphan = true;
        base.list = None;
        push_para_shape(registry, base)
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
            para_head(1, 4, NumberFormatType::CircledDigit, "^4"),
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
        "List Acceptance Visual Pack",
        "bullet, numbered, outline, checkable, continuation, mixed transition을 한 파일에서 본다. continuation 문단에 checkbox가 다시 붙지 않는지까지 눈으로 확인하면 된다.",
        styles,
    );
    paras.extend(build_bullet_block(styles));
    paras.push(title("Numbered / outline", styles).with_page_break());
    paras.extend(build_numbered_outline_block(styles));
    paras.push(title("Checkable", styles).with_page_break());
    paras.extend(build_checkable_block(styles));
    paras.push(title("Checkable continuation", styles).with_page_break());
    paras.extend(build_continuation_block(styles));
    paras.push(title("Mixed transition", styles).with_page_break());
    paras.extend(build_mixed_block(styles));
    paras
}

fn build_bullet_case(styles: &VisualStyles) -> Vec<Paragraph> {
    let mut paras =
        cover("Bullet Acceptance", "plain bullet depth 1..4, interruption, resume를 본다.", styles);
    paras.extend(build_bullet_block(styles));
    paras
}

fn build_numbered_outline_case(styles: &VisualStyles) -> Vec<Paragraph> {
    let mut paras = cover(
        "Numbered / Outline Acceptance",
        "numbered depth와 outline depth를 한 파일에서 분리해 본다.",
        styles,
    );
    paras.extend(build_numbered_outline_block(styles));
    paras
}

fn build_checkable_case(styles: &VisualStyles) -> Vec<Paragraph> {
    let mut paras =
        cover("Checkable Acceptance", "unchecked / checked / nested depth를 한 번에 본다.", styles);
    paras.extend(build_checkable_block(styles));
    paras
}

fn build_continuation_case(styles: &VisualStyles) -> Vec<Paragraph> {
    let mut paras = cover(
        "Checkable Continuation Acceptance",
        "같은 task item의 두 번째 문단에는 checkbox가 다시 붙지 않아야 한다.",
        styles,
    );
    paras.extend(build_continuation_block(styles));
    paras
}

fn build_mixed_case(styles: &VisualStyles) -> Vec<Paragraph> {
    let mut paras = cover(
        "Mixed List Acceptance",
        "bullet -> checkable -> numbered -> outline -> ordered parent + task child -> resume 순서를 본다.",
        styles,
    );
    paras.extend(build_mixed_block(styles));
    paras
}

fn build_bullet_block(styles: &VisualStyles) -> Vec<Paragraph> {
    vec![
        title("plain bullet depth", styles),
        list_item("bullet level 1", styles, styles.bullet[0]),
        list_item("bullet level 2", styles, styles.bullet[1]),
        list_item("bullet level 3", styles, styles.bullet[2]),
        list_item("bullet level 4", styles, styles.bullet[3]),
        body("이 문단은 list가 아닌 일반 본문이다.", styles),
        list_item("bullet resume after interruption", styles, styles.bullet[0]),
        blank(styles),
        note("모든 bullet은 일반 bullet glyph여야 하고, interruption 뒤 resume도 다시 bullet로 보여야 한다.", styles),
    ]
}

fn build_numbered_outline_block(styles: &VisualStyles) -> Vec<Paragraph> {
    vec![
        title("numbered depth", styles),
        list_item("numbered level 1", styles, styles.numbered[0]),
        list_item("numbered level 2", styles, styles.numbered[1]),
        list_item("numbered level 3", styles, styles.numbered[2]),
        list_item("numbered level 4", styles, styles.numbered[3]),
        blank(styles),
        title("outline depth", styles),
        list_item("outline level 1", styles, styles.outline[0]),
        list_item("outline level 2", styles, styles.outline[1]),
        list_item("outline level 3", styles, styles.outline[2]),
        list_item("outline level 4", styles, styles.outline[3]),
        blank(styles),
        note("numbered는 NUMBER, outline은 OUTLINE heading type으로 내려가야 한다. 화면상 depth와 번호 체계가 섞이지 않으면 된다.", styles),
    ]
}

fn build_checkable_block(styles: &VisualStyles) -> Vec<Paragraph> {
    vec![
        title("basic checkable", styles),
        list_item("unchecked item A", styles, styles.check_unchecked[0]),
        list_item("checked item B", styles, styles.check_checked[0]),
        list_item("unchecked item C", styles, styles.check_unchecked[0]),
        list_item("checked item D", styles, styles.check_checked[0]),
        blank(styles),
        title("nested checkable", styles),
        list_item("level 1 unchecked", styles, styles.check_unchecked[0]),
        list_item("level 2 checked", styles, styles.check_checked[1]),
        list_item("level 3 unchecked", styles, styles.check_unchecked[2]),
        list_item("level 4 checked", styles, styles.check_checked[3]),
        blank(styles),
        note("checkable bullet은 unchecked/checked glyph와 paragraph checked state가 같이 내려가야 한다.", styles),
    ]
}

fn build_continuation_block(styles: &VisualStyles) -> Vec<Paragraph> {
    vec![
        title("single task item with continuation paragraph", styles),
        list_item("first paragraph of the same task item", styles, styles.check_unchecked[0]),
        list_item("second paragraph of the same task item", styles, styles.continuation[0]),
        list_item("next real task item", styles, styles.check_checked[0]),
        blank(styles),
        note("정상이라면 두 번째 문단은 첫 item 아래 들여쓰기만 유지되고 checkbox는 다시 나오지 않는다.", styles),
    ]
}

fn build_mixed_block(styles: &VisualStyles) -> Vec<Paragraph> {
    vec![
        title("mixed transition", styles),
        list_item("plain bullet item", styles, styles.bullet[0]),
        list_item("checkable unchecked", styles, styles.check_unchecked[0]),
        list_item("checkable checked", styles, styles.check_checked[0]),
        list_item("numbered item 1", styles, styles.numbered[0]),
        list_item("numbered item 2", styles, styles.numbered[1]),
        list_item("outline item 1", styles, styles.outline[0]),
        list_item("outline item 2", styles, styles.outline[1]),
        body("이 문단은 list가 아닌 일반 본문이다.", styles),
        title("ordered parent + task child", styles),
        list_item("ordered parent", styles, styles.numbered[0]),
        list_item("task child checked", styles, styles.check_checked[1]),
        list_item("task child unchecked", styles, styles.check_unchecked[1]),
        body("끝 본문", styles),
        list_item("checkable resume", styles, styles.check_unchecked[0]),
        blank(styles),
        note(
            "서로 다른 list semantics 전환 후에도 glyph, numbering, depth가 뒤섞이지 않는지 본다.",
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
    text.push_str("# List Acceptance Visual Checklist\n\n");
    text.push_str("Generated files:\n");
    text.push_str("- 00_all_in_one.hwpx: 전체 종합본\n");
    text.push_str("- 01_bullet.hwpx: plain bullet depth / interruption / resume\n");
    text.push_str("- 02_numbered_outline.hwpx: numbered + outline depth\n");
    text.push_str("- 03_checkable.hwpx: checked / unchecked / nested checkable\n");
    text.push_str("- 04_checkable_continuation.hwpx: 같은 task item의 continuation paragraph\n");
    text.push_str("- 05_mixed_transition.hwpx: mixed transition + ordered parent + task child\n\n");
    text.push_str("Visual checks:\n");
    text.push_str("- bullet은 일반 bullet glyph로 보이는지\n");
    text.push_str("- numbered depth가 level별 포맷으로 보이는지\n");
    text.push_str("- outline depth가 numbered와 섞이지 않는지\n");
    text.push_str("- checkable unchecked/checked가 빈 체크박스/체크된 박스로 보이는지\n");
    text.push_str("- continuation 문단에는 checkbox가 다시 붙지 않는지\n");
    text.push_str("- mixed transition 후에도 resume list가 올바른 semantics로 보이는지\n");
    fs::write(out_dir.join("README.md"), text).expect("write manifest");
}
