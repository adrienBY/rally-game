//! How other players' cars move on screen, and when our car's state is worth sending.
//!
//! Both phones run the same prediction from the last state sent. When the state carries the driver's inputs (and the
//! caller gives the stage and the car's tune: `Phys`), that prediction is the real car physics run on from the state
//! with those inputs held: corners, braking and slides come out as the car would do them (pred_bench: 3-5 times
//! closer than guessing from the motion, 0.2-0.5 s ahead). Otherwise the car carries on along its arc with its current
//! acceleration, glued to the stage's terrain (height, pitch and roll follow the ground under it), or flies
//! ballistically while airborne. The sender only transmits when its real car has drifted away from that prediction
//! (or its inputs changed), so the receiver's picture is never further off than the send thresholds (plus what changed
//! during the trip).
//!
//! When a new state arrives the receiver does not jump to it: the gap between where the car was being shown and the
//! new prediction becomes an offset that fades to zero along a cubic curve within a fixed time. Position *and*
//! velocity of the shown car stay continuous, so corrections look like the car steering back onto its line, never a
//! twitch, and the error is fully gone a third of a second later (no long tail).
use super::proto::{CarSnap, Ctl};
use crate::physics::{Car, Input, Obstacles, Rebuild, Tune};
use crate::world::World;

/// stage terrain height at (x, z): identical on every phone, so sender and receiver predict the same thing
pub type Ground<'a> = Option<&'a dyn Fn(f32, f32) -> f32>;

/// flags bit: the car is in the air (it flies ballistically instead of following the ground)
pub const AIR: u8 = 1;
/// flags bits 1-4: the owner's contact counter (mod 16). It moves on when the owner's phone starts settling a contact
/// with another player's car and whenever a bump message from another phone reaches it (taken or not: either way its
/// states now carry its own answer to that contact), so a receiver knows from which state on the bump is included.
pub fn epoch(flags: u8) -> u8 { (flags >> 1) & 15 }
pub fn with_epoch(flags: u8, e: u8) -> u8 { (flags & AIR) | (e & 15) << 1 }
/// how long a bump we showed on another player's car is kept on their incoming states, waiting for their own phone's
/// states to include it (s; normally a round trip)
const PEND_LIFE: f64 = 0.8;
/// a gap this long (s) without touching ends a contact
const EPISODE_GAP: f64 = 0.3;
/// how long after a contact a car is guessed from its motion instead of the physics (s)
const CONTACT_CALM: f64 = 1.0;
const G: f32 = 9.81;
/// body height above the ground at rest (the physics keeps the car's reference point ~7.5 cm above the terrain)
const REST: f32 = 0.075;
/// predictions run in full up to this age, then ease to a stop over the next half second instead of freezing
const FULL_PREDICT: f32 = 0.5;
/// a gap this large is a reset or a teleport: show the new state straight away
const SNAP_DIST: f32 = 7.0;
/// a velocity change this large (m/s) between two states is a collision, not a drift
const IMPACT_DV: f32 = 6.0;

fn wrap(a: f32) -> f32 { let tau = std::f32::consts::TAU; let a = a.rem_euclid(tau); if a > std::f32::consts::PI { a - tau } else { a } }

/// predicted time: real age up to FULL_PREDICT, then slowing down (continuous rate) so a stalled link glides, not freezes
/// (slightly negative ages are fine: rates are measured on both sides of a state)
fn soft_age(dt: f32) -> f32 { if dt <= FULL_PREDICT { dt } else { FULL_PREDICT + 0.5 * (1.0 - (-(dt - FULL_PREDICT) / 0.5).exp()) } }

/// ground pitch and roll under a car at (x, z) with heading h (pitch: nose up, roll: right side up; same signs as the physics)
fn slope(g: &dyn Fn(f32, f32) -> f32, x: f32, z: f32, h: f32) -> (f32, f32) {
    let (sh, ch) = (h.sin(), h.cos());
    let f = g(x + sh * 1.25, z + ch * 1.25) - g(x - sh * 1.25, z - ch * 1.25);
    let l = g(x + ch * 0.75, z - sh * 0.75) - g(x - ch * 0.75, z + sh * 0.75);
    ((f / 2.5).atan(), (l / 1.5).atan())
}

