// Fogwood relay on Cloudflare (Workers free plan): one Durable Object per online room passes each game packet to the
// other players in it. Same messages as rust/relay (see its main.rs); clients connect to wss://<host>/ws/<room code>.
//   client -> relay  0x01 JOIN room u32, id u32 | 0x02 SEND to u32 (0: everyone else), payload | 0x03 PING t f64 | 0x04 LEAVE
//   relay -> client  0x81 WELCOME clock f64, members u8 | 0x82 FROM from u32, payload | 0x83 PONG t f64, clock f64
export default {
  async fetch(req, env) {
    const url = new URL(req.url); const m = url.pathname.match(/^\/ws\/(\d{1,10})$/);
    if (!m) return new Response("fogwood relay", { status: 200 });
    if (req.headers.get("Upgrade") !== "websocket") return new Response("websocket only", { status: 426 });
    return env.ROOMS.get(env.ROOMS.idFromName(m[1])).fetch(req);
  },
};

export class Room {
  constructor(ctx) { this.ctx = ctx; }
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
  }
  webSocketClose(ws, code) { try { ws.close(code, "closed"); } catch (_) {} }
  webSocketError(ws) { try { ws.close(1011, "error"); } catch (_) {} }
}
