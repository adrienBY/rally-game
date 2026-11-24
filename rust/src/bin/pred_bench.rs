//! pred_bench [map ...]: how well a remote car is guessed ahead, offline. One car drives a real stage (real physics, a
//! pure-pursuit driver that keeps changing its mind: steering corrections, brake taps, lifts, the odd handbrake). At
//! many moments its state goes "out" exactly as the game sends it (CAR3, quantised) with the driver's inputs, and is
//! run ahead 0.1 .. 0.5 s by each method, against where the car really went:
//!   kinematic  the current guess: the car keeps turning and accelerating the way it was (net::smooth::predict)
//!   physics    the real car physics run on from the received state with the inputs it carried (held)
//!   physics*   the same from the sender's exact state (the best input-holding can do)
//! Env: SECS (90), SEED.
use fogwood_core::net::proto::{self, CarSnap};
use fogwood_core::net::smooth::{self, Ground};
use fogwood_core::physics::{Car, Input, Rebuild, Tune, NPARAM};

struct Rng(u64);
impl Rng { fn f(&mut self) -> f64 { self.0 ^= self.0 << 13; self.0 ^= self.0 >> 7; self.0 ^= self.0 << 17; (self.0 >> 11) as f64 / (1u64 << 53) as f64 } }
fn wrap(a: f64) -> f64 { let t = std::f64::consts::TAU; let a = a.rem_euclid(t); if a > std::f64::consts::PI { a - t } else { a } }
fn pct(v: &mut Vec<f64>, p: f64) -> f64 { if v.is_empty() { return 0.0; } v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v[((v.len() - 1) as f64 * p) as usize] }
fn rms(v: &[f64]) -> f64 { (v.iter().map(|x| x * x).sum::<f64>() / v.len().max(1) as f64).sqrt() }
fn env(k: &str, d: f64) -> f64 { std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d) }

fn snap_of(c: &Car) -> CarSnap {
    CarSnap { t: 0.0, x: c.x as f32, y: c.y as f32, z: c.z as f32, h: c.h as f32, vx: c.vx as f32, vy: c.vy as f32, vz: c.vz as f32, r: c.r as f32, pitch: c.pitch as f32, roll: c.roll as f32,
        steer: c.steer as f32, u: c.u as f32, gear: c.gear as i8, rpm: c.rpm.clamp(0.0, 65000.0) as u16, d: [c.wh[0].d as f32, c.wh[1].d as f32, c.wh[2].d as f32, c.wh[3].d as f32],
        ang: [0.0; 4], flags: if c.contacts == 0.0 && c.air_t > 0.06 { smooth::AIR } else { 0 }, a: 0.0, ctl: None }
}
pub fn rebuild_of(s: &CarSnap, i: &Input) -> Rebuild {
    Rebuild { x: s.x as f64, y: s.y as f64, z: s.z as f64, h: s.h as f64, vx: s.vx as f64, vy: s.vy as f64, vz: s.vz as f64, r: s.r as f64, pitch: s.pitch as f64, roll: s.roll as f64,
        steer: s.steer as f64, gear: s.gear as f64, rpm: s.rpm as f64, a: s.a as f64, throttle: i.throttle, brake: i.brake, hb: i.hb, air: s.flags & smooth::AIR != 0 }
}

