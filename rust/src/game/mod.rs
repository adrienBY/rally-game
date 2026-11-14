//! The game, natively: the frame loop that used to run in the page (camera, laps, race and rivals, dust, set pieces, HUD, sound control),
//! ported line for line from the JavaScript so it plays the same. The page only builds each stage and hands it over (see `rt`).
pub mod hud;
pub mod math;
pub mod race;
pub mod rt;
mod draw;

use crate::bridge;
use crate::noise::{clamp, fbm, hash2, smooth, vnoise, wrap_a};
use crate::physics::{Car, Input, Tune};
use crate::render::Renderer;
use crate::world::{World, ROAD, WL};
use math::*;
use race::Race;
use rt::Rt;
use std::collections::HashMap;

pub const TIRE_R: f64 = 0.31;
pub const VEH_L: f64 = 1.22 - -1.28;
/// ground tiles switch to every 2nd grid line past LOD1 metres, every 4th past LOD2
const LOD1: f64 = 150.0; const LOD2: f64 = 300.0;
const REST: f64 = 0.21; const STOP: f64 = 0.33;
const PIX: [f64; 3] = [170.0, 220.0, 340.0];
pub const MAPS: [&str; 6] = ["forest", "snow", "canyon", "lakes", "touge", "riviera"];

/// xorshift64*: the page's Math.random / seeded rnd() stand-in (particles, mistakes, grid reactions)
pub struct Rng(u64);
impl Rng {
    pub fn f(&mut self) -> f64 { let mut x = self.0; x ^= x >> 12; x ^= x << 25; x ^= x >> 27; self.0 = x; ((x.wrapping_mul(0x2545F4914F6CDD1D) >> 11) as f64) / (1u64 << 53) as f64 }
    pub fn r(&mut self, a: f64, b: f64) -> f64 { a + (b - a) * self.f() }
}

