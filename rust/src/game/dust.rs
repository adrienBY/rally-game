//! Dust, gravel and tyre smoke: a small CPU particle system that is cheap but behaves like the real thing.
//!
//! Kinds (each with its own drag, gravity, growth and look):
//!  * dust: fine powder lifted at the contact patch, hanging for seconds (longer on dry, sandy ground); light, so it relaxes quickly to the surrounding air (wind, the wake of
//!    passing cars, turbulence), settles slowly, and spreads out: its radius grows like the square root of its age (diffusion)
//!    while it thins out in proportion (the same dust over a bigger volume), so a trail stays visible but never turns into a wall
//!  * grit: stones flung by the tread (many more with wheelspin); heavy, ballistic, gone (with a little puff) when they land
//!  * smoke: rubber vapour on tarmac. A tyre only smokes once its tread surface is hot enough: every wheel keeps a heat level
//!    that the friction power of sliding (slip speed x load) pumps in and the air takes away (faster at speed). An ordinary slide
//!    through a bend never gets there; a long drift, a burnout or a locked wheel does, and then it rises, grows fast, stays faint
//!  * spray / powder / chunks: snow. Loose grains thrown off the tread on arcs (crisp white specks that fall back within a
//!    second), the fine powder that breaks off them and hangs briefly as mist, and packed clumps on wheelspin
//! Stones, grains and clumps are drawn as crisp specks of whole pixels; dust, powder and smoke as the cloud.
//! What the tread throws leaves it along the tread at a share of its speed relative to the hub: with grip it still moves
//! forward with the car (into the arches); wheelspin sends it back past the car as a rooster tail.
//! The air around each car is modelled cheaply but physically (in the car's frame, within 10 m):
//!  * the body is a solid (a rounded box): air cannot go through it, so it slides along the surface and smoke wraps round the
//!    car; next to the skin it is dragged along with the car (boundary layer); a particle that ends up inside is pushed out
//!  * behind it the wake: low pressure drags air after the car and pulls it in towards the middle
//! Swirl that outlives the car: every car sheds vortex blobs (short pieces of spinning air) into the air behind it, from the
//! rear corners (the two trailing vortices), over the tail (the roll that tumbles dust up and over), and from the trailing
//! side when it slides (the mushroom). Blobs stay where they were dropped, drift with the wind, push each other, slide out
//! along the ground (their mirror image under it), slowly weaken and spread; every light particle near one is turned by it.
//! Found through a small hash grid, so each particle only looks at the few blobs around it.
//!  * each wheel drags the air round with its tread: smoke at the tyre is spun up the back of the wheel and thrown forward
//!    over the arch, faster when the wheel is spinning
//! plus the stage wind (gusting, stronger higher up) and turbulence from a curl-noise eddy field: divergence-free, shared by
//! every particle (so neighbouring dust swirls together like a fluid instead of jittering on its own), stronger for older,
//! more spread-out puffs, refreshed per particle every third frame. Dust that lands spreads out along the ground.
//! The renderer adds the puffs up into a density layer, blurs it and shades the result as one cloud, so draw order does not
//! matter and thousands of small puffs merge into a continuous volume.
//! Rates follow tyre physics: more speed, more slip and more load throw more; far cars emit fewer, bigger puffs; past 160 m none.
//! Everything lives in flat arrays sent to the GPU as one point cloud; the shader draws soft, sun-lit, pixel-noise puffs.
use crate::physics::Car;
use crate::world::{World, ROAD};

pub const DUST: u8 = 0; pub const GRIT: u8 = 1; pub const SMOKE: u8 = 2; pub const POWDER: u8 = 3; pub const CHUNK: u8 = 4; pub const SPRAY: u8 = 5;
/// drawn as crisp specks (their colour is the actual rgb), not as part of the dust cloud
#[inline] pub fn is_speck(k: u8) -> bool { k == GRIT || k == CHUNK || k == SPRAY }
/// per kind: relaxation time to the air (s), gravity (m/s2), growth (m per sqrt s), base alpha, how much the wake moves it
const TAU: [f32; 6] = [0.32, 3.0, 0.45, 0.3, 2.2, 0.35];
const GRAV: [f32; 6] = [-0.22, -9.8, 0.4, -0.6, -9.8, -9.8];
const GROW: [f32; 6] = [1.25, 0.0, 1.6, 0.6, 0.0, 0.0];
const WAKE: [f32; 6] = [1.0, 0.15, 0.9, 1.0, 0.2, 0.5];

