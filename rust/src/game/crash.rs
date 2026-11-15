//! Crash damage: a visual-only soft body for your car's bodywork, in the spirit of BeamNG's node-and-beam cars (much smaller,
//! and it never touches the driving physics).
//!
//! The structure is built from the car's own body: its shell is gathered into nodes about 0.3 m apart (each node sits at the
//! average of the body's vertices around it, so they follow the bonnet, the wings, the pillars and the roof), plus two
//! chassis rails low down along the car. Beams join every node to its neighbours within about half a metre; the rails are
//! stiffer and harder to bend. Beams are plastic: strained past their yield point they take on their new length (within a
//! crush limit), so the metal keeps a dent, a front end folds back on itself and a hard corner hit leaves the shell bent or
//! twisted. Each node is also tied to its place on the frame by a spring that yields too (the structure behind the panel and
//! the inertia of the rest of the car), so a push fades within a metre and the far end of the car stays straight.
//!
//! Hits come from what was actually hit: the obstacle's real shape (a tree trunk or a post as a cylinder of its radius, a rock,
//! the other car as its rounded box, a wall as a plane) is pressed into the car along the direction of the impulse for about
//! a tenth of a second, as deep as the closing speed says a car's structure gives way (a narrow pole goes deeper than a flat
//! flank). Nodes it reaches are pushed out to its surface, so a pole leaves a narrow deep notch wrapped round it and a car's
//! flank a wide shallow dent (they are driven along the push, not squeezed out sideways); the beams carry the push into the panels around it; then it lets go and the metal rings.
//!
//! Every vertex of the body meshes follows its four nearest nodes (each node's position and its local rotation, found by
//! shape matching its beam neighbours), so bent panels turn and catch the light; where the beams yielded the panels also
//! crumple into small creases. Nothing runs between hits: the structure sleeps once still and the meshes keep their shape.
use crate::render::Renderer;

/// beam stiffness and damping (per unit node mass, 1/s2 and 1/s), overall damping (1/s), yield strain, crush limits
const K: f32 = 1500.0; const C: f32 = 9.0; const DAMP: f32 = 4.0; const YIELD: f32 = 0.018; const SHORTEST: f32 = 0.3; const LONGEST: f32 = 1.25;
/// each node's tie to its place on the frame: stiffness (1/s2) and how far it gives before it yields (m)
const KH: f32 = 800.0; const YH: f32 = 0.025;
const SUB: usize = 8;
/// how long a hit presses (s), node spacing on the shell (m), beam reach (m)
const IND_T: f32 = 0.09; const CELL: f32 = 0.3; const REACH: f32 = 0.52;

#[inline] fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] { [a[0] - b[0], a[1] - b[1], a[2] - b[2]] }
#[inline] fn dot(a: [f32; 3], b: [f32; 3]) -> f32 { a[0] * b[0] + a[1] * b[1] + a[2] * b[2] }
#[inline] fn len(a: [f32; 3]) -> f32 { dot(a, a).sqrt() }
type M3 = [[f32; 3]; 3];
#[inline] fn mv(m: &M3, v: [f32; 3]) -> [f32; 3] { [dot(m[0], v), dot(m[1], v), dot(m[2], v)] }
fn det(m: &M3) -> f32 { m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]) }
const I3: M3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
/// the rotation part of a matrix (polar decomposition by averaging with the inverse transpose)
fn polar(a: &M3) -> M3 {
    let mut r = *a; if det(&r) <= 1e-9 { return I3; }
    for _ in 0..8 { let d = det(&r); if d.abs() < 1e-9 { return I3; }
        // inverse transpose = cofactor / det
        let c = [[r[1][1] * r[2][2] - r[1][2] * r[2][1], r[1][2] * r[2][0] - r[1][0] * r[2][2], r[1][0] * r[2][1] - r[1][1] * r[2][0]],
                 [r[0][2] * r[2][1] - r[0][1] * r[2][2], r[0][0] * r[2][2] - r[0][2] * r[2][0], r[0][1] * r[2][0] - r[0][0] * r[2][1]],
                 [r[0][1] * r[1][2] - r[0][2] * r[1][1], r[0][2] * r[1][0] - r[0][0] * r[1][2], r[0][0] * r[1][1] - r[0][1] * r[1][0]]];
        for i in 0..3 { for j in 0..3 { r[i][j] = 0.5 * (r[i][j] + c[i][j] / d); } } }
    r
}
/// the crease pattern: two crossed sine waves (about 0.7 m and 0.8 m across) and their gradient
#[inline] fn wrinkle(p: [f32; 3]) -> (f32, [f32; 3]) {
    const F1: [f32; 3] = [7.3, 5.1, 9.7]; const F2: [f32; 3] = [-6.1, 8.7, 4.3];
    let (a, b) = (dot(F1, p) + 0.7, dot(F2, p) + 2.1); let (sa, ca, sb, cb) = (a.sin(), a.cos(), b.sin(), b.cos());
    (sa * sb, [ca * sb * F1[0] + sa * cb * F2[0], ca * sb * F1[1] + sa * cb * F2[1], ca * sb * F1[2] + sa * cb * F2[2]])
}