/// saved settings and records, one "key=value" line each (the page's localStorage keys)
pub struct Saves { path: String, pub kv: HashMap<String, String>, dirty: bool }
impl Saves {
    pub fn load(path: &str) -> Saves {
        let mut kv = HashMap::new();
        if let Ok(s) = std::fs::read_to_string(path) { for l in s.lines() { if let Some((k, v)) = l.split_once('=') { kv.insert(k.to_string(), v.to_string()); } } }
        Saves { path: path.to_string(), kv, dirty: false }
    }
    pub fn get(&self, k: &str) -> Option<&str> { self.kv.get(k).map(|s| s.as_str()) }
    pub fn set(&mut self, k: &str, v: String) { if self.kv.get(k) != Some(&v) { self.kv.insert(k.to_string(), v); self.dirty = true; } }
    /// records the page still had in its own storage come over once
    pub fn import(&mut self, list: &[(String, String)]) { for (k, v) in list { if !self.kv.contains_key(k) { self.kv.insert(k.clone(), v.clone()); self.dirty = true; } } }
    pub fn flush(&mut self) {
        if !self.dirty || self.path.is_empty() { return; } self.dirty = false;
        let mut s = String::new(); let mut keys: Vec<_> = self.kv.keys().collect(); keys.sort();
        for k in keys { s.push_str(k); s.push('='); s.push_str(&self.kv[k]); s.push('\n'); }
        let tmp = format!("{}.tmp", self.path); if std::fs::write(&tmp, s).is_ok() { let _ = std::fs::rename(&tmp, &self.path); }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum State { Title, Drive }

#[derive(Clone, Copy, Default)]
pub struct Rect { pub x: f64, pub y: f64, pub w: f64, pub h: f64 }

pub struct CamState { pub y: f64, pub h: f64, pub init: bool, pub mode: i32, pub fov: f64, pub lag: f64, pub cp: Option<f64>, pub cr: Option<f64>,
    pub shot: i32, pub cut_t: f64, pub passed: f64, pub anchor: V3, pub target: usize, pub side: f64 }

/// settings that survive switching stages
pub struct Persist { pub saves: Saves, pub sound_on: bool, pub vib_on: bool, pub pix_idx: usize, pub touch_used: bool, pub downgraded: bool,
    pub race_mode: bool, pub race_skill: f64 }
impl Persist {
    pub fn new(save_path: &str) -> Persist {
        let saves = Saves::load(save_path);
        let mut p = Persist { saves, sound_on: true, vib_on: true, pix_idx: 2, touch_used: false, downgraded: false, race_mode: false, race_skill: 0.95 };
        p.reload(); p
    }
    pub fn reload(&mut self) {
        self.vib_on = self.saves.get("fogwood-vib") != Some("0");
        if let Some(v) = self.saves.get("fogwood-pix").and_then(|v| v.parse::<usize>().ok()) { if v < PIX.len() { self.pix_idx = v; } }
        self.race_mode = self.saves.get("fogwood-mode") == Some("race");
        if let Some(v) = self.saves.get("fogwood-skill").and_then(|v| v.parse::<f64>().ok()) { if v > 0.8 && v < 1.1 { self.race_skill = v; } }
    }
}

pub struct Game {
    pub rt: Rt, pub np: usize, pub snow: bool, pub can: bool, pub lak: bool, pub tou: bool, pub riv: bool,
    pub tune: Tune, pub cam_follow: f64, pub car: Car, pub rpm_shown: f64, pub shake: f64, pub car_hit: f64, pub prev_throttle: f64,
    pub state: State, pub g_t: f64, pub lap: i32, pub lap_start: f64, pub best: Option<f64>, pub flags: [bool; 3], pub last_idx: usize,
    pub msg: String, pub msg_t: f64, pub wrong_t: f64, pub started: bool, pub go: f64,
    pub cam: CamState, pub cam_pos: V3, pub cam_rot: M, pub fov_now: f64, pub view: M, pub proj: M, pub w: f64, pub h: f64,
    pub lw: i32, pub lh: i32, pub u: f64, pub inset: [f64; 4], pub inset_px: [f64; 4],
    pub race: Race, pub ui: HashMap<&'static str, Rect>, touches: HashMap<i32, &'static str>,
    pub time: f64, pub wind: f64, pub rng: Rng, pub events: Vec<String>, pub ps: Persist, pub loading: Option<String>,
    // scene
    node_base: Vec<(V3, V3, V3, Option<M>)>, node_world: Vec<M>, obj_last: HashMap<usize, M>, named: HashMap<String, usize>,
    cull_vis: Vec<bool>, glit_op: Vec<f32>,
    // particles
    d_pos: Vec<f32>, d_a: Vec<f32>, d_s: Vec<f32>, d_v: Vec<f64>, d_age: Vec<f64>, d_life: Vec<f64>, d_head: usize, dust_was: bool,
    sf_pos: Vec<f32>, sf_init: bool,
    pub boats: Vec<rt::Boat>,
    // light
    pub sun_vis: f64, pub expo: f64, pub dim: f64, pub expo_tun: bool, pub sun_uv: [f64; 2],
    // feel
    hap_t: f64, hap_cool: f64, aud_wet: f64, pub car_audio_on: bool, race_was: bool, pop_cool: f64, fps_acc: f64, fps_n: i32, hud_tick: bool, pub hud: hud::Canvas,
    shadow_op: f32, pub flash: f64, pub speed_k: f64, clock: std::time::Instant, pub last_pidx: usize,
    /// test hook: the demo driver drives your car too
    pub auto: bool,
    tile_lod: Vec<usize>, fps30: Option<bool>, pub shade_dim: f64,
    /// benchmark: ms spent in each part of the last tick (physics, race, sound, fx, camera, rivals, world, hud) and whether the HUD was uploaded
    pub prof: [f32; 8], pub hud_uploaded: bool, pub bench_req: bool, pub bench_secs: Option<u32>, pub d_live: usize,
}

pub fn path_idx(w: &World, x: f64, z: f64) -> usize {
    let i = ((x + 0.5).floor() as i64 + w.gxh as i64).clamp(0, w.gnx as i64 - 1) as usize;
    let j = ((z + 0.5).floor() as i64 + w.gzh as i64).clamp(0, w.gnz as i64 - 1) as usize;
    w.pi[j * w.gnx + i] as usize
}

impl Game {
    pub fn new(rt: Rt, mut ps: Persist) -> Game {
        ps.saves.import(&rt.saves); ps.reload(); ps.saves.flush();
        let np = rt.np; let mid = rt.map_id;
        let mut tv = [0f64; 33]; let names = crate::physics::PARAM_NAMES;
        for (i, n) in names.iter().enumerate() { if let Some((_, v)) = rt.tune.iter().find(|(k, _)| k == n) { tv[i] = *v; } }
        tv[29] = rt.susp[0]; tv[30] = rt.susp[1]; tv[31] = rt.susp[2]; tv[32] = rt.iz;
        let cam_follow = rt.tune.iter().find(|(k, _)| k == "camFollow").map(|v| v.1).unwrap_or(16.0);
        let best = ps.saves.get(&format!("fogwood-best-{}", rt.map)).and_then(|v| v.parse::<f64>().ok()).filter(|v| *v > 0.0);
        let named = rt.nodes.iter().enumerate().map(|(i, n)| (n.name.clone(), i)).collect();
        let node_base = rt.nodes.iter().map(|n| (n.p, n.r, n.s, None)).collect();
        let dn = rt.dust.map(|d| d.1).unwrap_or(420);
        let sf_pos = rt.snow.as_ref().map(|s| s.2.clone()).unwrap_or_default();
        let boats = std::mem::take(&mut { let mut b = Vec::new(); for q in &rt.boats { b.push(rt::Boat { x: q.x, z: q.z, h: q.h, v: q.v, sail: q.sail, ph: q.ph, sc: q.sc }); } b });
        let race = Race::new(&rt, &ps);
        let ncull = rt.cull.len(); let nglit = rt.glit.len();
        let mut g = Game {
            np, snow: mid == 1, can: mid == 2, lak: mid == 3, tou: mid == 5, riv: mid == 6,
            tune: Tune::from_slice(&tv), cam_follow, car: Car::new(), rpm_shown: 900.0, shake: 0.0, car_hit: 0.0, prev_throttle: 0.0,
            state: State::Title, g_t: 0.0, lap: 1, lap_start: 0.0, best, flags: [false; 3], last_idx: 0, msg: String::new(), msg_t: 0.0, wrong_t: 0.0, started: false, go: 0.0,
            cam: CamState { y: 0.0, h: 0.0, init: false, mode: 0, fov: 56.0, lag: 0.0, cp: None, cr: None, shot: -1, cut_t: 0.0, passed: 0.0, anchor: [0.0; 3], target: 0, side: 1.0 },
            cam_pos: [0.0; 3], cam_rot: ID, fov_now: 56.0, view: ID, proj: ID, w: 1280.0, h: 720.0, lw: 320, lh: 180, u: 1.0, inset: [0.0; 4], inset_px: [0.0; 4],
            race, ui: HashMap::new(), touches: HashMap::new(), time: 0.0, wind: 0.5, rng: Rng(0x9E3779B97F4A7C15 ^ (mid as u64 + 1)), events: Vec::new(), ps, loading: None,
            node_base, node_world: vec![ID; rt.nodes.len()], obj_last: HashMap::new(), named, cull_vis: vec![true; ncull], glit_op: vec![-1.0; nglit],
            d_pos: vec![0.0; dn * 3], d_a: vec![0.0; dn], d_s: vec![0.0; dn], d_v: vec![0.0; dn * 3], d_age: vec![0.0; dn], d_life: vec![0.0; dn], d_head: 0, dust_was: true,
            sf_pos, sf_init: false, boats,
            sun_vis: 0.0, expo: 1.0, dim: 1.0, expo_tun: false, sun_uv: [0.5, 0.8],
            hap_t: 0.0, hap_cool: 0.0, aud_wet: 0.0, car_audio_on: false, race_was: false, pop_cool: 0.0, fps_acc: 0.0, fps_n: 0, hud_tick: false, hud: hud::Canvas::new(320, 180),
            shadow_op: -1.0, flash: 0.0, speed_k: 0.0, clock: std::time::Instant::now(), last_pidx: 0, auto: false, tile_lod: Vec::new(), fps30: None, shade_dim: 1.0, prof: [0.0; 8], hud_uploaded: false, bench_req: false, bench_secs: None, d_live: 0, rt,
        };
        if let Some(e) = bridge::lock().as_ref() { g.place_at(&e.world, 6); }
        g
    }
    pub fn into_persist(self) -> Persist { self.ps }
    /// once the scene is on the GPU: coarse grids for the far ground tiles
    pub fn attach(&mut self, r: &mut Renderer) {
        for t in &self.rt.tiles { r.add_grid_lods(t.0, t.1, t.2); }
        self.tile_lod = vec![usize::MAX; self.rt.tiles.len()];
    }
    /// the GPU copy of the scene was rebuilt (lost GL context): push every object state again
    pub fn scene_reloaded(&mut self) {
        self.tile_lod.clear(); self.obj_last.clear(); for v in self.cull_vis.iter_mut() { *v = !*v; } self.shadow_op = -1.0; for v in self.glit_op.iter_mut() { *v = -1.0; }
        self.race.shown = false; self.race_was = self.race.on; self.dust_was = true; self.hud.invalidate();
    }
    pub fn clock_ms(&self) -> f64 { self.clock.elapsed().as_secs_f64() * 1000.0 }
    pub fn stage_km(&self) -> String { format!("{:.2}", self.np as f64 / 1000.0) }

    // ---------------- road helpers ----------------
    pub fn road_dist(&self, w: &World, x: f64, z: f64) -> f64 { let p = self.rt.path[path_idx(w, x, z)]; (x - p[0]).hypot(z - p[1]) }
    fn cdi(&self, a: i64, b: i64) -> i64 { let n = self.np as i64; let d = ((a - b) % n + n) % n; d.min(n - d) }
    fn tun_at(&self, i: usize) -> Option<[i64; 4]> { for t in &self.rt.tun { if self.cdi(i as i64, t[0]) <= t[1] { return Some(*t); } } None }

    /// placeRaw: the car at rest on the road at index i
    pub fn place_raw(car: &mut Car, rt: &Rt, w: &World, i: usize) {
        let p = rt.path[i]; let t = rt.tan[i];
        car.x = p[0]; car.z = p[1]; car.h = t[0].atan2(t[1]); car.vx = 0.0; car.vz = 0.0; car.r = 0.0; car.gear = 1.0; car.rpm = 900.0; car.steer = 0.0;
        car.ax_f = 0.0; car.ay_f = 0.0; car.y = w.height(car.x, car.z) + 0.075; car.vy = 0.0; car.pitch = 0.0; car.roll = 0.0; car.pr = 0.0; car.rr = 0.0;
        car.air_t = 0.0; car.landed = false; car.land_v = 0.0;
        for wh in car.wh.iter_mut() { wh.w = 0.0; wh.comp = 0.135; wh.d = 0.075; wh.fz = 2900.0; }
        car.hb_t = 0.0; car.p_t = 0.0; car.p_b = 0.0; car.impact = 0.0;
    }
    pub fn place_at(&mut self, w: &World, i: usize) { Self::place_raw(&mut self.car, &self.rt, w, i); self.cam.init = false; }

    pub fn buzz(&mut self, k: &str, s: f64) { if self.ps.vib_on { self.events.push(format!("h {} {:.3}", k, s.clamp(0.0, 1.0))); } }
    pub fn flash_msg(&mut self, m: &str, t: f64) { self.msg = m.to_string(); self.msg_t = t; }

    // ---------------- screen ----------------
    pub fn resize(&mut self, w: f64, h: f64, inset_px: [f64; 4]) {
        self.w = w.max(1.0); self.h = h.max(1.0); self.inset_px = inset_px;
        let s = self.w.min(self.h) / PIX[self.ps.pix_idx];
        self.lw = ((self.w / s + 0.5).floor() as i32).max(120); self.lh = ((self.h / s + 0.5).floor() as i32).max(90);
        self.u = ((self.lh as f64).min(self.lw as f64 * 0.56) / 150.0).floor().max(1.0);
        let k = self.lw as f64 / self.w; for i in 0..4 { self.inset[i] = inset_px[i] * k; }
        self.hud.resize(self.lw, self.lh);
    }

    // ---------------- input ----------------
    fn pressed(&self, n: &str) -> bool { self.touches.values().any(|v| *v == n) }
    pub fn read_input(&self) -> Input {
        let l = self.pressed("left"); let r = self.pressed("right");
        Input { steer: (l as i32 - r as i32) as f64, throttle: if self.pressed("gas") { 1.0 } else { 0.0 }, brake: if self.pressed("brake") { 1.0 } else { 0.0 }, hb: self.pressed("hb") }
    }
    pub fn ai_input(&self, w: &World) -> Input {
        use libm::{atan, atan2, sin}; use crate::physics::js_hypot;
        let s = &self.car; let np = self.np; let i = path_idx(w, s.x, s.z); let u = s.u.max(0.0); let ld = 7.0 + u * 0.42;
        let tgt = self.rt.path[(i + (ld + 0.5).floor() as usize) % np];
        let (dx, dz) = (tgt[0] - s.x, tgt[1] - s.z);
        let sp = js_hypot(s.vx, s.vz); let vh = if sp > 2.0 { atan2(s.vx, s.vz) } else { s.h };
        let al = wrap_a(atan2(dx, dz) - vh); let r_want = 2.0 * sp.max(1.0) * sin(al) / ld;
        let delta = wrap_a(vh - s.h) + atan(2.0 * VEH_L * sin(al) / ld) - (s.r - r_want) * 0.12; let lock = self.steer_lock(u);
        let vt = self.rt.vprof[(i + 4) % np] * 0.9; let slide_cut = clamp(1.0 - (s.vl.abs() - 1.5) / 3.0, 0.15, 1.0);
        Input { steer: clamp(delta / lock, -1.0, 1.0), throttle: clamp((vt - u) * 0.5, 0.0, 1.0) * slide_cut, brake: clamp((u - vt - 1.5) * 0.3, 0.0, 1.0), hb: false }
    }
    pub fn steer_lock(&self, u: f64) -> f64 { self.tune.lock / (1.0 + u.max(0.0) / self.tune.lock_fall) }

    fn hit_button(&self, x: f64, y: f64) -> Option<&'static str> {
        let inb = |b: &Rect, l: f64, r: f64, t: f64, bt: f64| x >= b.x - l && x <= b.x + b.w + r && y >= b.y - t && y <= b.y + b.h + bt;
        if self.state == State::Title { for k in ["mapF", "mapS", "mapC", "mapL", "mapT", "mapR", "mode"] { if let Some(b) = self.ui.get(k) { if inb(b, 2.0, 2.0, 3.0, 3.0) { return Some(k); } } } }
        for k in TOPB { if let Some(b) = self.ui.get(k) { if inb(b, 2.0, 2.0, 2.0, 3.0) { return Some(k); } } }
        let (Some(hb), Some(left), Some(right), Some(gas)) = (self.ui.get("hb"), self.ui.get("left"), self.ui.get("right"), self.ui.get("gas")) else { return None };
        if !self.ps.touch_used { return None; }
        if inb(hb, 3.0, 3.0, 3.0, 2.0) { return Some("hb"); }
        let (lw, lh) = (self.lw as f64, self.lh as f64);
        if x < lw * 0.38 && y > lh * 0.35 { return Some(if x < (left.x + left.w + right.x) / 2.0 { "left" } else { "right" }); }
        if x > lw * 0.62 && y > lh * 0.35 { return Some(if x > gas.x - 2.0 * self.u { "gas" } else { "brake" }); }
        None
    }
    fn low_xy(&self, x: f64, y: f64) -> (f64, f64) { (x / self.w * self.lw as f64, y / self.h * self.lh as f64) }
    /// pointer events in screen pixels: 0 down, 1 move, 2 up/cancel
    pub fn touch(&mut self, action: i32, id: i32, sx: f64, sy: f64) {
        let (x, y) = self.low_xy(sx, sy);
        match action {
            0 => {
                self.ps.touch_used = true; let b = self.hit_button(x, y);
                if let Some(b) = b { self.buzz("tap", if TOPB.contains(&b) || b.starts_with("map") { 0.55 } else { 0.4 }); } else if self.state == State::Title { self.buzz("tap", 0.55); }
                match b {
                    Some("snd") => self.toggle_sound(),
                    Some("vib") => { self.ps.vib_on = !self.ps.vib_on; self.ps.saves.set("fogwood-vib", if self.ps.vib_on { "1".into() } else { "0".into() }); if self.ps.vib_on { self.buzz("shift", 0.7); }
                        let m = if self.ps.vib_on { "HAPTICS ON" } else { "HAPTICS OFF" }; self.flash_msg(m, 1.0); }
                    Some("pix") => { self.ps.pix_idx = (self.ps.pix_idx + 1) % PIX.len(); self.ps.saves.set("fogwood-pix", self.ps.pix_idx.to_string()); let (w, h, i) = (self.w, self.h, self.inset_px); self.resize(w, h, i);
                        self.flash_msg(["CHUNKY PIXELS", "MEDIUM PIXELS", "FINE PIXELS"][self.ps.pix_idx], 1.0); }
                    Some("mode") => self.toggle_mode(),
                    Some("rec") => { self.bench_req = true; }   // temporary: benchmark recording on / off
                    Some(k) if k.starts_with("map") => { let want = match k { "mapF" => "forest", "mapS" => "snow", "mapC" => "canyon", "mapL" => "lakes", "mapT" => "touge", _ => "riviera" }; if want != self.rt.map { self.switch_map(want); } }
                    _ if self.state == State::Title => self.start_game(),
                    None if self.race.on && self.race.phase == race::Phase::Done && self.race.done_t > 1.2 => { self.to_title(); }
                    Some("menu") => { self.to_title(); }
                    Some("cam") => self.cycle_cam(),
                    Some("rst") => self.reset_car(),
                    Some(k) => { self.touches.insert(id, k); }
                    None => {}
                }
            }
            1 => { if !self.touches.contains_key(&id) { return; } if let Some(b) = self.hit_button(x, y) { if !TOPB.contains(&b) { if self.touches.get(&id) != Some(&b) { self.buzz("tap", 0.3); } self.touches.insert(id, b); } } }
            _ => { self.touches.remove(&id); }
        }
    }
    /// Android back: to the menu while driving; false lets the app close from the menu
    pub fn back(&mut self) -> bool { self.to_title() }

    pub fn switch_map(&mut self, name: &str) { if self.loading.is_none() { self.loading = Some(name.to_string()); self.events.push(format!("map {}", name)); self.ps.saves.set("fogwood-map", name.to_string()); } }
    fn toggle_mode(&mut self) { self.ps.race_mode = !self.ps.race_mode; self.ps.saves.set("fogwood-mode", if self.ps.race_mode { "race".into() } else { "tt".into() }); }
    pub fn toggle_sound(&mut self) { self.ps.sound_on = !self.ps.sound_on; let m = if self.ps.sound_on { "SOUND ON" } else { "SOUND OFF" }; self.flash_msg(m, 1.0); }
    fn cycle_cam(&mut self) { self.cam.mode = (self.cam.mode + 1) % 3; self.cam.init = false; self.flash_msg(["CHASE CAM", "CLOSE CAM", "BONNET CAM"][self.cam.mode as usize], 1.2); }
    fn reset_car(&mut self) { if self.state != State::Drive { return; } let g = bridge::lock(); if let Some(e) = g.as_ref() { let i = path_idx(&e.world, self.car.x, self.car.z); self.place_at(&e.world, i); } drop(g); self.flash_msg("RESET", 1.0); }
    pub fn start_game(&mut self) {
        if self.state != State::Title { return; }
        let g = bridge::lock(); let Some(e) = g.as_ref() else { return };
        self.state = State::Drive; let np = self.np; self.place_at(&e.world, np - 6); self.lap = 1; self.started = false; self.g_t = 0.0; self.flags = [false; 3]; self.last_idx = np - 6; self.go = 2.2;
        if self.ps.race_mode { race::start(self, &e.world); }
        self.car_audio_on = true;
    }
    pub fn to_title(&mut self) -> bool {
        if self.state != State::Drive { return false; }
        race::stop(self); self.state = State::Title; self.go = 0.0; self.started = false; self.g_t = 0.0; self.msg_t = 0.0; self.wrong_t = 0.0; self.touches.clear();
        let g = bridge::lock(); if let Some(e) = g.as_ref() { self.place_at(&e.world, 6); } true
    }

    fn update_lap(&mut self, w: &World, dt: f64) {
        if self.go > 0.0 { self.go -= dt; }
        if !self.started && self.car.u > 0.5 && self.go <= 0.9 { self.started = true; self.lap_start = self.g_t; }
        if self.started { self.g_t += dt; }
        let np = self.np; let i = path_idx(w, self.car.x, self.car.z); let li = self.last_idx;
        let cs = [(np as f64 / 4.0 + 0.5).floor() as usize, (np as f64 / 2.0 + 0.5).floor() as usize, (np as f64 * 3.0 / 4.0 + 0.5).floor() as usize];
        for (k, c) in cs.iter().enumerate() { if li < *c && i >= *c && i - li < 80 { self.flags[k] = true; } }
        if li > np - 80 && i < 80 {
            if self.flags.iter().all(|f| *f) && self.started {
                let lt = self.g_t - self.lap_start;
                if self.best.map(|b| lt < b).unwrap_or(true) { self.best = Some(lt); self.flash_msg(&format!("NEW BEST {}", fmt(Some(lt))), 3.0); let k = format!("fogwood-best-{}", self.rt.map); self.ps.saves.set(&k, format!("{}", lt)); }
                else { self.flash_msg(&format!("STAGE {}", fmt(Some(lt))), 3.0); }
                self.lap += 1; self.lap_start = self.g_t;
            }
            self.flags = [false; 3];
        }
        self.last_idx = i;
        let tn = self.rt.tan[i]; let fwd = self.car.h.sin() * tn[0] + self.car.h.cos() * tn[1];
        self.wrong_t = if fwd < -0.4 && self.car.u.abs() > 4.0 { self.wrong_t + dt } else { (self.wrong_t - dt * 2.0).max(0.0) };
        if self.msg_t > 0.0 { self.msg_t -= dt; }
    }

    fn emit_dust(&mut self, x: f64, y: f64, z: f64, vx: f64, vy: f64, vz: f64, size: f64) {
        let n = self.d_a.len(); if n == 0 { return; } let i = self.d_head; self.d_head = (self.d_head + 1) % n;
        self.d_pos[i * 3] = x as f32; self.d_pos[i * 3 + 1] = y as f32; self.d_pos[i * 3 + 2] = z as f32; self.d_v[i * 3] = vx; self.d_v[i * 3 + 1] = vy; self.d_v[i * 3 + 2] = vz;
        self.d_age[i] = 0.0; self.d_life[i] = self.rng.r(1.2, 2.4); self.d_s[i] = size as f32;
    }

    /// One frame of the game (dt seconds), drawn into the renderer's scene.
    pub fn tick(&mut self, r: &mut Renderer, dt: f64) {
        if self.loading.is_some() { self.hud_frame(r); return; }
        let guard = bridge::lock(); let Some(eng) = guard.as_ref() else { return };
        let w = &eng.world; let obs = &eng.obstacles;
        self.time += dt;
        self.fps_acc += dt; self.fps_n += 1;
        if self.fps_n == 90 { if !self.ps.downgraded && self.fps_acc / 90.0 > 1.0 / 36.0 && self.ps.pix_idx > 0 { self.ps.pix_idx -= 1; let (a, b, c) = (self.w, self.h, self.inset_px); self.resize(a, b, c); self.ps.downgraded = true; } self.fps_acc = 0.0; self.fps_n = 0; }
        let gust = clamp(0.5 + (fbm(self.time * 0.06, 3.3, 3) - 0.5) * 1.8, 0.0, 1.0); self.wind = gust;
        let inp = if self.state == State::Drive {
            if self.go > 0.9 { Input { steer: 0.0, throttle: 0.0, brake: 0.0, hb: true } } else if self.auto || (self.race.on && self.race.phase == race::Phase::Done) { self.ai_input(w) } else { self.read_input() }
        } else { self.ai_input(w) };
        let mut tp = std::time::Instant::now(); let mut prof = [0f32; 8];
        macro_rules! lap { ($k:expr) => {{ let n = std::time::Instant::now(); prof[$k] += (n - tp).as_secs_f32() * 1000.0; tp = n; }} }
        let shifted = self.car.update(w, obs, &self.tune, dt, &inp); lap!(0);
        if shifted { if self.state == State::Drive { self.buzz("shift", 0.45); } self.audio_msg(&[7.0, 2.0]); }
        if self.state == State::Drive { if self.race.on { race::step(self, w, obs, dt); } else { self.update_lap(w, dt); } } lap!(1);
        self.update_audio(w, dt, &inp, gust); lap!(2);
        if self.car.landed { let v = -self.car.land_v; self.car.landed = false;
            if v > 1.8 && self.state == State::Drive { self.buzz("land", clamp((v - 1.5) / 7.0, 0.15, 1.0)); }
            if v > 1.2 { self.audio_msg(&[2.0, 2.0, v as f32, 0.0]); }
            if v > 2.5 { self.shake = self.shake.max(clamp((v - 2.0) / 9.0, 0.0, 0.8));
                for k in 0..4 { let (wx, wz) = (self.car.wh[k].wx, self.car.wh[k].wz); for _ in 0..6 { let (a, b, c, d) = (self.rng.r(-2.0, 2.0), self.rng.r(0.5, 2.0), self.rng.r(-2.0, 2.0), self.rng.r(0.8, 1.4)); self.emit_dust(wx, w.height(wx, wz) + 0.2, wz, a, b, c, d); } } }
            self.car.land_v = 0.0; }
        if self.car.impact > 2.5 { self.shake = (self.car.impact / 10.0).min(1.0); }
        if self.state == State::Drive {
            self.hap_cool = (self.hap_cool - dt).max(0.0);
            if self.car.impact > 1.5 && self.hap_cool <= 0.0 { let s = clamp(self.car.impact / 9.0, 0.25, 1.0); self.buzz("impact", s); self.hap_cool = 0.12; }
            let spd = self.car.u.abs(); let slide_k = if self.snow { clamp((self.car.slide - 5.5) / 9.0, 0.0, 1.0) } else { clamp((self.car.slide - 3.2) / 7.0, 0.0, 1.0) };
            let rough_k = if self.car.contacts > 0.0 && !self.car.on_road { clamp((spd - 4.0) / 22.0, 0.0, 1.0) * if self.snow { 0.35 } else { 0.55 } } else { 0.0 };
            let k = slide_k.max(rough_k); self.hap_t -= dt;
            if k > 0.05 && self.car.contacts > 0.0 && self.hap_t <= 0.0 { self.buzz("rumble", 0.2 + 0.6 * k); self.hap_t = 0.12 - 0.05 * k; }
        }
        self.shake = (self.shake - dt * 3.0).max(0.0);
        // dust from the wheels
        let sp = self.car.vx.hypot(self.car.vz);
        for k in 0..4 { let wh = self.car.wh[k]; let tar = (self.tou || self.riv) && wh.road;
            let mut e = ((if wh.road { if self.tou || self.riv { 0.0 } else { sp / if self.can { 20.0 } else { 30.0 } } } else { sp / if self.snow { 22.0 } else if self.can { 26.0 } else { 70.0 } })
                + if tar { clamp(wh.slide - 4.8, 0.0, 10.0) / 10.0 } else { clamp(wh.slide - 1.5, 0.0, 10.0) / 5.0 }) * 18.0 * dt * if self.snow { 1.4 } else if self.can { 1.35 } else { 1.0 };
            while e > 0.0 { if self.rng.f() < e { let (vx, vz) = (self.car.vx, self.car.vz); let (a, b, c, d) = (self.rng.r(-0.8, 0.8), self.rng.r(0.3, 1.2), self.rng.r(-0.8, 0.8), self.rng.r(0.5, 0.9));
                self.emit_dust(wh.wx, w.height(wh.wx, wh.wz) + 0.2, wh.wz, -vx * 0.1 + a, b, -vz * 0.1 + c, d * if wh.road { 1.0 } else { 0.6 }); } e -= 1.0; } }
        let mut d_live = 0;
        for i in 0..self.d_a.len() { if self.d_life[i] <= 0.0 { self.d_a[i] = 0.0; continue; } d_live += 1; self.d_age[i] += dt; let k = self.d_age[i] / self.d_life[i];
            if k >= 1.0 { self.d_life[i] = 0.0; self.d_a[i] = 0.0; continue; }
            for q in 0..3 { self.d_pos[i * 3 + q] += (self.d_v[i * 3 + q] * dt) as f32; }
            self.d_v[i * 3] *= 1.0 - dt * 1.5; self.d_v[i * 3 + 1] *= 1.0 - dt * 1.2; self.d_v[i * 3 + 2] *= 1.0 - dt * 1.5; self.d_s[i] += (dt * 1.7) as f32;
            self.d_a[i] = ((if self.tou || self.riv { 0.26 } else { 0.4 }) * (1.0 - k).powf(1.6)) as f32; }
        if let Some((obj, _)) = self.rt.dust { if d_live > 0 || self.dust_was { r.set_attr(obj, "position", &self.d_pos); r.set_attr(obj, "aA", &self.d_a); r.set_attr(obj, "aS", &self.d_s); } }
        self.dust_was = d_live > 0;
        self.d_live = d_live; lap!(3);
        self.last_pidx = path_idx(w, self.car.x, self.car.z);
        self.camera(w, dt); lap!(4);
        if self.race.on { race::gfx(self, r, w); race::audio(self); } else if self.race_was { race::hide(self, r); self.audio_msg(&[8.0, 0.0]); }
        self.race_was = self.race.on; lap!(5);
        self.world_fx(r, w, dt);
        self.light(w, dt);
        self.push_nodes(r); lap!(6);
        drop(guard);
        self.hud_uploaded = false; self.hud_frame(r); lap!(7); self.prof = prof;
        self.write_frame(r);
        let slow = self.state == State::Title;   // the menu's film crew runs at 30 fps, the driving at 60
        if self.fps30 != Some(slow) { self.fps30 = Some(slow); self.events.push(if slow { "fps 30".into() } else { "fps 60".into() }); }
        self.ps.saves.flush();
    }

    fn camera(&mut self, w: &World, dt: f64) {
        let s = self.car; let sp = s.vx.hypot(s.vz); let sp_k = clamp((sp - 6.0) / 40.0, 0.0, 1.0); self.speed_k = sp_k;
        let mode = self.cam.mode;
        if mode == 2 {
            let kk = (dt * 5.0).min(1.0); let cp = self.cam.cp.unwrap_or(s.pitch); let cr = self.cam.cr.unwrap_or(s.roll);
            let cp = cp + (s.pitch - cp) * kk; let cr = cr + (s.roll - cr) * kk; self.cam.cp = Some(cp); self.cam.cr = Some(cr);
            let root = trs([s.x, s.y, s.z], [-s.pitch, s.h, s.roll], true, [1.0; 3]);
            self.cam_pos = apply(&root, [0.0, 1.5, 0.82]); self.cam_rot = euler(cp * 0.45 - 0.075, s.h + std::f64::consts::PI, -cr * 0.4, true);
        } else {
            self.cam.lag += (clamp(s.ax_f * 0.07, -0.35, 0.7) - self.cam.lag) * (dt * 2.5).min(1.0);
            let dist = (if mode == 1 { 4.7 } else { 6.4 }) + sp_k * 0.9 + self.cam.lag; let up = (if mode == 1 { 1.95 } else { 2.6 }) - sp_k * 0.3;
            if !self.cam.init { self.cam.h = s.h; self.cam.y = s.y + up; self.cam.init = true; }
            self.cam.h += wrap_a(s.h - self.cam.h) * (dt * if self.state == State::Title { 4.0 } else { self.cam_follow }).min(1.0);
            let (sh, ch) = (self.cam.h.sin(), self.cam.h.cos()); let (cxp, czp) = (s.x - sh * dist, s.z - ch * dist);
            let ty = (s.y + up).max(w.height(cxp, czp) + 0.9); self.cam.y += (ty - self.cam.y) * (dt * 14.0).min(1.0);
            let shk = self.shake * 0.12; let rum = sp_k * sp_k * (if s.on_road { 0.03 } else { 0.07 }) * if s.contacts > 0.0 { 1.0 } else { 0.2 }; let tt = self.time;
            let (a, b, c) = (self.rng.r(-1.0, 1.0), self.rng.r(-1.0, 1.0), self.rng.r(-1.0, 1.0));
            self.cam_pos = [cxp + a * shk + (vnoise(tt * 19.0, 1.3) - 0.5) * rum, self.cam.y + b * shk + (vnoise(tt * 23.0, 7.1) - 0.5) * rum * 1.6, czp + c * shk];
            self.cam_rot = look_rot(self.cam_pos, [s.x + sh * 4.0, s.y + 1.05, s.z + ch * 4.0]);
        }
        let mut fov_t = 54.0 + sp_k * 20.0;
        if self.state == State::Title { fov_t = self.cinematic(w, dt); } else { self.cam.shot = -1; }
        if self.state == State::Title && self.cam.shot >= 0 && self.cam.cut_t < 0.05 { self.cam.fov = fov_t; }
        self.cam.fov += (fov_t - self.cam.fov) * (dt * 3.0).min(1.0);
        self.fov_now = if self.w > self.h { self.cam.fov } else { self.cam.fov + 20.0 };
        let cw = compose(self.cam_pos, &self.cam_rot, [1.0; 3]); self.view = invert(&cw);
        self.proj = perspective(self.fov_now, self.w / self.h, self.rt.near, self.rt.far);
    }

    fn look(&mut self, p: V3, t: V3) { self.cam_pos = p; self.cam_rot = look_rot(p, t); }
    /// title screen: trackside telephoto, low tracking shot, drone (and on the riviera: tunnel mouth, out over the sea, the village pavement)
    fn cinematic(&mut self, w: &World, dt: f64) -> f64 {
        let np = self.np; let s = self.car; self.cam.cut_t += dt;
        let idx = path_idx(w, s.x, s.z); let (sh, ch) = (s.h.sin(), s.h.cos());
        let new_shot = self.cam.shot < 0 || self.cam.cut_t > 7.5 || (self.cam.shot == 0 && self.cam.passed > 22.0);
        if new_shot {
            self.cam.shot = (self.cam.shot + 1) % if self.riv { 6 } else { 3 }; self.cam.cut_t = 0.0; self.cam.passed = 0.0;
            if self.riv && self.cam.shot >= 3 {
                let ahead = |k0: usize, lo: usize, hi: usize| { let d = (k0 + np - idx) % np; d >= lo && d <= hi };
                if self.cam.shot == 3 { match self.rt.tun.iter().find(|t| ahead(t[3] as usize, 50, 450)) { None => self.cam.shot = 4,
                    Some(t) => { let k = (t[3] as usize + 24) % np; let p = self.rt.path[k]; let tn = self.rt.tan[k]; let dk = self.rt.drop.get(k).copied().unwrap_or(0) as f64;
                        let sd = if dk != 0.0 { -dk } else { -1.0 }; self.cam.anchor = [p[0] - tn[1] * 4.0 * sd, self.rt.roadh[k] + 1.2, p[1] + tn[0] * 4.0 * sd]; self.cam.target = k; } } }
                if self.cam.shot == 4 { let k = (idx + 80) % np; let p = self.rt.path[k]; let tn = self.rt.tan[k]; self.cam.anchor = [p[0] - tn[1] * 46.0, self.rt.roadh[k] + 9.0, p[1] + tn[0] * 46.0]; }
                if self.cam.shot == 5 { let vp = self.rt.vil.map(|v| v.0).unwrap_or(0) as usize;
                    if !ahead((vp + 80) % np, 40, 420) { self.cam.shot = 2; }
                    else { let k = (vp + 80) % np; let p = self.rt.path[k]; let tn = self.rt.tan[k]; self.cam.anchor = [p[0] + tn[1] * (ROAD + 1.3), self.rt.roadh[k] + 1.5, p[1] - tn[0] * (ROAD + 1.3)]; self.cam.target = k; } }
            }
            if self.cam.shot == 0 {
                let mut k = (idx + 70) % np; let mut q = 55; while q <= 95 { let c = (idx + q) % np; if self.rt.vcurve[c] > 20.0 { k = c; break; } q += 5; }
                let p = self.rt.path[k]; let tn = self.rt.tan[k]; let side = if hash2(k as f64, 3.0) < 0.5 { -1.0 } else { 1.0 }; let off = ROAD + 6.0 + hash2(k as f64, 5.0) * 4.0;
                let (x, z) = (p[0] - tn[1] * off * side, p[1] + tn[0] * off * side); self.cam.anchor = [x, (w.height(x, z) + 1.9).max(self.rt.roadh[k] + 1.6), z];
            }
            self.cam.side = if hash2(idx as f64, 9.0) < 0.5 { -1.0 } else { 1.0 };
        }
        let fov;
        let shot = self.cam.shot;
        if shot == 0 {
            let a = self.cam.anchor; self.look(a, [s.x, s.y + 0.8, s.z]);
            let (dx, dz) = (a[0] - s.x, a[2] - s.z); if dx * sh + dz * ch < 0.0 { self.cam.passed += dt * 8.0; } fov = clamp(16.0 + dx.hypot(dz) * 0.35, 18.0, 50.0);
        } else if shot == 1 {
            let sd = self.cam.side; let (x, z) = (s.x + ch * 5.5 * sd + sh * 2.5, s.z - sh * 5.5 * sd + ch * 2.5); let y = (w.height(x, z) + 1.0).max(s.y + 0.9);
            self.look([x, y, z], [s.x - sh * 1.5, s.y + 0.6, s.z - ch * 1.5]); fov = 44.0;
        } else if self.riv && shot >= 3 {
            let a = self.cam.anchor; let d = (a[0] - s.x).hypot(a[2] - s.z);
            self.look([a[0], a[1] + if shot == 4 { self.cam.cut_t * 0.5 } else { 0.0 }, a[2]], [s.x, s.y + 0.7, s.z]);
            fov = if shot == 4 { 38.0 } else { clamp(14.0 + d * 0.32, 20.0, 54.0) };
            let past = (idx + np - self.cam.target) % np; if shot != 4 && past < np / 2 && past > 25 { self.cam.cut_t = 99.0; }
        } else {
            let rr = if self.can { 16.0 } else { 22.0 }; let up = if self.can { 9.0 } else { 26.0 };
            let a = s.h + std::f64::consts::PI + 0.5 * (self.cam.cut_t * 0.25).sin() * self.cam.side; let (x, z) = (s.x + a.sin() * rr, s.z + a.cos() * rr);
            self.look([x, (s.y + up).max(w.height(x, z) + 4.0), z], [s.x + sh * 10.0, s.y, s.z + ch * 10.0]); fov = if self.can { 50.0 } else { 46.0 };
        }
        fov
    }

    // ---------------- scene nodes ----------------
    fn node(&self, name: &str) -> Option<usize> { self.named.get(name).copied() }
    fn set_node(&mut self, name: &str, p: Option<V3>, r: Option<V3>, s: Option<V3>) { if let Some(i) = self.node(name) { let b = &mut self.node_base[i]; if let Some(v) = p { b.0 = v; } if let Some(v) = r { b.1 = v; } if let Some(v) = s { b.2 = v; } } }
    fn node_m(&self, i: usize, world: &[M]) -> M {
        let n = &self.rt.nodes[i]; let (p, r, s, ov) = &self.node_base[i];
        let local = match ov { Some(rot) => compose(*p, rot, *s), None => trs(*p, *r, n.yxz, *s) };
        let parent = if n.parent >= 0 { world[n.parent as usize] } else { ID };
        mul(&mul(&parent, &n.pre), &local)
    }
    /// the car hierarchy (root, 4 pivots, 4 spins) posed for a car state: used for your car and for every rival's puppet
    pub fn car_pose(&self, c: &Car) -> HashMap<usize, M> {
        let mut out = HashMap::new(); let mut world = self.node_world.clone();
        let Some(root) = self.node("car") else { return out };
        let set = |i: usize, p: V3, r: V3, world: &mut Vec<M>| { let n = &self.rt.nodes[i]; let s = self.node_base[i].2; let local = trs(p, r, n.yxz, s);
            let parent = if n.parent >= 0 { world[n.parent as usize] } else { ID }; world[i] = mul(&mul(&parent, &n.pre), &local); };
        set(root, [c.x, c.y, c.z], [-c.pitch, c.h, c.roll], &mut world); out.insert(root, world[root]);
        for k in 0..4 {
            if let Some(pi) = self.node(&format!("pivot{k}")) { let b = self.rt.nodes[pi].p; let front = k < 2;
                set(pi, [b[0], TIRE_R - clamp(c.wh[k].d, REST - STOP - 0.04, REST), b[2]], [0.0, if front { c.steer } else { 0.0 }, 0.0], &mut world); out.insert(pi, world[pi]); }
            if let Some(si) = self.node(&format!("spin{k}")) { let b = self.rt.nodes[si].p; set(si, b, [c.wh[k].ang, 0.0, 0.0], &mut world); out.insert(si, world[si]); }
        }
        out
    }
    fn push_nodes(&mut self, r: &mut Renderer) {
        // your car
        let pose = self.car_pose(&self.car.clone());
        for (i, m) in pose { self.node_world[i] = m; }
        let skip: Vec<bool> = self.rt.nodes.iter().map(|n| n.name == "car" || n.name.starts_with("pivot") || n.name.starts_with("spin")).collect();
        for i in 0..self.rt.nodes.len() { if skip[i] { continue; } let m = self.node_m(i, &self.node_world); self.node_world[i] = m; }
        for k in 0..self.rt.objn.len() { let (id, ni, local) = (self.rt.objn[k].0, self.rt.objn[k].1, self.rt.objn[k].2);
            let mw = mul(&self.node_world[ni], &local);
            if self.obj_last.get(&id) != Some(&mw) { r.set_matrix(id, &to_f32(&mw)); self.obj_last.insert(id, mw); } }
    }

    fn world_fx(&mut self, r: &mut Renderer, w: &World, dt: f64) {
        let cp = self.cam_pos; let t = self.time;
        // shadow under your car
        let s = self.car; let gh = w.height(s.x, s.z); let above = s.y - gh;
        self.set_node("shadow", Some([s.x, gh + 0.05, s.z]), Some([0.0, s.h, 0.0]), Some([1.0 + clamp(above - 0.08, 0.0, 4.0) * 0.12; 3]));
        let op = (0.62 * clamp(1.0 - (above - 0.08) / 3.5, 0.12, 1.0)) as f32;
        if let Some(id) = self.rt.shadow_obj { if (op - self.shadow_op).abs() > 1e-4 { r.set_opacity(id, op); self.shadow_op = op; } }
        // far ground tiles drawn from every 2nd / 4th grid line (4x / 16x fewer triangles), with a little hysteresis
        if self.tile_lod.len() == self.rt.tiles.len() { for k in 0..self.rt.tiles.len() { let (id, _, _, c) = self.rt.tiles[k]; let d = (cp[0] - c[0]).hypot(cp[2] - c[2]); let cur = self.tile_lod[k];
            let h = if cur == usize::MAX { 0.0 } else { 12.0 };
            let lod = if d < LOD1 + if cur == 0 { h } else { -h } { 0 } else if d < LOD2 + if cur <= 1 { h } else { -h } { 1 } else { 2 };
            if lod != cur { r.set_lod(id, lod); self.tile_lod[k] = lod; } } }
        // culling by distance (fog-swallowed chunks)
        for k in 0..self.rt.cull.len() { let c = &self.rt.cull[k]; let d = ((cp[0] - c.c[0]).powi(2) + (cp[1] - c.c[1]).powi(2) + (cp[2] - c.c[2]).powi(2)).sqrt();
            let vis = d - c.r < c.d; if vis != self.cull_vis[k] { self.cull_vis[k] = vis; for &id in &c.ids { r.set_vis(id, vis); } } }
        // the sky, distant peaks and stars ride with the camera
        self.set_node("sky", Some(cp), None, None);
        if let Some(i) = self.node("peaks") { let p = self.node_base[i].0; self.node_base[i].0 = [cp[0], p[1], cp[2]]; }
        self.set_node("stars", Some(cp), None, None);
        if self.snow || self.tou { self.snowfall(r, dt); }
        // riviera: cirrus dome, low sun, boats
        if self.riv {
            for k in 0..4 { self.set_node(&format!("clouds{k}"), Some(cp), Some([0.0, t * 0.0015, 0.0]), None); }
            let sun = self.rt.sun; let rot = self.cam_rot;
            for k in 0..4 { if let Some(i) = self.node(&format!("sunb{k}")) { let d = 1420.0 - k as f64 * 6.0; self.node_base[i].0 = [sun[0] * d + cp[0], sun[1] * d + cp[1], sun[2] * d + cp[2]]; self.node_base[i].3 = Some(rot); } }
            self.boats(r, w, dt);
        }
        for k in 0..self.rt.glit.len() { let name = format!("glit{k}");
            if let Some(i) = self.node(&name) { let p = self.node_base[i].0; self.node_base[i].0 = [js_round(cp[0] / 5.0) * 5.0, p[1], js_round(cp[2] / 5.0) * 5.0]; }
            if k < 2 { let op = (0.45 + 0.45 * (t * (2.3 + k as f64 * 0.7) + k as f64 * 1.9).sin().max(0.0)) as f32; if (op - self.glit_op[k]).abs() > 1e-4 { r.set_opacity(self.rt.glit[k], op); self.glit_op[k] = op; } } }
        if self.node("water0").is_some() { let a = [(t * 0.55) % 6.0, (t * 0.2) % 6.0]; let b = [-((t * 0.35) % 7.5), (t * 0.6) % 7.5];
            if let Some(i) = self.node("water0") { let p = self.node_base[i].0; self.node_base[i].0 = [a[0], p[1], a[1]]; }
            if let Some(i) = self.node("water1") { let p = self.node_base[i].0; self.node_base[i].0 = [b[0], p[1], b[1]]; } }
    }
    fn snowfall(&mut self, r: &mut Renderer, dt: f64) {
        let Some((obj, n, _, ref seed)) = self.rt.snow else { return }; let seed = seed.clone();
        let (cx, cy, cz) = (self.cam_pos[0] as f32, self.cam_pos[1] as f32, self.cam_pos[2] as f32); let t = self.time;
        let tou = self.tou; let wx = if tou { 0.4 + self.wind * 1.2 } else { 1.2 + self.wind * 2.0 }; let fl = if tou { 2.2 } else { 0.6 }; let fall = if tou { 0.55 } else { 1.1 };
        let p = &mut self.sf_pos;
        if !self.sf_init { for i in 0..n { p[i * 3] += cx; p[i * 3 + 1] += cy; p[i * 3 + 2] += cz; } self.sf_init = true; }
        for i in 0..n { let k = i * 3; let sd = seed[i] as f64;
            p[k] += ((wx + (t * 1.3 + sd * 20.0).sin() * fl) * dt) as f32; p[k + 1] -= ((fall + sd * fall) * dt) as f32; p[k + 2] += ((t * 1.1 + sd * 13.0).cos() * 0.5 * dt) as f32;
            let dx = p[k] - cx; let dz = p[k + 2] - cz; if dx > 32.0 { p[k] -= 64.0; } else if dx < -32.0 { p[k] += 64.0; } if dz > 32.0 { p[k + 2] -= 64.0; } else if dz < -32.0 { p[k + 2] += 64.0; }
            if p[k + 1] < cy - 6.0 { p[k + 1] += 26.0; } else if p[k + 1] > cy + 20.0 { p[k + 1] -= 26.0; } }
        r.set_attr(obj, "position", &self.sf_pos);
    }
    fn boats(&mut self, r: &mut Renderer, w: &World, dt: f64) {
        let Some([hull, sail, cab, wake]) = self.rt.boat_ims else { return }; let t = self.time;
        let (mut mh, mut ms, mut mc, mut mw) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut boats = std::mem::take(&mut self.boats);
        for b in boats.iter_mut() {
            b.x += b.h.sin() * b.v * dt; b.z += b.h.cos() * b.v * dt;
            if b.x.hypot(b.z) > 1500.0 || self.far_h_riv(w, b.x + b.h.sin() * 40.0, b.z + b.h.cos() * 40.0) > -5.0 { b.h += std::f64::consts::PI * 0.9; }
            let bob = (t * 0.9 + b.ph).sin() * 0.06; let heel = if b.sail { 0.16 + (t * 0.5 + b.ph).sin() * 0.04 } else { 0.0 };
            let rot = euler((t * 0.7 + b.ph).sin() * 0.03, b.h, heel, false); let m = compose([b.x, 0.25 + bob, b.z], &rot, [b.sc; 3]);
            mh.extend_from_slice(&to_f32(&m));
            if b.sail { ms.extend_from_slice(&to_f32(&m)); }
            else { mw.extend_from_slice(&to_f32(&m));
                // cabin: up one hull height, then 0.6 m back along the boat's own axis
                let dz = apply(&rot, [0.0, 0.0, -0.6]); let m2 = compose([b.x + dz[0], 0.25 + bob + 1.0 * b.sc + dz[1], b.z + dz[2]], &rot, [b.sc; 3]); mc.extend_from_slice(&to_f32(&m2)); }
        }
        self.boats = boats;
        r.set_instances(hull, (mh.len() / 16) as i32, &mh); r.set_instances(sail, (ms.len() / 16) as i32, &ms); r.set_instances(cab, (mc.len() / 16) as i32, &mc); r.set_instances(wake, (mw.len() / 16) as i32, &mw);
    }

    /// riviera farH: the land and sea beyond the detailed stage (only the boats need it)
    fn far_h_riv(&self, w: &World, x: f64, z: f64) -> f64 {
        let (ex, ez) = ((self.rt.gxh - 2) as f64, (self.rt.gzh - 2) as f64);
        let (cx, cz) = (clamp(x, -ex, ex), clamp(z, -ez, ez)); let d_out = (x - cx).hypot(z - cz);
        if d_out <= 0.0 { return w.height(x, z); }
        let r = x.hypot(z) + 1.0; let sec = smooth(0.25, 0.65, (x * 0.94 - z * 0.34) / r);
        let relief = sec * smooth(900.0, 1400.0, r) * (30.0 + crate::noise::ridged(x * 0.0021 + 5.0, z * 0.0021 - 1.0).powf(1.5) * 420.0);
        w.height(cx, cz) + crate::world::riv_land(x, z, true) - crate::world::riv_land(cx, cz, true) + relief * smooth(0.0, 380.0, d_out)
    }

    fn light(&mut self, w: &World, dt: f64) {
        let sun = self.rt.sun; let cp = self.cam_pos;
        let v = project(&self.view, &self.proj, [sun[0] * 150.0 + cp[0], sun[1] * 150.0 + cp[1], sun[2] * 150.0 + cp[2]]);
        let fwd = [-self.cam_rot[8], -self.cam_rot[9], -self.cam_rot[10]];
        self.sun_uv = [v[0] * 0.5 + 0.5, v[1] * 0.5 + 0.5];
        let edge = if v[2] < 1.0 { smooth(1.45, 0.95, v[0].abs().max(v[1].abs())) } else { 0.0 };
        let tgt = clamp((fwd[0] * sun[0] + fwd[1] * sun[1] + fwd[2] * sun[2]) * 1.6 + 0.15, 0.0, 1.0) * edge * if self.riv { 0.38 } else { 1.0 };
        self.sun_vis += (tgt - self.sun_vis) * (dt * 5.0).min(1.0);
        self.flash = self.shake * 0.5;
        // your car in the baked tree shadows: it loses the sun's share of its light
        if self.rt.shade.len() == self.np * 13 {
            let s = self.car; let i = path_idx(w, s.x, s.z); let p = self.rt.path[i]; let tn = self.rt.tan[i];
            let lat = (s.x - p[0]) * -tn[1] + (s.z - p[1]) * tn[0]; let l = (js_round(lat) + 6.0).clamp(0.0, 12.0) as usize;
            let occ = self.rt.shade[i * 13 + l] as f64 / 99.0; let t = &self.rt.shade_tint; let loss = 1.0 - (t[0] + t[1] + t[2]) / 3.0;
            let tgt = 1.0 - occ * loss * 0.85; self.shade_dim += (tgt - self.shade_dim) * (dt * 6.0).min(1.0);
        }
        if self.riv {
            let ci = path_idx(w, cp[0], cp[2]); let ct = self.tun_at(ci);
            let inside = ct.map(|t| self.road_dist(w, cp[0], cp[2]) < 4.6 && cp[1] < self.rt.roadh[ci] + 6.2 && t[1] - self.cdi(ci as i64, t[0]) > 1).unwrap_or(false);
            let tgt = if inside { 2.15 } else { 1.0 }; self.expo += (tgt - self.expo) * (dt / if tgt > self.expo { 0.75 } else { 1.1 }).min(1.0);
            let s = self.car; let ki = path_idx(w, s.x, s.z); let kt = self.tun_at(ki); let near = self.road_dist(w, s.x, s.z) < 5.0;
            let dim = match kt { Some(t) if near => 1.0 - 0.74 * smooth(-1.0, 5.0, (t[1] - self.cdi(ki as i64, t[0])) as f64), _ => 1.0 };
            self.dim += (dim - self.dim) * (dt * 8.0).min(1.0); self.expo_tun = kt.is_some() && near;
        }
    }

    fn write_frame(&mut self, r: &mut Renderer) {
        let fr = &mut r.frame;
        fr.lw = self.lw; fr.lh = self.lh; fr.view = to_f32(&self.view); fr.proj = to_f32(&self.proj);
        fr.cam = [self.cam_pos[0] as f32, self.cam_pos[1] as f32, self.cam_pos[2] as f32];
        fr.time = self.time as f32; fr.wind = self.wind as f32; fr.sun = [self.sun_uv[0] as f32, self.sun_uv[1] as f32]; fr.sun_vis = self.sun_vis as f32;
        fr.flash = self.flash as f32; fr.speed = (if self.cam.mode == 2 { self.speed_k * 1.1 } else { self.speed_k }) as f32;
        fr.dust_scale = (self.lh as f64 / (2.0 * (self.fov_now * std::f64::consts::PI / 360.0).tan())) as f32;
        fr.expo = self.expo as f32; fr.dim = (self.dim * self.shade_dim) as f32;
        fr.split_s = [self.rt.split[0], self.rt.split[1], self.rt.split[2]]; fr.split_h = [self.rt.split[3], self.rt.split[4], self.rt.split[5]];
        fr.texel = [1.0 / self.lw as f32, 1.0 / self.lh as f32]; fr.valid = true;
    }

    // ---------------- sound ----------------
    pub fn audio_msg(&self, v: &[f32]) { let mut b = Vec::with_capacity(v.len() * 4); for x in v { b.extend_from_slice(&x.to_le_bytes()); } fogwood_audio::native::message(&b); }
    fn obstacle_mat(&self, x: f64, z: f64) -> f64 {
        let mut best = 9.0; let mut m = 0.0;
        for o in &self.rt.obstacles { if (o[0] - x).abs() > 24.0 || (o[1] - z).abs() > 24.0 { continue; } let d = (o[0] - x).hypot(o[1] - z) - o[2]; if d < best { best = d; m = o[3]; } }
        m
    }
    fn update_audio(&mut self, w: &World, dt: f64, inp: &Input, gust: f64) {
        let s = self.car; self.rpm_shown += (s.rpm - self.rpm_shown) * (dt * 14.0).min(1.0);
        // engine and tyres (the build 16 recordings, played by the native mixer)
        let thr = if s.gear < 0.0 { inp.brake } else { inp.throttle };
        let tar = (self.tou || self.riv) && s.on_road;
        let slide = if self.snow { clamp((s.slide - 5.5) / 9.0, 0.0, 1.0) } else if tar { clamp((s.slide - 4.8) / 8.0, 0.0, 1.0) } else { clamp((s.slide - 3.0) / 7.0, 0.0, 1.0) };
        let surf = if self.snow { 1.0 } else if self.can { 2.0 } else if tar { 3.0 } else if s.on_road { 4.0 } else { 0.0 };
        let sp = s.vx.hypot(s.vz);
        let on = self.ps.sound_on && self.car_audio_on;
        self.audio_msg(&[6.0, self.rpm_shown as f32, thr as f32, slide as f32, surf as f32, (sp / 35.0).min(1.0) as f32, if s.on_road { 1.0 } else { 0.0 },
            if self.expo_tun { 1.0 } else { 0.0 }, if s.shift_t > 0.0 { 1.0 } else { 0.0 }, if on { 0.9 } else { 0.0 }]);
        self.pop_cool -= dt;
        if self.prev_throttle > 0.0 && inp.throttle == 0.0 && self.rpm_shown > 4800.0 && self.pop_cool <= 0.0 && s.gear > 0.0 {
            self.pop_cool = 0.6; let a = self.rng.r(1.9, 2.4); let two = self.rng.f() < 0.5; let b = self.rng.r(2.2, 2.6);
            self.audio_msg(&[7.0, 1.0, a as f32, if two { b as f32 } else { 0.0 }]); }
        let mut wet = 0.0; for wh in &s.wh { if self.lak && wh.hg < WL - 0.05 { wet += 0.25; } }
        if wet > 0.0 && self.aud_wet == 0.0 && sp > 5.0 { self.audio_msg(&[2.0, 4.0, sp as f32, 0.0]); }
        self.aud_wet = wet;
        if s.impact > 1.5 { let m = if self.car_hit > 0.0 { 1.0 } else { self.obstacle_mat(s.x, s.z) }; self.audio_msg(&[2.0, 1.0, s.impact as f32, m as f32]); } self.car_hit = 0.0;
        let fwd = [-self.cam_rot[8], -self.cam_rot[10]]; let on_road: f64 = s.wh.iter().map(|w| if w.road { 0.25 } else { 0.0 }).sum();
        let master = if self.ps.sound_on { 0.9 } else { 0.0 };
        let cp = self.cam_pos; let _ = w;
        self.audio_msg(&[1.0, self.rpm_shown as f32, thr as f32, sp as f32, s.slide as f32, on_road as f32, if self.snow { 1.0 } else if self.can { 2.0 } else if self.tou || self.riv { 3.0 } else { 0.0 }, wet as f32,
            if s.contacts == 0.0 { 1.0 } else { 0.0 }, s.gear as f32, if self.state == State::Title { 3.0 } else { self.cam.mode as f32 }, cp[0] as f32, cp[1] as f32, cp[2] as f32, fwd[0].atan2(fwd[1]) as f32,
            s.x as f32, s.y as f32, s.z as f32, gust as f32, if self.state == State::Drive { 1.0 } else { 0.0 }, master as f32, if s.shift_t > 0.0 { 1.0 } else { 0.0 }, inp.brake as f32,
            if s.on_road { 0.0 } else { clamp(sp / 20.0, 0.0, 1.0) as f32 }]);
        self.prev_throttle = inp.throttle;
    }

    fn hud_frame(&mut self, r: &mut Renderer) {
        self.hud_tick = !self.hud_tick;
        if !(self.hud_tick || self.state != State::Drive || !self.touches.is_empty() || self.loading.is_some()) { return; }
        draw::hud(self);
        if let Some(rects) = self.hud.finish() { r.set_hud_rects(&self.hud.bytes, self.hud.w, self.hud.h, &rects); self.hud_uploaded = true; }
    }
}

pub const TOPB: [&str; 7] = ["snd", "vib", "cam", "pix", "rst", "menu", "rec"];
#[inline] pub fn js_round(v: f64) -> f64 { (v + 0.5).floor() }
pub fn fmt(t: Option<f64>) -> String {
    match t { None => "-:--.--".into(), Some(t) => { let m = (t / 60.0).floor(); let s = t - m * 60.0; format!("{}:{}{:.2}", m as i64, if s < 10.0 { "0" } else { "" }, s) } }
}