/// a piece of spinning air left behind by a car: centre, its own drift, unit axis, circulation (m2/s), core radius, length
#[derive(Clone, Copy, Default)]
struct Vort { p: [f32; 3], v: [f32; 3], ax: [f32; 3], g: f32, rc0: f32, rc: f32, len: f32, age: f32, gy: f32 }
/// blobs kept, how long they last, how fast they weaken (s), how fast the core spreads (m2/s), reach (m), grid cell, buckets
const NV: usize = 256; const V_LIFE: f32 = 3.2; const V_DECAY: f32 = 1.6; const V_NU: f32 = 0.09; const V_REACH: f32 = 3.2;
const VCELL: f32 = 4.0; const VBUCK: usize = 512;
/// the air a blob turns at r (from its centre): Lamb-Oseen-like core, compact both across and along the axis
#[inline]
fn vort_u(b: &Vort, rx: f32, ry: f32, rz: f32) -> [f32; 3] {
    let al = rx * b.ax[0] + ry * b.ax[1] + rz * b.ax[2]; let q = al * al / (b.len * b.len); if q >= 4.0 { return [0.0; 3]; }
    let (px, py, pz) = (rx - al * b.ax[0], ry - al * b.ax[1], rz - al * b.ax[2]); let r2 = px * px + py * py + pz * pz;
    let rr = V_REACH * V_REACH; if r2 >= rr { return [0.0; 3]; }
    let fa = 1.0 - q * 0.25; let fr = 1.0 - r2 / rr;
    let k = b.g * 0.159_155 / (r2 + b.rc * b.rc) * fa * fa * fr * fr;
    [(b.ax[1] * pz - b.ax[2] * py) * k, (b.ax[2] * px - b.ax[0] * pz) * k, (b.ax[0] * py - b.ax[1] * px) * k]
}
#[inline]
fn vcell(x: f32, z: f32) -> (i32, i32) { ((x / VCELL).floor() as i32, (z / VCELL).floor() as i32) }
#[inline]
fn vbucket(c: (i32, i32)) -> usize { ((c.0 as u32).wrapping_mul(73_856_093) ^ (c.1 as u32).wrapping_mul(19_349_663)) as usize & (VBUCK - 1) }

/// what kind of stage this is (sets the surfaces and colours)
#[derive(Clone, Copy, Default)]
pub struct Env { pub tarmac: bool, pub snow: bool, pub sand: bool, pub lush: bool, pub alpha: f32,
    /// how long the dust hangs (1 = an ordinary dirt road; drier and finer lasts longer), and the colour of the stones
    pub dry: f32, pub grit: [f32; 3] }

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

/// Tread surface heat, 1 = rubber starts to vapourise. Heated by the friction power of sliding (slip speed x load, taking the
/// tarmac's friction as about 1), cooled towards the carcass (a time constant of about 1.3 s at rest, shorter as the air rushes past).
/// Steady state = power x time constant / TYRE_E: a 3 m/s slide on a loaded tyre (~9 kW) settles near 0.4 and never smokes;
/// a drift at ~25 kW crosses 1 after about 2 s, a burnout (~50 kW) after about 0.6 s, a locked wheel at speed in about 0.4 s.
pub const TYRE_E: f32 = 26_000.0;
pub fn tyre_heat(h: f32, slip: f32, load: f32, speed: f32, dt: f32) -> f32 {
    let power = slip.max(0.0) * load.max(0.0); let tau = 1.3 / (1.0 + speed / 60.0);
    let k = (-dt / tau).exp(); let target = power * tau / TYRE_E;
    target + (h - target) * k
}
/// puffs per second from one wheel this far over the smoke point and sliding this fast
pub fn smoke_rate(hot: f32, slip: f32) -> f32 { (hot * 34.0 * (slip / 6.0).min(1.5)).min(48.0) }

/// Curl of a vector potential made of two octaves of plane waves A_k = sin(a_k . p + w_k t): a divergence-free swirl field
/// (no sources or sinks, so dust is stirred, never bunched up or torn apart), about unit strength; the last octave is the
/// small, quick eddies (1-3 m) that make cloud edges billow. 9 cosines per call.
fn curl(x: f32, y: f32, z: f32, t: f32) -> [f32; 3] {
    let mut o = [0.0f32; 3];
    for (k, sc) in [(1.0f32, 1.0f32), (2.3, 0.45), (5.2, 0.42)] {
        let (a1, a2, a3) = ([0.31 * k, 0.83 * k, 0.47 * k], [0.71 * k, 0.29 * k, 0.61 * k], [0.53 * k, 0.67 * k, 0.37 * k]);
        let c1 = (a1[0] * x + a1[1] * y + a1[2] * z + 0.9 * t * k).cos(); let c2 = (a2[0] * x + a2[1] * y + a2[2] * z - 0.7 * t * k + 1.7).cos();
        let c3 = (a3[0] * x + a3[1] * y + a3[2] * z + 1.1 * t * k + 3.1).cos();
        let s = sc / k;   // speed set by sc alone
        o[0] += (a3[1] * c3 - a2[2] * c2) * s; o[1] += (a1[2] * c1 - a3[0] * c3) * s; o[2] += (a2[0] * c2 - a1[1] * c1) * s;
    }
    [o[0] * 1.6, o[1] * 1.6, o[2] * 1.6]
}

