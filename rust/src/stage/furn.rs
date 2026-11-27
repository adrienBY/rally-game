//! Rally furniture and hazards: the launch ramp, Ember Peak's lava, bridges and fireball pools, the tile-fall pool, the prism's
//! kerbs, gates and crystals, the start gantry, chevrons, bales, signs and marker posts, and the grass field.
use super::canvas::{self, Ctx};
use super::three::*;
use super::{hyp, push_t, whites, St};
use crate::noise::{fbm, hash2};
use std::f64::consts::PI;

pub fn ramp(st: &mut St) {
    if st.r.ramp_i < 0 { return; }
    let plank = st.canvas_tex(32, 32, true, |st, g| { let s = 32.0; g.fill_style("rgb(150,108,66)"); g.fill_rect(0.0, 0.0, s, s);
        let mut y = 0i64; while y < 32 { g.fill_style("rgba(60,38,20,0.75)"); g.fill_rect(0.0, y as f64, s, 1.0); g.fill_c(canvas::rgba((190 + (y * 7) % 40) as f64, (140 + (y * 5) % 30) as f64, 90.0, 0.5)); g.fill_rect(0.0, (y + 1) as f64, s, 1.0); y += 4; }
        for _ in 0..40 { g.fill_style("rgba(70,45,25,0.5)"); let x = st.rr(0.0, s) as i64 as f64; let y = st.rr(0.0, s) as i64 as f64; g.fill_rect(x, y, 1.0, 1.0); }
        g.fill_style("rgb(40,40,40)"); let mut y = 1.0; while y < s { g.fill_rect(3.0, y + 1.0, 1.0, 1.0); g.fill_rect(s - 4.0, y + 1.0, 1.0, 1.0); y += 8.0; } });
    let hazard = st.canvas_tex(32, 32, true, |_, g| { let s = 32.0; let mut x = -s; while x < s * 2.0 { g.fill_style("#f2c230"); g.begin_path(); g.move_to(x, 0.0); g.line_to(x + 4.0, 0.0); g.line_to(x + 4.0 - s, s); g.line_to(x - s, s); g.fill(); x += 8.0; }
        g.dest_over = true; g.fill_style("#161616"); g.fill_rect(0.0, 0.0, s, s); g.dest_over = false; });
    let (ri, rl, rh) = (st.r.ramp_i, st.r.ramp_l, st.r.ramp_h); let half = st.road() - 0.15;
    let at = |st: &St, s: usize, side: f64, y: Option<f64>| -> [f64; 3] { let k = st.r.wrap(ri - rl as i64 + s as i64); let (x, z) = st.side(k, half * side); [x, y.unwrap_or(st.height(x, z) + 0.12), z] };
    let base_y = |st: &St, s: usize, side: f64| at(st, s, side, None)[1] - rh * s as f64 / rl as f64 - 0.12;
    let (mut p, mut uv, mut ph, mut uvh, mut idx, mut idh): (Vec<f32>, Vec<f32>, Vec<f32>, Vec<f32>, Vec<u32>, Vec<u32>) = Default::default();
    for s in 0..=rl { for q in [at(st, s, -1.0, None), at(st, s, 1.0, None)] { p.extend(q.iter().map(|&v| v as f32)); } uv.extend_from_slice(&[0.0, (s as f64 / 2.0) as f32, 1.0, (s as f64 / 2.0) as f32]); }
    for s in 0..rl as u32 { let a = s * 2; idx.extend_from_slice(&[a, a + 1, a + 2, a + 1, a + 3, a + 2]); }
    let mut quad = |a: [f64; 3], b: [f64; 3], c: [f64; 3], d: [f64; 3], uvs: [f64; 8]| { let o = (ph.len() / 3) as u32; for q in [a, b, c, d] { ph.extend(q.iter().map(|&v| v as f32)); } uvh.extend(uvs.iter().map(|&v| v as f32)); idh.extend_from_slice(&[o, o + 1, o + 2, o, o + 2, o + 3]); };
    for side in [-1.0, 1.0] { for s in 0..rl { let (t0, t1) = (at(st, s, side, None), at(st, s + 1, side, None)); let b0 = [t0[0], base_y(st, s, side), t0[2]]; let b1 = [t1[0], base_y(st, s + 1, side), t1[2]];
        quad(b0, b1, t1, t0, [s as f64 / 4.0, 0.0, (s + 1) as f64 / 4.0, 0.0, (s + 1) as f64 / 4.0, (t1[1] - b1[1]) / 1.4, s as f64 / 4.0, (t0[1] - b0[1]) / 1.4]); } }
    { let (tl, tr) = (at(st, rl, -1.0, None), at(st, rl, 1.0, None)); let bl = [tl[0], base_y(st, rl, -1.0), tl[2]]; let br = [tr[0], base_y(st, rl, 1.0), tr[2]]; quad(bl, br, tr, tl, [0.0, 0.0, 3.0, 0.0, 3.0, 0.7, 0.0, 0.7]); }
    for (pp, uu, ii, tex) in [(p, uv, idx, plank), (ph, uvh, idh, hazard)] { let mut g = Geo::new(pp).with_uv(uu).with_index(ii); g.compute_normals();
        let m = st.mat(Mat::lambert().map(tex).double().po(-3.0, -8.0)); let o = st.mesh(g, m); st.add(o); }
    for side in [-1.0, 1.0] { let q = at(st, rl, side * 1.25, None); let m = st.mat(Mat::lambert().hex(0xeeeeee)); let pole = st.mesh(box_geo(0.1, 2.6, 0.1), m);
        let gy = st.height(q[0], q[2]); st.sc.set_pos(pole, q[0], gy + 1.3, q[2]); st.add(pole); st.obstacles.push([q[0], q[2], 0.15, 0.0]);
        let m = st.mat(Mat::lambert().hex(if side < 0.0 { 0xd8342a } else { 0xf2c230 }).double()); let flag = st.mesh(plane(0.9, 0.55, 1, 1), m);
        let tn = st.t(ri as usize); st.sc.set_pos(flag, q[0] + tn[0] * 0.45, gy + 2.3, q[2] + tn[1] * 0.45); st.sc.set_rot(flag, 0.0, tn[0].atan2(tn[1]) + PI / 2.0, 0.0); st.add(flag); }
}

