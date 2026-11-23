//! net_sim [map ...]: how other players' cars look on screen, offline. A car is driven round a real stage (physics at 60 Hz,
//! a pure-pursuit driver pushing hard enough to slide), its states go through the send policy and a simulated link
//! (latency, jitter, loss; UDP or the broker's TCP), and the receiving phone draws it every frame. Reports, for the old
//! netcode (v1: plain dead reckoning, wall-clock sampling) and the current one:
//!   err    distance between where the car is shown and where it really is, at the same instant (m, rms / 99th pct)
//!   shake  extra acceleration of the shown car compared with the real one: what reads as choppiness (m/s2 rms / 99th pct;
//!          a sudden 1 cm pop is ~36)
//!   yshk   the same for height (m/s2)
//!   pkt/s, B/s  car traffic per stream, and MB an hour for a whole 4- and 8-player room (both directions, all headers)
//! Env: SECS (90), LAT ms (60), JIT ms (25), LOSS (0.02), TCP=1 (broker relay: in order, stalls on loss), TICKJIT ms (4),
//!      STAMPJIT ms (0: when in the frame the sender reads its clock; RAWSTAMP=1 stamps with that reading as before),
//!      PLAYERS (4: sets the send-rate cap), THR=pos,head,speed[,height,body], STIFF (fade speed x), ACC=1 (acceleration
//!      percentiles, shown vs real), DRV=1 (driver log), SPIKE=1 (list big jumps).
use fogwood_core::net::proto::{self, CarSnap};
use fogwood_core::net::smooth::{self, Ground, Policy, Thresholds, Track};
use fogwood_core::physics::{Car, Input, Tune, NPARAM};

fn env(k: &str, d: f64) -> f64 { std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d) }
struct Rng(u64);
impl Rng { fn f(&mut self) -> f64 { self.0 ^= self.0 << 13; self.0 ^= self.0 >> 7; self.0 ^= self.0 << 17; (self.0 >> 11) as f64 / (1u64 << 53) as f64 } }
fn wrap(a: f64) -> f64 { let t = std::f64::consts::TAU; let a = a.rem_euclid(t); if a > std::f64::consts::PI { a - t } else { a } }

fn snap_of(c: &Car) -> CarSnap {
    CarSnap { t: 0.0, x: c.x as f32, y: c.y as f32, z: c.z as f32, h: c.h as f32, vx: c.vx as f32, vy: c.vy as f32, vz: c.vz as f32, r: c.r as f32, pitch: c.pitch as f32, roll: c.roll as f32,
        steer: c.steer as f32, u: c.u as f32, gear: c.gear as i8, rpm: c.rpm.clamp(0.0, 65000.0) as u16, d: [c.wh[0].d as f32, c.wh[1].d as f32, c.wh[2].d as f32, c.wh[3].d as f32],
        ang: [0.0; 4], flags: if c.contacts == 0.0 && c.air_t > 0.06 { smooth::AIR } else { 0 }, a: 0.0 }
}

