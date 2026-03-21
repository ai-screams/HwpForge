//! Visual showcase for shared list semantics.
//!
//! Generates several HWPX documents under `temp/list_shared_semantics_visual/`
//! so they can be opened in Hancom Office for manual inspection.
//!
//! Usage:
//!   cargo run -p hwpforge-smithy-hwpx --example list_shared_semantics_visual

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
use hwpforge_core::table::{Table, TableCell, TablePageBreak, TableRow};
use hwpforge_core::PageSettings;
use hwpforge_foundation::{
    Alignment, BreakType, BulletIndex, CharShapeIndex, Color, HwpUnit, NumberFormatType,
    NumberingIndex, ParaShapeIndex,
};
use hwpforge_smithy_hwpx::{HwpxEncoder, HwpxRegistryBridge};

const OUT_DIR: &str = "temp/list_shared_semantics_visual";
const DEBUG_DIR: &str = "temp/list_shared_semantics_visual/debug_isolation";

#[derive(Clone, Copy)]
struct VisualStyles {
    body_cs: CharShapeIndex,
    title_cs: CharShapeIndex,
    note_cs: CharShapeIndex,
    body_ps: ParaShapeIndex,
    title_ps: ParaShapeIndex,
    note_ps: ParaShapeIndex,
    bullet_primary: [ParaShapeIndex; 5],
    bullet_secondary: [ParaShapeIndex; 5],
    bullet_edge: [ParaShapeIndex; 5],
    numbered_primary: [ParaShapeIndex; 10],
    numbered_offset: [ParaShapeIndex; 4],
    numbered_nested: [ParaShapeIndex; 4],
    outline: [ParaShapeIndex; 10],
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
    fs::create_dir_all(DEBUG_DIR).expect("create debug output directory");