/// What hit the car, in the car's frame (x right, y up, z forward), placed where it is now. `top`: how high it reaches (m).
#[derive(Clone, Copy, Debug)]
pub enum Shape {
    /// a tree, a post, a rock: a vertical cylinder
    Circle { c: [f32; 2], r: f32, top: f32 },
    /// another car: a rounded box, `ax` its forward direction (unit), half length and half width
    Box { c: [f32; 2], ax: [f32; 2], hl: f32, hw: f32, top: f32 },
}
impl Shape {
    /// signed distance from a point to the shape moved by `off` (negative inside) and the way out
    fn sdf(&self, p: [f32; 3], off: [f32; 3]) -> (f32, [f32; 3]) {
        let (x, z) = (p[0] - off[0], p[2] - off[2]);
        match *self {
            Shape::Circle { c, r, top } => { if p[1] > top { return (1e9, [0.0; 3]); }
                let (dx, dz) = (x - c[0], z - c[1]); let d = dx.hypot(dz).max(1e-6); (d - r, [dx / d, 0.0, dz / d]) }
            Shape::Box { c, ax, hl, hw, top } => { if p[1] > top { return (1e9, [0.0; 3]); }
                let rc = 0.25; let (dx, dz) = (x - c[0], z - c[1]); let (u, v) = (dx * ax[0] + dz * ax[1], -dx * ax[1] + dz * ax[0]);
                let (qu, qv) = (u.abs() - (hl - rc), v.abs() - (hw - rc)); let (ou, ov) = (qu.max(0.0), qv.max(0.0)); let ol = ou.hypot(ov);
                let d = ol + qu.max(qv).min(0.0) - rc;
                let (gu, gv) = if ol > 1e-6 { (ou / ol * u.signum(), ov / ol * v.signum()) } else if qu > qv { (u.signum(), 0.0) } else { (0.0, v.signum()) };
                (d, [gu * ax[0] - gv * ax[1], 0.0, gu * ax[1] + gv * ax[0]]) }
        }
    }
    /// how much deeper a narrow thing goes than a broad one, for the same closing speed
    fn sharpness(&self) -> f32 { match *self { Shape::Circle { r, .. } => (0.5 / (r + 0.15)).powf(0.35).clamp(0.8, 1.4), Shape::Box { .. } => 0.85 } }
}

/// one body mesh tied to the nodes
struct Skin { obj: usize, rest_p: Vec<f32>, rest_n: Vec<f32>, nodes: Vec<[u16; 4]>, w: Vec<[f32; 4]>, pos: Vec<f32>, nrm: Vec<f32> }

pub struct Crash {
    x0: Vec<[f32; 3]>, x: Vec<[f32; 3]>, v: Vec<[f32; 3]>, home: Vec<[f32; 3]>,
    /// node a, node b, rest length now, rest length as built, yield strain
    beams: Vec<(u16, u16, f32, f32, f32)>,
    /// beam neighbours of each node (for its local rotation)
    nbr: Vec<Vec<u16>>,
    /// how much the metal around each node has yielded (m)
    crush: Vec<f32>,
    pub active: bool, calm: f32, dirty: bool, pub damaged: bool,
    skins: Vec<Skin>, pub model: Option<usize>,
    /// the body the damage belongs to (kept through a GPU reload, unlike `model`)
    shown: Option<usize>,
    /// what is pressing in: its shape, direction, where it started (offset along dir), final depth, time so far
    ind: Option<(Shape, [f32; 3], f32, f32, f32)>,
}

