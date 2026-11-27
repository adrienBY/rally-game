//! The recordings (wind, birds, engine, tyres, impacts, water, lava...): decoded natively (MP3 and 16-bit WAV), resampled to 24 kHz and
//! capped at 30 s as the page's audio decoding did, and handed to the sound engine as "recording" messages.
/// file name, the sound engine's id, looped
pub const SOUNDS: [(&str, u32, bool); 17] = [("wind.mp3", 0, true), ("birds.mp3", 1, true), ("engine.wav", 2, true), ("skid.wav", 3, true), ("impact.wav", 4, false),
    ("water_a.mp3", 5, true), ("water_b.mp3", 6, true), ("traffic.mp3", 7, true), ("splash.mp3", 8, false), ("metal.mp3", 9, false), ("plank.mp3", 10, false), ("soft.mp3", 11, false),
    ("wind2.mp3", 12, true), ("boom.mp3", 13, false), ("explo.mp3", 14, false), ("lava.mp3", 15, true), ("erupt.mp3", 16, true)];
const RATE: f64 = 24000.0;

fn wav(b: &[u8]) -> Option<(Vec<Vec<f32>>, f64)> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" { return None; }
    let (mut ch, mut rate, mut bits) = (0usize, 0f64, 0u16); let mut p = 12;
    while p + 8 <= b.len() { let id = &b[p..p + 4]; let n = u32::from_le_bytes([b[p + 4], b[p + 5], b[p + 6], b[p + 7]]) as usize; let d = &b[p + 8..(p + 8 + n).min(b.len())];
        if id == b"fmt " && d.len() >= 16 { ch = u16::from_le_bytes([d[2], d[3]]) as usize; rate = u32::from_le_bytes([d[4], d[5], d[6], d[7]]) as f64; bits = u16::from_le_bytes([d[14], d[15]]); }
        if id == b"data" && ch > 0 && bits == 16 { let frames = d.len() / 2 / ch; let mut out = vec![Vec::with_capacity(frames); ch];
            for f in 0..frames { for c in 0..ch { let i = (f * ch + c) * 2; out[c].push(i16::from_le_bytes([d[i], d[i + 1]]) as f32 / 32768.0); } } return Some((out, rate)); }
        p += 8 + n + (n & 1); }
    None
}
fn mp3(b: &[u8]) -> Option<(Vec<Vec<f32>>, f64)> {
    let mut dec = nanomp3::Decoder::new(); let mut pcm = vec![0f32; nanomp3::MAX_SAMPLES_PER_FRAME]; let mut pos = 0; let mut out: Vec<Vec<f32>> = Vec::new(); let mut rate = 0.0;
    while pos < b.len() { let (used, info) = dec.decode(&b[pos..], &mut pcm); if used == 0 && info.is_none() { break; } pos += used;
        if let Some(i) = info { let ch = i.channels.num() as usize; if out.is_empty() { out = vec![Vec::new(); ch]; rate = i.sample_rate as f64; }
            for f in 0..i.samples_produced { for c in 0..out.len() { out[c].push(pcm[f * ch + c.min(ch - 1)]); } } } }
    if out.is_empty() { None } else { Some((out, rate)) }
}
/// the engine message for one recording file (None: unknown name or undecodable)
pub fn message(name: &str, bytes: &[u8]) -> Option<Vec<u8>> {
    let &(_, id, looped) = SOUNDS.iter().find(|s| s.0 == name)?;
    let (chs, rate) = if name.ends_with(".wav") { wav(bytes)? } else { mp3(bytes)? };
    let ch = chs.len().min(2); let src_n = chs[0].len(); let n = ((src_n as f64 * RATE / rate).floor() as usize).min(24000 * 30);
    let mut m = Vec::with_capacity(24 + n * ch * 2); for v in [4.0, id as f32, ch as f32, RATE as f32, if looped { 1.0 } else { 0.0 }, n as f32] { m.extend_from_slice(&(v as f32).to_le_bytes()); }
    for c in 0..ch { let s = &chs[c]; for i in 0..n { let t = i as f64 * rate / RATE; let k = t as usize; let f = (t - k as f64) as f32;
        let v = if k + 1 < src_n { s[k] + (s[k + 1] - s[k]) * f } else { s[src_n - 1] }; m.extend_from_slice(&(((v * 32767.0).clamp(-32767.0, 32767.0)) as i16).to_le_bytes()); } }
    Some(m)
}
/// decode and hand over (to the app's engine, or the browser's worklet)
pub fn load(name: &str, bytes: &[u8]) -> bool { match message(name, bytes) { Some(m) => { super::audio_out(&m); true } None => false } }
#[cfg(test)]
mod tests {
    #[test]
    fn decodes_every_recording() {
        let web = concat!(env!("CARGO_MANIFEST_DIR"), "/../web/");
        for (name, _, _) in super::SOUNDS { let p = if ["wind.mp3", "birds.mp3", "engine.wav", "skid.wav", "impact.wav"].contains(&name) { format!("{web}{name}") } else { format!("{web}snd/{name}") };
            let b = std::fs::read(&p).unwrap(); let m = super::message(name, &b).unwrap_or_else(|| panic!("{name}"));
            let n = f32::from_le_bytes([m[20], m[21], m[22], m[23]]) as usize; let ch = f32::from_le_bytes([m[8], m[9], m[10], m[11]]) as usize;
            assert!(n > 2400, "{name} {n}"); assert_eq!(m.len(), 24 + n * ch * 2); }
    }
}
