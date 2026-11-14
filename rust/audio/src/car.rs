//! The car: an inline-four engine model (firing pulses through exhaust resonances, combustion rasp, induction roar,
//! turbo spool and blow-off, overrun crackle, straight-cut gear whine) layered with the engine recording played
//! a little lower for body, plus tyres on each surface, wind rush and one-shot impacts.
use crate::dsp::*;

pub const S_ENGINE: usize = 2;
pub const S_SKID: usize = 3;
pub const S_IMPACT: usize = 4;
pub const S_SPLASH: usize = 8;
pub const S_METAL: usize = 9;
pub const S_PLANK: usize = 10;
pub const S_SOFT: usize = 11;
pub const S_WATER_A: usize = 5;
/// The page plays the engine and tyres itself (the build 16 recordings); this engine then only adds water and one-shots.
pub const PAGE_CAR: bool = true;

#[derive(Clone, Copy, Default)]
pub struct CarIn { pub rpm: f32, pub thr: f32, pub speed: f32, pub slide: f32, pub on_road: f32, pub surface: u32, pub wet: f32, pub air: f32,
    pub gear: f32, pub cam: u32, pub shifting: f32, pub brake: f32, pub rough: f32, pub drive: bool }

/// surfaces: 0 gravel, 1 snow, 2 sand, 3 tarmac, 4 black gravel (iceland)
pub struct Car {
    sr: f32, rng: Rng,
    // smoothed controls
    rpm: f32, thr: f32, load: f32, spd: f32, slide: f32, wet: f32, air: f32, road: f32,
    // engine
    cyc: f32, last_cyl: i32, cyl_amp: [f32; 4], pulse: f32, burst: f32, ex1: Biquad, ex2: Biquad, ex3: Biquad, muff: Biquad, growl: Biquad, rasp: Biquad,
    shelf: Biquad, intake: Svf, whine_ph: f32, boost: f32, turbo_ph: f32, bov: f32, bov_f: Biquad, prev_thr: f32, crackle_cool: f32,
    eng_a: Player, eng_b: Player, eng_lp: Biquad, sub_lp: Biquad, dc: OnePole,
    // tyres and air
    crunch: Biquad, crunch_lo: Biquad, hiss: Svf, rumble: Biquad, squeal_ph: f32, squeal_f: f32, skid: Player, skid_lp: Biquad, rush: Svf, water: Player, spray: Biquad,
    // one-shots
    shots: [(usize, Player, f32, f32, bool); 8], thump: f32, thump_ph: f32, clunk: f32, upd: u32, kf: f32, ks: f32, kup: f32, kdn: f32,
}

impl Car {
    pub fn new(sr: f32) -> Self {
        Car { sr, rng: Rng(0x9e37_79b9), rpm: 900.0, thr: 0.0, load: 0.0, spd: 0.0, slide: 0.0, wet: 0.0, air: 0.0, road: 1.0,
            cyc: 0.0, last_cyl: 0, cyl_amp: [1.0, 0.9, 1.07, 0.95], pulse: 0.0, burst: 0.0,
            ex1: Biquad::bp(sr, 95.0, 2.2), ex2: Biquad::bp(sr, 210.0, 2.8), ex3: Biquad::bp(sr, 520.0, 2.0), muff: Biquad::lp(sr, 1700.0, 0.7), growl: Biquad::lp(sr, 140.0, 0.9),
            rasp: Biquad::bp(sr, 780.0, 0.9), shelf: Biquad::low_shelf(sr, 160.0, 6.0), intake: Svf::default(), whine_ph: 0.0, boost: 0.0, turbo_ph: 0.0, bov: 0.0,
            bov_f: Biquad::bp(sr, 3200.0, 1.4), prev_thr: 0.0, crackle_cool: 0.0, eng_a: Player::default(), eng_b: Player { pos: 7000.0, done: false },
            eng_lp: Biquad::lp(sr, 2600.0, 0.7), sub_lp: Biquad::lp(sr, 230.0, 0.8), dc: OnePole::new(sr, 18.0),
            crunch: Biquad::bp(sr, 2300.0, 0.9), crunch_lo: Biquad::bp(sr, 650.0, 1.0), hiss: Svf::default(), rumble: Biquad::lp(sr, 110.0, 0.8),
            squeal_ph: 0.0, squeal_f: 1100.0, skid: Player::default(), skid_lp: Biquad::lp(sr, 2400.0, 0.7), rush: Svf::default(), water: Player::default(),
            spray: Biquad::hp(sr, 1800.0, 0.7), shots: [(0, Player { pos: 0.0, done: true }, 0.0, 1.0, false); 8], thump: 0.0, thump_ph: 0.0, clunk: 0.0, upd: 0,
            kf: 1.0 - expf(-1.0 / (0.025 * sr)), ks: 1.0 - expf(-1.0 / (0.12 * sr)), kup: 1.0 - expf(-1.0 / (0.7 * sr)), kdn: 1.0 - expf(-1.0 / (0.25 * sr)) }
    }

