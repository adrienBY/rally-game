//! The stage's first pixel textures (foliage, bark, ground, and the road surface of each stage), painted as the page painted them.
//! The foliage, bark and ground come first in every stage's random sequence, so they are the same everywhere: painted once per run.
use super::canvas::{self, rgbi, Ctx};
use super::three::Tex;
use super::{Rng, St};
use crate::noise::vnoise;
use std::sync::OnceLock;

struct Base { texs: Vec<(usize, Vec<u8>, bool)>, seed: i64 }
static BASE: OnceLock<Base> = OnceLock::new();

fn paint_base(st: &mut Rng) -> Vec<(usize, Vec<u8>, bool)> {
    let mut out = Vec::new();
    let mut tex = |st: &mut Rng, s: usize, repeat: bool, f: &mut dyn FnMut(&mut Rng, &mut Ctx)| { let mut g = Ctx::new(s, s); f(st, &mut g); out.push((s, g.data(), repeat)); };
    // leaves: soft grey blobs in a disc
    tex(st, 128, false, &mut |st, g| { let s = 128.0;
        for _ in 0..900 { let a = st.rnd() * std::f64::consts::PI * 2.0; let r = st.rnd().powf(0.6) * s * 0.47; let l = (120.0 + st.rnd() * 135.0 * (1.0 - r / s * 0.9)) as i64 as f64;
            g.fill_c(rgbi(l, l, l)); g.begin_path(); let (rx, ry) = (st.rr(2.0, 4.5), st.rr(1.2, 2.6)); let rot = st.rnd() * 3.0;
            g.ellipse(s / 2.0 + a.cos() * r, s / 2.0 + a.sin() * r * 0.9, rx, ry, rot); g.fill(); } });
    // needles: rows of short round-capped strokes
    tex(st, 128, false, &mut |st, g| { let s = 128.0; g.round_cap = true;
        for k in 0..7 { let (y0, w) = (20.0 + k as f64 * 14.0, 18.0 + k as f64 * 7.0);
            for _ in 0..70 { let x = s / 2.0 + st.rr(-w, w); let y = y0 + st.rr(-5.0, 8.0); let l = (110.0 + st.rnd() * 120.0) as i64 as f64;
                g.stroke_c(rgbi(l, l, l)); g.line_width = st.rr(1.5, 3.0) as f32; g.begin_path(); g.move_to(x, y); let (dx, dy) = (st.rr(-4.0, 4.0), st.rr(4.0, 10.0)); g.line_to(x + dx, y + dy); g.stroke(); } } });
    // grass blades
    tex(st, 64, false, &mut |st, g| { let s = 64.0;
        for _ in 0..60 { let x = st.rr(4.0, s - 4.0); let h = st.rr(s * 0.35, s * 0.98); let lean = st.rr(-10.0, 10.0); let l = (110.0 + st.rnd() * 145.0) as i64 as f64;
            g.stroke_c(rgbi(l, l, l)); g.line_width = st.rr(1.0, 2.4) as f32; g.begin_path(); g.move_to(x, s); g.quad_to(x + lean * 0.3, s - h * 0.6, x + lean, s - h); g.stroke(); } });
    // fern fronds
    tex(st, 128, false, &mut |st, g| { let s = 128.0;
        for _ in 0..9 { let ang = -std::f64::consts::PI / 2.0 + st.rr(-1.1, 1.1); let len = st.rr(s * 0.32, s * 0.5);
            for k in 0..22 { let t = k as f64 / 22.0; let x = s / 2.0 + ang.cos() * len * t + (t * 3.0).sin() * 4.0; let y = s - 4.0 + ang.sin() * len * t + t * t * 18.0;
                let l = (120.0 + st.rnd() * 120.0) as i64 as f64; let w = (1.0 - t) * 11.0 + 2.0; g.fill_c(rgbi(l, l, l)); g.fill_rect(x - w, y - 1.0, w, 2.4); g.fill_rect(x, y - 1.0, w, 2.4); } } });
    // flowers: stems and pale petals
    tex(st, 64, false, &mut |st, g| { let s = 64.0;
        for _ in 0..22 { let x = st.rr(6.0, s - 6.0); g.stroke_style("#304a22"); g.line_width = 1.5; g.begin_path(); g.move_to(x, s); let (dx, y1) = (st.rr(-3.0, 3.0), st.rr(s * 0.15, s * 0.6)); g.line_to(x + dx, y1); g.stroke(); }
        for _ in 0..70 { let l = (200.0 + st.rnd() * 55.0) as i64 as f64; g.fill_c(rgbi(l, l, l * 0.85)); let (x, y, w, h) = (st.rr(4.0, s - 4.0), st.rr(s * 0.1, s * 0.62), st.rr(2.0, 4.0), st.rr(2.0, 4.0)); g.fill_rect(x, y, w, h); } });
    // bark
    tex(st, 64, true, &mut |st, g| { let s = 64.0; g.fill_style("#6b4a33"); g.fill_rect(0.0, 0.0, s, s);
        for _ in 0..160 { let c = if st.rnd() < 0.5 { canvas::rgba(30.0, 20.0, 14.0, st.rr(0.2, 0.6)) } else { canvas::rgba(160.0, 110.0, 70.0, st.rr(0.1, 0.35)) }; g.fill_c(c);
            let (x, y, w, h) = (st.rr(0.0, s), st.rr(-10.0, s), st.rr(1.0, 3.0), st.rr(8.0, 30.0)); g.fill_rect(x, y, w, h); } });
    // ground speckle (image data)
    tex(st, 128, true, &mut |st, g| { let n = 128 * 128; let mut d = vec![0u8; n * 4];
        for i in 0..n { let a = (st.rnd() * 85.0) as i64; let v = (170 + a - if st.rnd() < 0.08 { 60 } else { 0 }) as u8; d[i * 4] = v; d[i * 4 + 1] = v; d[i * 4 + 2] = v; d[i * 4 + 3] = 255; }
        g.put(&d); });
    out
}

