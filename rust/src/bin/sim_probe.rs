//! sim_probe host <stage.fws> <secs> | join <code> <secs>: the relay's AI cars, end to end, without a game. The host uploads
//! the stage and starts a race with three AI cars on the relay; both sides send a moving test car and print, every
//! second, whether the relay drives the AI cars and where each one is (it should be racing round the stage).
//! FW_RELAY overrides the relay (default: the Cloudflare one).
use fogwood_core::net;
fn main() {
    let a: Vec<String> = std::env::args().collect(); let secs: f64 = a.get(3).and_then(|v| v.parse().ok()).unwrap_or(30.0);
    let host = a[1] == "host";
    let blob = if host { std::fs::read(&a[2]).expect("stage file") } else { Vec::new() };
    let code = if host { net::host("SIMHOST", "lakes") } else { let c = a[2].parse().unwrap(); net::join(c, "SIMGUEST", "lakes"); c };
    println!("room {}", code); if host { std::fs::write("/tmp/sim_code", code.to_string()).ok(); }
    let t0 = std::time::Instant::now(); let mut next = 0.0; let mut started = false; let mut nb = 0usize; let mut sent_stage = false;
    while t0.elapsed().as_secs_f64() < secs {
        let t = t0.elapsed().as_secs_f64();
        net::with(|s| { s.poll();
            let c = net::proto::CarSnap { x: (t * 10.0) as f32, vx: 10.0, h: std::f32::consts::FRAC_PI_2, ..Default::default() }; s.send_car(c, 0.016, None, None, 1e9); s.flush(); s.advance_view(0.016);
            if host && !started && !s.peers.is_empty() && t > 4.0 {
                if !sent_stage { sent_stage = true; println!("relay can drive AI cars: {}; stage upload ({} bytes): {}", s.sim_capable(), blob.len(), s.upload_stage(&blob)); }
                started = true; let st = s.start_race("lakes", 3, 6.0, &[0, 1, 2], true, 0).unwrap(); println!("start go_at {:.3} srv {}", st.go_at, st.srv);
                s.racing = true; nb = st.bots.len(); s.begin_race(nb, st.srv); }
            if let Some(st) = s.start_in.take() { println!("got start: {} AI cars, on the relay: {}", st.bots.len(), st.srv); s.racing = true; nb = st.bots.len(); s.begin_race(nb, st.srv); }
            if t >= next { next += 1.0;
                let bots: Vec<String> = (0..nb as u8).map(|k| match s.remote(net::bot_id(k), None, None) { Some(c) => format!("[{:.0},{:.0} {:.0}km/h]", c.x, c.z, c.u * 3.6), None => "[-]".into() }).collect();
                println!("t={:.0} peers {} relay stage {:?} relay drives {} silence {:.1}s AI cars {}", t, s.peers.len(), s.sim_state(), s.server_bots, s.server_silence(), bots.join(" ")); } });
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
    net::leave();
}
