//! Structural reporting for equations embedded in visual HWPX objects.

use serde::Serialize;

use crate::error::{HwpxError, HwpxResult};
use crate::schema::section::{
    HxContainer, HxContainerChildOrder, HxCtrl, HxDrawText, HxEllipse, HxEquation, HxOffset,
    HxParagraph, HxPic, HxPolygon, HxRect, HxRenderingInfo, HxRun, HxRunChildOrder, HxSizeAttr,
    HxSubList, HxTable, HxTablePos, HxTableSz,
};

pub(crate) const HWPX_VISUAL_EQUATION_SCHEMA_VERSION: u32 = 4;

/// Versioned report of equations that styled Markdown intentionally leaves
/// inside picture captions and grouped drawing text.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HwpxVisualEquationReport {
    /// Sidecar schema version.
    pub schema_version: u32,
    /// Visual equations in stable document order.
    pub equations: Vec<HwpxVisualEquation>,
}

impl Default for HwpxVisualEquationReport {
    fn default() -> Self {
        Self { schema_version: HWPX_VISUAL_EQUATION_SCHEMA_VERSION, equations: Vec::new() }
    }
}

/// One equation occurrence embedded in a visual HWPX parent.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HwpxVisualEquation {
    /// Equation wire ID, or a deterministic path-derived fallback.
    pub id: String,
    /// Visual domain that owns the equation.
    pub domain: HwpxVisualEquationDomain,
    /// Equation object's wire `@id`, when present.
    pub equation_object_id: Option<String>,
    /// Nearest picture or grouped-rectangle wire `@id`, when present.
    pub parent_object_id: Option<String>,
    /// Nearest picture or grouped-rectangle wire `@instid`, when present.
    pub parent_instance_id: Option<String>,
    /// Visual parent category.
    pub parent_kind: HwpxVisualEquationParentKind,
    /// Stable section/paragraph/run/type-index path to the visual parent.
    pub parent_path: String,
    /// Zero-based order across every visual equation in the document.
    pub document_order: usize,
    /// Zero-based equation occurrence order inside the visual parent.
    pub parent_order: usize,
    /// Equation z-order, falling back to the nearest visual parent when absent.
    pub z_order: u32,
    /// Raw equation position before the visual parent's rendering translation.
    pub raw_position: HwpxVisualEquationPosition,
    /// Exact translation selected from the visual parent's final `scaMatrix`.
    pub translation: HwpxVisualEquationTranslation,
    /// Display-space position after applying the rendering translation.
    pub display_position: Option<HwpxVisualEquationPosition>,
    /// Raw and display-space geometry for faithful visual composition.
    pub geometry: HwpxVisualEquationGeometry,
    /// Original HancomEQN source.
    pub script: String,
    /// Converted LaTeX, populated by a format consumer such as the CLI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latex: Option<String>,
}

/// Supported visual-equation domains.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HwpxVisualEquationDomain {
    /// Equation found under `pic/caption` paragraph content.
    PictureCaption,
    /// Equation found under `container/rect/drawText` paragraph content.
    GroupDrawText,
}

/// Supported visual parent categories.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HwpxVisualEquationParentKind {
    /// Picture object.
    Picture,
    /// Group container, represented by its nearest text-bearing rectangle.
    Container,
}

/// HWPX position offsets for a visual equation.
#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
pub struct HwpxVisualEquationPosition {
    /// Horizontal offset in HWP units.
    pub horz_offset: i32,
    /// Vertical offset in HWP units.
    pub vert_offset: i32,
}

/// Exact horizontal and vertical translation strings preserved from HWPX.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HwpxVisualEquationTranslation {
    /// Horizontal translation (`scaMatrix.e3`).
    pub horz: String,
    /// Vertical translation (`scaMatrix.e6`).
    pub vert: String,
}

/// Raw wire geometry and scale-applied display geometry for a visual equation.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HwpxVisualEquationGeometry {
    /// Parent box size before its rendering scale is applied.
    pub raw_box_size: Option<HwpxVisualEquationSize>,
    /// Equation size before its parent rendering scale is applied.
    pub raw_equation_size: Option<HwpxVisualEquationSize>,
    /// Equation `baseUnit` before its parent rendering scale is applied.
    pub raw_base_unit: Option<u32>,
    /// Canonical equation font base for rendering, falling back to raw equation height.
    pub render_base_unit: Option<u32>,
    /// Exact wire scale selected from the visual parent's final `scaMatrix`.
    pub scale: HwpxVisualEquationScale,
    /// Parent box size after applying the wire scale, rounded to HWP units.
    pub display_box_size: Option<HwpxVisualEquationSize>,
    /// Equation size after applying the wire scale, rounded to HWP units.
    pub display_equation_size: Option<HwpxVisualEquationSize>,
}

/// Width and height in HWP units.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct HwpxVisualEquationSize {
    /// Horizontal extent.
    pub width: i32,
    /// Vertical extent.
    pub height: i32,
}

