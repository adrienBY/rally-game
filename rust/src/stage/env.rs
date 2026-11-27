//! The land: the touge/riviera drop sides and lanterns, the stage ground and the road ribbon, the land out to the horizon, water,
//! the sky dome, the riviera's clouds and sun, the distant ranges.
use super::canvas::{self, Ctx, Style};
use super::road::{canyon_mesa, terrain, M};
use super::three::*;
use super::{hyp, sm, CullE, Lan, St};
use crate::noise::{clamp, fbm, ridged, vnoise};
use std::f64::consts::PI;

impl St {
    /// ground palettes of the warm stages (canyon, lakes, touge, riviera, ember), shared by the stage ground and the land beyond
    pub fn ground_col(&self, _x: f64, _z: f64, h: f64, slope: f64, n: f64, n2: f64) -> [f64; 3] {
        let wl = crate::world::WL;
        if self.riv {
            let (mut r, mut g, mut b) = (0.72 + n * 0.1, 0.56 + n * 0.08, 0.36 + n * 0.05);
            let scrub = sm(0.42, 0.6, n2) * 0.8; r += (0.4 - r) * scrub; g += (0.46 - g) * scrub; b += (0.2 - b) * scrub;
            let dry = sm(0.62, 0.74, n2) * 0.5; r += (0.82 - r) * dry; g += (0.68 - g) * dry; b += (0.4 - b) * dry;
            let rock = sm(0.5, 0.95, slope + (n - 0.5) * 0.3); r += (0.92 + n * 0.08 - r) * rock; g += (0.82 + n * 0.07 - g) * rock; b += (0.64 + n * 0.05 - b) * rock;
            if h < 1.5 { let k = sm(1.5, -1.0, h); r += (0.24 - r) * k; g += (0.21 - g) * k; b += (0.2 - b) * k; }
            if h < -2.0 { let k = sm(-2.0, -10.0, h); r += (0.36 - r) * k; g += (0.62 - g) * k; b += (0.58 - b) * k; }
            return [r, g, b];
        }
        if self.vol {
            let (mut r, mut g, mut b) = (0.12 + n * 0.05, 0.1 + n * 0.04, 0.1 + n * 0.04);
            let ash = sm(0.5, 0.64, n2) * (1.0 - sm(0.5, 0.9, slope)) * 0.8; r += (0.36 - r) * ash; g += (0.34 - g) * ash; b += (0.33 - b) * ash;
            let rust = sm(0.45, 0.85, slope + (n - 0.5) * 0.4) * 0.8; r += (0.36 + n * 0.08 - r) * rust; g += (0.16 + n * 0.04 - g) * rust; b += (0.11 - b) * rust;
            let top = sm(170.0, 230.0, h); r += (0.22 - r) * top * 0.6; g += (0.12 - g) * top * 0.6; b += (0.1 - b) * top * 0.6;
            return [r, g, b];
        }
        if self.tou {
            let (mut r, mut g, mut b) = (0.24 + n * 0.08, 0.17 + n * 0.05, 0.11 + n * 0.03);
            let red = sm(0.52, 0.66, n2); r += (0.52 - r) * red; g += (0.16 - g) * red; b += (0.08 - b) * red;
            let moss = sm(0.4, 0.3, n2) * 0.8; r += (0.18 - r) * moss; g += (0.27 - g) * moss; b += (0.12 - b) * moss;
            let rock = sm(0.55, 0.9, slope + (n - 0.5) * 0.3); r += (0.4 - r) * rock; g += (0.39 - g) * rock; b += (0.41 - b) * rock;
            return [r, g, b];
        }
        if self.can {
            let (mut r, mut g, mut b) = (0.8 + n * 0.08, 0.53 + n * 0.06, 0.34 + n * 0.04);
            let rock = sm(0.45, 0.85, slope + (n - 0.5) * 0.3); let bi = (h / 2.6 + n * 0.8).floor() as i64; let band = [0.15, 0.6, 0.32, 0.9, 0.45, 0.72][(((bi % 6) + 6) % 6) as usize];
            let rc = [0.55 + band * 0.3, 0.24 + band * 0.24, 0.15 + band * 0.17]; r += (rc[0] - r) * rock; g += (rc[1] - g) * rock; b += (rc[2] - b) * rock;
            let dark = (n2 - 0.55).max(0.0) * 0.5; return [r * (1.0 - dark), g * (1.0 - dark), b * (1.0 - dark)];
        }
        let lit = (n2 - 0.5).max(0.0) * 1.4;
        let (mut r, mut g, mut b) = (0.3 + n * 0.1 + lit * 0.1, 0.42 + n * 0.12, 0.2 + n * 0.05);
        let heather = sm(0.58, 0.7, n2) * 0.6; r += (0.46 - r) * heather; g += (0.34 - g) * heather; b += (0.36 - b) * heather;
        let rock = sm(0.55, 0.95, slope + (n - 0.5) * 0.3); r += (0.46 - r) * rock; g += (0.46 - g) * rock; b += (0.44 - b) * rock;
        let shore = 1.0 - sm(wl + 0.2, wl + 1.0, h); r += (0.56 - r) * shore; g += (0.5 - g) * shore; b += (0.38 - b) * shore;
        if h < wl - 0.05 { let k = sm(0.0, 1.2, wl - h); r += (0.2 - r) * k; g += (0.23 - g) * k; b += (0.2 - b) * k; }
        [r, g, b]
    }
    pub fn ex(&self) -> f64 { (self.r.gxh - 2) as f64 }
    pub fn ez(&self) -> f64 { (self.r.gzh - 2) as f64 }
    pub fn far(&self) -> f64 { if self.forest { 950.0 } else if self.snow { 1400.0 } else if self.can { 1700.0 } else { 1500.0 } }
    pub fn relief(&self, x: f64, z: f64) -> f64 {
        if self.riv { let r = hyp(x, z) + 1.0; let sec = sm(0.25, 0.65, (x * 0.94 - z * 0.34) / r); return sec * sm(900.0, 1400.0, r) * (30.0 + ridged(x * 0.0021 + 5.0, z * 0.0021 - 1.0).powf(1.5) * 420.0); }
        if self.vol { return (fbm(x * 0.004 + 3.0, z * 0.004 - 7.0, 3) - 0.5) * 50.0 + ridged(x * 0.0025 + 1.0, z * 0.0025 + 4.0) * 160.0 * sm(900.0, 1500.0, hyp(x, z)); }
        if self.tou { return -135.0 + (fbm(x * 0.004, z * 0.004, 3) - 0.5) * 30.0 + ridged(x * 0.002 + 5.0, z * 0.002 - 1.0) * 520.0 * sm(760.0, 1400.0, hyp(x, z)); }
        if self.can { let mut h = sm(0.54, 0.565, fbm(x * 0.0032 + 9.0, z * 0.0032 - 3.0, 3)) * (70.0 + fbm(x * 0.008, z * 0.008, 3) * 60.0) + sm(0.86, 0.9, vnoise(x * 0.018 + 3.0, z * 0.018)) * 55.0;
            let t = h / 13.0; let f = t - t.floor(); h = (t.floor() + sm(0.72, 1.0, f)) * 13.0; return (fbm(x * 0.0025 + 9.0, z * 0.0025 - 3.0, 3) - 0.5) * 30.0 + h; }
        if self.lak { return (fbm(x * 0.003 + 4.0, z * 0.003 - 6.0, 3) - 0.5) * 110.0 + 6.0; }
        if self.snow { return ridged(x * 0.0024 - 3.0, z * 0.0024 + 6.0) * 460.0 - 80.0; }
        (fbm(x * 0.0035 + 2.0, z * 0.0035 - 5.0, 3) - 0.5) * 120.0 + 18.0
    }
    fn detail(&self, x: f64, z: f64) -> f64 { let t = terrain(self.m, x, z); if self.can { t + canyon_mesa(x, z) } else { t } }
    pub fn far_h(&self, x: f64, z: f64) -> f64 {
        let (ex, ez) = (self.ex(), self.ez()); let cx = clamp(x, -ex, ex); let cz = clamp(z, -ez, ez); let d = hyp(x - cx, z - cz);
        if d <= 0.0 { return self.height(x, z); }
        self.height(cx, cz) + self.detail(x, z) - self.detail(cx, cz) + self.relief(x, z) * sm(0.0, if self.can { 500.0 } else if self.lak { 450.0 } else if self.snow { 480.0 } else if self.tou { 420.0 } else { 380.0 }, d)
    }
    /// baked sun shadows of every tree canopy and trunk on a 1 m grid, then the ground, road and grass colours darkened under them
    pub fn bake_sun_shadows(&mut self) {
        let sun = self.sun; let sh = hyp(sun.x, sun.z); let sh = if sh == 0.0 { 1.0 } else { sh }; let elev = sun.y.atan2(sh).max(0.36); let (tan_e, sin_e) = (elev.tan(), elev.sin());
        let (dx, dz) = (-sun.x / sh, -sun.z / sh); let w = &self.r.world; let (gxh, gzh, gnx, gnz) = (w.gxh as i64, w.gzh as i64, w.gnx as i64, w.gnz as i64);
        let mut ss = std::mem::take(&mut self.ss);
        let mut put = |x: f64, z: f64, v: f64| { let gx = super::jr(x) as i64 + gxh; let gz = super::jr(z) as i64 + gzh; if gx < 0 || gz < 0 || gx >= gnx || gz >= gnz { return; } let q = (gz * gnx + gx) as usize; ss[q] = (1.0 - (1.0 - ss[q] as f64) * (1.0 - v)) as f32; };
        for t in &self.trees { let Some(c) = t.cv else { continue };
            let off = (c.y - t.y).max(0.0) / tan_e; let (sx, sz) = (c.x + dx * off, c.z + dz * off);
            let wd = c.r; let l = (c.r * 3.0).min(c.l.unwrap_or(c.r * 0.6) / tan_e + c.r / sin_e * 0.4 + c.r * 0.5); let span = wd.max(l).ceil() as i64 + 1;
            for j in -span..=span { for i in -span..=span { let x = super::jr(sx) + i as f64; let z = super::jr(sz) + j as f64; let (ax, az) = (x - sx, z - sz);
                let a = ax * dx + az * dz; let b = -ax * dz + az * dx; let q = (a / l).powi(2) + (b / wd).powi(2); if q >= 1.0 { continue; }
                let dap = 0.78 + 0.22 * vnoise(x * 0.38 + 11.0, z * 0.38 - 7.0); put(x, z, (c.d * 1.12).min(1.0) * sm(1.0, 0.4, q) * dap); } }
            let tl = (off - l * 0.6).max(0.0); let wt = c.tr + 0.35; let mut s2 = 0.0;
            while s2 <= tl { let (x, z) = (t.x + dx * s2, t.z + dz * s2); put(x, z, 0.5); put(x - dz * wt * 0.5, z + dx * wt * 0.5, 0.25); put(x + dz * wt * 0.5, z - dx * wt * 0.5, 0.25); s2 += 0.5; } }
        let (gnx, gnz) = (gnx as usize, gnz as usize);
        for _ in 0..2 { let t = ss.clone(); for z in 1..gnz - 1 { for x in 1..gnx - 1 { let q = z * gnx + x; ss[q] = (t[q] as f64 * 0.5 + (t[q - 1] as f64 + t[q + 1] as f64 + t[q - gnx] as f64 + t[q + gnx] as f64) * 0.125) as f32; } } }
        self.ss = ss;
        let mut gs = self.baked_ground.clone(); if let Some(r) = self.baked_road { gs.push(r); }
        for g in gs { let n = self.sc.geos[g].count();
            for i in 0..n { let (x, z) = (self.sc.geos[g].pos[i * 3] as f64, self.sc.geos[g].pos[i * 3 + 2] as f64); let o = self.sun_shadow_at(x, z); if o < 0.004 { continue; }
                let m = [self.shadow_mul(o, 0), self.shadow_mul(o, 1), self.shadow_mul(o, 2)]; let c = self.sc.geos[g].col.as_mut().unwrap(); for k in 0..3 { c[i * 3 + k] = (c[i * 3 + k] as f64 * m[k]) as f32; } } }
    }
}

