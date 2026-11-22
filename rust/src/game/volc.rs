//! Ember Peak's hazards. Lava rivers run down the volcano in canyons; the road crosses each on a narrow bridge with no rails, and a car
//! that drops into a canyon is put back on the road before the bridge. The crater throws fireballs that land on
//! the road and throw cars about. Fireballs run on a shared clock (the wall clock, like the prism waves), so every phone in a race sees
//! the same ones; each phone kicks the cars it drives.
use super::rt::{River, Rt};
use crate::physics::Car;
use crate::world::{World, ROAD};

/// a fireball leaves the crater every SLOT seconds and lands FLIGHT seconds later; its blast shows for BOOM seconds and reaches REACH m
pub const SLOT: f64 = 0.2;
pub const FLIGHT: f64 = 3.2;
pub const BOOM: f64 = 0.6;
pub const REACH: f64 = 6.5;
const APEX: f64 = 140.0;
/// the schedule repeats every PERIOD seconds (a whole number of slots), so the shared clock can wrap
pub const PERIOD: f64 = 600.0;
const NSLOT: i64 = 3000;

fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15); z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9); z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB); z ^ (z >> 31)
}
fn unit(h: u64, k: u32) -> f64 { ((h >> (k * 16)) & 0xFFFF) as f64 / 65535.0 }

pub struct Ball { pub t0: f64, pub from: [f64; 3], pub to: [f64; 3], pub spin: f64 }
impl Ball {
    /// where it is `u` (0..1) of the way through its flight
    pub fn pos(&self, u: f64) -> [f64; 3] {
        let l = |a: f64, b: f64| a + (b - a) * u;
        [l(self.from[0], self.to[0]), l(self.from[1], self.to[1]) + 4.0 * APEX * u * (1.0 - u), l(self.from[2], self.to[2])]
    }
}
/// fireball number `n`: launched at n x SLOT from somewhere in the crater, landing on the road at a random place along the lap
pub fn ball(rt: &Rt, w: &World, n: i64) -> Option<Ball> {
    let v = rt.volc.as_ref()?; let np = rt.path.len(); if np == 0 { return None; }
    let h = mix(n.rem_euclid(NSLOT) as u64 ^ 0xE3BE_7A11);
    let mut k = ((unit(h, 0) * np as f64) as usize).min(np - 1); let lat = (unit(h, 1) * 2.0 - 1.0) * (ROAD - 0.5);
    // never on a bridge (or just before or after one): the deck is narrow enough already - moved on past it
    for &(a, b) in &v.bridges { let l = (b + np - a) % np; let d = (k + np - a) % np; if d <= l + 12 || np - d <= 12 { k = (b + 13 + (d % 7)) % np; } }
    let (p, t) = (rt.path[k], rt.tan[k]); let x = p[0] - t[1] * lat; let z = p[1] + t[0] * lat;
    let a = unit(h, 2) * std::f64::consts::TAU; let r = 10.0 + unit(h, 3) * 25.0;
    Some(Ball { t0: n as f64 * SLOT, from: [v.crater[0] + a.cos() * r, v.crater[1] + 2.0, v.crater[2] + a.sin() * r], to: [x, w.height(x, z), z], spin: if h & 1 == 0 { 1.0 } else { -1.0 } })
}

/// fallen into a lava canyon: well below the rim (off a bridge or over the edge) - the river whose bridge to go back before
pub fn in_canyon<'a>(rt: &'a Rt, w: &World, x: f64, y: f64, z: f64) -> Option<&'a River> {
    let v = rt.volc.as_ref()?; let (dc, j) = crate::world::ember_canyon(x, z);
    if dc < 4.0 || y > w.base_height(x, z) + dc - 4.0 { return None; }
    v.rivers.get(j).or(v.rivers.first())
}

