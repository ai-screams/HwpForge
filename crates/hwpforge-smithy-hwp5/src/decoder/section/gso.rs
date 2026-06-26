//! GSO (그리기 개체) builders for the HWP5 `BodyText` decoder.
//!
//! Cohesive cluster of paragraph-local GSO collection contexts and group
//! builders (`InlineGsoContext`, `GsoChildBuilder`, `GsoGroupBuilder`, …) that
//! accumulate shape-component evidence and finalize it into model controls.
//! Split out of `decoder/section.rs` verbatim (E7 file split); no behavior
//! change. The shared `classify_gso_control` finalizer and the `CTRL_ID_GSO` /
//! `GSO_GROUP_MAX_DEPTH` / `SHAPE_COMPONENT_TYPE_GROUP` items stay in the parent
//! module and are reached via `super::`.

use crate::schema::record::{Record, TagId};
use crate::schema::section::{
    Hwp5ShapeComponentCurve, Hwp5ShapeComponentEllipse, Hwp5ShapeComponentGeometry,
    Hwp5ShapeComponentLine, Hwp5ShapeComponentOle, Hwp5ShapeComponentPolygon, Hwp5ShapePicture,
    Hwp5ShapeTextArt,
};

use super::{
    classify_gso_control, segments_to_string, BodyTextParserState, Hwp5Control, Hwp5GroupChild,
    Hwp5GroupControl, Hwp5Paragraph, Hwp5Warning, CTRL_ID_GSO, GSO_GROUP_MAX_DEPTH,
    SHAPE_COMPONENT_TYPE_GROUP,
};

/// Active paragraph-local `gso ` scope while collecting image evidence.
pub(super) struct InlineGsoContext {
    pub(super) ctrl_depth: u16,
    ctrl_id: u32,
    /// Wave 12p Step 1c-3: GSO CtrlHeader trailer instance ID,
    /// carried through to typed `Hwp5ImageControl` and friends.
    instance_id: u32,
    saw_shape_component: bool,
    saw_shape_rectangle: bool,
    geometry: Option<Hwp5ShapeComponentGeometry>,
    picture: Option<Hwp5ShapePicture>,
    ole: Option<Hwp5ShapeComponentOle>,
    line: Option<Hwp5ShapeComponentLine>,
    polygon: Option<Hwp5ShapeComponentPolygon>,
    ellipse: Option<Hwp5ShapeComponentEllipse>,
    curve: Option<Hwp5ShapeComponentCurve>,
    text_art: Option<Hwp5ShapeTextArt>,
    /// Leading 4-byte type tag from the `ShapeComponent` (`0x4C`) record.
    shape_component_kind: Option<[u8; 4]>,
}

pub(super) struct GsoClassificationInput {
    pub(super) ctrl_id: u32,
    pub(super) saw_shape_component: bool,
    pub(super) saw_shape_rectangle: bool,
    pub(super) geometry: Option<Hwp5ShapeComponentGeometry>,
    pub(super) picture: Option<Hwp5ShapePicture>,
    pub(super) ole: Option<Hwp5ShapeComponentOle>,
    pub(super) line: Option<Hwp5ShapeComponentLine>,
    pub(super) polygon: Option<Hwp5ShapeComponentPolygon>,
    pub(super) ellipse: Option<Hwp5ShapeComponentEllipse>,
    pub(super) curve: Option<Hwp5ShapeComponentCurve>,
    pub(super) text_art: Option<Hwp5ShapeTextArt>,
    /// Leading 4-byte type tag from the `ShapeComponent` (`0x4C`) record.
    pub(super) shape_component_kind: Option<[u8; 4]>,
    /// Wave 12p Step 1c-3: `gso ` CtrlHeader trailer instance ID,
    /// passed through to `Hwp5ImageControl` (and other typed GSO
    /// variants in the future).
    pub(super) instance_id: u32,
}

