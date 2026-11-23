// Fogwood relay on Cloudflare (Workers free plan): one Durable Object per online room passes each game packet to the
// other players in it. Same messages as rust/relay (see its main.rs); clients connect to wss://<host>/ws/<room code>.
//   client -> relay  0x01 JOIN room u32, id u32 | 0x02 SEND to u32 (0: everyone else), payload | 0x03 PING t f64 | 0x04 LEAVE
//   relay -> client  0x81 WELCOME clock f64, members u8 | 0x82 FROM from u32, payload | 0x83 PONG t f64, clock f64
// Open rooms: a host sends 0x05 ANNOUNCE + JSON {c: code, n: host name, m: stage, p: players, r: racing, x: closed}
// every few seconds; the room list lives in one Directory object. Clients connect to wss://<host>/lobby and send
// 0x06 LIST; the answer is 0x86 + JSON [{c, n, m, p, r}] of rooms heard from in the last 12 s.
// Play stats: the app POSTs small JSON notes to /stat (rust/src/game/stats.rs: launch, play, stage, finish), kept in one
// Stats object (SQLite); /stats?k=<key> shows them as a page (the key's SHA-256 is STATS_KEY_SHA256 below).
// AI cars: WELCOME carries one more byte, bit 0 set: this relay drives a race's AI cars. The host sends 0x07 STAGE +
// the stage blob (rust/src/server.rs, stage_blob), answered 0x87 + 1 (ready) or 2 (failed); the room's packets (to
// everyone, or to the server's id) then also go to the race simulation (fogwood_core::server through WebAssembly, built
// into sim/ by the deploy workflow), stepped every 20 ms, and what it sends goes out as from that id.
import simWasm from "./sim/fogwood_sim_bg.wasm";
import { initSync, Room as SimRoom, server_id } from "./sim/fogwood_sim.js";
let simReady = false;
function simInit() { if (!simReady) { initSync({ module: simWasm }); simReady = true; } return server_id(); }
const STATS_KEY_SHA256 = "80f06fd1ad99de2fd23f489f7f410d426a9de19dd38854801f889d7e34857364";
async function sha256hex(s) { const h = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(s)); return [...new Uint8Array(h)].map((b) => b.toString(16).padStart(2, "0")).join(""); }
export default {
  async fetch(req, env) {
    const url = new URL(req.url);
    if (url.pathname === "/stat") {
      const cors = { "Access-Control-Allow-Origin": "*", "Access-Control-Allow-Methods": "POST", "Access-Control-Allow-Headers": "Content-Type" };
      if (req.method === "OPTIONS") return new Response(null, { status: 204, headers: cors });
      if (req.method !== "POST") return new Response("post only", { status: 405, headers: cors });
      const body = await req.text(); if (body.length > 1500) return new Response("too big", { status: 413, headers: cors });
      const cf = req.cf || {};
      await env.STATS.get(env.STATS.idFromName("all")).fetch("https://stats/add", { method: "POST", body, headers: { "x-cc": String(cf.country || ""), "x-city": String(cf.city || "") } }).catch(() => {});
      return new Response(null, { status: 204, headers: cors });
    }
    if (url.pathname === "/stats" || url.pathname === "/stats.json") {
      if ((await sha256hex(url.searchParams.get("k") || "")) !== STATS_KEY_SHA256) return new Response("fogwood relay", { status: 404 });
      let rooms = []; try { rooms = await (await env.DIR.get(env.DIR.idFromName("all")).fetch("https://dir/list")).json(); } catch (_) {}
      const r = await env.STATS.get(env.STATS.idFromName("all")).fetch("https://stats" + url.pathname, { method: "POST", body: JSON.stringify({ rooms }) });
      return new Response(r.body, { headers: { "content-type": url.pathname.endsWith(".json") ? "application/json" : "text/html; charset=utf-8", "cache-control": "no-store", "x-robots-tag": "noindex" } });
    }
    if (url.pathname === "/lobby") { if (req.headers.get("Upgrade") !== "websocket") return new Response("websocket only", { status: 426 }); return env.DIR.get(env.DIR.idFromName("all")).fetch(req); }
    const m = url.pathname.match(/^\/ws\/(\d{1,10})$/);
    if (!m) return new Response("fogwood relay", { status: 200 });
    if (req.headers.get("Upgrade") !== "websocket") return new Response("websocket only", { status: 426 });
    return env.ROOMS.get(env.ROOMS.idFromName(m[1])).fetch(req);
  },
};