// ---- the netcode as it was (v1), for comparison
fn wrapf(a: f32) -> f32 { wrap(a as f64) as f32 }
fn predict_v1(c: &CarSnap, dt: f32) -> CarSnap {
    let mut s = *c; let (sh, ch) = (c.h.sin(), c.h.cos()); let u = c.vx * sh + c.vz * ch; let vl = c.vx * ch - c.vz * sh;
    let n = 4; let st = dt / n as f32; let mut h = c.h;
    for _ in 0..n { let hm = h + c.r * st * 0.5; let (a, b) = (hm.sin(), hm.cos()); s.x += (u * a + vl * b) * st; s.z += (u * b - vl * a) * st; h += c.r * st; }
    s.h = h; let (a, b) = (h.sin(), h.cos()); s.vx = u * a + vl * b; s.vz = u * b - vl * a; s.u = u; s
}
#[derive(Default)]
struct V1 { sent: Option<CarSnap>, latest: Option<CarSnap>, offset: [f32; 3], shown_t: f64 }
impl V1 {
    fn due(&mut self, c: &CarSnap) -> bool {
        let th = c.t; match &self.sent { None => true, Some(s) => { let age = th - s.t; let p = predict_v1(s, age as f32);
            let dh = wrapf(c.h - p.h).abs(); let de = ((c.x - p.x).powi(2) + (c.z - p.z).powi(2) + (c.y - p.y).powi(2)).sqrt();
            age >= 0.4 || (age >= 1.0 / 31.0 && (de > 0.15 || dh > 0.026 || (c.u - p.u).abs() > 0.8 || c.gear != s.gear || c.flags != s.flags)) } }
    }
    fn push(&mut self, c: CarSnap, th: f64) {
        if let Some(old) = self.latest { let before = predict_v1(&old, (th - old.t).min(0.6) as f32); let after = predict_v1(&c, (th - c.t).min(0.6) as f32);
            let shown = [before.x + self.offset[0], before.z + self.offset[1], before.h + self.offset[2]];
            let off = [shown[0] - after.x, shown[1] - after.z, wrapf(shown[2] - after.h)]; self.offset = if off[0].hypot(off[1]) < 8.0 { off } else { [0.0; 3] }; }
        self.latest = Some(c);
    }
    fn sample(&mut self, th: f64) -> Option<CarSnap> {
        let c = self.latest?; let tau = (0.12 + (th - c.t).max(0.0) * 0.6).min(0.3);
        let k = (-(th - self.shown_t).max(0.0) / tau).exp() as f32; self.shown_t = th; for o in self.offset.iter_mut() { *o *= k; }
        let mut s = predict_v1(&c, (th - c.t).clamp(0.0, 0.6) as f32); s.x += self.offset[0]; s.z += self.offset[1]; s.h += self.offset[2]; Some(s)
    }
}

struct Stats { err: Vec<f64>, shake: Vec<f64>, yshk: Vec<f64>, pkts: usize, bytes: usize, acc: Vec<f64>, acct: Vec<f64> }
fn pct(v: &mut Vec<f64>, p: f64) -> f64 { if v.is_empty() { return 0.0; } v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v[((v.len() - 1) as f64 * p) as usize] }
fn rms(v: &[f64]) -> f64 { (v.iter().map(|x| x * x).sum::<f64>() / v.len().max(1) as f64).sqrt() }