impl Default for Crash { fn default() -> Self { Self::new() } }

impl Crash {
    pub fn new() -> Crash {
        Crash { x0: Vec::new(), x: Vec::new(), v: Vec::new(), home: Vec::new(), beams: Vec::new(), nbr: Vec::new(), crush: Vec::new(), active: false, calm: 0.0,
            dirty: false, damaged: false, skins: Vec::new(), model: None, shown: None, ind: None }
    }
    pub fn nodes(&self) -> usize { self.x0.len() }

    /// Build the structure from points on the body (car frame): shell nodes about CELL apart, two chassis rails, beams.
    pub fn build(&mut self, pts: &[[f32; 3]]) {
        use std::collections::HashMap;
        let mut cells: HashMap<(i32, i32, i32), ([f32; 3], u32)> = HashMap::new();
        let (mut lo, mut hi) = ([1e9f32; 3], [-1e9f32; 3]);
        for p in pts { let k = ((p[0] / CELL).floor() as i32, (p[1] / CELL).floor() as i32, (p[2] / CELL).floor() as i32);
            let e = cells.entry(k).or_insert(([0.0; 3], 0)); for q in 0..3 { e.0[q] += p[q]; lo[q] = lo[q].min(p[q]); hi[q] = hi[q].max(p[q]); } e.1 += 1; }
        let mut keys: Vec<_> = cells.keys().copied().collect(); keys.sort();
        let mut x0: Vec<[f32; 3]> = keys.iter().map(|k| { let (s, n) = cells[k]; [s[0] / n as f32, s[1] / n as f32, s[2] / n as f32] }).collect();
        let shell = x0.len();
        // chassis rails: low, inboard, nose to tail
        let (z0, z1) = (lo[2] + 0.3, hi[2] - 0.3); let nr = ((z1 - z0) / 0.35).ceil().max(1.0) as usize;
        for s in [-1.0f32, 1.0] { for i in 0..=nr { x0.push([s * 0.5, (lo[1] + 0.12).max(0.3), z0 + (z1 - z0) * i as f32 / nr as f32]); } }
        let n = x0.len(); let mut beams = Vec::new(); let mut nbr = vec![Vec::new(); n];
        for a in 0..n { for b in a + 1..n {
            let l = len(sub(x0[b], x0[a])); let rail = a >= shell && b >= shell;
            let reach = if rail { 0.4 } else if a >= shell || b >= shell { 0.6 } else { REACH };
            if l < reach && l > 1e-3 { beams.push((a as u16, b as u16, l, l, if rail { YIELD * 2.5 } else { YIELD })); nbr[a].push(b as u16); nbr[b].push(a as u16); }
        } }
        self.x = x0.clone(); self.home = x0.clone(); self.v = vec![[0.0; 3]; n]; self.crush = vec![0.0; n]; self.x0 = x0; self.beams = beams; self.nbr = nbr;
        self.active = false; self.damaged = false; self.ind = None; self.dirty = true;
    }

    /// straighten everything out (a new run)
    pub fn repair(&mut self) {
        if !self.damaged && !self.active { return; }
        self.x = self.x0.clone(); self.home = self.x0.clone(); self.v = vec![[0.0; 3]; self.x0.len()]; for b in self.beams.iter_mut() { b.2 = b.3; } self.crush = vec![0.0; self.x0.len()];
        self.active = false; self.damaged = false; self.dirty = true; self.ind = None;
    }

