//! Stage builder: everything the hidden WebView page used to do before handing a stage to the engine - the road, the terrain mesh, the
//! pixel textures, every tree, rock, sign and set piece, the cars and their liveries, the rivals' instanced copies, the sound emitters -
//! now native. `build(map)` returns the same two packets the page posted (scene "FWS1", runtime "FRT1") plus the world grids and
//! obstacles, so the renderer and the game take them exactly as before. On a phone it runs on a background thread while the menu
//! keeps drawing; textures that never change between stages (foliage, cars and liveries) are painted once per run.
pub mod three;
pub mod canvas;
pub mod shape;
pub mod road;
mod tex;
mod env;
mod flora;
mod furn;
mod sets;
mod cars;
mod out;
pub mod sound;

use self::road::{Road, M};
use self::three::*;
use crate::noise::{clamp, js_round, smooth};
use crate::physics::js_hypot;
use std::collections::HashMap;

pub struct Built { pub scene: Vec<u8>, pub rt: Vec<u8>, pub audio: Vec<Vec<u8>>, pub world: crate::world::World, pub obstacles: Vec<crate::physics::Obstacle>, pub ms: f64, pub map: String }

#[derive(Clone, Copy, Default)]
pub struct Cv { pub x: f64, pub y: f64, pub z: f64, pub r: f64, pub l: Option<f64>, pub d: f64, pub tr: f64 }
#[derive(Clone, Copy)]
pub struct Tree { pub x: f64, pub z: f64, pub y: f64, pub cv: Option<Cv> }
#[derive(Clone, Copy)]
pub struct Lan { pub x: f64, pub z: f64, pub y: f64, pub i: usize }
pub struct CullE { pub o: ObjId, pub c: V3, pub r: f64, pub d: f64 }
pub struct Boat { pub x: f64, pub z: f64, pub h: f64, pub v: f64, pub sail: bool, pub ph: f64, pub sc: f64 }
pub struct VolcRiver { pub pts: Vec<f64>, pub hw: Vec<f64>, pub kc: usize, pub sg: f64 }
pub struct Wheel { pub pivot: ObjId, pub spin: ObjId, pub front: bool }
pub struct RgPart { pub im: ObjId, pub src: Vec<ObjId>, pub model: i64, pub tint: bool, pub liv: usize, pub paint: bool }