pub struct Dust {
    pub n: usize, pub pos: Vec<f32>, pub col: Vec<f32>, pub a: Vec<f32>, pub s: Vec<f32>,
    vel: Vec<[f32; 3]>, turb: Vec<[f32; 3]>, age: Vec<f32>, life: Vec<f32>, s0: Vec<f32>, a0: Vec<f32>, kind: Vec<u8>, gy: Vec<f32>, seed: Vec<f32>,
    head: usize, pub live: usize, was: bool, rng: u64, frame: u32,
    /// the live puffs, packed for the GPU
    out_pos: Vec<f32>, out_col: Vec<f32>, out_a: Vec<f32>, out_s: Vec<f32>,
    /// wind direction (unit, x z) and the time
    wdir: [f32; 2], t: f32,
    /// vortex blobs, the next slot, how many are alive; the grid (bucket starts, blob indices, each blob's cell)
    vort: Vec<Vort>, vhead: usize, pub vlive: usize, vstart: Vec<u32>, vitems: Vec<u16>, vcellof: Vec<(i32, i32)>,
    /// where the camera is (far cars shed no blobs)
    pub cam: [f32; 3],
    /// tread surface heat per car (0 = you, 1.. = race cars) and wheel; 1 = hot enough to smoke
    heat: Vec<[f32; 4]>,
}

