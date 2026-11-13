//! Stage generation: distance-to-road field, nearest-road index and terrain height on a 1 m grid.
use crate::noise::*;

pub const ROAD: f64 = 3.4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Map { Forest = 0, Snow = 1, Canyon = 2, Lakes = 3 }
impl Map { pub fn from_id(i: i32) -> Map { match i { 1 => Map::Snow, 2 => Map::Canyon, 3 => Map::Lakes, _ => Map::Forest } } }
/// Lakes: water level
pub const WL: f64 = -4.5;

pub struct World {
    pub map: Map,
    pub gxh: i32, pub gzh: i32, pub gnx: usize, pub gnz: usize,
    pub pd: Vec<f32>, pub pi: Vec<i16>, pub h: Vec<f32>,
}

fn terrain(map: Map, x: f64, z: f64) -> f64 {
    match map {
        Map::Canyon => (fbm(x * 0.008 + 3.0, z * 0.008 - 2.0, 4) - 0.5) * 18.0 + (fbm(x * 0.05, z * 0.05, 4) - 0.5) * 1.2,
        Map::Lakes => (fbm(x * 0.006 + 5.0, z * 0.006 + 1.0, 4) - 0.5) * 44.0 + (fbm(x * 0.02, z * 0.02, 4) - 0.5) * 8.0 + (fbm(x * 0.07, z * 0.07, 4) - 0.5) * 1.2,
        Map::Snow => ridged(x * 0.0052 + 3.0, z * 0.0052 - 7.0) * 58.0 + (fbm(x * 0.016, z * 0.016, 4) - 0.5) * 12.0 + (fbm(x * 0.07, z * 0.07, 4) - 0.5) * 1.4,
        Map::Forest => (fbm(x * 0.014 + 10.0, z * 0.014 - 4.0, 4) - 0.5) * 16.0 + (fbm(x * 0.06, z * 0.06, 4) - 0.5) * 1.6,
    }
}
/// Canyon: flat-topped mesas with cliff edges
fn canyon_mesa(x: f64, z: f64) -> f64 {
    smooth(0.53, 0.585, fbm(x * 0.0065 + 21.0, z * 0.0065 + 7.0, 4)) * (24.0 + fbm(x * 0.03 + 5.0, z * 0.03, 4) * 14.0)
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
            let w = 1.0 - match map { Map::Snow => smooth(ROAD + 1.5, ROAD + 22.0, pdv), Map::Canyon => smooth(ROAD + 1.0, ROAD + 9.0, pdv),
                Map::Lakes => smooth(ROAD + 1.0, ROAD + 14.0, pdv), Map::Forest => smooth(ROAD + 1.0, ROAD + 10.0, pdv) };
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