/// the stage under construction (the page's boot() closure, as fields)
pub struct St {
    pub r: Road, pub m: M, pub sc: Scene,
    pub snow: bool, pub can: bool, pub lak: bool, pub tou: bool, pub riv: bool, pub pri: bool, pub vol: bool, pub forest: bool, pub warm: bool, pub gc: bool,
    seed: i64,
    pub fog: Col, pub fog_d: f64, pub sun: V3, pub lights: Lights, pub far_cam: f64,
    pub cull: Vec<CullE>, pub fogcut: f64, pub obstacles: Vec<[f64; 4]>,
    pub trees: Vec<Tree>, pub th: HashMap<i64, Vec<usize>>, pub sh: Vec<f32>, pub ss: Vec<f32>, pub shadow_tint: [f64; 3],
    pub drop: Vec<i8>, pub lan: Vec<Lan>, pub lg: HashMap<i64, Vec<usize>>,
    pub baked_ground: Vec<GeoId>, pub baked_road: Option<GeoId>,
    pub t_leaf: TexId, pub t_needle: TexId, pub t_grass: TexId, pub t_fern: TexId, pub t_flower: TexId, pub t_bark: TexId, pub t_ground: TexId, pub t_gravel: TexId,
    pub water_l: Vec<ObjId>, pub glit: Vec<ObjId>, pub clouds: Vec<ObjId>, pub sunb: Vec<ObjId>, pub sky: ObjId, pub peaks: ObjId, pub star_g: ObjId, pub aur_g: ObjId,
    pub snowfall: ObjId, pub dust: ObjId, pub tilefall: ObjId, pub sfn: usize, pub sf_pos: Vec<f32>, pub sf_seed: Vec<f32>, pub dn: usize,
    pub unit_cyl: GeoId, pub card: GeoId, pub clump2: GeoId, pub clump3: GeoId,
    pub volc_rivers: Vec<VolcRiver>, pub volc_ims: Option<[ObjId; 5]>, pub volc_crater: [f64; 3],
    pub boats: Vec<Boat>, pub boat_ims: Option<[ObjId; 4]>, pub town: Option<V3>,
    pub car_root: ObjId, pub car_models: Vec<ObjId>, pub wheels: Vec<Wheel>, pub car_shadow: ObjId, pub car_w: ObjId,
    pub rg_parts: Vec<RgPart>, pub rg_shadow: ObjId,
    pub split: [f64; 6], pub tune: Vec<(&'static str, f64)>,
}

/// the page's seeded generator (Park-Miller)
pub struct Rng(pub i64);
impl Rng {
    pub fn rnd(&mut self) -> f64 { self.0 = (self.0 * 16807) % 2147483647; (self.0 - 1) as f64 / 2147483646.0 }
    pub fn rr(&mut self, a: f64, b: f64) -> f64 { a + (b - a) * self.rnd() }
}

impl St {
    // ---- the seeded generator (seed 1337 every stage)
    pub fn rnd(&mut self) -> f64 { self.seed = (self.seed * 16807) % 2147483647; (self.seed - 1) as f64 / 2147483646.0 }
    pub fn rr(&mut self, a: f64, b: f64) -> f64 { a + (b - a) * self.rnd() }
    // ---- the world
    pub fn height(&self, x: f64, z: f64) -> f64 { self.r.height(x, z) }
    pub fn path_dist(&self, x: f64, z: f64) -> f64 { self.r.path_dist(x, z) }
    pub fn path_idx(&self, x: f64, z: f64) -> usize { self.r.path_idx(x, z) }
    pub fn road_dist(&self, x: f64, z: f64) -> f64 { self.r.road_dist(x, z) }
    pub fn np(&self) -> usize { self.r.np }
    pub fn road(&self) -> f64 { self.r.road }
    pub fn p(&self, i: usize) -> [f64; 2] { self.r.path[i] }
    pub fn t(&self, i: usize) -> [f64; 2] { self.r.tan[i] }
    /// the point `off` metres right of the centre line at road index `i` (the page's p[0] - tn[1] * off, p[1] + tn[0] * off)
    pub fn side(&self, i: usize, off: f64) -> (f64, f64) { let p = self.r.path[i]; let t = self.r.tan[i]; (p[0] - t[1] * off, p[1] + t[0] * off) }
    pub fn bil(&self, g: &[f32], x: f64, z: f64) -> f64 {
        let w = &self.r.world; let fx = clamp(x + w.gxh as f64, 0.0, w.gnx as f64 - 1.001); let fz = clamp(z + w.gzh as f64, 0.0, w.gnz as f64 - 1.001);
        let i = fx as usize; let j = fz as usize; let u = fx - i as f64; let v = fz - j as f64; let k = j * w.gnx + i;
        (g[k] as f64 * (1.0 - u) + g[k + 1] as f64 * u) * (1.0 - v) + (g[k + w.gnx] as f64 * (1.0 - u) + g[k + w.gnx + 1] as f64 * u) * v
    }
    pub fn shade_at(&self, x: f64, z: f64) -> f64 { self.bil(&self.sh, x, z).min(1.0) }
    pub fn sun_shadow_at(&self, x: f64, z: f64) -> f64 { self.bil(&self.ss, x, z).min(0.85) }
    pub fn shadow_mul(&self, occ: f64, c: usize) -> f64 { 1.0 - occ * (1.0 - self.shadow_tint[c]) }
    pub fn lamp_at(&self, x: f64, z: f64) -> f64 {
        if self.lan.is_empty() { return 0.0; } let mut a = 0.0; let (ci, cj) = ((x / 16.0).floor() as i64, (z / 16.0).floor() as i64);
        for i in -1..=1 { for j in -1..=1 { if let Some(l) = self.lg.get(&((ci + i) * 4096 + cj + j)) { for &q in l { let q = self.lan[q]; let d2 = (x - q.x).powi(2) + (z - q.z).powi(2); if d2 < 100.0 { let k = 1.0 - d2.sqrt() / 10.0; a += k * k; } } } } }
        (1.3f64).min(a)
    }
    // ---- the scene
    pub fn add(&mut self, o: ObjId) { self.sc.add(o); }
    pub fn mesh(&mut self, g: Geo, m: MatId) -> ObjId { let o = self.sc.mesh_new(g, m); o }
    pub fn mat(&mut self, m: Mat) -> MatId { self.sc.mat(m) }
    pub fn geo(&mut self, g: Geo) -> GeoId { self.sc.geo(g) }
    pub fn tex(&mut self, t: Tex) -> TexId { self.sc.tex(t) }
    /// canvasTex(w, draw, repeat, h)
    pub fn canvas_tex(&mut self, w: usize, h: usize, repeat: bool, draw: impl FnOnce(&mut St, &mut canvas::Ctx)) -> TexId {
        let mut g = canvas::Ctx::new(w, h); draw(self, &mut g); let t = Tex::canvas(w, h, g.data(), repeat); self.tex(t)
    }
    /// instanced props split into ~CH m chunks with bounding spheres, culled past max_d; returns the first chunk
    pub fn instanced(&mut self, geo: GeoId, mat: MatId, mats: &[M4], cols: &[Col], max_d: f64, ch: f64) -> Option<ObjId> {
        if mats.is_empty() { return None; }
        let mut keys: Vec<i64> = Vec::new(); let mut groups: HashMap<i64, Vec<usize>> = HashMap::new();
        for (i, m) in mats.iter().enumerate() { let p = m.pos(); let k = (p.x / ch).floor() as i64 * 4096 + (p.z / ch).floor() as i64; groups.entry(k).or_insert_with(|| { keys.push(k); Vec::new() }).push(i); }
        let mut first = None;
        for k in keys { let ids = &groups[&k]; let im = self.sc.inst(geo, mat, ids.len());
            for (j, &id) in ids.iter().enumerate() { self.sc.set_matrix_at(im, j, &mats[id]); self.sc.set_color_at(im, j, cols[id]); }
            let bs = self.sc.inst_bs(im); self.add(im); self.cull.push(CullE { o: im, c: v3(bs[0], bs[1], bs[2]), r: bs[3], d: max_d }); if first.is_none() { first = Some(im); } }
        first
    }
    pub fn instanced_d(&mut self, geo: GeoId, mat: MatId, mats: &[M4], cols: &[Col]) -> Option<ObjId> { let d = self.fogcut; self.instanced(geo, mat, mats, cols, d, 160.0) }
    pub fn instanced_dd(&mut self, geo: GeoId, mat: MatId, mats: &[M4], cols: &[Col], d: f64) -> Option<ObjId> { self.instanced(geo, mat, mats, cols, d, 160.0) }
}
/// pushT: the composed matrix and the colour
pub fn push_t(list: &mut Vec<M4>, cols: &mut Vec<Col>, o: &Tmp, c: Col) { list.push(o.matrix()); cols.push(c); }
pub fn whites(n: usize) -> Vec<Col> { vec![WHITE; n] }
pub fn hyp(a: f64, b: f64) -> f64 { js_hypot(a, b) }
pub fn hyp3(a: f64, b: f64, c: f64) -> f64 { (a * a + b * b + c * c).sqrt() }
pub fn sm(e0: f64, e1: f64, x: f64) -> f64 { smooth(e0, e1, x) }
pub fn jr(v: f64) -> f64 { js_round(v) }

/// Build `map` from scratch. Deterministic: the same stage every time, on every device.
pub fn build(name: &str, saves: &[(String, String)]) -> Built {
    let t0 = web_time::Instant::now();
    let m = M::from(name);
    let r = road::build(m);
    let (snow, can, lak, tou, riv, pri, vol) = (m == M::Snow, m == M::Canyon, m == M::Lakes, m == M::Touge, m == M::Riviera, m == M::Prism, m == M::Ember);
    let fog = if vol { rgb(0.17, 0.065, 0.045) } else if pri { rgb(0.06, 0.03, 0.13) } else if riv { rgb(0.84, 0.58, 0.52) } else if tou { rgb(0.36, 0.33, 0.5) } else if snow { rgb(0.79, 0.84, 0.9) } else if can { rgb(0.88, 0.66, 0.5) } else if lak { rgb(0.84, 0.74, 0.76) } else { rgb(0.60, 0.65, 0.64) };
    let fog_d = if vol { 0.0046 } else if pri { 0.0011 } else if riv { 0.0019 } else if tou { 0.0026 } else if snow { 0.0034 } else if can { 0.0024 } else if lak { 0.0029 } else { 0.0056 };
    let sun = (if vol { v3(0.52, 0.2, -0.83) } else if pri { v3(0.55, 0.42, -0.72) } else if riv { v3(-0.86, 0.075, -0.33) } else if tou { v3(0.74, 0.07, 0.66) } else if can { v3(-0.62, 0.2, -0.76) } else if lak { v3(0.7, 0.13, -0.7) } else if snow { v3(-0.55, 0.24, -0.8) } else { v3(0.6, 0.48, -0.62) }).norm();
    let (hs, hg, hi) = if vol { (0xa86450, 0x240a07, 1.55) } else if pri { (0xa8a0ff, 0x402060, 2.3) } else if riv { (0xb4b0dc, 0x80604a, 2.1) } else if tou { (0x8c92cc, 0x2a2232, 1.75) } else if can { (0xf2c9a0, 0x6a3a26, 2.0) } else if lak { (0xd8c6d6, 0x34452f, 2.1) } else if snow { (0xdbe6f5, 0x6f7f96, 2.0) } else { (0xc8d2cf, 0x2b3420, 2.3) };
    let (dc, di) = if vol { (0xff6a38, 1.15) } else if pri { (0xf0e8ff, 1.9) } else if riv { (0xffa868, 2.3) } else if tou { (0xff8a58, 1.35) } else if can { (0xffb26b, 2.6) } else if lak { (0xffc09a, 2.4) } else if snow { (0xffd2a6, 2.2) } else { (0xffefc8, 1.9) };
    let lights = Lights { hemi: (Col::hex(hs), Col::hex(hg), hi, v3(0.0, 1.0, 0.0)), dir: (Col::hex(dc), di, sun) };
    let gn = r.world.gnx * r.world.gnz; let np = r.np;
    let shadow_tint = if vol { [0.62, 0.46, 0.44] } else if riv { [0.52, 0.54, 0.72] } else if tou { [0.66, 0.66, 0.78] } else if snow { [0.48, 0.56, 0.8] } else if can { [0.42, 0.42, 0.58] } else if lak { [0.46, 0.5, 0.66] } else { [0.5, 0.55, 0.62] };
    let mut st = St { m, sc: Scene::new(), snow, can, lak, tou, riv, pri, vol, forest: m == M::Forest, warm: can || lak, gc: can || lak || tou || riv || vol,
        seed: 1337, fog, fog_d, sun, lights, far_cam: 1900.0, cull: Vec::new(), fogcut: 1.8 / fog_d, obstacles: Vec::new(), trees: Vec::new(), th: HashMap::new(),
        sh: vec![0.0; gn], ss: vec![0.0; gn], shadow_tint, drop: vec![0; np], lan: Vec::new(), lg: HashMap::new(), baked_ground: Vec::new(), baked_road: None,
        t_leaf: 0, t_needle: 0, t_grass: 0, t_fern: 0, t_flower: 0, t_bark: 0, t_ground: 0, t_gravel: 0, water_l: Vec::new(), glit: Vec::new(), clouds: Vec::new(), sunb: Vec::new(),
        sky: 0, peaks: 0, star_g: 0, aur_g: 0, snowfall: 0, dust: 0, tilefall: 0, sfn: 0, sf_pos: Vec::new(), sf_seed: Vec::new(), dn: 6000, unit_cyl: 0, card: 0, clump2: 0, clump3: 0,
        volc_rivers: Vec::new(), volc_ims: None, volc_crater: [0.0; 3], boats: Vec::new(), boat_ims: None, town: None,
        car_root: 0, car_models: Vec::new(), wheels: Vec::new(), car_shadow: 0, car_w: 0, rg_parts: Vec::new(), rg_shadow: 0, split: [0.0; 6], tune: Vec::new(), r };
    // the page's order: textures, trees, the ground and road, the land beyond, water, sky, peaks, foliage, ground cover, the ramp,
    // the hazards, the furniture, grass, the cars, snowfall and dust, stars, the set pieces; then merged statics and the handoff
    tex::base(&mut st);
    flora::place_trees(&mut st);
    env::drops_and_lanterns(&mut st);
    env::ground_and_road(&mut st);
    env::far_land(&mut st);
    env::water(&mut st);
    env::sky(&mut st);
    env::riviera_sky(&mut st);
    env::peaks(&mut st);
    flora::trees(&mut st);
    flora::far_forest(&mut st);
    flora::cover(&mut st);
    furn::ramp(&mut st);
    if vol { furn::volcano(&mut st); }
    furn::tilefall(&mut st);
    if pri { furn::prism(&mut st); } else { furn::furniture(&mut st); }
    furn::grass(&mut st);
    cars::cars(&mut st);
    sets::snow_and_dust(&mut st);
    sets::stars(&mut st);
    if tou { sets::touge(&mut st); }
    if riv { sets::riviera(&mut st); }
    out::merge_static(&mut st);
    if riv { st.split = [-0.03, 0.012, 0.06, 0.05, 0.02, -0.035]; }
    out::tune(&mut st);
    cars::rivals(&mut st);
    let audio = out::audio(&st);
    let fogc = st.fog;
    let ex = export(&st.sc, m.name(), (fogc, st.fog_d), fogc, &st.lights, st.car_root);
    let rt = out::rt_packet(&st, &ex.ids, saves);
    let obstacles = st.obstacles.iter().map(|o| crate::physics::Obstacle { x: o[0], z: o[1], r: o[2], soft: o[3] == 3.0 }).collect();
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    Built { scene: ex.bytes, rt, audio, world: st.r.world, obstacles, ms, map: m.name().to_string() }
}

// ---------------------------------------------------------------- running it for the game
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
static GEN: AtomicU64 = AtomicU64::new(0);
static CUR: Mutex<String> = Mutex::new(String::new());
#[cfg(target_arch = "wasm32")]
static PENDING: Mutex<Option<(String, bool, u64)>> = Mutex::new(None);

pub(crate) fn audio_out(b: &[u8]) {
    #[cfg(not(target_arch = "wasm32"))] fogwood_audio::native::message(b);
    #[cfg(target_arch = "wasm32")] crate::web_api::audio(b);
}
/// the stage now wanted (and being built)
pub fn current() -> String { CUR.lock().unwrap_or_else(|e| e.into_inner()).clone() }
/// Build `name` and hand it to the game: world and obstacles to the engine, the sound emitters, then the scene and the runtime packet.
/// `scene_only`: the GL context was lost, the running game just needs its scene again. On a phone it runs on its own thread (the menu
/// keeps drawing the loading screen); in the browser at the start of the next frame.
pub fn request(name: &str, scene_only: bool) {
    let g = GEN.fetch_add(1, Ordering::SeqCst) + 1; let name = if crate::game::MAPS.contains(&name) { name.to_string() } else { "forest".to_string() };
    if !scene_only { *CUR.lock().unwrap_or_else(|e| e.into_inner()) = name.clone(); }
    #[cfg(not(target_arch = "wasm32"))]
    { let _ = std::thread::Builder::new().name("stage".into()).stack_size(16 << 20).spawn(move || run(&name, scene_only, g)); }
    #[cfg(target_arch = "wasm32")]
    { *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some((name, scene_only, g)); }
}
/// the browser: build a stage asked for since the last frame
pub fn poll() {
    #[cfg(target_arch = "wasm32")]
    { let p = PENDING.lock().unwrap_or_else(|e| e.into_inner()).take(); if let Some((n, s, g)) = p { run(&n, s, g); } }
}
fn run(name: &str, scene_only: bool, g: u64) {
    let Some(b) = crate::guard::run_noted("stage", || build(name, &[])) else { return };
    if GEN.load(Ordering::SeqCst) != g && !scene_only { return; }   // a newer stage was asked for meanwhile
    if !scene_only {
        *crate::bridge::lock() = Some(crate::bridge::Engine { world: b.world, obstacles: crate::physics::Obstacles::new(&b.obstacles), grid: Vec::new() });
        for m in &b.audio { audio_out(m); }
    }
    crate::gfx::push_message(b.scene);
    if !scene_only { crate::gfx::push_message(b.rt); }
}
