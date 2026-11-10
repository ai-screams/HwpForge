//! Observed table shapes eligible for height recalculation.
use super::{attr, exact, invalid, require, HwpxResult, Part};

type AttributeGroups = &'static [Attributes];
type Attributes = &'static [(&'static str, &'static str)];
struct TableProfile {
    table: AttributeGroups,
    position: AttributeGroups,
    size: Attributes,
    sublist: Attributes,
    cells: CellRules,
}
enum CellRules {
    // Keep the established variable-size flow profile, including its one fixed cell.
    DefaultFlow,
    Fixed { rows: usize, columns: usize, height: &'static str, layout: &'static [Cell] },
}
struct Cell {
    row: usize,
    column: usize,
    row_span: usize,
    column_span: usize,
    width: &'static str,
    alignment: &'static str,
}
const SIMPLE: &[Cell] = &[
    Cell { row: 0, column: 0, row_span: 1, column_span: 1, width: "16063", alignment: "CENTER" },
    Cell { row: 0, column: 1, row_span: 1, column_span: 1, width: "16063", alignment: "CENTER" },
    Cell { row: 0, column: 2, row_span: 1, column_span: 1, width: "16063", alignment: "BOTTOM" },
    Cell { row: 1, column: 0, row_span: 1, column_span: 1, width: "16063", alignment: "CENTER" },
    Cell { row: 1, column: 1, row_span: 1, column_span: 1, width: "16063", alignment: "CENTER" },
    Cell { row: 1, column: 2, row_span: 1, column_span: 1, width: "16063", alignment: "CENTER" },
];
const MERGED: &[Cell] = &[
    Cell { row: 0, column: 0, row_span: 2, column_span: 2, width: "32126", alignment: "CENTER" },
    Cell { row: 0, column: 2, row_span: 1, column_span: 1, width: "16063", alignment: "CENTER" },
    Cell { row: 1, column: 2, row_span: 1, column_span: 1, width: "16063", alignment: "CENTER" },
    Cell { row: 2, column: 0, row_span: 1, column_span: 2, width: "32126", alignment: "CENTER" },
    Cell { row: 2, column: 2, row_span: 1, column_span: 1, width: "16063", alignment: "CENTER" },
];
const TABLE: Attributes = &[
    ("pageBreak", "CELL"),
    ("repeatHeader", "1"),
    ("textWrap", "TOP_AND_BOTTOM"),
    ("textFlow", "BOTH_SIDES"),
];
const POSITION: Attributes = &[
    ("affectLSpacing", "0"),
    ("flowWithText", "1"),
    ("allowOverlap", "0"),
    ("holdAnchorAndSO", "0"),
    ("vertRelTo", "PARA"),
    ("vertAlign", "TOP"),
    ("horzAlign", "LEFT"),
    ("vertOffset", "0"),
    ("horzOffset", "0"),
];
const SUBLIST: Attributes = &[
    ("textDirection", "HORIZONTAL"),
    ("lineWrap", "BREAK"),
    ("textWidth", "0"),
    ("textHeight", "0"),
    ("hasTextRef", "0"),
    ("hasNumRef", "0"),
    ("linkListIDRef", "0"),
    ("linkListNextIDRef", "0"),
];
const INNER_MARGIN: Attributes =
    &[("left", "510"), ("right", "510"), ("top", "141"), ("bottom", "141")];
const OUTER_MARGIN: Attributes =
    &[("left", "283"), ("right", "283"), ("top", "283"), ("bottom", "1000")];
const INLINE_TABLE: Attributes =
    &[("numberingType", "TABLE"), ("lock", "0"), ("dropcapstyle", "None"), ("cellSpacing", "0")];
const INLINE_SIZE: Attributes = &[
    ("heightRelTo", "ABSOLUTE"),
    ("width", "48189"),
    ("widthRelTo", "ABSOLUTE"),
    ("protect", "0"),
];
const CELL: Attributes =
    &[("header", "0"), ("hasMargin", "0"), ("protect", "0"), ("editable", "0"), ("dirty", "0")];