pub fn volcano(st: &mut St) {
    let lava = st.canvas_tex(64, 64, true, |st, g| { let s = 64.0; g.fill_style("#ff5a12"); g.fill_rect(0.0, 0.0, s, s);
        for _ in 0..90 { g.fill_style(if st.rnd() < 0.5 { "#ffb22e" } else { "#ff7a1c" }); let (x, y, w, h) = (st.rr(0.0, s), st.rr(0.0, s), st.rr(2.0, 9.0), st.rr(1.0, 3.0)); g.fill_rect(x, y, w, h); }
        for _ in 0..26 { g.fill_style(if st.rnd() < 0.6 { "#5a1408" } else { "#2a0a06" }); let (x, y, w, h) = (st.rr(0.0, s), st.rr(0.0, s), st.rr(3.0, 10.0), st.rr(2.0, 6.0)); g.fill_rect(x, y, w, h); g.fill_style("#ffe066"); g.fill_rect(x - 1.0, y + h, w + 2.0, 1.0); }
        for _ in 0..30 { g.fill_style("#fff2a0"); let (x, y) = (st.rr(0.0, s), st.rr(0.0, s)); g.fill_rect(x, y, 1.0, 1.0); } });
    let lava_m = st.mat(Mat::basic().map(lava).vc().double().po(-2.0, -2.0)); let np = st.np(); let road = st.road();
    for j in 0..5 { let jf = j as f64; let th0 = jf * PI * 2.0 / 5.0 + 0.45; let (mut pts, mut hw) = (Vec::new(), Vec::new());
        let mut r = 92.0; while r <= 560.0 { let th = th0 + 0.12 * (r * 0.02 + jf).sin() + 0.05 * (r * 0.07 + 2.0 * jf).sin(); pts.push(r * th.cos()); pts.push(r * th.sin()); hw.push(4.6 + 1.4 * (r * 0.03 + jf).sin()); r += 1.5; }
        let (mut kc, mut best) = (0usize, 1e9); for q in 0..pts.len() / 2 { let d = st.path_dist(pts[q * 2], pts[q * 2 + 1]); if d < best { best = d; kc = st.path_idx(pts[q * 2], pts[q * 2 + 1]); } }
        let mut turn = 0.0; for d in -30i64..=30 { let k = st.r.wrap(kc as i64 + d); let ta = st.t((k + np - 5) % np); let tb = st.t((k + 5) % np); let t = st.t(k); turn += (tb[0] - ta[0]) * -t[1] + (tb[1] - ta[1]) * t[0]; }
        let sg = if turn >= 0.0 { 1.0 } else { -1.0 };
        let n = pts.len() / 2; let a = 6usize; let (mut p, mut uv, mut c, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new()); let mut v = 0.0;
        for q in 0..n { let (ia, ib) = (q.saturating_sub(1), (n - 1).min(q + 1)); let (dx, dz) = (pts[ib * 2] - pts[ia * 2], pts[ib * 2 + 1] - pts[ia * 2 + 1]); let l = hyp(dx, dz); let l = if l == 0.0 { 1.0 } else { l }; let (nx, nz) = (-dz / l, dx / l);
            for cc in 0..=a { let o = (cc as f64 / a as f64 * 2.0 - 1.0) * hw[q]; let (x, z) = (pts[q * 2] + nx * o, pts[q * 2 + 1] + nz * o);
                p.extend_from_slice(&[x as f32, (st.height(x, z) + 0.22) as f32, z as f32]); uv.extend_from_slice(&[(cc as f64 / a as f64 * 1.6) as f32, v as f32]); c.extend_from_slice(&[1.0f32, 1.0, 1.0]); }
            v = q as f64 * 1.5 / 10.0; for cc in 0..=a { uv[(q * (a + 1) + cc) * 2 + 1] = v as f32; } }
        for q in 0..n - 1 { for cc in 0..a { let aa = (q * (a + 1) + cc) as u32; let b = aa + a as u32 + 1; idx.extend_from_slice(&[aa, b, aa + 1, aa + 1, b, b + 1]); } }
        st.volc_rivers.push(super::VolcRiver { pts, hw, kc, sg });
        let o = st.mesh(Geo::new(p).with_uv(uv).with_col(c).with_index(idx), lava_m); st.sc.objs[o].culled = false; st.sc.objs[o].dyn_ = true; st.add(o);
    }
    let _ = road;
    // bridges
    { let deck_h = |st: &St, k: usize| st.r.roadh[k] - 0.12; let bhw = 2.3;
        let (mut tp, mut tu, mut tc, mut ti) = (Vec::new(), Vec::new(), Vec::new(), Vec::new()); let mut tv = 0u32;
        for &(k0, k1) in &st.r.bridges.clone() { let l = (k1 + np - k0) % np;
            for q in 0..=l + 1 { let k = (k0 + q) % np; for sd in [-1.0, 1.0] { let (x, z) = st.side(k, sd * bhw); tp.extend_from_slice(&[x as f32, (deck_h(st, k) + 0.035) as f32, z as f32]); tu.extend_from_slice(&[((sd + 1.0) / 2.0 * 0.6 + 0.2) as f32, (k as f64 / 7.0) as f32]); let sh = (0.95 + 0.05 * sd) as f32; tc.extend_from_slice(&[sh, sh, sh]); }
                if q > 0 { let a = tv + (q as u32 - 1) * 2; ti.extend_from_slice(&[a, a + 2, a + 1, a + 1, a + 2, a + 3]); } }
            tv += (l as u32 + 2) * 2; }
        if !tp.is_empty() { let mut g = Geo::new(tp).with_uv(tu).with_col(tc).with_index(ti); g.compute_normals(); let m = st.mat(Mat::lambert().map(st.t_gravel).vc().double().po(-1.0, -2.0)); let o = st.mesh(g, m); st.sc.objs[o].culled = false; st.add(o); }
        let mut bx = box_geo(1.0, 1.0, 1.0).to_non_indexed(); { let n = bx.count(); let mut c = Vec::with_capacity(n * 3); for i in 0..n { let y = bx.pos[i * 3 + 1] as f64 + 0.5; let k = (0.55 + 0.45 * y) as f32; c.extend_from_slice(&[k, k, k]); } bx.col = Some(c); }
        let bxg = st.geo(bx); let (mut sm_, mut sc_, mut gm, mut gc, mut lm, mut lc) = (vec![], vec![], vec![], vec![], vec![], vec![]); let mut o = Tmp::default();
        let mut stone = |sm_: &mut Vec<M4>, sc_: &mut Vec<Col>, x: f64, y: f64, z: f64, rot: f64, sx: f64, sy: f64, sz: f64, c: [f64; 3]| { let mut t = Tmp::default(); t.pos(x, y, z).rot(0.0, rot, 0.0).scale(sx, sy, sz); push_t(sm_, sc_, &t, rgb(c[0], c[1], c[2])); };
        let (bas, kerb) = ([0.2, 0.15, 0.14], [0.34, 0.27, 0.24]);
        for &(k0, k1) in &st.r.bridges.clone() { let l = (k1 + np - k0) % np;
            for q in 0..=l { let k = (k0 + q) % np; let n = (k + 1) % np; let (p, pn) = (st.p(k), st.p(n)); let (cx, cz) = ((p[0] + pn[0]) / 2.0, (p[1] + pn[1]) / 2.0); let t = st.t(k); let rot = t[0].atan2(t[1]); let y = (deck_h(st, k) + deck_h(st, n)) / 2.0;
                stone(&mut sm_, &mut sc_, cx, y - 0.4, cz, rot, bhw * 2.0 + 0.2, 0.8, 1.06, bas);
                if q % 3 != 2 { for sd in [-1.0, 1.0] { let of = sd * (bhw + 0.16); stone(&mut sm_, &mut sc_, cx - t[1] * of, y + 0.06, cz + t[0] * of, rot, 0.3, 0.32, 1.02, kerb); } } }
            let mut q = 4; while q + 4 <= l { let k = (k0 + q) % np; let p = st.p(k); let f = st.height(p[0], p[1]); let top = deck_h(st, k) - 0.8; if top - f >= 4.0 {
                let rot = st.t(k)[0].atan2(st.t(k)[1]); stone(&mut sm_, &mut sc_, p[0], (top + f) / 2.0 - 0.5, p[1], rot, 1.5, top - f + 1.0, 1.5, [0.26, 0.17, 0.14]);
                o.pos(p[0], f + 0.6, p[1]).rot(0.0, rot, 0.0).scale(1.75, 1.2, 1.75); push_t(&mut gm, &mut gc, &o, rgb(1.0, 0.42, 0.12)); } q += 8; }
            for (k, dir) in [(k0, -1.0), (k1, 1.0)] { let p = st.p(k); let t = st.t(k); let rot = t[0].atan2(t[1]);
                stone(&mut sm_, &mut sc_, p[0] + t[0] * dir * 0.5, deck_h(st, k) - 2.2, p[1] + t[1] * dir * 0.5, rot, bhw * 2.0 + 1.6, 3.6, 3.0, bas);
                for sd in [-1.0, 1.0] { let of = sd * (bhw + 0.5); let (x, z) = (p[0] - t[1] * of, p[1] + t[0] * of); stone(&mut sm_, &mut sc_, x, deck_h(st, k) + 0.65, z, rot, 0.34, 1.5, 0.34, [0.18, 0.15, 0.14]);
                    o.pos(x, deck_h(st, k) + 1.5, z).rot(0.0, rot, 0.0).scale(0.3, 0.3, 0.3); push_t(&mut lm, &mut lc, &o, rgb(1.0, 0.62, 0.22)); } } }
        if !sm_.is_empty() { let m = st.mat(Mat::lambert().vc()); st.instanced_d(bxg, m, &sm_, &sc_); }
        if !gm.is_empty() { let m = st.mat(Mat::basic().vc()); st.instanced_d(bxg, m, &gm, &gc); }
        if !lm.is_empty() { let g = st.geo(box_geo(1.0, 1.0, 1.0)); let m = st.mat(Mat::basic()); st.instanced_d(g, m, &lm, &lc); } }
    // the crater lake and its glow
    { let y = st.height(0.0, 0.0) + 4.0; let m = st.mat(Mat::basic().map(lava).double()); let lake = st.mesh(circle(52.0, 40), m); st.sc.set_rot(lake, -PI / 2.0, 0.0, 0.0); st.sc.set_pos(lake, 0.0, y, 0.0); st.add(lake);
        st.volc_crater = [0.0, y, 0.0];
        for (h, r, k) in [(18.0, 46.0, 1.0), (55.0, 34.0, 0.7), (100.0, 24.0, 0.45)] { let m = st.mat(Mat::basic().col(Col::hex(0x5a1a06).mul(k)).transparent().no_dw().blend(ADDITIVE).no_fog().blend2()); let o = st.mesh(icosahedron(r, 2), m); st.sc.set_pos(o, 0.0, y + h, 0.0); st.add(o); } }
    // fireball pools
    let mk = |st: &mut St, g: Geo, m: MatId, n: usize| { let g = st.geo(g); let im = st.sc.inst(g, m, n); let mut t = Tmp::default(); t.pos(0.0, -999.0, 0.0).scalar(0.001); let mm = t.matrix();
        for i in 0..n { st.sc.set_matrix_at(im, i, &mm); st.sc.set_color_at(im, i, rgb(0.0, 0.0, 0.0)); } st.sc.objs[im].culled = false; st.sc.objs[im].dyn_ = true; st.add(im); im };
    let add = |st: &mut St, vc: bool| { let mut m = Mat::basic().transparent().no_dw().blend(ADDITIVE).double().po(-3.0, -3.0).blend2(); m.vc = vc; st.mat(m) };
    let facet = |st: &mut St, lo: f64, hi: f64| { let mut g = icosahedron(1.0, 0).to_non_indexed(); g.compute_normals(); let n = g.count(); let mut c = Vec::new();
        let mut f = 0; while f < n { let ny = (g.n(f).y + g.n(f + 1).y + g.n(f + 2).y) / 3.0; let k = ((lo + (hi - lo) * (0.5 + 0.5 * ny)) * (0.92 + 0.08 * st.rnd())) as f32; for _ in 0..9 { c.push(k); } f += 3; }
        g.col = Some(c); g };
    let mut solid = icosahedron(1.0, 0).to_non_indexed(); solid.compute_normals();
    let mut ring_g = ring(0.86, 1.0, 40, 1); ring_g.rotate_x(-PI / 2.0);
    let mut disc = circle(1.0, 28); disc.rotate_x(-PI / 2.0); { let n = disc.count(); let mut c = Vec::new(); for i in 0..n { let k = (1.0 - 0.78 * hyp(disc.pos[i * 3] as f64, disc.pos[i * 3 + 2] as f64)) as f32; c.extend_from_slice(&[k, k, k]); } disc.col = Some(c); }
    let fg = facet(st, 0.6, 1.0); let m1 = st.mat(Mat::basic().vc()); let fire = mk(st, fg, m1, 1200);
    let m2 = st.mat(Mat::lambert()); let sol = mk(st, solid, m2, 400);
    let m3 = add(st, false); let glow = mk(st, icosahedron(1.0, 1), m3, 128);
    let m4 = add(st, false); let rg = mk(st, ring_g, m4, 64);
    let m5 = add(st, true); let dg = mk(st, disc, m5, 200);
    st.volc_ims = Some([fire, sol, glow, rg, dg]);
}

