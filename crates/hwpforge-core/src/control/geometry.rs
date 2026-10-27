//! Format-neutral geometry of drawing shapes.
//!
//! A shape's Core value — width, height, defining points, `ShapeStyle`
//! rotation and flip, and its placement offset — describes one exact
//! position on the page. This module states that meaning once, so every
//! codec reads and writes the same thing:
//!
//! | Item | Meaning |
//! | -- | -- |
//! | Coordinates | HWPUNIT, x to the right, y downward |
//! | Local box | `(0, 0)`–`(width, height)`; defining points live here |
//! | Local transform `L` | `L(p) = C + R(θ)·F·(p − C)`, `C` = box centre: mirror first, then rotate, both about the centre. The offset is not part of `L` |
//! | Placement | a group child sits at `offset + L(p)` in its **direct** parent group, the offset measured from that group's top-left corner. Outside a group the offset keeps its [`ObjectPlacement`] meaning |
//! | Rotation | `ShapeStyle::rotation` in degrees; positive turns clockwise on the page. No rounding beyond `f32` |
//! | Flip | `Horizontal` = diag(−1, 1), `Vertical` = diag(1, −1), `Both` = −I |
//!
//! This is the order DrawingML `xfrm` (`off`·`ext`·`rot`·`flipH/V`) uses,
//! and the one Hancom stores: a rectangle the user turned by 30° and then
//! mirrored left-right is saved as `angle=330` + horizontal flip with the
//! single matrix `R(330°)·diag(−1, 1)`.
//!
//! Two jobs build on it:
//!
//! - **Folding** ([`fold_group_child`], [`fold_box`]): a codec that stores a
//!   group child as a matrix chain composes that chain into one
//!   [`ShapeTransform`] and gets back the exact Core value. Folding never
//!   depends on an output format. What Core cannot hold exactly — a shear,
//!   an axis of size zero, a value outside the HWPUNIT range, a residual
//!   left by integer HWPUNIT storage, a rotation a variant has no field
//!   for — comes back as a [`FoldLoss`].
//! - **Emission** ([`local_transform`], [`emitted_corners`],
//!   [`emission_residual`]): a writer states what its format can store as an
//!   [`EmissionPolicy`] (angle rounding, coefficient digits, whether a mirror
//!   and a rotation fit in one matrix) and gets the matrix to write and how
//!   far that lands from the exact Core value.
//!
//! The composition order of a format's own matrix chain is not here — it
//! belongs to the codec that reads it.

use hwpforge_foundation::{Flip, HwpUnit};

use super::{Control, Fill, ShapePoint, ShapeStyle};
use crate::placement::{ObjectPlacement, ObjectRelativeTo, ObjectTextWrap};

/// Largest distance, in HWPUNIT, between where the source puts a point and
/// where the Core value puts it, still treated as exact. Hancom itself rounds
/// a mirrored group's axis to an even width, so its own "ungroup" result can
/// differ from the exact reflection by 1.23.
pub(crate) const EXACT_TOLERANCE: f64 = 2.0;

/// Largest |cos| between the two columns of a box's linear part that still
/// counts as "no shear".
pub(crate) const SHEAR_TOLERANCE: f64 = 1e-3;

// ── ShapeTransform ─────────────────────────────────────────────

/// A 2D affine map `x' = a·x + b·y + c`, `y' = d·x + e·y + f`.
///
/// Codecs build one from a stored matrix chain and hand it to
/// [`FoldInput::new`]; [`local_transform`] returns one for a writer.
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::geometry::ShapeTransform;
///
/// let scale = ShapeTransform::from_matrix([2.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
/// let shift = ShapeTransform::from_matrix([1.0, 0.0, 10.0, 0.0, 1.0, 0.0]);
/// // `shift.then(scale)` scales first, then shifts.
/// assert_eq!(shift.then(scale).apply((3.0, 1.0)), (16.0, 1.0));
/// assert_eq!(shift.matrix(), [1.0, 0.0, 10.0, 0.0, 1.0, 0.0]);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapeTransform {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl ShapeTransform {
    pub(crate) const IDENTITY: Self = Self { a: 1.0, b: 0.0, c: 0.0, d: 0.0, e: 1.0, f: 0.0 };

    /// The map with coefficients `[a, b, c, d, e, f]` (row-major, the
    /// translation in `c` and `f`).
    pub fn from_matrix(m: [f64; 6]) -> Self {
        Self { a: m[0], b: m[1], c: m[2], d: m[3], e: m[4], f: m[5] }
    }

    /// `self · inner`: applies `inner` first, then `self`.
    pub fn then(self, inner: Self) -> Self {
        Self {
            a: self.a * inner.a + self.b * inner.d,
            b: self.a * inner.b + self.b * inner.e,
            c: self.a * inner.c + self.b * inner.f + self.c,
            d: self.d * inner.a + self.e * inner.d,
            e: self.d * inner.b + self.e * inner.e,
            f: self.d * inner.c + self.e * inner.f + self.f,
        }
    }

    /// The coefficients `[a, b, c, d, e, f]`.
    pub fn matrix(&self) -> [f64; 6] {
        [self.a, self.b, self.c, self.d, self.e, self.f]
    }

    /// Maps a point.
    pub fn apply(&self, (x, y): (f64, f64)) -> (f64, f64) {
        (self.a * x + self.b * y + self.c, self.d * x + self.e * y + self.f)
    }

    fn translate(x: f64, y: f64) -> Self {
        Self { c: x, f: y, ..Self::IDENTITY }
    }

    fn is_finite(&self) -> bool {
        self.matrix().iter().all(|v| v.is_finite())
    }
}

// ── Emission policy ────────────────────────────────────────────

/// How a writer stores a rotation angle.
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::geometry::{AngleRounding, EmissionPolicy};
///
/// assert_eq!(EmissionPolicy::exact().angle, AngleRounding::Exact);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AngleRounding {
    /// The angle as Core holds it.
    Exact,
    /// Whole degrees: the angle is wrapped into `[0, 360)` and rounded half
    /// away from zero (359.6° becomes 360°, −0.4° becomes 360°).
    WholeDegrees,
}

/// How a writer stores matrix coefficients.
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::geometry::CoefficientRounding;
///
/// assert_ne!(CoefficientRounding::Exact, CoefficientRounding::Decimals(6));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CoefficientRounding {
    /// Full `f64` precision.
    Exact,
    /// Rounded to this many decimal places, the way `{:.N}` prints them.
    Decimals(u8),
}

/// What a writer's format can actually store for a shape transform.
///
/// The names describe storage limits, not a format: a writer builds its own
/// policy and keeps it next to its encoder.
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::geometry::{AngleRounding, CoefficientRounding, EmissionPolicy};
///
/// let policy =
///     EmissionPolicy::new(AngleRounding::WholeDegrees, CoefficientRounding::Decimals(6), true);
/// assert!(policy.mirror_with_rotation);
/// assert_eq!(EmissionPolicy::default(), EmissionPolicy::exact());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct EmissionPolicy {
    /// How the rotation angle is stored.
    pub angle: AngleRounding,
    /// How matrix coefficients are stored.
    pub coefficients: CoefficientRounding,
    /// Whether a mirror and a rotation can be stored together in one matrix.
    /// When `false`, a shape with both is written mirrored only.
    pub mirror_with_rotation: bool,
}

impl EmissionPolicy {
    /// A policy from its three limits.
    pub const fn new(
        angle: AngleRounding,
        coefficients: CoefficientRounding,
        mirror_with_rotation: bool,
    ) -> Self {
        Self { angle, coefficients, mirror_with_rotation }
    }

    /// Stores everything exactly: the Core definition itself.
    pub const fn exact() -> Self {
        Self::new(AngleRounding::Exact, CoefficientRounding::Exact, true)
    }
}

impl Default for EmissionPolicy {
    fn default() -> Self {
        Self::exact()
    }
}

/// Width, height, offset and rotation fields of a control with a box.
struct ShapeBox<'a> {
    width: i32,
    height: i32,
    offset: (i32, i32),
    style: Option<&'a ShapeStyle>,
}

fn placement_offset(placement: Option<&ObjectPlacement>) -> (i32, i32) {
    placement.map_or((0, 0), |p| (p.horz_offset.as_i32(), p.vert_offset.as_i32()))
}

fn shape_box(control: &Control) -> Option<ShapeBox<'_>> {
    let (width, height, placement, style) = match control {
        Control::TextBox { width, height, placement, style, .. }
        | Control::Rect { width, height, placement, style, .. }
        | Control::Ellipse { width, height, placement, style, .. }
        | Control::Arc { width, height, placement, style, .. }
        | Control::Polygon { width, height, placement, style, .. }
        | Control::Curve { width, height, placement, style, .. }
        | Control::ConnectLine { width, height, placement, style, .. }
        | Control::Line { width, height, placement, style, .. } => {
            (width, height, placement.as_ref(), style.as_ref())
        }
        Control::Group { width, height, placement, .. }
        | Control::TextArt { width, height, placement, .. }
        | Control::EmbeddedChart { width, height, placement, .. } => {
            (width, height, placement.as_ref(), None)
        }
        Control::Equation { width, height, .. } => (width, height, None, None),
        _ => return None,
    };
    Some(ShapeBox {
        width: width.as_i32(),
        height: height.as_i32(),
        offset: placement_offset(placement),
        style,
    })
}

/// The angle in degrees a policy stores for `rotation`. `0.0` = no rotation.
fn stored_angle(rotation: Option<f32>, rounding: AngleRounding) -> f64 {
    let Some(r) = rotation.filter(|r| r.is_finite()) else { return 0.0 };
    match rounding {
        AngleRounding::Exact => f64::from(r),
        // Wrap and round in f32, as a writer storing `f32` degrees as an
        // integer does; `as` saturates.
        AngleRounding::WholeDegrees => f64::from(r.rem_euclid(360.0).round() as i32),
    }
}

fn flip_signs(flip: Option<Flip>) -> (f64, f64) {
    match flip {
        Some(Flip::Horizontal) => (-1.0, 1.0),
        Some(Flip::Vertical) => (1.0, -1.0),
        Some(Flip::Both) => (-1.0, -1.0),
        _ => (1.0, 1.0),
    }
}

fn quantize(v: f64, rounding: CoefficientRounding) -> f64 {
    match rounding {
        CoefficientRounding::Exact => v,
        CoefficientRounding::Decimals(n) => {
            let n = usize::from(n);
            format!("{v:.n$}").parse().unwrap_or(v)
        }
    }
}

/// `L` for a `width × height` box under `policy`, and whether the policy had
/// to drop the rotation because it cannot store it with a mirror.
fn local_affine(
    width: f64,
    height: f64,
    rotation: Option<f32>,
    flip: Option<Flip>,
    policy: &EmissionPolicy,
) -> (ShapeTransform, bool) {
    let mut angle = stored_angle(rotation, policy.angle);
    let (mut fx, mut fy) = flip_signs(flip);
    if fx < 0.0 && fy < 0.0 && angle != 0.0 {
        // Mirrored both ways is −I, a half turn: with a rotation it is a
        // rotation 180° further and no mirror, which any policy can store.
        (fx, fy) = (1.0, 1.0);
        angle += 180.0;
    }
    let mirrored = fx < 0.0 || fy < 0.0;
    let unstorable = angle != 0.0 && mirrored && !policy.mirror_with_rotation;
    let angle = if unstorable { 0.0 } else { angle };
    let (cx, cy) = (width / 2.0, height / 2.0);
    let m = if angle == 0.0 {
        // Mirror alone keeps integer coefficients.
        [fx, 0.0, if fx < 0.0 { width } else { 0.0 }, 0.0, fy, if fy < 0.0 { height } else { 0.0 }]
    } else {
        let (sin, cos) = angle.to_radians().sin_cos();
        if mirrored {
            let (a, b, d, e) = (cos * fx, -sin * fy, sin * fx, cos * fy);
            [a, b, cx - (a * cx + b * cy), d, e, cy - (d * cx + e * cy)]
        } else {
            // Rotation about the centre, written the way writers have always
            // computed it so their bytes do not move.
            [cos, -sin, cx * (1.0 - cos) + cy * sin, sin, cos, cy * (1.0 - cos) - cx * sin]
        }
    };
    let q = |v: f64| quantize(v, policy.coefficients);
    (
        ShapeTransform::from_matrix([q(m[0]), q(m[1]), q(m[2]), q(m[3]), q(m[4]), q(m[5])]),
        unstorable,
    )
}

