//! Tile fall: an online party mode. Everyone starts on a ring on a square arena of tiles floating high over the stage; a tile you
//! drive over cracks, flashes and drops away a moment later, and whoever falls through is out. The last car still on the arena
//! wins. After a while the arena also crumbles in from its edges, so a round always ends.
//!
//! Who decides what: each phone drives its own car (as in a race) and falls on its own; the host marks the tiles (from every car's
//! position as it sees them) and sends the list of marks to everyone, so all phones drop the same tiles at the same moment of the
//! shared race clock. Each phone sees for itself who has fallen (a car well under the arena's top).
use super::race::Phase;
use super::Game;
use crate::physics::{Car, Input};
use crate::render::Renderer;
use crate::world::{Arena, World};

/// tiles per side, tile size (m), time from a tile being marked to its drop (s), the arena's height over the ground under it (m)
pub const N: usize = 12;
pub const S: f64 = 6.5;
pub const DELAY: f64 = 0.8;
const LIFT: f64 = 46.0;
/// a car this far under the top has fallen; this far, its fall is stopped (out of the way) and the camera watches the arena
const OUT_DY: f64 = 4.0;
const HOLD_DY: f64 = 26.0;
/// the arena starts crumbling in from its edges this long after the start, a tile every CRUMBLE seconds
const SUDDEN: f64 = 40.0;
const CRUMBLE: f64 = 0.45;

#[derive(Default)]
pub struct TileFall {
    pub on: bool,
    /// the arena's corner and top
    pub x0: f64, pub z0: f64, pub y: f64,
    /// race time each tile was marked (it drops DELAY later)
    pub mark: Vec<Option<f64>>,
    /// per race car: the race time it fell
    pub out: Vec<Option<f64>>,
    pub winner: Option<usize>,
    solid: Vec<bool>, crumble_t: f64, mats: Vec<f32>, cols: Vec<f32>,
}

impl TileFall {
    pub fn alive(&self, k: usize) -> bool { self.out.get(k).map(|o| o.is_none()).unwrap_or(true) }
    pub fn centre(&self) -> (f64, f64) { (self.x0 + N as f64 * S / 2.0, self.z0 + N as f64 * S / 2.0) }
    fn cell(&self, x: f64, z: f64) -> Option<usize> {
        let i = ((x - self.x0) / S).floor(); let j = ((z - self.z0) / S).floor();
        if i < 0.0 || j < 0.0 || i >= N as f64 || j >= N as f64 { None } else { Some(j as usize * N + i as usize) }
    }
    fn tile_centre(&self, k: usize) -> (f64, f64) { (self.x0 + ((k % N) as f64 + 0.5) * S, self.z0 + ((k / N) as f64 + 0.5) * S) }
    /// still there at race time t
    pub fn solid_at(&self, k: usize, t: f64) -> bool { self.mark[k].map(|m| t < m + DELAY).unwrap_or(true) }
    /// the camera once your car is out: a slow orbit high over the arena
    pub fn spectate(&self, time: f64) -> ([f64; 3], [f64; 3], f64) {
        let (cx, cz) = self.centre(); let a = time * 0.13; let r = N as f64 * S * 0.62;
        ([cx + a.cos() * r, self.y + 34.0, cz + a.sin() * r], [cx, self.y - 2.0, cz], 52.0)
    }
}

/// a car at rest at (x, z) on the arena, facing heading h
fn put(car: &mut Car, x: f64, z: f64, y: f64, h: f64) {
    car.x = x; car.z = z; car.h = h; car.vx = 0.0; car.vz = 0.0; car.r = 0.0; car.gear = 1.0; car.rpm = 900.0; car.steer = 0.0;
    car.ax_f = 0.0; car.ay_f = 0.0; car.y = y + 0.075; car.vy = 0.0; car.pitch = 0.0; car.roll = 0.0; car.pr = 0.0; car.rr = 0.0;
    car.air_t = 0.0; car.landed = false; car.land_v = 0.0; car.falling = false; car.u = 0.0;
    for wh in car.wh.iter_mut() { wh.w = 0.0; wh.comp = 0.135; wh.d = 0.075; wh.fz = 2900.0; wh.hg = y; }
    car.hb_t = 0.0; car.p_t = 0.0; car.p_b = 0.0; car.impact = 0.0;
}

