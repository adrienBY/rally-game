//! Race mode: five rival drivers on exactly the same car physics, close racing, contact, learning the stage, adaptive difficulty.
use super::math::*;
use super::rt::Rt;
use super::{path_idx, Game, Persist};
use crate::noise::{clamp, wrap_a};
use crate::physics::{Car, Input, Obstacles, Tune};
use crate::render::Renderer;
use crate::world::{World, ROAD};

pub struct Rival { pub name: &'static str, pub tag: &'static str, pub col: [f64; 3], pub pace: f64, pub aggr: f64, pub cons: f64 }
pub const RIVALS: [Rival; 5] = [
    Rival { name: "LENA VOSS", tag: "VOS", col: [1.0, 0.3, 0.24], pace: 1.0, aggr: 0.85, cons: 0.86 },
    Rival { name: "KAITO MORI", tag: "MOR", col: [0.34, 0.55, 1.0], pace: 0.985, aggr: 0.45, cons: 0.97 },
    Rival { name: "JOOST BAKKER", tag: "BAK", col: [1.0, 0.58, 0.18], pace: 0.99, aggr: 0.95, cons: 0.72 },
    Rival { name: "RUI SALGADO", tag: "SAL", col: [0.38, 0.86, 0.44], pace: 0.955, aggr: 0.55, cons: 0.9 },
    Rival { name: "ELISE DUVAL", tag: "DUV", col: [1.0, 0.88, 0.32], pace: 0.935, aggr: 0.35, cons: 0.93 }];
const ENG_PITCH: [f64; 5] = [1.0, 1.1, 0.9, 1.05, 0.95];
const RT_MU: f64 = 1.08; const RT_GRIP: f64 = 1.5; const RT_YAW: f64 = 1.15;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Phase { Off, Grid, Race, Done }

#[derive(Clone)]
pub struct RCar {
    pub r: Option<usize>, pub car: Car, pub prog: f64, pub last_idx: usize, pub i: usize, pub lat: f64, pub v: f64, pub lane: f64, pub lane_t: f64, pub pass_t: f64,
    pub cap: f64, pub pv: f64, pub boost: f64, pub form: f64, pub mist: f64, pub mist_cd: f64, pub stuck_t: f64, pub lap_t0: f64, pub laps_done: i64,
    pub last: Option<f64>, pub best: Option<f64>, pub finished: bool, pub finish_t: f64, pub react: f64, pub tag_x: f64, pub tag_y: f64, pub tag_on: bool,
    pub err: f64, pub trust: f64, pub off_at: Option<usize>, pub resets: i32,
}
impl RCar { pub fn is_player(&self) -> bool { self.r.is_none() } }

pub struct Race {
    pub on: bool, pub laps: i64, pub cars: Vec<RCar>, pub player: usize, pub phase: Phase, pub t: f64, pub done_t: f64, pub note: String, pub pos: usize,
    pub order: Vec<usize>, pub finish_pos: usize, pub rlearn: Vec<f64>, pub shown: bool,
}

impl Race {
    pub fn new(rt: &Rt, ps: &Persist) -> Race {
        let np = rt.np; let n = (np + 9) / 10; let mut rlearn = vec![1.0; n];
        if let Some(v) = ps.saves.get(&format!("fogwood-learn-{}", rt.map)) { if v.len() == n * 2 { for i in 0..n { rlearn[i] = 0.7 + v[i * 2..i * 2 + 2].parse::<f64>().unwrap_or(99.0) / 330.0; } } }
        Race { on: false, laps: if np > 2300 { 2 } else { 3 }, cars: Vec::new(), player: 0, phase: Phase::Off, t: 0.0, done_t: 0.0, note: String::new(), pos: 0, order: Vec::new(), finish_pos: 0, rlearn, shown: false }
    }
}