export class Room {
  constructor(ctx, env) { this.ctx = ctx; this.env = env; this.sim = null; this.timer = null; this.code = 0; }
  // the AI cars: moved on to now, and what they have to say sent to the room
  simTick() {
    const now = Date.now() / 1000, sim = this.sim; if (!sim) return;
    try {
      sim.step(now); this.simFlush();
      const l = sim.log(); if (l) console.log("room " + this.code + ": " + l);
      if (sim.idle(now) || this.ctx.getWebSockets().length === 0) this.simStop();
    } catch (e) { console.log("sim error: " + e); this.simStop(); }
  }
  simFlush() {
    const out = this.sim.take_out(); if (out.length === 0) return;
    const dv = new DataView(out.buffer, out.byteOffset, out.byteLength), socks = this.ctx.getWebSockets(), sid = server_id();
    for (let o = 0; o + 6 <= out.length;) {
      const to = dv.getUint32(o, true), n = dv.getUint16(o + 4, true); o += 6;
      const w = new Uint8Array(n + 5); w[0] = 0x82; new DataView(w.buffer).setUint32(1, sid, true); w.set(out.subarray(o, o + n), 5); o += n;
      for (const s of socks) { const a = s.deserializeAttachment(); if (!a || (to !== 0 && a.id !== to)) continue; try { s.send(w); } catch (_) {} }
    }
  }
  simStop() { if (this.timer) clearInterval(this.timer); this.timer = null; if (this.sim) { try { this.sim.free(); } catch (_) {} } this.sim = null; }
  async fetch() {
    if (this.ctx.getWebSockets().length >= 16) return new Response("room full", { status: 503 });
    const [client, server] = Object.values(new WebSocketPair());
    this.ctx.acceptWebSocket(server);   // hibernation API: no duration billed while the room is quiet
    return new Response(null, { status: 101, webSocket: client });
  }
  webSocketMessage(ws, msg) {
    if (typeof msg === "string") return;
    const b = new Uint8Array(msg); if (b.length === 0) return;
    const dv = new DataView(msg); const now = Date.now() / 1000;
    if (b[0] === 0x01 && b.length >= 9) {
      ws.serializeAttachment({ id: dv.getUint32(5, true) }); this.code = dv.getUint32(1, true);
      const w = new ArrayBuffer(11), o = new DataView(w); o.setUint8(0, 0x81); o.setFloat64(1, now, true); o.setUint8(9, this.ctx.getWebSockets().length); o.setUint8(10, 1);
      ws.send(w);
    } else if (b[0] === 0x02 && b.length >= 5 && b.length <= 1205) {
      const me = ws.deserializeAttachment(); if (!me) return;
      const to = dv.getUint32(1, true), toSim = simReady && to === server_id();
      if (!toSim) { const w = new Uint8Array(b.length); w[0] = 0x82; new DataView(w.buffer).setUint32(1, me.id, true); w.set(b.subarray(5), 5);
        for (const o of this.ctx.getWebSockets()) { if (o === ws) continue; const a = o.deserializeAttachment(); if (!a || a.id === me.id || (to !== 0 && a.id !== to)) continue; try { o.send(w); } catch (_) {} } }
      if (this.sim && (to === 0 || toSim)) { try { this.sim.packet(me.id, b.subarray(5), now); } catch (e) { console.log("sim packet: " + e); } }
    } else if (b[0] === 0x07 && b.length > 8) {   // the stage for this room's AI cars (from the host)
      const me = ws.deserializeAttachment(); if (!me) return;
      let ok = 2;
      try { simInit(); this.simStop(); const t0 = Date.now(); this.sim = new SimRoom(b.subarray(1), this.code, me.id, now); ok = 1;
        console.log("room " + this.code + ": AI cars on " + this.sim.map() + " (" + (Date.now() - t0) + " ms)");
        this.timer = setInterval(() => this.simTick(), 20);
      } catch (e) { console.log("room " + this.code + ": stage refused: " + e); this.simStop(); }
      ws.send(new Uint8Array([0x87, ok]));
    } else if (b[0] === 0x03 && b.length >= 9) {
      const w = new Uint8Array(17); w[0] = 0x83; w.set(b.subarray(1, 9), 1); new DataView(w.buffer).setFloat64(9, now, true); ws.send(w);
    } else if (b[0] === 0x04) { try { ws.close(1000, "bye"); } catch (_) {} }
    else if (b[0] === 0x05 && b.length < 400) {   // a host's room for the open-rooms list
      const body = new TextDecoder().decode(b.subarray(1));
      this.ctx.waitUntil(this.env.DIR.get(this.env.DIR.idFromName("all")).fetch("https://dir/announce", { method: "POST", body }).catch(() => {}));
    }
  }
  webSocketClose(ws, code) { try { ws.close(code, "closed"); } catch (_) {} }
  webSocketError(ws) { try { ws.close(1011, "error"); } catch (_) {} }
}