/// The car `dt` seconds after state `c`: constant yaw rate and sideways slip, forward speed changing at `c.a` (never
/// through zero), height/pitch/roll following the terrain or a ballistic arc in the air.
pub fn predict(c: &CarSnap, dt: f32, g: Ground) -> CarSnap {
    let dt = soft_age(dt);
    let mut s = *c; let (sh, ch) = (c.h.sin(), c.h.cos()); let u0 = c.vx * sh + c.vz * ch; let vl = c.vx * ch - c.vz * sh;
    // forward speed under constant acceleration, stopping at zero rather than reversing
    let a = c.a; let t_stop = if a * u0 < 0.0 { -u0 / a } else { f32::INFINITY };
    let n = ((dt.abs() / 0.03).ceil() as usize).clamp(1, 16); let st = dt / n as f32; let mut h = c.h; let mut t = 0.0f32;
    let speed = |t: f32| u0 + a * t.min(t_stop);
    for _ in 0..n {
        let um = speed(t + st * 0.5); let hm = h + c.r * st * 0.5; let (sa, ca) = (hm.sin(), hm.cos());
        s.x += (um * sa + vl * ca) * st; s.z += (um * ca - vl * sa) * st; h += c.r * st; t += st;
    }
    let u = speed(t); s.h = h; let (sa, ca) = (h.sin(), h.cos()); s.vx = u * sa + vl * ca; s.vz = u * ca - vl * sa; s.u = u;
    if let Some(g) = g {
        let g0 = g(c.x, c.z); let g1 = g(s.x, s.z);
        if c.flags & AIR != 0 {
            let floor = g1 + REST; s.y = c.y + c.vy * dt - 0.5 * G * dt * dt; s.vy = c.vy - G * dt;
            if s.y < floor { s.y = floor; s.vy = 0.0; }
        } else {
            s.y = g1 + (c.y - g0);
            let (p0, r0) = slope(g, c.x, c.z, c.h); let (p1, r1) = slope(g, s.x, s.z, s.h);
            s.pitch = c.pitch + (p1 - p0); s.roll = c.roll + (r1 - r0);
        }
    }
    s
}

/// What the physics prediction needs: the stage (identical on every phone) and the tune of the car being predicted
/// (game::cars::tune_for for a player's car, race::ai_tune for an AI car).
pub struct Phys<'a> { pub world: &'a World, pub obs: &'a Obstacles, pub tune: Tune }

const PDT: f64 = 1.0 / 60.0;
/// physics steps kept per prediction: up to FULL_PREDICT, then it glides on (`soft_age`) from there
const PSTEPS: usize = (FULL_PREDICT as f64 * 60.0) as usize;

/// One physics prediction: the state it starts from, the car being run on, and where it was after each step.
#[derive(Clone, Debug, Default)]
pub struct Traj { key: Option<CarSnap>, car: Car, pts: Vec<CarSnap>, used: u64 }
impl Traj {
    fn at(&mut self, c: &CarSnap, k: &Ctl, age: f32, ph: &Phys, g: Ground) -> CarSnap {
        if age < 0.0 { return predict(c, age, g); }   // (a moment before the state: the motion is as good)
        if self.key != Some(*c) {
            let r = Rebuild { x: c.x as f64, y: c.y as f64, z: c.z as f64, h: c.h as f64, vx: c.vx as f64, vy: c.vy as f64, vz: c.vz as f64, r: c.r as f64,
                pitch: c.pitch as f64, roll: c.roll as f64, steer: c.steer as f64, gear: c.gear.max(-1) as f64, rpm: c.rpm as f64, a: c.a as f64,
                throttle: k.throttle as f64, brake: k.brake as f64, hb: k.hb, air: c.flags & AIR != 0 };
            self.key = Some(*c); self.car = Car::rebuilt(ph.world, &r); self.pts.clear(); self.pts.push(*c);
        }
        let inp = Input { steer: k.steer as f64, throttle: k.throttle as f64, brake: k.brake as f64, hb: k.hb };
        let a = (age as f64 / PDT).min(PSTEPS as f64); let i0 = (a.floor() as usize).min(PSTEPS - 1); let f = (a - i0 as f64) as f32;
        while self.pts.len() < i0 + 2 {
            self.car.update(ph.world, ph.obs, &ph.tune, PDT, &inp);
            let s = &self.car; let mut p = *c;
            p.t = c.t + self.pts.len() as f64 * PDT; p.x = s.x as f32; p.y = s.y as f32; p.z = s.z as f32; p.h = s.h as f32; p.vx = s.vx as f32; p.vy = s.vy as f32; p.vz = s.vz as f32;
            p.r = s.r as f32; p.pitch = s.pitch as f32; p.roll = s.roll as f32; p.steer = s.steer as f32; p.u = s.u as f32; p.gear = s.gear as i8; p.rpm = s.rpm.clamp(0.0, 65000.0) as u16;
            p.flags = (p.flags & !AIR) | if s.contacts == 0.0 && s.air_t > 0.06 { AIR } else { 0 };
            self.pts.push(p);
        }
        // between two steps: cubic (Hermite) on position and heading, from their rates, so the drawn car's speed and
        // turning stay smooth; the rest straight
        let (p, q) = (self.pts[i0], self.pts[i0 + 1]); let d = PDT as f32;
        let herm = |a: f32, va: f32, b: f32, vb: f32| { let (f2, f3) = (f * f, f * f * f); (2.0 * f3 - 3.0 * f2 + 1.0) * a + (f3 - 2.0 * f2 + f) * d * va + (-2.0 * f3 + 3.0 * f2) * b + (f3 - f2) * d * vb };
        let lerp = |a: f32, b: f32| a + (b - a) * f;
        let mut s = p; s.t = p.t + f as f64 * PDT;
        s.x = herm(p.x, p.vx, q.x, q.vx); s.z = herm(p.z, p.vz, q.z, q.vz); s.h = herm(p.h, p.r, p.h + wrap(q.h - p.h), q.r);
        s.y = lerp(p.y, q.y); s.vx = lerp(p.vx, q.vx); s.vy = lerp(p.vy, q.vy); s.vz = lerp(p.vz, q.vz); s.r = lerp(p.r, q.r);
        s.pitch = lerp(p.pitch, q.pitch); s.roll = lerp(p.roll, q.roll); s.steer = lerp(p.steer, q.steer); s.u = lerp(p.u, q.u);
        // past the physics horizon: glide on from its end (a stalled link slows the car down, it doesn't freeze or drive off)
        if age > FULL_PREDICT { let mut e = s; e.a = 0.0; e.ctl = None; return predict(&e, soft_age(age) - FULL_PREDICT, g); }
        s
    }
}

