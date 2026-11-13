//! Vehicle dynamics: a straight port of the JavaScript model (same order of operations, same constants),
//! so a car driven by the native engine behaves exactly like the browser build.
use crate::noise::{clamp, js_round, sgn, vnoise, wrap_a};
use crate::world::{Map, World, ROAD, WL};
use std::collections::HashMap;

#[inline] fn sin(x: f64) -> f64 { libm::sin(x) }
#[inline] fn cos(x: f64) -> f64 { libm::cos(x) }
#[inline] fn atan(x: f64) -> f64 { libm::atan(x) }
#[inline] fn atan2(y: f64, x: f64) -> f64 { libm::atan2(y, x) }
#[inline] fn tan(x: f64) -> f64 { libm::tan(x) }
#[inline] fn exp(x: f64) -> f64 { libm::exp(x) }

/// Math.hypot exactly as V8 computes it (scaled, Kahan-compensated sum).
#[inline]
pub fn js_hypot(a: f64, b: f64) -> f64 {
    let (aa, ab) = (a.abs(), b.abs());
    if aa.is_infinite() || ab.is_infinite() { return f64::INFINITY; }
    if aa.is_nan() || ab.is_nan() { return f64::NAN; }
    let max = aa.max(ab);
    if max == 0.0 { return 0.0; }
    let (mut sum, mut comp) = (0.0f64, 0.0f64);
    for v in [aa, ab] { let n = v / max; let summand = n * n - comp; let pre = sum + summand; comp = (pre - sum) - summand; sum = pre; }
    sum.sqrt() * max
}

pub const NPARAM: usize = 33;
/// Order of the parameter block sent by the game (TUNE..., SUSP.K, SUSP.C, SUSP.KARB, VEH.Iz).
pub const PARAM_NAMES: [&str; NPARAM] = ["lock", "lockFall", "steerRate", "returnRate", "rateFall", "assist", "csExtra", "csRate", "latKeep",
    "gripF", "gripR", "By", "Cy", "muRoad", "muGrass", "split", "power", "brake", "brakeRear", "abs", "hb", "arcade", "aGrip", "aYaw",
    "aBrake", "aAccel", "tc", "aHbYaw", "aYawCap", "K", "C", "KARB", "Iz"];

#[derive(Clone, Copy, Default, Debug)]
pub struct Tune {
    pub lock: f64, pub lock_fall: f64, pub steer_rate: f64, pub return_rate: f64, pub rate_fall: f64, pub assist: f64, pub cs_extra: f64, pub cs_rate: f64,
    pub lat_keep: f64, pub grip_f: f64, pub grip_r: f64, pub by: f64, pub cy: f64, pub mu_road: f64, pub mu_grass: f64, pub split: f64, pub power: f64,
    pub brake: f64, pub brake_rear: f64, pub abs: f64, pub hb: f64, pub arcade: f64, pub a_grip: f64, pub a_yaw: f64, pub a_brake: f64, pub a_accel: f64,
    pub tc: f64, pub a_hb_yaw: f64, pub a_yaw_cap: f64, pub k: f64, pub c: f64, pub karb: f64, pub iz: f64,
}
impl Tune {
    pub fn from_slice(p: &[f64]) -> Tune {
        let g = |i: usize| p.get(i).copied().unwrap_or(0.0);
        Tune { lock: g(0), lock_fall: g(1), steer_rate: g(2), return_rate: g(3), rate_fall: g(4), assist: g(5), cs_extra: g(6), cs_rate: g(7), lat_keep: g(8),
            grip_f: g(9), grip_r: g(10), by: g(11), cy: g(12), mu_road: g(13), mu_grass: g(14), split: g(15), power: g(16), brake: g(17), brake_rear: g(18),
            abs: g(19), hb: g(20), arcade: g(21), a_grip: g(22), a_yaw: g(23), a_brake: g(24), a_accel: g(25), tc: g(26), a_hb_yaw: g(27), a_yaw_cap: g(28),
            k: g(29), c: g(30), karb: g(31), iz: g(32) }
    }
}

