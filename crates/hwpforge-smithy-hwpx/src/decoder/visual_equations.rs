//! Structural reporting for equations embedded in visual HWPX objects.

use serde::Serialize;

use crate::schema::section::{
    HxContainer, HxCtrl, HxEquation, HxParagraph, HxPic, HxRect, HxRun, HxRunChildOrder, HxSubList,
    HxTable, HxTablePos,
};

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
        Self { schema_version: 1, equations: Vec::new() }
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
    /// Equation position, falling back to the nearest visual parent when absent.
    pub position: HwpxVisualEquationPosition,
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

#[derive(Clone, Copy)]
struct VisualParent<'a> {
    domain: HwpxVisualEquationDomain,
    kind: HwpxVisualEquationParentKind,
    object_id: &'a str,
    instance_id: &'a str,
    z_order: u32,
    position: Option<&'a HxTablePos>,
}

pub(crate) fn collect_section(
    paragraphs: &[HxParagraph],
    section_index: usize,
) -> Vec<HwpxVisualEquation> {
    let mut equations = Vec::new();
    let root = format!("section[{section_index}]");
    walk_visual_paragraphs(paragraphs, &root, &mut equations);
    equations
}

fn walk_visual_paragraphs(
    paragraphs: &[HxParagraph],
    prefix: &str,
    equations: &mut Vec<HwpxVisualEquation>,
) {
    for (paragraph_index, paragraph) in paragraphs.iter().enumerate() {
        let paragraph_path = format!("{prefix}/paragraph[{paragraph_index}]");
        for (run_index, run) in paragraph.runs.iter().enumerate() {
            let run_path = format!("{paragraph_path}/run[{run_index}]");
            walk_visual_run(run, &run_path, equations);
        }
    }
}

fn walk_visual_run(run: &HxRun, path: &str, equations: &mut Vec<HwpxVisualEquation>) {
    if run.child_order.is_empty() {
        walk_run_by_type(run, path, equations);
        return;
    }

    for child in &run.child_order {
        match *child {
            HxRunChildOrder::Table(index) => {
                walk_table(&run.tables[index], &format!("{path}/table[{index}]"), equations);
            }
            HxRunChildOrder::Picture(index) => {
                collect_picture(
                    &run.pictures[index],
                    &format!("{path}/picture[{index}]"),
                    equations,
                );
            }
            HxRunChildOrder::Ctrl(index) => {
                walk_ctrl(&run.ctrls[index], &format!("{path}/ctrl[{index}]"), equations);
            }
            HxRunChildOrder::Container(index) => {
                collect_container(
                    &run.containers[index],
                    &format!("{path}/container[{index}]"),
                    equations,
                );
            }
            _ => {}
        }
    }
}

fn walk_run_by_type(run: &HxRun, path: &str, equations: &mut Vec<HwpxVisualEquation>) {
    for (index, table) in run.tables.iter().enumerate() {
        walk_table(table, &format!("{path}/table[{index}]"), equations);
    }
    for (index, picture) in run.pictures.iter().enumerate() {
        collect_picture(picture, &format!("{path}/picture[{index}]"), equations);
    }
    for (index, ctrl) in run.ctrls.iter().enumerate() {
        walk_ctrl(ctrl, &format!("{path}/ctrl[{index}]"), equations);
    }
    for (index, container) in run.containers.iter().enumerate() {
        collect_container(container, &format!("{path}/container[{index}]"), equations);
    }
}

fn walk_table(table: &HxTable, path: &str, equations: &mut Vec<HwpxVisualEquation>) {
    for (row_index, row) in table.rows.iter().enumerate() {
        for (cell_index, cell) in row.cells.iter().enumerate() {
            if let Some(sub_list) = &cell.sub_list {
                walk_visual_paragraphs(
                    &sub_list.paragraphs,
                    &format!("{path}/row[{row_index}]/cell[{cell_index}]"),
                    equations,
                );
            }
        }
    }
}

fn walk_ctrl(ctrl: &HxCtrl, path: &str, equations: &mut Vec<HwpxVisualEquation>) {
    if let Some(footnote) = &ctrl.foot_note {
        walk_visual_paragraphs(
            &footnote.sub_list.paragraphs,
            &format!("{path}/footnote"),
            equations,
        );
    }
    if let Some(endnote) = &ctrl.end_note {
        walk_visual_paragraphs(&endnote.sub_list.paragraphs, &format!("{path}/endnote"), equations);
    }
}

