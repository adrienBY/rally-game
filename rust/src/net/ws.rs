//! The app's WebSocket to the relay (wss://), on its own thread: connects (and reconnects with back-off), sends what
//! is queued, collects what arrives. TLS by rustls; the relay's certificate is checked against the usual web roots.
use std::io::ErrorKind;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct Ws { tx: Sender<Vec<u8>>, inbox: Arc<Mutex<Vec<Vec<u8>>>>, open: Arc<AtomicBool>, fresh: Arc<AtomicBool>, stop: Arc<AtomicBool>, kick: Arc<AtomicBool> }

impl Ws {
    pub fn open(url: String) -> Ws {
        let (tx, rx) = channel::<Vec<u8>>(); let inbox = Arc::new(Mutex::new(Vec::new()));
        let (open, fresh, stop, kick) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
        let (i2, o2, f2, s2, k2) = (inbox.clone(), open.clone(), fresh.clone(), stop.clone(), kick.clone());
        std::thread::Builder::new().name("relay ws".into()).spawn(move || {
            let mut backoff = 1.0f64;
            while !s2.load(Ordering::Relaxed) {
                let r = session(&url, &rx, &i2, &o2, &f2, &s2, &k2);
                if std::env::var("FW_WSDEBUG").is_ok() { eprintln!("ws session ended: {:?} kick={}", r.as_ref().err(), k2.load(Ordering::Relaxed)); }
                o2.store(false, Ordering::Relaxed);
                // a kicked socket (gone silent) reconnects at once; a failed one backs off
                let kicked = k2.swap(false, Ordering::Relaxed); if r.is_ok() || kicked { backoff = 0.25; }
                while rx.try_recv().is_ok() {}
                std::thread::sleep(Duration::from_secs_f64(backoff)); backoff = (backoff * 2.0).min(30.0);
            }
        }).ok();
        Ws { tx, inbox, open, fresh, stop, kick }
    }
    /// drop the connection and connect again (it has gone silent: a dead TCP connection never reports an error by itself)
    pub fn reconnect(&self) { if self.open.load(Ordering::Relaxed) { self.kick.store(true, Ordering::Relaxed); } }
    pub fn send(&self, b: &[u8]) -> bool { if self.open.load(Ordering::Relaxed) { self.tx.send(b.to_vec()).is_ok() } else { false } }
    pub fn take(&self) -> Vec<Vec<u8>> { std::mem::take(&mut *self.inbox.lock().unwrap()) }
    /// connected since the last call
    pub fn fresh(&self) -> bool { self.fresh.swap(false, Ordering::Relaxed) }
}
impl Drop for Ws { fn drop(&mut self) { self.stop.store(true, Ordering::Relaxed); } }

fn tls_config() -> Arc<rustls::ClientConfig> {
    let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    Arc::new(rustls::ClientConfig::builder_with_provider(provider).with_safe_default_protocol_versions().expect("tls versions").with_root_certificates(roots).with_no_client_auth())
}

fn session(url: &str, rx: &Receiver<Vec<u8>>, inbox: &Mutex<Vec<Vec<u8>>>, open: &AtomicBool, fresh: &AtomicBool, stop: &AtomicBool, kick: &AtomicBool) -> std::io::Result<()> {
    let err = |e: &str| std::io::Error::new(ErrorKind::Other, e.to_string());
    let (tls, rest) = if let Some(r) = url.strip_prefix("wss://") { (true, r) } else if let Some(r) = url.strip_prefix("ws://") { (false, r) } else { return Err(err("url")) };
    let hostport = rest.split('/').next().unwrap_or(rest); let host = hostport.split(':').next().unwrap_or(hostport).to_string();
    let port: u16 = hostport.split(':').nth(1).and_then(|p| p.parse().ok()).unwrap_or(if tls { 443 } else { 80 });
    let addr = (host.as_str(), port).to_socket_addrs()?.next().ok_or_else(|| err("dns"))?;
    let tcp = TcpStream::connect_timeout(&addr, Duration::from_secs(6))?; tcp.set_nodelay(true)?; tcp.set_read_timeout(Some(Duration::from_secs(8)))?;
    let stream: Box<dyn RW> = if tls {
        let name = rustls::pki_types::ServerName::try_from(host.clone()).map_err(|_| err("name"))?;
        let conn = rustls::ClientConnection::new(tls_config(), name).map_err(|e| err(&e.to_string()))?;
        Box::new(rustls::StreamOwned::new(conn, tcp))
    } else { Box::new(tcp) };
    let (mut ws, _) = tungstenite::client(url, stream).map_err(|e| err(&e.to_string()))?;
    ws.get_mut().timeout(Some(Duration::from_millis(4)))?;
    open.store(true, Ordering::Relaxed); fresh.store(true, Ordering::Relaxed);
    if std::env::var("FW_WSDEBUG").is_ok() { eprintln!("ws open"); }
    loop {
        if stop.load(Ordering::Relaxed) { let _ = ws.close(None); return Ok(()); }
        if kick.load(Ordering::Relaxed) { return Ok(()); }
        match ws.read() {
            Ok(tungstenite::Message::Binary(b)) => { let mut g = inbox.lock().unwrap(); if g.len() < 1024 { g.push(b.to_vec()); } }
            Ok(tungstenite::Message::Close(_)) => return Ok(()),
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {}
            Err(e) => return Err(err(&e.to_string())),
        }
        let mut wrote = false;
        while let Ok(b) = rx.try_recv() { ws.write(tungstenite::Message::Binary(b.into())).map_err(|e| err(&e.to_string()))?; wrote = true; }
        if wrote { ws.flush().map_err(|e| err(&e.to_string()))?; }
    }
}

/// a stream the WebSocket runs over (plain TCP or TLS) whose read timeout can be changed after the handshake
trait RW: std::io::Read + std::io::Write { fn timeout(&mut self, t: Option<Duration>) -> std::io::Result<()>; }
impl RW for TcpStream { fn timeout(&mut self, t: Option<Duration>) -> std::io::Result<()> { self.set_read_timeout(t) } }
impl RW for rustls::StreamOwned<rustls::ClientConnection, TcpStream> { fn timeout(&mut self, t: Option<Duration>) -> std::io::Result<()> { self.sock.set_read_timeout(t) } }
impl RW for Box<dyn RW> { fn timeout(&mut self, t: Option<Duration>) -> std::io::Result<()> { (**self).timeout(t) } }