/// The car `dt` after state `c`: the physics when the state carries inputs and `ph` is given (cached in `tr`), the
/// motion otherwise.
pub fn guess(tr: &mut [Traj], c: &CarSnap, dt: f32, g: Ground, ph: Option<&Phys>) -> CarSnap {
    match (c.ctl, ph) {
        (Some(k), Some(ph)) if !tr.is_empty() => {
            // the cached prediction for this state, else the one used longest ago
            let n = tr.iter().position(|t| t.key == Some(*c)).unwrap_or_else(|| (0..tr.len()).min_by_key(|&i| tr[i].used).unwrap());
            let used = tr.iter().map(|t| t.used).max().unwrap_or(0) + 1; tr[n].used = used;
            tr[n].at(c, &k, dt, ph, g)
        }
        _ => predict(c, dt, g),
    }
}

/// what the receiver smooths: x, z, heading, height above ground, pitch and roll relative to the ground, steering
const NCH: usize = 7;
fn channels(s: &CarSnap, g: Ground) -> [f32; NCH] {
    let (gy, gp, gr) = match g { Some(g) => { let (p, r) = slope(g, s.x, s.z, s.h); (g(s.x, s.z), p, r) } None => (0.0, 0.0, 0.0) };
    [s.x, s.z, s.h, s.y - gy, s.pitch - gp, s.roll - gr, s.steer]
}

/// Critically damped third-order fade of an offset released with value x0, rate v0 and acceleration a0:
/// x(t) = (c0 + c1 t + c2 t^2) e^(-w t). Position, velocity *and* acceleration of the shown car stay continuous.
/// Returns (x, v, a) at `tau`.
fn fade3(x0: f32, v0: f32, a0: f32, w: f32, tau: f32) -> (f32, f32, f32) {
    let (c0, c1) = (x0, v0 + w * x0); let c2 = (a0 + 2.0 * w * v0 + w * w * x0) * 0.5;
    let e = (-w * tau).exp(); let p = c0 + c1 * tau + c2 * tau * tau; let dp = c1 + 2.0 * c2 * tau;
    (p * e, (dp - w * p) * e, (2.0 * c2 - 2.0 * w * dp + w * w * p) * e)
}
/// One remote car on this phone.
///
/// It is drawn a little in the past: just far enough back (the 90th percentile of how late its states arrive, at most
/// `DELAY_CAP`) that the state the sender transmitted for a moment has usually arrived before that moment is drawn.
/// The switch to each new state then only has to absorb the sender's own drift threshold (15 cm), not the distance the
/// car covered while the packet was travelling: corrections become tiny, and a slow or bursty link (the broker relay)
/// stays smooth. When a state is late anyway the prediction carries on, as before.
#[derive(Clone, Debug, Default)]
pub struct Track {
    /// newest state received (its `t` is on the host clock)
    pub latest: Option<CarSnap>,
    /// states not drawn yet, oldest first, and how late recent ones arrived (s)
    queue: std::collections::VecDeque<CarSnap>, late: std::collections::VecDeque<f32>,
    /// how far in the past the car is drawn now (s; follows the lateness slowly), and the last sample's time
    pub delay: f32, last_t: f64,
    /// most delay allowed (0: DELAY_CAP; negative: draw at the present, the old behaviour)
    pub cap: f32,
    /// how close to our car (0: far, 1: close): close cars are drawn at the present (contacts need the true position
    /// more than smoothness). It eases in over a 25 m band, so the switch never visibly speeds the car up.
    pub near: f32,
    /// bumps this phone showed on the car (when, push), not yet in the owner's states; whether the owner's states have
    /// taken the current contact in (then only its phone moves the car); when we last pushed it
    pend: Vec<(f64, [f32; 5])>, acked: bool, last_nudge: f64,
    /// the owner's contact counter in the newest state, and the time of the first state where it last moved on
    ep_last: Option<u8>, ep_change_t: f64,
    /// the moment the car was last drawn at (host clock, delay included)
    shown_t: f64,
    /// the netcode before contact counters (net_contact compares): bumps only on states already here, a hard 25 m switch
    pub legacy: bool,
    /// the state the current offsets are measured from
    base: Option<CarSnap>,
    /// offsets, their rates and accelerations when the last state was taken in, when that was (view clock), how fast they fade
    off: [f32; NCH], offv: [f32; NCH], offa: [f32; NCH], anchor_t: f64, w: f32,
    /// fade speed multiplier (0 means 1; the simulator tunes with it)
    pub stiff: f32,
    /// physics predictions of the states in use (the one drawn from, and the newest while it takes over), and whether
    /// the car was last guessed with the physics
    traj: [Traj; 2], phys_on: bool,
}