/// The local transform `L` of `control` under `policy` — the matrix a writer
/// stores for the shape's own rotation and mirror (its offset excluded).
///
/// With [`EmissionPolicy::exact`] this is the definition itself. Groups,
/// TextArt, charts and equations have no rotation field: their `L` is the
/// identity. `None` for controls without a box.
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::geometry::{local_transform, EmissionPolicy};
/// use hwpforge_core::control::{Control, ShapeStyle};
/// use hwpforge_foundation::{Flip, HwpUnit};
///
/// let mut rect = Control::rect(HwpUnit::new(100).unwrap(), HwpUnit::new(50).unwrap()).unwrap();
/// if let Control::Rect { style, .. } = &mut rect {
///     *style = Some(ShapeStyle { flip: Some(Flip::Horizontal), ..ShapeStyle::default() });
/// }
/// let l = local_transform(&rect, &EmissionPolicy::exact()).unwrap();
/// assert_eq!(l.matrix(), [-1.0, 0.0, 100.0, 0.0, 1.0, 0.0]);
/// assert!(local_transform(&Control::bookmark("x"), &EmissionPolicy::exact()).is_none());
/// ```
pub fn local_transform(control: &Control, policy: &EmissionPolicy) -> Option<ShapeTransform> {
    let b = shape_box(control)?;
    let (rotation, flip) = b.style.map_or((None, None), |s| (s.rotation, s.flip));
    Some(local_affine(f64::from(b.width), f64::from(b.height), rotation, flip, policy).0)
}

fn box_corners(w: f64, h: f64) -> [(f64, f64); 4] {
    [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]
}

/// The four corners of `control`'s box at `offset + L` in its parent's
/// coordinates (top-left, top-right, bottom-right, bottom-left of the
/// local box). `None` for controls without a box.
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::geometry::{emitted_corners, EmissionPolicy};
/// use hwpforge_core::control::Control;
/// use hwpforge_foundation::HwpUnit;
///
/// let rect = Control::rect(HwpUnit::new(100).unwrap(), HwpUnit::new(50).unwrap()).unwrap();
/// let c = emitted_corners(&rect, &EmissionPolicy::exact()).unwrap();
/// assert_eq!(c, [(0.0, 0.0), (100.0, 0.0), (100.0, 50.0), (0.0, 50.0)]);
/// ```
pub fn emitted_corners(control: &Control, policy: &EmissionPolicy) -> Option<[(f64, f64); 4]> {
    let b = shape_box(control)?;
    let l = local_transform(control, policy)?;
    let shift = ShapeTransform::translate(f64::from(b.offset.0), f64::from(b.offset.1));
    let map = shift.then(l);
    Some(box_corners(f64::from(b.width), f64::from(b.height)).map(|p| map.apply(p)))
}

/// The outcome of [`emission_residual`]: how far a policy's stored matrix
/// puts the shape from its exact Core position.
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::geometry::{emission_residual, EmissionPolicy};
/// use hwpforge_core::control::Control;
/// use hwpforge_foundation::HwpUnit;
///
/// let rect = Control::rect(HwpUnit::new(100).unwrap(), HwpUnit::new(50).unwrap()).unwrap();
/// let check = emission_residual(&rect, &EmissionPolicy::exact());
/// assert_eq!(check.residual(), 0.0);
/// assert!(!check.is_unstorable() && check.loss().is_none());
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EmissionCheck {
    residual: f64,
    unstorable: bool,
}

impl EmissionCheck {
    /// Largest distance, in HWPUNIT, between a box corner or defining point
    /// at its exact position and where the policy's matrix puts it.
    pub fn residual(&self) -> f64 {
        self.residual
    }

    /// The policy cannot store this shape's mirror together with its
    /// rotation and writes it mirrored only.
    pub fn is_unstorable(&self) -> bool {
        self.unstorable
    }

    /// The loss to report, if any: [`FoldLossKind::Unemittable`] for an
    /// unstorable combination, [`FoldLossKind::Residual`] for a residual
    /// above 2 HWPUNIT.
    pub fn loss(&self) -> Option<FoldLoss> {
        if self.unstorable {
            Some(FoldLoss::new(
                FoldLossKind::Unemittable,
                format!(
                    "mirror and rotation cannot be stored together (off by {:.2})",
                    self.residual
                ),
            ))
        } else if exceeds_tolerance(self.residual) {
            Some(FoldLoss::new(
                FoldLossKind::Residual,
                format!("stored transform is off by {:.2} HWPUNIT", self.residual),
            ))
        } else {
            None
        }
    }
}

/// Compares `control` at its exact position with where `policy` stores it,
/// over the box corners and every defining point. A control without a box
/// has nothing to compare (residual 0).
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::geometry::{
///     emission_residual, AngleRounding, CoefficientRounding, EmissionPolicy,
/// };
/// use hwpforge_core::control::{Control, ShapeStyle};
/// use hwpforge_foundation::HwpUnit;
///
/// let mut rect =
///     Control::rect(HwpUnit::new(10_000).unwrap(), HwpUnit::new(1_000).unwrap()).unwrap();
/// if let Control::Rect { style, .. } = &mut rect {
///     *style = Some(ShapeStyle { rotation: Some(30.4), ..ShapeStyle::default() });
/// }
/// let whole =
///     EmissionPolicy::new(AngleRounding::WholeDegrees, CoefficientRounding::Decimals(6), true);
/// // Storing 30° for 30.4° moves a corner by about 35 HWPUNIT.
/// assert!(emission_residual(&rect, &whole).loss().is_some());
/// ```
pub fn emission_residual(control: &Control, policy: &EmissionPolicy) -> EmissionCheck {
    let Some(b) = shape_box(control) else {
        return EmissionCheck { residual: 0.0, unstorable: false };
    };
    let (rotation, flip) = b.style.map_or((None, None), |s| (s.rotation, s.flip));
    let (w, h) = (f64::from(b.width), f64::from(b.height));
    let (exact, _) = local_affine(w, h, rotation, flip, &EmissionPolicy::exact());
    let (stored, unstorable) = local_affine(w, h, rotation, flip, policy);
    let mut pts: Vec<(f64, f64)> = box_corners(w, h).to_vec();
    pts.extend(defining_points(control).iter().map(|p| (f64::from(p.x), f64::from(p.y))));
    let exact_pts: Vec<(f64, f64)> = pts.iter().map(|&p| exact.apply(p)).collect();
    let stored_pts: Vec<(f64, f64)> = pts.iter().map(|&p| stored.apply(p)).collect();
    let residual = max_residual(&exact_pts, &stored_pts);
    EmissionCheck { residual, unstorable }
}

// ── Frame emission plan ────────────────────────────────────────

/// What a writer stores for one object's box: the original size, the scale
/// between it and the displayed size, and the defining points in the stored
/// box — decided once, so the writer and a decoder checking a source against
/// it read the same values.
///
/// [`frame_plan`] builds it in three steps:
///
/// 1. the frame as Core holds it: original box `O` (the
///    [`ObjectFrame`](crate::frame::ObjectFrame) value or the kind's
///    default), scale `width / O.width` and `height / O.height` (1 on an
///    original axis of 0) and the points as they are;
/// 2. the stored values put back through the policy (coefficients rounded,
///    the angle rounded) are compared with the exact Core shape at the box
///    corners and every defining point;
/// 3. more than 2 HWPUNIT off with a frame that is not the displayed box:
///    the plan falls back to the displayed box — original size = displayed
///    size, scale 1, points rounded into the displayed box — and is measured
///    again. What is still off is reported ([`Self::loss`]).
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::geometry::{frame_plan, EmissionPolicy};
/// use hwpforge_core::control::Control;
/// use hwpforge_foundation::{HwpUnit, Size};
///
/// let u = |v| HwpUnit::new(v).unwrap();
/// let mut circle = Control::ellipse(u(560), u(560));
/// circle.resize(u(850), u(850));
/// let plan = frame_plan(&circle, &EmissionPolicy::exact()).unwrap();
/// assert_eq!(plan.original_size(), (560, 560));
/// assert_eq!(plan.points()[0].x, 280); // the centre, in the original box
/// assert!((plan.scale().0 - 850.0 / 560.0).abs() < 1e-12);
/// assert!(!plan.is_fallback() && plan.loss().is_none());
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct FramePlan {
    original: (i32, i32),
    scale: (f64, f64),
    points: Vec<ShapePoint>,
    stored: Vec<(f64, f64)>,
    fallback: bool,
    loss: Option<FoldLoss>,
}

impl FramePlan {
    /// The original size to store (`<hp:orgSz>`).
    pub fn original_size(&self) -> (i32, i32) {
        self.original
    }

    /// The scale to store, exact: displayed / original on each axis,
    /// `(1.0, 1.0)` when the original box is the displayed one.
    pub fn scale(&self) -> (f64, f64) {
        self.scale
    }

    /// The defining points to store, in the original box, in
    /// [`defining_points`] order.
    pub fn points(&self) -> &[ShapePoint] {
        &self.points
    }

    /// Where the stored values put the original box's corners (top-left,
    /// top-right, bottom-right, bottom-left) and then each point, before the
    /// object's offset: the stored rotation and mirror applied to the stored
    /// scale applied to the stored point.
    pub fn stored_positions(&self) -> &[(f64, f64)] {
        &self.stored
    }

    /// The frame did not survive the policy and the plan uses the displayed
    /// box instead (step 3).
    pub fn is_fallback(&self) -> bool {
        self.fallback
    }

    /// What the stored values still miss: [`FoldLossKind::Residual`] more
    /// than 2 HWPUNIT from the exact shape, [`FoldLossKind::Unemittable`]
    /// for a mirror and rotation the policy cannot store together.
    pub fn loss(&self) -> Option<&FoldLoss> {
        self.loss.as_ref()
    }
}

/// The emission plan of `control`'s box under `policy` (see [`FramePlan`]).
/// `None` for a control without a frame. Groups and TextArt have no rotation
/// field: their box is stored unturned.
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::geometry::{frame_plan, EmissionPolicy};
/// use hwpforge_core::control::Control;
/// use hwpforge_foundation::HwpUnit;
///
/// let w = HwpUnit::new(100).unwrap();
/// let plan = frame_plan(&Control::rect(w, w).unwrap(), &EmissionPolicy::exact()).unwrap();
/// assert_eq!((plan.original_size(), plan.scale()), ((100, 100), (1.0, 1.0)));
/// assert!(frame_plan(&Control::bookmark("x"), &EmissionPolicy::exact()).is_none());
/// ```
pub fn frame_plan(control: &Control, policy: &EmissionPolicy) -> Option<FramePlan> {
    control_frame_plan(control, policy)
}

/// The geometry of a group-child frame. Image brush candidates use exact
/// Core geometry: only [`preserve_group_child`] may judge their stored error,
/// after the ancestor chain and absolute translation are known.
pub fn group_child_frame_plan(control: &Control, policy: &EmissionPolicy) -> Option<FramePlan> {
    if has_image_fill(control) {
        control_frame_plan(control, &EmissionPolicy::exact())
    } else {
        frame_plan(control, policy)
    }
}

/// Geometry-only decision input for a group child. Image fills are judged in
/// D with rounded defining points and no original box; their brush box is
/// admitted separately by [`preserve_group_child`]. No content tree is copied.
pub fn group_child_decision_frame(control: &Control, policy: &EmissionPolicy) -> Option<FramePlan> {
    if !has_image_fill(control) {
        return frame_plan(control, policy);
    }
    let b = shape_box(control)?;
    let original = control.effective_original_size()?;
    let (sx, sy) = (
        axis_scale(b.width, original.width.as_i32()),
        axis_scale(b.height, original.height.as_i32()),
    );
    let points = defining_points(control)
        .into_iter()
        .map(|p| ShapePoint {
            x: round_i32(f64::from(p.x) * sx),
            y: round_i32(f64::from(p.y) * sy),
        })
        .collect();
    let (rotation, flip) = b.style.map_or((None, None), |s| (s.rotation, s.flip));
    // 이것을 실패시키는 것: 보존된 그림 O를 writer 재계획에 다시 넣어 형제를 접기.
    Some(plan_box((b.width, b.height), (b.width, b.height), rotation, flip, points, policy))
}

fn has_image_fill(control: &Control) -> bool {
    matches!(
        shape_box(control).and_then(|b| b.style).and_then(|s| s.fill.as_ref()),
        Some(Fill::Image { .. })
    )
}

fn control_frame_plan(control: &Control, policy: &EmissionPolicy) -> Option<FramePlan> {
    let original = control.effective_original_size()?;
    let b = shape_box(control)?;
    let (rotation, flip) = b.style.map_or((None, None), |s| (s.rotation, s.flip));
    Some(plan_box(
        (b.width, b.height),
        (original.width.as_i32(), original.height.as_i32()),
        rotation,
        flip,
        defining_points(control),
        policy,
    ))
}

/// The emission plan of a picture's box under `policy` (see [`FramePlan`]):
/// no rotation, no points.
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::geometry::{picture_frame_plan, EmissionPolicy};
/// use hwpforge_core::image::{Image, ImageFormat};
/// use hwpforge_foundation::HwpUnit;
///
/// let u = |v| HwpUnit::new(v).unwrap();
/// let mut img = Image::new("BinData/a.png", u(1000), u(500), ImageFormat::Png);
/// img.resize(u(2000), u(500));
/// assert_eq!(picture_frame_plan(&img, &EmissionPolicy::exact()).scale(), (2.0, 1.0));
/// ```
pub fn picture_frame_plan(image: &crate::image::Image, policy: &EmissionPolicy) -> FramePlan {
    let o = image.effective_original_size();
    plan_box(
        (image.width.as_i32(), image.height.as_i32()),
        (o.width.as_i32(), o.height.as_i32()),
        image.rotation,
        image.flip,
        Vec::new(),
        policy,
    )
}

/// Drops `control`'s original size: its defining points move into the
/// displayed box (scaled and rounded), so the shape draws the same up to
/// rounding, and the frame keeps only its size lock. A TextArt, whose
/// default box is its design box, takes its displayed size as its original
/// box. Returns `false`, changing nothing, when the frame holds no original
/// size.
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::geometry::drop_original_size;
/// use hwpforge_core::control::{Control, ShapePoint};
/// use hwpforge_foundation::HwpUnit;
///
/// let u = |v| HwpUnit::new(v).unwrap();
/// let mut circle = Control::ellipse(u(560), u(560));
/// circle.resize(u(850), u(850));
/// assert!(drop_original_size(&mut circle));
/// let Control::Ellipse { center, frame, .. } = &circle else { unreachable!() };
/// assert_eq!(*center, ShapePoint { x: 425, y: 425 });
/// assert!(frame.original_size.is_none());
/// ```
pub fn drop_original_size(control: &mut Control) -> bool {
    let Some(original) = control.frame().and_then(|f| f.original_size) else { return false };
    let Some(b) = shape_box(control) else { return false };
    let (sx, sy) = (
        axis_scale(b.width, original.width.as_i32()),
        axis_scale(b.height, original.height.as_i32()),
    );
    let displayed = hwpforge_foundation::Size::new(
        HwpUnit::new(b.width).unwrap_or(HwpUnit::ZERO),
        HwpUnit::new(b.height).unwrap_or(HwpUnit::ZERO),
    );
    for p in defining_points_mut(control) {
        p.x = round_i32(f64::from(p.x) * sx);
        p.y = round_i32(f64::from(p.y) * sy);
    }
    // The kind's default is the displayed size for every kind but TextArt,
    // whose default is its design box.
    // 이것을 실패시키는 것: 글맵시도 `None` 으로 두기(원래 상자가 14173 설계
    // 상자로 돌아가 배율이 바뀐다 — `drop_original_size_keeps_the_drawing`).
    let design = |v: HwpUnit| v.as_i32() == crate::frame::TEXT_ART_DESIGN_SIDE;
    let keep = matches!(control, Control::TextArt { .. })
        && !(design(displayed.width) && design(displayed.height));
    let Some(frame) = control.frame_mut() else { return false };
    frame.original_size = keep.then_some(displayed);
    true
}

/// Displayed / original on one axis; 1 for an original axis of 0 (its
/// points all lie on that axis, nothing to stretch).
fn axis_scale(displayed: i32, original: i32) -> f64 {
    if original == 0 {
        1.0
    } else {
        f64::from(displayed) / f64::from(original)
    }
}

#[allow(clippy::too_many_arguments)]
fn plan_box(
    (w, h): (i32, i32),
    (ow, oh): (i32, i32),
    rotation: Option<f32>,
    flip: Option<Flip>,
    points: Vec<ShapePoint>,
    policy: &EmissionPolicy,
) -> FramePlan {
    let (wf, hf) = (f64::from(w), f64::from(h));
    let (sx, sy) = (axis_scale(w, ow), axis_scale(h, oh));
    let (exact_l, _) = local_affine(wf, hf, rotation, flip, &EmissionPolicy::exact());
    let (stored_l, unstorable) = local_affine(wf, hf, rotation, flip, policy);
    let source: Vec<(f64, f64)> = box_corners(f64::from(ow), f64::from(oh))
        .into_iter()
        .chain(points.iter().map(|p| (f64::from(p.x), f64::from(p.y))))
        .collect();
    let exact: Vec<(f64, f64)> =
        source.iter().map(|&(x, y)| exact_l.apply((x * sx, y * sy))).collect();
    let (qx, qy) = (quantize(sx, policy.coefficients), quantize(sy, policy.coefficients));
    let stored: Vec<(f64, f64)> =
        source.iter().map(|&(x, y)| stored_l.apply((x * qx, y * qy))).collect();
    let residual = max_residual(&exact, &stored);
    let loss = |residual: f64| {
        if unstorable {
            Some(FoldLoss::new(
                FoldLossKind::Unemittable,
                format!("mirror and rotation cannot be stored together (off by {residual:.2})"),
            ))
        } else if exceeds_tolerance(residual) {
            Some(FoldLoss::new(
                FoldLossKind::Residual,
                format!("stored transform is off by {residual:.2} HWPUNIT"),
            ))
        } else {
            None
        }
    };
    // 이것을 실패시키는 것: 이 fallback 을 지우기(극단 비율이 배율 0.000000 으로
    // 써진다 — `extreme_ratio_falls_back`) / 원래 상자가 표시 상자와 같을 때도
    // fallback 으로 보기.
    if exceeds_tolerance(residual) && (ow, oh) != (w, h) {
        let points: Vec<ShapePoint> = points
            .iter()
            .map(|p| ShapePoint {
                x: round_i32(f64::from(p.x) * sx),
                y: round_i32(f64::from(p.y) * sy),
            })
            .collect();
        let stored: Vec<(f64, f64)> = box_corners(wf, hf)
            .into_iter()
            .chain(points.iter().map(|p| (f64::from(p.x), f64::from(p.y))))
            .map(|p| stored_l.apply(p))
            .collect();
        let residual = max_residual(&exact, &stored);
        return FramePlan {
            original: (w, h),
            scale: (1.0, 1.0),
            points,
            stored,
            fallback: true,
            loss: loss(residual),
        };
    }
    FramePlan {
        original: (ow, oh),
        scale: (sx, sy),
        points,
        stored,
        fallback: false,
        loss: loss(residual),
    }
}

// ── Group emission plan ────────────────────────────────────────

/// One stored `(sca, rot)` pair, exact coefficients `[a, b, c, d, e, f]`;
/// a writer prints them with its policy's digits.
pub type MatrixPair = ([f64; 6], [f64; 6]);

const IDENTITY_M: [f64; 6] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0];