// Each row is a complete profile. Axes from different rows cannot be combined.
const TABLE_PROFILES: &[TableProfile] = &[
    TableProfile {
        table: &[TABLE],
        position: &[POSITION, &[("treatAsChar", "0"), ("horzRelTo", "COLUMN")]],
        size: &[("heightRelTo", "ABSOLUTE")],
        sublist: SUBLIST,
        cells: CellRules::DefaultFlow,
    },
    TableProfile {
        table: &[TABLE, INLINE_TABLE, &[("noAdjust", "0"), ("zOrder", "0")]],
        position: &[POSITION, &[("treatAsChar", "1"), ("horzRelTo", "COLUMN")]],
        size: INLINE_SIZE,
        sublist: SUBLIST,
        cells: CellRules::Fixed { rows: 2, columns: 3, height: "0", layout: SIMPLE },
    },
    TableProfile {
        table: &[TABLE, INLINE_TABLE, &[("noAdjust", "0"), ("zOrder", "0")]],
        position: &[POSITION, &[("treatAsChar", "1"), ("horzRelTo", "PARA")]],
        size: INLINE_SIZE,
        sublist: SUBLIST,
        cells: CellRules::Fixed { rows: 3, columns: 3, height: "4688", layout: MERGED },
    },
    TableProfile {
        table: &[TABLE, INLINE_TABLE, &[("noAdjust", "1"), ("zOrder", "0")]],
        position: &[POSITION, &[("treatAsChar", "1"), ("horzRelTo", "PARA")]],
        size: INLINE_SIZE,
        sublist: SUBLIST,
        cells: CellRules::Fixed { rows: 3, columns: 3, height: "2148", layout: MERGED },
    },
];

pub(super) fn matches(part: &Part<'_>, table: usize) -> bool {
    TABLE_PROFILES.iter().any(|profile| profile.check(part, table).is_ok())
        || check_inline_category(part, table).is_ok()
}

