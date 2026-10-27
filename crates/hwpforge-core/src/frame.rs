//! An object's box: its original size and whether that size is locked.
//!
//! Every drawing object (the shape [`Control`](crate::control::Control)
//! variants, [`Image`](crate::image::Image)) and every
//! [`Table`](crate::table::Table) carries one [`ObjectFrame`] next to its
//! `width` and `height`.
//!
//! | Item | Meaning |
//! | -- | -- |
//! | `width` × `height` | the displayed size, as before. Layout, PDF and Markdown read only this |
//! | original box `O` | [`ObjectFrame::original_size`], or the kind's default when `None` (the displayed size; a TextArt's 14173 × 14173 design box) |
//! | defining points | a shape's points (polygon vertices, line ends, ellipse centre and axes, …) are coordinates **in `O`** |
//! | drawn shape | each point scaled by `width / O.width` and `height / O.height`, then turned and mirrored about the centre of the displayed box ([`geometry`](crate::control::geometry)) |
//!
//! This is how Hancom stores an object (HWPX `<hp:orgSz>` + `<hp:sz>` with the
//! `scaMatrix` between them; HWP 5.0 the shape component's initial and current
//! size): resizing an object keeps its original box and changes only the
//! scale. An object without a frame value is drawn exactly as before — its
//! points are in its displayed box, scale 1.
//!
//! # Examples
//!
//! ```
//! use hwpforge_core::frame::ObjectFrame;
//! use hwpforge_foundation::{HwpUnit, Size};
//!
//! let unit = |v| HwpUnit::new(v).unwrap();
//! let frame = ObjectFrame::new().with_original_size(Size::new(unit(560), unit(560)));
//! assert_eq!(frame.original_size, Some(Size::new(unit(560), unit(560))));
//! assert!(!frame.size_protect);
//! assert!(ObjectFrame::new().is_unset());
//! ```

use hwpforge_foundation::Size;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The side of a TextArt's design box (HWPUNIT): a TextArt without an
/// original size has the original box `14173 × 14173`, the box Hancom draws
/// its glyph outlines in.
pub const TEXT_ART_DESIGN_SIDE: i32 = 14173;

/// An object's original (unscaled) box and its size lock.
///
/// The default — no original size, not locked — is what every object had
/// before this type existed, and what JSON without a `frame` reads as.
///
/// The struct is `#[non_exhaustive]`: build one with [`ObjectFrame::new`] and
/// the `with_*` builders; fields stay public for reading and in-place edits.
///
/// # Examples
///
/// ```
/// use hwpforge_core::frame::ObjectFrame;
///
/// let locked = ObjectFrame::new().with_size_protect(true);
/// assert!(locked.size_protect);
/// assert!(!locked.is_unset());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub struct ObjectFrame {
    /// The object's original box (HWPX `<hp:orgSz>`, HWP 5.0 shape component
    /// initial width and height). A shape's defining points are coordinates
    /// in this box. `None` = the kind's default: the displayed size, or for a
    /// TextArt its 14173 × 14173 design box.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_size: Option<Size>,
    /// The size is locked against resizing in Hancom's editor (HWPX
    /// `<hp:sz protect="1">`, HWP 5.0 object common property bit 20). Core
    /// edits are not restricted by it; it is carried as is.
    #[serde(default)]
    pub size_protect: bool,
}

impl ObjectFrame {
    /// The default frame: no original size, not locked.
    #[must_use]
    pub const fn new() -> Self {
        Self { original_size: None, size_protect: false }
    }

    /// The frame with `original_size` set.
    #[must_use]
    pub const fn with_original_size(mut self, original_size: Size) -> Self {
        self.original_size = Some(original_size);
        self
    }

    /// The frame with `size_protect` set.
    #[must_use]
    pub const fn with_size_protect(mut self, size_protect: bool) -> Self {
        self.size_protect = size_protect;
        self
    }

