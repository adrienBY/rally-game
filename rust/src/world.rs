//! Stage generation: distance-to-road field, nearest-road index and terrain height on a 1 m grid.
use crate::noise::*;

pub const ROAD: f64 = 3.4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Map { Forest = 0, Snow = 1, Canyon = 2, Lakes = 3, Iceland = 4, Touge = 5, Riviera = 6 }
impl Map { pub fn from_id(i: i32) -> Map { match i { 1 => Map::Snow, 2 => Map::Canyon, 3 => Map::Lakes, 4 => Map::Iceland, 5 => Map::Touge, 6 => Map::Riviera, _ => Map::Forest } } }
/// Lakes: water level
pub const WL: f64 = -4.5;

pub struct World {
    pub map: Map,
    pub gxh: i32, pub gzh: i32, pub gnx: usize, pub gnz: usize,
    pub pd: Vec<f32>, pub pi: Vec<i16>, pub h: Vec<f32>,
}

fn terrain(map: Map, x: f64, z: f64) -> f64 {
    match map {
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
    {
        let (pd_r, pi_r) = (&pd, &pi);
        par_rows(&mut h, gnx, |j, row| { for i in 0..gnx {
            let id = j * gnx + i; let x = i as f64 - gxh as f64; let z = j as f64 - gzh as f64; let pdv = pd_r[id] as f64;
            let mut hv = terrain(map, x, z);
            if map == Map::Canyon { hv += canyon_mesa(x, z) * smooth(ROAD + 6.0, ROAD + 16.0, pdv); }
            if map == Map::Iceland { hv += ice_cliff(x, z) * smooth(ROAD + 7.0, ROAD + 16.5, pdv); }
            let w = 1.0 - match map { Map::Snow => smooth(ROAD + 1.5, ROAD + 22.0, pdv), Map::Canyon => smooth(ROAD + 1.0, ROAD + 9.0, pdv),
                Map::Lakes => smooth(ROAD + 1.0, ROAD + 14.0, pdv), Map::Iceland => smooth(ROAD + 1.0, ROAD + 12.0, pdv),
                Map::Touge => smooth(ROAD + 1.2, ROAD + 8.0, pdv), Map::Riviera => smooth(ROAD + 1.0, ROAD + 7.0, pdv), Map::Forest => smooth(ROAD + 1.0, ROAD + 10.0, pdv) };
            hv = hv + (roadh[pi_r[id] as usize] - 0.12 - hv) * w;
            if map == Map::Lakes && hv < WL { hv = WL - (1.6f64).min((WL - hv) * 0.2); }
            row[i] = hv as f32;
        } });
    }
    World { map, gxh, gzh, gnx, gnz, pd, pi, h }
}

impl World {
    #[inline]
    fn bil(&self, g: &[f32], x: f64, z: f64) -> f64 {
        let fx = clamp(x + self.gxh as f64, 0.0, self.gnx as f64 - 1.001); let fz = clamp(z + self.gzh as f64, 0.0, self.gnz as f64 - 1.001);
        let i = fx as usize; let j = fz as usize; let u = fx - i as f64; let v = fz - j as f64; let k = j * self.gnx + i;
        (g[k] as f64 * (1.0 - u) + g[k + 1] as f64 * u) * (1.0 - v) + (g[k + self.gnx] as f64 * (1.0 - u) + g[k + self.gnx + 1] as f64 * u) * v
    }
    #[inline] pub fn height(&self, x: f64, z: f64) -> f64 { self.bil(&self.h, x, z) }
    #[inline] pub fn path_dist(&self, x: f64, z: f64) -> f64 { self.bil(&self.pd, x, z) }

    /// Packed for the JS side: [pd f32 * n][h f32 * n][pi i16 * n], little endian.
    pub fn to_bytes(&self) -> Vec<u8> {
        let n = self.pd.len(); let mut out = Vec::with_capacity(n * 10);
        for v in &self.pd { out.extend_from_slice(&v.to_le_bytes()); }
        for v in &self.h { out.extend_from_slice(&v.to_le_bytes()); }
        for v in &self.pi { out.extend_from_slice(&v.to_le_bytes()); }
        out
    }
}
