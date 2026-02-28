//! Document validation logic (crate-private).
//!
//! This module contains the validation rules for transitioning a
//! `Document<Draft>` to `Document<Validated>`. It is not part of the
//! public API; it is called exclusively by `Document<Draft>::validate()`.

use crate::control::Control;
use crate::error::ValidationError;
use crate::run::RunContent;
use crate::section::Section;

/// Validates the document structure.
///
/// # Rules
///
/// 1. At least 1 section
/// 2. Every section has at least 1 paragraph
/// 3. Every paragraph has at least 1 run
/// 4. Every table has at least 1 row with at least 1 cell
/// 5. Every table cell has at least 1 paragraph
/// 6. `col_span >= 1`, `row_span >= 1`
/// 7. TextBox controls have at least 1 paragraph
/// 8. Footnote controls have at least 1 paragraph
///
/// Errors carry precise location context.
pub(crate) fn validate_sections(sections: &[Section]) -> Result<(), ValidationError> {
    if sections.is_empty() {
        return Err(ValidationError::EmptyDocument);
    }

    for (si, section) in sections.iter().enumerate() {
        if section.paragraphs.is_empty() {
            return Err(ValidationError::EmptySection { section_index: si });
        }

        for (pi, paragraph) in section.paragraphs.iter().enumerate() {
            if paragraph.runs.is_empty() {
                return Err(ValidationError::EmptyParagraph {
                    section_index: si,
                    paragraph_index: pi,
                });
            }

            for (ri, run) in paragraph.runs.iter().enumerate() {
                validate_run_content(&run.content, si, pi, ri)?;
            }
        }
    }

    Ok(())
}

