//! Small column-major matrix helpers (same layout as three.js `elements`).
pub type M4 = [f32; 16];

pub fn mul(a: &M4, b: &M4) -> M4 {
    let mut o = [0f32; 16];
    for c in 0..4 { for r in 0..4 { let mut s = 0.0; for k in 0..4 { s += a[k * 4 + r] * b[c * 4 + k]; } o[c * 4 + r] = s; } }
    o
}
pub fn transform_point(m: &M4, p: [f32; 3]) -> [f32; 3] {
    [m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12], m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13], m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14]]
}
pub fn max_scale(m: &M4) -> f32 {
    let sx = m[0] * m[0] + m[1] * m[1] + m[2] * m[2]; let sy = m[4] * m[4] + m[5] * m[5] + m[6] * m[6]; let sz = m[8] * m[8] + m[9] * m[9] + m[10] * m[10];
    sx.max(sy).max(sz).sqrt()
}
pub fn det3(m: &M4) -> f32 {
    m[0] * (m[5] * m[10] - m[9] * m[6]) - m[4] * (m[1] * m[10] - m[9] * m[2]) + m[8] * (m[1] * m[6] - m[5] * m[2])
}
/// Inverse-transpose of the upper 3x3 (column-major 3x3 result).
pub fn normal_matrix(m: &M4) -> [f32; 9] {
    let (a, b, c, d, e, f, g, h, i) = (m[0], m[4], m[8], m[1], m[5], m[9], m[2], m[6], m[10]); // row-major a b c / d e f / g h i
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    if det.abs() < 1e-12 { return [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]; }
    let k = 1.0 / det;
    // inverse (row-major) entries
    let inv = [(e * i - f * h) * k, (c * h - b * i) * k, (b * f - c * e) * k,
               (f * g - d * i) * k, (a * i - c * g) * k, (c * d - a * f) * k,
               (d * h - e * g) * k, (b * g - a * h) * k, (a * e - b * d) * k];
    // transpose of inverse, stored column-major == inverse stored row-major
    inv
}
/// Six planes (a,b,c,d) of the clip volume for a combined projection*view matrix.
pub fn frustum(pv: &M4) -> [[f32; 4]; 6] {
    let m = pv; let row = |r: usize| [m[r], m[4 + r], m[8 + r], m[12 + r]];
    let (r0, r1, r2, r3) = (row(0), row(1), row(2), row(3));
    let mut p = [[0f32; 4]; 6];
    for k in 0..4 { p[0][k] = r3[k] + r0[k]; p[1][k] = r3[k] - r0[k]; p[2][k] = r3[k] + r1[k]; p[3][k] = r3[k] - r1[k]; p[4][k] = r3[k] + r2[k]; p[5][k] = r3[k] - r2[k]; }
    for pl in p.iter_mut() { let l = (pl[0] * pl[0] + pl[1] * pl[1] + pl[2] * pl[2]).sqrt(); if l > 0.0 { for v in pl.iter_mut() { *v /= l; } } }
    p
}
pub fn sphere_visible(pl: &[[f32; 4]; 6], c: [f32; 3], r: f32) -> bool {
    pl.iter().all(|p| p[0] * c[0] + p[1] * c[1] + p[2] * c[2] + p[3] >= -r)
}
