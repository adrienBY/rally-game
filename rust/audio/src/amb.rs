//! Ambience: wind beds, rustling leaves, procedural wildlife placed around the listener, and the positional sounds
//! of each stage (waterfalls, rivers, steam vents, lake shores, cricket clusters, the town in the valley).
use crate::dsp::*;

pub const S_WIND: usize = 0;
pub const S_BIRDS: usize = 1;
pub const S_WATER_A: usize = 5;
pub const S_WATER_B: usize = 6;
pub const S_TRAFFIC: usize = 7;
pub const S_WIND2: usize = 12;

#[derive(Clone, Copy, Default, PartialEq)]
pub enum Kind { #[default] Off, Bird, Plover, Hawk, Loon, Owl, Cricket, Bell, Higurashi, Woodpecker }

/// A procedural animal (or bell) at a fixed spot in the world, singing phrases on its own schedule.
pub struct Voice {
    pub kind: Kind, pub x: f32, pub y: f32, pub z: f32, pub gain: f32, pub refd: f32, pub spat: Spat,
    t: f32, next: f32, note: i32, notes: i32, f0: f32, f1: f32, dur: f32, gap: f32, ph: f32, ph2: f32, env: f32, rng: Rng, base: f32, bp: Biquad,
    partials: [(f32, f32, f32); 7], pdec: [f32; 7], pamp: [f32; 7], quiet: u32,
}
impl Voice {
    pub fn new(sr: f32, seed: u32) -> Self {
        Voice { kind: Kind::Off, x: 0.0, y: 0.0, z: 0.0, gain: 0.0, refd: 10.0, spat: Spat::new(sr), t: 0.0, next: 1.0, note: 0, notes: 0, f0: 0.0, f1: 0.0, dur: 0.1,
            gap: 0.1, ph: 0.0, ph2: 0.0, env: 0.0, rng: Rng(seed | 1), base: 3000.0, bp: Biquad::bp(sr, 1200.0, 2.0), partials: [(0.0, 0.0, 0.0); 7], pdec: [0.0; 7], pamp: [0.0; 7], quiet: 0 }
    }
    pub fn spawn(&mut self, kind: Kind, x: f32, y: f32, z: f32, gain: f32, refd: f32, first: f32) {
        self.kind = kind; self.x = x; self.y = y; self.z = z; self.gain = gain; self.refd = refd; self.t = 0.0; self.next = first; self.notes = 0; self.note = 0; self.env = 0.0;
        self.base = match kind { Kind::Bird => self.rng.range(2300.0, 4800.0), Kind::Cricket => self.rng.range(3700.0, 5200.0), Kind::Higurashi => self.rng.range(4900.0, 5800.0),
            Kind::Owl => self.rng.range(340.0, 420.0), Kind::Loon => self.rng.range(650.0, 820.0), Kind::Plover => self.rng.range(2000.0, 2500.0), _ => 1.0 };
    }
    fn phrase(&mut self) {
        let r = &mut self.rng;
        match self.kind {
            Kind::Bird => { self.notes = 3 + (r.f() * 7.0) as i32; }
            Kind::Plover => { self.notes = 1 + (r.f() * 2.0) as i32; }
            Kind::Hawk => { self.notes = 1 + (r.f() * 2.0) as i32; }
            Kind::Loon => { self.notes = 3; }
            Kind::Owl => { self.notes = 3; }
            Kind::Cricket => { self.notes = if r.f() < 0.3 { 1 } else { 3 + (r.f() * 4.0) as i32 }; }
            Kind::Higurashi => { self.notes = 34 + (r.f() * 12.0) as i32; }
            Kind::Woodpecker => { self.notes = 12 + (r.f() * 8.0) as i32; }
            Kind::Bell => { self.notes = 1; let f = 98.0 * r.range(0.95, 1.05);
                self.partials = [(f, 1.0, 9.0), (f * 1.003, 0.6, 7.0), (f * 2.0, 0.45, 5.5), (f * 2.76, 0.4, 4.0), (f * 3.02, 0.25, 3.2), (f * 4.07, 0.18, 2.4), (f * 5.4, 0.12, 1.6)];
                for (i, pt) in self.partials.iter().enumerate() { self.pamp[i] = pt.1; self.pdec[i] = 1.0 - 1.0 / (pt.2 * 48000.0); } }
            Kind::Off => {}
        }
        self.note = 0; self.start_note();
    }
    fn start_note(&mut self) {
        let r = &mut self.rng; let b = self.base;
        let (f0, f1, dur, gap) = match self.kind {
            Kind::Bird => { let a = b * r.range(0.8, 1.35); (a, a * r.range(0.7, 1.4), r.range(0.04, 0.14), r.range(0.02, 0.09)) }
            Kind::Plover => (b, b * 0.82, r.range(0.35, 0.55), 0.25),
            Kind::Hawk => (3100.0, 2100.0, r.range(0.7, 1.1), 0.5),
            Kind::Loon => { let s = [(1.0, 1.55, 1.3), (1.55, 1.3, 1.6), (1.3, 1.45, 1.8)][self.note.clamp(0, 2) as usize]; (b * s.0, b * s.1, s.2, 0.15) }
            Kind::Owl => (b, b * 0.93, if self.note == 0 { 0.55 } else { 0.32 }, if self.note == 0 { 0.5 } else { 0.18 }),
            Kind::Cricket => (b, b, 0.016, 0.018),
            Kind::Higurashi => { let p = self.note as f32 / self.notes.max(1) as f32; let f = b * (1.0 - 0.18 * p); (f, f * 0.97, 0.03 + 0.03 * p, 0.022 + 0.06 * p) }
            Kind::Woodpecker => (1200.0, 1200.0, 0.006, 0.05),
            Kind::Bell => (98.0, 98.0, 14.0, 0.0),
            Kind::Off => (0.0, 0.0, 0.1, 0.1),
        };
        self.f0 = f0; self.f1 = f1; self.dur = dur; self.gap = gap; self.t = 0.0;
    }
    fn rest(&mut self) -> f32 {
        let r = &mut self.rng;
        match self.kind { Kind::Bird => r.range(1.5, 7.0), Kind::Plover => r.range(6.0, 18.0), Kind::Hawk => r.range(25.0, 60.0), Kind::Loon => r.range(28.0, 60.0),
            Kind::Owl => r.range(18.0, 45.0), Kind::Cricket => r.range(0.25, 1.1), Kind::Higurashi => r.range(14.0, 35.0), Kind::Woodpecker => r.range(25.0, 70.0),
            Kind::Bell => r.range(45.0, 85.0), Kind::Off => 1.0 }
    }
    /// false once the voice has been silent for a few milliseconds, so its spatialiser can be skipped
    #[inline] pub fn audible(&mut self, s: f32) -> bool { if s != 0.0 { self.quiet = 0; true } else { self.quiet += 1; self.quiet < 256 } }
    /// mono sample at the source
    #[inline] pub fn tick(&mut self, sr: f32, noise: f32) -> f32 {
        if self.kind == Kind::Off { return 0.0; }
        let dt = 1.0 / sr;
        if self.notes == 0 { self.next -= dt; if self.next <= 0.0 { self.phrase(); } return 0.0; }
        self.t += dt;
        let total = self.dur + self.gap;
        if self.t >= total { self.note += 1; if self.note >= self.notes { self.notes = 0; self.next = self.rest(); return 0.0; } self.start_note(); }
        if self.t > self.dur { return 0.0; }
        let p = self.t / self.dur; let f = lerp(self.f0, self.f1, p);
        match self.kind {
            Kind::Bell => {
                let mut s = 0.0; for (i, pt) in self.partials.iter().enumerate() { if pt.1 <= 0.0 { continue; } self.pamp[i] *= self.pdec[i]; let e = self.pamp[i];
                    s += fsin(self.ph * pt.0 / 98.0 + i as f32 * 0.37) * e; }
                self.ph += 98.0 / sr; if self.ph > 1000.0 { self.ph -= 1000.0; }
                let strike = if self.t < 0.15 { (1.0 - self.t / 0.15) * noise * 0.4 } else { 0.0 }; (s * 0.22 + strike) * smooth(0.0, 0.004, self.t)
            }
            Kind::Woodpecker => { self.env = (1.0 - self.t / 0.006).max(0.0); self.bp.run(noise) * self.env * self.env * 3.0 }
            Kind::Hawk => { self.ph += (f + fsin(self.t * 6.4) * 60.0) / sr; let e = fsin(p * 0.5) * (1.0 - p * 0.4);
                (fsin(self.ph) * 0.7 + noise * 0.25) * e * e * 0.5 }
            _ => {
                let vib = match self.kind { Kind::Loon => fsin(self.t * 5.4) * 0.012, Kind::Bird => fsin(self.t * 25.5) * 0.03, Kind::Plover => fsin(self.t * 3.2) * 0.006, _ => 0.0 };
                self.ph += f * (1.0 + vib) / sr; if self.ph > 1.0 { self.ph -= 1.0; }
                let e = match self.kind { Kind::Cricket => fsin(p * 0.5), Kind::Owl => smooth(0.0, 0.25, p) * (1.0 - smooth(0.6, 1.0, p)),
                    Kind::Loon => smooth(0.0, 0.15, p) * (1.0 - smooth(0.75, 1.0, p)), _ => smooth(0.0, 0.12, p) * (1.0 - smooth(0.7, 1.0, p)) };
                let s = fsin(self.ph);
                let harm = match self.kind { Kind::Owl => s + 0.25 * fsin(self.ph * 2.0) + noise * 0.05, Kind::Higurashi => s * (0.6 + 0.4 * fsin(self.ph * 0.5)), _ => s };
                harm * e * match self.kind { Kind::Cricket => 0.5, Kind::Higurashi => 0.45, _ => 0.6 }
            }
        }
    }
}

