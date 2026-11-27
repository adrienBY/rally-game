//! Falling snow and the dust buffer, stars and the prism's planet, and the set pieces of Touge (guardrails, lanterns, torii, the town)
//! and the Riviera (parapets, tunnels, the village, villas, palms, bougainvillea, the lighthouse, boats).
use super::canvas::{self};
use super::shape::shape_geo;
use super::three::*;
use super::{hyp, push_t, sm, whites, Boat, St};
use crate::noise::{fbm, hash2};
use std::f64::consts::PI;

pub fn snow_and_dust(st: &mut St) {
    st.sfn = if st.snow { 1100 } else if st.tou { 420 } else if st.vol { 700 } else { 0 }; let n = st.sfn.max(1);
    st.sf_pos = vec![0.0; n * 3]; st.sf_seed = vec![0.0; n];
    for i in 0..st.sfn { st.sf_pos[i * 3] = st.rr(-32.0, 32.0) as f32; st.sf_pos[i * 3 + 1] = st.rr(-6.0, 20.0) as f32; st.sf_pos[i * 3 + 2] = st.rr(-32.0, 32.0) as f32; st.sf_seed[i] = st.rnd() as f32; }
    let g = Geo::new(st.sf_pos.clone()); let mut m = Mat::points(if st.tou || st.vol { 2.0 } else { 1.6 }, Col::hex(if st.vol { 0xff7a28 } else if st.tou { 0xd65a2a } else { 0xf4f8ff })).transparent().opacity(0.85).no_dw();
    let _ = &mut m; let m = st.mat(m); let gg = st.geo(g); let o = st.sc.points(gg, m); st.sc.objs[o].culled = false; st.sc.objs[o].visible = st.snow || st.tou || st.vol; st.add(o); st.snowfall = o;
    // the app's dust: 6000 particles (position, alpha, size, colour)
    let dn = st.dn; let mut g = Geo::new(vec![0.0; dn * 3]); g.a_a = Some(vec![0.0; dn]); g.a_s = Some(vec![0.0; dn]); g.col = Some(vec![0.0; dn * 3]);
    let col = if st.tou || st.riv { rgb(0.68, 0.68, 0.72) } else if st.can { rgb(0.86, 0.64, 0.45) } else if st.lak { rgb(0.62, 0.56, 0.48) } else if st.snow { rgb(0.86, 0.9, 0.96) } else { rgb(0.6, 0.5, 0.37) };
    let mut dm = Mat::shader(Kind::Dust).transparent().no_dw(); dm.dust = Some((st.fog_d, st.fog, col)); let dm = st.mat(dm); let gg = st.geo(g);
    let o = st.sc.points(gg, dm); st.sc.objs[o].culled = false; st.sc.objs[o].ro = 10; st.add(o); st.dust = o;
    // the soft-particle layer of the old iceland stage: one empty point, hidden
    let mut g = Geo::new(vec![0.0; 3]); g.a_a = Some(vec![0.0]); g.a_s = Some(vec![0.0]);
    let mut fm = Mat::shader(Kind::Dust).transparent().no_dw(); fm.dust = Some((st.fog_d, st.fog, rgb(0.9, 0.93, 0.97))); let fm = st.mat(fm); let gg = st.geo(g);
    let o = st.sc.points(gg, fm); st.sc.objs[o].culled = false; st.sc.objs[o].visible = false; st.add(o);
}

pub fn stars(st: &mut St) {
    let sg = st.sc.group(); st.add(sg); st.star_g = sg; let ag = st.sc.group(); st.add(ag); st.aur_g = ag;
    if st.tou || st.pri {
        let sets: Vec<(usize, f64, f64, u32)> = if st.pri { vec![(2600, 1.0, 0.9, 0xe6e0ff), (260, 2.0, 1.0, 0xfff4ff), (140, 2.0, 0.95, 0x9fd8ff)] } else { vec![(1100, 1.0, 0.85, 0xdfe6ff), (110, 2.0, 0.9, 0xfff6e8)] };
        for (n, size, op, col) in sets { let mut pos = vec![0f32; n * 3];
            for i in 0..n { let u = st.rnd() * PI * 2.0; let y = if st.pri { st.rr(-1.0, 1.0) } else { st.rr(0.16, 1.0) }; let r = (1.0 - y * y).sqrt(); pos[i * 3] = (u.cos() * r * 1860.0) as f32; pos[i * 3 + 1] = (y * 1860.0) as f32; pos[i * 3 + 2] = (u.sin() * r * 1860.0) as f32; }
            let g = st.geo(Geo::new(pos)); let m = st.mat(Mat::points(size, Col::hex(col)).transparent().opacity(op).no_fog().no_dw()); let o = st.sc.points(g, m); st.sc.objs[o].culled = false; st.sc.add_to(sg, o); }
    }
    if st.pri {
        let pt = st.canvas_tex(8, 128, false, |_, g| { let h = 128.0; for y in 0..128 { let v = y as f64 / h; let b = 0.5 + 0.5 * (v * 31.0 + (v * 9.0).sin() * 2.0).sin();
            let (r, gg, bb) = (120.0 + 90.0 * b, 60.0 + 50.0 * (v * 13.0 + 1.0).sin(), 170.0 + 70.0 * (1.0 - b)); g.fill_c(canvas::rgbi(r, gg, bb)); g.fill_rect(0.0, y as f64, 8.0, 1.0); } });
        let m = st.mat(Mat::basic().map(pt).no_fog()); let pl = st.mesh(sphere(260.0, 32, 20), m); st.sc.set_pos(pl, -760.0, 330.0, -1000.0); st.sc.set_rot(pl, 0.0, 0.0, 0.35); st.sc.add_to(sg, pl);
        let mut rg = ring(330.0, 520.0, 64, 1); let n = rg.count(); let mut rc = Vec::with_capacity(n * 3);
        for i in 0..n { let d = hyp(rg.pos[i * 3] as f64, rg.pos[i * 3 + 1] as f64); let f = (d - 330.0) / 190.0; let k = 0.55 + 0.45 * (f * 40.0).sin() * (f * 7.0 + 1.0).sin(); rc.extend_from_slice(&[(0.75 * k) as f32, (0.55 * k) as f32, (0.95 * k) as f32]); }
        rg.col = Some(rc); let m = st.mat(Mat::basic().vc().double().no_fog().transparent().no_dw().blend(ADDITIVE).blend2()); let o = st.mesh(rg, m);
        st.sc.set_pos(o, -760.0, 330.0, -1000.0); st.sc.set_rot(o, -1.25, 0.2, 0.35); st.sc.add_to(sg, o);
    }
}