/// What a writer stores for one node under a group:
/// its original box, the `(sca, rot)` pairs of its matrix chain after the
/// translation, its points, and where they land in the outermost group's
/// space.
///
/// A node's written pairs are its enclosing groups' own pairs (outermost
/// first) followed by its own; each own pair is `inverse(ancestors) · L`,
/// `L` = the node's own rotation and mirror (`ShapeStyle`) times
/// `diag(width / O.width, height / O.height)`. When every enclosing pair is
/// the identity only the node's own pair is written — the form groups were
/// written in before S20. [`preserve_group_child`] adds all identity pairs for
/// image-filled children: Hancom needs `groupLevel + 1` pairs for their brush.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct GroupNodePlan {
    translation: (i32, i32),
    original: (i32, i32),
    pairs: Vec<MatrixPair>,
    points: Vec<ShapePoint>,
    stored: Vec<(f64, f64)>,
}

impl GroupNodePlan {
    /// Where the node's box starts in the outermost group's space (its
    /// `transMatrix` translation).
    pub fn translation(&self) -> (i32, i32) {
        self.translation
    }

    /// The original size to store.
    pub fn original_size(&self) -> (i32, i32) {
        self.original
    }

    /// The pairs to store after the translation, outermost first.
    pub fn pairs(&self) -> &[MatrixPair] {
        &self.pairs
    }

    /// The defining points to store (in the original box).
    pub fn points(&self) -> &[ShapePoint] {
        &self.points
    }

    /// Where the stored chain puts the original box's corners and then the
    /// points, in the outermost group's space.
    pub fn stored_positions(&self) -> &[(f64, f64)] {
        &self.stored
    }
}

/// The emission plan of a top-level group: its own box ([`FramePlan`]) and
/// every node under it in preorder — a nested group before its children,
/// pictures included, controls without a group form skipped.
///
/// Built with the frames first; a node the frames change — one with an
/// original box of its own, or under a group that stores a scale — more than
/// 2 HWPUNIT from its exact Core place (or whose own box falls back) makes
/// the whole group fall back: every original box
/// becomes the displayed box and every enclosing pair the identity — the
/// leaves keep their own rotation and mirror — and it is measured again.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct GroupPlan {
    own: FramePlan,
    nodes: Vec<GroupNodePlan>,
    fallback: bool,
    loss: Option<FoldLoss>,
}

impl GroupPlan {
    /// The group's own box.
    pub fn own(&self) -> &FramePlan {
        &self.own
    }

    /// The nodes in preorder.
    pub fn nodes(&self) -> &[GroupNodePlan] {
        &self.nodes
    }

    /// The group decision falls back to displayed boxes.
    pub fn is_fallback(&self) -> bool {
        self.fallback
    }

    /// What the stored values still miss after the fallback.
    pub fn loss(&self) -> Option<&FoldLoss> {
        self.loss.as_ref()
    }
}

/// Tries to retain an image-filled child's original box after the group
/// decision has finished. `folded` is that decision's node, and `level` is
/// the number of enclosing groups. Other children are unchanged.
///
/// The candidate uses the actual ancestor pairs and is compared directly
/// with exact Core coordinates. On loss, return the folded node with the same
/// ancestor pair count and a diagnostic for this child; never feed preservation
/// back into `GroupPlan`.
pub fn preserve_group_child(
    control: &Control,
    folded: &GroupNodePlan,
    level: usize,
    policy: &EmissionPolicy,
) -> (GroupNodePlan, Option<FoldLoss>) {
    if !has_image_fill(control) {
        return (folded.clone(), None);
    }
    let b = shape_box(control).expect("image-filled shape box");
    let original = control.effective_original_size().expect("image-filled original box");
    let (ow, oh) = (original.width.as_i32(), original.height.as_i32());
    let mut ancestors = folded.pairs[..folded.pairs.len() - 1].to_vec();
    // 이것을 실패시키는 것: 그림만 필요한 groupLevel+1 단위 쌍을 생략.
    if ancestors.is_empty() {
        ancestors.resize(level, (IDENTITY_M, IDENTITY_M));
    }
    let scale = ancestors.iter().fold((1.0, 1.0), |a, p| (a.0 * p.0[0], a.1 * p.0[4]));
    let (rotation, flip) = b.style.map_or((None, None), |s| (s.rotation, s.flip));
    let (node, residual, _) = node_plan(
        folded.translation,
        (ow, oh),
        (b.width, b.height),
        (axis_scale(b.width, ow), axis_scale(b.height, oh)),
        scale,
        rotation,
        flip,
        defining_points(control),
        &ancestors,
        policy,
    );
    // 이것을 실패시키는 것: 이동 없는 로컬 잔차를 다시 재서 2.0의 반대쪽으로 판정하기.
    // node_plan measures exact Core against the actual quantized ancestor chain,
    // both including the same absolute translation. This is the only admission residual.
    if exceeds_tolerance(residual) {
        // 이것을 실패시키는 것: 거부된 그림만 단일 쌍으로 내어 다음 왕복에서 쌍 수가 바뀌기.
        let mut rejected = folded.clone();
        if rejected.pairs.len() == 1 {
            rejected.pairs.splice(0..0, vec![(IDENTITY_M, IDENTITY_M); level]);
        }
        (rejected, Some(FoldLoss::new(FoldLossKind::Residual, format!(
            "image-filled child's full stored transform is off by {residual:.6} HWPUNIT; original box folded for this child"
        ))))
    } else {
        (node, None)
    }
}