    /// one-shot: sample id, rate, gain, low-pass flag
    pub fn shot(&mut self, id: usize, lo: f32, hi: f32, gain: f32, dark: bool) {
        let rate = self.rng.range(lo, hi); let mut slot = 0; for i in 0..8 { if self.shots[i].1.done { slot = i; break; } }
        self.shots[slot] = (id, Player { pos: 0.0, done: false }, gain, rate, dark);
    }
    pub fn land(&mut self, v: f32) { self.thump = clampf(v / 7.0, 0.1, 1.0); self.thump_ph = 0.0; if v > 2.5 { self.shot(S_IMPACT, 0.55, 0.7, clampf(v / 9.0, 0.2, 0.9), true); } }
    pub fn impact(&mut self, s: f32, mat: u32) {
        let g = clampf(s / 9.0, 0.2, 1.1);
        match mat { 1 => { self.shot(S_METAL, 0.85, 1.05, g, false); self.shot(S_IMPACT, 0.7, 0.85, g * 0.6, true); }
                    2 => { self.shot(S_PLANK, 0.8, 1.0, g, false); }
                    3 => { self.shot(S_SOFT, 0.8, 1.0, g, false); }
                    _ => { self.shot(S_IMPACT, 0.75, 1.0, g, false); } }
        self.thump = self.thump.max(g * 0.6); self.thump_ph = 0.0;
    }
    pub fn shift(&mut self) { self.clunk = 1.0; }
    pub fn splash(&mut self, v: f32) { self.shot(S_SPLASH, 0.8, 1.1, clampf(v / 18.0, 0.2, 0.7), false); }

    /// water churn in a ford plus impacts, landings and splashes
    fn fx_only(&mut self, w: f32, smp: &[Option<crate::dsp::Sample>]) -> f32 {
        let sr = self.sr; let spd = self.spd;
        let mut out = 0.0;
        if self.wet > 0.001 {
            let (wa, _) = if let Some(s) = &smp[S_WATER_A] { self.water.tick(s, 1.0 + spd * 0.02, true) } else { (0.0, 0.0) };
            out += (wa * 0.7 + self.spray.run(w) * 0.25) * self.wet * smooth(1.0, 8.0, spd);
        }
        for s in self.shots.iter_mut() { if s.1.done { continue; } if let Some(sm) = &smp[s.0] { let (a, _) = s.1.tick(sm, s.3, false); out += a * s.2 * if s.4 { 0.8 } else { 1.0 }; } else { s.1.done = true; } }
        if self.thump > 0.001 { self.thump_ph += 52.0 / sr; out += fsin(self.thump_ph) * self.thump * 0.5; self.thump *= 1.0 - 9.0 / sr; }
        out
    }