/// One child shape being collected inside a [`GsoGroupBuilder`].
///
/// Mirrors the single-shape slot set of [`InlineGsoContext`] /
/// [`NestedSubtreeContext`] (same `note_shape_*` mutators, same
/// `classify_gso_control` finalize path) plus a nested paragraph list for
/// children that carry `drawText` content (the native group fixture's rect
/// and ellipse both hold text). A child is finalized into a single
/// `Hwp5Control` when its scope closes.
pub(super) struct GsoChildBuilder {
    /// Record level of the child's own `ShapeComponent` (`0x4C`). The child
    /// stays open while later records sit deeper than this.
    comp_depth: u16,
    saw_shape_rectangle: bool,
    saw_list_header: bool,
    /// `속성` (UINT32) word of this child's `HWPTAG_LIST_HEADER` record (표 65),
    /// when present. Bits 5–6 carry text vertical alignment.
    list_header_properties: Option<u32>,
    geometry: Option<Hwp5ShapeComponentGeometry>,
    picture: Option<Hwp5ShapePicture>,
    ole: Option<Hwp5ShapeComponentOle>,
    line: Option<Hwp5ShapeComponentLine>,
    polygon: Option<Hwp5ShapeComponentPolygon>,
    ellipse: Option<Hwp5ShapeComponentEllipse>,
    curve: Option<Hwp5ShapeComponentCurve>,
    text_art: Option<Hwp5ShapeTextArt>,
    shape_component_kind: Option<[u8; 4]>,
    paragraphs: Vec<Hwp5Paragraph>,
}

impl GsoChildBuilder {
    fn new(
        comp_depth: u16,
        shape_component_kind: Option<[u8; 4]>,
        geometry: Option<Hwp5ShapeComponentGeometry>,
    ) -> Self {
        Self {
            comp_depth,
            saw_shape_rectangle: false,
            saw_list_header: false,
            list_header_properties: None,
            geometry,
            picture: None,
            ole: None,
            line: None,
            polygon: None,
            ellipse: None,
            curve: None,
            text_art: None,
            shape_component_kind,
            paragraphs: Vec::new(),
        }
    }

    /// Finalize this child into a [`Hwp5GroupChild`] (typed shape control +
    /// any `drawText` paragraphs it carried).
    ///
    /// A rect/ellipse/line/etc. is classified by the shared
    /// `classify_gso_control` path (no new shape logic). The child's
    /// `drawText` paragraphs ride alongside so the projection layer can build
    /// a text-bearing `Control::TextBox` (rects) or `ellipse_with_text`
    /// (ellipses). A nested `$con` (group inside a group) is normally recursed
    /// through [`GsoActiveChild::Nested`] (Wave B); this leaf path only sees a
    /// `$con` kind when the [`GSO_GROUP_MAX_DEPTH`] cap forced the nested group
    /// to degrade to a leaf, in which case it becomes `Unknown` with a warning.
    fn into_child(self, warnings: &mut Vec<Hwp5Warning>) -> Hwp5GroupChild {
        if self.shape_component_kind == Some(SHAPE_COMPONENT_TYPE_GROUP) {
            warnings.push(Hwp5Warning::DroppedControl {
                control: "gso_group",
                reason: "nested group ($con inside $con) exceeded GSO_GROUP_MAX_DEPTH; \
                         degrading over-cap nested group child to Unknown"
                    .to_string(),
            });
            return Hwp5GroupChild {
                control: Hwp5Control::Unknown { ctrl_id: CTRL_ID_GSO, header_data: Vec::new() },
                paragraphs: Vec::new(),
                list_header_properties: None,
            };
        }

        let control = classify_gso_control(GsoClassificationInput {
            ctrl_id: CTRL_ID_GSO,
            saw_shape_component: true,
            saw_shape_rectangle: self.saw_shape_rectangle,
            geometry: self.geometry,
            picture: self.picture,
            ole: self.ole,
            line: self.line,
            polygon: self.polygon,
            ellipse: self.ellipse,
            curve: self.curve,
            text_art: self.text_art,
            shape_component_kind: self.shape_component_kind,
            instance_id: 0,
        });
        Hwp5GroupChild {
            control,
            paragraphs: self.paragraphs,
            list_header_properties: self.list_header_properties,
        }
    }
}

/// Group (묶음 객체) builder activated when a `gso ` scope's first
/// `ShapeComponent` (`0x4C`) carries the `"$con"` type tag.
///
/// Models the scope-stack discipline of [`TableContext`]: a child opens on a
/// deeper `ShapeComponent` and closes when a record at or above its depth
/// arrives. Wave A keeps a single live child at a time (flat children only);
/// the [`GSO_GROUP_MAX_DEPTH`] cap is wired so Wave B's recursive `$con`
/// handling is a small lift.
pub(super) struct GsoGroupBuilder {
    /// Level of the `$con` `ShapeComponent` (`0x4C`) that opened this group.
    comp_depth: u16,
    /// `1`-based nesting depth used for the depth cap. The outermost group
    /// is depth `1`.
    depth: u16,
    geometry: Hwp5ShapeComponentGeometry,
    instance_id: u32,
    children: Vec<Hwp5GroupChild>,
    current_child: Option<GsoActiveChild>,
}

