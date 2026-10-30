//! Centralized bridge helpers for HWPX list semantics.
//!
//! HWPX stores paragraph list state as a wire-level
//! `heading(type, idRef, level)` triple on `<hh:paraPr>`, while the shared IR
//! uses [`hwpforge_core::ParagraphListRef`]. This module is the only place that
//! is allowed to translate between those representations.

use hwpforge_core::{BulletDef, NumberingDef, ParaHead, ParagraphListRef};
use hwpforge_foundation::{BulletIndex, HeadingType, NumberFormatType, NumberingIndex};

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
/// Hancom: branded indices become 1-based positions, irrespective of definition id.
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
            level: wire_level(level)?,
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
                })
                .map(|_| numbering_id.get() as u32 + 1)?,
            level: wire_level(level)?,
            checked: false,
        }),
        Some(ParagraphListRef::Bullet { bullet_id, level }) => {
            let bullet = resolve_bullet(bullets, bullet_id)?;
            if bullet.is_checkable() {
                return Err(HwpxError::InvalidStructure {
                    detail: format!(
                        "plain bullet paragraph references checkable bullet definition {}; use CheckBullet semantics",
                        bullet.id
                    ),
                });
            }
            Ok(WireListParts {
                heading_type: HeadingType::Bullet,
                id_ref: bullet_id.get() as u32 + 1,
                level: wire_level(level)?,
                checked: false,
            })
        }
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
                id_ref: bullet_id.get() as u32 + 1,
                level: wire_level(level)?,
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

/// Converts HWPX wire components back into shared list semantics, the
/// reverse of [`list_ref_to_wire_parts`]: a NUMBER or BULLET `idRef` is the
/// 1-based position of its definition, a BULLET is a [`ParagraphListRef::CheckBullet`]
/// exactly when its definition is checkable, and `checked` is read only
/// there (a plain NUMBER or BULLET has no checked state to carry).
///
/// # Errors
///
/// A reference to no definition (`idRef` 0 or past the table) or a level
/// outside `0..=9` cannot be carried; it is an error, never another
/// definition or a clamped level.
pub(crate) fn wire_parts_to_list_ref(
    parts: WireListParts,
    numberings: &[NumberingDef],
    bullets: &[BulletDef],
) -> HwpxResult<Option<ParagraphListRef>> {
    let list = match parts.heading_type {
        HeadingType::None => return Ok(None),
        HeadingType::Outline => ParagraphListRef::Outline { level: shared_level(parts.level)? },
        HeadingType::Number => ParagraphListRef::Number {
            numbering_id: NumberingIndex::new(definition_position(
                parts.id_ref,
                numberings.len(),
                "numbering definition",
            )?),
            level: shared_level(parts.level)?,
        },
        HeadingType::Bullet => {
            let index = definition_position(parts.id_ref, bullets.len(), "bullet definition")?;
            let (bullet_id, level) = (BulletIndex::new(index), shared_level(parts.level)?);
            // 이것을 실패시키는 것: 체크 판정을 정의 대신 `checked` 로
            // (`wire_parts_to_list_ref_reads_checked_only_on_checkable_bullets`).
            if bullets[index].is_checkable() {
                ParagraphListRef::CheckBullet { bullet_id, level, checked: parts.checked }
            } else {
                ParagraphListRef::Bullet { bullet_id, level }
            }
        }
        other => {
            return Err(HwpxError::InvalidStructure {
                detail: format!("heading type {other:?} has no shared list semantics"),
            })
        }
    };
    Ok(Some(list))
}

/// The definition index a 1-based wire `idRef` selects.
/// 이것을 실패시키는 것: `idRef − 1` 대신 `idRef`
/// (`wire_parts_to_list_ref_inverts_list_ref_to_wire_parts`) · 0 을 첫 정의로
/// (`wire_parts_to_list_ref_rejects_what_it_cannot_carry`).
fn definition_position(id_ref: u32, count: usize, kind: &'static str) -> HwpxResult<usize> {
    (id_ref as usize)
        .checked_sub(1)
        .filter(|&index| index < count)
        .ok_or(HwpxError::IndexOutOfBounds { kind, index: id_ref, max: count as u32 })
}

