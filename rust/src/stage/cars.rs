//! The cars: five lofted bodies (each with its pixel livery atlas and every pickable livery), one shared set of wheels, the shadow, and
//! the rivals' instanced copies of every part. The livery atlases never change: painted once per run, the five bodies in parallel.
use super::canvas::{self, Ctx};
use super::shape::extrude;
use super::three::*;
use super::{RgPart, St, Wheel};
use std::f64::consts::PI;
use std::sync::OnceLock;

const WB_F: f64 = 1.22; const WB_R: f64 = -1.28; const TRACK: f64 = 0.75; const TIRE_R: f64 = 0.31;
#[derive(Clone, Copy)] struct Wing { z: f64, chord: f64, y: f64, span: f64, base: f64, stay: f64, swan: bool }
#[derive(Clone, Copy)] struct Liv { pat: &'static str, base: &'static str, a: &'static str, b: &'static str, c: &'static str }
#[derive(Clone, Copy, PartialEq)] enum Lights { Swept, Popup, Rect, Round, Strip }
struct Spec { zn: f64, zt: f64, zw: f64, zr: f64, zre: f64, zh: f64, nose: [f64; 5], hood: [f64; 2], belt: f64, deck_rise: f64, tail_drop: f64, yb: f64, lift_f: f64, lift_r: f64, w0: f64, flare: [f64; 2],
    roof: f64, roof_arc: f64, roof_w: f64, inset: f64, pow: [f64; 2], z_b: Option<f64>, thick_c: Option<f64>, lights: Lights, wing: Option<Wing>, scoop: bool, louvres: bool, pods: bool, bonnet_vents: bool, splitter: bool, liv: Liv }
fn specs() -> Vec<Spec> {
    vec![
        Spec { zn: 2.03, zt: -1.99, zw: 0.86, zr: 0.02, zre: -1.22, zh: -1.9, nose: [1.8, 0.79, 0.1, 1.8, 0.36], hood: [0.96, 0.015], belt: 1.02, deck_rise: 0.0, tail_drop: 0.09, yb: 0.25, lift_f: 0.06, lift_r: 0.1, w0: 0.83, flare: [0.115, 0.125],
            roof: 1.415, roof_arc: 0.02, roof_w: 0.6, inset: 0.135, pow: [1.12, 0.85], z_b: Some(-0.57), thick_c: Some(-1.18), lights: Lights::Swept, wing: Some(Wing { z: -1.48, chord: 0.42, y: 1.52, span: 1.5, base: 1.08, stay: 0.42, swan: false }), scoop: true,
            louvres: false, pods: false, bonnet_vents: false, splitter: false, liv: Liv { pat: "swoosh", base: "#ece8dc", a: "#1d2c5c", b: "#e2662a", c: "#e8b43a" } },
        Spec { zn: 2.06, zt: -2.02, zw: 0.36, zr: -0.2, zre: -0.72, zh: -1.68, nose: [1.92, 0.63, 0.06, 1.5, 0.3], hood: [0.89, 0.0], belt: 0.97, deck_rise: 0.07, tail_drop: 0.1, yb: 0.24, lift_f: 0.05, lift_r: 0.08, w0: 0.85, flare: [0.15, 0.18],
            roof: 1.29, roof_arc: 0.01, roof_w: 0.56, inset: 0.15, pow: [1.0, 1.05], z_b: None, thick_c: None, lights: Lights::Popup, wing: Some(Wing { z: -1.66, chord: 0.36, y: 1.2, span: 1.56, base: 0.98, stay: 0.5, swan: false }), scoop: false,
            louvres: true, pods: true, bonnet_vents: false, splitter: false, liv: Liv { pat: "stripes", base: "#b92c2a", a: "#ece8dc", b: "#141414", c: "#e8b43a" } },
        Spec { zn: 2.09, zt: -2.07, zw: 0.64, zr: -0.1, zre: -0.95, zh: -1.45, nose: [1.86, 0.8, 0.09, 1.7, 0.3], hood: [0.95, 0.01], belt: 1.0, deck_rise: 0.02, tail_drop: 0.08, yb: 0.25, lift_f: 0.05, lift_r: 0.07, w0: 0.85, flare: [0.11, 0.12],
            roof: 1.42, roof_arc: 0.015, roof_w: 0.6, inset: 0.13, pow: [1.1, 0.75], z_b: Some(-0.42), thick_c: None, lights: Lights::Rect, wing: Some(Wing { z: -1.66, chord: 0.38, y: 1.42, span: 1.5, base: 0.99, stay: 0.5, swan: false }), scoop: true,
            louvres: false, pods: false, bonnet_vents: true, splitter: false, liv: Liv { pat: "twotone", base: "#1f5a3d", a: "#ece8dc", b: "#e8b43a", c: "#141414" } },
        Spec { zn: 1.91, zt: -1.87, zw: 0.72, zr: 0.14, zre: -0.86, zh: -1.58, nose: [1.74, 0.77, 0.12, 2.0, 0.42], hood: [0.93, 0.01], belt: 0.95, deck_rise: 0.05, tail_drop: 0.12, yb: 0.27, lift_f: 0.05, lift_r: 0.06, w0: 0.78, flare: [0.13, 0.15],
            roof: 1.33, roof_arc: 0.025, roof_w: 0.56, inset: 0.11, pow: [1.0, 0.9], z_b: None, thick_c: None, lights: Lights::Round, wing: None, scoop: false,
            louvres: false, pods: true, bonnet_vents: false, splitter: false, liv: Liv { pat: "classic", base: "#e07a2a", a: "#141414", b: "#ece8dc", c: "#e8b43a" } },
        Spec { zn: 2.12, zt: -2.06, zw: 0.6, zr: -0.06, zre: -0.86, zh: -1.76, nose: [1.66, 0.74, 0.15, 1.4, 0.3], hood: [0.93, 0.02], belt: 0.99, deck_rise: 0.0, tail_drop: 0.1, yb: 0.24, lift_f: 0.04, lift_r: 0.08, w0: 0.86, flare: [0.15, 0.165],
            roof: 1.36, roof_arc: 0.02, roof_w: 0.55, inset: 0.15, pow: [1.18, 0.8], z_b: Some(-0.5), thick_c: Some(-1.05), lights: Lights::Strip, wing: Some(Wing { z: -1.52, chord: 0.46, y: 1.5, span: 1.62, base: 1.04, stay: 0.36, swan: true }), scoop: true,
            louvres: false, pods: false, bonnet_vents: false, splitter: true, liv: Liv { pat: "hoop", base: "#2c3038", a: "#3fd0d8", b: "#e2662a", c: "#ece8dc" } },
    ]
}
fn liveries() -> Vec<Liv> {
    let l = |pat, base, a, b, c| Liv { pat, base, a, b, c };
    let mut v = vec![l("bolt", "#18181b", "#f2c230", "#ece8dc", "#c23a2c"), l("checker", "#ece8dc", "#141414", "#c23a2c", "#141414"), l("flames", "#1b1b22", "#e2662a", "#f2c230", "#b8281f"), l("camo", "#5b6640", "#3b4429", "#8c8a5e", "#2a2e22"),
        l("split", "#1aa39a", "#141414", "#ece8dc", "#141414"), l("fade", "#1d2c5c", "#c2347a", "#ece8dc", "#1d2c5c"), l("gulf", "#8ec5e6", "#e2662a", "#141414", "#141414"), l("zebra", "#ece8dc", "#141414", "#141414", "#c23a2c"),
        l("sunset", "#2a1a4a", "#e2662a", "#f2c230", "#c2347a"), l("chevron", "#20242c", "#f2c230", "#ece8dc", "#20242c"), l("dots", "#c23a2c", "#ece8dc", "#141414", "#ece8dc"), l("carbon", "#1c1d20", "#2c2e33", "#c23a2c", "#3a3d44"),
        l("neon", "#101014", "#3fd0d8", "#e23aa8", "#101014"), l("pixel", "#2e7d4f", "#8fd16a", "#f2c230", "#1d4f32"), l("tricolore", "#ece8dc", "#1d3a8c", "#c23a2c", "#ece8dc")];
    for (a, b) in [("#c23a2c", "#f2c230"), ("#1d5fc4", "#ece8dc"), ("#2e9a4f", "#141414"), ("#f2c230", "#141414"), ("#7a3ac2", "#3fd0d8"), ("#e2662a", "#141414")] { v.push(l("team", "#141414", a, b, "#ece8dc")); }
    v
}
fn ss(a: f64, b: f64, x: f64) -> f64 { let t = ((x - a) / (b - a)).clamp(0.0, 1.0); t * t * (3.0 - 2.0 * t) }
fn lerp(a: f64, b: f64, t: f64) -> f64 { a + (b - a) * t }
const ARCH_Y: f64 = 0.27; const ARCH_R: f64 = 0.43; const LINER: f64 = 0.6;
fn arch_y(z: f64) -> f64 { let mut y: f64 = 0.0; for c in [WB_F, WB_R] { let d = z - c; if d.abs() < ARCH_R { y = y.max(ARCH_Y + (ARCH_R * ARCH_R - d * d).sqrt()); } } y }
impl Spec {
    fn ztop0(&self) -> f64 { self.zt + (0.12f64).min(self.zh - self.zt) }
    fn y_top(&self, z: f64) -> f64 { let [nz, ntop, ndrop, npow, _] = self.nose;
        if z > nz { ntop - ndrop * ((z - nz).max(0.0) / (self.zn - nz)).powf(npow) }
        else if z > self.zw { lerp(self.hood[0], ntop, ((z - self.zw) / (nz - self.zw)).powf(1.25)) + self.hood[1] * (PI * (z - self.zw) / (nz - self.zw)).sin() }
        else if z > self.zh { lerp(self.hood[0], self.belt, (self.zw - z) / (self.zw - self.zh)) }
        else { let zt0 = self.ztop0(); self.belt + self.deck_rise * (1f64).min((self.zh - z) / (0.05f64).max(self.zh - zt0)) - self.tail_drop * ((zt0 - z).max(0.0) / (zt0 - self.zt)).powf(1.6) } }
    fn y_bot(&self, z: f64) -> f64 { self.yb + self.lift_f * ss(self.zn - 0.48, self.zn, z) + self.lift_r * ss(self.zt + 0.44, self.zt, z) }
    fn half_w(&self, z: f64) -> f64 {
        let mut w = self.w0 + self.flare[0] * (-((z - WB_F) / 0.42).powi(4)).exp() + self.flare[1] * (-((z - WB_R) / 0.44).powi(4)).exp() + 0.02 * ss(1.9, -1.0, z) * ss(-2.0, -1.0, z);
        let nz = self.zn - 0.41; if z > nz { w *= (1.0 - self.nose[4] * ((z - nz) / (self.zn - nz)).powf(2.6)).sqrt(); } let tz = self.zt + 0.29; if z < tz { w *= 1.0 - 0.1 * ((tz - z) / (tz - self.zt)).powi(2); } w }
    fn lower_half(&self, z: f64) -> Vec<(f64, f64)> {
        let (yb, yt, w) = (self.y_bot(z), self.y_top(z), self.half_w(z)); let ya = yb.max(arch_y(z)); let h = yt - yb; let ys = yt - (0.13f64).min(h * 0.32);
        let mut p = vec![(0.0, yb), (LINER, yb), (LINER, ya)]; let mut y = (yb + 0.035).max(ya); p.push((w - 0.035, y));
        y = (yb + 0.11).max(ya + 0.025); p.push((w, y)); let y4 = lerp(yb, ys, 0.45).max(y + 0.02); p.push((w + 0.018, y4)); let y5 = (ys - 0.06).max(y4 + 0.02); p.push((w + 0.006, y5));
        p.push((w - 0.02, ys.max(y5 + 0.01))); p.push((w - 0.075, yt - 0.018)); p.push((w - 0.16, yt - 0.004)); p.push((w * 0.45, yt + 0.006)); p.push((0.0, yt + 0.014)); p }
    fn roof_y(&self, z: f64) -> f64 { if z > self.zr { lerp(self.roof, self.y_top(self.zw) - 0.005, ((z - self.zr) / (self.zw - self.zr)).powf(self.pow[0])) } else if z > self.zre { self.roof + self.roof_arc * (PI * (self.zr - z) / (self.zr - self.zre)).sin() } else { lerp(self.roof, self.y_top(self.zh) - 0.005, ((self.zre - z) / (self.zre - self.zh)).powf(self.pow[1])) } }
    fn cabin_half(&self, z: f64) -> Vec<(f64, f64)> {
        let yb = self.y_top(z) - 0.012; let yr = self.roof_y(z).max(yb + 0.002); let wb = self.half_w(z) - self.inset; let k = (yr - yb) / 0.45; let wt = wb.min(self.roof_w);
        vec![(0.0, yb - 0.04), (wb * 0.6, yb - 0.04), (wb, yb), (lerp(wb, wt, 0.55) + 0.012 * k, lerp(yb, yr, 0.55)), (wt + 0.035 * k, yr - 0.06 * k), (wt - 0.02 * k, yr - 0.012 * k), (wt * 0.5, yr + 0.004), (0.0, yr + 0.012)] }
}