/// The live child of a [`GsoGroupBuilder`]: either a flat leaf shape or a
/// nested group (`$con`-in-`$con`, Wave B). Boxed on the `Nested` arm to
/// break the `GsoGroupBuilder` → `GsoActiveChild` → `GsoGroupBuilder` type
/// cycle.
pub(super) enum GsoActiveChild {
    /// A flat shape child (rect/ellipse/line/…). Boxed to keep the enum small
    /// (`GsoChildBuilder` is large; `clippy::large_enum_variant`).
    Leaf(Box<GsoChildBuilder>),
    /// A nested group child — recurses through its own `GsoGroupBuilder`.
    Nested(Box<GsoGroupBuilder>),
}

impl GsoActiveChild {
    /// The TLV level of the `ShapeComponent` that opened this child (used by
    /// the parent's close rule, identical for both variants).
    fn comp_depth(&self) -> u16 {
        match self {
            Self::Leaf(b) => b.comp_depth,
            Self::Nested(b) => b.comp_depth,
        }
    }

    /// Finalize this child into a [`Hwp5GroupChild`]. A leaf classifies via
    /// the shared `classify_gso_control`; a nested group recurses through
    /// `GsoGroupBuilder::into_control` to a `Hwp5Control::Group`.
    fn into_child(self, warnings: &mut Vec<Hwp5Warning>) -> Hwp5GroupChild {
        match self {
            Self::Leaf(b) => b.into_child(warnings),
            Self::Nested(b) => Hwp5GroupChild {
                control: b.into_control(warnings),
                paragraphs: Vec::new(),
                list_header_properties: None,
            },
        }
    }
}

impl GsoGroupBuilder {
    pub(super) fn new(
        comp_depth: u16,
        geometry: Hwp5ShapeComponentGeometry,
        instance_id: u32,
    ) -> Self {
        Self {
            comp_depth,
            depth: 1,
            geometry,
            instance_id,
            children: Vec::new(),
            current_child: None,
        }
    }

    /// Close the live child (if any) and append it to `children`.
    fn flush_current_child(&mut self, warnings: &mut Vec<Hwp5Warning>) {
        if let Some(child) = self.current_child.take() {
            self.children.push(child.into_child(warnings));
        }
    }

