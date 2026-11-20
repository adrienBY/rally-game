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
pub struct Canvas { pub w: i32, pub h: i32, pub bytes: Vec<u8>, ops: Vec<[u32; 6]>, last_ops: Vec<[u32; 6]>, pub last: Vec<u8>, pub fill: Col, dirty: bool, diff: Diff }

/// The frame-to-frame op diff: a counting hash table kept between frames (open addressing, a cheap multiply-rotate hash, slots
/// stamped with a generation instead of cleared) - a few thousand ops a frame, so the general-purpose HashMap was most of the HUD's cost.
#[derive(Default)]
struct Diff { keys: Vec<[u32; 6]>, cnt: Vec<i32>, gen: Vec<u32>, now: u32, used: Vec<u32> }
impl Diff {
    #[inline] fn hash(k: &[u32; 6]) -> u32 { let mut h: u32 = 0; for v in k { h = (h.rotate_left(5) ^ v).wrapping_mul(0x9E37_79B9); } h ^ (h >> 15) }
    fn begin(&mut self, n: usize) {
        let want = (n * 2).next_power_of_two().max(1024);
        if self.keys.len() < want { self.keys = vec![[0; 6]; want]; self.cnt = vec![0; want]; self.gen = vec![0; want]; self.now = 0; }
        self.now = self.now.wrapping_add(1); if self.now == 0 { for g in self.gen.iter_mut() { *g = 0; } self.now = 1; }
        self.used.clear();
    }
    /// the slot for `k` (claimed for this generation if new)
    #[inline] fn slot(&mut self, k: &[u32; 6]) -> usize {
        let m = self.keys.len() - 1; let mut i = Self::hash(k) as usize & m;
        loop { if self.gen[i] != self.now { self.gen[i] = self.now; self.keys[i] = *k; self.cnt[i] = 0; self.used.push(i as u32); return i; }
            if self.keys[i] == *k { return i; } i = (i + 1) & m; }
    }
}

impl Default for Canvas { fn default() -> Self { Canvas::new(1, 1) } }