    /// A hit by `shape` pushing the car along `dir` (unit, car frame) at closing speed `speed` (m/s).
    /// Below about 2 m/s the metal only flexes; harder hits dent, the hardest fold the structure.
    pub fn hit(&mut self, shape: Shape, dir: [f32; 3], speed: f32) {
        if self.x0.is_empty() { return; }
        let s = (speed - 2.0).clamp(0.0, 16.0); if s <= 0.0 { return; }
        let depth = ((0.0028 * s * s + 0.008 * s) * shape.sharpness()).min(0.55);
        // a scrape keeps reporting contact: a weaker report while one is still pressing changes nothing
        if let Some(i) = self.ind { if i.4 < IND_T && i.3 >= depth { return; } }
        // start from where it just touches the body: slide it along the push until no node is inside
        let inside = |t: f32| self.x.iter().any(|p| shape.sdf(*p, [dir[0] * t, 0.0, dir[2] * t]).0 < 0.0);
        let (mut a, mut b) = (-3.0f32, 3.0f32); if inside(a) { return; }
        if !inside(b) { return; }   // it never reaches the body (a glancing touch)
        for _ in 0..24 { let m = (a + b) * 0.5; if inside(m) { b = m; } else { a = m; } }
        self.ind = Some((shape, dir, a, depth, 0.0)); self.active = true; self.calm = 0.0; self.damaged = true;
    }

    /// advance the structure (only while something is moving); true if the shape changed
    pub fn step(&mut self, dt: f32) -> bool {
        if !self.active { return false; }
        let n = self.x0.len(); let h = dt.min(1.0 / 30.0) / SUB as f32; let mut acc = vec![[0.0f32; 3]; n];
        for _ in 0..SUB {
            for (i, a) in acc.iter_mut().enumerate() { for q in 0..3 { a[q] = -KH * (self.x[i][q] - self.home[i][q]); } }
            for b in &self.beams {
                let (i, j) = (b.0 as usize, b.1 as usize); let d = sub(self.x[j], self.x[i]); let l = len(d).max(1e-5); let u = [d[0] / l, d[1] / l, d[2] / l];
                let f = K * (l - b.2) + C * dot(sub(self.v[j], self.v[i]), u);
                for q in 0..3 { acc[i][q] += f * u[q]; acc[j][q] -= f * u[q]; }
            }
            let keep = 1.0 - DAMP * h;
            for i in 0..n { for q in 0..3 { self.v[i][q] = self.v[i][q] * keep + acc[i][q] * h; self.x[i][q] += self.v[i][q] * h; } }
            // plastic flow: a beam strained past yield takes on its new length (within the crush limits)
            for b in self.beams.iter_mut() {
                let (i, j) = (b.0 as usize, b.1 as usize); let l = len(sub(self.x[j], self.x[i])); let e = (l - b.2) / b.2;
                if e.abs() > b.4 { let nr = (l / (1.0 + b.4 * e.signum())).clamp(b.3 * SHORTEST, b.3 * LONGEST); let dl = (nr - b.2).abs(); b.2 = nr; self.crush[i] += dl; self.crush[j] += dl; }
            }
            for i in 0..n { let e = sub(self.x[i], self.home[i]); let l = len(e);
                if l > YH { let k = 1.0 - YH / l; for q in 0..3 { self.home[i][q] += e[q] * k; } self.crush[i] += l * k; } }
            // the obstacle pressing in: nodes it reaches go out to its surface
            if let Some((shape, dir, t0, depth, t)) = self.ind.as_mut().map(|i| { i.4 += h; *i }) {
                let tt = t0 + depth * (t / IND_T).min(1.0); let off = [dir[0] * tt, 0.0, dir[2] * tt];
                for i in 0..n { if shape.sdf(self.x[i], off).0 >= 0.0 { continue; }
                    // driven along the push until clear of it (not squeezed out sideways): metal round a pole wraps it, it doesn't part
                    let (mut lo, mut hi) = (0.0f32, 1.5f32);
                    for _ in 0..14 { let m = (lo + hi) * 0.5; let p = [self.x[i][0] + dir[0] * m, self.x[i][1], self.x[i][2] + dir[2] * m]; if shape.sdf(p, off).0 < 0.0 { lo = m; } else { hi = m; } }
                    for q in [0, 2] { self.x[i][q] += dir[q] * hi; } }
                if t >= IND_T { self.ind = None; }
            }
        }
        let ke: f32 = self.v.iter().map(|v| dot(*v, *v)).sum::<f32>() / n.max(1) as f32;
        if ke < 1e-4 { self.calm += dt; } else { self.calm = 0.0; }
        if self.calm > 0.25 && self.ind.is_none() { self.active = false; self.v = vec![[0.0; 3]; n]; }
        self.dirty = true; true
    }

