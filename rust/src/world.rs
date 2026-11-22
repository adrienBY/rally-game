//! Stage generation: distance-to-road field, nearest-road index and terrain height on a 1 m grid.
use crate::noise::*;

pub const ROAD: f64 = 3.4;
/// Prism: the outer edge of the glowing kerb the page draws (1.08 x the half-width + 0.5 m)
pub const PRISM_KERB: f64 = PRISM_ROAD * 1.08 + 0.5;
/// Prism: where the ground ends for everything that asks the world (shadows, the camera, a car already falling): a little past the kerb,
/// so the shadow of a wheel hanging over the edge still lands. Whether a car is on the deck is decided by its centre (see Car::step).
pub const PRISM_EDGE: f64 = PRISM_KERB + 1.1;
/// Prism: the road's half-width (wider than the rally stages: room for one more car across; the page uses the same)
pub const PRISM_ROAD: f64 = 4.5;
/// the road's half-width on a map
pub fn road_half(map: Map) -> f64 { if map == Map::Prism { PRISM_ROAD } else { ROAD } }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Map { Forest = 0, Snow = 1, Canyon = 2, Lakes = 3, Iceland = 4, Touge = 5, Riviera = 6, Prism = 7, Ember = 8 }
impl Map { pub fn from_id(i: i32) -> Map { match i { 1 => Map::Snow, 2 => Map::Canyon, 3 => Map::Lakes, 4 => Map::Iceland, 5 => Map::Touge, 6 => Map::Riviera, 7 => Map::Prism, 8 => Map::Ember, _ => Map::Forest } } }
/// Prism: the floor of space, far below the skyway (falling to it means a respawn)
pub const PRISM_VOID: f64 = -400.0;
/// Lakes: water level
pub const WL: f64 = -4.5;

pub struct World {
    pub map: Map,
    /// prism: the deck's live waves (set once the game has the stage) and the clock they run on
    pub wave: std::sync::OnceLock<Wave>, pub wave_t: std::sync::atomic::AtomicU64,
    /// ember peak: the bridges over the lava canyons (set once the game has the stage)
    pub bridge: std::sync::OnceLock<Bridges>,
    pub gxh: i32, pub gzh: i32, pub gnx: usize, pub gnz: usize,
    pub pd: Vec<f32>, pub pi: Vec<i16>, pub h: Vec<f32>,
}

fn terrain(map: Map, x: f64, z: f64) -> f64 {
    match map {
        Map::Prism => PRISM_VOID,
        Map::Ember => vol_mount(x, z),
        Map::Riviera => riv_land(x, z, true),
        Map::Iceland => { let b = ice_base(x, z); b - ice_riv(x, z) * (b + 1.0) }
        Map::Touge => { let k = (x * x + z * z) / 90000.0; 66.0 / (1.0 + k) - (k - 0.62).max(0.0).min(2.5) * 46.0 + tou_hill(x, z) }
        Map::Canyon => (fbm(x * 0.008 + 3.0, z * 0.008 - 2.0, 4) - 0.5) * 18.0 + (fbm(x * 0.05, z * 0.05, 4) - 0.5) * 1.2,
        Map::Lakes => (fbm(x * 0.006 + 5.0, z * 0.006 + 1.0, 4) - 0.5) * 44.0 + (fbm(x * 0.02, z * 0.02, 4) - 0.5) * 8.0 + (fbm(x * 0.07, z * 0.07, 4) - 0.5) * 1.2,
        Map::Snow => ridged(x * 0.0052 + 3.0, z * 0.0052 - 7.0) * 58.0 + (fbm(x * 0.016, z * 0.016, 4) - 0.5) * 12.0 + (fbm(x * 0.07, z * 0.07, 4) - 0.5) * 1.4,
        Map::Forest => (fbm(x * 0.014 + 10.0, z * 0.014 - 4.0, 4) - 0.5) * 16.0 + (fbm(x * 0.06, z * 0.06, 4) - 0.5) * 1.6,
    }
}
/// Iceland: plains + lava humps, braided rivers (bed at -1 m, water at 0), basalt escarpments
fn ice_riv(x: f64, z: f64) -> f64 { 1.0 - smooth(0.012, 0.03, (fbm(x * 0.0032 + 41.0, z * 0.0032 - 17.0, 3) - 0.5).abs()) }
fn ice_cliff(x: f64, z: f64) -> f64 { smooth(0.55, 0.585, fbm(x * 0.0042 + 27.0, z * 0.0042 - 31.0, 4)) * (38.0 + fbm(x * 0.02 + 3.0, z * 0.02, 3) * 16.0) }
fn ice_base(x: f64, z: f64) -> f64 { 3.0 + (fbm(x * 0.006 + 13.0, z * 0.006 - 7.0, 4) - 0.5) * 7.0 + ridged(x * 0.035 + 2.0, z * 0.035 + 5.0) * 1.6 + (fbm(x * 0.08, z * 0.08, 4) - 0.5) * 0.6 }