pub fn touge(st: &mut St) {
    let np = st.np(); let road = st.road(); let mut o = Tmp::default();
    let at = |st: &St, i: i64, off: f64, sd: f64| st.side(st.r.wrap(i), off * sd);
    { let (mut rm, mut pm, mut fm) = (vec![], vec![], vec![]); let rail = st.geo(box_geo(0.07, 0.32, 1.0)); let mut post = box_geo(0.1, 0.8, 0.1); post.translate(0.0, 0.4, 0.0); let post = st.geo(post); let refl = st.geo(box_geo(0.05, 0.1, 0.14));
        let mut i = 0; while i < np { let sd = st.drop[i]; if sd != 0 && st.drop[(i + 2) % np] == sd && !(st.r.ramp_i >= 0 && st.r.cdist(i, st.r.ramp_i as usize) < 14) { let sd = sd as f64;
            let a = at(st, i as i64, road + 1.15, sd); let b = at(st, i as i64 + 2, road + 1.15, sd); let (mx, mz) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0); let len = hyp(b.0 - a.0, b.1 - a.1);
            o.pos(mx, st.height(mx, mz) + 0.62, mz).rot(0.0, (b.0 - a.0).atan2(b.1 - a.1), 0.0).scale(1.0, 1.0, len * 1.04); rm.push(o.matrix());
            if i % 4 == 0 { o.pos(a.0, st.height(a.0, a.1) - 0.05, a.1).scale(1.0, 1.0, 1.0); pm.push(o.matrix()); }
            if i % 10 == 0 { o.pos(mx, st.height(mx, mz) + 0.83, mz).scale(1.0, 1.0, 1.0); fm.push(o.matrix()); }
            let q = at(st, i as i64 + 1, road + 1.25, sd); st.obstacles.push([q.0, q.1, 0.32, 1.0]); } i += 2; }
        let m = st.mat(Mat::lambert().hex(0xc6cacf)); st.instanced_dd(rail, m, &rm, &whites(rm.len()), 340.0);
        let m = st.mat(Mat::lambert().hex(0x80848a)); st.instanced_dd(post, m, &pm, &whites(pm.len()), 300.0);
        let m = st.mat(Mat::basic().hex(0xffa040)); st.instanced_dd(refl, m, &fm, &whites(fm.len()), 220.0); }
    { let stone = st.mat(Mat::lambert().hex(0xa29e94).map(st.t_ground)); let glow = st.mat(Mat::basic().hex(0xffb45a));
        let mut roof = cone(0.66, 0.4, 4, 1, false); roof.rotate_y(PI / 4.0);
        let parts = vec![(box_geo(0.8, 0.28, 0.8), 0.14, stone), (box_geo(0.3, 0.86, 0.3), 0.71, stone), (box_geo(0.62, 0.12, 0.62), 1.16, stone), (box_geo(0.48, 0.42, 0.48), 1.43, glow), (roof, 1.84, stone), (box_geo(0.16, 0.18, 0.16), 2.12, stone)];
        for (geo, y, mat) in parts { let g = st.geo(geo); let (mut m, mut c) = (vec![], vec![]);
            for l in st.lan.clone() { let t = st.t(l.i); o.pos(l.x, l.y + y, l.z).rot(0.0, t[0].atan2(t[1]), 0.0).scale(1.0, 1.0, 1.0); push_t(&mut m, &mut c, &o, WHITE); }
            st.instanced_d(g, mat, &m, &c); } }
    { let red = st.mat(Mat::lambert().hex(0xd8401f)); let blk = st.mat(Mat::lambert().hex(0x1e1b1b));
        for f in [0.14, 0.47, 0.8] { let (mut best, mut bv) = (0usize, 0.0); let c0 = super::jr(np as f64 * f) as i64;
            for k in c0 - 70..c0 + 70 { let q = st.r.wrap(k); if st.r.vcurve[q] > bv && (st.r.ramp_i < 0 || st.r.cdist(q, st.r.ramp_i as usize) > 40) { bv = st.r.vcurve[q]; best = q; } }
            let k = best; let p = st.p(k); let tn = st.t(k); let yaw = tn[0].atan2(tn[1]); let half = road + 1.45; let y0 = st.r.roadh[k];
            for sd in [-1.0, 1.0] { let q = at(st, k as i64, half, sd); let gy = st.height(q.0, q.1);
                let pil = st.mesh(cylinder(0.25, 0.3, 6.6, 10, 1, false), red); st.sc.set_pos(pil, q.0, gy + 3.0, q.1); st.add(pil);
                let ft = st.mesh(cylinder(0.34, 0.34, 0.5, 10, 1, false), blk); st.sc.set_pos(ft, q.0, gy + 0.2, q.1); st.add(ft); st.obstacles.push([q.0, q.1, 0.42, 2.0]); }
            for (w, h, d, y, mat) in [(half * 2.0 + 2.6, 0.38, 0.62, 6.25, red), (half * 2.0 + 3.0, 0.17, 0.72, 6.52, blk), (half * 2.0 + 0.9, 0.26, 0.26, 5.0, red), (0.5, 0.95, 0.2, 5.6, blk)] {
                let b = st.mesh(box_geo(w, h, d), mat); st.sc.set_pos(b, p[0], y0 + y, p[1]); st.sc.set_rot(b, 0.0, yaw, 0.0); st.add(b); } } }
    { let (ex, ez) = (st.ex(), st.ez()); let mut t: [Vec<f32>; 3] = [vec![], vec![], vec![]]; let mut tries = 0;
        while tries < 140000 && t[0].len() + t[1].len() + t[2].len() < 9000 { tries += 1;
            let a = st.rnd() * PI * 2.0; let r = st.rr(460.0, 1180.0); let (x, z) = (a.cos() * r, a.sin() * r);
            if x.abs() < ex && z.abs() < ez { continue; } if fbm(x * 0.005 + 7.0, z * 0.005 - 3.0, 3) < 0.52 { continue; }
            let h = st.far_h(x, z); if h > -70.0 { continue; }
            let c = st.rnd(); let k = if c < 0.62 { 0 } else if c < 0.9 { 1 } else { 2 }; t[k].extend_from_slice(&[x as f32, (h + 1.2) as f32, z as f32]); }
        for (i, list) in t.iter().enumerate() { if list.is_empty() { continue; } let g = st.geo(Geo::new(list.clone()));
            let m = st.mat(Mat::points(if i == 1 { 1.0 } else { 2.0 }, Col::hex([0xffbe6a, 0xfff0d6, 0x9ec2ff][i])).no_fog()); let ob = st.sc.points(g, m); st.sc.objs[ob].culled = false; st.add(ob); }
        if !t[0].is_empty() { let n = (t[0].len() / 3) as f64; let (mut ax, mut ay, mut az) = (0.0, 0.0, 0.0); for c in t[0].chunks_exact(3) { ax += c[0] as f64; ay += c[1] as f64; az += c[2] as f64; } st.town = Some(v3(ax / n, ay / n, az / n)); } }
}

