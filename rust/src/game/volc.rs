//! Ember Peak's hazards. Lava rivers run down the volcano and across the road: one side of each crossing is a crust of cooled rock you
//! can drive over, the rest (and the river off the road) puts you back on the road before it. The crater throws fireballs that land on
//! the road and throw cars about. Fireballs run on a shared clock (the wall clock, like the prism waves), so every phone in a race sees
//! the same ones; each phone kicks the cars it drives.
use super::path_idx;
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
    let k = ((unit(h, 0) * np as f64) as usize).min(np - 1); let lat = (unit(h, 1) * 2.0 - 1.0) * (ROAD - 0.5);
    let (p, t) = (rt.path[k], rt.tan[k]); let x = p[0] - t[1] * lat; let z = p[1] + t[0] * lat;
    let a = unit(h, 2) * std::f64::consts::TAU; let r = 10.0 + unit(h, 3) * 25.0;
    Some(Ball { t0: n as f64 * SLOT, from: [v.crater[0] + a.cos() * r, v.crater[1] + 2.0, v.crater[2] + a.sin() * r], to: [x, w.height(x, z), z], spin: if h & 1 == 0 { 1.0 } else { -1.0 } })
}

/// the lava river a point is in, if it is in one (the crust side of a road crossing is safe)
pub fn in_lava<'a>(rt: &'a Rt, w: &World, x: f64, z: f64) -> Option<&'a River> {
    let v = rt.volc.as_ref()?;
    for rv in &v.rivers {
        if x < rv.bb[0] || x > rv.bb[1] || z < rv.bb[2] || z > rv.bb[3] { continue; }
        let mut inside = false;
        for i in 0..rv.pts.len().saturating_sub(1) {
            let (a, b) = (rv.pts[i], rv.pts[i + 1]); let (dx, dz) = (b.0 - a.0, b.1 - a.1); let l2 = dx * dx + dz * dz; if l2 <= 0.0 { continue; }
            let u = (((x - a.0) * dx + (z - a.1) * dz) / l2).clamp(0.0, 1.0); let (qx, qz) = (a.0 + dx * u - x, a.1 + dz * u - z);
            let hw = rv.hw[i] + (rv.hw[(i + 1).min(rv.hw.len() - 1)] - rv.hw[i]) * u; if qx * qx + qz * qz < hw * hw { inside = true; break; }
        }
        if !inside { continue; }
        let k = path_idx(w, x, z); let (p, t) = (rt.path[k], rt.tan[k]); let lat = (x - p[0]) * -t[1] + (z - p[1]) * t[0];
        if w.path_dist(x, z) < ROAD + 0.6 && lat * rv.sg >= 0.0 { return None; }   // on the crust (half the road)
        return Some(rv);
    }
    None
}

/// where to steer to get past the next lava crossing (the middle of the crust side), from 55 m before it until just after it
pub fn dodge(rt: &Rt, i: usize) -> Option<f64> {
    let v = rt.volc.as_ref()?; let np = rt.path.len();
    v.rivers.iter().find(|rv| { let d = (rv.kc + np - i) % np; d < 60 || d + 12 > np }).map(|rv| rv.sg * 2.0)
}
/// the road index to put a car back at after lava: before the crossing it was at, or a little back along the road elsewhere
pub fn back_from(rt: &Rt, rv: &River, i: usize) -> usize {
    let np = rt.path.len(); let d = (i + np - rv.kc) % np;
    if d < 40 || d + 40 > np { (rv.kc + np - 25) % np } else { (i + np - 10) % np }
}

/// a fireball landing at `b` throws the car out from the blast and up; returns how hard (0: out of reach)
pub fn kick(c: &mut Car, b: &Ball) -> f64 {
    let (dx, dz) = (c.x - b.to[0], c.z - b.to[2]); let d = dx.hypot(dz);
    if d > REACH || (c.y - b.to[1]).abs() > 4.0 { return 0.0; }
    let k = 1.0 - d / REACH; let (ux, uz) = if d > 0.2 { (dx / d, dz / d) } else { (c.h.sin(), c.h.cos()) };
    let push = 2.0 + 4.5 * k; c.vx += ux * push; c.vz += uz * push; c.vy += 2.0 + 3.0 * k; c.r += b.spin * 1.3 * k;
    c.impact = c.impact.max(5.0 + 7.0 * k); k
}

/// one frame of fireball drawing: instance matrices for cores, glows, warning rings and blasts, colours for the rings and blasts, and the
/// fireballs that landed since the last frame
pub struct Frame { pub core: Vec<f32>, pub halo: Vec<f32>, pub ring: Vec<f32>, pub ring_c: Vec<f32>, pub boom: Vec<f32>, pub boom_c: Vec<f32>, pub landed: Vec<Ball> }
fn mat(out: &mut Vec<f32>, p: [f64; 3], s: f64) { out.extend_from_slice(&[s as f32, 0.0, 0.0, 0.0, 0.0, s as f32, 0.0, 0.0, 0.0, 0.0, s as f32, 0.0, p[0] as f32, p[1] as f32, p[2] as f32, 1.0]); }
pub fn frame(rt: &Rt, w: &World, t: f64, t_prev: f64) -> Frame {
    let mut f = Frame { core: vec![], halo: vec![], ring: vec![], ring_c: vec![], boom: vec![], boom_c: vec![], landed: vec![] };
    let (lo, hi) = (((t - FLIGHT - BOOM) / SLOT).floor() as i64, (t / SLOT).floor() as i64);
    for n in lo..=hi { let Some(b) = ball(rt, w, n) else { continue };
        let u = (t - b.t0) / FLIGHT;
        if (0.0..1.0).contains(&u) {
            let p = b.pos(u); mat(&mut f.core, p, 1.5); mat(&mut f.halo, p, 3.4);
            if u > 0.4 { let q = (u - 0.4) / 0.6; let k = (0.35 + 0.65 * q) * (0.75 + 0.25 * (t * 22.0).sin());
                mat(&mut f.ring, [b.to[0], b.to[1] + 0.12, b.to[2]], 2.4 + 2.2 * q); f.ring_c.extend_from_slice(&[(1.0 * k) as f32, (0.28 * k) as f32, (0.05 * k) as f32]); }
        } else if u >= 1.0 { let q = (t - b.t0 - FLIGHT) / BOOM; if q < 1.0 { let k = (1.0 - q).powf(1.5);
            mat(&mut f.boom, [b.to[0], b.to[1] + 1.0, b.to[2]], 1.5 + 9.0 * q); f.boom_c.extend_from_slice(&[(1.0 * k) as f32, (0.6 * k) as f32, (0.22 * k) as f32]); } }
        let tl = b.t0 + FLIGHT; if tl > t_prev && tl <= t { f.landed.push(b); }
    }
    f
}
