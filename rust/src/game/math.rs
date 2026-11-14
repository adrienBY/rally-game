//! three.js conventions in f64: column-major 4x4 matrices, Euler orders "XYZ" / "YXZ", cameras looking down -Z.
pub type M = [f64; 16];
pub type V3 = [f64; 3];

pub const ID: M = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

pub fn mul(a: &M, b: &M) -> M {
    let mut o = [0.0; 16];
    for c in 0..4 { for r in 0..4 { let mut s = 0.0; for k in 0..4 { s += a[k * 4 + r] * b[c * 4 + k]; } o[c * 4 + r] = s; } }
    o
}
pub fn to_f32(m: &M) -> [f32; 16] { let mut o = [0f32; 16]; for i in 0..16 { o[i] = m[i] as f32; } o }
pub fn from_slice(v: &[f64]) -> M { let mut m = ID; for i in 0..16.min(v.len()) { m[i] = v[i]; } m }

/// rotation part of Matrix4.makeRotationFromEuler (three.js)
pub fn euler(x: f64, y: f64, z: f64, yxz: bool) -> M {
    let (a, b) = (x.cos(), x.sin()); let (c, d) = (y.cos(), y.sin()); let (e, f) = (z.cos(), z.sin());
    let mut t = ID;
    if yxz {
        let (ce, cf, de, df) = (c * e, c * f, d * e, d * f);
        t[0] = ce + df * b; t[4] = de * b - cf; t[8] = a * d;
        t[1] = a * f; t[5] = a * e; t[9] = -b;
        t[2] = cf * b - de; t[6] = df + ce * b; t[10] = a * c;
    } else {
        let (ae, af, be, bf) = (a * e, a * f, b * e, b * f);
        t[0] = c * e; t[4] = -c * f; t[8] = d;
        t[1] = af + be * d; t[5] = ae - bf * d; t[9] = -b * c;
        t[2] = bf - ae * d; t[6] = be + af * d; t[10] = a * c;
    }
    t
}
/// position * rotation * scale
pub fn compose(p: V3, rot: &M, s: V3) -> M {
    let mut m = *rot;
    for k in 0..3 { m[k] *= s[0]; m[4 + k] *= s[1]; m[8 + k] *= s[2]; }
    m[12] = p[0]; m[13] = p[1]; m[14] = p[2]; m[3] = 0.0; m[7] = 0.0; m[11] = 0.0; m[15] = 1.0;
    m
}
pub fn trs(p: V3, r: V3, yxz: bool, s: V3) -> M { compose(p, &euler(r[0], r[1], r[2], yxz), s) }

