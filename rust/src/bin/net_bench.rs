//! net_bench <k> <secs> <code file>: one phone of a room, for measuring how other players' cars look through the real
//! netcode and relay. Phone 0 hosts (and writes the room code to the file), the others join. Every phone drives a car
//! along a known figure of eight (60 frames a second; phone k a phase further on), so each can compare the cars it
//! draws with where they really were. Prints, per remote car:
//!   shake  extra acceleration of the drawn car compared with the real one (m/s2, p50 / p99): choppiness
//!   path   distance from the drawn car to the real path at the moment it is drawn for (m, rms / p99): accuracy
//!   delay  how far in the past it is drawn (ms, mean)
//!   plus this phone's traffic (B/s both ways, all headers) and the link.
//! Env as for the game: FW_RELAY, FW_NODIRECT, FW_LAG / FW_LOSS (everything we receive), FW_LANELAG (each relay lane its
//! own slow TCP link), FW_ONELANE, FW_NOCOALESCE, FW_NOBATCH. BOTS=n: phone 0 also drives n AI-like cars.
use fogwood_core::net::{self, proto::CarSnap};

/// the real car of phone k (or AI car k + 100) at host time t: a figure of eight with a wobble, 20-35 m/s
fn truth(k: u32, t: f64) -> CarSnap {
    let pos = |t: f64| { let ph = k as f64 * 1.7; let a = 0.11;
        (180.0 * (a * t + ph).sin() + 2.0 * (1.3 * t + ph).sin(), 90.0 * (2.0 * a * t + ph).sin() + 1.5 * (0.9 * t).cos()) };
    let e = 1e-3; let (x, z) = pos(t); let (x1, z1) = pos(t + e); let (x0, z0) = pos(t - e);
    let (vx, vz) = ((x1 - x0) / (2.0 * e), (z1 - z0) / (2.0 * e)); let h = vx.atan2(vz);
    let hn = (x1 - x).atan2(z1 - z); let hp = (x - x0).atan2(z - z0); let mut dh = hn - hp; while dh > std::f64::consts::PI { dh -= std::f64::consts::TAU; } while dh < -std::f64::consts::PI { dh += std::f64::consts::TAU; }
    let u = vx.hypot(vz);
    CarSnap { t, x: x as f32, z: z as f32, h: h as f32, vx: vx as f32, vz: vz as f32, r: (dh / e) as f32, u: u as f32, gear: 4, rpm: 6000, y: 0.075, ..Default::default() }
}
fn pct(v: &mut Vec<f64>, p: f64) -> f64 { if v.is_empty() { return 0.0; } v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v[((v.len() - 1) as f64 * p) as usize] }
fn rms(v: &[f64]) -> f64 { (v.iter().map(|x| x * x).sum::<f64>() / v.len().max(1) as f64).sqrt() }

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let k: u32 = a[1].parse().unwrap(); let secs: f64 = a[2].parse().unwrap(); let file = &a[3];
    let bots: usize = std::env::var("BOTS").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    let name = format!("P{}", k);
    if k == 0 { let c = net::host(&name, "lakes"); std::fs::write(file, c.to_string()).unwrap(); }
    else { let c: u32 = loop { if let Ok(s) = std::fs::read_to_string(file) { if let Ok(c) = s.trim().parse() { break c; } } std::thread::sleep(std::time::Duration::from_millis(50)); }; net::join(c, &name, "lakes"); }
    let dt = 1.0 / 60.0; let t0 = std::time::Instant::now(); let mut frame = 0u64;
    // per remote car (by name): drawn positions with their draw times
    let mut drawn: std::collections::HashMap<String, Vec<(f64, f64, f32, f32)>> = Default::default();
    let warm = 8.0; let mut started = false; let mut late = 0u32; let mut snap: Option<([u64; 4], [u64; 8])> = None;
    loop {
        let wall = t0.elapsed().as_secs_f64(); if wall > secs { break; }
        net::with(|s| {
            s.poll();
            if k == 0 && !started && wall > 4.0 { started = true; s.start_race("lakes", 3, 1.0, &[], false, 0, 1); }
            if !s.racing && wall > 4.0 && (k == 0 || s.start_in.take().is_some()) { s.racing = true; s.begin_race(bots, false); }
            let th = s.host_now();
            s.send_car(truth(k, th), dt, None, None, 60.0);
            if k == 0 { for b in 0..bots { s.send_bot(b, truth(100 + b as u32, th), dt, None, None, 60.0); } }
            s.flush();
            let view = s.advance_view(dt);
            if wall > warm {
                if snap.is_none() { snap = Some((s.traffic.get(), s.ways.get())); }
                let ids: Vec<(u32, String)> = s.peers.values().map(|p| (p.id, p.name.clone())).collect();
                for (id, nm) in ids { if let Some(c) = s.remote(id, None, None) { let d = s.peers.get(&id).map(|p| p.track.delay).unwrap_or(0.0); drawn.entry(nm).or_default().push((view + dt, d as f64, c.x, c.z)); } }
                if k != 0 { for b in 0..bots { let id = net::bot_id(b as u8); if let Some(c) = s.remote(id, None, None) { let d = s.bots.get(&(b as u8)).map(|x| x.track.delay).unwrap_or(0.0); drawn.entry(format!("A{}", b)).or_default().push((view + dt, d as f64, c.x, c.z)); } } }
            }
        });
        frame += 1; let next = frame as f64 * dt; let now = t0.elapsed().as_secs_f64(); if next > now { std::thread::sleep(std::time::Duration::from_secs_f64(next - now)); } else if now - next > 0.008 { late += 1; }
    }
    let (tr, pk, link, ways) = net::with(|s| (s.traffic.get(), s.packets.get(), s.peers.values().map(|p| format!("{}:{:?}:{:.0}ms", p.name, p.link(), p.track.spread() * 1000.0)).collect::<Vec<_>>().join(" "), s.ways.get())).unwrap();
    // traffic while racing (after the warm-up)
    let (t0r, w0) = snap.unwrap_or_default(); let rs = (secs - warm).max(1.0);
    let tr: Vec<u64> = (0..4).map(|i| tr[i] - t0r[i]).collect(); let ways: Vec<u64> = (0..8).map(|i| ways[i] - w0[i]).collect(); let secs = rs;
    let up = (tr[0] + tr[2]) as f64 / secs; let down = (tr[1] + tr[3]) as f64 / secs;
    eprintln!("ways B/s up/down: webrtc {:.0}/{:.0} relay {:.0}/{:.0} udp {:.0}/{:.0} brokers {:.0}/{:.0}", ways[0] as f64 / secs, ways[1] as f64 / secs, ways[2] as f64 / secs, ways[3] as f64 / secs, ways[4] as f64 / secs, ways[5] as f64 / secs, ways[6] as f64 / secs, ways[7] as f64 / secs);
    let mut lines = Vec::new();
    for (nm, v) in drawn.iter_mut() {
        if v.len() < 100 { continue; }
        let kk: u32 = if let Some(b) = nm.strip_prefix('A') { 100 + b.parse::<u32>().unwrap() } else { nm[1..].parse().unwrap() };
        let mut shake = Vec::new(); let mut path = Vec::new(); let mut del = 0.0;
        for i in 1..v.len() - 1 {
            let (t, d, x, z) = v[i]; let (ta, _, xa, za) = v[i - 1]; let (tc, _, xc, zc) = v[i + 1];
            if (t - ta - dt).abs() > 0.004 || (tc - t - dt).abs() > 0.004 { continue; }
            // the real car at the moment this one is drawn for (drawn in the past by `d`)
            let r = |t: f64| truth(kk, t - d); let (ra, rb, rc) = (r(ta), r(t), r(tc));
            let ax = (xa as f64 - 2.0 * x as f64 + xc as f64) - (ra.x as f64 - 2.0 * rb.x as f64 + rc.x as f64);
            let az = (za as f64 - 2.0 * z as f64 + zc as f64) - (ra.z as f64 - 2.0 * rb.z as f64 + rc.z as f64);
            shake.push(ax.hypot(az) / (dt * dt)); path.push((x as f64 - rb.x as f64).hypot(z as f64 - rb.z as f64)); del += d;
            // FW_BENCHTRACE=A0: that car's worst error each half second
            if std::env::var("FW_BENCHTRACE").map(|v| v == *nm).unwrap_or(false) { let e = *path.last().unwrap(); if e > 0.5 { eprintln!("trace {} t={:.2} err {:.2} delay {:.0}ms", nm, t, e, d * 1000.0); } }
        }
        let n = shake.len().max(1) as f64;
        lines.push(format!("  {:4} shake {:5.1} / {:6.1}   path {:5.2} / {:5.2}   delay {:4.0} ms", nm, pct(&mut shake, 0.5), pct(&mut shake, 0.99), rms(&path), pct(&mut path, 0.99), del / n * 1000.0));
    }
    lines.sort();
    println!("P{} up {:.0} B/s down {:.0} B/s ({:.1} MB/h)  pkts {:?}  late frames {}  {}", k, up, down, (up + down) * 3600.0 / 1e6, pk, late, link);
    for l in lines { println!("{}", l); }
    net::leave();
}
