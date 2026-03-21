//! Centralized bridge helpers for HWPX list semantics.
//!
//! HWPX stores paragraph list state as a wire-level
//! `heading(type, idRef, level)` triple on `<hh:paraPr>`, while the shared IR
//! uses [`hwpforge_core::ParagraphListRef`]. This module is the only place that
//! is allowed to translate between those representations.

use hwpforge_core::{BulletDef, NumberingDef, ParaHead, ParagraphListRef};
use hwpforge_foundation::{HeadingType, NumberFormatType};

use crate::error::{HwpxError, HwpxResult};
use crate::schema::header::{HxBullet, HxBulletParaHead, HxHeading};

/// Shared paragraph-list wire components stored on `hh:paraPr`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WireListParts {
    pub heading_type: HeadingType,
    pub id_ref: u32,
    pub level: u32,
    pub checked: bool,
}

/// Converts shared list semantics into HWPX wire components.
pub(crate) fn list_ref_to_wire_parts(
    list_ref: Option<ParagraphListRef>,
    numberings: &[NumberingDef],
    bullets: &[BulletDef],
) -> HwpxResult<WireListParts> {
    match list_ref {
        // HWPX `paraPr/heading(level)` is zero-based for outline as well.
        // Only `<hh:numbering>/<hh:paraHead level="...">` uses one-based
        // numbering levels.
        Some(ParagraphListRef::Outline { level }) => Ok(WireListParts {
            heading_type: HeadingType::Outline,
            id_ref: 0,
            level: u32::from(level),
            checked: false,
        }),
        Some(ParagraphListRef::Number { numbering_id, level }) => Ok(WireListParts {
            heading_type: HeadingType::Number,
            id_ref: numberings
                .get(numbering_id.get())
                .ok_or_else(|| HwpxError::IndexOutOfBounds {
                    kind: "numbering definition",
                    index: numbering_id.get() as u32,
                    max: numberings.len() as u32,
                })?
                .id,
            level: u32::from(level),
            checked: false,
        }),
        Some(ParagraphListRef::Bullet { bullet_id, level }) => Ok(WireListParts {
            heading_type: HeadingType::Bullet,
            id_ref: resolve_bullet(bullets, bullet_id)?.id,
            level: u32::from(level),
            checked: false,
        }),
        Some(ParagraphListRef::CheckBullet { bullet_id, level, checked }) => {
            let bullet = resolve_bullet(bullets, bullet_id)?;
            if !bullet.is_checkable() {
                return Err(HwpxError::InvalidStructure {
                    detail: format!(
                        "checkable bullet paragraph references non-checkable bullet definition {}",
                        bullet.id
                    ),
                });
            }
            Ok(WireListParts {
                heading_type: HeadingType::Bullet,
                id_ref: bullet.id,
                level: u32::from(level),
                checked,
            })
        }
        None => Ok(WireListParts {
            heading_type: HeadingType::None,
            id_ref: 0,
            level: 0,
            checked: false,
        }),
    }
}

/// Converts raw wire components into a HWPX heading element.
pub(crate) fn wire_parts_to_heading(
    heading_type: HeadingType,
    id_ref: u32,
    level: u32,
) -> HxHeading {
    HxHeading { heading_type: heading_type.to_hwpx_str().into(), id_ref, level }
}

/// Converts a HWPX heading type into the `para_list_type()` string used by the
/// Markdown bridge.
pub(crate) fn heading_type_to_para_list_type(heading_type: HeadingType) -> Option<&'static str> {
    match heading_type {
        HeadingType::Bullet => Some("BULLET"),
        HeadingType::Number => Some("NUMBER"),
        _ => None,
    }
}

/// Converts a shared bullet definition into the HWPX bullet schema type.
pub(crate) fn bullet_def_to_hwpx(bullet: &BulletDef) -> HxBullet {
    HxBullet {
        id: bullet.id,
        bullet_char: bullet.bullet_char.clone(),
        checked_char: bullet.checked_char.clone(),
        use_image: u32::from(bullet.use_image),
        para_heads: vec![para_head_to_hwpx(&bullet.para_head, bullet.is_checkable())],
    }
}

