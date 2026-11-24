//! The relay's copy of car states, only where it helps. With a WebRTC link to a phone, our car states go over both the
//! link and the relay, the first copy winning. On a good link the relay's copy almost never wins, and it costs a quarter
//! of the data. So the receiving phone watches how each sender's states arrive and tells the sender when the relay's copy
//! has stopped helping; the sender then sends that phone only a thin stream through the relay (a copy every THIN
//! seconds: the receiver's yardstick, and a safety net) until the reports stop or turn bad.
//!
//! Receiver: `Arrivals::note` for every car state from the sender (its own car and, from the host, its AI cars: each a
//! stream of sequence numbers) and which way it came, `verdict` a few times a second. Verdict good: in the last WINDOW
//! seconds at most 3% of the states were lost on the link (gaps in the sequence numbers that no relay copy filled; a lone
//! one is fine), and the relay's copy came first (by more than AHEAD) for at most a tenth of the states that had one. The receiver reports good every REPORT seconds over the link, bad at once over both paths.
//! Sender: a good report is trusted for TRUST seconds (link dead: the reports stop, the full relay stream is back
//! within that), a bad one ends it.
use std::collections::VecDeque;

/// which way a car state came
pub const RTC: u8 = 1;
pub const RELAY: u8 = 2;
/// the span judged (states are counted once both copies had time to arrive: SETTLE)
const WINDOW: f64 = 4.0;
const SETTLE: f64 = 0.3;
/// the relay's copy counts as first when it beat the link's by more than this: a copy a few ms sooner changes nothing
/// on screen (other cars are drawn a little in the past, by about the jitter); one much sooner fills a real hole
const AHEAD: f64 = 0.025;
/// a sender skipping the relay for a phone still sends it a copy through the relay this often
pub const THIN: f64 = 0.2;
/// how long a sender trusts a good report, and how often receivers send one
pub const TRUST: f64 = 1.5;
pub const REPORT: f64 = 0.5;
/// after a bad verdict, how long a receiver waits before reporting good again (the window refills with full copies)
pub const CALM: f64 = 3.0;

/// the sender's own car's stream (AI cars: their index)
pub const OWN: u8 = 255;
/// one sender's car states lately: (stream, sequence number, when the link's copy came, when the relay's; -1: not yet)
/// (and when the link last brought a state, and when a relay copy last suggested the link had gone quiet)
pub struct Arrivals { log: VecDeque<E>, last_rtc: f64, suspect: f64 }
impl Default for Arrivals { fn default() -> Self { Arrivals { log: VecDeque::new(), last_rtc: -99.0, suspect: -99.0 } } }
type E = (u8, u8, f64, f64);

fn first(e: &E) -> f64 { if e.2 < 0.0 { e.3 } else if e.3 < 0.0 { e.2 } else { e.2.min(e.3) } }

