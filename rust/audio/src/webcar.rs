//! The engine, tyres and rival engines exactly as the page played them with Web Audio (the build 16 recordings):
//! looped recordings with smoothed playback rates, biquads, tanh wave shapers, a tunnel echo, exhaust pops, a compressor.
//! Message 6 sets your car's parameters, 7 fires events (1 pop, 2 gear-shift dip), 8 sets the rival engines.
use crate::dsp::*;

const S_WIND: usize = 0; const S_ENGINE: usize = 2; const S_SKID: usize = 3; const S_IMPACT: usize = 4;

/// AudioParam.setTargetAtTime: exponential approach with time constant tau
#[derive(Clone, Copy)]
struct P { v: f32, t: f32, k: f32 }
impl P {
    fn new(v: f32) -> P { P { v, t: v, k: 0.0 } }
    fn tau(&mut self, sr: f32, tau: f32) { self.k = 1.0 - expf(-1.0 / (tau * sr)); }
    #[inline] fn tick(&mut self) -> f32 { self.v += (self.t - self.v) * self.k; self.v }
}
/// Web Audio BiquadFilterNode lowpass/highpass: Q is in dB
fn qlin(qdb: f32) -> f32 { powf(10.0, qdb / 20.0) }

#[derive(Clone, Copy, Default)]
struct St { l: Biquad, r: Biquad }
impl St {
    fn lp(&mut self, sr: f32, f: f32, q: f32) { self.l.set_lp(sr, f, q); self.r.set_lp(sr, f, q); }
    fn hp(&mut self, sr: f32, f: f32, q: f32) { self.l.set_hp(sr, f, q); self.r.set_hp(sr, f, q); }
    #[inline] fn run(&mut self, x: (f32, f32)) -> (f32, f32) { (self.l.run(x.0), self.r.run(x.1)) }
}
#[inline] fn shape(x: f32, k: f32, tk: f32) -> f32 { tanhf(clampf(x, -1.0, 1.0) * k) / tk }

struct Rival { a: Player, b: Player, rate: P, hi: P, gain: P, lp: P, pan: P, f: St, live: bool }

pub struct WebCar {
    sr: f32, started: bool, n: u32,
    low: Player, high: Player, skid: Player, rush: Player, rum: Player,
    rate: P, low_g: P, high_g: P, lp_f: P, eng_g: P, skid_g: P, tun: P, skid_rate: P, skid_lp: P, rum_g: P, rum_rate: P, rush_g: P, rush_rate: P, master: P,
    hp350: St, eng_lp: St, skid_f: St, rush_hp: St, rum_lp: St, tun_lp: St, dly_l: Delay, dly_r: Delay, dly_n: f32,
    dip: f32, dip_from: f32, shifting: bool,
    pops: [(Player, f32, f32, St, f32); 4],
    rivals: Vec<Rival>, rng: Rng,
    env: f32, att: f32, rel: f32, lim: f32, lim_rel: f32,
}

