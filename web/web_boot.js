/* Browser build of the native game (GitHub Pages): the Rust engine compiled to WebAssembly does everything, exactly like the Android
   app - it builds each stage, decodes the recordings and runs the game in a WebGL2 canvas. This page only holds the canvas, the
   input, the sound worklet, the sockets and the name box. */
import init, * as fw from "./fogwood_core.js";
import { makeRtc } from "./fw_rtc.js";
const fallback = () => { location.replace("legacy.html" + location.hash); };   // no WebGL2 / WebAssembly: the older all-JavaScript game
/* sound: the Rust sound engine (rust/audio) as WebAssembly inside an AudioWorklet, started by the first tap (autoplay rules). Messages
   from the game wait until then: recordings and the stage's preset are kept, per-frame ones are dropped. */
const WORKLET = `class F extends AudioWorkletProcessor { constructor() { super(); this.ex = null; this.q = []; this.port.onmessage = (e) => this.msg(e.data); }
  boot(mod) { const imp = {}; for (const i of WebAssembly.Module.imports(mod)) { (imp[i.module] = imp[i.module] || {})[i.name] = i.kind === "function" ? () => 0 : undefined; }
    this.ex = new WebAssembly.Instance(mod, imp).exports; this.ex.fa_init(sampleRate); for (const m of this.q) this.feed(m); this.q = []; this.port.postMessage({ ok: 1 }); }
  feed(ab) { const u = new Uint8Array(ab); const p = this.ex.fa_msg_ptr(u.length); new Uint8Array(this.ex.memory.buffer, p, u.length).set(u); this.ex.fa_message(u.length); }
  msg(d) { if (d && d.wasm) { try { this.boot(new WebAssembly.Module(d.wasm)); } catch (e) { this.port.postMessage({ err: String(e) }); } return; }
    if (d instanceof ArrayBuffer) { if (this.ex) this.feed(d); else this.q.push(d); } }
  process(i, o) { const out = o[0]; if (!this.ex || out.length < 2) return true; const n = out[0].length, p = this.ex.fa_render(n), f = new Float32Array(this.ex.memory.buffer, p, n * 2);
    const L = out[0], R = out[1]; for (let k = 0; k < n; k++) { L[k] = f[k * 2]; R[k] = f[k * 2 + 1]; } return true; } }
registerProcessor("fogwood", F);`;
const SND = { ac: null, node: null, started: false, rec: new Map(), keep: new Map() };
SND.send = (ab) => { if (SND.node) { SND.node.port.postMessage(ab, [ab]); return; }
  const k = new Float32Array(ab, 0, 2); if (k[0] === 4) SND.rec.set(k[1], ab); else if (k[0] === 5 || k[0] === 3) SND.keep.set(k[0], ab); };
SND.start = async () => { if (SND.started) { try { SND.ac && SND.ac.resume(); } catch (_) {} return; } SND.started = true;
  try { const AC = window.AudioContext || window.webkitAudioContext; const ac = SND.ac = new AC({ latencyHint: "interactive" });
    const wasm = await (await fetch("fogwood_audio.wasm")).arrayBuffer();
    await ac.audioWorklet.addModule(URL.createObjectURL(new Blob([WORKLET], { type: "application/javascript" })));
    const node = new AudioWorkletNode(ac, "fogwood", { numberOfInputs: 0, numberOfOutputs: 1, outputChannelCount: [2] }); node.connect(ac.destination);
    node.port.onmessage = (e) => { if (e.data && e.data.err) console.warn("sound engine", e.data.err); };
    node.port.postMessage({ wasm });
    for (const m of [...SND.rec.values(), ...SND.keep.values()]) node.port.postMessage(m, [m]); SND.rec.clear(); SND.keep.clear();
    SND.node = node; ac.resume(); } catch (e) { console.warn("sound engine", e); } };