/// A positional sound from the stage itself
#[derive(Clone, Copy, Default)]
pub struct Emitter { pub kind: u32, pub x: f32, pub y: f32, pub z: f32, pub gain: f32 }

pub struct EmVoice { pub em: i32, pub spat: Spat, pl: Player, pl2: Player, brown: Brown, pink: Pink, bp: Biquad, lp: Biquad, lfo: f32, rate: f32, crick: [Voice; 2], blip: f32, blip_f: f32, blip_ph: f32 }
impl EmVoice {
    pub fn new(sr: f32, seed: u32) -> Self {
        EmVoice { em: -1, spat: Spat::new(sr), pl: Player::default(), pl2: Player { pos: 30000.0, done: false }, brown: Brown::default(), pink: Pink::default(),
            bp: Biquad::bp(sr, 1200.0, 0.8), lp: Biquad::lp(sr, 600.0, 0.7), lfo: (seed % 97) as f32 * 0.07, rate: 1.0,
            crick: [Voice::new(sr, seed.wrapping_mul(7) + 3), Voice::new(sr, seed.wrapping_mul(13) + 5)], blip: 0.0, blip_f: 600.0, blip_ph: 0.0 }
    }
    pub fn assign(&mut self, i: i32, e: &Emitter, sr: f32) {
        self.em = i; self.rate = 0.9 + ((i as u32).wrapping_mul(2654435761) >> 24) as f32 / 255.0 * 0.25;
        match e.kind { 1 => { self.bp.set_bp(sr, 1100.0, 0.6); self.lp.set_lp(sr, 420.0, 0.7); } 3 => { self.bp.set_bp(sr, 3600.0, 0.7); } 6 => { self.lp.set_lp(sr, 700.0, 0.7); } _ => {} }
        if e.kind == 5 { for (k, c) in self.crick.iter_mut().enumerate() { c.spawn(Kind::Cricket, 0.0, 0.0, 0.0, 1.0, 1.0, 0.1 + k as f32 * 0.3); } }
    }
    #[inline] pub fn tick(&mut self, e: &Emitter, smp: &[Option<Sample>], sr: f32, w: f32, rng: &mut Rng) -> f32 {
        match e.kind {
            1 => { // waterfall: low roar + broadband rush + the water recording slowed down
                let (a, _) = if let Some(s) = &smp[S_WATER_B] { self.pl.tick(s, 0.62 * self.rate, true) } else { (0.0, 0.0) };
                let roar = self.lp.run(self.brown.run(w)) * 1.6; let rush = self.bp.run(self.pink.run(w)) * 0.9; (roar + rush + a * 0.8) * 0.9 }
            2 => { // river and ford: running water plus a babble of little bubbles
                let (a, _) = if let Some(s) = &smp[S_WATER_A] { self.pl.tick(s, self.rate, true) } else { (0.0, 0.0) };
                if self.blip < 0.001 && rng.f() < 18.0 / sr { self.blip = rng.range(0.2, 0.6); self.blip_f = rng.range(380.0, 1500.0); }
                self.blip_ph += self.blip_f / sr; self.blip_f *= 1.0 + 3.0 / sr; let b = fsin(self.blip_ph) * self.blip; self.blip *= 1.0 - 60.0 / sr;
                a * 0.9 + b * 0.12 }
            3 => { self.lfo += 0.31 / sr; let pulse = 0.6 + 0.4 * fsin(self.lfo) * fsin(self.lfo * 2.3); self.bp.run(w) * pulse * 0.9 } // steam vent
            4 => { // lake shore: slow lapping waves
                self.lfo += 0.17 / sr; let q = 0.5 + 0.5 * fsin(self.lfo); let wave = q * q * q;
                let (a, _) = if let Some(s) = &smp[S_WATER_B] { self.pl.tick(s, 0.8 * self.rate, true) } else { (0.0, 0.0) }; a * (0.25 + wave) * 0.7 }
            5 => { let n = w; self.crick[0].tick(sr, n) + self.crick[1].tick(sr, n) * 0.8 } // cricket cluster
            6 => { let (a, _) = if let Some(s) = &smp[S_TRAFFIC] { self.pl.tick(s, 0.85, true) } else { (0.0, 0.0) }; self.lp.run(a) } // the town far below
            _ => 0.0,
        }
    }
}

