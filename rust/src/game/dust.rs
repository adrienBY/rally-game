//! Dust, gravel and tyre smoke: a small CPU particle system that is cheap but behaves like the real thing.
//!
//! Kinds (each with its own drag, gravity, growth and look):
//!  * dust: fine powder lifted at the contact patch; light, so it relaxes quickly to the surrounding air (wind, the wake of
//!    passing cars, turbulence), settles slowly, and spreads out: its radius grows like the square root of its age (diffusion)
//!    while it thins out in proportion (the same dust over a bigger volume), so a trail stays visible but never turns into a wall
//!  * grit: stones flung back by spinning tyres; heavy, ballistic, gone (with a little puff) when they land
//!  * smoke: sliding on tarmac; rises, grows fast, very faint
//!  * powder / chunks: the snow versions of dust and grit
//! The air around each car is modelled cheaply but physically (in the car's frame, within 10 m):
//!  * the body is a solid (a rounded box): air cannot go through it, so it slides along the surface and smoke wraps round the
//!    car; next to the skin it is dragged along with the car (boundary layer); a particle that ends up inside is pushed out
//!  * behind it the wake: low pressure drags air after the car and pulls it in towards the middle, and two vortices shed from
//!    the rear corners roll it up and outwards (the curl you see behind a sliding car)
//!  * each wheel drags the air round with its tread: smoke at the tyre is spun up the back of the wheel and thrown forward
//!    over the arch, faster when the wheel is spinning
//! plus the stage wind (gusting) and a few sines of position and time as turbulence.
//! Rates follow tyre physics: more speed, more slip and more load throw more; far cars emit fewer, bigger puffs; past 160 m none.
//! Everything lives in flat arrays sent to the GPU as one point cloud; the shader draws soft, sun-lit, pixel-noise puffs.
use crate::physics::Car;
use crate::world::{World, ROAD};

pub const DUST: u8 = 0; pub const GRIT: u8 = 1; pub const SMOKE: u8 = 2; pub const POWDER: u8 = 3; pub const CHUNK: u8 = 4;
/// per kind: relaxation time to the air (s), gravity (m/s2), growth (m per sqrt s), base alpha, how much the wake moves it
const TAU: [f32; 5] = [0.32, 3.0, 0.45, 0.28, 2.2];
const GRAV: [f32; 5] = [-0.22, -9.8, 0.4, -0.35, -9.8];
const GROW: [f32; 5] = [1.8, 0.0, 2.3, 1.5, 0.0];
const WAKE: [f32; 5] = [1.0, 0.15, 0.9, 1.0, 0.2];

/// what kind of stage this is (sets the surfaces and colours)
#[derive(Clone, Copy, Default)]
pub struct Env { pub tarmac: bool, pub snow: bool, pub sand: bool, pub lush: bool, pub alpha: f32 }