impl WebCar {
    pub fn new(sr: f32) -> WebCar {
        let mut w = WebCar { sr, started: false, n: 0,
            low: Player::default(), high: Player::default(), skid: Player::default(), rush: Player::default(), rum: Player::default(),
            rate: P::new(0.95), low_g: P::new(1.0), high_g: P::new(0.0), lp_f: P::new(1400.0), eng_g: P::new(0.55), skid_g: P::new(0.0), tun: P::new(0.0),
            skid_rate: P::new(0.8), skid_lp: P::new(2400.0), rum_g: P::new(0.0), rum_rate: P::new(0.42), rush_g: P::new(0.0), rush_rate: P::new(1.7), master: P::new(0.0),
            hp350: St::default(), eng_lp: St::default(), skid_f: St::default(), rush_hp: St::default(), rum_lp: St::default(), tun_lp: St::default(),
            dly_l: Delay::new((sr * 0.5) as usize), dly_r: Delay::new((sr * 0.5) as usize), dly_n: sr * 0.055,
            dip: -1.0, dip_from: 0.55, shifting: false,
            pops: core::array::from_fn(|_| (Player { pos: 0.0, done: true }, 0.0, 0.0, St::default(), 0.0)),
            rivals: Vec::new(), rng: Rng(0x2468_ace1), env: 0.0, att: 1.0 - expf(-1.0 / (0.003 * sr)), rel: 1.0 - expf(-1.0 / (0.25 * sr)), lim: 1.0, lim_rel: 1.0 - expf(-1.0 / (0.12 * sr)) };
        for (p, t) in [(&mut w.rate, 0.02), (&mut w.low_g, 0.05), (&mut w.high_g, 0.05), (&mut w.lp_f, 0.06), (&mut w.eng_g, 0.08), (&mut w.skid_g, 0.12), (&mut w.tun, 0.08),
            (&mut w.skid_rate, 0.1), (&mut w.skid_lp, 0.1), (&mut w.rum_g, 0.1), (&mut w.rum_rate, 0.1), (&mut w.rush_g, 0.15), (&mut w.rush_rate, 0.2), (&mut w.master, 0.6)] { p.tau(sr, t); }
        w.hp350.hp(sr, 350.0, qlin(1.0)); w.rush_hp.hp(sr, 700.0, qlin(1.0)); w.rum_lp.lp(sr, 520.0, qlin(1.0)); w.tun_lp.lp(sr, 2200.0, qlin(1.0));
        w.eng_lp.lp(sr, 1400.0, qlin(0.7)); w.skid_f.lp(sr, 2400.0, qlin(1.0));
        w
    }
    fn start(&mut self, smp: &[Option<Sample>]) {
        self.started = true; let sr = self.sr;
        let off = |id: usize, secs: f64| -> f64 { smp[id].as_ref().map(|s| { let n = s.len() as f64; if n > 0.0 { (secs * s.step as f64 * sr as f64) % n } else { 0.0 } }).unwrap_or(0.0) };
        self.high.pos = off(S_ENGINE, 1.3); self.rush.pos = off(S_WIND, 20.0); self.rum.pos = off(S_SKID, 1.1);
        self.master.t = 0.0;
    }
    /// [6, rpmShown, thr, slide, surface(1 snow 2 sand 3 tarmac 4 gravel road 0 off road), speed01, onRoad, tunnel, shifting, master]
    pub fn params(&mut self, f: &dyn Fn(usize) -> f32) {
        let (rpm, thr, slide, surf, sp, on_road, tun, shifting, master) = (f(1), f(2), f(3), f(4) as i32, f(5), f(6) > 0.5, f(7) > 0.5, f(8) > 0.5, f(9));
        let n = clampf((rpm - 900.0) / 6400.0, 0.0, 1.05); let rate = 0.95 + n * 2.6;
        self.rate.t = rate; self.low_g.t = 0.85 - n * 0.35; self.high_g.t = 0.1 + n * 0.75 * (0.5 + thr * 0.5); self.lp_f.t = 900.0 + thr * 3800.0 + n * 2200.0;
        self.shifting = shifting; if !shifting && self.dip < 0.0 { self.eng_g.t = 0.42 + thr * 0.2 + n * 0.08; }
        let (snow, sand, tar) = (surf == 1, surf == 2, surf == 3);
        self.skid_g.t = slide * slide * if snow { 0.2 } else if sand { 0.3 } else if tar { 0.16 } else if on_road { 0.4 } else { 0.22 };
        self.tun.t = if tun { 0.55 } else { 0.0 };
        self.skid_rate.t = if snow { 0.5 + slide * 0.1 } else if sand { 0.62 + slide * 0.15 } else if on_road { 0.76 + slide * 0.22 } else { 0.6 };
        self.skid_lp.t = if snow { 900.0 } else if sand { 1500.0 } else if on_road { 2600.0 } else { 1100.0 };
        self.rum_g.t = sp * if on_road { 0.3 } else { 0.45 }; self.rum_rate.t = 0.32 + sp * 0.3;
        self.rush_g.t = sp * sp * 1.4; self.rush_rate.t = 1.3 + sp * 0.9;
        if master > 0.0 && !self.started { self.master.tau(self.sr, 0.6); } else { self.master.tau(self.sr, 0.15); }
        self.master.t = master;
    }
    /// [7, 1, rate1, rate2]: exhaust pops; [7, 2]: gear-shift dip
    pub fn event(&mut self, f: &dyn Fn(usize) -> f32) {
        match f(1) as i32 {
            1 => { let sr = self.sr; let slot = |rate: f32, gain: f32, hp: f32, delay: f32, pops: &mut [(Player, f32, f32, St, f32); 4]| {
                    if let Some(p) = pops.iter_mut().find(|p| p.0.done) { p.0 = Player { pos: 0.0, done: false }; p.1 = rate; p.2 = gain; p.3 = St::default(); p.3.hp(sr, hp, qlin(1.0)); p.4 = delay * sr; } };
                slot(f(2), 0.22, 500.0, 0.0, &mut self.pops); if f(3) > 0.0 { slot(f(3), 0.15, 600.0, 0.09, &mut self.pops); } }
            2 => { self.dip = 0.0; self.dip_from = self.eng_g.v; }
            _ => {}
        }
    }
    /// [8, n, (rival, rate, hiGain, gain, lowpass, pan) * n]
    pub fn rivals(&mut self, f: &dyn Fn(usize) -> f32, smp: &[Option<Sample>]) {
        let sr = self.sr; let n = f(1) as usize;
        for r in self.rivals.iter_mut() { r.live = false; r.gain.t = 0.0; }
        for k in 0..n { let b = 2 + k * 6; let id = f(b) as usize; if id > 7 { continue; }
            while self.rivals.len() <= id {
                let len = smp[S_ENGINE].as_ref().map(|s| s.len() as f64).unwrap_or(1.0).max(1.0);
                let mut r = Rival { a: Player { pos: self.rng.f() as f64 * 1.5 * sr as f64 % len, done: false }, b: Player { pos: self.rng.f() as f64 * 1.5 * sr as f64 % len, done: false },
                    rate: P::new(1.0), hi: P::new(0.0), gain: P::new(0.0), lp: P::new(1500.0), pan: P::new(0.0), f: St::default(), live: false };
                for (p, t) in [(&mut r.rate, 0.04), (&mut r.hi, 0.06), (&mut r.gain, 0.06), (&mut r.lp, 0.08), (&mut r.pan, 0.05)] { p.tau(sr, t); }
                r.f.lp(sr, 1500.0, qlin(1.0)); self.rivals.push(r); }
            let r = &mut self.rivals[id]; r.live = true; r.rate.t = f(b + 1); r.hi.t = f(b + 2); r.gain.t = f(b + 3); r.lp.t = f(b + 4); r.pan.t = f(b + 5);
        }
    }

