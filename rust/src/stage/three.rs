//! The little part of three.js (r160) the stage builder used, ported so the scenes come out the same: vectors, quaternions, Euler
//! angles, matrices, colours, the scene graph, buffer geometries with the generators the game uses, materials, textures, instanced
//! meshes, and the scene packet ("FWS1") the renderer loads. Numbers follow three's own arithmetic (f64 math, f32 storage).
use std::f64::consts::PI;

// ---------------------------------------------------------------- math
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct V3 { pub x: f64, pub y: f64, pub z: f64 }
pub const fn v3(x: f64, y: f64, z: f64) -> V3 { V3 { x, y, z } }
impl V3 {
    pub fn add(self, o: V3) -> V3 { v3(self.x + o.x, self.y + o.y, self.z + o.z) }
    pub fn sub(self, o: V3) -> V3 { v3(self.x - o.x, self.y - o.y, self.z - o.z) }
    pub fn mul(self, s: f64) -> V3 { v3(self.x * s, self.y * s, self.z * s) }
    pub fn dot(self, o: V3) -> f64 { self.x * o.x + self.y * o.y + self.z * o.z }
    pub fn cross(self, o: V3) -> V3 { v3(self.y * o.z - self.z * o.y, self.z * o.x - self.x * o.z, self.x * o.y - self.y * o.x) }
    pub fn len_sq(self) -> f64 { self.x * self.x + self.y * self.y + self.z * self.z }
    pub fn len(self) -> f64 { self.len_sq().sqrt() }
    /// three's normalize: divide by the length, or by 1 for a zero vector
    pub fn norm(self) -> V3 { let l = self.len(); let l = if l == 0.0 { 1.0 } else { l }; self.mul(1.0 / l) }
    pub fn lerp(self, o: V3, t: f64) -> V3 { v3(self.x + (o.x - self.x) * t, self.y + (o.y - self.y) * t, self.z + (o.z - self.z) * t) }
    pub fn dist_sq(self, o: V3) -> f64 { self.sub(o).len_sq() }
    pub fn apply_m4(self, m: &M4) -> V3 {
        let e = &m.0; let (x, y, z) = (self.x, self.y, self.z); let w = 1.0 / (e[3] * x + e[7] * y + e[11] * z + e[15]);
        v3((e[0] * x + e[4] * y + e[8] * z + e[12]) * w, (e[1] * x + e[5] * y + e[9] * z + e[13]) * w, (e[2] * x + e[6] * y + e[10] * z + e[14]) * w)
    }
    pub fn apply_quat(self, q: Quat) -> V3 {
        let (vx, vy, vz) = (self.x, self.y, self.z);
        let tx = 2.0 * (q.y * vz - q.z * vy); let ty = 2.0 * (q.z * vx - q.x * vz); let tz = 2.0 * (q.x * vy - q.y * vx);
        v3(vx + q.w * tx + q.y * tz - q.z * ty, vy + q.w * ty + q.z * tx - q.x * tz, vz + q.w * tz + q.x * ty - q.y * tx)
    }
    pub fn apply_axis_angle(self, axis: V3, a: f64) -> V3 { self.apply_quat(Quat::axis_angle(axis, a)) }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quat { pub x: f64, pub y: f64, pub z: f64, pub w: f64 }
impl Default for Quat { fn default() -> Self { Quat { x: 0.0, y: 0.0, z: 0.0, w: 1.0 } } }
impl Quat {
    pub fn from_euler(x: f64, y: f64, z: f64, yxz: bool) -> Quat {
        let (c1, c2, c3) = ((x / 2.0).cos(), (y / 2.0).cos(), (z / 2.0).cos()); let (s1, s2, s3) = ((x / 2.0).sin(), (y / 2.0).sin(), (z / 2.0).sin());
        if yxz { Quat { x: s1 * c2 * c3 + c1 * s2 * s3, y: c1 * s2 * c3 - s1 * c2 * s3, z: c1 * c2 * s3 - s1 * s2 * c3, w: c1 * c2 * c3 + s1 * s2 * s3 } }
        else { Quat { x: s1 * c2 * c3 + c1 * s2 * s3, y: c1 * s2 * c3 - s1 * c2 * s3, z: c1 * c2 * s3 + s1 * s2 * c3, w: c1 * c2 * c3 - s1 * s2 * s3 } }
    }
    pub fn axis_angle(a: V3, ang: f64) -> Quat { let h = ang / 2.0; let s = h.sin(); Quat { x: a.x * s, y: a.y * s, z: a.z * s, w: h.cos() } }
    pub fn mul(self, b: Quat) -> Quat {
        let a = self;
        Quat { x: a.x * b.w + a.w * b.x + a.y * b.z - a.z * b.y, y: a.y * b.w + a.w * b.y + a.z * b.x - a.x * b.z,
            z: a.z * b.w + a.w * b.z + a.x * b.y - a.y * b.x, w: a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z }
    }
    /// from the upper 3x3 of a pure rotation matrix
    pub fn from_rot(m: &M4) -> Quat {
        let te = &m.0; let (m11, m12, m13, m21, m22, m23, m31, m32, m33) = (te[0], te[4], te[8], te[1], te[5], te[9], te[2], te[6], te[10]);
        let tr = m11 + m22 + m33;
        if tr > 0.0 { let s = 0.5 / (tr + 1.0).sqrt(); Quat { w: 0.25 / s, x: (m32 - m23) * s, y: (m13 - m31) * s, z: (m21 - m12) * s } }
        else if m11 > m22 && m11 > m33 { let s = 2.0 * (1.0 + m11 - m22 - m33).sqrt(); Quat { w: (m32 - m23) / s, x: 0.25 * s, y: (m12 + m21) / s, z: (m13 + m31) / s } }
        else if m22 > m33 { let s = 2.0 * (1.0 + m22 - m11 - m33).sqrt(); Quat { w: (m13 - m31) / s, x: (m12 + m21) / s, y: 0.25 * s, z: (m23 + m32) / s } }
        else { let s = 2.0 * (1.0 + m33 - m11 - m22).sqrt(); Quat { w: (m21 - m12) / s, x: (m13 + m31) / s, y: (m23 + m32) / s, z: 0.25 * s } }
    }
}

/// column-major 4x4, as three's Matrix4.elements
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct M4(pub [f64; 16]);
impl Default for M4 { fn default() -> Self { M4::ID } }
impl M4 {
    pub const ID: M4 = M4([1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
    pub fn compose(p: V3, q: Quat, s: V3) -> M4 {
        let (x, y, z, w) = (q.x, q.y, q.z, q.w); let (x2, y2, z2) = (x + x, y + y, z + z);
        let (xx, xy, xz) = (x * x2, x * y2, x * z2); let (yy, yz, zz) = (y * y2, y * z2, z * z2); let (wx, wy, wz) = (w * x2, w * y2, w * z2);
        M4([(1.0 - (yy + zz)) * s.x, (xy + wz) * s.x, (xz - wy) * s.x, 0.0, (xy - wz) * s.y, (1.0 - (xx + zz)) * s.y, (yz + wx) * s.y, 0.0,
            (xz + wy) * s.z, (yz - wx) * s.z, (1.0 - (xx + yy)) * s.z, 0.0, p.x, p.y, p.z, 1.0])
    }
    pub fn mul(&self, b: &M4) -> M4 {
        let (a, b) = (&self.0, &b.0); let mut o = [0.0; 16];
        for r in 0..4 { for c in 0..4 { o[c * 4 + r] = a[r] * b[c * 4] + a[4 + r] * b[c * 4 + 1] + a[8 + r] * b[c * 4 + 2] + a[12 + r] * b[c * 4 + 3]; } }
        M4(o)
    }
    pub fn rot_x(t: f64) -> M4 { let (c, s) = (t.cos(), t.sin()); M4([1.0, 0.0, 0.0, 0.0, 0.0, c, s, 0.0, 0.0, -s, c, 0.0, 0.0, 0.0, 0.0, 1.0]) }
    pub fn rot_y(t: f64) -> M4 { let (c, s) = (t.cos(), t.sin()); M4([c, 0.0, -s, 0.0, 0.0, 1.0, 0.0, 0.0, s, 0.0, c, 0.0, 0.0, 0.0, 0.0, 1.0]) }
    pub fn rot_z(t: f64) -> M4 { let (c, s) = (t.cos(), t.sin()); M4([c, s, 0.0, 0.0, -s, c, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]) }
    pub fn trans(x: f64, y: f64, z: f64) -> M4 { let mut m = M4::ID; m.0[12] = x; m.0[13] = y; m.0[14] = z; m }
    pub fn scale(x: f64, y: f64, z: f64) -> M4 { let mut m = M4::ID; m.0[0] = x; m.0[5] = y; m.0[10] = z; m }
    /// makeBasis(x, y, z).setPosition(p)
    pub fn basis(x: V3, y: V3, z: V3, p: V3) -> M4 { M4([x.x, x.y, x.z, 0.0, y.x, y.y, y.z, 0.0, z.x, z.y, z.z, 0.0, p.x, p.y, p.z, 1.0]) }
    pub fn det(&self) -> f64 {
        let te = &self.0;
        let (n11, n12, n13, n14) = (te[0], te[4], te[8], te[12]); let (n21, n22, n23, n24) = (te[1], te[5], te[9], te[13]);
        let (n31, n32, n33, n34) = (te[2], te[6], te[10], te[14]); let (n41, n42, n43, n44) = (te[3], te[7], te[11], te[15]);
        n41 * (n14 * n23 * n32 - n13 * n24 * n32 - n14 * n22 * n33 + n12 * n24 * n33 + n13 * n22 * n34 - n12 * n23 * n34)
            + n42 * (n11 * n23 * n34 - n11 * n24 * n33 + n14 * n21 * n33 - n13 * n21 * n34 + n13 * n24 * n31 - n14 * n23 * n31)
            + n43 * (n11 * n24 * n32 - n11 * n22 * n34 - n14 * n21 * n32 + n12 * n21 * n34 + n14 * n22 * n31 - n12 * n24 * n31)
            + n44 * (-n13 * n22 * n31 - n11 * n23 * n32 + n11 * n22 * n33 + n13 * n21 * n32 - n12 * n21 * n33 + n12 * n23 * n31)
    }
    pub fn invert(&self) -> M4 {
        let te = &self.0;
        let (n11, n21, n31, n41) = (te[0], te[1], te[2], te[3]); let (n12, n22, n32, n42) = (te[4], te[5], te[6], te[7]);
        let (n13, n23, n33, n43) = (te[8], te[9], te[10], te[11]); let (n14, n24, n34, n44) = (te[12], te[13], te[14], te[15]);
        let t11 = n23 * n34 * n42 - n24 * n33 * n42 + n24 * n32 * n43 - n22 * n34 * n43 - n23 * n32 * n44 + n22 * n33 * n44;
        let t12 = n14 * n33 * n42 - n13 * n34 * n42 - n14 * n32 * n43 + n12 * n34 * n43 + n13 * n32 * n44 - n12 * n33 * n44;
        let t13 = n13 * n24 * n42 - n14 * n23 * n42 + n14 * n22 * n43 - n12 * n24 * n43 - n13 * n22 * n44 + n12 * n23 * n44;
        let t14 = n14 * n23 * n32 - n13 * n24 * n32 - n14 * n22 * n33 + n12 * n24 * n33 + n13 * n22 * n34 - n12 * n23 * n34;
        let det = n11 * t11 + n21 * t12 + n31 * t13 + n41 * t14;
        if det == 0.0 { return M4([0.0; 16]); }
        let di = 1.0 / det; let mut o = [0.0; 16];
        o[0] = t11 * di; o[1] = (n24 * n33 * n41 - n23 * n34 * n41 - n24 * n31 * n43 + n21 * n34 * n43 + n23 * n31 * n44 - n21 * n33 * n44) * di;
        o[2] = (n22 * n34 * n41 - n24 * n32 * n41 + n24 * n31 * n42 - n21 * n34 * n42 - n22 * n31 * n44 + n21 * n32 * n44) * di;
        o[3] = (n23 * n32 * n41 - n22 * n33 * n41 - n23 * n31 * n42 + n21 * n33 * n42 + n22 * n31 * n43 - n21 * n32 * n43) * di;
        o[4] = t12 * di; o[5] = (n13 * n34 * n41 - n14 * n33 * n41 + n14 * n31 * n43 - n11 * n34 * n43 - n13 * n31 * n44 + n11 * n33 * n44) * di;
        o[6] = (n14 * n32 * n41 - n12 * n34 * n41 - n14 * n31 * n42 + n11 * n34 * n42 + n12 * n31 * n44 - n11 * n32 * n44) * di;
        o[7] = (n12 * n33 * n41 - n13 * n32 * n41 + n13 * n31 * n42 - n11 * n33 * n42 - n12 * n31 * n43 + n11 * n32 * n43) * di;
        o[8] = t13 * di; o[9] = (n14 * n23 * n41 - n13 * n24 * n41 - n14 * n21 * n43 + n11 * n24 * n43 + n13 * n21 * n44 - n11 * n23 * n44) * di;
        o[10] = (n12 * n24 * n41 - n14 * n22 * n41 + n14 * n21 * n42 - n11 * n24 * n42 - n12 * n21 * n44 + n11 * n22 * n44) * di;
        o[11] = (n13 * n22 * n41 - n12 * n23 * n41 - n13 * n21 * n42 + n11 * n23 * n42 + n12 * n21 * n43 - n11 * n22 * n43) * di;
        o[12] = t14 * di; o[13] = (n13 * n24 * n31 - n14 * n23 * n31 + n14 * n21 * n33 - n11 * n24 * n33 - n13 * n21 * n34 + n11 * n23 * n34) * di;
        o[14] = (n14 * n22 * n31 - n12 * n24 * n31 - n14 * n21 * n32 + n11 * n24 * n32 + n12 * n21 * n34 - n11 * n22 * n34) * di;
        o[15] = (n12 * n23 * n31 - n13 * n22 * n31 + n13 * n21 * n32 - n11 * n23 * n32 - n12 * n21 * n33 + n11 * n22 * n33) * di;
        M4(o)
    }
    /// the normal matrix (inverse transpose of the upper 3x3), as a 3x3 row set applied by `apply_normal`
    pub fn normal_matrix(&self) -> [f64; 9] {
        let e = &self.0; // Matrix3.setFromMatrix4 then invert then transpose
        let (n11, n21, n31, n12, n22, n32, n13, n23, n33) = (e[0], e[1], e[2], e[4], e[5], e[6], e[8], e[9], e[10]);
        let t11 = n33 * n22 - n32 * n23; let t12 = n32 * n13 - n33 * n12; let t13 = n23 * n12 - n22 * n13;
        let det = n11 * t11 + n21 * t12 + n31 * t13;
        if det == 0.0 { return [0.0; 9]; }
        let di = 1.0 / det;
        // inverse (column-major te)
        let te = [t11 * di, (n31 * n23 - n33 * n21) * di, (n32 * n21 - n31 * n22) * di, t12 * di, (n33 * n11 - n31 * n13) * di, (n31 * n12 - n32 * n11) * di,
            t13 * di, (n21 * n13 - n23 * n11) * di, (n22 * n11 - n21 * n12) * di];
        // transpose
        [te[0], te[3], te[6], te[1], te[4], te[7], te[2], te[5], te[8]]
    }
    pub fn max_scale(&self) -> f64 {
        let te = &self.0;
        let sx = te[0] * te[0] + te[1] * te[1] + te[2] * te[2]; let sy = te[4] * te[4] + te[5] * te[5] + te[6] * te[6]; let sz = te[8] * te[8] + te[9] * te[9] + te[10] * te[10];
        sx.max(sy).max(sz).sqrt()
    }
    pub fn pos(&self) -> V3 { v3(self.0[12], self.0[13], self.0[14]) }
    /// the matrix as an InstancedMesh stores it (Float32Array) and reads it back
    pub fn f32r(&self) -> M4 { let mut o = self.0; for v in o.iter_mut() { *v = *v as f32 as f64; } M4(o) }
    /// Matrix4.lookAt(eye, target, up): the rotation part only
    pub fn look_at(eye: V3, target: V3, up: V3) -> M4 {
        let mut z = eye.sub(target); if z.len_sq() == 0.0 { z.z = 1.0; } z = z.norm();
        let mut x = up.cross(z);
        if x.len_sq() == 0.0 { if up.z.abs() == 1.0 { z.x += 0.0001; } else { z.z += 0.0001; } z = z.norm(); x = up.cross(z); }
        x = x.norm(); let y = z.cross(x);
        M4([x.x, x.y, x.z, 0.0, y.x, y.y, y.z, 0.0, z.x, z.y, z.z, 0.0, 0.0, 0.0, 0.0, 1.0])
    }
}
/// a normal through a normal matrix (Vector3.applyNormalMatrix: m3 then normalize)
fn apply_nm(nm: &[f64; 9], n: V3) -> V3 {
    // nm rows as Matrix3 elements column-major: e = [n11,n21,n31,n12,n22,n32,n13,n23,n33]
    let e = nm; v3(e[0] * n.x + e[3] * n.y + e[6] * n.z, e[1] * n.x + e[4] * n.y + e[7] * n.z, e[2] * n.x + e[5] * n.y + e[8] * n.z).norm()
}

// ---------------------------------------------------------------- colour (colour management off: plain numbers)
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Col { pub r: f64, pub g: f64, pub b: f64 }
pub const fn rgb(r: f64, g: f64, b: f64) -> Col { Col { r, g, b } }
pub const WHITE: Col = rgb(1.0, 1.0, 1.0);
impl Col {
    pub fn hex(h: u32) -> Col { rgb(((h >> 16) & 255) as f64 / 255.0, ((h >> 8) & 255) as f64 / 255.0, (h & 255) as f64 / 255.0) }
    /// "#rrggbb" / "#rgb"
    pub fn css(s: &str) -> Col {
        let t = s.trim_start_matches('#');
        if t.len() == 3 { let v = u32::from_str_radix(t, 16).unwrap_or(0); let (r, g, b) = ((v >> 8) & 15, (v >> 4) & 15, v & 15); return rgb((r * 17) as f64 / 255.0, (g * 17) as f64 / 255.0, (b * 17) as f64 / 255.0); }
        Col::hex(u32::from_str_radix(t, 16).unwrap_or(0))
    }
    pub fn hsl(h: f64, s: f64, l: f64) -> Col {
        let h = ((h % 1.0) + 1.0) % 1.0; let s = s.clamp(0.0, 1.0); let l = l.clamp(0.0, 1.0);
        if s == 0.0 { return rgb(l, l, l); }
        let p = if l <= 0.5 { l * (1.0 + s) } else { l + s - l * s }; let q = 2.0 * l - p;
        let f = |p: f64, q: f64, mut t: f64| { if t < 0.0 { t += 1.0; } if t > 1.0 { t -= 1.0; } if t < 1.0 / 6.0 { return p + (q - p) * 6.0 * t; } if t < 0.5 { return q; } if t < 2.0 / 3.0 { return p + (q - p) * 6.0 * (2.0 / 3.0 - t); } p };
        rgb(f(q, p, h + 1.0 / 3.0), f(q, p, h), f(q, p, h - 1.0 / 3.0))
    }
    pub fn mul(self, k: f64) -> Col { rgb(self.r * k, self.g * k, self.b * k) }
    pub fn add(self, o: Col) -> Col { rgb(self.r + o.r, self.g + o.g, self.b + o.b) }
    pub fn arr(self) -> [f64; 3] { [self.r, self.g, self.b] }
}

// ---------------------------------------------------------------- geometry
#[derive(Clone, Debug, Default)]
pub struct Attr { pub size: usize, pub d: Vec<f32> }
#[derive(Clone, Debug, Default)]
pub struct Geo {
    /// position, normal, uv, color, aA, aS (the only names the packet carries)
    pub pos: Vec<f32>, pub nrm: Option<Vec<f32>>, pub uv: Option<Vec<f32>>, pub col: Option<Vec<f32>>, pub a_a: Option<Vec<f32>>, pub a_s: Option<Vec<f32>>,
    pub index: Option<Vec<u32>>, pub dr: (usize, i64), pub bs: Option<[f64; 4]>,
}
#[inline] fn f(v: f64) -> f32 { v as f32 }
impl Geo {
    pub fn new(pos: Vec<f32>) -> Geo { Geo { pos, dr: (0, -1), ..Default::default() } }
    pub fn count(&self) -> usize { self.pos.len() / 3 }
    pub fn p(&self, i: usize) -> V3 { v3(self.pos[i * 3] as f64, self.pos[i * 3 + 1] as f64, self.pos[i * 3 + 2] as f64) }
    pub fn set_p(&mut self, i: usize, v: V3) { self.pos[i * 3] = f(v.x); self.pos[i * 3 + 1] = f(v.y); self.pos[i * 3 + 2] = f(v.z); }
    pub fn n(&self, i: usize) -> V3 { let n = self.nrm.as_ref().unwrap(); v3(n[i * 3] as f64, n[i * 3 + 1] as f64, n[i * 3 + 2] as f64) }
    pub fn with_uv(mut self, uv: Vec<f32>) -> Geo { self.uv = Some(uv); self }
    pub fn with_col(mut self, c: Vec<f32>) -> Geo { self.col = Some(c); self }
    pub fn with_index(mut self, i: Vec<u32>) -> Geo { self.index = Some(i); self }
    pub fn with_nrm(mut self, n: Vec<f32>) -> Geo { self.nrm = Some(n); self }
    pub fn apply(&mut self, m: &M4) {
        for i in 0..self.count() { let p = self.p(i).apply_m4(m); self.set_p(i, p); }
        if let Some(n) = self.nrm.as_mut() { let nm = m.normal_matrix();
            for i in 0..n.len() / 3 { let v = apply_nm(&nm, v3(n[i * 3] as f64, n[i * 3 + 1] as f64, n[i * 3 + 2] as f64)); n[i * 3] = f(v.x); n[i * 3 + 1] = f(v.y); n[i * 3 + 2] = f(v.z); } }
        if self.bs.is_some() { self.compute_bs(); }
    }
    pub fn translate(&mut self, x: f64, y: f64, z: f64) -> &mut Self { self.apply(&M4::trans(x, y, z)); self }
    pub fn rotate_x(&mut self, a: f64) -> &mut Self { self.apply(&M4::rot_x(a)); self }
    pub fn rotate_y(&mut self, a: f64) -> &mut Self { self.apply(&M4::rot_y(a)); self }
    pub fn rotate_z(&mut self, a: f64) -> &mut Self { self.apply(&M4::rot_z(a)); self }
    pub fn scale(&mut self, x: f64, y: f64, z: f64) -> &mut Self { self.apply(&M4::scale(x, y, z)); self }
    pub fn to_non_indexed(&self) -> Geo {
        let Some(idx) = &self.index else { return self.clone(); };
        let pick = |a: &Vec<f32>, s: usize| { let mut o = Vec::with_capacity(idx.len() * s); for &i in idx { o.extend_from_slice(&a[i as usize * s..i as usize * s + s]); } o };
        Geo { pos: pick(&self.pos, 3), nrm: self.nrm.as_ref().map(|a| pick(a, 3)), uv: self.uv.as_ref().map(|a| pick(a, 2)), col: self.col.as_ref().map(|a| pick(a, 3)),
            a_a: self.a_a.as_ref().map(|a| pick(a, 1)), a_s: self.a_s.as_ref().map(|a| pick(a, 1)), index: None, dr: (0, -1), bs: None }
    }
    pub fn compute_normals(&mut self) {
        let n = self.count(); let mut acc = vec![0.0f64; n * 3];
        // a normal attribute already there is reused (three zeroes it for indexed geometry, overwrites per face otherwise)
        if let Some(idx) = &self.index {
            for t in idx.chunks_exact(3) { let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
                let (pa, pb, pc) = (self.p(a), self.p(b), self.p(c)); let cb = pc.sub(pb).cross(pa.sub(pb));
                for k in [a, b, c] { let s = &mut acc[k * 3..k * 3 + 3]; s[0] = f(s[0] + cb.x) as f64; s[1] = f(s[1] + cb.y) as f64; s[2] = f(s[2] + cb.z) as f64; } }
        } else {
            let mut i = 0; while i + 2 < n { let (pa, pb, pc) = (self.p(i), self.p(i + 1), self.p(i + 2)); let cb = pc.sub(pb).cross(pa.sub(pb));
                for k in i..i + 3 { acc[k * 3] = f(cb.x) as f64; acc[k * 3 + 1] = f(cb.y) as f64; acc[k * 3 + 2] = f(cb.z) as f64; } i += 3; }
        }
        let mut out = vec![0f32; n * 3];
        for i in 0..n { let v = v3(acc[i * 3], acc[i * 3 + 1], acc[i * 3 + 2]).norm(); out[i * 3] = f(v.x); out[i * 3 + 1] = f(v.y); out[i * 3 + 2] = f(v.z); }
        self.nrm = Some(out);
    }
    pub fn compute_bs(&mut self) -> [f64; 4] {
        let n = self.count(); if n == 0 { let b = [0.0, 0.0, 0.0, 0.0]; self.bs = Some(b); return b; }
        let (mut mn, mut mx) = (v3(f64::INFINITY, f64::INFINITY, f64::INFINITY), v3(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY));
        for i in 0..n { let p = self.p(i); mn = v3(mn.x.min(p.x), mn.y.min(p.y), mn.z.min(p.z)); mx = v3(mx.x.max(p.x), mx.y.max(p.y), mx.z.max(p.z)); }
        let c = mn.add(mx).mul(0.5); let mut r2: f64 = 0.0; for i in 0..n { r2 = r2.max(c.dist_sq(self.p(i))); }
        let b = [c.x, c.y, c.z, r2.sqrt()]; self.bs = Some(b); b
    }
    pub fn bsphere(&mut self) -> [f64; 4] { match self.bs { Some(b) => b, None => self.compute_bs() } }
}

pub fn plane(w: f64, h: f64, ws: usize, hs: usize) -> Geo {
    let (wh, hh) = (w / 2.0, h / 2.0); let (gx, gy) = (ws.max(1), hs.max(1)); let (gx1, gy1) = (gx + 1, gy + 1); let (sw, sh) = (w / gx as f64, h / gy as f64);
    let (mut p, mut n, mut uv, mut idx) = (Vec::with_capacity(gx1 * gy1 * 3), Vec::with_capacity(gx1 * gy1 * 3), Vec::with_capacity(gx1 * gy1 * 2), Vec::with_capacity(gx * gy * 6));
    for iy in 0..gy1 { let y = iy as f64 * sh - hh; for ix in 0..gx1 { let x = ix as f64 * sw - wh; p.extend_from_slice(&[f(x), f(-y), 0.0]); n.extend_from_slice(&[0.0, 0.0, 1.0]); uv.push(f(ix as f64 / gx as f64)); uv.push(f(1.0 - iy as f64 / gy as f64)); } }
    for iy in 0..gy { for ix in 0..gx { let a = (ix + gx1 * iy) as u32; let b = (ix + gx1 * (iy + 1)) as u32; let c = (ix + 1 + gx1 * (iy + 1)) as u32; let d = (ix + 1 + gx1 * iy) as u32; idx.extend_from_slice(&[a, b, d, b, c, d]); } }
    Geo::new(p).with_nrm(n).with_uv(uv).with_index(idx)
}

pub fn box_geo(w: f64, h: f64, d: f64) -> Geo { box_seg(w, h, d, 1, 1, 1) }
pub fn box_seg(w: f64, h: f64, d: f64, ws: usize, hs: usize, ds: usize) -> Geo {
    let (mut p, mut n, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new()); let mut nv = 0u32;
    let mut plane = |u: usize, v: usize, wi: usize, udir: f64, vdir: f64, width: f64, height: f64, depth: f64, gx: usize, gy: usize| {
        let (sw, sh) = (width / gx as f64, height / gy as f64); let (wh, hh, dh) = (width / 2.0, height / 2.0, depth / 2.0); let (gx1, gy1) = (gx + 1, gy + 1); let mut cnt = 0u32;
        for iy in 0..gy1 { let y = iy as f64 * sh - hh; for ix in 0..gx1 { let x = ix as f64 * sw - wh;
            let mut vec = [0.0; 3]; vec[u] = x * udir; vec[v] = y * vdir; vec[wi] = dh; p.extend(vec.iter().map(|&q| f(q)));
            let mut nn = [0.0; 3]; nn[wi] = if depth > 0.0 { 1.0 } else { -1.0 }; n.extend(nn.iter().map(|&q| f(q)));
            uv.push(f(ix as f64 / gx as f64)); uv.push(f(1.0 - iy as f64 / gy as f64)); cnt += 1; } }
        for iy in 0..gy { for ix in 0..gx { let (gx1, ix, iy) = (gx1 as u32, ix as u32, iy as u32);
            let a = nv + ix + gx1 * iy; let b = nv + ix + gx1 * (iy + 1); let c = nv + ix + 1 + gx1 * (iy + 1); let dd = nv + ix + 1 + gx1 * iy; idx.extend_from_slice(&[a, b, dd, b, c, dd]); } }
        nv += cnt;
    };
    plane(2, 1, 0, -1.0, -1.0, d, h, w, ds, hs); plane(2, 1, 0, 1.0, -1.0, d, h, -w, ds, hs);
    plane(0, 2, 1, 1.0, 1.0, w, d, h, ws, ds); plane(0, 2, 1, 1.0, -1.0, w, d, -h, ws, ds);
    plane(0, 1, 2, 1.0, -1.0, w, h, d, ws, hs); plane(0, 1, 2, -1.0, -1.0, w, h, -d, ws, hs);
    Geo::new(p).with_nrm(n).with_uv(uv).with_index(idx)
}

pub fn cylinder(rt: f64, rb: f64, h: f64, rs: usize, hs: usize, open: bool) -> Geo { cylinder_t(rt, rb, h, rs, hs, open, 0.0, 2.0 * PI) }
pub fn cylinder_t(rt: f64, rb: f64, h: f64, rs: usize, hs: usize, open: bool, t0: f64, tl: f64) -> Geo {
    let (mut p, mut n, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new()); let mut index = 0u32; let hh = h / 2.0;
    let slope = (rb - rt) / h; let mut rows: Vec<Vec<u32>> = Vec::new();
    for y in 0..=hs { let mut row = Vec::new(); let vv = y as f64 / hs as f64; let r = vv * (rb - rt) + rt;
        for x in 0..=rs { let u = x as f64 / rs as f64; let th = u * tl + t0; let (st, ct) = (th.sin(), th.cos());
            p.extend_from_slice(&[f(r * st), f(-vv * h + hh), f(r * ct)]); let nn = v3(st, slope, ct).norm(); n.extend_from_slice(&[f(nn.x), f(nn.y), f(nn.z)]);
            uv.push(f(u)); uv.push(f(1.0 - vv)); row.push(index); index += 1; }
        rows.push(row); }
    for x in 0..rs { for y in 0..hs { let (a, b, c, d) = (rows[y][x], rows[y + 1][x], rows[y + 1][x + 1], rows[y][x + 1]); idx.extend_from_slice(&[a, b, d, b, c, d]); } }
    if !open {
        for top in [true, false] { let r = if top { rt } else { rb }; if r <= 0.0 { continue; } let sg = if top { 1.0 } else { -1.0 };
            let cs = index; for _ in 1..=rs { p.extend_from_slice(&[0.0, f(hh * sg), 0.0]); n.extend_from_slice(&[0.0, f(sg), 0.0]); uv.extend_from_slice(&[0.5, 0.5]); index += 1; }
            let ce = index;
            for x in 0..=rs { let u = x as f64 / rs as f64; let th = u * tl + t0; let (ct, st) = (th.cos(), th.sin());
                p.extend_from_slice(&[f(r * st), f(hh * sg), f(r * ct)]); n.extend_from_slice(&[0.0, f(sg), 0.0]); uv.push(f(ct * 0.5 + 0.5)); uv.push(f(st * 0.5 * sg + 0.5)); index += 1; }
            for x in 0..rs as u32 { let c = cs + x; let i = ce + x; if top { idx.extend_from_slice(&[i, i + 1, c]); } else { idx.extend_from_slice(&[i + 1, i, c]); } }
        }
    }
    Geo::new(p).with_nrm(n).with_uv(uv).with_index(idx)
}
pub fn cone(r: f64, h: f64, rs: usize, hs: usize, open: bool) -> Geo { cylinder(0.0, r, h, rs, hs, open) }

pub fn circle(r: f64, seg: usize) -> Geo {
    let seg = seg.max(3); let (mut p, mut n, mut uv, mut idx) = (vec![0.0f32; 3], vec![0.0, 0.0, 1.0f32], vec![0.5f32, 0.5], Vec::new());
    for s in 0..=seg { let a = s as f64 / seg as f64 * 2.0 * PI; let (x, y) = (r * a.cos(), r * a.sin()); p.extend_from_slice(&[f(x), f(y), 0.0]); n.extend_from_slice(&[0.0, 0.0, 1.0]);
        uv.push(f((f(x) as f64 / r + 1.0) / 2.0)); uv.push(f((f(y) as f64 / r + 1.0) / 2.0)); }
    for i in 1..=seg as u32 { idx.extend_from_slice(&[i, i + 1, 0]); }
    Geo::new(p).with_nrm(n).with_uv(uv).with_index(idx)
}
pub fn ring(ri: f64, ro: f64, ts: usize, ps: usize) -> Geo {
    let ts = ts.max(3); let ps = ps.max(1); let (mut p, mut n, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new()); let mut r = ri; let step = (ro - ri) / ps as f64;
    for _ in 0..=ps { for i in 0..=ts { let a = i as f64 / ts as f64 * 2.0 * PI; let (x, y) = (r * a.cos(), r * a.sin()); p.extend_from_slice(&[f(x), f(y), 0.0]); n.extend_from_slice(&[0.0, 0.0, 1.0]);
        uv.push(f((x / ro + 1.0) / 2.0)); uv.push(f((y / ro + 1.0) / 2.0)); } r += step; }
    for j in 0..ps { let lvl = j * (ts + 1); for i in 0..ts { let s = (i + lvl) as u32; let t = ts as u32; idx.extend_from_slice(&[s, s + t + 1, s + 1, s + t + 1, s + t + 2, s + 1]); } }
    Geo::new(p).with_nrm(n).with_uv(uv).with_index(idx)
}
pub fn sphere(r: f64, ws: usize, hs: usize) -> Geo { sphere_p(r, ws, hs, 0.0, 2.0 * PI, 0.0, PI) }
pub fn sphere_p(r: f64, ws: usize, hs: usize, p0: f64, pl: f64, t0: f64, tl: f64) -> Geo {
    let ws = ws.max(3); let hs = hs.max(2); let tend = (t0 + tl).min(PI); let (mut p, mut n, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut grid: Vec<Vec<u32>> = Vec::new(); let mut index = 0u32;
    for iy in 0..=hs { let vv = iy as f64 / hs as f64; let mut row = Vec::new();
        let uo = if iy == 0 && t0 == 0.0 { 0.5 / ws as f64 } else if iy == hs && tend == PI { -0.5 / ws as f64 } else { 0.0 };
        for ix in 0..=ws { let u = ix as f64 / ws as f64;
            let x = -r * (p0 + u * pl).cos() * (t0 + vv * tl).sin(); let y = r * (t0 + vv * tl).cos(); let z = r * (p0 + u * pl).sin() * (t0 + vv * tl).sin();
            p.extend_from_slice(&[f(x), f(y), f(z)]); let nn = v3(x, y, z).norm(); n.extend_from_slice(&[f(nn.x), f(nn.y), f(nn.z)]); uv.push(f(u + uo)); uv.push(f(1.0 - vv));
            row.push(index); index += 1; }
        grid.push(row); }
    for iy in 0..hs { for ix in 0..ws { let (a, b, c, d) = (grid[iy][ix + 1], grid[iy][ix], grid[iy + 1][ix], grid[iy + 1][ix + 1]);
        if iy != 0 || t0 > 0.0 { idx.extend_from_slice(&[a, b, d]); } if iy != hs - 1 || tend < PI { idx.extend_from_slice(&[b, c, d]); } } }
    Geo::new(p).with_nrm(n).with_uv(uv).with_index(idx)
}
pub fn torus(r: f64, tube: f64, rs: usize, ts: usize) -> Geo {
    let (mut p, mut n, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for j in 0..=rs { for i in 0..=ts { let u = i as f64 / ts as f64 * 2.0 * PI; let vv = j as f64 / rs as f64 * PI * 2.0;
        let vx = (r + tube * vv.cos()) * u.cos(); let vy = (r + tube * vv.cos()) * u.sin(); let vz = tube * vv.sin(); p.extend_from_slice(&[f(vx), f(vy), f(vz)]);
        let nn = v3(vx - r * u.cos(), vy - r * u.sin(), vz).norm(); n.extend_from_slice(&[f(nn.x), f(nn.y), f(nn.z)]); uv.push(f(i as f64 / ts as f64)); uv.push(f(j as f64 / rs as f64)); } }
    for j in 1..=rs { for i in 1..=ts { let a = ((ts + 1) * j + i - 1) as u32; let b = ((ts + 1) * (j - 1) + i - 1) as u32; let c = ((ts + 1) * (j - 1) + i) as u32; let d = ((ts + 1) * j + i) as u32; idx.extend_from_slice(&[a, b, d, b, c, d]); } }
    Geo::new(p).with_nrm(n).with_uv(uv).with_index(idx)
}
fn polyhedron(verts: &[f64], ind: &[usize], r: f64, detail: usize) -> Geo {
    let mut vb: Vec<V3> = Vec::new();
    let gv = |i: usize| v3(verts[i * 3], verts[i * 3 + 1], verts[i * 3 + 2]);
    for t in ind.chunks_exact(3) { let (a, b, c) = (gv(t[0]), gv(t[1]), gv(t[2])); let cols = detail + 1; let mut v: Vec<Vec<V3>> = Vec::new();
        for i in 0..=cols { let aj = a.lerp(c, i as f64 / cols as f64); let bj = b.lerp(c, i as f64 / cols as f64); let rows = cols - i; let mut row = Vec::new();
            for j in 0..=rows { if j == 0 && i == cols { row.push(aj); } else { row.push(aj.lerp(bj, j as f64 / rows as f64)); } } v.push(row); }
        for i in 0..cols { for j in 0..(2 * (cols - i) - 1) { let k = j / 2;
            if j % 2 == 0 { vb.push(v[i][k + 1]); vb.push(v[i + 1][k]); vb.push(v[i][k]); } else { vb.push(v[i][k + 1]); vb.push(v[i + 1][k + 1]); vb.push(v[i + 1][k]); } } } }
    // the float32 round trip of the subdivision happens in three's vertex buffer (a plain array): keep f64 until the radius is applied
    let vb: Vec<V3> = vb.into_iter().map(|q| q.norm().mul(r)).collect();
    let mut uv = Vec::with_capacity(vb.len() * 2);
    for q in &vb { let u = q.z.atan2(-q.x) / 2.0 / PI + 0.5; let vv = (-q.y).atan2((q.x * q.x + q.z * q.z).sqrt()) / PI + 0.5; uv.push(u); uv.push(1.0 - vv); }
    // correctUVs: a vertex on the pole or on the u = 1 edge of a face west of the seam
    for t in 0..vb.len() / 3 { let c = vb[t * 3].add(vb[t * 3 + 1]).add(vb[t * 3 + 2]).mul(1.0 / 3.0); let azi = c.z.atan2(-c.x);
        for k in 0..3 { let i = t * 3 + k; if azi < 0.0 && uv[i * 2] == 1.0 { uv[i * 2] -= 1.0; } if vb[i].x == 0.0 && vb[i].z == 0.0 { uv[i * 2] = azi / 2.0 / PI + 0.5; } } }
    // correctSeam (faces straddling it)
    for t in 0..vb.len() / 3 { let i = t * 3; let us = [uv[i * 2], uv[i * 2 + 2], uv[i * 2 + 4]]; let mx = us.iter().cloned().fold(f64::MIN, f64::max); let mn = us.iter().cloned().fold(f64::MAX, f64::min);
        if mx > 0.9 && mn < 0.1 { for k in 0..3 { if uv[(i + k) * 2] < 0.2 { uv[(i + k) * 2] += 1.0; } } } }
    let pos: Vec<f32> = vb.iter().flat_map(|q| [f(q.x), f(q.y), f(q.z)]).collect();
    let mut g = Geo::new(pos.clone()).with_uv(uv.iter().map(|&v| f(v)).collect());
    if detail == 0 { g.compute_normals(); } else { let n: Vec<f32> = vb.iter().flat_map(|q| { let n = q.norm(); [f(n.x), f(n.y), f(n.z)] }).collect(); g.nrm = Some(n); }
    g
}
pub fn icosahedron(r: f64, detail: usize) -> Geo {
    let t = (1.0 + 5f64.sqrt()) / 2.0;
    let v = [-1.0, t, 0.0, 1.0, t, 0.0, -1.0, -t, 0.0, 1.0, -t, 0.0, 0.0, -1.0, t, 0.0, 1.0, t, 0.0, -1.0, -t, 0.0, 1.0, -t, t, 0.0, -1.0, t, 0.0, 1.0, -t, 0.0, -1.0, -t, 0.0, 1.0];
    let i = [0, 11, 5, 0, 5, 1, 0, 1, 7, 0, 7, 10, 0, 10, 11, 1, 5, 9, 5, 11, 4, 11, 10, 2, 10, 7, 6, 7, 1, 8, 3, 9, 4, 3, 4, 2, 3, 2, 6, 3, 6, 8, 3, 8, 9, 4, 9, 5, 2, 4, 11, 6, 2, 10, 8, 6, 7, 9, 8, 1];
    polyhedron(&v, &i, r, detail)
}
pub fn octahedron(r: f64) -> Geo {
    let v = [1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0];
    let i = [0, 2, 4, 0, 4, 3, 0, 3, 5, 0, 5, 2, 1, 2, 5, 1, 5, 3, 1, 3, 4, 1, 4, 2];
    polyhedron(&v, &i, r, 0)
}
pub fn lathe(pts: &[(f64, f64)], seg: usize) -> Geo {
    let (mut p, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new()); let n = pts.len();
    for i in 0..=seg { let phi = i as f64 * (1.0 / seg as f64) * 2.0 * PI; let (s, c) = (phi.sin(), phi.cos());
        for j in 0..n { p.extend_from_slice(&[f(pts[j].0 * s), f(pts[j].1), f(pts[j].0 * c)]); uv.push(f(i as f64 / seg as f64)); uv.push(f(j as f64 / (n - 1) as f64)); } }
    for i in 0..seg { for j in 0..n - 1 { let base = (j + i * n) as u32; let (a, b, c, d) = (base, base + n as u32, base + n as u32 + 1, base + 1); idx.extend_from_slice(&[a, b, d, c, d, b]); } }
    Geo::new(p).with_uv(uv).with_index(idx)
}

// ---------------------------------------------------------------- textures and materials
pub const REPEAT: i32 = 1000; pub const CLAMP: i32 = 1001; pub const NEAREST: i32 = 1003; pub const NEAREST_MIP_NEAREST: i32 = 1004; pub const LINEAR: i32 = 1006; pub const LINEAR_MIP_LINEAR: i32 = 1008;
#[derive(Clone)]
pub struct Tex { pub w: usize, pub h: usize, pub data: Vec<u8>, pub flip_y: bool, pub ws: i32, pub wt: i32, pub mag: i32, pub min: i32, pub mip: bool, pub repeat: (f64, f64) }
impl Tex {
    /// a canvas texture as canvasTex() made it (nearest, nearest-mipmap-nearest; repeat wrapping on request)
    pub fn canvas(w: usize, h: usize, data: Vec<u8>, repeat: bool) -> Tex {
        Tex { w, h, data, flip_y: true, ws: if repeat { REPEAT } else { CLAMP }, wt: if repeat { REPEAT } else { CLAMP }, mag: NEAREST, min: NEAREST_MIP_NEAREST, mip: true, repeat: (1.0, 1.0) }
    }
    pub fn linear(mut self) -> Tex { self.mag = LINEAR; self.min = LINEAR; self }
    pub fn no_mip(mut self) -> Tex { self.mip = false; self }
}
pub type TexId = usize; pub type MatId = usize; pub type GeoId = usize; pub type ObjId = usize;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind { Basic, Lambert, Phong, Points, Sky, Dust }
pub const FRONT: i32 = 0; pub const BACK: i32 = 1; pub const DOUBLE: i32 = 2;
pub const NO_BLEND: i32 = 0; pub const NORMAL: i32 = 1; pub const ADDITIVE: i32 = 2; pub const CUSTOM: i32 = 5;
#[derive(Clone)]
pub struct Mat {
    pub kind: Kind, pub color: Option<Col>, pub spec: Option<Col>, pub shin: f64, pub emis: Option<Col>, pub map: Option<TexId>, pub alpha_test: f64,
    pub transparent: bool, pub opacity: f64, pub side: i32, pub vc: bool, pub fog: bool, pub dw: bool, pub dt: bool, pub po: Option<(f64, f64)>,
    pub blending: i32, pub blend2: Option<i32>, pub sway: Option<Vec<f64>>, pub size: f64,
    /// sky: sun, fog, top, glow, halo; dust: dens, fog, col
    pub sky: Option<(V3, Col, Col, Col, f64)>, pub dust: Option<(f64, Col, Col)>,
    pub paint: bool,
}
impl Mat {
    fn base(kind: Kind) -> Mat { Mat { kind, color: Some(WHITE), spec: None, shin: 0.0, emis: None, map: None, alpha_test: 0.0, transparent: false, opacity: 1.0, side: FRONT, vc: false,
        fog: true, dw: true, dt: true, po: None, blending: NORMAL, blend2: None, sway: None, size: 1.0, sky: None, dust: None, paint: false } }
    pub fn basic() -> Mat { Mat::base(Kind::Basic) }
    pub fn lambert() -> Mat { let mut m = Mat::base(Kind::Lambert); m.emis = Some(rgb(0.0, 0.0, 0.0)); m }
    pub fn phong() -> Mat { let mut m = Mat::base(Kind::Phong); m.emis = Some(rgb(0.0, 0.0, 0.0)); m.spec = Some(Col::hex(0x111111)); m.shin = 30.0; m }
    pub fn points(size: f64, c: Col) -> Mat { let mut m = Mat::base(Kind::Points); m.size = size; m.color = Some(c); m }
    pub fn shader(kind: Kind) -> Mat { let mut m = Mat::base(kind); m.color = None; m.fog = false; m }
    pub fn col(mut self, c: Col) -> Mat { self.color = Some(c); self }
    pub fn hex(self, h: u32) -> Mat { self.col(Col::hex(h)) }
    pub fn map(mut self, t: TexId) -> Mat { self.map = Some(t); self }
    pub fn vc(mut self) -> Mat { self.vc = true; self }
    pub fn side(mut self, s: i32) -> Mat { self.side = s; self }
    pub fn double(self) -> Mat { self.side(DOUBLE) }
    pub fn at(mut self, a: f64) -> Mat { self.alpha_test = a; self }
    pub fn transparent(mut self) -> Mat { self.transparent = true; self }
    pub fn opacity(mut self, o: f64) -> Mat { self.opacity = o; self }
    pub fn no_dw(mut self) -> Mat { self.dw = false; self }
    pub fn no_fog(mut self) -> Mat { self.fog = false; self }
    pub fn po(mut self, f: f64, u: f64) -> Mat { self.po = Some((f, u)); self }
    pub fn blend(mut self, b: i32) -> Mat { self.blending = b; self }
    pub fn blend2(mut self) -> Mat { self.blend2 = Some(2); self }
    pub fn spec(mut self, c: u32) -> Mat { self.spec = Some(Col::hex(c)); self }
    pub fn shin(mut self, s: f64) -> Mat { self.shin = s; self }
    pub fn emis(mut self, c: u32) -> Mat { self.emis = Some(Col::hex(c)); self }
    /// the wind sway the page added to foliage materials (onBeforeCompile): amplitude, sway by uv.y, fade-in band
    pub fn sway(mut self, amp: f64, use_uv: bool, fade: Option<(f64, f64)>) -> Mat {
        self.sway = Some(match fade { Some((a, b)) => vec![amp, use_uv as i32 as f64, a, b], None => vec![amp, use_uv as i32 as f64] }); self
    }
}

// ---------------------------------------------------------------- the scene graph
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum OKind { Group, Mesh, Inst, Points }
#[derive(Clone)]
pub struct Obj {
    pub kind: OKind, pub parent: Option<ObjId>, pub children: Vec<ObjId>,
    pub pos: V3, pub quat: Quat, pub rot: V3, pub yxz: bool, pub scl: V3, pub visible: bool, pub ro: i32, pub culled: bool,
    pub geo: GeoId, pub mat: MatId,
    /// instanced: matrices (as stored, f32), colours, count
    pub im: Vec<f32>, pub ic: Option<Vec<f32>>, pub count: usize, pub ibs: Option<[f64; 4]>,
    // userData the page used
    pub dyn_: bool, pub frozen: bool, pub wave: bool, pub grid: Option<(usize, usize)>, pub model: Option<usize>, pub liv_carrier: bool, pub liveries: Vec<ObjId>,
    pub removed: bool,
}
pub struct Scene {
    pub objs: Vec<Obj>, pub root: Vec<ObjId>, pub geos: Vec<Geo>, pub mats: Vec<Mat>, pub texs: Vec<Tex>,
}
impl Default for Scene { fn default() -> Self { Scene::new() } }
impl Scene {
    pub fn new() -> Scene { Scene { objs: Vec::new(), root: Vec::new(), geos: Vec::new(), mats: Vec::new(), texs: Vec::new() } }
    pub fn tex(&mut self, t: Tex) -> TexId { self.texs.push(t); self.texs.len() - 1 }
    pub fn mat(&mut self, m: Mat) -> MatId { self.mats.push(m); self.mats.len() - 1 }
    pub fn geo(&mut self, g: Geo) -> GeoId { self.geos.push(g); self.geos.len() - 1 }
    fn obj(&mut self, kind: OKind, geo: GeoId, mat: MatId) -> ObjId {
        self.objs.push(Obj { kind, parent: None, children: Vec::new(), pos: V3::default(), quat: Quat::default(), rot: V3::default(), yxz: false, scl: v3(1.0, 1.0, 1.0), visible: true, ro: 0, culled: true,
            geo, mat, im: Vec::new(), ic: None, count: 0, ibs: None, dyn_: false, frozen: false, wave: false, grid: None, model: None, liv_carrier: false, liveries: Vec::new(), removed: false });
        self.objs.len() - 1
    }
    pub fn group(&mut self) -> ObjId { self.obj(OKind::Group, usize::MAX, usize::MAX) }
    pub fn mesh(&mut self, g: GeoId, m: MatId) -> ObjId { self.obj(OKind::Mesh, g, m) }
    pub fn mesh_new(&mut self, g: Geo, m: MatId) -> ObjId { let g = self.geo(g); self.mesh(g, m) }
    pub fn points(&mut self, g: GeoId, m: MatId) -> ObjId { self.obj(OKind::Points, g, m) }
    /// a new InstancedMesh with `n` identity matrices
    pub fn inst(&mut self, g: GeoId, m: MatId, n: usize) -> ObjId {
        let o = self.obj(OKind::Inst, g, m); let mut im = Vec::with_capacity(n * 16); for _ in 0..n { im.extend(M4::ID.0.iter().map(|&v| v as f32)); }
        self.objs[o].im = im; self.objs[o].count = n; o
    }
    pub fn set_matrix_at(&mut self, o: ObjId, i: usize, m: &M4) { let im = &mut self.objs[o].im; for k in 0..16 { im[i * 16 + k] = m.0[k] as f32; } }
    pub fn set_color_at(&mut self, o: ObjId, i: usize, c: Col) {
        let n = self.objs[o].im.len() / 16; let ic = self.objs[o].ic.get_or_insert_with(|| vec![0.0; n * 3]); ic[i * 3] = c.r as f32; ic[i * 3 + 1] = c.g as f32; ic[i * 3 + 2] = c.b as f32;
    }
    pub fn matrix_at(&self, o: ObjId, i: usize) -> M4 { let im = &self.objs[o].im; let mut m = [0.0; 16]; for k in 0..16 { m[k] = im[i * 16 + k] as f64; } M4(m) }
    /// InstancedMesh.computeBoundingSphere
    pub fn inst_bs(&mut self, o: ObjId) -> [f64; 4] {
        let gb = { let g = self.objs[o].geo; self.geos[g].bsphere() };
        let (mut c, mut r) = (V3::default(), -1.0f64);
        for i in 0..self.objs[o].count { let m = self.matrix_at(o, i); let sc = v3(gb[0], gb[1], gb[2]).apply_m4(&m); let sr = gb[3] * m.max_scale();
            if sr < 0.0 { continue; } if r < 0.0 { c = sc; r = sr; continue; }
            if c == sc { r = r.max(sr); continue; }
            let d = sc.sub(c); let l = d.len(); let dv = if l == 0.0 { d } else { d.mul(sr / l) };
            for pt in [sc.add(dv), sc.sub(dv)] { if r < 0.0 { c = pt; r = 0.0; continue; } let v = pt.sub(c); let ls = v.len_sq(); if ls > r * r { let len = ls.sqrt(); let dl = (len - r) * 0.5; c = c.add(v.mul(dl / len)); r += dl; } }
        }
        let b = [c.x, c.y, c.z, r]; self.objs[o].ibs = Some(b); b
    }
    pub fn add(&mut self, o: ObjId) { self.objs[o].parent = None; self.root.push(o); }
    pub fn add_to(&mut self, parent: ObjId, o: ObjId) {
        if let Some(p) = self.objs[o].parent { self.objs[p].children.retain(|&c| c != o); } else { self.root.retain(|&c| c != o); }
        self.objs[o].parent = Some(parent); self.objs[parent].children.push(o);
    }
    pub fn remove_root(&mut self, o: ObjId) { self.root.retain(|&c| c != o); self.objs[o].removed = true; }
    pub fn remove_child(&mut self, parent: ObjId, o: ObjId) { self.objs[parent].children.retain(|&c| c != o); self.objs[o].parent = None; self.objs[o].removed = true; }
    // transforms
    pub fn set_pos(&mut self, o: ObjId, x: f64, y: f64, z: f64) { self.objs[o].pos = v3(x, y, z); }
    pub fn set_rot(&mut self, o: ObjId, x: f64, y: f64, z: f64) { let ob = &mut self.objs[o]; ob.rot = v3(x, y, z); ob.quat = Quat::from_euler(x, y, z, ob.yxz); }
    pub fn set_rot_yxz(&mut self, o: ObjId, x: f64, y: f64, z: f64) { self.objs[o].yxz = true; self.set_rot(o, x, y, z); }
    pub fn set_scale(&mut self, o: ObjId, x: f64, y: f64, z: f64) { self.objs[o].scl = v3(x, y, z); }
    pub fn local(&self, o: ObjId) -> M4 { let ob = &self.objs[o]; M4::compose(ob.pos, ob.quat, ob.scl) }
    pub fn world(&self, o: ObjId) -> M4 { let l = self.local(o); match self.objs[o].parent { Some(p) => self.world(p).mul(&l), None => l } }
    pub fn visible_chain(&self, o: ObjId) -> bool { let mut q = Some(o); while let Some(i) = q { if !self.objs[i].visible { return false; } q = self.objs[i].parent; } true }
    /// pre-order traversal of the whole scene (as scene.traverse)
    pub fn traverse(&self) -> Vec<ObjId> { let mut out = Vec::new(); for &r in &self.root { self.walk(r, &mut out); } out }
    pub fn walk(&self, o: ObjId, out: &mut Vec<ObjId>) { out.push(o); for &c in &self.objs[o].children { self.walk(c, out); } }
    pub fn top_of(&self, o: ObjId) -> ObjId { let mut t = o; while let Some(p) = self.objs[t].parent { t = p; } t }
}

/// A reusable Object3D for composing instance matrices, as the page's tmpO (Euler XYZ; rotateX/Y/Z multiply the quaternion locally)
#[derive(Clone, Copy)]
pub struct Tmp { pub pos: V3, pub quat: Quat, pub scl: V3 }
impl Default for Tmp { fn default() -> Self { Tmp { pos: V3::default(), quat: Quat::default(), scl: v3(1.0, 1.0, 1.0) } } }
impl Tmp {
    pub fn pos(&mut self, x: f64, y: f64, z: f64) -> &mut Self { self.pos = v3(x, y, z); self }
    pub fn rot(&mut self, x: f64, y: f64, z: f64) -> &mut Self { self.quat = Quat::from_euler(x, y, z, false); self }
    pub fn scale(&mut self, x: f64, y: f64, z: f64) -> &mut Self { self.scl = v3(x, y, z); self }
    pub fn scalar(&mut self, s: f64) -> &mut Self { self.scl = v3(s, s, s); self }
    pub fn rotate_x(&mut self, a: f64) -> &mut Self { self.quat = self.quat.mul(Quat::axis_angle(v3(1.0, 0.0, 0.0), a)); self }
    pub fn rotate_y(&mut self, a: f64) -> &mut Self { self.quat = self.quat.mul(Quat::axis_angle(v3(0.0, 1.0, 0.0), a)); self }
    pub fn rotate_z(&mut self, a: f64) -> &mut Self { self.quat = self.quat.mul(Quat::axis_angle(v3(0.0, 0.0, 1.0), a)); self }
    /// translateZ: along the object's own z axis
    pub fn translate_z(&mut self, d: f64) -> &mut Self { let v = v3(0.0, 0.0, 1.0).apply_quat(self.quat); self.pos = self.pos.add(v.mul(d)); self }
    pub fn matrix(&self) -> M4 { M4::compose(self.pos, self.quat, self.scl) }
}

// ---------------------------------------------------------------- the scene packet
/// "FWS1" + json head + one blob of every buffer; the same layout nglExport() wrote. Returns the packet and the scene ids of each object
/// (an object with several draws has several; the page never used multi-material meshes, so one each).
pub struct Export { pub bytes: Vec<u8>, pub ids: Vec<Option<usize>> }
pub struct Lights { pub hemi: (Col, Col, f64, V3), pub dir: (Col, f64, V3) }
pub fn export(sc: &Scene, map: &str, fog: (Col, f64), clear: Col, lights: &Lights, car_root: ObjId) -> Export {
    use serde_json::{json, Value};
    let mut blob: Vec<u8> = Vec::with_capacity(48 << 20);
    let mut add = |b: &[u8]| -> [usize; 2] { let off = blob.len(); blob.extend_from_slice(b); while blob.len() % 4 != 0 { blob.push(0); } [off, b.len()] };
    fn f32b(v: &[f32]) -> &[u8] { unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 4) } }
    fn u32b(v: &[u32]) -> &[u8] { unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 4) } }
    let (mut tex_ix, mut geo_ix, mut mat_ix) = (vec![usize::MAX; sc.texs.len()], vec![usize::MAX; sc.geos.len()], vec![usize::MAX; sc.mats.len()]);
    let (mut texs, mut geos, mut mats, mut objs): (Vec<Value>, Vec<Value>, Vec<Value>, Vec<Value>) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let col = |c: Option<Col>| -> Value { let c = c.unwrap_or(WHITE); json!([c.r, c.g, c.b]) };
    let mut ids = vec![None; sc.objs.len()];
    let order = sc.traverse();
    for &o in &order {
        let ob = &sc.objs[o]; if ob.kind == OKind::Group { continue; }
        // geometry
        let gi = if geo_ix[ob.geo] != usize::MAX { geo_ix[ob.geo] } else {
            let g = &sc.geos[ob.geo]; let mut attrs = Vec::new();
            for (name, size, d) in [("position", 3, Some(&g.pos)), ("normal", 3, g.nrm.as_ref()), ("uv", 2, g.uv.as_ref()), ("color", 3, g.col.as_ref()), ("aA", 1, g.a_a.as_ref()), ("aS", 1, g.a_s.as_ref())] {
                if let Some(d) = d { attrs.push(json!({ "name": name, "size": size, "data": add(f32b(d)) })); } }
            let bs = match g.bs { Some(b) => b, None => { let mut gg = Geo::new(g.pos.clone()); gg.compute_bs() } };
            let e = json!({ "id": geos.len(), "attrs": attrs, "index": g.index.as_ref().map(|i| json!(add(u32b(i)))).unwrap_or(Value::Null),
                "count": g.index.as_ref().map(|i| i.len()).unwrap_or(g.count()), "dr": [g.dr.0, g.dr.1], "bs": bs });
            geo_ix[ob.geo] = geos.len(); geos.push(e); geos.len() - 1 };
        // material (and its texture)
        let mi = if mat_ix[ob.mat] != usize::MAX { mat_ix[ob.mat] } else {
            let m = &sc.mats[ob.mat];
            let map = match m.map { None => -1, Some(t) => { if tex_ix[t] == usize::MAX { let tx = &sc.texs[t];
                let mm = [tx.repeat.0, 0.0, 0.0, 0.0, tx.repeat.1, 0.0, 0.0, 0.0, 1.0];
                texs.push(json!({ "id": texs.len(), "w": tx.w, "h": tx.h, "data": add(&tx.data), "flipY": tx.flip_y as i32, "ws": tx.ws, "wt": tx.wt, "mag": tx.mag, "min": tx.min, "mip": tx.mip as i32, "m": mm }));
                tex_ix[t] = texs.len() - 1; } tex_ix[t] as i64 } };
            let kind = match m.kind { Kind::Basic => "basic", Kind::Lambert => "lambert", Kind::Phong => "phong", Kind::Points => "points", Kind::Sky => "sky", Kind::Dust => "dust" };
            let mut e = json!({ "id": mats.len(), "kind": kind, "color": col(m.color), "spec": col(m.spec), "shin": if m.shin != 0.0 { m.shin } else { 30.0 }, "emis": m.emis.map(|c| json!([c.r, c.g, c.b])).unwrap_or(json!([0, 0, 0])),
                "map": map, "at": m.alpha_test, "tr": m.transparent as i32, "op": m.opacity, "side": m.side, "vc": m.vc as i32, "fog": m.fog as i32, "dw": m.dw as i32, "dt": m.dt as i32,
                "po": m.po.map(|(a, b)| json!([a, b])).unwrap_or(Value::Null), "blend": m.blend2.unwrap_or(m.blending), "sway": m.sway.as_ref().map(|s| json!(s)).unwrap_or(Value::Null), "size": m.size });
            if let Some(b) = m.blend2 { e["blend2"] = json!(b); }
            if let Some((sun, fogc, top, glow, halo)) = m.sky { e["u"] = json!({ "sun": [sun.x, sun.y, sun.z], "fog": col(Some(fogc)), "top": col(Some(top)), "glow": col(Some(glow)), "halo": halo }); }
            if let Some((dens, fogc, c)) = m.dust { e["u"] = json!({ "dens": dens, "fog": col(Some(fogc)), "col": col(Some(c)) }); }
            mat_ix[ob.mat] = mats.len(); mats.push(e); mats.len() - 1 };
        let mw = sc.world(o);
        let mut e = json!({ "id": objs.len(), "type": match ob.kind { OKind::Inst => "inst", OKind::Points => "points", _ => "mesh" }, "g": gi, "m": mi, "mw": mw.0.to_vec(),
            "ro": ob.ro, "cull": ob.culled as i32, "vis": sc.visible_chain(o) as i32 });
        { let mut q = Some(o); while let Some(i) = q { if i == car_root { e["car"] = json!(1); break; } q = sc.objs[i].parent; } }
        if ob.wave { e["wave"] = json!(1); }
        if ob.kind == OKind::Inst {
            e["count"] = json!(ob.count); e["max"] = json!(ob.im.len() / 16); e["im"] = json!(add(f32b(&ob.im)));
            e["ic"] = ob.ic.as_ref().map(|c| json!(add(f32b(c)))).unwrap_or(Value::Null);
            match ob.ibs { Some(b) => e["ibs"] = json!(b), None => e["cull"] = json!(0) }
        }
        ids[o] = Some(objs.len()); objs.push(e);
    }
    let (hs, hg, hi, hd) = lights.hemi; let (dc, di, dd) = lights.dir;
    let head = json!({ "v": 1, "map": map, "fog": { "c": [fog.0.r, fog.0.g, fog.0.b], "d": fog.1 }, "clear": [clear.r, clear.g, clear.b, 0],
        "lights": { "hemi": [{ "sky": [hs.r * hi, hs.g * hi, hs.b * hi], "ground": [hg.r * hi, hg.g * hi, hg.b * hi], "d": [hd.x, hd.y, hd.z] }], "dir": [{ "c": [dc.r * di, dc.g * di, dc.b * di], "d": [dd.x, dd.y, dd.z] }] },
        "texs": texs, "geos": geos, "mats": mats, "objs": objs });
    let js = serde_json::to_vec(&head).unwrap_or_default(); let jl = (js.len() + 3) & !3;
    let mut out = Vec::with_capacity(8 + jl + blob.len()); out.extend_from_slice(b"FWS1"); out.extend_from_slice(&(js.len() as u32).to_le_bytes()); out.extend_from_slice(&js);
    while out.len() < 8 + jl { out.push(b' '); } out.extend_from_slice(&blob);
    Export { bytes: out, ids }
}