impl Dust {
    pub fn new(n: usize, seed: u64) -> Dust {
        let mut d = Dust { n, pos: vec![0.0; n * 3], col: vec![0.0; n * 3], a: vec![0.0; n], s: vec![0.0; n], vel: vec![[0.0; 3]; n], turb: vec![[0.0; 3]; n], age: vec![0.0; n], life: vec![0.0; n],
            s0: vec![0.0; n], a0: vec![0.0; n], kind: vec![0; n], gy: vec![0.0; n], seed: vec![0.0; n], head: 0, live: 0, was: true, rng: seed | 1, frame: 0, wdir: [1.0, 0.0], t: 0.0,
            out_pos: vec![0.0; n * 3], out_col: vec![0.0; n * 3], out_a: vec![0.0; n], out_s: vec![0.0; n],
            vort: vec![Vort::default(); NV], vhead: 0, vlive: 0, vstart: vec![0; VBUCK + 1], vitems: vec![0; NV], vcellof: vec![(0, 0); NV], cam: [0.0; 3], heat: Vec::new() };
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

    /// one speck (a stone, a snow grain or clump): its colour is the rgb it is drawn with
    #[allow(clippy::too_many_arguments)]
    fn speck(&mut self, kind: u8, p: [f32; 3], v: [f32; 3], size: f32, life: f32, rgb: [f32; 3], ground: f32) {
        if self.n == 0 { return; } let i = self.head; self.emit(kind, p, v, size, life, 1.0, rgb[0], rgb[1], ground); self.col[i * 3 + 2] = rgb[2];
    }
    /// What a car's four tyres throw this frame. `id`: which car (0 = yours, keeps its tyre heat between frames);
    /// `slips`: per-wheel slip speed (m/s) and load (N; 0 = in the air).
    #[allow(clippy::too_many_arguments)]
    pub fn wheels(&mut self, id: usize, w: &World, env: &Env, c: &Car, slips: [(f32, f32); 4], throttle: f32, cam: [f64; 3], dt: f32) {
        let sp = (c.vx.hypot(c.vz)) as f32; let d_cam = ((c.x - cam[0]).hypot(c.z - cam[2])) as f32;
        if self.heat.len() <= id { self.heat.resize(id + 1, [0.0; 4]); }
        for k in 0..4 { let h = &mut self.heat[id][k]; *h = tyre_heat(*h, slips[k].0, slips[k].1, sp, dt); }
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
            // what the tread throws: material leaving the tyre at angle th behind the contact patch goes off along the tread, at a
            // share of the tread's speed relative to the hub (rolling speed plus wheelspin): up and back from the wheel, so with grip
            // it still moves forward with the car (into the arches), and only wheelspin sends it back past the car (a rooster tail)
            let vt = sp + slip * (0.4 + throttle * 0.6);
            let fwd = [sh, ch];
            let tread = |me: &mut Self, share: f32, th0: f32, th1: f32| -> [f32; 3] {
                let th = me.r(th0, th1); let k = vt * share * me.r(0.6, 1.1); let side = me.r(-1.0, 1.0) * (0.4 + k * 0.12);
                [c.vx as f32 - fwd[0] * k * th.cos() + ch * side, k * th.sin(), c.vz as f32 - fwd[1] * k * th.cos() - sh * side] };
            if env.snow {
                // snow: a spray of loose grains (crisp white specks on arcs that fall back within a second or so) and the fine
                // powder that breaks off them and hangs a little longer as a thin mist
                let rate = loose * (1.25 * sp + 7.0 * slip) * lk * front * lod;
                let n = self.count(rate.min(110.0) * dt);
                for _ in 0..n { let v = tread(self, 0.5, 0.25, 1.4); let tq = self.r(0.0, 1.0) * dt;
                    let p = [wx + back[0] * 0.3 - c.vx as f32 * tq, gy + 0.1, wz + back[1] * 0.3 - c.vz as f32 * tq];
                    let (b, sz, lf) = (self.r(0.92, 1.08), self.r(0.07, 0.13), self.r(1.2, 2.0)); self.speck(SPRAY, p, v, sz, lf, [b, b, b], gy); }
                let n = self.count((rate * 0.5).min(55.0) * dt);
                for _ in 0..n { let mut v = tread(self, 0.2, 0.3, 1.1); v[1] += self.r(0.2, 0.9); let tq = self.r(0.0, 1.0) * dt;
                    let p = [wx + back[0] * 0.5 + self.r(-0.2, 0.2) - c.vx as f32 * tq, gy + 0.25, wz + back[1] * 0.5 + self.r(-0.2, 0.2) - c.vz as f32 * tq];
                    let (al, sz, lf, br) = (env.alpha * self.r(0.7, 1.0), self.r(0.16, 0.3) * big, self.r(1.2, 2.4), self.r(1.0, 1.1)); self.emit(POWDER, p, v, sz, lf, al, br, 0.0, gy); }
            } else {
                // fine dust: lifted at the contact patch and by the wheel's own air, then handed to the wake and the wind; it hangs
                // for seconds (fine powder settles at a few cm/s), spreading and thinning, longer on dry ground
                let rate = loose * (0.62 * sp + 4.0 * slip) * lk * front * lod;
                let n = self.count(rate.min(60.0) * dt);
                for _ in 0..n {
                    let up = self.r(0.5, 1.3) + sp * 0.05 + slip * 0.08; let side = self.r(-0.7, 0.7);
                    let v = [c.vx as f32 * 0.32 + back[0] * slip * 0.25 + ch * side, up, c.vz as f32 * 0.32 + back[1] * slip * 0.25 - sh * side];
                    // spread along where the wheel went during this frame, so the stream is continuous, not dotted
                    let tq = self.r(0.0, 1.0) * dt;
                    let p = [wx + back[0] * 0.3 + self.r(-0.15, 0.15) - c.vx as f32 * tq, gy + 0.15, wz + back[1] * 0.3 + self.r(-0.15, 0.15) - c.vz as f32 * tq];
                    let size = self.r(0.28, 0.5) * big; let life = self.r(2.6, 4.6) * env.dry;
                    let (al, br) = (env.alpha * self.r(0.8, 1.1), self.r(0.88, 1.08)); self.emit(DUST, p, v, size, life, al, br, earth, gy);
                }
            }
            // stones / snow clumps: flung by the tread, many more with wheelspin, mostly from the driven rear; specks on arcs
            if loose > 0.0 && d_cam < 80.0 {
                let spin = slip * (0.6 + throttle * 0.8);
                let n = self.count((loose.min(1.2) * (0.06 * sp + 3.2 * (spin - 0.8).max(0.0)) * lk * front * lod).min(45.0) * dt);
                for _ in 0..n {
                    let v = tread(self, 0.32, 0.2, 0.8);
                    let p = [wx + back[0] * 0.25, gy + 0.12, wz + back[1] * 0.25];
                    if env.snow { let (b, sz, lf) = (self.r(0.95, 1.1), self.r(0.1, 0.2), self.r(0.6, 1.1)); self.speck(CHUNK, p, v, sz, lf, [b, b, b], gy); }
                    else { let g = env.grit; let (b, sz, lf) = (self.r(0.65, 1.15), self.r(0.05, 0.11), self.r(0.5, 0.9)); self.speck(GRIT, p, v, sz, lf, [g[0] * b, g[1] * b, g[2] * b], gy); }
                }
            }
            // tyre smoke on tarmac: only from a hot tread that is still sliding; a thin wisp at first, thicker the hotter it runs
            let hot = self.heat[id][k] - 1.0;
            if tarmac && hot > 0.0 && slip > 1.0 {
                let n = self.count(smoke_rate(hot, slip) * lk * lod * dt);
                let al = 0.26 * (0.25 + hot * 1.2).min(1.0);
                for _ in 0..n {
                    // vapour leaves the contact patch at the tread's speed relative to the road, then the air takes it
                    let v = [c.vx as f32 * 0.2 + self.r(-0.4, 0.4), self.r(0.2, 0.6), c.vz as f32 * 0.2 + self.r(-0.4, 0.4)];
                    let tq = self.r(0.0, 1.0) * dt; let p = [wx - c.vx as f32 * tq, gy + 0.12, wz - c.vz as f32 * tq];
                    let (sz, lf, br) = (self.r(0.22, 0.38) * big, self.r(1.2, 2.2), self.r(1.12, 1.26)); self.emit(SMOKE, p, v, sz, lf, al, br, 0.0, gy);
                }
            }
        }
    }
    /// live particles per kind (dust, grit, smoke, powder, chunk, spray), for tools
    pub fn kind_counts(&self) -> [usize; 6] { let mut c = [0; 6]; for i in 0..self.n { if self.life[i] > 0.0 { c[self.kind[i] as usize] += 1; } } c }
    /// one wheel's tread heat (`id`, wheel 0..3), for tests and tools
    pub fn tyre_heat_of(&self, id: usize, k: usize) -> f32 { self.heat.get(id).map_or(0.0, |h| h[k]) }
    /// stochastic rounding: an expected number of particles this frame -> a whole number
    fn count(&mut self, e: f32) -> usize { let base = e.floor(); base as usize + if self.f() < e - base { 1 } else { 0 } }

