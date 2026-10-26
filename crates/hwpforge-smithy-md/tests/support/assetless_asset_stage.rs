use super::*;

pub(super) fn names_empty(warnings: &[MdWarning]) -> bool {
    warnings.iter().any(|w| matches!(w, MdWarning::ImageEmbedSkipped { src, .. } if src.is_empty()))
}