/// fade speed (1/s) per channel for small corrections; bigger ones fade faster (see `sample`)
const OMEGA: [f32; NCH] = [13.0, 13.0, 14.0, 16.0, 16.0, 16.0, 18.0];
/// offset (m) at which the fade speed doubles: a car far off its line gets back quicker, small drifts melt away gently
const ADAPT: f32 = 0.3;

/// longest the car is drawn in the past (s): further back, contacts with it would be too far off
pub const DELAY_CAP: f32 = 0.22;
/// lateness (s) left to the prediction before the car is drawn in the past
pub const FREE: f32 = 0.06;

impl Track {
    pub fn push(&mut self, c: CarSnap) { self.push_at(c, c.t); }
    /// a state that arrived at host time `arrival`
    pub fn push_at(&mut self, mut c: CarSnap, arrival: f64) {
        // a bump we showed: once the owner's contact counter moves on, its states carry the bump (their phone settled it
        // or took our push); until then every new state gets it too, so the car doesn't slide back and then get shoved again
        let e = epoch(c.flags);
        if self.ep_last.map(|p| p != e).unwrap_or(false) { self.ep_change_t = c.t;
            // their phone took up a contact around here: the pushes we showed up to just after it are in this state
            // (their own answer); for the rest of this contact their phone moves the car
            if !self.legacy { self.pend.retain(|p| p.0 > c.t + 0.1); if self.shown_t - self.last_nudge <= EPISODE_GAP { self.acked = true; } } }
        self.ep_last = Some(e);
        self.pend.retain(|p| c.t - p.0 < PEND_LIFE);
        for (t, d) in &self.pend {
            // no answer for a while (a lost message, an older build): the shown push fades out over a few states
            // rather than vanishing from one state to the next
            let k = (((PEND_LIFE - (c.t - *t)) / 0.4).min(1.0)) as f32; let age = soft_age((*t - c.t) as f32);
            c.vx += d[0] * k; c.vz += d[1] * k; c.r += d[2] * k; c.x += (d[3] - d[0] * age) * k; c.z += (d[4] - d[1] * age) * k; c.h -= d[2] * age * k;
            c.ctl = None;   // a state carrying a push of ours is predicted from its motion (see `nudge`)
        }
        self.latest = Some(c);
        let k = self.queue.iter().position(|q| q.t > c.t).unwrap_or(self.queue.len()); self.queue.insert(k, c);
        while self.queue.len() > 24 { self.queue.pop_front(); }
        self.late.push_back(((arrival - c.t) as f32).clamp(0.0, 2.0)); while self.late.len() > 40 { self.late.pop_front(); }
    }
    /// how far ahead of its newest state the car is drawn (s): how much of what is shown is prediction
    pub fn horizon(&self) -> f32 { self.latest.map(|c| (self.shown_t - c.t).max(0.0) as f32).unwrap_or(0.0) }
    /// how bursty its arrivals are: the 95th minus the median lateness (s; a TCP stall shows as a long tail)
    pub fn spread(&self) -> f32 {
        if self.late.len() < 12 { return 0.0; } let mut v: Vec<f32> = self.late.iter().copied().collect(); v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[((v.len() - 1) as f32 * 0.95) as usize] - v[(v.len() - 1) / 2] }
    /// lateness the prediction covers on its own (a fast link is drawn at the present: contacts stay exact)
    fn free(&self) -> f32 { static F: std::sync::OnceLock<f32> = std::sync::OnceLock::new(); *F.get_or_init(|| std::env::var("FW_FREE").ok().and_then(|v| v.parse().ok()).unwrap_or(FREE)) }
    /// the delay the lateness asks for: its 90th percentile plus a frame
    fn want_delay(&self) -> f32 {
        let cap = if self.cap > 0.0 { self.cap } else if self.cap < 0.0 { return 0.0 } else { DELAY_CAP };
        if self.late.len() < 4 { return 0.0; }
        let mut v: Vec<f32> = self.late.iter().copied().collect(); v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        (v[((v.len() - 1) as f32 * 0.9) as usize] + 0.017 - self.free()).clamp(0.0, cap)
    }

