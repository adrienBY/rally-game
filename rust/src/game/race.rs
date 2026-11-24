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
/// how hard the AI cars drive (offline: the RIVALS setting, ADAPTIVE for NORMAL; online: the host's pick in the lobby)
pub const AI_EASY: u8 = 0; pub const AI_NORMAL: u8 = 1; pub const AI_HARD: u8 = 2; pub const AI_EXTREME: u8 = 3;
pub const AI_NAMES: [&str; 4] = ["EASY", "NORMAL", "HARD", "EXTREME"];
/// EXTREME's cars: corner speeds this much over the speed profile, on this much more grip, power, braking and acceleration,
/// a firmer hold on their line and a quicker turn-in than the other levels' cars; braking planned again for that speed and
/// grip, then practised on the stage (x_practice); no mistakes, no easing off when ahead, sharp starts, and Ember Peak's
/// blasts only shove them this much. (Measured on all eight stages, AI cars alone: 6-11% quicker per lap than HARD,
/// Prism Skyway ~4% as its gap jumps set the pace there.)
const X_SKILL: f64 = 1.18; const X_MU: f64 = 1.3; const X_POW: f64 = 1.3; const X_BRK: f64 = 1.3; const X_ACC: f64 = 1.3; const X_AGRIP: f64 = 1.5; const X_YAW: f64 = 1.25;
pub const X_KICK: f64 = 0.35;
/// EXTREME's braking (the page's aB, on the extra grip): for (m/s)² per path step
fn x_brake(g: &Game) -> f64 { 9.81 * g.base_tune.mu_road * 0.85 * 1.1 * RT_MU.min(1.0) * X_MU }
/// the speed plan braked for: nowhere faster than it can slow down from for what comes next
fn rebrake(v: &mut [f64], ab: f64) { let np = v.len(); for _ in 0..3 { for i in (0..np).rev() { v[i] = v[i].min((v[(i + 1) % np].powi(2) + 2.0 * ab).sqrt()); } } }
/// EXTREME's speed plan before practice: the profile's speeds scaled, then braked for again with the grip those cars have
/// (scaling the whole profile, as the other levels do, would scale its braking zones too: the cars would overshoot the bends)
fn x_profile(g: &Game) -> Vec<f64> { let mut v: Vec<f64> = g.rt.rprof.iter().map(|p| p * X_SKILL).collect(); rebrake(&mut v, x_brake(g)); v }
/// slow the plan for the stretch before path point i (a slip there), keeping it brakeable
/// (never below `floor` x the stage's profile)
fn x_slow(g: &mut Game, i: usize, f_near: f64, f_far: f64, floor: f64) {
    let ab = x_brake(g); let np = g.np; let v = &mut g.race.xprof;
    for d in 0..100 { let j = (i + np * 2 + 10 - d) % np; let f = if d < 60 { f_near } else { f_far }; v[j] = (v[j] * f).max(g.rt.rprof[j] * floor); } rebrake(v, ab);
}
/// EXTREME's practice laps, as its race loads: one car alone on the stage, on EXTREME's plan and a little quicker than it
/// races (margin). Wherever it would have needed a reset (off the road a while, into the lava, off the skyway, on its roof,
/// turned round) the plan slows the stretch before that spot, and the car goes on from just behind it; until clean laps on
/// the racing line and on both passing lanes (or 24 laps). So each stage gets a plan that suits it, with no tuning per
/// stage. Done a slice at a time (Race::xbudget physics steps per frame, through the grid countdown, carrying on into the
/// race if it must), and kept per stage in the saves, so the next EXTREME race there starts with it.
pub struct XPrac { car: Option<RCar>, laps: usize, fixes: usize, streak: usize, run: usize, last: usize, steps: usize, clean: bool, pub done: bool, ms: f64, off_t: f64 }
const XPLAN_VER: &str = "x3";
const X_PRAC_PACE: f64 = 1.03;
fn xplan_key(g: &Game) -> String { format!("fogwood-xplan-{}", g.rt.map) }
fn xlog() -> bool { std::env::var("FW_XLOG").is_ok() }
/// a race on EXTREME begins: its plan from the saves if this stage has been practised (with these settings), else practice
pub fn x_begin(g: &mut Game) {
    g.race.xprof = x_profile(g); g.race.xprac = None; let np = g.np;
    if std::env::var("FW_XFRESH").is_err() { if let Some(v) = g.ps.saves.get(&xplan_key(g)) {
        if let Some(rest) = v.strip_prefix(&format!("{}:{}:", XPLAN_VER, np)) { let f: Vec<f64> = rest.split(',').filter_map(|x| x.parse::<f64>().ok()).collect();
            if f.len() == (np + 9) / 10 { let ab = x_brake(g); for i in 0..np { g.race.xprof[i] = (g.race.xprof[i] * f[i / 10] / 1000.0).max(g.rt.rprof[i] * 0.7); }
                rebrake(&mut g.race.xprof, ab); if xlog() { eprintln!("EXTREME plan from the saves"); } return; } } } }
    g.race.xprac = Some(Box::new(XPrac { car: None, laps: 0, fixes: 0, streak: 0, run: 0, last: 0, steps: 0, clean: true, done: false, ms: 0.0, off_t: 0.0 }));
}
/// up to `budget` more practice steps; true once practice is over
pub fn x_practice(g: &mut Game, w: &World, obs: &Obstacles, budget: usize) -> bool {
    let Some(mut xp) = g.race.xprac.take() else { return true; };
    if xp.done { g.race.xprac = Some(xp); return true; }
    let t_start = web_time::Instant::now(); let np = g.np; let wk = w.road() / ROAD; let lanes = [0.0, 1.9 * wk, -1.9 * wk];
    let (phase, t0) = (g.race.phase, g.race.t); g.race.phase = Phase::Race; g.race.t = 1e6;   // (rival_input drives only in a race)
    let mut c = match xp.car.take() { Some(c) => c, None => {
        let mut c = mk_car(g, Some(0), 0, 0.0); c.pv = X_PRAC_PACE; c.form = 1.0; c.react = 0.0; c.mist_cd = 1e9;
        rc_place(&mut c.car, &g.rt, w, 0, 0.0, g.race.xprof[0].min(20.0)); xp.last = 0; xp.laps = 1; c } };
    let k = g.race.cars.len(); let tune = ai_tune(&g.base_tune, 0.0, AI_EXTREME); let dt = 1.0 / 60.0;
    for _ in 0..budget {
        g.race.cars.push(c);
        let inp = rival_input(g, w, k); c = g.race.cars.pop().unwrap(); let mut car = c.car; car.update(w, obs, &tune, dt, &inp); c.car = car; xp.steps += 1;
        let i = path_idx(w, car.x, car.z); let d = (i + np - xp.last) % np; if d < np / 4 { xp.run += d; } xp.last = i;
        let pd = w.path_dist(car.x, car.z); let tn = g.rt.tan[i]; let fwd = car.h.sin() * tn[0] + car.h.cos() * tn[1];
        let lava = g.vol && super::volc::in_canyon(&g.rt, w, car.x, car.y, car.z).is_some();
        let fell = g.pri && car.y < g.rt.roadh[i] - 6.0;
        // what a race would reset (off the road a while, in the lava, off the skyway, rolled, turned round), only sooner
        let off = pd > w.road() + 4.0 || fwd < -0.2; xp.off_t = if off { xp.off_t + dt } else { 0.0 };
        if xp.off_t > 0.5 || lava || fell || car.roll.abs() > 0.9 || car.pitch.abs() > 0.9 {
            if std::env::var("FW_XFIXLOG").is_ok() { eprintln!("XFIX lap {} lane {:.1} at {} off_t {:.2} lava {} fell {} roll {:.2} pitch {:.2} sp {:.1} air {:.2}", xp.laps, c.lane, i, xp.off_t, lava, fell, car.roll, car.pitch, car.vx.hypot(car.vz), car.air_t); }
            // (falling off Prism Skyway in the air: a gap jump, where too slow falls short too - no slower than the stage's own plan)
            let floor = if g.pri && car.air_t > 0.25 { 1.0 } else { 0.7 };
            xp.off_t = 0.0; xp.clean = false; xp.fixes += 1; x_slow(g, i, 0.95, 0.975, floor);
            let back = (i + np - 40) % np; rc_place(&mut c.car, &g.rt, w, back, 0.0, g.race.xprof[back] * 0.9); c.err = 0.0; c.trust = 1.0; xp.last = back; xp.run = xp.run.saturating_sub(40);
        }
        if xp.run >= np || xp.steps >= 60 * 300 {   // a lap: the next on the next lane
            xp.streak = if xp.clean { xp.streak + 1 } else { 0 };
            if xp.streak >= 3 || xp.laps >= 24 || xp.fixes > 400 { xp.done = true; break; }
            let lane = lanes[xp.laps % 3]; c.lane = lane; c.lane_t = lane; xp.laps += 1; xp.run = 0; xp.steps = 0; xp.clean = true;
        }
    }
    g.race.phase = phase; g.race.t = t0; xp.ms += t_start.elapsed().as_secs_f64() * 1000.0;
    if xp.done {
        // kept per stage: the plan as factors of the plain one, per 10 path points (the slowest in each)
        let base = x_profile(g); let f: Vec<String> = (0..(np + 9) / 10).map(|b| { let m = (b * 10..((b + 1) * 10).min(np)).map(|i| g.race.xprof[i] / base[i].max(0.1)).fold(1.0, f64::min); format!("{}", (m * 1000.0).round() as i64) }).collect();
        let key = xplan_key(g); g.ps.saves.set(&key, format!("{}:{}:{}", XPLAN_VER, np, f.join(",")));
        if xlog() { eprintln!("EXTREME practice: {} laps, {} fixes, {:.0} ms", xp.laps, xp.fixes, xp.ms); }
    } else { xp.car = Some(c); }
    let done = xp.done; g.race.xprac = Some(xp); done
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Phase { Off, Grid, Race, Done }

#[derive(Clone)]
pub struct RCar {
    pub r: Option<usize>, pub car: Car, pub prog: f64, pub last_idx: usize, pub i: usize, pub lat: f64, pub v: f64, pub lane: f64, pub lane_t: f64, pub pass_t: f64,
    pub cap: f64, pub pv: f64, pub boost: f64, pub form: f64, pub mist: f64, pub mist_cd: f64, pub stuck_t: f64, pub lap_t0: f64, pub laps_done: i64,
    pub last: Option<f64>, pub best: Option<f64>, pub finished: bool, pub finish_t: f64, pub react: f64, pub tag_x: f64, pub tag_y: f64, pub tag_on: bool,
    pub err: f64, pub trust: f64, pub off_at: Option<usize>, pub resets: i32,
    /// online: the peer driving this car (its colour still comes from `r`) and their name
    pub remote: Option<u32>, pub name: String,
    /// which body this car is drawn with (garage family)
    pub model: usize,
    /// online: the player dropped out (hidden, listed as OUT)
    pub left: bool,
    /// online: whether, for now, only its own phone moves this car (a contact its states already include: pushing it
    /// here too would make it twitch)
    pub owned: bool,
    /// online: how far ahead of its newest state this car is drawn (s): how much of it is prediction
    pub horizon: f32,
    /// online: one of the host's AI cars (its index in the start message); driven here on the host, shown from the
    /// host's states on the guests
    pub bot: Option<u8>,
    /// online, the host's AI cars: their contact counter (travels with their states: see net::smooth::epoch)
    pub epoch: u8,
    /// the livery it is drawn with (0: its body's own; another player's pick, or a duo's team livery)
    pub paint: usize,
    /// duos: its team
    pub team: Option<u8>,
    /// an AI car: what its driver did this frame (they travel with its states: net::smooth::Phys)
    pub inp: Input,
}
impl RCar {
    pub fn is_player(&self) -> bool { self.r.is_none() }
    /// driven by this phone's AI (not you, not another player)
    pub fn is_ai(&self) -> bool { self.r.is_some() && self.remote.is_none() }
    /// three-letter tag for standings and name tags
    pub fn tag(&self) -> String { if self.remote.is_some() && self.bot.is_none() { let t: String = self.name.chars().filter(|c| c.is_ascii_alphanumeric()).take(3).collect(); if t.is_empty() { "???".into() } else { t.to_uppercase() } } else { RIVALS[self.r.unwrap_or(0)].tag.to_string() } }
    /// name on the results board
    pub fn full_name(&self) -> String { if self.remote.is_some() && self.bot.is_none() { self.name.to_uppercase() } else { let nm = RIVALS[self.r.unwrap_or(0)].name; format!("{}. {}", &nm[..1], nm.split(' ').nth(1).unwrap_or("")) } }
}

pub struct Race {
    pub on: bool, pub laps: i64, pub cars: Vec<RCar>, pub player: usize, pub phase: Phase, pub t: f64, pub done_t: f64, pub note: String, pub pos: usize,
    pub order: Vec<usize>, pub finish_pos: usize, pub rlearn: Vec<f64>, pub shown: bool,
    /// online race: lights out at `go_at` on the host's clock
    pub online: bool, pub go_at: f64,
    /// online: pushes this phone gave cars other phones drive this frame (their id, which of our cars did it: BY_PLAYER or
    /// an AI car's index, [dvx, dvz, yaw rate, dx, dz], impact), sent to their owners
    pub hits: Vec<(u32, u8, [f64; 5], f64)>, pub me_id: u32,
    /// online: pushes other phones gave cars this phone drives, still to apply (our car, the pusher, push, impact), and our
    /// own car's contact counter (it travels with our states: see net::smooth::epoch)
    pub net_hits: Vec<(usize, usize, [f64; 5], f64)>, pub epoch: u8,
    /// online: when this phone last settled a contact between two cars itself, and when another phone last pushed one of
    /// ours over a contact with the other (race clock, by car pair)
    pub touch_t: std::collections::HashMap<(usize, usize), f64>, pub pushed_t: std::collections::HashMap<(usize, usize), f64>,
    /// the menu's attract-mode race (AI drives your car too; nothing it does is saved)
    pub demo: bool,
    /// online duos: teams of two, the best average time wins
    pub duos: bool,
    /// AI_EASY .. AI_EXTREME for this race, and EXTREME's speed plan (x_profile, built at its first step)
    pub ai: u8, pub xprof: Vec<f64>,
    /// EXTREME's practice laps while they last (x_practice)
    pub xprac: Option<Box<XPrac>>,
    /// practice steps per frame on the grid and once racing (the relay, which drives AI cars for many rooms, takes it slower)
    pub xbudget: (usize, usize),
}

impl Race {
    fn key(a: usize, b: usize) -> (usize, usize) { (a.min(b), a.max(b)) }
    /// when this phone last settled a contact between cars a and b itself
    pub fn touched(&self, a: usize, b: usize) -> f64 { self.touch_t.get(&Race::key(a, b)).copied().unwrap_or(-9.0) }
    /// a contact of car `ours` (driven here) with `other`: true if it is a new one (nothing between them for 0.3 s)
    fn new_contact(&self, ours: usize, other: usize) -> bool { let k = Race::key(ours, other); self.t - self.touch_t.get(&k).copied().unwrap_or(-9.0).max(self.pushed_t.get(&k).copied().unwrap_or(-9.0)) > 0.3 }
    /// moves on the contact counter of car `k` (driven here): the other phones then know its states include the contact
    pub fn tick_epoch(&mut self, k: usize) { if k == self.player { self.epoch = self.epoch.wrapping_add(1) & 15; } else { let c = &mut self.cars[k]; c.epoch = c.epoch.wrapping_add(1) & 15; } }
    pub fn new(rt: &Rt, ps: &Persist) -> Race {
        let np = rt.np; let n = (np + 9) / 10; let mut rlearn = vec![1.0; n];
        if let Some(v) = ps.saves.get(&format!("fogwood-learn-{}", rt.map)) { if v.len() == n * 2 { for i in 0..n { rlearn[i] = 0.7 + v[i * 2..i * 2 + 2].parse::<f64>().unwrap_or(99.0) / 330.0; } } }
        Race { on: false, laps: if np > 2300 { 2 } else { 3 }, cars: Vec::new(), player: 0, phase: Phase::Off, t: 0.0, done_t: 0.0, note: String::new(), pos: 0, order: Vec::new(), finish_pos: 0, rlearn, shown: false, online: false, go_at: 0.0, hits: Vec::new(), me_id: 0, net_hits: Vec::new(), epoch: 0, touch_t: Default::default(), pushed_t: Default::default(), demo: false, duos: false, ai: AI_NORMAL, xprof: Vec::new(), xprac: None, xbudget: (700, 150) }
    }
}

pub(crate) fn mk_car(g: &mut Game, r: Option<usize>, k: usize, lat: f64) -> RCar {
    let xt = g.race.ai == AI_EXTREME;
    let form = if xt { 1.0 + g.rng.f() * 0.02 } else { 0.98 + g.rng.f() * 0.04 }; let mist_cd = 5.0 + g.rng.f() * 5.0; let react = if xt { 0.06 + g.rng.f() * 0.08 } else { 0.12 + g.rng.f() * 0.4 };
    RCar { paint: 0, team: None, r, car: Car::new(), prog: k as f64 - g.np as f64, last_idx: k, i: k, lat, v: 0.0, lane: lat, lane_t: lat, pass_t: 3.0, cap: 99.0, pv: 1.0, boost: 0.0, form, mist: 0.0, mist_cd,
        stuck_t: 0.0, lap_t0: 0.0, laps_done: 0, last: None, best: None, finished: false, finish_t: 0.0, react, inp: Input::default(), tag_x: -99.0, tag_y: -99.0, tag_on: false, err: 0.0, trust: 1.0, off_at: None, resets: 0, remote: None, name: String::new(), left: false, owned: false, horizon: 0.0, bot: None, epoch: 0,
        model: match r { Some(i) => super::cars::rival_model(i), None => g.ps.car } }
}
/// rcPlace: on the road at index i, lat metres right of the centre line, optionally already rolling
pub(crate) fn rc_place(c: &mut Car, rt: &Rt, w: &World, i: usize, lat: f64, speed: f64) {
    Game::place_raw(c, rt, w, i); let tn = rt.tan[i]; c.x += -tn[1] * lat; c.z += tn[0] * lat; c.y = w.height(c.x, c.z) + 0.075; for wh in c.wh.iter_mut() { wh.hg = c.y - 0.075; }
    if speed > 0.0 { c.vx = tn[0] * speed; c.vz = tn[1] * speed; c.gear = if speed > 20.0 { 3.0 } else { 2.0 }; c.rpm = 4300.0; for wh in c.wh.iter_mut() { wh.w = speed / super::TIRE_R; } }
}

/// grid slot s: (path index, metres right of the centre line)
pub(crate) fn slot(np: usize, s: usize) -> (usize, f64) { let row = s >> 1; let col = s & 1; ((np * 4 - 9 - row * 11 - col * 5) % np, if col == 1 { 1.55 } else { -1.55 }) }
pub fn start(g: &mut Game, w: &World) {
    g.race.ai = g.ps.rivals.clamp(0, 3) as u8; g.race.xprof.clear();
    let np = g.np; g.race.on = true; g.race.phase = Phase::Grid; g.race.t = 0.0; g.race.done_t = 0.0; g.race.note.clear(); g.race.cars.clear(); g.go = 4.5; g.race.laps = g.ps.race_laps.clamp(1, 5) as i64;
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
/// the tune an AI car drives on: the stage's, with the rivals' grip and the catch-up boost (0: none; other phones predict
/// it without)
/// the AI cars' tune (also what the phones and the relay use to guess their moves): the stage tune with more grip, plus
/// its catch-up boost, plus EXTREME's extra grip and power
pub fn ai_tune(b: &Tune, bo: f64, ai: u8) -> Tune {
    let mut t: Tune = *b; t.power = b.power * (1.0 + bo); t.mu_road = b.mu_road * (RT_MU + bo * 0.4); t.mu_grass = b.mu_grass * (1.0 + bo * 0.4);
    t.a_grip = b.a_grip * RT_GRIP; t.a_yaw_cap = b.a_yaw_cap * RT_YAW;
    if ai == AI_EXTREME { t.mu_road *= X_MU; t.mu_grass *= X_MU; t.power *= X_POW; t.brake *= X_BRK; t.a_brake *= X_BRK; t.a_accel *= X_ACC; t.a_grip *= X_AGRIP; t.a_yaw_cap *= X_YAW; }
    t
}
pub fn stop(g: &mut Game) { g.race.duos = false; g.race.online = false; g.race.demo = false; g.race.on = false; g.race.phase = Phase::Off; g.race.cars.clear(); }

fn state<'a>(g: &'a Game, k: usize) -> &'a Car { if g.race.cars[k].is_player() { &g.car } else { &g.race.cars[k].car } }

/// the drivers' heads: who is ahead, can I pass, who is alongside, when do I make a mistake
fn plan(g: &mut Game, w: &World, dt: f64) {
    let wk = w.road() / ROAD;   // lanes spread over a wider road (prism)
    let n = g.race.cars.len(); let np = g.np;
    for k in 0..n { let st = *state(g, k); let i = path_idx(w, st.x, st.z); let p = g.rt.path[i]; let tn = g.rt.tan[i];
        let c = &mut g.race.cars[k]; c.i = i; c.lat = (st.x - p[0]) * -tn[1] + (st.z - p[1]) * tn[0]; c.v = (st.vx * tn[0] + st.vz * tn[1]).max(0.0); }
    let xt = g.race.ai == AI_EXTREME;
    let skill = match g.race.ai { AI_EASY => 0.87, AI_HARD => 1.07, AI_EXTREME => 1.0, _ => if g.race.online { 0.95 } else { g.ps.race_skill } };
    let pprog = g.race.cars[g.race.player].prog; let br = 6.5 * g.tune.mu_road;
    // online, the AI cars keep close to the players: each measures its gap to the nearest player still racing
    let humans: Vec<f64> = if g.race.online { g.race.cars.iter().filter(|c| c.bot.is_none() && !c.left && !c.finished).map(|c| c.prog).collect() } else { Vec::new() };
    for k in 0..n {
        if !g.race.cars[k].is_ai() { continue; }
        let (cprog, clat, cv, ci, clane, aggr) = { let c = &g.race.cars[k]; (c.prog, c.lat, c.v, c.i, c.lane, RIVALS[c.r.unwrap()].aggr) };
        let gap = if humans.is_empty() { pprog - cprog } else { humans.iter().map(|p| p - cprog).fold(f64::MAX, |m, d| if d.abs() < m.abs() { d } else { m }) };
        let band = clamp(gap / 1100.0, if xt { 0.0 } else { -0.07 }, 0.07);
        let band = if std::env::var("FW_NOBAND").is_ok() { 0.0 } else { band };   // test hook: raw pace
        // (EXTREME: the drivers' pace differences mostly gone)
        let pv = { let c = &g.race.cars[k]; let pace = RIVALS[c.r.unwrap()].pace; (if xt { 1.0 - (1.0 - pace) * 0.3 } else { pace }) * skill * (1.0 + band) * c.form };
        let boost = clamp((gap - 60.0) / 900.0, 0.0, 0.22);
        let mut block: Option<usize> = None; let mut bd = 99.0; let mut side = 0.0;
        for o in 0..n { if o == k { continue; } let oc = &g.race.cars[o]; let dp = oc.prog - cprog; let dl = oc.lat - clat;
            if dp > 0.0 && dp < 60.0 && dl.abs() < 2.1 + if oc.v < cv - 6.0 { 1.0 } else { 0.0 } && dp < bd { bd = dp; block = Some(o); }
            if dp.abs() < 5.5 && dl.abs() < 2.5 { side += if dl > 0.0 { -1.0 } else { 1.0 }; } }
        let rprof_ci = g.rt.rprof[ci]; let vc25 = g.rt.vcurve[(ci + 25) % np];
        let c = &mut g.race.cars[k]; c.pv = pv; c.boost = boost; c.cap = 99.0;
        if let Some(b) = block {
            let (blat, bv) = { let bc = &g.race.cars[b]; (bc.lat, bc.v) }; let c = &mut g.race.cars[k];
            let room = |sg: f64| -> Option<f64> { let t = clamp(blat + sg * 2.45, -2.35 * wk, 2.35 * wk); if (t - blat).abs() > 1.95 { Some(t) } else { None } };
            let keen = aggr > 0.6 || cv - bv > 1.2 || bv < rprof_ci * pv * 0.8;
            let mut t = None; if keen && bd < 24.0 && vc25 > 13.0 { let pref = if clane >= blat { 1.0 } else { -1.0 }; t = room(pref); if t.is_none() { t = room(-pref); } }
            if let Some(t) = t { c.lane_t = t; c.pass_t = 2.2; c.cap = if (blat - clat).abs() > 1.5 { 99.0 } else { (bv * bv + 2.0 * br * (bd - 6.0).max(0.0)).sqrt() + 2.0 }; }
            else { c.cap = (bv * bv + 2.0 * br * (bd - 7.5).max(0.0)).sqrt(); }
        } else { c.pass_t -= dt; if c.pass_t <= 0.0 { c.lane_t = 0.0; } }
        let c = &mut g.race.cars[k];
        if side != 0.0 { c.lane_t = clamp(c.lane_t + side * 0.9, -2.35 * wk, 2.35 * wk); }
        let ls = if c.pass_t > 0.0 { 2.4 } else { 1.6 }; c.lane += clamp(c.lane_t - c.lane, -ls * dt, ls * dt);
        c.mist_cd -= dt; c.mist = (c.mist - dt).max(0.0);
        if !xt && c.mist_cd <= 0.0 && g.rt.vcurve[(ci + 45) % np] < 18.0 && g.rt.vcurve[ci] > 24.0 { c.mist_cd = 7.0; let cons = RIVALS[c.r.unwrap()].cons; let q = g.rng.f(); let c = &mut g.race.cars[k]; if q < (1.0 - cons) * 0.5 { c.mist = 1.7; } }
    }
}

fn rival_input(g: &mut Game, w: &World, k: usize) -> Input {
    let np = g.np; let rt = &g.rt; let race_t = g.race.t; let phase = g.race.phase; let time = g.time;
    let c = &mut g.race.cars[k]; let s = c.car;
    if phase == Phase::Grid || race_t < c.react { return Input { steer: 0.0, throttle: 0.0, brake: 0.0, hb: true }; }
    let i = path_idx(w, s.x, s.z); let u = s.u.max(0.0); let ld = 7.0 + u * 0.42; let kk = (i + (ld + 0.5).floor() as usize) % np;
    let wk = w.road() / ROAD; let lat = clamp(c.lane + rt.rline[kk] * (1.0 - (c.lane.abs() / 2.2).min(1.0)), -1.9 * wk, 1.9 * wk);
    let lat = if rt.volc.is_some() { super::volc::dodge(rt, i).unwrap_or(lat) } else { lat };   // Ember Peak: down the middle of the bridges
    let p = rt.path[kk]; let tn = rt.tan[kk];
    { let p0 = rt.path[i]; let t0 = rt.tan[i]; let now = (s.x - p0[0]) * -t0[1] + (s.z - p0[1]) * t0[0]; c.err = (now - lat).abs(); }
    let (dx, dz) = (p[0] - tn[1] * lat - s.x, p[1] + tn[0] * lat - s.z); let sp = s.vx.hypot(s.vz); let vh = if sp > 2.0 { s.vx.atan2(s.vz) } else { s.h };
    let al = wrap_a(dx.atan2(dz) - vh); let r_want = 2.0 * sp.max(1.0) * al.sin() / ld;
    let delta = wrap_a(vh - s.h) + (2.0 * 2.5 * al.sin() / ld).atan() - (s.r - r_want) * 0.12 + if c.mist > 0.0 { (time * 6.0 + c.last_idx as f64).sin() * 0.04 } else { 0.0 };
    let q = (i + 4) % np;
    let prof = if g.race.xprof.len() == np && g.race.ai == AI_EXTREME { g.race.xprof[q] } else { rt.rprof[q] };   // EXTREME: its practised plan
    let rl = if g.race.ai == AI_EXTREME { 1.0 } else { g.race.rlearn[q / 10] };   // EXTREME's plan comes from its practice laps
    let mut vt = prof * rl * c.pv * (1.0 + c.boost * 0.5); if c.mist > 0.0 { vt *= 1.1; }
    c.trust = clamp(c.trust + ((if c.err > 1.3 || s.vl.abs() > 2.6 { 0.82 } else { 1.0 }) - c.trust) * 0.05, 0.8, 1.0);
    vt = (vt * c.trust * clamp(1.12 - c.err * 0.09, 0.72, 1.0)).min(c.cap);
    if let Some(c) = super::volc::bridge_cap(rt, i) { vt = vt.min(c); }   // Ember Peak: steady over the bridges
    let slide_cut = clamp(1.0 - (s.vl.abs() - 1.5) / 3.0, 0.15, 1.0);
    let lock = g.tune.lock / (1.0 + u / g.tune.lock_fall);
    let inp = Input { steer: clamp(delta / lock, -1.0, 1.0), throttle: clamp((vt - u) * 0.5, 0.0, 1.0) * slide_cut, brake: clamp((u - vt - 0.8) * 0.45, 0.0, 1.0), hb: false };
    inp
}

fn learn(g: &mut Game, i: usize) {
    // EXTREME: a slip in the race slows its own plan a little (this race only), not the saved learning of the other levels
    if g.race.ai == AI_EXTREME && g.race.xprof.len() == g.np { x_slow(g, i, 0.975, 0.99, 0.7); return; }
    let np = g.np as i64; let mut d = -70; while d <= 10 { let b = (((i as i64 + d + np) % np) / 10) as usize; let rl = &mut g.race.rlearn[b]; *rl = (*rl * if d > -40 { 0.93 } else { 0.96 }).max(0.72); d += 10; }
    let s: String = g.race.rlearn.iter().map(|v| { let q = format!("{:02}", ((v - 0.7) * 330.0 + 0.5).floor() as i64); q[q.len() - 2..].to_string() }).collect();
    let k = format!("fogwood-learn-{}", g.rt.map); g.ps.saves.set(&k, s);
}

/// beached, stuck, wrong way or on its roof: back on the road a little behind
fn recover(g: &mut Game, w: &World, k: usize, dt: f64) {
    if g.race.phase == Phase::Grid { return; }
    let np = g.np; let s = g.race.cars[k].car; let i = path_idx(w, s.x, s.z); let pd = w.path_dist(s.x, s.z); let sp = s.vx.hypot(s.vz); let tn = g.rt.tan[i];
    let fwd = s.h.sin() * tn[0] + s.h.cos() * tn[1];
    let fell = g.pri && s.y < g.rt.roadh[i] - 6.0;   // prism: dropped off the skyway
    let lava_kc = if g.vol { super::volc::in_canyon(&g.rt, w, s.x, s.y, s.z).map(|rv| super::volc::back_from(&g.rt, rv, i)) } else { None };   // Ember Peak: into a lava canyon
    let c = &mut g.race.cars[k];
    if pd < w.road() + 1.0 { c.off_at = Some(i); }
    let off = pd > w.road() + 5.0; let bad = off || (sp < 1.5 && g.race.t > c.react + 2.0) || s.roll.abs() > 0.9 || s.pitch.abs() > 0.9 || (fwd < -0.2 && sp < 8.0);
    c.stuck_t = if bad { c.stuck_t + dt } else { (c.stuck_t - dt * 2.0).max(0.0) };
    if lava_kc.is_some() { c.stuck_t = 9.0; }
    if fell { c.stuck_t = 9.0; if std::env::var("FW_FALLS").is_ok() { eprintln!("FALL car {} at {} off_at {:?} sp {:.1}", k, i, c.off_at, sp); } }
    if c.stuck_t > if off { 1.6 } else { 2.6 } {
        c.stuck_t = 0.0; let off_at = c.off_at.unwrap_or(i);
        if std::env::var("FW_RESETS").is_ok() { let lt = { let p0 = g.rt.path[i]; let t0 = g.rt.tan[i]; (s.x - p0[0]) * -t0[1] + (s.z - p0[1]) * t0[0] }; let near = (0..g.race.cars.len()).filter(|&o| o != k).map(|o| { let st = state(g, o); (st.x - s.x).hypot(st.z - s.z) }).fold(99.0, f64::min);
            eprintln!("RESET car {} at {} lava {} off {} sp {:.1} lat {:+.2} lane {:.2} roll {:.2} pitch {:.2} fwd {:.2} near {:.1} t {:.1}", k, i, lava_kc.is_some(), off, sp, lt, g.race.cars[k].lane, s.roll, s.pitch, fwd, near, g.race.t); }
        if off { learn(g, off_at); }
        let kk = lava_kc.unwrap_or((i + np - 6) % np); let mut lat = 0.0;
        for o in 0..g.race.cars.len() { if o == k { continue; } let st = state(g, o); if (st.x - g.rt.path[kk][0]).hypot(st.z - g.rt.path[kk][1]) < 6.0 { lat = if lat != 0.0 { -lat } else { 2.0 }; } }
        let speed = (g.rt.rprof[kk] * 0.6).min(16.0);
        let mut car = g.race.cars[k].car; rc_place(&mut car, &g.rt, w, kk, lat, speed);
        let c = &mut g.race.cars[k]; c.car = car; c.lane = lat; c.lane_t = lat; c.resets += 1;
    }
}

/// Two cars touching (three circles along each body): pushed apart, impulses with friction at the contact point.
/// Returns the closing speed of the hardest touch (0 if none). Used by races and by the netcode simulator.
pub fn bump(a: &mut Car, b: &mut Car, m: f64, iz: f64) -> f64 { bump_r(a, b, m, iz, 1.72, 1.3) }
/// the same with the cars' half-width `rr` (m) and the impulse factor `e` (1 + restitution: 1.3 bounces a little, 1.0 not at all)
pub fn bump_r(a: &mut Car, b: &mut Car, m: f64, iz: f64, rr: f64, e: f64) -> f64 {
    let (sa, ca, sb, cb) = (a.h.sin(), a.h.cos(), b.h.sin(), b.h.cos()); let mut hit: f64 = 0.0;
    for oa in [1.3, 0.0, -1.3] { for ob in [1.3, 0.0, -1.3] {
        let (ax, az, bx, bz) = (a.x + sa * oa, a.z + ca * oa, b.x + sb * ob, b.z + cb * ob); let (dx, dz) = (ax - bx, az - bz); let d2 = dx * dx + dz * dz;
        if d2 >= rr * rr || d2 < 1e-6 { continue; }
        let d = d2.sqrt(); let (nx, nz) = (dx / d, dz / d); let pen = (rr - d) * 0.5;
        a.x += nx * pen; a.z += nz * pen; b.x -= nx * pen; b.z -= nz * pen;
        let (cx, cz) = ((ax + bx) / 2.0, (az + bz) / 2.0); let (pax, paz, pbx, pbz) = (cx - a.x, cz - a.z, cx - b.x, cz - b.z);
        let (vax, vaz, vbx, vbz) = (a.vx + a.r * paz, a.vz - a.r * pax, b.vx + b.r * pbz, b.vz - b.r * pbx);
        let vn = (vax - vbx) * nx + (vaz - vbz) * nz; if vn >= 0.0 { continue; }
        let (ra, rb) = (paz * nx - pax * nz, pbz * nx - pbx * nz); let jn = -e * vn / (2.0 / m + ra * ra / iz + rb * rb / iz);
        a.vx += jn * nx / m; a.vz += jn * nz / m; a.r += jn * ra / iz; b.vx -= jn * nx / m; b.vz -= jn * nz / m; b.r -= jn * rb / iz;
        let (tx, tz) = (-nz, nx); let vt = (vax - vbx) * tx + (vaz - vbz) * tz; let (rta, rtb) = (paz * tx - pax * tz, pbz * tx - pbx * tz);
        let jt = clamp(-vt / (2.0 / m + rta * rta / iz + rtb * rtb / iz), -0.35 * jn, 0.35 * jn);
        a.vx += jt * tx / m; a.vz += jt * tz / m; a.r += jt * rta / iz; b.vx -= jt * tx / m; b.vz -= jt * tz / m; b.r -= jt * rtb / iz;
        hit = hit.max(-vn); } }
    hit
}

/// Contacts with a car another phone drives, tuned in net_contact (two phones door to door through 40-160 ms links):
/// its push is shown at SHOW of what this phone worked out (its own phone's states bring the rest, or correct it), which
/// halves the twitch of a car pushed here and then pulled about by its real states; and while it is drawn from
/// prediction it is met slightly narrower (MARGIN a second of prediction, at most MARGIN_MAX), so a car drawn a little
/// off its true place is bumped less often where it isn't. The push sent to its phone is the full one.
pub const SHOW: f64 = 0.5; const MARGIN: f64 = 0.25; const MARGIN_MAX: f64 = 0.25;

fn contacts(g: &mut Game) {
    let n = g.race.cars.len(); let (m, iz) = (1180.0, g.tune.iz);
    let mut st: Vec<Car> = (0..n).map(|k| *state(g, k)).collect();
    let pl = g.race.player;
    let online = g.race.online;
    // cars this phone drives: ours, and the AI cars when we are the host (offline, every car)
    let local: Vec<bool> = (0..n).map(|k| !online || k == pl || g.race.cars[k].is_ai()).collect();
    // pushes other phones gave cars we drive (contacts this phone did not see itself): applied here, after the physics
    // step like every other contact, so the crash damage and the knock effects see them
    for (k, from, d, imp) in std::mem::take(&mut g.race.net_hits) {
        if k >= n || from >= n || g.race.cars[from].left { continue; }
        let c = &mut st[k]; c.vx += d[0]; c.vz += d[1]; c.r += d[2];
        // the position part was their screen's overlap, already over by now: only a little of it, so nothing pops
        let s = (d[3].hypot(d[4]) / 0.25).max(1.0); c.x += d[3] * 0.5 / s; c.z += d[4] * 0.5 / s;
        if imp > 0.3 { c.impact = c.impact.max(imp); c.hb_t = c.hb_t.max(clamp((imp - 2.0) / 6.0, 0.0, 0.6)); if k == pl { g.car_hit = g.car_hit.max(imp); } }
        g.race.pushed_t.insert(Race::key(k, from), g.race.t);
    }
    for a in 0..n { for b in (a + 1)..n {
        // online: each phone settles the bumps of the cars it drives (the other phones settle theirs)
        if online && ((!local[a] && !local[b]) || g.race.cars[a].left || g.race.cars[b].left) { continue; }
        // with a car another phone drives, this phone settles the bump for its own car at once (no waiting); the other
        // car's push is shown here and sent to its phone, until that phone's states (which took it in) arrive
        let (sa0, sb0) = (st[a], st[b]);
        let (dx0, dz0) = (st[a].x - st[b].x, st[a].z - st[b].z); if dx0 * dx0 + dz0 * dz0 > 30.0 || (st[a].y - st[b].y).abs() > 1.6 { continue; }
        let h = |k: usize| if online && !local[k] { (g.race.cars[k].horizon as f64 * MARGIN).min(MARGIN_MAX) } else { 0.0 };
        let rr = 1.72 - h(a).max(h(b));
        let hit = { let (l, r) = st.split_at_mut(b); bump_r(&mut l[a], &mut r[0], m, iz, rr, 1.3) };
        if hit > 0.3 { for x in [a, b] { st[x].impact = st[x].impact.max(hit); st[x].hb_t = st[x].hb_t.max(clamp((hit - 2.0) / 6.0, 0.0, 0.6)); }
            if a == pl || b == pl { g.car_hit = g.car_hit.max(hit); } }
        if online { for (x, o, x0) in [(a, b, sa0), (b, a, sb0)] {
            if local[x] { continue; } let s1 = st[x];
            let d = [s1.vx - x0.vx, s1.vz - x0.vz, s1.r - x0.r, s1.x - x0.x, s1.z - x0.z];
            if d.iter().any(|v| v.abs() > 1e-6) { if let Some(id) = g.race.cars[x].remote {
                let by = if o == pl { crate::net::proto::BY_PLAYER } else { g.race.cars[o].bot.unwrap_or(crate::net::proto::BY_PLAYER) };
                g.race.hits.push((id, by, d, hit));
                if g.race.new_contact(o, x) { g.race.tick_epoch(o); } g.race.touch_t.insert(Race::key(o, x), g.race.t); }
                // shown here at SHOW of it
                let c = &mut st[x]; c.vx = x0.vx + d[0] * SHOW; c.vz = x0.vz + d[1] * SHOW; c.r = x0.r + d[2] * SHOW; c.x = x0.x + d[3] * SHOW; c.z = x0.z + d[4] * SHOW; }
        } }
    } }
    // a car whose own phone is already handling this contact is not moved here (it would jump back next frame)
    for k in 0..n { if k == pl { g.car = st[k]; } else if !(online && g.race.cars[k].owned) { g.race.cars[k].car = st[k]; } }
}

fn progress(g: &mut Game, w: &World) {
    let n = g.race.cars.len(); let np = g.np as i64;
    for k in 0..n { let st = *state(g, k); let i = path_idx(w, st.x, st.z) as i64;
        let (phase, rt_t, laps) = (g.race.phase, g.race.t, g.race.laps);
        if g.race.cars[k].left { continue; }
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
        let (xo, yo) = (x.left && !x.finished, y.left && !y.finished);
        if xo != yo { return if xo { std::cmp::Ordering::Greater } else { std::cmp::Ordering::Less }; }
        if x.finished && y.finished { x.finish_t.partial_cmp(&y.finish_t).unwrap() } else if x.finished { std::cmp::Ordering::Less } else if y.finished { std::cmp::Ordering::Greater } else { y.prog.partial_cmp(&x.prog).unwrap() } }); }
    let pos = order.iter().position(|&k| k == g.race.player).unwrap_or(0) + 1;
    if g.race.phase == Phase::Race && g.race.pos > 0 && pos < g.race.pos { g.flash_msg(&format!("P{}", pos), 1.0); }
    g.race.pos = pos; g.race.order = order;
}