/// Converts a HWPX bullet schema type into the shared bullet definition.
pub(crate) fn bullet_def_from_hwpx(bullet: &HxBullet) -> BulletDef {
    let para_head =
        bullet.para_heads.first().map(para_head_from_hwpx).unwrap_or_else(default_bullet_para_head);

    BulletDef {
        id: bullet.id,
        bullet_char: bullet.bullet_char.clone(),
        checked_char: bullet.checked_char.clone(),
        use_image: bullet.use_image != 0,
        para_head,
    }
}

fn para_head_to_hwpx(para_head: &ParaHead, is_checkable: bool) -> HxBulletParaHead {
    HxBulletParaHead {
        level: para_head.level.saturating_sub(1),
        align: "LEFT".into(),
        use_inst_width: 0,
        auto_indent: 1,
        width_adjust: 0,
        text_offset_type: "PERCENT".into(),
        text_offset: 50,
        num_format: number_format_to_hwpx(para_head.num_format).into(),
        char_pr_id_ref: u32::MAX,
        checkable: u32::from(is_checkable),
        text: para_head.text.clone(),
    }
}

fn para_head_from_hwpx(para_head: &HxBulletParaHead) -> ParaHead {
    ParaHead {
        start: 0,
        level: para_head.level + 1,
        num_format: number_format_from_hwpx(&para_head.num_format),
        text: para_head.text.clone(),
        checkable: para_head.checkable != 0,
    }
}

fn default_bullet_para_head() -> ParaHead {
    ParaHead {
        start: 0,
        level: 1,
        num_format: NumberFormatType::Digit,
        text: String::new(),
        checkable: false,
    }
}

fn resolve_bullet(
    bullets: &[BulletDef],
    bullet_id: hwpforge_foundation::BulletIndex,
) -> HwpxResult<&BulletDef> {
    bullets.get(bullet_id.get()).ok_or_else(|| HwpxError::IndexOutOfBounds {
        kind: "bullet definition",
        index: bullet_id.get() as u32,
        max: bullets.len() as u32,
    })
}

fn number_format_to_hwpx(format: NumberFormatType) -> &'static str {
    match format {
        NumberFormatType::Digit => "DIGIT",
        NumberFormatType::CircledDigit => "CIRCLED_DIGIT",
        NumberFormatType::RomanCapital => "ROMAN_CAPITAL",
        NumberFormatType::RomanSmall => "ROMAN_SMALL",
        NumberFormatType::LatinCapital => "LATIN_CAPITAL",
        NumberFormatType::LatinSmall => "LATIN_SMALL",
        NumberFormatType::CircledLatinSmall => "CIRCLED_LATIN_SMALL",
        NumberFormatType::HangulSyllable => "HANGUL_SYLLABLE",
        NumberFormatType::HangulJamo => "HANGUL_JAMO",
        NumberFormatType::HanjaDigit => "HANJA_DIGIT",
        NumberFormatType::CircledHangulSyllable => "CIRCLED_HANGUL_SYLLABLE",
        _ => "DIGIT",
    }
}