// ---------------------------------------------------------------- livery atlases
const AW: f64 = 512.0; const SH: f64 = 128.0; const ZL: f64 = 4.3; const Z0: f64 = -2.15; const Y0: f64 = 0.2; const YH: f64 = 1.3; const PX: f64 = 4.0;
struct Crnd(i64);
impl Crnd { fn r(&mut self) -> f64 { self.0 = (self.0 * 16807) % 2147483647; (self.0 - 1) as f64 / 2147483646.0 } fn rr(&mut self, a: f64, b: f64) -> f64 { a + (b - a) * self.r() } }
fn paint_atlas(s: &Spec, l: &Liv, cr: &mut Crnd) -> Vec<u8> {
    let mut g = Ctx::new(AW as usize, (4.0 * SH) as usize); let c = canvas::parse;
    let side = |g: &mut Ctx, cr: &mut Crnd, row: f64, mirror: bool| {
        let x = |z: f64| (if mirror { Z0 + ZL - z } else { z - Z0 }) / ZL * AW; let y = |yy: f64| row + (1.0 - (yy - Y0) / YH) * SH;
        g.fill_style(l.base); g.fill_rect(0.0, row, AW, SH);
        let band = |g: &mut Ctx, zf: f64, zr: f64, y0f: f64, y0r: f64, th: f64, col: &str| { g.fill_style(col); g.begin_path(); g.move_to(x(zf), y(y0f)); g.line_to(x(zr), y(y0r)); g.line_to(x(zr), y(y0r + th)); g.line_to(x(zf), y(y0f + th)); g.close_path(); g.fill(); };
        let hband = |g: &mut Ctx, y0: f64, y1: f64, col: &str| { g.fill_style(col); g.fill_rect(0.0, y(y1), AW, y(y0) - y(y1)); };
        let poly = |g: &mut Ctx, pts: &[(f64, f64)], col: &str| { g.fill_style(col); g.begin_path(); for (k, &(z, yy)) in pts.iter().enumerate() { if k == 0 { g.move_to(x(z), y(yy)); } else { g.line_to(x(z), y(yy)); } } g.close_path(); g.fill(); };
        match l.pat {
            "swoosh" => { band(g, 2.2, -2.2, 0.3, 0.8, 0.16, l.a); band(g, 2.2, -2.2, 0.48, 0.98, 0.07, l.b); band(g, 2.2, -2.2, 0.57, 1.07, 0.03, l.c); hband(g, 1.03, 1.75, l.a); }
            "stripes" => { hband(g, 0.6, 0.68, l.a); hband(g, 0.56, 0.59, l.b); hband(g, 0.69, 0.72, l.b); hband(g, 0.98, 1.75, l.b); }
            "twotone" => { hband(g, 0.74, 1.75, l.a); hband(g, 0.72, 0.745, l.b); hband(g, 0.98, 1.75, l.c); }
            "classic" => { hband(g, 0.5, 0.6, l.b); hband(g, 0.48, 0.5, l.a); hband(g, 0.6, 0.62, l.a); hband(g, 0.97, 1.75, l.a); }
            "hoop" => { band(g, 1.0, 0.45, 0.3, 0.3, 0.7, l.a); band(g, 0.92, 0.38, 0.3, 0.3, 0.7, l.base); band(g, 0.88, 0.35, 0.3, 0.3, 0.7, l.a); hband(g, 0.38, 0.41, l.b); hband(g, 0.97, 1.75, "#16181c"); }
            "bolt" => { let p = [(2.3, 0.6), (1.15, 0.74), (0.75, 0.52), (-0.45, 0.74), (-0.85, 0.5), (-2.3, 0.64)];
                for k in 0..p.len() - 1 { band(g, p[k].0, p[k + 1].0, p[k].1 - 0.02, p[k + 1].1 - 0.02, 0.15, l.b); band(g, p[k].0, p[k + 1].0, p[k].1, p[k + 1].1, 0.11, l.a); } hband(g, 0.98, 1.75, l.base); }
            "checker" => { let sq = 0.13; for r in 0..2 { let rf = r as f64; let mut z = -2.3; while z < 2.3 { if (super::jr((z + 2.3) / sq) as i64 + r) % 2 == 0 { poly(g, &[(z, 0.5 + rf * sq), (z + sq, 0.5 + rf * sq), (z + sq, 0.5 + (rf + 1.0) * sq), (z, 0.5 + (rf + 1.0) * sq)], l.a); } z += sq; } }
                hband(g, 0.76, 0.79, l.b); hband(g, 0.47, 0.5, l.b); hband(g, 0.98, 1.75, l.a); }
            "flames" => { for (y0, len, col) in [(0.62, 2.4, l.c), (0.6, 1.9, l.a), (0.58, 1.3, l.b)] { for t in 0..4 { let yb = y0 - 0.2 + t as f64 * 0.12; let ln = len * (0.75 + 0.25 * ((t * 37) % 5) as f64 / 4.0);
                poly(g, &[(2.3, yb - 0.09), (2.3 - ln * 0.55, yb - 0.07), (2.3 - ln, yb + 0.03), (2.3 - ln * 0.6, yb + 0.06), (2.3 - ln * 0.8, yb + 0.12), (2.3, yb + 0.1)], col); } } hband(g, 0.98, 1.75, l.base); }
            "camo" => { for k in 0..260 { let z = cr.rr(-2.3, 2.3); let yy = cr.rr(0.15, 1.75); let w = (2.0 + (cr.r() * 4.0).floor()) * PX * 2.0; let h = (1.0 + (cr.r() * 3.0).floor()) * PX * 2.0;
                g.fill_style([l.a, l.b, l.c][k % 3]); g.fill_rect(super::jr(x(z) / (2.0 * PX)) * 2.0 * PX, super::jr(y(yy) / (2.0 * PX)) * 2.0 * PX, w, h); } }
            "split" => { poly(g, &[(1.2, 0.1), (-2.4, 0.1), (-2.4, 1.8), (-0.4, 1.8)], l.a); band(g, 1.2, -0.4, 0.1, 1.8, 0.0001, l.b); poly(g, &[(1.2, 0.1), (1.32, 0.1), (-0.28, 1.8), (-0.4, 1.8)], l.b); }
            "fade" => { let b4 = [0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0]; let mut z = -2.3; while z < 2.3 { let mut yy = 0.15; while yy < 1.75 { let t = (z + 2.3) / 4.6; let bx = super::jr(x(z) / PX) as i64; let by = super::jr(y(yy) / PX) as i64;
                if t * 1.1 - 0.05 > b4[((by & 3) * 4 + (bx & 3)) as usize] / 16.0 { g.fill_style(l.a); g.fill_rect(bx as f64 * PX, by as f64 * PX, PX, PX); } yy += 0.04; } z += 0.034; }
                for k in 0..3 { let z0 = 0.35 - k as f64 * 0.32; poly(g, &[(z0, 0.5), (z0 - 0.12, 0.5), (z0 - 0.26, 0.64), (z0 - 0.12, 0.78), (z0, 0.78), (z0 - 0.14, 0.64)], l.b); } hband(g, 0.98, 1.75, l.base); }
            "gulf" => { hband(g, 0.52, 0.78, l.a); hband(g, 0.5, 0.52, l.b); hband(g, 0.78, 0.8, l.b); }
            "zebra" => { for k in 0..14 { let z = 2.3 - k as f64 * 0.34; let w = 0.1 + (k % 3) as f64 * 0.03; poly(g, &[(z, 0.15), (z - w, 0.15), (z - w - 0.25, 1.0), (z - 0.25, 1.0)], l.a); } hband(g, 0.98, 1.75, l.a); }
            "sunset" => { let b4 = [0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0]; let mut z = -2.3; while z < 2.3 { let mut yy = 0.15; while yy < 1.0 { let t = (yy - 0.15) / 0.85; let bx = super::jr(x(z) / PX) as i64; let by = super::jr(y(yy) / PX) as i64; let d = b4[((by & 3) * 4 + (bx & 3)) as usize] / 16.0;
                g.fill_style(if t < 0.33 { if t * 3.0 > d { l.c } else { l.a } } else if t < 0.66 { if (t - 0.33) * 3.0 > d { l.b } else { l.c } } else if (t - 0.66) * 3.0 > d { l.base } else { l.b }); g.fill_rect(bx as f64 * PX, by as f64 * PX, PX, PX); yy += 0.04; } z += 0.034; }
                hband(g, 0.98, 1.75, l.base); }
            "chevron" => { for k in 0..4 { let z = 1.7 - k as f64 * 0.5; poly(g, &[(z, 0.62), (z - 0.3, 0.42), (z - 0.45, 0.42), (z - 0.15, 0.62), (z - 0.45, 0.82), (z - 0.3, 0.82)], if k % 2 == 1 { l.b } else { l.a }); } hband(g, 0.98, 1.75, l.base); }
            "dots" => { let mut z = -2.2; while z < 2.3 { let mut yy = 0.4; while yy < 0.95 { let off = if (super::jr((yy - 0.4) / 0.26) as i64) % 2 == 1 { 0.21 } else { 0.0 }; let (cx, cy) = (x(z + off), y(yy));
                g.fill_style(l.a); g.fill_rect(super::jr((cx - 12.0) / PX) * PX, super::jr((cy - 12.0) / PX) * PX, 24.0, 24.0); yy += 0.26; } z += 0.42; } hband(g, 0.98, 1.75, l.b); }
            "carbon" => { let mut bx = 0.0; while bx < AW { let mut by = row; while by < row + SH { if (((bx / (2.0 * PX)) + (by / (2.0 * PX))) as i64) % 2 == 0 { g.fill_style(l.a); g.fill_rect(bx, by, 2.0 * PX, PX); } by += 2.0 * PX; } bx += 2.0 * PX; } hband(g, 0.5, 0.56, l.b); }
            "neon" => { band(g, 2.3, -2.3, 0.45, 0.62, 0.04, l.a); band(g, 2.3, -2.3, 0.52, 0.69, 0.04, l.b); band(g, 2.3, -2.3, 0.59, 0.76, 0.02, l.a); hband(g, 0.98, 1.75, l.base); }
            "pixel" => { for k in 0..90 { let z = cr.rr(-2.3, 2.3); let yy = cr.rr(0.15, 1.0); g.fill_style([l.a, l.b, l.c][k % 3]); g.fill_rect(super::jr(x(z) / (4.0 * PX)) * 4.0 * PX, super::jr(y(yy) / (4.0 * PX)) * 4.0 * PX, 4.0 * PX, 4.0 * PX); } hband(g, 0.98, 1.75, l.c); }
            "tricolore" => { poly(g, &[(2.3, 0.15), (0.75, 0.15), (0.75, 1.75), (2.3, 1.75)], l.a); poly(g, &[(-0.75, 0.15), (-2.3, 0.15), (-2.3, 1.75), (-0.75, 1.75)], l.b); }
            "team" => { poly(g, &[(2.3, 0.15), (0.9, 0.15), (-0.2, 1.0), (2.3, 1.0)], l.a); band(g, 0.9, -0.2, 0.15, 1.0, 0.0001, l.b); poly(g, &[(0.9, 0.15), (1.08, 0.15), (-0.02, 1.0), (-0.2, 1.0)], l.b);
                poly(g, &[(-0.9, 0.15), (-2.3, 0.15), (-2.3, 1.0), (-1.6, 1.0)], l.a); hband(g, 0.98, 1.75, l.a); hband(g, 0.4, 0.44, l.b); }
            _ => {}
        }
        g.fill_style("#111"); g.fill_rect(0.0, y(0.37), AW, y(0.15) - y(0.37)); g.fill_style(l.b); g.fill_rect(0.0, y(0.385), AW, 1.0);
        g.fill_style("rgba(24,24,24,0.6)"); for z in [s.zw - 0.04, s.z_b.unwrap_or(s.zre + 0.08)] { g.fill_rect(x(z) as i64 as f64, y(0.97), 1.0, y(0.36) - y(0.97)); } g.fill_rect(x(lerp(s.zw, s.zre, 0.7)) as i64 as f64, y(0.86) as i64 as f64, 6.0, 1.0);
        for _ in 0..150 { let z = cr.rr(-2.15, 2.15); let near = if (z - WB_F).abs().min((z - WB_R).abs()) < 0.6 { 1.0 } else { 0.15 }; if cr.r() > near { continue; }
            let yy = 0.26 + cr.r().powf(2.2) * 0.3 * (0.5 + near); g.fill_style(if cr.r() < 0.55 { "#7a6248" } else { "#5e4a36" }); g.fill_rect((x(z) / PX).floor() * PX, (y(yy) / PX).floor() * PX, PX, PX); }
    };
    side(&mut g, cr, 0.0, false); side(&mut g, cr, SH, true);
    let tx = |z: f64| (z - Z0) / ZL * AW; let ty = |xx: f64| 2.0 * SH + (xx + 1.1) / 2.2 * SH;
    let top = |g: &mut Ctx, z0: f64, z1: f64, x0: f64, x1: f64, col: &str| { g.fill_style(col); g.fill_rect(tx(z0), ty(x0), tx(z1) - tx(z0), ty(x1) - ty(x0)); };
    g.fill_style(l.base); g.fill_rect(0.0, 2.0 * SH, AW, SH);
    let roof_col = match l.pat { "twotone" => l.a, "hoop" => "#16181c", "classic" => l.a, "stripes" => l.base, _ => l.a };
    top(&mut g, s.zh - 0.1, s.zw - 0.06, -1.1, 1.1, roof_col);
    match l.pat {
        "swoosh" => { top(&mut g, s.zw - 0.1, 2.3, -0.27, 0.27, l.c); top(&mut g, s.zw - 0.1, 2.3, -0.24, 0.24, l.b); top(&mut g, s.zw - 0.1, 2.3, -0.17, 0.17, l.a); }
        "stripes" => { for sg in [-1.0, 1.0] { top(&mut g, -2.3, 2.3, sg * 0.1, sg * 0.22, l.a); top(&mut g, -2.3, 2.3, sg * 0.22, sg * 0.25, l.b); } }
        "twotone" => top(&mut g, s.zw - 0.1, 2.3, -0.03, 0.03, l.b),
        "classic" => { top(&mut g, s.zw + 0.1, 2.3, -1.1, 1.1, l.a); top(&mut g, s.zw + 0.1, 2.3, -0.06, 0.06, l.b); }
        "hoop" => top(&mut g, -2.3, 2.3, -0.04, 0.04, l.a),
        "bolt" => { top(&mut g, -2.3, 2.3, -0.1, 0.1, l.a); top(&mut g, -2.3, 2.3, -0.13, -0.1, l.b); top(&mut g, -2.3, 2.3, 0.1, 0.13, l.b); }
        "checker" => { let sq = 0.13; let mut z = s.zh - 0.1; while z < s.zw { let mut xx = -0.65; while xx < 0.65 { if (super::jr((z + 3.0) / sq) as i64 + super::jr((xx + 1.0) / sq) as i64) % 2 == 0 { top(&mut g, z, z + sq, xx, xx + sq, l.a); } xx += sq; } z += sq; } }
        "flames" => { for (w, len, col) in [(0.5, 1.5, l.c), (0.38, 1.15, l.a), (0.24, 0.8, l.b)] { g.fill_style(col); g.begin_path(); g.move_to(tx(2.3), ty(-w));
            for k in 0..=6 { let xx = -w + 2.0 * w * k as f64 / 6.0; let ln = len * if k % 2 == 1 { 0.7 } else { 1.0 }; g.line_to(tx(2.3 - ln), ty(xx)); } g.line_to(tx(2.3), ty(w)); g.close_path(); g.fill(); } }
        "camo" => { for k in 0..120 { let z = cr.rr(-2.3, 2.3); let xx = cr.rr(-1.1, 1.1); let w = (2.0 + (cr.r() * 4.0).floor()) * PX * 2.0; let h = (1.0 + (cr.r() * 3.0).floor()) * PX * 2.0;
            g.fill_style([l.a, l.b, l.c][k % 3]); g.fill_rect(super::jr(tx(z) / (2.0 * PX)) * 2.0 * PX, super::jr(ty(xx) / (2.0 * PX)) * 2.0 * PX, w, h); } }
        "split" => { top(&mut g, -2.3, 0.4, -1.1, 1.1, l.a); top(&mut g, 0.4, 0.46, -1.1, 1.1, l.b); }
        "fade" => top(&mut g, s.zw - 0.1, 2.3, -1.1, 1.1, l.a),
        "gulf" => { top(&mut g, -2.3, 2.3, -0.2, 0.2, l.a); top(&mut g, -2.3, 2.3, -0.22, -0.2, l.b); top(&mut g, -2.3, 2.3, 0.2, 0.22, l.b); }
        "zebra" => { for k in 0..14 { let z = 2.3 - k as f64 * 0.34; top(&mut g, z - 0.12, z, -1.1, 1.1, l.a); } }
        "sunset" | "neon" | "carbon" => { top(&mut g, -2.3, 2.3, -0.12, 0.12, l.a); top(&mut g, -2.3, 2.3, -0.16, -0.12, l.b); top(&mut g, -2.3, 2.3, 0.12, 0.16, l.b); }
        "chevron" => { for k in 0..4 { let z = 1.7 - k as f64 * 0.5; top(&mut g, z - 0.15, z, -0.5, 0.5, if k % 2 == 1 { l.b } else { l.a }); } }
        "dots" => { let mut z = -2.2; while z < 2.3 { for xx in [-0.6, 0.0, 0.6] { top(&mut g, z, z + 0.16, xx - 0.08, xx + 0.08, l.a); } z += 0.42; } }
        "pixel" => { for k in 0..40 { let z = cr.rr(-2.3, 2.3); let xx = cr.rr(-1.1, 1.1); g.fill_style([l.a, l.b, l.c][k % 3]); g.fill_rect(super::jr(tx(z) / (4.0 * PX)) * 4.0 * PX, super::jr(ty(xx) / (4.0 * PX)) * 4.0 * PX, 4.0 * PX, 4.0 * PX); } }
        "tricolore" => { top(&mut g, 0.75, 2.3, -1.1, 1.1, l.a); top(&mut g, -2.3, -0.75, -1.1, 1.1, l.b); }
        "team" => { top(&mut g, -2.3, 2.3, -1.1, 1.1, l.a); top(&mut g, -2.3, 2.3, -0.18, 0.18, l.b); top(&mut g, -2.3, 2.3, -0.22, -0.18, l.c); top(&mut g, -2.3, 2.3, 0.18, 0.22, l.c); }
        _ => {}
    }
    g.fill_style("#111"); g.fill_rect(tx(s.zh - 0.2), 2.0 * SH, 1.0, SH);
    let fx = |xx: f64, rear: bool| (if rear { 0.5 + (0.55 - xx / 2.2) * 0.5 } else { (xx / 2.2 + 0.5) * 0.5 }) * AW; let fy = |yy: f64| 3.0 * SH + (1.0 - (yy - Y0) / YH) * SH;
    g.fill_style(l.base); g.fill_rect(0.0, 3.0 * SH, AW, SH);
    match l.pat { "swoosh" => { g.fill_style(l.a); g.fill_rect(fx(-0.17, false), fy(1.2), fx(0.17, false) - fx(-0.17, false), fy(0.62) - fy(1.2)); }
        "stripes" => { for sg in [-1.0, 1.0] { g.fill_style(l.a); g.fill_rect(fx(if sg > 0.0 { 0.1 } else { -0.22 }, false), fy(1.2), fx(0.12, false) - fx(0.0, false), fy(0.3) - fy(1.2)); g.fill_rect(fx(if sg > 0.0 { -0.22 } else { 0.1 }, true), fy(1.2), fx(0.12, false) - fx(0.0, false), fy(0.3) - fy(1.2)); } }
        "twotone" => { g.fill_style(l.a); g.fill_rect(0.0, fy(1.75), AW, fy(0.74) - fy(1.75)); }
        "classic" => { g.fill_style(l.a); g.fill_rect(0.0, fy(1.75), AW / 2.0, fy(0.66) - fy(1.75)); }
        _ => {} }
    g.fill_style("#111"); g.fill_rect(0.0, fy(0.42), AW, fy(0.1) - fy(0.42));
    g.fill_style(if l.pat == "hoop" { "#16181c" } else if l.pat == "stripes" { l.b } else { l.a }); g.fill_rect(0.0, fy(1.75), AW, fy(1.0) - fy(1.75));
    g.fill_style(l.a); g.fill_rect(AW / 2.0, fy(0.7), AW / 2.0, fy(0.5) - fy(0.7)); g.fill_style(l.b); g.fill_rect(AW / 2.0, fy(0.72), AW / 2.0, 3.0);
    let fr = |g: &mut Ctx, x0: f64, x1: f64, y0: f64, y1: f64, col: &str, rear: bool| { g.fill_style(col); let (a, b) = (fx(x0, rear), fx(x1, rear)); g.fill_rect(a.min(b), fy(y1), (b - a).abs(), fy(y0) - fy(y1)); };
    match l.pat {
        "bolt" => { for r in [false, true] { fr(&mut g, -0.1, 0.1, 0.3, 1.3, l.a, r); } }
        "checker" => { for r in [false, true] { let mut xx = -1.1; let mut k = 0; while xx < 1.1 { for q in 0..2 { if (k + q) % 2 == 0 { fr(&mut g, xx, xx + 0.13, 0.5 + q as f64 * 0.13, 0.63 + q as f64 * 0.13, l.a, r); } } xx += 0.13; k += 1; } } }
        "flames" => { fr(&mut g, -0.5, 0.5, 0.4, 0.75, l.c, false); fr(&mut g, -0.35, 0.35, 0.45, 0.72, l.a, false); fr(&mut g, -0.2, 0.2, 0.5, 0.68, l.b, false); }
        "camo" => { for k in 0..60 { g.fill_style([l.a, l.b, l.c][k % 3]); let a = cr.rr(0.0, AW); let b = cr.rr(fy(1.75), fy(0.2)); let w = (2.0 + (cr.r() * 4.0).floor()) * PX * 2.0; let h = (1.0 + (cr.r() * 3.0).floor()) * PX * 2.0;
            g.fill_rect(super::jr(a / (2.0 * PX)) * 2.0 * PX, super::jr(b / (2.0 * PX)) * 2.0 * PX, w, h); } }
        "split" => fr(&mut g, -1.1, 1.1, 0.2, 1.75, l.a, true),
        "fade" => fr(&mut g, -1.1, 1.1, 0.2, 1.75, l.a, false),
        "gulf" => { for r in [false, true] { fr(&mut g, -0.2, 0.2, 0.3, 1.3, l.a, r); } }
        "zebra" => { for r in [false, true] { for k in 0..5 { fr(&mut g, -1.1 + k as f64 * 0.45, -0.95 + k as f64 * 0.45, 0.2, 1.3, l.a, r); } } }
        "sunset" | "neon" | "carbon" => { for r in [false, true] { fr(&mut g, -0.12, 0.12, 0.3, 1.3, l.a, r); } }
        "chevron" | "dots" => { for r in [false, true] { fr(&mut g, -0.5, 0.5, 0.5, 0.65, l.a, r); } }
        "tricolore" => { fr(&mut g, -1.1, -0.37, 0.2, 1.75, l.a, false); fr(&mut g, 0.37, 1.1, 0.2, 1.75, l.b, false); fr(&mut g, -1.1, -0.37, 0.2, 1.75, l.a, true); fr(&mut g, 0.37, 1.1, 0.2, 1.75, l.b, true); }
        "team" => { for r in [false, true] { fr(&mut g, -1.1, 1.1, 0.2, 1.75, l.a, r); fr(&mut g, -0.2, 0.2, 0.2, 1.75, l.b, r); } }
        _ => {}
    }
    let _ = c;
    // pixelate 4x (most common colour of each block), then the old-console shading
    let big = g.data(); let (w, h) = (128usize, 128usize); let bw = AW as usize; let mut out = vec![0u8; w * h * 4];
    for y in 0..h { for x in 0..w { let mut keys = [0u32; 16]; let mut cnt = [0u32; 16]; let mut nk = 0; let (mut best, mut bk) = (0u32, 0u32);
        for j in 0..4 { for i in 0..4 { let p = ((y * 4 + j) * bw + x * 4 + i) * 4; let key = (big[p] as u32) << 16 | (big[p + 1] as u32) << 8 | big[p + 2] as u32;
            let mut f = nk; for q in 0..nk { if keys[q] == key { f = q; break; } } if f == nk { keys[nk] = key; cnt[nk] = 0; nk += 1; } cnt[f] += 1; if cnt[f] > best { best = cnt[f]; bk = key; } } }
        let q = (y * w + x) * 4; out[q] = (bk >> 16) as u8; out[q + 1] = (bk >> 8) as u8; out[q + 2] = bk as u8; out[q + 3] = 255; } }
    let bay = [0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0]; let row_h = 32usize;
    for y in 0..h { for x in 0..w { let q = (y * w + x) * 4; let row = y / row_h; let fy = (y % row_h) as f64 / row_h as f64; let th = (bay[(y & 3) * 4 + (x & 3)] + 0.5) / 16.0;
        let mut lv = 0.0; if row < 2 { let t = 1.0 - fy; lv = if t > 0.62 { 1.0 } else { 0.0 }; let b = if fy > 0.62 { (fy - 0.62) / 0.3 } else { 0.0 }; if th < b { lv = -1.0; } else if t > 0.5 && th < (t - 0.5) * 6.0 && lv == 0.0 { lv = 1.0; } }
        else if row == 3 { lv = if fy > 0.7 && th < (fy - 0.7) / 0.25 { -1.0 } else { 0.0 }; }
        let k = 1.0 + lv * 0.14; let (r, gg, b) = (out[q] as f64, out[q + 1] as f64, out[q + 2] as f64); let grey = (r + gg + b) / 3.0;
        for (i, cc) in [(0, r), (1, gg), (2, b)] { out[q + i] = ((cc * 0.86 + grey * 0.1 + 6.0) * k).clamp(0.0, 255.0) as u8; } } }
    out
}
static ATLASES: OnceLock<Vec<Vec<Vec<u8>>>> = OnceLock::new();
/// every body's 22 atlases (its own livery, the 15 pickable ones, the 6 team ones); painted once, the bodies on parallel threads
pub fn atlases() -> &'static Vec<Vec<Vec<u8>>> {
    ATLASES.get_or_init(|| {
        let sp = specs(); let lv = liveries();
        let body = |k: usize| -> Vec<Vec<u8>> { let s = &sp[k]; let mut cr = Crnd(4242 + k as i64 * 977); let mut v = vec![paint_atlas(s, &s.liv, &mut cr)]; for l in &lv { v.push(paint_atlas(s, l, &mut cr)); } v };
        #[cfg(not(target_arch = "wasm32"))]
        { std::thread::scope(|sc| { let hs: Vec<_> = (0..sp.len()).map(|k| { let b = &body; sc.spawn(move || b(k)) }).collect(); hs.into_iter().map(|h| h.join().unwrap_or_default()).collect() }) }
        #[cfg(target_arch = "wasm32")]
        { (0..sp.len()).map(body).collect() }
    })
}