/// Applies source-box candidates only after the ordinary group decision.
/// `candidates` contains preorder node indices and geometry-only frames; codecs
/// build the group with folded decision geometry first. Aligned image children
/// already carry their candidate in Core and are checked even without an entry.
/// Returns child paths and losses for candidates that cannot be stored.
pub fn preserve_group_original_boxes(
    group: &mut Control,
    candidates: &[(usize, FramePlan)],
    policy: &EmissionPolicy,
) -> Vec<(Vec<usize>, FoldLoss)> {
    fn contains_image(control: &Control) -> bool {
        has_image_fill(control)
            || matches!(control, Control::Group { children, .. } if children.iter().any(|child| {
                matches!(child, super::GroupChild::Shape(shape) if contains_image(shape))
            }))
    }
    if candidates.is_empty() && !contains_image(group) {
        return Vec::new();
    }
    let Some(plan) = group_plan(group, policy) else { return Vec::new() };
    fn visit(
        group: &mut Control,
        path: &mut Vec<usize>,
        index: &mut usize,
        candidates: &[(usize, FramePlan)],
        plan: &GroupPlan,
        policy: &EmissionPolicy,
        losses: &mut Vec<(Vec<usize>, FoldLoss)>,
    ) {
        let Control::Group { children, .. } = group else { return };
        for (child_index, child) in children.iter_mut().enumerate() {
            path.push(child_index);
            if let super::GroupChild::Shape(control) = child {
                if shape_box(control).is_none()
                    || matches!(control, Control::Equation { .. } | Control::EmbeddedChart { .. })
                {
                    path.pop();
                    continue;
                }
                // Ordinary fallback still folds every original box, but its image
                // children must go through the same final emission check below.
                // 이것을 실패시키는 것: 비그림 형제의 fallback을 이유로 그림 손실 검사를 생략.
                if plan.is_fallback() {
                    if has_image_fill(control) {
                        drop_original_size(control);
                    }
                } else if let Ok(candidate_index) =
                    candidates.binary_search_by_key(index, |(i, _)| *i)
                {
                    let candidate = &candidates[candidate_index].1;
                    let (w, h) = candidate.original_size();
                    let original = hwpforge_foundation::Size::new(
                        HwpUnit::new(w).expect("candidate width"),
                        HwpUnit::new(h).expect("candidate height"),
                    );
                    let b = shape_box(control).expect("candidate shape");
                    let changed = (w, h) != (b.width, b.height);
                    control.frame_mut().expect("candidate frame").original_size =
                        changed.then_some(original);
                    for (point, source) in
                        defining_points_mut(control).into_iter().zip(candidate.points())
                    {
                        *point = *source;
                    }
                }
                if has_image_fill(control) {
                    let (_, loss) =
                        preserve_group_child(control, &plan.nodes()[*index], path.len(), policy);
                    if let Some(loss) = loss {
                        drop_original_size(control);
                        losses.push((path.clone(), loss));
                    }
                }
                *index += 1;
                visit(control, path, index, candidates, plan, policy, losses);
            } else {
                *index += 1;
            }
            path.pop();
        }
    }
    let mut losses = Vec::new();
    visit(group, &mut Vec::new(), &mut 0, candidates, &plan, policy, &mut losses);
    losses
}

fn diag_scale(m: &[f64; 6]) -> (f64, f64) {
    (m[0], m[4])
}

fn quantize_pair(p: &MatrixPair, rounding: CoefficientRounding) -> MatrixPair {
    let q = |m: [f64; 6]| m.map(|v| quantize(v, rounding));
    (q(p.0), q(p.1))
}

/// The emission plan of a top-level `group` under `policy`; `None` for a
/// control that is not a group.
pub fn group_plan(group: &Control, policy: &EmissionPolicy) -> Option<GroupPlan> {
    let Control::Group { children, .. } = group else { return None };
    let own = frame_plan(group, policy)?;
    let mut nodes = Vec::new();
    let mut worst = 0.0_f64;
    let q = |v: f64| quantize(v, policy.coefficients);
    let top_scale =
        if own.is_fallback() { (1.0, 1.0) } else { (q(own.scale().0), q(own.scale().1)) };
    let mut exact = Vec::new();
    plan_children(children, (0, 0), &[top_scale], true, policy, &mut nodes, &mut worst, &mut exact);
    if !(exceeds_tolerance(worst) || own.is_fallback() || own.loss().is_some()) {
        return Some(GroupPlan { own, nodes, fallback: false, loss: None });
    }
    let mut plain = group.clone();
    drop_original_sizes(&mut plain);
    let own = frame_plan(&plain, policy)?;
    let Control::Group { children, .. } = &plain else { return None };
    let mut nodes = Vec::new();
    let mut unused = 0.0_f64;
    plan_children(
        children,
        (0, 0),
        &[(1.0, 1.0)],
        false,
        policy,
        &mut nodes,
        &mut unused,
        &mut Vec::new(),
    );
    // The fallback is measured against the exact Core places of the group as
    // given, not against the copy whose points were rounded into the
    // displayed boxes: a point that moved more than 2 in that
    // rounding, or saturated, is a loss the writer must report.
    // 이것을 실패시키는 것: 반올림한 사본을 정답으로 다시 재기
    // (`fallback_measures_against_the_original_points`).
    let worst = if nodes.len() == exact.len() {
        nodes
            .iter()
            .zip(&exact)
            .fold(0.0_f64, |w, (n, e)| e.as_ref().map_or(w, |e| w.max(max_residual(e, &n.stored))))
    } else {
        f64::INFINITY
    };
    let loss = residual_loss(worst);
    Some(GroupPlan { own, nodes, fallback: true, loss })
}

