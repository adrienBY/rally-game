//! Offline checks: renders scenarios to WAV and prints levels, spectra and render cost.
use fogwood_audio::Engine;
use std::io::Write;

fn msg(v: &[f32]) -> Vec<u8> { v.iter().flat_map(|x| x.to_le_bytes()).collect() }
fn load(e: &mut Engine, id: usize, path: &str, looped: bool) {
    // raw 24 kHz mono s16le made by ffmpeg
    let Ok(b) = std::fs::read(path) else { eprintln!("missing {path}"); return };
    let frames = b.len() / 2; let mut m = msg(&[4.0, id as f32, 1.0, 24000.0, if looped { 1.0 } else { 0.0 }, frames as f32]); m.extend_from_slice(&b); e.message(&m);
}
fn params(rpm: f32, thr: f32, speed: f32, slide: f32, surface: f32, cam: f32, l: (f32, f32, f32, f32), c: (f32, f32, f32), wet: f32) -> Vec<u8> {
    msg(&[1.0, rpm, thr, speed, slide, 1.0, surface, wet, 0.0, 3.0, cam, l.0, l.1, l.2, l.3, c.0, c.1, c.2, 0.5, 1.0, 1.0, 0.0, 0.0, 0.0])
}
fn wav(path: &str, d: &[f32], sr: u32) {
    let mut f = std::fs::File::create(path).unwrap(); let n = d.len() as u32 * 2;
    f.write_all(b"RIFF").unwrap(); f.write_all(&(36 + n).to_le_bytes()).unwrap(); f.write_all(b"WAVEfmt ").unwrap();
    f.write_all(&16u32.to_le_bytes()).unwrap(); f.write_all(&1u16.to_le_bytes()).unwrap(); f.write_all(&2u16.to_le_bytes()).unwrap();
    f.write_all(&sr.to_le_bytes()).unwrap(); f.write_all(&(sr * 4).to_le_bytes()).unwrap(); f.write_all(&4u16.to_le_bytes()).unwrap(); f.write_all(&16u16.to_le_bytes()).unwrap();
    f.write_all(b"data").unwrap(); f.write_all(&n.to_le_bytes()).unwrap();
    for v in d { f.write_all(&((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()).unwrap(); }
}
fn stats(name: &str, d: &[f32], ms: f64, secs: f32) {
    let mut pk = 0.0f32; let mut ss = 0.0f64; let mut nan = 0; let mut lr = 0.0f64;
    for (i, v) in d.iter().enumerate() { if !v.is_finite() { nan += 1; continue; } pk = pk.max(v.abs()); ss += (*v as f64).powi(2); if i % 2 == 0 && i + 1 < d.len() { lr += ((d[i] - d[i + 1]) as f64).powi(2); } }
    let rms = (ss / d.len() as f64).sqrt(); let side = (lr / (d.len() / 2) as f64).sqrt();
    println!("{name:12} peak {pk:.3} rms {rms:.4} side {side:.4} nan {nan} | {ms:.1} ms for {secs} s ({:.2}% realtime)", ms / (secs as f64 * 10.0));
}
fn main() {
    let dir = std::env::args().nth(1).unwrap_or(".".into()); let sr = 48000u32;
    let ids = [(0, "wind", true), (1, "birds", true), (2, "engine", true), (3, "skid", true), (4, "impact", false), (5, "water_a", true), (6, "water_b", true), (7, "traffic", true), (8, "splash", false), (9, "metal", false), (12, "wind2", true)];
    let only = std::env::var("ONLY").unwrap_or_default();
    let run = |name: &str, map: f32, secs: f32, f: &dyn Fn(&mut Engine, f32)| {
        if !only.is_empty() && only != name { return vec![0.0f32; 48000 * 12]; }
        let mut e = Engine::new(sr as f32); for (id, n, l) in ids { load(&mut e, id, &format!("{dir}/{n}.raw"), l); } e.message(&msg(&[5.0, map]));
        let mut out = vec![0.0f32; 0]; let blk = 512; let mut buf = vec![0.0f32; blk * 2]; let t0 = std::time::Instant::now();
        let n = (secs * sr as f32) as usize / blk;
        for b in 0..n { let t = (b * blk) as f32 / sr as f32; f(&mut e, t); e.render(&mut buf); out.extend_from_slice(&buf); }
        let ms = t0.elapsed().as_secs_f64() * 1000.0; stats(name, &out, ms, secs); wav(&format!("{dir}/out_{name}.wav"), &out, sr); out
    };
    // engine: idle, full-throttle sweep to the limiter, lift off (overrun, blow-off), chase camera
    let sweep = run("engine", 0.0, 10.0, &|e, t| { let (rpm, thr) = if t < 1.5 { (900.0, 0.0) } else if t < 6.5 { (1500.0 + (t - 1.5) * 1200.0, 1.0) } else { (7400.0 - (t - 6.5) * 1500.0, 0.0) };
        e.message(&params(rpm, thr, (rpm - 900.0) / 200.0, 0.0, 0.0, 0.0, (0.0, 2.6, -6.4, 0.0), (0.0, 0.4, 0.0), 0.0)); });
    // spectral balance per rpm band: energy below 250 Hz vs total, via a simple DFT on 4096-sample windows
    for (t, label) in [(1.0f32, "idle"), (3.0, "3300rpm"), (5.5, "6300rpm")] {
        let s = (t * sr as f32) as usize * 2; let n = 4096; let mut lo = 0.0f64; let mut tot = 0.0f64;
        for kk in 1..400 { let f = kk as f64 * sr as f64 / n as f64; let (mut re, mut im) = (0.0f64, 0.0f64);
            for j in 0..n { let v = sweep[s + j * 2] as f64; let a = 2.0 * std::f64::consts::PI * kk as f64 * j as f64 / n as f64; re += v * a.cos(); im -= v * a.sin(); }
            let p = re * re + im * im; tot += p; if f < 250.0 { lo += p; } }
        println!("  {label:8} share of energy below 250 Hz (of <4.7 kHz): {:.0}%", lo / tot * 100.0);
    }
    // fly-by: listener at the roadside, car passes at 30 m/s (doppler + panning)
    run("flyby", 0.0, 6.0, &|e, t| { let x = -90.0 + t * 30.0; e.message(&params(6000.0, 1.0, 30.0, 0.0, 0.0, 3.0, (0.0, 1.5, 8.0, std::f32::consts::PI), (x, 0.4, 0.0), 0.0)); });
    for (m, name) in [(0.0, "forest"), (1.0, "snow"), (2.0, "canyon"), (3.0, "lakes"), (4.0, "iceland"), (5.0, "touge")] {
        run(name, m, 20.0, &|e, t| {
            if t == 0.0 { // the kind of emitters each stage really has
                let list: Vec<[f32; 5]> = match m as i32 { 3 => vec![[4.0, 15.0, -4.5, 10.0, 1.0], [4.0, -25.0, -4.5, 30.0, 1.0]],
                    4 => vec![[1.0, 90.0, 5.0, 60.0, 1.0], [2.0, -14.0, 0.0, 6.0, 1.0], [3.0, 30.0, 0.0, -25.0, 1.0]],
                    5 => vec![[5.0, 4.0, 0.0, 3.0, 1.0], [5.0, -6.0, 0.0, -2.0, 1.0], [6.0, 400.0, -80.0, 300.0, 1.0]], _ => vec![] };
                let mut v = vec![3.0, list.len() as f32]; for q in list { v.extend_from_slice(&q); } e.message(&msg(&v)); }
            e.message(&params(1100.0, 0.05, 0.0, 0.0, 0.0, 0.0, (0.0, 2.6, -6.4, t * 0.1), (0.0, 0.4, 0.0), 0.0)); });
    }
    run("gravel_slide", 0.0, 5.0, &|e, t| { e.message(&params(5000.0, 0.8, 22.0, if t > 2.0 { 7.0 } else { 0.0 }, 0.0, 0.0, (0.0, 2.6, -6.4, 0.0), (0.0, 0.4, 0.0), 0.0)); });
    run("tarmac_drift", 5.0, 5.0, &|e, t| { e.message(&params(5600.0, 0.9, 20.0, if t > 2.0 { 6.0 } else { 0.0 }, 3.0, 0.0, (0.0, 2.6, -6.4, 0.0), (0.0, 0.4, 0.0), 0.0)); });
    run("ford", 4.0, 4.0, &|e, t| { if (t * 10.0) as i32 == 10 { e.message(&msg(&[2.0, 4.0, 16.0, 0.0])); } e.message(&params(4500.0, 0.7, 16.0, 0.5, 4.0, 0.0, (0.0, 2.6, -6.4, 0.0), (0.0, 0.4, 0.0), if t > 1.0 && t < 3.0 { 1.0 } else { 0.0 })); });
}
