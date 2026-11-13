//! Small DSP toolkit: noise, filters, delay lines, sample playback, a stereo spatialiser and a feedback-delay reverb.
//! No allocation happens after construction, so everything can run inside an audio callback.
pub use libm::{cosf, expf, fabsf, floorf, powf, sinf, sqrtf, tanf, tanhf};

pub const TAU: f32 = 6.283_185_3;
pub const PI: f32 = 3.141_592_7;

#[inline] pub fn clampf(v: f32, a: f32, b: f32) -> f32 { if v < a { a } else if v > b { b } else { v } }
#[inline] pub fn lerp(a: f32, b: f32, t: f32) -> f32 { a + (b - a) * t }
#[inline] pub fn smooth(e0: f32, e1: f32, x: f32) -> f32 { let t = clampf((x - e0) / (e1 - e0), 0.0, 1.0); t * t * (3.0 - 2.0 * t) }
#[inline] pub fn db(g: f32) -> f32 { powf(10.0, g / 20.0) }
/// sine of a phase in turns (0..1 is one cycle), parabolic approximation with one refinement step (|err| < 0.001)
#[inline] pub fn fsin(ph: f32) -> f32 { let x = ph - floorf(ph + 0.5); let y = 8.0 * x - 16.0 * x * fabsf(x); 0.225 * (y * fabsf(y) - y) + y }
/// tanh, rational approximation (soft clipper)
#[inline] pub fn ftanh(x: f32) -> f32 { let x = clampf(x, -3.0, 3.0); let x2 = x * x; x * (27.0 + x2) / (27.0 + 9.0 * x2) }
/// tan for 0..~1.2 rad (Pade), plenty for filter warping below ~0.4 * sample rate
#[inline] pub fn ftan(x: f32) -> f32 { let x2 = x * x; x * (15.0 - x2) / (15.0 - 6.0 * x2) }

/// xorshift32
#[derive(Clone, Copy)]
pub struct Rng(pub u32);
impl Rng {
    #[inline] pub fn u(&mut self) -> u32 { let mut x = self.0; x ^= x << 13; x ^= x >> 17; x ^= x << 5; self.0 = x; x }
    /// [0, 1)
    #[inline] pub fn f(&mut self) -> f32 { (self.u() >> 8) as f32 / 16_777_216.0 }
    /// [-1, 1)
    #[inline] pub fn bi(&mut self) -> f32 { self.f() * 2.0 - 1.0 }
    #[inline] pub fn range(&mut self, a: f32, b: f32) -> f32 { a + (b - a) * self.f() }
}

