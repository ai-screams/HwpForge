use hwpforge_core::table::TableVerticalAlign;

use crate::decoder::section::Hwp5TableCellVerticalAlign;
use crate::semantic::Hwp5SemanticTableCellVerticalAlign;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KnownHwp5TableCellVerticalAlign {
    Top,
    Center,
    Bottom,
}

impl From<KnownHwp5TableCellVerticalAlign> for TableVerticalAlign {
    fn from(value: KnownHwp5TableCellVerticalAlign) -> Self {
        match value {
            KnownHwp5TableCellVerticalAlign::Top => Self::Top,
            KnownHwp5TableCellVerticalAlign::Center => Self::Center,
            KnownHwp5TableCellVerticalAlign::Bottom => Self::Bottom,
        }
    }
}

impl From<KnownHwp5TableCellVerticalAlign> for Hwp5SemanticTableCellVerticalAlign {
    fn from(value: KnownHwp5TableCellVerticalAlign) -> Self {
        match value {
            KnownHwp5TableCellVerticalAlign::Top => Self::Top,
            KnownHwp5TableCellVerticalAlign::Center => Self::Center,
            KnownHwp5TableCellVerticalAlign::Bottom => Self::Bottom,
        }
    }
}

fn classify_hwp5_table_cell_vertical_align(
    value: Hwp5TableCellVerticalAlign,
) -> Result<KnownHwp5TableCellVerticalAlign, u8> {
    match value {
        Hwp5TableCellVerticalAlign::Top => Ok(KnownHwp5TableCellVerticalAlign::Top),
        Hwp5TableCellVerticalAlign::Center => Ok(KnownHwp5TableCellVerticalAlign::Center),
        Hwp5TableCellVerticalAlign::Bottom => Ok(KnownHwp5TableCellVerticalAlign::Bottom),
        Hwp5TableCellVerticalAlign::Unknown(raw) => Err(raw),
    }
}

pub(crate) fn core_table_cell_vertical_align(
    value: Hwp5TableCellVerticalAlign,
) -> Option<TableVerticalAlign> {
    classify_hwp5_table_cell_vertical_align(value).ok().map(Into::into)
}

pub(crate) fn semantic_table_cell_vertical_align(
    value: Hwp5TableCellVerticalAlign,
) -> Hwp5SemanticTableCellVerticalAlign {
    match classify_hwp5_table_cell_vertical_align(value) {
        Ok(known) => known.into(),
        Err(raw) => Hwp5SemanticTableCellVerticalAlign::Unknown(raw),
    }
}

pub(crate) fn unknown_hwp5_table_cell_vertical_align_raw(
    value: Hwp5TableCellVerticalAlign,
) -> Option<u8> {
    classify_hwp5_table_cell_vertical_align(value).err()
}
