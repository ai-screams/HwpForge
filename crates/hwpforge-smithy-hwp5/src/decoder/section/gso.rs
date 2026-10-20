//! GSO (그리기 개체) builders for the HWP5 `BodyText` decoder.
//!
//! Cohesive cluster of paragraph-local GSO collection contexts and group
//! builders (`GsoChildBuilder`, `GsoGroupBuilder`) that
//! accumulate shape-component evidence and finalize it into model controls.
//! Paragraph text delegates to the body control interpreter. The shared `classify_gso_control` finalizer and the `CTRL_ID_GSO` /
//! `GSO_GROUP_MAX_DEPTH` / `SHAPE_COMPONENT_TYPE_GROUP` items stay in the parent
//! module and are reached via `super::`.

use crate::schema::record::{Record, TagId};
use crate::schema::section::{
    Hwp5ShapeComponentCurve, Hwp5ShapeComponentEllipse, Hwp5ShapeComponentGeometry,
    Hwp5ShapeComponentLine, Hwp5ShapeComponentOle, Hwp5ShapeComponentPolygon, Hwp5ShapePicture,
    Hwp5ShapeRender, Hwp5ShapeTextArt,
};

use super::{
    classify_gso_control, BodyTextParserState, Hwp5Control, Hwp5GroupChild, Hwp5GroupControl,
    Hwp5Warning, CTRL_ID_GSO, GSO_GROUP_MAX_DEPTH, SHAPE_COMPONENT_TYPE_GROUP,
};

