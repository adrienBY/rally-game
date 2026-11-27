//! The 2D canvas calls the stage's pixel textures were painted with (fillRect, paths, ellipses, round-capped strokes, radial gradients,
//! image data), on tiny-skia, Skia's own rasterizer ported to Rust - the one Chrome's canvas uses - so the textures come out the same.
use tiny_skia::{BlendMode, Color, FillRule, GradientStop, LineCap, Paint, PathBuilder, Pixmap, Point, RadialGradient, Rect, Shader, SpreadMode, Stroke, Transform};

#[derive(Clone)]
pub enum Style { Solid([u8; 4]), Radial { x0: f32, y0: f32, r0: f32, x1: f32, y1: f32, r1: f32, stops: Vec<(f32, [u8; 4])> } }

pub struct Ctx {
    pub pm: Pixmap, pub w: usize, pub h: usize,
    fill: Style, stroke: [u8; 4], pub line_width: f32, pub round_cap: bool, ts: Transform, stack: Vec<Transform>, path: PathBuilder, pub dest_over: bool,
}

/// "rgb(r,g,b)", "rgba(r,g,b,a)", "#rrggbb", "#rgb"; the alpha kept to 8 bits, as the canvas does
pub fn parse(s: &str) -> [u8; 4] {
    let s = s.trim();
    if let Some(h) = s.strip_prefix('#') {
        if h.len() == 3 { let v = u32::from_str_radix(h, 16).unwrap_or(0); return [((v >> 8) & 15) as u8 * 17, ((v >> 4) & 15) as u8 * 17, (v & 15) as u8 * 17, 255]; }
        let v = u32::from_str_radix(h, 16).unwrap_or(0); return [(v >> 16) as u8, (v >> 8) as u8, v as u8, 255];
    }
    let inner = s.trim_start_matches("rgba").trim_start_matches("rgb").trim_start_matches('(').trim_end_matches(')');
    let p: Vec<f64> = inner.split(',').map(|t| t.trim().parse::<f64>().unwrap_or(0.0)).collect();
    let c = |v: f64| v.round().clamp(0.0, 255.0) as u8;
    [c(p.first().copied().unwrap_or(0.0)), c(p.get(1).copied().unwrap_or(0.0)), c(p.get(2).copied().unwrap_or(0.0)), c(p.get(3).copied().unwrap_or(1.0) * 255.0)]
}
pub fn rgba(r: f64, g: f64, b: f64, a: f64) -> [u8; 4] { let c = |v: f64| v.round().clamp(0.0, 255.0) as u8; [c(r), c(g), c(b), c(a * 255.0)] }
/// `rgb(${x | 0}, ...)` strings: components truncated
pub fn rgbi(r: f64, g: f64, b: f64) -> [u8; 4] { let c = |v: f64| (v as i64).clamp(0, 255) as u8; [c(r), c(g), c(b), 255] }
fn col(c: [u8; 4]) -> Color { Color::from_rgba8(c[0], c[1], c[2], c[3]) }