/// Pink noise (Paul Kellet's economy filter)
#[derive(Clone, Copy, Default)]
pub struct Pink { b0: f32, b1: f32, b2: f32 }
impl Pink {
    #[inline] pub fn run(&mut self, w: f32) -> f32 {
        self.b0 = 0.99765 * self.b0 + w * 0.099_046; self.b1 = 0.963 * self.b1 + w * 0.296_516_4; self.b2 = 0.57 * self.b2 + w * 1.052_691_3;
        (self.b0 + self.b1 + self.b2 + w * 0.1848) * 0.25
    }
}
/// Brown (red) noise, leaky integrator
#[derive(Clone, Copy, Default)]
pub struct Brown { z: f32 }
impl Brown { #[inline] pub fn run(&mut self, w: f32) -> f32 { self.z = (self.z + w * 0.02) * 0.998; self.z * 3.5 } }

/// One-pole low-pass (and high-pass as input minus low-pass)
#[derive(Clone, Copy, Default)]
pub struct OnePole { pub a: f32, pub z: f32 }
impl OnePole {
    pub fn new(sr: f32, f: f32) -> Self { let mut o = OnePole::default(); o.set(sr, f); o }
    #[inline] pub fn set(&mut self, sr: f32, f: f32) { self.a = 1.0 - expf(-TAU * clampf(f, 1.0, sr * 0.49) / sr); }
    #[inline] pub fn lp(&mut self, x: f32) -> f32 { self.z += self.a * (x - self.z); self.z }
    #[inline] pub fn hp(&mut self, x: f32) -> f32 { x - self.lp(x) }
}

/// RBJ biquad, transposed direct form II
#[derive(Clone, Copy, Default)]
pub struct Biquad { b0: f32, b1: f32, b2: f32, a1: f32, a2: f32, z1: f32, z2: f32 }
impl Biquad {
    fn set(&mut self, b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) { self.b0 = b0 / a0; self.b1 = b1 / a0; self.b2 = b2 / a0; self.a1 = a1 / a0; self.a2 = a2 / a0; }
    fn wc(sr: f32, f: f32, q: f32) -> (f32, f32) { let w = TAU * clampf(f, 5.0, sr * 0.45) / sr; (cosf(w), sinf(w) / (2.0 * q)) }
    pub fn lp(sr: f32, f: f32, q: f32) -> Self { let mut b = Biquad::default(); b.set_lp(sr, f, q); b }
    pub fn hp(sr: f32, f: f32, q: f32) -> Self { let mut b = Biquad::default(); b.set_hp(sr, f, q); b }
    pub fn bp(sr: f32, f: f32, q: f32) -> Self { let mut b = Biquad::default(); b.set_bp(sr, f, q); b }
    pub fn set_lp(&mut self, sr: f32, f: f32, q: f32) { let (c, a) = Self::wc(sr, f, q); self.set((1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0, 1.0 + a, -2.0 * c, 1.0 - a); }
    pub fn set_hp(&mut self, sr: f32, f: f32, q: f32) { let (c, a) = Self::wc(sr, f, q); self.set((1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0, 1.0 + a, -2.0 * c, 1.0 - a); }
    /// band-pass, 0 dB peak gain
    pub fn set_bp(&mut self, sr: f32, f: f32, q: f32) { let (c, a) = Self::wc(sr, f, q); self.set(a, 0.0, -a, 1.0 + a, -2.0 * c, 1.0 - a); }
    pub fn peak(sr: f32, f: f32, q: f32, gain_db: f32) -> Self {
        let mut b = Biquad::default(); let a_ = powf(10.0, gain_db / 40.0); let (c, a) = Self::wc(sr, f, q);
        b.set(1.0 + a * a_, -2.0 * c, 1.0 - a * a_, 1.0 + a / a_, -2.0 * c, 1.0 - a / a_); b
    }
    pub fn low_shelf(sr: f32, f: f32, gain_db: f32) -> Self {
        let mut b = Biquad::default(); let a_ = powf(10.0, gain_db / 40.0); let w = TAU * f / sr; let (c, s) = (cosf(w), sinf(w));
        let al = s / 2.0 * sqrtf(2.0); let sa = 2.0 * sqrtf(a_) * al;
        b.set(a_ * ((a_ + 1.0) - (a_ - 1.0) * c + sa), 2.0 * a_ * ((a_ - 1.0) - (a_ + 1.0) * c), a_ * ((a_ + 1.0) - (a_ - 1.0) * c - sa),
              (a_ + 1.0) + (a_ - 1.0) * c + sa, -2.0 * ((a_ - 1.0) + (a_ + 1.0) * c), (a_ + 1.0) + (a_ - 1.0) * c - sa); b
    }
    #[inline] pub fn run(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1; self.z1 = self.b1 * x - self.a1 * y + self.z2; self.z2 = self.b2 * x - self.a2 * y;
        if !y.is_finite() { self.z1 = 0.0; self.z2 = 0.0; return 0.0; } y
    }
}

/// Zero-delay-feedback state variable filter, safe to modulate every sample. Returns (low, band, high).
#[derive(Clone, Copy, Default)]
pub struct Svf { ic1: f32, ic2: f32 }
impl Svf {
    #[inline] pub fn run(&mut self, x: f32, sr: f32, f: f32, q: f32) -> (f32, f32, f32) {
        let g = ftan(PI * clampf(f, 10.0, sr * 0.38) / sr); let k = 1.0 / q.max(0.05);
        let a1 = 1.0 / (1.0 + g * (g + k)); let a2 = g * a1; let a3 = g * a2;
        let v3 = x - self.ic2; let v1 = a1 * self.ic1 + a2 * v3; let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1; self.ic2 = 2.0 * v2 - self.ic2;
        (v2, v1, x - k * v1 - v2)
    }
}

/// Circular delay line with fractional reads
pub struct Delay { buf: Vec<f32>, w: usize }
impl Delay {
    pub fn new(n: usize) -> Self { Delay { buf: vec![0.0; n.max(4)], w: 0 } }
    #[inline] pub fn push(&mut self, x: f32) { self.buf[self.w] = x; self.w += 1; if self.w == self.buf.len() { self.w = 0; } }
    /// d samples ago (1 = the sample just pushed)
    #[inline] fn at(&self, back: usize) -> f32 { let n = self.buf.len(); let i = if self.w >= back { self.w - back } else { self.w + n - back }; unsafe { *self.buf.get_unchecked(i) } }
    #[inline] pub fn tap(&self, d: usize) -> f32 { let n = self.buf.len(); self.at(d.min(n - 1).max(1)) }
    #[inline] pub fn read(&self, d: f32) -> f32 {
        let n = self.buf.len(); let d = clampf(d, 1.0, (n - 2) as f32); let i = d as usize; let fr = d - i as f32;
        let a = self.at(i); let b = self.at(i + 1); a + (b - a) * fr
    }
}