/// a car in the air flow: where it is, how it moves, and its wheels (local x, local z, spin in rad/s)
#[derive(Clone, Copy)]
pub struct Body { pub x: f32, pub y: f32, pub z: f32, pub sh: f32, pub ch: f32, pub vx: f32, pub vz: f32, pub sp: f32, pub wheels: [(f32, f32, f32); 4] }
impl Body {
    pub fn of(c: &Car) -> Body {
        let w = |k: usize| (c.wh[k].lx as f32, c.wh[k].lz as f32, c.wh[k].w as f32);
        Body { x: c.x as f32, y: c.y as f32, z: c.z as f32, sh: c.h.sin() as f32, ch: c.h.cos() as f32, vx: c.vx as f32, vz: c.vz as f32, sp: c.vx.hypot(c.vz) as f32, wheels: [w(0), w(1), w(2), w(3)] }
    }
}
/// the body as a rounded box around (0, BY, 0) in the car's frame: half sizes and corner radius
const BX: f32 = 0.7; const BY: f32 = 0.82; const BHY: f32 = 0.38; const BZ: f32 = 1.8; const BR: f32 = 0.27;
/// distance to the body (negative inside) and the outward normal, car frame
fn body_sdf(x: f32, y: f32, z: f32) -> (f32, [f32; 3]) {
    let q = [x.abs() - BX, (y - BY).abs() - BHY, z.abs() - BZ];
    let o = [q[0].max(0.0), q[1].max(0.0), q[2].max(0.0)]; let ol = (o[0] * o[0] + o[1] * o[1] + o[2] * o[2]).sqrt();
    let inside = q[0].max(q[1]).max(q[2]).min(0.0); let d = ol + inside - BR;
    let sg = [x.signum(), (y - BY).signum(), z.signum()];
    let n = if ol > 1e-5 { [o[0] / ol * sg[0], o[1] / ol * sg[1], o[2] / ol * sg[2]] }
        else if q[0] >= q[1] && q[0] >= q[2] { [sg[0], 0.0, 0.0] } else if q[1] >= q[2] { [0.0, sg[1], 0.0] } else { [0.0, 0.0, sg[2]] };
    (d, n)
}
const WHEEL_Y: f32 = 0.24; const TYRE_R: f32 = 0.31;

pub struct Dust {
    pub n: usize, pub pos: Vec<f32>, pub col: Vec<f32>, pub a: Vec<f32>, pub s: Vec<f32>,
    vel: Vec<[f32; 3]>, age: Vec<f32>, life: Vec<f32>, s0: Vec<f32>, a0: Vec<f32>, kind: Vec<u8>, gy: Vec<f32>, seed: Vec<f32>,
    head: usize, pub live: usize, was: bool, rng: u64, frame: u32,
    /// back-to-front order and the arrays sent to the GPU in that order
    order: Vec<(f32, u32)>, out_pos: Vec<f32>, out_col: Vec<f32>, out_a: Vec<f32>, out_s: Vec<f32>,
    /// wind direction (unit, x z) and the time
    wdir: [f32; 2], t: f32,
}

impl Dust {
    pub fn new(n: usize, seed: u64) -> Dust {
        let mut d = Dust { n, pos: vec![0.0; n * 3], col: vec![0.0; n * 3], a: vec![0.0; n], s: vec![0.0; n], vel: vec![[0.0; 3]; n], age: vec![0.0; n], life: vec![0.0; n],
            s0: vec![0.0; n], a0: vec![0.0; n], kind: vec![0; n], gy: vec![0.0; n], seed: vec![0.0; n], head: 0, live: 0, was: true, rng: seed | 1, frame: 0, wdir: [1.0, 0.0], t: 0.0,
            order: Vec::with_capacity(n), out_pos: vec![0.0; n * 3], out_col: vec![0.0; n * 3], out_a: vec![0.0; n], out_s: vec![0.0; n] };
        let ang = d.f() * std::f32::consts::TAU; d.wdir = [ang.cos(), ang.sin()]; d
    }
    fn f(&mut self) -> f32 { self.rng ^= self.rng << 13; self.rng ^= self.rng >> 7; self.rng ^= self.rng << 17; (self.rng >> 40) as f32 / (1u64 << 24) as f32 }
    fn r(&mut self, a: f32, b: f32) -> f32 { a + (b - a) * self.f() }

    /// one particle. bright: colour multiplier; earth: 0 = the stage's dust colour .. 1 = dark soil
    #[allow(clippy::too_many_arguments)]
    pub fn emit(&mut self, kind: u8, p: [f32; 3], v: [f32; 3], size: f32, life: f32, alpha: f32, bright: f32, earth: f32, ground: f32) {
        if self.n == 0 { return; }
        let i = self.head; self.head = (self.head + 1) % self.n;
        self.pos[i * 3] = p[0]; self.pos[i * 3 + 1] = p[1]; self.pos[i * 3 + 2] = p[2]; self.vel[i] = v;
        self.age[i] = 0.0; self.life[i] = life; self.s0[i] = size; self.s[i] = size; self.a0[i] = alpha; self.a[i] = 0.0; self.kind[i] = kind; self.gy[i] = ground;
        let sd = self.f(); self.seed[i] = sd; self.col[i * 3] = bright; self.col[i * 3 + 1] = earth; self.col[i * 3 + 2] = sd.max(0.004);
    }

