//! Deferred, per-output projection of marker formatting onto shared HWPX heads.
use crate::encoder::wire_rules::{marker_base_width, marker_metrics};
use crate::paragraph_measure::invalid;
use crate::style_store::{fidelity::HeadMetrics, HwpxParaShape};
use crate::{EncodeWarning, HwpxResult, HwpxStyleStore};
use hwpforge_blueprint::ListMarkerLayout;
use hwpforge_core::{Document, ParaHead, Paragraph, ParagraphMeasure, StyleLookup, Validated};
use hwpforge_foundation::{Alignment, CharShapeIndex, HeadingType, ParaShapeIndex};
use std::{borrow::Cow, collections::BTreeMap};

// Bullet kind, one-based definition position, and the emitted head level.
type Key = (bool, u32, u32);
fn target<'a>(store: &'a HwpxStyleStore, shape: &HwpxParaShape) -> HwpxResult<(Key, &'a ParaHead)> {
    let position = shape.heading_id_ref;
    let index = usize::try_from(
        position.checked_sub(1).ok_or_else(|| invalid("marker definition position is zero"))?,
    )
    .map_err(|_| invalid("marker definition position overflow"))?;
    match shape.heading_type {
        HeadingType::Number => {
            let level = shape
                .heading_level
                .checked_add(1)
                .ok_or_else(|| invalid("marker level overflow"))?;
            let def = store
                .numbering_at(index)
                .ok_or_else(|| invalid("missing marker numbering definition"))?;
            let head = def
                .levels
                .iter()
                .find(|h| h.level == level)
                .ok_or_else(|| invalid("missing marker numbering level"))?;
            Ok(((false, position, head.level), head))
        }
        HeadingType::Bullet => {
            let def = store
                .bullet_at(index)
                .ok_or_else(|| invalid("missing marker bullet definition"))?;
            let wire = crate::list_bridge::bullet_def_to_hwpx(def);
            let level = wire.para_heads[0].level;
            Ok(((true, position, level), &def.para_head))
        }
        _ => Err(invalid("marker layout requires Number, Bullet or CheckBullet semantics")),
    }
}
fn existing(store: &HwpxStyleStore, (bullet, position, level): Key) -> HeadMetrics {
    let map = if bullet { &store.header_fidelity.bullet } else { &store.header_fidelity.numbering };
    map.get(&position)
        .and_then(|ls| ls.get(&level))
        .cloned()
        .unwrap_or_else(|| HeadMetrics::generated(u32::from(!bullet)))
}
pub(crate) fn validate_requests(store: &HwpxStyleStore) -> HwpxResult<()> {
    for (&id, &layout) in &store.list_marker_layouts {
        ListMarkerLayout::new(layout.align, layout.text_start)
            .map_err(|e| invalid(e.to_string()))?;
        let shape = store.para_shape(ParaShapeIndex::new(
            usize::try_from(id).map_err(|_| invalid("marker paragraph position overflow"))?,
        ))?;
        store.para_list_ref(shape)?;
        target(store, shape)?;
        if crate::paragraph_measure::parts(shape.indent) != (0, "HWPUNIT") {
            return Err(invalid("marker layout requires zero first-line indentation"));
        }
        if crate::paragraph_measure::parts(shape.margin_left).1 != "HWPUNIT" {
            return Err(invalid("marker anchor must use HwpUnit"));
        }
        if let Some(id) = layout.char_shape {
            store.char_shape(id)?;
        }
    }
    Ok(())
}
fn loss(kind: &str, reason: impl Into<String>) -> EncodeWarning {
    EncodeWarning::ContentLost {
        part: "Contents/header.xml".into(),
        path: crate::decoder::ParagraphPath(Vec::new()),
        kind: kind.into(),
        reason: reason.into(),
    }
}
fn wire_metrics(m: &HeadMetrics) -> String {
    format!(
        concat!(
            "align={} useInstWidth={} autoIndent={} widthAdjust={} ",
            "textOffsetType={} textOffset={} charPrIDRef={}",
        ),
        m.align,
        m.use_inst_width,
        m.auto_indent,
        m.width_adjust,
        m.text_offset_type,
        m.text_offset,
        m.char_pr_id_ref,
    )
}
struct Group {
    first: HeadMetrics,
    first_requested: bool,
    // The existing metrics setter takes the definition level, before wire conversion.
    native_level: u32,
    shift: i32,
    second: Option<(HeadMetrics, i32)>,
    count: usize,
    requested: bool,
    margins: BTreeMap<ParaShapeIndex, (i32, &'static str)>,
    warnings: Vec<EncodeWarning>,
}
fn font_context(
    store: &HwpxStyleStore,
    p: &Paragraph,
    layout: ListMarkerLayout,
    old: &HeadMetrics,
    warnings: &mut Vec<EncodeWarning>,
) -> HwpxResult<(i32, u32)> {
    let explicit = match layout.char_shape {
        Some(id) => Some(id),
        None if old.char_pr_id_ref != u32::MAX => Some(CharShapeIndex::new(
            usize::try_from(old.char_pr_id_ref)
                .map_err(|_| invalid("marker character reference overflow"))?,
        )),
        None => None,
    };
    let inherited = p
        .runs
        .first()
        .map(|r| r.char_shape_id)
        .or(p.end_char_shape)
        .or_else(|| p.style_ref.and_then(|s| store.style_char_shape(s)));
    let id = explicit.or(inherited).ok_or_else(|| invalid("marker has no character context"))?;
    let context = store.char_shape(id)?;
    if explicit.is_none()
        && p.runs.iter().any(|r| store.char_shape(r.char_shape_id).ok() != Some(context))
    {
        warnings.push(loss(
            "list_marker_font_context",
            "Inherited marker character context differs between runs; first run selected",
        ));
    }
    let size = context.height.as_i32();
    if size <= 0 {
        return Err(invalid("marker character size must be positive"));
    }
    let reference = match layout.char_shape {
        Some(id) => {
            u32::try_from(id.get()).map_err(|_| invalid("marker character reference overflow"))?
        }
        None => old.char_pr_id_ref,
    };
    Ok((size, reference))
}
/// Projects the first effective use of each shared head, including an unspecified request.
/// Sections follow storage order; each paragraph is visited before run descendants,
/// with table rows/cells/paragraphs before captions, then headers, footers and page backgrounds.
/// Every original anchor receives the first use's relative margin shift once.
/// Only the selected candidate's approximation warnings survive, even on conflict.
/// Uses the existing paragraph walk, which excludes run-picture captions.
/// Outline paragraphs keep their separate outline semantics and are not counted;
/// conflicts between ordinary-list requests and those uses are not detected here.
pub(crate) fn prepare<'a>(
    document: &Document<Validated>,
    store: &'a HwpxStyleStore,
) -> HwpxResult<(Cow<'a, HwpxStyleStore>, Vec<EncodeWarning>)> {
    if store.list_marker_layouts.is_empty() {
        return Ok((Cow::Borrowed(store), Vec::new()));
    }
    validate_requests(store)?;
    let mut groups: BTreeMap<Key, Group> = BTreeMap::new();
    let mut warnings = vec![];
    let mut error = None;
    for section in document.sections() {
        section.for_each_paragraph(|p| {
            let result = (|| -> HwpxResult<()> {
                let shape = store.para_shape(p.para_shape_id)?;
                if !matches!(shape.heading_type, HeadingType::Number | HeadingType::Bullet) {
                    return Ok(());
                }
                let request = store
                    .list_marker_layouts
                    .get(
                        &u32::try_from(p.para_shape_id.get())
                            .map_err(|_| invalid("marker paragraph reference overflow"))?,
                    )
                    .copied();
                let (key, head) = match target(store, shape) {
                    Ok(target) => target,
                    Err(_) if request.is_none() => return Ok(()),
                    Err(error) => return Err(error),
                };
                let old = existing(store, key);
                let mut pending = vec![];
                let (metrics, shift) = if let Some(layout) = request {
                    let (anchor, _) = crate::paragraph_measure::parts(shape.margin_left);
                    let (size, reference) = font_context(store, p, layout, &old, &mut pending)?;
                    let (base, approximate) =
                        marker_base_width(if key.0 { None } else { Some(head) }, size)?;
                    if approximate {
                        pending.push(loss(
                            "list_marker_base_width",
                            "Unmeasured numbering format uses one em for the nominal base width",
                        ));
                    }
                    let (margin, metrics, approximation) =
                        marker_metrics(layout, anchor, base, reference)?;
                    if let Some(reason) = approximation {
                        pending.push(loss("list_marker_alignment", reason));
                    }
                    (
                        metrics,
                        margin
                            .checked_sub(anchor)
                            .ok_or_else(|| invalid("marker margin shift overflow"))?,
                    )
                } else {
                    (old, 0)
                };
                let group = groups.entry(key).or_insert_with(|| Group {
                    first: metrics.clone(),
                    first_requested: request.is_some(),
                    native_level: head.level,
                    shift,
                    second: None,
                    count: 0,
                    requested: false,
                    margins: BTreeMap::new(),
                    warnings: pending,
                });
                group.count += 1;
                group.requested |= request.is_some();
                if (metrics != group.first || shift != group.shift) && group.second.is_none() {
                    group.second = Some((metrics, shift));
                }
                let (anchor, unit) = crate::paragraph_measure::parts(shape.margin_left);
                group.margins.insert(p.para_shape_id, (anchor, unit));
                Ok(())
            })();
            if let Err(e) = result {
                error.get_or_insert(e);
            }
        });
    }
    if let Some(e) = error {
        return Err(e);
    }
    let mut out = store.clone();
    for ((bullet, position, level), group) in groups {
        if !group.requested {
            continue;
        }
        if let Some((other, shift)) = group.second {
            let definition = if bullet { "bullet" } else { "numbering" };
            warnings.push(loss(
                "list_marker_conflict",
                format!(
                    concat!(
                        "{} definition position {}, head level {}: {} margin shift {} versus {} margin shift {}; ",
                        "{} uses; first use head and relative margin shift applied",
                    ),
                    definition,
                    position,
                    level,
                    wire_metrics(&group.first),
                    group.shift,
                    wire_metrics(&other),
                    shift,
                    group.count,
                ),
            ));
        }
        warnings.extend(group.warnings);
        if !group.first_requested && group.shift == 0 {
            continue;
        }
        let m = group.first;
        let align = match m.align.as_str() {
            "LEFT" => Alignment::Left,
            "RIGHT" => Alignment::Right,
            "CENTER" => Alignment::Center,
            _ => return Err(invalid("unsupported marker head alignment")),
        };
        out.set_paragraph_head_metrics(
            if bullet { HeadingType::Bullet } else { HeadingType::Number },
            position,
            group.native_level,
            align,
            m.use_inst_width != 0,
            m.auto_indent != 0,
            m.width_adjust,
            m.text_offset,
            m.text_offset_type == "HWPUNIT",
            m.char_pr_id_ref,
        )?;
        for (id, (anchor, unit)) in group.margins {
            if group.shift == 0 {
                continue;
            }
            if unit != "HWPUNIT" {
                return Err(invalid("marker margin shift requires HwpUnit"));
            }
            let margin =
                anchor.checked_add(group.shift).ok_or_else(|| invalid("marker margin overflow"))?;
            out.para_shape_mut(id)?.margin_left =
                ParagraphMeasure::from_hwp_unit(hwpforge_foundation::HwpUnit::new(margin)?);
        }
    }
    out.list_marker_layouts.clear();
    Ok((Cow::Owned(out), warnings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{style_store::HwpxCharShape, EncodeOptions, HwpxEncoder};
    use hwpforge_blueprint::{ListMarkerAlign as A, ListTextStart as T};
    use hwpforge_core::{ImageStore, NumberingDef, PageSettings, ParaHead, Run, Section};
    use hwpforge_foundation::{HwpUnit, NumberFormatType};
    // 이것을 실패시키는 것: 빈 fidelity를 JSON에 빈 객체로 직렬화.
    #[test]
    fn empty_header_fidelity_preserves_store_json_bytes() {
        let store = HwpxStyleStore::new();
        let bytes = serde_json::to_vec(&store).unwrap();
        let mut expected = serde_json::to_value(&store).unwrap();
        assert!(expected.get("header_fidelity").is_none());
        expected.as_object_mut().unwrap().insert("header_fidelity".into(), serde_json::json!({}));
        let restored: HwpxStyleStore = serde_json::from_value(expected).unwrap();
        assert_eq!(serde_json::to_vec(&restored).unwrap(), bytes);
    }
    // 이것을 실패시키는 것: 자동 간격만 담긴 fidelity를 빈 값으로 취급하여 JSON에서 제거.
    #[test]
    fn automatic_spacing_fidelity_alone_survives_store_json() {
        let mut s = HwpxStyleStore::new();
        s.push_para_shape(HwpxParaShape::default());
        s.header_fidelity.auto_spacing.insert(0, Some([0, 1]));
        let value = serde_json::to_value(&s).unwrap();
        assert_eq!(value["header_fidelity"]["auto_spacing"]["0"], serde_json::json!([0, 1]));
        let restored: HwpxStyleStore = serde_json::from_value(value).unwrap();
        assert_eq!(restored.header_fidelity.auto_spacing.get(&0), Some(&Some([0, 1])));
    }
    // 이것을 실패시키는 것: 지연 요청만 다른 저장소도 동일한 편집 스타일로 판정.
    #[test]
    fn deferred_requests_participate_in_style_identity() {
        let a = HwpxStyleStore::new();
        let mut b = a.clone();
        b.list_marker_layouts
            .insert(0, ListMarkerLayout::new(A::Left, T::FromAnchor(HwpUnit::ZERO)).unwrap());
        assert!(!a.same_definitions(&b));
    }
    // 이것을 실패시키는 것: 빈 요청에서도 문단을 순회하거나 store 사본을 생성.
    #[test]
    fn empty_marker_requests_borrow_the_original_store() {
        let mut d = Document::new();
        d.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::text("body", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        ));
        let s = HwpxStyleStore::new();
        let (store, warnings) = prepare(&d.validate().unwrap(), &s).unwrap();
        assert!(matches!(store, Cow::Borrowed(_)));
        assert!(warnings.is_empty());
    }
    // 이것을 실패시키는 것: 빈 문단의 끝 표시보다 style을 먼저 선택하거나 유효 문자 문맥을 합성.
    #[test]
    fn empty_marker_context_uses_end_then_style() {
        let mut s = HwpxStyleStore::with_default_fonts("맑은 고딕");
        s.push_char_shape(HwpxCharShape {
            height: HwpUnit::new(1100).unwrap(),
            ..Default::default()
        });
        s.push_char_shape(HwpxCharShape {
            height: HwpUnit::new(1400).unwrap(),
            ..Default::default()
        });
        s.push_style(crate::style_store::HwpxStyle::new(
            0, "PARA", "body", "body", 0, 1, 0, 1042, 0,
        ));
        let mut p = Paragraph::new(ParaShapeIndex::new(0));
        p.end_char_shape = Some(CharShapeIndex::new(0));
        p.style_ref = Some(hwpforge_foundation::StyleRef::new(0));
        let m = ListMarkerLayout::new(A::Left, T::FromAnchor(HwpUnit::ZERO)).unwrap();
        let old = HeadMetrics::generated(1);
        assert_eq!(font_context(&s, &p, m, &old, &mut vec![]).unwrap(), (1100, u32::MAX));
        p.end_char_shape = None;
        assert_eq!(font_context(&s, &p, m, &old, &mut vec![]).unwrap(), (1400, u32::MAX));
        p.style_ref = None;
        assert!(font_context(&s, &p, m, &old, &mut vec![]).is_err());
    }
    // 이것을 실패시키는 것: ContentLost 새 kind를 비의미 경고로 분류.
    #[test]
    fn marker_loss_fails_closed_in_editors() {
        for kind in [
            "list_marker_alignment",
            "list_marker_conflict",
            "list_marker_base_width",
            "list_marker_font_context",
        ] {
            let mut s = HwpxStyleStore::with_default_fonts("맑은 고딕");
            s.push_char_shape(HwpxCharShape {
                height: HwpUnit::new(1100).unwrap(),
                ..Default::default()
            });
            s.push_char_shape(HwpxCharShape {
                height: HwpUnit::new(1400).unwrap(),
                ..Default::default()
            });
            s.push_numbering(NumberingDef {
                id: 1,
                start: 1,
                levels: vec![ParaHead {
                    level: 1,
                    start: 1,
                    num_format: if kind == "list_marker_base_width" {
                        NumberFormatType::RomanCapital
                    } else {
                        NumberFormatType::Digit
                    },
                    text: "^1.".into(),
                    checkable: false,
                }],
            });
            let shape = HwpxParaShape {
                heading_type: HeadingType::Number,
                heading_id_ref: 1,
                ..Default::default()
            };
            s.push_para_shape(shape.clone());
            s.push_para_shape(shape);
            let mut m = ListMarkerLayout::new(
                if kind == "list_marker_alignment" { A::Right } else { A::Left },
                if kind == "list_marker_alignment" {
                    T::AfterMarker(HwpUnit::ZERO)
                } else {
                    T::FromAnchor(HwpUnit::new(1800).unwrap())
                },
            )
            .unwrap();
            m.char_shape = if kind == "list_marker_font_context" {
                None
            } else {
                Some(CharShapeIndex::new(0))
            };
            s.list_marker_layouts.insert(0, m);
            let mut p = Paragraph::with_runs(
                vec![Run::text("a", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            );
            if kind == "list_marker_font_context" {
                p.add_run(Run::text("b", CharShapeIndex::new(1)));
            }
            let mut paragraphs = vec![p];
            if kind == "list_marker_conflict" {
                paragraphs.push(Paragraph::with_runs(
                    vec![Run::text("c", CharShapeIndex::new(0))],
                    ParaShapeIndex::new(1),
                ));
            }
            let mut d = Document::new();
            d.add_section(Section::with_paragraphs(paragraphs, PageSettings::a4()));
            let out = HwpxEncoder::encode_with_diagnostics(
                &d.validate().unwrap(),
                &s,
                &ImageStore::new(),
                EncodeOptions::default(),
            )
            .unwrap();
            assert!(
                out.warnings
                    .iter()
                    .any(|w| matches!(w,EncodeWarning::ContentLost{kind:k,..} if k==kind)),
                "{kind}"
            );
            assert!(crate::encoder::split_successful_encode(out).is_err(), "{kind}");
        }
    }
}