/// Set up a round: the arena over the start of the stage (clear of the ground under it), every car on a ring round its middle
/// facing in (in race-car order, which is the same on every phone).
pub fn begin(g: &mut Game, w: &World) {
    w.arena_on.store(false, std::sync::atomic::Ordering::Relaxed);
    let p = g.rt.path[0]; let half = N as f64 * S / 2.0;
    let (x0, z0) = (p[0] - half, p[1] - half);
    let mut top = f64::MIN; for a in 0..=8 { for b in 0..=8 { top = top.max(w.height(x0 + a as f64 * half / 4.0, z0 + b as f64 * half / 4.0)); } }
    let top = top.max(g.rt.roadh[0]) + LIFT;
    let n = g.race.cars.len();
    g.tf = TileFall { on: true, x0, z0, y: top, mark: vec![None; N * N], out: vec![None; n], winner: None, solid: vec![true; N * N], crumble_t: 0.0, mats: Vec::new(), cols: Vec::new() };
    *w.arena.write().unwrap() = Some(Arena { x0, z0, y: top, s: S, n: N, solid: vec![true; N * N] });
    w.arena_on.store(true, std::sync::atomic::Ordering::Relaxed);
    let (cx, cz) = g.tf.centre(); let r = half * 0.62;
    for k in 0..n { let a = k as f64 / n.max(1) as f64 * std::f64::consts::TAU + 0.4;
        let (x, z) = (cx + a.cos() * r, cz + a.sin() * r); let h = (cx - x).atan2(cz - z);
        if g.race.cars[k].is_player() { put(&mut g.car, x, z, top, h); g.cam.init = false; g.crash.repair(); } else { put(&mut g.race.cars[k].car, x, z, top, h); } }
    g.race.order = (0..n).collect();
}

pub fn stop(g: &mut Game, w: &World, r: Option<&mut Renderer>) {
    if !g.tf.on { return; }
    w.arena_on.store(false, std::sync::atomic::Ordering::Relaxed); *w.arena.write().unwrap() = None;
    g.tf.on = false;
    if let (Some(r), Some(id)) = (r, g.rt.tilefall) { r.set_instances(id, 0, &[]); }
}

fn pos(g: &Game, k: usize) -> Car { if g.race.cars[k].is_player() { g.car } else { g.race.cars[k].car } }