// ---------------------------------------------------------------- bodies
#[derive(Clone, Copy, PartialEq)] enum Cls { Paint, Black, Tail, Glass }
struct Mats { paint: MatId, black: MatId, tail: MatId, glass: MatId }
fn loft(st: &mut St, b: ObjId, zs: &[f64], half: &dyn Fn(f64) -> Vec<(f64, f64)>, classify: &dyn Fn([f64; 3], [f64; 3]) -> Option<Cls>, cap_front: Option<bool>, cap_back: Option<bool>, group: &dyn Fn(usize) -> i64, m: &Mats, paint_uv: &dyn Fn([f64; 3], [f64; 3]) -> [f64; 2]) {
    let fine = |h: Vec<(f64, f64)>| { let mut o = vec![h[0]]; for i in 1..h.len() { o.push(((h[i - 1].0 + h[i].0) / 2.0, (h[i - 1].1 + h[i].1) / 2.0)); o.push(h[i]); } o };
    let rings: Vec<Vec<[f64; 3]>> = zs.iter().map(|&z| { let h = fine(half(z)); let mut r: Vec<[f64; 3]> = h.iter().map(|&(x, y)| [x, y, z]).collect(); for i in (1..h.len() - 1).rev() { r.push([-h[i].0, h[i].1, z]); } r }).collect();
    let n = rings[0].len(); let mm = (n + 2) / 2; let hseg = |j: usize| (if j < mm - 1 { j } else { n - 1 - j }) >> 1;
    let mut p: Vec<[f64; 3]> = rings.iter().flatten().cloned().collect(); let mut tris: Vec<[i64; 4]> = Vec::new();
    for s in 0..rings.len() - 1 { for i in 0..n { let a = s * n + i; let bb = s * n + (i + 1) % n; let c = (s + 1) * n + i; let d = (s + 1) * n + (i + 1) % n; let gr = group(hseg(i));
        tris.push([a as i64, bb as i64, c as i64, gr]); tris.push([bb as i64, d as i64, c as i64, gr]); } }
    for (s, back) in [(0usize, cap_back), (rings.len() - 1, cap_front)] { let Some(back) = back else { continue }; let r0 = &rings[s]; let mut c = [0.0; 3]; for v in r0 { for k in 0..3 { c[k] += v[k] / n as f64; } }
        let mut prev: Vec<usize> = (0..n).map(|i| s * n + i).collect(); let kk = 4;
        for k in 1..=kk { let f = 1.0 - k as f64 / kk as f64; let mut cur = Vec::new();
            if k == kk { cur.push(p.len()); p.push(c); } else { for v in r0 { cur.push(p.len()); p.push([c[0] + (v[0] - c[0]) * f, c[1] + (v[1] - c[1]) * f, c[2] + (if back { -0.004 } else { 0.004 }) * k as f64]); } }
            for i in 0..n { let (a, bb) = (prev[i], prev[(i + 1) % n]); let q = cur[if k == kk { 0 } else { i }]; let d = cur[if k == kk { 0 } else { (i + 1) % n }];
                tris.push(if back { [a as i64, q as i64, bb as i64, -1] } else { [a as i64, bb as i64, q as i64, -1] }); if k < kk { tris.push(if back { [bb as i64, q as i64, d as i64, -1] } else { [bb as i64, d as i64, q as i64, -1] }); } }
            prev = cur; } }
    let fnorm = |t: &[i64; 4]| { let (a, b2, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]); let u = [b2[0] - a[0], b2[1] - a[1], b2[2] - a[2]]; let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]] };
    let mut nm: std::collections::HashMap<i64, [f64; 3]> = Default::default();
    for t in &tris { if t[3] < 0 { continue; } let f = fnorm(t); for k in 0..3 { let e = nm.entry(t[k] * 16 + t[3]).or_insert([0.0; 3]); e[0] += f[0]; e[1] += f[1]; e[2] += f[2]; } }
    let nrm = |v: [f64; 3]| { let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt(); let l = if l == 0.0 { 1.0 } else { l }; [v[0] / l, v[1] / l, v[2] / l] };
    let mut order: Vec<Cls> = Vec::new(); let mut outs: Vec<(Vec<f32>, Vec<f32>, Vec<f32>)> = Vec::new();
    for t in &tris { let f = nrm(fnorm(t)); let cen = [0, 1, 2].map(|k| (p[t[0] as usize][k] + p[t[1] as usize][k] + p[t[2] as usize][k]) / 3.0); let Some(r) = classify(cen, f) else { continue };
        let oi = match order.iter().position(|&o| o == r) { Some(i) => i, None => { order.push(r); outs.push(Default::default()); order.len() - 1 } };
        for k in 0..3 { let v = p[t[k] as usize]; let nn = if t[3] < 0 { f } else { nrm(nm[&(t[k] * 16 + t[3])]) }; let o = &mut outs[oi];
            o.0.extend(v.iter().map(|&q| q as f32)); o.1.extend(nn.iter().map(|&q| q as f32)); let uv = if r == Cls::Paint { paint_uv(v, f) } else { [0.0, 0.0] }; o.2.extend_from_slice(&[uv[0] as f32, uv[1] as f32]); } }
    for (i, r) in order.iter().enumerate() { let (pp, nn, uu) = std::mem::take(&mut outs[i]); let mat = match r { Cls::Paint => m.paint, Cls::Black => m.black, Cls::Tail => m.tail, Cls::Glass => m.glass };
        let o = st.mesh(Geo::new(pp).with_nrm(nn).with_uv(uu), mat); st.sc.add_to(b, o); }
}
fn add_mesh(st: &mut St, b: ObjId, g: Geo, m: MatId, pos: (f64, f64, f64), rot: (f64, f64, f64)) -> ObjId { let o = st.mesh(g, m); st.sc.set_pos(o, pos.0, pos.1, pos.2); st.sc.set_rot(o, rot.0, rot.1, rot.2); st.sc.add_to(b, o); o }
/// extrude(pts, depth, mat, bevel): the side profile (z, y) extruded across the car
fn ext(st: &mut St, b: ObjId, pts: &[(f64, f64)], depth: f64, mat: MatId, bevel: f64, x: f64) -> ObjId {
    let mut g = extrude(pts, depth - bevel * 2.0, bevel); g.translate(0.0, 0.0, -(depth - bevel * 2.0) / 2.0); g.rotate_y(-PI / 2.0); add_mesh(st, b, g, mat, (x, 0.0, 0.0), (0.0, 0.0, 0.0))
}