pub fn drops_and_lanterns(st: &mut St) {
    if !(st.tou || st.riv) { return; }
    let np = st.np(); let road = st.road(); let mut raw = vec![0i8; np];
    for i in 0..np { let (mut best, mut bs) = (0.0, 0i8);
        for sd in [-1i8, 1] { let (x, z) = st.side(i, (road + 10.0) * sd as f64); let d = st.r.roadh[i] - st.height(x, z); if d > best { best = d; bs = sd; } }
        raw[i] = if best > 1.6 { bs } else { 0 }; }
    for i in 0..np { let (mut a, mut b) = (0, 0); for d in -6i64..=6 { let v = raw[st.r.wrap(i as i64 + d)]; if v > 0 { a += 1; } else if v < 0 { b += 1; } } st.drop[i] = if a >= 7 { 1 } else if b >= 7 { -1 } else { 0 }; }
    if st.tou { let mut i = 10; while i + 10 < np {
        let sd = if st.drop[i] != 0 { -st.drop[i] as f64 } else if (i / 34) % 2 == 1 { 1.0 } else { -1.0 }; let q = st.side(i, (road + 2.0) * sd);
        if !(st.path_dist(q.0, q.1) < road + 1.3 || (st.r.ramp_i >= 0 && st.r.cdist(i, st.r.ramp_i as usize) < 20)) {
            let y = st.height(q.0, q.1); st.lan.push(Lan { x: q.0, z: q.1, y, i }); st.obstacles.push([q.0, q.1, 0.45, 0.0]); }
        i += 34; } }
    for (k, l) in st.lan.iter().enumerate() { st.lg.entry((l.x / 16.0).floor() as i64 * 4096 + (l.z / 16.0).floor() as i64).or_default().push(k); }
}