/// One frame of a round (before the race's own step): marks, drops, who is out, the end.
pub fn step(g: &mut Game, w: &World, dt: f64) {
    if !g.tf.on { return; }
    let t = g.race.t; let running = g.race.phase == Phase::Race; let top = g.tf.y;
    let host = !g.race.online || crate::net::with(|s| s.is_host).unwrap_or(true);
    if host && running {
        let mut new = false; let grace = t < 1.5;   // a moment to get going before the tiles under the grid crack
        for k in 0..g.race.cars.len() { if !g.tf.alive(k) || g.race.cars[k].left { continue; } let c = pos(g, k);
            if grace || c.y < top - 1.0 || c.y > top + 2.5 { continue; }
            let (sh, ch) = (c.h.sin(), c.h.cos());
            for d in [-1.3, 0.0, 1.3] { if let Some(q) = g.tf.cell(c.x + sh * d, c.z + ch * d) { if g.tf.mark[q].is_none() { g.tf.mark[q] = Some(t); new = true; } } } }
        // sudden death: the edges crumble inwards (outermost ring first, round the ring)
        if t > SUDDEN { g.tf.crumble_t += dt; while g.tf.crumble_t > CRUMBLE { g.tf.crumble_t -= CRUMBLE;
            let ring = |k: usize| { let (i, j) = ((k % N) as i64, (k / N) as i64); i.min(j).min(N as i64 - 1 - i).min(N as i64 - 1 - j) };
            if let Some(q) = (0..N * N).filter(|&q| g.tf.mark[q].is_none()).min_by_key(|&q| (ring(q), ((q * 37) % 97) as i64)) { g.tf.mark[q] = Some(t); new = true; } } }
        if new && g.race.online { let list: Vec<(u8, u16)> = g.tf.mark.iter().enumerate().filter_map(|(q, m)| m.map(|m| (q as u8, (m * 20.0).round().clamp(0.0, 65535.0) as u16))).collect();
            crate::net::with(|s| s.tiles_out = list); }
    } else if !host {
        if let Some(v) = crate::net::with(|s| s.tiles_in.take()).flatten() { for (q, m) in v { let q = q as usize; if q < N * N { let m = m as f64 / 20.0; if g.tf.mark[q].map(|o| m < o).unwrap_or(true) { g.tf.mark[q] = Some(m); } } } }
    }
    // the physics' copy of which tiles are there
    let solid: Vec<bool> = (0..N * N).map(|q| g.tf.solid_at(q, t)).collect();
    if solid != g.tf.solid { if let Ok(mut a) = w.arena.write() { if let Some(a) = a.as_mut() { a.solid = solid.clone(); } } g.tf.solid = solid; }
    // who is out
    let me = g.race.player;
    for k in 0..g.race.cars.len() { if !g.tf.alive(k) { continue; } let c = pos(g, k);
        if (g.race.phase != Phase::Grid && c.y < top - OUT_DY) || g.race.cars[k].left { g.tf.out[k] = Some(t);
            if k == me { g.flash_msg("OUT!", 2.0); g.buzz("land", 0.9); } else if running { let nm = g.race.cars[k].tag(); g.flash_msg(&format!("{} IS OUT", nm), 1.4); } } }
    // your car, once well under the arena: held there out of sight while the camera watches the others
    if !g.tf.alive(me) && g.car.y < top - HOLD_DY { g.car.y = top - HOLD_DY; g.car.vx = 0.0; g.car.vy = 0.0; g.car.vz = 0.0; }
    // the order: still in first, then the last to fall
    let n = g.race.cars.len(); let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| { let (x, y) = (g.tf.out[a].unwrap_or(1e9), g.tf.out[b].unwrap_or(1e9)); y.partial_cmp(&x).unwrap() });
    g.race.pos = order.iter().position(|&k| k == me).unwrap_or(0) + 1; g.race.order = order;
    // the end: one car left (or none, or the only car fell when alone)
    if running { let alive: Vec<usize> = (0..n).filter(|&k| g.tf.alive(k)).collect();
        if alive.is_empty() || (n >= 2 && alive.len() <= 1) {
            g.race.phase = Phase::Done; g.race.done_t = 0.0; g.tf.winner = alive.first().copied();
            let m = match g.tf.winner { Some(k) if k == me => "YOU WIN!".to_string(), Some(k) => format!("{} WINS", g.race.cars[k].tag()), None => "NO ONE LEFT".into() };
            g.flash_msg(&m, 3.0); g.race.finish_pos = g.race.pos; } }
}

/// results, best first: (car, race time it fell or None for the winner)
pub fn results(g: &Game) -> Vec<(usize, Option<f64>)> { g.race.order.iter().map(|&k| (k, g.tf.out.get(k).copied().flatten())).collect() }

/// Driving yourself round the arena (the auto-driver, and your car once the round is over): towards the nearest tile still
/// unmarked, not too close to the edge, slowly.
pub fn auto_input(g: &Game) -> Input {
    let c = g.car; let t = g.race.t; let (sh, ch) = (c.h.sin(), c.h.cos());
    if g.race.phase == Phase::Done || !g.tf.alive(g.race.player) { return Input { steer: 0.0, throttle: 0.0, brake: 1.0, hb: true }; }
    let mut best: Option<(f64, f64, f64)> = None;
    for q in 0..N * N { if g.tf.mark[q].is_some() || !g.tf.solid_at(q, t) { continue; } let (x, z) = g.tf.tile_centre(q);
        let (dx, dz) = (x - c.x, z - c.z); let d = dx.hypot(dz); if d < S * 1.2 { continue; }
        let ahead = (dx * sh + dz * ch) / d.max(1e-3);
        // free tiles round it (a tile in the middle of a free patch beats one by a hole)
        let (i, j) = ((q % N) as i64, (q / N) as i64); let mut free = 0.0;
        for a in -1..=1 { for b in -1..=1 { let (ii, jj) = (i + a, j + b); if ii >= 0 && jj >= 0 && ii < N as i64 && jj < N as i64 && g.tf.mark[jj as usize * N + ii as usize].is_none() { free += 1.0; } } }
        let score = d - ahead * 9.0 - free * 2.5; if best.map(|b| score < b.0).unwrap_or(true) { best = Some((score, x, z)); } }
    let Some((_, x, z)) = best else { return Input { steer: 0.0, throttle: 0.2, brake: 0.0, hb: false } };
    let want = (x - c.x).atan2(z - c.z); let mut e = want - c.h; while e > std::f64::consts::PI { e -= std::f64::consts::TAU; } while e < -std::f64::consts::PI { e += std::f64::consts::TAU; }
    let steer = (e * 1.8).clamp(-1.0, 1.0); let slow = c.u > 13.0 || (e.abs() > 0.9 && c.u > 7.0);
    Input { steer, throttle: if slow { 0.0 } else { 0.75 }, brake: if slow { 0.5 } else { 0.0 }, hb: false }
}