fn mk_car(g: &mut Game, r: Option<usize>, k: usize, lat: f64) -> RCar {
    let form = 0.98 + g.rng.f() * 0.04; let mist_cd = 5.0 + g.rng.f() * 5.0; let react = 0.12 + g.rng.f() * 0.4;
    RCar { r, car: Car::new(), prog: k as f64 - g.np as f64, last_idx: k, i: k, lat, v: 0.0, lane: lat, lane_t: lat, pass_t: 3.0, cap: 99.0, pv: 1.0, boost: 0.0, form, mist: 0.0, mist_cd,
        stuck_t: 0.0, lap_t0: 0.0, laps_done: 0, last: None, best: None, finished: false, finish_t: 0.0, react, tag_x: -99.0, tag_y: -99.0, tag_on: false, err: 0.0, trust: 1.0, off_at: None, resets: 0 }
}
/// rcPlace: on the road at index i, lat metres right of the centre line, optionally already rolling
fn rc_place(c: &mut Car, rt: &Rt, w: &World, i: usize, lat: f64, speed: f64) {
    Game::place_raw(c, rt, w, i); let tn = rt.tan[i]; c.x += -tn[1] * lat; c.z += tn[0] * lat; c.y = w.height(c.x, c.z) + 0.075;
    if speed > 0.0 { c.vx = tn[0] * speed; c.vz = tn[1] * speed; c.gear = if speed > 20.0 { 3.0 } else { 2.0 }; c.rpm = 4300.0; for wh in c.wh.iter_mut() { wh.w = speed / super::TIRE_R; } }
}

pub fn start(g: &mut Game, w: &World) {
    let np = g.np; g.race.on = true; g.race.phase = Phase::Grid; g.race.t = 0.0; g.race.done_t = 0.0; g.race.note.clear(); g.race.cars.clear(); g.go = 4.5;
    let slot = |s: usize| -> (usize, f64) { let row = s >> 1; let col = s & 1; ((np * 4 - 9 - row * 11 - col * 5) % np, if col == 1 { 1.55 } else { -1.55 }) };
    let mut order: Vec<usize> = (0..5).collect(); order.sort_by(|a, b| RIVALS[*b].pace.partial_cmp(&RIVALS[*a].pace).unwrap());
    const PS: usize = 3;
    { let (k, lat) = slot(PS); g.place_at(w, k); let tn = g.rt.tan[k]; g.car.x += -tn[1] * lat; g.car.z += tn[0] * lat; g.car.y = w.height(g.car.x, g.car.z) + 0.075; }
    let mut ri = 0;
    for s in 0..6 {
        let (k, lat) = slot(s);
        if s == PS { let c = mk_car(g, None, k, lat); g.race.player = g.race.cars.len(); g.race.cars.push(c); continue; }
        let mut c = mk_car(g, Some(order[ri]), k, lat); ri += 1; rc_place(&mut c.car, &g.rt, w, k, lat, 0.0); g.race.cars.push(c);
    }
}
pub fn stop(g: &mut Game) { g.race.on = false; g.race.phase = Phase::Off; g.race.cars.clear(); }

fn state<'a>(g: &'a Game, k: usize) -> &'a Car { if g.race.cars[k].is_player() { &g.car } else { &g.race.cars[k].car } }

