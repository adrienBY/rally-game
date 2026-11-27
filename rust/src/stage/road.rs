//! The stage's road: the closed loop resampled every metre, its tangents and banking, the speed plans, the road height (terrain,
//! smoothing, gradient caps, crests, kickers, the launch ramp), the riviera tunnels and village, the ember bridges - exactly as the page
//! computed them before it built anything (fdlibm maths, so every phone and the browser get the same numbers).
use crate::noise::{clamp, fbm, hash2, js_round, ridged, smooth};
use crate::physics::js_hypot;
use crate::world::{self, Map, World};
use libm::{atan2, cos, exp, fabs, pow, sin, sqrt};

pub const PI: f64 = std::f64::consts::PI;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum M { Forest, Snow, Canyon, Lakes, Touge, Riviera, Prism, Ember }
impl M {
    pub fn from(name: &str) -> M { match name { "snow" => M::Snow, "canyon" => M::Canyon, "lakes" => M::Lakes, "touge" => M::Touge, "riviera" => M::Riviera, "prism" => M::Prism, "ember" => M::Ember, _ => M::Forest } }
    pub fn name(self) -> &'static str { match self { M::Forest => "forest", M::Snow => "snow", M::Canyon => "canyon", M::Lakes => "lakes", M::Touge => "touge", M::Riviera => "riviera", M::Prism => "prism", M::Ember => "ember" } }
    pub fn id(self) -> i32 { match self { M::Forest => 0, M::Snow => 1, M::Canyon => 2, M::Lakes => 3, M::Touge => 5, M::Riviera => 6, M::Prism => 7, M::Ember => 8 } }
    pub fn world(self) -> Map { Map::from_id(self.id()) }
}

pub struct Tunnel { pub m: usize, pub i0: usize, pub i1: usize, pub half: usize }
pub struct Jump { pub i: usize, pub big: bool }

