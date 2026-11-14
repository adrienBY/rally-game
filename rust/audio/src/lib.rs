//! Fogwood Rally sound engine.
//!
//! One `Engine` renders interleaved stereo. The game talks to it with small binary messages (little-endian f32s):
//!   [1, params...]            per-frame car/listener state (see `Params`)
//!   [2, kind, a, b]           events: 1 impact(strength, material) 2 landing(v) 3 gear shift 4 splash(v)
//!   [3, n, (kind x y z gain)*n]  positional emitters of the stage
//!   [4, id, ch, rate, looped, frames, pcm(i16 packed two per float slot)...]  a recording
//!   [5, map]                  stage preset (0 forest, 1 snow, 2 canyon, 3 lakes, 4 iceland, 5 touge)
//! The same code runs natively on Android (pulled by an AudioTrack thread through JNI) and as WebAssembly in an
//! AudioWorklet in the browser.
pub mod amb;
pub mod car;
pub mod dsp;

use amb::{Amb, EmVoice, Emitter};
use car::{Car, CarIn};
use dsp::*;

pub const NSAMPLES: usize = 16;

#[derive(Clone, Copy, Default)]
pub struct Params {
    pub car: CarIn, pub lx: f32, pub ly: f32, pub lz: f32, pub lyaw: f32, pub cx: f32, pub cy: f32, pub cz: f32, pub wind: f32, pub master: f32,
}

pub struct Engine {
    pub sr: f32, rng: Rng, p: Params, samples: Vec<Option<Sample>>,
    car: Car, car_delay: Delay, car_d: f32, car_dt: f32, car_spat: Spat,
    amb: Amb, ems: Vec<Emitter>, ev: Vec<EmVoice>, rev: Reverb, map: u32,
    blk: usize, mgr: f32, sel: f32, lim: f32, master: f32, have_params: bool, pink_l: Pink, pink_r: Pink,
}

impl Engine {
    pub fn new(sr: f32) -> Self {
        let sr = sr.clamp(8000.0, 192_000.0);
        let mut e = Engine { sr, rng: Rng(0x6d2b_79f5), p: Params::default(), samples: (0..NSAMPLES).map(|_| None).collect(),
            car: Car::new(sr), car_delay: Delay::new((sr * 1.3) as usize), car_d: 1.0, car_dt: 1.0, car_spat: Spat::new(sr),
            amb: Amb::new(sr), ems: Vec::new(), ev: (0..8).map(|i| EmVoice::new(sr, 77 + i * 31)).collect(), rev: Reverb::new(sr), map: 99,
            blk: 0, mgr: 0.0, sel: 0.0, lim: 1.0, master: 0.0, have_params: false, pink_l: Pink::default(), pink_r: Pink::default() };
        e.p.master = 0.0; e.set_map(0); e
    }

    pub fn set_map(&mut self, m: u32) {
        if m == self.map { return; } self.map = m; self.amb.set_map(m); let sr = self.sr;
        match m {   // size, T60, damping, predelay, wet, echoes off the scenery
            1 => self.rev.configure(sr, 0.5, 0.9, 0.65, 0.01, 0.07, &[]),
            2 => self.rev.configure(sr, 1.0, 2.3, 0.3, 0.02, 0.13, &[(0.41, 0.34, -0.7), (0.66, 0.24, 0.6), (1.08, 0.13, 0.0), (1.45, 0.07, -0.3)]),
            3 => self.rev.configure(sr, 0.8, 1.7, 0.35, 0.02, 0.11, &[(0.9, 0.1, 0.4)]),
            4 => self.rev.configure(sr, 1.0, 2.1, 0.32, 0.025, 0.12, &[(0.55, 0.2, -0.5), (0.97, 0.11, 0.5)]),
            5 => self.rev.configure(sr, 0.9, 1.9, 0.45, 0.02, 0.13, &[(0.78, 0.17, 0.6), (1.32, 0.09, -0.5)]),
            _ => self.rev.configure(sr, 0.6, 1.2, 0.42, 0.012, 0.1, &[]),
        }
        self.ems.clear(); for v in self.ev.iter_mut() { v.em = -1; }
    }