fn main() {
    let maps: Vec<String> = { let a: Vec<String> = std::env::args().skip(1).collect(); if a.is_empty() { vec!["lakes".into(), "touge".into()] } else { a } };
    let secs = env("SECS", 90.0); let dt = 1.0 / 60.0; let hz: [usize; 5] = [6, 12, 18, 24, 30];
    println!("{:7} {:10} {}", "map", "method", hz.iter().map(|h| format!("{:>17}", format!("{:.1} s rms/p95", *h as f64 * dt))).collect::<String>());
    for map in &maps {
        let txt = std::fs::read_to_string(format!("{}/tests/fixtures/{}.txt", env!("CARGO_MANIFEST_DIR"), map)).expect("fixture");
        let mut lines = txt.lines(); let head: Vec<&str> = lines.next().unwrap().split(' ').collect();
        let (mid, gxh, gzh): (i32, i32, i32) = (head[0].parse().unwrap(), head[1].parse().unwrap(), head[2].parse().unwrap());
        let path_s = lines.next().unwrap(); let roadh = lines.next().unwrap(); let obs = lines.next().unwrap();
        fogwood_core::bridge::prepare(mid, path_s, roadh, gxh, gzh); fogwood_core::bridge::set_obstacles(obs);
        let first = fogwood_core::bridge::parse_csv(lines.next().unwrap().split_once('|').unwrap().0);
        let tune = Tune::from_slice(&first[5..5 + NPARAM]);
        let pv = fogwood_core::bridge::parse_csv(path_s); let path: Vec<(f64, f64)> = pv.chunks(2).map(|c| (c[0], c[1])).collect(); let np = path.len();
        let lock = fogwood_core::bridge::lock(); let e = lock.as_ref().unwrap(); let w = &e.world; let ob = &e.obstacles;
        let ground = |x: f32, z: f32| w.height(x as f64, z as f64) as f32; let g: Ground = Some(&ground);
        let amax = if mid == 1 { 4.5 } else { 7.0 };
        let curv: Vec<f64> = (0..np).map(|i| { let a = path[(i + np - 6) % np]; let b = path[i]; let c = path[(i + 6) % np];
            let h1 = (b.0 - a.0).atan2(b.1 - a.1); let h2 = (c.0 - b.0).atan2(c.1 - b.1); (wrap(h2 - h1) / 12.0).abs() }).collect();
        let mut vplan: Vec<f64> = curv.iter().map(|k| (amax / k.max(1e-4)).sqrt().min(40.0)).collect();
        for _ in 0..2 { for i in (0..np).rev() { let n = vplan[(i + 1) % np]; vplan[i] = vplan[i].min((n * n + 2.0 * 7.0).sqrt()); } }
        let tan = |i: usize| { let a = path[(i + np - 1) % np]; let b = path[(i + 1) % np]; let (dx, dz) = (b.0 - a.0, b.1 - a.1); let l = dx.hypot(dz).max(1e-6); (dx / l, dz / l) };
        // the drive: a car and the inputs of every frame
        let mut car = Car::new(); { let (tx, tz) = tan(0); car.x = path[0].0; car.z = path[0].1; car.h = tx.atan2(tz); car.y = w.height(car.x, car.z) + 0.075;
            for wh in car.wh.iter_mut() { wh.comp = 0.135; wh.d = 0.075; wh.fz = 2900.0; } }
        let mut rng = Rng(env("SEED", 1.0) as u64 * 977 + 5);
        let (mut kick, mut kick_t, mut kind) = (0.0f64, 0.0f64, 0u8); let mut acc = 0.0f64; let mut u_prev = 0.0f64;
        let mut cars: Vec<Car> = Vec::new(); let mut inps: Vec<Input> = Vec::new(); let mut acc_v: Vec<f64> = Vec::new();
        let n = (secs / dt) as usize;
        for f in 0..n {
            let t = f as f64 * dt;
            // the driver changes its mind every 0.4 .. 1.6 s: a steering correction, a brake tap, a lift, a handbrake flick
            if t >= kick_t { kick_t = t + 0.4 + rng.f() * 1.2; kind = (rng.f() * 5.0) as u8; kick = rng.f() * 2.0 - 1.0; }
            let live = t < kick_t - 0.0 && kick_t - t < 0.45;
            let i = fogwood_core::game::path_idx(w, car.x, car.z); let u = car.u.max(0.0); let ld = 6.0 + u * 0.38; let k = (i + ld as usize) % np;
            let lane = 1.2 * (t * 0.31).sin(); let (tx, tz) = tan(k); let (px, pz) = (path[k].0 - tz * lane, path[k].1 + tx * lane);
            let vh = if car.vx.hypot(car.vz) > 2.0 { car.vx.atan2(car.vz) } else { car.h };
            let al = wrap((px - car.x).atan2(pz - car.z) - vh); let lock_k = tune.lock / (1.0 + u / tune.lock_fall);
            let sp = car.vx.hypot(car.vz); let r_want = 2.0 * sp.max(1.0) * al.sin() / ld;
            let delta = wrap(vh - car.h) + (2.0 * 2.5 * al.sin() / ld).atan() - (car.r - r_want) * 0.12;
            let pace = 0.9 + 0.08 * (t * 0.23).sin(); let vt = vplan[(i + 4) % np] * pace;
            let mut inp = Input { steer: (delta / lock_k).clamp(-1.0, 1.0), throttle: ((vt - u) * 0.6).clamp(0.0, 1.0), brake: ((u - vt - 0.8) * 0.5).clamp(0.0, 1.0), hb: false };
            if live { match kind { 0 => inp.steer = (inp.steer + kick * 0.5).clamp(-1.0, 1.0), 1 => { inp.brake = 0.4 + 0.5 * kick.abs(); inp.throttle = 0.0; } 2 => inp.throttle = 0.0,
                3 => { if u > 12.0 && kick.abs() > 0.6 { inp.hb = true; } } _ => inp.throttle = 1.0 } }
            cars.push(car); inps.push(inp);
            if f > 0 { acc += (((car.u - u_prev) / dt).clamp(-30.0, 30.0) - acc) * (dt * 12.0).min(1.0); } u_prev = car.u; acc_v.push(acc);
            car.update(w, ob, &tune, dt, &inp);
        }
        // the guesses
        let mut errs: Vec<Vec<Vec<f64>>> = vec![vec![Vec::new(); hz.len()]; 3]; let mut steps = 0u64; let t0w = std::time::Instant::now();
        let mut f = (3.0 / dt) as usize;
        while f + 31 < n {
            let mut s = snap_of(&cars[f]); s.a = acc_v[f] as f32; s.t = 0.0;
            let q = { let b = proto::car3(1, 0, &s); let (_, _, _, mut c) = proto::read_car2(&b).unwrap(); c.t = 0.0; c };
            let inp = inps[f];
            let mut pq = Car::rebuilt(w, &rebuild_of(&q, &inp)); let mut px = cars[f];
            let mut k = 0;
            for (hi, h) in hz.iter().enumerate() {
                while k < *h { pq.update(w, ob, &tune, dt, &inp); px.update(w, ob, &tune, dt, &inp); k += 1; steps += 2; }
                let tr = &cars[f + h]; let kin = smooth::predict(&q, *h as f32 * dt as f32, g);
                // ALONG=1: the error along the car's way only (how far ahead or behind: what nose-to-tail contacts feel)
                let e = |x: f64, z: f64| if std::env::var("ALONG").is_ok() { ((x - tr.x) * tr.h.sin() + (z - tr.z) * tr.h.cos()).abs() } else { (x - tr.x).hypot(z - tr.z) };
                errs[0][hi].push(e(kin.x as f64, kin.z as f64));
                errs[1][hi].push(e(pq.x, pq.z));
                errs[2][hi].push(e(px.x, px.z));
            }
            f += 3;
        }
        let us = t0w.elapsed().as_secs_f64() * 1e6 / steps.max(1) as f64;
        for (m, name) in ["kinematic", "physics", "physics*"].iter().enumerate() {
            println!("{:7} {:10} {}", map, name, (0..hz.len()).map(|hi| { let mut v = errs[m][hi].clone(); format!("{:>17}", format!("{:.2} / {:.2} m", rms(&v), pct(&mut v, 0.95))) }).collect::<String>());
        }
        println!("{:7} (one physics step {:.1} us)", map, us);
    }
}