/// Schroeder all-pass for diffusion
pub struct Allpass { d: Delay, n: usize, g: f32 }
impl Allpass {
    pub fn new(n: usize, g: f32) -> Self { Allpass { d: Delay::new(n + 2), n, g } }
    #[inline] pub fn run(&mut self, x: f32) -> f32 { let v = self.d.tap(self.n); let u = x + self.g * v; self.d.push(u); v - self.g * u }
}

/// A decoded recording: one or two channels at its own rate. Looping recordings get their seam crossfaded at load time.
pub struct Sample { pub l: Vec<f32>, pub r: Vec<f32>, pub step: f32 }
impl Sample {
    pub fn new(l: Vec<f32>, r: Option<Vec<f32>>, rate: f32, sr: f32, looped: bool) -> Self {
        let mut l = l; let mut r = r.unwrap_or_default();
        if looped && l.len() > 4000 {
            let xf = (rate * 0.12) as usize; let n = l.len();
            if n > xf * 3 { for i in 0..xf { let t = i as f32 / xf as f32; l[i] = l[i] * t + l[n - xf + i] * (1.0 - t); if !r.is_empty() { r[i] = r[i] * t + r[n - xf + i] * (1.0 - t); } }
                l.truncate(n - xf); if !r.is_empty() { r.truncate(n - xf); } }
        }
        Sample { l, r, step: rate / sr }
    }
    pub fn len(&self) -> usize { self.l.len() }
    pub fn is_empty(&self) -> bool { self.l.len() < 4 }
}
/// Plays a sample with variable rate (linear interpolation)
#[derive(Clone, Copy, Default)]
pub struct Player { pub pos: f64, pub done: bool }
impl Player {
    #[inline] pub fn tick(&mut self, s: &Sample, rate: f32, looped: bool) -> (f32, f32) {
        let n = s.len(); if n < 4 || self.done { return (0.0, 0.0); }
        let i = self.pos as usize; let fr = (self.pos - i as f64) as f32; let j = if i + 1 >= n { if looped { 0 } else { n - 1 } } else { i + 1 };
        let l = s.l[i] + (s.l[j] - s.l[i]) * fr; let r = if s.r.len() == n { s.r[i] + (s.r[j] - s.r[i]) * fr } else { l };
        self.pos += (s.step * rate.max(0.0)) as f64;
        if self.pos >= n as f64 { if looped { self.pos -= n as f64; } else { self.done = true; } }
        (l, r)
    }
}

/// Places a mono source in the stereo field relative to the listener: equal-power pan, interaural delay,
/// distance attenuation, air absorption and a darker sound from behind. Targets are set once per block and
/// reached with linear ramps over the next block.
pub struct Spat { d: Delay, lp: OnePole, rear: OnePole, gl: f32, gr: f32, dgl: f32, dgr: f32, il: usize, ir: usize, pub send: f32, dsend: f32, da: f32, dra: f32, first: bool }
impl Spat {
    pub fn new(sr: f32) -> Self {
        Spat { d: Delay::new((sr * 0.002) as usize + 8), lp: OnePole::new(sr, 18000.0), rear: OnePole::new(sr, 18000.0), gl: 0.0, gr: 0.0, dgl: 0.0, dgr: 0.0,
               il: 1, ir: 1, send: 0.0, dsend: 0.0, da: 0.0, dra: 0.0, first: true }
    }
    /// right/fwd/up: source offset in the listener frame (metres); gain: source level; refd: distance at which the level halves
    pub fn aim(&mut self, sr: f32, right: f32, fwd: f32, up: f32, gain: f32, refd: f32, send0: f32) {
        const N: f32 = 1.0 / 256.0;
        let d = sqrtf(right * right + fwd * fwd + up * up).max(0.3);
        let s = clampf(right / d, -1.0, 1.0); let p = (s * 0.92 + 1.0) * PI / 4.0;
        let att = gain * refd / (refd + d);
        let (tl, tr) = (cosf(p) * att, sinf(p) * att);
        let itd = (fabsf(s) * 0.00063 * sr) as usize; self.il = 1 + if s > 0.0 { itd } else { 0 }; self.ir = 1 + if s < 0.0 { itd } else { 0 };
        let af = clampf(19000.0 / (1.0 + d / 140.0), 900.0, 19000.0); let ta = 1.0 - expf(-TAU * af.min(sr * 0.49) / sr);
        let rf = if fwd < 0.0 { lerp(19000.0, 5500.0, clampf(-fwd / d, 0.0, 1.0)) } else { 19000.0 }; let tra = 1.0 - expf(-TAU * rf.min(sr * 0.49) / sr);
        let tsend = clampf(send0 + 0.55 * d / (d + 40.0), 0.0, 1.0) * gain;
        if self.first { self.first = false; self.gl = tl; self.gr = tr; self.send = tsend; self.lp.a = ta; self.rear.a = tra; }
        self.dgl = (tl - self.gl) * N; self.dgr = (tr - self.gr) * N; self.dsend = (tsend - self.send) * N; self.da = (ta - self.lp.a) * N; self.dra = (tra - self.rear.a) * N;
    }
    #[inline] pub fn run(&mut self, x: f32, _sr: f32, _k: f32) -> (f32, f32) {
        self.gl += self.dgl; self.gr += self.dgr; self.send += self.dsend; self.lp.a += self.da; self.rear.a += self.dra;
        let y = self.rear.lp(self.lp.lp(x)); self.d.push(y);
        (self.d.tap(self.il) * self.gl, self.d.tap(self.ir) * self.gr)
    }
}