/// the arena's tiles on screen: in place, shaking and flashing once marked, then tumbling down
pub fn gfx(g: &mut Game, r: &mut Renderer) {
    let Some(id) = g.rt.tilefall else { return };
    if !g.tf.on { return; }
    let t = g.race.t; let top = g.tf.y; let tf = &mut g.tf; tf.mats.clear(); tf.cols.clear();
    // two stone tones in a checker, warmer on the volcano
    let (a, b) = if g.vol { ([0.62, 0.5, 0.42], [0.48, 0.38, 0.33]) } else if g.pri { ([0.78, 0.8, 0.95], [0.6, 0.62, 0.82]) } else { ([0.86, 0.82, 0.72], [0.66, 0.63, 0.57]) };
    for q in 0..N * N {
        let (x, z) = tf.tile_centre(q); let base = if ((q % N) + (q / N)) % 2 == 0 { a } else { b };
        let (mut y, mut rx, mut rz, mut col) = (top - 0.6, 0.0f64, 0.0f64, base);
        let (mut px, mut pz) = (x, z);
        if let Some(m) = tf.mark[q] { let since = t - m;
            if since < DELAY {   // cracking: shakes harder and flashes faster towards the drop
                let k = (since / DELAY).clamp(0.0, 1.0); let hz = 6.0 + 18.0 * k; let on = ((since * hz) as i64) % 2 == 0;
                let hot = [0.95, 0.32, 0.14]; let f = if on { 0.35 + 0.6 * k } else { 0.15 * k };
                col = [base[0] + (hot[0] - base[0]) * f, base[1] + (hot[1] - base[1]) * f, base[2] + (hot[2] - base[2]) * f];
                let j = 0.05 + 0.12 * k; px += ((since * 61.0).sin()) * j; pz += ((since * 47.0).cos()) * j; y -= k * 0.08;
            } else { let d = since - DELAY; if d > 4.0 { continue; }
                y -= 0.5 * 9.81 * 1.3 * d * d; let s = if q % 2 == 0 { 1.0 } else { -1.0 }; rx = d * 0.9 * s; rz = d * 0.6 * if q % 3 == 0 { -s } else { s };
                col = [base[0] * 0.8, base[1] * 0.8, base[2] * 0.8]; } }
        // R = Rz(rz) * Rx(rx), scaled (tile, thickness 1.2, tile), column-major
        let (sx, sy, sz) = (S - 0.22, 1.2, S - 0.22); let (cx, sxr) = (rx.cos(), rx.sin()); let (cz, szr) = (rz.cos(), rz.sin());
        let c0 = [cz, szr, 0.0]; let c1 = [-szr * cx, cz * cx, sxr]; let c2 = [szr * sxr, -cz * sxr, cx];
        tf.mats.extend_from_slice(&[(c0[0] * sx) as f32, (c0[1] * sx) as f32, (c0[2] * sx) as f32, 0.0, (c1[0] * sy) as f32, (c1[1] * sy) as f32, (c1[2] * sy) as f32, 0.0,
            (c2[0] * sz) as f32, (c2[1] * sz) as f32, (c2[2] * sz) as f32, 0.0, px as f32, y as f32, pz as f32, 1.0]);
        tf.cols.extend_from_slice(&[col[0] as f32, col[1] as f32, col[2] as f32]);
    }
    let n = (tf.mats.len() / 16) as i32; r.set_instances(id, n, &tf.mats); r.set_instance_colors(id, &tf.cols);
}
