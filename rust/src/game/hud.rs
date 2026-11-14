//! The HUD canvas: a straight-alpha RGBA buffer at render resolution with the few canvas operations the game uses
//! (fillRect with source-over, the 5x7 pixel font, panels, arrows, pixel lines). Drawn on the CPU, uploaded only when it changed.

#[derive(Clone, Copy, PartialEq)]
pub struct Col { pub r: f32, pub g: f32, pub b: f32, pub a: f32 }
pub const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Col { Col { r: r as f32, g: g as f32, b: b as f32, a } }
pub const fn hex(v: u32) -> Col { rgba((v >> 16) as u8, (v >> 8) as u8, v as u8, 1.0) }

pub const INK: Col = hex(0xe9ece2);
pub const AMBER: Col = hex(0xf3cf62);
pub const RED: Col = hex(0xd8483a);
pub const PANEL: Col = rgba(8, 12, 8, 0.62);
pub const EDGE: Col = rgba(233, 236, 226, 0.4);
pub const SHADOW: Col = rgba(0, 0, 0, 0.55);

/// Draw calls are recorded (integer pixel bounds + colour); when a frame's list equals the last one nothing is rasterized or uploaded.
pub struct Canvas { pub w: i32, pub h: i32, pub bytes: Vec<u8>, ops: Vec<[u32; 6]>, last_ops: Vec<[u32; 6]>, pub last: Vec<u8>, pub fill: Col, dirty: bool }

impl Canvas {
    pub fn new(w: i32, h: i32) -> Canvas { let n = (w.max(1) * h.max(1)) as usize; Canvas { w, h, bytes: vec![0; n * 4], ops: Vec::new(), last_ops: Vec::new(), last: Vec::new(), fill: INK, dirty: true } }
    pub fn resize(&mut self, w: i32, h: i32) { if w != self.w || h != self.h { *self = Canvas::new(w, h); } }
    pub fn clear(&mut self) { self.ops.clear(); }
    /// force the next frame through (the texture was lost)
    pub fn invalidate(&mut self) { self.dirty = true; self.last_ops.clear(); }
    pub fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) { let c = self.fill; self.rect_c(x, y, w, h, c); }
    /// fillRect (canvas semantics for integer edges: the pixels whose centres fall inside)
    pub fn rect_c(&mut self, x: f64, y: f64, w: f64, h: f64, c: Col) {
        if c.a <= 0.0 || w == 0.0 || h == 0.0 { return; }
        let (x0, x1) = if w < 0.0 { (x + w, x) } else { (x, x + w) }; let (y0, y1) = if h < 0.0 { (y + h, y) } else { (y, y + h) };
        let xa = (x0 - 0.5).ceil().max(0.0) as i32; let xb = ((x1 - 0.5).ceil() as i32).min(self.w);
        let ya = (y0 - 0.5).ceil().max(0.0) as i32; let yb = ((y1 - 0.5).ceil() as i32).min(self.h);
        if xa >= xb || ya >= yb { return; }
        let col = (c.r as u32) | (c.g as u32) << 8 | (c.b as u32) << 16; let a = (c.a.min(1.0) * 65535.0) as u32;
        self.ops.push([xa as u32, ya as u32, xb as u32, yb as u32, col, a]);
    }
    fn raster(&mut self) {
        for p in self.bytes.iter_mut() { *p = 0; }
        let w = self.w as usize;
        for op in &self.ops {
            let (xa, ya, xb, yb) = (op[0] as usize, op[1] as usize, op[2] as usize, op[3] as usize);
            let (sr, sg, sb) = ((op[4] & 255) as f32, ((op[4] >> 8) & 255) as f32, ((op[4] >> 16) & 255) as f32); let sa = op[5] as f32 / 65535.0;
            for yy in ya..yb { let row = yy * w;
                for xx in xa..xb { let q = (row + xx) * 4; let p = &mut self.bytes[q..q + 4];
                    if sa >= 0.9999 { p[0] = sr as u8; p[1] = sg as u8; p[2] = sb as u8; p[3] = 255; continue; }
                    let da = p[3] as f32 / 255.0; let oa = sa + da * (1.0 - sa); if oa <= 0.0 { continue; } let k = da * (1.0 - sa);
                    p[0] = ((sr * sa + p[0] as f32 * k) / oa + 0.5) as u8; p[1] = ((sg * sa + p[1] as f32 * k) / oa + 0.5) as u8; p[2] = ((sb * sa + p[2] as f32 * k) / oa + 0.5) as u8;
                    p[3] = (oa * 255.0 + 0.5) as u8; } } }
    }
    /// rasterize if this frame differs from the last; true when the texture needs the new bytes
    pub fn finish(&mut self) -> bool {
        if !self.dirty && self.ops == self.last_ops { return false; }
        self.raster(); std::mem::swap(&mut self.ops, &mut self.last_ops); self.dirty = false; true
    }
}

