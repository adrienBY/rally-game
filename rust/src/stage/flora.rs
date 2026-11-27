//! Trees in a band along the stage (placement, shadows, instanced trunks and cards), the far forest out to the horizon, and the ground
//! cover: ferns, flowers, garrigue and scrub, snowbanks, rocks and logs.
use super::three::*;
use super::{hyp, push_t, sm, Cv, St, Tree};
use crate::noise::{clamp, fbm, hash2, vnoise};
use std::collections::HashMap;
use std::f64::consts::PI;

pub fn place_trees(st: &mut St) {
    let np = st.np(); let road = st.road(); let wl = crate::world::WL;
    let mut tries = 0; let limit = if st.pri || st.vol { 0 } else { 32000 };
    while tries < limit && st.trees.len() < 1900 { tries += 1;
        let k = (st.rnd() * np as f64) as usize; let p = st.p(k); let tn = st.t(k); let side = if st.rnd() < 0.5 { -1.0 } else { 1.0 };
        let off = (road + 2.3 + st.rnd().powf(if st.snow { 1.1 } else { 1.35 }) * if st.snow { 36.0 } else { 46.0 }) * side;
        let x = p[0] - tn[1] * off + st.rr(-1.0, 1.0); let z = p[1] + tn[0] * off + st.rr(-1.0, 1.0);
        if st.path_dist(x, z) < road + 2.2 { continue; }
        if st.lak && st.height(x, z) < wl + 0.5 { continue; }
        if st.riv && (st.height(x, z) < 2.5 || st.r.tun_at(k).is_some() || (st.r.in_village(k) && st.path_dist(x, z) < 15.0) || (st.height(x, z) < st.r.roadh[k] - 3.0 && st.rnd() < 0.7)) { continue; }
        if st.tou && st.height(x, z) < st.r.roadh[k] - 2.5 && st.rnd() < 0.9 { continue; }
        let skip = if st.riv { st.rnd() > 0.3 + fbm(x * 0.03 + 50.0, z * 0.03, 4) * 0.5 } else if st.tou { st.rnd() > 0.45 + fbm(x * 0.03 + 50.0, z * 0.03, 4) * 0.6 }
            else if st.can { st.rnd() > 0.016 } else if st.snow { st.rnd() > sm(0.42, 0.72, fbm(x * 0.022 + 50.0, z * 0.022, 4)) * 0.9 } else { st.rnd() > 0.3 + fbm(x * 0.03 + 50.0, z * 0.03, 4) * 0.95 };
        if skip { continue; }
        let (ci, cj) = ((x / 4.0).floor() as i64, (z / 4.0).floor() as i64); let mut ok = true;
        'o: for a in -1..=1 { for b in -1..=1 { if let Some(l) = st.th.get(&((ci + a) * 4096 + cj + b)) { for &t in l { let t = st.trees[t]; if (t.x - x).powi(2) + (t.z - z).powi(2) < 16.0 { ok = false; break 'o; } } } } }
        if !ok { continue; }
        let y = st.height(x, z); st.trees.push(Tree { x, z, y, cv: None }); let n = st.trees.len() - 1; st.th.entry(ci * 4096 + cj).or_default().push(n);
    }
    let w = &st.r.world; let (gxh, gzh, gnx, gnz) = (w.gxh as i64, w.gzh as i64, w.gnx as i64, w.gnz as i64);
    for t in &st.trees { for i in -7i64..=7 { for j in -7i64..=7 { let gx = super::jr(t.x) as i64 + i + gxh; let gz = super::jr(t.z) as i64 + j + gzh; if gx < 0 || gz < 0 || gx >= gnx || gz >= gnz { continue; }
        let d = hyp((gx - gxh) as f64 - t.x, (gz - gzh) as f64 - t.z); if d < 6.5 { let q = (gz * gnx + gx) as usize; st.sh[q] = (st.sh[q] as f64 + (1.0 - d / 6.5) * 0.6) as f32; } } } }
}