fn finish(g: &mut Game) {
    g.race.phase = Phase::Done; g.race.done_t = 0.0;
    let pos = g.race.cars.iter().filter(|c| c.finished).count();
    if !g.race.demo { let mode = if g.race.duos { "duos" } else if g.race.online { "online" } else { "race" }; let t = g.race.cars[g.race.player].finish_t;
        g.stat("finish", &[("o", super::stats::js(mode)), ("t", format!("{:.3}", t)), ("r", pos.to_string()), ("k", g.race.cars.len().to_string())]); }
    let np = g.np as f64; let laps = g.race.laps as f64;
    let next = if g.race.cars.iter().filter(|c| !c.is_player() && c.finished).count() == 0 {
        let mut best = 1e9; for c in &g.race.cars { if !c.is_player() { best = f64::min(best, (laps * np - c.prog) / c.v.max(15.0)); } } best } else { 0.0 };
    let d = if pos == 1 { if next > 10.0 { 0.05 } else if next > 4.0 { 0.03 } else { 0.015 } } else if pos <= 3 { 0.0 } else if pos <= 5 { -0.015 } else { -0.03 };
    let d = if g.race.online { 0.0 } else { d };
    if !g.race.online && !g.race.demo && g.race.ai == AI_NORMAL { g.ps.race_skill = clamp(g.ps.race_skill + d, 0.84, 1.1); let v = format!("{:.3}", g.ps.race_skill); g.ps.saves.set("fogwood-skill", v); }
    g.race.note = if d > 0.0 { "RIVALS WILL PUSH HARDER".into() } else if d < 0.0 { "RIVALS WILL EASE OFF".into() } else { String::new() }; g.race.finish_pos = pos;
    let m = if pos == 1 { "YOU WIN!".to_string() } else { format!("FINISHED P{}", pos) }; g.flash_msg(&m, 3.0);
}