// ---------------- pixel font (5x7) ----------------
fn glyph(ch: char) -> [u8; 7] {
    match ch {
        'A' => [14,17,17,31,17,17,17], 'B' => [30,17,17,30,17,17,30], 'C' => [14,17,16,16,16,17,14], 'D' => [30,17,17,17,17,17,30], 'E' => [31,16,16,30,16,16,31],
        'F' => [31,16,16,30,16,16,16], 'G' => [14,17,16,23,17,17,15], 'H' => [17,17,17,31,17,17,17], 'I' => [14,4,4,4,4,4,14], 'J' => [7,2,2,2,2,18,12],
        'K' => [17,18,20,24,20,18,17], 'L' => [16,16,16,16,16,16,31], 'M' => [17,27,21,21,17,17,17], 'N' => [17,17,25,21,19,17,17], 'O' => [14,17,17,17,17,17,14],
        'P' => [30,17,17,30,16,16,16], 'Q' => [14,17,17,17,21,18,13], 'R' => [30,17,17,30,20,18,17], 'S' => [15,16,16,14,1,1,30], 'T' => [31,4,4,4,4,4,4],
        'U' => [17,17,17,17,17,17,14], 'V' => [17,17,17,17,17,10,4], 'W' => [17,17,17,21,21,21,10], 'X' => [17,17,10,4,10,17,17], 'Y' => [17,17,10,4,4,4,4], 'Z' => [31,1,2,4,8,16,31],
        '0' => [14,17,19,21,25,17,14], '1' => [4,12,4,4,4,4,14], '2' => [14,17,1,2,4,8,31], '3' => [31,2,4,2,1,17,14], '4' => [2,6,10,18,31,2,2],
        '5' => [31,16,30,1,1,17,14], '6' => [6,8,16,30,17,17,14], '7' => [31,1,2,4,8,8,8], '8' => [14,17,17,14,17,17,14], '9' => [14,17,17,15,1,2,12],
        ' ' => [0; 7], '.' => [0,0,0,0,0,12,12], ':' => [0,12,12,0,12,12,0], '/' => [1,1,2,4,8,16,16], '-' => [0,0,0,31,0,0,0], '\'' => [4,4,8,0,0,0,0],
        '!' => [4,4,4,4,4,0,4], '+' => [0,4,4,31,4,4,0], '(' => [2,4,8,8,8,4,2], ')' => [8,4,2,2,2,4,8], ',' => [0,0,0,0,12,4,8],
        '<' => [2,4,8,16,8,4,2], '>' => [8,4,2,1,2,4,8],
        _ => [14,17,1,2,4,0,4],   // '?'
    }
}
pub fn text_w(s: &str, sc: f64) -> f64 { s.chars().count() as f64 * 6.0 * sc - sc }

impl Canvas {
    fn glyphs(&mut self, s: &str, x: f64, y: f64, sc: f64, c: Col) {
        for (n, ch) in s.chars().enumerate() { if ch == ' ' { continue; } let gl = glyph(ch.to_ascii_uppercase());
            for (r, row) in gl.iter().enumerate() { if *row == 0 { continue; }
                for b in 0..5 { if row & (16 >> b) != 0 { self.rect_c(x + (n as f64 * 6.0 + b as f64) * sc, y + r as f64 * sc, sc, sc, c); } } } }
    }
    /// text(g, s, x, y, sc, col, shadow): the shadow (one pixel down-right) under every glyph, then the glyphs
    pub fn text(&mut self, s: &str, x: f64, y: f64, sc: f64, c: Col, shadow: Option<Col>) {
        let s = s.to_uppercase(); let (x, y) = (js_round(x), js_round(y));
        if let Some(sh) = shadow { self.glyphs(&s, x + sc, y + sc, sc, sh); }
        self.glyphs(&s, x, y, sc, c);
    }
    pub fn tx(&mut self, s: &str, x: f64, y: f64, sc: f64, c: Col) { self.text(s, x, y, sc, c, Some(SHADOW)); }
    /// panel with clipped corners and an optional one-pixel edge
    pub fn panel(&mut self, x: f64, y: f64, w: f64, h: f64, u: f64, fill: Col, edge: Option<Col>) {
        let (x, y, w, h) = (js_round(x), js_round(y), js_round(w), js_round(h));
        self.rect_c(x + u, y, w - 2.0 * u, h, fill); self.rect_c(x, y + u, w, h - 2.0 * u, fill);
        if let Some(e) = edge { self.rect_c(x + u, y, w - 2.0 * u, u, e); self.rect_c(x + u, y + h - u, w - 2.0 * u, u, e); self.rect_c(x, y + u, u, h - 2.0 * u, e); self.rect_c(x + w - u, y + u, u, h - 2.0 * u, e); }
    }
    pub fn arrow(&mut self, cx: f64, cy: f64, dir: f64, sz: f64, u: f64, c: Col) {
        let mut r = -sz; while r <= sz { let len = sz - r.abs(); if len > 0.0 { let x0 = if dir < 0.0 { cx - sz / 2.0 + r.abs() } else { cx - sz / 2.0 }; self.rect_c(js_round(x0), js_round(cy + r), js_round(len), u, c); } r += u; }
    }
    pub fn px_line(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, c: Col, t: f64, u: f64) {
        let n = ((x1 - x0).hypot(y1 - y0) / u).ceil() as i32; let n = n.max(1);
        for i in 0..=n { let k = i as f64 / n as f64; self.rect_c(js_round(x0 + (x1 - x0) * k - t / 2.0), js_round(y0 + (y1 - y0) * k - t / 2.0), t, t, c); }
    }
}
#[inline] pub fn js_round(v: f64) -> f64 { (v + 0.5).floor() }
