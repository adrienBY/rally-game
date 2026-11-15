//! Panics never cross into Java: a Rust panic unwinding out of a JNI call aborts the whole app. Every entry point runs
//! inside `run`, which catches the panic, keeps a note of it (message, place, time, what was running) in
//! `fogwood-crash.txt` next to the save, and returns a harmless value so the frame loop carries on.
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, Once};

static HOOK: Once = Once::new();
static DIR: Mutex<String> = Mutex::new(String::new());
/// the last panic's message and place, filled in by the hook (which runs before unwinding starts)
static LAST: Mutex<String> = Mutex::new(String::new());
static COUNT: AtomicU32 = AtomicU32::new(0);

/// where the crash note goes (the save file's folder)
pub fn set_dir(save_path: &str) {
    let d = std::path::Path::new(save_path).parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    if let Ok(mut g) = DIR.lock() { *g = d; }
}

pub fn install() {
    HOOK.call_once(|| {
        std::panic::set_hook(Box::new(|info| {
            let msg = if let Some(s) = info.payload().downcast_ref::<&str>() { s.to_string() } else if let Some(s) = info.payload().downcast_ref::<String>() { s.clone() } else { "panic".into() };
            let at = info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())).unwrap_or_default();
            let th = std::thread::current().name().unwrap_or("?").to_string();
            // try_lock: the hook can run while this thread already holds the lock
            if let Ok(mut g) = LAST.try_lock() { *g = format!("{msg} at {at} (thread {th})"); }
        }));
    });
}

/// how many panics were caught since the app started
pub fn count() -> u32 { COUNT.load(Ordering::Relaxed) }

/// run `f`; on a panic, note it and return `fallback`. Returns the crash note's path as well when one was written.
pub fn run<T>(what: &str, fallback: T, f: impl FnOnce() -> T) -> T { run_noted(what, f).unwrap_or(fallback) }

/// run `f`; None if it panicked (already noted)
pub fn run_noted<T>(what: &str, f: impl FnOnce() -> T) -> Option<T> {
    install();
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(v) => Some(v),
        Err(_) => { note(what); None }
    }
}

/// the crash note's path, if the folder is known
pub fn note_path() -> Option<String> {
    let d = DIR.try_lock().ok()?.clone(); if d.is_empty() { return None; }
    Some(format!("{d}/fogwood-crash.txt"))
}

fn note(what: &str) {
    let n = COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    let last = LAST.try_lock().map(|g| g.clone()).unwrap_or_default();
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let line = format!("[{secs}] #{n} in {what}: {last}\n");
    #[cfg(target_os = "android")]
    { eprintln!("Fogwood caught a panic: {line}"); }
    #[cfg(not(target_os = "android"))]
    { eprint!("caught panic: {line}"); }
    if let Some(p) = note_path() {
        use std::io::Write;
        // keep the note small: start over past 64 KB
        let big = std::fs::metadata(&p).map(|m| m.len() > 65536).unwrap_or(false);
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(!big).write(true).truncate(big).open(&p) { let _ = f.write_all(line.as_bytes()); }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn caught_and_counted() {
        let n0 = super::count();
        let v: i32 = super::run("test", -1, || { let v: Vec<i32> = Vec::new(); v[3] });
        assert_eq!(v, -1); assert_eq!(super::count(), n0 + 1);
        assert_eq!(super::run("test", 0, || 5), 5);
    }
}