/// A wire `heading@level` as a shared list level, the one level check of
/// both directions.
/// 이것을 실패시키는 것: 상한 검사 삭제(`wire_parts_to_list_ref_rejects_what_it_cannot_carry`).
fn shared_level(level: u32) -> HwpxResult<u8> {
    u8::try_from(level).ok().filter(|&level| level <= ParagraphListRef::MAX_LEVEL).ok_or_else(
        || HwpxError::InvalidStructure {
            detail: format!("list level {level} is outside 0..={}", ParagraphListRef::MAX_LEVEL),
        },
    )
}

/// A shared list level as a wire `heading@level`, checked as the reverse
/// direction checks it.
fn wire_level(level: u8) -> HwpxResult<u32> {
    shared_level(u32::from(level)).map(u32::from)
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

    #[test]
    fn list_ref_to_wire_parts_uses_positions_not_definition_labels() {
        // Hancom: wire references select list positions, even for sparse labels.
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
        let bullets = vec![
            BulletDef {
                id: 7,
                bullet_char: "•".into(),
                checked_char: None,
                use_image: false,
                para_head: default_bullet_para_head(),
            },
            BulletDef {
                id: 8,
                bullet_char: "☐".into(),
                checked_char: Some("☑".into()),
                use_image: false,
                para_head: ParaHead { checkable: true, ..default_bullet_para_head() },
            },
        ];

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
                id_ref: 1,
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
                id_ref: 1,
                level: 0,
                checked: false
            },
        );
        assert_eq!(
            list_ref_to_wire_parts(
                Some(ParagraphListRef::CheckBullet {
                    bullet_id: BulletIndex::new(1),
                    level: 1,
                    checked: true,
                }),
                &numberings,
                &bullets,
            )
            .unwrap(),
            WireListParts { heading_type: HeadingType::Bullet, id_ref: 2, level: 1, checked: true },
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
    fn list_ref_to_wire_parts_rejects_plain_bullet_against_checkable_definition() {
        let bullets = vec![BulletDef {
            id: 7,
            bullet_char: "☐".into(),
            checked_char: Some("☑".into()),
            use_image: false,
            para_head: ParaHead { checkable: true, ..default_bullet_para_head() },
        }];

        let err = list_ref_to_wire_parts(
            Some(ParagraphListRef::Bullet { bullet_id: BulletIndex::new(0), level: 0 }),
            &[],
            &bullets,
        )
        .unwrap_err();
        assert!(matches!(err, HwpxError::InvalidStructure { .. }));
    }

    /// Definitions with sparse labels (42, 7, 8): positions, not labels,
    /// are what a reference selects.
    fn sparse_definitions() -> (Vec<NumberingDef>, Vec<BulletDef>) {
        let numbering = |id| NumberingDef { id, start: 0, levels: Vec::new() };
        let plain = BulletDef {
            id: 7,
            bullet_char: "•".into(),
            checked_char: None,
            use_image: false,
            para_head: default_bullet_para_head(),
        };
        let checkable = BulletDef {
            id: 8,
            bullet_char: "☐".into(),
            checked_char: Some("☑".into()),
            use_image: false,
            para_head: ParaHead { checkable: true, ..default_bullet_para_head() },
        };
        (vec![numbering(42), numbering(3)], vec![plain, checkable])
    }

    fn wire(heading_type: HeadingType, id_ref: u32, level: u32, checked: bool) -> WireListParts {
        WireListParts { heading_type, id_ref, level, checked }
    }

    /// Every carried reference comes back from its wire components.
    /// 이것을 실패시키는 것: `idRef − 1` 대신 `idRef`.
    #[test]
    fn wire_parts_to_list_ref_inverts_list_ref_to_wire_parts() {
        let (numberings, bullets) = sparse_definitions();
        for level in [0, 1, 2, 9] {
            let refs = [
                ParagraphListRef::Outline { level },
                ParagraphListRef::Number { numbering_id: NumberingIndex::new(1), level },
                ParagraphListRef::Bullet { bullet_id: BulletIndex::new(0), level },
                ParagraphListRef::CheckBullet {
                    bullet_id: BulletIndex::new(1),
                    level,
                    checked: false,
                },
                ParagraphListRef::CheckBullet {
                    bullet_id: BulletIndex::new(1),
                    level,
                    checked: true,
                },
            ];
            for list in refs {
                let parts = list_ref_to_wire_parts(Some(list), &numberings, &bullets).unwrap();
                assert_eq!(
                    wire_parts_to_list_ref(parts, &numberings, &bullets).unwrap(),
                    Some(list),
                    "{parts:?}"
                );
            }
        }
        assert_eq!(
            wire_parts_to_list_ref(wire(HeadingType::Number, 2, 3, false), &numberings, &bullets)
                .unwrap(),
            Some(ParagraphListRef::Number { numbering_id: NumberingIndex::new(1), level: 3 })
        );
    }

    /// Wire components the shared reference does not keep come back in
    /// their one spelling: no list is NONE 0/0/false, an outline has no
    /// idRef and no checked state, a NUMBER no checked state.
    /// 이것을 실패시키는 것: NONE 을 개요로 읽기.
    #[test]
    fn list_ref_to_wire_parts_of_a_read_back_is_canonical() {
        let (numberings, bullets) = sparse_definitions();
        let cases = [
            (wire(HeadingType::None, 2, 4, true), wire(HeadingType::None, 0, 0, false)),
            (wire(HeadingType::Outline, 3, 1, true), wire(HeadingType::Outline, 0, 1, false)),
            (wire(HeadingType::Number, 1, 2, true), wire(HeadingType::Number, 1, 2, false)),
            (wire(HeadingType::Bullet, 2, 1, true), wire(HeadingType::Bullet, 2, 1, true)),
        ];
        for (input, canonical) in cases {
            let list = wire_parts_to_list_ref(input, &numberings, &bullets).unwrap();
            assert_eq!(
                list_ref_to_wire_parts(list, &numberings, &bullets).unwrap(),
                canonical,
                "{input:?}"
            );
        }
    }

    /// A BULLET is checkable by its definition; `paraPr@checked` on a plain
    /// bullet is not a checkbox.
    /// 이것을 실패시키는 것: 체크 판정을 정의 대신 `checked` 로.
    #[test]
    fn wire_parts_to_list_ref_reads_checked_only_on_checkable_bullets() {
        let (numberings, bullets) = sparse_definitions();
        let read = |parts| wire_parts_to_list_ref(parts, &numberings, &bullets).unwrap();
        assert_eq!(
            read(wire(HeadingType::Bullet, 1, 0, true)),
            Some(ParagraphListRef::Bullet { bullet_id: BulletIndex::new(0), level: 0 })
        );
        assert_eq!(
            read(wire(HeadingType::Bullet, 2, 0, false)),
            Some(ParagraphListRef::CheckBullet {
                bullet_id: BulletIndex::new(1),
                level: 0,
                checked: false
            })
        );
    }

    /// No definition and a level past 9 are errors: never the first
    /// definition, never a clamped level.
    /// 이것을 실패시키는 것: `idRef` 0 을 첫 정의로(`checked_sub` 대신 `saturating_sub`).
    #[test]
    fn wire_parts_to_list_ref_rejects_what_it_cannot_carry() {
        let (numberings, bullets) = sparse_definitions();
        for parts in [
            wire(HeadingType::Number, 0, 0, false),
            wire(HeadingType::Number, 3, 0, false),
            wire(HeadingType::Bullet, 0, 0, false),
            wire(HeadingType::Bullet, 3, 0, false),
        ] {
            let err = wire_parts_to_list_ref(parts, &numberings, &bullets).unwrap_err();
            assert!(matches!(err, HwpxError::IndexOutOfBounds { .. }), "{parts:?}: {err}");
        }
        for heading_type in [HeadingType::Outline, HeadingType::Number, HeadingType::Bullet] {
            let err =
                wire_parts_to_list_ref(wire(heading_type, 1, 10, false), &numberings, &bullets)
                    .unwrap_err();
            assert!(matches!(err, HwpxError::InvalidStructure { .. }), "{heading_type:?}: {err}");
        }
    }

    /// The writing direction checks the level as reading does.
    /// 이것을 실패시키는 것: 쓰기 쪽 `wire_level` 을 `u32::from` 으로.
    #[test]
    fn list_ref_to_wire_parts_rejects_a_level_past_nine() {
        let err = list_ref_to_wire_parts(Some(ParagraphListRef::Outline { level: 10 }), &[], &[])
            .unwrap_err();
        assert!(matches!(err, HwpxError::InvalidStructure { .. }), "{err}");
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