const RIV_CO: [f64; 33] = [365.0943089645332, 11.436903918090803, 4.545725825613665, 113.1855404692721, 3.5734199846815837, 3.985531199612783, -12.617522459756444, -4.700187825096951, 24.485745853738244, -4.1766535012051635, -2.8397619313238534, -8.227522873444984, 7.426301383840519, -2.7763978739119093, -6.637529415252545, -4.710651589545555, -5.776624135423112, -4.665001169047276, 1.9423783806500399, 5.676152373037559, -4.566612283315319, 0.756121770018548, -4.584140164551264, 1.9514458472883758, -1.868534773179199, -0.6026043725066449, 0.5503129741098648, 1.5881490516580001, 2.2857083143419112, 1.2341923187040604, 0.005228537432869059, -1.7935242357328982, 1.315808959662767];
const RIV_CL: [f64; 13] = [31.81134008102376, 12.41158534922303, 3.5275303935796125, 3.7753895871459235, 1.8256511098772177, -3.4063965472223505, 0.8009726687413219, 2.1537015290667467, 1.94814461591344, 0.28048831502508087, -2.222168807077778, -1.3767467974531142, 0.74358329361819];
const RIV_TUN: [[f64; 4]; 2] = [[362.865, -172.048, -0.721827, -0.692073], [-367.341, 72.814, 0.669072, 0.743197]];
const RIV_SW: [f64; 6] = [-97.687, -175.09, -0.999997, -0.002485, 22.83, 69.16];
/// Riviera: a headland with a fitted polar coastline, cliffs into the sea, land climbing to a wooded hill, the switchback slope and two rock
/// spurs the road tunnels through. Exact arithmetic only, written exactly like the JS so the grids are bit-identical.
pub fn riv_land(x: f64, z: f64, spur: bool) -> f64 {
    let rho = (x * x + z * z + 1.0).sqrt(); let c1 = x / rho; let s1 = z / rho;
    let (mut coast, mut cliff, mut ck, mut sk) = (RIV_CO[0], RIV_CL[0], c1, s1);
    for k in 1..=16usize { coast = coast + RIV_CO[2 * k - 1] * ck + RIV_CO[2 * k] * sk;
        if k <= 6 { cliff = cliff + RIV_CL[2 * k - 1] * ck + RIV_CL[2 * k] * sk; }
        let nc = ck * c1 - sk * s1; sk = sk * c1 + ck * s1; ck = nc; }
    let mut s = rho - coast + (fbm(x * 0.018 + 3.0, z * 0.018 - 5.0, 3) - 0.5) * 26.0;
    let inland = (-s - 8.0).max(0.0); let cl = cliff + (34.0 - cliff) * smooth(40.0, 150.0, inland);
    let mut h = cl + inland * 0.3 / (1.0 + inland / 110.0) + smooth(30.0, 190.0, inland) * 34.0 + (fbm(x * 0.0065 + 11.0, z * 0.0065 - 4.0, 3) - 0.5) * 70.0 * smooth(60.0, 200.0, inland) + (fbm(x * 0.03, z * 0.03, 3) - 0.5) * 6.0 + (fbm(x * 0.12, z * 0.12, 2) - 0.5) * 1.0;
    { let (rx, rz, tx, tz, cc, hh) = (RIV_SW[0], RIV_SW[1], RIV_SW[2], RIV_SW[3], RIV_SW[4], RIV_SW[5]);
      let al = (x - rx) * tx + (z - rz) * tz; let lt = (x - rx) * -tz + (z - rz) * tx;
      let m = (1.0 - smooth(hh + 10.0, hh + 70.0, (al - cc).abs())) * smooth(-60.0, -15.0, lt) * (1.0 - smooth(70.0, 110.0, lt));
      h = h + (43.0 - lt * (if lt > 0.0 { 0.48 } else { 0.15 }) - h) * m; }
    if spur { for q in 0..2 { let (mx, mz, tx, tz) = (RIV_TUN[q][0], RIV_TUN[q][1], RIV_TUN[q][2], RIV_TUN[q][3]);
        let al = (x - mx) * tx + (z - mz) * tz; let ac = (x - mx) * -tz + (z - mz) * tx;
        let b = (1.0 - smooth(20.0, 38.0, al.abs())) * (1.0 - smooth(50.0, 110.0, -ac)); h = h + b * 22.0; s = s - b * 30.0; } }
    let k = smooth(-7.0, 5.0, s); h + (-15.0 - h) * k
}