impl Ctx {
    pub fn new(w: usize, h: usize) -> Ctx {
        Ctx { pm: Pixmap::new(w.max(1) as u32, h.max(1) as u32).unwrap(), w, h, fill: Style::Solid([0, 0, 0, 255]), stroke: [0, 0, 0, 255], line_width: 1.0, round_cap: false,
            ts: Transform::identity(), stack: Vec::new(), path: PathBuilder::new(), dest_over: false }
    }
    pub fn fill_style(&mut self, s: &str) { self.fill = Style::Solid(parse(s)); }
    pub fn fill_c(&mut self, c: [u8; 4]) { self.fill = Style::Solid(c); }
    pub fn fill_grad(&mut self, g: Style) { self.fill = g; }
    pub fn stroke_style(&mut self, s: &str) { self.stroke = parse(s); }
    pub fn stroke_c(&mut self, c: [u8; 4]) { self.stroke = c; }
    fn paint(&self) -> Paint<'static> {
        let mut p = Paint::default(); p.anti_alias = true; if self.dest_over { p.blend_mode = BlendMode::DestinationOver; }
        match &self.fill {
            Style::Solid(c) => p.set_color(col(*c)),
            Style::Radial { x0, y0, r0, x1, y1, r1, stops } => {
                // concentric gradients with an inner radius: remap the stops onto one that starts at the centre
                let k = |o: f32| if *r1 > 0.0 { (r0 + o * (r1 - r0)) / r1 } else { o };
                let mut st: Vec<GradientStop> = Vec::new();
                if *r0 > 0.0 { if let Some(s0) = stops.first() { st.push(GradientStop::new(0.0, col(s0.1))); } }
                for (o, c) in stops { st.push(GradientStop::new(k(*o), col(*c))); }
                let _ = (x0, y0);
                p.shader = RadialGradient::new(Point::from_xy(*x1, *y1), Point::from_xy(*x1, *y1), *r1, st, SpreadMode::Pad, Transform::identity()).unwrap_or(Shader::SolidColor(Color::TRANSPARENT));
            }
        }
        p
    }
    pub fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        let (x0, x1) = if w < 0.0 { (x + w, x) } else { (x, x + w) }; let (y0, y1) = if h < 0.0 { (y + h, y) } else { (y, y + h) };
        if let Some(r) = Rect::from_ltrb(x0 as f32, y0 as f32, x1 as f32, y1 as f32) { let p = self.paint(); self.pm.fill_rect(r, &p, self.ts, None); }
    }
    /// a whole pixel set straight (the page's 1x1 fillRects at integer spots, opaque colours)
    pub fn px(&mut self, x: i64, y: i64, c: [u8; 4]) { self.fill_c(c); self.fill_rect(x as f64, y as f64, 1.0, 1.0); }
    pub fn clear_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        if let Some(r) = Rect::from_ltrb(x as f32, y as f32, (x + w) as f32, (y + h) as f32) { let mut p = Paint::default(); p.blend_mode = BlendMode::Clear; self.pm.fill_rect(r, &p, self.ts, None); }
    }
    pub fn begin_path(&mut self) { self.path = PathBuilder::new(); }
    pub fn move_to(&mut self, x: f64, y: f64) { self.path.move_to(x as f32, y as f32); }
    pub fn line_to(&mut self, x: f64, y: f64) { if self.path.is_empty() { self.path.move_to(x as f32, y as f32); } else { self.path.line_to(x as f32, y as f32); } }
    pub fn quad_to(&mut self, cx: f64, cy: f64, x: f64, y: f64) { self.path.quad_to(cx as f32, cy as f32, x as f32, y as f32); }
    pub fn close_path(&mut self) { self.path.close(); }
    /// a full circle (the page's arcs all go round: 0..7 rad)
    pub fn circle(&mut self, x: f64, y: f64, r: f64) { self.path.push_circle(x as f32, y as f32, r as f32); }
    /// a full ellipse, rotated by `rot`
    pub fn ellipse(&mut self, x: f64, y: f64, rx: f64, ry: f64, rot: f64) {
        // four conic quarter arcs as cubics, transformed
        let k = 0.5522847498f64; let (c, s) = (rot.cos(), rot.sin());
        let p = |u: f64, v: f64| ((x + u * c - v * s) as f32, (y + u * s + v * c) as f32);
        let pts = [(rx, 0.0), (rx, ry * k, ), (rx * k, ry), (0.0, ry), (-rx * k, ry), (-rx, ry * k), (-rx, 0.0), (-rx, -ry * k), (-rx * k, -ry), (0.0, -ry), (rx * k, -ry), (rx, -ry * k), (rx, 0.0)];
        let (sx, sy) = p(pts[0].0, pts[0].1); self.path.move_to(sx, sy);
        for q in 0..4 { let a = p(pts[q * 3 + 1].0, pts[q * 3 + 1].1); let b = p(pts[q * 3 + 2].0, pts[q * 3 + 2].1); let e = p(pts[q * 3 + 3].0, pts[q * 3 + 3].1); self.path.cubic_to(a.0, a.1, b.0, b.1, e.0, e.1); }
        self.path.close();
    }
    pub fn fill(&mut self) {
        let pb = std::mem::replace(&mut self.path, PathBuilder::new()); let keep = pb.clone(); self.path = keep;
        if let Some(path) = pb.finish() { let p = self.paint(); self.pm.fill_path(&path, &p, FillRule::Winding, self.ts, None); }
    }
    pub fn stroke(&mut self) {
        let pb = self.path.clone();
        if let Some(path) = pb.finish() { let mut p = Paint::default(); p.anti_alias = true; p.set_color(col(self.stroke));
            let st = Stroke { width: self.line_width, line_cap: if self.round_cap { LineCap::Round } else { LineCap::Butt }, ..Default::default() };
            self.pm.stroke_path(&path, &p, &st, self.ts, None); }
    }
    pub fn save(&mut self) { self.stack.push(self.ts); }
    pub fn restore(&mut self) { if let Some(t) = self.stack.pop() { self.ts = t; } }
    pub fn translate(&mut self, x: f64, y: f64) { self.ts = self.ts.pre_translate(x as f32, y as f32); }
    pub fn scale(&mut self, x: f64, y: f64) { self.ts = self.ts.pre_scale(x as f32, y as f32); }
    /// getImageData: straight (unpremultiplied) RGBA
    pub fn data(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.w * self.h * 4);
        for p in self.pm.pixels() { let c = p.demultiply(); out.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]); }
        out
    }
    /// putImageData at (0, 0): the pixels replaced as given (straight RGBA)
    pub fn put(&mut self, rgba: &[u8]) {
        for (p, c) in self.pm.pixels_mut().iter_mut().zip(rgba.chunks_exact(4)) {
            *p = tiny_skia::ColorU8::from_rgba(c[0], c[1], c[2], c[3]).premultiply();
        }
    }
}