/// Exact horizontal and vertical scale strings preserved from HWPX.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HwpxVisualEquationScale {
    /// Horizontal scale (`scaMatrix.e1`).
    pub horz: String,
    /// Vertical scale (`scaMatrix.e5`).
    pub vert: String,
}

#[derive(Clone, Copy)]
struct VisualScale<'a> {
    horz: &'a str,
    vert: &'a str,
}

#[derive(Clone, Copy)]
struct VisualTranslation<'a> {
    horz: &'a str,
    vert: &'a str,
}

impl Default for VisualTranslation<'_> {
    fn default() -> Self {
        Self { horz: "0", vert: "0" }
    }
}

impl Default for VisualScale<'_> {
    fn default() -> Self {
        Self { horz: "1", vert: "1" }
    }
}

#[derive(Clone, Copy)]
struct VisualParent<'a> {
    domain: HwpxVisualEquationDomain,
    kind: HwpxVisualEquationParentKind,
    object_id: &'a str,
    instance_id: &'a str,
    z_order: u32,
    position: Option<HwpxVisualEquationPosition>,
    raw_box_size: Option<HwpxVisualEquationSize>,
    scale: VisualScale<'a>,
    translation: VisualTranslation<'a>,
    compose_child_position: bool,
}

pub(crate) fn collect_section(
    paragraphs: &[HxParagraph],
    section_index: usize,
) -> HwpxResult<Vec<HwpxVisualEquation>> {
    let mut equations = Vec::new();
    let root = format!("section[{section_index}]");
    walk_visual_paragraphs(paragraphs, &root, 0, &mut equations)?;
    Ok(equations)
}

fn walk_visual_paragraphs(
    paragraphs: &[HxParagraph],
    prefix: &str,
    depth: usize,
    equations: &mut Vec<HwpxVisualEquation>,
) -> HwpxResult<()> {
    ensure_depth(depth)?;
    for (paragraph_index, paragraph) in paragraphs.iter().enumerate() {
        let paragraph_path = format!("{prefix}/paragraph[{paragraph_index}]");
        for (run_index, run) in paragraph.runs.iter().enumerate() {
            let run_path = format!("{paragraph_path}/run[{run_index}]");
            walk_visual_run(run, &run_path, depth, equations)?;
        }
    }
    Ok(())
}

fn walk_visual_run(
    run: &HxRun,
    path: &str,
    depth: usize,
    equations: &mut Vec<HwpxVisualEquation>,
) -> HwpxResult<()> {
    if run.child_order.is_empty() {
        return walk_run_by_type(run, path, depth, equations);
    }

    for child in &run.child_order {
        match *child {
            HxRunChildOrder::Table(index) => {
                walk_table(
                    &run.tables[index],
                    &format!("{path}/table[{index}]"),
                    depth,
                    equations,
                )?;
            }
            HxRunChildOrder::Picture(index) => {
                collect_picture(
                    &run.pictures[index],
                    &format!("{path}/picture[{index}]"),
                    depth,
                    equations,
                )?;
            }
            HxRunChildOrder::Ctrl(index) => {
                walk_ctrl(&run.ctrls[index], &format!("{path}/ctrl[{index}]"), depth, equations)?;
            }
            HxRunChildOrder::Container(index) => {
                collect_container(
                    &run.containers[index],
                    &format!("{path}/container[{index}]"),
                    depth,
                    None,
                    equations,
                )?;
            }
            HxRunChildOrder::Rect(index) => walk_shape_text_visuals(
                run.rects[index].draw_text.as_ref(),
                &format!("{path}/rect[{index}]"),
                depth,
                equations,
            )?,
            HxRunChildOrder::Ellipse(index) => walk_shape_text_visuals(
                run.ellipses[index].draw_text.as_ref(),
                &format!("{path}/ellipse[{index}]"),
                depth,
                equations,
            )?,
            HxRunChildOrder::Polygon(index) => walk_shape_text_visuals(
                run.polygons[index].draw_text.as_ref(),
                &format!("{path}/polygon[{index}]"),
                depth,
                equations,
            )?,
            _ => {}
        }
    }
    Ok(())
}

fn walk_run_by_type(
    run: &HxRun,
    path: &str,
    depth: usize,
    equations: &mut Vec<HwpxVisualEquation>,
) -> HwpxResult<()> {
    for (index, table) in run.tables.iter().enumerate() {
        walk_table(table, &format!("{path}/table[{index}]"), depth, equations)?;
    }
    for (index, picture) in run.pictures.iter().enumerate() {
        collect_picture(picture, &format!("{path}/picture[{index}]"), depth, equations)?;
    }
    for (index, ctrl) in run.ctrls.iter().enumerate() {
        walk_ctrl(ctrl, &format!("{path}/ctrl[{index}]"), depth, equations)?;
    }
    for (index, container) in run.containers.iter().enumerate() {
        collect_container(
            container,
            &format!("{path}/container[{index}]"),
            depth,
            None,
            equations,
        )?;
    }
    for (index, rect) in run.rects.iter().enumerate() {
        walk_shape_text_visuals(
            rect.draw_text.as_ref(),
            &format!("{path}/rect[{index}]"),
            depth,
            equations,
        )?;
    }
    for (index, ellipse) in run.ellipses.iter().enumerate() {
        walk_shape_text_visuals(
            ellipse.draw_text.as_ref(),
            &format!("{path}/ellipse[{index}]"),
            depth,
            equations,
        )?;
    }
    for (index, polygon) in run.polygons.iter().enumerate() {
        walk_shape_text_visuals(
            polygon.draw_text.as_ref(),
            &format!("{path}/polygon[{index}]"),
            depth,
            equations,
        )?;
    }
    Ok(())
}