/// Ember Peak: a lone volcano - a broad cone, a deep crater inside a raised rim, lava ridges down the flanks (as the page's volMount)
fn vol_mount(x: f64, z: f64) -> f64 {
    let r = (x * x + z * z + 1.0).sqrt(); let k = (x * x + z * z) / 108900.0;
    250.0 / (1.0 + k) - 95.0 * (1.0 - smooth(35.0, 100.0, r)) + 12.0 * smooth(60.0, 95.0, r) * (1.0 - smooth(95.0, 140.0, r))
        + ridged(x * 0.009 + 2.0, z * 0.009 - 5.0) * 16.0 * smooth(90.0, 220.0, r) + (fbm(x * 0.03 + 7.0, z * 0.03, 3) - 0.5) * 6.0 + (fbm(x * 0.11, z * 0.11, 2) - 0.5) * 0.8
}
/// Ember Peak: the five lava rivers run down the flanks from the crater at the bottom of canyons; the road crosses each on a narrow
/// bridge (half-width BRIDGE_HW) at the canyon's rim. Same river lines as the page's volcanoHazards.
pub const BRIDGE_HW: f64 = 2.3;
/// a wheel is carried out to the kerb stones' outer edge
pub const BRIDGE_EDGE: f64 = BRIDGE_HW + 0.3;
pub const EMBER_RIVERS: usize = 5;
/// the river's centre angle and half-width at radius `r`, and how fast the angle turns with r
#[inline] pub fn ember_river(j: usize, r: f64) -> (f64, f64, f64) {
    let jf = j as f64; let th0 = jf * std::f64::consts::PI * 2.0 / 5.0 + 0.45;
    (th0 + 0.12 * (r * 0.02 + jf).sin() + 0.05 * (r * 0.07 + 2.0 * jf).sin(), 4.6 + 1.4 * (r * 0.03 + jf).sin(), 0.0024 * (r * 0.02 + jf).cos() + 0.0035 * (r * 0.07 + 2.0 * jf).cos())
}
/// how far below the rim the canyon is cut at (x, z): (depth, river) - full depth over the lava, two stepped walls up to the rim
pub fn ember_canyon(x: f64, z: f64) -> (f64, usize) {
    let r = (x * x + z * z).sqrt(); if !(80.0..600.0).contains(&r) { return (0.0, 0); }
    let th = z.atan2(x); let mut best = (0.0, 0);
    for j in 0..EMBER_RIVERS {
        let (tc, hw, dt) = ember_river(j, r); let mut d = th - tc; d -= (d / std::f64::consts::TAU).round() * std::f64::consts::TAU;
        let lat = (r * d).abs() / (1.0 + (r * dt) * (r * dt)).sqrt(); if lat > hw + 8.0 { continue; }
        let lat = lat + (fbm(x * 0.13 + 3.0 * j as f64, z * 0.13, 2) - 0.5) * 1.6;
        let cut = 1.0 - 0.55 * smooth(hw + 0.6, hw + 2.3, lat) - 0.45 * smooth(hw + 3.7, hw + 5.8, lat);
        let depth = 13.0 * smooth(92.0, 140.0, r) * (1.0 - smooth(535.0, 580.0, r)) * cut; if depth > best.0 { best = (depth, j); }
    }
    best
}
/// how far (x, z) is from open lava: the crater lake or a river's edge
pub fn ember_lava_dist(x: f64, z: f64) -> f64 {
    let r = (x * x + z * z).sqrt(); let th = z.atan2(x); let mut best = (r - 85.0).max(0.0);
    if (92.0..575.0).contains(&r) { for j in 0..EMBER_RIVERS { let (tc, hw, dt) = ember_river(j, r); let mut d = th - tc; d -= (d / std::f64::consts::TAU).round() * std::f64::consts::TAU;
        let lat = (r * d).abs() / (1.0 + (r * dt) * (r * dt)).sqrt(); best = best.min((lat - hw).max(0.0)); } }
    best
}
/// the bridges, once the game has the stage: road line and height, which road points are on a bridge
pub struct Bridges { pub path: Vec<(f64, f64)>, pub tan: Vec<(f64, f64)>, pub roadh: Vec<f64>, pub on: Vec<bool> }