    /// each node's local rotation, by shape matching its beam neighbours
    fn rotations(&self) -> Vec<M3> {
        (0..self.x0.len()).map(|i| {
            let mut a = [[0.0f32; 3]; 3];
            for &j in &self.nbr[i] { let j = j as usize; let (p, q) = (sub(self.x[j], self.x[i]), sub(self.x0[j], self.x0[i]));
                for r in 0..3 { for c in 0..3 { a[r][c] += p[r] * q[c]; } } }
            if self.nbr[i].len() < 3 { I3 } else { polar(&a) }
        }).collect()
    }

    /// the nearest four nodes of a point and their weights
    fn weights(&self, p: [f32; 3]) -> ([u16; 4], [f32; 4]) {
        let mut best = [(f32::MAX, 0u16); 4];
        for (i, x) in self.x0.iter().enumerate() { let d = dot(sub(*x, p), sub(*x, p));
            if d < best[3].0 { best[3] = (d, i as u16); let mut k = 3; while k > 0 && best[k].0 < best[k - 1].0 { best.swap(k, k - 1); k -= 1; } } }
        let mut w = [0.0f32; 4]; let mut s = 0.0; for k in 0..4 { w[k] = 1.0 / (best[k].0 + 0.004); s += w[k]; } for v in w.iter_mut() { *v /= s; }
        ([best[0].1, best[1].1, best[2].1, best[3].1], w)
    }

    /// where a point of the body (car frame, at rest) is now
    pub fn deformed(&self, p: [f32; 3]) -> [f32; 3] {
        let (nd, w) = self.weights(p); let rot = self.rotations(); let mut o = [0.0f32; 3];
        for k in 0..4 { let i = nd[k] as usize; let r = mv(&rot[i], sub(p, self.x0[i])); for q in 0..3 { o[q] += w[k] * (self.x[i][q] + r[q]); } }
        o
    }

    /// build the structure for model `k` from its body meshes and tie them to it (once per model and scene)
    pub fn bind(&mut self, r: &Renderer, model: usize, objs: &[usize]) {
        if self.model == Some(model) && !self.skins.is_empty() { return; }
        let keep = self.shown == Some(model) && !self.x0.is_empty();   // a GPU reload: same body, keep its damage
        let mut pts = Vec::new();
        for &obj in objs { if let Some((p, _)) = r.car_rest(obj) { for v in p.chunks_exact(3) { pts.push([v[0], v[1], v[2]]); } } }
        if pts.is_empty() { return; }   // the scene isn't on the GPU yet
        if !keep { self.build(&pts); }
        self.model = Some(model); self.shown = Some(model); self.skins.clear();
        for &obj in objs {
            let Some((p, n)) = r.car_rest(obj) else { continue };
            let (mut nodes, mut w) = (Vec::with_capacity(p.len() / 3), Vec::with_capacity(p.len() / 3));
            for v in p.chunks_exact(3) { let (a, b) = self.weights([v[0], v[1], v[2]]); nodes.push(a); w.push(b); }
            self.skins.push(Skin { obj, rest_p: p.clone(), rest_n: n.clone(), nodes, w, pos: p.clone(), nrm: n.clone() });
        }
        self.dirty = self.damaged;
    }
    /// the GPU lost its copy (new scene): bind again and send the shape again
    pub fn rebind(&mut self) { self.skins.clear(); self.model = None; }