    /// landing from a jump: a ring of dust blown outwards
    pub fn landing(&mut self, w: &World, env: &Env, c: &Car, v: f32) {
        if env.tarmac && w.path_dist(c.x, c.z) < ROAD { return; }
        // the body slams down and the air under it rushes out: a low ring blown outwards, many small puffs (not a few big ones,
        // which would read as discs), and on snow a burst of grains
        let n = ((v - 1.5) * 12.0).clamp(12.0, 70.0) as usize; let gy = w.height(c.x, c.z) as f32;
        for q in 0..n { let a = q as f32 / n as f32 * std::f32::consts::TAU + self.r(-0.15, 0.15); let sp = self.r(1.5, 2.8) + v * 0.4;
            let p = [c.x as f32 + a.cos() * self.r(0.8, 1.2), gy + 0.15, c.z as f32 + a.sin() * self.r(1.6, 2.1)];
            let vel = [a.cos() * sp + c.vx as f32 * 0.4, self.r(0.3, 1.1), a.sin() * sp + c.vz as f32 * 0.4];
            if env.snow { let (sz, lf, br) = (self.r(0.2, 0.34), self.r(0.9, 1.7), self.r(0.95, 1.05)); self.emit(POWDER, p, vel, sz, lf, env.alpha, br, 0.0, gy);
                let (b, gs, gl) = (self.r(0.92, 1.08), self.r(0.045, 0.09), self.r(0.6, 1.1)); let gv = [vel[0] * 1.2, self.r(1.5, 3.5), vel[2] * 1.2]; self.speck(SPRAY, p, gv, gs, gl, [b, b, b], gy); }
            else { let (sz, lf, br) = (self.r(0.35, 0.6), self.r(2.0, 3.6) * env.dry, self.r(0.9, 1.05)); self.emit(DUST, p, vel, sz, lf, env.alpha, br, 0.2, gy); } }
    }