/// Touge: the wooded mountain the pass is cut into
fn tou_hill(x: f64, z: f64) -> f64 { ridged(x * 0.006 + 4.0, z * 0.006 - 9.0) * 16.0 + (fbm(x * 0.02, z * 0.02, 4) - 0.5) * 8.0 + (fbm(x * 0.08, z * 0.08, 4) - 0.5) * 0.8 }

/// Canyon: flat-topped mesas with cliff edges
fn canyon_mesa(x: f64, z: f64) -> f64 {
    let h = smooth(0.545, 0.57, fbm(x * 0.0065 + 21.0, z * 0.0065 + 7.0, 4)) * (24.0 + fbm(x * 0.03 + 5.0, z * 0.03, 4) * 14.0); let t = h / 6.0; let f = t - t.floor();
    (t.floor() + smooth(0.7, 1.0, f)) * 6.0
}

/// `path` = road centreline every metre, `roadh` = road height per point (both as the JS build computed them).
/// Run `f(row_index, row)` over all rows of a grid on every core.
fn par_rows<F: Fn(usize, &mut [f32]) + Sync>(g: &mut [f32], gnx: usize, f: F) {
    // the browser build has no threads: one pass on the calling thread
    #[cfg(target_arch = "wasm32")]
    { for (k, row) in g.chunks_mut(gnx).enumerate() { f(k, row); } return; }
    #[allow(unreachable_code)]
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 8);
    let rows = g.len() / gnx; let per = (rows + threads - 1) / threads;
    std::thread::scope(|sc| {
        for (c, chunk) in g.chunks_mut(per * gnx).enumerate() {
            let f = &f;
            sc.spawn(move || { for (k, row) in chunk.chunks_mut(gnx).enumerate() { f(c * per + k, row); } });
        }
    });
}

