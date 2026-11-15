//! Crash damage: a visual-only soft body for your car's bodywork. It never touches the driving physics.
//!
//! The body is wrapped in a lattice of 5 x 4 x 9 nodes (a cage around the car, in its own frame) joined by springs to every
//! neighbour, diagonals included, so the cage resists stretching, shearing and bending like a shell. A hit drives an
//! indenter into the car at the contact for about a tenth of a second (a tree, a rock or the other car's flank, softened at
//! its edge), as deep as the closing speed says a car's structure gives way (a few cm at 20 km/h, ~0.45 m at 50 km/h);
//! the springs carry the push into the panels around it, then the indenter lets go and the metal rings for a moment.
//! The springs are plastic: once one is strained past the yield point its rest length follows it, so the metal keeps the
//! dent instead of popping back, a heavy corner hit leaves the shell bent or twisted along its length, and everything stays
//! within a crush limit. Every node is also tied to where it belongs on the car's frame by a spring of its own (the
//! structure behind the panel, and the inertia of the rest of the car): that tie yields too, so a struck area stays pushed
//! in, while the push fades within a metre or so and the far end of the car is untouched. Every vertex of the body meshes is tied to its cage cell (trilinear weights) and
//! follows it; its normal is turned by the local deformation (cofactor of the deformation gradient) so dents catch the
//! light. Where the springs yielded the panels also crumple: small creases along a fixed wrinkle pattern, deeper the more
//! the metal there was crushed. Nothing runs between hits: the cage sleeps once it is still and the meshes keep their shape.
use crate::render::Renderer;

const NX: usize = 5; const NY: usize = 4; const NZ: usize = 9; const N: usize = NX * NY * NZ;
/// the cage around the body (car frame: x right, y up from the ground, z forward), metres
const LO: [f32; 3] = [-1.05, 0.08, -2.3]; const HI: [f32; 3] = [1.05, 1.7, 2.3];
/// spring stiffness and damping (per unit mass, 1/s2 and 1/s), overall air damping (1/s), yield strain, crush limits
const K: f32 = 1500.0; const C: f32 = 9.0; const DAMP: f32 = 4.0; const YIELD: f32 = 0.018; const SHORTEST: f32 = 0.3; const LONGEST: f32 = 1.25;
const SUB: usize = 8;
/// each node's tie to its place on the frame: stiffness (1/s2) and how far it gives before it yields (m)
const KH: f32 = 800.0; const YH: f32 = 0.025;
/// how long an indenter presses (s)
const IND_T: f32 = 0.09;

fn idx(i: usize, j: usize, k: usize) -> usize { i + NX * (j + NY * k) }
#[inline] fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] { [a[0] - b[0], a[1] - b[1], a[2] - b[2]] }
#[inline] fn dot(a: [f32; 3], b: [f32; 3]) -> f32 { a[0] * b[0] + a[1] * b[1] + a[2] * b[2] }
#[inline] fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] { [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]] }
#[inline] fn len(a: [f32; 3]) -> f32 { dot(a, a).sqrt() }

/// the crease pattern: two crossed sine waves (about 0.7 m and 0.8 m across) and their gradient
#[inline] fn wrinkle(p: [f32; 3]) -> (f32, [f32; 3]) {
    const F1: [f32; 3] = [7.3, 5.1, 9.7]; const F2: [f32; 3] = [-6.1, 8.7, 4.3];
    let (a, b) = (dot(F1, p) + 0.7, dot(F2, p) + 2.1); let (sa, ca, sb, cb) = (a.sin(), a.cos(), b.sin(), b.cos());
    (sa * sb, [ca * sb * F1[0] + sa * cb * F2[0], ca * sb * F1[1] + sa * cb * F2[1], ca * sb * F1[2] + sa * cb * F2[2]])
}

/// one body mesh tied to the cage
struct Skin { obj: usize, rest_p: Vec<f32>, rest_n: Vec<f32>, cell: Vec<u32>, frac: Vec<[f32; 3]>, pos: Vec<f32>, nrm: Vec<f32> }