/// duos: (team, its cars, average time, both done) best first; a car still out on the stage counts with its projected time
pub fn team_results(g: &Game) -> Vec<(u8, Vec<usize>, f64, bool)> {
    let res = results(g); let mut teams: Vec<(u8, Vec<usize>, f64, bool)> = Vec::new();
    for (k, t, done) in res { let Some(tm) = g.race.cars[k].team else { continue };
        match teams.iter_mut().find(|x| x.0 == tm) { Some(x) => { x.1.push(k); x.2 += t; x.3 &= done; } None => teams.push((tm, vec![k], t, done)) } }
    for x in teams.iter_mut() { x.2 /= x.1.len() as f64; }
    teams.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap()); teams
}
pub const TEAM_NAMES: [&str; 6] = ["RED", "BLUE", "GREEN", "YELLOW", "PURPLE", "ORANGE"];
pub const TEAM_COLS: [[f64; 3]; 6] = [[0.76, 0.23, 0.17], [0.11, 0.37, 0.77], [0.18, 0.6, 0.31], [0.95, 0.76, 0.19], [0.48, 0.23, 0.76], [0.89, 0.4, 0.16]];

/// (car index, time, done) best first
pub fn results(g: &Game) -> Vec<(usize, f64, bool)> {
    let np = g.np as f64; let laps = g.race.laps as f64;
    let mut v: Vec<(usize, f64, bool)> = g.race.cars.iter().enumerate().map(|(k, c)| (k, if c.left && !c.finished { 1e9 } else if c.finished { c.finish_t } else { g.race.t + (laps * np - c.prog) / c.v.max(18.0) }, c.finished)).collect();
    v.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap()); v
}

