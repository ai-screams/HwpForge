//! Private reasons established by the decoder at the warning's creation site.
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LossCause {
    PerLanguageScalar,
    DrawnExtentOnly,
    GroupPictureAppearance,
    InlineStyle,
    HeaderOnlyNonlinearOrder,
}
impl LossCause {
    pub(crate) fn kind(self) -> &'static str {
        match self {
            Self::PerLanguageScalar => "char_shape_per_language",
            Self::DrawnExtentOnly => "shape_drawn_extent",
            Self::GroupPictureAppearance => "group_child_picture",
            Self::InlineStyle => "t.charStyleIDRef",
            Self::HeaderOnlyNonlinearOrder => "package_reading_order",
        }
    }
}
#[derive(Debug, Clone)]
pub(crate) enum LossLocation {
    Header { definition: u32, digest: [u8; 32] },
    Body { part: String, span: Range<usize>, digest: [u8; 32] },
    Package { sections: Vec<String> },
}
#[derive(Debug, Clone)]
pub(crate) struct LossEvidence {
    pub(crate) warning: usize,
    pub(crate) cause: LossCause,
    pub(crate) location: LossLocation,
}