pub struct Road {
    pub map: M, pub road: f64, pub np: usize,
    pub path: Vec<[f64; 2]>, pub tan: Vec<[f64; 2]>, pub bank: Vec<f64>,
    pub vcurve: Vec<f64>, pub vprof: Vec<f64>, pub jcap: Vec<f64>, pub roadh: Vec<f64>,
    pub waves: Vec<[f64; 5]>, pub tun: Vec<Tunnel>, pub vil: Option<(usize, usize)>, pub rivsec: Vec<(&'static str, usize, usize)>,
    pub bridge_on: Vec<bool>, pub bridges: Vec<(usize, usize)>,
    pub jumps: Vec<Jump>, pub ramp_i: i64, pub ramp_l: usize, pub ramp_h: f64,
    pub gxh: i32, pub gzh: i32, pub world: World,
}

/// f32 round trip (the page kept these in Float32Arrays)
#[inline] pub fn f32r(v: f64) -> f64 { v as f32 as f64 }

fn riv_path(out: &mut Vec<[f64; 2]>, sec: &mut Vec<(&'static str, usize, usize)>) {
    let d = PI / 180.0; let k = 0.8; let (a, b) = (62.0, 70.0);
    let seq = |le: f64, ls: f64| -> Vec<(f64, f64, f64, &'static str)> { vec![(le * 0.45, 0.0, 0.0, "start"), (0.0, 300.0 * k, -14.0, ""), (0.0, 300.0 * k, 14.0, ""), (le * 0.55, 0.0, 0.0, ""),
        (0.0, 180.0 * k, 46.0, ""), (80.0, 0.0, 0.0, "tunnelA"), (0.0, 150.0 * k, 44.0, ""), (30.0, 0.0, 0.0, ""), (0.0, 200.0 * k, -16.0, ""), (0.0, 190.0 * k, 16.0, ""),
        (0.0, 100.0, 45.0, "stepS"), (14.0, 0.0, 0.0, ""), (0.0, 100.0, -45.0, ""), (160.0, 0.0, 0.0, "top"), (0.0, 13.0, -180.0, "hp1"), (112.0, 0.0, 0.0, "leg2"), (0.0, 13.0, 180.0, "hp2"),
        (95.0, 0.0, 0.0, "leg3"), (150.0, 0.0, 0.0, "village"), (0.0, 140.0 * k, 70.0, ""), (40.0, 0.0, 0.0, "west"), (0.0, 190.0 * k, a, ""), (90.0, 0.0, 0.0, "tunnelB"),
        (0.0, 190.0 * k, -22.0, ""), (0.0, 170.0 * k, b, ""), (ls * 0.3, 0.0, 0.0, "south"), (0.0, 160.0, 24.0, ""), (0.0, 160.0, -24.0, ""), (ls * 0.7, 0.0, 0.0, ""),
        (0.0, 30.0, 360.0 - (46.0 + 44.0 + 70.0 + a - 22.0 + b), "sharp"), (30.0, 0.0, 0.0, "")] };
    let kap = |sq: &[(f64, f64, f64, &'static str)]| -> (Vec<f64>, Vec<(&'static str, usize, usize)>, usize) {
        let mut blocks = Vec::new(); let mut tags = Vec::new(); let mut pos = 0usize;
        for &(l, r, deg, tag) in sq { let n = if r == 0.0 { js_round(l) as usize } else { js_round(fabs(deg) * d * r) as usize };
            if r != 0.0 { blocks.push((pos, n, crate::noise::sgn(deg) / r, r)); } tags.push((tag, pos, n)); pos += n; }
        let nn = pos; let mut k2 = vec![0.0f64; nn];
        for &(p, n, kk, r) in &blocks { let w = (16f64).min((4f64).max(r * 0.1)).trunc() as i64; let kk = kk / (2 * w + 1) as f64;
            for j in -w..=w { for i in p..p + n { k2[(((i as i64 + j) % nn as i64 + nn as i64) % nn as i64) as usize] += kk; } } }
        let mut sum = 0.0; for v in &k2 { sum += v; } let sc = 2.0 * PI / sum; for v in k2.iter_mut() { *v *= sc; }
        (k2, tags, nn) };
    let integ = |k2: &[f64], nn: usize| -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        let (mut x, mut z, mut phi) = (vec![0.0; nn], vec![0.0; nn], vec![0.0; nn]); let (mut c, mut sx, mut sz) = (0.0, 0.0, 0.0);
        for i in 0..nn { c += k2[i]; phi[i] = -(c - k2[0]) - PI / 2.0; sx += cos(phi[i]); sz += sin(phi[i]); x[i] = sx; z[i] = sz; }
        (x, z, phi) };
    let (mut le, mut ls) = (200.0, 300.0);
    let err = |x: &[f64], z: &[f64], phi: &[f64], nn: usize| (x[nn - 1] + cos(phi[0]) - x[0], z[nn - 1] + sin(phi[0]) - z[0]);
    for _ in 0..8 { let (k2, _, nn) = kap(&seq(le, ls)); let (x, z, phi) = integ(&k2, nn); let (ex, ez) = err(&x, &z, &phi, nn); le += ez; ls -= ex; }
    let (k2, tags, nn) = kap(&seq(le, ls)); let (mut x, mut z, phi) = integ(&k2, nn); let (ex, ez) = err(&x, &z, &phi, nn);
    let (mut x0, mut x1, mut z0, mut z1) = (1e9f64, -1e9f64, 1e9f64, -1e9f64);
    for i in 0..nn { x[i] -= ex * i as f64 / nn as f64; z[i] -= ez * i as f64 / nn as f64; x0 = x0.min(x[i]); x1 = x1.max(x[i]); z0 = z0.min(z[i]); z1 = z1.max(z[i]); }
    let (cx, cz) = ((x1 + x0) / 2.0, (z1 + z0) / 2.0); for i in 0..nn { out.push([x[i] - cx, z[i] - cz]); }
    for (tag, p, n) in tags { if !tag.is_empty() { sec.retain(|s| s.0 != tag); sec.push((tag, p, n)); } }
}