struct Common { black: MatId, trim: MatId, dark: MatId, seat: MatId, suit: MatId, glass: MatId, lamp_on: MatId, tail: MatId, fog_lamp: MatId, mirror: MatId, plate: MatId, flap: MatId }
fn phong(m: Mat) -> Mat { let mut p = Mat::phong(); p.shin = 14.0; p.spec = Some(Col::hex(0x1e1e1e)); p.color = m.color; p.map = m.map; p }

fn build_body(st: &mut St, s: &Spec, k: usize, cm: &Common, atl: &[Vec<u8>]) -> ObjId {
    let b = st.sc.group(); st.sc.objs[b].model = Some(k);
    let texs: Vec<TexId> = atl.iter().map(|d| { let mut t = Tex::canvas(128, 128, d.clone(), false); t.mag = NEAREST; t.min = NEAREST; t.mip = false; st.tex(t) }).collect();
    let mut pm = phong(Mat::phong().map(texs[0])); pm.paint = true; let paint = st.mat(pm);
    { let cg = st.geo(Geo::new(vec![0.0, 0.5, 0.0, 0.0, 0.5, 0.0, 0.0, 0.5, 0.0]).with_nrm(vec![0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0]).with_uv(vec![0.0; 6]));
        let mut carriers = Vec::new(); for &t in &texs[1..] { let m = st.mat(phong(Mat::phong().map(t))); let o = st.sc.mesh(cg, m); st.sc.objs[o].liv_carrier = true; st.sc.add_to(b, o); carriers.push(o); }
        st.sc.objs[b].liveries = carriers; }
    let ah = 4.0 * SH; let v = |row: f64| 1.0 - row / ah;
    let uv_side = |p: [f64; 3]| { let mir = p[0] > 0.0; let u = (if mir { Z0 + ZL - p[2] } else { p[2] - Z0 }) / ZL; [u, v((if mir { SH } else { 0.0 }) + (1.0 - (p[1] - Y0) / YH) * SH)] };
    let uv_top = |p: [f64; 3]| [(p[2] - Z0) / ZL, v(2.0 * SH + (p[0] + 1.1) / 2.2 * SH)];
    let uv_front = |p: [f64; 3], rear: bool| [if rear { 0.5 + (0.55 - p[0] / 2.2) * 0.5 } else { (p[0] / 2.2 + 0.5) * 0.5 }, v(3.0 * SH + (1.0 - (p[1] - Y0) / YH) * SH)];
    let paint_uv = |p: [f64; 3], n: [f64; 3]| if n[1] > 0.62 { uv_top(p) } else if n[2].abs() > n[0].abs() * 1.15 { uv_front(p, n[2] < 0.0) } else { uv_side(p) };
    let mats = Mats { paint, black: cm.black, tail: cm.tail, glass: cm.glass };
    let stations = |a: f64, bb: f64, n: usize| (0..=n).map(|i| lerp(a, bb, i as f64 / n as f64)).collect::<Vec<f64>>();
    let mut zs_low = stations(s.zt, s.zt + 0.32, 4); zs_low.extend(stations(s.zt + 0.32, s.zn - 0.4, 14).into_iter().skip(1)); zs_low.extend(stations(s.zn - 0.4, s.zn, 6).into_iter().skip(1));
    loft(st, b, &zs_low, &|z| s.lower_half(z), &|c, n| { let (x, y, z) = (c[0], c[1], c[2]); let ax = x.abs();
        if n[1] < -0.45 { return Some(Cls::Black); } if ax < LINER + 0.03 && n[0].abs() > 0.6 { return Some(Cls::Black); }
        if arch_y(z) > 0.0 && y < arch_y(z) + 0.075 && ax > s.half_w(z) - 0.08 { return Some(Cls::Black); }
        if z > s.zn - 0.42 && n[2] > 0.1 { let yt = s.y_top(z);
            if s.lights == Lights::Popup && y > yt - 0.1 && y < yt - 0.03 && ax < 0.58 && n[2] > 0.5 { return Some(Cls::Black); }
            if y < yt - 0.14 && y > 0.39 && ax < 0.42 && n[2] > 0.6 { return Some(Cls::Black); } if y < 0.4 && n[2] > 0.5 && ax < 0.62 { return Some(Cls::Black); } }
        if z < s.zt + 0.32 && n[2] < -0.1 { let yt = s.y_top(z);
            if y > yt - 0.19 && y < yt - 0.03 && ax > 0.38 && ax < 0.82 && n[2] < -0.3 { return Some(if y > yt - 0.16 && y < yt - 0.06 && ax > 0.44 { Cls::Tail } else { Cls::Black }); }
            if y < 0.47 && n[2] < -0.5 { return Some(Cls::Black); } }
        if s.bonnet_vents && n[1] > 0.8 && z > s.zw + 0.25 && z < s.zw + 0.55 && ax > 0.25 && ax < 0.45 { return Some(Cls::Black); }
        if s.louvres && n[1] > 0.6 && z < s.zh - 0.02 && z > s.zt + 0.2 && ax < 0.55 && ((z / 0.06).floor() as i64) % 2 == 0 { return Some(Cls::Black); }
        if y < s.y_bot(z) + 0.1 { return Some(Cls::Black); } Some(Cls::Paint)
    }, Some(false), Some(true), &|sg| if sg <= 1 { 0 } else if sg <= 4 { 1 } else if sg <= 7 { 2 } else { 3 }, &mats, &paint_uv);
    loft(st, b, &stations(s.zh, s.zw, 12), &|z| s.cabin_half(z), &|c, n| { let (x, y, z) = (c[0], c[1], c[2]); let _ = x;
        if n[1] < -0.3 { return None; } if n[1] > 0.8 && z < s.zr + 0.05 && z > s.zre - 0.05 { return Some(Cls::Paint); }
        if z > s.zr - 0.02 { return Some(if n[0].abs() > 0.42 { Cls::Paint } else { Cls::Glass }); }
        if z < s.zre + 0.02 { if n[0].abs() > 0.42 { return Some(Cls::Paint); } if s.louvres { return Some(if ((z / 0.06).floor() as i64) % 2 == 0 { Cls::Black } else { Cls::Paint }); } return Some(Cls::Glass); }
        if let Some(zb) = s.z_b { if (z - zb).abs() < 0.07 { return Some(Cls::Paint); } } if let Some(tc) = s.thick_c { if z < tc { return Some(Cls::Paint); } }
        if y > s.roof_y(z) - 0.03 { return Some(Cls::Paint); } Some(Cls::Glass)
    }, None, None, &|sg| if sg <= 1 { 0 } else if sg <= 3 { 1 } else { 2 }, &mats, &paint_uv);
    let l = &s.liv;
    let accent = st.mat(phong(Mat::phong().col(Col::css(if l.pat == "hoop" { l.a } else { l.b })))); let wing_paint = st.mat(phong(Mat::phong().col(Col::css(match l.pat { "swoosh" | "twotone" => l.a, "hoop" => "#16181c", _ => l.base }))));
    let black = cm.black;
    if let Some(w) = s.wing { let mut foil = Vec::new();
        for i in 0..=10 { let t = i as f64 / 10.0; let th = 0.06 * (1.0 - t) * (t + 0.02).sqrt() * 2.2; foil.push((w.z - t * w.chord, w.y + th - 0.06 * t)); }
        for i in (0..=10).rev() { let t = i as f64 / 10.0; foil.push((w.z - t * w.chord, w.y - 0.012 - 0.06 * t)); }
        ext(st, b, &foil, w.span, wing_paint, 0.0, 0.0);
        for sg in [-1.0, 1.0] { ext(st, b, &[(w.z + 0.06, w.y - 0.08), (w.z + 0.06, w.y + 0.08), (w.z - w.chord - 0.08, w.y + 0.04), (w.z - w.chord - 0.08, w.y - 0.12)], 0.025, accent, 0.0, sg * w.span / 2.0);
            let zc = w.z - w.chord * 0.45;
            if w.swan { ext(st, b, &[(zc + 0.04, w.base), (zc - 0.04, w.base), (zc - 0.06, w.y + 0.07), (zc + 0.02, w.y + 0.07)], 0.03, black, 0.0, sg * w.stay); }
            else { ext(st, b, &[(zc + 0.04, w.base), (zc - 0.04, w.base), (zc, w.y - 0.02), (zc + 0.08, w.y - 0.02)], 0.035, black, 0.0, sg * w.stay); } } }
    else { let yt = s.y_top(s.zt + 0.15); ext(st, b, &[(s.zt + 0.28, yt + 0.01), (s.zt + 0.02, yt + 0.07), (s.zt - 0.0, yt + 0.04), (s.zt + 0.2, yt - 0.01)], 1.5, black, 0.0, 0.0); }
    if s.scoop { ext(st, b, &[(s.zr - 0.02, s.roof + 0.008), (s.zr - 0.04, s.roof + 0.07), (s.zr - 0.38, s.roof + 0.07), (s.zr - 0.44, s.roof + 0.008)], 0.34, wing_paint, 0.02, 0.0);
        add_mesh(st, b, box_geo(0.28, 0.045, 0.01), black, (0.0, s.roof + 0.045, s.zr - 0.02), (0.0, 0.0, 0.0)); }
    add_mesh(st, b, cylinder(0.006, 0.006, 0.5, 4, 1, false), black, (-0.4, s.roof + 0.24, lerp(s.zr, s.zre, 0.75)), (0.0, 0.0, 0.0));
    let zm = s.zw - 0.14; let xm = s.half_w(zm) - 0.02; let ym = s.y_top(zm) + 0.05;
    for sg in [-1.0, 1.0] { ext(st, b, &[(zm + 0.08, ym - 0.04), (zm - 0.1, ym - 0.04), (zm - 0.14, ym + 0.06), (zm + 0.04, ym + 0.08)], 0.2, accent, 0.015, sg * xm);
        add_mesh(st, b, plane(0.16, 0.08, 1, 1), cm.mirror, (sg * xm, ym + 0.015, zm - 0.145), (0.0, PI, 0.0)); }
    let yn = s.y_top(s.nose[0]);
    let lens_m = st.mat(Mat::basic().hex(0xe6dfc6)); let recess_m = st.mat(Mat::lambert().hex(0x141414));
    let surf_z = |x: f64, y: f64| { let mut z = s.zn; while z > s.zn - 0.8 { if s.y_top(z) >= y && s.y_bot(z) <= y && s.half_w(z) >= x.abs() { return z; } z -= 0.004; } s.zn - 0.8 };
    let lamp = |st: &mut St, x: f64, y: f64, w: f64, h: f64| { let z = surf_z(x, y); let e = 0.03;
        let n = v3(-(surf_z(x + e, y) - surf_z(x - e, y)) / (2.0 * e), -(surf_z(x, y + e) - surf_z(x, y - e)) / (2.0 * e), 1.0).norm();
        for (m, sw, sh, off) in [(recess_m, w + 0.04, h + 0.04, 0.012), (lens_m, w, h, 0.018)] { let o = st.mesh(plane(sw, sh, 1, 1), m); let pos = v3(x, y, z).add(n.mul(off)); st.sc.objs[o].pos = pos;
            let rm = M4::look_at(pos.add(n), pos, v3(0.0, 1.0, 0.0)); st.sc.objs[o].quat = Quat::from_rot(&rm); st.sc.add_to(b, o); } };
    if matches!(s.lights, Lights::Swept | Lights::Rect | Lights::Strip) { let (w, h, dy) = match s.lights { Lights::Strip => (0.36, 0.035, 0.05), Lights::Rect => (0.26, 0.08, 0.09), _ => (0.26, 0.065, 0.075) };
        for sx in [-1.0, 1.0] { lamp(st, sx * 0.46, s.y_top(s.zn - 0.03) - dy, w, h); } }
    if s.lights == Lights::Popup { for sg in [-1.0, 1.0] { let pz = s.nose[0] - 0.12; let py = s.y_top(pz);
        ext(st, b, &[(pz + 0.12, py - 0.02), (pz + 0.1, py + 0.09), (pz - 0.1, py + 0.07), (pz - 0.12, py - 0.02)], 0.36, black, 0.0, sg * 0.5);
        add_mesh(st, b, plane(0.3, 0.07, 1, 1), cm.lamp_on, (sg * 0.5, py + 0.035, pz + 0.115), (-0.18, 0.0, 0.0)); } }
    if s.lights == Lights::Round { for sg in [-1.0, 1.0] { let ly = s.y_top(s.zn) - 0.09; add_mesh(st, b, circle(0.105, 14), black, (sg * 0.44, ly, s.zn + 0.02), (0.0, 0.0, 0.0)); add_mesh(st, b, circle(0.08, 14), cm.lamp_on, (sg * 0.44, ly, s.zn + 0.024), (0.0, 0.0, 0.0)); } }
    if s.pods { let zp = if s.lights == Lights::Popup { s.nose[0] - 0.38 } else { s.zn - 0.12 }; let yp = (if s.lights == Lights::Popup { s.y_top(zp) } else { yn }) + 0.1;
        add_mesh(st, b, box_geo(1.2, 0.05, 0.05), black, (0.0, yp - 0.07, zp), (0.0, 0.0, 0.0));
        for x in [-0.45, -0.15, 0.15, 0.45] { add_mesh(st, b, cylinder(0.1, 0.11, 0.11, 12, 1, false), black, (x, yp, zp), (PI / 2.0, 0.0, 0.0)); add_mesh(st, b, circle(0.083, 12), cm.lamp_on, (x, yp, zp + 0.057), (0.0, 0.0, 0.0)); } }
    if s.splitter { add_mesh(st, b, box_geo(1.5, 0.03, 0.22), black, (0.0, s.y_bot(s.zn) + 0.01, s.zn - 0.04), (0.0, 0.0, 0.0));
        for sg in [-1.0, 1.0] { ext(st, b, &[(s.zn - 0.05, 0.42), (s.zn - 0.3, 0.42), (s.zn - 0.3, 0.45), (s.zn - 0.1, 0.46)], 0.12, black, 0.0, sg * (s.half_w(s.zn - 0.2) - 0.02)); } }
    let plate_g = st.geo(box_geo(0.42, 0.1, 0.02)); let pl = st.sc.mesh(plate_g, cm.plate); st.sc.set_pos(pl, 0.0, 0.44, s.zn - 0.005); st.sc.add_to(b, pl);
    if s.lights != Lights::Round { for x in [-0.56, 0.56] { add_mesh(st, b, circle(0.055, 10), cm.fog_lamp, (x, 0.44, s.zn - 0.1), (0.0, 0.0, 0.0)); } }
    add_mesh(st, b, box_geo(0.06, 0.12, 0.02), accent, (0.46, 0.33, s.zn - 0.01), (0.0, 0.0, 0.0));
    let rp = st.sc.mesh(plate_g, cm.plate); st.sc.set_pos(rp, 0.0, 0.62, s.zt + 0.005); st.sc.add_to(b, rp);
    add_mesh(st, b, cylinder(0.06, 0.06, 0.2, 10, 1, false), cm.trim, (0.5, 0.33, s.zt + 0.02), (PI / 2.0, 0.0, 0.0));
    add_mesh(st, b, circle(0.042, 10), black, (0.5, 0.33, s.zt - 0.081), (0.0, PI, 0.0));
    for sg in [-1.0, 1.0] { for z in [WB_F - 0.46, WB_R - 0.47] { add_mesh(st, b, box_geo(0.26, 0.2, 0.02), cm.flap, (sg * 0.75, 0.2, z), (0.0, 0.0, 0.0)); } }
    let zc = lerp(s.zr, s.zre, 0.42); let hy = s.roof - 0.155;
    add_mesh(st, b, box_geo(1.2, 0.14, 0.3), cm.dark, (0.0, s.y_top(s.zw) + 0.02, s.zw - 0.36), (0.0, 0.0, 0.0));
    for x in [0.32, -0.32] { add_mesh(st, b, box_geo(0.42, 0.56, 0.09), cm.seat, (x, hy - 0.17, zc - 0.21), (-0.14, 0.0, 0.0)); add_mesh(st, b, box_geo(0.38, 0.38, 0.28), cm.suit, (x, hy - 0.21, zc - 0.01), (0.0, 0.0, 0.0));
        let hm = st.mat(phong(Mat::phong().hex(if x > 0.0 { 0xe8e4d8 } else { 0xd9a53a }))); add_mesh(st, b, sphere(0.13, 10, 8), hm, (x, hy, zc), (0.0, 0.0, 0.0));
        add_mesh(st, b, box_geo(0.19, 0.07, 0.04), black, (x, hy, zc + 0.125), (0.0, 0.0, 0.0)); add_mesh(st, b, cylinder(0.02, 0.02, 0.4, 5, 1, false), cm.trim, (x * 1.7, hy - 0.1, zc - 0.35), (0.0, 0.0, 0.0)); }
    add_mesh(st, b, cylinder(0.02, 0.02, 1.1, 5, 1, false), cm.trim, (0.0, s.roof - 0.06, zc - 0.35), (0.0, 0.0, PI / 2.0));
    add_mesh(st, b, torus(0.15, 0.02, 4, 12), cm.dark, (0.32, hy - 0.19, zc + 0.49), (-0.5, 0.0, 0.0));
    b
}