/// The property word of a shape's text `LIST_HEADER` (§19.22 D4): a u32
/// paragraph count, then the property word at bytes 4..8 — not the cell
/// layout (u16 count) the three readers used before, which put the vertical
/// align two bytes off. Every shape text list in the corpus is 20, 28, 33 or
/// 45 bytes long (2,139 records); in the Hancom twin pairs the 33- and
/// 45-byte forms match `vertAlign` 348/348. `None` when shorter than 8 bytes.
///
/// 이것을 실패시키는 것: 오프셋을 2..6 으로(`shape_text_list_header_reads_its_own_layout`).
pub(super) fn shape_text_list_properties(data: &[u8]) -> Option<u32> {
    data.get(4..8).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// The inner margin of a shape's text `LIST_HEADER` (§19.22 D4): bytes
/// 8..16, left, right, top, bottom (Hancom twins match `textMargin` in all
/// 348 text boxes). `None` when shorter than 16 bytes.
///
/// 이것을 실패시키는 것: 오프셋을 바꾸기(`text_margins_are_read`).
pub(super) fn shape_text_list_margin(data: &[u8]) -> Option<super::Hwp5TableCellMargin> {
    super::Hwp5TableCellMargin::read(data, 8)
}

/// The warning for a shape text `LIST_HEADER` too short for its property
/// word or its margin (none in the corpus): what is missing keeps its
/// default (top alignment, 283 margin).
pub(super) fn short_list_header_warning(len: usize) -> Hwp5Warning {
    Hwp5Warning::ProjectionFallback {
        subject: "textbox.list_header_short",
        reason: format!(
            "shape text list header of {len} bytes; its properties or margin are not read"
        ),
    }
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
    /// The shape's outline and fill, shared by standalone and group projection.
    pub(super) look: Option<crate::schema::section::Hwp5ShapeLook>,
    /// The shape's text paragraphs and its `LIST_HEADER` word (single
    /// shapes; empty for group children, whose text the group projection
    /// decides). An ellipse or polygon keeps them; a rectangle with text is
    /// a text box before classification.
    pub(super) paragraphs: Vec<super::Hwp5Paragraph>,
    pub(super) list_header_properties: Option<u32>,
    pub(super) text_margin: Option<crate::decoder::section::Hwp5TableCellMargin>,
    /// The initial box of the scope's first `ShapeComponent` (single
    /// shapes; `None` for group children).
    pub(super) original_size: Option<(u32, u32)>,
    pub(super) standalone_render: Option<Result<Hwp5ShapeRender, String>>,
}

/// One child shape being collected inside a [`GsoGroupBuilder`].
///
/// Mirrors the single-shape slot set of
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
    text_margin: Option<crate::decoder::section::Hwp5TableCellMargin>,
    geometry: Option<Hwp5ShapeComponentGeometry>,
    picture: Option<Hwp5ShapePicture>,
    ole: Option<Hwp5ShapeComponentOle>,
    line: Option<Hwp5ShapeComponentLine>,
    polygon: Option<Hwp5ShapeComponentPolygon>,
    ellipse: Option<Hwp5ShapeComponentEllipse>,
    curve: Option<Hwp5ShapeComponentCurve>,
    text_art: Option<Hwp5ShapeTextArt>,
    shape_component_kind: Option<[u8; 4]>,
    text_parser: Option<Box<BodyTextParserState>>,
    render: Result<Hwp5ShapeRender, String>,
    /// Control ids of `CtrlHeader` records under this child.
    nested_controls: Vec<u32>,
    look: Option<crate::schema::section::Hwp5ShapeLook>,
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
            text_margin: None,
            geometry,
            picture: None,
            ole: None,
            line: None,
            polygon: None,
            ellipse: None,
            curve: None,
            text_art: None,
            shape_component_kind,
            text_parser: None,
            render,
            nested_controls: Vec::new(),
            link: None,
            look: None,
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
    fn into_child(mut self, warnings: &mut Vec<Hwp5Warning>) -> Hwp5GroupChild {
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
                text_margin: None,
                render: self.render,
                nested_controls: self.nested_controls,
            };
        }

        let paragraphs = self
            .text_parser
            .take()
            .map(|parser| {
                let result = parser.finish();
                warnings.extend(result.warnings);
                result.paragraphs
            })
            .unwrap_or_default();
        let control = classify_gso_control(GsoClassificationInput {
            standalone_render: None,
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
            look: self.look,
            paragraphs: Vec::new(),
            list_header_properties: None,
            text_margin: None,
            original_size: None,
        });
        Hwp5GroupChild {
            control,
            paragraphs,
            list_header_properties: self.list_header_properties,
            text_margin: self.text_margin,
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
    pub(super) wire_sequence: std::rc::Rc<std::cell::Cell<usize>>,
    /// Level of the `$con` `ShapeComponent` (`0x4C`) that opened this group.
    comp_depth: u16,
    /// Cumulative path budget inherited from the owning control/parser.
    budget: super::NestingBudget,
    #[cfg(test)]
    pub(super) work: std::rc::Rc<std::cell::Cell<usize>>,
    #[cfg(test)]
    pub(super) record_identity: std::rc::Rc<std::cell::Cell<*const Record>>,
    geometry: Hwp5ShapeComponentGeometry,
    instance_id: u32,
    /// `CtrlHeader` object properties (top group only; `0` when nested).
    ctrl_properties: u32,
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
                    text_margin: None,
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
        ctrl_properties: u32,
        render: Result<Hwp5ShapeRender, String>,
        budget: super::NestingBudget,
    ) -> Self {
        Self {
            comp_depth,
            budget,
            wire_sequence: Default::default(),
            #[cfg(test)]
            work: Default::default(),
            #[cfg(test)]
            record_identity: Default::default(),
            geometry,
            instance_id,
            ctrl_properties,
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
        #[cfg(test)]
        {
            self.work.set(self.work.get() + 1);
            let expected = self.record_identity.get();
            assert!(expected.is_null() || std::ptr::eq(expected, record), "record was cloned");
        }
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
                if self.budget.enter(false, true).is_none() {
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
                    let mut nested = GsoGroupBuilder::new(
                        level,
                        bbox,
                        0,
                        0,
                        render,
                        self.budget.enter(false, true).expect("budget checked above"),
                    );
                    nested.wire_sequence = self.wire_sequence.clone();
                    #[cfg(test)]
                    let nested = GsoGroupBuilder {
                        work: self.work.clone(),
                        record_identity: self.record_identity.clone(),
                        ..nested
                    };
                    self.current_child = Some(GsoActiveChild::Nested(Box::new(nested)));
                }
            } else {
                let mut child = GsoChildBuilder::new(level, kind, geometry, render);
                child.look = Hwp5ShapeRender::parse_look(&record.data, false);
                self.current_child = Some(GsoActiveChild::Leaf(Box::new(child)));
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
        // Decode the complete text subtree with the body parser and an
        // absolute paragraph base. Borrow the original record at every depth.
        // It owns paragraph/control order and table/cell boundaries; shape
        // records at the child's own level stay here.
        let own = level == child.comp_depth.saturating_add(1);
        if child.saw_list_header
            && matches!(tag, TagId::ParaHeader)
            && own
            && child.text_parser.is_none()
        {
            let Some(budget) = self.budget.enter(false, false) else {
                warnings.push(Hwp5Warning::DroppedControl {
                    control: "group_child_text",
                    reason: "shared container depth cap exceeded".to_string(),
                });
                return;
            };
            child.text_parser = Some(Box::new(BodyTextParserState {
                wire_sequence: self.wire_sequence.clone(),
                base_level: child.comp_depth + 1,
                budget,
                active_budget: budget,
                nested: true,
                #[cfg(test)]
                work: self.work.clone(),
                #[cfg(test)]
                record_identity: self.record_identity.clone(),
                ..BodyTextParserState::default()
            }));
        }
        if (own && matches!(tag, TagId::ParaHeader)) || level > child.comp_depth.saturating_add(1) {
            if let Some(parser) = child.text_parser.as_mut() {
                parser.handle_record(record);
                return;
            }
        }
        match tag {
            TagId::ListHeader if own => {
                child.saw_list_header = true;
                child.list_header_properties = shape_text_list_properties(&record.data);
                child.text_margin = shape_text_list_margin(&record.data);
                if child.text_margin.is_none() {
                    warnings.push(short_list_header_warning(record.data.len()));
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
            ctrl_properties: self.ctrl_properties,
        })
    }
}
