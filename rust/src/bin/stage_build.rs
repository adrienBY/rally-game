//! stage_build <map|all> [outdir]: builds stages natively (the Rust stage builder) and writes <map>.fws / <map>.frt, with timings.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let which = a.get(1).cloned().unwrap_or("forest".into()); let out = a.get(2).cloned().unwrap_or("/tmp/rstages".into());
    std::fs::create_dir_all(&out).unwrap();
    let maps: Vec<String> = if which == "all" { fogwood_core::game::MAPS.iter().map(|s| s.to_string()).collect() } else { vec![which] };
    for m in maps {
        let b = fogwood_core::stage::build(&m, &[]);
        std::fs::write(format!("{}/{}.fws", out, m), &b.scene).unwrap(); std::fs::write(format!("{}/{}.frt", out, m), &b.rt).unwrap();
        println!("{} built in {:.0} ms: scene {} bytes, runtime {} bytes, {} obstacles", m, b.ms, b.scene.len(), b.rt.len(), b.obstacles.len());
    }
}