    /// send the reshaped meshes to the renderer if the shape changed
    pub fn upload(&mut self, r: &mut Renderer) {
        if !self.dirty || self.skins.is_empty() { return; } self.dirty = false;
        let rot = self.rotations();
        let (x, x0, crush) = (&self.x, &self.x0, &self.crush);
        for sk in self.skins.iter_mut() {
            let nv = sk.rest_p.len() / 3; let has_n = sk.rest_n.len() == sk.rest_p.len();
            for v in 0..nv {
                let rp = [sk.rest_p[v * 3], sk.rest_p[v * 3 + 1], sk.rest_p[v * 3 + 2]]; let n0 = if has_n { [sk.rest_n[v * 3], sk.rest_n[v * 3 + 1], sk.rest_n[v * 3 + 2]] } else { [0.0; 3] };
                let (mut p, mut nn, mut cr) = ([0.0f32; 3], [0.0f32; 3], 0.0f32);
                for k in 0..4 { let i = sk.nodes[v][k] as usize; let w = sk.w[v][k]; let rr = mv(&rot[i], sub(rp, x0[i])); let rn = mv(&rot[i], n0); cr += w * crush[i];
                    for q in 0..3 { p[q] += w * (x[i][q] + rr[q]); nn[q] += w * rn[q]; } }
                let nl = len(nn); if nl > 1e-6 { for q in 0..3 { nn[q] /= nl; } }
                // crumpled metal: creases where the beams yielded
                let amp = (cr * 0.12).min(0.06);
                if amp > 1e-4 && has_n {
                    let (wv, g) = wrinkle(rp); for q in 0..3 { p[q] += nn[q] * amp * wv; }
                    let gn = dot(g, nn); let k = (amp * 0.9).min(0.05);
                    for q in 0..3 { nn[q] -= (g[q] - gn * nn[q]) * k; } let nl = len(nn); if nl > 1e-6 { for q in 0..3 { nn[q] /= nl; } }
                }
                sk.pos[v * 3..v * 3 + 3].copy_from_slice(&p); if has_n { sk.nrm[v * 3..v * 3 + 3].copy_from_slice(&nn); }
            }
            r.reshape(sk.obj, &sk.pos, if has_n { Some(&sk.nrm) } else { None });
        }
    }
}

/// Which way a hit pushed and roughly where, from what it did to the car this frame: the change in velocity (world, m/s)
/// gives the push direction, the change in yaw rate its lever arm (the impulse's line of action); the contact is where that
/// line enters the car's footprint from outside. Returns (point, direction into the car), car frame.
pub fn locate(dvx: f64, dvz: f64, dyaw: f64, heading: f64, mass: f64, iz: f64) -> Option<([f32; 3], [f32; 3])> {
    let dv = dvx.hypot(dvz); if dv < 0.5 { return None; }
    let (sh, ch) = (heading.sin(), heading.cos());
    let (nx, nz) = ((ch * dvx - sh * dvz) / dv, (sh * dvx + ch * dvz) / dv);
    // lever arm, as the physics computes it: dyaw = j (pz nx - px nz) / iz (a 2D cross product, the same in the car's frame)
    let arm = dyaw * iz / (mass * dv);
    let (tx, tz) = (-nz, nx); let (bx, bz) = (0.9, 2.05);
    let (px, pz) = (arm * tx, arm * tz);
    let slab = |p: f64, d: f64, h: f64| -> (f64, f64) { if d.abs() < 1e-9 { if p.abs() <= h { (-1e9, 1e9) } else { (1e9, -1e9) } } else { let (a, b) = ((-h - p) / d, (h - p) / d); (a.min(b), a.max(b)) } };
    let (a0, a1) = slab(px, nx, bx); let (b0, b1) = slab(pz, nz, bz); let enter = a0.max(b0);
    let (cx, cz) = if enter <= a1.min(b1) { (px + enter * nx, pz + enter * nz) } else {
        let k = (bx / nx.abs().max(1e-6)).min(bz / nz.abs().max(1e-6)); (-nx * k, -nz * k) };
    Some(([cx as f32, 0.6, cz as f32], [nx as f32, 0.0, nz as f32]))
}
/// a world offset into the car's frame (x right, z forward)
pub fn to_car(dx: f64, dz: f64, heading: f64) -> [f32; 2] { let (sh, ch) = (heading.sin(), heading.cos()); [(ch * dx - sh * dz) as f32, (sh * dx + ch * dz) as f32] }
/// a car-frame offset into the world
pub fn to_world(lx: f32, lz: f32, heading: f64) -> [f64; 2] { let (sh, ch) = (heading.sin(), heading.cos()); [ch * lx as f64 + sh * lz as f64, -sh * lx as f64 + ch * lz as f64] }