pub fn trees(st: &mut St) {
    let mut uc = cylinder(0.62, 1.0, 1.0, 6, 1, true); uc.translate(0.0, 0.5, 0.0); st.unit_cyl = st.geo(uc); st.card = st.geo(plane(1.0, 1.0, 1, 1));
    let (mut trunk_m, mut trunk_c, mut leaf_m, mut leaf_c, mut needle_m, mut needle_c, mut snow_m, mut snow_c) = (vec![], vec![], vec![], vec![], vec![], vec![], vec![], vec![]);
    let mut o = Tmp::default(); let sun = st.sun;
    for ti in 0..st.trees.len() { let t = st.trees[ti];
        if st.riv {
            let kind = st.rnd(); let umb = kind < 0.56; let cyp = !umb && kind < 0.76; let mut bark = rgb(0.95, 0.78, 0.66).mul(st.rr(0.8, 1.0));
            if umb {
                let h = st.rr(9.0, 14.0); let r = st.rr(0.22, 0.32); let lx = st.rr(-0.13, 0.13); let lz = st.rr(-0.13, 0.13);
                st.obstacles.push([t.x, t.z, r + 0.06, 2.0]);
                o.pos(t.x, t.y - 0.3, t.z).rot(lx, 0.0, lz).scale(r, h * 0.86, r); push_t(&mut trunk_m, &mut trunk_c, &o, bark);
                let cx = t.x + lz.sin() * -h * 0.86 + lx.sin() * 0.0; let cz = t.z + lx.sin() * h * 0.86; let cy = t.y + h * 0.86;
                for b in 0..4 { let a = b as f64 * 1.57 + st.rnd(); o.pos(cx, cy - 0.8, cz).rot(0.0, a, 0.0); let rz = st.rr(0.9, 1.25); o.rotate_z(rz);
                    let sy = st.rr(2.4, 3.4); o.scale(r * 0.45, sy, r * 0.45); push_t(&mut trunk_m, &mut trunk_c, &o, bark); }
                let rad_r = st.rr(4.6, 6.4); let ry = st.rr(1.0, 1.5); let base = { let (a, b, c) = (st.rr(0.25, 0.3), st.rr(0.32, 0.42), st.rr(0.17, 0.22)); Col::hsl(a, b, c) };
                st.trees[ti].cv = Some(Cv { x: cx, y: cy + 0.6, z: cz, r: rad_r * 0.95, l: None, d: 0.85, tr: r });
                for _ in 0..30 { let u = st.rnd() * PI * 2.0; let rad = st.rnd().sqrt(); let (dx, dz) = (u.cos() * rad, u.sin() * rad); let dy = (1.0 - rad * rad) * 0.7 + st.rr(-0.35, 0.35);
                    o.pos(cx + dx * rad_r, cy + 0.6 + dy * ry, cz + dz * rad_r); let (a, b, c) = (-PI / 2.0 + st.rr(-0.6, 0.6), st.rnd() * 6.0, st.rr(-0.6, 0.6)); o.rot(a, b, c);
                    let sc = st.rr(2.8, 3.8); o.scalar(sc); let lit = (dx * sun.x + dz * sun.z).max(0.0) * rad;
                    let k = (0.62 + 0.34 * (dy * 0.5 + 0.5) + 0.5 * lit + st.rr(-0.06, 0.06)) * 1.55; push_t(&mut leaf_m, &mut leaf_c, &o, base.mul(k)); }
            } else if cyp {
                let h = st.rr(9.0, 15.0); let base = { let (a, b, c) = (st.rr(0.3, 0.36), st.rr(0.28, 0.36), st.rr(0.12, 0.15)); Col::hsl(a, b, c) };
                st.trees[ti].cv = Some(Cv { x: t.x, y: t.y + h * 0.52, z: t.z, r: 1.25, l: Some(h * 0.42), d: 0.9, tr: 0.16 });
                st.obstacles.push([t.x, t.z, 0.3, 2.0]);
                o.pos(t.x, t.y - 0.3, t.z).rot(0.0, 0.0, 0.0).scale(0.16, h * 0.5, 0.16); push_t(&mut trunk_m, &mut trunk_c, &o, bark);
                for l in 0..9 { let f = l as f64 / 9.0; let y = t.y + h * (0.12 + f * 0.84); let rad = ((f * 0.9 + 0.08) * PI).sin() * st.rr(0.9, 1.2) * (1.0 - f * 0.45) + 0.25;
                    for _ in 0..3 { let a = st.rnd() * PI * 2.0; o.pos(t.x + a.cos() * rad * 0.3, y, t.z + a.sin() * rad * 0.3); let (rx, rz) = (st.rr(-0.2, 0.2), st.rr(-0.2, 0.2)); o.rot(rx, a, rz);
                        let sc = rad * 1.5 + 0.9; o.scale(sc * 0.8, sc * 1.25, sc * 0.8); let k = (0.7 + 0.45 * f + st.rr(-0.06, 0.06)) * 1.6; push_t(&mut needle_m, &mut needle_c, &o, base.mul(k)); } }
            } else {
                let h = st.rr(4.0, 6.5); let r = st.rr(0.2, 0.3); st.obstacles.push([t.x, t.z, r + 0.06, 2.0]);
                let (rx, ry, rz) = (st.rr(-0.15, 0.15), st.rnd() * 6.0, st.rr(-0.15, 0.15)); o.pos(t.x, t.y - 0.3, t.z).rot(rx, ry, rz).scale(r, h * 0.6, r); bark = rgb(0.7, 0.66, 0.6); push_t(&mut trunk_m, &mut trunk_c, &o, bark);
                let cy = t.y + h * 0.7; let rx = st.rr(2.4, 3.4); let base = { let (a, b, c) = (st.rr(0.17, 0.22), st.rr(0.14, 0.22), st.rr(0.38, 0.46)); Col::hsl(a, b, c) };
                st.trees[ti].cv = Some(Cv { x: t.x, y: cy, z: t.z, r: rx * 0.9, l: None, d: 0.7, tr: r });
                for _ in 0..16 { let u = st.rnd() * PI * 2.0; let v = st.rr(-0.6, 1.0).acos(); let sr = st.rnd().powf(0.4); let (dx, dy, dz) = (v.sin() * u.cos() * sr, v.cos() * sr, v.sin() * u.sin() * sr);
                    o.pos(t.x + dx * rx, cy + dy * rx * 0.6, t.z + dz * rx); let (a, b, c) = (st.rnd() * 3.0, st.rnd() * 3.0, st.rnd() * 3.0); o.rot(a, b, c); let sc = st.rr(2.0, 2.8); o.scalar(sc);
                    let k = (0.6 + 0.3 * (dy * 0.5 + 0.5) + 0.3 * (dx * sun.x + dz * sun.z).max(0.0) + st.rr(-0.05, 0.05)) * 1.5; push_t(&mut leaf_m, &mut leaf_c, &o, base.mul(k)); }
            }
            continue;
        }
        let pine = if st.snow { true } else if st.can { false } else if st.lak { st.rnd() < 0.55 } else if st.tou { st.rnd() < 0.4 } else { st.rnd() < 0.22 };
        let autumn = !pine && !st.can && st.rnd() < if st.lak { 0.12 } else if st.tou { 0.86 } else { 0.07 }; let birch = st.lak && !pine;
        let h = if pine { st.rr(15.0, 22.0) } else if st.can { st.rr(4.0, 7.0) } else if birch { st.rr(12.0, 17.0) } else { st.rr(11.0, 17.0) };
        let r = if pine { st.rr(0.18, 0.28) } else if st.can { st.rr(0.14, 0.24) } else if birch { st.rr(0.14, 0.22) } else { st.rr(0.25, 0.5) };
        st.obstacles.push([t.x, t.z, r + 0.06, 2.0]);
        let mut bark = WHITE.mul(st.rr(0.7, 1.05)); if pine { bark = rgb(1.05, 0.78, 0.7); } if birch { bark = rgb(2.0, 2.25, 2.5).mul(st.rr(0.85, 1.0)); } if st.can { bark = rgb(1.1, 0.95, 0.85); }
        let (rx, ry, rz) = (st.rr(-0.05, 0.05), st.rnd() * 6.0, st.rr(-0.05, 0.05)); o.pos(t.x, t.y - 0.3, t.z).rot(rx, ry, rz).scale(r, if pine { h } else { h * 0.72 }, r); push_t(&mut trunk_m, &mut trunk_c, &o, bark);
        if !pine {
            let nb = 3 + (st.rnd() * 3.0) as usize;
            for _ in 0..nb { let y = t.y + h * st.rr(0.42, 0.66); let ry = st.rnd() * 6.28; o.pos(t.x, y, t.z).rot(0.0, ry, 0.0); let rz = st.rr(0.6, 1.1); o.rotate_z(rz);
                let sy = st.rr(2.5, 4.5); o.scale(r * 0.42, sy, r * 0.42); push_t(&mut trunk_m, &mut trunk_c, &o, bark); }
            let cs = if st.can { 0.42 } else if birch { 0.62 } else { 1.0 }; let cy = t.y + h * 0.78; let rx = st.rr(4.5, 7.0) * cs; let ry = st.rr(3.0, 4.6) * cs; let rz = st.rr(4.5, 7.0) * cs;
            st.trees[ti].cv = Some(Cv { x: t.x, y: cy, z: t.z, r: (rx + rz) * 0.42, l: Some(ry * 0.6), d: if birch { 0.6 } else { 0.8 }, tr: r });
            let base = if autumn { if st.tou { [rgb(0.8, 0.16, 0.07), rgb(0.9, 0.38, 0.1), rgb(0.86, 0.62, 0.16), rgb(0.72, 0.1, 0.1)][(st.rnd() * 4.0) as usize] } else if st.lak { rgb(0.95, 0.72, 0.2) } else { rgb(0.86, 0.48, 0.18) } }
                else if st.can { let (a, b, c) = (st.rr(0.17, 0.22), st.rr(0.18, 0.28), st.rr(0.28, 0.34)); Col::hsl(a, b, c) }
                else if birch { let (a, b, c) = (st.rr(0.2, 0.25), st.rr(0.45, 0.55), st.rr(0.36, 0.42)); Col::hsl(a, b, c) }
                else { let (a, b, c) = (st.rr(0.24, 0.30), st.rr(0.35, 0.5), st.rr(0.28, 0.36)); Col::hsl(a, b, c) };
            let n = if st.can { 14 + (st.rnd() * 6.0) as usize } else if birch { 22 + (st.rnd() * 8.0) as usize } else { 32 + (st.rnd() * 12.0) as usize };
            for _ in 0..n { let u = st.rnd() * PI * 2.0; let v = st.rr(-0.7, 1.0).acos(); let s = st.rnd().powf(0.35);
                let (dx, dy, dz) = (v.sin() * u.cos() * s, v.cos() * s, v.sin() * u.sin() * s);
                o.pos(t.x + dx * rx, cy + dy * ry, t.z + dz * rz); let (a, b, c) = (st.rnd() * 3.0, st.rnd() * 3.0, st.rnd() * 3.0); o.rot(a, b, c);
                let sc = st.rr(2.8, 4.2) * if st.can { 0.55 } else if birch { 0.75 } else { 1.0 }; o.scalar(sc); let lit = (dx * sun.x + dy * sun.y + dz * sun.z).max(0.0);
                let k = (0.42 + 0.32 * (dy * 0.5 + 0.5) + 0.42 * lit * s + st.rr(-0.06, 0.06)) * 1.55; push_t(&mut leaf_m, &mut leaf_c, &o, base.mul(k)); }
        } else {
            let base = if st.snow { let (a, b, c) = (st.rr(0.38, 0.43), st.rr(0.2, 0.3), st.rr(0.12, 0.17)); Col::hsl(a, b, c) } else if st.tou { let (a, b, c) = (st.rr(0.33, 0.4), st.rr(0.28, 0.36), st.rr(0.13, 0.17)); Col::hsl(a, b, c) }
                else { let (a, b, c) = (st.rr(0.33, 0.38), st.rr(0.25, 0.35), st.rr(0.2, 0.26)); Col::hsl(a, b, c) };
            st.trees[ti].cv = Some(Cv { x: t.x, y: t.y + h * 0.58, z: t.z, r: 2.1, l: Some(h * 0.36), d: 0.85, tr: r });
            for l in 0..9 { let f = l as f64 / 9.0; let y = t.y + h * (0.32 + f * 0.68); let rad = (1.0 - f) * st.rr(2.6, 3.4) + 0.4; let cards = 4 + (st.rnd() * 2.0) as usize;
                for _ in 0..cards { let a = st.rnd() * PI * 2.0;
                    o.pos(t.x + a.cos() * rad * 0.5, y, t.z + a.sin() * rad * 0.5); let (rx, rz) = (st.rr(-0.4, 0.4), st.rr(-0.3, 0.3)); o.rot(rx, a, rz);
                    let sc = rad * 1.4 + 0.8; o.scale(sc, sc * 0.75, sc); let k = (0.6 + 0.5 * f + st.rr(-0.06, 0.06)) * 1.6; push_t(&mut needle_m, &mut needle_c, &o, base.mul(k));
                    if st.snow && st.rnd() < 0.8 { o.pos.y += 0.3; let rx = -PI / 2.0 + st.rr(-0.3, 0.3); o.rot(rx, 0.0, a); o.scalar(sc * 0.8);
                        push_t(&mut snow_m, &mut snow_c, &o, rgb(0.92, 0.95, 1.0).mul(0.85 + 0.2 * f)); } } }
        }
    }
    let m = st.mat(Mat::lambert().map(st.t_bark)); st.instanced_d(st.unit_cyl, m, &trunk_m, &trunk_c);
    let m = st.mat(Mat::basic().map(st.t_leaf).at(0.5).double().sway(0.16, false, None)); st.instanced_d(st.card, m, &leaf_m, &leaf_c);
    st.bake_sun_shadows();
    let m = st.mat(Mat::basic().map(st.t_needle).at(0.5).double().sway(0.08, false, None)); st.instanced_d(st.card, m, &needle_m, &needle_c);
    if !snow_m.is_empty() { let m = st.mat(Mat::basic().map(st.t_leaf).at(0.5).double().sway(0.06, false, None)); st.instanced_d(st.card, m, &snow_m, &snow_c); }
}

