//! drawHUD, ported: menu, stage panel, standings and tags, speedo, touch controls, start lights, messages, results.
use super::hud::*;
use super::race::{self, Phase, RIVALS};
use super::{fmt, js_round, Game, Rect, State};
use crate::noise::clamp;

fn rgb_of(c: [f64; 3]) -> Col { let q = |v: f64| js_round(v.min(1.0) * 230.0) as u8; rgba(q(c[0]), q(c[1]), q(c[2]), 1.0) }

pub fn hud(gm: &mut Game) {
    let now = gm.clock_ms();
    let (lw, lh, u) = (gm.lw as f64, gm.lh as f64, gm.u);
    let ins = gm.inset; // t r b l
    let (pl, pr, pt, pb) = (js_round(ins[3] + 5.0 * u), js_round(ins[1] + 5.0 * u), js_round(ins[0] + 4.0 * u), js_round(ins[2] + 5.0 * u));
    let portrait = gm.h > gm.w;
    let inp = if gm.state == State::Drive { gm.read_input() } else { crate::physics::Input::default() };
    let mut ui: Vec<(&'static str, Rect)> = Vec::new();
    let km_total = gm.stage_km();
    let res = if gm.race.on && gm.race.phase == Phase::Done && gm.race.done_t > 1.2 { race::results(gm) } else { Vec::new() };
    for k in super::online::KEYS { gm.ui.remove(k); } for k in super::menu::KEYS { gm.ui.remove(k); } gm.ui.remove("garage");
    if gm.ps.online_ui != super::online::OnlineUi::Closed || gm.gar.open { for k in ["mapF", "mapS", "mapC", "mapL", "mapT", "mapR", "mapP", "mapE", "mode"] { gm.ui.remove(k); } }
    let g = &mut gm.hud; g.clear();
    let (bw, bh, gap) = (22.0 * u, 13.0 * u, 3.0 * u); let mut bx = lw - pr - bw;
    let mut btn = |g: &mut Canvas, ui: &mut Vec<(&'static str, Rect)>, k: &'static str, label: &str, on: bool| {
        let w = bw.max(text_w(label, u) + 6.0 * u); let x = bx + bw - w; ui.push((k, Rect { x, y: pt, w, h: bh }));
        g.panel(x, pt, w, bh, u, if on { rgba(243, 207, 98, 0.28) } else { PANEL }, Some(if on { AMBER } else { EDGE }));
        g.tx(label, x + (w - text_w(label, u)) / 2.0, pt + 3.0 * u, u, if on { AMBER } else { INK }); bx = x - gap - bw; };
    let drive = gm.state == State::Drive;
    // settings live on the stage-select screen only, never mid-stage
    if !drive && !gm.gar.open && gm.state != State::Title { btn(g, &mut ui, "set", "SET", gm.set.open); }
    if drive { btn(g, &mut ui, "cam", "CAM", false); btn(g, &mut ui, "rst", "RST", false); btn(g, &mut ui, "menu", "MENU", false); }
    for k in ["set", "cam", "rst", "menu", "rec", "tel"] { gm.ui.remove(k); }
    if gm.ps.debug && drive { let rec = gm.bench_secs; let l = match rec { Some(s) => format!("REC {}", s), None => "REC".to_string() }; btn(g, &mut ui, "rec", &l, rec.is_some()); }   // temporary benchmark button
    if (gm.ps.debug || gm.tel.is_some()) && drive { let l = match &gm.tel { Some(t) => format!("TEL {}:{:02}", (t.t / 60.0) as u32, (t.t as u32) % 60), None => "TEL".to_string() }; btn(g, &mut ui, "tel", &l, gm.tel.is_some()); }
    if gm.state == State::Title && gm.gar.open && !portrait {
        super::garage::draw(&mut gm.gar, &gm.ps, gm.time, g, lw, lh, u, pl, pr, pt, pb);
    } else if gm.state == State::Title && gm.ps.online_ui != super::online::OnlineUi::Closed && !portrait {
        g.rect_c(0.0, 0.0, lw, lh, rgba(6, 9, 6, 0.42));
        super::online::draw(&gm.ps, &gm.rt.map, g, &mut ui, lw, lh, u, pt + 16.0 * u);
    } else if gm.state == State::Title {
        if !portrait {
            let mut m = std::mem::take(g); super::menu::draw(gm, &mut m, &mut ui, lw, lh, u, pl, pt, pb);
            if gm.loading.is_some() { super::menu::loading(gm, &mut m, lw, lh, u, pl, pb); }
            gm.hud = m;
        }
    }
    let g = &mut gm.hud;
    if portrait {
        g.rect_c(0.0, 0.0, lw, lh, rgba(6, 9, 6, 0.78));
        let (cx, cy) = (lw / 2.0, lh * 0.42); let a = (now / 900.0) % 2.0 >= 1.0; let (w, h) = (if a { 30.0 } else { 16.0 } * u, if a { 16.0 } else { 30.0 } * u);
        g.panel(cx - w / 2.0, cy - h / 2.0, w, h, u, rgba(233, 236, 226, 0.12), Some(INK));
        g.tx("TURN YOUR PHONE", cx - text_w("TURN YOUR PHONE", u) / 2.0, cy + 22.0 * u, u, INK); g.tx("SIDEWAYS TO DRIVE", cx - text_w("SIDEWAYS TO DRIVE", u) / 2.0, cy + 31.0 * u, u, AMBER);
        finish(gm, ui); return;
    }
    if !drive { finish(gm, ui); return; }
    // stage panel
    let km = format!("{:.2}", gm.last_pidx as f64 / 1000.0);
    let race_on = gm.race.on;
    let lines: Vec<(String, String)> = if race_on { let p = &gm.race.cars[gm.race.player];
        vec![("POS".into(), format!("{}/{}", gm.race.pos, gm.race.cars.len())), ("LAP".into(), format!("{}/{}", gm.race.laps.min((p.laps_done + 1).max(1)), gm.race.laps)),
            ("TIME".into(), fmt(Some(if gm.race.phase == Phase::Grid { 0.0 } else if p.finished { p.finish_t } else { gm.race.t }))), ("BEST".into(), fmt(p.best))] }
        else { vec![("RUN".into(), gm.lap.to_string()), ("TIME".into(), fmt(Some(if gm.started { gm.g_t - gm.lap_start } else { 0.0 }))), ("BEST".into(), fmt(gm.best)), ("KM".into(), format!("{}/{}", km, km_total))] };
    let (pw, ph) = (78.0 * u, (lines.len() as f64 * 9.0 + 5.0) * u); g.panel(pl, pt, pw, ph, u, PANEL, Some(EDGE));
    for (n, (k, v)) in lines.iter().enumerate() { let y = pt + (4.0 + n as f64 * 9.0) * u; g.tx(k, pl + 4.0 * u, y, u, rgba(233, 236, 226, 0.65));
        g.tx(v, pl + pw - 4.0 * u - text_w(v, u), y, u, if race_on { if n == 0 { AMBER } else { INK } } else if n == 1 { AMBER } else { INK }); }
    let css = |r: Option<usize>| match r { None => hex(0xece8dc), Some(i) => rgb_of(RIVALS[i].col) };
    if race_on {
        let mut y = pt + ph + 3.0 * u; let rw = 66.0 * u; let pp = gm.race.cars[gm.race.player].prog; let pv = gm.race.cars[gm.race.player].v;
        let order: Vec<usize> = if gm.race.order.is_empty() { (0..gm.race.cars.len()).collect() } else { gm.race.order.clone() };
        for (n, &k) in order.iter().enumerate() { let c = &gm.race.cars[k]; let me = c.is_player();
            g.panel(pl, y, rw, 9.0 * u, u, if me { rgba(243, 207, 98, 0.3) } else { rgba(8, 12, 8, 0.5) }, if me { Some(AMBER) } else { None });
            g.rect_c(pl + 3.0 * u, y + 2.0 * u, 3.0 * u, 5.0 * u, css(c.r));
            g.tx(&format!("{} {}", n + 1, if me { "YOU".to_string() } else { c.tag() }), pl + 8.0 * u, y + u, u, if me { AMBER } else if c.left && !c.finished { rgba(233, 236, 226, 0.45) } else { INK });
            if c.left && !c.finished { let lab = "OUT"; g.text(lab, pl + rw - 3.0 * u - text_w(lab, u), y + u, u, rgba(233, 236, 226, 0.6), None); }
            else if !me && gm.race.phase != Phase::Grid { let gs = (c.prog - pp) / ((c.v + pv) / 2.0).max(12.0);
                if gs.abs() < 99.0 { let lab = format!("{}{:.1}", if gs > 0.0 { "-" } else { "+" }, gs.abs()); g.text(&lab, pl + rw - 3.0 * u - text_w(&lab, u), y + u, u, rgba(233, 236, 226, 0.7), None); } }
            y += 10.0 * u; }
        for c in &gm.race.cars { if c.is_player() || !c.tag_on { continue; } let tag = c.tag(); let tag = tag.as_str(); let w = text_w(tag, u) + 4.0 * u; let x = js_round(c.tag_x - w / 2.0); let ty = js_round(c.tag_y - 9.0 * u);
            g.panel(x, ty, w, 9.0 * u, u, rgba(8, 12, 8, 0.55), None); g.rect_c(x, ty + 8.0 * u, w, u, css(c.r)); g.text(tag, x + 2.0 * u, ty + u, u, INK, None); }
    }
    // speedo
    let r = 26.0 * u; let (cx, cy) = (js_round(lw / 2.0), js_round(lh - pb - 3.0 * u)); let rpm_s = gm.rpm_shown;
    { let lit = ((rpm_s / 125.0).floor()).clamp(-1.0, 64.0); let (ox, oy) = (r + 4.0 * u, r + 5.0 * u); let (x0, y0) = (cx - ox, cy - oy); let (xx, yy0) = (x0 + ox, y0 + oy);
        let mut yy = 0.0; while yy <= r + 3.0 * u { let half = ((r + 3.0 * u).powi(2) - yy * yy).max(0.0).sqrt(); g.rect_c(js_round(xx - half), yy0 - yy - u, js_round(half * 2.0), u, PANEL); yy += u; }
        let mut rpm = 0.0; while rpm <= 8000.0 { let a = std::f64::consts::PI - rpm / 8000.0 * std::f64::consts::PI; let red = rpm >= 7000.0;
            let r0 = if rpm % 1000.0 == 0.0 { r - 4.0 * u } else { r - u }; let c = if red { RED } else if rpm / 125.0 <= lit { AMBER } else { rgba(233, 236, 226, 0.5) };
            let mut rr = r0; while rr <= r { g.rect_c(js_round(xx + a.cos() * rr), js_round(yy0 - a.sin() * rr), u, u, c); rr += u; } rpm += 125.0; } }
    let na = std::f64::consts::PI - clamp(rpm_s, 0.0, 8000.0) / 8000.0 * std::f64::consts::PI; g.px_line(cx, cy - u, cx + na.cos() * (r - 5.0 * u), cy - u - na.sin() * (r - 5.0 * u), RED, u, u);
    let gear = if gm.car.gear < 0.0 { "R".to_string() } else { format!("{}", gm.car.gear as i64) }; g.tx(&gear, cx - text_w(&gear, 2.0 * u) / 2.0, cy - 23.0 * u, 2.0 * u, INK);
    let kmh = format!("{:03}", js_round(gm.car.u.abs() * if gm.ps.mph { 2.23694 } else { 3.6 }) as i64); g.tx(&kmh, cx - text_w(&kmh, u) / 2.0, cy - 8.0 * u, u, AMBER);
    g.tx(if gm.ps.mph { "MPH" } else { "KM/H" }, cx + r + 6.0 * u, cy - 8.0 * u, u, rgba(233, 236, 226, 0.6));
    if gm.ps.touch_used {
        let (sw, shh) = (38.0 * u, 32.0 * u); let sy = lh - pb - shh;
        let left = Rect { x: pl, y: sy, w: sw, h: shh }; let right = Rect { x: pl + sw + 4.0 * u, y: sy, w: sw, h: shh }; ui.push(("left", left)); ui.push(("right", right));
        for (b, d) in [(left, -1.0), (right, 1.0)] { let on = inp.steer == -d;
            g.panel(b.x, b.y, b.w, b.h, u, if on { rgba(243, 207, 98, 0.3) } else { PANEL }, Some(if on { AMBER } else { EDGE })); g.arrow(b.x + b.w / 2.0, b.y + b.h / 2.0, d, 10.0 * u, u, if on { AMBER } else { INK }); }
        let (gw, gh) = (28.0 * u, 46.0 * u); let (gx, gy) = (lw - pr - gw, lh - pb - gh); let (brw, brh) = (46.0 * u, 28.0 * u); let (brx, bry) = (gx - brw - 5.0 * u, lh - pb - brh);
        let gas = Rect { x: gx, y: gy, w: gw, h: gh }; let brake = Rect { x: brx, y: bry, w: brw, h: brh }; let hb = Rect { x: brx, y: bry - brh - 4.0 * u, w: brw, h: brh };
        ui.push(("gas", gas)); ui.push(("brake", brake)); ui.push(("hb", hb));
        let pedal = |g: &mut Canvas, b: Rect, on: bool, label: &str| { let dy = if on { 2.0 * u } else { 0.0 };
            g.panel(b.x, b.y + dy, b.w, b.h - dy, u, if on { rgba(243, 207, 98, 0.3) } else { PANEL }, Some(if on { AMBER } else { EDGE }));
            let c = if on { rgba(243, 207, 98, 0.55) } else { rgba(233, 236, 226, 0.28) }; let mut yy = b.y + dy + 4.0 * u; while yy < b.y + b.h - 12.0 * u { g.rect_c(b.x + 4.0 * u, yy, b.w - 8.0 * u, 2.0 * u, c); yy += 4.0 * u; }
            g.tx(label, b.x + (b.w - text_w(label, u)) / 2.0, b.y + b.h - 10.0 * u, u, if on { AMBER } else { INK }); };
        pedal(g, gas, inp.throttle > 0.0, "GAS"); pedal(g, brake, inp.brake > 0.0, if gm.car.gear < 0.0 { "REV" } else { "BRAKE" });
        let hb_on = inp.hb; g.panel(hb.x, hb.y, hb.w, hb.h, u, if hb_on { rgba(216, 72, 58, 0.4) } else { PANEL }, Some(if hb_on { RED } else { EDGE }));
        let c = if hb_on { rgba(216, 72, 58, 0.55) } else { rgba(233, 236, 226, 0.28) }; let mut yy = hb.y + 4.0 * u; while yy < hb.y + hb.h - 12.0 * u { g.rect_c(hb.x + 4.0 * u, yy, hb.w - 8.0 * u, 2.0 * u, c); yy += 4.0 * u; }
        g.tx("H-BRAKE", hb.x + (hb.w - text_w("H-BRAKE", u)) / 2.0, hb.y + hb.h - 10.0 * u, u, if hb_on { RED } else { INK });
    }
    let (mut m, mut mc) = (String::new(), INK);
    if race_on && gm.go > 0.0 {
        let n = ((gm.go - 0.9) / 1.2).ceil() as i64; let lwd = 12.0 * u; let (gx, gy) = (js_round(lw / 2.0 - lwd * 2.0), js_round(lh * 0.16));
        g.panel(gx - 3.0 * u, gy - 3.0 * u, lwd * 4.0 + 6.0 * u, lwd + 6.0 * u, u, rgba(8, 8, 8, 0.8), Some(EDGE));
        for q in 0..4 { let on = if n <= 0 { q < 4 } else { q < 4 - n }; let c = if n <= 0 { hex(0x4fd862) } else if on { RED } else { rgba(80, 30, 26, 0.9) }; g.rect_c(gx + q as f64 * lwd + u, gy + u, lwd - 2.0 * u, lwd - 2.0 * u, c); }
        m = if n > 0 { n.to_string() } else { "GO!".into() }; mc = if n > 0 { INK } else { AMBER };
    } else if gm.go > 0.0 { m = if gm.go > 0.9 { "READY".into() } else { "GO!".into() }; mc = if gm.go > 0.9 { INK } else { AMBER }; }
    else if gm.wrong_t > 1.0 { m = if ((now / 300.0) as i64) % 2 == 1 { "WRONG WAY".into() } else { String::new() }; mc = RED; }
    else if gm.msg_t > 0.0 { m = gm.msg.clone(); mc = AMBER; }
    if !m.is_empty() { let sc = 2.0 * u; g.tx(&m, (lw - text_w(&m, sc)) / 2.0, lh * 0.3, sc, mc); }
    if race_on && gm.race.phase == Phase::Done && gm.race.done_t > 1.2 {
        let w0 = res[0].1; let rw = (lw - 16.0 * u).min(196.0 * u); let rh = (res.len() as f64 * 10.0 + 46.0) * u; let (rx, ry) = (js_round((lw - rw) / 2.0), js_round((lh - rh) / 2.0));
        g.panel(rx, ry, rw, rh, u, rgba(8, 12, 8, 0.86), Some(AMBER)); g.tx("RACE RESULT", rx + (rw - text_w("RACE RESULT", u)) / 2.0, ry + 4.0 * u, u, AMBER);
        g.text("BEST LAP", rx + rw - 6.0 * u - text_w("BEST LAP", u), ry + 14.0 * u, u, rgba(233, 236, 226, 0.55), None);
        for (n, (k, t, done)) in res.iter().enumerate() { let c = &gm.race.cars[*k]; let y = ry + (24.0 + n as f64 * 10.0) * u; let me = c.is_player();
            if me { g.rect_c(rx + 2.0 * u, y - u, rw - 4.0 * u, 9.0 * u, rgba(243, 207, 98, 0.18)); }
            g.rect_c(rx + 6.0 * u, y, 3.0 * u, 6.0 * u, css(c.r));
            let name = if me { "YOU".to_string() } else { c.full_name() };
            g.tx(&format!("{}  {}", n + 1, name), rx + 12.0 * u, y, u, if me { AMBER } else { INK });
            let tm = if c.left && !*done { "OUT".to_string() } else if n == 0 { fmt(Some(*t)) } else { format!("+{:.2}{}", t - w0, if *done { "" } else { "*" }) }; g.text(&tm, rx + rw - 62.0 * u - text_w(&tm, u), y, u, INK, None);
            let bl = fmt(c.best); g.text(&bl, rx + rw - 6.0 * u - text_w(&bl, u), y, u, rgba(233, 236, 226, 0.75), None); }
        if !gm.race.note.is_empty() { let nt = gm.race.note.clone(); g.text(&nt, rx + (rw - text_w(&nt, u)) / 2.0, ry + rh - 20.0 * u, u, rgba(233, 236, 226, 0.7), None); }
        let tc = "TAP TO CONTINUE"; if ((now / 500.0) as i64) % 2 == 0 { g.text(tc, rx + (rw - text_w(tc, u)) / 2.0, ry + rh - 10.0 * u, u, INK, None); }
    }
    finish(gm, ui);
}

/// hit areas for this frame, then the settings screen on top (on the menu too)
fn finish(gm: &mut Game, ui: Vec<(&'static str, Rect)>) {
    for (k, r) in ui { gm.ui.insert(k, r); }
    let (lw, lh, u) = (gm.lw as f64, gm.lh as f64, gm.u);
    let paused = gm.settings_paused(); let cam = gm.cam.mode as u8; let time = gm.time;
    super::settings::draw(&mut gm.set, &gm.ps, cam, paused, &mut gm.hud, lw, lh, u, time);
    { let mut h = std::mem::take(&mut gm.hud); super::menu::overlays(gm, &mut h, lw, lh, u); gm.hud = h; }
}