    /// move everything: air (wind, the cars' flow fields, turbulence), drag, gravity, ground, growth and thinning
    pub fn step(&mut self, w: &World, bodies: &[Body], gust: f32, dt: f32) {
        self.t += dt; self.frame = self.frame.wrapping_add(1); let t = self.t;
        let wind = [self.wdir[0] * (0.5 + gust * 2.4), self.wdir[1] * (0.5 + gust * 2.4)];
        for b in bodies { self.shed(b, dt); }
        self.step_vortices(w, wind, dt);
        let mut live = 0; let mut puffs: Vec<([f32; 3], bool, f32)> = Vec::new();
        for i in 0..self.n {
            if self.life[i] <= 0.0 { self.a[i] = 0.0; continue; }
            self.age[i] += dt; let k = self.age[i] / self.life[i];
            if k >= 1.0 { self.life[i] = 0.0; self.a[i] = 0.0; continue; }
            live += 1; let kd = self.kind[i] as usize; let (mut x, mut y, mut z) = (self.pos[i * 3], self.pos[i * 3 + 1], self.pos[i * 3 + 2]); let sd = self.seed[i];
            let heavy = kd == GRIT as usize || kd == CHUNK as usize; let ballistic = heavy || kd == SPRAY as usize;
            // the air here: wind and turbulence (grows as the puff ages; a few sines, different for every particle)
            let mut air = [wind[0], 0.0, wind[1]];
            // wind grows with height above the ground (shear); eddies from the curl-noise field, a gentle lift so clouds tower
            let hk = 0.55 + 0.45 * ((y - self.gy[i]) / 3.0).clamp(0.0, 1.0); air[0] *= hk; air[2] *= hk;
            if !heavy { if (i as u32 + self.frame) % 3 == 0 { self.turb[i] = curl(x, y, z, t); }
                let ta = 0.45 + self.age[i] * 1.1; let tb = self.turb[i];
                air[0] += tb[0] * ta; air[1] += (tb[1] * 0.75 + 0.3) * ta; air[2] += tb[2] * ta;
                // the spinning air the cars left behind
                if self.vlive > 0 { let u = self.vort_at(x, y, z, usize::MAX); let wk = WAKE[kd]; air[0] += u[0] * wk; air[1] += u[1] * wk; air[2] += u[2] * wk; } }
            let mut tau = TAU[kd]; let v = &mut self.vel[i];
            // each nearby car's flow field, worked out in its own frame (x right, y up, z forward)
            for b in bodies {
                let (dx, dz) = (x - b.x, z - b.z); if dx * dx + dz * dz > 100.0 { continue; }
                let (lx, ly, lz) = (dx * b.ch - dz * b.sh, y - b.y, dx * b.sh + dz * b.ch);
                let tol = |vx: f32, vz: f32| (vx * b.ch - vz * b.sh, vx * b.sh + vz * b.ch); let tow = |lx: f32, lz: f32| (lx * b.ch + lz * b.sh, -lx * b.sh + lz * b.ch);
                let (cvx, cvz) = tol(b.vx, b.vz); let (ax, az) = tol(air[0], air[2]); let mut al = [ax, air[1], az]; let cv = [cvx, 0.0, cvz];
                let wk = WAKE[kd]; let spd = b.sp.min(40.0);
                // wake: dragged after the car, pulled in to the middle, a little upwash (the vortices are shed blobs, below)
                let behind = -lz - BZ - 0.1;
                if behind > -0.5 && behind < 6.5 && spd > 1.0 {
                    let fall = (1.0 - behind.max(0.0) / 6.5).powf(1.8);
                    let wf = fall * (-lx * lx / 1.4).exp() * (-(ly - 0.6) * (ly - 0.6) / 1.2).exp() * wk;
                    al[0] += cv[0] * 0.42 * wf - lx * spd * 0.04 * wf; al[2] += cv[2] * 0.42 * wf; al[1] += spd * 0.05 * wf;
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
            if ballistic || (i as u32 + self.frame) % 4 == 0 { self.gy[i] = w.height(nx as f64, nz as f64) as f32; }
            let floor = self.gy[i] + if ballistic { 0.04 } else { self.s[i] * 0.22 };
            if ny < floor { if ballistic { self.life[i] = 0.0; self.a[i] = 0.0; if sd < if kd == SPRAY as usize { 0.06 } else { 0.3 } { puffs.push(([nx, floor + 0.1, nz], kd != GRIT as usize, self.gy[i])); } continue; }
                // dust hitting the ground spreads out along it instead of stopping dead
                ny = floor; if v[1] < 0.0 { let lost = -v[1]; let hs = (v[0] * v[0] + v[2] * v[2]).sqrt().max(0.3); v[0] += v[0] / hs * lost * 0.45; v[2] += v[2] / hs * lost * 0.45; v[1] = 0.0; } }
            self.pos[i * 3] = nx; self.pos[i * 3 + 1] = ny; self.pos[i * 3 + 2] = nz;
            // spreading and thinning (the same dust over a bigger puff, seen through its depth); a quick fade in so puffs never pop
            let s = self.s0[i] + GROW[kd] * self.age[i].sqrt(); self.s[i] = s;
            let thin = if GROW[kd] > 0.0 { (self.s0[i] / s).powf(0.5) } else { 1.0 };
            self.a[i] = self.a0[i] * (self.age[i] / 0.08).min(1.0) * (1.0 - k).powf(1.4) * thin;
        }
        for (p, snow, gy) in puffs { self.emit(if snow { POWDER } else { DUST }, p, [0.0, 0.5, 0.0], if snow { 0.22 } else { 0.3 }, if snow { 0.8 } else { 1.4 }, 0.28, 0.95, if snow { 0.0 } else { 0.3 }, gy); }
        self.live = live;
    }
    fn add_vort(&mut self, p: [f32; 3], v: [f32; 3], ax: [f32; 3], g: f32, rc0: f32, len: f32, gy: f32) {
        let i = self.vhead; self.vhead = (self.vhead + 1) % NV;
        self.vort[i] = Vort { p, v, ax, g, rc0, rc: rc0, len, age: 0.0, gy };
    }
    /// what a moving car leaves in the air this frame
    fn shed(&mut self, b: &Body, dt: f32) {
        let sp = b.sp.min(40.0); let (dx, dz) = (b.x - self.cam[0], b.z - self.cam[2]);
        if dx * dx + dz * dz > 150.0 * 150.0 { return; }
        let (cvx, _) = (b.vx * b.ch - b.vz * b.sh, b.vx * b.sh + b.vz * b.ch);   // sideways speed, car frame
        let fwd = [b.sh, 0.0, b.ch]; let right = [b.ch, 0.0, -b.sh];
        let at = |lx: f32, ly: f32, lz: f32, tq: f32| [b.x + lx * b.ch + lz * b.sh - b.vx * tq, b.y + ly, b.z - lx * b.sh + lz * b.ch - b.vz * tq];
        let gy = b.y;   // refreshed from the terrain within a few frames
        let carry = [b.vx * 0.35, 0.0, b.vz * 0.35];
        if sp > 4.0 {
            // trailing vortices off the rear corners (right one turns +forward), and the roll over the tail
            let rate = (sp * 0.45).min(11.0); let len = (sp / rate * 0.7).max(1.0);
            let n = self.count(rate * dt);
            for _ in 0..n { let tq = self.f() * dt; let g = 0.69 * sp * (1.0 + 0.04 * cvx.abs());
                for s in [-1.0f32, 1.0] { let p = at(s * 0.82, 0.85, -BZ - 0.15, tq); self.add_vort(p, carry, [fwd[0] * s, 0.0, fwd[2] * s], g, 0.32, len, gy); }
                let p = at(0.0, 1.15, -BZ - 0.5, tq); self.add_vort(p, carry, [-right[0], 0.0, -right[2]], 0.33 * sp, 0.4, len.max(1.3), gy); }
        }
        // sliding: air spills over the trailing side and rolls up (the mushroom cloud off a drifting car)
        let lat = cvx.abs();
        if lat > 2.5 {
            let n = self.count(((lat - 2.5) * 2.2).min(10.0) * dt); let sg = cvx.signum();
            for _ in 0..n { let tq = self.f() * dt; let lz = self.r(-1.6, 1.4); let p = at(-sg * 0.95, 1.0, lz, tq);
                self.add_vort(p, carry, [fwd[0] * sg, 0.0, fwd[2] * sg], 0.8 * lat.min(25.0), 0.35, 1.3, gy); }
        }
    }
    /// sort the live blobs into the grid
    fn build_vgrid(&mut self) {
        for c in self.vstart.iter_mut() { *c = 0; }
        let mut n = 0;
        for i in 0..NV { if self.vort[i].g <= 0.0 { continue; } n += 1; let c = vcell(self.vort[i].p[0], self.vort[i].p[2]); self.vcellof[i] = c; self.vstart[vbucket(c) + 1] += 1; }
        for k in 0..VBUCK { self.vstart[k + 1] += self.vstart[k]; }
        let mut fill = self.vstart.clone();
        for i in 0..NV { if self.vort[i].g <= 0.0 { continue; } let k = vbucket(self.vcellof[i]); self.vitems[fill[k] as usize] = i as u16; fill[k] += 1; }
        self.vlive = n;
    }
    /// the air all blobs near (x, y, z) turn, skipping blob `skip`
    fn vort_at(&self, x: f32, y: f32, z: f32, skip: usize) -> [f32; 3] {
        let mut u = [0.0f32; 3]; let c = vcell(x, z);
        for dz in -1..=1 { for dx in -1..=1 { let cc = (c.0 + dx, c.1 + dz); let k = vbucket(cc);
            for &j in &self.vitems[self.vstart[k] as usize..self.vstart[k + 1] as usize] { let j = j as usize;
                if j == skip || self.vcellof[j] != cc { continue; } let b = &self.vort[j];
                let d = vort_u(b, x - b.p[0], y - b.p[1], z - b.p[2]); u[0] += d[0]; u[1] += d[1]; u[2] += d[2]; } } }
        u
    }
    /// blobs move with the wind and with each other, slide out along the ground, weaken and spread
    fn step_vortices(&mut self, w: &World, wind: [f32; 2], dt: f32) {
        self.build_vgrid(); if self.vlive == 0 { return; }
        let mut vel = [[0.0f32; 3]; NV];
        for i in 0..NV { let b = self.vort[i]; if b.g <= 0.0 { continue; }
            let mut u = self.vort_at(b.p[0], b.p[1], b.p[2], i);
            // its mirror image under the ground (horizontal spin flips): a pair near the ground slides apart along it
            let h = b.p[1] - b.gy; if h < 3.0 { let im = Vort { ax: [-b.ax[0], b.ax[1], -b.ax[2]], p: [b.p[0], b.gy - h, b.p[2]], ..b };
                let d = vort_u(&im, 0.0, 2.0 * h, 0.0); u[0] += d[0]; u[1] += d[1]; u[2] += d[2]; }
            let hk = 0.55 + 0.45 * (h / 3.0).clamp(0.0, 1.0);
            vel[i] = [u[0] * 0.6 + wind[0] * hk, u[1] * 0.6, u[2] * 0.6 + wind[1] * hk]; }
        let rel = 1.0 - (-dt / 0.5f32).exp(); let decay = (-dt / V_DECAY).exp();
        for i in 0..NV { let b = &mut self.vort[i]; if b.g <= 0.0 { continue; }
            b.age += dt; if b.age > V_LIFE { b.g = 0.0; continue; }
            for q in 0..3 { b.v[q] += (vel[i][q] - b.v[q]) * rel; b.p[q] += b.v[q] * dt; }
            if (i as u32 + self.frame) % 4 == 0 { b.gy = w.height(b.p[0] as f64, b.p[2] as f64) as f32; }
            let floor = b.gy + b.rc * 0.8; if b.p[1] < floor { b.p[1] = floor; b.v[1] = b.v[1].max(0.0); }
            b.g *= decay; b.rc = (b.rc0 * b.rc0 + 4.0 * V_NU * b.age).sqrt(); }
        self.build_vgrid();
    }
    /// send the live puffs to the GPU (the dust layer adds them up, so their order does not matter); skipped while nothing is alive
    pub fn upload(&mut self, r: &mut crate::render::Renderer, obj: usize) {
        if self.live > 0 || self.was {
            let (mut j, mut sp, mut sc) = (0, Vec::with_capacity(512), Vec::with_capacity(512));
            for i in 0..self.n { if self.a[i] <= 0.0 { continue; }
                if is_speck(self.kind[i]) { sp.extend_from_slice(&[self.pos[i * 3], self.pos[i * 3 + 1], self.pos[i * 3 + 2], self.s[i]]); sc.extend_from_slice(&[self.col[i * 3], self.col[i * 3 + 1], self.col[i * 3 + 2], self.a[i]]); continue; }
                self.out_pos[j * 3..j * 3 + 3].copy_from_slice(&self.pos[i * 3..i * 3 + 3]); self.out_col[j * 3..j * 3 + 3].copy_from_slice(&self.col[i * 3..i * 3 + 3]);
                self.out_a[j] = self.a[i]; self.out_s[j] = self.s[i]; j += 1; }
            for k in j..self.n { self.out_a[k] = 0.0; self.out_s[k] = 0.0; }
            r.set_attr(obj, "position", &self.out_pos); r.set_attr(obj, "aA", &self.out_a); r.set_attr(obj, "aS", &self.out_s); r.set_attr(obj, "color", &self.out_col);
            r.set_specks(&sp, &sc);
        }
        self.was = self.live > 0;
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn eddies_are_divergence_free() {
        let e = 1e-3; for &(x, y, z) in &[(0.3f32, 1.2f32, -4.0f32), (12.0, 0.5, 7.7), (-30.0, 3.0, 2.2)] {
            let dv = (curl(x + e, y, z, 1.0)[0] - curl(x - e, y, z, 1.0)[0] + curl(x, y + e, z, 1.0)[1] - curl(x, y - e, z, 1.0)[1] + curl(x, y, z + e, 1.0)[2] - curl(x, y, z - e, 1.0)[2]) / (2.0 * e);
            assert!(dv.abs() < 2e-2, "divergence {dv}"); }
    }
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
    #[test]
    fn a_passing_car_leaves_swirl_behind() {
        let n = 64; let w = crate::world::World { map: crate::world::Map::Forest, gxh: 32, gzh: 32, gnx: n, gnz: n, pd: vec![0.0; n * n], pi: vec![0; n * n], h: vec![0.0; n * n] };
        // a car drives past at 25 m/s for 0.6 s and is gone; a second later dust put next to its path is still being turned
        let run = |car: bool| { let mut d = Dust::new(8, 11);
            for k in 0..36 { let z = -10.0 + 25.0 * k as f32 / 60.0;
                let b = Body { x: 0.0, y: 0.0, z, sh: 0.0, ch: 1.0, vx: 0.0, vz: 25.0, sp: 25.0, wheels: [(0.0, 0.0, 0.0); 4] };
                let bs = [b]; d.step(&w, if car { &bs[..] } else { &[] }, 0.0, 1.0 / 60.0); }
            for _ in 0..60 { d.step(&w, &[], 0.0, 1.0 / 60.0); }
            let live = d.vlive; d.emit(DUST, [0.4, 0.9, -2.0], [0.0; 3], 0.4, 3.0, 0.4, 1.0, 0.0, 0.0);
            for _ in 0..20 { d.step(&w, &[], 0.0, 1.0 / 60.0); }
            let v = d.vel[0]; (live, (v[0] * v[0] + v[1] * v[1]).sqrt()) };
        let (live, with) = run(true); let (_, without) = run(false);
        assert!(live > 10, "blobs alive {live}"); assert!(with > without + 0.4, "swirl {with} vs calm {without}");
    }
    #[test]
    fn tyres_smoke_only_when_hot() {
        // time (s) for a wheel held at this slip, load and car speed to reach the smoke point (None: never within 10 s)
        let t_smoke = |slip: f32, load: f32, speed: f32| { let (mut h, mut t) = (0.0f32, 0.0f32);
            while t < 10.0 { h = tyre_heat(h, slip, load, speed, 1.0 / 60.0); t += 1.0 / 60.0; if h >= 1.0 { return Some(t); } } None };
        assert_eq!(t_smoke(3.0, 3000.0, 20.0), None, "an ordinary slide through a bend");
        assert_eq!(t_smoke(5.0, 3000.0, 25.0), None, "a committed slide at speed");
        let drift = t_smoke(8.0, 3200.0, 12.0).expect("a long drift smokes"); assert!(drift > 1.0 && drift < 3.5, "drift {drift}");
        let burn = t_smoke(15.0, 3600.0, 0.5).expect("a burnout smokes"); assert!(burn < 0.9, "burnout {burn}");
        let lock = t_smoke(25.0, 4200.0, 25.0).expect("a locked wheel smokes"); assert!(lock < 0.6, "lock-up {lock}");
        // and cools back down once it grips again
        let mut h = 1.5; for _ in 0..120 { h = tyre_heat(h, 0.3, 3000.0, 20.0, 1.0 / 60.0); } assert!(h < 0.3, "cooled to {h}");
    }
}