pub struct Crash {
    x0: Vec<[f32; 3]>, x: Vec<[f32; 3]>, v: Vec<[f32; 3]>,
    /// where each node's tie holds it now (starts at x0, moves when the tie yields)
    home: Vec<[f32; 3]>,
    /// node a, node b, rest length now, rest length as built
    springs: Vec<(u16, u16, f32, f32)>,
    /// how much the metal around each node has yielded (sum of rest-length changes, m)
    crush: Vec<f32>,
    pub active: bool, calm: f32, dirty: bool, pub damaged: bool,
    skins: Vec<Skin>, pub model: Option<usize>,
    /// the body the damage belongs to (kept through a GPU reload, unlike `model`)
    shown: Option<usize>,
    /// the obstacle pressing in: contact point, direction, final depth, radius, time so far
    ind: Option<([f32; 3], [f32; 3], f32, f32, f32)>,
}

impl Default for Crash { fn default() -> Self { Self::new() } }

impl Crash {
    pub fn new() -> Crash {
        let mut x0 = Vec::with_capacity(N);
        for k in 0..NZ { for j in 0..NY { for i in 0..NX {
            x0.push([LO[0] + (HI[0] - LO[0]) * i as f32 / (NX - 1) as f32, LO[1] + (HI[1] - LO[1]) * j as f32 / (NY - 1) as f32, LO[2] + (HI[2] - LO[2]) * k as f32 / (NZ - 1) as f32]);
        } } }
        let mut springs = Vec::new();
        for k in 0..NZ { for j in 0..NY { for i in 0..NX {
            for dk in 0..=1i32 { for dj in -1..=1i32 { for di in -1..=1i32 {
                // each neighbour pair once: forward in k, or same k and forward in j, or same j and k and forward in i
                if (dk, dj, di) <= (0, 0, 0) { continue; }
                let (ni, nj, nk) = (i as i32 + di, j as i32 + dj, k as i32 + dk);
                if ni < 0 || nj < 0 || ni >= NX as i32 || nj >= NY as i32 || nk >= NZ as i32 { continue; }
                let (a, b) = (idx(i, j, k), idx(ni as usize, nj as usize, nk as usize)); let l = len(sub(x0[b], x0[a]));
                springs.push((a as u16, b as u16, l, l));
            } } }
        } } }
        Crash { home: x0.clone(), x: x0.clone(), v: vec![[0.0; 3]; N], x0, springs, crush: vec![0.0; N], active: false, calm: 0.0, dirty: false, damaged: false, skins: Vec::new(), model: None, shown: None, ind: None }
    }

    /// straighten everything out (a new run)
    pub fn repair(&mut self) {
        if !self.damaged && !self.active { return; }
        self.x = self.x0.clone(); self.home = self.x0.clone(); self.v = vec![[0.0; 3]; N]; for s in self.springs.iter_mut() { s.2 = s.3; } self.crush = vec![0.0; N];
        self.active = false; self.damaged = false; self.dirty = true; self.ind = None;
    }

    /// A hit. `p`: where, `dir`: unit direction of the push into the car, both in the car's frame; `speed`: closing speed (m/s).
    /// Below about 2 m/s the metal only flexes; harder hits dent, the hardest bend the shell.
    pub fn hit(&mut self, p: [f32; 3], dir: [f32; 3], speed: f32) {
        let s = (speed - 2.0).clamp(0.0, 16.0); if s <= 0.0 { return; }
        let depth = (0.0028 * s * s + 0.008 * s).min(0.5); let r = 0.32 + 0.02 * s;
        // a scrape keeps reporting contact: a weaker report while one is still pressing changes nothing
        if let Some(i) = self.ind { if i.4 < IND_T && i.2 >= depth { return; } }
        // the indenter starts at the face of the cage: walk back out of the car along the push until leaving the cage
        let mut out = 1e9f32; for q in 0..3 { if dir[q].abs() > 1e-6 { let t = if dir[q] > 0.0 { (p[q] - LO[q]) / dir[q] } else { (p[q] - HI[q]) / dir[q] }; out = out.min(t.max(0.0)); } }
        let p = [p[0] - dir[0] * out, p[1] - dir[1] * out, p[2] - dir[2] * out];
        self.ind = Some((p, dir, depth, r, 0.0)); self.active = true; self.calm = 0.0; self.damaged = true;
    }