pub fn ground_and_road(st: &mut St) {
    let (gxh, gzh) = (st.r.gxh as f64, st.r.gzh as f64); let road = st.road(); let np = st.np();
    let (sxw, szw) = (gxh * 2.0 - 4.0, gzh * 2.0 - 4.0); let (sx, sz) = (super::jr(sxw / 2.0) as usize, super::jr(szw / 2.0) as usize);
    let lamp = [1.0, 0.6, 0.26];
    if !st.pri {
        let mut geo = plane(sxw, szw, sx, sz); geo.rotate_x(-PI / 2.0);
        let n = geo.count(); let mut col = vec![0f32; n * 3]; let mut uv = geo.uv.take().unwrap();
        for i in 0..n {
            let (x, z) = (geo.pos[i * 3] as f64, geo.pos[i * 3 + 2] as f64); geo.pos[i * 3 + 1] = st.height(x, z) as f32;
            if st.riv { let k = st.path_idx(x, z); if st.r.tun_at(k).is_some() && st.path_dist(x, z) < road + 9.0 { geo.pos[i * 3 + 1] = st.height(x, z).min(st.r.roadh[k] - 0.15) as f32; } }
            let pd = st.path_dist(x, z); let nn = fbm(x * 0.15, z * 0.15, 4); let n2 = fbm(x * 0.04 + 7.0, z * 0.04, 4);
            let c3: [f64; 3];
            if st.gc {
                let slope = hyp(st.height(x + 1.0, z) - st.height(x - 1.0, z), st.height(x, z + 1.0) - st.height(x, z - 1.0)) / 2.0; let hh = st.height(x, z);
                let mut c = st.ground_col(x, z, hh, slope, nn, n2);
                let cdv = if st.vol { crate::world::ember_canyon(x, z).0 } else { 0.0 };
                let p = clamp((road + 1.1 - pd) / 1.4, 0.0, 1.0) * (0.75 + nn * 0.3) * (1.0 - sm(0.4, 2.0, cdv));
                let rc = if st.vol { [0.16, 0.13, 0.13] } else if st.can { [0.8, 0.6, 0.42] } else if st.tou { [0.2, 0.19, 0.2] } else if st.riv { [0.36, 0.34, 0.33] } else { [0.5, 0.43, 0.33] };
                for q in 0..3 { c[q] += (rc[q] - c[q]) * p; }
                if cdv > 0.5 { let gl = (cdv / 13.0).powf(2.2); let dk = sm(0.5, 4.0, cdv) * 0.5; for q in 0..3 { c[q] *= 1.0 - dk; } c[0] += (0.95 - c[0]) * gl * 0.8; c[1] += (0.3 - c[1]) * gl * 0.8; c[2] += (0.08 - c[2]) * gl * 0.8; }
                if st.riv { let k = st.path_idx(x, z); if st.r.in_village(k) && pd < road + 3.4 { let s = 0.72 + nn * 0.08; c = [s, s * 0.95, s * 0.87]; }
                    if let Some(ts) = st.r.tun_shade(k, pd) { for q in 0..3 { c[q] *= ts[q]; } } }
                let sh = 1.0 - st.shade_at(x, z) * 0.45; let lk = if st.tou { st.lamp_at(x, z) * 1.5 } else { 0.0 };
                c3 = [c[0] * sh + lamp[0] * lk, c[1] * sh + lamp[1] * lk, c[2] * sh + lamp[2] * lk];
            } else if st.snow {
                let slope = hyp(st.height(x + 1.0, z) - st.height(x - 1.0, z), st.height(x, z + 1.0) - st.height(x, z - 1.0)) / 2.0;
                let rock = sm(0.62, 0.95, slope + (nn - 0.5) * 0.35) * if pd > road + 2.0 { 1.0 } else { 0.0 };
                let (mut r, mut g, mut b) = (0.8 + nn * 0.06, 0.84 + nn * 0.05, 0.92 + nn * 0.04);
                r += (0.25 - r) * rock; g += (0.26 - g) * rock; b += (0.3 - b) * rock;
                let p = clamp((road + 1.6 - pd) / 1.8, 0.0, 1.0) * 0.6; r += (0.66 - r) * p; g += (0.7 - g) * p; b += (0.78 - b) * p;
                let sh = 1.0 - st.shade_at(x, z) * 0.32; c3 = [r * sh * 0.96, g * sh * 0.98, b * sh];
            } else {
                let (mut r, mut g, mut b) = (0.26 + nn * 0.12, 0.38 + nn * 0.14, 0.17 + nn * 0.06);
                let lit = (n2 - 0.5).max(0.0) * 1.6; r += lit * 0.12; g -= lit * 0.02; b -= lit * 0.02;
                let p = clamp((road + 1.0 - pd) / 1.4, 0.0, 1.0) * (0.8 + nn * 0.3); r += (0.47 - r) * p; g += (0.39 - g) * p; b += (0.27 - b) * p;
                let sh = 1.0 - st.shade_at(x, z) * 0.5; c3 = [r * sh, g * sh, b * sh * 1.05];
            }
            col[i * 3] = c3[0] as f32; col[i * 3 + 1] = c3[1] as f32; col[i * 3 + 2] = c3[2] as f32;
            uv[i * 2] = (x / 3.2) as f32; uv[i * 2 + 1] = (z / 3.2) as f32;
        }
        geo.col = Some(col); geo.uv = Some(uv); geo.compute_normals();
        let gm = st.mat(Mat::lambert().vc().map(st.t_ground)); let w1 = sx + 1; let tt = 64;
        let (pos, nrm, cl, uvv) = (&geo.pos, geo.nrm.as_ref().unwrap(), geo.col.as_ref().unwrap(), geo.uv.as_ref().unwrap());
        let mut tiles: Vec<(Geo, usize, usize)> = Vec::new();
        let mut ty = 0; while ty < sz { let mut tx = 0; while tx < sx {
            let (x1, y1) = ((tx + tt).min(sx), (ty + tt).min(sz)); let (tw, th) = (x1 - tx + 1, y1 - ty + 1); let n = tw * th;
            let (mut p, mut nn, mut c, mut u2, mut ix) = (vec![0f32; n * 3], vec![0f32; n * 3], vec![0f32; n * 3], vec![0f32; n * 2], Vec::with_capacity((tw - 1) * (th - 1) * 6));
            for iy in 0..th { for ixx in 0..tw { let src = (ty + iy) * w1 + tx + ixx; let dst = iy * tw + ixx;
                for q in 0..3 { p[dst * 3 + q] = pos[src * 3 + q]; nn[dst * 3 + q] = nrm[src * 3 + q]; c[dst * 3 + q] = cl[src * 3 + q]; } u2[dst * 2] = uvv[src * 2]; u2[dst * 2 + 1] = uvv[src * 2 + 1]; } }
            for iy in 0..th - 1 { for ixx in 0..tw - 1 { let a = (ixx + tw * iy) as u32; let b = (ixx + tw * (iy + 1)) as u32; let cc = (ixx + 1 + tw * (iy + 1)) as u32; let d = (ixx + 1 + tw * iy) as u32; ix.extend_from_slice(&[a, b, d, b, cc, d]); } }
            let mut tg = Geo::new(p).with_nrm(nn).with_col(c).with_uv(u2).with_index(ix); tg.compute_bs(); tiles.push((tg, tw, th));
            tx += tt; } ty += tt; }
        for (tg, tw, th) in tiles { let bs = tg.bs.unwrap(); let g = st.geo(tg); st.baked_ground.push(g); let o = st.sc.mesh(g, gm); st.sc.objs[o].dyn_ = true; st.sc.objs[o].grid = Some((tw, th)); st.add(o);
            st.cull.push(CullE { o, c: v3(bs[0], bs[1], bs[2]), r: bs[3], d: st.fogcut }); }
    }
    let ac = [-1.08, -0.78, -0.42, 0.0, 0.42, 0.78, 1.08]; let na = ac.len();
    let (mut p, mut uva, mut co, mut idx) = (vec![0f32; np * na * 3], vec![0f32; np * na * 2], vec![0f32; np * na * 3], Vec::new());
    for i in 0..np { let (px, pz) = (st.p(i)[0], st.p(i)[1]); let tn = st.t(i);
        for a in 0..na { let s = ac[a] * road; let (x, z) = (px - tn[1] * s, pz + tn[0] * s); let q = i * na + a;
            p[q * 3] = x as f32; p[q * 3 + 1] = ((if st.pri { st.r.roadh[i] - 0.12 + s * st.r.bank[i] } else { st.height(x, z) }) + 0.035) as f32; p[q * 3 + 2] = z as f32;
            uva[q * 2] = ((ac[a] + 1.08) / 2.16) as f32; uva[q * 2 + 1] = (i as f64 / 7.0) as f32;
            let edge = if (ac[a] as f64).abs() > 1.0 { if st.snow { 0.92 } else if st.can { 0.86 } else if st.tou || st.riv { 0.92 } else { 0.72 } } else if st.snow { 0.8 } else { 1.0 };
            let sh = (1.0 - st.shade_at(x, z) * 0.45) * edge; let lk = if st.tou { st.lamp_at(x, z) * 3.0 } else { 0.0 };
            let mut c = [sh + lamp[0] * lk, sh * if (ac[a] as f64).abs() > 1.0 { 1.06 } else { 1.0 } + lamp[1] * lk, sh + lamp[2] * lk];
            if st.riv { if let Some(ts) = st.r.tun_shade(i, 0.0) { for k in 0..3 { c[k] = (c[k] as f32 as f64) * ts[k]; } } }
            if st.pri { c = [1.0, 1.0, 1.0]; }
            for k in 0..3 { co[q * 3 + k] = c[k] as f32; } } }
    for i in 0..np { let n = (i + 1) % np; if st.r.bridge_on[i] || st.r.bridge_on[n] { continue; } for a in 0..na - 1 { let (a0, b0) = ((i * na + a) as u32, (n * na + a) as u32); idx.extend_from_slice(&[a0, b0, a0 + 1, a0 + 1, b0, b0 + 1]); } }
    let mut rg = Geo::new(p).with_uv(uva).with_col(co).with_index(idx); rg.compute_normals(); let rgid = st.geo(rg); st.baked_road = Some(rgid);
    let base = if st.pri { Mat::basic() } else { Mat::lambert() };
    let m = st.mat(base.map(st.t_gravel).vc().double().po(-1.0, -2.0)); let o = st.sc.mesh(rgid, m); st.sc.objs[o].culled = false;
    if st.pri { st.sc.objs[o].dyn_ = true; st.sc.objs[o].wave = true; } st.add(o);
}