    /// one stereo frame
    #[inline]
    pub fn tick(&mut self, smp: &[Option<Sample>]) -> (f32, f32) {
        let (Some(eng), Some(skid)) = (smp[S_ENGINE].as_ref(), smp[S_SKID].as_ref()) else { return (0.0, 0.0) };
        if !self.started { if self.master.t <= 0.0 { return (0.0, 0.0); } self.start(smp); }
        let sr = self.sr; self.n = self.n.wrapping_add(1);
        let rate = self.rate.tick(); let lp_f = self.lp_f.tick(); let skid_lp = self.skid_lp.tick();
        if self.n % 32 == 0 { self.eng_lp.lp(sr, lp_f, qlin(0.7)); self.skid_f.lp(sr, skid_lp, qlin(1.0)); }
        // engine: low loop + high loop (high-passed) into the shaper, low-passed, engine gain
        let a = self.low.tick(eng, rate, true); let b = self.high.tick(eng, rate * 1.5, true); let lg = self.low_g.tick(); let hg = self.high_g.tick();
        let bh = self.hp350.run(b);
        let tk = tanhf(1.6); let x = (shape(a.0 * lg + bh.0 * hg, 1.6, tk), shape(a.1 * lg + bh.1 * hg, 1.6, tk));
        let e = self.eng_lp.run(x);
        let eg = if self.dip >= 0.0 { let t = self.dip; self.dip += 1.0 / sr;
            let v = if t < 0.05 { self.dip_from + (0.2 - self.dip_from) * t / 0.05 } else if t < 0.24 { 0.2 + 0.4 * (t - 0.05) / 0.19 } else { if !self.shifting { self.dip = -1.0; } 0.6 };
            self.eng_g.v = v; v } else { self.eng_g.tick() };
        let (el, er) = (e.0 * eg, e.1 * eg);
        // tyres, rumble, wind rush
        let s = self.skid.tick(skid, self.skid_rate.tick(), true); let s = self.skid_f.run(s); let sg = self.skid_g.tick(); let (sl, sr2) = (s.0 * sg, s.1 * sg);
        let rm = self.rum.tick(skid, self.rum_rate.tick(), true); let rm = self.rum_lp.run(rm); let rg = self.rum_g.tick();
        let rr_ = self.rush_rate.tick(); let ru = match smp[S_WIND].as_ref() { Some(wind) => self.rush.tick(wind, rr_, true), None => (0.0, 0.0) }; let ru = self.rush_hp.run(ru); let rsg = self.rush_g.tick();
        // tunnel echo: engine + tyres into a 55 ms feedback delay, low-passed
        let ts = self.tun.tick(); let dl = self.dly_l.read(self.dly_n); let dr = self.dly_r.read(self.dly_n);
        let t = self.tun_lp.run((dl, dr)); self.dly_l.push((el + sl) * ts + t.0 * 0.42); self.dly_r.push((er + sr2) * ts + t.1 * 0.42);
        let mut l = el + sl + rm.0 * rg + ru.0 * rsg + t.0; let mut r = er + sr2 + rm.1 * rg + ru.1 * rsg + t.1;
        // exhaust pops
        if let Some(imp) = smp[S_IMPACT].as_ref() { for p in self.pops.iter_mut() { if p.0.done { continue; } if p.4 > 0.0 { p.4 -= 1.0; continue; }
            let v = p.0.tick(imp, p.1, false); let v = p.3.run(v); l += v.0 * p.2; r += v.1 * p.2; } }
        // rival engines
        let tk2 = tanhf(2.2);
        for rv in self.rivals.iter_mut() { let g = rv.gain.tick(); if g < 1e-5 && !rv.live { continue; }
            let rt = rv.rate.tick(); let a = rv.a.tick(eng, rt, true); let b = rv.b.tick(eng, rt * 1.5, true); let hi = rv.hi.tick(); let lpf = rv.lp.tick();
            if self.n % 32 == 0 { rv.f.lp(sr, lpf, qlin(1.0)); }
            let x = rv.f.run((shape(a.0 + b.0 * hi, 2.2, tk2), shape(a.1 + b.1 * hi, 2.2, tk2)));
            let pan = rv.pan.tick(); let pp = (pan + 1.0) * 0.5 * PI * 0.5;
            // StereoPannerNode on a stereo input
            let (xl, xr) = (x.0 * g, x.1 * g);
            if pan <= 0.0 { let k = (pan + 1.0) * PI * 0.5; l += xl + xr * cosf(k); r += xr * sinf(k); }
            else { let k = pan * PI * 0.5; l += xl * cosf(k); r += xr + xl * sinf(k); }
            let _ = pp; }
        // master, then the compressor (threshold -14 dB, ratio 4, knee 30 dB, attack 3 ms, release 250 ms, auto make-up)
        let m = self.master.tick(); l *= m; r *= m;
        let pk = fabsf(l).max(fabsf(r)); let k = if pk > self.env { self.att } else { self.rel }; self.env += (pk - self.env) * k;
        let db_in = 20.0 * libm::log10f(self.env.max(1e-6)); let (t0, knee, ratio) = (-14.0, 30.0, 4.0);
        let db_out = if db_in <= t0 { db_in } else if db_in < t0 + knee { db_in + (1.0 / ratio - 1.0) * (db_in - t0) * (db_in - t0) / (2.0 * knee) } else { t0 + knee * (1.0 / ratio + 1.0) / 2.0 + (db_in - t0 - knee) / ratio };
        let gain = db(db_out - db_in) * 1.185; let (l, r) = (l * gain, r * gain);
        // and a peak limiter behind it, so a grid full of revving rivals rounds off instead of clipping
        let pk = fabsf(l).max(fabsf(r)); let want = if pk > 0.85 { 0.85 / pk } else { 1.0 };
        self.lim = if want < self.lim { want } else { self.lim + (want - self.lim) * self.lim_rel };
        (l * self.lim, r * self.lim)
    }
}