pub fn step(g: &mut Game, w: &World, obs: &Obstacles, dt: f64) {
    if g.race.online { if let Some(hn) = g.race_clock.or_else(|| crate::net::with(|s| s.host_now())) { g.go = (g.race.go_at - hn + 0.9).max(0.0); if g.race.phase != Phase::Grid { g.race.t = (hn - g.race.go_at).max(0.0); } } }
    else if g.go > 0.0 { g.go -= dt; }
    if g.race.phase == Phase::Grid && g.go <= 0.9 { g.race.phase = Phase::Race; }
    if g.race.phase != Phase::Grid && !g.race.online { g.race.t += dt; }
    if g.race.ai == AI_EXTREME && g.race.cars.iter().any(|c| c.is_ai()) {   // (only where AI cars are driven)
        if g.race.xprof.len() != g.np { x_begin(g); }
        if g.race.xprac.as_ref().map(|x| !x.done).unwrap_or(false) { let b = if g.race.phase == Phase::Grid { g.race.xbudget.0 } else { g.race.xbudget.1 }; x_practice(g, w, obs, b); }
    }
    plan(g, w, dt);
    for k in 0..g.race.cars.len() {
        if !g.race.cars[k].is_ai() { continue; }
        // rivals drive on the stage tune (their pace is balanced for it), whatever car you picked
        let t = ai_tune(&g.base_tune, g.race.cars[k].boost, g.race.ai);
        let inp = rival_input(g, w, k); let mut car = g.race.cars[k].car; car.update(w, obs, &t, dt, &inp); g.race.cars[k].car = car; g.race.cars[k].inp = inp;
        recover(g, w, k, dt);
    }
    contacts(g); if g.tf.on { super::tilefall::step(g, w, dt); } else { progress(g, w); }
    if g.race.phase == Phase::Done { g.race.done_t += dt; }
    let i = path_idx(w, g.car.x, g.car.z); let tn = g.rt.tan[i]; let fwd = g.car.h.sin() * tn[0] + g.car.h.cos() * tn[1];
    g.wrong_t = if g.tf.on { 0.0 } else if fwd < -0.4 && g.car.u.abs() > 4.0 { g.wrong_t + dt } else { (g.wrong_t - dt * 2.0).max(0.0) };
    if g.msg_t > 0.0 { g.msg_t -= dt; }
}