/// the drivers' heads: who is ahead, can I pass, who is alongside, when do I make a mistake
fn plan(g: &mut Game, w: &World, dt: f64) {
    let n = g.race.cars.len(); let np = g.np;
    for k in 0..n { let st = *state(g, k); let i = path_idx(w, st.x, st.z); let p = g.rt.path[i]; let tn = g.rt.tan[i];
        let c = &mut g.race.cars[k]; c.i = i; c.lat = (st.x - p[0]) * -tn[1] + (st.z - p[1]) * tn[0]; c.v = (st.vx * tn[0] + st.vz * tn[1]).max(0.0); }
    let pprog = g.race.cars[g.race.player].prog; let skill = g.ps.race_skill; let br = 6.5 * g.tune.mu_road;
    for k in 0..n {
        if g.race.cars[k].is_player() { continue; }
        let (cprog, clat, cv, ci, clane, aggr) = { let c = &g.race.cars[k]; (c.prog, c.lat, c.v, c.i, c.lane, RIVALS[c.r.unwrap()].aggr) };
        let gap = pprog - cprog; let band = clamp(gap / 1100.0, -0.07, 0.07);
        let pv = { let c = &g.race.cars[k]; RIVALS[c.r.unwrap()].pace * skill * (1.0 + band) * c.form };
        let boost = clamp((gap - 60.0) / 900.0, 0.0, 0.22);
        let mut block: Option<usize> = None; let mut bd = 99.0; let mut side = 0.0;
        for o in 0..n { if o == k { continue; } let oc = &g.race.cars[o]; let dp = oc.prog - cprog; let dl = oc.lat - clat;
            if dp > 0.0 && dp < 60.0 && dl.abs() < 2.1 + if oc.v < cv - 6.0 { 1.0 } else { 0.0 } && dp < bd { bd = dp; block = Some(o); }
            if dp.abs() < 5.5 && dl.abs() < 2.5 { side += if dl > 0.0 { -1.0 } else { 1.0 }; } }
        let rprof_ci = g.rt.rprof[ci]; let vc25 = g.rt.vcurve[(ci + 25) % np];
        let c = &mut g.race.cars[k]; c.pv = pv; c.boost = boost; c.cap = 99.0;
        if let Some(b) = block {
            let (blat, bv) = { let bc = &g.race.cars[b]; (bc.lat, bc.v) }; let c = &mut g.race.cars[k];
            let room = |sg: f64| -> Option<f64> { let t = clamp(blat + sg * 2.45, -2.35, 2.35); if (t - blat).abs() > 1.95 { Some(t) } else { None } };
            let keen = aggr > 0.6 || cv - bv > 1.2 || bv < rprof_ci * pv * 0.8;
            let mut t = None; if keen && bd < 24.0 && vc25 > 13.0 { let pref = if clane >= blat { 1.0 } else { -1.0 }; t = room(pref); if t.is_none() { t = room(-pref); } }
            if let Some(t) = t { c.lane_t = t; c.pass_t = 2.2; c.cap = if (blat - clat).abs() > 1.5 { 99.0 } else { (bv * bv + 2.0 * br * (bd - 6.0).max(0.0)).sqrt() + 2.0 }; }
            else { c.cap = (bv * bv + 2.0 * br * (bd - 7.5).max(0.0)).sqrt(); }
        } else { c.pass_t -= dt; if c.pass_t <= 0.0 { c.lane_t = 0.0; } }
        let c = &mut g.race.cars[k];
        if side != 0.0 { c.lane_t = clamp(c.lane_t + side * 0.9, -2.35, 2.35); }
        let ls = if c.pass_t > 0.0 { 2.4 } else { 1.6 }; c.lane += clamp(c.lane_t - c.lane, -ls * dt, ls * dt);
        c.mist_cd -= dt; c.mist = (c.mist - dt).max(0.0);
        if c.mist_cd <= 0.0 && g.rt.vcurve[(ci + 45) % np] < 18.0 && g.rt.vcurve[ci] > 24.0 { c.mist_cd = 7.0; let cons = RIVALS[c.r.unwrap()].cons; let q = g.rng.f(); let c = &mut g.race.cars[k]; if q < (1.0 - cons) * 0.5 { c.mist = 1.7; } }
    }
}

fn rival_input(g: &mut Game, w: &World, k: usize) -> Input {
    let np = g.np; let rt = &g.rt; let race_t = g.race.t; let phase = g.race.phase; let time = g.time;
    let c = &mut g.race.cars[k]; let s = c.car;
    if phase == Phase::Grid || race_t < c.react { return Input { steer: 0.0, throttle: 0.0, brake: 0.0, hb: true }; }
    let i = path_idx(w, s.x, s.z); let u = s.u.max(0.0); let ld = 7.0 + u * 0.42; let kk = (i + (ld + 0.5).floor() as usize) % np;
    let lat = clamp(c.lane + rt.rline[kk] * (1.0 - (c.lane.abs() / 2.2).min(1.0)), -1.9, 1.9); let p = rt.path[kk]; let tn = rt.tan[kk];
    { let p0 = rt.path[i]; let t0 = rt.tan[i]; let now = (s.x - p0[0]) * -t0[1] + (s.z - p0[1]) * t0[0]; c.err = (now - lat).abs(); }
    let (dx, dz) = (p[0] - tn[1] * lat - s.x, p[1] + tn[0] * lat - s.z); let sp = s.vx.hypot(s.vz); let vh = if sp > 2.0 { s.vx.atan2(s.vz) } else { s.h };
    let al = wrap_a(dx.atan2(dz) - vh); let r_want = 2.0 * sp.max(1.0) * al.sin() / ld;
    let delta = wrap_a(vh - s.h) + (2.0 * 2.5 * al.sin() / ld).atan() - (s.r - r_want) * 0.12 + if c.mist > 0.0 { (time * 6.0 + c.last_idx as f64).sin() * 0.04 } else { 0.0 };
    let q = (i + 4) % np; let mut vt = rt.rprof[q] * g.race.rlearn[q / 10] * c.pv * (1.0 + c.boost * 0.5); if c.mist > 0.0 { vt *= 1.1; }
    c.trust = clamp(c.trust + ((if c.err > 1.3 || s.vl.abs() > 2.6 { 0.82 } else { 1.0 }) - c.trust) * 0.05, 0.8, 1.0);
    vt = (vt * c.trust * clamp(1.12 - c.err * 0.09, 0.72, 1.0)).min(c.cap);
    let slide_cut = clamp(1.0 - (s.vl.abs() - 1.5) / 3.0, 0.15, 1.0);
    let lock = g.tune.lock / (1.0 + u / g.tune.lock_fall);
    Input { steer: clamp(delta / lock, -1.0, 1.0), throttle: clamp((vt - u) * 0.5, 0.0, 1.0) * slide_cut, brake: clamp((u - vt - 0.8) * 0.45, 0.0, 1.0), hb: false }
}