pub fn tilefall(st: &mut St) {
    let tex = st.canvas_tex(16, 16, false, |_, g| { let s = 16.0; g.fill_style("#2a2a2a"); g.fill_rect(0.0, 0.0, s, s); g.fill_style("#d8d8d8"); g.fill_rect(1.0, 1.0, s - 2.0, s - 2.0);
        g.fill_style("#f4f4f4"); g.fill_rect(2.0, 2.0, s - 4.0, 2.0); g.fill_rect(2.0, 2.0, 2.0, s - 4.0); g.fill_style("#a8a8a8"); g.fill_rect(2.0, s - 4.0, s - 4.0, 2.0); g.fill_rect(s - 4.0, 2.0, 2.0, s - 4.0);
        let fr = |v: f64| ((v % 1.0) + 1.0) % 1.0;
        for i in 0..9 { let i = i as f64; let x = (4.0 + fr((i * 12.9898).sin() * 43758.5453) * 8.0) as i64; let y = (4.0 + fr((i * 78.233).sin() * 43758.5453) * 8.0) as i64; g.px(x, y, canvas::parse("#c4c4c4")); } });
    let mut geo = box_geo(1.0, 1.0, 1.0).to_non_indexed(); geo.compute_normals(); { let n = geo.count(); let mut c = Vec::new(); for i in 0..n { let y = geo.n(i).y; let k = if y > 0.5 { 1.0 } else if y < -0.5 { 0.3 } else { 0.62 }; c.extend_from_slice(&[k, k, k]); } geo.col = Some(c); }
    let g = st.geo(geo); let m = st.mat(Mat::lambert().map(tex).vc()); let im = st.sc.inst(g, m, 144); let mut t = Tmp::default(); t.pos(0.0, -999.0, 0.0).scalar(0.001); let mm = t.matrix();
    for i in 0..144 { st.sc.set_matrix_at(im, i, &mm); st.sc.set_color_at(im, i, WHITE); }
    st.sc.objs[im].count = 0; st.sc.objs[im].culled = false; st.sc.objs[im].dyn_ = true; st.add(im); st.tilefall = im;
}