/// where to steer over the next bridge (its middle), from 80 m before its span until just after it
pub fn dodge(rt: &Rt, i: usize) -> Option<f64> {
    let v = rt.volc.as_ref()?; let np = rt.path.len();
    v.bridges.iter().find(|&&(a, b)| { let l = (b + np - a) % np; let d = (i + np - a) % np; d <= l + 6 || np - d <= 80 }).map(|_| 0.0)
}
/// how fast to take the bridge ahead (m/s): steady from 45 m before it until across
pub fn bridge_cap(rt: &Rt, i: usize) -> Option<f64> {
    let v = rt.volc.as_ref()?; let np = rt.path.len();
    v.bridges.iter().find(|&&(a, b)| { let l = (b + np - a) % np; let d = (i + np - a) % np; d <= l + 3 || np - d <= 45 }).map(|_| 16.0)
}
/// the road index to put a car back at after falling in: before the bridge it was nearest
pub fn back_from(rt: &Rt, rv: &River, _i: usize) -> usize { let np = rt.path.len(); (rv.kc + np - 28) % np }

/// a fireball landing at `b` throws the car out from the blast and up; returns how hard (0: out of reach)
pub fn kick(c: &mut Car, b: &Ball) -> f64 {
    let (dx, dz) = (c.x - b.to[0], c.z - b.to[2]); let d = dx.hypot(dz);
    if d > REACH || (c.y - b.to[1]).abs() > 4.0 { return 0.0; }
    let k = 1.0 - d / REACH; let (ux, uz) = if d > 0.2 { (dx / d, dz / d) } else { (c.h.sin(), c.h.cos()) };
    let push = 2.0 + 4.5 * k; c.vx += ux * push; c.vz += uz * push; c.vy += 2.0 + 3.0 * k; c.r += b.spin * 1.3 * k;
    c.impact = c.impact.max(5.0 + 7.0 * k); k
}

// ---- the look ------------------------------------------------------------------------------------------------------------------------
// Everything below is drawing only: a molten rock with a comet trail and sparks in the air, a target on the road that fills to the blast's
// real reach as it comes down, then a flash, a billow of faceted fire puffs, a shockwave, rocks and embers thrown out, a smoke column and a
// splash of lava on the road that cools. All of it comes from the fireball's number, so it looks the same on every phone.

/// how long a blast's smoke, debris and cooling lava stay
pub const LIFE: f64 = 3.6;
/// blasts further than this from the camera are skipped (the fog has them by then)
const DRAW_FAR: f64 = 460.0;
type V = [f64; 3];
#[inline] fn add(a: V, b: V) -> V { [a[0] + b[0], a[1] + b[1], a[2] + b[2]] }
#[inline] fn sub(a: V, b: V) -> V { [a[0] - b[0], a[1] - b[1], a[2] - b[2]] }
#[inline] fn mul(a: V, k: f64) -> V { [a[0] * k, a[1] * k, a[2] * k] }
#[inline] fn len(a: V) -> f64 { (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt() }
#[inline] fn norm(a: V) -> V { let l = len(a).max(1e-9); mul(a, 1.0 / l) }
#[inline] fn cross(a: V, b: V) -> V { [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]] }
#[inline] fn lerp3(a: V, b: V, k: f64) -> V { [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k] }
#[inline] fn smooth(a: f64, b: f64, x: f64) -> f64 { let t = ((x - a) / (b - a)).clamp(0.0, 1.0); t * t * (3.0 - 2.0 * t) }
const UP: V = [0.0, 1.0, 0.0];

/// the fire ramp, from white-hot to the dark red of cooling rock (the lava texture's colours)
const HEAT: [V; 7] = [[1.0, 0.97, 0.72], [1.0, 0.86, 0.38], [1.0, 0.68, 0.17], [1.0, 0.47, 0.10], [0.92, 0.28, 0.06], [0.52, 0.12, 0.05], [0.24, 0.06, 0.035]];
fn heat(x: f64) -> V { let x = x.clamp(0.0, 1.0) * 6.0; let i = (x as usize).min(5); lerp3(HEAT[i], HEAT[i + 1], x - i as f64) }
const ROCK: V = [0.15, 0.085, 0.065];
const SMOKE: V = [0.3, 0.255, 0.235];
const SMOKE_LIT: V = [0.62, 0.26, 0.12];