    /// Whether this is the default frame (written as nothing in JSON).
    #[must_use]
    pub fn is_unset(&self) -> bool {
        *self == Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_foundation::HwpUnit;

    fn size(w: i32, h: i32) -> Size {
        Size::new(HwpUnit::new(w).unwrap(), HwpUnit::new(h).unwrap())
    }

    /// The JSON forms: an absent original size is not written, and an
    /// empty object reads as the default.
    /// 이것을 실패시키는 것: `original_size` 의 `skip_serializing_if` 를
    /// 지우기(`"original_size":null` 이 써진다) / `default` 를 지우기(`{}` 을
    /// 못 읽는다).
    #[test]
    fn frame_json_forms() {
        assert_eq!(
            serde_json::to_string(&ObjectFrame::new()).unwrap(),
            r#"{"size_protect":false}"#
        );
        assert_eq!(serde_json::from_str::<ObjectFrame>("{}").unwrap(), ObjectFrame::new());
        let locked = ObjectFrame::new().with_size_protect(true);
        assert_eq!(serde_json::to_string(&locked).unwrap(), r#"{"size_protect":true}"#);
        let both = locked.with_original_size(size(560, 280));
        let json = serde_json::to_string(&both).unwrap();
        assert_eq!(json, r#"{"original_size":{"width":560,"height":280},"size_protect":true}"#);
        assert_eq!(serde_json::from_str::<ObjectFrame>(&json).unwrap(), both);
    }

    /// Hancom's resize: the frame-less circle of the r1 counterexample —
    /// width 850, centre 425 — resized to 1700 keeps its original box 850,
    /// so the centre is drawn at 425 × 1700 / 850 = 850.
    /// 이것을 실패시키는 것: 접기를 크기 바꾸기 전 상태 기준으로 하기(원래 크기가
    /// `None` 으로 남아 중심이 425 에 그려진다).
    #[test]
    fn resize_keeps_the_original_box() {
        use crate::control::{Control, ShapePoint};
        let u = |v| HwpUnit::new(v).unwrap();
        let mut circle = Control::ellipse(u(850), u(850));
        assert!(circle.resize(u(1700), u(1700)));
        let Control::Ellipse { center, width, frame, .. } = &circle else { unreachable!() };
        assert_eq!(*center, ShapePoint { x: 425, y: 425 });
        let o = frame.original_size.unwrap();
        let drawn = f64::from(center.x) * f64::from(width.as_i32()) / f64::from(o.width.as_i32());
        assert_eq!(drawn, 850.0);
        // A second resize keeps the first original box.
        assert!(circle.resize(u(425), u(425)));
        assert_eq!(circle.effective_original_size(), Some(size(850, 850)));
        // Back to the original size: the frame is the default again.
        assert!(circle.resize(u(850), u(850)));
        assert!(circle.frame().unwrap().is_unset());
    }

    /// A TextArt's default original box is its fixed design box, so a resize
    /// leaves no frame; a group and a control without a frame are not
    /// resized. 이것을 실패시키는 것: 글맵시 기본값을 현재 크기로 하기(크기를
    /// 바꾼 글맵시에 frame 이 생긴다) / 묶음 거부를 지우기.
    #[test]
    fn resize_by_kind() {
        use crate::control::Control;
        let u = |v| HwpUnit::new(v).unwrap();
        let mut art = Control::TextArt {
            text: "가".into(),
            shape: "WAVE2".into(),
            font_name: String::new(),
            font_style: String::new(),
            align: "LEFT".into(),
            line_spacing: 120,
            char_spacing: 100,
            width: u(5000),
            height: u(3000),
            frame: ObjectFrame::new(),
            placement: None,
            fill_color: None,
            inst_id: None,
        };
        assert_eq!(art.effective_original_size(), Some(size(14173, 14173)));
        assert!(art.resize(u(9000), u(4000)));
        assert!(art.frame().unwrap().is_unset());
        let mut group = Control::Group {
            children: Vec::new(),
            width: u(100),
            height: u(100),
            frame: ObjectFrame::new(),
            placement: None,
            inst_id: None,
        };
        assert!(!group.resize(u(200), u(200)));
        assert_eq!(group.frame().unwrap().original_size, None);
        assert!(!Control::bookmark("x").resize(u(1), u(1)));
    }

    /// 이것을 실패시키는 것: 그림 resize 의 비교를 바꾸기 전 크기로 하기.
    #[test]
    fn picture_resize_keeps_the_original_box() {
        use crate::image::{Image, ImageFormat};
        let u = |v| HwpUnit::new(v).unwrap();
        let mut img = Image::new("BinData/a.png", u(1000), u(500), ImageFormat::Png);
        img.resize(u(2000), u(1000));
        assert_eq!(img.frame.original_size, Some(size(1000, 500)));
        img.resize(u(1000), u(500));
        assert_eq!(img.frame.original_size, None);
    }

    /// A control's frame in JSON: absent when unset (old JSON reads and is
    /// written the same), present otherwise.
    /// 이것을 실패시키는 것: variant 필드의 `skip_serializing_if` 를 지우기.
    #[test]
    fn control_frame_json() {
        use crate::control::Control;
        let u = |v| HwpUnit::new(v).unwrap();
        let plain = Control::ellipse(u(850), u(850));
        let json = serde_json::to_string(&plain).unwrap();
        assert!(!json.contains("frame"), "{json}");
        assert_eq!(serde_json::from_str::<Control>(&json).unwrap(), plain);
        let mut framed = plain.clone();
        framed.frame_mut().unwrap().original_size = Some(size(560, 560));
        let json = serde_json::to_string(&framed).unwrap();
        assert!(json.contains(r#""frame":{"original_size":{"width":560,"height":560}"#), "{json}");
        assert_eq!(serde_json::from_str::<Control>(&json).unwrap(), framed);
    }

    /// 이것을 실패시키는 것: `is_unset` 이 `size_protect` 를 보지 않기(잠금만
    /// 있는 frame 이 JSON 에서 사라진다).
    #[test]
    fn a_lock_alone_is_not_unset() {
        assert!(ObjectFrame::new().is_unset());
        assert!(!ObjectFrame::new().with_size_protect(true).is_unset());
        assert!(!ObjectFrame::new().with_original_size(size(1, 1)).is_unset());
    }
}
