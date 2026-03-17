use hwpforge_core::table::TablePageBreak;

use crate::decoder::section::Hwp5TablePageBreak;
use crate::semantic::Hwp5SemanticTablePageBreak;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KnownHwp5TablePageBreak {
    None,
    Cell,
    Table,
}

impl From<KnownHwp5TablePageBreak> for TablePageBreak {
    fn from(value: KnownHwp5TablePageBreak) -> Self {
        match value {
            KnownHwp5TablePageBreak::None => Self::None,
            KnownHwp5TablePageBreak::Cell => Self::Cell,
            KnownHwp5TablePageBreak::Table => Self::Table,
        }
    }
}

impl From<KnownHwp5TablePageBreak> for Hwp5SemanticTablePageBreak {
    fn from(value: KnownHwp5TablePageBreak) -> Self {
        match value {
            KnownHwp5TablePageBreak::None => Self::None,
            KnownHwp5TablePageBreak::Cell => Self::Cell,
            KnownHwp5TablePageBreak::Table => Self::Table,
        }
    }
}

fn classify_hwp5_table_page_break(
    value: Hwp5TablePageBreak,
) -> Result<KnownHwp5TablePageBreak, u8> {
    match value {
        Hwp5TablePageBreak::None => Ok(KnownHwp5TablePageBreak::None),
        Hwp5TablePageBreak::Cell => Ok(KnownHwp5TablePageBreak::Cell),
        Hwp5TablePageBreak::Table => Ok(KnownHwp5TablePageBreak::Table),
        Hwp5TablePageBreak::Unknown(raw) => Err(raw),
    }
}

pub(crate) fn core_table_page_break(value: Hwp5TablePageBreak) -> Option<TablePageBreak> {
    classify_hwp5_table_page_break(value).ok().map(Into::into)
}

pub(crate) fn semantic_table_page_break(value: Hwp5TablePageBreak) -> Hwp5SemanticTablePageBreak {
    match classify_hwp5_table_page_break(value) {
        Ok(known) => known.into(),
        Err(raw) => Hwp5SemanticTablePageBreak::Unknown(raw),
    }
}

pub(crate) fn unknown_hwp5_table_page_break_raw(value: Hwp5TablePageBreak) -> Option<u8> {
    classify_hwp5_table_page_break(value).err()
}