/// a random-but-fixed number in 0..1 for fireball hash `h`, draw `k`
#[inline] fn rn(h: u64, k: u64) -> f64 { unit(mix(h ^ k.wrapping_mul(0x9E37_79B9_7F4A_7C15)), 0) }
/// a fixed random orientation for (h, k), turned `ang` about its own axis
fn basis(h: u64, k: u64, ang: f64) -> (V, V, V) {
    let th = rn(h, k) * std::f64::consts::TAU; let z = rn(h, k + 1) * 2.0 - 1.0; let r = (1.0 - z * z).sqrt();
    let ax = [r * th.cos(), z, r * th.sin()]; let a0 = rn(h, k + 2) * std::f64::consts::TAU + ang; let (s, c) = a0.sin_cos();
    let rot = |v: V| -> V { let d = ax[0] * v[0] + ax[1] * v[1] + ax[2] * v[2]; let cr = cross(ax, v); [v[0] * c + cr[0] * s + ax[0] * d * (1.0 - c), v[1] * c + cr[1] * s + ax[1] * d * (1.0 - c), v[2] * c + cr[2] * s + ax[2] * d * (1.0 - c)] };
    (rot([1.0, 0.0, 0.0]), rot([0.0, 1.0, 0.0]), rot([0.0, 0.0, 1.0]))
}
/// the axes of something pointing along `d`, twisted `tw` about it
fn along(d: V, tw: f64) -> (V, V, V) {
    let z = norm(d); let r = if z[1].abs() < 0.95 { UP } else { [1.0, 0.0, 0.0] }; let x0 = norm(cross(r, z)); let y0 = cross(z, x0); let (s, c) = tw.sin_cos();
    (add(mul(x0, c), mul(y0, s)), add(mul(y0, c), mul(x0, -s)), z)
}

/// one pool's instances: matrices and colours
#[derive(Default)] pub struct Pool { pub m: Vec<f32>, pub c: Vec<f32> }
impl Pool {
    fn put(&mut self, p: V, ax: (V, V, V), s: V, col: V) {
        let (x, y, z) = ax; let (x, y, z) = (mul(x, s[0]), mul(y, s[1]), mul(z, s[2]));
        self.m.extend_from_slice(&[x[0] as f32, x[1] as f32, x[2] as f32, 0.0, y[0] as f32, y[1] as f32, y[2] as f32, 0.0, z[0] as f32, z[1] as f32, z[2] as f32, 0.0, p[0] as f32, p[1] as f32, p[2] as f32, 1.0]);
        self.c.extend_from_slice(&[col[0] as f32, col[1] as f32, col[2] as f32]);
    }
    fn flat(&mut self, p: V, r: f64, col: V) { self.put(p, ([1.0, 0.0, 0.0], UP, [0.0, 0.0, 1.0]), [r; 3], col); }
    /// a soft glow: three nested additive shells, bright in the middle and fading out
    fn soft(&mut self, p: V, r: f64, col: V) { for (k, w) in [(0.28, 0.42), (0.44, 0.26), (0.6, 0.16), (0.78, 0.09), (1.0, 0.045)] { self.flat(p, r * k, mul(col, w)); } }
    pub fn n(&self) -> i32 { (self.m.len() / 16) as i32 }
}

/// one frame of fireball drawing (the five pools), and the fireballs that landed since the last frame
pub struct Frame { pub fire: Pool, pub solid: Pool, pub glow: Pool, pub ring: Pool, pub disc: Pool, pub landed: Vec<Ball> }
pub fn frame(rt: &Rt, w: &World, t: f64, t_prev: f64, cam: V) -> Frame {
    let mut f = Frame { fire: Pool::default(), solid: Pool::default(), glow: Pool::default(), ring: Pool::default(), disc: Pool::default(), landed: vec![] };
    let (lo, hi) = (((t - FLIGHT - LIFE) / SLOT).floor() as i64, (t / SLOT).floor() as i64);
    for n in lo..=hi { let Some(b) = ball(rt, w, n) else { continue };
        let h = mix(n.rem_euclid(NSLOT) as u64 ^ 0x51DE_F1AE);
        let u = (t - b.t0) / FLIGHT;
        if (0.0..1.0).contains(&u) { in_flight(&mut f, &b, h, t, u); }
        else if u >= 1.0 { let a = t - b.t0 - FLIGHT; if a < LIFE && len(sub(b.to, cam)) < DRAW_FAR { blast(&mut f, &b, h, t, a); } }
        let tl = b.t0 + FLIGHT; if tl > t_prev && tl <= t { f.landed.push(b); }
    }
    f
}