impl TableProfile {
    fn check(&self, part: &Part<'_>, table: usize) -> HwpxResult<()> {
        let tags = &part.tags;
        for attributes in self.table {
            exact(&tags[table], attributes)?;
        }
        let pos = part.one_child(table, "pos")?;
        for attributes in self.position {
            exact(&tags[pos], attributes)?;
        }
        let size = part.one_child(table, "sz")?;
        exact(&tags[size], self.size)?;
        let elements = part.source.elements();
        let descendants = || {
            elements
                .iter()
                .enumerate()
                .skip(table + 1)
                .take_while(|(_, e)| e.outer().start < elements[table].outer().end)
        };
        match self.cells {
            CellRules::DefaultFlow => {
                let no_adjust = attr(&tags[table], "noAdjust")?;
                require(matches!(no_adjust, "0" | "1"))?;
                let mut cells = 0;
                let mut fixed = 0;
                for (id, element) in descendants() {
                    crate::work::tick();
                    match element.name() {
                        "subList" => exact(&tags[id], self.sublist)?,
                        "tc" => {
                            let span = part.one_child(id, "cellSpan")?;
                            exact(&tags[span], &[("rowSpan", "1"), ("colSpan", "1")])?;
                            let size = part.one_child(id, "cellSz")?;
                            let height = attr(&tags[size], "height")?
                                .parse::<u32>()
                                .map_err(|_| invalid())?;
                            require(height == 0 || height == 2400 && no_adjust == "1")?;
                            fixed += usize::from(height == 2400);
                            cells += 1;
                            part.one_child(id, "subList")?;
                        }
                        _ => {}
                    }
                }
                require(cells > 0 && fixed <= 1)
            }
            CellRules::Fixed { rows, columns, height, layout } => {
                require(elements[table].children().iter().all(|&child| {
                    matches!(
                        tags[child].name.as_str(),
                        "sz" | "pos" | "outMargin" | "inMargin" | "tr"
                    )
                }))?;
                require(attr(&tags[table], "rowCnt")? == rows.to_string())?;
                require(attr(&tags[table], "colCnt")? == columns.to_string())?;
                exact(&tags[part.one_child(table, "outMargin")?], OUTER_MARGIN)?;
                exact(&tags[part.one_child(table, "inMargin")?], INNER_MARGIN)?;
                let row_ids: Vec<_> = elements[table]
                    .children()
                    .iter()
                    .copied()
                    .filter(|&id| elements[id].name() == "tr")
                    .collect();
                require(row_ids.len() == rows)?;
                let mut cell_index = 0;
                for (row_index, row) in row_ids.into_iter().enumerate() {
                    for &cell in elements[row].children() {
                        crate::work::tick();
                        require(tags[cell].name == "tc")?;
                        let expected = layout.get(cell_index).ok_or_else(invalid)?;
                        require(row_index == expected.row)?;
                        require(elements[cell].children().iter().all(|&child| {
                            matches!(
                                tags[child].name.as_str(),
                                "subList" | "cellAddr" | "cellSpan" | "cellSz" | "cellMargin"
                            )
                        }))?;
                        exact(&tags[cell], CELL)?;
                        let address = part.one_child(cell, "cellAddr")?;
                        require(attr(&tags[address], "rowAddr")? == expected.row.to_string())?;
                        require(attr(&tags[address], "colAddr")? == expected.column.to_string())?;
                        let span = part.one_child(cell, "cellSpan")?;
                        require(attr(&tags[span], "rowSpan")? == expected.row_span.to_string())?;
                        require(attr(&tags[span], "colSpan")? == expected.column_span.to_string())?;
                        let size = part.one_child(cell, "cellSz")?;
                        exact(&tags[size], &[("width", expected.width), ("height", height)])?;
                        exact(&tags[part.one_child(cell, "cellMargin")?], INNER_MARGIN)?;
                        let list = part.one_child(cell, "subList")?;
                        exact(&tags[list], self.sublist)?;
                        exact(&tags[list], &[("vertAlign", expected.alignment)])?;
                        cell_index += 1;
                    }
                }
                require(cell_index == layout.len())?;
                // Reject hidden extra cells/lists, including nested table descendants.
                require(descendants().filter(|(_, e)| e.name() == "tc").count() == cell_index)?;
                require(descendants().filter(|(_, e)| e.name() == "subList").count() == cell_index)
            }
        }
    }
}

fn number(part: &Part<'_>, id: usize, key: &str, low: u32, high: u32) -> HwpxResult<u32> {
    let raw = attr(&part.tags[id], key)?;
    require(raw.bytes().all(|b| b.is_ascii_digit()))?;
    require(raw == "0" || !raw.starts_with('0'))?;
    let value = raw.parse::<u32>().map_err(|_| invalid())?;
    require((low..=high).contains(&value))?;
    Ok(value)
}
fn margin(part: &Part<'_>, id: usize, allowed: &[[u32; 4]]) -> HwpxResult<()> {
    let mut sides = [0; 4];
    for (side, key) in sides.iter_mut().zip(["left", "right", "top", "bottom"]) {
        *side = number(part, id, key, 0, u32::MAX)?;
    }
    require(allowed.contains(&sides))
}

