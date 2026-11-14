//! net_probe host|join <code> <secs>: one online session, prints the room and sends a moving test car (for local tests).
use fogwood_core::net;
fn main() {
    let a: Vec<String> = std::env::args().collect(); let secs: f64 = a.get(3).and_then(|v| v.parse().ok()).unwrap_or(8.0);
    let code = if a[1] == "host" { net::host("HOSTY", "lakes") } else { let c = a[2].parse().unwrap(); net::join(c, "GUESTY", "lakes"); c };
    println!("room {}", code); let t0 = std::time::Instant::now(); let mut next = 0.0; let mut started = false;
    while t0.elapsed().as_secs_f64() < secs {
        let t = t0.elapsed().as_secs_f64();
        net::with(|s| { s.poll();
            let c = net::proto::CarSnap { x: (t * 10.0) as f32, vx: 10.0, h: 0.0, ..Default::default() }; s.send_car(c);
            if s.is_host && t > 3.0 && !started { started = true; let st = s.start_race("lakes", 3, 5.0).unwrap(); println!("start go_at {:.3}", st.go_at); }
            if let Some(st) = s.start_in.take() { println!("got start map {} laps {} go_at {:.3} slots {:?} (host now {:.3})", st.map, st.laps, st.go_at, st.slots, s.host_now()); }
            if t >= next { next += 1.0;
                for p in s.peers.values() { let r = s.remote(p.id); println!("t={:.1} peer {} {:?} rtt {:.1} ms host_now {:.3} remote x {:?} (expect ~{:.1})", t, p.name, p.link(), p.rtt * 1000.0, s.host_now(), r.map(|c| c.x), (t - if p.direct.is_some() { 0.1 } else { 0.25 }) * 10.0); }
                if s.peers.is_empty() { println!("t={:.1} brokers up {} public {:?}", t, s.brokers_up(), s.public); } } });
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
    net::leave();
}
