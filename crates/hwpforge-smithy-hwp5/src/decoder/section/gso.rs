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
    Hwp5ShapeRender, Hwp5ShapeTextArt,
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
    /// W2p: GSO CtrlHeader `data[4..8]` 공통 개체 속성 DWORD (표 70).
    /// bit0 = 글자처럼 취급 — `Hwp5ImageControl::ctrl_properties` 로 carry.
    ctrl_properties: u32,
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
    /// Level of the scope's first `ShapeComponent`.
    comp_level: Option<u16>,
    /// The shape's hyperlink (`CtrlData` one level below its component).
    link: Option<Result<String, String>>,
    /// Set when the scope's first `ShapeComponent` is a group (`$con`):
    /// every deeper record then belongs to the group, as in body paragraphs.
    group: Option<Box<GsoGroupBuilder>>,
    /// The shape has a text list (`LIST_HEADER` one level below its
    /// component) — with a rectangle it is a text box, as in the body.
    saw_list_header: bool,
    /// `속성` word of that `LIST_HEADER` (bits 5–6 = text vertical align).
    list_header_properties: Option<u32>,
    /// The shape's text paragraphs (§19.5 1a — before, a shape in a table
    /// cell or in a text box lost its text).
    paragraphs: Vec<Hwp5Paragraph>,
    current_para: Option<super::ParaBuf>,
    /// A shape inside this shape's text, collected the same way.
    inner: Option<Box<InlineGsoContext>>,
    /// Outline and fill from the scope's first `ShapeComponent`.
    look: Option<crate::schema::section::Hwp5ShapeLook>,
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
    /// W2p: GSO CtrlHeader `data[4..8]` 공통 개체 속성 DWORD (표 70).
    /// bit0 = 글자처럼 취급 — `Hwp5ImageControl::ctrl_properties` 로 carry.
    pub(super) ctrl_properties: u32,
    /// The shape's hyperlink (a picture keeps it).
    pub(super) link: Option<Result<String, String>>,
    /// The shape's outline and fill (single shapes; `None` for group
    /// children).
    pub(super) look: Option<crate::schema::section::Hwp5ShapeLook>,
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
    render: Result<Hwp5ShapeRender, String>,
    /// Control ids of `CtrlHeader` records under this child.
    nested_controls: Vec<u32>,
    /// The child's hyperlink (its own `CtrlData`).
    link: Option<Result<String, String>>,
}

impl GsoChildBuilder {
    fn new(
        comp_depth: u16,
        shape_component_kind: Option<[u8; 4]>,
        geometry: Option<Hwp5ShapeComponentGeometry>,
        render: Result<Hwp5ShapeRender, String>,
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
            render,
            nested_controls: Vec::new(),
            link: None,
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
                render: self.render,
                nested_controls: self.nested_controls,
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
            // 그룹 자식의 개체 속성은 그룹 배치(W5)가 지배 — 자식 단독
            // treat_as_char 는 W5 에서 해석.
            ctrl_properties: 0,
            link: self.link,
            look: None,
        });
        Hwp5GroupChild {
            control,
            paragraphs: self.paragraphs,
            list_header_properties: self.list_header_properties,
            render: self.render,
            nested_controls: self.nested_controls,
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
    render: Result<Hwp5ShapeRender, String>,
    /// The group has a hyperlink of its own (a `CtrlData` record).
    has_link: bool,
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
            Self::Nested(b) => {
                let render = b.render.clone();
                Hwp5GroupChild {
                    control: b.into_control(warnings),
                    paragraphs: Vec::new(),
                    list_header_properties: None,
                    render,
                    nested_controls: Vec::new(),
                }
            }
        }
    }
}