    /// Dispatch one subtree record into the group / current-child state.
    ///
    /// `level` is the record's TLV level. Returns nothing; warnings are
    /// surfaced for malformed sub-records and depth-cap exceed.
    pub(super) fn handle_record(
        &mut self,
        record: &Record,
        tag: TagId,
        level: u16,
        warnings: &mut Vec<Hwp5Warning>,
    ) {
        // Close the live child when a record arrives at or above its depth
        // (mirrors the `table_stack` close rule). A sibling `ShapeComponent`
        // at the child depth closes the prior child before opening the next.
        if self.current_child.as_ref().is_some_and(|c| level <= c.comp_depth()) {
            self.flush_current_child(warnings);
        }

        // A `ShapeComponent` (`0x4C`) one level below the `$con` opens a new
        // child. A nested `$con` child opens a recursive `GsoGroupBuilder`
        // (Wave B); any other shape opens a flat leaf builder.
        if matches!(tag, TagId::ShapeComponent) && level == self.comp_depth.saturating_add(1) {
            let kind = record.data.get(..4).map(|c| [c[0], c[1], c[2], c[3]]);
            // Parse the child's group-relative geometry from its own
            // ShapeComponent common header (NOT the gso CtrlHeader — children
            // have no per-shape CtrlHeader). Without it, classify_gso_control
            // drops the child to Unknown.
            let geometry =
                Hwp5ShapeComponentGeometry::parse_from_shape_component(&record.data).ok();

            if kind == Some(SHAPE_COMPONENT_TYPE_GROUP) {
                // Depth cap: a nested `$con` past the limit degrades to a leaf
                // (→ Unknown at finalize) instead of recursing — bounds both
                // recursion depth and malicious nesting.
                if self.depth.saturating_add(1) > GSO_GROUP_MAX_DEPTH {
                    warnings.push(Hwp5Warning::DroppedControl {
                        control: "gso_group",
                        reason: format!(
                            "group nesting exceeds depth cap {GSO_GROUP_MAX_DEPTH}; \
                             degrading deepest group to Unknown"
                        ),
                    });
                    self.current_child = Some(GsoActiveChild::Leaf(Box::new(
                        GsoChildBuilder::new(level, kind, geometry),
                    )));
                } else {
                    // Nested group: its bounding box is the child geometry
                    // (fall back to the parent bbox if unrecoverable); it has
                    // no per-child CtrlHeader so instance_id = 0.
                    let bbox = geometry.unwrap_or_else(|| self.geometry.clone());
                    let mut nested = GsoGroupBuilder::new(level, bbox, 0);
                    nested.depth = self.depth.saturating_add(1);
                    self.current_child = Some(GsoActiveChild::Nested(Box::new(nested)));
                }
            } else {
                self.current_child = Some(GsoActiveChild::Leaf(Box::new(GsoChildBuilder::new(
                    level, kind, geometry,
                ))));
            }
            return;
        }

        // Everything deeper belongs to the live child. A nested group recurses;
        // a leaf accumulates its shape sub-records.
        let child = match self.current_child.as_mut() {
            Some(GsoActiveChild::Nested(nested)) => {
                nested.handle_record(record, tag, level, warnings);
                return;
            }
            Some(GsoActiveChild::Leaf(leaf)) => leaf,
            None => return,
        };
        match tag {
            TagId::ListHeader => {
                child.saw_list_header = true;
                if let Some(bytes) = record.data.get(2..6) {
                    if let Ok(arr) = <[u8; 4]>::try_from(bytes) {
                        child.list_header_properties = Some(u32::from_le_bytes(arr));
                    }
                }
            }
            TagId::ShapeComponentRect => child.saw_shape_rectangle = true,
            TagId::ShapeComponentLine => match Hwp5ShapeComponentLine::parse(&record.data) {
                Ok(line) => child.line = Some(line),
                Err(_) => warnings
                    .push(Hwp5Warning::UnsupportedTag { tag_id: record.header.tag_id, offset: 0 }),
            },
            TagId::ShapeComponentPolygon => match Hwp5ShapeComponentPolygon::parse(&record.data) {
                Ok(polygon) => child.polygon = Some(polygon),
                Err(_) => warnings
                    .push(Hwp5Warning::UnsupportedTag { tag_id: record.header.tag_id, offset: 0 }),
            },
            TagId::ShapeComponentEllipse => match Hwp5ShapeComponentEllipse::parse(&record.data) {
                Ok(ellipse) => child.ellipse = Some(ellipse),
                Err(_) => warnings
                    .push(Hwp5Warning::UnsupportedTag { tag_id: record.header.tag_id, offset: 0 }),
            },
            TagId::ShapeComponentCurve => match Hwp5ShapeComponentCurve::parse(&record.data) {
                Ok(curve) => child.curve = Some(curve),
                Err(_) => warnings
                    .push(Hwp5Warning::UnsupportedTag { tag_id: record.header.tag_id, offset: 0 }),
            },
            TagId::ShapeTextArt => {
                match crate::schema::section::Hwp5ShapeTextArt::parse(&record.data) {
                    Ok(ta) => child.text_art = Some(ta),
                    Err(_) => warnings.push(Hwp5Warning::UnsupportedTag {
                        tag_id: record.header.tag_id,
                        offset: 0,
                    }),
                }
            }
            TagId::ShapePicture => match Hwp5ShapePicture::parse(&record.data) {
                Ok(picture) => child.picture = Some(picture),
                Err(_) => warnings
                    .push(Hwp5Warning::UnsupportedTag { tag_id: record.header.tag_id, offset: 0 }),
            },
            TagId::ShapeComponentOle => match Hwp5ShapeComponentOle::parse(&record.data) {
                Ok(ole) => child.ole = Some(ole),
                Err(_) => warnings
                    .push(Hwp5Warning::UnsupportedTag { tag_id: record.header.tag_id, offset: 0 }),
            },
            TagId::ParaHeader => {
                if child.saw_list_header {
                    if let Some(buf) = BodyTextParserState::parse_para_header_buf(
                        record.header.tag_id,
                        &record.data,
                        warnings,
                    ) {
                        child.paragraphs.push(buf.finish());
                    }
                }
            }
            TagId::ParaText => {
                if let Some(text) = BodyTextParserState::parse_para_text_value(
                    record.header.tag_id,
                    &record.data,
                    warnings,
                ) {
                    if let Some(last) = child.paragraphs.last_mut() {
                        last.text_segments = text.segments;
                        last.text = segments_to_string(&last.text_segments);
                    }
                }
            }
            TagId::ParaCharShape => {
                if let Some(runs) = BodyTextParserState::parse_para_char_shape_runs(
                    record.header.tag_id,
                    &record.data,
                    warnings,
                ) {
                    if let Some(last) = child.paragraphs.last_mut() {
                        last.char_shape_runs = runs;
                    }
                }
            }
            TagId::ParaLineSeg => {
                if let Some(segments) = BodyTextParserState::parse_para_line_segments(
                    record.header.tag_id,
                    &record.data,
                    warnings,
                ) {
                    if let Some(last) = child.paragraphs.last_mut() {
                        last.line_segments = segments;
                    }
                }
            }
            TagId::Unknown(id) => {
                warnings.push(Hwp5Warning::UnsupportedTag { tag_id: id, offset: 0 });
            }
            _ => {}
        }
    }