pub fn build(map: Map, path: &[(f64, f64)], roadh: &[f64], gxh: i32, gzh: i32) -> World {
    let gnx = (gxh * 2 + 1) as usize; let gnz = (gzh * 2 + 1) as usize; let n = gnx * gnz;
    let np = path.len();
    let mut pd = vec![20.0f32; n]; let mut pi = vec![0i16; n]; let mut h = vec![0f32; n];
    // splat each road point into the cells within 20 m
    for (k, &(px, pz)) in path.iter().enumerate() {
        let ci = js_round(px) as i64 + gxh as i64; let cj = js_round(pz) as i64 + gzh as i64;
        for j in (cj - 20)..=(cj + 20) { if j < 0 || j >= gnz as i64 { continue; }
            for i in (ci - 20)..=(ci + 20) { if i < 0 || i >= gnx as i64 { continue; }
                let dx = i as f64 - gxh as f64 - px; let dz = j as f64 - gzh as f64 - pz;
                let d = crate::physics::js_hypot(dx, dz); let id = j as usize * gnx + i as usize;
                if d < pd[id] as f64 { pd[id] = d as f32; pi[id] = k as i16; } } }
    }
    // far from the road: coarse nearest index in 8x8 blocks
    let mut j = 0; while j < gnz { let mut i = 0; while i < gnx {
        let mut need = false;
        'outer: for jj in j..(j + 8).min(gnz) { for ii in i..(i + 8).min(gnx) { if pd[jj * gnx + ii] >= 20.0 { need = true; break 'outer; } } }
        if need {
            let (mut best, mut bk) = (1e12f64, 0usize); let mut k = 0;
            while k < np { let (px, pz) = path[k]; let d = (px - (i as f64 - gxh as f64)).powi(2) + (pz - (j as f64 - gzh as f64)).powi(2); if d < best { best = d; bk = k; } k += 4; }
            for jj in j..(j + 8).min(gnz) { for ii in i..(i + 8).min(gnx) { let q = jj * gnx + ii; if pd[q] >= 20.0 { pi[q] = bk as i16; } } }
        }
        i += 8; } j += 8; }
    // prism: the deck banks into the curves (same recipe as the page: lateral turn of the tangent over +-10 m, clamped, box-smoothed)
    let bank = if map == Map::Prism { prism_bank(path) } else { Vec::new() };
    let tan: Vec<(f64, f64)> = if map == Map::Prism { (0..np).map(|i| path_tan(path, i)).collect() } else { Vec::new() };
    {
        let (pd_r, pi_r) = (&pd, &pi);
        par_rows(&mut h, gnx, |j, row| { for i in 0..gnx {
            let id = j * gnx + i; let x = i as f64 - gxh as f64; let z = j as f64 - gzh as f64; let pdv = pd_r[id] as f64;
            if map == Map::Prism {
                let k = pi_r[id] as usize; let (px, pz) = path[k]; let (tx, tz) = tan[k];
                let lat = (x - px) * -tz + (z - pz) * tx; let w2 = 1.0 - smooth(PRISM_ROAD + 2.4, PRISM_ROAD + 3.0, pdv);
                row[i] = ((roadh[k] - 0.12 + lat * bank[k]) * w2 + PRISM_VOID * (1.0 - w2)) as f32; continue;
            }
            let mut hv = terrain(map, x, z);
            if map == Map::Canyon { hv += canyon_mesa(x, z) * smooth(ROAD + 6.0, ROAD + 16.0, pdv); }
            if map == Map::Iceland { hv += ice_cliff(x, z) * smooth(ROAD + 7.0, ROAD + 16.5, pdv); }
            let w = 1.0 - match map { Map::Snow => smooth(ROAD + 1.5, ROAD + 22.0, pdv), Map::Canyon => smooth(ROAD + 1.0, ROAD + 9.0, pdv),
                Map::Lakes => smooth(ROAD + 1.0, ROAD + 14.0, pdv), Map::Iceland => smooth(ROAD + 1.0, ROAD + 12.0, pdv),
                Map::Touge => smooth(ROAD + 1.2, ROAD + 8.0, pdv), Map::Riviera => smooth(ROAD + 1.0, ROAD + 7.0, pdv), Map::Prism => 1.0, Map::Ember => smooth(ROAD + 1.2, ROAD + 14.0, pdv), Map::Forest => smooth(ROAD + 1.0, ROAD + 10.0, pdv) };
            hv = hv + (roadh[pi_r[id] as usize] - 0.12 - hv) * w;
            // ember peak: the lava canyons, cut below the raw slope (the road's bridges cross them at the rim), at least 10 m under the road
            if map == Map::Ember { let (dc, _) = ember_canyon(x, z); if dc > 0.0 { let floor = (terrain(map, x, z) - 13.0).min(roadh[pi_r[id] as usize] - 10.5); let k = dc / 13.0;
                hv = hv.min(hv + (floor - hv) * k.min(1.0)); } }
            if map == Map::Lakes && hv < WL { hv = WL - (1.6f64).min((WL - hv) * 0.2); }
            row[i] = hv as f32;
        } });
    }
    World { map, wave: Default::default(), wave_t: Default::default(), bridge: Default::default(), gxh, gzh, gnx, gnz, pd, pi, h }
}

/// Unit tangent of the centreline at `i` (from the points 2 m either side), as the page computes TAN.
pub fn path_tan(path: &[(f64, f64)], i: usize) -> (f64, f64) {
    let np = path.len(); let a = path[(i + np - 2) % np]; let b = path[(i + 2) % np];
    let (dx, dz) = (b.0 - a.0, b.1 - a.1); let l = crate::physics::js_hypot(dx, dz); (dx / l, dz / l)
}
/// Prism banking: height gained per metre of lateral offset at each path point.
pub fn prism_bank(path: &[(f64, f64)]) -> Vec<f64> {
    let np = path.len(); let tan: Vec<(f64, f64)> = (0..np).map(|i| path_tan(path, i)).collect();
    let mut raw: Vec<f64> = (0..np).map(|i| { let a = tan[(i + np - 10) % np]; let b = tan[(i + 10) % np]; let t = tan[i];
        let dr = (b.0 - a.0) * -t.1 + (b.1 - a.1) * t.0; clamp(-0.9 * dr, -0.24, 0.24) }).collect();
    for _ in 0..3 { let c = raw.clone(); for i in 0..np { let mut s = 0.0; for k in -15i64..=15 { s += c[((i as i64 + k + np as i64) % np as i64) as usize]; } raw[i] = s / 31.0; } }
    // the page keeps these in a Float32Array
    raw.iter().map(|&v| v as f32 as f64).collect()
}

