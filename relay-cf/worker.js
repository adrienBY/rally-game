// Fogwood relay on Cloudflare (Workers free plan): one Durable Object per online room passes each game packet to the
// other players in it. Same messages as rust/relay (see its main.rs); clients connect to wss://<host>/ws/<room code>.
//   client -> relay  0x01 JOIN room u32, id u32 | 0x02 SEND to u32 (0: everyone else), payload | 0x03 PING t f64 | 0x04 LEAVE
//   relay -> client  0x81 WELCOME clock f64, members u8 | 0x82 FROM from u32, payload | 0x83 PONG t f64, clock f64
// Open rooms: a host sends 0x05 ANNOUNCE + JSON {c: code, n: host name, m: stage, p: players, r: racing, x: closed}
// every few seconds; the room list lives in one Directory object. Clients connect to wss://<host>/lobby and send
// 0x06 LIST; the answer is 0x86 + JSON [{c, n, m, p, r}] of rooms heard from in the last 12 s.
export default {
  async fetch(req, env) {
    const url = new URL(req.url);
    if (url.pathname === "/lobby") { if (req.headers.get("Upgrade") !== "websocket") return new Response("websocket only", { status: 426 }); return env.DIR.get(env.DIR.idFromName("all")).fetch(req); }
    const m = url.pathname.match(/^\/ws\/(\d{1,10})$/);
    if (!m) return new Response("fogwood relay", { status: 200 });
    if (req.headers.get("Upgrade") !== "websocket") return new Response("websocket only", { status: 426 });
    return env.ROOMS.get(env.ROOMS.idFromName(m[1])).fetch(req);
  },
};

export class Room {
  constructor(ctx, env) { this.ctx = ctx; this.env = env; }
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
      ws.serializeAttachment({ id: dv.getUint32(5, true) });
      const w = new ArrayBuffer(10), o = new DataView(w); o.setUint8(0, 0x81); o.setFloat64(1, now, true); o.setUint8(9, this.ctx.getWebSockets().length);
      ws.send(w);
    } else if (b[0] === 0x02 && b.length >= 5 && b.length <= 1205) {
      const me = ws.deserializeAttachment(); if (!me) return;
      const to = dv.getUint32(1, true);
      const w = new Uint8Array(b.length); w[0] = 0x82; new DataView(w.buffer).setUint32(1, me.id, true); w.set(b.subarray(5), 5);
      for (const o of this.ctx.getWebSockets()) { if (o === ws) continue; const a = o.deserializeAttachment(); if (!a || a.id === me.id || (to !== 0 && a.id !== to)) continue; try { o.send(w); } catch (_) {} }
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