    /// Returns (engine, tyres) mono signals at the car, already balanced for the camera mode.
    #[inline]
    pub fn tick(&mut self, i: &CarIn, smp: &[Option<crate::dsp::Sample>]) -> (f32, f32) {
        let sr = self.sr; let k = self.kf; let ks = self.ks;
        self.rpm += (i.rpm.max(600.0) - self.rpm) * k * 2.0; self.thr += (i.thr - self.thr) * k;
        self.load += ((i.thr * 0.85 + 0.15 * clampf((i.rpm - self.rpm) / 400.0, 0.0, 1.0)) - self.load) * k;
        self.spd += (i.speed - self.spd) * ks; self.slide += (i.slide - self.slide) * k; self.wet += (i.wet - self.wet) * ks; self.air += (i.air - self.air) * k * 2.0;
        self.road += (i.on_road - self.road) * ks;
        let rpm = self.rpm; let load = self.load; let w = self.rng.bi();
        if PAGE_CAR { return (0.0, self.fx_only(w, smp)); }

        // ---- firing pulses: four cylinders over two crank revolutions, slightly uneven like a real engine
        self.cyc += rpm / 120.0 / sr; if self.cyc >= 1.0 { self.cyc -= 1.0; }
        let cyl = (self.cyc * 4.0) as i32;
        if cyl != self.last_cyl {
            self.last_cyl = cyl; let c = cyl.clamp(0, 3) as usize;
            let a = self.cyl_amp[c] * (0.32 + 0.68 * load) * (1.0 + self.rng.bi() * 0.09);
            self.pulse += a; self.burst = self.burst.max(a);
            // overrun: crackles and pops when the throttle is shut at high revs
            if i.thr < 0.04 && rpm > 3300.0 && self.crackle_cool <= 0.0 && self.rng.f() < 0.05 + 0.1 * clampf((rpm - 3300.0) / 3000.0, 0.0, 1.0) {
                self.pulse += self.rng.range(1.5, 3.2); self.burst = 2.0; self.crackle_cool = self.rng.range(0.02, 0.09); }
        }
        self.crackle_cool -= 1.0 / sr;
        let x = self.pulse; self.pulse *= 0.6;   // ~1 ms pulse
        // exhaust: pipe resonances that open up with revs, then the silencer
        let fr = rpm / 7000.0;
        self.upd = self.upd.wrapping_add(1); if self.upd % 64 == 0 { self.ex1.set_bp(sr, 85.0 + fr * 45.0, 2.0); self.ex2.set_bp(sr, 190.0 + fr * 90.0, 2.6); self.ex3.set_bp(sr, 470.0 + fr * 180.0, 1.8); }
        let ex = self.ex1.run(x) * 1.7 + self.ex2.run(x) * 1.1 + self.ex3.run(x) * 0.5;
        let growl = self.growl.run(x) * 2.6;
        self.burst *= 0.9965; let rasp = self.rasp.run(w * self.burst) * (0.35 + 0.65 * load);
        let mut eng = self.muff.run(ex + growl + rasp * 0.9);
        // induction roar
        let (_, ib, _) = self.intake.run(w, sr, 700.0 + rpm * 0.22, 1.6);
        let intake = ib * self.thr * (0.25 + 0.75 * fr) * 0.55;
        // turbo: spools with load above ~2500 rpm, whistles, sneezes through the blow-off valve when the throttle shuts
        let bt = i.thr * smooth(2300.0, 5200.0, rpm); self.boost += (bt - self.boost) * if bt > self.boost { self.kup } else { self.kdn };
        if self.prev_thr > 0.6 && i.thr < 0.15 && self.boost > 0.35 { self.bov = self.boost; }
        self.prev_thr = i.thr;
        self.turbo_ph += (1900.0 + self.boost * 2600.0) / sr; if self.turbo_ph > 1.0 { self.turbo_ph -= 1.0; }
        let turbo = fsin(self.turbo_ph) * self.boost * self.boost * 0.035;
        self.bov *= 1.0 - 4.0 / sr; let bov = self.bov_f.run(w) * self.bov * 0.5;
        // straight-cut gearbox whine, strongest in the low gears under load
        self.whine_ph += rpm * (0.13 + 0.02 * i.gear) / sr; if self.whine_ph > 1.0 { self.whine_ph -= 1.0; }
        let whine = fsin(self.whine_ph) * 0.012 * (0.3 + self.thr) * smooth(1.0, 6.0, self.spd) * (1.4 - i.gear.min(5.0) * 0.18);
        // the recording, pitched a touch lower for body, plus a sub-octave layer for depth
        let er = rpm / 3400.0;
        let (sa, _) = if let Some(s) = &smp[S_ENGINE] { self.eng_a.tick(s, er * 0.82, true) } else { (0.0, 0.0) };
        let (sb, _) = if let Some(s) = &smp[S_ENGINE] { self.eng_b.tick(s, er * 0.41, true) } else { (0.0, 0.0) };
        let rec = self.eng_lp.run(sa) * (0.4 + 0.35 * load) + self.sub_lp.run(sb) * 0.55;
        eng = eng * 0.85 + rec;
        eng = self.shelf.run(eng);
        // engine-bay vs tailpipe balance per camera
        let (ke, ki) = match i.cam { 2 => (0.6, 1.5), 1 => (1.0, 0.8), _ => (1.0, 0.6) };
        let drive = 1.6 + load * 1.4; let sig = eng * ke + intake * ki + turbo + bov + whine;
        let mut e = ftanh(sig * drive) / drive * 1.3;
        e = e - self.dc.lp(e);

        // ---- tyres
        let ground = 1.0 - self.air; let spd = self.spd; let road = self.road;
        let (gravelish, snowy, sandy, tarmac) = match i.surface { 1 => (0.2, 1.0, 0.0, 0.0), 2 => (0.4, 0.0, 1.0, 0.0), 3 => (0.0, 0.0, 0.0, 1.0), _ => (1.0, 0.0, 0.0, 0.0) };
        let off = 1.0 - road;
        // gravel: dense random grains, rate grows with speed
        let rate = spd * (18.0 + 30.0 * gravelish + 20.0 * off) * (1.0 - tarmac * 0.9 * road);
        let grain = if self.rng.f() < rate / sr { self.rng.bi() * self.rng.range(0.4, 1.0) } else { 0.0 };
        let crunch = (self.crunch.run(grain) * 1.4 + self.crunch_lo.run(grain) * 0.8) * (gravelish + off * 0.6 + sandy * 0.3) * smooth(0.5, 6.0, spd);
        let (hl, hb, _) = self.hiss.run(w, sr, if tarmac > 0.0 { 1500.0 } else if snowy > 0.0 { 700.0 } else { 2600.0 }, 0.7);
        let hiss = (if snowy > 0.0 { hl * 0.6 } else { hb }) * (0.004 + 0.0011 * spd) * spd * (0.35 + 0.65 * (sandy + snowy + tarmac * 0.6 + gravelish * 0.4));
        let rumble = self.rumble.run(w) * (0.02 + 0.03 * off + i.rough * 0.12) * spd * 0.12;
        // sliding: gravel scrabble (recording) or a proper tarmac squeal
        let sl = clampf((self.slide - 1.2) / 6.0, 0.0, 1.0) * ground;
        let (sk, _) = if let Some(s) = &smp[S_SKID] { self.skid.tick(s, 0.75 + sl * 0.2, true) } else { (0.0, 0.0) };
        let scrab = self.skid_lp.run(sk) * sl * (1.0 - tarmac * road) * 0.7;
        self.squeal_f += ((1050.0 + self.rng.bi() * 220.0 + sl * 300.0) - self.squeal_f) * 0.002;
        self.squeal_ph += self.squeal_f / sr; if self.squeal_ph > 1.0 { self.squeal_ph -= 1.0; }
        let sq = (fsin(self.squeal_ph) + 0.35 * fsin(self.squeal_ph * 1.53)) * (0.7 + 0.3 * w) * smooth(0.15, 0.6, sl) * tarmac * road * 0.09;
        // water: churning the ford
        let (wa, _) = if let Some(s) = &smp[S_WATER_A] { self.water.tick(s, 1.0 + spd * 0.02, true) } else { (0.0, 0.0) };
        let splash = (wa * 0.7 + self.spray.run(w) * 0.25) * self.wet * smooth(1.0, 8.0, spd);
        let tyres = (crunch + hiss + rumble + scrab + sq) * ground * (1.0 - self.wet * 0.7) + splash;
        // wind rush over the body
        let (_, rb, rh) = self.rush.run(w, sr, 900.0 + spd * 25.0, 0.5);
        let rush = (rb * 0.6 + rh * 0.25) * smooth(10.0, 45.0, spd) * smooth(10.0, 45.0, spd) * 0.11 * if i.cam == 2 { 0.6 } else { 1.0 };

        // ---- one-shots, landing thump, shift clunk
        let mut fx = 0.0;
        for s in self.shots.iter_mut() { if s.1.done { continue; } if let Some(sm) = &smp[s.0] { let (a, _) = s.1.tick(sm, s.3, false); fx += a * s.2 * if s.4 { 0.8 } else { 1.0 }; } else { s.1.done = true; } }
        if self.thump > 0.001 { self.thump_ph += 52.0 / sr; fx += fsin(self.thump_ph) * self.thump * 0.5; self.thump *= 1.0 - 9.0 / sr; }
        if self.clunk > 0.001 { fx += (w * 0.3 + fsin(self.whine_ph * 5.9) * 0.4) * self.clunk * 0.12; self.clunk *= 1.0 - 60.0 / sr; }
        let _ = i.drive;
        (e * 0.55, tyres + rush + fx)
    }
}