/// mergeKids: every rigid part of a group that shares a material look becomes one mesh (untextured opaque ones baked to vertex colours)
fn merge_kids(st: &mut St, grp: ObjId) {
    let key = |st: &St, m: MatId| { let mt = &st.sc.mats[m]; if mt.map.is_some() || mt.transparent { format!("u{}", m) } else {
        let ph = mt.kind == Kind::Phong; format!("{:?}|{}|{}|{}|{}", mt.kind, if ph { mt.shin } else { 0.0 }, if ph { format!("{:?}", mt.spec.map(|c| c.arr())) } else { String::new() }, mt.side, if mt.kind == Kind::Basic { String::new() } else { format!("{:?}", mt.emis.map(|c| c.arr())) }) } };
    let mut keys: Vec<String> = Vec::new(); let mut by: std::collections::HashMap<String, Vec<ObjId>> = Default::default();
    for &o in &st.sc.objs[grp].children.clone() { if st.sc.objs[o].kind != OKind::Mesh { continue; } let k = key(st, st.sc.objs[o].mat); by.entry(k.clone()).or_insert_with(|| { keys.push(k); Vec::new() }).push(o); }
    for k in keys { let list = by.remove(&k).unwrap(); if list.len() < 2 { continue; }
        let m0 = st.sc.objs[list[0]].mat; let bake = st.sc.mats[m0].map.is_none() && !st.sc.mats[m0].transparent; let use_uv = st.sc.mats[m0].map.is_some();
        let (mut p, mut n, mut uv, mut c) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for &o in &list { let lm = st.sc.local(o); let src = &st.sc.geos[st.sc.objs[o].geo]; let mut g = if src.index.is_some() { src.to_non_indexed() } else { src.clone() }; g.bs = None; g.apply(&lm); if g.nrm.is_none() { g.compute_normals(); }
            let cnt = g.count(); let col = st.sc.mats[st.sc.objs[o].mat].color.unwrap_or(WHITE);
            p.extend_from_slice(&g.pos); n.extend_from_slice(g.nrm.as_ref().unwrap());
            if use_uv { match &g.uv { Some(u) => uv.extend_from_slice(u), None => uv.extend(std::iter::repeat(0.0).take(cnt * 2)) } }
            if bake { for _ in 0..cnt { c.extend_from_slice(&[col.r as f32, col.g as f32, col.b as f32]); } }
            st.sc.remove_child(grp, o); }
        let mut g = Geo::new(p).with_nrm(n); if use_uv { g.uv = Some(uv); }
        let m = if bake { g.col = Some(c); let mut mm = st.sc.mats[m0].clone(); mm.color = Some(WHITE); mm.vc = true; st.mat(mm) } else { m0 };
        let o = st.mesh(g, m); st.sc.add_to(grp, o); }
}