    /// advance the cage (only while something is moving); true if the shape changed
    pub fn step(&mut self, dt: f32) -> bool {
        if !self.active { return false; }
        let h = dt.min(1.0 / 30.0) / SUB as f32; let mut a = vec![[0.0f32; 3]; N];
        for _ in 0..SUB {
            for v in a.iter_mut() { *v = [0.0; 3]; }
            for s in &self.springs {
                let (i, j) = (s.0 as usize, s.1 as usize); let d = sub(self.x[j], self.x[i]); let l = len(d).max(1e-5); let u = [d[0] / l, d[1] / l, d[2] / l];
                let f = K * (l - s.2) + C * dot(sub(self.v[j], self.v[i]), u);
                for q in 0..3 { a[i][q] += f * u[q]; a[j][q] -= f * u[q]; }
            }
            for n in 0..N { for q in 0..3 { a[n][q] -= KH * (self.x[n][q] - self.home[n][q]); } }
            let keep = 1.0 - DAMP * h;
            for n in 0..N { for q in 0..3 { self.v[n][q] = self.v[n][q] * keep + a[n][q] * h; self.x[n][q] += self.v[n][q] * h; } }
            // plastic flow: a spring strained past yield takes on its new length (within the crush limits)
            for s in self.springs.iter_mut() {
                let (i, j) = (s.0 as usize, s.1 as usize); let l = len(sub(self.x[j], self.x[i])); let e = (l - s.2) / s.2;
                if e.abs() > YIELD { let nr = (l / (1.0 + YIELD * e.signum())).clamp(s.3 * SHORTEST, s.3 * LONGEST); let dl = (nr - s.2).abs(); s.2 = nr; self.crush[i] += dl; self.crush[j] += dl; }
            }
            for n in 0..N { let e = sub(self.x[n], self.home[n]); let l = len(e);
                if l > YH { let k = 1.0 - YH / l; for q in 0..3 { self.home[n][q] += e[q] * k; } self.crush[n] += l * k; } }
            if let Some((p, dir, depth, r, t)) = self.ind.as_mut().map(|i| { i.4 += h; *i }) {
                let d = depth * (t / IND_T).min(1.0);
                for n in 0..N {
                    let rl = sub(self.x0[n], p); let along = dot(rl, dir); let lat = sub(rl, [dir[0] * along, dir[1] * along, dir[2] * along]);
                    let w = (-dot(lat, lat) / (r * r)).exp(); if w < 0.02 { continue; }
                    let sn = dot(sub(self.x[n], p), dir); let want = d * w;   // the indenter's face, rounded at its edge
                    if sn < want { for q in 0..3 { self.x[n][q] += dir[q] * (want - sn); } }
                }
                if t >= IND_T { self.ind = None; }
            }
        }
        let ke: f32 = self.v.iter().map(|v| dot(*v, *v)).sum::<f32>() / N as f32;
        if ke < 1e-4 { self.calm += dt; } else { self.calm = 0.0; }
        if self.calm > 0.25 && self.ind.is_none() { self.active = false; self.v = vec![[0.0; 3]; N]; }
        self.dirty = true; true
    }