pub fn apply(m: &M, v: V3) -> V3 {
    [m[0] * v[0] + m[4] * v[1] + m[8] * v[2] + m[12], m[1] * v[0] + m[5] * v[1] + m[9] * v[2] + m[13], m[2] * v[0] + m[6] * v[1] + m[10] * v[2] + m[14]]
}
/// Vector3.project: world -> NDC through view and projection (with the perspective divide)
pub fn project(view: &M, proj: &M, v: V3) -> V3 {
    let a = apply(view, v);
    let x = proj[0] * a[0] + proj[4] * a[1] + proj[8] * a[2] + proj[12];
    let y = proj[1] * a[0] + proj[5] * a[1] + proj[9] * a[2] + proj[13];
    let z = proj[2] * a[0] + proj[6] * a[1] + proj[10] * a[2] + proj[14];
    let w = proj[3] * a[0] + proj[7] * a[1] + proj[11] * a[2] + proj[15];
    let iw = 1.0 / w; [x * iw, y * iw, z * iw]
}
/// inverse of a rigid (rotation + translation, possibly uniformly scaled) matrix; general enough for cameras
pub fn invert(m: &M) -> M {
    // full 4x4 inverse (three.js Matrix4.invert)
    let (n11, n21, n31, n41) = (m[0], m[1], m[2], m[3]);
    let (n12, n22, n32, n42) = (m[4], m[5], m[6], m[7]);
    let (n13, n23, n33, n43) = (m[8], m[9], m[10], m[11]);
    let (n14, n24, n34, n44) = (m[12], m[13], m[14], m[15]);
    let t11 = n23 * n34 * n42 - n24 * n33 * n42 + n24 * n32 * n43 - n22 * n34 * n43 - n23 * n32 * n44 + n22 * n33 * n44;
    let t12 = n14 * n33 * n42 - n13 * n34 * n42 - n14 * n32 * n43 + n12 * n34 * n43 + n13 * n32 * n44 - n12 * n33 * n44;
    let t13 = n13 * n24 * n42 - n14 * n23 * n42 + n14 * n22 * n43 - n12 * n24 * n43 - n13 * n22 * n44 + n12 * n23 * n44;
    let t14 = n14 * n23 * n32 - n13 * n24 * n32 - n14 * n22 * n33 + n12 * n24 * n33 + n13 * n22 * n34 - n12 * n23 * n34;
    let det = n11 * t11 + n21 * t12 + n31 * t13 + n41 * t14;
    if det == 0.0 { return [0.0; 16]; }
    let d = 1.0 / det; let mut o = [0.0; 16];
    o[0] = t11 * d;
    o[1] = (n24 * n33 * n41 - n23 * n34 * n41 - n24 * n31 * n43 + n21 * n34 * n43 + n23 * n31 * n44 - n21 * n33 * n44) * d;
    o[2] = (n22 * n34 * n41 - n24 * n32 * n41 + n24 * n31 * n42 - n21 * n34 * n42 - n22 * n31 * n44 + n21 * n32 * n44) * d;
    o[3] = (n23 * n32 * n41 - n22 * n33 * n41 - n23 * n31 * n42 + n21 * n33 * n42 + n22 * n31 * n43 - n21 * n32 * n43) * d;
    o[4] = t12 * d;
    o[5] = (n13 * n34 * n41 - n14 * n33 * n41 + n14 * n31 * n43 - n11 * n34 * n43 - n13 * n31 * n44 + n11 * n33 * n44) * d;
    o[6] = (n14 * n32 * n41 - n12 * n34 * n41 - n14 * n31 * n42 + n11 * n34 * n42 + n12 * n31 * n44 - n11 * n32 * n44) * d;
    o[7] = (n12 * n33 * n41 - n13 * n32 * n41 + n13 * n31 * n42 - n11 * n33 * n42 - n12 * n31 * n43 + n11 * n32 * n43) * d;
    o[8] = t13 * d;
    o[9] = (n14 * n23 * n41 - n13 * n24 * n41 - n14 * n21 * n43 + n11 * n24 * n43 + n13 * n21 * n44 - n11 * n23 * n44) * d;
    o[10] = (n12 * n24 * n41 - n14 * n22 * n41 + n14 * n21 * n42 - n11 * n24 * n42 - n12 * n21 * n44 + n11 * n22 * n44) * d;
    o[11] = (n13 * n22 * n41 - n12 * n23 * n41 - n13 * n21 * n42 + n11 * n23 * n42 + n12 * n21 * n43 - n11 * n22 * n43) * d;
    o[12] = t14 * d;
    o[13] = (n13 * n24 * n31 - n14 * n23 * n31 + n14 * n21 * n33 - n11 * n24 * n33 - n13 * n21 * n34 + n11 * n23 * n34) * d;
    o[14] = (n14 * n22 * n31 - n12 * n24 * n31 - n14 * n21 * n32 + n11 * n24 * n32 + n12 * n21 * n34 - n11 * n22 * n34) * d;
    o[15] = (n12 * n23 * n31 - n13 * n22 * n31 + n13 * n21 * n32 - n11 * n23 * n32 - n12 * n21 * n33 + n11 * n22 * n33) * d;
    o
}
fn norm(v: V3) -> V3 { let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt(); if l == 0.0 { v } else { [v[0] / l, v[1] / l, v[2] / l] } }
fn cross(a: V3, b: V3) -> V3 { [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]] }
fn len2(v: V3) -> f64 { v[0] * v[0] + v[1] * v[1] + v[2] * v[2] }
/// Object3D.lookAt for a camera (Matrix4.lookAt(eye, target, up)): rotation only
pub fn look_rot(eye: V3, target: V3) -> M {
    let up = [0.0, 1.0, 0.0];
    let mut z = [eye[0] - target[0], eye[1] - target[1], eye[2] - target[2]];
    if len2(z) == 0.0 { z[2] = 1.0; }
    z = norm(z);
    let mut x = cross(up, z);
    if len2(x) == 0.0 { if up[2].abs() == 1.0 { z[0] += 0.0001; } else { z[2] += 0.0001; } z = norm(z); x = cross(up, z); }
    x = norm(x); let y = cross(z, x);
    let mut m = ID;
    m[0] = x[0]; m[1] = x[1]; m[2] = x[2]; m[4] = y[0]; m[5] = y[1]; m[6] = y[2]; m[8] = z[0]; m[9] = z[1]; m[10] = z[2];
    m
}
/// PerspectiveCamera.updateProjectionMatrix (zoom 1, no view offset)
pub fn perspective(fov_deg: f64, aspect: f64, near: f64, far: f64) -> M {
    let top = near * (fov_deg.to_radians() * 0.5).tan(); let height = 2.0 * top; let width = aspect * height; let left = -0.5 * width;
    let (r, l, t, b) = (left + width, left, top, top - height);
    let mut m = [0.0; 16];
    m[0] = 2.0 * near / (r - l); m[5] = 2.0 * near / (t - b); m[8] = (r + l) / (r - l); m[9] = (t + b) / (t - b);
    m[10] = -(far + near) / (far - near); m[11] = -1.0; m[14] = -2.0 * far * near / (far - near);
    m
}
