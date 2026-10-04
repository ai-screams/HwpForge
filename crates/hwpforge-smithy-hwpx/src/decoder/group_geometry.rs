//! Affine geometry for group (묶음) children.
//!
//! A shape inside `<hp:container>` has no `<hp:sz>`/`<hp:pos>`; its place is
//! the matrix chain in `<hp:renderingInfo>`: `trans, sca1, rot1, sca2, rot2,
//! …`, one `(sca, rot)` pair per group level. A wire-local point `p` lands
//! in the outermost group's coordinate space at
//!
//! ```text
//! M · p,   M = T · R1 · S1 · R2 · S2 · … · Rn · Sn
//! ```
//!
//! so the last pair (the shape's own) applies first and the first pair (the
//! one the outermost group gave after grouping) applies last. Leaves of a
//! nested group map straight into the outermost group's space; the matrices
//! of the groups in between are not on the leaf's path. Every matrix carries
//! its translation (`e3`, `e6`). This was checked shape by shape against
//! Hancom's own "ungroup" output (`tests/fixtures/shapes/group-f1-*`).

use crate::schema::section::{HxMatrix, HxRenderingInfo};

/// Largest distance, in HWPUNIT, between where the wire puts a defining
/// point and where the encoder would put it from the decoded Core value,
/// still treated as exact. Hancom itself rounds a reflected group's axis
/// to an even width, so its own "ungroup" result can differ from the exact
/// reflection by 1.23 (fixture `group-f1-g3-flipped`).
pub(crate) const EXACT_TOLERANCE: f64 = 2.0;

/// 2D affine map `x' = a·x + b·y + c`, `y' = d·x + e·y + f` — the wire's
/// `e1 … e6` in order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Affine {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Affine {
    /// The identity map.
    pub(crate) const IDENTITY: Self = Self { a: 1.0, b: 0.0, c: 0.0, d: 0.0, e: 1.0, f: 0.0 };

    /// A pure translation.
    pub(crate) fn translate(x: f64, y: f64) -> Self {
        Self { c: x, f: y, ..Self::IDENTITY }
    }

    /// Parses a wire matrix. `None` when an element is not a finite number.
    pub(crate) fn from_hx(m: &HxMatrix) -> Option<Self> {
        let p = |s: &str| s.trim().parse::<f64>().ok().filter(|v| v.is_finite());
        Some(Self {
            a: p(&m.e1)?,
            b: p(&m.e2)?,
            c: p(&m.e3)?,
            d: p(&m.e4)?,
            e: p(&m.e5)?,
            f: p(&m.e6)?,
        })
    }

    /// `self · other` (apply `other` first).
    pub(crate) fn then_after(self, other: Self) -> Self {
        Self {
            a: self.a * other.a + self.b * other.d,
            b: self.a * other.b + self.b * other.e,
            c: self.a * other.c + self.b * other.f + self.c,
            d: self.d * other.a + self.e * other.d,
            e: self.d * other.b + self.e * other.e,
            f: self.d * other.c + self.e * other.f + self.f,
        }
    }

    /// Maps a point.
    pub(crate) fn apply(self, (x, y): (f64, f64)) -> (f64, f64) {
        (self.a * x + self.b * y + self.c, self.d * x + self.e * y + self.f)
    }

    /// The same map as the Core type the shared fold takes.
    pub(crate) fn to_transform(self) -> hwpforge_core::control::geometry::ShapeTransform {
        hwpforge_core::control::geometry::ShapeTransform::from_matrix([
            self.a, self.b, self.c, self.d, self.e, self.f,
        ])
    }
}

/// The composed map `T · R1 · S1 · … · Rn · Sn` of a `<hp:renderingInfo>`.
/// `None` when a matrix element does not parse.
pub(crate) fn wire_chain(ri: &HxRenderingInfo) -> Option<Affine> {
    let mut m = Affine::from_hx(&ri.trans_matrix)?;
    for (sca, rot) in ri.pairs() {
        m = m.then_after(Affine::from_hx(rot)?).then_after(Affine::from_hx(sca)?);
    }
    Some(m)
}

/// Axis-aligned bounds `(min_x, min_y, max_x, max_y)` of a point list.
pub(crate) fn bounds(points: &[(f64, f64)]) -> Option<(f64, f64, f64, f64)> {
    let (first, rest) = points.split_first()?;
    Some(rest.iter().fold((first.0, first.1, first.0, first.1), |b, p| {
        (b.0.min(p.0), b.1.min(p.1), b.2.max(p.0), b.3.max(p.1))
    }))
}

/// The four corners of a `w × h` box at the origin.
pub(crate) fn corners(w: f64, h: f64) -> [(f64, f64); 4] {
    [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hx(e: [&str; 6]) -> HxMatrix {
        HxMatrix {
            e1: e[0].into(),
            e2: e[1].into(),
            e3: e[2].into(),
            e4: e[3].into(),
            e5: e[4].into(),
            e6: e[5].into(),
        }
    }

    fn ri(trans: HxMatrix, pairs: Vec<(HxMatrix, HxMatrix)>) -> HxRenderingInfo {
        let mut it = pairs.into_iter();
        let (sca_matrix, rot_matrix) = it.next().unwrap();
        HxRenderingInfo { trans_matrix: trans, sca_matrix, rot_matrix, extra_pairs: it.collect() }
    }

    const ID: [&str; 6] = ["1", "0", "0", "0", "1", "0"];

    /// Inside one pair the rotation applies after the scale: `R · S`.
    ///
    /// S = Diag(2, 1), R = 90°. `R·S·(1, 0)` = R·(2, 0) = (0, 2);
    /// `S·R·(1, 0)` = S·(0, 1) = (0, 1).
    ///
    /// 이것을 실패시키는 것: `wire_chain` 이 한 쌍 안에서 sca 를 rot 보다
    /// 먼저 곱하도록(`S·R`) 바꾸는 것.
    #[test]
    fn pair_applies_scale_then_rotation() {
        let r = ri(
            hx(ID),
            vec![(hx(["2", "0", "0", "0", "1", "0"]), hx(["0", "-1", "0", "1", "0", "0"]))],
        );
        let m = wire_chain(&r).unwrap();
        let (x, y) = m.apply((1.0, 0.0));
        assert!((x - 0.0).abs() < 1e-9 && (y - 2.0).abs() < 1e-9, "({x}, {y})");
    }

    /// The last pair is the shape's own and applies first; the first pair
    /// applies last. Pair 1 = scale x by 0.5 with translation 310; pair 2 =
    /// scale x by 3. `(10, 0)` → own ×3 → 30 → ×0.5 + 310 → 325.
    /// Reversed (first pair first): 10·0.5+310 = 315 → ×3 → 945.
    ///
    /// 이것을 실패시키는 것: 쌍을 첫 쌍부터 적용하는 것, 또는 sca 의 이동
    /// 성분(e3)을 버리는 것 (→ 15).
    #[test]
    fn last_pair_applies_first_and_keeps_its_translation() {
        let r = ri(
            hx(ID),
            vec![
                (hx(["0.5", "0", "310", "0", "1", "0"]), hx(ID)),
                (hx(["3", "0", "0", "0", "1", "0"]), hx(ID)),
            ],
        );
        let (x, _) = wire_chain(&r).unwrap().apply((10.0, 0.0));
        assert!((x - 325.0).abs() < 1e-9, "{x}");
    }

    #[test]
    fn unparseable_matrix_is_none() {
        let r = ri(hx(["1", "0", "x", "0", "1", "0"]), vec![(hx(ID), hx(ID))]);
        assert!(wire_chain(&r).is_none());
    }
}