pub fn base(st: &mut St) {
    let b = BASE.get_or_init(|| { let mut tmp = Rng(1337); let texs = paint_base(&mut tmp); Base { texs, seed: tmp.0 } });
    let mut ids = Vec::new();
    for (s, d, rep) in &b.texs { ids.push(st.tex(Tex::canvas(*s, *s, d.clone(), *rep))); }
    st.seed = b.seed;
    st.t_leaf = ids[0]; st.t_needle = ids[1]; st.t_grass = ids[2]; st.t_fern = ids[3]; st.t_flower = ids[4]; st.t_bark = ids[5]; st.t_ground = ids[6];
    st.t_gravel = gravel(st);
}

/// the road surface: gravel, tarmac, sand, snow or the prism's rainbow, per stage
fn gravel(st: &mut St) -> usize {
    let (pri, riv, vol, tou, can, lak, snow) = (st.pri, st.riv, st.vol, st.tou, st.can, st.lak, st.snow);
    st.canvas_tex(64, 64, true, |st, g| { let s = 64.0;
        let ruts = |st: &mut St, g: &mut Ctx, base: f64, col: [u8; 4]| { for cx in [0.27, 0.73] { for y in 0..64 { let y = y as f64; let w = base + (vnoise(y * 0.3, cx * 10.0) * 3.0) as i64 as f64;
            g.fill_c(col); g.fill_rect(cx * s - w / 2.0 + (vnoise(y * 0.15, cx) * 3.0) as i64 as f64, y, w, 1.0); let _ = &st; } } };
        let specks = |st: &mut St, g: &mut Ctx, n: usize, p: f64, a: (f64, f64, f64, f64, f64), b: (f64, f64, f64, f64, f64), two: f64| {
            for _ in 0..n { let c = if st.rnd() < p { canvas::rgba(a.0, a.1, a.2, st.rr(a.3, a.4)) } else { canvas::rgba(b.0, b.1, b.2, st.rr(b.3, b.4)) }; g.fill_c(c);
                let x = st.rr(0.0, s) as i64 as f64; let y = st.rr(0.0, s) as i64 as f64; let w = if two > 0.0 { if st.rnd() < two { 2.0 } else { 1.0 } } else { 1.0 }; g.fill_rect(x, y, w, 1.0); } };
        if pri {
            let bands: [[f64; 3]; 7] = [[255.0, 64.0, 72.0], [255.0, 150.0, 40.0], [255.0, 232.0, 60.0], [70.0, 230.0, 110.0], [50.0, 210.0, 255.0], [80.0, 110.0, 255.0], [190.0, 80.0, 255.0]];
            for x in 0..64i64 { let f = x as f64 / s * 7.0; let b = (6f64).min(f.floor()) as usize; let e = (f - b as f64).min(b as f64 + 1.0 - f); let seam = if e < 0.07 { 0.35 } else { 1.0 };
                for y in 0..64i64 { let n = 0.86 + (((x * 13 + y * 7 + (x * y) % 5) % 9) as f64) / 60.0; let pulse = if y % 32 < 2 { 1.35 } else { 1.0 }; let c = bands[b];
                    g.px(x, y, rgbi((255f64).min(c[0] * n * seam * pulse), (255f64).min(c[1] * n * seam * pulse), (255f64).min(c[2] * n * seam * pulse))); } }
        } else if riv {
            g.fill_style("rgb(56,56,62)"); g.fill_rect(0.0, 0.0, s, s);
            specks(st, g, 700, 0.5, (34.0, 34.0, 40.0, 0.2, 0.45), (104.0, 104.0, 112.0, 0.12, 0.3), 0.0);
            for cx in [0.3, 0.7] { for y in 0..64 { g.fill_style("rgba(28,28,32,0.22)"); g.fill_rect(cx * s - 4.0, y as f64, 8.0, 1.0); } }
            g.fill_style("rgb(228,226,216)"); g.fill_rect(3.0, 0.0, 2.0, s); g.fill_rect(s - 5.0, 0.0, 2.0, s);
            let mut y = 0.0; while y < s { g.fill_rect(s / 2.0 - 1.0, y, 2.0, 10.0); y += 16.0; }
        } else if vol {
            g.fill_style("rgb(92,86,82)"); g.fill_rect(0.0, 0.0, s, s);
            specks(st, g, 1000, 0.55, (40.0, 38.0, 38.0, 0.25, 0.55), (150.0, 140.0, 128.0, 0.2, 0.45), 0.25);
            ruts(st, g, 6.0, canvas::parse("rgba(52,48,46,0.5)"));
        } else if tou {
            g.fill_style("rgb(70,70,76)"); g.fill_rect(0.0, 0.0, s, s);
            specks(st, g, 900, 0.5, (40.0, 40.0, 46.0, 0.2, 0.5), (120.0, 120.0, 126.0, 0.15, 0.35), 0.0);
            for cx in [0.3, 0.7] { for y in 0..64 { g.fill_style("rgba(36,36,40,0.35)"); g.fill_rect(cx * s - 4.0, y as f64, 8.0, 1.0); } }
            g.fill_style("rgb(214,212,200)"); g.fill_rect(3.0, 0.0, 2.0, s); g.fill_rect(s - 5.0, 0.0, 2.0, s);
            g.fill_style("rgb(228,196,92)"); let mut y = 0.0; while y < s { g.fill_rect(s / 2.0 - 1.0, y, 2.0, 9.0); y += 16.0; }
            for _ in 0..60 { g.fill_style("rgba(60,60,64,0.6)"); let x = st.rr(2.0, s - 2.0) as i64 as f64; let y = st.rr(0.0, s) as i64 as f64; g.fill_rect(x, y, 1.0, 1.0); }
        } else if can {
            g.fill_style("rgb(196,140,96)"); g.fill_rect(0.0, 0.0, s, s);
            specks(st, g, 900, 0.5, (150.0, 96.0, 60.0, 0.2, 0.5), (236.0, 196.0, 150.0, 0.2, 0.5), 0.2);
            ruts(st, g, 7.0, canvas::parse("rgba(140,88,54,0.42)"));
        } else if lak {
            g.fill_style("rgb(138,124,106)"); g.fill_rect(0.0, 0.0, s, s);
            specks(st, g, 1000, 0.5, (80.0, 72.0, 62.0, 0.2, 0.5), (200.0, 190.0, 172.0, 0.2, 0.5), 0.25);
            ruts(st, g, 6.0, canvas::parse("rgba(84,76,66,0.45)"));
        } else if snow {
            g.fill_style("rgb(214,221,232)"); g.fill_rect(0.0, 0.0, s, s);
            specks(st, g, 700, 0.6, (170.0, 182.0, 200.0, 0.2, 0.5), (250.0, 252.0, 255.0, 0.3, 0.7), 0.2);
            for cx in [0.27, 0.73] { for y in 0..64 { let y = y as f64; let w = 7.0 + (vnoise(y * 0.3, cx * 10.0) * 3.0) as i64 as f64;
                g.fill_style("rgba(128,142,165,0.55)"); g.fill_rect(cx * s - w / 2.0 + (vnoise(y * 0.15, cx) * 3.0) as i64 as f64, y, w, 1.0);
                if st.rnd() < 0.3 { g.fill_style("rgba(235,242,255,0.6)"); let o = (st.rnd() * 3.0) as i64 as f64; g.fill_rect(cx * s - 1.0 + o, y, 1.0, 1.0); } } }
            for _ in 0..40 { let c = canvas::rgba(110.0, 92.0, 74.0, st.rr(0.3, 0.6)); g.fill_c(c); let x = st.rr(0.0, s) as i64 as f64; let y = st.rr(0.0, s) as i64 as f64; g.fill_rect(x, y, 1.0, 1.0); }
        } else {
            g.fill_style("rgb(148,118,86)"); g.fill_rect(0.0, 0.0, s, s);
            specks(st, g, 900, 0.5, (90.0, 70.0, 50.0, 0.2, 0.5), (200.0, 180.0, 150.0, 0.2, 0.5), 0.2);
            ruts(st, g, 6.0, canvas::parse("rgba(92,72,52,0.45)"));
            for _ in 0..120 { let c = canvas::rgba(215.0, 200.0, 175.0, st.rr(0.4, 0.8)); g.fill_c(c); let x = (s * st.rr(0.42, 0.58)) as i64 as f64; let y = st.rr(0.0, s) as i64 as f64; g.fill_rect(x, y, 1.0, 1.0); }
        }
    })
}