    /// A bump this phone resolved, at the moment the car was last drawn: the push shows at once (like it does on an AI
    /// car), velocity included, and stays on the states still to come until the owner's own states include the contact.
    /// Once they do, the owner's phone moves the car for the rest of that contact: false, nothing was applied.
    pub fn nudge(&mut self, d: [f32; 5]) -> bool {
        let now = self.shown_t;
        // a new contact: if their phone has just taken one up itself, it is already answering this one
        if now - self.last_nudge > EPISODE_GAP { self.pend.clear(); self.acked = self.ep_change_t > 0.0 && self.ep_change_t >= now - EPISODE_GAP; }
        self.last_nudge = now;
        if self.acked && !self.legacy { return false; }
        for c in [self.latest.as_mut(), self.base.as_mut()].into_iter().flatten().chain(self.queue.iter_mut()) {
            let age = soft_age((now - c.t) as f32);
            c.vx += d[0]; c.vz += d[1]; c.r += d[2]; c.x += d[3] - d[0] * age; c.z += d[4] - d[1] * age; c.h -= d[2] * age;
            // ... and from its motion, not the physics: the push travels as a velocity carried on from the moment it was
            // shown, which the physics would brake and steer away (the car drawn short of where the contact put it, then
            // pushed again every frame: net_contact found three times the phantom bumps)
            c.ctl = None;
        }
        if self.pend.len() < 64 && !self.legacy { self.pend.push((now, d)); }
        true
    }
    /// in a contact the owner's phone has already taken in: this phone must not move the car (it would twitch back next frame)
    pub fn owned(&self) -> bool { !self.legacy && self.acked && self.shown_t - self.last_nudge <= EPISODE_GAP }

    /// The car as shown at host time `t` (call once per frame, with a smoothly advancing `t`).
    pub fn sample(&mut self, t: f64, g: Ground, ph: Option<&Phys>) -> Option<CarSnap> {
        self.latest?;
        // around a contact (one we showed, or one its own phone took up) the car is guessed from its motion, as before the
        // physics: a contact's pushes travel as velocities carried on, which the contact handling is tuned for (the
        // physics turns them into a car that steers back into ours: net_contact found 2-4x the phantom bumps)
        // around a contact the push travels as a velocity the physics would steer away from, so draw it kinematic for a moment
        let ph_all = ph; let ph = if self.shown_t - self.last_nudge.max(self.ep_change_t) < CONTACT_CALM { None } else { ph };
        let ph_was = if self.phys_on { ph_all } else { None };
        // the delay eases toward what the link needs: at most 6% faster or 4% slower than real time, so the car never
        // visibly speeds up or brakes because of it
        let dtv = if self.last_t > 0.0 { (t - self.last_t).clamp(0.0, 0.1) as f32 } else { 0.0 }; self.last_t = t;
        // (closing in, it may catch up a little quicker: at most 10%)
        let want = self.want_delay() * (1.0 - self.near.clamp(0.0, 1.0)); let down = if self.near <= 0.0 { 0.04 } else if self.legacy { 0.25 } else { 0.1 };
        self.delay += (want - self.delay).clamp(-down * dtv, 0.06 * dtv);
        let t = t - self.delay as f64; self.shown_t = t;
        // the newest state already due at that moment (the next ones wait for their turn)
        let k = self.queue.iter().rposition(|q| q.t <= t + 1e-4).unwrap_or(0);
        for _ in 0..k { self.queue.pop_front(); }
        let latest = *self.queue.front()?;
        let ks = if self.stiff > 0.0 { self.stiff } else { 1.0 }; let tau = (t - self.anchor_t).max(0.0) as f32;
        // the offsets as they stand now
        let (mut off, mut offv, mut offa) = ([0f32; NCH], [0f32; NCH], [0f32; NCH]);
        for k in 0..NCH { let (x, v, a) = fade3(self.off[k], self.offv[k], self.offa[k], OMEGA[k] * self.w * ks, tau); off[k] = x; offv[k] = v; offa[k] = a; }
        // a prediction's channels with their rates and accelerations at time t
        const E: f32 = 1.0 / 120.0;
        let tr = &mut self.traj;
        let mut at = |c: &CarSnap, t: f64, ph: Option<&Phys>| -> ([f32; NCH], [f32; NCH], [f32; NCH]) {
            let age = (t - c.t) as f32; let m = channels(&guess(tr, c, age - E, g, ph), g); let p = channels(&guess(tr, c, age, g, ph), g); let n = channels(&guess(tr, c, age + E, g, ph), g);
            let (mut v, mut a) = ([0f32; NCH], [0f32; NCH]);
            for k in 0..NCH { let (d1, d0) = if k == 2 { (wrap(n[k] - p[k]), wrap(p[k] - m[k])) } else { (n[k] - p[k], p[k] - m[k]) }; v[k] = (d1 + d0) / (2.0 * E); a[k] = (d1 - d0) / (E * E); }
            (p, v, a) };
        match self.base {
            Some(b) if b != latest || self.phys_on != ph.is_some() => {
                // re-anchor: keep showing the same pose moving the same way, and let the offset fade it onto the new line
                // (the same when the guess switches between physics and motion: no jump either way)
                let (p0, v0, a0) = at(&b, t, ph_was); let (p1, v1, a1) = at(&latest, t, ph);
                for k in 0..NCH {
                    let d = p0[k] + off[k] - p1[k]; off[k] = if k == 2 { wrap(d) } else { d };
                    offv[k] = v0[k] + offv[k] - v1[k]; offa[k] = a0[k] + offa[k] - a1[k];
                }
                // a real knock (wall, tree, another car) changes the velocity at once: show it at once instead of easing into it
                if offv[0].hypot(offv[1]) > IMPACT_DV { for k in 0..3 { offv[k] = 0.0; offa[k] = 0.0; } }
                if off[0].hypot(off[1]) > SNAP_DIST || off[2].abs() > 1.2 { off = [0.0; NCH]; offv = [0.0; NCH]; offa = [0.0; NCH]; }
                self.w = 1.0 + off[0].hypot(off[1]) / ADAPT;
                self.base = Some(latest); self.off = off; self.offv = offv; self.offa = offa; self.anchor_t = t;
            }
            None => { self.base = Some(latest); self.off = [0.0; NCH]; self.offv = [0.0; NCH]; self.offa = [0.0; NCH]; self.anchor_t = t; self.w = 1.0; }
            _ => {}
        }
        self.phys_on = ph.is_some();
        let c = self.base?; let mut s = guess(&mut self.traj, &c, (t - c.t) as f32, g, ph);
        let p = channels(&s, g);
        s.x = p[0] + off[0]; s.z = p[1] + off[1]; s.h = p[2] + off[2]; s.steer = p[6] + off[6];
        let (gy, gp, gr) = match g { Some(g) => { let (pp, rr) = slope(g, s.x, s.z, s.h); (g(s.x, s.z), pp, rr) } None => (0.0, 0.0, 0.0) };
        s.y = gy + p[3] + off[3]; s.pitch = gp + p[4] + off[4]; s.roll = gr + p[5] + off[5];
        // the shown velocity includes the correction, so wheels, sound and contacts see the motion that is drawn
        s.vx += offv[0]; s.vz += offv[1]; s.r += offv[2];
        Some(s)
    }
}