    /// What a car's four tyres throw this frame. `slips`: per-wheel slip speed (m/s) and load (N; 0 = in the air).
    #[allow(clippy::too_many_arguments)]
    pub fn wheels(&mut self, w: &World, env: &Env, c: &Car, slips: [(f32, f32); 4], throttle: f32, cam: [f64; 3], dt: f32) {
        let sp = (c.vx.hypot(c.vz)) as f32; let d_cam = ((c.x - cam[0]).hypot(c.z - cam[2])) as f32;
        if d_cam > 160.0 || sp < 0.8 && slips.iter().all(|s| s.0 < 1.0) { return; }
        // level of detail: far cars throw fewer, bigger puffs
        let lod = (1.35 - d_cam / 95.0).clamp(0.22, 1.0); let big = 1.0 + (d_cam / 70.0).min(1.6);
        let (sh, ch) = (c.h.sin() as f32, c.h.cos() as f32);
        for k in 0..4 {
            let (slip, load) = slips[k]; if load <= 0.0 { continue; }
            let wh = &c.wh[k]; let (lx, lz) = (wh.lx as f32, wh.lz as f32);
            let wx = c.x as f32 + ch * lx + sh * lz; let wz = c.z as f32 - sh * lx + ch * lz;
            let road = w.path_dist(wx as f64, wz as f64) < ROAD; let tarmac = env.tarmac && road;
            let gy = w.height(wx as f64, wz as f64) as f32; let lk = (load / 2900.0).sqrt().min(1.6); let front = if wh.f { 0.6 } else { 1.0 };
            // how loose the surface is and what it is made of
            let (loose, earth) = if env.snow { (if road { 1.15 } else { 1.4 }, 0.0) } else if tarmac { (0.0, 0.0) }
                else if road { (if env.sand { 1.35 } else { 1.0 }, 0.0) } else if env.sand { (1.2, 0.15) } else if env.lush { (0.4, 0.65) } else { (0.8, 0.4) };
            let back = [-sh, -ch];   // behind the car
            // fine dust: speed and slip lift it, load presses more out
            let rate = loose * (0.26 * sp + 1.7 * slip) * lk * front * lod;
            let n = self.count(rate.min(20.0) * dt);
            for _ in 0..n {
                let (fine, kind) = if env.snow { (POWDER, POWDER) } else { (DUST, DUST) }; let _ = fine;
                let up = self.r(0.5, 1.5) + slip * 0.05; let side = self.r(-0.7, 0.7);
                let v = [c.vx as f32 * 0.32 + back[0] * slip * 0.25 + ch * side, up, c.vz as f32 * 0.32 + back[1] * slip * 0.25 - sh * side];
                let p = [wx + back[0] * 0.3 + self.r(-0.15, 0.15), gy + 0.15, wz + back[1] * 0.3 + self.r(-0.15, 0.15)];
                let size = self.r(0.7, 1.15) * big * if env.snow { 0.9 } else { 1.0 }; let life = self.r(1.2, 2.4) * if env.sand { 1.3 } else { 1.0 };
                let bright = self.r(0.88, 1.08) * if env.snow { 1.05 } else { 1.0 };
                let al = env.alpha * self.r(0.8, 1.1); self.emit(kind, p, v, size, life, al, bright, earth, gy);
            }
            // grit / snow chunks: thrown by wheelspin and sliding, mostly from the driven rear
            if loose > 0.0 && d_cam < 70.0 {
                let spin = slip * (0.6 + throttle * 0.8);
                let n = self.count((loose.min(1.2) * 3.2 * (spin - 1.2).max(0.0) * lk * front * lod).min(40.0) * dt);
                for _ in 0..n {
                    let kick = (3.0 + spin * 0.45).min(9.0) * self.r(0.6, 1.1); let side = self.r(-1.2, 1.2);
                    let v = [c.vx as f32 * 0.6 + back[0] * kick + ch * side, self.r(1.8, 4.2), c.vz as f32 * 0.6 + back[1] * kick - sh * side];
                    let p = [wx + back[0] * 0.25, gy + 0.12, wz + back[1] * 0.25];
                    let (kind, bright) = if env.snow { (CHUNK, self.r(1.0, 1.15)) } else { (GRIT, self.r(0.45, 0.7)) };
                    let (sz, lf) = (self.r(0.09, 0.17), self.r(0.5, 0.9)); self.emit(kind, p, v, sz, lf, 0.95, bright, earth * 0.5 + 0.3 * (1.0 - env.snow as u8 as f32), gy);
                }
            }
            // tyre smoke on tarmac
            if tarmac && slip > 2.2 {
                let n = self.count(((slip - 2.2) * 5.5 * lk * lod).min(30.0) * dt);
                for _ in 0..n {
                    let v = [c.vx as f32 * 0.25 + self.r(-0.5, 0.5), self.r(0.3, 0.9), c.vz as f32 * 0.25 + self.r(-0.5, 0.5)];
                    let (sz, lf, br) = (self.r(0.5, 0.8) * big, self.r(1.6, 2.6), self.r(1.15, 1.3)); self.emit(SMOKE, [wx, gy + 0.2, wz], v, sz, lf, 0.36 * (0.5 + (slip - 2.2) / 8.0).min(1.0), br, 0.0, gy);
                }
            }
        }
    }
    /// stochastic rounding: an expected number of particles this frame -> a whole number
    fn count(&mut self, e: f32) -> usize { let base = e.floor(); base as usize + if self.f() < e - base { 1 } else { 0 } }