fn collect_picture(pic: &HxPic, path: &str, equations: &mut Vec<HwpxVisualEquation>) {
    let Some(caption) = &pic.caption else { return };
    let parent = VisualParent {
        domain: HwpxVisualEquationDomain::PictureCaption,
        kind: HwpxVisualEquationParentKind::Picture,
        object_id: &pic.id,
        instance_id: &pic.instid,
        z_order: pic.z_order,
        position: pic.pos.as_ref(),
    };
    collect_parent_equations(&caption.sub_list, parent, path, equations);
}

fn collect_container(container: &HxContainer, path: &str, equations: &mut Vec<HwpxVisualEquation>) {
    for (rect_index, rect) in container.rects.iter().enumerate() {
        collect_group_rect(rect, &format!("{path}/rect[{rect_index}]"), equations);
    }
    for (container_index, nested) in container.containers.iter().enumerate() {
        collect_container(nested, &format!("{path}/container[{container_index}]"), equations);
    }
}

fn collect_group_rect(rect: &HxRect, path: &str, equations: &mut Vec<HwpxVisualEquation>) {
    let Some(draw_text) = &rect.draw_text else { return };
    let parent = VisualParent {
        domain: HwpxVisualEquationDomain::GroupDrawText,
        kind: HwpxVisualEquationParentKind::Container,
        object_id: &rect.id,
        instance_id: &rect.instid,
        z_order: rect.z_order,
        position: rect.pos.as_ref(),
    };
    collect_parent_equations(&draw_text.sub_list, parent, path, equations);
}

fn collect_parent_equations(
    sub_list: &HxSubList,
    parent: VisualParent<'_>,
    parent_path: &str,
    equations: &mut Vec<HwpxVisualEquation>,
) {
    let mut occurrences = Vec::new();
    collect_equations_from_paragraphs(&sub_list.paragraphs, &mut occurrences);
    for (parent_order, equation) in occurrences.into_iter().enumerate() {
        let equation_object_id = nonempty(&equation.id);
        let id = equation_object_id
            .clone()
            .unwrap_or_else(|| format!("{parent_path}/equation[{parent_order}]"));
        let position =
            equation.pos.as_ref().or(parent.position).map(position_from).unwrap_or_default();
        let z_order = if equation.z_order == 0 { parent.z_order } else { equation.z_order };
        equations.push(HwpxVisualEquation {
            id,
            domain: parent.domain,
            equation_object_id,
            parent_object_id: nonempty(parent.object_id),
            parent_instance_id: nonempty(parent.instance_id),
            parent_kind: parent.kind,
            parent_path: parent_path.to_string(),
            document_order: 0,
            parent_order,
            z_order,
            position,
            script: equation.script.as_ref().map(|script| script.text.clone()).unwrap_or_default(),
            latex: None,
        });
    }
}

fn collect_equations_from_paragraphs<'a>(
    paragraphs: &'a [HxParagraph],
    equations: &mut Vec<&'a HxEquation>,
) {
    for paragraph in paragraphs {
        for run in &paragraph.runs {
            if run.child_order.is_empty() {
                equations.extend(&run.equations);
            } else {
                for child in &run.child_order {
                    match *child {
                        HxRunChildOrder::Equation(index) => equations.push(&run.equations[index]),
                        HxRunChildOrder::Table(index) => {
                            collect_equations_from_table(&run.tables[index], equations);
                        }
                        HxRunChildOrder::Ctrl(index) => {
                            collect_equations_from_ctrl(&run.ctrls[index], equations);
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

fn collect_equations_from_table<'a>(table: &'a HxTable, equations: &mut Vec<&'a HxEquation>) {
    for row in &table.rows {
        for cell in &row.cells {
            if let Some(sub_list) = &cell.sub_list {
                collect_equations_from_paragraphs(&sub_list.paragraphs, equations);
            }
        }
    }
}

fn collect_equations_from_ctrl<'a>(ctrl: &'a HxCtrl, equations: &mut Vec<&'a HxEquation>) {
    if let Some(footnote) = &ctrl.foot_note {
        collect_equations_from_paragraphs(&footnote.sub_list.paragraphs, equations);
    }
    if let Some(endnote) = &ctrl.end_note {
        collect_equations_from_paragraphs(&endnote.sub_list.paragraphs, equations);
    }
}

fn nonempty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_string())
}

fn position_from(position: &HxTablePos) -> HwpxVisualEquationPosition {
    HwpxVisualEquationPosition {
        horz_offset: position.horz_offset,
        vert_offset: position.vert_offset,
    }
}