const WB_F: f64 = 1.22; const WB_R: f64 = -1.28; const TRACK: f64 = 0.75; const TIRE_R: f64 = 0.31;
const M: f64 = 1180.0; const HCG: f64 = 0.5; const IW: f64 = 1.25; const DRAG: f64 = 0.44; const VEH_L: f64 = WB_F - WB_R;
const GEARS: [f64; 5] = [3.15, 2.1, 1.55, 1.21, 0.98]; const FINAL: f64 = 4.1; const REV: f64 = 3.3;
const TQ_R: [f64; 9] = [800.0, 1500.0, 2500.0, 3500.0, 4500.0, 5500.0, 6500.0, 7200.0, 7600.0];
const TQ_N: [f64; 9] = [170.0, 290.0, 410.0, 520.0, 575.0, 560.0, 505.0, 420.0, 0.0];
const REST: f64 = 0.21; const STOP: f64 = 0.33; const IYY: f64 = 1500.0; const IXX: f64 = 460.0;
const PI: f64 = std::f64::consts::PI;

fn torque_base(rpm: f64) -> f64 {
    if rpm <= TQ_R[0] { return TQ_N[0]; }
    for i in 1..TQ_R.len() { if rpm < TQ_R[i] { let f = (rpm - TQ_R[i - 1]) / (TQ_R[i] - TQ_R[i - 1]); return TQ_N[i - 1] + (TQ_N[i] - TQ_N[i - 1]) * f; } }
    0.0
}

#[derive(Clone, Copy, Default, Debug)]
pub struct Wheel { pub lx: f64, pub lz: f64, pub f: bool, pub w: f64, pub ang: f64, pub comp: f64, pub d: f64, pub fz: f64, pub slide: f64,
    pub road: bool, pub bank: bool, pub mu: f64, pub rr: f64, pub wx: f64, pub wz: f64, pub hg: f64 }

#[derive(Clone, Copy, Default, Debug)]
pub struct Car {
    pub x: f64, pub z: f64, pub h: f64, pub vx: f64, pub vz: f64, pub r: f64, pub steer: f64, pub gear: f64, pub rpm: f64, pub shift_t: f64, pub rev_t: f64,
    pub y: f64, pub vy: f64, pub pitch: f64, pub roll: f64, pub pr: f64, pub rr: f64, pub ax_f: f64, pub ay_f: f64, pub u: f64, pub vl: f64, pub slide: f64,
    pub on_road: bool, pub contacts: f64, pub air_t: f64, pub land_v: f64, pub last_air: f64, pub landed: bool, pub impact: f64, pub clutch: f64,
    pub p_t: f64, pub p_b: f64, pub hb_t: f64,
    pub wh: [Wheel; 4],
}
pub const NSTATE: usize = 33; pub const NWHEEL: usize = 13;

#[derive(Clone, Copy, Default, Debug)]
pub struct Input { pub throttle: f64, pub brake: f64, pub hb: bool, pub steer: f64 }

#[derive(Clone, Copy)]
struct Ctl { throttle: f64, brake: f64, hb: bool, steer: f64 }

#[derive(Clone, Copy, Debug)]
pub struct Obstacle { pub x: f64, pub z: f64, pub r: f64 }

pub struct Obstacles { cells: HashMap<i64, Vec<Obstacle>> }
const OC: f64 = 8.0;
#[inline] fn okey(i: f64, j: f64) -> i64 { (i * 4096.0 + j) as i64 }
impl Obstacles {
    pub fn new(list: &[Obstacle]) -> Obstacles {
        let mut cells: HashMap<i64, Vec<Obstacle>> = HashMap::new();
        for o in list { cells.entry(okey((o.x / OC).floor(), (o.z / OC).floor())).or_default().push(*o); }
        Obstacles { cells }
    }
}

