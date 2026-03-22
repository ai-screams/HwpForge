use hwpforge_blueprint::registry::StyleRegistry;
use hwpforge_blueprint::style::{CharShape, ParaShape};
use hwpforge_core::numbering::{BulletDef, NumberingDef, ParaHead, ParagraphListRef};
use hwpforge_foundation::{
    Alignment, BreakType, BulletIndex, CharShapeIndex, Color, HwpUnit, NumberFormatType,
    NumberingIndex, ParaShapeIndex,
};

#[derive(Clone, Copy)]
pub(super) struct BaseVisualStyleIds {
    pub body_cs: CharShapeIndex,
    pub title_cs: CharShapeIndex,
    pub note_cs: CharShapeIndex,
    pub body_ps: ParaShapeIndex,
    pub title_ps: ParaShapeIndex,
    pub note_ps: ParaShapeIndex,
}

pub(super) fn build_base_visual_style_ids(registry: &mut StyleRegistry) -> BaseVisualStyleIds {
    let (body_cs, title_cs, body_ps) = resolve_base_style_ids(registry);
    let note_cs = build_note_char_shape(registry, body_cs);
    let title_ps = build_title_para_shape(registry, body_ps);
    let note_ps = build_note_para_shape(registry, body_ps);

    BaseVisualStyleIds { body_cs, title_cs, note_cs, body_ps, title_ps, note_ps }
}

fn resolve_base_style_ids(
    registry: &StyleRegistry,
) -> (CharShapeIndex, CharShapeIndex, ParaShapeIndex) {
    let body_entry = *registry.get_style("body").expect("body style");
    let heading_entry = *registry.get_style("heading1").expect("heading1 style");
    (body_entry.char_shape_id, heading_entry.char_shape_id, body_entry.para_shape_id)
}

fn build_note_char_shape(registry: &mut StyleRegistry, body_cs: CharShapeIndex) -> CharShapeIndex {
    let mut note = registry.char_shape(body_cs).expect("body char shape").clone();
    note.size = HwpUnit::from_pt(9.0).expect("9pt");
    note.color = Color::from_rgb(90, 90, 90);
    note.italic = true;
    push_char_shape(registry, note)
}

fn build_title_para_shape(registry: &mut StyleRegistry, body_ps: ParaShapeIndex) -> ParaShapeIndex {
    let mut title = registry.para_shape(body_ps).expect("body para shape").clone();
    title.alignment = Alignment::Left;
    title.space_before = HwpUnit::from_mm(4.0).expect("4mm");
    title.space_after = HwpUnit::from_mm(2.5).expect("2.5mm");
    title.keep_with_next = true;
    push_para_shape(registry, title)
}

fn build_note_para_shape(registry: &mut StyleRegistry, body_ps: ParaShapeIndex) -> ParaShapeIndex {
    let mut note = registry.para_shape(body_ps).expect("body para shape").clone();
    note.space_after = HwpUnit::from_mm(1.5).expect("1.5mm");
    push_para_shape(registry, note)
}

pub(super) fn build_list_shape_array<const N: usize>(
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

#[allow(dead_code)]
pub(super) fn build_continuation_shape_array<const N: usize>(
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

pub(super) fn push_numbering(
    registry: &mut StyleRegistry,
    numbering: NumberingDef,
) -> NumberingIndex {
    let idx = NumberingIndex::new(registry.numberings.len());
    registry.numberings.push(numbering);
    idx
}

pub(super) fn push_bullet(registry: &mut StyleRegistry, bullet: BulletDef) -> BulletIndex {
    let idx = BulletIndex::new(registry.bullets.len());
    registry.bullets.push(bullet);
    idx
}

pub(super) fn bullet_def(id: u32, bullet_char: &str) -> BulletDef {
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

#[allow(dead_code)]
pub(super) fn checkable_bullet_def(id: u32, bullet_char: &str, checked_char: &str) -> BulletDef {
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

pub(super) fn para_head(
    start: u32,
    level: u32,
    num_format: NumberFormatType,
    text: &str,
) -> ParaHead {
    ParaHead { start, level, num_format, text: text.to_string(), checkable: false }
}