    pub fn message(&mut self, bytes: &[u8]) {
        let n = bytes.len() / 4; if n == 0 { return; }
        let f = |i: usize| -> f32 { if i < n { f32::from_le_bytes([bytes[i * 4], bytes[i * 4 + 1], bytes[i * 4 + 2], bytes[i * 4 + 3]]) } else { 0.0 } };
        match f(0) as i32 {
            1 => {
                let c = CarIn { rpm: f(1), thr: f(2), speed: f(3), slide: f(4), on_road: f(5), surface: f(6) as u32, wet: f(7), air: f(8), gear: f(9), cam: f(10) as u32,
                    shifting: f(21), brake: f(22), rough: f(23), drive: f(19) > 0.5 };
                self.p = Params { car: c, lx: f(11), ly: f(12), lz: f(13), lyaw: f(14), cx: f(15), cy: f(16), cz: f(17), wind: f(18), master: f(20) };
                if !self.have_params { self.have_params = true; self.retarget(); self.car_d = self.car_dt; }
            }
            2 => match f(1) as i32 { 1 => self.car.impact(f(2), f(3) as u32), 2 => self.car.land(f(2)), 3 => self.car.shift(), 4 => self.car.splash(f(2)), _ => {} },
            3 => { let cnt = (f(1) as usize).min((n - 2) / 5); self.ems.clear();
                for k in 0..cnt { let b = 2 + k * 5; self.ems.push(Emitter { kind: f(b) as u32, x: f(b + 1), y: f(b + 2), z: f(b + 3), gain: f(b + 4) }); }
                for v in self.ev.iter_mut() { v.em = -1; } }
            4 => {
                let id = f(1) as usize; let ch = (f(2) as usize).clamp(1, 2); let rate = f(3); let looped = f(4) > 0.5; let frames = f(5) as usize;
                let need = 6 * 4 + frames * ch * 2; if id >= NSAMPLES || bytes.len() < need || frames < 4 { return; }
                let pcm = &bytes[24..need]; let rd = |i: usize| i16::from_le_bytes([pcm[i * 2], pcm[i * 2 + 1]]) as f32 / 32768.0;
                let l: Vec<f32> = (0..frames).map(rd).collect(); let r = if ch == 2 { Some((0..frames).map(|i| rd(frames + i)).collect()) } else { None };
                self.samples[id] = Some(Sample::new(l, r, rate, self.sr, looped));
            }
            5 => self.set_map(f(1) as u32),
            _ => {}
        }
    }

    #[inline] fn rel(&self, x: f32, y: f32, z: f32) -> (f32, f32, f32) {
        let (s, c) = (sinf(self.p.lyaw), cosf(self.p.lyaw)); let dx = x - self.p.lx; let dz = z - self.p.lz;
        (-dx * c + dz * s, dx * s + dz * c, y - self.p.ly)
    }

    /// once per block: aim every positional source, pick which stage emitters are worth voicing
    fn retarget(&mut self) {
        let sr = self.sr; let p = self.p;
        let (r, f, u) = self.rel(p.cx, p.cy + 0.6, p.cz); let dist = sqrtf(r * r + f * f + u * u);
        self.car_dt = (dist / 343.0 * sr).clamp(1.0, sr * 1.25);
        self.car_spat.aim(sr, r, f, u, 1.0, 9.0, 0.08);
        let vs: Vec<(f32, f32, f32, f32, f32)> = self.amb.voices.iter().map(|v| { let (a, b, c) = self.rel(v.x, v.y, v.z); (a, b, c, v.gain, v.refd) }).collect();
        for (v, q) in self.amb.voices.iter_mut().zip(vs) { let s0 = if v.kind == amb::Kind::Bell { 0.5 } else { 0.12 }; v.spat.aim(sr, q.0, q.1, q.2, q.3, q.4, s0); }
        self.sel -= 256.0 / sr;
        if self.sel <= 0.0 {
            self.sel = 0.25;
            // nearest emitters within their audible range get the voices; voices already on a chosen emitter keep it
            let range = |k: u32| match k { 1 => 450.0, 2 => 140.0, 3 => 70.0, 4 => 90.0, 5 => 45.0, 6 => 3000.0, _ => 60.0 };
            let mut cand: Vec<(f32, usize)> = self.ems.iter().enumerate().filter_map(|(i, e)| { let d = (e.x - p.lx).hypot(e.z - p.lz); if d < range(e.kind) { Some((d / range(e.kind), i)) } else { None } }).collect();
            cand.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(core::cmp::Ordering::Equal)); cand.truncate(self.ev.len());
            let chosen: Vec<i32> = cand.iter().map(|c| c.1 as i32).collect();
            for v in self.ev.iter_mut() { if v.em >= 0 && !chosen.contains(&v.em) { v.em = -1; } }
            for &c in chosen.iter() { if self.ev.iter().any(|v| v.em == c) { continue; } if let Some(v) = self.ev.iter_mut().find(|v| v.em < 0) { v.assign(c, &self.ems[c as usize], sr); } }
        }
        for k in 0..self.ev.len() { let em = self.ev[k].em; if em < 0 { continue; } let e = self.ems[em as usize];
            let (a, b, c) = self.rel(e.x, e.y, e.z); let (g, refd) = match e.kind { 1 => (0.8, 20.0), 2 => (0.7, 10.0), 3 => (0.55, 7.0), 4 => (0.5, 9.0), 5 => (0.36, 5.0), 6 => (0.9, 260.0), _ => (0.4, 10.0) };
            self.ev[k].spat.aim(sr, a, b, c, g * e.gain, refd, 0.1); }
        self.mgr -= 256.0 / sr;
        if self.mgr <= 0.0 { self.mgr = 0.5; self.amb.manage(p.lx, p.ly, p.lz); }
    }

    /// fills interleaved stereo
    pub fn render(&mut self, out: &mut [f32]) {
        let sr = self.sr; let frames = out.len() / 2; let k = 1.0 - expf(-1.0 / (0.03 * sr));
        for i in 0..frames {
            if self.blk == 0 { self.retarget(); } self.blk = (self.blk + 1) % 256;
            let wl = self.pink_l.run(self.rng.bi()); let wr = self.pink_r.run(self.rng.bi()); let w = self.rng.bi();
            // car, through a travel-time delay (doppler on fly-bys), then into the stereo field
            let (eng, tyres) = if self.have_params { self.car.tick(&self.p.car, &self.samples) } else { (0.0, 0.0) };
            self.car_delay.push((eng + tyres) * 1.9);
            self.car_d += (self.car_dt - self.car_d) * 0.0004;
            let cs = self.car_delay.read(self.car_d);
            let (mut l, mut r) = self.car_spat.run(cs, sr, k);
            let mut send = cs * self.car_spat.send;
            // beds
            let (bl, br) = self.amb.beds(&self.samples, self.p.wind, wl, wr); l += bl; r += br; send += (bl + br) * 0.15;
            // wildlife
            for v in self.amb.voices.iter_mut() { if v.kind == amb::Kind::Off { continue; } let s = v.tick(sr, w); if !v.audible(s) { continue; } let (a, b) = v.spat.run(s, sr, k); l += a; r += b; send += s * v.spat.send; }
            // the stage's own sounds
            for v in self.ev.iter_mut() { if v.em < 0 { continue; } let e = self.ems[v.em as usize]; let s = v.tick(&e, &self.samples, sr, w, &mut self.rng); let (a, b) = v.spat.run(s, sr, k); l += a; r += b; send += s * v.spat.send; }
            // space
            let (rl, rr) = self.rev.run(send, send, sr); l += rl; r += rr;
            // master: fade, gentle limiter, soft clip
            self.master += (self.p.master - self.master) * (1.0 / (0.15 * sr));
            let g = self.master * 0.8 * FX_LEVEL; l *= g; r *= g;
            let pk = fabsf(l).max(fabsf(r)); let want = if pk > 0.85 { 0.85 / pk } else { 1.0 };
            self.lim = if want < self.lim { want } else { self.lim + (want - self.lim) * (1.0 / (0.25 * sr)) };
            out[i * 2] = ftanh(l * self.lim); out[i * 2 + 1] = ftanh(r * self.lim);
        }
    }
}