pub fn prism(st: &mut St) {
    let hue = |h: f64, k: f64| { let c = Col::hsl(((h % 1.0) + 1.0) % 1.0, 0.95, 0.58); [c.r * k, c.g * k, c.b * k] };
    let at = |st: &St, i: usize, lat: f64, up: f64| { let (x, z) = st.side(i, lat); [x, st.r.roadh[i] - 0.12 + lat * st.r.bank[i] + up, z] };
    let np = st.np(); let road = st.road();
    { let e = 1.08 * road; let (mut p, mut c, mut idx, mut gp, mut gc) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for i in 0..=np { let k = i % np; let h = i as f64 / 180.0;
            for sd in [-1.0, 1.0] { let a = at(st, k, sd * e, 0.045); let b = at(st, k, sd * (e + 0.5), 0.045); let w = hue(h + if sd > 0.0 { 0.5 } else { 0.0 }, 1.25);
                p.extend(a.iter().chain(b.iter()).map(|&v| v as f32)); c.extend([1.0, 1.0, 1.0, w[0], w[1], w[2]].iter().map(|&v| v as f32));
                gp.extend([b[0], b[1] - 0.02, b[2], b[0], b[1] - 1.7, b[2]].iter().map(|&v| v as f32)); let g2 = hue(h + if sd > 0.0 { 0.5 } else { 0.0 }, 0.9); gc.extend([g2[0], g2[1], g2[2], 0.0, 0.0, 0.0].iter().map(|&v| v as f32)); } }
        for i in 0..np as u32 { for sd in 0..2 { let a = (i * 2 + sd) * 2; let b = ((i + 1) * 2 + sd) * 2; idx.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]); } }
        let m = st.mat(Mat::basic().vc().double().po(-2.0, -3.0)); let o = st.mesh(Geo::new(p).with_col(c).with_index(idx.clone()), m); st.sc.objs[o].culled = false; st.sc.objs[o].dyn_ = true; st.sc.objs[o].wave = true; st.add(o);
        let m = st.mat(Mat::basic().vc().double().transparent().no_dw().blend(ADDITIVE).blend2()); let o = st.mesh(Geo::new(gp).with_col(gc).with_index(idx), m); st.sc.objs[o].culled = false; st.sc.objs[o].dyn_ = true; st.sc.objs[o].wave = true; st.add(o); }
    { let cq = st.canvas_tex(16, 16, false, |_, g| { for y in 0..4 { for x in 0..16 { g.fill_style(if (x + y) % 2 == 1 { "#141418" } else { "#f4f2ff" }); g.fill_rect(x as f64, (y * 4) as f64, 1.0, 4.0); } } });
        { let t = &mut st.sc.texs[cq]; t.min = NEAREST; t.mip = false; }
        let (mut p, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new()); let n = 8u32;
        for r in 0..2 { for a in 0..=n { let lat = (a as f64 / n as f64 * 2.0 - 1.0) * road; p.extend(at(st, (if r == 1 { 1 } else { np - 1 }) % np, lat, 0.06).iter().map(|&v| v as f32)); uv.extend_from_slice(&[a as f32 / n as f32, r as f32]); } }
        for a in 0..n { idx.extend_from_slice(&[a, a + n + 1, a + 1, a + 1, a + n + 1, a + n + 2]); }
        let m = st.mat(Mat::basic().map(cq).double().po(-3.0, -4.0)); let o = st.mesh(Geo::new(p).with_uv(uv).with_index(idx), m); st.sc.objs[o].dyn_ = true; st.sc.objs[o].wave = true; st.add(o); }
    { let (mut p, mut c, mut idx) = (Vec::new(), Vec::new(), Vec::new());
        let mut gate = |st: &St, i: usize, r: f64, tube: f64, seg: usize| { let t = st.t(i); let cc = at(st, i, 0.0, 0.0); let bank = st.r.bank[i].atan();
            let fwd = v3(t[0], 0.0, t[1]); let right = v3(-t[1], 0.0, t[0]).apply_axis_angle(fwd, -bank); let up = v3(0.0, 1.0, 0.0).apply_axis_angle(fwd, -bank);
            let base = (p.len() / 3) as u32; let ring = 6u32;
            for a in 0..=seg { let th = PI * a as f64 / seg as f64; let (cx, sy) = (th.cos(), th.sin()); let col = hue(a as f64 / seg as f64 * 0.85, 1.15);
                for q in 0..ring { let ph = q as f64 / ring as f64 * PI * 2.0; let rr2 = r + ph.cos() * tube; let off = ph.sin() * tube;
                    let v = right.mul(cx * rr2).add(up.mul(sy * rr2)).add(fwd.mul(off)); p.extend_from_slice(&[(cc[0] + v.x) as f32, (cc[1] + v.y) as f32, (cc[2] + v.z) as f32]); c.extend(col.iter().map(|&v| v as f32)); } }
            for a in 0..seg as u32 { for q in 0..ring { let aa = base + a * ring + q; let b = base + a * ring + (q + 1) % ring; idx.extend_from_slice(&[aa, aa + ring, b, b, aa + ring, b + ring]); } } };
        gate(st, 0, road * 1.08 + 1.6, 0.32, 40); let mut i = 140; while i + 60 < np { gate(st, i, road * 1.08 + 0.95, 0.16, 28); i += 140; }
        let m = st.mat(Mat::basic().vc()); let o = st.mesh(Geo::new(p).with_col(c).with_index(idx), m); st.sc.objs[o].culled = false; st.sc.objs[o].dyn_ = true; st.sc.objs[o].wave = true; st.add(o); }
    { let g = st.geo(octahedron(1.0)); let (mut m, mut cc) = (vec![], vec![]); let mut o = Tmp::default();
        for _ in 0..900 { let k = (st.rnd() * np as f64) as usize; let side = if st.rnd() < 0.5 { -1.0 } else { 1.0 }; let off = (road + 30.0 + st.rnd().powf(1.3) * 300.0) * side; let (x, z) = st.side(k, off);
            if st.path_dist(x, z) < road + 26.0 { continue; } let y = st.r.roadh[k] + st.rr(-120.0, 60.0); let s2 = st.rr(0.4, 2.2) * (1.0 + off.abs() / 160.0);
            let (a, b, c) = (st.rnd() * 3.0, st.rnd() * 3.0, st.rnd() * 3.0); o.pos(x, y, z).rot(a, b, c); let sy = st.rr(1.2, 2.2); o.scale(s2, s2 * sy, s2); m.push(o.matrix());
            let h = hue(st.rnd(), 0.9); cc.push(rgb(h[0], h[1], h[2])); }
        let mt = st.mat(Mat::basic()); let d = st.fogcut; st.instanced(g, mt, &m, &cc, d, 400.0); }
}