/// Validates a single run's content recursively.
fn validate_run_content(
    content: &RunContent,
    si: usize,
    pi: usize,
    ri: usize,
) -> Result<(), ValidationError> {
    match content {
        RunContent::Table(table) => {
            if table.rows.is_empty() {
                return Err(ValidationError::EmptyTable {
                    section_index: si,
                    paragraph_index: pi,
                    run_index: ri,
                });
            }
            for (row_i, row) in table.rows.iter().enumerate() {
                if row.cells.is_empty() {
                    return Err(ValidationError::EmptyTableRow {
                        section_index: si,
                        paragraph_index: pi,
                        run_index: ri,
                        row_index: row_i,
                    });
                }
                for (cell_i, cell) in row.cells.iter().enumerate() {
                    if cell.col_span == 0 {
                        return Err(ValidationError::InvalidSpan {
                            field: "col_span",
                            value: 0,
                            section_index: si,
                            paragraph_index: pi,
                            run_index: ri,
                            row_index: row_i,
                            cell_index: cell_i,
                        });
                    }
                    if cell.row_span == 0 {
                        return Err(ValidationError::InvalidSpan {
                            field: "row_span",
                            value: 0,
                            section_index: si,
                            paragraph_index: pi,
                            run_index: ri,
                            row_index: row_i,
                            cell_index: cell_i,
                        });
                    }
                    if cell.paragraphs.is_empty() {
                        return Err(ValidationError::EmptyTableCell {
                            section_index: si,
                            paragraph_index: pi,
                            run_index: ri,
                            row_index: row_i,
                            cell_index: cell_i,
                        });
                    }
                }
            }
        }
        RunContent::Control(control) => match control.as_ref() {
            Control::TextBox { paragraphs, .. } => {
                if paragraphs.is_empty() {
                    return Err(ValidationError::EmptyTextBox {
                        section_index: si,
                        paragraph_index: pi,
                        run_index: ri,
                    });
                }
            }
            Control::Footnote { paragraphs, .. } => {
                if paragraphs.is_empty() {
                    return Err(ValidationError::EmptyFootnote {
                        section_index: si,
                        paragraph_index: pi,
                        run_index: ri,
                    });
                }
            }
            Control::Endnote { paragraphs, .. } => {
                if paragraphs.is_empty() {
                    return Err(ValidationError::EmptyEndnote {
                        section_index: si,
                        paragraph_index: pi,
                        run_index: ri,
                    });
                }
            }
            Control::Line { .. } => {
                // No structural validation needed for lines
                // Lines can have zero height (horizontal) or zero width (vertical)
            }
            Control::Ellipse { width, height, .. } => {
                // Ellipse must have non-zero dimensions
                if width.as_i32() == 0 || height.as_i32() == 0 {
                    return Err(ValidationError::InvalidShapeDimension {
                        section_index: si,
                        paragraph_index: pi,
                        run_index: ri,
                        shape_type: "Ellipse",
                    });
                }
            }
            Control::Polygon { vertices, width, height, .. } => {
                // Polygon must have at least 3 vertices
                if vertices.len() < 3 {
                    return Err(ValidationError::InvalidPolygon {
                        section_index: si,
                        paragraph_index: pi,
                        run_index: ri,
                        vertex_count: vertices.len(),
                    });
                }
                // Polygon must have non-zero dimensions
                if width.as_i32() == 0 || height.as_i32() == 0 {
                    return Err(ValidationError::InvalidShapeDimension {
                        section_index: si,
                        paragraph_index: pi,
                        run_index: ri,
                        shape_type: "Polygon",
                    });
                }
            }
            Control::Chart { data, width, height, .. } => {
                // Chart must have non-empty data
                if data.has_no_series() {
                    return Err(ValidationError::EmptyChartData {
                        section_index: si,
                        paragraph_index: pi,
                        run_index: ri,
                    });
                }
                // Category charts must have category labels
                if let crate::chart::ChartData::Category { categories, .. } = data {
                    if categories.is_empty() {
                        return Err(ValidationError::EmptyCategoryLabels {
                            section_index: si,
                            paragraph_index: pi,
                            run_index: ri,
                        });
                    }
                }
                // XY series must have matching x/y lengths
                if let crate::chart::ChartData::Xy { series } = data {
                    for s in series {
                        if s.x_values.len() != s.y_values.len() {
                            return Err(ValidationError::MismatchedSeriesLengths {
                                section_index: si,
                                paragraph_index: pi,
                                run_index: ri,
                                series_name: s.name.clone(),
                                x_len: s.x_values.len(),
                                y_len: s.y_values.len(),
                            });
                        }
                    }
                }
                // Chart must have non-zero dimensions
                if width.as_i32() == 0 || height.as_i32() == 0 {
                    return Err(ValidationError::InvalidShapeDimension {
                        section_index: si,
                        paragraph_index: pi,
                        run_index: ri,
                        shape_type: "Chart",
                    });
                }
            }
            Control::Equation { script, width, height, .. } => {
                // Equation must have non-empty script
                if script.is_empty() {
                    return Err(ValidationError::EmptyEquation {
                        section_index: si,
                        paragraph_index: pi,
                        run_index: ri,
                    });
                }
                // Equation must have non-zero dimensions
                if width.as_i32() == 0 || height.as_i32() == 0 {
                    return Err(ValidationError::InvalidShapeDimension {
                        section_index: si,
                        paragraph_index: pi,
                        run_index: ri,
                        shape_type: "Equation",
                    });
                }
            }
            Control::Hyperlink { .. } | Control::Unknown { .. } => {
                // No structural validation needed for these variants
            }
        },
        RunContent::Text(_) | RunContent::Image(_) => {
            // No structural validation needed
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page::PageSettings;
    use crate::paragraph::Paragraph;
    use crate::run::Run;
    use crate::section::Section;
    use crate::table::{Table, TableCell, TableRow};
    use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};

    fn text_run(s: &str) -> Run {
        Run::text(s, CharShapeIndex::new(0))
    }

    fn simple_paragraph() -> Paragraph {
        Paragraph::with_runs(vec![text_run("text")], ParaShapeIndex::new(0))
    }

    fn simple_section() -> Section {
        Section::with_paragraphs(vec![simple_paragraph()], PageSettings::a4())
    }

    fn simple_cell() -> TableCell {
        TableCell::new(vec![simple_paragraph()], HwpUnit::from_mm(50.0).unwrap())
    }

    // === Rule 1: At least 1 section ===

    #[test]
    fn empty_sections_rejected() {
        let result = validate_sections(&[]);
        assert_eq!(result, Err(ValidationError::EmptyDocument));
    }

    #[test]
    fn one_section_accepted() {
        let result = validate_sections(&[simple_section()]);
        assert!(result.is_ok());
    }

    // === Rule 2: Every section has at least 1 paragraph ===

    #[test]
    fn empty_section_rejected() {
        let sections = vec![Section::new(PageSettings::a4())];
        let result = validate_sections(&sections);
        assert_eq!(result, Err(ValidationError::EmptySection { section_index: 0 }));
    }

    #[test]
    fn second_empty_section_reports_index_1() {
        let sections = vec![simple_section(), Section::new(PageSettings::a4())];
        let result = validate_sections(&sections);
        assert_eq!(result, Err(ValidationError::EmptySection { section_index: 1 }));
    }

    // === Rule 3: Every paragraph has at least 1 run ===

    #[test]
    fn empty_paragraph_rejected() {
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::new(ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        let result = validate_sections(&sections);
        assert_eq!(
            result,
            Err(ValidationError::EmptyParagraph { section_index: 0, paragraph_index: 0 })
        );
    }

    #[test]
    fn second_empty_paragraph_reports_correct_index() {
        let sections = vec![Section::with_paragraphs(
            vec![simple_paragraph(), Paragraph::new(ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        let result = validate_sections(&sections);
        assert_eq!(
            result,
            Err(ValidationError::EmptyParagraph { section_index: 0, paragraph_index: 1 })
        );
    }

    // === Rule 4: Tables have at least 1 row ===

    #[test]
    fn empty_table_rejected() {
        let table_run = Run::table(Table::new(vec![]), CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![table_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        let result = validate_sections(&sections);
        assert_eq!(
            result,
            Err(ValidationError::EmptyTable { section_index: 0, paragraph_index: 0, run_index: 0 })
        );
    }

    // === Rule 4b: Table rows have at least 1 cell ===

    #[test]
    fn empty_table_row_rejected() {
        let table = Table::new(vec![TableRow { cells: vec![], height: None }]);
        let table_run = Run::table(table, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![table_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        let result = validate_sections(&sections);
        assert_eq!(
            result,
            Err(ValidationError::EmptyTableRow {
                section_index: 0,
                paragraph_index: 0,
                run_index: 0,
                row_index: 0,
            })
        );
    }

    // === Rule 5: Table cells have at least 1 paragraph ===

    #[test]
    fn empty_table_cell_rejected() {
        let cell = TableCell::new(vec![], HwpUnit::from_mm(50.0).unwrap());
        let table = Table::new(vec![TableRow { cells: vec![cell], height: None }]);
        let table_run = Run::table(table, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![table_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        let result = validate_sections(&sections);
        assert_eq!(
            result,
            Err(ValidationError::EmptyTableCell {
                section_index: 0,
                paragraph_index: 0,
                run_index: 0,
                row_index: 0,
                cell_index: 0,
            })
        );
    }

    // === Rule 6: Spans >= 1 ===

    #[test]
    fn zero_col_span_rejected() {
        let cell = TableCell::with_span(
            vec![simple_paragraph()],
            HwpUnit::from_mm(50.0).unwrap(),
            0, // invalid
            1,
        );
        let table = Table::new(vec![TableRow { cells: vec![cell], height: None }]);
        let table_run = Run::table(table, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![table_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        let result = validate_sections(&sections);
        assert!(matches!(result, Err(ValidationError::InvalidSpan { field: "col_span", .. })));
    }

    #[test]
    fn zero_row_span_rejected() {
        let cell = TableCell::with_span(
            vec![simple_paragraph()],
            HwpUnit::from_mm(50.0).unwrap(),
            1,
            0, // invalid
        );
        let table = Table::new(vec![TableRow { cells: vec![cell], height: None }]);
        let table_run = Run::table(table, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![table_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        let result = validate_sections(&sections);
        assert!(matches!(result, Err(ValidationError::InvalidSpan { field: "row_span", .. })));
    }

    // === Rule 7: TextBox has paragraphs ===

    #[test]
    fn empty_text_box_rejected() {
        let ctrl = Control::TextBox {
            paragraphs: vec![],
            width: HwpUnit::from_mm(80.0).unwrap(),
            height: HwpUnit::from_mm(40.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        let result = validate_sections(&sections);
        assert!(matches!(result, Err(ValidationError::EmptyTextBox { .. })));
    }

    // === Rule 8: Footnote has paragraphs ===

    #[test]
    fn empty_footnote_rejected() {
        let ctrl = Control::Footnote { inst_id: None, paragraphs: vec![] };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        let result = validate_sections(&sections);
        assert!(matches!(result, Err(ValidationError::EmptyFootnote { .. })));
    }

    // === Happy paths ===

    #[test]
    fn valid_table_accepted() {
        let table = Table::new(vec![TableRow { cells: vec![simple_cell()], height: None }]);
        let table_run = Run::table(table, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![table_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        assert!(validate_sections(&sections).is_ok());
    }

    #[test]
    fn valid_text_box_accepted() {
        let ctrl = Control::TextBox {
            paragraphs: vec![simple_paragraph()],
            width: HwpUnit::from_mm(80.0).unwrap(),
            height: HwpUnit::from_mm(40.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        assert!(validate_sections(&sections).is_ok());
    }

    #[test]
    fn valid_footnote_accepted() {
        let ctrl = Control::Footnote { inst_id: None, paragraphs: vec![simple_paragraph()] };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        assert!(validate_sections(&sections).is_ok());
    }

    #[test]
    fn hyperlink_always_valid() {
        let ctrl =
            Control::Hyperlink { text: "link".to_string(), url: "https://example.com".to_string() };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        assert!(validate_sections(&sections).is_ok());
    }

    #[test]
    fn unknown_control_always_valid() {
        let ctrl = Control::Unknown { tag: "x".to_string(), data: None };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        assert!(validate_sections(&sections).is_ok());
    }

    #[test]
    fn multiple_valid_sections() {
        let sections = vec![simple_section(), simple_section(), simple_section()];
        assert!(validate_sections(&sections).is_ok());
    }

    #[test]
    fn large_span_values_accepted() {
        let cell = TableCell::with_span(
            vec![simple_paragraph()],
            HwpUnit::from_mm(50.0).unwrap(),
            100, // large but valid
            50,
        );
        let table = Table::new(vec![TableRow { cells: vec![cell], height: None }]);
        let table_run = Run::table(table, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![table_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        assert!(validate_sections(&sections).is_ok());
    }

    // === Rule 9: Endnote has paragraphs ===

    #[test]
    fn empty_endnote_rejected() {
        let ctrl = Control::Endnote { inst_id: None, paragraphs: vec![] };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        let result = validate_sections(&sections);
        assert!(matches!(result, Err(ValidationError::EmptyEndnote { .. })));
    }

    #[test]
    fn valid_endnote_accepted() {
        let ctrl = Control::Endnote { inst_id: Some(999), paragraphs: vec![simple_paragraph()] };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        assert!(validate_sections(&sections).is_ok());
    }

    // === Rule 10: Polygon has at least 3 vertices ===

    #[test]
    fn polygon_zero_vertices_rejected() {
        let ctrl = Control::Polygon {
            vertices: vec![],
            width: HwpUnit::from_mm(50.0).unwrap(),
            height: HwpUnit::from_mm(50.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![],
            caption: None,
            style: None,
        };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        let result = validate_sections(&sections);
        assert!(matches!(result, Err(ValidationError::InvalidPolygon { vertex_count: 0, .. })));
    }

    #[test]
    fn polygon_two_vertices_rejected() {
        use crate::control::ShapePoint;
        let ctrl = Control::Polygon {
            vertices: vec![ShapePoint { x: 0, y: 0 }, ShapePoint { x: 100, y: 100 }],
            width: HwpUnit::from_mm(50.0).unwrap(),
            height: HwpUnit::from_mm(50.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![],
            caption: None,
            style: None,
        };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        let result = validate_sections(&sections);
        assert!(matches!(result, Err(ValidationError::InvalidPolygon { vertex_count: 2, .. })));
    }

    #[test]
    fn polygon_three_vertices_accepted() {
        use crate::control::ShapePoint;
        let ctrl = Control::Polygon {
            vertices: vec![
                ShapePoint { x: 0, y: 0 },
                ShapePoint { x: 1000, y: 0 },
                ShapePoint { x: 500, y: 1000 },
            ],
            width: HwpUnit::from_mm(50.0).unwrap(),
            height: HwpUnit::from_mm(50.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![],
            caption: None,
            style: None,
        };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        assert!(validate_sections(&sections).is_ok());
    }

    // === Rule 11: Shape dimensions (Ellipse and Polygon only) ===

    #[test]
    fn ellipse_zero_width_rejected() {
        use crate::control::ShapePoint;
        let ctrl = Control::Ellipse {
            center: ShapePoint { x: 500, y: 500 },
            axis1: ShapePoint { x: 1000, y: 500 },
            axis2: ShapePoint { x: 500, y: 1000 },
            width: HwpUnit::new(0).unwrap(), // invalid
            height: HwpUnit::from_mm(30.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![],
            caption: None,
            style: None,
        };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        let result = validate_sections(&sections);
        assert!(matches!(
            result,
            Err(ValidationError::InvalidShapeDimension { shape_type: "Ellipse", .. })
        ));
    }

    #[test]
    fn ellipse_zero_height_rejected() {
        use crate::control::ShapePoint;
        let ctrl = Control::Ellipse {
            center: ShapePoint { x: 500, y: 500 },
            axis1: ShapePoint { x: 1000, y: 500 },
            axis2: ShapePoint { x: 500, y: 1000 },
            width: HwpUnit::from_mm(40.0).unwrap(),
            height: HwpUnit::new(0).unwrap(), // invalid
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![],
            caption: None,
            style: None,
        };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        let result = validate_sections(&sections);
        assert!(matches!(
            result,
            Err(ValidationError::InvalidShapeDimension { shape_type: "Ellipse", .. })
        ));
    }

    #[test]
    fn polygon_zero_width_rejected() {
        use crate::control::ShapePoint;
        let ctrl = Control::Polygon {
            vertices: vec![
                ShapePoint { x: 0, y: 0 },
                ShapePoint { x: 1000, y: 0 },
                ShapePoint { x: 500, y: 1000 },
            ],
            width: HwpUnit::new(0).unwrap(), // invalid
            height: HwpUnit::from_mm(50.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![],
            caption: None,
            style: None,
        };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        let result = validate_sections(&sections);
        assert!(matches!(
            result,
            Err(ValidationError::InvalidShapeDimension { shape_type: "Polygon", .. })
        ));
    }

    #[test]
    fn line_zero_height_accepted() {
        use crate::control::ShapePoint;
        // Lines can have zero height (horizontal line)
        let ctrl = Control::Line {
            start: ShapePoint { x: 0, y: 0 },
            end: ShapePoint { x: 1000, y: 0 },
            width: HwpUnit::from_mm(50.0).unwrap(),
            height: HwpUnit::new(0).unwrap(), // valid for lines
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        assert!(validate_sections(&sections).is_ok());
    }

    #[test]
    fn line_zero_width_accepted() {
        use crate::control::ShapePoint;
        // Lines can have zero width (vertical line)
        let ctrl = Control::Line {
            start: ShapePoint { x: 0, y: 0 },
            end: ShapePoint { x: 0, y: 1000 },
            width: HwpUnit::new(0).unwrap(), // valid for lines
            height: HwpUnit::from_mm(50.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        assert!(validate_sections(&sections).is_ok());
    }

    #[test]
    fn valid_line_accepted() {
        use crate::control::ShapePoint;
        let ctrl = Control::Line {
            start: ShapePoint { x: 0, y: 0 },
            end: ShapePoint { x: 1000, y: 500 },
            width: HwpUnit::from_mm(50.0).unwrap(),
            height: HwpUnit::from_mm(25.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        assert!(validate_sections(&sections).is_ok());
    }

    #[test]
    fn valid_ellipse_accepted() {
        use crate::control::ShapePoint;
        let ctrl = Control::Ellipse {
            center: ShapePoint { x: 500, y: 500 },
            axis1: ShapePoint { x: 1000, y: 500 },
            axis2: ShapePoint { x: 500, y: 1000 },
            width: HwpUnit::from_mm(40.0).unwrap(),
            height: HwpUnit::from_mm(30.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![],
            caption: None,
            style: None,
        };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        assert!(validate_sections(&sections).is_ok());
    }

    #[test]
    fn valid_polygon_accepted() {
        use crate::control::ShapePoint;
        let ctrl = Control::Polygon {
            vertices: vec![
                ShapePoint { x: 0, y: 0 },
                ShapePoint { x: 1000, y: 0 },
                ShapePoint { x: 500, y: 1000 },
            ],
            width: HwpUnit::from_mm(50.0).unwrap(),
            height: HwpUnit::from_mm(50.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![],
            caption: None,
            style: None,
        };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        assert!(validate_sections(&sections).is_ok());
    }

    // === Chart validation ===

    fn chart_ctrl(data: crate::chart::ChartData) -> Control {
        Control::Chart {
            chart_type: crate::chart::ChartType::Column,
            data,
            title: None,
            legend: crate::chart::LegendPosition::default(),
            grouping: crate::chart::ChartGrouping::default(),
            width: HwpUnit::from_mm(100.0).unwrap(),
            height: HwpUnit::from_mm(80.0).unwrap(),
            stock_variant: None,
            bar_shape: None,
            scatter_style: None,
            radar_style: None,
            of_pie_type: None,
            explosion: None,
            wireframe: None,
            bubble_3d: None,
            show_markers: None,
        }
    }

    fn wrap_ctrl(ctrl: Control) -> Vec<Section> {
        vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        )]
    }

    #[test]
    fn chart_with_empty_data_rejected() {
        let data = crate::chart::ChartData::category(&["A"], &[]);
        let sections = wrap_ctrl(chart_ctrl(data));
        let result = validate_sections(&sections);
        assert!(matches!(result, Err(ValidationError::EmptyChartData { .. })));
    }

    #[test]
    fn chart_zero_width_rejected() {
        let data = crate::chart::ChartData::category(&["A"], &[("S", &[1.0])]);
        let ctrl = Control::Chart {
            chart_type: crate::chart::ChartType::Column,
            data,
            title: None,
            legend: crate::chart::LegendPosition::default(),
            grouping: crate::chart::ChartGrouping::default(),
            width: HwpUnit::new(0).unwrap(),
            height: HwpUnit::from_mm(80.0).unwrap(),
            stock_variant: None,
            bar_shape: None,
            scatter_style: None,
            radar_style: None,
            of_pie_type: None,
            explosion: None,
            wireframe: None,
            bubble_3d: None,
            show_markers: None,
        };
        let sections = wrap_ctrl(ctrl);
        let result = validate_sections(&sections);
        assert!(matches!(
            result,
            Err(ValidationError::InvalidShapeDimension { shape_type: "Chart", .. })
        ));
    }

    #[test]
    fn valid_chart_accepted() {
        let data = crate::chart::ChartData::category(&["A", "B"], &[("Sales", &[10.0, 20.0])]);
        let sections = wrap_ctrl(chart_ctrl(data));
        assert!(validate_sections(&sections).is_ok());
    }

    #[test]
    fn chart_empty_categories_rejected() {
        let data = crate::chart::ChartData::category(&[], &[("S", &[])]);
        let sections = wrap_ctrl(chart_ctrl(data));
        let result = validate_sections(&sections);
        assert!(matches!(result, Err(ValidationError::EmptyCategoryLabels { .. })));
    }

    #[test]
    fn chart_xy_mismatched_lengths_rejected() {
        let data = crate::chart::ChartData::Xy {
            series: vec![crate::chart::XySeries {
                name: "Points".to_string(),
                x_values: vec![1.0, 2.0, 3.0],
                y_values: vec![10.0, 20.0], // mismatched!
            }],
        };
        let ctrl = Control::Chart {
            chart_type: crate::chart::ChartType::Scatter,
            data,
            title: None,
            legend: crate::chart::LegendPosition::default(),
            grouping: crate::chart::ChartGrouping::default(),
            width: HwpUnit::from_mm(100.0).unwrap(),
            height: HwpUnit::from_mm(80.0).unwrap(),
            stock_variant: None,
            bar_shape: None,
            scatter_style: None,
            radar_style: None,
            of_pie_type: None,
            explosion: None,
            wireframe: None,
            bubble_3d: None,
            show_markers: None,
        };
        let sections = wrap_ctrl(ctrl);
        let result = validate_sections(&sections);
        assert!(matches!(result, Err(ValidationError::MismatchedSeriesLengths { .. })));
    }

    #[test]
    fn chart_xy_matching_lengths_accepted() {
        let data = crate::chart::ChartData::xy(&[("Pts", &[1.0, 2.0], &[3.0, 4.0])]);
        let ctrl = Control::Chart {
            chart_type: crate::chart::ChartType::Scatter,
            data,
            title: None,
            legend: crate::chart::LegendPosition::default(),
            grouping: crate::chart::ChartGrouping::default(),
            width: HwpUnit::from_mm(100.0).unwrap(),
            height: HwpUnit::from_mm(80.0).unwrap(),
            stock_variant: None,
            bar_shape: None,
            scatter_style: None,
            radar_style: None,
            of_pie_type: None,
            explosion: None,
            wireframe: None,
            bubble_3d: None,
            show_markers: None,
        };
        let sections = wrap_ctrl(ctrl);
        assert!(validate_sections(&sections).is_ok());
    }

    // === Equation validation ===

    #[test]
    fn equation_empty_script_rejected() {
        let ctrl = Control::Equation {
            script: String::new(),
            width: HwpUnit::from_mm(50.0).unwrap(),
            height: HwpUnit::from_mm(20.0).unwrap(),
            base_line: 71,
            text_color: hwpforge_foundation::Color::BLACK,
            font: "HancomEQN".to_string(),
        };
        let sections = wrap_ctrl(ctrl);
        let result = validate_sections(&sections);
        assert!(matches!(result, Err(ValidationError::EmptyEquation { .. })));
    }

    #[test]
    fn equation_zero_height_rejected() {
        let ctrl = Control::Equation {
            script: "x^2".to_string(),
            width: HwpUnit::from_mm(50.0).unwrap(),
            height: HwpUnit::new(0).unwrap(),
            base_line: 71,
            text_color: hwpforge_foundation::Color::BLACK,
            font: "HancomEQN".to_string(),
        };
        let sections = wrap_ctrl(ctrl);
        let result = validate_sections(&sections);
        assert!(matches!(
            result,
            Err(ValidationError::InvalidShapeDimension { shape_type: "Equation", .. })
        ));
    }

    #[test]
    fn equation_zero_width_rejected() {
        let ctrl = Control::Equation {
            script: "x^2".to_string(),
            width: HwpUnit::new(0).unwrap(),
            height: HwpUnit::from_mm(20.0).unwrap(),
            base_line: 71,
            text_color: hwpforge_foundation::Color::BLACK,
            font: "HancomEQN".to_string(),
        };
        let sections = wrap_ctrl(ctrl);
        let result = validate_sections(&sections);
        assert!(matches!(
            result,
            Err(ValidationError::InvalidShapeDimension { shape_type: "Equation", .. })
        ));
    }

    #[test]
    fn valid_equation_accepted() {
        let ctrl = Control::equation("{a+b} over {c+d}");
        let sections = wrap_ctrl(ctrl);
        assert!(validate_sections(&sections).is_ok());
    }
}