fn walk_shape_text_visuals(
    draw_text: Option<&HxDrawText>,
    path: &str,
    depth: usize,
    equations: &mut Vec<HwpxVisualEquation>,
) -> HwpxResult<()> {
    let Some(draw_text) = draw_text else { return Ok(()) };
    walk_visual_paragraphs(
        &draw_text.sub_list.paragraphs,
        &format!("{path}/drawText"),
        depth + 1,
        equations,
    )
}

fn walk_table(
    table: &HxTable,
    path: &str,
    depth: usize,
    equations: &mut Vec<HwpxVisualEquation>,
) -> HwpxResult<()> {
    if let Some(caption) = &table.caption {
        walk_visual_paragraphs(
            &caption.sub_list.paragraphs,
            &format!("{path}/caption"),
            depth + 1,
            equations,
        )?;
    }
    for (row_index, row) in table.rows.iter().enumerate() {
        for (cell_index, cell) in row.cells.iter().enumerate() {
            if let Some(sub_list) = &cell.sub_list {
                walk_visual_paragraphs(
                    &sub_list.paragraphs,
                    &format!("{path}/row[{row_index}]/cell[{cell_index}]"),
                    depth + 1,
                    equations,
                )?;
            }
        }
    }
    Ok(())
}

fn walk_ctrl(
    ctrl: &HxCtrl,
    path: &str,
    depth: usize,
    equations: &mut Vec<HwpxVisualEquation>,
) -> HwpxResult<()> {
    if let Some(header) = &ctrl.header {
        if let Some(sub_list) = &header.sub_list {
            walk_visual_paragraphs(
                &sub_list.paragraphs,
                &format!("{path}/header"),
                depth + 1,
                equations,
            )?;
        }
    }
    if let Some(footer) = &ctrl.footer {
        if let Some(sub_list) = &footer.sub_list {
            walk_visual_paragraphs(
                &sub_list.paragraphs,
                &format!("{path}/footer"),
                depth + 1,
                equations,
            )?;
        }
    }
    if let Some(footnote) = &ctrl.foot_note {
        walk_visual_paragraphs(
            &footnote.sub_list.paragraphs,
            &format!("{path}/footnote"),
            depth + 1,
            equations,
        )?;
    }
    if let Some(endnote) = &ctrl.end_note {
        walk_visual_paragraphs(
            &endnote.sub_list.paragraphs,
            &format!("{path}/endnote"),
            depth + 1,
            equations,
        )?;
    }
    Ok(())
}

fn collect_picture(
    pic: &HxPic,
    path: &str,
    depth: usize,
    equations: &mut Vec<HwpxVisualEquation>,
) -> HwpxResult<()> {
    let Some(caption) = &pic.caption else { return Ok(()) };
    let parent = VisualParent {
        domain: HwpxVisualEquationDomain::PictureCaption,
        kind: HwpxVisualEquationParentKind::Picture,
        object_id: &pic.id,
        instance_id: &pic.instid,
        z_order: pic.z_order,
        position: pic
            .pos
            .as_ref()
            .map(position_from_table)
            .or_else(|| pic.offset.as_ref().map(position_from_offset)),
        raw_box_size: None,
        scale: pic
            .rendering_info
            .as_ref()
            .map(|rendering| VisualScale {
                horz: rendering.sca_matrix.e1.as_str(),
                vert: rendering.sca_matrix.e5.as_str(),
            })
            .unwrap_or_default(),
        translation: pic
            .rendering_info
            .as_ref()
            .map(|rendering| VisualTranslation {
                horz: rendering.sca_matrix.e3.as_str(),
                vert: rendering.sca_matrix.e6.as_str(),
            })
            .unwrap_or_default(),
        compose_child_position: false,
    };
    collect_parent_equations(&caption.sub_list, parent, path, depth, equations)
}