impl Canvas {
    pub fn new(w: i32, h: i32) -> Canvas { let n = (w.max(1) * h.max(1)) as usize; Canvas { w, h, bytes: vec![0; n * 4], ops: Vec::new(), last_ops: Vec::new(), last: Vec::new(), fill: INK, dirty: true, diff: Diff::default() } }
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
    /// draw one op, clipped to a tile. Source-over in integers: an opaque op is a straight copy, a translucent one over an empty
    /// pixel is the op itself, and only translucent over painted pixels blends (one reciprocal per pixel instead of three divisions).
    fn fill_clip(bytes: &mut [u8], w: usize, op: &[u32; 6], cx0: usize, cy0: usize, cx1: usize, cy1: usize) {
        let (xa, ya, xb, yb) = ((op[0] as usize).max(cx0), (op[1] as usize).max(cy0), (op[2] as usize).min(cx1), (op[3] as usize).min(cy1));
        if xa >= xb || ya >= yb { return; }
        let (sr, sg, sb) = (op[4] & 255, (op[4] >> 8) & 255, (op[4] >> 16) & 255); let sa = op[5] as f32 / 65535.0;
        let sa8 = (sa * 255.0 + 0.5) as u32;
        if sa >= 0.9999 { let px = [sr as u8, sg as u8, sb as u8, 255];
            for yy in ya..yb { let r = (yy * w + xa) * 4; for p in bytes[r..r + (xb - xa) * 4].chunks_exact_mut(4) { p.copy_from_slice(&px); } } return; }
        if sa8 == 0 { return; }
        let ia = 255 - sa8; let (wr, wg, wb) = (sr * sa8 * 255, sg * sa8 * 255, sb * sa8 * 255);
        for yy in ya..yb { let r = (yy * w + xa) * 4;
            for p in bytes[r..r + (xb - xa) * 4].chunks_exact_mut(4) {
                let da = p[3] as u32;
                if da == 0 { p[0] = sr as u8; p[1] = sg as u8; p[2] = sb as u8; p[3] = sa8 as u8; continue; }
                let k = da * ia; let oa = sa8 * 255 + k;   // out alpha x 255
                let inv = (1u64 << 32) / oa as u64; let mixc = |s: u32, d: u8| (((s + d as u32 * k) as u64 * inv + (1u64 << 31)) >> 32) as u8;
                p[0] = mixc(wr, p[0]); p[1] = mixc(wg, p[1]); p[2] = mixc(wb, p[2]); p[3] = ((oa + 127) / 255) as u8;
            } }
    }
    /// test hook: does the incrementally updated buffer equal a full redraw of the last frame?
    pub fn verify(&self) -> bool { let w = self.w as usize; let mut b = vec![0u8; self.bytes.len()]; for op in &self.last_ops { Self::fill_clip(&mut b, w, op, 0, 0, w, self.h as usize); } b == self.bytes }
    /// Compare this frame's draw calls with the last frame's; re-rasterize only the 32x32 tiles where something appeared, moved or vanished.
    /// Returns the rectangles to upload (None: nothing changed).
    pub fn finish(&mut self) -> Option<Vec<[i32; 4]>> {
        if !self.dirty && self.ops == self.last_ops { return None; }
        const T: usize = 32; let (w, h) = (self.w as usize, self.h as usize); let (tw, th) = ((w + T - 1) / T, (h + T - 1) / T);
        let mut dirty = vec![self.dirty; tw * th];
        let mark = |op: &[u32; 6], dirty: &mut Vec<bool>| { for ty in (op[1] as usize / T)..=(((op[3] as usize).max(1) - 1) / T).min(th - 1) { for tx in (op[0] as usize / T)..=(((op[2] as usize).max(1) - 1) / T).min(tw - 1) { dirty[ty * tw + tx] = true; } } };
        if !self.dirty {
            let df = &mut self.diff; df.begin(self.last_ops.len().max(self.ops.len()));
            for op in &self.last_ops { let i = df.slot(op); df.cnt[i] += 1; }
            for op in &self.ops { let i = df.slot(op); if df.cnt[i] > 0 { df.cnt[i] -= 1; } else { mark(op, &mut dirty); } }
            for &i in &df.used { if df.cnt[i as usize] > 0 { let k = df.keys[i as usize]; mark(&k, &mut dirty); } }
            // an op that stayed but now sits under or over a changed one is redrawn too, because whole tiles are redrawn in order
        }
        for ty in 0..th { for tx in 0..tw { if !dirty[ty * tw + tx] { continue; }
            let (x0, y0, x1, y1) = (tx * T, ty * T, ((tx + 1) * T).min(w), ((ty + 1) * T).min(h));
            for yy in y0..y1 { let r = (yy * w + x0) * 4; for v in &mut self.bytes[r..r + (x1 - x0) * 4] { *v = 0; } } } }
        for op in &self.ops {
            for ty in (op[1] as usize / T)..=(((op[3] as usize).max(1) - 1) / T).min(th - 1) { for tx in (op[0] as usize / T)..=(((op[2] as usize).max(1) - 1) / T).min(tw - 1) {
                if dirty[ty * tw + tx] { Self::fill_clip(&mut self.bytes, w, op, tx * T, ty * T, ((tx + 1) * T).min(w), ((ty + 1) * T).min(h)); } } }
        }
        // runs of dirty tiles along each tile row become one upload each
        let mut rects = Vec::new();
        for ty in 0..th { let mut tx = 0; while tx < tw { if !dirty[ty * tw + tx] { tx += 1; continue; } let s = tx; while tx < tw && dirty[ty * tw + tx] { tx += 1; }
            let (x0, y0) = (s * T, ty * T); rects.push([x0 as i32, y0 as i32, ((tx * T).min(w) - x0) as i32, (((ty + 1) * T).min(h) - y0) as i32]); } }
        std::mem::swap(&mut self.ops, &mut self.last_ops); self.dirty = false;
        if rects.is_empty() { None } else { Some(rects) }
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
                // one rect per run of lit pixels in the row (the same pixels as one per pixel: the runs tile exactly)
                let mut b = 0; while b < 5 { if row & (16 >> b) == 0 { b += 1; continue; } let s0 = b; while b < 5 && row & (16 >> b) != 0 { b += 1; }
                    self.rect_c(x + (n as f64 * 6.0 + s0 as f64) * sc, y + r as f64 * sc, (b - s0) as f64 * sc, sc, c); } } }
    }
    /// text(g, s, x, y, sc, col, shadow): the shadow (one pixel down-right) under every glyph, then the glyphs
    pub fn text(&mut self, s: &str, x: f64, y: f64, sc: f64, c: Col, shadow: Option<Col>) {
        let s = s.to_uppercase(); let (x, y) = (js_round(x), js_round(y));
        if let Some(sh) = shadow { self.glyphs(&s, x + sc, y + sc, sc, sh); }
        self.glyphs(&s, x, y, sc, c);
    }
    pub fn tx(&mut self, s: &str, x: f64, y: f64, sc: f64, c: Col) { self.text(s, x, y, sc, c, Some(SHADOW)); }
    /// Italic display lettering: each of the 7 glyph rows gets its own colour (a chrome / sunset gradient) and leans right
    /// (upper rows pushed right in whole pixels); `gap` extra letter spacing in glyph pixels, `sh` the drop shadow offset.
    pub fn text_italic(&mut self, s: &str, x: f64, y: f64, sc: f64, rows: &[Col; 7], gap: f64, sh: Option<(Col, f64)>) {
        let s = s.to_uppercase(); let (x, y) = (js_round(x), js_round(y)); let adv = (6.0 + gap) * sc;
        let pass = |g: &mut Canvas, ox: f64, oy: f64, solid: Option<Col>| {
            for (n, ch) in s.chars().enumerate() { if ch == ' ' { continue; } let gl = glyph(ch);
                for (r, row) in gl.iter().enumerate() { if *row == 0 { continue; } let lean = js_round((6 - r) as f64 * sc / 3.0); let c = solid.unwrap_or(rows[r]);
                    let mut b = 0; while b < 5 { if row & (16 >> b) == 0 { b += 1; continue; } let s0 = b; while b < 5 && row & (16 >> b) != 0 { b += 1; }
                        g.rect_c(x + ox + n as f64 * adv + s0 as f64 * sc + lean, y + oy + r as f64 * sc, (b - s0) as f64 * sc, sc, c); } } } };
        if let Some((c, d)) = sh { pass(self, d, d, Some(c)); }
        pass(self, 0.0, 0.0, None);
    }
    /// width of italic lettering (lean included)
    pub fn italic_w(s: &str, sc: f64, gap: f64) -> f64 { s.chars().count() as f64 * (6.0 + gap) * sc - (1.0 + gap) * sc + 2.0 * sc }
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