fn tex_text(st: &mut St, w: usize, h: usize, f: impl FnOnce(&mut Ctx)) -> TexId { st.canvas_tex(w, h, false, |_, g| f(g)) }

pub fn furniture(st: &mut St) {
    let np = st.np(); let road = st.road(); let post_m = st.mat(Mat::lambert().hex(0x2a2a2a));
    let p0 = st.p(0); let t0 = st.t(0); let (nx, nz) = (-t0[1], t0[0]); let yaw = t0[0].atan2(t0[1]); let h0 = st.height(p0[0], p0[1]);
    for s in [-1.0, 1.0] { let (x, z) = (p0[0] + nx * (road + 1.3) * s, p0[1] + nz * (road + 1.3) * s); let o = st.mesh(box_geo(0.25, 5.2, 0.25), post_m); st.sc.set_pos(o, x, st.height(x, z) + 2.4, z); st.add(o); st.obstacles.push([x, z, 0.2, 0.0]); }
    let ss = match st.m { super::road::M::Snow => "SS2", super::road::M::Canyon => "SS3", super::road::M::Lakes => "SS4", super::road::M::Touge => "SS5", super::road::M::Riviera => "SS6", super::road::M::Prism => "SS7", super::road::M::Ember => "SS8", _ => "SS1" };
    let banner = tex_text(st, 128, 24, |g| { g.fill_style("#e9e6dc"); g.fill_rect(0.0, 0.0, 128.0, 24.0);
        for i in 0..32 { for j in 0..2 { g.fill_style(if (i + j) % 2 == 1 { "#151515" } else { "#e9e6dc" }); g.fill_rect((i * 4) as f64, (j * 4) as f64, 4.0, 4.0); g.fill_rect((i * 4) as f64, (16 + j * 4) as f64, 4.0, 4.0); } }
        canvas::text(g, "FOGWOOD", 6, 8, canvas::parse("#c23a2c")); canvas::text(g, "RALLY", 52, 8, canvas::parse("#22324f")); canvas::text(g, ss, 90, 8, canvas::parse("#151515")); });
    let m = st.mat(Mat::lambert().map(banner).double()); let ban = st.mesh(plane((road + 1.3) * 2.0, 1.1, 1, 1), m); st.sc.set_rot(ban, 0.0, yaw, 0.0); st.sc.set_pos(ban, p0[0], h0 + 4.6, p0[1]); st.add(ban);
    let m = st.mat(Mat::lambert().hex(0xd9d4c4).po(-3.0, -4.0)); let line = st.mesh(plane(road * 2.0, 0.5, 1, 1), m); st.sc.set_rot_yxz(line, -PI / 2.0, yaw, 0.0); st.sc.set_pos(line, p0[0], h0 + 0.05, p0[1]); st.add(line);
    let chev = tex_text(st, 32, 16, |g| { g.fill_style("#c8302a"); g.fill_rect(0.0, 0.0, 32.0, 16.0); g.fill_style("#eeeae0"); for ox in [3, 13, 23] { for r in 0..12i64 { g.fill_rect((ox + (r - 6).abs()) as f64, (2 + r) as f64, 3.0, 1.0); } } });
    let chev_m = st.mat(Mat::lambert().map(chev).double()); let board = st.geo(plane(1.6, 0.8, 1, 1));
    let mut bale = cylinder(0.62, 0.62, 1.2, 10, 1, false); bale.rotate_z(PI / 2.0); let bale = st.geo(bale);
    let straw = tex_text(st, 32, 32, |g| { let cs = ["#d9b866", "#c9a453", "#e6c97d", "#b8914a", "#a07c3c"];
        for y in 0..32i64 { let mut x = 0i64; while x < 32 { let l = 2 + ((x * 7 + y * 13) % 5); g.fill_style(cs[((x * 3 + y * 5 + ((x * y) % 7)) % 5) as usize]); g.fill_rect(x as f64, y as f64, l as f64, 1.0); x += l; } }
        g.fill_style("#7a5a2a"); for k in 0..40i64 { g.fill_rect(((k * 11) % 32) as f64, ((k * 17) % 32) as f64, 2.0, 1.0); }
        g.fill_style("#8e3b2a"); g.fill_rect(9.0, 0.0, 1.0, 32.0); g.fill_rect(22.0, 0.0, 1.0, 32.0); });
    let bale_m = st.mat(Mat::lambert().map(straw));
    let (mut chev_i, mut post_i, mut bale_i) = (vec![], vec![], vec![]); let mut o = Tmp::default();
    let mut i = 0; while i < np {
        if !(st.r.vcurve[i] > 17.0 || st.r.tun_at(i).is_some() || st.r.in_village(i) || (st.vol && st.r.bridge_near(i, 12))) {
            let a = st.t((i + np - 10) % np); let b = st.t((i + 10) % np); let turn = a[0] * b[1] - a[1] * b[0]; let side = if turn > 0.0 { -1.0 } else { 1.0 };
            let tn = st.t(i); let (x, z) = st.side(i, (road + 1.7) * side);
            o.pos(x, st.height(x, z) + 1.1, z).rot(0.0, (-tn[0]).atan2(-tn[1]), 0.0).scale(if side < 0.0 { -1.0 } else { 1.0 }, 1.0, 1.0); chev_i.push(o.matrix());
            o.pos(x, st.height(x, z) + 0.45, z).rot(0.0, 0.0, 0.0).scale(1.0, 1.0, 1.0); post_i.push(o.matrix());
            if !st.snow && st.r.vcurve[i] < 13.0 && i % 14 == 0 { for q in -1i64..=1 { let k2 = st.r.wrap(i as i64 + q * 3); let tt = st.t(k2); let (bx, bz) = st.side(k2, (road + 1.0) * side);
                o.pos(bx, st.height(bx, bz) + 0.6, bz).rot(0.0, tt[0].atan2(tt[1]) + PI / 2.0, 0.0).scale(1.0, 1.0, 1.0); bale_i.push(o.matrix()); st.obstacles.push([bx, bz, 0.6, 3.0]); } }
        }
        i += 7; }
    st.instanced_dd(board, chev_m, &chev_i, &whites(chev_i.len()), 320.0); let pg = st.geo(box_geo(0.08, 1.1, 0.08)); st.instanced_dd(pg, post_m, &post_i, &whites(post_i.len()), 320.0);
    if !bale_i.is_empty() { st.instanced_dd(bale, bale_m, &bale_i, &whites(bale_i.len()), 320.0); }
    { let sign = |st: &mut St, draw: &dyn Fn(&mut Ctx)| tex_text(st, 32, 32, |g| { g.fill_style("#151515"); g.fill_rect(0.0, 0.0, 32.0, 32.0); g.fill_style("#f2c230"); g.fill_rect(2.0, 2.0, 28.0, 28.0); g.fill_style("#151515"); draw(g); });
        let bend_t = sign(st, &|g: &mut Ctx| { g.fill_rect(18.0, 13.0, 3.0, 13.0); g.fill_rect(12.0, 11.0, 9.0, 3.0); for r in 0..=6 { g.fill_rect((6 + r) as f64, (12 - r) as f64, 1.0, (1 + 2 * r) as f64); } });
        let jump_t = sign(st, &|g: &mut Ctx| { for x in 4..28 { let h = super::jr(7.0 * (-((x as f64 - 13.0).powi(2)) / 30.0).exp()); g.fill_rect(x as f64, 25.0 - h, 1.0, h + 2.0); }
            g.fill_rect(17.0, 8.0, 9.0, 3.0); g.fill_rect(19.0, 6.0, 5.0, 2.0); g.fill_rect(17.0, 11.0, 2.0, 2.0); g.fill_rect(24.0, 11.0, 2.0, 2.0); });
        let bend_m = st.mat(Mat::lambert().map(bend_t).double()); let jump_m = st.mat(Mat::lambert().map(jump_t).double());
        let dgeo = st.geo(plane(1.15, 1.15, 1, 1)); let pole_g = st.geo(box_geo(0.08, 1.9, 0.08));
        let (mut bend_i, mut jump_i, mut pole_i) = (vec![], vec![], vec![]);
        let place = |st: &mut St, list: &mut Vec<M4>, pole_i: &mut Vec<M4>, i: usize, side: f64, mirror: bool| { let tn = st.t(i); let (x, z) = st.side(i, (road + 1.9) * side); let gy = st.height(x, z);
            if st.path_dist(x, z) < road + 0.8 { return; } let mut o = Tmp::default();
            o.pos(x, gy + 2.0, z).rot(0.0, (-tn[0]).atan2(-tn[1]), PI / 4.0).scale(if mirror { -1.0 } else { 1.0 }, 1.0, 1.0); list.push(o.matrix());
            o.pos(x, gy + 0.95, z).rot(0.0, 0.0, 0.0).scale(1.0, 1.0, 1.0); pole_i.push(o.matrix()); st.obstacles.push([x, z, 0.12, 0.0]); };
        let mut last_bend: i64 = -999; let mut i = 30; while i < np {
            if !(st.r.vcurve[i] > 10.0 || st.r.vcurve[(i + np - 20) % np] < 18.0 || (i as i64) - last_bend < 160) {
                let a = st.t((i + np - 2) % np); let b = st.t((i + 18) % np); let left = a[0] * b[1] - a[1] * b[0] > 0.0;
                let k = (i + np - 45) % np; if st.r.tun_at(k).is_none() && !st.r.in_village(k) { place(st, &mut bend_i, &mut pole_i, k, if left { -1.0 } else { 1.0 }, !left); }
                last_bend = i as i64; }
            i += 3; }
        let jl: Vec<usize> = st.r.jumps.iter().map(|j| j.i).collect();
        for ji in jl { let k = (ji + np - 55) % np; if st.r.tun_at(k).is_none() { place(st, &mut jump_i, &mut pole_i, k, 1.0, false); } }
        if st.r.ramp_i >= 0 { let k = st.r.wrap(st.r.ramp_i - st.r.ramp_l as i64 - 60); place(st, &mut jump_i, &mut pole_i, k, 1.0, false); }
        if !bend_i.is_empty() { st.instanced_dd(dgeo, bend_m, &bend_i, &whites(bend_i.len()), 320.0); }
        if !jump_i.is_empty() { st.instanced_dd(dgeo, jump_m, &jump_i, &whites(jump_i.len()), 320.0); }
        if !pole_i.is_empty() { st.instanced_dd(pole_g, post_m, &pole_i, &whites(pole_i.len()), 320.0); } }
    { let mut pg = box_geo(0.11, 1.0, 0.11); pg.translate(0.0, 0.5, 0.0); let mut cg = box_geo(0.115, 0.22, 0.115); cg.translate(0.0, 0.86, 0.0); let mut rf = box_geo(0.03, 0.12, 0.06); rf.translate(0.0, 0.86, 0.0);
        let (pg, cg, rf) = (st.geo(pg), st.geo(cg), st.geo(rf)); let (mut pm, mut pc) = (vec![], vec![]);
        let mut i = 0; while i < np { for s in [-1.0, 1.0] { let tn = st.t(i); let (x, z) = st.side(i, (road + 0.9) * s);
            if st.path_dist(x, z) < road + 0.6 || st.r.tun_at(i).is_some() || st.r.in_village(i) || (st.vol && st.r.bridge_near(i, 6)) { continue; }
            o.pos(x, st.height(x, z) - 0.05, z).rot(0.0, tn[0].atan2(tn[1]), 0.0).scale(1.0, 1.0, 1.0); pm.push(o.matrix()); pc.push(rgb(0.92, 0.9, 0.84)); } i += 20; }
        let m = st.mat(Mat::lambert()); st.instanced_dd(pg, m, &pm, &pc, 260.0);
        let m = st.mat(Mat::lambert().hex(0x1a1a1a)); st.instanced_dd(cg, m, &pm, &whites(pm.len()), 260.0);
        let m = st.mat(Mat::basic().hex(0xff6a3a)); st.instanced_dd(rf, m, &pm, &whites(pm.len()), 200.0); }
    let jt = tex_text(st, 32, 32, |g| { g.fill_style("#f0c832"); g.fill_rect(0.0, 0.0, 32.0, 32.0); g.fill_style("#151515"); g.fill_rect(1.0, 1.0, 30.0, 1.0); g.fill_rect(1.0, 30.0, 30.0, 1.0); g.fill_rect(1.0, 1.0, 1.0, 30.0); g.fill_rect(30.0, 1.0, 1.0, 30.0);
        for r in 0..10 { g.fill_rect((16 - r) as f64, (5 + r) as f64, (r * 2 + 1) as f64, 1.0); } canvas::text(g, "JUMP", 5, 20, canvas::parse("#151515")); });
    let jm = st.mat(Mat::lambert().map(jt).double());
    let jl: Vec<usize> = st.r.jumps.iter().map(|j| j.i).collect();
    for ji in jl { let k = (ji + np - 55) % np; let tn = st.t(k); let (x, z) = st.side(k, -(road + 1.4));
        let bd = st.mesh(plane(0.9, 0.9, 1, 1), jm); st.sc.set_rot(bd, 0.0, (-tn[0]).atan2(-tn[1]), 0.0); st.sc.set_pos(bd, x, st.height(x, z) + 1.5, z); st.add(bd);
        let ps = st.mesh(box_geo(0.07, 1.3, 0.07), post_m); st.sc.set_pos(ps, x, st.height(x, z) + 0.6, z); st.add(ps); }
}