/// Drops the original box of `control` and, for a group, of every node under
/// it ([`drop_original_size`] on each; a picture's original box is cleared):
/// what a group's fallback writes ([`GroupPlan::is_fallback`]).
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::geometry::drop_original_sizes;
/// use hwpforge_core::control::Control;
/// use hwpforge_foundation::HwpUnit;
///
/// let u = |v| HwpUnit::new(v).unwrap();
/// let mut circle = Control::ellipse(u(560), u(560));
/// circle.resize(u(850), u(850));
/// drop_original_sizes(&mut circle);
/// assert!(circle.frame().unwrap().is_unset());
/// ```
pub fn drop_original_sizes(control: &mut Control) {
    drop_original_size(control);
    if let Control::Group { children, .. } = control {
        for child in children {
            match child {
                super::GroupChild::Shape(shape) => drop_original_sizes(shape),
                super::GroupChild::Picture(image) => image.frame.original_size = None,
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn plan_children(
    children: &[super::GroupChild],
    parent_origin: (i32, i32),
    ancestors: &[(f64, f64)],
    frames: bool,
    policy: &EmissionPolicy,
    nodes: &mut Vec<GroupNodePlan>,
    worst: &mut f64,
    exact: &mut Vec<Option<Vec<(f64, f64)>>>,
) {
    let (ax, ay) = ancestors.iter().fold((1.0, 1.0), |a, s| (a.0 * s.0, a.1 * s.1));
    let scaled = ancestors.iter().any(|s| *s != (1.0, 1.0));
    let ancestor_pairs: Vec<MatrixPair> = if scaled {
        ancestors.iter().map(|&(sx, sy)| ([sx, 0.0, 0.0, 0.0, sy, 0.0], IDENTITY_M)).collect()
    } else {
        Vec::new()
    };
    for child in children {
        match child {
            super::GroupChild::Picture(image) => {
                let (dx, dy) = placement_offset(image.placement.as_ref());
                let t = (parent_origin.0.saturating_add(dx), parent_origin.1.saturating_add(dy));
                let o = image.effective_original_size();
                let (ow, oh) = (o.width.as_i32(), o.height.as_i32());
                let (w, h) = (image.width.as_i32(), image.height.as_i32());
                let lin = (axis_scale(w, ow), axis_scale(h, oh));
                let node = node_plan(
                    t,
                    (ow, oh),
                    (w, h),
                    lin,
                    (ax, ay),
                    None,
                    None,
                    Vec::new(),
                    &ancestor_pairs,
                    policy,
                );
                let counted = (ow, oh) != (w, h) || scaled;
                if counted {
                    *worst = worst.max(node.1);
                }
                exact.push(counted.then_some(node.2));
                if frames
                    && plan_box((w, h), (ow, oh), None, None, Vec::new(), policy).is_fallback()
                {
                    *worst = f64::INFINITY;
                }
                nodes.push(node.0);
            }
            super::GroupChild::Shape(control) => {
                let Some(b) = shape_box(control) else { continue };
                if matches!(control, Control::Equation { .. } | Control::EmbeddedChart { .. }) {
                    // No group form: the writer leaves it out.
                    continue;
                }
                let t = (
                    parent_origin.0.saturating_add(b.offset.0),
                    parent_origin.1.saturating_add(b.offset.1),
                );
                // 이것을 실패시키는 것: 비그림의 정의점/잔차를 FramePlan에서 불필요하게 두 번 계산.
                let ((ow, oh), points) = if has_image_fill(control) {
                    let decision = group_child_decision_frame(control, &EmissionPolicy::exact())
                        .expect("image-filled decision frame");
                    (decision.original_size(), decision.points)
                } else {
                    let Some(original) = control.effective_original_size() else { continue };
                    ((original.width.as_i32(), original.height.as_i32()), defining_points(control))
                };
                let lin = (axis_scale(b.width, ow), axis_scale(b.height, oh));
                let (rotation, flip) = b.style.map_or((None, None), |s| (s.rotation, s.flip));
                let node = node_plan(
                    t,
                    (ow, oh),
                    (b.width, b.height),
                    lin,
                    (ax, ay),
                    rotation,
                    flip,
                    points,
                    &ancestor_pairs,
                    policy,
                );
                // Only a node the frames change counts — one with an original
                // box of its own or under a scaled group; a frameless node's
                // own miss (a rotation rounded to whole degrees) is the same
                // with or without them and is reported by the shape's writer.
                // 이것을 실패시키는 것: 모든 노드를 세기(비스듬한 형제 하나 때문에
                // 그림의 원래 상자를 잃는다 — `frameless_sibling_does_not_drop_the_boxes`).
                let counted = (ow, oh) != (b.width, b.height) || scaled;
                if counted {
                    *worst = worst.max(node.1);
                }
                exact.push(counted.then_some(node.2));
                if frames
                    && group_child_decision_frame(control, policy).is_some_and(|p| p.is_fallback())
                {
                    *worst = f64::INFINITY;
                }
                let own = diag_scale(&node.0.pairs.last().map_or(IDENTITY_M, |p| p.0));
                let own_written =
                    (quantize(own.0, policy.coefficients), quantize(own.1, policy.coefficients));
                nodes.push(node.0);
                if let Control::Group { children, .. } = control {
                    let mut next = ancestors.to_vec();
                    next.push(own_written);
                    plan_children(children, t, &next, frames, policy, nodes, worst, exact);
                }
            }
        }
    }
}

/// One node: its own pair `inverse(A) · L` after the ancestors' pairs, its
/// stored and exact positions, and the distance between them.
#[allow(clippy::too_many_arguments)]
fn node_plan(
    t: (i32, i32),
    (ow, oh): (i32, i32),
    (w, h): (i32, i32),
    (sx, sy): (f64, f64),
    (ax, ay): (f64, f64),
    rotation: Option<f32>,
    flip: Option<Flip>,
    points: Vec<ShapePoint>,
    ancestor_pairs: &[MatrixPair],
    policy: &EmissionPolicy,
) -> (GroupNodePlan, f64, Vec<(f64, f64)>) {
    let (wf, hf) = (f64::from(w), f64::from(h));
    let (exact_l, _) = local_affine(wf, hf, rotation, flip, &EmissionPolicy::exact());
    let (stored_l, _) = local_affine(wf, hf, rotation, flip, policy);
    let inv_a = ShapeTransform::from_matrix([1.0 / ax, 0.0, 0.0, 0.0, 1.0 / ay, 0.0]);
    let a = ShapeTransform::from_matrix([ax, 0.0, 0.0, 0.0, ay, 0.0]);
    let own_rot = if (ax, ay) == (1.0, 1.0) { stored_l } else { inv_a.then(stored_l).then(a) };
    let own_sca = [sx / ax, 0.0, 0.0, 0.0, sy / ay, 0.0];
    let own: MatrixPair = (own_sca, own_rot.matrix());
    let mut pairs = ancestor_pairs.to_vec();
    pairs.push(own);
    // The chain as stored: translation, then each pair `rot · sca` (quantized).
    let chain =
        pairs.iter().fold(ShapeTransform::translate(f64::from(t.0), f64::from(t.1)), |m, p| {
            let (s, r) = quantize_pair(p, policy.coefficients);
            m.then(ShapeTransform::from_matrix(r)).then(ShapeTransform::from_matrix(s))
        });
    let exact_map = ShapeTransform::translate(f64::from(t.0), f64::from(t.1)).then(exact_l);
    let q: Vec<(f64, f64)> = box_corners(f64::from(ow), f64::from(oh))
        .into_iter()
        .chain(points.iter().map(|p| (f64::from(p.x), f64::from(p.y))))
        .collect();
    let stored: Vec<(f64, f64)> = q.iter().map(|&p| chain.apply(p)).collect();
    let exact: Vec<(f64, f64)> =
        q.iter().map(|&(x, y)| exact_map.apply((x * sx, y * sy))).collect();
    let residual = max_residual(&exact, &stored);
    (GroupNodePlan { translation: t, original: (ow, oh), pairs, points, stored }, residual, exact)
}

// ── Folding ────────────────────────────────────────────────────

/// Facts about a shape that decide how its transform folds, which Core may
/// not carry (a picture fill, say).
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::geometry::GeometryFacts;
///
/// assert!(!GeometryFacts::default().content_turns_with_shape);
/// assert!(GeometryFacts::new(true).content_turns_with_shape);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct GeometryFacts {
    /// The shape holds something that turns with it — text, a caption, or a
    /// directional fill (gradient, pattern, picture). A line, polygon, curve
    /// or connector with such content folds like a box (it keeps a rotation)
    /// instead of taking the whole map onto its points.
    pub content_turns_with_shape: bool,
}

impl GeometryFacts {
    /// Facts with `content_turns_with_shape` set as given.
    pub const fn new(content_turns_with_shape: bool) -> Self {
        Self { content_turns_with_shape }
    }
}

/// What a codec knows about one group child before folding it.
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::geometry::{FoldInput, GeometryFacts, ShapeTransform};
///
/// let map = ShapeTransform::from_matrix([1.0, 0.0, 500.0, 0.0, 1.0, 0.0]);
/// let input = FoldInput::new(map, (100.0, 50.0), (0.0, 0.0))
///     .with_geometry_facts(GeometryFacts::new(true));
/// # let _ = input;
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct FoldInput {
    map: ShapeTransform,
    original_size: (f64, f64),
    parent_origin: (f64, f64),
    facts: GeometryFacts,
}

impl FoldInput {
    /// `map` takes the child's stored local coordinates to the outermost
    /// group's coordinates; `original_size` is the stored box the local
    /// coordinates refer to; `parent_origin` is the top-left corner of the
    /// child's direct parent group in the outermost group's coordinates.
    pub fn new(map: ShapeTransform, original_size: (f64, f64), parent_origin: (f64, f64)) -> Self {
        Self { map, original_size, parent_origin, facts: GeometryFacts::default() }
    }

    /// Replaces the [`GeometryFacts`].
    pub fn with_geometry_facts(mut self, facts: GeometryFacts) -> Self {
        self.facts = facts;
        self
    }
}

/// Why a fold could not carry a child exactly.
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::geometry::FoldLossKind;
///
/// assert_ne!(FoldLossKind::Shear, FoldLossKind::Residual);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FoldLossKind {
    /// The map shears the box; Core boxes are rotated rectangles.
    Shear,
    /// The map collapses an axis (or a polygon) to size zero.
    Degenerate,
    /// The Core value, with its integer sizes and offsets, puts a point more
    /// than 2 HWPUNIT from where the source put it.
    Residual,
    /// A size or offset falls outside the HWPUNIT range.
    OutOfRange,
    /// A writer's policy cannot store the shape's transform (see
    /// [`EmissionCheck::loss`]).
    Unemittable,
    /// The Core variant has no field for part of the transform (TextArt and
    /// charts have no rotation or mirror, an equation has no offset).
    Unstorable,
}

/// One thing a fold could not carry, with a human-readable detail.
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::geometry::{fold_box, FoldInput, FoldLossKind, ShapeTransform};
///
/// let flat = ShapeTransform::from_matrix([1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
/// let loss = fold_box(100.0, 50.0, &FoldInput::new(flat, (100.0, 50.0), (0.0, 0.0))).unwrap_err();
/// assert_eq!(loss.kind(), FoldLossKind::Degenerate);
/// assert!(!loss.detail().is_empty());
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct FoldLoss {
    kind: FoldLossKind,
    detail: String,
}

impl FoldLoss {
    fn new(kind: FoldLossKind, detail: impl Into<String>) -> Self {
        Self { kind, detail: detail.into() }
    }

    /// What kind of loss this is.
    pub fn kind(&self) -> FoldLossKind {
        self.kind
    }

    /// Human-readable detail.
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl std::fmt::Display for FoldLoss {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.detail)
    }
}

/// The result of [`fold_group_child`].
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::geometry::{fold_group_child, FoldInput, FoldOutcome, ShapeTransform};
/// use hwpforge_core::control::Control;
/// use hwpforge_foundation::HwpUnit;
///
/// let rect = Control::rect(HwpUnit::new(100).unwrap(), HwpUnit::new(50).unwrap()).unwrap();
/// let map = ShapeTransform::from_matrix([1.0, 0.0, 300.0, 0.0, 1.0, 40.0]);
/// match fold_group_child(rect, &FoldInput::new(map, (100.0, 50.0), (0.0, 0.0))) {
///     FoldOutcome::Kept { losses, .. } => assert!(losses.is_empty()),
///     _ => unreachable!(),
/// }
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum FoldOutcome {
    /// The child stays, with its Core geometry set; `losses` lists what it
    /// could not carry exactly.
    Kept {
        /// The folded child.
        control: Box<Control>,
        /// What was not carried exactly.
        losses: Vec<FoldLoss>,
    },
    /// The child cannot be placed and was removed.
    Dropped {
        /// Why.
        losses: Vec<FoldLoss>,
    },
}

/// A box folded by [`fold_box`]: its Core size, offset in the direct parent
/// group, rotation and mirror.
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::geometry::{fold_box, FoldInput, ShapeTransform};
///
/// let map = ShapeTransform::from_matrix([2.0, 0.0, 300.0, 0.0, 1.0, 40.0]);
/// let b = fold_box(100.0, 50.0, &FoldInput::new(map, (100.0, 50.0), (0.0, 0.0))).unwrap();
/// assert_eq!((b.width(), b.height(), b.offset()), (200, 50, (300, 40)));
/// assert_eq!((b.rotation(), b.flip()), (None, None));
/// assert!(b.losses().is_empty());
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct BoxFold {
    width: i32,
    height: i32,
    offset: (i32, i32),
    rotation: Option<f32>,
    flip: Option<Flip>,
    losses: Vec<FoldLoss>,
}

impl BoxFold {
    /// Width in HWPUNIT.
    pub fn width(&self) -> i32 {
        self.width
    }

    /// Height in HWPUNIT.
    pub fn height(&self) -> i32 {
        self.height
    }

    /// Offset from the direct parent group's top-left corner.
    pub fn offset(&self) -> (i32, i32) {
        self.offset
    }

    /// Rotation in degrees, `None` when there is none.
    pub fn rotation(&self) -> Option<f32> {
        self.rotation
    }

    /// Mirror, `None` when there is none. A fold always reports a mirror as
    /// [`Flip::Horizontal`] with the rotation adjusted.
    pub fn flip(&self) -> Option<Flip> {
        self.flip
    }

    /// What the box does not carry exactly (a residual).
    pub fn losses(&self) -> &[FoldLoss] {
        &self.losses
    }
}

/// Linear part of a map split as `Rot(θ) · Diag(sx, sy) · F`, `F` a
/// horizontal mirror when the determinant is negative.
#[derive(Debug, Clone, Copy, PartialEq)]
struct BoxDecomposition {
    sx: f64,
    sy: f64,
    /// Degrees in `[0, 360)`.
    angle: f64,
    flip: bool,
    /// |cos| of the angle between the two columns (`0` = no shear).
    shear: f64,
}

fn decompose_box(m: &ShapeTransform) -> Option<BoxDecomposition> {
    let sx = m.a.hypot(m.d);
    let sy = m.b.hypot(m.e);
    if !(sx * sy).is_finite() || sx * sy <= f64::EPSILON {
        return None;
    }
    Some(decompose_columns(m, sx, sy))
}

fn decompose_columns(m: &ShapeTransform, sx: f64, sy: f64) -> BoxDecomposition {
    let det = m.a * m.e - m.b * m.d;
    let flip = det < 0.0;
    let angle = if flip { (-m.d).atan2(-m.a) } else { m.d.atan2(m.a) }.to_degrees();
    BoxDecomposition {
        sx,
        sy,
        angle: angle.rem_euclid(360.0),
        flip,
        shear: (m.a * m.b + m.d * m.e).abs() / (sx * sy),
    }
}

/// Restores a standalone shape from its complete source map. Defining points,
/// placement, text and non-geometric style stay untouched. No mutation on error.
///
/// # Errors
/// Reports shear, collapsed axes, invalid sizes or an unrepresentable residual.
pub fn fold_standalone(
    control: &mut Control,
    input: &FoldInput,
) -> Result<Vec<FoldLoss>, FoldLoss> {
    if style_mut(control).is_none() {
        return Err(FoldLoss::new(FoldLossKind::Unstorable, "not a standalone drawing shape"));
    }
    let line = matches!(control, Control::Line { .. } | Control::ConnectLine { .. });
    let points = defining_points(control);
    let fold = standalone_box(input, line, &points)?;
    let (width, height) = match control {
        Control::TextBox { width, height, .. }
        | Control::Rect { width, height, .. }
        | Control::Ellipse { width, height, .. }
        | Control::Arc { width, height, .. }
        | Control::Polygon { width, height, .. }
        | Control::Curve { width, height, .. }
        | Control::Line { width, height, .. }
        | Control::ConnectLine { width, height, .. } => (width, height),
        _ => unreachable!(),
    };
    *width = HwpUnit::new(fold.width).map_err(|_| out_of_range())?;
    *height = HwpUnit::new(fold.height).map_err(|_| out_of_range())?;
    if let Some(frame) = control.frame_mut() {
        frame.original_size = Some(standalone_original(input)?);
    }
    set_rotation_and_flip(control, fold.rotation, fold.flip);
    Ok(fold.losses)
}

/// The picture counterpart of [`fold_standalone`]. Crop, margin, asset,
/// caption and placement are independent of this operation.
///
/// # Errors
/// Reports the same geometry losses as [`fold_standalone`].
pub fn fold_standalone_picture(
    image: &mut crate::image::Image,
    input: &FoldInput,
) -> Result<Vec<FoldLoss>, FoldLoss> {
    let fold = standalone_box(input, false, &[])?;
    let original = standalone_original(input)?;
    image.width = HwpUnit::new(fold.width).map_err(|_| out_of_range())?;
    image.height = HwpUnit::new(fold.height).map_err(|_| out_of_range())?;
    image.frame.original_size = Some(original);
    image.rotation = fold.rotation;
    image.flip = fold.flip;
    Ok(fold.losses)
}

/// HWPX integer placement of a standalone local box (whole degrees, six decimals).
/// Returns the centre-based matrix, placement size, and translation of the
/// rounded minimum to zero. No configurable policy or fold losses are exposed.
/// This does not change Core's exact local transform or group coordinates.
/// Callers supply validated, nonnegative HWPUNIT dimensions.
///
/// The writer and input diagnostics share this calculation; no source
/// offset, declared placement size, or metadata angle participates.
pub fn standalone_layout(
    size: (i32, i32),
    rotation: Option<f32>,
    flip: Option<Flip>,
) -> (ShapeTransform, (i32, i32), (i32, i32)) {
    let angle = stored_angle(rotation, AngleRounding::WholeDegrees);
    let (sin, cos) = angle.to_radians().sin_cos();
    let (fx, fy) = flip_signs(flip);
    let q = |v| quantize(v, CoefficientRounding::Decimals(6));
    let (a, b, d, e) = (q(cos * fx), q(-sin * fy), q(sin * fx), q(cos * fy));
    let (cx, cy) = (f64::from(size.0 / 2), f64::from(size.1 / 2));
    let map = ShapeTransform::from_matrix([a, b, cx - a * cx - b * cy, d, e, cy - d * cx - e * cy]);
    let bounds =
        group_bounds(box_corners(f64::from(size.0), f64::from(size.1)).map(|p| map.apply(p)))
            .expect("validated finite standalone box");
    let (x0, y0, x1, y1) = (
        bounds.min_x().round() as i32,
        bounds.min_y().round() as i32,
        bounds.max_x().round() as i32,
        bounds.max_y().round() as i32,
    );
    (map, (x1 - x0, y1 - y0), (-x0, -y0))
}

fn standalone_original(input: &FoldInput) -> Result<hwpforge_foundation::Size, FoldLoss> {
    let unit = |v: f64, axis: &str| {
        let invalid = || {
            FoldLoss::new(
                FoldLossKind::OutOfRange,
                format!(
                    "original {axis}: O={:?}, value={v} outside the HWPUNIT range",
                    input.original_size
                ),
            )
        };
        if !v.is_finite() || v < 0.0 || v.fract() != 0.0 || v > f64::from(i32::MAX) {
            return Err(invalid());
        }
        HwpUnit::new(v as i32).map_err(|_| invalid())
    };
    Ok(hwpforge_foundation::Size::new(
        unit(input.original_size.0, "x")?,
        unit(input.original_size.1, "y")?,
    ))
}

fn standalone_box(
    input: &FoldInput,
    line: bool,
    points: &[ShapePoint],
) -> Result<BoxFold, FoldLoss> {
    let (ow, oh) = input.original_size;
    finite_input(ow, oh, input)?;
    standalone_original(input)?;
    let m = &input.map;
    let (sx, sy) = (m.a.hypot(m.d), m.b.hypot(m.e));
    // 이것을 실패시키는 것: 작은 비영 열을 epsilon으로 0 처리 / 선 0축 일괄 거부.
    let d = if sx == 0.0 || sy == 0.0 {
        if !line || (sx == 0.0 && sy == 0.0) {
            return Err(FoldLoss::new(
                FoldLossKind::Degenerate,
                format!("axis collapse: O={ow}x{oh}, scales={sx},{sy}, map={:?}", m.matrix()),
            ));
        }
        BoxDecomposition {
            sx,
            sy,
            flip: false,
            shear: 0.0,
            angle: if sx == 0.0 { (-m.b).atan2(m.e) } else { m.d.atan2(m.a) }
                .to_degrees()
                .rem_euclid(360.0),
        }
    } else {
        decompose_columns(m, sx, sy)
    };
    if d.shear > SHEAR_TOLERANCE {
        return Err(FoldLoss::new(
            FoldLossKind::Shear,
            format!("shear={}, O={ow}x{oh}, map={:?}", d.shear, m.matrix()),
        ));
    }
    let dimension = |o: f64, s: f64, axis: &str| {
        let v = (o * s).round();
        if !v.is_finite() || v < 0.0 || v > f64::from(i32::MAX) {
            return Err(FoldLoss::new(
                FoldLossKind::OutOfRange,
                format!("{axis}: O={o}, scale={s}, calculated D={v}"),
            ));
        }
        let v = v as i32;
        HwpUnit::new(v).map_err(|_| {
            FoldLoss::new(
                FoldLossKind::OutOfRange,
                format!("{axis}: O={o}, scale={s}, calculated D={v} outside the HWPUNIT range"),
            )
        })?;
        if v == 0 && (!line || (s != 0.0 && o != 0.0)) {
            return Err(FoldLoss::new(
                FoldLossKind::Degenerate,
                format!("{axis}: nonzero size rounds to zero; O={o}, scale={s}"),
            ));
        }
        Ok(v)
    };
    let (w, h) = (dimension(ow, sx, "x")?, dimension(oh, sy, "y")?);
    let angle = d.angle as f32;
    let rotation = (angle != 0.0 && angle != 360.0).then_some(angle);
    let flip = d.flip.then_some(Flip::Horizontal);
    let (local, _) =
        local_affine(f64::from(w), f64::from(h), rotation, flip, &EmissionPolicy::exact());
    let scale = ShapeTransform::from_matrix([
        if ow == 0.0 { 1.0 } else { f64::from(w) / ow },
        0.0,
        0.0,
        0.0,
        if oh == 0.0 { 1.0 } else { f64::from(h) / oh },
        0.0,
    ]);
    let core = local.then(scale);
    // Normalize using the O box, never the defining-point bounds. Translation
    // mismatch belongs to the codec's G7 check and is not stored as placement.
    let minimum = |map: ShapeTransform| {
        box_corners(ow, oh)
            .iter()
            .map(|&p| map.apply(p))
            .fold((f64::INFINITY, f64::INFINITY), |a, p| (a.0.min(p.0), a.1.min(p.1)))
    };
    let source_min = minimum(*m);
    let core_min = minimum(core);
    let mut ps = box_corners(ow, oh).to_vec();
    ps.extend(points.iter().map(|p| (f64::from(p.x), f64::from(p.y))));
    let normalized = |map: ShapeTransform, min: (f64, f64)| {
        ps.iter()
            .map(|&p| {
                let q = map.apply(p);
                (q.0 - min.0, q.1 - min.1)
            })
            .collect::<Vec<_>>()
    };
    let residual = max_residual(&normalized(*m, source_min), &normalized(core, core_min));
    if let Some(loss) = residual_loss(residual) {
        return Err(loss);
    }
    Ok(BoxFold { width: w, height: h, offset: (0, 0), rotation, flip, losses: Vec::new() })
}

fn round_i32(v: f64) -> i32 {
    // Saturating float→int cast; out-of-range values fail the HwpUnit check.
    v.round() as i32
}

fn fits_hwpunit(values: &[i32]) -> bool {
    values.iter().all(|&v| HwpUnit::new(v).is_ok())
}

/// Largest pairwise distance; a non-finite distance makes the whole result
/// non-finite instead of being skipped.
fn max_residual(expected: &[(f64, f64)], actual: &[(f64, f64)]) -> f64 {
    expected.iter().zip(actual).map(|(e, a)| (e.0 - a.0).hypot(e.1 - a.1)).fold(0.0, |m, d| {
        if m.is_finite() && d.is_finite() {
            m.max(d)
        } else {
            f64::INFINITY
        }
    })
}

/// A residual over the tolerance — or one that is not a number at all.
fn exceeds_tolerance(residual: f64) -> bool {
    residual.is_nan() || residual > EXACT_TOLERANCE
}

fn residual_loss(residual: f64) -> Option<FoldLoss> {
    exceeds_tolerance(residual).then(|| {
        FoldLoss::new(
            FoldLossKind::Residual,
            format!("re-encoded geometry is off by {residual:.2} HWPUNIT"),
        )
    })
}

fn out_of_range() -> FoldLoss {
    FoldLoss::new(FoldLossKind::OutOfRange, "size or offset outside the HWPUNIT range")
}

/// The box fold before deciding whether a shear drops the box.
struct RawBoxFold {
    fold: BoxFold,
    sx: f64,
    sy: f64,
    shear: Option<FoldLoss>,
}

/// Every number a fold reads is finite.
fn finite_input(width: f64, height: f64, input: &FoldInput) -> Result<(), FoldLoss> {
    let (ow, oh) = input.original_size;
    let (px, py) = input.parent_origin;
    if input.map.is_finite() && [width, height, ow, oh, px, py].iter().all(|v| v.is_finite()) {
        Ok(())
    } else {
        Err(FoldLoss::new(FoldLossKind::Degenerate, "transform or size is not a finite number"))
    }
}

fn fold_box_raw(width: f64, height: f64, input: &FoldInput) -> Result<RawBoxFold, FoldLoss> {
    let m = &input.map;
    let degenerate =
        || FoldLoss::new(FoldLossKind::Degenerate, "transform collapses an axis to zero");
    finite_input(width, height, input)?;
    let d = decompose_box(m).ok_or_else(degenerate)?;
    let shear = (d.shear > SHEAR_TOLERANCE)
        .then(|| FoldLoss::new(FoldLossKind::Shear, format!("sheared transform ({:.4})", d.shear)));
    let w = round_i32(width * d.sx);
    let h = round_i32(height * d.sy);
    if w == 0 || h == 0 {
        return Err(FoldLoss::new(FoldLossKind::Degenerate, format!("shape collapses to {w}x{h}")));
    }
    // Core keeps f32 degrees; a value that rounds up to 360 is no rotation.
    let angle = d.angle as f32;
    let rotation = (angle != 0.0 && angle != 360.0).then_some(angle);
    let flip = d.flip.then_some(Flip::Horizontal);
    // Offset so the box's own origin corner lands where the map puts it.
    let (l, _) = local_affine(f64::from(w), f64::from(h), rotation, flip, &EmissionPolicy::exact());
    let (ox, oy) = l.apply((0.0, 0.0));
    let x = round_i32(m.c - ox - input.parent_origin.0);
    let y = round_i32(m.f - oy - input.parent_origin.1);
    if !fits_hwpunit(&[w, h, x, y]) {
        return Err(out_of_range());
    }
    Ok(RawBoxFold {
        fold: BoxFold { width: w, height: h, offset: (x, y), rotation, flip, losses: Vec::new() },
        sx: d.sx,
        sy: d.sy,
        shear,
    })
}

/// World positions of a folded box's corners under `input`'s parent origin.
fn folded_corners(fold: &BoxFold, origin: (f64, f64)) -> ShapeTransform {
    let (l, _) = local_affine(
        f64::from(fold.width),
        f64::from(fold.height),
        fold.rotation,
        fold.flip,
        &EmissionPolicy::exact(),
    );
    ShapeTransform::translate(
        origin.0 + f64::from(fold.offset.0),
        origin.1 + f64::from(fold.offset.1),
    )
    .then(l)
}

/// Folds a `width × height` box placed by `input`'s map into a Core box: the
/// map's linear part is split into rotation, scale along the box's own axes
/// and a horizontal mirror, and the offset is solved so the box's origin
/// corner lands where the map puts it.
///
/// For children that are not a [`Control`] (a picture in a group).
///
/// # Errors
///
/// Returns the [`FoldLoss`] that keeps the box from being placed at all —
/// [`FoldLossKind::Degenerate`] (an axis of size zero),
/// [`FoldLossKind::Shear`] or [`FoldLossKind::OutOfRange`]. The caller drops
/// the child and reports it.
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::geometry::{fold_box, FoldInput, ShapeTransform};
///
/// // Turned by 90°: the box keeps its size and gets a rotation.
/// let map = ShapeTransform::from_matrix([0.0, -1.0, 50.0, 1.0, 0.0, 0.0]);
/// let b = fold_box(100.0, 50.0, &FoldInput::new(map, (100.0, 50.0), (0.0, 0.0))).unwrap();
/// assert_eq!((b.width(), b.height(), b.rotation()), (100, 50, Some(90.0)));
/// assert_eq!(b.offset(), (-25, 25));
/// ```
pub fn fold_box(width: f64, height: f64, input: &FoldInput) -> Result<BoxFold, FoldLoss> {
    let RawBoxFold { mut fold, shear, .. } = fold_box_raw(width, height, input)?;
    if let Some(shear) = shear {
        return Err(shear);
    }
    let wire: Vec<(f64, f64)> =
        box_corners(width, height).iter().map(|&p| input.map.apply(p)).collect();
    let core = folded_corners(&fold, input.parent_origin);
    let ours: Vec<(f64, f64)> = box_corners(f64::from(fold.width), f64::from(fold.height))
        .iter()
        .map(|&p| core.apply(p))
        .collect();
    fold.losses.extend(residual_loss(max_residual(&wire, &ours)));
    Ok(fold)
}

/// Placement of a group child at `(x, y)` inside its parent group: the
/// origin stays `None`, any other offset floats.
fn group_child_placement(x: i32, y: i32) -> Option<ObjectPlacement> {
    if x == 0 && y == 0 {
        return None;
    }
    Some(
        ObjectPlacement::new(
            ObjectTextWrap::InFrontOfText,
            ObjectRelativeTo::Paper,
            ObjectRelativeTo::Paper,
        )
        .with_allow_overlap(true)
        .with_vert_offset(HwpUnit::new(y).unwrap_or(HwpUnit::ZERO))
        .with_horz_offset(HwpUnit::new(x).unwrap_or(HwpUnit::ZERO)),
    )
}

/// A shape's defining points in their stored order: a line's start and end;
/// an ellipse's centre and two axes; an arc's centre, axes, then start/end
/// 1 and 2; a polygon's or curve's points; a connector's start, end, then
/// control points. Empty for a control without points (boxes, groups,
/// TextArt).
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::geometry::defining_points;
/// use hwpforge_core::control::Control;
/// use hwpforge_foundation::HwpUnit;
///
/// let w = HwpUnit::new(100).unwrap();
/// assert_eq!(defining_points(&Control::ellipse(w, w)).len(), 3);
/// assert!(defining_points(&Control::rect(w, w).unwrap()).is_empty());
/// ```
pub fn defining_points(control: &Control) -> Vec<ShapePoint> {
    match control {
        Control::Line { start, end, .. } => vec![*start, *end],
        Control::Ellipse { center, axis1, axis2, .. } => vec![*center, *axis1, *axis2],
        Control::Arc {
            definition: super::ArcDefinition::AxisEndpoints,
            center,
            axis1,
            axis2,
            ..
        } => {
            vec![*center, *axis1, *axis2]
        }
        Control::Arc { center, axis1, axis2, start1, end1, start2, end2, .. } => {
            vec![*center, *axis1, *axis2, *start1, *end1, *start2, *end2]
        }
        Control::Polygon { vertices, .. } => vertices.clone(),
        Control::Curve { points, .. } => points.clone(),
        Control::ConnectLine { start, end, control_points, .. } => {
            let mut v = vec![*start, *end];
            v.extend(control_points.iter().copied());
            v
        }
        _ => Vec::new(),
    }
}

fn defining_points_mut(control: &mut Control) -> Vec<&mut ShapePoint> {
    match control {
        Control::Line { start, end, .. } => vec![start, end],
        Control::Ellipse { center, axis1, axis2, .. } => vec![center, axis1, axis2],
        Control::Arc {
            definition: super::ArcDefinition::AxisEndpoints,
            center,
            axis1,
            axis2,
            ..
        } => {
            vec![center, axis1, axis2]
        }
        Control::Arc { center, axis1, axis2, start1, end1, start2, end2, .. } => {
            vec![center, axis1, axis2, start1, end1, start2, end2]
        }
        Control::Polygon { vertices, .. } => vertices.iter_mut().collect(),
        Control::Curve { points, .. } => points.iter_mut().collect(),
        Control::ConnectLine { start, end, control_points, .. } => {
            let mut v = vec![start, end];
            v.extend(control_points.iter_mut());
            v
        }
        _ => Vec::new(),
    }
}

fn set_box(control: &mut Control, w: i32, h: i32, (x, y): (i32, i32)) {
    let new_placement = group_child_placement(x, y);
    let (width, height) = match control {
        Control::TextBox { width, height, placement, .. }
        | Control::Rect { width, height, placement, .. }
        | Control::Ellipse { width, height, placement, .. }
        | Control::Arc { width, height, placement, .. }
        | Control::Polygon { width, height, placement, .. }
        | Control::Curve { width, height, placement, .. }
        | Control::ConnectLine { width, height, placement, .. }
        | Control::Line { width, height, placement, .. }
        | Control::TextArt { width, height, placement, .. }
        | Control::EmbeddedChart { width, height, placement, .. } => {
            *placement = new_placement;
            (width, height)
        }
        Control::Equation { width, height, .. } => (width, height),
        _ => return,
    };
    *width = HwpUnit::new(w).unwrap_or(HwpUnit::ZERO);
    *height = HwpUnit::new(h).unwrap_or(HwpUnit::ZERO);
}

fn style_mut(control: &mut Control) -> Option<&mut Option<ShapeStyle>> {
    match control {
        Control::TextBox { style, .. }
        | Control::Rect { style, .. }
        | Control::Ellipse { style, .. }
        | Control::Arc { style, .. }
        | Control::Polygon { style, .. }
        | Control::Curve { style, .. }
        | Control::ConnectLine { style, .. }
        | Control::Line { style, .. } => Some(style),
        _ => None,
    }
}

/// Replaces the stored rotation and mirror with the folded ones.
fn set_rotation_and_flip(control: &mut Control, rotation: Option<f32>, flip: Option<Flip>) {
    let Some(style) = style_mut(control) else { return };
    let mut s = style.take().unwrap_or_default();
    s.rotation = rotation;
    s.flip = flip;
    *style = (s != ShapeStyle::default()).then_some(s);
}

/// Whether `control` itself shows content that turns with it.
fn has_turning_content(control: &Control) -> bool {
    let (paragraphs_empty, caption, style) = match control {
        Control::Polygon { paragraphs, caption, style, .. } => {
            (paragraphs.is_empty(), caption.is_some(), style)
        }
        Control::Line { caption, style, .. }
        | Control::Curve { caption, style, .. }
        | Control::ConnectLine { caption, style, .. } => (true, caption.is_some(), style),
        _ => return true,
    };
    let directional_fill = matches!(
        style.as_ref().and_then(|s| s.fill.as_ref()),
        Some(Fill::Gradient { .. } | Fill::Pattern { .. } | Fill::Image { .. })
    );
    !paragraphs_empty || caption || directional_fill
}

/// Folds one group child into exact Core geometry.
///
/// `control` comes in with its defining points in the stored local
/// coordinates (its width, height, offset, rotation and mirror are
/// replaced). Lines, polygons, curves and connectors with nothing that turns
/// with them take the whole map onto their points and keep no rotation;
/// every other shape is scaled along its own axes and gets the rotation and
/// a horizontal mirror as `ShapeStyle` values. TextArt, charts and equations
/// keep an unrotated box with the same centre and report what they cannot
/// hold as [`FoldLossKind::Unstorable`].
///
/// The result is checked point by point against the map: a Core value more
/// than 2 HWPUNIT off is kept with a [`FoldLossKind::Residual`]. A sheared
/// box is kept as the nearest box with a [`FoldLossKind::Shear`]. A child
/// that collapses or falls outside the HWPUNIT range is
/// [`FoldOutcome::Dropped`]. A child carries at most one loss.
///
/// A nested group is not folded here: fold its children, then size it with
/// [`group_bounds`].
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::geometry::{fold_group_child, FoldInput, FoldOutcome, ShapeTransform};
/// use hwpforge_core::control::Control;
/// use hwpforge_foundation::HwpUnit;
///
/// let rect = Control::rect(HwpUnit::new(100).unwrap(), HwpUnit::new(50).unwrap()).unwrap();
/// // Scaled ×1.5 and placed at (300, 40) in a group whose box starts at (100, 0).
/// let map = ShapeTransform::from_matrix([1.5, 0.0, 300.0, 0.0, 1.0, 40.0]);
/// let FoldOutcome::Kept { control, .. } =
///     fold_group_child(rect, &FoldInput::new(map, (100.0, 50.0), (100.0, 0.0)))
/// else {
///     unreachable!()
/// };
/// let Control::Rect { width, placement, .. } = control.as_ref() else { unreachable!() };
/// assert_eq!(width.as_i32(), 150);
/// assert_eq!(placement.as_ref().unwrap().horz_offset.as_i32(), 200);
/// ```
pub fn fold_group_child(control: Control, input: &FoldInput) -> FoldOutcome {
    fold_child(control, input, false)
}

/// Folds a group child while retaining the source box and defining points.
/// Codecs select this for image-filled children: the image brush uses that
/// box independently of the displayed extent. Invalid boxes and unrepresentable
/// transforms retain the usual range, degeneracy, shear and residual diagnostics.
pub fn fold_group_child_with_original_box(control: Control, input: &FoldInput) -> FoldOutcome {
    fold_child(control, input, true)
}

fn fold_child(mut control: Control, input: &FoldInput, preserve_original: bool) -> FoldOutcome {
    let point_based = matches!(
        control,
        Control::Line { .. }
            | Control::Polygon { .. }
            | Control::Curve { .. }
            | Control::ConnectLine { .. }
    );
    let boxed = shape_box(&control).is_some() && !matches!(control, Control::Group { .. });
    if !boxed {
        return FoldOutcome::Dropped {
            losses: vec![FoldLoss::new(
                FoldLossKind::Unstorable,
                format!("{} has no group-child geometry", control.kind_name()),
            )],
        };
    }
    if let Err(loss) = finite_input(0.0, 0.0, input) {
        return FoldOutcome::Dropped { losses: vec![loss] };
    }
    let turns = input.facts.content_turns_with_shape || has_turning_content(&control);
    if point_based && !turns && !preserve_original {
        fold_points(control, input)
    } else {
        fold_boxed(&mut control, input, preserve_original).map_or_else(
            |losses| FoldOutcome::Dropped { losses },
            |losses| FoldOutcome::Kept { control: Box::new(control), losses },
        )
    }
}

/// Folds a group child whose map is axis-aligned — `x' = a·x + c`,
/// `y' = e·y + f` with `a, e > 0`: no turn, mirror or shear — keeping its
/// defining points where they are, in its original box: the
/// original box is the map's input box (`FoldInput`'s original size), the
/// displayed size is that box scaled (`round(a · width)`, `round(e ·
/// height)`), the offset is where the map puts the box's origin. A child
/// drawn so draws where the map puts it, up to the rounding of its size and
/// offset (reported above 2 HWPUNIT as [`FoldLossKind::Residual`]).
///
/// `None` when the map is not axis-aligned, the control has no frame (a
/// chart, an equation) or is a group, or a box would collapse to zero along
/// an axis (lines, curves and connectors may be flat): fold it with
/// [`fold_group_child`] instead.
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::geometry::{fold_aligned_child, FoldInput, FoldOutcome, ShapeTransform};
/// use hwpforge_core::control::Control;
/// use hwpforge_foundation::{HwpUnit, Size};
///
/// let u = |v| HwpUnit::new(v).unwrap();
/// let circle = Control::ellipse(u(560), u(560));
/// let map = ShapeTransform::from_matrix([1.5, 0.0, 300.0, 0.0, 1.5, 40.0]);
/// let Some(FoldOutcome::Kept { control, losses }) =
///     fold_aligned_child(circle, &FoldInput::new(map, (560.0, 560.0), (0.0, 0.0)))
/// else {
///     unreachable!()
/// };
/// assert!(losses.is_empty());
/// let Control::Ellipse { width, center, frame, .. } = *control else { unreachable!() };
/// assert_eq!((width.as_i32(), center.x), (840, 280));
/// assert_eq!(frame.original_size, Some(Size::new(u(560), u(560))));
/// ```
pub fn fold_aligned_child(mut control: Control, input: &FoldInput) -> Option<FoldOutcome> {
    let m = &input.map;
    // 이것을 실패시키는 것: 비스듬함·돌림을 축 정렬로 보기(`aligned_fold_refuses_a_turn`).
    let aligned = m.b.abs() < 1e-9 && m.d.abs() < 1e-9 && m.a > 0.0 && m.e > 0.0;
    if !aligned || matches!(control, Control::Group { .. }) || control.frame().is_none() {
        return None;
    }
    if finite_input(0.0, 0.0, input).is_err() {
        return None;
    }
    let (ow, oh) = input.original_size;
    // An original box that cannot be a frame value — a negative axis (a
    // wrapped `orgSz`) or one out of the HWPUNIT range — is not this path:
    // the caller reports it and folds the child without one.
    // 이것을 실패시키는 것: 이 검사를 지우기(음수 원래 크기가 frame 에 남는다 —
    // `unusable_original_box_is_not_folded`).
    let (oiw, oih) = (round_i32(ow), round_i32(oh));
    if oiw < 0 || oih < 0 || !fits_hwpunit(&[oiw, oih]) {
        return None;
    }
    let w = round_i32(m.a * ow);
    let h = round_i32(m.e * oh);
    let flat_ok = matches!(
        control,
        Control::Line { .. } | Control::Curve { .. } | Control::ConnectLine { .. }
    );
    if (w == 0 || h == 0) && !flat_ok {
        return None;
    }
    let x = round_i32(m.c - input.parent_origin.0);
    let y = round_i32(m.f - input.parent_origin.1);
    if !fits_hwpunit(&[w, h, x, y]) {
        return Some(FoldOutcome::Dropped { losses: vec![out_of_range()] });
    }
    set_box(&mut control, w, h, (x, y));
    set_rotation_and_flip(&mut control, None, None);
    let original = hwpforge_foundation::Size::new(
        HwpUnit::new(oiw).unwrap_or(HwpUnit::ZERO),
        HwpUnit::new(oih).unwrap_or(HwpUnit::ZERO),
    );
    if let Some(frame) = control.frame_mut() {
        frame.original_size = Some(original);
    }
    // The kind's default is no frame value.
    let default = match control {
        Control::TextArt { .. } => {
            (crate::frame::TEXT_ART_DESIGN_SIDE, crate::frame::TEXT_ART_DESIGN_SIDE)
        }
        _ => (w, h),
    };
    if (oiw, oih) == default {
        if let Some(frame) = control.frame_mut() {
            frame.original_size = None;
        }
    }
    let (sx, sy) = (axis_scale(w, oiw), axis_scale(h, oih));
    let origin = (input.parent_origin.0 + f64::from(x), input.parent_origin.1 + f64::from(y));
    let source: Vec<(f64, f64)> = box_corners(ow, oh)
        .into_iter()
        .chain(defining_points(&control).iter().map(|p| (f64::from(p.x), f64::from(p.y))))
        .collect();
    let wire: Vec<(f64, f64)> = source.iter().map(|&p| m.apply(p)).collect();
    let ours: Vec<(f64, f64)> =
        source.iter().map(|&(px, py)| (origin.0 + px * sx, origin.1 + py * sy)).collect();
    let losses = residual_loss(max_residual(&wire, &ours)).into_iter().collect();
    Some(FoldOutcome::Kept { control: Box::new(control), losses })
}

/// The point rule: the whole map goes onto the points.
fn fold_points(mut control: Control, input: &FoldInput) -> FoldOutcome {
    let origin = input.parent_origin;
    let world: Vec<(f64, f64)> = defining_points(&control)
        .iter()
        .map(|p| input.map.apply((f64::from(p.x), f64::from(p.y))))
        .collect();
    let Some(b) = group_bounds(world.iter().copied()) else {
        return FoldOutcome::Dropped {
            losses: vec![FoldLoss::new(FoldLossKind::Degenerate, "shape has no points")],
        };
    };
    let x = round_i32(b.min_x - origin.0);
    let y = round_i32(b.min_y - origin.1);
    let base = (origin.0 + f64::from(x), origin.1 + f64::from(y));
    let local: Vec<(i32, i32)> =
        world.iter().map(|&(wx, wy)| (round_i32(wx - base.0), round_i32(wy - base.1))).collect();
    let w = local.iter().map(|p| p.0).max().unwrap_or(0).max(0);
    let h = local.iter().map(|p| p.1).max().unwrap_or(0).max(0);
    let source = defining_points(&control);
    let distinct = source.iter().any(|p| *p != source[0]);
    // Judged on the mapped points' own extent, not on the rounded local
    // sizes: points that land on one spot half a unit off the grid still
    // round to a 1 × 1 box.
    let (span_x, span_y) = (b.max_x - b.min_x, b.max_y - b.min_y);
    // …and on the stored integer points, which can still land on one spot
    // after the offset absorbs a half unit.
    // (equality, not a subtraction that could overflow before the range
    // check)
    let stored_flat_x = local.iter().all(|p| p.0 == local[0].0);
    let stored_flat_y = local.iter().all(|p| p.1 == local[0].1);
    let flat_x = w == 0 || span_x < 0.5 || stored_flat_x;
    let flat_y = h == 0 || span_y < 0.5 || stored_flat_y;
    let collapsed = match control {
        // A polygon needs area.
        Control::Polygon { .. } => flat_x || flat_y,
        // A line, curve or connector the map squeezes to a single point.
        _ => distinct && flat_x && flat_y,
    };
    if collapsed {
        return FoldOutcome::Dropped {
            losses: vec![FoldLoss::new(
                FoldLossKind::Degenerate,
                format!("{} collapses to {w}x{h}", control.kind_name()),
            )],
        };
    }
    if !fits_hwpunit(&[w, h, x, y]) {
        return FoldOutcome::Dropped { losses: vec![out_of_range()] };
    }
    for (p, &(lx, ly)) in defining_points_mut(&mut control).into_iter().zip(&local) {
        p.x = lx;
        p.y = ly;
    }
    set_box(&mut control, w, h, (x, y));
    set_rotation_and_flip(&mut control, None, None);
    // Rebuilt from what the control now holds.
    let (fx, fy) = shape_box(&control).map_or((0, 0), |b| b.offset);
    let ours: Vec<(f64, f64)> = defining_points(&control)
        .iter()
        .map(|p| (origin.0 + f64::from(fx + p.x), origin.1 + f64::from(fy + p.y)))
        .collect();
    let losses = residual_loss(max_residual(&world, &ours)).into_iter().collect();
    FoldOutcome::Kept { control: Box::new(control), losses }
}

/// The box rule. `Err` = drop the child with these losses.
fn fold_boxed(
    control: &mut Control,
    input: &FoldInput,
    preserve_original: bool,
) -> Result<Vec<FoldLoss>, Vec<FoldLoss>> {
    let (org_w, org_h) = input.original_size;
    let RawBoxFold { fold, sx, sy, shear } =
        fold_box_raw(org_w, org_h, input).map_err(|loss| vec![loss])?;
    let wire_local = defining_points(control);
    let mut wire: Vec<(f64, f64)> =
        box_corners(org_w, org_h).iter().map(|&p| input.map.apply(p)).collect();
    wire.extend(wire_local.iter().map(|p| input.map.apply((f64::from(p.x), f64::from(p.y)))));
    // 이것을 실패시키는 것: 그림 채우기 정의점을 D로 접거나 O만 복원하여 이중 배율 적용.
    if preserve_original {
        let original = standalone_original(input).map_err(|loss| vec![loss])?;
        let Some(frame) = control.frame_mut() else {
            return Err(vec![FoldLoss::new(FoldLossKind::Unstorable, "child has no original box")]);
        };
        // 이것을 실패시키는 것: O=D도 명시적 frame으로 만들어 기존 기본값과 달라짐.
        frame.original_size = ((original.width.as_i32(), original.height.as_i32())
            != (fold.width, fold.height))
            .then_some(original);
    } else {
        for (p, q) in defining_points_mut(control).into_iter().zip(&wire_local) {
            p.x = round_i32(f64::from(q.x) * sx);
            p.y = round_i32(f64::from(q.y) * sy);
        }
    }

    let rotates = style_mut(control).is_some();
    let turned = fold.rotation.is_some() || fold.flip.is_some();
    let unstorable_offset = matches!(control, Control::Equation { .. });
    if !rotates && (turned || unstorable_offset) {
        // No rotation (or, for an equation, no offset) field: keep an
        // unrotated box with the same centre.
        let centre = input.map.apply((org_w / 2.0, org_h / 2.0));
        let x = round_i32(centre.0 - f64::from(fold.width) / 2.0 - input.parent_origin.0);
        let y = round_i32(centre.1 - f64::from(fold.height) / 2.0 - input.parent_origin.1);
        if !fits_hwpunit(&[x, y]) {
            return Err(vec![out_of_range()]);
        }
        set_box(control, fold.width, fold.height, (x, y));
        let lost_offset = unstorable_offset && (x, y) != (0, 0);
        if turned || lost_offset {
            let what = match (turned, lost_offset) {
                (true, true) => "rotation, mirror and offset",
                (true, false) => "rotation and mirror",
                _ => "offset",
            };
            return Ok(vec![FoldLoss::new(
                FoldLossKind::Unstorable,
                format!("{} has no field for its {what}", control.kind_name()),
            )]);
        }
        if let Some(shear) = shear {
            return Ok(vec![shear]);
        }
        let origin = (input.parent_origin.0 + f64::from(x), input.parent_origin.1 + f64::from(y));
        let ours: Vec<(f64, f64)> = box_corners(f64::from(fold.width), f64::from(fold.height))
            .iter()
            .map(|&(px, py)| (origin.0 + px, origin.1 + py))
            .collect();
        return Ok(residual_loss(max_residual(&wire[..4], &ours)).into_iter().collect());
    }
    set_box(control, fold.width, fold.height, fold.offset);
    set_rotation_and_flip(control, fold.rotation, fold.flip);
    if let Some(shear) = shear {
        // A sheared child is already reported; its residual says nothing new.
        return Ok(vec![shear]);
    }
    // Rebuilt from what the control now holds.
    let Some(b) = shape_box(control) else { return Ok(Vec::new()) };
    let (rotation, flip) = b.style.map_or((None, None), |s| (s.rotation, s.flip));
    let (w, h) = (f64::from(b.width), f64::from(b.height));
    let (l, _) = local_affine(w, h, rotation, flip, &EmissionPolicy::exact());
    let map = ShapeTransform::translate(
        input.parent_origin.0 + f64::from(b.offset.0),
        input.parent_origin.1 + f64::from(b.offset.1),
    )
    .then(l);
    let mut ours: Vec<(f64, f64)> = box_corners(w, h).iter().map(|&p| map.apply(p)).collect();
    let (px, py) = if preserve_original { (w / org_w, h / org_h) } else { (1.0, 1.0) };
    ours.extend(
        defining_points(control)
            .iter()
            .map(|p| map.apply((f64::from(p.x) * px, f64::from(p.y) * py))),
    );
    Ok(residual_loss(max_residual(&wire, &ours)).into_iter().collect())
}

/// Axis-aligned bounds of a set of points.
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::geometry::group_bounds;
///
/// let b = group_bounds([(3.0, 4.0), (-1.0, 9.0)]).unwrap();
/// assert_eq!((b.min_x(), b.min_y(), b.max_x(), b.max_y()), (-1.0, 4.0, 3.0, 9.0));
/// assert!(group_bounds(std::iter::empty()).is_none());
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl Bounds {
    /// Left edge.
    pub fn min_x(&self) -> f64 {
        self.min_x
    }

    /// Top edge.
    pub fn min_y(&self) -> f64 {
        self.min_y
    }

    /// Right edge.
    pub fn max_x(&self) -> f64 {
        self.max_x
    }

    /// Bottom edge.
    pub fn max_y(&self) -> f64 {
        self.max_y
    }
}

/// The bounds of the final corners of a group's children — for a shape,
/// [`emitted_corners`] with [`EmissionPolicy::exact`]. `None` when there is
/// no point.
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::geometry::{emitted_corners, group_bounds, EmissionPolicy};
/// use hwpforge_core::control::Control;
/// use hwpforge_foundation::HwpUnit;
///
/// let rect = Control::rect(HwpUnit::new(100).unwrap(), HwpUnit::new(50).unwrap()).unwrap();
/// let corners = emitted_corners(&rect, &EmissionPolicy::exact()).unwrap();
/// let b = group_bounds(corners).unwrap();
/// assert_eq!((b.max_x(), b.max_y()), (100.0, 50.0));
/// ```
pub fn group_bounds(corners: impl IntoIterator<Item = (f64, f64)>) -> Option<Bounds> {
    let mut it = corners.into_iter();
    let (x, y) = it.next()?;
    Some(it.fold(Bounds { min_x: x, min_y: y, max_x: x, max_y: y }, |b, (x, y)| Bounds {
        min_x: b.min_x.min(x),
        min_y: b.min_y.min(y),
        max_x: b.max_x.max(x),
        max_y: b.max_y.max(y),
    }))
}

#[cfg(test)]
mod tests;
