//! Homogeneous CSS planes. Matrices use the CSS/DOMMatrix column-major order.
use crate::{Command, Rect, Transform2D};
pub type Matrix4 = euclid::default::Transform3D<f32>;
#[derive(Debug, Clone, PartialEq)]
pub struct SpatialLayer {
    pub matrix: [f32; 16],
    pub perspective: [f32; 16],
    pub preserve_3d: bool,
    pub backface_hidden: bool,
    pub commands: Vec<Command>,
    pub z: i32,
    pub opacity: f32,
    pub clip: Option<Rect>,
}
pub fn affine(m: Transform2D) -> Matrix4 {
    let [a, b, c, d, e, f] = m.m;
    Matrix4::new(a, b, 0., 0., c, d, 0., 0., 0., 0., 1., 0., e, f, 0., 1.)
}
pub fn multiply(a: Matrix4, b: Matrix4) -> Matrix4 {
    b.then(&a)
}
pub fn transform(m: Matrix4, p: [f32; 4]) -> [f32; 4] {
    let a = m.to_array();
    std::array::from_fn(|row| (0..4).map(|col| a[col * 4 + row] * p[col]).sum())
}
pub fn quad(m: Matrix4, r: Rect) -> [[f32; 4]; 4] {
    [
        [r.x, r.y],
        [r.x + r.w, r.y],
        [r.x + r.w, r.y + r.h],
        [r.x, r.y + r.h],
    ]
    .map(|[x, y]| transform(m, [x, y, 0., 1.]))
}
/// Clip a plane to the CSS camera half-space before bounds/depth reduction.
pub fn camera_clip(points: &[[f32; 4]]) -> Vec<[f32; 4]> {
    let mut result = Vec::new();
    for i in 0..points.len() {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        let inside_a = a[3] >= 1e-5;
        let inside_b = b[3] >= 1e-5;
        if inside_a {
            result.push(a);
        }
        if inside_a != inside_b {
            let t = (1e-5 - a[3]) / (b[3] - a[3]);
            result.push(std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t));
        }
    }
    result
}
pub fn bounds(points: &[[f32; 4]]) -> Option<Rect> {
    let p = camera_clip(points);
    if p.is_empty() {
        return None;
    }
    let mut lo = [f32::INFINITY; 2];
    let mut hi = [f32::NEG_INFINITY; 2];
    for p in p {
        for i in 0..2 {
            let v = p[i] / p[3];
            lo[i] = lo[i].min(v);
            hi[i] = hi[i].max(v);
        }
    }
    Some(Rect {
        x: lo[0],
        y: lo[1],
        w: hi[0] - lo[0],
        h: hi[1] - lo[1],
    })
}
pub fn back_facing(m: Matrix4) -> bool {
    // CSS Transforms: sign of the inverse matrix's m33, not projected winding.
    m.inverse().is_none_or(|inverse| inverse.m33 < 0.)
}