window.__SHARED = { send: (f32) => SND.send(f32.buffer.byteLength === f32.byteLength ? f32.buffer : f32.slice().buffer), start: () => SND.start(), get ac() { return SND.ac; } };
document.addEventListener("visibilitychange", () => { try { if (SND.ac) { if (document.hidden) SND.ac.suspend(); else if (window.__audioOn) SND.ac.resume(); } } catch (_) {} });
(async () => {
  let ok = false; const cv = document.createElement("canvas");
  try { if (!/[?&]rt=0/.test(location.search)) { await init(); ok = fw.web_init(cv); } } catch (e) { console.warn("native build unavailable", e); }
  if (!ok) { fallback(); return; }
  cv.id = "fw"; cv.style.cssText = "position:fixed;inset:0;width:100vw;height:100dvh;display:block;z-index:10;touch-action:none;background:#0b100b;outline:none";
  document.body.appendChild(cv);
  const rot = document.createElement("div");
  rot.style.cssText = "position:fixed;inset:0;z-index:11;display:none;align-items:center;justify-content:center;background:#0b100b;color:#e9ece2;font:16px monospace;letter-spacing:2px;text-align:center";
  rot.textContent = "TURN YOUR PHONE SIDEWAYS"; document.body.appendChild(rot);
  const vib = (k, s) => { try { navigator.vibrate && navigator.vibrate(k === "impact" ? Math.round(14 + 40 * s) : k === "land" ? Math.round(10 + 28 * s) : k === "rumble" ? Math.round(5 + 9 * s) : k === "shift" ? 12 : 7); } catch (_) {} };
  // online rooms: the engine's MQTT client talks to the public brokers over secure WebSockets held here
  const socks = new Map();
  window.__fwWs = {
    // (?ws= / ?relay=: test servers; a socket opened again replaces the old one)
    open: (k, url) => { try { const q = new URLSearchParams(location.search), o = k >= 1000 ? q.get("relay") : q.get("ws"); if (o) url = o;
      const old = socks.get(k); if (old) { old.onclose = old.onerror = old.onmessage = null; try { old.close(); } catch (_) {} }
      const ws = new WebSocket(url, k >= 1000 ? [] : ["mqtt"]); ws.binaryType = "arraybuffer"; socks.set(k, ws);
      ws.onopen = () => { if (socks.get(k) === ws) fw.web_ws_open(k); };
      ws.onmessage = (e) => { if (socks.get(k) === ws && e.data instanceof ArrayBuffer) fw.web_ws_data(k, new Uint8Array(e.data)); };
      ws.onclose = ws.onerror = () => { if (socks.get(k) === ws) { socks.delete(k); fw.web_ws_close(k); } };
    } catch (e) { setTimeout(() => fw.web_ws_close(k), 0); } },
    send: (k, b) => { const ws = socks.get(k); if (ws && ws.readyState === 1) try { ws.send(b.slice()); } catch (_) {} },
    close: (k) => { const ws = socks.get(k); if (ws) { socks.delete(k); ws.onclose = ws.onerror = ws.onmessage = null; try { ws.close(); } catch (_) {} } },
  };
  // WebRTC data channels to the other phones (UDP from the browser: see fw_rtc.js and rust/src/net/rtc.rs)
  window.__fwRtc = makeRtc({ open: (id) => fw.web_rtc_open(id), close: (id) => fw.web_rtc_close(id), data: (id, u8) => fw.web_rtc_data(id, u8), signal: (id, k, t) => fw.web_rtc_signal(id, k, t) });
  window.__fw = fw;
  window.__fwAudio = (u8) => { try { SND.send(u8.slice().buffer); } catch (_) {} };
  const map = fw.web_start("fogwood-native-save");   // (starts building that stage)
  // the recordings: fetched in the background and decoded by the engine
  for (const f of ["wind.mp3", "birds.mp3", "engine.wav", "skid.wav", "impact.wav", "water_a.mp3", "water_b.mp3", "traffic.mp3", "splash.mp3", "metal.mp3", "plank.mp3", "soft.mp3", "wind2.mp3", "boom.mp3", "explo.mp3", "lava.mp3", "erupt.mp3"])
    fetch("snd/" + f).then((r) => r.ok ? r.arrayBuffer() : null).then((b) => { if (b) setTimeout(() => { try { fw.web_sound(f, new Uint8Array(b)); } catch (e) { console.warn("sound", f, e); } }, 0); }).catch(() => {});
  if (!/[?&]rt=1/.test(location.search)) history.replaceState(null, "", "?rt=1" + location.search.replace(/^\?/, "&") + "#" + map);
  // size, safe areas. A home-screen web app on an iPhone owns the whole screen, but iOS sometimes reports a viewport
  // shorter than it: once the keyboard has been up (the driver name), for the rest of the session, and in landscape on
  // some versions. The page's dark background then showed as a black bar at the bottom. So there the canvas is sized to
  // the screen itself, and once the keyboard is down the viewport is made to measure itself again (heal).
  const probe = document.getElementById("probe");
  const homeApp = navigator.standalone === true || /[?&]homeapp=1/.test(location.search);   // (?homeapp=1: tests)
  const vh = document.createElement("div");   // a full-height box: laying it out again makes WebKit re-measure
  vh.style.cssText = "position:fixed;left:0;top:0;width:1px;height:100dvh;visibility:hidden;pointer-events:none"; document.body.appendChild(vh);
  const size = () => { let w = innerWidth, h = innerHeight;
    if (homeApp && screen.width > 0 && screen.height > 0) {
      const L = Math.max(screen.width, screen.height), S = Math.min(screen.width, screen.height), land = w > h;
      // the whole screen, when the viewport spans its width (not an iPad window or split view: trust the viewport there)
      if (Math.abs(w - (land ? L : S)) <= 2) h = Math.max(h, land ? S : L); }
    return [w, h]; };
  const fit = () => { const d = Math.min(window.devicePixelRatio || 1, 3); const [cw, ch] = size();
    cv.style.width = cw + "px"; cv.style.height = ch + "px";
    const w = Math.round(cw * d), h = Math.round(ch * d);
    if (cv.width !== w || cv.height !== h) { cv.width = w; cv.height = h; } fw.web_resize(w, h);
    const cs = getComputedStyle(probe); const px = (v) => (parseFloat(v) || 0) * d;
    fw.web_insets(px(cs.paddingTop), px(cs.paddingRight), px(cs.paddingBottom), px(cs.paddingLeft));
    rot.style.display = ch > cw * 1.05 ? "flex" : "none"; };
  const heal = () => setTimeout(() => { vh.style.display = "none"; void vh.offsetHeight; vh.style.display = "block"; void vh.offsetHeight; fit(); }, 150);
  addEventListener("resize", fit); addEventListener("orientationchange", () => setTimeout(fit, 250));
  if (window.visualViewport) visualViewport.addEventListener("resize", fit);
  document.addEventListener("focusout", heal); fit();
  // touches (and the mouse) in canvas pixels: 0 down, 1 move, 2 up
  let sound = false; const d = () => Math.min(window.devicePixelRatio || 1, 3);
  const live = new Set();   // pointers down: the first one also clears anything whose lift was lost (3)
  addEventListener("blur", () => { live.clear(); fw.web_touch(5, 0, 0, 0); });
  document.addEventListener("visibilitychange", () => { if (document.hidden) { live.clear(); fw.web_touch(5, 0, 0, 0); } });
  // iPhone: Web Audio is silenced by the ring/silent switch unless the page plays as media: ask for the playback session
  // (iOS 17+), and for older versions keep a silent <audio> loop going, which switches the page to the media category
  const unmute = () => { try { if (navigator.audioSession) navigator.audioSession.type = "playback"; } catch (_) {}
    try { const el = document.createElement("audio"); el.setAttribute("playsinline", ""); el.loop = true;
      el.src = "data:audio/wav;base64,UklGRiQAAABXQVZFZm10IBAAAAABAAEAQB8AAIA+AAACABAAZGF0YQAAAAA="; el.volume = 0.01; el.play().catch(() => {}); window.__fwSilent = el; } catch (_) {} };
  const start = () => { if (!sound) { sound = true; unmute(); window.__audioOn = true; SND.start(); } };
  const send = (a, id, x, y) => { if (a === 0) { if (live.size === 0) a = 3; live.add(id); } else if (a === 2) live.delete(id); fw.web_touch(a, id, x * d(), y * d()); };
  // fingers: Touch Events with preventDefault, so Safari never turns a press into a scroll, zoom, selection or
  // edge gesture (it cancels the touch when it does: steering and pedals let go by themselves)
  const tch = (a) => (e) => { e.preventDefault(); start();
    for (const t of e.changedTouches) send(a, t.identifier & 0xffff, t.clientX, t.clientY); };
  const opt = { passive: false };
  cv.addEventListener("touchstart", tch(0), opt); cv.addEventListener("touchmove", tch(1), opt);
  cv.addEventListener("touchend", (e) => { tch(2)(e); try { const a = SND.ac; if (a && a.state !== "running") a.resume(); } catch (_) {} }, opt); cv.addEventListener("touchcancel", tch(2), opt);
  for (const ev of ["gesturestart", "gesturechange", "dblclick"]) document.addEventListener(ev, (e) => e.preventDefault(), opt);
  // the mouse (computers): pointer events, mouse only
  const MID = 0x7fff;
  cv.addEventListener("pointerdown", (e) => { if (e.pointerType !== "mouse") return; e.preventDefault(); start(); try { cv.setPointerCapture(e.pointerId); } catch (_) {} send(0, MID, e.clientX, e.clientY); });
  cv.addEventListener("pointermove", (e) => { if (e.pointerType === "mouse" && e.buttons) send(1, MID, e.clientX, e.clientY); });
  for (const ev of ["pointerup", "pointercancel"]) cv.addEventListener(ev, (e) => { if (e.pointerType === "mouse") send(2, MID, e.clientX, e.clientY); });
  addEventListener("keydown", (e) => { if (e.target && e.target.tagName === "INPUT") return; if (e.key === "Escape" || e.key === "Backspace") fw.web_back(); });
  // the driver name: a text box over the game (the phone's own keyboard comes up); first run: no way round it
  let nameBox = null;
  const askName = (first, cur) => {
    if (nameBox) return;
    const d = document.createElement("div"); nameBox = d;
    d.style.cssText = "position:fixed;inset:0;z-index:50;display:flex;align-items:center;justify-content:center;background:rgba(0,0,0,.55);font-family:monospace";
    d.innerHTML = '<form style="background:#111;border:2px solid #f2b13a;padding:18px 20px;width:min(86vw,340px);color:#f2b13a;letter-spacing:1px">' +
      '<div style="font-size:16px;margin-bottom:6px">' + (first ? "CHOOSE YOUR DRIVER NAME" : "DRIVER NAME") + '</div>' +
      '<div style="font-size:12px;color:#bbb;margin-bottom:12px">OTHER PLAYERS SEE IT IN ONLINE RACES</div>' +
      '<input maxlength="12" autocomplete="off" autocapitalize="characters" spellcheck="false" style="width:100%;box-sizing:border-box;font:18px monospace;padding:8px;background:#000;color:#fff;border:1px solid #666;text-transform:uppercase">' +
      '<div style="display:flex;gap:10px;justify-content:flex-end;margin-top:14px">' + (first ? "" : '<button type="button" data-x style="font:14px monospace;padding:6px 12px">CANCEL</button>') +
      '<button type="submit" style="font:14px monospace;padding:6px 14px;background:#f2b13a;border:0">OK</button></div></form>';
    document.body.appendChild(d);
    const inp = d.querySelector("input"), close = () => { inp.blur(); d.remove(); nameBox = null; heal(); };
    inp.value = first ? "" : cur; setTimeout(() => inp.focus(), 30);
    d.querySelector("form").onsubmit = (e) => { e.preventDefault(); const n = inp.value.trim(); if (!/[a-z0-9]/i.test(n)) return; fw.web_name(n); close(); };
    const x = d.querySelector("[data-x]"); if (x) x.onclick = close;
  };
  // the frame loop (the engine asks for 30 fps on slow devices)
  let last = 0, gap = 12.5;
  const frame = (now) => { requestAnimationFrame(frame); if (last && now - last < gap) return;
    const dt = last ? Math.min(0.1, (now - last) / 1000) : 1 / 60; last = now;
    let ev = ""; try { ev = fw.web_frame(dt * 1e0); } catch (e) { console.warn(e); }
    if (ev) for (const l of ev.split("\n")) {
      if (l === "fps 30") gap = 29; else if (l === "fps 60") gap = 12.5;
      else if (l.startsWith("h ")) { const p = l.split(" "); if (p.length === 3) vib(p[1], +p[2] || 0.5); }
      else if (l.startsWith("stat ")) { try { fetch("https://fogwood-relay.adrien-paul-bonnay.workers.dev/stat", { method: "POST", mode: "no-cors", keepalive: true, body: '{"p":"' + (/(^|\.)itch\.(zone|io)$/.test(location.hostname) ? "itch" : "web") + '","x":' + l.slice(5) + "}" }).catch(() => {}); } catch (_) {} }
      else if (l.startsWith("name ")) { const f = l[5] === "1", c = l.slice(7); setTimeout(() => askName(f, c), 0); } } };
  requestAnimationFrame(frame);
})();