fn learn(g: &mut Game, i: usize) {
    let np = g.np as i64; let mut d = -70; while d <= 10 { let b = (((i as i64 + d + np) % np) / 10) as usize; let rl = &mut g.race.rlearn[b]; *rl = (*rl * if d > -40 { 0.93 } else { 0.96 }).max(0.72); d += 10; }
    let s: String = g.race.rlearn.iter().map(|v| { let q = format!("{:02}", ((v - 0.7) * 330.0 + 0.5).floor() as i64); q[q.len() - 2..].to_string() }).collect();
    let k = format!("fogwood-learn-{}", g.rt.map); g.ps.saves.set(&k, s);
}

/// beached, stuck, wrong way or on its roof: back on the road a little behind
fn recover(g: &mut Game, w: &World, k: usize, dt: f64) {
    if g.race.phase == Phase::Grid { return; }
    let np = g.np; let s = g.race.cars[k].car; let i = path_idx(w, s.x, s.z); let pd = w.path_dist(s.x, s.z); let sp = s.vx.hypot(s.vz); let tn = g.rt.tan[i];
    let fwd = s.h.sin() * tn[0] + s.h.cos() * tn[1];
    let c = &mut g.race.cars[k];
    if pd < ROAD + 1.0 { c.off_at = Some(i); }
    let off = pd > ROAD + 5.0; let bad = off || (sp < 1.5 && g.race.t > c.react + 2.0) || s.roll.abs() > 0.9 || s.pitch.abs() > 0.9 || (fwd < -0.2 && sp < 8.0);
    c.stuck_t = if bad { c.stuck_t + dt } else { (c.stuck_t - dt * 2.0).max(0.0) };
    if c.stuck_t > if off { 1.6 } else { 2.6 } {
        c.stuck_t = 0.0; let off_at = c.off_at.unwrap_or(i);
        if off { learn(g, off_at); }
        let kk = (i + np - 6) % np; let mut lat = 0.0;
        for o in 0..g.race.cars.len() { if o == k { continue; } let st = state(g, o); if (st.x - g.rt.path[kk][0]).hypot(st.z - g.rt.path[kk][1]) < 6.0 { lat = if lat != 0.0 { -lat } else { 2.0 }; } }
        let speed = (g.rt.rprof[kk] * 0.6).min(16.0);
        let mut car = g.race.cars[k].car; rc_place(&mut car, &g.rt, w, kk, lat, speed);
        let c = &mut g.race.cars[k]; c.car = car; c.lane = lat; c.lane_t = lat; c.resets += 1;
    }
}

