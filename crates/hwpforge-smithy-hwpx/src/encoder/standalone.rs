//! The single HWPX integer emission calculation for standalone drawings.
use crate::schema::section::{HxFlip, HxMatrix, HxRenderingInfo, HxRotationInfo, HxSizeAttr};
use hwpforge_core::control::geometry::{FramePlan, ShapeTransform};
use hwpforge_foundation::Flip;

pub(crate) struct StandalonePlan {
    pub size: (i32, i32),
    pub cur: HxSizeAttr,
    pub flip: HxFlip,
    pub rotation: HxRotationInfo,
    pub rendering: HxRenderingInfo,
    pub map: ShapeTransform,
}

pub(crate) fn transformed(rotation: Option<f32>, flip: Option<Flip>) -> bool {
    rotation.is_some_and(|v| v.rem_euclid(360.0) != 0.0)
        || matches!(flip, Some(Flip::Horizontal | Flip::Vertical | Flip::Both))
}

pub(crate) fn plan(
    size: (i32, i32),
    rotation: Option<f32>,
    flip: Option<Flip>,
    frame: &FramePlan,
) -> StandalonePlan {
    let (w, h) = size;
    let angle = rotation.unwrap_or(0.0).rem_euclid(360.0).round() as i32;
    let horizontal = matches!(flip, Some(Flip::Horizontal | Flip::Both));
    let vertical = matches!(flip, Some(Flip::Vertical | Flip::Both));
    let q = |v: f64| format!("{v:.6}").parse::<f64>().expect("finite coefficient");
    let (r, layout, (tx, ty)) =
        hwpforge_core::control::geometry::standalone_layout(size, rotation, flip);
    let [a, b, c, d, e, f] = r.matrix();
    let mut trans = HxMatrix::identity();
    trans.e3 = (tx).to_string();
    trans.e6 = (ty).to_string();
    let rot = HxMatrix {
        e1: format!("{a:.6}"),
        e2: format!("{b:.6}"),
        e3: format!("{c:.6}"),
        e4: format!("{d:.6}"),
        e5: format!("{e:.6}"),
        e6: format!("{f:.6}"),
    };
    let (sx, sy) = frame.scale();
    let stored = ShapeTransform::from_matrix([1.0, 0.0, f64::from(tx), 0.0, 1.0, f64::from(ty)])
        .then(r)
        .then(ShapeTransform::from_matrix([q(sx), 0.0, 0.0, 0.0, q(sy), 0.0]));
    let original = frame.original_size();
    StandalonePlan {
        size: layout,
        cur: HxSizeAttr {
            width: if w == original.0 { 0 } else { w },
            height: if h == original.1 { 0 } else { h },
        },
        flip: HxFlip { horizontal: u32::from(horizontal), vertical: u32::from(vertical) },
        rotation: HxRotationInfo {
            angle,
            center_x: w / 2 + tx,
            center_y: h / 2 + ty,
            rotate_image: 1,
        },
        rendering: HxRenderingInfo {
            trans_matrix: trans,
            sca_matrix: super::shapes::scale_matrix(frame.scale()),
            rot_matrix: rot,
            extra_pairs: Vec::new(),
        },
        map: stored,
    }
}

/// Compare exact Core points with the actual serialized T*R*S, including
/// the integer centre and the separately rounded placement translation.
pub(crate) fn residual(frame: &FramePlan, exact: &FramePlan, emitted: &StandalonePlan) -> f64 {
    let (ow, oh) = frame.original_size();
    let points =
        [(0., 0.), (f64::from(ow), 0.), (f64::from(ow), f64::from(oh)), (0., f64::from(oh))]
            .into_iter()
            .chain(frame.points().iter().map(|p| (f64::from(p.x), f64::from(p.y))));
    let minimum = exact.stored_positions()[..4]
        .iter()
        .fold((f64::INFINITY, f64::INFINITY), |m, p| (m.0.min(p.0), m.1.min(p.1)));
    exact
        .stored_positions()
        .iter()
        .zip(points)
        .map(|(p, q)| {
            let q = emitted.map.apply(q);
            (p.0 - minimum.0 - q.0).hypot(p.1 - minimum.1 - q.1)
        })
        .fold(0.0, f64::max)
}