impl Car {
    pub fn new() -> Car {
        let mut c = Car { gear: 1.0, rpm: 900.0, on_road: true, contacts: 4.0, clutch: 1.0, ..Default::default() };
        let pos = [(TRACK, WB_F, true), (-TRACK, WB_F, true), (TRACK, WB_R, false), (-TRACK, WB_R, false)];
        for (i, &(lx, lz, f)) in pos.iter().enumerate() { c.wh[i] = Wheel { lx, lz, f, mu: 0.86, rr: 0.022, road: true, ..Default::default() }; }
        c
    }

    pub fn to_vec(&self) -> Vec<f64> {
        let b = |v: bool| if v { 1.0 } else { 0.0 };
        let mut o = vec![self.x, self.z, self.h, self.vx, self.vz, self.r, self.steer, self.gear, self.rpm, self.shift_t, self.rev_t, self.y, self.vy,
            self.pitch, self.roll, self.pr, self.rr, self.ax_f, self.ay_f, self.u, self.vl, self.slide, b(self.on_road), self.contacts, self.air_t,
            self.land_v, self.last_air, b(self.landed), self.impact, self.clutch, self.p_t, self.p_b, self.hb_t];
        for w in &self.wh { o.extend_from_slice(&[w.w, w.ang, w.comp, w.d, w.fz, w.slide, b(w.road), b(w.bank), w.mu, w.rr, w.wx, w.wz, w.hg]); }
        o
    }
    pub fn from_slice(v: &[f64]) -> Car {
        let mut c = Car::new(); let g = |i: usize| v.get(i).copied().unwrap_or(0.0);
        c.x = g(0); c.z = g(1); c.h = g(2); c.vx = g(3); c.vz = g(4); c.r = g(5); c.steer = g(6); c.gear = g(7); c.rpm = g(8); c.shift_t = g(9);
        c.rev_t = g(10); c.y = g(11); c.vy = g(12); c.pitch = g(13); c.roll = g(14); c.pr = g(15); c.rr = g(16); c.ax_f = g(17); c.ay_f = g(18);
        c.u = g(19); c.vl = g(20); c.slide = g(21); c.on_road = g(22) != 0.0; c.contacts = g(23); c.air_t = g(24); c.land_v = g(25); c.last_air = g(26);
        c.landed = g(27) != 0.0; c.impact = g(28); c.clutch = g(29); c.p_t = g(30); c.p_b = g(31); c.hb_t = g(32);
        for i in 0..4 { let b = NSTATE + i * NWHEEL; let w = &mut c.wh[i];
            w.w = g(b); w.ang = g(b + 1); w.comp = g(b + 2); w.d = g(b + 3); w.fz = g(b + 4); w.slide = g(b + 5); w.road = g(b + 6) != 0.0;
            w.bank = g(b + 7) != 0.0; w.mu = g(b + 8); w.rr = g(b + 9); w.wx = g(b + 10); w.wz = g(b + 11); w.hg = g(b + 12); }
        c
    }

    fn steer_lock(t: &Tune, u: f64) -> f64 { t.lock / (1.0 + u.max(0.0) / t.lock_fall) }