fn contacts(g: &mut Game) {
    let n = g.race.cars.len(); let (m, iz) = (1180.0, g.tune.iz);
    let mut st: Vec<Car> = (0..n).map(|k| *state(g, k)).collect();
    let pl = g.race.player;
    for a in 0..n { for b in (a + 1)..n {
        let (dx0, dz0) = (st[a].x - st[b].x, st[a].z - st[b].z); if dx0 * dx0 + dz0 * dz0 > 30.0 || (st[a].y - st[b].y).abs() > 1.6 { continue; }
        let (sa, ca, sb, cb) = (st[a].h.sin(), st[a].h.cos(), st[b].h.sin(), st[b].h.cos()); let mut hit: f64 = 0.0;
        for oa in [1.3, 0.0, -1.3] { for ob in [1.3, 0.0, -1.3] {
            let (ax, az, bx, bz) = (st[a].x + sa * oa, st[a].z + ca * oa, st[b].x + sb * ob, st[b].z + cb * ob); let (dx, dz) = (ax - bx, az - bz); let rr = 1.72; let d2 = dx * dx + dz * dz;
            if d2 >= rr * rr || d2 < 1e-6 { continue; }
            let d = d2.sqrt(); let (nx, nz) = (dx / d, dz / d); let pen = (rr - d) * 0.5;
            st[a].x += nx * pen; st[a].z += nz * pen; st[b].x -= nx * pen; st[b].z -= nz * pen;
            let (cx, cz) = ((ax + bx) / 2.0, (az + bz) / 2.0); let (pax, paz, pbx, pbz) = (cx - st[a].x, cz - st[a].z, cx - st[b].x, cz - st[b].z);
            let (vax, vaz, vbx, vbz) = (st[a].vx + st[a].r * paz, st[a].vz - st[a].r * pax, st[b].vx + st[b].r * pbz, st[b].vz - st[b].r * pbx);
            let vn = (vax - vbx) * nx + (vaz - vbz) * nz; if vn >= 0.0 { continue; }
            let (ra, rb) = (paz * nx - pax * nz, pbz * nx - pbx * nz); let jn = -1.3 * vn / (2.0 / m + ra * ra / iz + rb * rb / iz);
            st[a].vx += jn * nx / m; st[a].vz += jn * nz / m; st[a].r += jn * ra / iz; st[b].vx -= jn * nx / m; st[b].vz -= jn * nz / m; st[b].r -= jn * rb / iz;
            let (tx, tz) = (-nz, nx); let vt = (vax - vbx) * tx + (vaz - vbz) * tz; let (rta, rtb) = (paz * tx - pax * tz, pbz * tx - pbx * tz);
            let jt = clamp(-vt / (2.0 / m + rta * rta / iz + rtb * rtb / iz), -0.35 * jn, 0.35 * jn);
            st[a].vx += jt * tx / m; st[a].vz += jt * tz / m; st[a].r += jt * rta / iz; st[b].vx -= jt * tx / m; st[b].vz -= jt * tz / m; st[b].r -= jt * rtb / iz;
            hit = hit.max(-vn); } }
        if hit > 0.3 { for x in [a, b] { st[x].impact = st[x].impact.max(hit); st[x].hb_t = st[x].hb_t.max(clamp((hit - 2.0) / 6.0, 0.0, 0.6)); }
            if a == pl || b == pl { g.car_hit = g.car_hit.max(hit); } }
    } }
    for k in 0..n { if k == pl { g.car = st[k]; } else { g.race.cars[k].car = st[k]; } }
}

fn progress(g: &mut Game, w: &World) {
    let n = g.race.cars.len(); let np = g.np as i64;
    for k in 0..n { let st = *state(g, k); let i = path_idx(w, st.x, st.z) as i64;
        let (phase, rt_t, laps) = (g.race.phase, g.race.t, g.race.laps);
        let c = &mut g.race.cars[k]; let mut d = i - c.last_idx as i64; if d < -np / 2 { d += np; } if d > np / 2 { d -= np; } c.prog += d as f64; c.last_idx = i as usize;
        if phase == Phase::Grid || c.finished { continue; }
        let lap = (c.prog / np as f64).floor() as i64; let mut msg = None;
        if lap > c.laps_done && c.prog > 0.0 { let lt = rt_t - c.lap_t0; c.last = Some(lt); if c.best.map(|b| lt < b).unwrap_or(true) { c.best = Some(lt); } c.lap_t0 = rt_t; c.laps_done = lap;
            if c.r.is_none() && lap < laps { msg = Some(format!("{}{}", if lap == laps - 1 { "FINAL LAP  ".to_string() } else { format!("LAP {}/{}  ", lap + 1, laps) }, super::fmt(Some(lt)))); } }
        let mut fin = false; if c.prog >= (laps * np) as f64 { c.finished = true; c.finish_t = rt_t; fin = c.r.is_none(); }
        if let Some(m) = msg { g.flash_msg(&m, 2.5); }
        if fin { finish(g); }
    }
    let mut order: Vec<usize> = (0..n).collect();
    { let cs = &g.race.cars; order.sort_by(|&a, &b| { let (x, y) = (&cs[a], &cs[b]);
        if x.finished && y.finished { x.finish_t.partial_cmp(&y.finish_t).unwrap() } else if x.finished { std::cmp::Ordering::Less } else if y.finished { std::cmp::Ordering::Greater } else { y.prog.partial_cmp(&x.prog).unwrap() } }); }
    let pos = order.iter().position(|&k| k == g.race.player).unwrap_or(0) + 1;
    if g.race.phase == Phase::Race && g.race.pos > 0 && pos < g.race.pos { g.flash_msg(&format!("P{}", pos), 1.0); }
    g.race.pos = pos; g.race.order = order;
}

