/* Browser build of the native game (GitHub Pages): the Rust engine compiled to WebAssembly runs the whole game in a
   WebGL2 canvas over the page, exactly like the Android app. This page plays the app's hidden stage builder: it builds
   each stage and hands the scene to the engine through the same FogwoodGL / FogwoodNative bridges the app provides. */
import init, * as fw from "./fogwood_core.js";
const main = document.getElementById("fwmain").textContent;
const fallback = () => { const s = document.createElement("script"); s.type = "module"; s.textContent = main; document.body.appendChild(s); };
(async () => {
  let ok = false; const cv = document.createElement("canvas");
  try { if (!/[?&]rt=0/.test(location.search)) { await init(); ok = fw.web_init(cv); } } catch (e) { console.warn("native build unavailable", e); }
  if (!ok) { fallback(); return; }   // no WebGL2 / WebAssembly: the older all-JS game
  cv.id = "fw"; cv.style.cssText = "position:fixed;inset:0;width:100vw;height:100dvh;display:block;z-index:10;touch-action:none;background:#0b100b;outline:none";
  document.body.appendChild(cv);
  const rot = document.createElement("div");
  rot.style.cssText = "position:fixed;inset:0;z-index:11;display:none;align-items:center;justify-content:center;background:#0b100b;color:#e9ece2;font:16px monospace;letter-spacing:2px;text-align:center";
  rot.textContent = "TURN YOUR PHONE SIDEWAYS"; document.body.appendChild(rot);
  // the page's own canvas stays hidden under ours; the bridges the stage builder talks to
  window.FogwoodGL = { postMessage: (ab) => { try { fw.web_message(new Uint8Array(ab)); } catch (e) { console.warn(e); } } };
  const vib = (k, s) => { try { navigator.vibrate && navigator.vibrate(k === "impact" ? Math.round(14 + 40 * s) : k === "land" ? Math.round(10 + 28 * s) : k === "rumble" ? Math.round(5 + 9 * s) : k === "shift" ? 12 : 7); } catch (_) {} };
  window.FogwoodNative = { prepare: (m, p, r, gx, gz) => fw.web_prepare(m, p, r, gx, gz), obstacles: (c) => fw.web_obstacles(c), step: (c) => fw.web_step(c), gridBytes: () => fw.web_grid_bytes(),
    haptic: vib, glStatus: () => fw.web_status(), version: () => "fogwood-core-wasm" };
  // online rooms: the engine's MQTT client talks to the public brokers over secure WebSockets held here
  const socks = new Map();
  window.__fwWs = {
    open: (k, url) => { try { const o = new URLSearchParams(location.search).get("ws"); if (o) url = o;   // ?ws=ws://host:port: a test broker const old = socks.get(k); if (old) { old.onclose = old.onerror = old.onmessage = null; try { old.close(); } catch (_) {} }
      const ws = new WebSocket(url, ["mqtt"]); ws.binaryType = "arraybuffer"; socks.set(k, ws);
      ws.onopen = () => { if (socks.get(k) === ws) fw.web_ws_open(k); };
      ws.onmessage = (e) => { if (socks.get(k) === ws && e.data instanceof ArrayBuffer) fw.web_ws_data(k, new Uint8Array(e.data)); };
      ws.onclose = ws.onerror = () => { if (socks.get(k) === ws) { socks.delete(k); fw.web_ws_close(k); } };
    } catch (e) { setTimeout(() => fw.web_ws_close(k), 0); } },
    send: (k, b) => { const ws = socks.get(k); if (ws && ws.readyState === 1) try { ws.send(b.slice()); } catch (_) {} },
    close: (k) => { const ws = socks.get(k); if (ws) { socks.delete(k); ws.onclose = ws.onerror = ws.onmessage = null; try { ws.close(); } catch (_) {} } },
  };
  window.__fw = fw;
  window.__fwAudio = (u8) => { try { const S = window.__SHARED; if (S) S.send(new Float32Array(u8.slice().buffer)); } catch (_) {} };
  const map = fw.web_start("fogwood-native-save");
  if (!/[?&]rt=1/.test(location.search)) history.replaceState(null, "", "?rt=1" + location.search.replace(/^\?/, "&") + "#" + map);
  // size, safe areas
  const probe = document.getElementById("probe");
  const fit = () => { const d = Math.min(window.devicePixelRatio || 1, 3); const w = Math.round(innerWidth * d), h = Math.round(innerHeight * d);
    if (cv.width !== w || cv.height !== h) { cv.width = w; cv.height = h; } fw.web_resize(w, h);
    const cs = getComputedStyle(probe); const px = (v) => (parseFloat(v) || 0) * d;
    fw.web_insets(px(cs.paddingTop), px(cs.paddingRight), px(cs.paddingBottom), px(cs.paddingLeft));
    rot.style.display = innerHeight > innerWidth * 1.05 ? "flex" : "none"; };
  addEventListener("resize", fit); addEventListener("orientationchange", () => setTimeout(fit, 250)); fit();
  // touches (and the mouse) in canvas pixels: 0 down, 1 move, 2 up
  let sound = false; const d = () => Math.min(window.devicePixelRatio || 1, 3);
  const live = new Set();   // pointers down: the first one also clears anything whose lift was lost (3)
  addEventListener("blur", () => { live.clear(); fw.web_touch(5, 0, 0, 0); });
  document.addEventListener("visibilitychange", () => { if (document.hidden) { live.clear(); fw.web_touch(5, 0, 0, 0); } });
  const ptr = (a0) => (e) => { e.preventDefault(); let a = a0;
    if (a === 0) { if (live.size === 0) a = 3; live.add(e.pointerId); } else if (a === 2) live.delete(e.pointerId); if (!sound) { sound = true; window.__audioOn = true; try { window.__SHARED && window.__SHARED.start(); } catch (_) {} }
    if (a0 === 0) try { cv.setPointerCapture(e.pointerId); } catch (_) {}
    fw.web_touch(a, e.pointerId & 0xffff, e.clientX * d(), e.clientY * d()); };
  cv.addEventListener("pointerdown", ptr(0)); cv.addEventListener("pointermove", (e) => { if (e.buttons || e.pointerType === "touch") ptr(1)(e); });
  cv.addEventListener("pointerup", ptr(2)); cv.addEventListener("pointercancel", ptr(2));
  addEventListener("keydown", (e) => { if (e.key === "Escape" || e.key === "Backspace") fw.web_back(); });
  // build the first stage
  fallback();
  // the frame loop (the engine asks for 30 fps on slow devices)
  let last = 0, gap = 12.5;
  const frame = (now) => { requestAnimationFrame(frame); if (last && now - last < gap) return;
    const dt = last ? Math.min(0.1, (now - last) / 1000) : 1 / 60; last = now;
    let ev = ""; try { ev = fw.web_frame(dt * 1e0); } catch (e) { console.warn(e); }
    if (ev) for (const l of ev.split("\n")) {
      if (l === "fps 30") gap = 29; else if (l === "fps 60") gap = 12.5;
      else if (l.startsWith("h ")) { const p = l.split(" "); if (p.length === 3) vib(p[1], +p[2] || 0.5); }
      else if (l.startsWith("map ")) { const n = l.slice(4).replace(/[^a-z]/gi, ""); setTimeout(() => window.__rtBoot && window.__rtBoot(n), 0); } } };
  requestAnimationFrame(frame);
})();