/// rivals on screen: every instanced part posed through the car puppet; body parts only for the cars driving that body, tinted
/// per team (colours follow the cars, so they stay right whoever is on which body); shadows; name tags
pub fn gfx(g: &mut Game, r: &mut Renderer, w: &World) {
    let nr = g.race.cars.iter().filter(|c| !c.is_player()).count();
    if !g.race.shown { for p in &g.rt.rg_parts { r.set_vis(p.obj, true); } if let Some(s) = g.rt.rg_shadow { r.set_vis(s, true); } g.race.shown = true; }
    let mut mats: Vec<Vec<f32>> = g.rt.rg_parts.iter().map(|p| Vec::with_capacity(nr * p.src.len() * 16)).collect(); let mut sh: Vec<f32> = Vec::with_capacity(nr * 16);
    let mut cols: Vec<Vec<f32>> = g.rt.rg_parts.iter().map(|_| Vec::new()).collect();
    let nmodels = super::cars::CARS.len();
    for k in 0..g.race.cars.len() {
        if g.race.cars[k].is_player() { continue; }
        if g.race.cars[k].left { g.race.cars[k].tag_on = false; continue; }
        let s = g.race.cars[k].car; let pose = g.car_pose(&s); let model = g.race.cars[k].model % nmodels; let paint = g.race.cars[k].paint;
        // AI cars: their team's tint on the body's own paint; players' cars: their own livery as they see it
        let col = if g.race.cars[k].remote.is_some() && g.race.cars[k].bot.is_none() { [1.0; 3] } else { RIVALS[g.race.cars[k].r.unwrap_or(0) % RIVALS.len()].col };
        for (pi, p) in g.rt.rg_parts.iter().enumerate() {
            if let Some(m) = p.model { if m != model { continue; } }
            if p.paint && p.liv != paint { continue; }
            for (node, local) in &p.src { let nw = pose.get(node).copied().unwrap_or(ID); mats[pi].extend_from_slice(&to_f32(&mul(&nw, local))); }
            if p.tint && p.model.is_some() { cols[pi].extend_from_slice(&[col[0] as f32, col[1] as f32, col[2] as f32]); }
        }
        let gh = w.height(s.x, s.z); let m = compose([s.x, gh + 0.05, s.z], &euler(0.0, s.h, 0.0, false), [1.0 + clamp(s.y - gh - 0.08, 0.0, 4.0) * 0.12; 3]); sh.extend_from_slice(&to_f32(&m));
        let v = project(&g.view, &g.proj, [s.x, s.y + 2.1, s.z]); let d = (s.x - g.cam_pos[0]).hypot(s.z - g.cam_pos[2]);
        let c = &mut g.race.cars[k]; c.tag_on = v[2] < 1.0 && v[0].abs() < 1.0 && v[1].abs() < 1.0 && d < 70.0 && d > 3.0;
        c.tag_x = (v[0] * 0.5 + 0.5) * g.lw as f64; c.tag_y = (0.5 - v[1] * 0.5) * g.lh as f64;
    }
    // the tints only change when the field does: upload them then, not every frame
    if g.rg_cols.len() != cols.len() { g.rg_cols = vec![Vec::new(); cols.len()]; }
    for (pi, p) in g.rt.rg_parts.iter().enumerate() { r.set_instances(p.obj, (mats[pi].len() / 16) as i32, &mats[pi]);
        if !cols[pi].is_empty() && cols[pi] != g.rg_cols[pi] { r.set_instance_colors(p.obj, &cols[pi]); g.rg_cols[pi] = cols[pi].clone(); } }
    if let Some(s) = g.rt.rg_shadow { r.set_instances(s, (sh.len() / 16) as i32, &sh); }
}
/// hide the rivals again (back on the menu)
pub fn hide(g: &mut Game, r: &mut Renderer) {
    if !g.race.shown { return; } g.rg_cols.clear(); for p in &g.rt.rg_parts { r.set_vis(p.obj, false); } if let Some(s) = g.rt.rg_shadow { r.set_vis(s, false); } g.race.shown = false;
}