pub fn build(map: M) -> Road {
    let (snow, can, lak, tou, riv, pri, vol) = (map == M::Snow, map == M::Canyon, map == M::Lakes, map == M::Touge, map == M::Riviera, map == M::Prism, map == M::Ember);
    let warm = can || lak;
    let road = if pri { 4.5 } else { 3.4 };
    let mut path: Vec<[f64; 2]> = Vec::new(); let mut rivsec = Vec::new();
    if riv { riv_path(&mut path, &mut rivsec); } else {
        let nr = 24000; let mut raw: Vec<[f64; 2]> = Vec::with_capacity(nr);
        for kk in 0..nr { let t = kk as f64 / nr as f64 * PI * 2.0;
            let (rx, rz) = if snow { let r = 1.3 * (140.0 + 34.0 * sin(3.0 * t + 0.7) + 22.0 * sin(2.0 * t + 2.1) + 13.0 * sin(5.0 * t + 1.0) + 6.0 * sin(9.0 * t + 0.3)); (1.15 * r * cos(t), r * sin(t)) }
                else if can { let r = 1.15 * (165.0 + 38.0 * sin(2.0 * t + 0.4) + 26.0 * sin(3.0 * t + 2.2) + 9.0 * sin(6.0 * t + 1.1) + 4.0 * sin(10.0 * t)); (1.25 * r * cos(t), 0.9 * r * sin(t)) }
                else if tou { let r = 1.05 * (150.0 + 46.0 * sin(5.0 * t + 0.3) + 16.0 * sin(2.0 * t + 1.7) + 10.0 * sin(8.0 * t + 2.2) + 4.0 * sin(13.0 * t)); (1.1 * r * cos(t), r * sin(t)) }
                else if pri { let r = 1.36 * (160.0 + 52.0 * sin(2.0 * t + 0.6) + 30.0 * sin(3.0 * t + 2.0) + 18.0 * sin(5.0 * t + 1.2) + 10.0 * sin(8.0 * t + 0.4) + 4.5 * sin(13.0 * t + 1.1)); (1.18 * r * cos(t), r * sin(t)) }
                else if vol { let r = 245.0 + 75.0 * cos(t + 0.3) + 10.0 * sin(6.0 * t + 1.0) + 6.0 * sin(11.0 * t + 2.0) + 3.0 * sin(15.0 * t); (r * cos(t), r * sin(t)) }
                else if lak { let r = 1.2 * (158.0 + 30.0 * sin(3.0 * t + 1.4) + 22.0 * sin(2.0 * t + 0.2) + 12.0 * sin(5.0 * t + 2.6) + 5.0 * sin(9.0 * t + 0.9)); (1.1 * r * cos(t), r * sin(t)) }
                else { let r = 1.25 * (150.0 + 42.0 * sin(2.0 * t + 1.0) + 24.0 * sin(3.0 * t + 0.3) + 11.0 * sin(7.0 * t + 2.0) + 5.0 * sin(11.0 * t + 0.7)); (1.2 * r * cos(t), 0.92 * r * sin(t)) };
            raw.push([rx, rz]); }
        let n = raw.len(); let mut cum = vec![0.0f64; n + 1];
        for i in 1..=n { let a = raw[i - 1]; let b = raw[i % n]; cum[i] = cum[i - 1] + js_hypot(b[0] - a[0], b[1] - a[1]); }
        let l = cum[n]; let mut j = 0usize;
        for s in 0..(l.floor() as usize) { let s = s as f64; while cum[j + 1] < s { j += 1; }
            let a = raw[j]; let b = raw[(j + 1) % n]; let f = (s - cum[j]) / (cum[j + 1] - cum[j]); path.push([a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f]); }
    }
    let np = path.len();
    let waves: Vec<[f64; 5]> = if pri { vec![[870.0, 1130.0, 1.6, 90.0, 4.0], [1190.0, 1430.0, 2.2, 120.0, 5.0], [1625.0, 1880.0, 1.4, 75.0, 3.0], [0.0, np as f64, 0.6, np as f64 / 5.0, 12.0]] } else { Vec::new() };
    let tan: Vec<[f64; 2]> = (0..np).map(|i| { let a = path[(i + np - 2) % np]; let b = path[(i + 2) % np]; let (dx, dz) = (b[0] - a[0], b[1] - a[1]); let l = js_hypot(dx, dz); [dx / l, dz / l] }).collect();
    let bank: Vec<f64> = if pri { world::prism_bank(&path.iter().map(|p| (p[0], p[1])).collect::<Vec<_>>()) } else { vec![0.0; np] };
    let cdi = |a: usize, b: usize| { let d = ((a as i64 - b as i64) % np as i64 + np as i64) % np as i64; d.min(np as i64 - d) as usize };
    let sec = |k: &str| rivsec.iter().find(|s| s.0 == k).map(|s| (s.1, s.2)).unwrap_or((0, 0));
    let tun: Vec<Tunnel> = if riv { ["tunnelA", "tunnelB"].iter().map(|k| { let (p, n) = sec(k); let m = p + n / 2; Tunnel { m, i0: (m + np - 27) % np, i1: (m + 27) % np, half: 27 } }).collect() } else { Vec::new() };
    let vil = if riv { let (p, n) = sec("village"); Some((p - 25, n + 30)) } else { None };
    // curvature -> safe cornering speed -> braking-aware speed profile
    let mut vcurve = vec![0.0f64; np]; let mut jcap = vec![99.0f64; np];
    for i in 0..np { let a = tan[(i + np - 6) % np]; let b = tan[(i + 6) % np];
        let d = fabs(atan2(a[0] * b[1] - a[1] * b[0], a[0] * b[0] + a[1] * b[1])) / 12.0;
        vcurve[i] = f32r((44f64).min(sqrt((if snow { 0.5 } else if can { 0.62 } else if tou { 0.74 } else if riv || pri { 0.8 } else { 0.66 }) * 9.81 / d.max(1e-4)))); }
    let mut vprof = vcurve.clone();
    let brake_pass = |vp: &mut Vec<f64>| { for _ in 0..3 { for i in (0..np).rev() { vp[i] = f32r(vp[i].min(sqrt(pow(vp[(i + 1) % np], 2.0) + 2.0 * 5.0))); } } };
    brake_pass(&mut vprof);
    // the world grids need the final road height; the road height needs the terrain only (computed here like the page)
    let mut mx = 0.0f64; let mut mz = 0.0f64; for p in &path { mx = mx.max(fabs(p[0])); mz = mz.max(fabs(p[1])); }
    let gxh = (mx + 60.0).ceil() as i32; let gzh = (mz + 60.0).ceil() as i32;
    let terrain = |x: f64, z: f64| terrain(map, x, z);
    let mut roadh = vec![0.0f64; np];
    for i in 0..np { let u = i as f64 / np as f64 * PI * 2.0;
        roadh[i] = f32r(if pri { 40.0 + 24.0 * sin(2.0 * u + 0.4) + 13.0 * sin(5.0 * u + 1.3) + 6.0 * sin(9.0 * u + 2.1) + 3.0 * sin(14.0 * u + 0.7) } else if riv { world::riv_land(path[i][0], path[i][1], false) } else { terrain(path[i][0], path[i][1]) }); }
    let boxp = |rh: &mut Vec<f64>, w: i64, passes: usize| { for _ in 0..passes { let c = rh.clone(); for i in 0..np { let mut s = 0.0; for k in -w..=w { s += c[((i as i64 + k + np as i64) % np as i64) as usize]; } rh[i] = f32r(s / (2 * w + 1) as f64); } } };
    boxp(&mut roadh, 14, if snow { 7 } else if lak || tou || riv { 5 } else { 4 });
    if lak { for v in roadh.iter_mut() { *v = f32r(v.max(world::WL + 1.6)); } boxp(&mut roadh, 6, 2); }
    // ember peak: lava canyons, crossed on bridges
    let mut bridge_on = vec![false; np]; let mut bridges = Vec::new();
    if vol { let raw: Vec<bool> = (0..np).map(|k| world::ember_canyon(path[k][0], path[k][1]).0 > 0.3).collect();
        for k in 0..np { if raw[k] { for d in -3i64..=3 { bridge_on[((k as i64 + d + np as i64) % np as i64) as usize] = true; } } }
        for k in 0..np { if bridge_on[k] && !bridge_on[(k + np - 1) % np] { let mut e = k; while bridge_on[(e + 1) % np] && e - k < np { e += 1; } bridges.push((k, e % np)); } } }
    if snow || tou || riv || pri || vol {
        let gmax = if tou { 0.09 } else if riv { 0.085 } else if pri { 0.15 } else if vol { 0.13 } else { 0.11 };
        for _ in 0..8 {
            for i in 1..=np * 2 { let a = roadh[(i - 1) % np]; let k = i % np; roadh[k] = f32r(clamp(roadh[k], a - gmax, a + gmax)); }
            for i in (1..=np * 2).rev() { let a = roadh[i % np]; let k = (i - 1) % np; roadh[k] = f32r(clamp(roadh[k], a - gmax, a + gmax)); }
        }
        boxp(&mut roadh, 8, 2);
    }
    // jumps: crests on the straights, kickers, rolling undulation, one launch ramp
    struct Jp { n: usize, gap: usize, big_a: f64, small_a: f64, su_b: f64, sd_b: f64, su_s: f64, sd_s: f64, v_b: f64, v_s: f64, kick: f64, kick_a: f64, und: f64 }
    let jp = if pri { Jp { n: 0, gap: 300, big_a: 0.0, small_a: 0.0, su_b: 14.0, sd_b: 8.0, su_s: 8.0, sd_s: 4.0, v_b: 36.0, v_s: 36.0, kick: 0.0, kick_a: 0.0, und: 0.0 } }
        else if vol { Jp { n: 1, gap: 400, big_a: 0.8, small_a: 0.5, su_b: 12.0, sd_b: 6.0, su_s: 8.0, sd_s: 4.0, v_b: 28.0, v_s: 30.0, kick: 0.0, kick_a: 0.0, und: 0.04 } }
        else if riv { Jp { n: 1, gap: 300, big_a: 0.5, small_a: 0.4, su_b: 14.0, sd_b: 8.0, su_s: 8.0, sd_s: 4.0, v_b: 36.0, v_s: 36.0, kick: 0.0, kick_a: 0.0, und: 0.03 } }
        else if tou { Jp { n: 2, gap: 300, big_a: 0.9, small_a: 0.6, su_b: 12.0, sd_b: 6.0, su_s: 8.0, sd_s: 4.0, v_b: 30.0, v_s: 32.0, kick: 0.0, kick_a: 0.0, und: 0.05 } }
        else if warm { Jp { n: 9, gap: 115, big_a: if lak { 2.3 } else { 1.9 }, small_a: 1.0, su_b: 13.0, sd_b: 5.5, su_s: 7.0, sd_s: 3.6, v_b: 23.0, v_s: 26.0, kick: 0.16, kick_a: 0.55, und: 0.34 } }
        else { Jp { n: 6, gap: 150, big_a: 1.5, small_a: 0.75, su_b: 10.0, sd_b: 4.6, su_s: 6.0, sd_s: 3.2, v_b: 24.0, v_s: 27.0, kick: 0.09, kick_a: 0.42, und: 0.24 } };
    let straight = |i: usize| { for k in -28i64..=36 { if vcurve[((i as i64 + k + np as i64) % np as i64) as usize] < 24.0 { return false; } } true };
    let mut jumps: Vec<Jump> = Vec::new();
    { let mut i = 90; while i + 90 < np { if straight(i) && !jumps.iter().any(|j| (j.i as i64 - i as i64).abs() < jp.gap as i64) { let big = jumps.len() % 2 == 0; jumps.push(Jump { i, big }); if jumps.len() >= jp.n { break; } } i += 3; } }
    let wrap = |i: i64| ((i % np as i64 + np as i64) % np as i64) as usize;
    for j in &jumps { let a = if j.big { jp.big_a } else { jp.small_a }; let su = if j.big { jp.su_b } else { jp.su_s }; let sd = if j.big { jp.sd_b } else { jp.sd_s };
        for s in -36i64..=24 { let k = wrap(j.i as i64 + s); let sg = if s < 0 { su } else { sd }; roadh[k] = f32r(roadh[k] + a * exp(-((s * s) as f64) / (2.0 * sg * sg))); }
        let v = if j.big { jp.v_b } else { jp.v_s };
        for s in -40i64..=30 { let k = wrap(j.i as i64 + s); vprof[k] = f32r(vprof[k].min(v)); jcap[k] = f32r(jcap[k].min(v * 1.05)); } }
    if can { let mut runs = 0; let mut i = 120;
        while i + 120 < np && runs < 4 { if vcurve[i] < 26.0 || jumps.iter().any(|j| (j.i as i64 - i as i64).abs() < 90) || hash2(i as f64, 31.0) > 0.2 { i += 5; continue; }
            for s in 0..=66usize { let k = (i + s) % np; roadh[k] = f32r(roadh[k] + 0.5 * pow(sin(s as f64 * PI / 13.2), 2.0) * sin(PI * s as f64 / 66.0)); vprof[k] = f32r(vprof[k].min(25.0)); jcap[k] = f32r(jcap[k].min(28.0)); }
            runs += 1; i += 200; i += 5; } }
    brake_pass(&mut vprof);
    { let mut i = 60; while i + 40 < np { if vcurve[i] < 20.0 || jumps.iter().any(|j| (j.i as i64 - i as i64).abs() < 60) || hash2(i as f64, 7.0) > jp.kick { i += 3; continue; }
        for s in -10i64..=8 { let k = wrap(i as i64 + s); let sg = if s < 0 { 3.2 } else { 2.2 }; roadh[k] = f32r(roadh[k] + jp.kick_a * exp(-((s * s) as f64) / (2.0 * sg * sg))); } i += 60; i += 3; } }
    for i in 0..np { roadh[i] = f32r(roadh[i] + jp.und * sin(i as f64 * PI * 2.0 / 26.0) * (0.5 + 0.5 * sin(i as f64 * PI * 2.0 / 190.0 + 1.0))); }
    let cdist = |a: usize, b: usize| { let d = (a as i64 - b as i64).abs() % np as i64; d.min(np as i64 - d) as usize };
    let (ramp_l, ramp_h) = (8usize, 1.4);
    let mut ramp_i: i64 = -1;
    { let mut i = js_round(np as f64 * 0.3) as usize; while (i as f64) < np as f64 * 0.92 && !riv && !pri { if straight(i) && straight((i + 40) % np) && !jumps.iter().any(|j| cdist(j.i, i) < 80) { ramp_i = i as i64; break; } i += 3; } }
    if ramp_i < 0 && !tou && !riv && !pri && !vol { ramp_i = js_round(np as f64 * 0.6) as i64; }
    if ramp_i >= 0 { for s in 0..=ramp_l { let k = wrap(ramp_i - ramp_l as i64 + s as i64); roadh[k] = f32r(roadh[k] + ramp_h * s as f64 / ramp_l as f64); }
        for s in -50i64..=45 { let k = wrap(ramp_i + s); vprof[k] = f32r(vprof[k].min(25.0)); jcap[k] = f32r(jcap[k].min(27.0)); } }
    brake_pass(&mut vprof);
    if pri {
        for w in &waves { let (s0, s1) = (w[0] as i64, w[1] as i64); if s1 - s0 < np as i64 { for i in (s0 - 30)..s1 { let k = wrap(i); jcap[k] = f32r(jcap[k].min(31.0)); } } }
        for i in 0..np { let k = -(roadh[(i + 8) % np] - 2.0 * roadh[i] + roadh[(i + np - 8) % np]) / 64.0; if k > 1e-4 { jcap[i] = f32r(jcap[i].min(sqrt(0.5 * 9.81 / k))); } vprof[i] = f32r(vprof[i].min(jcap[i])); }
        brake_pass(&mut vprof);
    }
    let _ = cdi;
    let w = world::build(map.world(), &path.iter().map(|p| (p[0], p[1])).collect::<Vec<_>>(), &roadh, gxh, gzh);
    Road { map, road, np, path, tan, bank, vcurve, vprof, jcap, roadh, waves, tun, vil, rivsec, bridge_on, bridges, jumps, ramp_i, ramp_l, ramp_h, gxh, gzh, world: w }
}