/// The clock other cars are drawn at. It advances by exactly each frame's length (vsync to vsync), so they move in
/// step with our own car and the camera, and follows the host clock through a slow loop: reading the wall clock at a
/// varying point of each frame would shake a car at 30 m/s by several centimetres.
#[derive(Clone, Debug, Default)]
pub struct ViewClock { t: Option<f64>, err: f64 }
impl ViewClock {
    /// `now`: the host clock read this frame, `dt`: the frame's length
    pub fn advance(&mut self, now: f64, dt: f64) -> f64 {
        let t = match self.t {
            Some(t) if (now - (t + dt)).abs() < 0.25 => {
                // low-passed offset (~0.3 s), corrected at 1/s: the rate wobbles by well under a tenth of a percent
                let t = t + dt; self.err += (now - t - self.err) * (dt * 3.0).min(1.0); t + self.err * (dt * 1.0).min(1.0)
            }
            _ => { self.err = 0.0; now }
        };
        self.t = Some(t); t
    }
    pub fn get(&self) -> Option<f64> { self.t }
}

/// Sender side: decides when our car's state goes out.
#[derive(Clone, Debug, Default)]
pub struct Policy {
    /// exactly what receivers decoded from our last packet (both sides predict from these numbers)
    pub sent: Option<CarSnap>,
    tokens: f32, last_t: f64, u_prev: Option<f32>, accel: f32,
    /// the physics prediction of `sent` (the same one the receivers run)
    traj: [Traj; 1],
}

/// thresholds: position m, heading rad, speed m/s, height m, body angle rad
#[derive(Clone, Copy, Debug)]
pub struct Thresholds { pub pos: f32, pub head: f32, pub speed: f32, pub height: f32, pub body: f32 }
impl Default for Thresholds { fn default() -> Self { Thresholds { pos: 0.15, head: 0.026, speed: 0.8, height: 0.12, body: 0.06 } } }

