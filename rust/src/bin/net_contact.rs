//! net_contact [map ...]: two phones racing door to door through a simulated link, offline. Each phone drives its own
//! car (real physics on a real stage, a pure-pursuit driver whose pace and line wander so the cars keep catching and
//! leaning on each other), sends its states, draws the other car from what arrives and settles contacts against that
//! drawn car exactly like the game (`race::bump`). Compares the netcode before contact counters ("old") with now:
//!   felt     true contacts (the two real cars overlap) that the victim's own phone answered with a push, and how
//!            long after the true contact that push came (ms, mean / 90th pct)
//!   ghost    pushes a phone gave its own car with no true contact within 0.4 s (bumping into where the car isn't)
//!   err      where the other car is drawn vs where it really is, in the second after each contact (m, rms / p99)
//!   shake    extra acceleration of the drawn car in that second (m/s2, p50 / p99): the twitch of a car pushed here,
//!            pulled back by its old states, then pushed again by its own phone
//!   gap      side by side without contact: drawn distance minus real distance between the cars (m, rms)
//!   jolt     how much more the drawn car is thrown about than the real one in the second around a contact (m/s of
//!            summed acceleration, median / 90th pct): 0 when the drawn car takes the knock once, the way it happened
//! Env: SECS (120), LAT ms (50), JIT ms (20), LOSS (0.02), TCP=1 (relay over a WebSocket: in order, stalls on loss),
//!      TICKJIT ms (4, when in the frame each phone runs), SEED; the contact tuning (race::contacts): NUDGE (share of the
//!      push shown on the other car, game: race::SHOW), AMARGIN (narrower per second of prediction, game 0.25), MARGIN,
//!      RESTI (impulse factor), COMP=1 (lag-compensated pushes), NEARRATE=1 (smooth::relevance near the other car).
use fogwood_core::game::race::{bump, bump_r};
use fogwood_core::net::proto::{self, CarSnap};
use fogwood_core::net::smooth::{self, Ground, Policy, Thresholds, Track, ViewClock};
use fogwood_core::physics::{Car, Input, Tune, NPARAM};

fn env(k: &str, d: f64) -> f64 { std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d) }
struct Rng(u64);
impl Rng { fn f(&mut self) -> f64 { self.0 ^= self.0 << 13; self.0 ^= self.0 >> 7; self.0 ^= self.0 << 17; (self.0 >> 11) as f64 / (1u64 << 53) as f64 } }
fn wrap(a: f64) -> f64 { let t = std::f64::consts::TAU; let a = a.rem_euclid(t); if a > std::f64::consts::PI { a - t } else { a } }
fn pct(v: &mut Vec<f64>, p: f64) -> f64 { if v.is_empty() { return 0.0; } v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v[((v.len() - 1) as f64 * p) as usize] }
fn rms(v: &[f64]) -> f64 { (v.iter().map(|x| x * x).sum::<f64>() / v.len().max(1) as f64).sqrt() }

struct Link { lat: f64, jit: f64, loss: f64, tcp: bool, last: f64, rng: Rng }
impl Link {
    fn arrival(&mut self, t: f64) -> Option<f64> {
        let lost = self.rng.f() < self.loss; let mut a = t + self.lat + (self.rng.f() * 2.0 - 1.0) * self.jit;
        if self.tcp { if lost { a += 0.2 + self.lat * 2.0; } a = a.max(self.last); self.last = a; Some(a) } else if lost { None } else { Some(a) }
    }
}
enum Msg { Car(Vec<u8>), Hit([f32; 5], f32, f64) }

fn snap_of(c: &Car, epoch: u8) -> CarSnap {
    CarSnap { t: 0.0, x: c.x as f32, y: c.y as f32, z: c.z as f32, h: c.h as f32, vx: c.vx as f32, vy: c.vy as f32, vz: c.vz as f32, r: c.r as f32, pitch: c.pitch as f32, roll: c.roll as f32,
        steer: c.steer as f32, u: c.u as f32, gear: c.gear as i8, rpm: c.rpm.clamp(0.0, 65000.0) as u16, d: [c.wh[0].d as f32, c.wh[1].d as f32, c.wh[2].d as f32, c.wh[3].d as f32],
        ang: [0.0; 4], flags: smooth::with_epoch(if c.contacts == 0.0 && c.air_t > 0.06 { smooth::AIR } else { 0 }, epoch), a: 0.0 }
}