/// the raw terrain (before the road is cut in), as the page's terrain()
pub fn terrain(map: M, x: f64, z: f64) -> f64 {
    match map {
        M::Prism => world::PRISM_VOID,
        M::Riviera => world::riv_land(x, z, true),
        M::Touge => { let k = (x * x + z * z) / 90000.0; 66.0 / (1.0 + k) - (2.5f64).min((0f64).max(k - 0.62)) * 46.0 + ridged(x * 0.006 + 4.0, z * 0.006 - 9.0) * 16.0 + (fbm(x * 0.02, z * 0.02, 4) - 0.5) * 8.0 + (fbm(x * 0.08, z * 0.08, 4) - 0.5) * 0.8 }
        M::Ember => vol_mount(x, z),
        M::Canyon => (fbm(x * 0.008 + 3.0, z * 0.008 - 2.0, 4) - 0.5) * 18.0 + (fbm(x * 0.05, z * 0.05, 4) - 0.5) * 1.2,
        M::Lakes => (fbm(x * 0.006 + 5.0, z * 0.006 + 1.0, 4) - 0.5) * 44.0 + (fbm(x * 0.02, z * 0.02, 4) - 0.5) * 8.0 + (fbm(x * 0.07, z * 0.07, 4) - 0.5) * 1.2,
        M::Snow => ridged(x * 0.0052 + 3.0, z * 0.0052 - 7.0) * 58.0 + (fbm(x * 0.016, z * 0.016, 4) - 0.5) * 12.0 + (fbm(x * 0.07, z * 0.07, 4) - 0.5) * 1.4,
        M::Forest => (fbm(x * 0.014 + 10.0, z * 0.014 - 4.0, 4) - 0.5) * 16.0 + (fbm(x * 0.06, z * 0.06, 4) - 0.5) * 1.6,
    }
}
pub fn vol_mount(x: f64, z: f64) -> f64 {
    let r = sqrt(x * x + z * z + 1.0); let k = (x * x + z * z) / 108900.0;
    250.0 / (1.0 + k) - 95.0 * (1.0 - smooth(35.0, 100.0, r)) + 12.0 * smooth(60.0, 95.0, r) * (1.0 - smooth(95.0, 140.0, r))
        + ridged(x * 0.009 + 2.0, z * 0.009 - 5.0) * 16.0 * smooth(90.0, 220.0, r) + (fbm(x * 0.03 + 7.0, z * 0.03, 3) - 0.5) * 6.0 + (fbm(x * 0.11, z * 0.11, 2) - 0.5) * 0.8
}
pub fn canyon_mesa(x: f64, z: f64) -> f64 {
    let h = smooth(0.545, 0.57, fbm(x * 0.0065 + 21.0, z * 0.0065 + 7.0, 4)) * (24.0 + fbm(x * 0.03 + 5.0, z * 0.03, 4) * 14.0); let t = h / 6.0; let f = t - t.floor();
    (t.floor() + smooth(0.7, 1.0, f)) * 6.0
}