pub fn riviera(st: &mut St) {
    let np = st.np(); let road = st.road(); let mut o = Tmp::default();
    let stone_t = st.canvas_tex(64, 64, true, |st, g| { let s = 64.0; g.fill_style("rgb(112,98,84)"); g.fill_rect(0.0, 0.0, s, s);
        let mut y = 0.0; while y < s { let hh = 6.0 + (st.rnd() * 5.0).floor(); let mut x = -(st.rnd() * 10.0).floor(); while x < s { let ww = 8.0 + (st.rnd() * 12.0).floor(); let l = 0.82 + st.rnd() * 0.3;
            g.fill_c(canvas::rgbi(218.0 * l, 198.0 * l, 166.0 * l)); g.fill_rect(x + 1.0, y + 1.0, ww - 2.0, hh - 2.0); let a = st.rr(0.1, 0.25); g.fill_c(canvas::rgba(255.0, 244.0, 224.0, a)); g.fill_rect(x + 1.0, y + 1.0, ww - 2.0, 1.0); x += ww; } y += hh; } });
    { let (mut m, mut c) = (vec![], vec![]); let mut geo = box_geo(0.44, 0.8, 1.0); geo.translate(0.0, 0.4, 0.0); let g = st.geo(geo);
        let mut i = 0; while i < np { let sd = st.drop[i]; let j = (i + 2) % np;
            if !(sd == 0 || st.drop[j] != sd || st.r.tun_at(i).is_some() || st.r.tun_at(j).is_some() || st.r.in_village(i) || st.r.in_village(j)) { let sd = sd as f64;
                let a = st.side(i, (road + 1.15) * sd); let b = st.side(j, (road + 1.15) * sd); let (mx, mz) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0); let len = hyp(b.0 - a.0, b.1 - a.1);
                o.pos(mx, st.height(mx, mz).min(st.r.roadh[i]) - 0.06, mz).rot(0.0, (b.0 - a.0).atan2(b.1 - a.1), 0.0).scale(1.0, 1.0, len * 1.03); m.push(o.matrix()); c.push(WHITE.mul(0.9 + hash2(i as f64, 5.0) * 0.14));
                let q = st.side((i + 1) % np, (road + 1.2) * sd); st.obstacles.push([q.0, q.1, 0.34, 0.0]); }
            i += 2; }
        let mt = st.mat(Mat::lambert().map(stone_t)); st.instanced_dd(g, mt, &m, &c, 360.0); }
    { let tun_t = st.canvas_tex(64, 64, true, |st, g| { let s = 64.0; g.fill_style("rgb(150,146,138)"); g.fill_rect(0.0, 0.0, s, s);
            for _ in 0..500 { let c = if st.rnd() < 0.5 { canvas::rgba(90.0, 86.0, 80.0, st.rr(0.15, 0.4)) } else { canvas::rgba(190.0, 186.0, 176.0, st.rr(0.1, 0.3)) }; g.fill_c(c); let x = st.rr(0.0, s) as i64 as f64; let y = st.rr(0.0, s) as i64 as f64; g.fill_rect(x, y, 2.0, 2.0); }
            g.fill_style("rgba(60,56,52,0.55)"); let mut y = 0.0; while y < s { g.fill_rect(0.0, y, s, 1.0); y += 16.0; } let mut x = 0.0; while x < s { g.fill_rect(x, 0.0, 1.0, s); x += 32.0; } });
        let mut prof: Vec<(f64, f64)> = vec![(-4.4, -0.6)]; for a in 0..=12 { let th = PI - a as f64 / 12.0 * PI; prof.push((th.cos() * 4.4, 3.4 + th.sin() * 2.7)); } prof.push((4.4, -0.6));
        let mut plen = vec![0.0]; for q in 1..prof.len() { plen.push(plen[q - 1] + hyp(prof[q].0 - prof[q - 1].0, prof[q].1 - prof[q - 1].1)); }
        let tube_m = st.mat(Mat::basic().map(tun_t).vc().double()); let port_m = st.mat(Mat::lambert().map(stone_t).double()); let cap_m = st.mat(Mat::lambert().map(st.t_ground).vc().double());
        let (mut lm, mut lc) = (vec![], vec![]); let lamp_g = st.geo(box_geo(0.5, 0.14, 1.3));
        let tuns: Vec<(usize, usize)> = st.r.tun.iter().map(|t| (t.m, t.half)).collect();
        for (tm, half) in tuns { let tref = super::road::Tunnel { m: tm, i0: 0, i1: 0, half }; let n_p = prof.len();
            let (mut p, mut c, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new()); let mut rows = 0;
            for s2 in -(half as i64)..=half as i64 { let k = st.r.wrap(tm as i64 + s2); let pp = st.p(k); let tn = st.t(k); let (nx, nz) = (-tn[1], tn[0]); let y0 = st.r.roadh[k]; let gl = st.r.tun_lamp(&tref, k);
                for q in 0..n_p { let (w, y) = prof[q]; p.extend_from_slice(&[(pp[0] + nx * w) as f32, (y0 + y) as f32, (pp[1] + nz * w) as f32]); uv.extend_from_slice(&[(plen[q] / 3.0) as f32, (s2 as f64 / 3.0) as f32]);
                    let up = sm(1.0, 5.5, y); let b = 0.12 + 0.05 * up + gl * (0.16 + 0.5 * up); c.extend_from_slice(&[(b * 1.18) as f32, (b * 0.96) as f32, (b * 0.74) as f32]); }
                rows += 1; }
            for r in 0..rows - 1 { for q in 0..n_p - 1 { let a = (r * n_p + q) as u32; let b = a + n_p as u32; idx.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]); } }
            let ob = st.mesh(Geo::new(p).with_uv(uv).with_col(c).with_index(idx), tube_m); st.add(ob);
            let mut l = -(half as i64) + 4; while l <= half as i64 - 3 { let k = st.r.wrap(tm as i64 + l); let pp = st.p(k); let tn = st.t(k); o.pos(pp[0], st.r.roadh[k] + 6.0, pp[1]).rot(0.0, tn[0].atan2(tn[1]), 0.0).scale(1.0, 1.0, 1.0); push_t(&mut lm, &mut lc, &o, WHITE); l += 8; }
            let mut s2 = -(half as f64); while s2 <= half as f64 { let k = st.r.wrap(tm as i64 + super::jr(s2) as i64); for sd in [-1.0, 1.0] { let q = st.side(k, 3.95 * sd); st.obstacles.push([q.0, q.1, 0.45, 0.0]); } s2 += 1.5; }
            for sg in [-1.0, 1.0] { let k = st.r.wrap(tm as i64 + (sg * half as f64) as i64); let pp = st.p(k); let tn = st.t(k); let (nx, nz) = (-tn[1], tn[0]);
                let top = (17f64).min((8f64).max(crate::world::riv_land(pp[0], pp[1], true) - st.r.roadh[k] - 0.3));
                let shp = vec![(-15.5, -1.6), (15.5, -1.6), (15.5, top), (-15.5, top), (-15.5, -1.6)];
                let mut hole = vec![(4.4, -1.0)]; for q in (1..=prof.len() - 2).rev() { hole.push(prof[q]); } hole.push((-4.4, -1.0)); hole.push((4.4, -1.0));
                let mut pg = shape_geo(&shp, &[hole]); { let n = pg.count(); let u = pg.uv.as_mut().unwrap(); for i in 0..n { u[i * 2] = (pg.pos[i * 3] as f64 / 3.0) as f32; u[i * 2 + 1] = (pg.pos[i * 3 + 1] as f64 / 3.0) as f32; } }
                let m4 = M4::basis(v3(nx, 0.0, nz), v3(0.0, 1.0, 0.0), v3(tn[0], 0.0, tn[1]), v3(pp[0] + tn[0] * sg * 0.05, st.r.roadh[k], pp[1] + tn[1] * sg * 0.05));
                pg.apply(&m4); pg.compute_normals(); let ob = st.mesh(pg, port_m); st.add(ob);
                let mut band = vec![(5.4, -0.6)]; for q in (1..=prof.len() - 2).rev() { band.push((prof[q].0 * 1.23, (prof[q].1 - 3.4) * 1.3 + 3.4)); } band.push((-5.4, -0.6)); band.push((-4.4, -0.6));
                for q in 1..=prof.len() - 2 { band.push(prof[q]); } band.push((4.4, -0.6)); band.push((5.4, -0.6));
                let mut bg = shape_geo(&band, &[]); { let n = bg.count(); let u = bg.uv.as_mut().unwrap(); for i in 0..n { u[i * 2] = (bg.pos[i * 3] as f64 / 1.5) as f32; u[i * 2 + 1] = (bg.pos[i * 3 + 1] as f64 / 1.5) as f32; } }
                bg.apply(&M4::basis(v3(nx, 0.0, nz), v3(0.0, 1.0, 0.0), v3(tn[0], 0.0, tn[1]), v3(pp[0] + tn[0] * sg * 0.35, st.r.roadh[k], pp[1] + tn[1] * sg * 0.35))); bg.compute_normals();
                let bm = st.mat(Mat::lambert().map(stone_t).hex(0xd8cbb4).double()); let ob = st.mesh(bg, bm); st.add(ob); }
            { let lat = [-12.0, -8.0, -4.5, 0.0, 4.5, 8.0, 12.0]; let (mut p2, mut c2, mut i2) = (Vec::new(), Vec::new(), Vec::new()); let mut rows2 = 0;
                let mut s2 = -(half as i64); while s2 <= half as i64 { let k = st.r.wrap(tm as i64 + s2);
                    for &w in &lat { let (x, z) = st.side(k, w); let y = (st.r.roadh[k] + 6.6).max(crate::world::riv_land(x, z, true) - 0.25); p2.extend_from_slice(&[x as f32, y as f32, z as f32]);
                        let cc = st.ground_col(x, z, y, 0.6, fbm(x * 0.15, z * 0.15, 4), fbm(x * 0.04 + 7.0, z * 0.04, 4)); c2.extend(cc.iter().map(|&v| v as f32)); }
                    rows2 += 1; s2 += 2; }
                for r in 0..rows2 - 1 { for q in 0..lat.len() - 1 { let a = (r * lat.len() + q) as u32; let b = a + lat.len() as u32; i2.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]); } }
                let n = p2.len() / 3; let mut uv2 = vec![0f32; n * 2]; for i in 0..n { uv2[i * 2] = (p2[i * 3] as f64 / 3.2) as f32; uv2[i * 2 + 1] = (p2[i * 3 + 2] as f64 / 3.2) as f32; }
                let mut g2 = Geo::new(p2).with_col(c2).with_index(i2).with_uv(uv2); g2.compute_normals(); let ob = st.mesh(g2, cap_m); st.add(ob); } }
        let m = st.mat(Mat::basic().hex(0xffc46e)); st.instanced_dd(lamp_g, m, &lm, &lc, 300.0); }
    // the village
    let facade = st.canvas_tex(64, 64, true, |st, g| { let s = 64.0; g.fill_style("rgb(236,230,218)"); g.fill_rect(0.0, 0.0, s, s);
        for _ in 0..300 { let c = if st.rnd() < 0.5 { (160.0, 140.0, 120.0) } else { (255.0, 250.0, 240.0) }; let a = st.rr(0.05, 0.16); g.fill_c(canvas::rgba(c.0, c.1, c.2, a)); let x = st.rr(0.0, s) as i64 as f64; let y = st.rr(0.0, s) as i64 as f64; g.fill_rect(x, y, 2.0, 2.0); }
        g.fill_style("rgba(120,96,72,0.35)"); g.fill_rect(0.0, 0.0, s, 2.0);
        g.fill_style("rgb(46,44,52)"); g.fill_rect(24.0, 16.0, 16.0, 30.0); g.fill_style("rgb(252,236,200)"); g.fill_rect(23.0, 46.0, 18.0, 2.0);
        g.fill_style("rgba(255,190,110,0.7)"); g.fill_rect(26.0, 19.0, 12.0, 9.0);
        for x0 in [13.0, 41.0] { g.fill_style("rgb(62,108,78)"); g.fill_rect(x0, 15.0, 10.0, 32.0); g.fill_style("rgba(20,46,30,0.6)"); let mut y = 17.0; while y < 46.0 { g.fill_rect(x0 + 1.0, y, 8.0, 1.0); y += 3.0; } } });
    let wall_m = st.mat(Mat::lambert().map(facade).vc()); let roof_m = st.mat(Mat::lambert().map(st.t_ground).vc());
    let pal = [[0.94, 0.79, 0.52], [0.92, 0.63, 0.45], [0.96, 0.89, 0.75], [0.9, 0.56, 0.46], [0.97, 0.84, 0.6], [0.86, 0.74, 0.66], [0.95, 0.72, 0.56]];
    let house = |st: &mut St, cx: f64, cz: f64, yaw: f64, w: f64, d: f64, y0: f64, y1: f64, col: [f64; 3]| {
        let h = y1 - y0; let mut g = box_geo(d, h, w); { let uv = g.uv.as_mut().unwrap(); for f in 0..6 { let su = if f < 2 { w / 3.2 } else if f >= 4 { d / 3.2 } else { 0.02 }; let sv = if f < 2 || f >= 4 { h / 3.0 } else { 0.02 };
            for v in 0..4 { let q = f * 4 + v; uv[q * 2] = (uv[q * 2] as f64 * su) as f32; uv[q * 2 + 1] = (uv[q * 2 + 1] as f64 * sv) as f32; } } }
        let mut cl = vec![0f32; 72]; for i in 0..24 { let k = 0.94 + hash2(i as f64, (cx * 3.0) as i64 as f64) * 0.08; for q in 0..3 { cl[i * 3 + q] = (col[q] * k) as f32; } } g.col = Some(cl);
        let m = st.mesh(g, wall_m); st.sc.set_pos(m, cx, (y0 + y1) / 2.0, cz); st.sc.set_rot(m, 0.0, yaw, 0.0); st.add(m);
        let rh = w.min(d) * 0.3; let mut rg = cone(1.0, 1.0, 4, 1, false).to_non_indexed(); rg.rotate_y(PI / 4.0); rg.scale(d * 0.8, rh, w * 0.8);
        let n = rg.count(); let t0 = st.rr(0.85, 1.05); let mut rc = vec![0f32; n * 3]; for i in 0..n { rc[i * 3] = (0.78 * t0) as f32; rc[i * 3 + 1] = (0.38 * t0) as f32; rc[i * 3 + 2] = (0.24 * t0) as f32; }
        rg.col = Some(rc); rg.compute_normals(); let r = st.mesh(rg, roof_m); st.sc.set_pos(r, cx, y1 + rh / 2.0, cz); st.sc.set_rot(r, 0.0, yaw, 0.0); st.add(r); };
    let (vp, vn) = st.r.vil.unwrap(); let (i0, i1) = (vp as f64 + 24.0, (vp + vn) as f64 - 20.0); let mut tower = false;
    for sd in [-1.0, 1.0] { let mut i = i0 + if sd > 0.0 { 4.0 } else { 0.0 };
        while i < i1 { let w = st.rr(6.0, 10.0); let d = st.rr(6.5, 9.5); let gap = st.rr(0.3, 1.6); let k = super::jr(i + w / 2.0) as usize % np; let p = st.p(k); let tn = st.t(k); let (nx, nz) = (-tn[1] * sd, tn[0] * sd); let front = road + 2.4;
            let corner = |a: f64, b: f64| (p[0] + tn[0] * a + nx * b, p[1] + tn[1] * a + nz * b);
            let cs = [corner(-w / 2.0, front), corner(w / 2.0, front), corner(-w / 2.0, front + d), corner(w / 2.0, front + d)]; let gh: Vec<f64> = cs.iter().map(|q| st.height(q.0, q.1)).collect();
            let gmin = gh.iter().cloned().fold(f64::INFINITY, f64::min);
            if cs.iter().all(|q| st.path_dist(q.0, q.1) > road + 1.7) && gmin > 0.8 {
                let fl = 2.0 + (st.rnd() * 2.4).floor(); let y1 = st.r.roadh[k] + fl * 3.0 + 0.4; let y0 = gmin.min(st.r.roadh[k]) - 1.6; let c = corner(0.0, front + d / 2.0);
                let pc = pal[(st.rnd() * pal.len() as f64) as usize]; house(st, c.0, c.1, tn[0].atan2(tn[1]), w, d, y0, y1, pc);
                let mut a = -w / 2.0; while a <= w / 2.0 + 0.01 { let q = corner(a, front + 0.55); st.obstacles.push([q.0, q.1, 0.6, 0.0]); a += 1.5; }
                if !tower && sd < 0.0 && i > (i0 + i1) / 2.0 - 20.0 { tower = true; let tc = corner(0.0, front + d + 3.2); house(st, tc.0, tc.1, tn[0].atan2(tn[1]), 4.6, 4.6, y0, st.r.roadh[k] + 19.0, [0.93, 0.74, 0.5]); } }
            i += w + gap; } }
    { let (mut pm, mut am, mut wm) = (vec![], vec![], vec![]); let mut post = box_geo(0.13, 4.4, 0.13); post.translate(0.0, 2.2, 0.0); let mut arm = box_geo(0.08, 0.08, 1.0); arm.translate(0.0, 4.3, 0.45); let mut lamp = box_geo(0.34, 0.3, 0.34); lamp.translate(0.0, 4.1, 0.9);
        let (post, arm, lamp) = (st.geo(post), st.geo(arm), st.geo(lamp));
        let mut i = i0 as usize + 6; let mut n = 0; while (i as f64) < i1 { let sd = if n % 2 == 1 { 1.0 } else { -1.0 }; let k = i % np; let tn = st.t(k); let q = st.side(k, (road + 1.5) * sd);
            o.pos(q.0, st.r.roadh[k] - 0.05, q.1).rot(0.0, (tn[1] * sd).atan2(-tn[0] * sd), 0.0).scale(1.0, 1.0, 1.0); pm.push(o.matrix()); am.push(o.matrix()); wm.push(o.matrix()); st.obstacles.push([q.0, q.1, 0.18, 1.0]);
            i += 26; n += 1; }
        let dark = st.mat(Mat::lambert().hex(0x24302a)); st.instanced_dd(post, dark, &pm, &whites(pm.len()), 300.0); st.instanced_dd(arm, dark, &am, &whites(am.len()), 300.0);
        let lm = st.mat(Mat::basic().hex(0xffd890)); st.instanced_dd(lamp, lm, &wm, &whites(wm.len()), 300.0); }
    // villas with pools
    { let pool_m = st.mat(Mat::lambert().hex(0x46d6dc).emis(0x0a3a40)); let pool_g = st.geo(box_geo(1.0, 0.3, 1.0)); let (mut pom, mut poc) = (vec![], vec![]); let mut vil2: Vec<(f64, f64)> = vec![];
        let mut tries = 0; while tries < 6000 && vil2.len() < 30 { tries += 1;
            let k = (st.rnd() * np as f64) as usize; let tn = st.t(k); let sd = if st.drop[k] != 0 { if st.rnd() < 0.8 { -st.drop[k] as f64 } else { st.drop[k] as f64 } } else { -1.0 }; let off = (road + st.rr(19.0, 46.0)) * sd; let (x, z) = st.side(k, off);
            if st.r.tun_at(k).is_some() || st.r.in_village(k) || st.road_dist(x, z) < road + 15.0 || vil2.iter().any(|v| hyp(v.0 - x, v.1 - z) < 34.0) { continue; }
            let w = st.rr(9.0, 13.0); let d = st.rr(8.0, 11.0); let hs: Vec<f64> = [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)].iter().map(|(a, b)| st.height(x + a * w * 0.6, z + b * d * 0.6)).collect();
            let hmin = hs.iter().cloned().fold(f64::INFINITY, f64::min); let hmax = hs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            if hmin < 6.0 || hmax - hmin > 5.0 { continue; }
            let (ci, cj) = ((x / 4.0).floor() as i64, (z / 4.0).floor() as i64); let mut clear = true;
            'o: for a in -3..=3 { for b in -3..=3 { if let Some(l) = st.th.get(&((ci + a) * 4096 + cj + b)) { for &t in l { let t = st.trees[t]; if hyp(t.x - x, t.z - z) < 9.0 { clear = false; break 'o; } } } } }
            if !clear { continue; }
            let yaw = tn[0].atan2(tn[1]) + if st.rnd() < 0.5 { 0.0 } else { PI / 2.0 }; let fl = if st.rnd() < 0.6 { 2.0 } else { 1.0 };
            let pc = pal[(st.rnd() * pal.len() as f64) as usize]; house(st, x, z, yaw, w, d, hmin - 2.0, hmax + fl * 3.1 + 0.3, pc); vil2.push((x, z));
            for (a, b) in [(0.0, 0.0), (-0.35, 0.0), (0.35, 0.0), (0.0, -0.35), (0.0, 0.35)] { st.obstacles.push([x + a * w, z + b * d, w.min(d) * 0.42, 0.0]); }
            let (px, pz) = (x + tn[1] * sd * (d * 0.5 + 3.2), z - tn[0] * sd * (d * 0.5 + 3.2));
            if (st.height(px, pz) - hmax).abs() < 3.0 && st.road_dist(px, pz) > road + 6.0 { o.pos(px, st.height(px, pz) + 0.05, pz).rot(0.0, yaw, 0.0).scale(4.2, 1.0, 7.0); push_t(&mut pom, &mut poc, &o, WHITE); } }
        st.instanced_dd(pool_g, pool_m, &pom, &poc, 500.0); }
    // palms
    { let frond_t = st.canvas_tex(64, 32, false, |_, g| { let (s, h) = (64.0, 32.0); g.stroke_style("rgb(150,170,110)"); g.line_width = 2.0; g.begin_path(); g.move_to(2.0, h / 2.0); g.line_to(s - 2.0, h / 2.0); g.stroke();
            let mut x = 4i64; while x < 61 { let l = (1.0 - x as f64 / s) * h * 0.42 + 2.0; let c = (150 + (x * 7) % 60) as f64; g.stroke_c(canvas::rgbi(c * 0.55, c, c * 0.45)); g.line_width = 1.4;
                g.begin_path(); g.move_to(x as f64, h / 2.0); g.line_to(x as f64 + 5.0, h / 2.0 - l); g.stroke(); g.begin_path(); g.move_to(x as f64, h / 2.0); g.line_to(x as f64 + 5.0, h / 2.0 + l); g.stroke(); x += 2; } });
        let mut frond = plane(1.0, 0.36, 1, 1); frond.translate(0.5, 0.0, 0.0); let frond = st.geo(frond);
        let (mut trm, mut trc, mut frm, mut frc) = (vec![], vec![], vec![], vec![]); let mut spots: Vec<(usize, f64)> = vec![];
        let (vp, vn) = st.r.vil.unwrap(); for e in [vp + 14, vp + vn - 12] { for sd in [-1.0, 1.0] { spots.push((e % np, (road + 1.9) * sd)); } }
        let mut n = 0; while n < 220 && spots.len() < 34 { n += 1; let k = (st.rnd() * np as f64) as usize; if st.drop[k] == 0 || st.r.tun_at(k).is_some() || st.r.in_village(k) || st.r.vcurve[k] < 15.0 { continue; }
            let mut q = 0; loop { let cnt = 2 + (st.rnd() * 2.0) as usize; if q >= cnt { break; } let off = (road + st.rr(2.6, 4.2)) * st.drop[k] as f64; spots.push(((k + q * 7) % np, off)); q += 1; } }
        for (k, off) in spots { let (x, z) = st.side(k, off); let y = st.height(x, z); if y < 1.5 || st.path_dist(x, z) < road + 1.4 { continue; }
            let h = st.rr(7.0, 11.0); let lean = st.rr(-0.12, 0.12); let la = st.rnd() * 6.28; st.obstacles.push([x, z, 0.3, 2.0]);
            o.pos(x, y - 0.2, z).rot(la.cos() * lean, 0.0, la.sin() * lean).scale(0.2, h, 0.2); push_t(&mut trm, &mut trc, &o, rgb(0.95, 0.85, 0.72));
            let (tx, tz, ty) = (x - (la.sin() * lean).sin() * h, z + (la.cos() * lean).sin() * h, y - 0.2 + h * lean.cos());
            for f in 0..10 { let a = f as f64 / 10.0 * 6.28 + st.rr(-0.2, 0.2); let len = st.rr(3.2, 4.4);
                o.pos(tx, ty, tz).rot(0.0, a, 0.0); let rz = -st.rr(0.25, 0.75); o.rotate_z(rz); let rx = st.rr(-0.3, 0.3); o.rotate_x(rx); o.scalar(len);
                let (hh, ss, ll) = (st.rr(0.22, 0.28), st.rr(0.35, 0.45), st.rr(0.3, 0.38)); push_t(&mut frm, &mut frc, &o, Col::hsl(hh, ss, ll).mul(1.5)); } }
        let m = st.mat(Mat::lambert().map(st.t_bark)); st.instanced_dd(st.unit_cyl, m, &trm, &trc, 600.0);
        let m = st.mat(Mat::basic().map(frond_t).at(0.4).double().sway(0.12, false, None)); st.instanced_dd(frond, m, &frm, &frc, 600.0); }
    // bougainvillea
    { let (mut bgm, mut bgc) = (vec![], vec![]);
        let bloom = |st: &mut St, bgm: &mut Vec<M4>, bgc: &mut Vec<Col>, x: f64, y: f64, z: f64, n: usize, sz: f64| { let mut o = Tmp::default(); for _ in 0..n {
            let (dx, dy, dz) = (st.rr(-1.0, 1.0), st.rr(-0.6, 0.6), st.rr(-1.0, 1.0)); o.pos(x + dx * sz, y + dy * sz, z + dz * sz); let (a, b, c) = (st.rnd() * 3.0, st.rnd() * 3.0, st.rnd() * 3.0); o.rot(a, b, c); let sc = st.rr(1.0, 1.7) * sz; o.scalar(sc);
            let col = if st.rnd() < 0.78 { let (r, g, b) = (st.rr(0.95, 1.1), st.rr(0.18, 0.32), st.rr(0.55, 0.75)); rgb(r, g, b).mul(1.2) } else { Col::hsl(0.28, 0.45, 0.28).mul(1.4) }; push_t(bgm, bgc, &o, col); } };
        let mut i = 0; while i < np { let sd = st.drop[i]; if !(sd == 0 || st.r.tun_at(i).is_some() || st.r.in_village(i) || hash2(i as f64, 77.0) > 0.09) {
            let (x, z) = st.side(i, (road + 1.3) * sd as f64); let y = st.r.roadh[i] + 0.7; bloom(st, &mut bgm, &mut bgc, x, y, z, 9, 0.9); } i += 2; }
        let (vp, vn) = st.r.vil.unwrap(); let mut i = vp + 26; while i + 20 < vp + vn { for sd in [-1.0, 1.0] { if hash2(i as f64, sd + 9.0) > 0.45 { continue; }
            let (x, z) = st.side(i % np, (road + 2.6) * sd); let y = st.r.roadh[i % np] + st.rr(2.5, 4.5); bloom(st, &mut bgm, &mut bgc, x, y, z, 10, 0.8); } i += 9; }
        let m = st.mat(Mat::basic().map(st.t_leaf).at(0.5).double().sway(0.05, false, None)); st.instanced_dd(st.card, m, &bgm, &bgc, 420.0); }
    // the lighthouse
    { let k = st.r.tun[0].m; let mut best = None; let mut of = 26.0; while of <= 110.0 { let (x, z) = st.side(k, of); if st.height(x, z) < 4.0 { break; } let (x2, z2) = st.side(k, of - 7.0); best = Some((x2, z2, st.height(x2, z2))); of += 2.0; }
        if let Some((x, z, y)) = best { let white = st.mat(Mat::lambert().hex(0xf2eee4)); let red = st.mat(Mat::lambert().hex(0xc8352a)); let lampm = st.mat(Mat::basic().hex(0xffe9a8));
            for (g, yy, m) in [(cylinder(1.5, 2.1, 13.0, 12, 1, false), 6.5, white), (cylinder(1.52, 1.62, 2.2, 12, 1, false), 9.6, red), (cylinder(2.2, 2.2, 0.35, 12, 1, false), 13.2, red),
                (cylinder(1.1, 1.1, 1.7, 10, 1, false), 14.2, lampm), (cone(1.4, 1.3, 10, 1, false), 15.7, red), (box_geo(3.6, 2.4, 3.6), 1.2, white)] { let ob = st.mesh(g, m); st.sc.set_pos(ob, x, y - 0.6 + yy, z); st.add(ob); }
            st.obstacles.push([x, z, 2.3, 0.0]); } }
    // boats
    { let mut hull = box_seg(2.4, 1.0, 8.0, 1, 1, 4); for i in 0..hull.count() { let mut v = hull.p(i); if v.z > 1.5 { v.x *= (0.08f64).max(1.0 - (v.z - 1.5) / 2.6); } if v.y < 0.0 { v.x *= 0.75; } hull.set_p(i, v); } hull.compute_normals();
        let mut sail = Geo::new([0.0, 1.2, -2.6, 0.0, 11.5, -0.6, 0.0, 1.2, 1.4, 0.0, 1.4, 1.7, 0.0, 9.4, -0.3, 0.0, 1.4, 3.8].iter().map(|&v| v as f32).collect()); sail.compute_normals();
        let cab = box_geo(2.2, 1.4, 5.2); let mut wake = plane(7.0, 30.0, 1, 1); wake.rotate_x(-PI / 2.0); wake.translate(0.0, 0.0, -17.0);
        let wake_t = st.canvas_tex(32, 64, false, |_, g| { let (s, h) = (32usize, 64usize); let mut d = vec![0u8; s * h * 4];
            for y in 0..h { for x in 0..s { let u = (x as f64 / (s - 1) as f64 * 2.0 - 1.0).abs(); let v = y as f64 / (h - 1) as f64;
                let edge = (-((u - (1.0 - v) * 0.85) * 6.0).powi(2)).exp() + (-u * u * 40.0).exp() * (1.0 - v) * 0.8; let a = edge.min(1.0) * (1.0 - v).powf(0.5) * (0.5 + 0.5 * hash2(x as f64, y as f64));
                let q = (y * s + x) * 4; d[q] = 255; d[q + 1] = 255; d[q + 2] = 255; d[q + 3] = (a * 200.0) as u8; } } g.put(&d); });
        st.sc.texs[wake_t].mag = LINEAR;
        let mut tries = 0; while tries < 3000 && st.boats.len() < 18 { tries += 1;
            let a = st.rnd() * 6.28; let r = st.rr(360.0, 1150.0); let (x, z) = (a.cos() * r, a.sin() * r);
            if st.far_h(x, z) > -6.0 || st.far_h(x + 20.0, z) > -6.0 || st.far_h(x - 20.0, z) > -6.0 || st.far_h(x, z + 20.0) > -6.0 || st.far_h(x, z - 20.0) > -6.0 || st.road_dist(x, z) < 110.0 || st.boats.iter().any(|b| hyp(b.x - x, b.z - z) < 60.0) { continue; }
            let sl = st.rnd() < 0.62; let h = st.rnd() * 6.28; let v = if sl { st.rr(1.2, 2.6) } else { st.rr(2.5, 5.0) }; let ph = st.rnd() * 6.28; let sc = if sl { st.rr(0.9, 1.2) } else { st.rr(1.3, 1.8) };
            st.boats.push(Boat { x, z, h, v, sail: sl, ph, sc }); }
        let n = st.boats.len(); let ns = st.boats.iter().filter(|b| b.sail).count(); let nm = n - ns;
        let mk = |st: &mut St, g: Geo, m: Mat, cnt: usize| { let g = st.geo(g); let m = st.mat(m); let im = st.sc.inst(g, m, cnt.max(1)); st.sc.objs[im].count = cnt; st.sc.objs[im].culled = false; st.sc.objs[im].dyn_ = true; st.add(im); im };
        let h_ = mk(st, hull, Mat::lambert().hex(0xf4f2ec), n); let s_ = mk(st, sail, Mat::lambert().hex(0xfff6e6).double(), ns); let c_ = mk(st, cab, Mat::lambert().hex(0xe8e8ea), nm);
        let w_ = mk(st, wake, Mat::basic().map(wake_t).transparent().no_dw().opacity(0.75), nm);
        st.boat_ims = Some([h_, s_, c_, w_]); }
}