// Inline body tables were checked with bounded grids, unequal widths, both height
// policies, three alignments, merges and consecutive tables. Stored heights are
// invalidated without calculating layout or changing cell content.
fn check_inline_category(part: &Part<'_>, table: usize) -> HwpxResult<()> {
    let tags = &part.tags;
    let elements = part.source.elements();
    exact(&tags[table], TABLE)?;
    exact(&tags[table], INLINE_TABLE)?;
    let no_adjust = attr(&tags[table], "noAdjust")?;
    require(matches!(no_adjust, "0" | "1"))?;
    number(part, table, "zOrder", 0, 237)?;
    let rows = number(part, table, "rowCnt", 1, 22)? as usize;
    let columns = number(part, table, "colCnt", 1, 6)? as usize;
    let size = part.one_child(table, "sz")?;
    exact(
        &tags[size],
        &[("widthRelTo", "ABSOLUTE"), ("heightRelTo", "ABSOLUTE"), ("protect", "0")],
    )?;
    number(part, size, "width", 16084, 48545)?;
    number(part, size, "height", 1, 52523)?;
    let position = part.one_child(table, "pos")?;
    exact(&tags[position], POSITION)?;
    exact(&tags[position], &[("treatAsChar", "1"), ("horzRelTo", "PARA")])?;
    margin(
        part,
        part.one_child(table, "inMargin")?,
        &[[141, 141, 141, 141], [510, 510, 141, 141], [283, 283, 283, 283]],
    )?;
    margin(
        part,
        part.one_child(table, "outMargin")?,
        &[
            [283, 283, 283, 283],
            [0, 0, 0, 0],
            [141, 141, 141, 141],
            [141, 141, 0, 141],
            [283, 283, 0, 0],
            [283, 283, 283, 1000],
        ],
    )?;
    require(elements[table].children().iter().all(|&child| {
        matches!(tags[child].name.as_str(), "sz" | "pos" | "outMargin" | "inMargin" | "tr")
    }))?;
    let row_ids: Vec<_> = elements[table]
        .children()
        .iter()
        .copied()
        .filter(|&id| elements[id].name() == "tr")
        .collect();
    require(row_ids.len() == rows)?;
    let mut occupied = [false; 132];
    let mut anchors = 0;
    for (row_index, row) in row_ids.into_iter().enumerate() {
        for &cell in elements[row].children() {
            crate::work::tick();
            // CELL 속성 검사와 겹치지만 행 자식의 이름도 확인하는 중복 방어다.
            require(tags[cell].name == "tc")?;
            require(elements[cell].children().iter().all(|&child| {
                matches!(
                    tags[child].name.as_str(),
                    "subList" | "cellAddr" | "cellSpan" | "cellSz" | "cellMargin"
                )
            }))?;
            exact(&tags[cell], CELL)?;
            let address = part.one_child(cell, "cellAddr")?;
            let r = number(part, address, "rowAddr", 0, rows as u32 - 1)? as usize;
            let c = number(part, address, "colAddr", 0, columns as u32 - 1)? as usize;
            require(r == row_index)?;
            let span = part.one_child(cell, "cellSpan")?;
            let rs = number(part, span, "rowSpan", 1, rows as u32)? as usize;
            let cs = number(part, span, "colSpan", 1, columns as u32)? as usize;
            require(rs == 1 || cs == 1 || rs == 2 && cs == 2)?;
            let end_row = r.checked_add(rs).ok_or_else(invalid)?;
            let end_column = c.checked_add(cs).ok_or_else(invalid)?;
            require(end_row <= rows)?;
            require(end_column <= columns)?;
            for rr in r..end_row {
                for cc in c..end_column {
                    let index = rr
                        .checked_mul(columns)
                        .and_then(|v| v.checked_add(cc))
                        .ok_or_else(invalid)?;
                    let slot = occupied.get_mut(index).ok_or_else(invalid)?;
                    require(!*slot)?;
                    *slot = true;
                }
            }
            let size = part.one_child(cell, "cellSz")?;
            number(part, size, "width", 1303, 48545)?;
            let (low, high) = if no_adjust == "0" { (282, 19637) } else { (1282, 6260) };
            number(part, size, "height", low, high)?;
            exact(&tags[part.one_child(cell, "cellMargin")?], INNER_MARGIN)?;
            let list = part.one_child(cell, "subList")?;
            exact(&tags[list], SUBLIST)?;
            require(matches!(attr(&tags[list], "vertAlign")?, "TOP" | "CENTER" | "BOTTOM"))?;
            anchors += 1;
        }
    }
    require(occupied[..rows * columns].iter().all(|&slot| slot))?;
    let descendants = || {
        elements
            .iter()
            .enumerate()
            .skip(table + 1)
            .take_while(|(_, e)| e.outer().start < elements[table].outer().end)
    };
    require(descendants().filter(|(_, e)| e.name() == "tc").count() == anchors)?;
    require(descendants().filter(|(_, e)| e.name() == "subList").count() == anchors)?;
    require(!descendants().any(|(_, e)| e.name() == "tbl"))
}