/// Prism: the deck as the engine drives on it - the road line, its height and banking, and the travelling waves along it. `secs` =
/// [from, to, amplitude, wavelength, period] in metres of road and seconds; a section covering the whole lap has no ramps (its
/// wavelength divides the lap so it closes on itself).
pub struct Wave { pub path: Vec<(f64, f64)>, pub tan: Vec<(f64, f64)>, pub roadh: Vec<f64>, pub bank: Vec<f64>, pub secs: Vec<[f64; 5]> }
impl Wave {
    pub fn new(path: &[[f64; 2]], tan: &[[f64; 2]], roadh: &[f64], secs: &[[f64; 5]]) -> Wave {
        let path: Vec<(f64, f64)> = path.iter().map(|p| (p[0], p[1])).collect(); let bank = prism_bank(&path);
        Wave { tan: tan.iter().map(|t| (t[0], t[1])).collect(), roadh: roadh.to_vec(), bank, secs: secs.to_vec(), path }
    }
    /// the still deck at `s` metres along the road and `lat` across it (as the page draws it: height and banking blended between points)
    pub fn deck(&self, s: f64, lat: f64) -> f64 {
        let np = self.path.len(); let k0 = (s.floor() as usize).min(np - 1); let k1 = (k0 + 1) % np; let f = s - k0 as f64;
        let h = self.roadh[k0] + (self.roadh[k1] - self.roadh[k0]) * f; let b = self.bank[k0] + (self.bank[k1] - self.bank[k0]) * f;
        h - 0.12 + lat * b
    }
    /// deck lift at `s` metres along the road, at time `t`
    pub fn at(&self, s: f64, t: f64) -> f64 {
        let np = self.path.len() as f64; let tau = std::f64::consts::TAU; let mut h = 0.0;
        for &[s0, s1, a, lam, per] in &self.secs {
            if lam <= 0.0 || per <= 0.0 { continue; }
            if s1 - s0 >= np - 0.5 { h += a * (tau * (s / lam - t / per)).sin(); continue; }
            if s <= s0 || s >= s1 { continue; }
            let e = smooth(s0, s0 + 40.0, s) * (1.0 - smooth(s1 - 40.0, s1, s));
            h += a * e * (tau * ((s - s0) / lam - t / per)).sin();
        }
        h
    }
    /// can the deck at `s` ever move (any section reaching it)?
    pub fn live(&self, s: f64) -> bool { let np = self.path.len() as f64; self.secs.iter().any(|q| q[1] - q[0] >= np - 0.5 || (s > q[0] && s < q[1])) }
}