/// How precisely and how often a car goes out, by how far (m) the nearest other car is. Close by (contacts, side by
/// side, a car being chased) the drift allowed shrinks and the rate goes up; out of everyone's sight it relaxes, and
/// what that saves is what the pack spends. Receivers draw a far car in the past and smooth it anyway: they can't see
/// the difference, but the room's data goes where the eyes are. (base thresholds, rate, heartbeat) -> the same, adjusted
pub fn relevance(t: Thresholds, rate: f32, beat: f32, dist: f32) -> (Thresholds, f32, f32) {
    if dist < 30.0 {
        // within 30 m: tighter, tightest and quickest at contact range (12 m)
        let k = ((30.0 - dist) / 18.0).clamp(0.0, 1.0); let m = |a: f32, b: f32| a + (b - a) * k;
        (Thresholds { pos: t.pos * m(0.6, 0.35), head: t.head * m(0.7, 0.45), speed: t.speed * m(0.7, 0.45), height: t.height, body: t.body }, (rate * m(1.25, 1.6)).min(30.0), beat.min(m(0.4, 0.2)))
    } else if dist > 120.0 {
        let k = ((dist - 120.0) / 180.0).clamp(0.0, 1.0); let m = 1.0 + k;
        (Thresholds { pos: t.pos * m, head: t.head * m, speed: t.speed * m, height: t.height * m, body: t.body * m }, rate * (1.0 - 0.45 * k), (beat * (1.0 + 0.6 * k)).min(1.1))
    } else { (t, rate, beat) }
}