pub fn grass(st: &mut St) {
    if st.snow || st.pri || st.vol { return; }
    let (gcell, gnear, gfar) = (1.1, 30.0, 46.0); let road = st.road(); let band = road + 26.0; let (gxh, gzh) = (st.r.gxh as f64, st.r.gzh as f64); let wl = crate::world::WL;
    let (mut m, mut c) = (vec![], vec![]); let mut o = Tmp::default(); let lamp = [1.0, 0.6, 0.26];
    for cj in (-gzh / gcell).floor() as i64..=(gzh / gcell).floor() as i64 { for ci in (-gxh / gcell).floor() as i64..=(gxh / gcell).floor() as i64 { let (fi, fj) = (ci as f64, cj as f64);
        let (x0, z0) = (fi * gcell, fj * gcell); let p0 = st.path_dist(x0, z0); if p0 >= 19.9 && st.road_dist(x0, z0) > band { continue; }
        for k in 0..2 { let kf = k as f64;
            let hx = hash2(fi * 3.0 + kf, fj); let hz = hash2(fi, fj * 5.0 + kf); let hs = hash2(fi + 77.0 * kf, fj - 31.0);
            let (x, z) = ((fi + hx - 0.5) * gcell, (fj + hz - 0.5) * gcell); let pd = st.path_dist(x, z);
            if pd < road + 0.3 + hs * 0.6 || hs > fbm(x * 0.07 + 3.0, z * 0.07, 4) * (if st.can { 0.7 } else if st.tou { 0.45 } else if st.riv { 0.42 } else { 1.6 }) + (if st.can { 0.02 } else if st.tou || st.riv { 0.0 } else { 0.15 }) { continue; }
            if st.lak && st.height(x, z) < wl + 0.2 { continue; }
            if st.riv { let kk = st.path_idx(x, z); if st.height(x, z) < 2.0 || st.r.tun_at(kk).is_some() || (st.r.in_village(kk) && pd < 13.0) { continue; } }
            let s = (0.45 + hs * 0.65 * if pd < road + 1.5 { 0.6 } else { 1.0 }) * if st.tou { 0.7 } else if st.riv { 0.75 } else { 1.0 };
            o.pos(x, st.height(x, z) - 0.04, z).rot(0.0, hx * 6.28, 0.0).scale(s * 1.4, s, s * 1.4); m.push(o.matrix());
            let g = (0.55 + hz * 0.3) * (1.0 - st.shade_at(x, z) * 0.45);
            let mut cc = if st.can { rgb(0.95 * g + 0.2, 0.75 * g + 0.12, 0.42 * g + 0.06) } else if st.tou { if hs > 0.5 { rgb(0.3 * g + 0.04, 0.42 * g + 0.04, 0.18 * g) } else { rgb(0.5 * g + 0.1, 0.38 * g, 0.18 * g) } }
                else if st.riv { if hs > 0.45 { rgb(0.62 * g + 0.12, 0.54 * g + 0.06, 0.26 * g) } else { rgb(0.4 * g + 0.05, 0.46 * g + 0.05, 0.2 * g) } } else { rgb(0.36 * g + hs * 0.08, 0.62 * g, 0.24 * g) };
            let oc = st.sun_shadow_at(x, z); if oc > 0.004 { cc.r *= st.shadow_mul(oc, 0); cc.g *= st.shadow_mul(oc, 1); cc.b *= st.shadow_mul(oc, 2); }
            if st.tou { let lk = st.lamp_at(x, z) * 0.9; cc.r += lamp[0] * lk; cc.g += lamp[1] * lk; cc.b += lamp[2] * lk; }
            c.push(cc); } } }
    let mt = st.mat(Mat::basic().map(st.t_grass).at(0.5).double().sway(0.14, true, Some((gnear, gfar)))); st.instanced(st.clump2, mt, &m, &c, gfar + 12.0, 48.0);
}