impl Road {
    pub fn height(&self, x: f64, z: f64) -> f64 { self.world.base_height(x, z) }
    pub fn path_dist(&self, x: f64, z: f64) -> f64 { self.world.path_dist(x, z) }
    pub fn path_idx(&self, x: f64, z: f64) -> usize {
        let w = &self.world; let j = clamp(js_round(z) + w.gzh as f64, 0.0, w.gnz as f64 - 1.0) as usize; let i = clamp(js_round(x) + w.gxh as f64, 0.0, w.gnx as f64 - 1.0) as usize;
        w.pi[j * w.gnx + i] as usize
    }
    pub fn road_dist(&self, x: f64, z: f64) -> f64 { let p = self.path[self.path_idx(x, z)]; js_hypot(x - p[0], z - p[1]) }
    pub fn cdi(&self, a: usize, b: usize) -> usize { let np = self.np as i64; let d = ((a as i64 - b as i64) % np + np) % np; d.min(np - d) as usize }
    pub fn cdist(&self, a: usize, b: usize) -> usize { let np = self.np as i64; let d = (a as i64 - b as i64).abs() % np; d.min(np - d) as usize }
    pub fn tun_at(&self, i: usize) -> Option<&Tunnel> { self.tun.iter().find(|t| self.cdi(i, t.m) <= t.half) }
    pub fn in_village(&self, i: usize) -> bool { match self.vil { Some((p, n)) => ((i + self.np - p % self.np) % self.np) < n, None => false } }
    pub fn tun_lamp(&self, t: &Tunnel, i: usize) -> f64 {
        let mut g: f64 = 0.0; let mut l = -(t.half as i64) + 4; while l <= t.half as i64 - 3 { let d = self.cdi(i, ((t.m as i64 + l + self.np as i64) % self.np as i64) as usize) as f64; g = g.max((-d * d / 7.0).exp()); l += 8; } g
    }
    pub fn tun_shade(&self, i: usize, pd: f64) -> Option<[f64; 3]> {
        let t = self.tun_at(i)?; if pd > self.road + 6.0 { return None; }
        let k = smooth(-1.5, 3.0, t.half as f64 - self.cdi(i, t.m) as f64); let b = 0.15 + 0.5 * self.tun_lamp(t, i);
        Some([1.0 - k * (1.0 - b * 1.12), 1.0 - k * (1.0 - b * 0.95), 1.0 - k * (1.0 - b * 0.72)])
    }
    pub fn bridge_near(&self, i: usize, m: usize) -> bool {
        self.bridges.iter().any(|&(a, b)| { let l = (b + self.np - a) % self.np; let d = (i + self.np - a) % self.np; d <= l + m || self.np - d <= m })
    }
    pub fn wrap(&self, i: i64) -> usize { let np = self.np as i64; ((i % np + np) % np) as usize }
}