pub fn far_land(st: &mut St) {
    if st.pri { return; }
    let (ex, ez, far) = (st.ex(), st.ez(), st.far());
    let axis = |e: f64, he: f64| -> Vec<f64> { let mut v = Vec::new(); let step = (2.0 * he) / (1f64).max(super::jr(2.0 * he / 6.0)); let mut x = -he; while x <= he + 0.01 { v.push(x); x += step; }
        let mut out = Vec::new(); let mut x = e; let mut d; while x < far { out.push(x); d = x - e; x += 6.0 + d * 0.055; } out.push(far);
        let mut r: Vec<f64> = out.iter().rev().map(|q| -q).collect(); r.extend(v); r.extend(out); r };
    let (hx, hz) = (ex - 6.0, ez - 6.0); let ax = axis(ex, hx); let az = axis(ez, hz); let (nxa, nza) = (ax.len(), az.len());
    let mut pos = vec![0f32; nxa * nza * 3]; let mut col = vec![0f32; nxa * nza * 3]; let mut idx = Vec::new();
    for j in 0..nza { for i in 0..nxa { let (x, z) = (ax[i], az[j]); let k = j * nxa + i; let inside = x.abs() <= ex + 0.01 && z.abs() <= ez + 0.01;
        let edge = inside && ((x.abs() - ex).abs() < 0.02 || (z.abs() - ez).abs() < 0.02);
        let h = if inside { st.height(x, z) - if edge { 0.35 } else { 1.0 } } else { st.far_h(x, z) };
        let slope = if inside { 0.0 } else { hyp(st.far_h(x + 4.0, z) - st.far_h(x - 4.0, z), st.far_h(x, z + 4.0) - st.far_h(x, z - 4.0)) / 8.0 };
        pos[k * 3] = x as f32; pos[k * 3 + 1] = h as f32; pos[k * 3 + 2] = z as f32;
        let n = fbm(x * 0.05, z * 0.05, 3); let n2 = fbm(x * 0.011 + 7.0, z * 0.011, 3);
        let c = if st.gc { st.ground_col(x, z, h, slope, n, n2) } else if st.snow { let rock = sm(0.75, 1.15, slope + (n - 0.5) * 0.4); let (mut r, mut g, mut b) = (0.8 + n * 0.06, 0.84 + n * 0.05, 0.92 + n * 0.04);
            r += (0.27 - r) * rock; g += (0.28 - g) * rock; b += (0.33 - b) * rock; [r * 0.96, g * 0.98, b] } else { let lit = (n2 - 0.5).max(0.0) * 1.6; [0.25 + n * 0.12 + lit * 0.1, 0.36 + n * 0.14, 0.17 + n * 0.06] };
        col[k * 3] = c[0] as f32; col[k * 3 + 1] = c[1] as f32; col[k * 3 + 2] = c[2] as f32; } }
    for j in 0..nza - 1 { for i in 0..nxa - 1 { if ax[i] >= -hx - 0.01 && ax[i + 1] <= hx + 0.01 && az[j] >= -hz - 0.01 && az[j + 1] <= hz + 0.01 { continue; }
        let a = (j * nxa + i) as u32; let (b, c) = (a + 1, a + nxa as u32); let e = c + 1; idx.extend_from_slice(&[a, c, b, b, c, e]); } }
    let mut g = Geo::new(pos).with_col(col).with_index(idx).to_non_indexed(); let n = g.count(); let mut uv = vec![0f32; n * 2];
    for i in 0..n { let (x, y, z) = (g.pos[i * 3] as f64, g.pos[i * 3 + 1] as f64, g.pos[i * 3 + 2] as f64); uv[i * 2] = (x / 15.0 + y / 31.0) as f32; uv[i * 2 + 1] = (z / 15.0 + y / 23.0) as f32; }
    g.uv = Some(uv); g.compute_normals();
    let fm = st.mat(Mat::lambert().vc().map(st.t_ground)); let mut keys: Vec<i64> = Vec::new(); let mut b: std::collections::HashMap<i64, Vec<usize>> = Default::default();
    let mut t = 0; while t < n { let cx = (g.pos[t * 3] as f64 + g.pos[t * 3 + 3] as f64 + g.pos[t * 3 + 6] as f64) / 3.0; let cz = (g.pos[t * 3 + 2] as f64 + g.pos[t * 3 + 5] as f64 + g.pos[t * 3 + 8] as f64) / 3.0;
        let k = (cx / 750.0).floor() as i64 * 1024 + (cz / 750.0).floor() as i64; b.entry(k).or_insert_with(|| { keys.push(k); Vec::new() }).push(t); t += 3; }
    let (nr, cl, uvv) = (g.nrm.as_ref().unwrap(), g.col.as_ref().unwrap(), g.uv.as_ref().unwrap());
    let mut sectors = Vec::new();
    for k in keys { let list = &b[&k]; let pick = |a: &Vec<f32>, s: usize| { let mut o = Vec::with_capacity(list.len() * 3 * s); for &t in list { for v in 0..3 { o.extend_from_slice(&a[(t + v) * s..(t + v) * s + s]); } } o };
        let mut bg = Geo::new(pick(&g.pos, 3)).with_nrm(pick(nr, 3)).with_col(pick(cl, 3)).with_uv(pick(uvv, 2)); bg.compute_bs(); sectors.push(bg); }
    for bg in sectors { let o = st.mesh(bg, fm); st.sc.objs[o].dyn_ = true; st.add(o); }
}