fn in_flight(f: &mut Frame, b: &Ball, h: u64, t: f64, u: f64) {
    let p = b.pos(u); let fl = 0.5 + 0.5 * (t * 31.0 + rn(h, 1) * 9.0).sin();
    // the head: a dark crust over molten rock, the two turning different ways so the glow shows through the cracks
    f.solid.put(p, basis(h, 10, t * 3.1 * b.spin), [1.3, 1.08, 1.18], ROCK);
    f.fire.put(p, basis(h, 20, -t * 2.2 * b.spin), [1.16; 3], heat(0.1 + 0.12 * fl));
    f.glow.soft(p, 4.6 + 0.5 * fl, mul([1.0, 0.38, 0.09], 0.55 + 0.12 * fl));
    // the trail: stretched puffs between points of the path it just flew, white-hot at the head, cooling to red, then smoke
    const DU: [f64; 10] = [0.0, 0.006, 0.013, 0.021, 0.031, 0.043, 0.057, 0.074, 0.094, 0.118];
    for k in 0..9 {
        let (ua, ub) = (u - DU[k], u - DU[k + 1]); if ub < 0.0 { break; }
        let (pa, pb) = (b.pos(ua), b.pos(ub)); let d = sub(pa, pb); let l = len(d); if l < 1e-3 { continue; }
        let mid = mul(add(pa, pb), 0.5); let ax = along(d, rn(h, 30 + k as u64) * 6.0 + t * 1.5);
        if k < 6 { let wd = 1.05 * (1.0 - k as f64 / 7.0).powf(0.8) + 0.18; f.fire.put(mid, ax, [wd, wd, l * 0.62], heat(0.06 + k as f64 * 0.13 + 0.05 * fl)); }
        else { let q = (k - 6) as f64 / 3.0; let wd = 1.0 + 0.5 * q; f.fire.put(add(mid, [0.0, q * 0.8, 0.0]), ax, [wd, wd * 0.9, l * 0.6], lerp3(SMOKE_LIT, SMOKE, 0.5 + 0.5 * q)); }
    }
    // sparks shed as it flies
    let el = t - b.t0; let j1 = (el / 0.13).floor() as i64;
    for j in (j1 - 3)..=j1 { if j < 0 { continue; } let age = el - j as f64 * 0.13; if !(0.0..0.42).contains(&age) { continue; }
        let hj = h ^ (j as u64).wrapping_mul(0xA24B_AED4_963E_E407); let th = rn(hj, 1) * std::f64::consts::TAU; let sp = 3.0 + 5.0 * rn(hj, 2);
        let v = [th.cos() * sp, 1.5 + 3.0 * rn(hj, 3), th.sin() * sp]; let q = add(add(b.pos(u - age / FLIGHT), mul(v, age)), [0.0, -6.0 * age * age, 0.0]);
        let s = 0.2 * (1.0 - age / 0.42); f.fire.put(q, along(v, 0.0), [s, s, s * 1.8], heat(0.08 + age * 1.5)); }
    // the target on the road: an outline at the blast's reach, filling in as it comes down, beating faster near the end
    if u > 0.3 { let q = (u - 0.3) / 0.7; let beat = 0.5 + 0.5 * (t * (7.0 + 20.0 * q)).sin(); let g = [b.to[0], b.to[1] + 0.12, b.to[2]];
        f.ring.flat(g, REACH, mul([1.0, 0.3, 0.06], (0.3 + 0.7 * q) * (0.6 + 0.4 * beat)));
        f.disc.flat(g, REACH * (0.15 + 0.85 * q * q), mul([0.9, 0.2, 0.04], (0.4 + 0.6 * q) * (0.7 + 0.3 * beat)));
        if q > 0.6 { f.ring.flat(g, REACH * 0.24, mul([1.0, 0.55, 0.15], (q - 0.6) / 0.4 * (0.5 + 0.5 * beat))); } }
}

