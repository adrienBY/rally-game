//! Desktop harness: speaks the same protocol as the Android bridge over stdin/stdout (one request per line),
//! so the game can be tested in a desktop browser against the real native core.
//!   P <map> <gxh> <gzh> <pathcsv> <roadhcsv>   -> ok ...
//!   G <file>                                  -> writes grid bytes to file, prints byte count
//!   O <csv>                                   -> obstacle count
//!   S <csv>                                   -> state csv
use fogwood_core::bridge;
use std::io::{BufRead, Write};
fn main() {
    let stdin = std::io::stdin(); let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let (cmd, rest) = line.split_at(1.min(line.len())); let rest = rest.trim_start();
        let resp = match cmd {
            "P" => { let p: Vec<&str> = rest.splitn(5, ' ').collect();
                if p.len() < 5 { "err args".to_string() } else { bridge::prepare(p[0].parse().unwrap_or(0), p[3], p[4], p[1].parse().unwrap_or(0), p[2].parse().unwrap_or(0)) } }
            "G" => { let b = bridge::grid_bytes(); std::fs::write(rest, &b).map(|_| b.len().to_string()).unwrap_or_else(|e| format!("err {e}")) }
            "O" => bridge::set_obstacles(rest).to_string(),
            "S" => bridge::step(rest),
            _ => "err cmd".to_string(),
        };
        let _ = writeln!(out, "{}", resp); let _ = out.flush();
    }
}