fn number_format_from_hwpx(format: &str) -> NumberFormatType {
    match format {
        "CIRCLED_DIGIT" => NumberFormatType::CircledDigit,
        "ROMAN_CAPITAL" => NumberFormatType::RomanCapital,
        "ROMAN_SMALL" => NumberFormatType::RomanSmall,
        "LATIN_CAPITAL" => NumberFormatType::LatinCapital,
        "LATIN_SMALL" => NumberFormatType::LatinSmall,
        "CIRCLED_LATIN_SMALL" => NumberFormatType::CircledLatinSmall,
        "HANGUL_SYLLABLE" => NumberFormatType::HangulSyllable,
        "HANGUL_JAMO" => NumberFormatType::HangulJamo,
        "HANJA_DIGIT" => NumberFormatType::HanjaDigit,
        "CIRCLED_HANGUL_SYLLABLE" => NumberFormatType::CircledHangulSyllable,
        _ => NumberFormatType::Digit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_foundation::{BulletIndex, NumberingIndex};

    #[test]
    fn list_ref_to_wire_parts_uses_definition_ids_not_indices() {
        let numberings = vec![NumberingDef {
            id: 42,
            start: 0,
            levels: vec![ParaHead {
                start: 1,
                level: 1,
                num_format: NumberFormatType::Digit,
                text: "^1.".into(),
                checkable: false,
            }],
        }];
        let bullets = vec![BulletDef {
            id: 7,
            bullet_char: "•".into(),
            checked_char: Some("☑".into()),
            use_image: false,
            para_head: ParaHead { checkable: true, ..default_bullet_para_head() },
        }];

        assert_eq!(
            list_ref_to_wire_parts(
                Some(ParagraphListRef::Outline { level: 0 }),
                &numberings,
                &bullets,
            )
            .unwrap(),
            WireListParts {
                heading_type: HeadingType::Outline,
                id_ref: 0,
                level: 0,
                checked: false
            },
        );
        assert_eq!(
            list_ref_to_wire_parts(
                Some(ParagraphListRef::Number { numbering_id: NumberingIndex::new(0), level: 2 }),
                &numberings,
                &bullets,
            )
            .unwrap(),
            WireListParts {
                heading_type: HeadingType::Number,
                id_ref: 42,
                level: 2,
                checked: false
            },
        );
        assert_eq!(
            list_ref_to_wire_parts(
                Some(ParagraphListRef::Bullet { bullet_id: BulletIndex::new(0), level: 0 }),
                &numberings,
                &bullets,
            )
            .unwrap(),
            WireListParts {
                heading_type: HeadingType::Bullet,
                id_ref: 7,
                level: 0,
                checked: false
            },
        );
        assert_eq!(
            list_ref_to_wire_parts(
                Some(ParagraphListRef::CheckBullet {
                    bullet_id: BulletIndex::new(0),
                    level: 1,
                    checked: true,
                }),
                &numberings,
                &bullets,
            )
            .unwrap(),
            WireListParts { heading_type: HeadingType::Bullet, id_ref: 7, level: 1, checked: true },
        );
    }

    #[test]
    fn list_ref_to_wire_parts_rejects_invalid_definition_indices() {
        let err = list_ref_to_wire_parts(
            Some(ParagraphListRef::Number { numbering_id: NumberingIndex::new(99), level: 0 }),
            &[],
            &[],
        )
        .unwrap_err();
        assert!(matches!(err, HwpxError::IndexOutOfBounds { kind: "numbering definition", .. }));

        let err = list_ref_to_wire_parts(
            Some(ParagraphListRef::Bullet { bullet_id: BulletIndex::new(99), level: 0 }),
            &[],
            &[],
        )
        .unwrap_err();
        assert!(matches!(err, HwpxError::IndexOutOfBounds { kind: "bullet definition", .. }));
    }

    #[test]
    fn bullet_roundtrip_preserves_use_image_and_text() {
        let bullet = BulletDef {
            id: 1,
            bullet_char: "".into(),
            checked_char: Some("☑".into()),
            use_image: true,
            para_head: ParaHead {
                start: 0,
                level: 1,
                num_format: NumberFormatType::Digit,
                text: String::new(),
                checkable: false,
            },
        };

        let hwpx = bullet_def_to_hwpx(&bullet);
        let roundtrip = bullet_def_from_hwpx(&hwpx);

        assert_eq!(roundtrip.id, 1);
        assert_eq!(roundtrip.bullet_char, "");
        assert_eq!(roundtrip.checked_char.as_deref(), Some("☑"));
        assert!(roundtrip.use_image);
        assert_eq!(roundtrip.para_head.level, 1);
        assert!(roundtrip.para_head.checkable);
    }
}