#[cfg(test)]
mod tests {
    use super::*;
    /// a car-sized body: points on a box (4.1 x 1.8 x 0.7 m) with a cabin on top
    pub fn body() -> Crash {
        let mut pts = Vec::new(); let st = 0.05;
        let mut add = |x0: f32, x1: f32, y0: f32, y1: f32, z0: f32, z1: f32| {
            let mut z = z0; while z <= z1 { let mut y = y0; while y <= y1 { for x in [x0, x1] { pts.push([x, y, z]); } y += st; } let mut x = x0; while x <= x1 { for y in [y0, y1] { pts.push([x, y, z]); } x += st; } z += st; }
            let mut x = x0; while x <= x1 { let mut y = y0; while y <= y1 { for z in [z0, z1] { pts.push([x, y, z]); } y += st; } x += st; } };
        add(-0.9, 0.9, 0.25, 0.95, -2.05, 2.05); add(-0.65, 0.65, 0.95, 1.4, -1.1, 0.7);
        let mut c = Crash::new(); c.build(&pts); c
    }
    fn settle(c: &mut Crash) { for _ in 0..600 { c.step(1.0 / 60.0); if !c.active { break; } } }
    fn moved(c: &Crash, p: [f32; 3]) -> [f32; 3] { sub(c.deformed(p), p) }
    #[test]
    fn structure_is_reasonable() { let c = body(); assert!(c.nodes() > 150 && c.nodes() < 700, "{} nodes", c.nodes()); assert!(c.beams.len() > c.nodes() * 4, "{} beams", c.beams.len()); }
    #[test]
    fn a_tap_flexes_a_tree_dents() {
        let nose = [0.0, 0.6, 2.05]; let tree = Shape::Circle { c: [0.0, 2.4], r: 0.25, top: 10.0 };
        let mut c = body(); c.hit(tree, [0.0, 0.0, -1.0], 3.0); settle(&mut c); assert!(len(moved(&c, nose)) < 0.04, "a 3 m/s tap leaves {:?}", moved(&c, nose));
        let mut c = body(); c.hit(tree, [0.0, 0.0, -1.0], 14.0); settle(&mut c); assert!(!c.active, "settles");
        let d = moved(&c, nose); assert!(d[2] < -0.12 && d[2] > -0.8, "a 14 m/s tree hit notches the nose {:?}", d);
        let side = moved(&c, [0.85, 0.6, 2.0]); assert!(side[2] > d[2] * 0.6, "a pole leaves a notch, the corners stay forward {:?} vs {:?}", side, d);
        let tail = moved(&c, [0.0, 0.6, -2.0]); assert!(len(tail) < 0.05, "the tail stays put {:?}", tail);
    }
    #[test]
    fn a_car_flank_dents_wide_and_shallow() {
        let other = Shape::Box { c: [2.0, 0.0], ax: [0.0, 1.0], hl: 2.05, hw: 0.9, top: 1.3 };
        let mut c = body(); c.hit(other, [-1.0, 0.0, 0.0], 12.0); settle(&mut c);
        let (a, b, roof) = (moved(&c, [0.9, 0.6, 0.0]), moved(&c, [0.9, 0.6, 1.4]), moved(&c, [0.0, 1.4, -0.2]));
        assert!(a[0] < -0.04 && b[0] < -0.02, "the whole flank goes in {:?} {:?}", a, b); assert!(len(roof) < 0.05, "the roof is above it {:?}", roof);
    }
    #[test]
    fn locates_hits() {
        let (p, d) = locate(0.0, -8.0, 0.0, 0.0, 1180.0, 1900.0).unwrap(); assert!(p[2] > 2.0 && p[0].abs() < 0.05 && d[2] < -0.99, "{:?} {:?}", p, d);
        let (p, d) = locate(-5.0, 0.0, 0.0, 0.0, 1180.0, 1900.0).unwrap(); assert!(p[0] > 0.85 && d[0] < -0.99, "{:?} {:?}", p, d);
        let w = to_world(0.3, 1.7, 0.8); let c = to_car(w[0], w[1], 0.8); assert!((c[0] - 0.3).abs() < 1e-5 && (c[1] - 1.7).abs() < 1e-5);
    }
}