impl Arrivals {
    pub fn note(&mut self, stream: u8, seq: u8, via: u8, now: f64) {
        while self.log.front().map(|e| now - first(e) > WINDOW + SETTLE + 0.5).unwrap_or(false) { self.log.pop_front(); }
        fn slot(e: &mut E, via: u8) -> &mut f64 { if via == RTC { &mut e.2 } else { &mut e.3 } }
        let had_rtc = match self.log.iter_mut().rev().take(160).find(|e| e.0 == stream && e.1 == seq) {
            Some(e) => { let s = slot(e, via); if *s < 0.0 { *s = now; } e.2 >= 0.0 }
            None => { let mut e = (stream, seq, -1.0, -1.0); *slot(&mut e, via) = now; self.log.push_back(e); if self.log.len() > 1200 { self.log.pop_front(); } via == RTC } };
        if via == RTC { self.last_rtc = now; return; }
        // a relay copy the link never brought, while the link has been quiet for well over its usual gap: it has stopped
        // (judged at once, not after SETTLE: the sender is skipping the relay for us and every moment counts), unless the
        // link brings something in the same round (after a hitch here both copies are read together, the relay's first)
        if !had_rtc { let n = self.log.iter().filter(|e| e.2 >= 0.0 && now - e.2 < WINDOW).count();
            let gap = if n > 0 { WINDOW / n as f64 } else { 1.0 }; if now - self.last_rtc > (2.5 * gap).max(0.2) { self.suspect = now; } }
    }
    /// Some(true): the relay's copies don't help (the link brings everything, first); Some(false): they do; None: not
    /// enough seen to tell (fewer than 10 states settled, or fewer than 4 with a relay copy)
    pub fn verdict(&self, now: f64) -> Option<bool> { self.judge(now).0 }
    /// the verdict and what it rests on (states settled, sequence span, gaps, with a relay copy, relay first)
    pub fn judge(&self, now: f64) -> (Option<bool>, [usize; 5]) {
        let s: Vec<&E> = self.log.iter().filter(|e| { let a = now - first(e); a > SETTLE && a < WINDOW + SETTLE }).collect();
        if self.last_rtc < self.suspect && now - self.suspect < 2.0 { return (Some(false), [s.len(), 0, 0, 0, 99]); }
        if s.len() < 8 { return (None, [s.len(), 0, 0, 0, 0]); }
        // states that came neither way, stream by stream: sequence numbers relative to the stream's newest settled one
        // (a stream sends at most 30 a second: a window holds < 128)
        let (mut span, mut gaps) = (0usize, 0usize);
        let mut streams: Vec<u8> = s.iter().map(|e| e.0).collect(); streams.sort_unstable(); streams.dedup();
        for st in streams {
            let v: Vec<u8> = s.iter().filter(|e| e.0 == st).map(|e| e.1).collect(); let last = *v.last().unwrap();
            let rel: Vec<i32> = v.iter().map(|q| last.wrapping_sub(*q) as i8 as i32).collect();
            let sp = (rel.iter().max().unwrap() - rel.iter().min().unwrap() + 1) as usize;
            let mut seen = v.clone(); seen.sort_unstable(); seen.dedup(); span += sp; gaps += sp.saturating_sub(seen.len());
        }
        let with_relay = s.iter().filter(|e| e.3 >= 0.0).count();
        let ahead = s.iter().filter(|e| e.3 >= 0.0 && (e.2 < 0.0 || e.3 < e.2 - AHEAD)).count();
        let n = [s.len(), span, gaps, with_relay, ahead];
        if (gaps >= 2 && gaps * 33 > span) || (ahead >= 2 && ahead * 10 > with_relay) { return (Some(false), n); }
        if with_relay < 4 { return (None, n); }
        (Some(true), n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// a sender at 20 states a second for `secs`: `rtc(k)` / `relay(k)` give each copy's delay (None: lost or not sent)
    fn run(secs: f64, rtc: impl Fn(u32) -> Option<f64>, relay: impl Fn(u32) -> Option<f64>) -> Arrivals {
        let mut a = Arrivals::default(); let mut ev: Vec<(f64, u8, u8)> = Vec::new();
        for k in 0..(secs * 20.0) as u32 { let t = k as f64 / 20.0;
            if let Some(d) = rtc(k) { ev.push((t + d, k as u8, RTC)); } if let Some(d) = relay(k) { ev.push((t + d, k as u8, RELAY)); } }
        ev.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap()); for (t, s, v) in ev { a.note(OWN, s, v, t); a.note(0, s.wrapping_add(77), v, t); } a
    }
    #[test]
    fn good_link_needs_no_relay_copy() {
        // link 40 ms, relay 70 ms, both copies: good
        assert_eq!(run(5.0, |_| Some(0.04), |_| Some(0.07)).verdict(5.0), Some(true));
        // thin relay copies only (every 5th state): still good
        assert_eq!(run(5.0, |_| Some(0.04), |k| (k % 5 == 0).then_some(0.07)).verdict(5.0), Some(true));
        // too little seen yet
        assert_eq!(run(0.3, |_| Some(0.04), |_| Some(0.07)).verdict(0.3), None);
        assert_eq!(run(5.0, |_| Some(0.04), |_| None).verdict(5.0), None);
    }
    #[test]
    fn relay_copy_helps() {
        // the link loses 5%, no relay copies to fill them
        assert_eq!(run(5.0, |k| (k % 20 != 7).then_some(0.04), |k| (k % 5 == 0).then_some(0.07)).verdict(5.0), Some(false));
        // jittery link: the relay's copy often much sooner
        assert_eq!(run(5.0, |k| Some(if k % 3 == 0 { 0.16 } else { 0.04 }), |_| Some(0.07)).verdict(5.0), Some(false));
        // a little jitter, a lone lost state: fine
        assert_eq!(run(5.0, |k| if k == 60 { None } else { Some(if k % 3 == 0 { 0.08 } else { 0.04 }) }, |_| Some(0.07)).verdict(5.0), Some(true));
        // the link died: only the thin relay copies arrive
        assert_eq!(run(5.0, |k| (k < 40).then_some(0.04), |k| (k % 5 == 0).then_some(0.07)).verdict(5.0), Some(false));
        // ... noticed at once: 0.3 s after it died, before any state settled
        let a = run(2.35, |k| (k < 40).then_some(0.04), |k| (k % 5 == 0).then_some(0.07)); assert_eq!(a.judge(2.35).1[4], 99);
        // a hitch here (nothing read for 0.4 s, then both copies at once, the relay's first): no alarm
        let mut a = run(3.0, |_| Some(0.04), |_| Some(0.07)); a.note(OWN, 70, RELAY, 3.4); a.note(OWN, 70, RTC, 3.4);
        assert_ne!(a.judge(3.4).1[4], 99);
        // sequence numbers wrapping (u8) don't count as gaps
        assert_eq!(run(20.0, |_| Some(0.04), |_| Some(0.07)).verdict(20.0), Some(true));
    }
}