impl Policy {
    /// forward acceleration from the speed history, smoothed (~80 ms) so one bumpy frame does not swing the prediction
    pub fn accel(&mut self, u: f32, dt: f32) -> f32 {
        if dt > 0.0 { if let Some(p) = self.u_prev { let raw = ((u - p) / dt).clamp(-30.0, 30.0); self.accel += (raw - self.accel) * (dt * 12.0).min(1.0); } }
        self.u_prev = Some(u); self.accel
    }
    /// Whether `c` (stamped `c.t` on the host clock) should be sent now: it has drifted from what receivers predict, or
    /// `heartbeat` seconds passed. Never more than `rate` packets a second on average (bursts of up to a quarter second).
    pub fn due(&mut self, c: &CarSnap, g: Ground, ph: Option<&Phys>, thr: &Thresholds, rate: f32, heartbeat: f32) -> bool {
        let dt = if self.last_t > 0.0 { (c.t - self.last_t).clamp(0.0, 0.5) as f32 } else { 0.0 }; self.last_t = c.t;
        let cap = (rate * 0.4).max(2.0); self.tokens = (self.tokens + dt * rate).min(cap);
        let Some(s) = &self.sent else { return self.take(); };
        let age = (c.t - s.t) as f32; if age < 1.0 / 31.0 || self.tokens < 1.0 { return false; }
        if age >= heartbeat || c.gear != s.gear || c.flags != s.flags || c.ctl.is_some() != s.ctl.is_some() { return self.take(); }
        // the driver changed their mind: the receivers' prediction runs on the old inputs, tell them now
        if let (Some(a), Some(b)) = (c.ctl, s.ctl) { if (a.steer - b.steer).abs() > 0.2 || (a.throttle - b.throttle).abs() > 0.35 || (a.brake - b.brake).abs() > 0.3 || a.hb != b.hb { return self.take(); } }
        let s = *s; let p = guess(&mut self.traj, &s, age, g, ph);
        let de = (c.x - p.x).hypot(c.z - p.z); let dh = wrap(c.h - p.h).abs();
        let dy = (c.y - p.y).abs(); let db = (c.pitch - p.pitch).abs().max((c.roll - p.roll).abs());
        // one score so several medium errors together also count
        let e = (de / thr.pos).max(dh / thr.head).max((c.u - p.u).abs() / thr.speed).max(dy / thr.height).max(db / thr.body);
        if e >= 1.0 { self.take() } else { false }
    }
    fn take(&mut self) -> bool { self.tokens = (self.tokens - 1.0).max(0.0); true }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prediction_follows_arc() {
        // 20 m/s with yaw rate 0.5 rad/s: a quarter turn later the car is on the circle of radius 40 around (40, 0)
        let c = CarSnap { h: 0.0, vz: 20.0, r: 0.5, ..Default::default() };
        let p = predict(&c, 0.4, None); assert!(((p.x - 40.0).hypot(p.z) - 40.0).abs() < 0.01, "{} {}", p.x, p.z); assert!((p.h - 0.2).abs() < 1e-4);
        // braking at 8 m/s2 from 10 m/s stops after 1.25 m... within the full-prediction window it covers u*t - a t^2/2
        let b = CarSnap { vz: 10.0, a: -8.0, ..Default::default() }; let q = predict(&b, 0.5, None);
        assert!((q.z - (5.0 - 1.0)).abs() < 1e-3, "{}", q.z); assert!((q.u - 6.0).abs() < 1e-4);
        let r = predict(&CarSnap { vz: 2.0, a: -8.0, ..Default::default() }, 0.5, None); assert!(r.u.abs() < 1e-6 && (r.z - 0.25).abs() < 1e-3, "{} {}", r.u, r.z);
    }
    #[test]
    fn terrain_and_air() {
        let ramp = |_x: f32, z: f32| z * 0.1;   // 10% climb along +z
        let g: &dyn Fn(f32, f32) -> f32 = &ramp;
        let c = CarSnap { vz: 20.0, y: REST, pitch: (0.1f32).atan(), ..Default::default() };
        let p = predict(&c, 0.5, Some(g)); assert!((p.y - (1.0 + REST)).abs() < 1e-3, "{}", p.y); assert!((p.pitch - c.pitch).abs() < 1e-4);
        let j = CarSnap { vz: 20.0, y: 2.0, vy: 3.0, flags: AIR, ..Default::default() }; let q = predict(&j, 0.3, Some(g));
        assert!((q.y - (2.0 + 0.9 - 0.5 * G * 0.09)).abs() < 1e-3);
    }
    /// a correction never makes the shown car jump: position moves smoothly from the old line to the new one
    #[test]
    fn corrections_are_continuous() {
        let mut tr = Track::default(); let dt = 1.0 / 60.0;
        tr.push(CarSnap { t: 0.0, vz: 20.0, ..Default::default() });
        let mut prev = tr.sample(0.0, None, None).unwrap(); let mut prev_v = 20.0f32; let mut worst_jump = 0.0f32; let mut worst_acc = 0.0f32;
        for f in 1..120 {
            let t = f as f64 * dt;
            // at 0.5 s the real car turns out to be 0.6 m to the side and slightly slower
            // (and from then on reports arrive every quarter second, 50 ms old, along that line)
            if f >= 30 && f % 15 == 0 { let ts = t - 0.05; tr.push(CarSnap { t: ts, x: 0.6, z: 9.0 + 19.0 * (ts - 0.45) as f32, vz: 19.0, ..Default::default() }); }
            let s = tr.sample(t, None, None).unwrap(); let step = (s.x - prev.x).hypot(s.z - prev.z); let v = step / dt as f32;
            worst_jump = worst_jump.max((step - prev_v * dt as f32).abs()); worst_acc = worst_acc.max(((v - prev_v) / dt as f32).abs()); prev = s; prev_v = v;
        }
        assert!(worst_jump < 0.05, "jump {worst_jump}"); assert!(worst_acc < 25.0, "accel {worst_acc}");
        assert!((prev.x - 0.6).abs() < 0.02, "settled at {}", prev.x);
    }
    /// a bump shown here stays on the car while the owner's states don't include it yet (no sliding back, no second
    /// shove), and the owner's states take over once its contact counter moves on
    #[test]
    fn bumps_hold_until_the_owner_answers() {
        let mut tr = Track::default(); tr.near = 1.0; let dt = 1.0 / 60.0; let mut last_tx = -1.0; let mut ep = 0u8; let mut pushed_at = None;
        let mut worst_after = 0.0f32;
        for f in 0..150 {
            let t = f as f64 * dt;
            // the owner answers 0.15 s after the contact: from then on its states carry a 2 m/s sideways push
            let answered = pushed_at.map(|p: f64| t - 0.05 >= p + 0.15).unwrap_or(false); if answered { ep = 1; }
            if t - last_tx >= 0.1 { last_tx = t; let ts = t - 0.05; let side = pushed_at.filter(|_| answered).map(|p| -2.0 * (ts - p) as f32).unwrap_or(0.0);
                tr.push_at(CarSnap { t: ts, x: side, z: 20.0 * ts as f32, vz: 20.0, vx: if answered { -2.0 } else { 0.0 }, flags: with_epoch(0, ep), ..Default::default() }, t); }
            let s = tr.sample(t, None, None).unwrap();
            if f == 60 { assert!(tr.nudge([-2.0, 0.0, 0.0, 0.0, 0.0])); pushed_at = Some(t); }
            if let Some(p) = pushed_at { if t > p + 0.05 { let want = -2.0 * (t - p) as f32; worst_after = worst_after.max((s.x - want).abs()); assert!(s.vx < -1.0, "slid back at {t:.3}: vx {}", s.vx); } }
            if f == 110 { assert!(tr.owned() || t - tr.last_nudge > EPISODE_GAP); }
        }
        assert!(worst_after < 0.12, "off the pushed line by {worst_after}");
    }
    /// their phone already took up the contact: this phone doesn't push their car at all
    #[test]
    fn owner_first_means_hands_off() {
        let mut tr = Track::default(); tr.near = 1.0;
        tr.push_at(CarSnap { t: 0.9, z: 18.0, vz: 20.0, flags: with_epoch(0, 3), ..Default::default() }, 0.95);
        tr.push_at(CarSnap { t: 0.95, z: 19.0, vz: 20.0, flags: with_epoch(0, 4), ..Default::default() }, 1.0);
        tr.sample(1.0, None, None); assert!(!tr.nudge([-2.0, 0.0, 0.0, 0.0, 0.0])); assert!(tr.owned());
        assert!(tr.sample(1.02, None, None).unwrap().vx.abs() < 1e-6);
    }

}