/// Feedback-delay-network reverb (8 lines, Hadamard mixing, damped), with input diffusion and optional discrete echoes
/// (canyon walls, valley sides) that feed back into the network.
pub struct Reverb {
    lines: Vec<Delay>, lens: [f32; 8], g: [f32; 8], damp: [OnePole; 8], ap: Vec<Allpass>, pre: Delay, predelay: f32,
    echo: Delay, taps: [(f32, f32, f32); 4], pub wet: f32, mod_ph: f32,
}
impl Reverb {
    pub fn new(sr: f32) -> Self {
        let lines = (0..8).map(|_| Delay::new((sr * 0.2) as usize)).collect();
        let ap = [0.0047f32, 0.0036, 0.0127, 0.0093].iter().map(|t| Allpass::new((t * sr) as usize, 0.62)).collect();
        let mut r = Reverb { lines, lens: [0.0; 8], g: [0.0; 8], damp: [OnePole::default(); 8], ap, pre: Delay::new((sr * 0.1) as usize), predelay: 1.0,
            echo: Delay::new((sr * 1.6) as usize), taps: [(0.0, 0.0, 0.0); 4], wet: 0.1, mod_ph: 0.0 };
        r.configure(sr, 0.7, 1.4, 0.4, 0.012, 0.1, &[]); r
    }
    /// size 0..1 scales the line lengths, t60 seconds, damp 0..1 (more = darker), predelay seconds, echoes: (delay s, gain, pan -1..1)
    pub fn configure(&mut self, sr: f32, size: f32, t60: f32, damp: f32, predelay: f32, wet: f32, echoes: &[(f32, f32, f32)]) {
        let base = [0.0297f32, 0.0371, 0.0411, 0.0437, 0.0533, 0.0617, 0.0719, 0.0823];
        for i in 0..8 { let l = base[i] * (0.45 + size * 1.1) * sr; self.lens[i] = l; self.g[i] = powf(10.0, -3.0 * l / (t60.max(0.2) * sr)); self.damp[i].set(sr, lerp(14000.0, 2200.0, damp)); }
        self.predelay = (predelay * sr).max(1.0); self.wet = wet;
        self.taps = [(0.0, 0.0, 0.0); 4]; for (i, e) in echoes.iter().take(4).enumerate() { self.taps[i] = (e.0 * sr, e.1, e.2); }
    }
    #[inline] pub fn run(&mut self, l: f32, r: f32, sr: f32) -> (f32, f32) {
        self.pre.push((l + r) * 0.5); let mut x = self.pre.read(self.predelay);
        for a in self.ap.iter_mut() { x = a.run(x); }
        self.mod_ph += 0.37 / sr; if self.mod_ph > 1.0 { self.mod_ph -= 1.0; } let m = fsin(self.mod_ph) * 6.0;
        let mut o = [0.0f32; 8];
        for i in 0..8 { o[i] = if i < 2 { self.lines[i].read(self.lens[i] + if i == 0 { m } else { -m }) } else { self.lines[i].tap(self.lens[i] as usize) }; }
        // fast Walsh-Hadamard (orthogonal after scaling)
        let mut h = o; let mut len = 1; while len < 8 { let mut i = 0; while i < 8 { for j in i..i + len { let a = h[j]; let b = h[j + len]; h[j] = a + b; h[j + len] = a - b; } i += len * 2; } len *= 2; }
        let sc = 0.353_553_4;
        // discrete echoes from the scenery
        self.echo.push(x + (o[0] + o[3]) * 0.15);
        let (mut el, mut er) = (0.0, 0.0);
        for t in self.taps.iter() { if t.1 > 0.0 { let v = self.echo.read(t.0) * t.1; el += v * (1.0 - t.2) * 0.5 * 1.4; er += v * (1.0 + t.2) * 0.5 * 1.4; } }
        for i in 0..8 { let fb = self.damp[i].lp(h[i] * sc * self.g[i]); self.lines[i].push(fb + x * 0.35 + (el + er) * 0.25); }
        let ol = o[0] + o[2] + o[4] + o[6]; let or = o[1] + o[3] + o[5] + o[7];
        ((ol * 0.5 + el) * self.wet, (or * 0.5 + er) * self.wet)
    }
}