struct Phone {
    car: Car, tr: Track, pol: Policy, stamp: ViewClock, view: ViewClock, seq: u8, last_seq: Option<u8>,
    inbox: Vec<(f64, Msg)>, hit_acc: Option<([f32; 5], f32)>, hit_tx: f64, net_hits: Vec<([f64; 5], f64)>,
    epoch: u8, contact_t: f64, hit_t: f64, shown: Car, have: bool,
    /// times this phone pushed its own car (local contact or a push from the other phone)
    pushes: Vec<f64>,
    /// lag compensation: the position and heading still to add to our car (spread over a few frames), and over how long
    catch: [f64; 3], catch_t: f64, hit_t0: f64,
}

fn main() {
    let maps: Vec<String> = { let a: Vec<String> = std::env::args().skip(1).collect(); if a.is_empty() { vec!["lakes".into(), "touge".into()] } else { a } };
    let secs = env("SECS", 120.0); let lat = env("LAT", 50.0) / 1000.0; let jit = env("JIT", 20.0) / 1000.0; let loss = env("LOSS", 0.02); let tcp = env("TCP", 0.0) > 0.0;
    let tickjit = env("TICKJIT", 4.0) / 1000.0; let dt = 1.0 / 60.0; let seed = env("SEED", 1.0) as u64;
    let comp = env("COMP", 0.0) > 0.0; let margin = env("MARGIN", 0.0); let nudge_k = env("NUDGE", fogwood_core::game::race::SHOW); let amargin = env("AMARGIN", 0.25); let resti = env("RESTI", 1.3); let near_rate = env("NEARRATE", 0.0) > 0.0;
    println!("link: {:.0} ms +/- {:.0} ms, {:.0}% loss, {}   frame-timing jitter {:.0} ms", lat * 1000.0, jit * 1000.0, loss * 100.0, if tcp { "relay over TCP" } else { "UDP" }, tickjit * 1000.0);
    println!("{:7} {:4} {:>7} {:>16} {:>6} {:>14} {:>14} {:>7} {:>12}", "map", "", "felt", "latency avg/p90", "ghost", "err rms/p99", "shake p50/p99", "gap rms", "jolt p50/p90");
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
        let amax = if mid == 1 { 4.5 } else { 7.0 };
        let curv: Vec<f64> = (0..np).map(|i| { let a = path[(i + np - 6) % np]; let b = path[i]; let c = path[(i + 6) % np];
            let h1 = (b.0 - a.0).atan2(b.1 - a.1); let h2 = (c.0 - b.0).atan2(c.1 - b.1); (wrap(h2 - h1) / 12.0).abs() }).collect();
        let mut vplan: Vec<f64> = curv.iter().map(|k| (amax / k.max(1e-4)).sqrt().min(40.0)).collect();
        for _ in 0..2 { for i in (0..np).rev() { let n = vplan[(i + 1) % np]; vplan[i] = vplan[i].min((n * n + 2.0 * 7.0).sqrt()); } }
        let tan = |i: usize| { let a = path[(i + np - 1) % np]; let b = path[(i + 1) % np]; let (dx, dz) = (b.0 - a.0, b.1 - a.1); let l = dx.hypot(dz).max(1e-6); (dx / l, dz / l) };
        let drive = |car: &Car, t: f64, who: usize, wob: f64, catch: f64| -> Input {
            let i = fogwood_core::game::path_idx(w, car.x, car.z); let u = car.u.max(0.0); let ld = 6.0 + u * 0.38; let k = (i + ld as usize) % np;
            // the line and the pace wander, out of step between the two cars: they keep closing up, leaning and passing
            let ph = if who == 0 { 0.0 } else { 2.2 }; let lane = 1.1 * (t * 0.37 + ph).sin(); let (tx, tz) = tan(k);
            let (px, pz) = (path[k].0 - tz * lane, path[k].1 + tx * lane);
            let vh = if car.vx.hypot(car.vz) > 2.0 { car.vx.atan2(car.vz) } else { car.h };
            let al = wrap((px - car.x).atan2(pz - car.z) - vh); let lock_k = tune.lock / (1.0 + u / tune.lock_fall);
            let sp = car.vx.hypot(car.vz); let r_want = 2.0 * sp.max(1.0) * al.sin() / ld;
            let delta = wrap(vh - car.h) + (2.0 * 2.5 * al.sin() / ld).atan() - (car.r - r_want) * 0.12 + wob;
            let pace = 0.86 + 0.08 * (t * 0.21 + if who == 0 { 0.0 } else { 3.1 }).sin() + catch;
            let vt = vplan[(i + 4) % np] * pace;
            Input { steer: (delta / lock_k).clamp(-1.0, 1.0), throttle: ((vt - u) * 0.6).clamp(0.0, 1.0), brake: ((u - vt - 0.8) * 0.5).clamp(0.0, 1.0), hb: false }
        };
        let place = |i: usize, lat: f64| -> Car { let mut car = Car::new(); let (tx, tz) = tan(i); car.x = path[i].0 - tz * lat; car.z = path[i].1 + tx * lat; car.h = tx.atan2(tz);
            car.gear = 1.0; car.rpm = 900.0; car.y = w.height(car.x, car.z) + 0.075; for wh in car.wh.iter_mut() { wh.comp = 0.135; wh.d = 0.075; wh.fz = 2900.0; } car };
        for ver in ["old", "now"] {
            let legacy = ver == "old";
            let mk = |c: Car| Phone { car: c, tr: { let mut t = Track::default(); t.legacy = legacy; t }, pol: Policy::default(), stamp: ViewClock::default(), view: ViewClock::default(), seq: 0, last_seq: None,
                inbox: Vec::new(), hit_acc: None, hit_tx: -9.0, net_hits: Vec::new(), epoch: 0, contact_t: -9.0, hit_t: -9.0, shown: c, have: false, pushes: Vec::new(), catch: [0.0; 3], catch_t: 0.0, hit_t0: 0.0 };
            let mut ph = [mk(place(0, 1.2)), mk(place(4, -1.2))];
            let mut links = [Link { lat, jit, loss, tcp, last: 0.0, rng: Rng(seed * 77 + 1) }, Link { lat, jit, loss, tcp, last: 0.0, rng: Rng(seed * 77 + 2) }];
            let mut rng = Rng(seed * 31 + 7); let mut wob = [0.0f64; 2];
            let rate = ((fogwood_core::net::BUDGET - 250.0) / (2.0 * 3.0 * (proto::CAR3_LEN as f64 + 28.0))).clamp(4.0, 24.0) as f32; let beat = (3.0 / rate).clamp(0.4, 0.75);
            let n = (secs / dt) as usize;
            // per frame: truth of both cars, what each phone drew of the other, whether the real cars touched
            let mut truth: Vec<[Car; 2]> = Vec::with_capacity(n); let mut drawn: Vec<[Car; 2]> = Vec::with_capacity(n); let mut touch: Vec<bool> = Vec::with_capacity(n);
            let mut prog = [0.0f64; 2]; let mut lidx = [0usize, 4usize]; let mut stuck = [0.0f64; 2]; let mut resets = 0;
            for f in 0..n {
                let t = f as f64 * dt;
                // the car behind pushes on, the one ahead lifts a little: they stay together for the whole run
                for p in 0..2 { let i = fogwood_core::game::path_idx(w, ph[p].car.x, ph[p].car.z); let mut d = i as i64 - lidx[p] as i64; if d < -(np as i64) / 2 { d += np as i64; } if d > np as i64 / 2 { d -= np as i64; } prog[p] += d as f64; lidx[p] = i; }
                // beached or stuck (a crash): back on the road like the game does, a little behind
                for p in 0..2 { let c = &ph[p].car; let bad = w.path_dist(c.x, c.z) > fogwood_core::world::ROAD + 4.0 || (c.u < 2.0 && t > 3.0);
                    stuck[p] = if bad { stuck[p] + dt } else { 0.0 };
                    if stuck[p] > 1.5 { stuck[p] = 0.0; resets += 1; let i = (lidx[p] + np - 5) % np; let mut nc = place(i, 0.0); let (tx, tz) = tan(i); nc.vx = tx * 12.0; nc.vz = tz * 12.0; nc.u = 12.0; ph[p].car = nc; } }
                let lead = prog[0] - prog[1]; let catch = [(-lead * 0.02).clamp(-0.06, 0.14), (lead * 0.02).clamp(-0.06, 0.14)];
                for p in 0..2 {
                    let o = 1 - p; let tick = t + rng.f() * tickjit;
                    // 1. what has arrived
                    let (due, rest): (Vec<(f64, Msg)>, Vec<(f64, Msg)>) = std::mem::take(&mut ph[p].inbox).into_iter().partition(|m| m.0 <= tick); ph[p].inbox = rest;
                    let mut due = due; due.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
                    for (_, m) in due { match m {
                        Msg::Car(b) => { let (_, sq, ms, mut c) = proto::read_car2(&b).unwrap(); let me = &mut ph[p];
                            if let Some(l) = me.last_seq { if (sq.wrapping_sub(l) as i8) <= 0 { continue; } } me.last_seq = Some(sq);
                            let age = ((tick * 1000.0).rem_euclid(65536.0) - ms as f64).rem_euclid(65536.0) / 1000.0; c.t = tick - age; me.tr.push_at(c, tick); me.have = true; }
                        Msg::Hit(d, imp, t0) => { let me = &mut ph[p]; if legacy { continue; } me.epoch = (me.epoch + 1) & 15; if t - me.contact_t < 0.35 { continue; } me.hit_t0 = t0; me.net_hits.push((d.map(|v| v as f64), imp as f64)); }
                    } }
                    // 2. our state out (before the physics step, like the game), bumps out
                    let dist = (ph[p].shown.x - ph[p].car.x).hypot(ph[p].shown.z - ph[p].car.z); let close = dist < if legacy { 25.0 } else { 30.0 };
                    let me = &mut ph[p]; let mut s = snap_of(&me.car, me.epoch); s.t = if legacy { tick } else { me.stamp.advance(tick, dt) }; s.a = me.pol.accel(s.u, dt as f32);
                    let mut thr = Thresholds::default(); let (mut r, mut bt) = (rate, beat);
                    if close { if near_rate && !legacy { let (t2, r2, b2) = smooth::relevance(thr, r, bt, dist as f32); thr = t2; r = r2; bt = b2; } else { thr.pos *= 0.4; thr.head *= 0.5; thr.speed *= 0.5; r = (r * 1.5).min(30.0); bt = bt.min(0.25); } }
                    if me.pol.due(&s, g, &thr, r, bt) {
                        me.seq = me.seq.wrapping_add(1); let b = proto::car3(7, me.seq, &s); let (_, _, _, mut q) = proto::read_car2(&b).unwrap(); q.t = s.t; me.pol.sent = Some(q);
                        if let Some(a) = links[p].arrival(tick) { ph[o].inbox.push((a, Msg::Car(b))); }
                    }
                    if !legacy { if let Some((d, imp)) = ph[p].hit_acc { if tick - ph[p].hit_tx >= 0.06 { ph[p].hit_tx = tick; ph[p].hit_acc = None; let t0 = ph[p].contact_t;
                        if let Some(a) = links[p].arrival(tick) { ph[o].inbox.push((a, Msg::Hit(d, imp, t0))); } } } }
                    // 3. the other car, drawn where it is at the end of this frame (old: at its start)
                    let me = &mut ph[p]; let vt = me.view.advance(tick, dt);
                    me.tr.near = if legacy { if dist < 25.0 { 1.0 } else { 0.0 } } else { let q = ((40.0 - dist) / 25.0).clamp(0.0, 1.0); (q * q * (3.0 - 2.0 * q)) as f32 };
                    let sn = if me.have { me.tr.sample(vt + if legacy { 0.0 } else { dt }, g) } else { None };
                    if let Some(c) = sn { let sh = &mut me.shown; sh.x = c.x as f64; sh.y = c.y as f64; sh.z = c.z as f64; sh.h = c.h as f64; sh.vx = c.vx as f64; sh.vz = c.vz as f64; sh.r = c.r as f64; sh.u = c.u as f64; }
                    let owned = me.tr.owned();
                    // 4. our physics
                    wob[p] += (rng.f() - 0.5) * 0.01 - wob[p] * 0.03; let inp = drive(&me.car, t, p, wob[p], catch[p]); me.car.update(w, &e.obstacles, &tune, dt, &inp);
                    // 5. pushes from the other phone
                    for (d, _) in std::mem::take(&mut me.net_hits) { me.car.vx += d[0]; me.car.vz += d[1]; me.car.r += d[2];
                        let s = (d[3].hypot(d[4]) / 0.25).max(1.0);
                        if comp { let age = (t - me.hit_t0).clamp(0.0, 0.4); me.catch = [me.catch[0] + d[3] + d[0] * age, me.catch[1] + d[4] + d[1] * age, me.catch[2] + d[2] * age]; me.catch_t = 0.12; }
                        else { me.car.x += d[3] * 0.5 / s; me.car.z += d[4] * 0.5 / s; }
                        me.hit_t = t; me.pushes.push(t); }
                    if comp && me.catch_t > 0.0 { let f = (dt / me.catch_t).min(1.0); me.car.x += me.catch[0] * f; me.car.z += me.catch[1] * f; me.car.h += me.catch[2] * f; for q in 0..3 { me.catch[q] *= 1.0 - f; } me.catch_t -= dt; }
                    // 6. contact with the car as drawn here
                    if me.have { let (mut a, mut b) = (me.car, me.shown); let o0 = me.shown; let mg = margin + (me.tr.horizon() as f64 * amargin).min(0.25); let hit = bump_r(&mut a, &mut b, 1180.0, tune.iz, 1.72 - mg, resti);
                        let d = [b.vx - o0.vx, b.vz - o0.vz, b.r - o0.r, b.x - o0.x, b.z - o0.z];
                        if d.iter().any(|v| v.abs() > 1e-6) {
                            me.car = a; if hit > 0.05 { me.pushes.push(t); }
                            if t - me.contact_t.max(me.hit_t) > 0.3 { me.epoch = (me.epoch + 1) & 15; } me.contact_t = t;
                            let df = d.map(|v| v as f32); me.tr.nudge(df.map(|v| v * nudge_k as f32));
                            if !legacy { let acc = me.hit_acc.get_or_insert(([0.0; 5], 0.0)); for k in 0..5 { acc.0[k] += df[k]; } acc.1 = acc.1.max(hit as f32); }
                            if !owned { let k = nudge_k; let sh = &mut me.shown; sh.x += (b.x - sh.x) * k; sh.z += (b.z - sh.z) * k; sh.vx += (b.vx - sh.vx) * k; sh.vz += (b.vz - sh.vz) * k; sh.r += (b.r - sh.r) * k; }
                        } }
                }
                let (mut a, mut b) = (ph[0].car, ph[1].car); let hit = bump(&mut a, &mut b, 1180.0, tune.iz);
                truth.push([ph[0].car, ph[1].car]); drawn.push([ph[0].shown, ph[1].shown]); touch.push(hit > 0.05);
            }
            // true contacts (episodes: 0.3 s apart) and the first push each car's own phone gave it around each
            let mut starts: Vec<f64> = Vec::new(); let mut last = -9.0;
            for (f, &tc) in touch.iter().enumerate() { if tc { let t = f as f64 * dt; if t - last > 0.3 { starts.push(t); } last = t; } }
            let (mut felt, mut lats) = (0usize, Vec::new());
            for &t0 in &starts { for p in 0..2 { if let Some(&tp) = ph[p].pushes.iter().find(|&&tp| tp >= t0 - 0.3 && tp <= t0 + 1.0) { felt += 1; lats.push(((tp - t0) * 1000.0).max(0.0)); } } }
            let touch_t: Vec<f64> = touch.iter().enumerate().filter(|x| *x.1).map(|x| x.0 as f64 * dt).collect();
            let ghost = (0..2).map(|p| { let mut ep = -9.0; let mut n = 0; for &tp in &ph[p].pushes { if tp - ep > 0.3 && !touch_t.iter().any(|&tt| (tt - tp).abs() < 0.4) { n += 1; } ep = tp; } n }).sum::<usize>();
            // the drawn car around contacts, and side by side without contact
            let (mut err, mut shake, mut gap) = (Vec::new(), Vec::new(), Vec::new()); let mut tv = vec![[0.0f64; 2]; starts.len() * 2];
            let near_contact = |f: usize| { let t = f as f64 * dt; starts.iter().any(|&s| t >= s - 0.1 && t <= s + 1.0) };
            for f in 120..n - 1 { for p in 0..2 { let o = 1 - p;
                let (a, b, c) = (&drawn[f - 1][p], &drawn[f][p], &drawn[f + 1][p]); let (ta, tb, tc) = (&truth[f - 1][o], &truth[f][o], &truth[f + 1][o]);
                let real_d = (truth[f][0].x - truth[f][1].x).hypot(truth[f][0].z - truth[f][1].z);
                if near_contact(f) {
                    err.push((b.x - tb.x).hypot(b.z - tb.z));
                    let ax = (a.x - 2.0 * b.x + c.x) - (ta.x - 2.0 * tb.x + tc.x); let az = (a.z - 2.0 * b.z + c.z) - (ta.z - 2.0 * tb.z + tc.z); shake.push(ax.hypot(az) / (dt * dt));
                    let t = f as f64 * dt; if let Some(k) = starts.iter().position(|&s| t >= s - 0.1 && t <= s + 1.0) {
                        tv[k * 2 + p][0] += (a.x - 2.0 * b.x + c.x).hypot(a.z - 2.0 * b.z + c.z) / dt; tv[k * 2 + p][1] += (ta.x - 2.0 * tb.x + tc.x).hypot(ta.z - 2.0 * tb.z + tc.z) / dt; }
                } else if real_d < 8.0 {
                    let me = &truth[f][p]; gap.push((b.x - me.x).hypot(b.z - me.z) - real_d);
                } } }
            let mut extra: Vec<f64> = tv.iter().map(|v| v[0] - v[1]).collect();
            if let Ok(k) = std::env::var("TRACE") { let k: usize = k.parse().unwrap_or(0); if let Some(&t0) = starts.get(k) {
                println!("episode {} at {:.2}: jolt {:.1} (phone 0 sees car 1) {:.1} (phone 1 sees car 0)", k, t0, tv[k * 2][0] - tv[k * 2][1], tv[k * 2 + 1][0] - tv[k * 2 + 1][1]);
                for f in ((t0 - 0.15) / dt) as usize..((t0 + 1.0) / dt) as usize { let t = f as f64 * dt; let (d1, t1, d0, t0c) = (&drawn[f][0], &truth[f][1], &drawn[f][1], &truth[f][0]);
                    println!("{:6.3} touch={} | 1 drawn ({:7.2},{:7.2}) v({:6.2},{:6.2}) true ({:7.2},{:7.2}) v({:6.2},{:6.2}) err {:4.2} | 0 drawn-true err {:4.2} | push0 {} push1 {}", t, touch[f] as u8,
                        d1.x, d1.z, d1.vx, d1.vz, t1.x, t1.z, t1.vx, t1.vz, (d1.x - t1.x).hypot(d1.z - t1.z), (d0.x - t0c.x).hypot(d0.z - t0c.z),
                        ph[0].pushes.iter().any(|&q| (q - t).abs() < 1e-6) as u8, ph[1].pushes.iter().any(|&q| (q - t).abs() < 1e-6) as u8); } } }
            if std::env::var("RESETS").is_ok() { println!("resets {}", resets); }
            println!("{:7} {:4} {:3}/{:<3} {:7.0} / {:6.0} {:6} {:6.2} / {:5.2} {:6.0} / {:5.0} {:7.2} {:6.1} / {:4.1}", map, ver, felt, starts.len() * 2, lats.iter().sum::<f64>() / lats.len().max(1) as f64, pct(&mut lats, 0.9), ghost,
                rms(&err), pct(&mut err, 0.99), pct(&mut shake, 0.5), pct(&mut shake, 0.99), rms(&gap), pct(&mut extra, 0.5), pct(&mut extra, 0.9));
        }
    }
}