    /// tie the body meshes of model `k` to the cage (once per model and scene)
    pub fn bind(&mut self, r: &Renderer, model: usize, objs: &[usize]) {
        if self.model == Some(model) && !self.skins.is_empty() { return; }
        if self.shown.is_some_and(|m| m != model) { self.repair(); }   // another body on show: it starts out straight
        self.model = Some(model); self.shown = Some(model); self.skins.clear();
        let cs = [(HI[0] - LO[0]) / (NX - 1) as f32, (HI[1] - LO[1]) / (NY - 1) as f32, (HI[2] - LO[2]) / (NZ - 1) as f32];
        for &obj in objs {
            let Some((p, n)) = r.car_rest(obj) else { continue };
            let nv = p.len() / 3; let (mut cell, mut frac) = (Vec::with_capacity(nv), Vec::with_capacity(nv));
            for v in 0..nv {
                let mut c = [0usize; 3]; let mut f = [0.0f32; 3];
                for q in 0..3 { let lim = [NX, NY, NZ][q] - 2; let t = ((p[v * 3 + q] - LO[q]) / cs[q]).clamp(0.0, (lim + 1) as f32);
                    c[q] = (t.floor() as usize).min(lim); f[q] = t - c[q] as f32; }
                cell.push(idx(c[0], c[1], c[2]) as u32); frac.push(f);
            }
            self.skins.push(Skin { obj, rest_p: p.clone(), rest_n: n.clone(), cell, frac, pos: p.clone(), nrm: n.clone() });
        }
        if self.skins.is_empty() { self.model = None; return; }   // the scene isn't on the GPU yet
        self.dirty = self.damaged;
    }
    /// the GPU lost its copy (new scene): bind again and send the shape again
    pub fn rebind(&mut self) { self.skins.clear(); self.model = None; }

    /// send the reshaped meshes to the renderer if the shape changed
    pub fn upload(&mut self, r: &mut Renderer) {
        if !self.dirty { return; } self.dirty = false;
        let cs = [(HI[0] - LO[0]) / (NX - 1) as f32, (HI[1] - LO[1]) / (NY - 1) as f32, (HI[2] - LO[2]) / (NZ - 1) as f32];
        let d: Vec<[f32; 3]> = (0..N).map(|n| sub(self.x[n], self.x0[n])).collect();
        let (dx, dy, dz) = (1usize, NX, NX * NY);
        for sk in self.skins.iter_mut() {
            let nv = sk.rest_p.len() / 3; let has_n = sk.rest_n.len() == sk.rest_p.len();
            for v in 0..nv {
                let b = sk.cell[v] as usize; let [u, w, t] = sk.frac[v];
                let c = [b, b + dx, b + dy, b + dx + dy, b + dz, b + dx + dz, b + dy + dz, b + dx + dy + dz];
                // trilinear weights and their derivatives along each axis of the cell
                let wt = [(1.0 - u) * (1.0 - w) * (1.0 - t), u * (1.0 - w) * (1.0 - t), (1.0 - u) * w * (1.0 - t), u * w * (1.0 - t), (1.0 - u) * (1.0 - w) * t, u * (1.0 - w) * t, (1.0 - u) * w * t, u * w * t];
                let du = [-(1.0 - w) * (1.0 - t), (1.0 - w) * (1.0 - t), -w * (1.0 - t), w * (1.0 - t), -(1.0 - w) * t, (1.0 - w) * t, -w * t, w * t];
                let dw = [-(1.0 - u) * (1.0 - t), -u * (1.0 - t), (1.0 - u) * (1.0 - t), u * (1.0 - t), -(1.0 - u) * t, -u * t, (1.0 - u) * t, u * t];
                let dt_ = [-(1.0 - u) * (1.0 - w), -u * (1.0 - w), -(1.0 - u) * w, -u * w, (1.0 - u) * (1.0 - w), u * (1.0 - w), (1.0 - u) * w, u * w];
                let (mut off, mut fa, mut fb, mut fc, mut cr) = ([0.0f32; 3], [0.0f32; 3], [0.0f32; 3], [0.0f32; 3], 0.0f32);
                for q in 0..8 { let dn = d[c[q]]; cr += wt[q] * self.crush[c[q]];
                    for e in 0..3 { off[e] += wt[q] * dn[e]; fa[e] += du[q] * dn[e]; fb[e] += dw[q] * dn[e]; fc[e] += dt_[q] * dn[e]; } }
                // deformation gradient F = I + dD/dX (columns fa, fb, fc, rescaled to metres); normals turn with its cofactor
                let fa = [1.0 + fa[0] / cs[0], fa[1] / cs[0], fa[2] / cs[0]]; let fb = [fb[0] / cs[1], 1.0 + fb[1] / cs[1], fb[2] / cs[1]]; let fc = [fc[0] / cs[2], fc[1] / cs[2], 1.0 + fc[2] / cs[2]];
                let rp = [sk.rest_p[v * 3], sk.rest_p[v * 3 + 1], sk.rest_p[v * 3 + 2]];
                let mut nn = if has_n {
                    let n0 = [sk.rest_n[v * 3], sk.rest_n[v * 3 + 1], sk.rest_n[v * 3 + 2]]; let (bc, ca, ab) = (cross(fb, fc), cross(fc, fa), cross(fa, fb));
                    [bc[0] * n0[0] + ca[0] * n0[1] + ab[0] * n0[2], bc[1] * n0[0] + ca[1] * n0[1] + ab[1] * n0[2], bc[2] * n0[0] + ca[2] * n0[1] + ab[2] * n0[2]]
                } else { [0.0, 0.0, 0.0] };
                let nl = len(nn); if nl > 1e-6 { for e in 0..3 { nn[e] /= nl; } }
                // crumpled metal: creases where the springs yielded
                let amp = (cr * 0.12).min(0.06);
                if amp > 1e-4 && has_n {
                    let (wv, g) = wrinkle(rp); for e in 0..3 { off[e] += nn[e] * amp * wv; }
                    let gn = dot(g, nn); let k = (amp * 0.9).min(0.05);
                    for e in 0..3 { nn[e] -= (g[e] - gn * nn[e]) * k; } let nl = len(nn); if nl > 1e-6 { for e in 0..3 { nn[e] /= nl; } }
                }
                for e in 0..3 { sk.pos[v * 3 + e] = rp[e] + off[e]; } if has_n { for e in 0..3 { sk.nrm[v * 3 + e] = nn[e]; } }
            }
            r.reshape(sk.obj, &sk.pos, if has_n { Some(&sk.nrm) } else { None });
        }
    }