    let cases = vec![
        (
            "00_all_in_one.hwpx",
            "Shared List Semantics - All In One",
            build_all_in_one_case(&visual.styles),
        ),
        (
            "01_bullet_matrix.hwpx",
            "Shared List Semantics - Bullet Matrix",
            build_bullet_matrix_case(&visual.styles),
        ),
        (
            "02_numbering_custom_formats.hwpx",
            "Shared List Semantics - Numbering Formats",
            build_numbering_case(&visual.styles),
        ),
        (
            "03_outline_depth.hwpx",
            "Shared List Semantics - Outline Depth",
            build_outline_case(&visual.styles),
        ),
        (
            "04_mixed_edge_cases.hwpx",
            "Shared List Semantics - Mixed Edge Cases",
            build_mixed_case(&visual.styles),
        ),
        (
            "05_table_ordered_lists.hwpx",
            "Shared List Semantics - Table Ordered Lists",
            build_table_case(&visual.styles),
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

    for (file_name, title, paragraphs) in build_debug_cases(&visual.styles) {
        let path = Path::new(DEBUG_DIR).join(file_name);
        write_case(&path, title, paragraphs, &visual.registry, &image_store);
        println!("generated {}", path.display());
    }

    write_debug_manifest(Path::new(DEBUG_DIR));
    println!("debug manifest {}", Path::new(DEBUG_DIR).join("README.md").display());
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

    let _outline_builtin = push_numbering(&mut registry, NumberingDef::default_outline());
    let numbering_primary = push_numbering(&mut registry, custom_formats_numbering(10));
    let numbering_offset = push_numbering(&mut registry, offset_numbering(11));
    let numbering_nested = push_numbering(&mut registry, nested_numbering(12));

    // Hancom is picky here. Reuse the fixture-proven glyph/id pattern instead
    // of experimenting with multiple arbitrary bullet definitions.
    let bullet_primary = push_bullet(&mut registry, bullet_def(1, ""));
    let bullet_secondary = bullet_primary;
    let bullet_edge = bullet_primary;

    let bullet_primary_shapes = build_list_shape_array(&mut registry, body_ps, |level| {
        ParagraphListRef::Bullet { bullet_id: bullet_primary, level }
    });
    let bullet_secondary_shapes = build_list_shape_array(&mut registry, body_ps, |level| {
        ParagraphListRef::Bullet { bullet_id: bullet_secondary, level }
    });
    let bullet_edge_shapes = build_list_shape_array(&mut registry, body_ps, |level| {
        ParagraphListRef::Bullet { bullet_id: bullet_edge, level }
    });
    let numbered_primary_shapes = build_list_shape_array_10(&mut registry, body_ps, |level| {
        ParagraphListRef::Number { numbering_id: numbering_primary, level }
    });
    let numbered_offset_shapes = build_list_shape_array(&mut registry, body_ps, |level| {
        ParagraphListRef::Number { numbering_id: numbering_offset, level }
    });
    let numbered_nested_shapes = build_list_shape_array(&mut registry, body_ps, |level| {
        ParagraphListRef::Number { numbering_id: numbering_nested, level }
    });
    let outline_shapes = build_list_shape_array_10(&mut registry, body_ps, |level| {
        ParagraphListRef::Outline { level }
    });

    VisualRegistry {
        registry,
        styles: VisualStyles {
            body_cs,
            title_cs,
            note_cs,
            body_ps,
            title_ps,
            note_ps,
            bullet_primary: bullet_primary_shapes,
            bullet_secondary: bullet_secondary_shapes,
            bullet_edge: bullet_edge_shapes,
            numbered_primary: numbered_primary_shapes,
            numbered_offset: numbered_offset_shapes,
            numbered_nested: numbered_nested_shapes,
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

fn build_list_shape_array_10(
    registry: &mut StyleRegistry,
    base_para: ParaShapeIndex,
    make_ref: impl Fn(u8) -> ParagraphListRef,
) -> [ParaShapeIndex; 10] {
    build_list_shape_array::<10>(registry, base_para, make_ref)
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

fn custom_formats_numbering(id: u32) -> NumberingDef {
    NumberingDef {
        id,
        start: 1,
        levels: vec![
            para_head(1, 1, NumberFormatType::Digit, "^1."),
            para_head(1, 2, NumberFormatType::CircledDigit, "^2"),
            para_head(1, 3, NumberFormatType::RomanSmall, "^3)"),
            para_head(1, 4, NumberFormatType::LatinCapital, "Section ^4."),
            para_head(1, 5, NumberFormatType::LatinSmall, "step ^5)"),
            para_head(1, 6, NumberFormatType::CircledLatinSmall, "^6"),
            para_head(1, 7, NumberFormatType::HangulSyllable, "제 ^7 항"),
            para_head(1, 8, NumberFormatType::CircledHangulSyllable, "^8"),
            para_head(1, 9, NumberFormatType::HangulJamo, "^9)"),
            para_head(1, 10, NumberFormatType::RomanCapital, "APPENDIX ^10"),
        ],
    }
}

fn offset_numbering(id: u32) -> NumberingDef {
    NumberingDef {
        id,
        start: 5,
        levels: vec![
            para_head(5, 1, NumberFormatType::Digit, "[^1]"),
            para_head(3, 2, NumberFormatType::LatinCapital, "CASE-^2"),
            para_head(7, 3, NumberFormatType::RomanCapital, "(^3)"),
            para_head(2, 4, NumberFormatType::HangulSyllable, "제 ^4 조"),
        ],
    }
}

fn nested_numbering(id: u32) -> NumberingDef {
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
        "공유 리스트 시각 검증 종합본",
        "bullet / ordered / outline shared semantics를 한 파일에 모았다. list indent, custom numbering glyph, mixed transition, outline depth를 한글 화면에서 직접 확인하면 된다.",
        styles,
    );
    paras.extend(build_bullet_block(styles));
    paras.push(title("숫자 목록 커스텀 포맷", styles).with_page_break());
    paras.extend(build_numbering_block(styles));
    paras.push(title("개요(outline) 레벨 1-10", styles).with_page_break());
    paras.extend(build_outline_block(styles));
    paras.push(title("혼합 전환 및 edge case", styles).with_page_break());
    paras.extend(build_mixed_block(styles));
    paras.push(title("table 안의 ordered / numbered list", styles).with_page_break());
    paras.extend(build_table_block(styles));
    paras
}

fn build_bullet_matrix_case(styles: &VisualStyles) -> Vec<Paragraph> {
    let mut paras = cover(
        "Bullet Matrix",
        "primary(•), secondary(◦), edge(※) bullet definition과 5단계 nested level을 확인한다.",
        styles,
    );
    paras.extend(build_bullet_block(styles));
    paras
}

fn build_numbering_case(styles: &VisualStyles) -> Vec<Paragraph> {
    let mut paras = cover(
        "Numbering Formats",
        "custom numFormat, custom text template, non-1 start 값을 눈으로 확인하는 문서다.",
        styles,
    );
    paras.extend(build_numbering_block(styles));
    paras
}

fn build_outline_case(styles: &VisualStyles) -> Vec<Paragraph> {
    let mut paras = cover(
        "Outline Depth",
        "outline level 1..10을 순서대로 배치했다. level 9/10은 기본 정의상 label text가 비어 있어 표시가 다르게 보일 수 있다.",
        styles,
    );
    paras.extend(build_outline_block(styles));
    paras
}

fn build_mixed_case(styles: &VisualStyles) -> Vec<Paragraph> {
    let mut paras = cover(
        "Mixed Edge Cases",
        "list가 normal paragraph로 끊겼다가 다시 이어지는 경우, bullet -> number -> outline 전환, nested list depth를 한 화면에서 본다.",
        styles,
    );
    paras.extend(build_mixed_block(styles));
    paras
}

fn build_table_case(styles: &VisualStyles) -> Vec<Paragraph> {
    let mut paras = cover(
        "Table Ordered Lists",
        "table cell 안에 ordered / numbered list paragraph를 직접 넣은 문서다. 셀 폭이 좁을 때의 줄바꿈, nested level, start offset, plain paragraph interruption까지 한 번에 본다.",
        styles,
    );
    paras.extend(build_table_block(styles));
    paras
}

fn build_debug_cases(styles: &VisualStyles) -> Vec<(&'static str, &'static str, Vec<Paragraph>)> {
    vec![
        (
            "00_step0_cover_only.hwpx",
            "Debug 00 step0 cover only",
            cover(
                "공유 리스트 시각 검증 종합본",
                "title + note + blank만 있는 최소 문서",
                styles,
            ),
        ),
        (
            "00_step1_cover_plus_bullet.hwpx",
            "Debug 00 step1 cover plus bullet",
            concat_blocks(vec![
                cover(
                    "공유 리스트 시각 검증 종합본",
                    "cover 다음에 bullet block만 붙인 문서",
                    styles,
                ),
                build_bullet_block(styles),
            ]),
        ),
        (
            "00_step2_add_numbering.hwpx",
            "Debug 00 step2 add numbering",
            concat_blocks(vec![
                cover(
                    "공유 리스트 시각 검증 종합본",
                    "bullet 다음에 numbering block을 추가한 문서",
                    styles,
                ),
                build_bullet_block(styles),
                vec![title("숫자 목록 커스텀 포맷", styles).with_page_break()],
                build_numbering_block(styles),
            ]),
        ),
        (
            "00_step3_add_outline.hwpx",
            "Debug 00 step3 add outline",
            concat_blocks(vec![
                cover(
                    "공유 리스트 시각 검증 종합본",
                    "outline block까지 추가한 문서",
                    styles,
                ),
                build_bullet_block(styles),
                vec![title("숫자 목록 커스텀 포맷", styles).with_page_break()],
                build_numbering_block(styles),
                vec![title("개요(outline) 레벨 1-10", styles).with_page_break()],
                build_outline_block(styles),
            ]),
        ),
        (
            "00_step4_add_mixed.hwpx",
            "Debug 00 step4 add mixed",
            concat_blocks(vec![
                cover(
                    "공유 리스트 시각 검증 종합본",
                    "mixed block까지 추가한 문서",
                    styles,
                ),
                build_bullet_block(styles),
                vec![title("숫자 목록 커스텀 포맷", styles).with_page_break()],
                build_numbering_block(styles),
                vec![title("개요(outline) 레벨 1-10", styles).with_page_break()],
                build_outline_block(styles),
                vec![title("혼합 전환 및 edge case", styles).with_page_break()],
                build_mixed_block(styles),
            ]),
        ),
        (
            "00_step5_add_table.hwpx",
            "Debug 00 step5 add table",
            build_all_in_one_case(styles),
        ),
        (
            "01_bullet_min_single.hwpx",
            "Debug bullet single",
            concat_blocks(vec![
                cover("Bullet Debug", "bullet item 한 줄만 있는 최소 문서", styles),
                vec![list_item("single bullet item", styles, styles.bullet_primary[0])],
            ]),
        ),
        (
            "02_bullet_nested_2.hwpx",
            "Debug bullet nested 2",
            concat_blocks(vec![
                cover("Bullet Debug", "level 1 + level 2 nested bullet", styles),
                vec![
                    list_item("level 1", styles, styles.bullet_primary[0]),
                    list_item("level 2", styles, styles.bullet_primary[1]),
                ],
            ]),
        ),
        (
            "03_bullet_nested_5.hwpx",
            "Debug bullet nested 5",
            concat_blocks(vec![
                cover("Bullet Debug", "5단계 nested bullet만 있는 문서", styles),
                styles
                    .bullet_primary
                    .iter()
                    .enumerate()
                    .map(|(idx, shape)| {
                        list_item(&format!("bullet level {}", idx + 1), styles, *shape)
                    })
                    .collect(),
            ]),
        ),
        (
            "04_bullet_wrapping.hwpx",
            "Debug bullet wrapping",
            concat_blocks(vec![
                cover("Bullet Debug", "긴 bullet text 줄바꿈만 보는 문서", styles),
                vec![
                    list_item(
                        "폭이 좁지 않아도 긴 문장이 여러 줄로 보일 수 있도록 bullet 본문을 충분히 길게 만들어 hanging indent 정렬을 눈으로 확인한다.",
                        styles,
                        styles.bullet_primary[0],
                    ),
                    list_item(
                        "두 번째 긴 bullet 문장도 동일하게 번호 대신 bullet glyph만 고정되고 본문은 다음 줄로 자연스럽게 접혀야 한다.",
                        styles,
                        styles.bullet_primary[1],
                    ),
                ],
            ]),
        ),
        (
            "05_bullet_interruption.hwpx",
            "Debug bullet interruption",
            concat_blocks(vec![
                cover("Bullet Debug", "bullet 사이에 plain paragraph가 끼는 문서", styles),
                vec![
                    list_item("before interruption", styles, styles.bullet_primary[0]),
                    body("이 문단은 list가 아닌 일반 문단이다.", styles),
                    list_item("after interruption", styles, styles.bullet_primary[0]),
                ],
            ]),
        ),
        (
            "06_bullet_plain_vs_semantic.hwpx",
            "Debug bullet plain vs semantic",
            concat_blocks(vec![
                cover("Bullet Debug", "plain text bullet과 real bullet semantics 비교", styles),
                vec![
                    body("• 이 줄은 문자 그대로 bullet 기호를 쓴 plain text다.", styles),
                    list_item("이 줄은 실제 bullet semantics다.", styles, styles.bullet_primary[0]),
                ],
            ]),
        ),
    ]
}

fn concat_blocks(blocks: Vec<Vec<Paragraph>>) -> Vec<Paragraph> {
    blocks.into_iter().flatten().collect()
}

fn build_bullet_block(styles: &VisualStyles) -> Vec<Paragraph> {
    let mut paras = vec![
        title("Bullet depth / wrapping / interruption", styles),
        note(
            "fixture에서 실제로 열린 bullet glyph를 그대로 써서 Hancom 안정성을 우선했다. 여기서는 여러 bullet definition보다 depth, wrapping, interruption을 보는 게 핵심이다.",
            styles,
        ),
    ];

    for (idx, shape) in styles.bullet_primary.iter().enumerate() {
        paras.push(list_item(
            &format!("bullet level {} - nested depth와 hanging indent 확인", idx + 1),
            styles,
            *shape,
        ));
    }
    paras.push(blank(styles));

    for (idx, shape) in styles.bullet_secondary.iter().enumerate() {
        paras.push(list_item(
            &format!(
                "wrapped bullet level {} - 폭이 좁아져도 bullet 정렬이 깨지지 않는 긴 문장 예제",
                idx + 1
            ),
            styles,
            *shape,
        ));
    }
    paras.push(blank(styles));

    paras.push(body("이 문단은 bullet 사이에 낀 일반 문단이다.", styles));

    for (idx, shape) in styles.bullet_edge.iter().enumerate() {
        paras.push(list_item(
            &format!("resume bullet level {} - plain paragraph 이후 재개", idx + 1),
            styles,
            *shape,
        ));
    }
    paras
}

fn build_numbering_block(styles: &VisualStyles) -> Vec<Paragraph> {
    let mut paras = vec![
        title("커스텀 numbering format 10종", styles),
        note("Digit, CircledDigit, RomanSmall, LatinCapital, LatinSmall, CircledLatinSmall, HangulSyllable, CircledHangulSyllable, HangulJamo, RomanCapital을 순서대로 배치했다.", styles),
    ];

    for (idx, shape) in styles.numbered_primary.iter().enumerate() {
        paras.push(list_item(
            &format!("custom numbering level {} - 포맷과 label template를 눈으로 확인", idx + 1),
            styles,
            *shape,
        ));
    }
    paras.push(blank(styles));
    paras.push(title("start offset / prefix-suffix edge", styles));
    paras.push(note(
        "첫 번째 항목이 1이 아니라 5부터 시작해야 하고, [^1], CASE-^2, 제 ^4 조 같은 template가 보이면 정상이다.",
        styles,
    ));
    for (idx, shape) in styles.numbered_offset.iter().enumerate() {
        paras.push(list_item(
            &format!("offset numbering level {} - 시작 번호와 접두/접미 확인", idx + 1),
            styles,
            *shape,
        ));
    }
    paras.push(blank(styles));
    paras.push(title("nested numbering 4단계", styles));
    for (idx, shape) in styles.numbered_nested.iter().enumerate() {
        paras.push(list_item(
            &format!("nested numbering level {} - depth 유지 확인", idx + 1),
            styles,
            *shape,
        ));
    }
    paras
}

fn build_outline_block(styles: &VisualStyles) -> Vec<Paragraph> {
    let mut paras = vec![
        title("Outline level 1..10", styles),
        note("shared IR과 HWPX paraPr/heading(level)은 모두 zero-based다. 화면에서 개요 번호가 정상적으로 보이면 bridge가 level을 올바르게 내린 것이다.", styles),
    ];
    for (idx, shape) in styles.outline.iter().enumerate() {
        paras.push(list_item(
            &format!("outline level {} - 개요 번호와 들여쓰기 확인", idx + 1),
            styles,
            *shape,
        ));
    }
    paras.push(blank(styles));
    paras.push(body(
        "이 문단은 list semantics가 없는 일반 문단이다. outline block 뒤에 plain paragraph가 들어가도 numbering state가 망가지지 않는지 같이 본다.",
        styles,
    ));
    paras
}

fn build_mixed_block(styles: &VisualStyles) -> Vec<Paragraph> {
    vec![
        title("bullet -> number -> outline 전환", styles),
        list_item("bullet 시작", styles, styles.bullet_primary[0]),
        list_item("bullet nested", styles, styles.bullet_primary[1]),
        list_item("numbered로 전환", styles, styles.numbered_nested[0]),
        list_item("numbered nested", styles, styles.numbered_nested[1]),
        list_item("outline로 전환", styles, styles.outline[0]),
        list_item("outline nested", styles, styles.outline[1]),
        blank(styles),
        title("list 중간에 plain paragraph 끼우기", styles),
        list_item("ordered item before interruption", styles, styles.numbered_offset[0]),
        body(
            "이 문단은 list가 아니다. 번호가 끊겼다가 다음 문단에서 어떻게 보이는지 확인한다.",
            styles,
        ),
        list_item("ordered item after interruption", styles, styles.numbered_offset[0]),
        blank(styles),
        title("깊은 nested bullet + ordered 조합", styles),
        list_item("edge bullet level 1", styles, styles.bullet_edge[0]),
        list_item("edge bullet level 2", styles, styles.bullet_edge[1]),
        list_item("edge bullet level 3", styles, styles.bullet_edge[2]),
        list_item("nested ordered level 4", styles, styles.numbered_nested[3]),
        list_item("custom numbering level 6", styles, styles.numbered_primary[5]),
        blank(styles),
        title("plain paragraph와 list text가 비슷해도 semantics는 달라야 한다", styles),
        body("1. 이 문단은 문자 그대로 '1.'을 쓴 plain text다.", styles),
        list_item(
            "이 문단은 실제 numbering semantics를 사용한다.",
            styles,
            styles.numbered_primary[0],
        ),
        body("• 이 문단도 문자 그대로 bullet glyph를 쓴 plain text다.", styles),
        list_item("이 문단은 실제 bullet semantics를 사용한다.", styles, styles.bullet_primary[0]),
    ]
}

fn build_table_block(styles: &VisualStyles) -> Vec<Paragraph> {
    let table_para = Paragraph::with_runs(
        vec![Run::table(build_ordered_list_table(styles), styles.body_cs)],
        styles.body_ps,
    );

    vec![
        title("table cell 내부 ordered list matrix", styles),
        note(
            "왼쪽은 케이스 이름, 가운데는 실제 cell 내부 list paragraph, 오른쪽은 눈으로 볼 포인트다. 특히 row 4와 row 5를 보면 list가 table 안에서도 depth와 줄바꿈을 유지하는지 바로 드러난다.",
            styles,
        ),
        table_para,
        blank(styles),
        note(
            "추가 확인: 셀 내부 plain paragraph가 list 사이에 끼어 있어도 numbering이 깨지지 않는지, 좁은 셀에서 hanging indent가 망가지지 않는지 본다.",
            styles,
        ),
    ]
}

fn build_ordered_list_table(styles: &VisualStyles) -> Table {
    let case_w = HwpUnit::from_mm(28.0).expect("case width");
    let content_w = HwpUnit::from_mm(92.0).expect("content width");
    let check_w = HwpUnit::from_mm(40.0).expect("check width");

    let header = TableRow::new(vec![
        header_cell("케이스", case_w, styles),
        header_cell("셀 안의 ordered / numbered list", content_w, styles),
        header_cell("확인 포인트", check_w, styles),
    ])
    .with_header(true);

    let row_simple = TableRow::new(vec![
        plain_cell(
            vec![body("top-level ordered", styles)],
            case_w,
            Some(Color::from_rgb(245, 245, 245)),
        ),
        plain_cell(
            vec![
                list_item("간단한 1단계 ordered item A", styles, styles.numbered_primary[0]),
                list_item("간단한 1단계 ordered item B", styles, styles.numbered_primary[0]),
                list_item("간단한 1단계 ordered item C", styles, styles.numbered_primary[0]),
            ],
            content_w,
            None,
        ),
        plain_cell(
            vec![body("기본 numbering glyph와 top-level hanging indent", styles)],
            check_w,
            None,
        ),
    ]);

    let row_nested = TableRow::new(vec![
        plain_cell(
            vec![body("nested depth", styles)],
            case_w,
            Some(Color::from_rgb(245, 245, 245)),
        ),
        plain_cell(
            vec![
                list_item("level 1 parent", styles, styles.numbered_nested[0]),
                list_item("level 2 child", styles, styles.numbered_nested[1]),
                list_item("level 3 grandchild", styles, styles.numbered_nested[2]),
                list_item("level 4 deep child", styles, styles.numbered_nested[3]),
            ],
            content_w,
            None,
        ),
        plain_cell(
            vec![body("셀 내부에서도 level별 들여쓰기 누락이 없어야 한다", styles)],
            check_w,
            None,
        ),
    ]);

    let row_custom = TableRow::new(vec![
        plain_cell(
            vec![body("custom formats", styles)],
            case_w,
            Some(Color::from_rgb(245, 245, 245)),
        ),
        plain_cell(
            vec![
                list_item("digit", styles, styles.numbered_primary[0]),
                list_item("roman small", styles, styles.numbered_primary[2]),
                list_item("latin capital", styles, styles.numbered_primary[3]),
                list_item("circled latin small", styles, styles.numbered_primary[5]),
                list_item("hangul syllable", styles, styles.numbered_primary[6]),
            ],
            content_w,
            None,
        ),
        plain_cell(
            vec![body("포맷별 glyph와 label template가 서로 다르게 보여야 한다", styles)],
            check_w,
            None,
        ),
    ]);

    let row_offset = TableRow::new(vec![
        plain_cell(
            vec![body("offset + interruption", styles)],
            case_w,
            Some(Color::from_rgb(245, 245, 245)),
        ),
        plain_cell(
            vec![
                list_item("offset list before plain paragraph", styles, styles.numbered_offset[0]),
                body("이 문단은 같은 셀 안의 plain paragraph다.", styles),
                list_item("offset list after plain paragraph", styles, styles.numbered_offset[0]),
                list_item("offset nested child", styles, styles.numbered_offset[1]),
            ],
            content_w,
            None,
        ),
        plain_cell(
            vec![body(
                "첫 번호가 5부터 시작하고, plain paragraph가 끼어도 흐름이 무너지지 않는지",
                styles,
            )],
            check_w,
            None,
        ),
    ]);

    let row_narrow = TableRow::new(vec![
        plain_cell(
            vec![body("narrow wrapping", styles)],
            case_w,
            Some(Color::from_rgb(245, 245, 245)),
        ),
        plain_cell(
            vec![
                list_item(
                    "폭이 좁은 셀 안에서 긴 ordered list 문장이 줄바꿈되더라도 번호와 hanging indent 정렬이 깨지지 않아야 한다.",
                    styles,
                    styles.numbered_primary[0],
                ),
                list_item(
                    "두 번째 항목도 동일하게 줄바꿈되어, 본문만 접히고 번호 시작선은 유지되는지 본다.",
                    styles,
                    styles.numbered_primary[1],
                ),
            ],
            HwpUnit::from_mm(70.0).expect("narrow content width"),
            None,
        ),
        plain_cell(
            vec![body("좁은 셀에서 번호와 본문 정렬이 분리되는지", styles)],
            check_w,
            None,
        ),
    ]);

    let row_compare = TableRow::new(vec![
        plain_cell(
            vec![body("same text / different semantics", styles)],
            case_w,
            Some(Color::from_rgb(245, 245, 245)),
        ),
        plain_cell(
            vec![
                body("1. 이 줄은 plain text다.", styles),
                list_item(
                    "이 줄은 실제 numbering semantics다.",
                    styles,
                    styles.numbered_primary[0],
                ),
                body("[5] 이 줄도 plain text다.", styles),
                list_item(
                    "이 줄은 offset numbering semantics다.",
                    styles,
                    styles.numbered_offset[0],
                ),
            ],
            content_w,
            None,
        ),
        plain_cell(
            vec![body("눈에 비슷해도 plain text와 real list가 다르게 작동해야 한다", styles)],
            check_w,
            None,
        ),
    ]);

    Table::new(vec![
        header,
        row_simple,
        row_nested,
        row_custom,
        row_offset,
        row_narrow,
        row_compare,
    ])
    .with_width(HwpUnit::from_mm(160.0).expect("table width"))
    .with_page_break(TablePageBreak::Cell)
    .with_repeat_header(true)
    .with_cell_spacing(HwpUnit::from_mm(1.0).expect("cell spacing"))
}

fn header_cell(text: &str, width: HwpUnit, styles: &VisualStyles) -> TableCell {
    plain_cell(vec![title(text, styles)], width, Some(Color::from_rgb(220, 228, 240)))
}

fn plain_cell(paragraphs: Vec<Paragraph>, width: HwpUnit, background: Option<Color>) -> TableCell {
    let cell = TableCell::new(paragraphs, width);
    if let Some(background) = background {
        return cell.with_background(background);
    }
    cell
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
    text.push_str("# List Shared Semantics Visual Checklist\n\n");
    text.push_str("Generated files:\n");
    text.push_str("- 00_all_in_one.hwpx: bullet / numbering / outline / mixed edge case 종합본\n");
    text.push_str("- 01_bullet_matrix.hwpx: bullet glyph 3종 + depth 5단계\n");
    text.push_str("- 02_numbering_custom_formats.hwpx: numbering format 10종 + start offset\n");
    text.push_str("- 03_outline_depth.hwpx: outline level 1..10\n");
    text.push_str("- 04_mixed_edge_cases.hwpx: bullet/number/outline transition\n\n");
    text.push_str("- 05_table_ordered_lists.hwpx: table cell 내부 ordered / numbered list\n\n");
    text.push_str("Visual checks:\n");
    text.push_str("- bullet glyph가 • / ◦ / ※ 로 바뀌는지\n");
    text.push_str("- nested level이 깊어질수록 들여쓰기가 유지되는지\n");
    text.push_str(
        "- numbering format이 Digit / Roman / Latin / Hangul / Circled 계열로 각각 보이는지\n",
    );
    text.push_str("- start offset 문단이 5부터 시작하는지\n");
    text.push_str("- outline level 1..10이 순서대로 보이는지\n");
    text.push_str("- plain paragraph와 real list paragraph가 화면에서 구분되는지\n");
    fs::write(out_dir.join("README.md"), text).expect("write manifest");
}

fn write_debug_manifest(out_dir: &Path) {
    let mut text = String::new();
    text.push_str("# Debug Isolation Opening Order\n\n");
    text.push_str("`00_*` files isolate the cumulative steps of `00_all_in_one.hwpx`.\n");
    text.push_str("`01_*` to `06_*` files isolate the bullet block that used to crash `01_bullet_matrix.hwpx`.\n\n");
    text.push_str("Recommended opening order:\n");
    text.push_str("1. 00_step0_cover_only.hwpx\n");
    text.push_str("2. 01_bullet_min_single.hwpx\n");
    text.push_str("3. 02_bullet_nested_2.hwpx\n");
    text.push_str("4. 03_bullet_nested_5.hwpx\n");
    text.push_str("5. 04_bullet_wrapping.hwpx\n");
    text.push_str("6. 05_bullet_interruption.hwpx\n");
    text.push_str("7. 06_bullet_plain_vs_semantic.hwpx\n");
    text.push_str("8. 00_step1_cover_plus_bullet.hwpx\n");
    text.push_str("9. 00_step2_add_numbering.hwpx\n");
    text.push_str("10. 00_step3_add_outline.hwpx\n");
    text.push_str("11. 00_step4_add_mixed.hwpx\n");
    text.push_str("12. 00_step5_add_table.hwpx\n\n");
    text.push_str(
        "Record which file first crashes Hancom. That tells us the smallest bad combination.\n",
    );
    fs::write(out_dir.join("README.md"), text).expect("write debug manifest");
}