/// everything this engine plays sits a little under the page's engine and tyres
const FX_LEVEL: f32 = 0.6;

// ---------------- WebAssembly interface (AudioWorklet) ----------------
static mut ENGINE: Option<Engine> = None;
static mut MSG: Vec<u8> = Vec::new();
static mut OUT: Vec<f32> = Vec::new();

#[no_mangle] pub extern "C" fn fa_init(sr: f32) { unsafe { *core::ptr::addr_of_mut!(ENGINE) = Some(Engine::new(sr)); } }
#[no_mangle] pub extern "C" fn fa_msg_ptr(len: u32) -> *mut u8 { unsafe { let m = &mut *core::ptr::addr_of_mut!(MSG); if m.len() < len as usize { m.resize(len as usize, 0); } m.as_mut_ptr() } }
#[no_mangle] pub extern "C" fn fa_message(len: u32) { unsafe { let m = &*core::ptr::addr_of!(MSG); if let Some(e) = (*core::ptr::addr_of_mut!(ENGINE)).as_mut() { e.message(&m[..(len as usize).min(m.len())]); } } }
#[no_mangle] pub extern "C" fn fa_render(frames: u32) -> *const f32 {
    unsafe { let o = &mut *core::ptr::addr_of_mut!(OUT); let n = frames as usize * 2; if o.len() < n { o.resize(n, 0.0); }
        if let Some(e) = (*core::ptr::addr_of_mut!(ENGINE)).as_mut() { e.render(&mut o[..n]); } else { for v in o[..n].iter_mut() { *v = 0.0; } } o.as_ptr() }
}

// ---------------- native interface (Android) ----------------
pub mod native {
    use super::Engine;
    use std::sync::Mutex;
    static ENG: Mutex<Option<Engine>> = Mutex::new(None);
    pub fn init(sr: f32) { let mut g = ENG.lock().unwrap_or_else(|e| e.into_inner()); if g.as_ref().map(|e| (e.sr - sr).abs() > 1.0).unwrap_or(true) { *g = Some(Engine::new(sr)); } }
    pub fn message(b: &[u8]) { if let Some(e) = ENG.lock().unwrap_or_else(|e| e.into_inner()).as_mut() { e.message(b); } }
    pub fn render(out: &mut [f32]) { if let Some(e) = ENG.lock().unwrap_or_else(|e| e.into_inner()).as_mut() { e.render(out); } else { for v in out.iter_mut() { *v = 0.0; } } }
}