    /// how far a point of the shell (car frame) has moved, for tests and tools
    pub fn displacement_at(&self, p: [f32; 3]) -> [f32; 3] {
        let cs = [(HI[0] - LO[0]) / (NX - 1) as f32, (HI[1] - LO[1]) / (NY - 1) as f32, (HI[2] - LO[2]) / (NZ - 1) as f32];
        let mut c = [0usize; 3]; let mut f = [0.0f32; 3];
        for q in 0..3 { let lim = [NX, NY, NZ][q] - 2; let t = ((p[q] - LO[q]) / cs[q]).clamp(0.0, (lim + 1) as f32); c[q] = (t.floor() as usize).min(lim); f[q] = t - c[q] as f32; }
        let mut o = [0.0f32; 3];
        for (q, (oi, oj, ok)) in [(0, 0, 0), (1, 0, 0), (0, 1, 0), (1, 1, 0), (0, 0, 1), (1, 0, 1), (0, 1, 1), (1, 1, 1)].iter().enumerate() {
            let _ = q; let n = idx(c[0] + oi, c[1] + oj, c[2] + ok);
            let w = (if *oi == 1 { f[0] } else { 1.0 - f[0] }) * (if *oj == 1 { f[1] } else { 1.0 - f[1] }) * (if *ok == 1 { f[2] } else { 1.0 - f[2] });
            let dn = sub(self.x[n], self.x0[n]); for e in 0..3 { o[e] += w * dn[e]; }
        }
        o
    }
}

