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
use crate::placement::{ObjectPlacement, ObjectRelativeTo, ObjectTextFlow, ObjectTextWrap};

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
    let det = m.a * m.e - m.b * m.d;
    let flip = det < 0.0;
    let angle = if flip { (-m.d).atan2(-m.a) } else { m.d.atan2(m.a) }.to_degrees();
    Some(BoxDecomposition {
        sx,
        sy,
        angle: angle.rem_euclid(360.0),
        flip,
        shear: (m.a * m.b + m.d * m.e).abs() / (sx * sy),
    })
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
    Some(ObjectPlacement {
        text_wrap: ObjectTextWrap::InFrontOfText,
        text_flow: ObjectTextFlow::BothSides,
        treat_as_char: false,
        flow_with_text: false,
        allow_overlap: true,
        vert_rel_to: ObjectRelativeTo::Paper,
        horz_rel_to: ObjectRelativeTo::Paper,
        vert_offset: HwpUnit::new(y).unwrap_or(HwpUnit::ZERO),
        horz_offset: HwpUnit::new(x).unwrap_or(HwpUnit::ZERO),
    })
}

fn defining_points(control: &Control) -> Vec<ShapePoint> {
    match control {
        Control::Line { start, end, .. } => vec![*start, *end],
        Control::Ellipse { center, axis1, axis2, .. } => vec![*center, *axis1, *axis2],
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
/// let Control::Rect { width, placement, .. } = *control else { unreachable!() };
/// assert_eq!(width.as_i32(), 150);
/// assert_eq!(placement.unwrap().horz_offset.as_i32(), 200);
/// ```
pub fn fold_group_child(mut control: Control, input: &FoldInput) -> FoldOutcome {
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
    if point_based && !turns {
        fold_points(control, input)
    } else {
        fold_boxed(&mut control, input).map_or_else(
            |losses| FoldOutcome::Dropped { losses },
            |losses| FoldOutcome::Kept { control: Box::new(control), losses },
        )
    }
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
    let collapsed = match control {
        // A polygon needs area.
        Control::Polygon { .. } => w == 0 || h == 0,
        // A line, curve or connector the map squeezes to a single point.
        _ => distinct && w == 0 && h == 0,
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
fn fold_boxed(control: &mut Control, input: &FoldInput) -> Result<Vec<FoldLoss>, Vec<FoldLoss>> {
    let (org_w, org_h) = input.original_size;
    let RawBoxFold { fold, sx, sy, shear } =
        fold_box_raw(org_w, org_h, input).map_err(|loss| vec![loss])?;
    let wire_local = defining_points(control);
    let mut wire: Vec<(f64, f64)> =
        box_corners(org_w, org_h).iter().map(|&p| input.map.apply(p)).collect();
    wire.extend(wire_local.iter().map(|p| input.map.apply((f64::from(p.x), f64::from(p.y)))));
    for (p, q) in defining_points_mut(control).into_iter().zip(&wire_local) {
        p.x = round_i32(f64::from(q.x) * sx);
        p.y = round_i32(f64::from(q.y) * sy);
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
    ours.extend(
        defining_points(control).iter().map(|p| map.apply((f64::from(p.x), f64::from(p.y)))),
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