    /// landing from a jump: a ring of dust blown outwards
    pub fn landing(&mut self, w: &World, env: &Env, c: &Car, v: f32) {
        if env.tarmac && w.path_dist(c.x, c.z) < ROAD { return; }
        let n = ((v - 1.5) * 6.0).clamp(6.0, 34.0) as usize; let gy = w.height(c.x, c.z) as f32;
        for q in 0..n { let a = q as f32 / n as f32 * std::f32::consts::TAU + self.r(-0.2, 0.2); let sp = self.r(1.5, 2.5) + v * 0.35;
            let p = [c.x as f32 + a.cos() * 1.1, gy + 0.2, c.z as f32 + a.sin() * 1.9];
            let kind = if env.snow { POWDER } else { DUST };
            let (vy, sz, lf, br) = (self.r(0.4, 1.4), self.r(0.7, 1.1), self.r(1.5, 2.6), self.r(0.9, 1.05)); self.emit(kind, p, [a.cos() * sp + c.vx as f32 * 0.4, vy, a.sin() * sp + c.vz as f32 * 0.4], sz, lf, env.alpha * 1.1, br, 0.2, gy); }
    }

    /// move everything: air (wind, the cars' flow fields, turbulence), drag, gravity, ground, growth and thinning
    pub fn step(&mut self, w: &World, bodies: &[Body], gust: f32, dt: f32) {
        self.t += dt; self.frame = self.frame.wrapping_add(1); let t = self.t;
        let wind = [self.wdir[0] * (0.5 + gust * 2.4), self.wdir[1] * (0.5 + gust * 2.4)];
        let mut live = 0; let mut puffs: Vec<([f32; 3], f32, f32)> = Vec::new();
        for i in 0..self.n {
            if self.life[i] <= 0.0 { self.a[i] = 0.0; continue; }
            self.age[i] += dt; let k = self.age[i] / self.life[i];
            if k >= 1.0 { self.life[i] = 0.0; self.a[i] = 0.0; continue; }
            live += 1; let kd = self.kind[i] as usize; let (mut x, mut y, mut z) = (self.pos[i * 3], self.pos[i * 3 + 1], self.pos[i * 3 + 2]); let sd = self.seed[i];
            let heavy = kd == GRIT as usize || kd == CHUNK as usize;
            // the air here: wind and turbulence (grows as the puff ages; a few sines, different for every particle)
            let mut air = [wind[0], 0.0, wind[1]];
            // billowing: rolls that grow with the puff, with a gentle lift (warm, stirred air) so clouds tower rather than smear
            if !heavy { let ta = 0.35 + self.age[i] * 0.9; let ph = sd * 40.0;
                air[0] += (y * 1.3 + t * 1.7 + ph).sin() * ta; air[1] += ((x * 0.9 + z * 0.7 + t * 1.3 + ph).sin() * 0.8 + 0.35) * ta * 0.7; air[2] += (y * 1.1 - t * 1.9 + ph * 1.3).cos() * ta; }
            let mut tau = TAU[kd]; let v = &mut self.vel[i];
            // each nearby car's flow field, worked out in its own frame (x right, y up, z forward)
            for b in bodies {
                let (dx, dz) = (x - b.x, z - b.z); if dx * dx + dz * dz > 100.0 { continue; }
                let (lx, ly, lz) = (dx * b.ch - dz * b.sh, y - b.y, dx * b.sh + dz * b.ch);
                let tol = |vx: f32, vz: f32| (vx * b.ch - vz * b.sh, vx * b.sh + vz * b.ch); let tow = |lx: f32, lz: f32| (lx * b.ch + lz * b.sh, -lx * b.sh + lz * b.ch);
                let (cvx, cvz) = tol(b.vx, b.vz); let (ax, az) = tol(air[0], air[2]); let mut al = [ax, air[1], az]; let cv = [cvx, 0.0, cvz];
                let wk = WAKE[kd]; let spd = b.sp.min(40.0);
                // wake: dragged after the car, pulled in to the middle, a little upwash; two corner vortices roll it up and out
                let behind = -lz - BZ - 0.1;
                if behind > -0.5 && behind < 6.5 && spd > 1.0 {
                    let fall = (1.0 - behind.max(0.0) / 6.5).powf(1.8);
                    let wf = fall * (-lx * lx / 1.4).exp() * (-(ly - 0.6) * (ly - 0.6) / 1.2).exp() * wk;
                    al[0] += cv[0] * 0.42 * wf - lx * spd * 0.04 * wf; al[2] += cv[2] * 0.42 * wf; al[1] += spd * 0.05 * wf;
                    for s in [-1.0f32, 1.0] { let (rx, ry) = (lx - s * 0.85, ly - 0.9); let r2 = rx * rx + ry * ry + 0.18; let g = spd * 0.11 * fall * wk / r2;
                        al[0] += -s * ry * g; al[1] += s * rx * g; }
                }
                // wheels: air in contact with the tread turns with it
                if !heavy { for &(wx, wz, om) in &b.wheels {
                    let rx = (lx - wx).abs(); if rx > 0.42 { continue; } let (ry, rz) = (ly - WHEEL_Y, lz - wz); let r = (ry * ry + rz * rz).sqrt(); if r > 0.75 { continue; }
                    let f = (-((r - TYRE_R) / 0.2).powi(2)).exp() * (1.0 - rx / 0.42) * 0.85; let om = om.clamp(-160.0, 160.0);
                    let wv = [cv[0], cv[1] - om * rz, cv[2] + om * ry]; for q in 0..3 { al[q] += (wv[q] - al[q]) * f; } tau = tau.min(TAU[kd] * 0.4); } }
                // the body: no flow through it, dragged along next to it; a particle inside is pushed out
                let (d, nn) = body_sdf(lx, ly, lz);
                if d < 0.9 {
                    let kb = (1.0 - d.max(0.0) / 0.9).powi(2); let mut rel = [al[0] - cv[0], al[1] - cv[1], al[2] - cv[2]];
                    let rn = rel[0] * nn[0] + rel[1] * nn[1] + rel[2] * nn[2]; if rn < 0.0 { for q in 0..3 { rel[q] -= nn[q] * rn * kb; } }
                    for q in 0..3 { al[q] = cv[q] + rel[q] * (1.0 - 0.45 * kb); } if !heavy { tau = tau.min(TAU[kd] * (1.0 - 0.7 * kb)); }
                    if d < 0.0 { let (px, pz) = tow(lx + nn[0] * -d, lz + nn[2] * -d); x = b.x + px; z = b.z + pz; y = b.y + ly + nn[1] * -d;
                        let (pvx, pvz) = tol(v[0], v[2]); let mut pr = [pvx - cv[0], v[1], pvz - cv[2]]; let vn = pr[0] * nn[0] + pr[1] * nn[1] + pr[2] * nn[2];
                        if vn < 0.0 { for q in 0..3 { pr[q] -= nn[q] * vn; } } let (wvx, wvz) = tow(pr[0] + cv[0], pr[2] + cv[2]); v[0] = wvx; v[1] = pr[1]; v[2] = wvz; }
                }
                let (wx, wz) = tow(al[0], al[2]); air = [wx, al[1], wz];
            }
            // drag towards the air (exact for a constant air over the step), then gravity
            let rel = 1.0 - (-dt / tau.max(0.02)).exp();
            for q in 0..3 { v[q] += (air[q] - v[q]) * rel; } v[1] += GRAV[kd] * dt;
            let (nx, mut ny, nz) = (x + v[0] * dt, y + v[1] * dt, z + v[2] * dt);
            // the ground: stones stop (with a small puff), dust rolls along it; heights refreshed every 4th frame for dust
            if heavy || (i as u32 + self.frame) % 4 == 0 { self.gy[i] = w.height(nx as f64, nz as f64) as f32; }
            let floor = self.gy[i] + if heavy { 0.05 } else { self.s[i] * 0.22 };
            if ny < floor { if heavy { self.life[i] = 0.0; self.a[i] = 0.0; if sd < 0.3 { puffs.push(([nx, floor + 0.1, nz], self.col[i * 3 + 1], self.gy[i])); } continue; }
                ny = floor; if v[1] < 0.0 { v[1] = 0.0; } }
            self.pos[i * 3] = nx; self.pos[i * 3 + 1] = ny; self.pos[i * 3 + 2] = nz;
            // spreading and thinning (the same dust over a bigger puff, seen through its depth); a quick fade in so puffs never pop
            let s = self.s0[i] + GROW[kd] * self.age[i].sqrt(); self.s[i] = s;
            let thin = if GROW[kd] > 0.0 { (self.s0[i] / s).powf(0.5) } else { 1.0 };
            self.a[i] = self.a0[i] * (self.age[i] / 0.08).min(1.0) * (1.0 - k).powf(1.4) * thin;
        }
        for (p, earth, gy) in puffs { let snow = earth <= 0.0; self.emit(if snow { POWDER } else { DUST }, p, [0.0, 0.5, 0.0], 0.35, 0.9, 0.3, 0.95, earth, gy); }
        self.live = live;
    }
    /// send the cloud to the GPU, far to near (alpha blending then builds up a proper volume); skipped while nothing is alive
    pub fn upload(&mut self, r: &mut crate::render::Renderer, obj: usize, cam: [f64; 3], fwd: [f64; 3]) {
        if self.live > 0 || self.was {
            self.order.clear();
            for i in 0..self.n { if self.a[i] > 0.0 { let d = (self.pos[i * 3] - cam[0] as f32) * fwd[0] as f32 + (self.pos[i * 3 + 1] - cam[1] as f32) * fwd[1] as f32 + (self.pos[i * 3 + 2] - cam[2] as f32) * fwd[2] as f32; self.order.push((-d, i as u32)); } }
            self.order.sort_unstable_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            for (j, &(_, i)) in self.order.iter().enumerate() { let i = i as usize;
                self.out_pos[j * 3..j * 3 + 3].copy_from_slice(&self.pos[i * 3..i * 3 + 3]); self.out_col[j * 3..j * 3 + 3].copy_from_slice(&self.col[i * 3..i * 3 + 3]);
                self.out_a[j] = self.a[i]; self.out_s[j] = self.s[i]; }
            for j in self.order.len()..self.n { self.out_a[j] = 0.0; self.out_s[j] = 0.0; }
            r.set_attr(obj, "position", &self.out_pos); r.set_attr(obj, "aA", &self.out_a); r.set_attr(obj, "aS", &self.out_s); r.set_attr(obj, "color", &self.out_col);
        }
        self.was = self.live > 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grit_falls_dust_spreads() {
        let n = 64; let w = crate::world::World { map: crate::world::Map::Forest, gxh: 32, gzh: 32, gnx: n, gnz: n, pd: vec![0.0; n * n], pi: vec![0; n * n], h: vec![0.0; n * n] };
        let mut d = Dust::new(16, 7);
        d.emit(GRIT, [0.0, 0.5, 0.0], [5.0, 3.0, 0.0], 0.1, 2.0, 1.0, 0.5, 0.0, 0.0);
        d.emit(DUST, [0.0, 0.5, 0.0], [0.0, 1.0, 0.0], 0.5, 2.0, 0.4, 1.0, 0.0, 0.0);
        let mut alive_grit = 0.0;
        for _ in 0..120 { d.step(&w, &[], 0.0, 1.0 / 60.0); if d.life[0] > 0.0 { alive_grit += 1.0 / 60.0; } }
        assert!(alive_grit > 0.3 && alive_grit < 1.2, "stone flight {alive_grit}");   // up at 3 m/s from 0.5 m: lands in ~0.75 s
        assert!(d.s[1] > 1.5, "dust grew to {}", d.s[1]); assert!(d.a[1] < 0.4 * 0.5, "dust thinned to {}", d.a[1]);
        // a car passing drags dust behind it along
        let mut e = Dust::new(4, 3); e.emit(DUST, [0.0, 0.5, -3.0], [0.0; 3], 0.5, 3.0, 0.4, 1.0, 0.0, 0.0);
        let wk = Body { x: 0.0, y: 0.0, z: 0.0, sh: 0.0, ch: 1.0, vx: 0.0, vz: 25.0, sp: 25.0, wheels: [(0.75, 1.22, 80.0), (-0.75, 1.22, 80.0), (0.75, -1.28, 80.0), (-0.75, -1.28, 80.0)] };
        for _ in 0..30 { e.step(&w, &[wk], 0.0, 1.0 / 60.0); }
        assert!(e.vel[0][2] > 4.0, "wake drag {}", e.vel[0][2]);
        // smoke that drifts into a parked car's flank slides round it instead of going through
        let mut f = Dust::new(4, 5); f.emit(SMOKE, [-2.0, 0.8, 0.0], [6.0, 0.0, 0.0], 0.5, 3.0, 0.4, 1.0, 0.0, 0.0);
        let parked = Body { x: 0.0, y: 0.0, z: 0.0, sh: 0.0, ch: 1.0, vx: 0.0, vz: 0.0, sp: 0.0, wheels: [(0.0, 0.0, 0.0); 4] };
        for _ in 0..90 { f.step(&w, &[parked], 0.0, 1.0 / 60.0); let (d, _) = body_sdf(f.pos[0], f.pos[1], f.pos[2]); assert!(d > -0.05, "inside the car: {d}"); }
    }
}