pub fn cars(st: &mut St) {
    // the first car's livery used the shared generator: burn the same draws so the rest of the stage comes out as before
    for _ in 0..1400 { let z = st.rr(-2.05, 2.05); let near_w = if (z - WB_F).abs().min((z - WB_R).abs()) < 0.6 { 1.0 } else { 0.4 }; st.rnd(); if st.rnd() > near_w + 0.2 { continue; } for _ in 0..5 { st.rnd(); } }
    let lam = |st: &mut St, c: u32| st.mat(Mat::lambert().hex(c));
    let cm = Common { black: lam(st, 0x161616), trim: lam(st, 0x3a3c3e), dark: lam(st, 0x202020), seat: lam(st, 0x7a1d18), suit: lam(st, 0x2b3b6a),
        glass: { let mut g = phong(Mat::phong().hex(0x1c2a34)); g.transparent = true; g.opacity = 0.7; g.shin = 12.0; g.spec = Some(Col::hex(0x2c3a44)); st.mat(g) },
        lamp_on: st.mat(Mat::basic().hex(0xe6dfc6)), tail: st.mat(Mat::basic().hex(0x9a1c14)), fog_lamp: st.mat(Mat::basic().hex(0xd8b860)),
        mirror: st.mat(Mat::lambert().hex(0x88a0a8).double()), plate: lam(st, 0xe6e2d6), flap: lam(st, 0x2a2a2a) };
    let root = st.sc.group(); st.car_root = root; let atl = atlases(); let sp = specs();
    for (k, s) in sp.iter().enumerate() { let b = build_body(st, s, k, &cm, &atl[k]); merge_kids(st, b); st.sc.objs[b].visible = k == 0; st.sc.add_to(root, b); st.car_models.push(b); }
    let w = st.sc.group(); st.sc.add_to(root, w); st.car_w = w;
    let prof = [(0.2, -0.11), (0.27, -0.118), (0.3, -0.104), (0.31, -0.06), (0.31, 0.06), (0.3, 0.104), (0.27, 0.118), (0.2, 0.11)];
    let mut tire = lathe(&prof, 12).to_non_indexed(); tire.compute_normals(); tire.rotate_z(PI / 2.0); let tire = st.geo(tire);
    let rim_t = st.canvas_tex(32, 32, false, |_, g| { g.fill_style("#ebe8de"); g.begin_path(); g.circle(16.0, 16.0, 15.0); g.fill(); g.fill_style("#2a2a2a");
        for k in 0..8 { let a = k as f64 / 8.0 * PI * 2.0; g.begin_path(); g.circle(16.0 + a.cos() * 9.5, 16.0 + a.sin() * 9.5, 2.7); g.fill(); }
        g.begin_path(); g.circle(16.0, 16.0, 3.5); g.fill(); g.fill_style("#bbb"); g.fill_rect(15.0, 15.0, 2.0, 2.0); });
    let rim_m = st.mat(Mat::lambert().map(rim_t).transparent().at(0.5)); let tire_m = st.mat(Mat::lambert().hex(0x1c1c1c).double()); let barrel_m = st.mat(Mat::lambert().hex(0xd8d5cc).double()); let disc_m = st.mat(Mat::lambert().hex(0x555555));
    let mut rim_g = cylinder(0.2, 0.2, 0.17, 14, 1, true); rim_g.rotate_z(PI / 2.0); let rim_g = st.geo(rim_g); let face_g = st.geo(circle(0.2, 14));
    for (lx, lz, front) in [(TRACK, WB_F, true), (-TRACK, WB_F, true), (TRACK, WB_R, false), (-TRACK, WB_R, false)] {
        let pivot = st.sc.group(); st.sc.set_pos(pivot, lx, TIRE_R, lz); let spin = st.sc.group(); st.sc.add_to(pivot, spin);
        let t = st.sc.mesh(tire, tire_m); st.sc.add_to(spin, t); let r = st.sc.mesh(rim_g, barrel_m); st.sc.add_to(spin, r);
        let sg = if lx > 0.0 { 1.0 } else { -1.0 }; let f = st.sc.mesh(face_g, rim_m); st.sc.set_pos(f, sg * 0.07, 0.0, 0.0); st.sc.set_rot(f, 0.0, sg * PI / 2.0, 0.0); st.sc.add_to(spin, f);
        let d = st.mesh(circle(0.16, 12), disc_m); st.sc.set_pos(d, -sg * 0.04, 0.0, 0.0); st.sc.set_rot(d, 0.0, sg * PI / 2.0, 0.0); st.sc.add_to(pivot, d);
        st.sc.add_to(w, pivot); st.wheels.push(Wheel { pivot, spin, front }); }
    for i in 0..4 { let (p, s) = (st.wheels[i].pivot, st.wheels[i].spin); merge_kids(st, s); merge_kids(st, p); }
    st.add(root); st.sc.objs[root].yxz = true;
    let sh_t = st.canvas_tex(64, 64, false, |_, g| { g.fill_grad(canvas::Style::Radial { x0: 32.0, y0: 32.0, r0: 4.0, x1: 32.0, y1: 32.0, r1: 32.0, stops: vec![(0.0, canvas::parse("rgba(0,0,0,0.85)")), (0.7, canvas::parse("rgba(0,0,0,0.5)")), (1.0, canvas::parse("rgba(0,0,0,0)"))] }); g.fill_rect(0.0, 0.0, 64.0, 64.0); });
    { let t = &mut st.sc.texs[sh_t]; t.mag = LINEAR; t.min = LINEAR; }
    let mut shg = plane(2.4, 4.6, 1, 1); shg.rotate_x(-PI / 2.0);
    let m = st.mat(Mat::basic().map(sh_t).transparent().no_dw().opacity(0.62).hex(0x000000).po(-4.0, -6.0)); let sh = st.mesh(shg, m); st.sc.objs[sh].yxz = true; st.add(sh); st.car_shadow = sh;
}