fn collect_container(
    container: &HxContainer,
    path: &str,
    depth: usize,
    inherited_position: Option<HwpxVisualEquationPosition>,
    equations: &mut Vec<HwpxVisualEquation>,
) -> HwpxResult<()> {
    ensure_depth(depth)?;
    let container_position = add_optional_positions(
        inherited_position,
        container.pos.as_ref().map(position_from_table),
    )?;
    if !container.child_order.is_empty() {
        for child in &container.child_order {
            match *child {
                HxContainerChildOrder::Rect(rect_index) => collect_group_rect(
                    &container.rects[rect_index],
                    &format!("{path}/rect[{rect_index}]"),
                    depth,
                    container_position,
                    equations,
                )?,
                HxContainerChildOrder::Ellipse(ellipse_index) => collect_group_ellipse(
                    &container.ellipses[ellipse_index],
                    &format!("{path}/ellipse[{ellipse_index}]"),
                    depth,
                    container_position,
                    equations,
                )?,
                HxContainerChildOrder::Polygon(polygon_index) => collect_group_polygon(
                    &container.polygons[polygon_index],
                    &format!("{path}/polygon[{polygon_index}]"),
                    depth,
                    container_position,
                    equations,
                )?,
                HxContainerChildOrder::Container(container_index) => collect_container(
                    &container.containers[container_index],
                    &format!("{path}/container[{container_index}]"),
                    depth + 1,
                    container_position,
                    equations,
                )?,
                _ => {}
            }
        }
        return Ok(());
    }
    for (rect_index, rect) in container.rects.iter().enumerate() {
        collect_group_rect(
            rect,
            &format!("{path}/rect[{rect_index}]"),
            depth,
            container_position,
            equations,
        )?;
    }
    for (ellipse_index, ellipse) in container.ellipses.iter().enumerate() {
        collect_group_ellipse(
            ellipse,
            &format!("{path}/ellipse[{ellipse_index}]"),
            depth,
            container_position,
            equations,
        )?;
    }
    for (polygon_index, polygon) in container.polygons.iter().enumerate() {
        collect_group_polygon(
            polygon,
            &format!("{path}/polygon[{polygon_index}]"),
            depth,
            container_position,
            equations,
        )?;
    }
    for (container_index, nested) in container.containers.iter().enumerate() {
        collect_container(
            nested,
            &format!("{path}/container[{container_index}]"),
            depth + 1,
            container_position,
            equations,
        )?;
    }
    Ok(())
}

fn collect_group_rect(
    rect: &HxRect,
    path: &str,
    depth: usize,
    container_position: Option<HwpxVisualEquationPosition>,
    equations: &mut Vec<HwpxVisualEquation>,
) -> HwpxResult<()> {
    collect_group_shape(
        rect.draw_text.as_ref(),
        &rect.id,
        &rect.instid,
        rect.z_order,
        rect.offset.as_ref(),
        rect.pos.as_ref(),
        rect.org_sz.as_ref(),
        rect.rendering_info.as_ref(),
        path,
        depth,
        container_position,
        equations,
    )
}

fn collect_group_ellipse(
    ellipse: &HxEllipse,
    path: &str,
    depth: usize,
    container_position: Option<HwpxVisualEquationPosition>,
    equations: &mut Vec<HwpxVisualEquation>,
) -> HwpxResult<()> {
    collect_group_shape(
        ellipse.draw_text.as_ref(),
        &ellipse.id,
        &ellipse.instid,
        ellipse.z_order,
        ellipse.offset.as_ref(),
        ellipse.pos.as_ref(),
        ellipse.org_sz.as_ref(),
        ellipse.rendering_info.as_ref(),
        path,
        depth,
        container_position,
        equations,
    )
}

fn collect_group_polygon(
    polygon: &HxPolygon,
    path: &str,
    depth: usize,
    container_position: Option<HwpxVisualEquationPosition>,
    equations: &mut Vec<HwpxVisualEquation>,
) -> HwpxResult<()> {
    collect_group_shape(
        polygon.draw_text.as_ref(),
        &polygon.id,
        &polygon.instid,
        polygon.z_order,
        polygon.offset.as_ref(),
        polygon.pos.as_ref(),
        polygon.org_sz.as_ref(),
        polygon.rendering_info.as_ref(),
        path,
        depth,
        container_position,
        equations,
    )
}

#[allow(clippy::too_many_arguments)]
fn collect_group_shape(
    draw_text: Option<&HxDrawText>,
    object_id: &str,
    instance_id: &str,
    z_order: u32,
    offset: Option<&HxOffset>,
    position: Option<&HxTablePos>,
    original_size: Option<&HxSizeAttr>,
    rendering_info: Option<&HxRenderingInfo>,
    path: &str,
    depth: usize,
    container_position: Option<HwpxVisualEquationPosition>,
    equations: &mut Vec<HwpxVisualEquation>,
) -> HwpxResult<()> {
    let Some(draw_text) = draw_text else { return Ok(()) };
    let shape_position =
        offset.map(position_from_offset).or_else(|| position.map(position_from_table));
    let parent = VisualParent {
        domain: HwpxVisualEquationDomain::GroupDrawText,
        kind: HwpxVisualEquationParentKind::Container,
        object_id,
        instance_id,
        z_order,
        position: add_optional_positions(container_position, shape_position)?,
        raw_box_size: original_size.map(size_from_original),
        scale: rendering_info
            .map(|rendering| VisualScale {
                horz: rendering.sca_matrix.e1.as_str(),
                vert: rendering.sca_matrix.e5.as_str(),
            })
            .unwrap_or_default(),
        translation: rendering_info
            .map(|rendering| VisualTranslation {
                horz: rendering.sca_matrix.e3.as_str(),
                vert: rendering.sca_matrix.e6.as_str(),
            })
            .unwrap_or_default(),
        compose_child_position: true,
    };
    collect_parent_equations(&draw_text.sub_list, parent, path, depth, equations)
}