impl World {
    #[inline]
    fn bil(&self, g: &[f32], x: f64, z: f64) -> f64 {
        let fx = clamp(x + self.gxh as f64, 0.0, self.gnx as f64 - 1.001); let fz = clamp(z + self.gzh as f64, 0.0, self.gnz as f64 - 1.001);
        let i = fx as usize; let j = fz as usize; let u = fx - i as f64; let v = fz - j as f64; let k = j * self.gnx + i;
        (g[k] as f64 * (1.0 - u) + g[k + 1] as f64 * u) * (1.0 - v) + (g[k + self.gnx] as f64 * (1.0 - u) + g[k + self.gnx + 1] as f64 * u) * v
    }
    /// ground height. Prism: the deck itself, exact to its drawn edge, and nothing beyond it (no grid slope to trip a wheel at the edge)
    #[inline] pub fn height(&self, x: f64, z: f64) -> f64 {
        if self.map == Map::Prism { if let Some(h) = self.prism_height(x, z) { return h; } }
        if self.map == Map::Ember { if let Some((h, _)) = self.bridge_deck(x, z) { return h; } }
        self.bil(&self.h, x, z)
    }
    /// ember peak: on a bridge's road span? Then the deck height there (exact to its edge) and how far off the centre line
    pub fn bridge_deck(&self, x: f64, z: f64) -> Option<(f64, f64)> {
        let b = self.bridge.get()?; let k = self.bridge_k(b, x, z)?; let np = b.path.len(); let (px, pz) = b.path[k]; let (tx, tz) = b.tan[k];
        let lat = (x - px) * -tz + (z - pz) * tx; if lat.abs() > BRIDGE_EDGE { return None; }
        let s = (x - px) * tx + (z - pz) * tz; let k2 = if s >= 0.0 { (k + 1) % np } else { (k + np - 1) % np };
        Some((b.roadh[k] + (b.roadh[k2] - b.roadh[k]) * s.abs() - 0.12, lat))
    }
    /// ember peak: the road point a bridge's span covers here, if any
    fn bridge_k(&self, b: &Bridges, x: f64, z: f64) -> Option<usize> {
        let i = clamp((x + self.gxh as f64).round(), 0.0, self.gnx as f64 - 1.0) as usize; let j = clamp((z + self.gzh as f64).round(), 0.0, self.gnz as f64 - 1.0) as usize;
        let k = (self.pi[j * self.gnx + i].max(0) as usize).min(b.path.len().saturating_sub(1)); if b.on.get(k).copied().unwrap_or(false) && self.bil(&self.pd, x, z) < ROAD + 3.0 { Some(k) } else { None }
    }
    /// ember peak: over a bridge's span (on the deck or beside it)
    pub fn in_bridge_span(&self, x: f64, z: f64) -> bool { self.bridge.get().map_or(false, |b| self.bridge_k(b, x, z).is_some()) }
    /// the static ground only (no prism waves)
    #[inline] pub fn base_height(&self, x: f64, z: f64) -> f64 { self.bil(&self.h, x, z) }
    pub fn set_wave_time(&self, t: f64) { self.wave_t.store(t.to_bits(), std::sync::atomic::Ordering::Relaxed); }
    pub fn wave_time(&self) -> f64 { f64::from_bits(self.wave_t.load(std::sync::atomic::Ordering::Relaxed)) }
    /// metres along the road for a point on (or near) the prism deck: the nearest index plus the offset along its tangent, so it runs smoothly
    pub fn road_s(&self, wv: &Wave, x: f64, z: f64) -> f64 { self.road_frame(wv, x, z).2 }
    /// nearest road index, lateral offset (+ = right of the direction of travel... as the page's `lat`) and metres along the road
    pub fn road_frame(&self, wv: &Wave, x: f64, z: f64) -> (usize, f64, f64) {
        let i = clamp((x + self.gxh as f64).round(), 0.0, self.gnx as f64 - 1.0) as usize; let j = clamp((z + self.gzh as f64).round(), 0.0, self.gnz as f64 - 1.0) as usize;
        let np = wv.path.len(); let k = (self.pi[j * self.gnx + i].max(0) as usize).min(np.saturating_sub(1)); let (px, pz) = wv.path[k]; let (tx, tz) = wv.tan[k];
        (k, (x - px) * -tz + (z - pz) * tx, (k as f64 + (x - px) * tx + (z - pz) * tz).rem_euclid(np as f64))
    }
    fn prism_height(&self, x: f64, z: f64) -> Option<f64> {
        let wv = self.wave.get()?; if wv.path.is_empty() || wv.roadh.len() != wv.path.len() { return None; }
        let (_, lat, s) = self.road_frame(wv, x, z);
        Some(if lat.abs() <= PRISM_EDGE { wv.deck(s, lat) + wv.at(s, self.wave_time()) } else { PRISM_VOID })
    }
    #[inline] pub fn path_dist(&self, x: f64, z: f64) -> f64 { self.bil(&self.pd, x, z) }
    /// the road's half-width here
    #[inline] pub fn road(&self) -> f64 { road_half(self.map) }

    /// Packed for the JS side: [pd f32 * n][h f32 * n][pi i16 * n], little endian.
    pub fn to_bytes(&self) -> Vec<u8> {
        let n = self.pd.len(); let mut out = Vec::with_capacity(n * 10);
        for v in &self.pd { out.extend_from_slice(&v.to_le_bytes()); }
        for v in &self.h { out.extend_from_slice(&v.to_le_bytes()); }
        for v in &self.pi { out.extend_from_slice(&v.to_le_bytes()); }
        out
    }
}