// ---------------- the 5x7 pixel font the page drew its banners and stickers with (scale 1, no shadow)
pub fn glyph(ch: char) -> [u8; 7] {
    match ch {
        'A' => [14,17,17,31,17,17,17], 'B' => [30,17,17,30,17,17,30], 'C' => [14,17,16,16,16,17,14], 'D' => [30,17,17,17,17,17,30], 'E' => [31,16,16,30,16,16,31],
        'F' => [31,16,16,30,16,16,16], 'G' => [14,17,16,23,17,17,15], 'H' => [17,17,17,31,17,17,17], 'I' => [14,4,4,4,4,4,14], 'J' => [7,2,2,2,2,18,12],
        'K' => [17,18,20,24,20,18,17], 'L' => [16,16,16,16,16,16,31], 'M' => [17,27,21,21,17,17,17], 'N' => [17,17,25,21,19,17,17], 'O' => [14,17,17,17,17,17,14],
        'P' => [30,17,17,30,16,16,16], 'Q' => [14,17,17,17,21,18,13], 'R' => [30,17,17,30,20,18,17], 'S' => [15,16,16,14,1,1,30], 'T' => [31,4,4,4,4,4,4],
        'U' => [17,17,17,17,17,17,14], 'V' => [17,17,17,17,17,10,4], 'W' => [17,17,17,21,21,21,10], 'X' => [17,17,10,4,10,17,17], 'Y' => [17,17,10,4,4,4,4], 'Z' => [31,1,2,4,8,16,31],
        '0' => [14,17,19,21,25,17,14], '1' => [4,12,4,4,4,4,14], '2' => [14,17,1,2,4,8,31], '3' => [31,2,4,2,1,17,14], '4' => [2,6,10,18,31,2,2],
        '5' => [31,16,30,1,1,17,14], '6' => [6,8,16,30,17,17,14], '7' => [31,1,2,4,8,8,8], '8' => [14,17,17,14,17,17,14], '9' => [14,17,17,15,1,2,12],
        ' ' => [0; 7], _ => [14,17,1,2,4,0,4],
    }
}
pub fn text(g: &mut Ctx, s: &str, x: i64, y: i64, c: [u8; 4]) {
    for (n, ch) in s.chars().enumerate() { if ch == ' ' { continue; } let gl = glyph(ch.to_ascii_uppercase());
        for (r, row) in gl.iter().enumerate() { for b in 0..5 { if row & (16 >> b) != 0 { g.px(x + n as i64 * 6 + b, y + r as i64, c); } } } }
}