const RIVAL_COLS: [[f64; 3]; 5] = [[1.0, 0.3, 0.24], [0.34, 0.55, 1.0], [1.0, 0.58, 0.18], [0.38, 0.86, 0.44], [1.0, 0.88, 0.32]];
/// the rivals' cars: one instanced copy of every part, posed by the app through a puppet of your car, tinted per team
pub fn rivals(st: &mut St) {
    let n = 5usize; let model_of = |st: &St, o: ObjId| { let mut q = Some(o); while let Some(i) = q { if let Some(m) = st.sc.objs[i].model { return m as i64; } q = st.sc.objs[i].parent; } -1 };
    let add_part = |st: &mut St, src: Option<ObjId>, geo: GeoId, mat: MatId, per: usize| -> usize {
        let mc = st.sc.mats[mat].clone(); let tint = mc.kind == Kind::Phong; let m2 = st.mat(mc); let im = st.sc.inst(geo, m2, n * per); st.sc.objs[im].culled = false; st.sc.objs[im].visible = false;
        for r in 0..n * per { let c = if tint { let k = RIVAL_COLS[r / per]; rgb(k[0], k[1], k[2]) } else { WHITE }; st.sc.set_color_at(im, r, c); }
        st.add(im); let model = src.map(|s| model_of(st, s)).unwrap_or(-1);
        st.rg_parts.push(RgPart { im, src: src.into_iter().collect(), model, tint, liv: 0, paint: false }); st.rg_parts.len() - 1 };
    let slots: Vec<Vec<ObjId>> = st.wheels.iter().map(|w| { let mut l = Vec::new(); st.sc.walk(w.pivot, &mut l); l.into_iter().filter(|&o| st.sc.objs[o].kind == OKind::Mesh).collect() }).collect();
    let in_wheel: Vec<ObjId> = slots.iter().flatten().cloned().collect();
    for q in 0..slots[0].len() { let o = slots[0][q]; let (g, m) = (st.sc.objs[o].geo, st.sc.objs[o].mat); let e = add_part(st, None, g, m, 4); st.rg_parts[e].src = slots.iter().map(|l| l[q]).collect(); }
    let mut all = Vec::new(); st.sc.walk(st.car_root, &mut all);
    for o in all { if st.sc.objs[o].kind != OKind::Mesh || in_wheel.contains(&o) || st.sc.objs[o].liv_carrier { continue; }
        let (g, m) = (st.sc.objs[o].geo, st.sc.objs[o].mat); let p = add_part(st, Some(o), g, m, 1); let is_paint = st.sc.mats[m].paint; st.rg_parts[p].paint = is_paint;
        if is_paint { let mdl = model_of(st, o); let carriers = st.sc.objs[st.car_models[mdl as usize]].liveries.clone();
            for (li, c) in carriers.into_iter().enumerate() { let lm = st.sc.objs[c].mat; let q = add_part(st, Some(o), g, lm, 1); st.rg_parts[q].tint = false; st.rg_parts[q].liv = li + 1; st.rg_parts[q].paint = true;
                let im = st.rg_parts[q].im; for r in 0..n { st.sc.set_color_at(im, r, WHITE); } } } }
    let (sg, sm) = (st.sc.objs[st.car_shadow].geo, st.sc.objs[st.car_shadow].mat); let mc = st.sc.mats[sm].clone(); let m2 = st.mat(mc);
    let sh = st.sc.inst(sg, m2, n); st.sc.objs[sh].culled = false; st.sc.objs[sh].visible = false; st.add(sh); st.rg_shadow = sh;
}