fn collect_parent_equations(
    sub_list: &HxSubList,
    parent: VisualParent<'_>,
    parent_path: &str,
    depth: usize,
    equations: &mut Vec<HwpxVisualEquation>,
) -> HwpxResult<()> {
    let mut parent_order = 0;
    collect_owned_visual_paragraphs(
        &sub_list.paragraphs,
        parent,
        parent_path,
        parent_path,
        depth,
        &mut parent_order,
        equations,
    )
}

#[allow(clippy::too_many_arguments)]
fn collect_owned_visual_paragraphs(
    paragraphs: &[HxParagraph],
    parent: VisualParent<'_>,
    parent_path: &str,
    prefix: &str,
    depth: usize,
    parent_order: &mut usize,
    equations: &mut Vec<HwpxVisualEquation>,
) -> HwpxResult<()> {
    ensure_depth(depth)?;
    for (paragraph_index, paragraph) in paragraphs.iter().enumerate() {
        let paragraph_path = format!("{prefix}/paragraph[{paragraph_index}]");
        for (run_index, run) in paragraph.runs.iter().enumerate() {
            let run_path = format!("{paragraph_path}/run[{run_index}]");
            if run.child_order.is_empty() {
                for equation in &run.equations {
                    push_parent_equation(equation, parent, parent_path, parent_order, equations)?;
                }
                for (index, table) in run.tables.iter().enumerate() {
                    collect_owned_visual_table(
                        table,
                        parent,
                        parent_path,
                        &format!("{run_path}/table[{index}]"),
                        depth,
                        parent_order,
                        equations,
                    )?;
                }
                for (index, ctrl) in run.ctrls.iter().enumerate() {
                    collect_owned_visual_ctrl(
                        ctrl,
                        parent,
                        parent_path,
                        &format!("{run_path}/ctrl[{index}]"),
                        depth,
                        parent_order,
                        equations,
                    )?;
                }
                for (index, rect) in run.rects.iter().enumerate() {
                    collect_owned_shape_text(
                        rect.draw_text.as_ref(),
                        rect.offset.as_ref(),
                        rect.pos.as_ref(),
                        rect.org_sz.as_ref(),
                        rect.rendering_info.as_ref(),
                        parent,
                        parent_path,
                        &format!("{run_path}/rect[{index}]"),
                        depth,
                        parent_order,
                        equations,
                    )?;
                }
                for (index, ellipse) in run.ellipses.iter().enumerate() {
                    collect_owned_shape_text(
                        ellipse.draw_text.as_ref(),
                        ellipse.offset.as_ref(),
                        ellipse.pos.as_ref(),
                        ellipse.org_sz.as_ref(),
                        ellipse.rendering_info.as_ref(),
                        parent,
                        parent_path,
                        &format!("{run_path}/ellipse[{index}]"),
                        depth,
                        parent_order,
                        equations,
                    )?;
                }
                for (index, polygon) in run.polygons.iter().enumerate() {
                    collect_owned_shape_text(
                        polygon.draw_text.as_ref(),
                        polygon.offset.as_ref(),
                        polygon.pos.as_ref(),
                        polygon.org_sz.as_ref(),
                        polygon.rendering_info.as_ref(),
                        parent,
                        parent_path,
                        &format!("{run_path}/polygon[{index}]"),
                        depth,
                        parent_order,
                        equations,
                    )?;
                }
                for (index, picture) in run.pictures.iter().enumerate() {
                    collect_picture(
                        picture,
                        &format!("{run_path}/picture[{index}]"),
                        depth,
                        equations,
                    )?;
                }
                for (index, container) in run.containers.iter().enumerate() {
                    collect_container(
                        container,
                        &format!("{run_path}/container[{index}]"),
                        depth,
                        None,
                        equations,
                    )?;
                }
            } else {
                for child in &run.child_order {
                    match *child {
                        HxRunChildOrder::Equation(index) => push_parent_equation(
                            &run.equations[index],
                            parent,
                            parent_path,
                            parent_order,
                            equations,
                        )?,
                        HxRunChildOrder::Table(index) => collect_owned_visual_table(
                            &run.tables[index],
                            parent,
                            parent_path,
                            &format!("{run_path}/table[{index}]"),
                            depth,
                            parent_order,
                            equations,
                        )?,
                        HxRunChildOrder::Ctrl(index) => collect_owned_visual_ctrl(
                            &run.ctrls[index],
                            parent,
                            parent_path,
                            &format!("{run_path}/ctrl[{index}]"),
                            depth,
                            parent_order,
                            equations,
                        )?,
                        HxRunChildOrder::Rect(index) => collect_owned_shape_text(
                            run.rects[index].draw_text.as_ref(),
                            run.rects[index].offset.as_ref(),
                            run.rects[index].pos.as_ref(),
                            run.rects[index].org_sz.as_ref(),
                            run.rects[index].rendering_info.as_ref(),
                            parent,
                            parent_path,
                            &format!("{run_path}/rect[{index}]"),
                            depth,
                            parent_order,
                            equations,
                        )?,
                        HxRunChildOrder::Ellipse(index) => collect_owned_shape_text(
                            run.ellipses[index].draw_text.as_ref(),
                            run.ellipses[index].offset.as_ref(),
                            run.ellipses[index].pos.as_ref(),
                            run.ellipses[index].org_sz.as_ref(),
                            run.ellipses[index].rendering_info.as_ref(),
                            parent,
                            parent_path,
                            &format!("{run_path}/ellipse[{index}]"),
                            depth,
                            parent_order,
                            equations,
                        )?,
                        HxRunChildOrder::Polygon(index) => collect_owned_shape_text(
                            run.polygons[index].draw_text.as_ref(),
                            run.polygons[index].offset.as_ref(),
                            run.polygons[index].pos.as_ref(),
                            run.polygons[index].org_sz.as_ref(),
                            run.polygons[index].rendering_info.as_ref(),
                            parent,
                            parent_path,
                            &format!("{run_path}/polygon[{index}]"),
                            depth,
                            parent_order,
                            equations,
                        )?,
                        HxRunChildOrder::Picture(index) => collect_picture(
                            &run.pictures[index],
                            &format!("{run_path}/picture[{index}]"),
                            depth,
                            equations,
                        )?,
                        HxRunChildOrder::Container(index) => collect_container(
                            &run.containers[index],
                            &format!("{run_path}/container[{index}]"),
                            depth,
                            None,
                            equations,
                        )?,
                        _ => {}
                    }
                }
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn collect_owned_shape_text(
    draw_text: Option<&HxDrawText>,
    offset: Option<&HxOffset>,
    position: Option<&HxTablePos>,
    original_size: Option<&HxSizeAttr>,
    rendering_info: Option<&HxRenderingInfo>,
    parent: VisualParent<'_>,
    parent_path: &str,
    path: &str,
    depth: usize,
    parent_order: &mut usize,
    equations: &mut Vec<HwpxVisualEquation>,
) -> HwpxResult<()> {
    if let Some(draw_text) = draw_text {
        let child_position =
            offset.map(position_from_offset).or_else(|| position.map(position_from_table));
        let child_scale = rendering_info
            .map(|rendering| VisualScale {
                horz: rendering.sca_matrix.e1.as_str(),
                vert: rendering.sca_matrix.e5.as_str(),
            })
            .unwrap_or_default();
        let child_translation = rendering_info
            .map(|rendering| VisualTranslation {
                horz: rendering.sca_matrix.e3.as_str(),
                vert: rendering.sca_matrix.e6.as_str(),
            })
            .unwrap_or_default();
        let scale_horz = compose_scale(parent.scale.horz, child_scale.horz)?;
        let scale_vert = compose_scale(parent.scale.vert, child_scale.vert)?;
        let translation_horz = compose_translation(
            parent.translation.horz,
            parent.scale.horz,
            child_translation.horz,
        )?;
        let translation_vert = compose_translation(
            parent.translation.vert,
            parent.scale.vert,
            child_translation.vert,
        )?;
        let nested_parent = VisualParent {
            position: add_optional_positions(parent.position, child_position)?,
            raw_box_size: original_size.map(size_from_original).or(parent.raw_box_size),
            scale: VisualScale { horz: &scale_horz, vert: &scale_vert },
            translation: VisualTranslation { horz: &translation_horz, vert: &translation_vert },
            compose_child_position: true,
            ..parent
        };
        collect_owned_visual_paragraphs(
            &draw_text.sub_list.paragraphs,
            nested_parent,
            parent_path,
            &format!("{path}/drawText"),
            depth + 1,
            parent_order,
            equations,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn collect_owned_visual_table(
    table: &HxTable,
    parent: VisualParent<'_>,
    parent_path: &str,
    path: &str,
    depth: usize,
    parent_order: &mut usize,
    equations: &mut Vec<HwpxVisualEquation>,
) -> HwpxResult<()> {
    if let Some(caption) = &table.caption {
        collect_owned_visual_paragraphs(
            &caption.sub_list.paragraphs,
            parent,
            parent_path,
            &format!("{path}/caption"),
            depth + 1,
            parent_order,
            equations,
        )?;
    }
    for (row_index, row) in table.rows.iter().enumerate() {
        for (cell_index, cell) in row.cells.iter().enumerate() {
            if let Some(sub_list) = &cell.sub_list {
                collect_owned_visual_paragraphs(
                    &sub_list.paragraphs,
                    parent,
                    parent_path,
                    &format!("{path}/row[{row_index}]/cell[{cell_index}]"),
                    depth + 1,
                    parent_order,
                    equations,
                )?;
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn collect_owned_visual_ctrl(
    ctrl: &HxCtrl,
    parent: VisualParent<'_>,
    parent_path: &str,
    path: &str,
    depth: usize,
    parent_order: &mut usize,
    equations: &mut Vec<HwpxVisualEquation>,
) -> HwpxResult<()> {
    if let Some(header) = &ctrl.header {
        if let Some(sub_list) = &header.sub_list {
            collect_owned_visual_paragraphs(
                &sub_list.paragraphs,
                parent,
                parent_path,
                &format!("{path}/header"),
                depth + 1,
                parent_order,
                equations,
            )?;
        }
    }
    if let Some(footer) = &ctrl.footer {
        if let Some(sub_list) = &footer.sub_list {
            collect_owned_visual_paragraphs(
                &sub_list.paragraphs,
                parent,
                parent_path,
                &format!("{path}/footer"),
                depth + 1,
                parent_order,
                equations,
            )?;
        }
    }
    if let Some(footnote) = &ctrl.foot_note {
        collect_owned_visual_paragraphs(
            &footnote.sub_list.paragraphs,
            parent,
            parent_path,
            &format!("{path}/footnote"),
            depth + 1,
            parent_order,
            equations,
        )?;
    }
    if let Some(endnote) = &ctrl.end_note {
        collect_owned_visual_paragraphs(
            &endnote.sub_list.paragraphs,
            parent,
            parent_path,
            &format!("{path}/endnote"),
            depth + 1,
            parent_order,
            equations,
        )?;
    }
    Ok(())
}

fn push_parent_equation(
    equation: &HxEquation,
    parent: VisualParent<'_>,
    parent_path: &str,
    parent_order: &mut usize,
    equations: &mut Vec<HwpxVisualEquation>,
) -> HwpxResult<()> {
    let current_parent_order = *parent_order;
    let equation_object_id = nonempty(&equation.id);
    let id = equation_object_id
        .clone()
        .unwrap_or_else(|| format!("{parent_path}/equation[{current_parent_order}]"));
    let raw_position = equation_position(equation, parent)?;
    let translation = HwpxVisualEquationTranslation {
        horz: parent.translation.horz.to_string(),
        vert: parent.translation.vert.to_string(),
    };
    let display_position = translate_position(raw_position, parent.translation);
    let geometry = equation_geometry(equation, parent);
    let z_order = equation.z_order.unwrap_or(parent.z_order);
    equations.push(HwpxVisualEquation {
        id,
        domain: parent.domain,
        equation_object_id,
        parent_object_id: nonempty(parent.object_id),
        parent_instance_id: nonempty(parent.instance_id),
        parent_kind: parent.kind,
        parent_path: parent_path.to_string(),
        document_order: 0,
        parent_order: current_parent_order,
        z_order,
        raw_position,
        translation,
        display_position,
        geometry,
        script: equation.script.as_ref().map(|script| script.text.clone()).unwrap_or_default(),
        latex: None,
    });
    *parent_order += 1;
    Ok(())
}

fn ensure_depth(depth: usize) -> HwpxResult<()> {
    if depth >= crate::decoder::section::MAX_NESTING_DEPTH {
        return Err(HwpxError::InvalidStructure {
            detail: format!(
                "visual-equation nesting depth {} exceeds limit of {}",
                depth,
                crate::decoder::section::MAX_NESTING_DEPTH
            ),
        });
    }
    Ok(())
}

fn nonempty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_string())
}

fn position_from_table(position: &HxTablePos) -> HwpxVisualEquationPosition {
    HwpxVisualEquationPosition {
        horz_offset: position.horz_offset,
        vert_offset: position.vert_offset,
    }
}

fn position_from_offset(position: &HxOffset) -> HwpxVisualEquationPosition {
    HwpxVisualEquationPosition { horz_offset: position.x, vert_offset: position.y }
}

fn size_from_original(size: &HxSizeAttr) -> HwpxVisualEquationSize {
    HwpxVisualEquationSize { width: size.width, height: size.height }
}

fn size_from_table(size: &HxTableSz) -> HwpxVisualEquationSize {
    HwpxVisualEquationSize { width: size.width, height: size.height }
}

fn equation_geometry(
    equation: &HxEquation,
    parent: VisualParent<'_>,
) -> HwpxVisualEquationGeometry {
    let raw_equation_size = equation.sz.as_ref().map(size_from_table);
    let raw_box_size = match parent.domain {
        HwpxVisualEquationDomain::PictureCaption => raw_equation_size,
        HwpxVisualEquationDomain::GroupDrawText => parent.raw_box_size,
    };
    let scale = HwpxVisualEquationScale {
        horz: parent.scale.horz.to_string(),
        vert: parent.scale.vert.to_string(),
    };
    let raw_base_unit = (equation.base_unit > 0).then_some(equation.base_unit);
    let render_base_unit = raw_base_unit.or_else(|| {
        raw_equation_size
            .and_then(|size| u32::try_from(size.height).ok().filter(|height| *height > 0))
    });
    HwpxVisualEquationGeometry {
        raw_box_size,
        raw_equation_size,
        raw_base_unit,
        render_base_unit,
        display_box_size: scale_size(raw_box_size, parent.scale),
        display_equation_size: scale_size(raw_equation_size, parent.scale),
        scale,
    }
}

fn scale_size(
    raw_size: Option<HwpxVisualEquationSize>,
    scale: VisualScale<'_>,
) -> Option<HwpxVisualEquationSize> {
    let raw_size = raw_size?;
    Some(HwpxVisualEquationSize {
        width: scale_dimension(raw_size.width, scale.horz)?,
        height: scale_dimension(raw_size.height, scale.vert)?,
    })
}

fn scale_dimension(raw: i32, scale: &str) -> Option<i32> {
    if raw <= 0 {
        return None;
    }
    let scale = scale.parse::<f64>().ok()?;
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let display = (f64::from(raw) * scale).round();
    if !display.is_finite() || display > f64::from(i32::MAX) {
        return None;
    }
    Some(display.max(1.0) as i32)
}

fn translate_position(
    raw: HwpxVisualEquationPosition,
    translation: VisualTranslation<'_>,
) -> Option<HwpxVisualEquationPosition> {
    Some(HwpxVisualEquationPosition {
        horz_offset: translate_coordinate(raw.horz_offset, translation.horz)?,
        vert_offset: translate_coordinate(raw.vert_offset, translation.vert)?,
    })
}

fn translate_coordinate(raw: i32, translation: &str) -> Option<i32> {
    let translation = translation.parse::<f64>().ok()?;
    if !translation.is_finite() {
        return None;
    }
    let display = (f64::from(raw) + translation).round();
    if !display.is_finite() || display < f64::from(i32::MIN) || display > f64::from(i32::MAX) {
        return None;
    }
    Some(display as i32)
}

fn equation_position(
    equation: &HxEquation,
    parent: VisualParent<'_>,
) -> HwpxResult<HwpxVisualEquationPosition> {
    let child_position = equation.pos.as_ref().map(position_from_table);
    if parent.compose_child_position {
        return add_positions(
            parent.position.unwrap_or_default(),
            child_position.unwrap_or_default(),
        );
    }
    match parent.domain {
        HwpxVisualEquationDomain::PictureCaption => {
            Ok(child_position.or(parent.position).unwrap_or_default())
        }
        HwpxVisualEquationDomain::GroupDrawText => {
            add_positions(parent.position.unwrap_or_default(), child_position.unwrap_or_default())
        }
    }
}

fn compose_scale(parent: &str, child: &str) -> HwpxResult<String> {
    let parent = parse_transform(parent)?;
    let child = parse_transform(child)?;
    Ok(format_transform(parent * child))
}

fn compose_translation(
    parent_translation: &str,
    parent_scale: &str,
    child_translation: &str,
) -> HwpxResult<String> {
    let parent_translation = parse_transform(parent_translation)?;
    let parent_scale = parse_transform(parent_scale)?;
    let child_translation = parse_transform(child_translation)?;
    Ok(format_transform(parent_translation + parent_scale * child_translation))
}

fn parse_transform(value: &str) -> HwpxResult<f64> {
    value.parse::<f64>().ok().filter(|value| value.is_finite()).ok_or_else(|| {
        HwpxError::InvalidStructure {
            detail: format!("visual-equation transform is not finite: {value}"),
        }
    })
}

fn format_transform(value: f64) -> String {
    if value == 0.0 {
        "0".to_string()
    } else {
        value.to_string()
    }
}

fn add_positions(
    parent: HwpxVisualEquationPosition,
    child: HwpxVisualEquationPosition,
) -> HwpxResult<HwpxVisualEquationPosition> {
    let horz_offset = parent.horz_offset.checked_add(child.horz_offset).ok_or_else(|| {
        HwpxError::InvalidStructure {
            detail: "visual-equation horizontal position exceeds i32 range".to_string(),
        }
    })?;
    let vert_offset = parent.vert_offset.checked_add(child.vert_offset).ok_or_else(|| {
        HwpxError::InvalidStructure {
            detail: "visual-equation vertical position exceeds i32 range".to_string(),
        }
    })?;
    Ok(HwpxVisualEquationPosition { horz_offset, vert_offset })
}

fn add_optional_positions(
    parent: Option<HwpxVisualEquationPosition>,
    child: Option<HwpxVisualEquationPosition>,
) -> HwpxResult<Option<HwpxVisualEquationPosition>> {
    match (parent, child) {
        (Some(parent), Some(child)) => add_positions(parent, child).map(Some),
        (Some(position), None) | (None, Some(position)) => Ok(Some(position)),
        (None, None) => Ok(None),
    }
}