impl GsoGroupBuilder {
    pub(super) fn new(
        comp_depth: u16,
        geometry: Hwp5ShapeComponentGeometry,
        instance_id: u32,
        render: Result<Hwp5ShapeRender, String>,
    ) -> Self {
        Self {
            comp_depth,
            depth: 1,
            geometry,
            instance_id,
            children: Vec::new(),
            current_child: None,
            render,
            has_link: false,
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
        if matches!(tag, TagId::CtrlData) && level == self.comp_depth.saturating_add(1) {
            self.has_link = true;
            return;
        }
        if matches!(tag, TagId::ShapeComponent) && level == self.comp_depth.saturating_add(1) {
            let kind = record.data.get(..4).map(|c| [c[0], c[1], c[2], c[3]]);
            // Parse the child's group-relative geometry from its own
            // ShapeComponent common header (NOT the gso CtrlHeader — children
            // have no per-shape CtrlHeader). Without it, classify_gso_control
            // drops the child to Unknown.
            let geometry =
                Hwp5ShapeComponentGeometry::parse_from_shape_component(&record.data).ok();
            // A component below the `gso` header writes its type tag once.
            let render = Hwp5ShapeRender::parse(&record.data, false).map_err(|e| e.to_string());

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
                        GsoChildBuilder::new(level, kind, geometry, render),
                    )));
                } else {
                    // Nested group: its bounding box is the child geometry
                    // (fall back to the parent bbox if unrecoverable); it has
                    // no per-child CtrlHeader so instance_id = 0.
                    let bbox = geometry.unwrap_or_else(|| self.geometry.clone());
                    let mut nested = GsoGroupBuilder::new(level, bbox, 0, render);
                    nested.depth = self.depth.saturating_add(1);
                    self.current_child = Some(GsoActiveChild::Nested(Box::new(nested)));
                }
            } else {
                self.current_child = Some(GsoActiveChild::Leaf(Box::new(GsoChildBuilder::new(
                    level, kind, geometry, render,
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
        // The child's own shape records and its text list sit one level
        // below its `ShapeComponent`, its paragraphs' records two levels
        // below. Anything deeper belongs to an object inside its text (a
        // table and its cells, a picture): not the child's own shape or
        // text — that object is reported through `nested_controls`.
        let own = level == child.comp_depth.saturating_add(1);
        let in_paragraph = level == child.comp_depth.saturating_add(2);
        match tag {
            TagId::ListHeader if own => {
                child.saw_list_header = true;
                if let Some(bytes) = record.data.get(2..6) {
                    if let Ok(arr) = <[u8; 4]>::try_from(bytes) {
                        child.list_header_properties = Some(u32::from_le_bytes(arr));
                    }
                }
            }
            TagId::CtrlHeader if in_paragraph => {
                if let Some(id) = record.data.get(..4) {
                    child.nested_controls.push(u32::from_le_bytes([id[0], id[1], id[2], id[3]]));
                }
            }
            TagId::ShapeComponentRect if own => child.saw_shape_rectangle = true,
            TagId::ShapeComponentLine if own => match Hwp5ShapeComponentLine::parse(&record.data) {
                Ok(line) => child.line = Some(line),
                Err(_) => warnings
                    .push(Hwp5Warning::UnsupportedTag { tag_id: record.header.tag_id, offset: 0 }),
            },
            TagId::ShapeComponentPolygon if own => {
                match Hwp5ShapeComponentPolygon::parse(&record.data) {
                    Ok(polygon) => child.polygon = Some(polygon),
                    Err(_) => warnings.push(Hwp5Warning::UnsupportedTag {
                        tag_id: record.header.tag_id,
                        offset: 0,
                    }),
                }
            }
            TagId::ShapeComponentEllipse if own => {
                match Hwp5ShapeComponentEllipse::parse(&record.data) {
                    Ok(ellipse) => child.ellipse = Some(ellipse),
                    Err(_) => warnings.push(Hwp5Warning::UnsupportedTag {
                        tag_id: record.header.tag_id,
                        offset: 0,
                    }),
                }
            }
            TagId::ShapeComponentCurve if own => match Hwp5ShapeComponentCurve::parse(&record.data)
            {
                Ok(curve) => child.curve = Some(curve),
                Err(_) => warnings
                    .push(Hwp5Warning::UnsupportedTag { tag_id: record.header.tag_id, offset: 0 }),
            },
            TagId::ShapeTextArt if own => {
                match crate::schema::section::Hwp5ShapeTextArt::parse(&record.data) {
                    Ok(ta) => child.text_art = Some(ta),
                    Err(_) => warnings.push(Hwp5Warning::UnsupportedTag {
                        tag_id: record.header.tag_id,
                        offset: 0,
                    }),
                }
            }
            TagId::CtrlData if own && child.link.is_none() => {
                child.link = Some(crate::schema::section::parse_object_link(&record.data));
            }
            TagId::ShapePicture if own => match Hwp5ShapePicture::parse(&record.data) {
                Ok(picture) => child.picture = Some(picture),
                Err(_) => warnings
                    .push(Hwp5Warning::UnsupportedTag { tag_id: record.header.tag_id, offset: 0 }),
            },
            TagId::ShapeComponentOle if own => match Hwp5ShapeComponentOle::parse(&record.data) {
                Ok(ole) => child.ole = Some(ole),
                Err(_) => warnings
                    .push(Hwp5Warning::UnsupportedTag { tag_id: record.header.tag_id, offset: 0 }),
            },
            TagId::ParaHeader if own => {
                if child.saw_list_header {
                    if let Some(buf) = BodyTextParserState::parse_para_header_buf(
                        record.header.tag_id,
                        &record.data,
                        warnings,
                    ) {
                        child.paragraphs.push(buf.finish(warnings));
                    }
                }
            }
            TagId::ParaText if in_paragraph => {
                if let Some(text) = BodyTextParserState::parse_para_text_value(
                    record.header.tag_id,
                    &record.data,
                    warnings,
                ) {
                    if let Some(last) = child.paragraphs.last_mut() {
                        last.text_segments = text.segments;
                        last.silent_wires = text.silent_wires;
                        last.text = segments_to_string(&last.text_segments);
                    }
                }
            }
            TagId::ParaCharShape if in_paragraph => {
                if let Some(runs) = BodyTextParserState::parse_para_char_shape_runs(
                    record.header.tag_id,
                    &record.data,
                    warnings,
                ) {
                    if let Some(last) = child.paragraphs.last_mut() {
                        last.char_shape_runs = super::char_shape_runs_to_text_positions(
                            &last.text_segments,
                            &last.silent_wires,
                            &runs,
                            warnings,
                        );
                    }
                }
            }
            TagId::ParaLineSeg if in_paragraph => {
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
            render: self.render,
            has_link: self.has_link,
        })
    }
}

impl InlineGsoContext {
    pub(super) fn new(
        ctrl_depth: u16,
        ctrl_id: u32,
        instance_id: u32,
        ctrl_properties: u32,
        geometry: Option<Hwp5ShapeComponentGeometry>,
    ) -> Self {
        Self {
            ctrl_depth,
            ctrl_id,
            instance_id,
            ctrl_properties,
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
            comp_level: None,
            link: None,
            group: None,
            saw_list_header: false,
            list_header_properties: None,
            paragraphs: Vec::new(),
            current_para: None,
            inner: None,
            look: None,
        }
    }

    /// Routes a record that belongs to this shape's text: its `LIST_HEADER`
    /// and paragraphs (one level below the component, their records two
    /// below), and a shape inside a paragraph, which opens an inner context
    /// that takes every record deeper than its control header — the body
    /// path's collection, one level down (§19.5 1a). Returns `true` when the
    /// record was taken. A record deeper than the text level that no inner
    /// shape takes (a table in the text) is taken and ignored, so it cannot
    /// be read as this shape's own.
    pub(super) fn route_text_record(
        &mut self,
        record: &Record,
        tag: TagId,
        level: u16,
        warnings: &mut Vec<Hwp5Warning>,
    ) -> bool {
        if self.inner.as_ref().is_some_and(|inner| level <= inner.ctrl_depth) {
            self.close_inner(warnings);
        }
        if let Some(inner) = self.inner.as_mut() {
            // 이것을 실패시키는 것: 안쪽 개체로 보내지 않기(안쪽 글상자 글이
            // 바깥 글상자 문단으로 섞임, `shape_text_in_a_cell_and_in_a_text_box`).
            if !inner.route_group_record(record, tag, level, warnings)
                && !inner.route_text_record(record, tag, level, warnings)
            {
                inner.note_shape_record(record, tag, warnings);
            }
            return true;
        }
        let Some(comp) = self.comp_level else {
            return false;
        };
        let own = level == comp.saturating_add(1);
        let in_paragraph = level == comp.saturating_add(2);
        match tag {
            TagId::ListHeader if own => {
                self.saw_list_header = true;
                if let Some(bytes) = record.data.get(2..6) {
                    if let Ok(arr) = <[u8; 4]>::try_from(bytes) {
                        self.list_header_properties = Some(u32::from_le_bytes(arr));
                    }
                }
                true
            }
            TagId::ParaHeader if own && self.saw_list_header => {
                if let Some(buf) = self.current_para.take() {
                    self.paragraphs.push(buf.finish(warnings));
                }
                self.current_para = BodyTextParserState::parse_para_header_buf(
                    record.header.tag_id,
                    &record.data,
                    warnings,
                );
                true
            }
            TagId::ParaText if in_paragraph => {
                if let (Some(buf), Some(text)) = (
                    self.current_para.as_mut(),
                    BodyTextParserState::parse_para_text_value(
                        record.header.tag_id,
                        &record.data,
                        warnings,
                    ),
                ) {
                    buf.text = Some(text);
                }
                true
            }
            TagId::ParaCharShape if in_paragraph => {
                if let (Some(buf), Some(runs)) = (
                    self.current_para.as_mut(),
                    BodyTextParserState::parse_para_char_shape_runs(
                        record.header.tag_id,
                        &record.data,
                        warnings,
                    ),
                ) {
                    buf.char_shape_runs = runs;
                }
                true
            }
            TagId::ParaLineSeg if in_paragraph => {
                if let (Some(buf), Some(segments)) = (
                    self.current_para.as_mut(),
                    BodyTextParserState::parse_para_line_segments(
                        record.header.tag_id,
                        &record.data,
                        warnings,
                    ),
                ) {
                    buf.line_segments = segments;
                }
                true
            }
            TagId::CtrlHeader if in_paragraph => {
                let ctrl_id = super::parse_ctrl_id(&record.data);
                if ctrl_id == CTRL_ID_GSO {
                    self.inner = Some(Box::new(InlineGsoContext::new(
                        level,
                        ctrl_id,
                        super::extract_ctrl_header_instance_id(&record.data, ctrl_id),
                        super::ctrl_header_properties_raw(&record.data),
                        Hwp5ShapeComponentGeometry::parse_from_ctrl_header(&record.data).ok(),
                    )));
                } else if let Some(buf) = self.current_para.as_mut() {
                    buf.controls.push(
                        BodyTextParserState::typed_inline_family_control(ctrl_id, &record.data)
                            .unwrap_or_else(|| Hwp5Control::Unknown {
                                ctrl_id,
                                header_data: record.data.clone(),
                            }),
                    );
                }
                true
            }
            _ => level > comp.saturating_add(1),
        }
    }

    /// Finishes the inner shape and attaches it to the current paragraph.
    fn close_inner(&mut self, warnings: &mut Vec<Hwp5Warning>) {
        if let Some(inner) = self.inner.take() {
            let control = inner.into_control(warnings);
            if let Some(buf) = self.current_para.as_mut() {
                buf.controls.push(control);
            }
        }
    }

    /// The shape's own records (component, shape bodies, link).
    pub(super) fn note_shape_record(
        &mut self,
        record: &Record,
        tag: TagId,
        warnings: &mut Vec<Hwp5Warning>,
    ) {
        let level = record.header.level;
        let unsupported = |warnings: &mut Vec<Hwp5Warning>| {
            warnings.push(Hwp5Warning::UnsupportedTag { tag_id: record.header.tag_id, offset: 0 })
        };
        match tag {
            TagId::ShapeComponent => self.note_shape_component(&record.data, level),
            TagId::CtrlData => self.note_ctrl_data(&record.data, level),
            TagId::ShapeComponentRect => self.note_shape_rectangle(),
            TagId::ShapeComponentLine => match Hwp5ShapeComponentLine::parse(&record.data) {
                Ok(line) => self.note_shape_line(line),
                Err(_) => unsupported(warnings),
            },
            TagId::ShapeComponentPolygon => match Hwp5ShapeComponentPolygon::parse(&record.data) {
                Ok(polygon) => self.note_shape_polygon(polygon),
                Err(_) => unsupported(warnings),
            },
            TagId::ShapeComponentEllipse => match Hwp5ShapeComponentEllipse::parse(&record.data) {
                Ok(ellipse) => self.note_shape_ellipse(ellipse),
                Err(_) => unsupported(warnings),
            },
            TagId::ShapeComponentCurve => match Hwp5ShapeComponentCurve::parse(&record.data) {
                Ok(curve) => self.note_shape_curve(curve),
                Err(_) => unsupported(warnings),
            },
            TagId::ShapeTextArt => match Hwp5ShapeTextArt::parse(&record.data) {
                Ok(ta) => self.note_shape_text_art(ta),
                Err(_) => unsupported(warnings),
            },
            TagId::ShapePicture => match Hwp5ShapePicture::parse(&record.data) {
                Ok(picture) => self.note_shape_picture(picture),
                Err(_) => unsupported(warnings),
            },
            TagId::ShapeComponentOle => match Hwp5ShapeComponentOle::parse(&record.data) {
                Ok(ole) => self.note_shape_ole(ole),
                Err(_) => unsupported(warnings),
            },
            TagId::Unknown(id) => {
                warnings.push(Hwp5Warning::UnsupportedTag { tag_id: id, offset: 0 });
            }
            _ => {}
        }
    }

    /// Routes a record into the group this scope holds, opening it on a
    /// `$con` `ShapeComponent` right under the control header (whose type tag
    /// is written twice). Returns `true` when the group took the record. A
    /// group without a recoverable control-header box cannot be placed; it is
    /// reported and left to the single-shape path, as in body paragraphs.
    pub(super) fn route_group_record(
        &mut self,
        record: &Record,
        tag: TagId,
        level: u16,
        warnings: &mut Vec<Hwp5Warning>,
    ) -> bool {
        if let Some(group) = self.group.as_mut() {
            group.handle_record(record, tag, level, warnings);
            return true;
        }
        let opens_group = matches!(tag, TagId::ShapeComponent)
            && !self.saw_shape_component
            && level == self.ctrl_depth.saturating_add(1)
            && record.data.get(..4) == Some(&SHAPE_COMPONENT_TYPE_GROUP);
        if !opens_group {
            return false;
        }
        let Some(geometry) = self.geometry.clone() else {
            warnings.push(Hwp5Warning::DroppedControl {
                control: "gso_group",
                reason: "group ($con) without recoverable bounding box; \
                         cannot place <hp:container>"
                    .to_string(),
            });
            return false;
        };
        let render = Hwp5ShapeRender::parse(&record.data, true).map_err(|e| e.to_string());
        self.saw_shape_component = true;
        self.shape_component_kind = Some(SHAPE_COMPONENT_TYPE_GROUP);
        self.group =
            Some(Box::new(GsoGroupBuilder::new(level, geometry, self.instance_id, render)));
        true
    }

    pub(super) fn note_shape_component(&mut self, data: &[u8], level: u16) {
        self.saw_shape_component = true;
        if self.comp_level.is_none() {
            // The component right under the control header writes its type
            // tag twice.
            self.look = crate::schema::section::Hwp5ShapeRender::parse_look(data, true);
        }
        self.comp_level.get_or_insert(level);
        if let Some(code) = data.get(..4) {
            self.shape_component_kind = Some([code[0], code[1], code[2], code[3]]);
        }
    }

    pub(super) fn note_shape_rectangle(&mut self) {
        self.saw_shape_rectangle = true;
    }

    /// A `CtrlData` record: the shape's hyperlink when it sits one level
    /// below the shape's component (deeper ones belong to its text).
    pub(super) fn note_ctrl_data(&mut self, data: &[u8], level: u16) {
        if self.link.is_none() && self.comp_level.is_some_and(|c| level == c.saturating_add(1)) {
            self.link = Some(crate::schema::section::parse_object_link(data));
        }
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

    pub(super) fn into_control(mut self, warnings: &mut Vec<Hwp5Warning>) -> Hwp5Control {
        if let Some(group) = self.group {
            return group.into_control(warnings);
        }
        self.close_inner(warnings);
        if let Some(buf) = self.current_para.take() {
            self.paragraphs.push(buf.finish(warnings));
        }
        // A rectangle with a text list is a text box, as in the body
        // (`NestedSubtreeContext::kind`). Other shapes' text is not carried
        // there either.
        // 이것을 실패시키는 것: 이 갈래를 지우기(칸 안 글상자가 글 없는 사각형으로).
        if self.saw_list_header && self.saw_shape_rectangle {
            if let Some(geometry) = self.geometry {
                return Hwp5Control::TextBox(super::Hwp5TextBoxControl {
                    look: self.look,
                    ctrl_id: self.ctrl_id,
                    geometry,
                    paragraphs: self.paragraphs,
                    list_header_properties: self.list_header_properties,
                    ctrl_properties: self.ctrl_properties,
                });
            }
        }
        classify_gso_control(GsoClassificationInput {
            ctrl_id: self.ctrl_id,
            saw_shape_component: self.saw_shape_component,
            saw_shape_rectangle: self.saw_shape_rectangle,
            instance_id: self.instance_id,
            ctrl_properties: self.ctrl_properties,
            geometry: self.geometry,
            picture: self.picture,
            ole: self.ole,
            line: self.line,
            polygon: self.polygon,
            ellipse: self.ellipse,
            curve: self.curve,
            text_art: self.text_art,
            shape_component_kind: self.shape_component_kind,
            link: self.link,
            look: self.look,
        })
    }
}