export class Directory {
  constructor(ctx) { this.ctx = ctx; this.rooms = new Map(); }
  list() {
    const now = Date.now(), out = [];
    for (const [c, r] of this.rooms) { if (now - r.t > 12000) this.rooms.delete(c); else out.push({ c, n: r.n, m: r.m, p: r.p, r: r.r }); }
    return out.sort((a, b) => b.p - a.p).slice(0, 20);
  }
  async fetch(req) {
    const url = new URL(req.url);
    if (url.pathname === "/announce") {
      try { const a = JSON.parse(await req.text()); const c = a.c >>> 0; if (!c) return new Response("bad");
        if (a.x) this.rooms.delete(c); else this.rooms.set(c, { n: String(a.n || "").slice(0, 16), m: String(a.m || "").slice(0, 12), p: Math.min(16, a.p | 0), r: !!a.r, t: Date.now() });
      } catch (_) {}
      return new Response("ok");
    }
    if (url.pathname === "/list") return Response.json(this.list());
    const [client, server] = Object.values(new WebSocketPair());
    this.ctx.acceptWebSocket(server);
    return new Response(null, { status: 101, webSocket: client });
  }
  webSocketMessage(ws, msg) {
    if (typeof msg === "string") return; const b = new Uint8Array(msg);
    if (b[0] === 0x06) { const j = new TextEncoder().encode(JSON.stringify(this.list())); const w = new Uint8Array(j.length + 1); w[0] = 0x86; w.set(j, 1); ws.send(w); }
  }
  webSocketClose(ws, code) { try { ws.close(code, "closed"); } catch (_) {} }
  webSocketError(ws) { try { ws.close(1011, "error"); } catch (_) {} }
}

// ---- play stats -------------------------------------------------------------------------------------------------
const DAY = 86400000;
const parisDay = new Intl.DateTimeFormat("en-CA", { timeZone: "Europe/Paris", year: "numeric", month: "2-digit", day: "2-digit" });
const parisWhen = new Intl.DateTimeFormat("en-GB", { timeZone: "Europe/Paris", day: "2-digit", month: "short", hour: "2-digit", minute: "2-digit" });
const MAPS = { forest: "Fogwood", snow: "Col de Givre", canyon: "Canyon Rouge", lakes: "Lakeland", touge: "Momiji Touge", riviera: "Riviera Corniche", prism: "Prism Skyway", ember: "Ember Peak" };
const MODES = { drive: "Free drive", race: "Race (AI)", online: "Online race", duos: "Duos", tiles: "Tile fall" };
const EVENTS = { launch: "opened the game", play: "started", stage: "finished a stage", finish: "finished" };
const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const tfmt = (t) => { if (!(t > 0)) return ""; const m = Math.floor(t / 60), s = t - m * 60; return m + ":" + s.toFixed(2).padStart(5, "0"); };
const ago = (ms) => { const s = (Date.now() - ms) / 1000; if (s < 90) return "just now"; if (s < 5400) return Math.round(s / 60) + " min ago"; if (s < 129600) return Math.round(s / 3600) + " h ago"; return Math.round(s / 86400) + " d ago"; };
const flag = (cc) => /^[A-Z]{2}$/.test(cc || "") ? String.fromCodePoint(...[...cc].map((c) => 0x1f1a5 + c.charCodeAt(0))) : "";