/// Per-stage mix of the beds
#[derive(Clone, Copy)]
pub struct Preset { pub wind: f32, pub wind_lp: f32, pub howl: f32, pub leaves: f32, pub birds: f32, pub cicada: f32 }

pub struct Amb {
    sr: f32, rng: Rng, pub map: u32, pub p: Preset,
    wind_a: Player, wind_b: Player, wl: Svf, wr: Svf, howl: Svf, howl_f: f32, leaves_l: Biquad, leaves_r: Biquad, gust: f32, birds: Player,
    cic: Biquad, cic_ph: f32, pub voices: Vec<Voice>, spawn_t: f32,
}
impl Amb {
    pub fn new(sr: f32) -> Self {
        Amb { sr, rng: Rng(0x2545_f491), map: 0, p: Preset { wind: 0.4, wind_lp: 3000.0, howl: 0.0, leaves: 0.4, birds: 0.5, cicada: 0.0 },
            wind_a: Player::default(), wind_b: Player { pos: 400_000.0, done: false }, wl: Svf::default(), wr: Svf::default(), howl: Svf::default(), howl_f: 400.0,
            leaves_l: Biquad::bp(sr, 4200.0, 0.8), leaves_r: Biquad::bp(sr, 3700.0, 0.8), gust: 0.5, birds: Player::default(),
            cic: Biquad::bp(sr, 6200.0, 3.0), cic_ph: 0.0, voices: (0..14).map(|i| Voice::new(sr, 0x1234 + i * 977)).collect(), spawn_t: 0.0 }
    }
    pub fn set_map(&mut self, m: u32) {
        self.map = m;
        self.p = match m {
            1 => Preset { wind: 1.0, wind_lp: 2400.0, howl: 0.5, leaves: 0.0, birds: 0.03, cicada: 0.0 },
            2 => Preset { wind: 0.7, wind_lp: 5000.0, howl: 0.0, leaves: 0.05, birds: 0.07, cicada: 0.22 },
            3 => Preset { wind: 0.45, wind_lp: 3200.0, howl: 0.0, leaves: 0.25, birds: 0.32, cicada: 0.0 },
            4 => Preset { wind: 1.15, wind_lp: 1800.0, howl: 0.25, leaves: 0.0, birds: 0.0, cicada: 0.0 },
            5 => Preset { wind: 0.3, wind_lp: 2600.0, howl: 0.0, leaves: 0.35, birds: 0.0, cicada: 0.0 },
            6 => Preset { wind: 0.42, wind_lp: 3400.0, howl: 0.0, leaves: 0.18, birds: 0.1, cicada: 0.3 },
            _ => Preset { wind: 0.38, wind_lp: 3600.0, howl: 0.0, leaves: 0.5, birds: 0.55, cicada: 0.0 },
        };
        for v in self.voices.iter_mut() { v.kind = Kind::Off; }
        self.spawn_t = 0.0;
    }
    /// keeps a population of wildlife around the listener: spawned at a distance, fixed in the world, retired when left behind
    pub fn manage(&mut self, lx: f32, ly: f32, lz: f32) {
        let m = self.map;
        // what lives here: (kind, count, min dist, max dist, height, gain, ref)
        let fauna: &[(Kind, usize, f32, f32, f32, f32, f32)] = match m {
            0 => &[(Kind::Bird, 7, 12.0, 70.0, 9.0, 0.5, 14.0), (Kind::Woodpecker, 1, 40.0, 90.0, 8.0, 0.5, 18.0)],
            1 => &[(Kind::Bird, 1, 40.0, 90.0, 8.0, 0.25, 12.0)],
            2 => &[(Kind::Hawk, 1, 120.0, 220.0, 70.0, 0.9, 60.0), (Kind::Bird, 2, 30.0, 80.0, 4.0, 0.3, 12.0)],
            3 => &[(Kind::Bird, 4, 15.0, 70.0, 7.0, 0.42, 14.0), (Kind::Loon, 1, 160.0, 300.0, 1.0, 1.0, 90.0)],
            4 => &[(Kind::Plover, 3, 30.0, 110.0, 1.0, 0.45, 25.0)],
            5 => &[(Kind::Cricket, 6, 6.0, 30.0, 0.3, 0.32, 5.0), (Kind::Higurashi, 1, 40.0, 90.0, 8.0, 0.55, 22.0), (Kind::Owl, 1, 70.0, 140.0, 12.0, 0.6, 30.0),
                    (Kind::Bell, 1, 600.0, 650.0, -60.0, 2.4, 300.0)],
            6 => &[(Kind::Cricket, 4, 6.0, 30.0, 0.3, 0.24, 5.0), (Kind::Bird, 2, 20.0, 70.0, 6.0, 0.28, 12.0)],
            _ => &[],
        };
        let mut idx = 0;
        for &(kind, n, d0, d1, h, g, refd) in fauna.iter() {
            for _ in 0..n {
                if idx >= self.voices.len() { break; }
                let v = &mut self.voices[idx]; idx += 1;
                let far = (v.x - lx).hypot(v.z - lz);
                let alive = v.kind == kind && far < d1 * 1.6;
                if alive && kind != Kind::Bell { continue; }
                if alive && kind == Kind::Bell { v.x = lx + 420.0; v.z = lz - 430.0; continue; }   // the temple stays on the same side of the valley
                let a = self.rng.f() * TAU; let r = self.rng.range(d0, d1);
                let first = match kind { Kind::Bell => self.rng.range(8.0, 25.0), Kind::Loon | Kind::Hawk | Kind::Owl => self.rng.range(5.0, 25.0), Kind::Higurashi => self.rng.range(3.0, 15.0), _ => self.rng.range(0.2, 4.0) };
                if kind == Kind::Bell { v.spawn(kind, lx + 420.0, ly + h, lz - 430.0, g, refd, first); } else { v.spawn(kind, lx + cosf(a) * r, ly + h, lz + sinf(a) * r, g, refd, first); }
            }
        }
        for v in self.voices[idx..].iter_mut() { v.kind = Kind::Off; }
    }
    /// stereo beds (wind, leaves, distant birds recording, cicadas); gust 0..1
    #[inline] pub fn beds(&mut self, smp: &[Option<Sample>], wind: f32, wl_: f32, wr_: f32) -> (f32, f32) {
        let sr = self.sr; let p = self.p;
        self.gust += (wind - self.gust) * (1.0 / (0.8 * sr));
        let g = self.gust;
        let (a_l, a_r) = if let Some(s) = &smp[S_WIND] { self.wind_a.tick(s, 0.92 + g * 0.16, true) } else { (0.0, 0.0) };
        let (b_l, b_r) = if let Some(s) = &smp[S_WIND2] { self.wind_b.tick(s, 0.85 + g * 0.2, true) } else { (a_r, a_l) };
        let fc = p.wind_lp * (0.6 + 0.8 * g);
        let (l, _, _) = self.wl.run(a_l * 0.6 + b_l * 0.7, sr, fc, 0.6); let (r, _, _) = self.wr.run(a_r * 0.6 + b_r * 0.7, sr, fc, 0.6);
        let lev = p.wind * (0.35 + 0.95 * g * g);
        let mut ol = l * lev; let mut or = r * lev;
        if p.howl > 0.0 { self.howl_f += ((280.0 + g * 520.0 + fsin(self.cic_ph * 0.11) * 90.0) - self.howl_f) * 0.0004;
            let (_, hb, _) = self.howl.run(wl_ * 0.5 + wr_ * 0.5, sr, self.howl_f, 9.0); let h = hb * p.howl * g * g * 0.5; ol += h; or += h * 0.85; }
        if p.leaves > 0.0 { let gate = smooth(0.35, 0.9, g + wl_ * 0.08); ol += self.leaves_l.run(wl_) * p.leaves * gate * 0.22; or += self.leaves_r.run(wr_) * p.leaves * gate * 0.22; }
        if p.birds > 0.0 { let (bl, br) = if let Some(s) = &smp[crate::amb::S_BIRDS] { self.birds.tick(s, 1.0, true) } else { (0.0, 0.0) }; ol += bl * p.birds; or += br * p.birds; }
        if p.cicada > 0.0 { self.cic_ph += 1.0 / sr; let am = 0.5 + 0.5 * fsin(self.cic_ph * 52.0); let swell = 0.6 + 0.4 * fsin(self.cic_ph * 0.07);
            let c = self.cic.run(wl_) * am * swell * p.cicada; ol += c; or += c * 0.8; }
        (ol, or)
    }
}