    /// Finalize the group into a `Hwp5Control::Group`, closing any open child.
    pub(super) fn into_control(mut self, warnings: &mut Vec<Hwp5Warning>) -> Hwp5Control {
        self.flush_current_child(warnings);
        Hwp5Control::Group(Hwp5GroupControl {
            ctrl_id: CTRL_ID_GSO,
            geometry: self.geometry,
            children: self.children,
            instance_id: self.instance_id,
        })
    }
}

impl InlineGsoContext {
    pub(super) fn new(
        ctrl_depth: u16,
        ctrl_id: u32,
        instance_id: u32,
        geometry: Option<Hwp5ShapeComponentGeometry>,
    ) -> Self {
        Self {
            ctrl_depth,
            ctrl_id,
            instance_id,
            saw_shape_component: false,
            saw_shape_rectangle: false,
            geometry,
            picture: None,
            ole: None,
            line: None,
            polygon: None,
            ellipse: None,
            curve: None,
            text_art: None,
            shape_component_kind: None,
        }
    }

    pub(super) fn note_shape_component(&mut self, data: &[u8]) {
        self.saw_shape_component = true;
        if let Some(code) = data.get(..4) {
            self.shape_component_kind = Some([code[0], code[1], code[2], code[3]]);
        }
    }

    pub(super) fn note_shape_rectangle(&mut self) {
        self.saw_shape_rectangle = true;
    }

    pub(super) fn note_shape_picture(&mut self, picture: Hwp5ShapePicture) {
        self.picture = Some(picture);
    }

    pub(super) fn note_shape_ole(&mut self, ole: Hwp5ShapeComponentOle) {
        self.ole = Some(ole);
    }

    pub(super) fn note_shape_line(&mut self, line: Hwp5ShapeComponentLine) {
        self.line = Some(line);
    }

    pub(super) fn note_shape_polygon(&mut self, polygon: Hwp5ShapeComponentPolygon) {
        self.polygon = Some(polygon);
    }

    pub(super) fn note_shape_ellipse(&mut self, ellipse: Hwp5ShapeComponentEllipse) {
        self.ellipse = Some(ellipse);
    }

    pub(super) fn note_shape_curve(&mut self, curve: Hwp5ShapeComponentCurve) {
        self.curve = Some(curve);
    }

    pub(super) fn note_shape_text_art(&mut self, ta: Hwp5ShapeTextArt) {
        self.text_art = Some(ta);
    }

    pub(super) fn into_control(self) -> Hwp5Control {
        classify_gso_control(GsoClassificationInput {
            ctrl_id: self.ctrl_id,
            saw_shape_component: self.saw_shape_component,
            saw_shape_rectangle: self.saw_shape_rectangle,
            instance_id: self.instance_id,
            geometry: self.geometry,
            picture: self.picture,
            ole: self.ole,
            line: self.line,
            polygon: self.polygon,
            ellipse: self.ellipse,
            curve: self.curve,
            text_art: self.text_art,
            shape_component_kind: self.shape_component_kind,
        })
    }
}