export class Stats {
  constructor(ctx) {
    this.ctx = ctx; this.sql = ctx.storage.sql; this.rate = new Map();
    this.sql.exec(`CREATE TABLE IF NOT EXISTS ev (t INTEGER, d TEXT, p TEXT, v INTEGER, e TEXT, n TEXT, m TEXT, c TEXT, o TEXT, k INTEGER, tm REAL, r INTEGER, h INTEGER, cc TEXT, city TEXT)`);
    this.sql.exec(`CREATE INDEX IF NOT EXISTS ev_t ON ev (t)`); this.sql.exec(`CREATE INDEX IF NOT EXISTS ev_d ON ev (d)`);
  }
  async fetch(req) {
    const url = new URL(req.url);
    if (url.pathname === "/add") return this.add(req);
    const { rooms } = await req.json().catch(() => ({ rooms: [] }));
    const data = this.summary(rooms || []);
    return new Response(url.pathname.endsWith(".json") ? JSON.stringify(data) : this.page(data));
  }
  async add(req) {
    let a; try { a = JSON.parse(await req.text()); } catch (_) { return new Response("bad", { status: 400 }); }
    const x = a && typeof a.x === "object" ? a.x : null; if (!x || !/^[0-9a-f]{8,16}$/.test(x.d || "") || !EVENTS[x.e]) return new Response("bad", { status: 400 });
    const now = Date.now(), r = this.rate.get(x.d) || { t: now, n: 0 }; if (now - r.t > 60000) { r.t = now; r.n = 0; } r.n++; this.rate.set(x.d, r); if (r.n > 30) return new Response("slow down", { status: 429 });
    const str = (v, n) => String(v ?? "").slice(0, n), num = (v) => (Number.isFinite(+v) ? +v : null);
    this.sql.exec(`INSERT INTO ev VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)`, now, x.d, a.p === "android" ? "android" : "web", num(a.v), x.e, str(x.n, 16), str(x.m, 12), str(x.c, 20), str(x.o, 10),
      num(x.k), num(x.t), num(x.r), num(x.h), str(req.headers.get("x-cc"), 2), str(req.headers.get("x-city"), 40));
    if (Math.random() < 0.01) this.sql.exec(`DELETE FROM ev WHERE t < ?`, now - 400 * DAY);   // keep about a year
    return new Response("ok");
  }
  q(s, ...b) { return [...this.sql.exec(s, ...b)]; }
  summary(rooms) {
    const now = Date.now(), today = parisDay.format(now);
    const one = (s, ...b) => (this.q(s, ...b)[0] || {});
    const totals = {
      players: one(`SELECT COUNT(DISTINCT d) n FROM ev`).n || 0,
      week: one(`SELECT COUNT(DISTINCT d) n FROM ev WHERE t > ?`, now - 7 * DAY).n || 0,
      launches: one(`SELECT COUNT(*) n FROM ev WHERE e = 'launch'`).n || 0,
      plays: one(`SELECT COUNT(*) n FROM ev WHERE e = 'play'`).n || 0,
      online: one(`SELECT COUNT(*) n FROM ev WHERE e = 'play' AND o IN ('online','duos','tiles')`).n || 0,
      now: one(`SELECT COUNT(DISTINCT d) n FROM ev WHERE t > ?`, now - 15 * 60000).n || 0,
    };
    // the last 30 days (Paris dates): players, launches, plays
    const days = []; for (let i = 29; i >= 0; i--) days.push({ day: parisDay.format(now - i * DAY), players: new Set(), launches: 0, plays: 0 });
    const idx = new Map(days.map((d, i) => [d.day, i]));
    for (const r of this.q(`SELECT t, d, e FROM ev WHERE t > ?`, now - 31 * DAY)) { const i = idx.get(parisDay.format(r.t)); if (i === undefined) continue; const d = days[i]; d.players.add(r.d); if (r.e === "launch") d.launches++; if (r.e === "play") d.plays++; }
    const daily = days.map((d) => ({ day: d.day, players: d.players.size, launches: d.launches, plays: d.plays }));
    totals.today = (daily.find((d) => d.day === today) || {}).players || 0;
    const players = this.q(`SELECT d, MAX(t) l, MIN(t) f, SUM(e = 'launch') nl, SUM(e = 'play') np, SUM(e IN ('finish','stage')) nf,
        (SELECT n FROM ev b WHERE b.d = a.d AND b.n <> '' ORDER BY t DESC LIMIT 1) n, (SELECT p FROM ev b WHERE b.d = a.d ORDER BY t DESC LIMIT 1) p,
        (SELECT cc FROM ev b WHERE b.d = a.d ORDER BY t DESC LIMIT 1) cc, (SELECT city FROM ev b WHERE b.d = a.d ORDER BY t DESC LIMIT 1) city,
        (SELECT c FROM ev b WHERE b.d = a.d AND b.c <> '' ORDER BY t DESC LIMIT 1) c, (SELECT v FROM ev b WHERE b.d = a.d ORDER BY t DESC LIMIT 1) v
      FROM ev a GROUP BY d ORDER BY l DESC LIMIT 300`);
    const maps = this.q(`SELECT m k, COUNT(*) n FROM ev WHERE e = 'play' GROUP BY m ORDER BY n DESC`);
    const modes = this.q(`SELECT o k, COUNT(*) n FROM ev WHERE e = 'play' GROUP BY o ORDER BY n DESC`);
    const cars = this.q(`SELECT c k, COUNT(*) n FROM ev WHERE e = 'play' AND c <> '' GROUP BY c ORDER BY n DESC`);
    const platforms = this.q(`SELECT p k, COUNT(DISTINCT d) n FROM ev GROUP BY p ORDER BY n DESC`);
    const countries = this.q(`SELECT cc k, COUNT(DISTINCT d) n FROM ev WHERE cc <> '' GROUP BY cc ORDER BY n DESC LIMIT 12`);
    const best = this.q(`SELECT m, n, c, MIN(tm) tm FROM ev WHERE e = 'stage' AND tm > 10 GROUP BY m ORDER BY m`);
    const recent = this.q(`SELECT t, d, p, e, n, m, o, c, tm, r, k, cc FROM ev ORDER BY t DESC LIMIT 60`);
    return { at: now, totals, daily, players, maps, modes, cars, platforms, countries, best, recent, rooms };
  }
  page(D) {
    const T = D.totals, maxd = Math.max(1, ...D.daily.map((d) => d.players));
    const bars = (list, names, unit) => { const m = Math.max(1, ...list.map((x) => x.n)); return list.length ? list.map((x) => `<div class="row"><span class="lab">${esc(names ? names[x.k] || x.k || "?" : x.k || "?")}</span><span class="bar"><i style="width:${(x.n / m * 100).toFixed(1)}%"></i></span><span class="num">${x.n}${unit || ""}</span></div>`).join("") : `<p class="dim">nothing yet</p>`; };
    const what = (r) => { const m = MAPS[r.m] || r.m, o = MODES[r.o] || r.o;
      if (r.e === "launch") return "opened the game";
      if (r.e === "play") return `started ${esc(o)} on ${esc(m)}${r.k ? ` (${r.k} cars)` : ""}`;
      if (r.e === "stage") return `ran ${esc(m)} in <b>${tfmt(r.tm)}</b>`;
      if (r.e === "finish") return r.o === "tiles" ? `tile fall: P${r.r} of ${r.k}, lasted ${tfmt(r.tm)}` : `finished ${esc(o)} on ${esc(m)}: P${r.r}${r.k ? "/" + r.k : ""} in <b>${tfmt(r.tm)}</b>`;
      return esc(r.e); };
    const who = (r) => esc(r.n || "unnamed") + ` <span class="dim">#${esc(String(r.d).slice(0, 4))}</span>`;
    return `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="robots" content="noindex">
<title>Fogwood Rally · players</title><link rel="preconnect" href="https://fonts.googleapis.com"><link href="https://fonts.googleapis.com/css2?family=Silkscreen&family=JetBrains+Mono:wght@400;600&display=swap" rel="stylesheet">
<style>
:root{--bg:#0b100c;--panel:#121a14;--line:#223024;--ink:#e9ecdf;--dim:#7f8b7f;--amber:#f3cf62;--red:#c23a2c;--green:#5fbf6a}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--ink);font:14px/1.45 "JetBrains Mono",ui-monospace,monospace;padding:18px;max-width:1100px;margin:auto}
h1,h2{font-family:Silkscreen,monospace;font-weight:400;letter-spacing:.04em;margin:0}h1{font-size:26px;color:var(--amber)}h2{font-size:15px;color:var(--amber);margin-bottom:10px}
header{display:flex;justify-content:space-between;align-items:baseline;gap:12px;flex-wrap:wrap;border-bottom:2px solid var(--line);padding-bottom:12px;margin-bottom:16px}
.dim{color:var(--dim)}.grid{display:grid;gap:12px;grid-template-columns:repeat(auto-fit,minmax(150px,1fr));margin-bottom:12px}
.tile,.panel{background:var(--panel);border:2px solid var(--line);padding:12px}.tile b{display:block;font-family:Silkscreen,monospace;font-size:28px;font-weight:400;color:var(--ink)}
.tile span{color:var(--dim);font-size:12px;text-transform:uppercase;letter-spacing:.06em}.tile.hot b{color:var(--green)}
.cols{display:grid;gap:12px;grid-template-columns:repeat(auto-fit,minmax(300px,1fr));margin-bottom:12px}
.chart{display:flex;align-items:flex-end;gap:3px;height:140px;padding-top:8px}.chart div{flex:1;background:var(--amber);min-height:2px;position:relative;opacity:.9}
.chart div.z{background:var(--line)}.chart div:hover{opacity:1;outline:2px solid var(--ink)}.axis{display:flex;justify-content:space-between;color:var(--dim);font-size:11px;margin-top:6px}
.row{display:grid;grid-template-columns:110px 1fr 44px;gap:8px;align-items:center;margin:5px 0}.lab{white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.bar{height:10px;background:var(--line)}.bar i{display:block;height:100%;background:var(--amber)}.num{text-align:right;color:var(--dim)}
table{width:100%;border-collapse:collapse;font-size:13px}th{text-align:left;color:var(--dim);font-weight:400;font-size:11px;text-transform:uppercase;letter-spacing:.06em;padding:4px 6px;border-bottom:2px solid var(--line)}
td{padding:5px 6px;border-bottom:1px solid var(--line);vertical-align:top}td.r,th.r{text-align:right}.wrap{overflow-x:auto}.pill{font-size:11px;padding:1px 5px;border:1px solid var(--line);color:var(--dim)}
.feed td:first-child{white-space:nowrap;color:var(--dim)}.live{color:var(--green)}
</style></head><body>
<header><h1>FOGWOOD RALLY</h1><span class="dim">players · ${esc(parisWhen.format(D.at))} Paris · <a href="" style="color:var(--amber)">refresh</a></span></header>
<div class="grid">
<div class="tile hot"><b>${T.now}</b><span>playing now (15 min)</span></div>
<div class="tile"><b>${T.today}</b><span>players today</span></div>
<div class="tile"><b>${T.week}</b><span>players, 7 days</span></div>
<div class="tile"><b>${T.players}</b><span>players, all time</span></div>
<div class="tile"><b>${T.plays}</b><span>drives &amp; races</span></div>
<div class="tile"><b>${T.online}</b><span>online rounds</span></div>
</div>
<div class="panel" style="margin-bottom:12px"><h2>PLAYERS PER DAY</h2>
<div class="chart">${D.daily.map((d) => `<div class="${d.players ? "" : "z"}" style="height:${(d.players / maxd * 100).toFixed(1)}%" title="${d.day}: ${d.players} players, ${d.launches} launches, ${d.plays} drives/races"></div>`).join("")}</div>
<div class="axis"><span>${D.daily[0].day.slice(5)}</span><span>max ${maxd}</span><span>today</span></div></div>
${D.rooms.length ? `<div class="panel" style="margin-bottom:12px"><h2 class="live">OPEN ROOMS NOW</h2>${D.rooms.map((r) => `<div>${esc(r.n)} · ${esc(MAPS[r.m] || r.m)} · ${r.p} player${r.p === 1 ? "" : "s"}${r.r ? " · racing" : ""}</div>`).join("")}</div>` : ""}
<div class="cols">
<div class="panel"><h2>STAGES</h2>${bars(D.maps, MAPS)}</div>
<div class="panel"><h2>MODES</h2>${bars(D.modes, MODES)}</div>
<div class="panel"><h2>CARS</h2>${bars(D.cars)}</div>
<div class="panel"><h2>WHERE</h2>${bars(D.platforms, { android: "Android app", web: "Web" }, "")}<div style="height:8px"></div>${bars(D.countries.map((c) => ({ k: flag(c.k) + " " + c.k, n: c.n })))}</div>
</div>
${D.best.length ? `<div class="panel" style="margin-bottom:12px"><h2>STAGE RECORDS</h2><table><tr><th>stage</th><th>driver</th><th>car</th><th class="r">time</th></tr>${D.best.map((b) => `<tr><td>${esc(MAPS[b.m] || b.m)}</td><td>${esc(b.n || "unnamed")}</td><td>${esc(b.c)}</td><td class="r"><b>${tfmt(b.tm)}</b></td></tr>`).join("")}</table></div>` : ""}
<div class="panel" style="margin-bottom:12px"><h2>PLAYERS</h2><div class="wrap"><table>
<tr><th>driver</th><th>on</th><th>from</th><th>car</th><th class="r">opens</th><th class="r">drives</th><th class="r">finishes</th><th>first seen</th><th>last seen</th></tr>
${D.players.map((p) => `<tr><td>${who(p)}</td><td><span class="pill">${p.p === "android" ? "app" + (p.v ? " " + p.v : "") : "web"}</span></td><td>${flag(p.cc)} ${esc(p.city || p.cc || "")}</td><td>${esc(p.c || "")}</td><td class="r">${p.nl}</td><td class="r">${p.np}</td><td class="r">${p.nf}</td><td class="dim">${esc(parisWhen.format(p.f))}</td><td>${ago(p.l)}</td></tr>`).join("") || `<tr><td colspan="9" class="dim">no one yet: stats start with the next app build</td></tr>`}
</table></div></div>
<div class="panel"><h2>LATEST</h2><div class="wrap"><table class="feed">${D.recent.map((r) => `<tr><td>${esc(parisWhen.format(r.t))}</td><td>${who(r)}</td><td>${what(r)}</td></tr>`).join("") || `<tr><td class="dim">nothing yet</td></tr>`}</table></div></div>
<p class="dim" style="font-size:12px;margin-top:14px">Each install sends a random id (no account, nothing from the device), its driver name, stage, car and times. Countries and cities come from Cloudflare's IP lookup; IPs aren't stored.</p>
</body></html>`;
  }
}