pub fn water(st: &mut St) {
    if !(st.lak || st.riv) { return; }
    let k8 = PI / 8.0; let riv = st.riv;
    let base = st.canvas_tex(32, 32, true, |_, g| { g.fill_style(if riv { "rgb(26,66,124)" } else { "rgb(40,104,178)" }); g.fill_rect(0.0, 0.0, 32.0, 32.0);
        for y in 0..32 { for x in 0..32 { let (xf, yf) = (x as f64, y as f64); let v = ((xf + (yf * k8).sin() * 3.0) * k8).sin() + ((yf + (xf * k8).sin() * 3.0) * k8).sin();
            if v > 1.0 { g.px(x, y, canvas::parse(if v > 1.6 { if riv { "rgb(150,150,186)" } else { "rgb(120,186,232)" } } else if riv { "rgb(58,98,156)" } else { "rgb(78,146,214)" })); }
            else if v < -1.2 { g.px(x, y, canvas::parse(if riv { "rgb(18,48,96)" } else { "rgb(30,82,150)" })); } } } });
    let spark = st.canvas_tex(32, 32, true, |_, g| { for i in 0..14i64 { let x = (i * 11 + (i * i * 5) % 7) % 32; let y = (i * 7 + (i * 13) % 5) % 32; let w = 2 + (i % 3) * 2;
        g.fill_style(if riv { if i % 4 != 0 { "rgba(255,214,170,0.85)" } else { "rgba(255,240,214,1)" } } else if i % 4 != 0 { "rgba(214,236,255,0.9)" } else { "rgba(255,255,255,1)" }); g.fill_rect(x as f64, y as f64, w as f64, 1.0); } });
    st.sc.texs[base].repeat = (700.0, 700.0); st.sc.texs[spark].repeat = (560.0, 560.0);
    for (tex, op, dy, ro) in [(base, 0.84, 0.0, -2), (spark, 0.55, 0.04, -1)] {
        let mut wg = plane(4200.0, 4200.0, 1, 1); wg.rotate_x(-PI / 2.0);
        let m = st.mat(Mat::basic().map(tex).transparent().opacity(op).no_dw()); let o = st.mesh(wg, m);
        st.sc.set_pos(o, 0.0, (if riv { 0.0 } else { crate::world::WL }) + dy, 0.0); st.sc.objs[o].ro = ro; st.add(o); st.water_l.push(o);
    }
    if riv {
        let (nl, nw, len) = (26usize, 9usize, 1500.0);
        for layer in 0..3usize {
            let sp = if layer == 2 { None } else { Some(st.canvas_tex(32, 32, true, |_, g| { for i in 0..48i64 { let l = layer as i64; let x = (i * 13 + l * 7 + (i * i * 3) % 11) % 32; let y = (i * 5 + l * 11 + (i * 17) % 7) % 32;
                g.fill_style(if i % 3 != 0 { "rgba(255,226,180,0.95)" } else { "rgba(255,250,236,1)" }); g.fill_rect(x as f64, y as f64, (1 + (i % 4)) as f64, 1.0); } })) };
            let (mut p, mut c, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
            for a in 0..nl { let t = a as f64 / (nl - 1) as f64; let d = 6.0 + t.powf(1.6) * len; let half = 3.0 + d * 0.11;
                for b in 0..nw { let u = b as f64 / (nw - 1) as f64 * 2.0 - 1.0; let x = u * half; p.extend_from_slice(&[x as f32, 0.0, d as f32]); uv.extend_from_slice(&[(x / 5.0) as f32, (d / 5.0) as f32]);
                    let k = (-u * u * if layer == 2 { 2.2 } else { 3.2 }).exp() * sm(0.0, 0.05, t) * (1.0 - sm(0.6, 1.0, t)) * (0.5 + 0.5 * t) * if layer == 2 { 0.32 } else { 2.4 };
                    c.extend_from_slice(&[(1.0 * k) as f32, (0.72 * k) as f32, (0.4 * k) as f32]); } }
            for a in 0..nl - 1 { for b in 0..nw - 1 { let q = (a * nw + b) as u32; let w = nw as u32; idx.extend_from_slice(&[q, q + w, q + 1, q + 1, q + w, q + w + 1]); } }
            let g = Geo::new(p).with_uv(uv).with_col(c).with_index(idx);
            let mut mm = Mat::basic().vc().transparent().opacity(if layer == 2 { 1.0 } else { 0.8 }).blend(ADDITIVE).no_dw().no_fog().double(); mm.map = sp;
            let m = st.mat(mm); let o = st.mesh(g, m); st.sc.set_rot(o, 0.0, st.sun.x.atan2(st.sun.z), 0.0); st.sc.set_pos(o, 0.0, 0.08 + layer as f64 * 0.02, 0.0);
            st.sc.objs[o].ro = 1; st.sc.objs[o].culled = false; st.sc.objs[o].dyn_ = true; st.add(o); st.glit.push(o);
        }
    }
}

pub fn sky(st: &mut St) {
    let fog = st.fog;
    let top = if st.vol { rgb(0.035, 0.012, 0.012) } else if st.pri { rgb(0.012, 0.004, 0.04) } else if st.riv { rgb(0.24, 0.26, 0.58) } else if st.tou { rgb(0.06, 0.07, 0.22) } else if st.can { rgb(0.3, 0.45, 0.76) } else if st.lak { rgb(0.38, 0.45, 0.72) } else if st.snow { rgb(0.36, 0.55, 0.84) } else { fog.add(rgb(0.12, 0.12, 0.1)) };
    let glow = if st.vol { rgb(1.0, 0.32, 0.08) } else if st.pri { rgb(0.55, 0.3, 0.85) } else if st.riv { rgb(0.5, 0.26, 0.09) } else if st.tou { rgb(0.95, 0.36, 0.2) } else if st.can { rgb(0.75, 0.42, 0.16) } else if st.lak { rgb(0.7, 0.4, 0.3) } else if st.snow { rgb(0.5, 0.32, 0.18) } else { rgb(0.32, 0.29, 0.2) };
    let halo = if st.vol { 0.6 } else if st.snow { 0.22 } else if st.can { 0.35 } else if st.lak { 0.4 } else if st.tou { 0.65 } else if st.riv { 0.3 } else { 0.0 };
    let mut m = Mat::shader(Kind::Sky).side(BACK).no_dw().blend(NO_BLEND); m.sky = Some((st.sun, fog, top, glow, halo));
    let m = st.mat(m); let o = st.mesh(sphere(1600.0, 24, 12), m); st.sc.objs[o].ro = -1; st.add(o); st.sky = o;
}

pub fn riviera_sky(st: &mut St) {
    if !st.riv { return; }
    let ct = st.canvas_tex(256, 64, true, |st, g| { let (s, h) = (256.0, 64.0);
        for _ in 0..210 { let y = h * st.rnd().powf(0.8) * 0.92 + 2.0; let x = st.rr(0.0, s); let w = st.rr(24.0, 120.0) * (0.6 + y / h); let hh = st.rr(1.0, 3.6) * (0.5 + y / h);
            let a = st.rr(0.2, 0.55) * sm(h, h * 0.62, y) * sm(0.0, h * 0.15, y);
            for ox in [-s, 0.0, s] { g.fill_grad(Style::Radial { x0: 0.0, y0: 0.0, r0: 0.0, x1: 0.0, y1: 0.0, r1: 1.0, stops: vec![(0.0, canvas::rgba(255.0, 255.0, 255.0, a)), (1.0, [255, 255, 255, 0])] });
                g.save(); g.translate(x + ox, y); g.scale(w, hh); g.begin_path(); g.circle(0.0, 0.0, 1.0); g.fill(); g.restore(); } } });
    { let t = &mut st.sc.texs[ct]; t.mag = LINEAR; t.min = LINEAR; t.mip = false; }
    let mut dg = sphere_p(1480.0, 48, 10, 0.0, PI * 2.0, 0.62, 0.9); let n = dg.count(); let (sx, sz) = (st.sun.x, st.sun.z); let sl = hyp(sx, sz); let mut dc = vec![0f32; n * 3];
    for i in 0..n { let (x, y, z) = (dg.pos[i * 3] as f64, dg.pos[i * 3 + 1] as f64, dg.pos[i * 3 + 2] as f64); let r = super::hyp3(x, y, z);
        let az = ((x * sx + z * sz) / (hyp(x, z) * sl + 1e-6)).max(0.0); let k = az.powf(1.6); let hi = y / r;
        let c = [0.74 + 0.22 * k + 0.1 * az.powi(10), 0.44 + 0.24 * k + 0.08 * az.powi(10), 0.6 - 0.28 * k]; let lift = sm(0.35, 0.8, hi) * 0.25;
        dc[i * 3] = (c[0] * (1.0 - lift) + 0.82 * lift) as f32; dc[i * 3 + 1] = (c[1] * (1.0 - lift) + 0.8 * lift) as f32; dc[i * 3 + 2] = (c[2] * (1.0 - lift) + 0.9 * lift) as f32;
        let u = dg.uv.as_mut().unwrap(); u[i * 2] = (u[i * 2] as f64 * 3.0) as f32; }
    dg.col = Some(dc);
    let m = st.mat(Mat::basic().map(ct).vc().transparent().no_dw().no_fog().side(BACK)); let o = st.mesh(dg, m); st.sc.objs[o].culled = false; st.sc.objs[o].dyn_ = true; st.add(o); st.clouds.push(o);
    let stex = |st: &mut St, inner: &str, outer: &str, a: f32| { let (i, o) = (canvas::parse(inner), canvas::parse(outer));
        st.canvas_tex(64, 64, false, |_, g| { g.fill_grad(Style::Radial { x0: 32.0, y0: 32.0, r0: 0.0, x1: 32.0, y1: 32.0, r1: 32.0, stops: vec![(0.0, i), (a, o), (1.0, canvas::parse("rgba(255,120,40,0)"))] }); g.fill_rect(0.0, 0.0, 64.0, 64.0); }) };
    let t1 = stex(st, "rgba(255,170,90,0.26)", "rgba(255,120,60,0.08)", 0.35); let t2 = stex(st, "rgba(255,250,226,1)", "rgba(255,200,120,0.95)", 0.6);
    for (size, tex) in [(420.0, t1), (92.0, t2)] {
        { let t = &mut st.sc.texs[tex]; t.mag = LINEAR; t.min = LINEAR; t.mip = false; }
        let m = st.mat(Mat::basic().map(tex).transparent().no_dw().no_fog().blend(CUSTOM).blend2()); let o = st.mesh(plane(size, size, 1, 1), m);
        st.sc.objs[o].culled = false; st.sc.objs[o].dyn_ = true; st.sc.objs[o].ro = -1; st.add(o); st.sunb.push(o);
    }
}

pub fn peaks(st: &mut St) {
    let g = st.sc.group(); st.add(g); st.peaks = g;
    if !(st.snow || st.tou || st.riv) { return; }
    let np = st.np(); let mut base = 0.0; let mut i = 0; while i < np { base += st.r.roadh[i]; i += 10; }
    let base = if st.riv { -2.0 } else { base / (np as f64 / 10.0).ceil() - 25.0 };
    let (rr_, hmin, hamp, haze, sd) = (1780.0, 260.0, 600.0, 0.6, 5.0); let n = 240; let fog = st.fog.arr();
    let (mut pos, mut col, mut idx) = (Vec::new(), Vec::new(), Vec::new());
    for i in 0..=n { let a = i as f64 / n as f64 * PI * 2.0; let (ca, sa) = (a.cos(), a.sin()); let fi = i as f64;
        let ht = if st.riv { -6.0 + sm(0.0, 0.45, (a + 0.35).cos() - 0.35) * (110.0 + 330.0 * ridged(ca * 2.2 + sd, sa * 2.2 + sd * 2.0).powf(1.4)) }
            else if st.tou { 120.0 + 260.0 * ridged(ca * 2.4 + sd, sa * 2.4 + sd * 2.0).powf(1.4) + 4.0 * vnoise(fi * 0.5, sd) }
            else { hmin + hamp * ridged(ca * 1.9 + sd, sa * 1.9 + sd * 2.0).powf(1.7) + 6.0 * vnoise(fi * 0.4, sd) };
        let glow = (ca * st.sun.x + sa * st.sun.z).max(0.0) * 0.6; let rock_n = vnoise(fi * 0.7, sd + 3.0);
        let top = if st.riv { [0.66 + glow * 0.3, 0.5 + glow * 0.1, 0.52] } else if st.tou { [0.2 + glow * 0.25, 0.18 + glow * 0.08, 0.3] } else { [0.96 + glow * 0.04, 0.93 - glow * 0.05, 0.95 - glow * 0.12] };
        let mid = if st.riv { [0.44, 0.42, 0.56] } else if st.tou { [0.16, 0.15, 0.26] } else if rock_n > 0.55 { [0.52, 0.56, 0.66] } else { [0.86, 0.89, 0.95] };
        let mix = |c: [f64; 3], k: f64| [c[0] + (fog[0] - c[0]) * k, c[1] + (fog[1] - c[1]) * k, c[2] + (fog[2] - c[2]) * k];
        for (y, c) in [(-400.0, mix(fog, 0.0)), (ht * 0.45, mix(mid, haze + 0.15)), (ht * 0.8, mix(top, haze * 0.6)), (ht, mix(top, haze * 0.45))] {
            pos.extend_from_slice(&[(ca * rr_) as f32, y as f32, (sa * rr_) as f32]); col.extend(c.iter().map(|&v| v as f32)); } }
    for i in 0..n as u32 { for r in 0..3 { let (a0, b0) = (i * 4 + r, (i + 1) * 4 + r); idx.extend_from_slice(&[a0, b0, a0 + 1, a0 + 1, b0, b0 + 1]); } }
    let m = st.mat(Mat::basic().vc().no_fog().double()); let o = st.mesh(Geo::new(pos).with_col(col).with_index(idx), m);
    st.sc.set_pos(o, 0.0, base, 0.0); st.sc.objs[o].culled = false; st.sc.add_to(g, o);
}
#[allow(dead_code)]
fn _unused(_: &Ctx, _: M) {}