    /// One frame: pedal smoothing, gearbox, steering rack, n fixed sub-steps, collisions. Returns true on an upshift.
    pub fn update(&mut self, world: &World, obs: &Obstacles, t: &Tune, dt: f64, input: &Input) -> bool {
        let mut shifted = false;
        self.p_t += clamp(input.throttle - self.p_t, -dt * 10.0, dt * 9.0);
        self.p_b += clamp(input.brake - self.p_b, -dt * 10.0, dt * 10.0);
        let (mut thr, mut brk) = (self.p_t, self.p_b);
        if self.gear < 0.0 { thr = self.p_b; brk = self.p_t; }
        if self.gear == 1.0 && self.u < 0.4 && input.brake != 0.0 && input.throttle == 0.0 { self.rev_t += dt; if self.rev_t > 0.5 { self.gear = -1.0; self.rev_t = 0.0; } } else { self.rev_t = 0.0; }
        if self.gear < 0.0 && input.throttle != 0.0 && self.u > -0.6 { self.gear = 1.0; }
        self.shift_t = (self.shift_t - dt).max(0.0);
        if self.gear > 0.0 && self.shift_t <= 0.0 && self.contacts > 0.0 {
            if self.rpm > 7050.0 && self.gear < 5.0 && self.clutch == 1.0 { self.gear += 1.0; self.shift_t = 0.14; shifted = true; }
            else if self.gear > 1.0 && self.rpm < 3100.0 {
                let gi = self.gear as usize; let nr = self.rpm * GEARS[gi - 2] / GEARS[gi - 1];
                if nr < 6600.0 { self.gear -= 1.0; self.shift_t = 0.1; }
            }
        }
        let spd0 = js_hypot(self.vx, self.vz);
        let slip_a = if spd0 > 3.0 { clamp(wrap_a(atan2(self.vx, self.vz) - self.h), -0.6, 0.6) } else { 0.0 };
        let counter = input.steer != 0.0 && sgn(input.steer) == sgn(slip_a) && slip_a.abs() > 0.05;
        let lock = if counter { Self::steer_lock(t, self.u).max(slip_a.abs() + t.cs_extra) } else { Self::steer_lock(t, self.u) };
        let target = clamp(clamp(input.steer, -1.0, 1.0) * lock + t.assist * slip_a, -0.8, 0.8);
        let rate = if counter { t.steer_rate * t.cs_rate } else { (if input.steer != 0.0 { t.steer_rate } else { t.return_rate }) / (1.0 + self.u.max(0.0) / t.rate_fall) };
        self.steer += clamp(target - self.steer, -rate * dt, rate * dt);
        let gx = (world.height(self.x + 0.6, self.z) - world.height(self.x - 0.6, self.z)) / 1.2;
        let gz = (world.height(self.x, self.z + 0.6) - world.height(self.x, self.z - 0.6)) / 1.2;
        let ctl = Ctl { throttle: thr, brake: brk, hb: input.hb, steer: input.steer };
        let n = clamp(js_round(dt * 360.0), 1.0, 24.0) as usize;
        let sdt = dt / n as f64;
        for _ in 0..n { self.step(world, t, sdt, &ctl, gx, gz); }
        self.collide(world, obs, t);
        for w in self.wh.iter_mut() { w.ang += w.w * dt; }
        shifted
    }