fn finish(g: &mut Game) {
    g.race.phase = Phase::Done; g.race.done_t = 0.0;
    let pos = g.race.cars.iter().filter(|c| c.finished).count();
    let np = g.np as f64; let laps = g.race.laps as f64;
    let next = if g.race.cars.iter().filter(|c| !c.is_player() && c.finished).count() == 0 {
        let mut best = 1e9; for c in &g.race.cars { if !c.is_player() { best = f64::min(best, (laps * np - c.prog) / c.v.max(15.0)); } } best } else { 0.0 };
    let d = if pos == 1 { if next > 10.0 { 0.05 } else if next > 4.0 { 0.03 } else { 0.015 } } else if pos <= 3 { 0.0 } else if pos <= 5 { -0.015 } else { -0.03 };
    g.ps.race_skill = clamp(g.ps.race_skill + d, 0.84, 1.1); let v = format!("{:.3}", g.ps.race_skill); g.ps.saves.set("fogwood-skill", v);
    g.race.note = if d > 0.0 { "RIVALS WILL PUSH HARDER".into() } else if d < 0.0 { "RIVALS WILL EASE OFF".into() } else { String::new() }; g.race.finish_pos = pos;
    let m = if pos == 1 { "YOU WIN!".to_string() } else { format!("FINISHED P{}", pos) }; g.flash_msg(&m, 3.0);
}

/// (car index, time, done) best first
pub fn results(g: &Game) -> Vec<(usize, f64, bool)> {
    let np = g.np as f64; let laps = g.race.laps as f64;
    let mut v: Vec<(usize, f64, bool)> = g.race.cars.iter().enumerate().map(|(k, c)| (k, if c.finished { c.finish_t } else { g.race.t + (laps * np - c.prog) / c.v.max(18.0) }, c.finished)).collect();
    v.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap()); v
}

pub fn step(g: &mut Game, w: &World, obs: &Obstacles, dt: f64) {
    if g.go > 0.0 { g.go -= dt; }
    if g.race.phase == Phase::Grid && g.go <= 0.9 { g.race.phase = Phase::Race; }
    if g.race.phase != Phase::Grid { g.race.t += dt; }
    plan(g, w, dt);
    for k in 0..g.race.cars.len() {
        if g.race.cars[k].is_player() { continue; }
        let bo = g.race.cars[k].boost; let mut t: Tune = g.tune;
        t.power = g.tune.power * (1.0 + bo); t.mu_road = g.tune.mu_road * (RT_MU + bo * 0.4); t.mu_grass = g.tune.mu_grass * (1.0 + bo * 0.4);
        t.a_grip = g.tune.a_grip * RT_GRIP; t.a_yaw_cap = g.tune.a_yaw_cap * RT_YAW;
        let inp = rival_input(g, w, k); let mut car = g.race.cars[k].car; car.update(w, obs, &t, dt, &inp); g.race.cars[k].car = car;
        recover(g, w, k, dt);
    }
    contacts(g); progress(g, w);
    if g.race.phase == Phase::Done { g.race.done_t += dt; }
    let i = path_idx(w, g.car.x, g.car.z); let tn = g.rt.tan[i]; let fwd = g.car.h.sin() * tn[0] + g.car.h.cos() * tn[1];
    g.wrong_t = if fwd < -0.4 && g.car.u.abs() > 4.0 { g.wrong_t + dt } else { (g.wrong_t - dt * 2.0).max(0.0) };
    if g.msg_t > 0.0 { g.msg_t -= dt; }
}

