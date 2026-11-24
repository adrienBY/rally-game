// WebRTC data channels for the browser build (rust/src/net/rtc.rs has the whole story): one unordered data channel per
// other phone, negotiated out of band as stream 0 (no extra handshake), each message retried for 200 ms at most. The
// engine drives it through window.__fwRtc (open / signal / send / close); what happens comes back through `cb`:
//   open(id), close(id), data(id, Uint8Array), signal(id, kind, text)   kind 0 offer, 1 answer, 2 candidate ("mid\ncand")
//
// TURN: when two phones can't reach each other directly (4G networks, school wifi that keeps devices apart), the link goes
// through Cloudflare's TURN servers (the nearest of their sites, free up to 1,000 GB a month). The relay hands out
// credentials valid for a day (GET /turn); the TURN key itself never leaves the relay. ICE prefers direct candidates, so
// only the pairs that need it use TURN. The app side needs nothing: it reaches the browser's relayed candidate like any
// other address. (?turn=0: off; ?turn=only: relayed candidates only, for tests)
const TURN_URL = "https://fogwood-relay.adrien-paul-bonnay.workers.dev/turn";
let turnCache = null, turnAt = 0;
export async function turnServers() {
  if (/[?&]turn=0/.test(location.search)) return [];
  const now = Date.now() / 1000; if (turnCache && turnCache.length && now - turnAt < 3600) return turnCache;
  try { const r = await fetch(TURN_URL, { cache: "no-store" }); const j = await r.json();
    turnCache = (j.iceServers || []).filter((s) => s.username && s.credential); turnAt = now;   // the TURN entries (not its STUN one)
  } catch (_) { turnCache = turnCache || []; }
  return turnCache;
}
export function makeRtc(cb) {
  turnServers();   // credentials ready by the first link (and kept fresh: each open asks again)
  const links = new Map();   // id -> { pc, dc, remote (the other side's description is set), queued candidates, dead }
  const kill = (id) => { const L = links.get(id); if (!L) return; links.delete(id); L.dead = true;
    for (const f of [() => L.dc.close(), () => L.pc.close()]) { try { f(); } catch (_) {} } };
  const api = {
    open(id, offerer, ice) { try { api.open_(id, offerer, ice); } catch (_) { kill(id); setTimeout(() => cb.close(id), 0); } },
    signal(id, kind, text) { try { api.signal_(id, kind, text); } catch (_) { kill(id); setTimeout(() => cb.close(id), 0); } },
    open_(id, offerer, ice) {
      kill(id);
      let iceServers = []; try { iceServers = JSON.parse(ice); } catch (_) {}
      iceServers = iceServers.concat(turnCache || []); turnServers();
      const only = /[?&]turn=only/.test(location.search);
      let pc; try { pc = new RTCPeerConnection(only ? { iceServers, iceTransportPolicy: "relay" } : { iceServers }); } catch (e) { setTimeout(() => cb.close(id), 0); return; }
      const dc = pc.createDataChannel("fw", { negotiated: true, id: 0, ordered: false, maxPacketLifeTime: 200 });
      dc.binaryType = "arraybuffer";
      const L = { pc, dc, remote: false, queued: [], dead: false }; links.set(id, L);
      const mine = () => links.get(id) === L && !L.dead;
      const dead = () => { if (!mine()) return; L.dead = true; cb.close(id); };
      dc.onopen = () => { if (mine()) cb.open(id); };
      dc.onclose = dead;
      dc.onmessage = (e) => { if (mine() && e.data instanceof ArrayBuffer) cb.data(id, new Uint8Array(e.data)); };
      pc.onicecandidate = (e) => { if (mine() && e.candidate && e.candidate.candidate) cb.signal(id, 2, (e.candidate.sdpMid ?? "0") + "\n" + e.candidate.candidate); };
      pc.onconnectionstatechange = () => { if (pc.connectionState === "failed" || pc.connectionState === "closed") dead(); };
      if (offerer) pc.createOffer().then((o) => pc.setLocalDescription(o)).then(() => { if (mine()) cb.signal(id, 0, pc.localDescription.sdp); }).catch(dead);
    },
    signal_(id, kind, text) {
      const L = links.get(id); if (!L || L.dead) return; const pc = L.pc;
      const mine = () => links.get(id) === L && !L.dead;
      const fail = () => { if (!mine()) return; L.dead = true; cb.close(id); };
      const flush = () => { L.remote = true; for (const c of L.queued.splice(0)) pc.addIceCandidate(c).catch(() => {}); };
      if (kind === 0) pc.setRemoteDescription({ type: "offer", sdp: text }).then(() => { flush(); return pc.createAnswer(); })
        .then((a) => pc.setLocalDescription(a)).then(() => { if (mine()) cb.signal(id, 1, pc.localDescription.sdp); }).catch(fail);
      else if (kind === 1) pc.setRemoteDescription({ type: "answer", sdp: text }).then(flush).catch(fail);
      else if (kind === 2) {
        const i = text.indexOf("\n"); const c = { candidate: i >= 0 ? text.slice(i + 1) : text, sdpMid: i >= 0 ? text.slice(0, i) : "0", sdpMLineIndex: 0 };
        if (L.remote) pc.addIceCandidate(c).catch(() => {}); else L.queued.push(c);
      }
    },
    send(id, b) {
      const L = links.get(id); if (!L || L.dead || L.dc.readyState !== "open" || L.dc.bufferedAmount > 65536) return false;
      try { L.dc.send(b.slice()); return true; } catch (_) { return false; }
    },
    close(id) { kill(id); },
  };
  return api;
}