    fn step(&mut self, world: &World, t: &Tune, dt: f64, ctl: &Ctl, gx: f64, gz: f64) {
        let snow = world.map == Map::Snow; let can = world.map == Map::Canyon; let lak = world.map == Map::Lakes;
        let sh = sin(self.h); let ch = cos(self.h); let lxw = ch; let lzw = -sh;
        let u = self.vx * sh + self.vz * ch; let vl = self.vx * lxw + self.vz * lzw;
        let (m, iz, r_, g) = (M, t.iz, TIRE_R, 9.81);
        let sp = sin(self.pitch); let sr = sin(self.roll);
        let (mut fsum, mut contacts, mut road_n) = (0.0f64, 0.0f64, 0);
        for w in self.wh.iter_mut() {
            let wx = self.x + ch * w.lx + sh * w.lz; let wz = self.z - sh * w.lx + ch * w.lz; w.wx = wx; w.wz = wz;
            let wpd = world.path_dist(wx, wz); w.road = wpd < ROAD + 0.25; w.bank = snow && !w.road && wpd < ROAD + 1.8; if w.road { road_n += 1; }
            let hg = world.height(wx, wz) + (vnoise(wx * 0.45 + 3.1, wz * 0.45) - 0.5) * (if w.road { 0.05 } else { 0.16 });
            let d = self.y + w.lz * sp + w.lx * sr - hg; let comp = REST - d; let rate = (comp - w.comp) / dt;
            w.comp = comp; w.d = d; w.hg = hg;
            let mut f = 0.0;
            if comp > 0.0 { f = t.k * comp + t.c * rate; if comp > STOP { f += t.k * 10.0 * (comp - STOP) + t.c * 2.0 * rate.max(0.0); } }
            let wet = lak && hg < WL - 0.05;
            w.fz = f; w.mu = if wet { 0.5 } else if w.road { t.mu_road } else { t.mu_grass };
            w.rr = if wet { 0.45 } else if w.road { if snow { 0.03 } else if can { 0.026 } else { 0.02 } } else if w.bank { 0.2 } else if snow { 0.11 } else if can { 0.13 } else { 0.06 };
        }
        for (ia, ib) in [(0usize, 1usize), (2, 3)] {
            let ca = self.wh[ia].comp; let cb = self.wh[ib].comp;
            if ca > 0.0 && cb > 0.0 { let fa = t.karb * (ca - cb); self.wh[ia].fz += fa; self.wh[ib].fz -= fa; }
        }
        for w in self.wh.iter_mut() { w.fz = js_max0(w.fz); fsum += w.fz; if w.fz > 0.0 { contacts += 1.0; } }
        self.on_road = road_n >= 2; self.contacts = contacts;
        // drivetrain
        let ratio = if self.gear > 0.0 { GEARS[self.gear as usize - 1] * FINAL } else { -REV * FINAL };
        let w_avg = (self.wh[0].w + self.wh[1].w + self.wh[2].w + self.wh[3].w) / 4.0;
        let rpm_w = (w_avg * ratio).abs() * 60.0 / (2.0 * PI);
        let thr = if self.shift_t > 0.0 { 0.0 } else { ctl.throttle };
        let (mut te, mut coupled) = (0.0f64, false);
        if rpm_w < 1150.0 && ctl.throttle > 0.02 {
            self.rpm += ((1300.0 + ctl.throttle * 3400.0) - self.rpm) * (dt * 8.0).min(1.0); self.rpm = self.rpm.max(rpm_w);
            te = torque_base(self.rpm) * t.power * thr * (0.5 + rpm_w / 1150.0).min(1.0); self.clutch = 0.5;
        } else if rpm_w < 900.0 { self.rpm += (900.0 - self.rpm) * (dt * 5.0).min(1.0); self.clutch = 0.0; }
        else { coupled = true; self.rpm = rpm_w; self.clutch = 1.0; te = if thr > 0.0 { torque_base(rpm_w) * t.power * thr } else { -(18.0 + rpm_w * 0.008) }; }
        if self.rpm > 7450.0 && te > 0.0 { te = 0.0; }
        let tw = te * ratio * 0.88; let ieng = if coupled { 0.17 * ratio * ratio / 4.0 } else { 0.0 };
        // tyres
        let (mut sum_f, mut sum_l, mut tq, mut t_p, mut t_r, mut slide) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
        let steer = self.steer; let r = self.r;
        for w in self.wh.iter_mut() {
            let fz = w.fz; let d = if w.f { steer } else { 0.0 }; let cd = cos(d); let sd = sin(d);
            let v_lat_w = vl + r * w.lz; let v_fwd_w = u - r * w.lx;
            let v_long = v_lat_w * sd + v_fwd_w * cd; let v_side = v_lat_w * cd - v_fwd_w * sd; let den = v_long.abs().max(3.0);
            let kap = clamp((w.w * r_ - v_long) / den, -1.6, 1.6); let alp_ = atan2(v_side, den);
            let dd = w.mu * fz * (if w.f { t.grip_f } else { t.grip_r }); let bx = 10.0; let cx = 1.35;
            let atx = atan(bx * kap); let aty = atan(t.by * alp_);
            let mut fx = dd * sin(cx * atx); let mut fy = -dd * sin(t.cy * aty);
            let mut sc = 1.0; let mag = js_hypot(fx, fy);
            if mag > dd && mag > 0.0 {
                let s0 = dd / mag; let fy0 = fy.abs() * s0; let fy_k = fy.abs().min(dd * 0.97);
                let fyv = fy0 + (fy_k - fy0) * t.lat_keep; let room = (dd * dd - fyv * fyv).max(0.0).sqrt(); let fx0 = fx;
                fy = sgn(fy) * fyv; fx = clamp(fx, -room, room); sc = if fx0 != 0.0 { (fx / fx0).abs() } else { 1.0 };
            }
            let bk = bx * kap;
            let k = js_max0(dd * cx * cos(cx * atx) * bx / (1.0 + bk * bk) * r_ / den * sc);
            let inertia = IW + ieng;
            let mut td = tw * (if w.f { t.split } else { 1.0 - t.split }) / 2.0;
            if t.tc > 0.0 && fz > 0.0 && td > 0.0 { td = td.min(dd * r_ * (1.05 + (1.0 - t.tc) * 3.0)); }
            let mut wn = w.w + dt * (td - fx * r_) / (inertia + dt * r_ * k);
            let mut tbr = ctl.brake * t.brake * (if w.f { 1.0 } else { t.brake_rear });
            if t.abs > 0.0 && kap < -t.abs && v_long.abs() > 1.5 && fz > 0.0 { tbr *= 0.25; }
            let tb = tbr + (if ctl.hb && !w.f { t.hb } else { 0.0 }) + w.rr * fz * r_;
            let dw = tb / inertia * dt; wn = if wn.abs() <= dw { 0.0 } else { wn - sgn(wn) * dw };
            w.w = wn;
            let flat = fx * sd + fy * cd; let ffwd = fx * cd - fy * sd;
            sum_f += ffwd; sum_l += flat; tq += w.lz * flat - w.lx * ffwd;
            t_p += fz * w.lz; t_r += fz * w.lx;
            w.slide = if fz > 0.0 { js_hypot(w.w * r_ - v_long, v_side) } else { 0.0 }; slide += w.slide;
        }
        t_p += sum_f * HCG; t_r += sum_l * HCG;
        // planar motion
        let spd = js_hypot(self.vx, self.vz); let drag_k = DRAG * spd / m; let a_f = sum_f / m; let a_l = sum_l / m; let cf = clamp(fsum / (m * g), 0.0, 1.5);
        self.vx += (a_f * sh + a_l * lxw - drag_k * self.vx - g * gx * cf) * dt;
        self.vz += (a_f * ch + a_l * lzw - drag_k * self.vz - g * gz * cf) * dt;
        self.r += tq / iz * dt;
        // arcade layer
        let ar = t.arcade;
        if ar > 0.0 {
            let gr = contacts / 4.0; self.hb_t = if ctl.hb { 0.6 } else { (self.hb_t - dt).max(0.0) };
            let drift = if ctl.hb { 0.0 } else { 1.0 - 0.8 * (self.hb_t / 0.6) };
            let mut uu = self.vx * sh + self.vz * ch; let mut vv = self.vx * lxw + self.vz * lzw; let u0 = uu;
            vv *= exp(-t.a_grip * ar * gr * drift * dt);
            if ctl.brake > 0.0 && uu > 0.5 && self.gear > 0.0 { uu = (uu - t.a_brake * ar * ctl.brake * gr * dt).max(0.0); }
            if ctl.throttle > 0.0 && self.gear > 0.0 && self.shift_t <= 0.0 { uu += t.a_accel * ar * ctl.throttle * gr * (1.0 - uu / 50.0).max(0.0) * dt; }
            self.vx = uu * sh + vv * lxw; self.vz = uu * ch + vv * lzw;
            if gr > 0.0 { for w in self.wh.iter_mut() { if w.fz > 0.0 { w.w += (uu - u0) / r_; } } }
            let max_r = t.a_yaw_cap * t.mu_road * g / uu.abs().max(3.0);
            let r_des = clamp(uu * tan(self.steer) / VEH_L, -max_r, max_r);
            self.r += (r_des - self.r) * (1.0 - exp(-t.a_yaw * ar * gr * drift * dt));
            if ctl.hb && gr > 0.0 && uu.abs() > 4.0 { self.r += ctl.steer * t.a_hb_yaw * ar * gr * dt; }
            if contacts == 0.0 { self.pr += (-self.pitch * 3.0 - self.pr * 1.6) * ar * dt; self.rr += (-self.roll * 4.0 - self.rr * 2.0) * ar * dt; self.r += ctl.steer * 1.4 * ar * dt; }
        }
        self.h += self.r * dt; self.x += self.vx * dt; self.z += self.vz * dt;
        // heave, pitch, roll
        if contacts > 0.0 && self.air_t > 0.12 { self.land_v = self.land_v.min(self.vy); self.last_air = self.air_t; self.landed = true; }
        if contacts == 0.0 { self.air_t += dt; } else { self.air_t = 0.0; }
        self.vy += (fsum / m - g) * dt; self.y += self.vy * dt;
        self.pr += t_p / IYY * dt; self.pitch += self.pr * dt;
        self.rr += t_r / IXX * dt; self.roll += self.rr * dt;
        if contacts == 0.0 { self.pr *= 1.0 - dt * 0.4; self.rr *= 1.0 - dt * 0.6; self.r *= 1.0 - dt * 0.3; }
        let mut worst = 0.0f64;
        let (sp2, sr2) = (sin(self.pitch), sin(self.roll));
        for w in &self.wh { let dd = self.y + w.lz * sp2 + w.lx * sr2 - w.hg; if dd < -0.2 { worst = worst.min(dd + 0.2); } }
        if worst < 0.0 { self.y -= worst; if self.vy < 0.0 { self.vy *= 0.3; } }
        self.pitch = clamp(self.pitch, -1.2, 1.2); self.roll = clamp(self.roll, -1.2, 1.2);
        let k2 = (dt * 9.0).min(1.0); self.ax_f += (a_f - self.ax_f) * k2; self.ay_f += (a_l - self.ay_f) * k2;
        self.u = u; self.vl = vl; self.slide = slide / 4.0;
    }