/// rivals on screen: every instanced part posed through the car puppet, tinted per team; shadows; name tags
pub fn gfx(g: &mut Game, r: &mut Renderer, w: &World) {
    let nr = g.race.cars.iter().filter(|c| !c.is_player()).count();
    if !g.race.shown { for p in &g.rt.rg_parts { r.set_vis(p.obj, true); } if let Some(s) = g.rt.rg_shadow { r.set_vis(s, true); } g.race.shown = true; }
    let mut mats: Vec<Vec<f32>> = g.rt.rg_parts.iter().map(|p| Vec::with_capacity(nr * p.src.len() * 16)).collect(); let mut sh: Vec<f32> = Vec::with_capacity(nr * 16);
    for k in 0..g.race.cars.len() {
        if g.race.cars[k].is_player() { continue; }
        let s = g.race.cars[k].car; let pose = g.car_pose(&s);
        for (pi, p) in g.rt.rg_parts.iter().enumerate() { for (node, local) in &p.src { let nw = pose.get(node).copied().unwrap_or(ID); mats[pi].extend_from_slice(&to_f32(&mul(&nw, local))); } }
        let gh = w.height(s.x, s.z); let m = compose([s.x, gh + 0.05, s.z], &euler(0.0, s.h, 0.0, false), [1.0 + clamp(s.y - gh - 0.08, 0.0, 4.0) * 0.12; 3]); sh.extend_from_slice(&to_f32(&m));
        let v = project(&g.view, &g.proj, [s.x, s.y + 2.1, s.z]); let d = (s.x - g.cam_pos[0]).hypot(s.z - g.cam_pos[2]);
        let c = &mut g.race.cars[k]; c.tag_on = v[2] < 1.0 && v[0].abs() < 1.0 && v[1].abs() < 1.0 && d < 70.0 && d > 3.0;
        c.tag_x = (v[0] * 0.5 + 0.5) * g.lw as f64; c.tag_y = (0.5 - v[1] * 0.5) * g.lh as f64;
    }
    for (pi, p) in g.rt.rg_parts.iter().enumerate() { r.set_instances(p.obj, (mats[pi].len() / 16) as i32, &mats[pi]); }
    if let Some(s) = g.rt.rg_shadow { r.set_instances(s, (sh.len() / 16) as i32, &sh); }
}
/// hide the rivals again (back on the menu)
pub fn hide(g: &mut Game, r: &mut Renderer) {
    if !g.race.shown { return; } for p in &g.rt.rg_parts { r.set_vis(p.obj, false); } if let Some(s) = g.rt.rg_shadow { r.set_vis(s, false); } g.race.shown = false;
}

/// rival engines: pitched from revs, doppler-shifted, panned, fading with distance (message 8: per rival rate, hi gain, gain, lowpass, pan)
pub fn audio(g: &mut Game) {
    let fwd = [-g.cam_rot[8], -g.cam_rot[9], -g.cam_rot[10]]; let (rx, rz) = (-fwd[2], fwd[0]); let rl = { let l = rx.hypot(rz); if l == 0.0 { 1.0 } else { l } };
    let cp = g.cam_pos; let mut v: Vec<f32> = vec![8.0, 0.0]; let mut n = 0;
    for c in &g.race.cars { let Some(ri) = c.r else { continue }; let s = &c.car;
        let (dx, dz) = (s.x - cp[0], s.z - cp[2]); let d = (dx * dx + dz * dz + (s.y - cp[1]).powi(2)).sqrt().max(1.0);
        let nn = clamp((s.rpm - 900.0) / 6400.0, 0.0, 1.05); let vr = ((s.vx - g.car.vx) * dx + (s.vz - g.car.vz) * dz) / d;
        let rate = (0.95 + nn * 2.6) * ENG_PITCH[ri] * clamp(343.0 / (343.0 + vr), 0.8, 1.25);
        let gain = if g.ps.sound_on { 0.85 / (1.0 + (d / 17.0) * (d / 17.0)) * (0.6 + 0.4 * nn) } else { 0.0 };
        let lp = (1200.0 + nn * 3000.0 - d * 12.0).max(500.0); let pan = clamp((dx * rx + dz * rz) / rl / d, -1.0, 1.0) * 0.85;
        v.extend_from_slice(&[ri as f32, rate as f32, (0.15 + nn * 0.5) as f32, gain as f32, lp as f32, pan as f32]); n += 1; }
    v[1] = n as f32; g.audio_msg(&v);
}