/// Where a hit landed and which way it pushed, from what it did to the car this frame: the change in velocity (world, m/s)
/// gives the push direction, the change in yaw rate gives its lever arm (the impulse's line of action); the contact is where
/// that line enters the car's footprint from outside. Returns (point, direction into the car), car frame.
pub fn locate(dvx: f64, dvz: f64, dyaw: f64, heading: f64, mass: f64, iz: f64) -> Option<([f32; 3], [f32; 3])> {
    let dv = dvx.hypot(dvz); if dv < 0.5 { return None; }
    let (sh, ch) = (heading.sin(), heading.cos());
    // into the car's frame (x right, z forward)
    let (nx, nz) = ((ch * dvx - sh * dvz) / dv, (sh * dvx + ch * dvz) / dv);
    // lever arm, as the physics computes it: dyaw = j (pz nx - px nz) / iz (a 2D cross product, the same in the car's frame)
    let arm = dyaw * iz / (mass * dv);
    let (tx, tz) = (-nz, nx);
    let (bx, bz) = (0.9, 2.05);
    // the line p(a) = arm * t + a * n; enter the box from the -n side (the obstacle's side)
    let (px, pz) = (arm * tx, arm * tz);
    let slab = |p: f64, d: f64, h: f64| -> (f64, f64) { if d.abs() < 1e-9 { if p.abs() <= h { (-1e9, 1e9) } else { (1e9, -1e9) } } else { let (a, b) = ((-h - p) / d, (h - p) / d); (a.min(b), a.max(b)) } };
    let (a0, a1) = slab(px, nx, bx); let (b0, b1) = slab(pz, nz, bz); let enter = a0.max(b0);
    let (cx, cz) = if enter <= a1.min(b1) { (px + enter * nx, pz + enter * nz) } else {
        // the line misses the footprint (glancing blow): the edge point on the struck side
        let k = (bx / nx.abs().max(1e-6)).min(bz / nz.abs().max(1e-6)); (-nx * k, -nz * k) };
    Some(([cx as f32, 0.6, cz as f32], [nx as f32, 0.0, nz as f32]))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn settle(c: &mut Crash) { for _ in 0..600 { c.step(1.0 / 60.0); if !c.active { break; } } }
    #[test]
    fn a_tap_flexes_a_crash_dents() {
        let nose = [0.0, 0.6, 2.05];
        let mut c = Crash::new(); c.hit(nose, [0.0, 0.0, -1.0], 3.0); settle(&mut c);
        let d = c.displacement_at(nose); assert!(len(d) < 0.03, "a 3 m/s tap leaves {:?}", d);
        let mut c = Crash::new(); c.hit(nose, [0.0, 0.0, -1.0], 14.0); settle(&mut c); assert!(!c.active, "settles");
        let d = c.displacement_at(nose); assert!(d[2] < -0.08 && d[2] > -0.6, "a 14 m/s front hit crushes the nose {:?}", d);
        let tail = c.displacement_at([0.0, 0.6, -2.0]); assert!(len(tail) < 0.06, "the tail stays put {:?}", tail);
    }
    #[test]
    fn a_corner_hit_bends_the_shell() {
        let mut c = Crash::new(); c.hit([0.9, 0.9, 1.9], [-0.7, 0.0, -0.7], 16.0); settle(&mut c);
        let fl = c.displacement_at([0.9, 0.9, 1.9]); let fr = c.displacement_at([-0.9, 0.9, 1.9]);
        assert!(fl[0] < -0.05, "front-right corner pushed in {:?}", fl); assert!(len(fl) > len(fr) * 1.5, "more on the struck side {:?} {:?}", fl, fr);
    }
    #[test]
    fn locates_hits() {
        // driving north (heading 0, forward +z) into a tree dead ahead: pushed back, no yaw change -> the nose
        let (p, d) = locate(0.0, -8.0, 0.0, 0.0, 1180.0, 1900.0).unwrap(); assert!(p[2] > 2.0 && p[0].abs() < 0.05 && d[2] < -0.99, "{:?} {:?}", p, d);
        // pushed to the left (world -x is car left... heading 0: car x = world x) from the right side
        let (p, d) = locate(-5.0, 0.0, 0.0, 0.0, 1180.0, 1900.0).unwrap(); assert!(p[0] > 0.85 && d[0] < -0.99, "{:?} {:?}", p, d);
    }
}