/// rival engines: pitched from revs, doppler-shifted, panned, fading with distance (message 8: per rival rate, hi gain, gain, lowpass, pan)
pub fn audio(g: &mut Game) {
    let fwd = [-g.cam_rot[8], -g.cam_rot[9], -g.cam_rot[10]]; let (rx, rz) = (-fwd[2], fwd[0]); let rl = { let l = rx.hypot(rz); if l == 0.0 { 1.0 } else { l } };
    let cp = g.cam_pos; let mut v: Vec<f32> = vec![8.0, 0.0]; let mut n = 0;
    // one voice per car (players and AI cars can share a rival's colour, so not one per rival); the mixer has eight
    for c in &g.race.cars { let Some(ri) = c.r else { continue }; let s = &c.car; let gone = c.left;
        let (dx, dz) = (s.x - cp[0], s.z - cp[2]); let d = (dx * dx + dz * dz + (s.y - cp[1]).powi(2)).sqrt().max(1.0);
        let nn = clamp((s.rpm - 900.0) / 6400.0, 0.0, 1.05); let vr = ((s.vx - g.car.vx) * dx + (s.vz - g.car.vz) * dz) / d;
        let rate = (0.95 + nn * 2.6) * ENG_PITCH[ri] * clamp(343.0 / (343.0 + vr), 0.8, 1.25);
        let gain = if g.ps.sound_on && !gone && !g.settings_paused() { g.ps.rvol as f64 / 10.0 * g.ps.vol as f64 / 10.0 * 0.85 / (1.0 + (d / 17.0) * (d / 17.0)) * (0.6 + 0.4 * nn) } else { 0.0 };
        let lp = (1200.0 + nn * 3000.0 - d * 12.0).max(500.0); let pan = clamp((dx * rx + dz * rz) / rl / d, -1.0, 1.0) * 0.85;
        if n >= 8 { break; } v.extend_from_slice(&[n as f32, rate as f32, (0.15 + nn * 0.5) as f32, gain as f32, lp as f32, pan as f32]); n += 1; }
    v[1] = n as f32; g.audio_msg(&v);
}