fn blast(f: &mut Frame, b: &Ball, h: u64, t: f64, a: f64) {
    let g = [b.to[0], b.to[1] + 0.1, b.to[2]]; let fl = 0.5 + 0.5 * (t * 23.0 + rn(h, 2) * 7.0).sin();
    let wind = [0.9 * (rn(h, 3) - 0.3), 0.0, 0.9 * (rn(h, 4) - 0.3)];
    // the flash
    if a < 0.18 { let k = 1.0 - a / 0.18; f.glow.soft(add(g, [0.0, 1.4, 0.0]), 4.0 + 5.0 * (1.0 - k * k), mul([1.0, 0.8, 0.48], k * k * 1.8));
        f.disc.flat(add(g, [0.0, 0.05, 0.0]), 9.0, mul([1.0, 0.62, 0.3], k * k * 0.9)); }
    // the shockwave along the ground
    if a < 0.42 { let k = a / 0.42; f.ring.flat(add(g, [0.0, 0.05, 0.0]), 1.0 + (REACH + 3.5) * (1.0 - (1.0 - k).powi(3)), mul([1.0, 0.62, 0.22], (1.0 - k).powf(1.5) * 1.3)); }
    // the billow: faceted puffs thrown out and up, growing fast, cooling through the ramp, then shrinking away
    for i in 0..11u64 {
        let life = 0.5 + 0.45 * rn(h, 100 + i); if a >= life { continue; }
        let (th, el) = (rn(h, 110 + i) * std::f64::consts::TAU, (0.12 + 0.78 * rn(h, 120 + i)) * std::f64::consts::FRAC_PI_2);
        let dir = if i == 0 { UP } else { [th.cos() * el.cos(), el.sin(), th.sin() * el.cos()] };
        let r = if i == 0 { 0.6 } else { 1.8 + 3.0 * rn(h, 130 + i) }; let e = 1.0 - (-a * 8.0).exp();
        let c = add(add(g, mul(dir, r * e)), [0.0, 0.7 + 1.5 * a, 0.0]);
        let smax = if i == 0 { 3.1 } else { 1.4 + 1.3 * rn(h, 140 + i) }; let s = smax * (0.4 + 0.6 * (a / 0.09).min(1.0)) * (1.0 - smooth(0.42 * life, life, a));
        f.fire.put(c, basis(h, 150 + i * 3, a * 2.5), [s, s * 0.92, s], heat(a / life * 0.97));
    }
    // little flames licking where it hit
    if a < 2.4 { for i in 0..3u64 { let k = 1.0 - a / 2.4; let ph = 0.5 + 0.5 * (t * (13.0 + 5.0 * i as f64) + i as f64 * 2.0).sin();
        let o = [(rn(h, 200 + i) - 0.5) * 2.4, 0.3 + 0.35 * ph * k, (rn(h, 210 + i) - 0.5) * 2.4];
        let s = (0.35 + 0.35 * ph) * k.sqrt(); f.fire.put(add(g, o), basis(h, 220 + i * 3, t * 4.0), [s * 0.8, s * 1.4, s * 0.8], heat(0.18 + 0.5 * (1.0 - k) + 0.12 * ph)); } }
    // the smoke column: dark faceted puffs, lit orange by the fire at first, rising and drifting
    for i in 0..11u64 {
        let s0 = 0.08 + i as f64 * 0.06; if a < s0 { continue; } let bb = a - s0; let life = 2.3 + 0.9 * rn(h, 300 + i); if bb >= life { continue; }
        let rise = (2.0 + 1.8 * rn(h, 310 + i)) * bb.powf(0.85); let o = [(rn(h, 320 + i) - 0.5) * 2.2, 1.3 + rise, (rn(h, 330 + i) - 0.5) * 2.2];
        let s = (0.8 + 0.5 * rn(h, 340 + i)) * (1.0 + 1.7 * bb / life) * (bb / 0.25).min(1.0) * (1.0 - smooth(0.62 * life, life, bb));
        f.fire.put(add(add(g, o), mul(wind, bb)), basis(h, 350 + i * 3, bb * 0.6), [s, s * 0.85, s], mul(lerp3(SMOKE_LIT, SMOKE, (bb / 0.7).min(1.0)), 1.0 - 0.35 * bb / life));
    }
    // rocks: half dark, half still glowing; thrown, landing, tumbling until they settle, then sinking away
    for i in 0..8u64 {
        let th = rn(h, 400 + i) * std::f64::consts::TAU; let sp = 4.0 + 5.5 * rn(h, 410 + i); let vy = 6.0 + 6.0 * rn(h, 420 + i); let gr = 20.0;
        let tl = 2.0 * vy / gr; let ta = a.min(tl); let y = if a < tl { vy * ta - 0.5 * gr * ta * ta } else { 0.12 };
        let c = add(g, [th.cos() * sp * ta, y, th.sin() * sp * ta]); let s = (0.24 + 0.32 * rn(h, 430 + i)) * (1.0 - smooth(1.8, 2.7, a)); if s <= 0.0 { continue; }
        let ax = basis(h, 440 + i * 3, ta * (6.0 + 6.0 * rn(h, 450 + i))); let dim = [s * 1.2, s * 0.8, s];
        if i < 4 { f.solid.put(c, ax, dim, ROCK); } else { f.fire.put(c, ax, dim, heat(0.22 + a * 0.32)); }
    }
    // embers: fast streaks with drag and gravity, then a few that float up on the heat
    for i in 0..16u64 {
        let life = 0.7 + 0.8 * rn(h, 500 + i); if a >= life { continue; }
        let th = rn(h, 510 + i) * std::f64::consts::TAU; let sp = 7.0 + 8.0 * rn(h, 520 + i); let v0 = [th.cos() * sp, 5.0 + 8.0 * rn(h, 530 + i), th.sin() * sp]; let kd = 1.8;
        let e = (1.0 - (-kd * a).exp()) / kd; let c = add(add(g, mul(v0, e)), [0.0, 0.5 - 6.0 * a * a, 0.0]);
        let v = add(mul(v0, (-kd * a).exp()), [0.0, -12.0 * a, 0.0]); let s = 0.13 * (1.0 - a / life);
        f.fire.put(c, along(v, 0.0), [s, s, s + 0.035 * len(v)], heat(a / life * 0.9));
    }
    if a < 2.6 { for i in 0..6u64 { let k = a / 2.6; let o = [(rn(h, 600 + i) - 0.5) * 3.0 + (t * 2.0 + i as f64).sin() * 0.3, 0.6 + (1.0 + 1.6 * rn(h, 610 + i)) * a, (rn(h, 620 + i) - 0.5) * 3.0];
        let s = 0.11 * (1.0 - k) * (0.7 + 0.3 * fl); f.fire.put(add(add(g, o), mul(wind, a)), basis(h, 630 + i * 3, t * 3.0), [s; 3], heat(0.2 + 0.6 * k)); } }
    // the lava splashed on the road, cooling: a pool and a few splats round it
    let cool = (-a * 1.05).exp() * (0.88 + 0.12 * fl) * (1.0 - smooth(LIFE - 0.6, LIFE, a));
    f.disc.flat(add(g, [0.0, 0.03, 0.0]), 3.6 * (1.0 - 0.2 * a / LIFE), mul([1.0, 0.45, 0.12], cool * 1.7));
    for i in 0..5u64 { let th = rn(h, 700 + i) * std::f64::consts::TAU; let r = 2.6 + 3.0 * rn(h, 710 + i); let land = 0.12 + 0.1 * rn(h, 720 + i); if a < land { continue; }
        f.disc.flat(add(g, [th.cos() * r, 0.04, th.sin() * r]), 0.5 + 0.6 * rn(h, 730 + i), mul([1.0, 0.5, 0.14], 1.6 * (-(a - land) * 1.8).exp() * (1.0 - smooth(LIFE - 0.6, LIFE, a)))); }
}