/// parts(): several geometries baked into one, coloured per vertex from its normal
fn parts(list: Vec<(Geo, Box<dyn Fn(V3, f64) -> [f64; 3]>)>) -> Geo {
    let (mut p, mut n, mut c) = (Vec::new(), Vec::new(), Vec::new());
    for (geo, cf) in list { let mut g = if geo.index.is_some() { geo.to_non_indexed() } else { geo }; g.compute_normals();
        for i in 0..g.count() { let pp = g.p(i); p.extend_from_slice(&[pp.x as f32, pp.y as f32, pp.z as f32]); let nn = g.n(i); n.extend_from_slice(&[nn.x as f32, nn.y as f32, nn.z as f32]);
            c.extend(cf(nn, pp.y).iter().map(|&v| v as f32)); } }
    Geo::new(p).with_nrm(n).with_col(c)
}

pub fn far_forest(st: &mut St) {
    let sun = st.sun;
    let shade = move |v: V3, base: [f64; 3], top: Option<[f64; 3]>| { let l = 0.5 + 0.45 * v.dot(sun).max(0.0) + 0.18 * v.y; let mut o = [base[0] * l, base[1] * l, base[2] * l]; if let Some(t) = top { for i in 0..3 { o[i] += t[i] * (v.y - 0.25).max(0.0); } } o };
    let trunk = |h: f64| { let mut t = cylinder(0.035, 0.05, h, 4, 1, true); t.translate(0.0, h / 2.0, 0.0); t };
    let cone_g = |r: f64, h: f64, y: f64| { let mut c = cone(r, h, 6, 1, true); c.translate(0.0, y + h / 2.0, 0.0); c };
    let bark = move |v: V3, _: f64| shade(v, [0.32, 0.25, 0.2], None); let white = move |v: V3, _: f64| shade(v, [1.0, 1.0, 1.0], None);
    let pine = parts(vec![(trunk(0.3), Box::new(bark)), (cone_g(0.42, 0.56, 0.18), Box::new(white)), (cone_g(0.3, 0.46, 0.52), Box::new(white))]);
    let snow_pine = parts(vec![(trunk(0.3), Box::new(bark)), (cone_g(0.42, 0.56, 0.18), Box::new(move |v: V3, _| shade(v, [0.2, 0.3, 0.24], Some([1.2, 1.2, 1.3])))), (cone_g(0.3, 0.46, 0.52), Box::new(move |v: V3, _| shade(v, [0.2, 0.3, 0.24], Some([1.3, 1.3, 1.4]))))]);
    let mut blob = icosahedron(0.5, 0); blob.scale(1.0, 0.78, 1.0); blob.translate(0.0, 0.66, 0.0);
    let leaf = parts(vec![(trunk(0.45), Box::new(bark)), (blob, Box::new(white))]);
    let mut flat = icosahedron(0.5, 0); flat.scale(1.0, 0.32, 1.0); flat.translate(0.0, 0.86, 0.0);
    let umb = parts(vec![(trunk(0.8), Box::new(bark)), (flat, Box::new(white))]); let cyp = parts(vec![(trunk(0.12), Box::new(bark)), (cone_g(0.13, 0.92, 0.06), Box::new(white))]);
    let gids = [st.geo(pine), st.geo(snow_pine), st.geo(leaf), st.geo(umb), st.geo(cyp)];
    let mat = st.mat(Mat::basic().vc());
    let mut gid_order: Vec<usize> = Vec::new(); let mut keys: Vec<i64> = Vec::new(); let mut chunks: HashMap<i64, (usize, Vec<M4>, Vec<Col>)> = HashMap::new();
    let tree_r: f64 = if st.forest { 760.0 } else if st.snow { 1050.0 } else if st.can { 900.0 } else if st.tou { 780.0 } else { 1100.0 }; let cell = 9.0; let near_band = if st.snow { 46.0 } else { 52.0 };
    let hr = |i: f64, j: f64, s: f64| hash2(i * 7.0 + s, j * 13.0 - s * 3.0);
    let (ex, ez) = (st.ex(), st.ez()); let wl = crate::world::WL;
    let roadavg = { let np = st.np(); let mut a = 0.0; let mut i = 0; while i < np { a += st.r.roadh[i]; i += 10; } a / (np as f64 / 10.0).ceil() };
    let mut o = Tmp::default();
    if !st.pri && !st.vol {
        let g0 = (-tree_r / cell).floor() as i64; let g1 = (tree_r / cell).floor() as i64;
        for gj in g0..=g1 { for gi in g0..=g1 { let (fi, fj) = (gi as f64, gj as f64);
            let x = (fi + hr(fi, fj, 1.0)) * cell; let z = (fj + hr(fi, fj, 2.0)) * cell;
            if x * x + z * z > tree_r * tree_r { continue; }
            let cx = clamp(x, -ex, ex); let cz = clamp(z, -ez, ez); let dout = hyp(x - cx, z - cz); let in_grid = dout == 0.0;
            let thin = 1.0 + dout / 260.0; if hr(fi, fj, 3.0) > 1.0 / (thin * thin) { continue; }
            let rd = st.road_dist(x, z); if rd < near_band + hr(fi, fj, 4.0) * 8.0 { continue; }
            let h = st.far_h(x, z); let slope = hyp(st.far_h(x + 3.0, z) - st.far_h(x - 3.0, z), st.far_h(x, z + 3.0) - st.far_h(x, z - 3.0)) / 6.0;
            let dens = if st.riv { if h > 3.0 && slope < 1.25 { clamp(0.75 - fbm(x * 0.03 + 50.0, z * 0.03, 4) * 0.6, 0.0, 1.0) * if h < st.r.roadh[st.path_idx(x, z)] - 6.0 { 0.3 } else { 1.0 } } else { 0.0 } }
                else if st.tou { if h < -95.0 { 0.06 } else if slope < 1.3 { clamp(1.0 - fbm(x * 0.03 + 50.0, z * 0.03, 4) * 0.7, 0.0, 1.0) * if h < st.r.roadh[st.path_idx(x, z)] - 5.0 { if rd < 220.0 { 0.1 } else { 0.4 } } else { 1.0 } } else { 0.15 } }
                else if st.can { if h < 22.0 && slope < 0.3 { 0.035 } else { 0.0 } }
                else if st.lak { if h > wl + 1.0 && slope < 1.0 { clamp(1.0 - fbm(x * 0.028 + 50.0, z * 0.028, 4) * 0.85, 0.0, 1.0) } else { 0.0 } }
                else if st.snow { if h < roadavg + 70.0 + (fbm(x * 0.01, z * 0.01, 4) - 0.5) * 50.0 && slope < 0.75 { sm(0.4, 0.66, fbm(x * 0.022 + 50.0, z * 0.022, 4)) * 0.95 } else { 0.0 } }
                else { clamp(0.95 - fbm(x * 0.03 + 50.0, z * 0.03, 4) * 0.9 + sm(80.0, 400.0, rd) * 0.25, 0.0, 1.0) * if slope < 1.1 { 1.0 } else { 0.3 } };
            if hr(fi, fj, 5.0) > dens { continue; }
            let pine = st.snow || if st.lak { hr(fi, fj, 6.0) < 0.6 } else if st.tou { hr(fi, fj, 6.0) < 0.45 } else { !st.can && hr(fi, fj, 6.0) < 0.22 }; let sc = thin.powf(0.75);
            let hh = (if pine { 15.0 + hr(fi, fj, 7.0) * 7.0 } else if st.can { 4.0 + hr(fi, fj, 7.0) * 3.0 } else { 11.0 + hr(fi, fj, 7.0) * 6.0 }) * sc; let ww = hh * if pine { 0.46 } else { 0.62 } * (0.85 + hr(fi, fj, 8.0) * 0.3);
            o.pos(x, h - 0.4, z).rot(0.0, hr(fi, fj, 9.0) * 6.28, 0.0).scale(ww, hh, ww);
            let v = 0.82 + hr(fi, fj, 10.0) * 0.3;
            let c = if st.snow { rgb(v, v, v) } else if pine { Col::hsl(0.33 + hr(fi, fj, 11.0) * 0.05, 0.3, if st.tou { 0.15 } else { 0.2 }).mul(1.45 * v) }
                else if st.can { Col::hsl(0.19, 0.22, 0.3).mul(1.2 * v) }
                else if st.tou { [rgb(0.8, 0.17, 0.08), rgb(0.9, 0.4, 0.12), rgb(0.85, 0.62, 0.18)][(hr(fi, fj, 12.0) * 3.0) as usize].mul(0.92 * v) }
                else if hr(fi, fj, 12.0) < if st.lak { 0.1 } else { 0.07 } { rgb(if st.lak { 0.95 } else { 0.86 }, if st.lak { 0.72 } else { 0.48 }, if st.lak { 0.2 } else { 0.18 }).mul(0.95 * v) }
                else { Col::hsl(if st.lak { 0.21 } else { 0.24 } + hr(fi, fj, 11.0) * 0.06, if st.lak { 0.48 } else { 0.42 }, if st.lak { 0.36 } else { 0.31 }).mul(1.25 * v) };
            let (geo, m4, col) = if st.riv { let q = hr(fi, fj, 6.0); let u = q < 0.55; let cy = !u && q < 0.75;
                    let hh = (if u { 10.0 + hr(fi, fj, 7.0) * 5.0 } else if cy { 9.0 + hr(fi, fj, 7.0) * 5.0 } else { 5.0 + hr(fi, fj, 7.0) * 2.0 }) * sc; let ww = hh * if u { 0.85 } else if cy { 0.25 } else { 0.8 };
                    o.scale(ww, hh, ww);
                    (if u { 3 } else if cy { 4 } else { 2 }, o.matrix(), if u { Col::hsl(0.27, 0.34, 0.18).mul(1.5 * v) } else if cy { Col::hsl(0.33, 0.3, 0.12).mul(1.5 * v) } else { Col::hsl(0.19, 0.18, 0.4).mul(1.3 * v) }) }
                else { (if st.snow { 1 } else if pine { 0 } else { 2 }, o.matrix(), c) };
            let gix = match gid_order.iter().position(|&g| g == geo) { Some(p) => p, None => { gid_order.push(geo); gid_order.len() - 1 } };
            let key = (x / 200.0).floor() as i64 * 64 + (z / 200.0).floor() as i64 + gix as i64 * 10000;
            let e = chunks.entry(key).or_insert_with(|| { keys.push(key); (geo, Vec::new(), Vec::new()) }); e.1.push(m4); e.2.push(col);
            if in_grid { st.obstacles.push([x, z, 0.35, 2.0]); }
        } }
    }
    for k in keys { let (geo, ms, cs) = chunks.remove(&k).unwrap(); let im = st.sc.inst(gids[geo], mat, ms.len());
        for (i, m) in ms.iter().enumerate() { st.sc.set_matrix_at(im, i, m); st.sc.set_color_at(im, i, cs[i]); }
        let bs = st.sc.inst_bs(im); st.add(im); st.cull.push(super::CullE { o: im, c: v3(bs[0], bs[1], bs[2]), r: bs[3], d: st.fogcut }); }
}