    fn collide(&mut self, world: &World, obs: &Obstacles, t: &Tune) {
        self.impact = 0.0;
        let sh = sin(self.h); let ch = cos(self.h); let m = M; let iz = t.iz;
        for off in [1.35, 0.0, -1.35] {
            let cr = 0.9; let cx = self.x + sh * off; let cz = self.z + ch * off; let bi = (cx / OC).floor(); let bj = (cz / OC).floor();
            for i in -1..=1 { for j in -1..=1 {
                let Some(l) = obs.cells.get(&okey(bi + i as f64, bj + j as f64)) else { continue };
                for o in l {
                    let dx = cx - o.x; let dz = cz - o.z; let rr = o.r + cr; let d2 = dx * dx + dz * dz; if d2 >= rr * rr || d2 < 1e-8 { continue; }
                    let d = d2.sqrt(); let nx = dx / d; let nz = dz / d; self.x += nx * (rr - d); self.z += nz * (rr - d);
                    let px = o.x + nx * o.r - self.x; let pz = o.z + nz * o.r - self.z;
                    let vpx = self.vx + self.r * pz; let vpz = self.vz - self.r * px; let vn = vpx * nx + vpz * nz; if vn >= 0.0 { continue; }
                    let crn = pz * nx - px * nz; let jn = -1.25 * vn / (1.0 / m + crn * crn / iz);
                    self.vx += jn * nx / m; self.vz += jn * nz / m; self.r += jn * crn / iz;
                    let tx = -nz; let tz = nx; let vt = vpx * tx + vpz * tz; let crt = pz * tx - px * tz;
                    let jt = clamp(-vt / (1.0 / m + crt * crt / iz), -0.45 * jn, 0.45 * jn); self.vx += jt * tx / m; self.vz += jt * tz / m; self.r += jt * crt / iz;
                    self.impact = self.impact.max(-vn);
                }
            } }
        }
        let gxh = world.gxh as f64; let gzh = world.gzh as f64;
        if self.x.abs() > gxh - 10.0 { self.impact = self.impact.max(self.vx.abs()); self.x = sgn(self.x) * (gxh - 10.0); self.vx *= -0.3; }
        if self.z.abs() > gzh - 10.0 { self.impact = self.impact.max(self.vz.abs()); self.z = sgn(self.z) * (gzh - 10.0); self.vz *= -0.3; }
    }
}

/// Math.max(0, v) with JS NaN semantics
#[inline] fn js_max0(v: f64) -> f64 { if v.is_nan() { v } else if v > 0.0 { v } else { 0.0 } }