fn main() {
    let maps: Vec<String> = { let a: Vec<String> = std::env::args().skip(1).collect(); if a.is_empty() { vec!["lakes".into(), "snow".into(), "touge".into()] } else { a } };
    let secs = env("SECS", 90.0); let lat = env("LAT", 60.0) / 1000.0; let jit = env("JIT", 25.0) / 1000.0; let loss = env("LOSS", 0.02); let tcp = env("TCP", 0.0) > 0.0;
    let tickjit = env("TICKJIT", 4.0) / 1000.0; let dt = 1.0 / 60.0; let stampjit = env("STAMPJIT", 0.0) / 1000.0;
    println!("link: {:.0} ms +/- {:.0} ms, {:.0}% loss, {}   frame-timing jitter {:.0} ms", lat * 1000.0, jit * 1000.0, loss * 100.0, if tcp { "broker relay (TCP)" } else { "direct (UDP)" }, tickjit * 1000.0);
    println!("{:7} {:4} {:>13} {:>14} {:>13} {:>6} {:>6} {:>7} {:>7}", "map", "", "err m rms/p99", "shake p50/p99", "yshk p50/p99", "pkt/s", "B/s", "MB/h 4p", "MB/h 8p");
    for map in &maps {
        let txt = std::fs::read_to_string(format!("{}/tests/fixtures/{}.txt", env!("CARGO_MANIFEST_DIR"), map)).expect("fixture");
        let mut lines = txt.lines(); let head: Vec<&str> = lines.next().unwrap().split(' ').collect();
        let (mid, gxh, gzh): (i32, i32, i32) = (head[0].parse().unwrap(), head[1].parse().unwrap(), head[2].parse().unwrap());
        let path_s = lines.next().unwrap(); let roadh = lines.next().unwrap(); let obs = lines.next().unwrap();
        fogwood_core::bridge::prepare(mid, path_s, roadh, gxh, gzh); fogwood_core::bridge::set_obstacles(obs);
        let first = fogwood_core::bridge::parse_csv(lines.next().unwrap().split_once('|').unwrap().0);
        let tune = Tune::from_slice(&first[5..5 + NPARAM]);
        let pv = fogwood_core::bridge::parse_csv(path_s); let path: Vec<(f64, f64)> = pv.chunks(2).map(|c| (c[0], c[1])).collect(); let np = path.len();
        let lock = fogwood_core::bridge::lock(); let e = lock.as_ref().unwrap(); let w = &e.world;
        let ground = |x: f32, z: f32| w.height(x as f64, z as f64) as f32; let g: Ground = Some(&ground);
        // curvature-limited speed plan, braking ahead of corners
        let amax = env("AMAX", if mid == 1 { 4.5 } else { 7.0 });
        let curv: Vec<f64> = (0..np).map(|i| { let a = path[(i + np - 6) % np]; let b = path[i]; let c = path[(i + 6) % np];
            let h1 = (b.0 - a.0).atan2(b.1 - a.1); let h2 = (c.0 - b.0).atan2(c.1 - b.1); (wrap(h2 - h1) / 12.0).abs() }).collect();
        let mut vplan: Vec<f64> = curv.iter().map(|k| (amax / k.max(1e-4)).sqrt().min(48.0)).collect();
        for _ in 0..2 { for i in (0..np).rev() { let n = vplan[(i + 1) % np]; vplan[i] = vplan[i].min((n * n + 2.0 * 7.0).sqrt()); } }
        // drive: truth at every frame
        let mut car = Car::new(); let p0 = path[0]; let p1 = path[1];
        car.x = p0.0; car.z = p0.1; car.h = (p1.0 - p0.0).atan2(p1.1 - p0.1); car.gear = 1.0; car.rpm = 900.0; car.y = w.height(car.x, car.z) + 0.075;
        for wh in car.wh.iter_mut() { wh.comp = 0.135; wh.d = 0.075; wh.fz = 2900.0; }
        let n = (secs / dt) as usize; let mut truth: Vec<Car> = Vec::with_capacity(n); let mut rng = Rng(0x9E3779B97F4A7C15 ^ mid as u64);
        let mut wob = 0.0;
        for f in 0..n {
            let i = fogwood_core::game::path_idx(w, car.x, car.z); let u = car.u.max(0.0); let ld = 6.0 + u * 0.38; let k = (i + ld as usize) % np;
            let (tx, tz) = path[k]; let vh = if car.vx.hypot(car.vz) > 2.0 { car.vx.atan2(car.vz) } else { car.h };
            let al = wrap((tx - car.x).atan2(tz - car.z) - vh); wob += (rng.f() - 0.5) * 0.01 - wob * 0.03;
            let lock_k = tune.lock / (1.0 + u / tune.lock_fall);
            let sp = car.vx.hypot(car.vz); let r_want = 2.0 * sp.max(1.0) * al.sin() / ld;
            let delta = wrap(vh - car.h) + (2.0 * 2.5 * al.sin() / ld).atan() - (car.r - r_want) * 0.12 + wob;
            let vt = vplan[(i + 4) % np] * (0.97 + 0.06 * ((f as f64) * 0.01).sin());
            let inp = Input { steer: (delta / lock_k).clamp(-1.0, 1.0), throttle: ((vt - u) * 0.6).clamp(0.0, 1.0), brake: ((u - vt - 0.8) * 0.5).clamp(0.0, 1.0), hb: false };
            car.update(w, &e.obstacles, &tune, dt, &inp); truth.push(car);
            if std::env::var("DRV").is_ok() && f % 120 == 0 { eprintln!("t={:.0} i={} u={:.1} vt={:.1} pd={:.1} imp={:.1} st={:.2}", f as f64 * dt, i, car.u, vt, w.path_dist(car.x, car.z), car.impact, inp.steer); }
        }
        for ver in ["v1", "now"] {
            let mut net_rng = Rng(12345); let v1 = ver == "v1";
            // send: (arrival, seq, bytes)
            let mut flight: Vec<(f64, Vec<Vec<u8>>)> = Vec::new(); let red = env("RED", 0.0) > 0.0 && !v1; let repeat = env("REPEAT", 0.0) / 1000.0; let dual = env("DUAL", 0.0) > 0.0 && !v1; let mut last_pkt: Option<(f64, Vec<u8>)> = None; let mut repeated = true; let mut tcp_last2 = 0.0f64; let hdr = env("HDR", -1.0); let mut pol = Policy::default(); let mut thr = Thresholds::default(); if let Ok(v) = std::env::var("THR") { let q: Vec<f32> = v.split(',').filter_map(|x| x.parse().ok()).collect(); thr.pos = q[0]; thr.head = q[1]; thr.speed = q[2]; if q.len() > 3 { thr.height = q[3]; thr.body = q[4]; } } let mut v1s = V1::default();
            let mut seq = 0u8; let mut tcp_last = 0.0f64; let mut stamp = smooth::ViewClock::default(); let (mut pkts, mut bytes) = (0usize, 0usize);
            // rate: a 4-player all-direct room under the budget
            let players = env("PLAYERS", 4.0); let rate = ((fogwood_core::net::BUDGET - 250.0) / (2.0 * (players - 1.0) * (proto::CAR3_LEN as f64 + 28.0))).clamp(4.0, 24.0) as f32; let beat = (3.0 / rate).clamp(0.4, 0.75);
            for (f, c) in truth.iter().enumerate() {
                // the sender's clock is read at a varying moment of each frame (STAMPJIT); the current netcode stamps on a
                // vsync-paced clock locked to it instead
                let t = f as f64 * dt; let mut s = snap_of(c); let raw = t + net_rng.f() * stampjit; s.t = if v1 || std::env::var("RAWSTAMP").is_ok() { raw } else { stamp.advance(raw, dt) };
                let send = if v1 { v1s.due(&s) } else { s.a = pol.accel(s.u, dt as f32); pol.due(&s, g, &thr, rate, beat) };
                if !send {
                    // a repeat of the last state, once, when nothing new followed it within REPEAT
                    if repeat > 0.0 && !v1 && !repeated { if let Some((lt, lb)) = &last_pkt { if t - lt >= repeat { repeated = true; let b = lb.clone();
                        pkts += 1; bytes += b.len() + if tcp { 4 + 10 + 40 } else { 28 };
                        if !(net_rng.f() < loss) && !tcp { flight.push((t + lat + (net_rng.f() * 2.0 - 1.0) * jit, vec![b])); } } } }
                    continue; }
                seq = seq.wrapping_add(1); let b = if v1 { proto::car2(7, seq, &s) } else { proto::car3(7, seq, &s) };
                let (_, _, _, mut q) = proto::read_car2(&b).unwrap(); q.t = t; if v1 { v1s.sent = Some(q); } else { pol.sent = Some(q); }
                let mut msg = Vec::new(); if red { if let Some((lt, lb)) = &last_pkt { if t - lt < 0.3 { msg.push(lb.clone()); } } } msg.push(b.clone());
                let payload: usize = msg.iter().map(|m| m.len()).sum::<usize>(); let over = if hdr >= 0.0 { hdr as usize } else if tcp { 4 + 10 + 40 } else { 28 };
                pkts += 1; bytes += payload + over; last_pkt = Some((t, b)); repeated = false;
                let lost = net_rng.f() < loss;
                let arr = if tcp { let mut a = t + lat + (net_rng.f() * 2.0 - 1.0) * jit; if lost { a += 0.2 + lat * 2.0; } a = a.max(tcp_last); tcp_last = a;
                        if dual { bytes += payload + over; let lost2 = net_rng.f() < loss; let mut a2 = t + lat + (net_rng.f() * 2.0 - 1.0) * jit; if lost2 { a2 += 0.2 + lat * 2.0; } a2 = a2.max(tcp_last2); tcp_last2 = a2; a = a.min(a2); }
                        Some(a) }
                    else if lost { None } else { Some(t + lat + (net_rng.f() * 2.0 - 1.0) * jit) };
                if let Some(a) = arr { flight.push((a, msg)); }
            }
            flight.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            // receive and draw
            let mut tr = Track::default(); tr.stiff = env("STIFF", 0.0) as f32; tr.cap = env("CAP", 0.0) as f32; let mut v1r = V1::default(); let mut fi = 0; let mut last_seq: Option<u8> = None;
            let mut view = smooth::ViewClock::default(); let mut shown: Vec<CarSnap> = Vec::with_capacity(n);
            for f in 0..n {
                let vs = f as f64 * dt; let tick = vs + net_rng.f() * tickjit;   // the frame starts a little after vsync, by a varying amount
                while fi < flight.len() && flight[fi].0 <= tick {
                    let msg = flight[fi].1.clone(); fi += 1;
                    for m in msg { let (_, sq, ms, mut c) = proto::read_car2(&m).unwrap();
                    if let Some(l) = last_seq { if (sq.wrapping_sub(l) as i8) <= 0 { continue; } } last_seq = Some(sq);
                    let age = ((tick * 1000.0).rem_euclid(65536.0) - ms as f64).rem_euclid(65536.0) / 1000.0; c.t = tick - age;
                    if v1 { v1r.push(c, tick); } else { tr.push_at(c, tick); } }
                }
                let s = if v1 { v1r.sample(tick) } else {
                    let t = view.advance(tick, dt);
                    tr.sample(t, g) };
                // v1 drew the received y as it was (no terrain prediction)
                shown.push(s.unwrap_or_default());
            }
            let mut st = Stats { err: vec![], shake: vec![], yshk: vec![], pkts, bytes, acc: vec![], acct: vec![] };
            for f in 180..n - 1 {
                let (a, b, c) = (&shown[f - 1], &shown[f], &shown[f + 1]); let (ta, tb, tc) = (&truth[f - 1], &truth[f], &truth[f + 1]);
                st.err.push((b.x as f64 - tb.x).hypot(b.z as f64 - tb.z));
                let ax = (a.x as f64 - 2.0 * b.x as f64 + c.x as f64) - (ta.x - 2.0 * tb.x + tc.x); let az = (a.z as f64 - 2.0 * b.z as f64 + c.z as f64) - (ta.z - 2.0 * tb.z + tc.z);
                st.shake.push(ax.hypot(az) / (dt * dt));
                if std::env::var("SPIKE").is_ok() && ax.hypot(az) / (dt * dt) > 800.0 { eprintln!("{} spike f={} t={:.2} shown ({:.2},{:.2}) prev ({:.2},{:.2}) true ({:.2},{:.2}) u={:.1} imp={:.1}", ver, f, f as f64 * dt, b.x, b.z, a.x, a.z, tb.x, tb.z, tb.u, tb.impact); }
                st.acc.push((a.x as f64 - 2.0 * b.x as f64 + c.x as f64).hypot(a.z as f64 - 2.0 * b.z as f64 + c.z as f64) / (dt * dt));
                st.acct.push((ta.x - 2.0 * tb.x + tc.x).hypot(ta.z - 2.0 * tb.z + tc.z) / (dt * dt));
                st.yshk.push(((a.y as f64 - 2.0 * b.y as f64 + c.y as f64) - (ta.y - 2.0 * tb.y + tc.y)).abs() / (dt * dt));
            }
            let bps = st.bytes as f64 / secs; let mbh = |np: f64| (bps * 2.0 * (np - 1.0) + 250.0) * 3600.0 / 1e6;
            println!("{:7} {:4} {:5.2} / {:5.2} {:6.1} / {:5.1} {:5.1} / {:5.1} {:6.1} {:6.0} {:7.1} {:7.1}", map, ver, rms(&st.err), pct(&mut st.err, 0.99), pct(&mut st.shake, 0.5), pct(&mut st.shake, 0.99),
                pct(&mut st.yshk, 0.5), pct(&mut st.yshk, 0.99), st.pkts as f64 / secs, bps, mbh(4.0), mbh(8.0));
            if std::env::var("ACC").is_ok() { println!("        accel p50/p90/p99 shown {:.0}/{:.0}/{:.0} real {:.0}/{:.0}/{:.0}; avg speed {:.1} m/s", pct(&mut st.acc, 0.5), pct(&mut st.acc, 0.9), pct(&mut st.acc, 0.99), pct(&mut st.acct, 0.5), pct(&mut st.acct, 0.9), pct(&mut st.acct, 0.99), truth.iter().map(|c| c.u).sum::<f64>() / n as f64); }
        }
    }
}