fn clump_geo(n: usize) -> Geo {
    let (mut pos, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new()); let mut off = 0u32;
    for i in 0..n { let mut p = plane(1.0, 1.0, 1, 1); p.translate(0.0, 0.5, 0.0); p.rotate_y(i as f64 * PI / n as f64);
        pos.extend_from_slice(&p.pos); uv.extend_from_slice(p.uv.as_ref().unwrap()); for &k in p.index.as_ref().unwrap() { idx.push(k + off); } off += p.count() as u32; }
    Geo::new(pos).with_uv(uv).with_index(idx)
}

pub fn cover(st: &mut St) {
    st.clump2 = st.geo(clump_geo(2)); st.clump3 = st.geo(clump_geo(3));
    let np = st.np(); let road = st.road(); let wl = crate::world::WL; let mut o = Tmp::default();
    if st.forest || st.lak || st.tou { let (mut m, mut c) = (vec![], vec![]);
        for _ in 0..4000 { let ti = (st.rnd() * st.trees.len() as f64) as usize; let t = st.trees[ti]; let a = st.rnd() * 6.28; let d = st.rr(1.0, 7.0); let (x, z) = (t.x + a.cos() * d, t.z + a.sin() * d);
            if st.path_dist(x, z) < road + 1.3 || (st.lak && st.height(x, z) < wl + 0.3) { continue; }
            let ry = st.rnd() * 6.0; o.pos(x, st.height(x, z) - 0.1, z).rot(0.0, ry, 0.0); let s = st.rr(1.0, 2.2); o.scale(s * 1.3, s, s * 1.3);
            let (h, s2, l) = (st.rr(0.25, 0.32), st.rr(0.35, 0.5), st.rr(0.26, 0.38)); push_t(&mut m, &mut c, &o, Col::hsl(h, s2, l).mul(1.5)); }
        let mt = st.mat(Mat::basic().map(st.t_fern).at(0.5).double().sway(0.12, true, None)); st.instanced(st.clump3, mt, &m, &c, 170.0, 96.0); }
    if st.snow {
        let mut geo = icosahedron(1.0, 1);
        for i in 0..geo.count() { let v = geo.p(i); let v = v.mul(0.8 + vnoise(v.x * 2.0 + 4.0, v.z * 2.0 + v.y) * 0.4); geo.set_p(i, v3(v.x, v.y.max(-0.3), v.z)); }
        geo.compute_normals(); let n = geo.count(); let mut col = vec![0f32; n * 3]; for i in 0..n { let k = 0.7 + geo.n(i).y.max(0.0) * 0.3; col[i * 3] = (0.84 * k) as f32; col[i * 3 + 1] = (0.88 * k) as f32; col[i * 3 + 2] = (0.96 * k) as f32; }
        geo.col = Some(col); let g = st.geo(geo); let (mut m, mut c) = (vec![], vec![]);
        let mut i = 0; while i < np { for sd in [-1.0, 1.0] { if st.rnd() < 0.15 { continue; }
            let off = (road + st.rr(0.75, 1.2)) * sd; let (x, z) = st.side(i, off); if st.path_dist(x, z) < road + 0.5 { continue; } let tn = st.t(i);
            let ry = tn[0].atan2(tn[1]) + st.rr(-0.3, 0.3); o.pos(x, st.height(x, z) - 0.05, z).rot(0.0, ry, 0.0); let (a, b, cc) = (st.rr(0.6, 1.0), st.rr(0.35, 0.7), st.rr(1.3, 2.2)); o.scale(a, b, cc);
            let k = st.rr(0.92, 1.05); push_t(&mut m, &mut c, &o, WHITE.mul(k)); } i += 2; }
        let mt = st.mat(Mat::lambert().vc().map(st.t_ground)); st.instanced_d(g, mt, &m, &c); }
    if st.riv || st.can { let (mut m, mut c) = (vec![], vec![]); let (n, rmax, cut) = if st.riv { (4200, 36.0, 0.44) } else { (3500, 40.0, 0.42) };
        for _ in 0..n { let k = (st.rnd() * np as f64) as usize; let off = st.rr(road + 1.0, rmax) * if st.rnd() < 0.5 { -1.0 } else { 1.0 }; let (bx, bz) = st.side(k, off); let x = bx + st.rr(-2.0, 2.0); let z = bz + st.rr(-2.0, 2.0);
            if st.riv { if st.path_dist(x, z) < road + 0.9 || fbm(x * 0.06, z * 0.06, 4) < cut || st.height(x, z) < 2.0 || st.r.tun_at(k).is_some() || (st.r.in_village(k) && st.path_dist(x, z) < 13.0) { continue; } }
            else if st.path_dist(x, z) < road + 0.8 || fbm(x * 0.06, z * 0.06, 4) < cut { continue; }
            let ry = st.rnd() * 6.0; o.pos(x, st.height(x, z) - 0.05, z).rot(0.0, ry, 0.0);
            if st.riv { let sc = st.rr(0.55, 1.25); o.scale(sc * 1.6, sc, sc * 1.6); let q = st.rnd(); let col = if q < 0.62 { let (a, b, cc) = (st.rr(0.42, 0.55), st.rr(0.52, 0.62), st.rr(0.3, 0.36)); rgb(a, b, cc) } else if q < 0.82 { rgb(0.66, 0.52, 0.95) } else { rgb(1.05, 0.88, 0.25) }; push_t(&mut m, &mut c, &o, col); }
            else { let sc = st.rr(0.5, 1.2); o.scale(sc * 1.5, sc, sc * 1.5); let (a, b, cc) = (st.rr(0.85, 1.0), st.rr(0.68, 0.8), st.rr(0.42, 0.5)); push_t(&mut m, &mut c, &o, rgb(a, b, cc)); } }
        let mt = st.mat(Mat::basic().map(st.t_fern).at(0.5).double().sway(0.08, true, None)); st.instanced(st.clump3, mt, &m, &c, 200.0, 96.0); }
    if st.forest || st.lak { let (mut m, mut c) = (vec![], vec![]);
        for _ in 0..6000 { let k = (st.rnd() * np as f64) as usize; let side = if st.rnd() < 0.5 { -1.0 } else { 1.0 }; let off = st.rr(road + 0.5, road + 2.8) * side; let (bx, bz) = st.side(k, off);
            let x = bx + st.rr(-0.5, 0.5); let z = bz + st.rr(-0.5, 0.5);
            if fbm(x * 0.09, z * 0.09, 4) < 0.48 || st.path_dist(x, z) < road + 0.4 || (st.lak && st.height(x, z) < wl + 0.3) { continue; }
            let ry = st.rnd() * 6.0; o.pos(x, st.height(x, z) - 0.05, z).rot(0.0, ry, 0.0); let s = st.rr(0.45, 0.85); o.scale(s * 1.3, s, s * 1.3);
            let col = if st.lak { if st.rnd() < 0.7 { rgb(1.05, 0.45, 0.85) } else { rgb(1.1, 1.1, 1.05) } } else if st.rnd() < 0.06 { rgb(0.85, 0.15, 0.12) } else { rgb(1.12, 1.02, 0.28) }; push_t(&mut m, &mut c, &o, col); }
        let mt = st.mat(Mat::basic().map(st.t_flower).at(0.5).double().sway(0.08, true, None)); st.instanced(st.clump2, mt, &m, &c, 160.0, 96.0); }
    // rocks
    { let mut geo = icosahedron(1.0, 2);
        for i in 0..geo.count() { let v = geo.p(i); let v = v.mul(0.75 + fbm(v.x * 1.7 + 3.0, v.y * 1.7 + v.z, 4) * 0.55); geo.set_p(i, v3(v.x, v.y * 0.7, v.z)); }
        geo.compute_normals(); let n = geo.count(); let mut col = vec![0f32; n * 3];
        for i in 0..n { let g = (if st.snow { 0.27 } else { 0.42 }) + st.rnd() * 0.08; let m = ((geo.n(i).y - 0.35) * 1.6).max(0.0);
            let c3 = if st.riv { let k = 0.82 + st.rnd() * 0.1 - m * 0.1; [0.86 * k, 0.78 * k, 0.66 * k] }
                else if st.can { let k = (m * 1.2).min(1.0); let r0 = 0.6 + st.rnd() * 0.08; [r0 + (0.8 - r0) * k, r0 * 0.52 + (0.56 - r0 * 0.52) * k, r0 * 0.36 + (0.38 - r0 * 0.36) * k] }
                else if st.snow { let k = (m * 1.3).min(1.0); [g + (0.84 - g) * k, g + (0.88 - g) * k, g * 1.08 + (0.96 - g * 1.08) * k] }
                else { [g + (0.24 - g) * m, g + (0.36 - g) * m, g * 0.98 + (0.16 - g) * m] };
            for k in 0..3 { col[i * 3 + k] = c3[k] as f32; } }
        geo.col = Some(col); let g = st.geo(geo); let (mut m, mut c) = (vec![], vec![]);
        let n = if st.pri { 0 } else if st.can { 640 } else if st.vol { 520 } else { 260 };
        for _ in 0..n { let k = (st.rnd() * np as f64) as usize; let off = st.rr(road + 4.0, if st.can { 60.0 } else { 40.0 }) * if st.rnd() < 0.5 { -1.0 } else { 1.0 }; let (x, z) = st.side(k, off);
            if st.path_dist(x, z) < road + 3.5 || (st.riv && (st.r.tun_at(k).is_some() || st.r.in_village(k) || st.height(x, z) < 1.0)) || (st.vol && crate::world::ember_canyon(x, z).0 > 0.0) { continue; }
            let s = if st.rnd() < 0.15 { st.rr(2.5, 4.0) } else { st.rr(0.8, 2.0) };
            if hyp(st.height(x + 2.0, z) - st.height(x - 2.0, z), st.height(x, z + 2.0) - st.height(x, z - 2.0)) > 2.4 { continue; }
            let ry = st.rnd() * 6.0; o.pos(x, st.height(x, z) - s * 0.15, z).rot(0.0, ry, 0.0); let (a, b) = (st.rr(1.0, 1.5), st.rr(1.0, 1.4)); o.scale(s * a, s, s * b);
            push_t(&mut m, &mut c, &o, WHITE); st.obstacles.push([x, z, s * 0.95, 0.0]); }
        let mt = st.mat(Mat::lambert().vc().map(st.t_ground)); st.instanced_d(g, mt, &m, &c); }
    // logs
    if !st.can && !st.riv && !st.pri && !st.vol { let mut geo = cylinder(1.0, 1.0, 1.0, 8, 1, false); geo.rotate_z(PI / 2.0); let g = st.geo(geo); let (mut m, mut c) = (vec![], vec![]);
        for _ in 0..40 { let k = (st.rnd() * np as f64) as usize; let off = st.rr(road + 6.0, 35.0) * if st.rnd() < 0.5 { -1.0 } else { 1.0 }; let (x, z) = st.side(k, off);
            if st.path_dist(x, z) < road + 5.0 { continue; } let r = st.rr(0.3, 0.5); let len = st.rr(4.0, 8.0); let a = st.rnd() * 6.0;
            let rz = st.rr(-0.06, 0.06); o.pos(x, st.height(x, z) + r * 0.6, z).rot(0.0, a, rz).scale(len, r, r); push_t(&mut m, &mut c, &o, rgb(0.8, 0.82, 0.7));
            for q in -1i32..=1 { let q = q as f64; st.obstacles.push([x + a.cos() * q * len * 0.35, z - a.sin() * q * len * 0.35, r + 0.15, 2.0]); } }
        let mt = st.mat(Mat::lambert().map(st.t_bark)); st.instanced_d(g, mt, &m, &c); }
}
