// Dumps what the JavaScript stage builder hands the Rust engine (the parity oracle for rust/src/stage):
//   node web/tools/dump_stage.mjs <out dir> [stage ...]
// Serves the app's assets (python3 web/build.py first), opens each stage in headless Chrome in app runtime mode (?rt=1) with
// stand-ins for the app's bridges, and writes <stage>.fws (scene packet) and <stage>.frt (runtime packet) plus build timings.
import { createRequire } from "module";
import http from "http"; import fs from "fs"; import path from "path";
const req = createRequire(process.env.PW_MODULES || "/tmp/node_modules/");
const { chromium } = req("playwright");
const out = process.argv[2] || "/tmp/stages"; fs.mkdirSync(out, { recursive: true });
const stages = process.argv.slice(3).length ? process.argv.slice(3) : ["forest", "snow", "canyon", "lakes", "touge", "riviera", "prism", "ember"];
const root = path.resolve(path.dirname(new URL(import.meta.url).pathname), "../../android/app/src/main/assets");
const srv = http.createServer((q, r) => { const f = path.join(root, decodeURIComponent(q.url.split("?")[0].split("#")[0]) || "index.html");
  fs.readFile(f, (e, d) => { if (e) { r.writeHead(404); r.end(); return; } r.writeHead(200, { "content-type": f.endsWith(".js") ? "text/javascript" : "text/html" }); r.end(d); }); });
await new Promise((ok) => srv.listen(0, "127.0.0.1", ok));
const port = srv.address().port;
const b = await chromium.launch({ executablePath: process.env.CHROME || "/usr/bin/google-chrome", args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"] });
for (const st of stages) {
  const p = await b.newPage({ viewport: { width: 1280, height: 720 } });
  p.on("pageerror", (e) => console.error(st, "page error:", String(e)));
  await p.addInitScript(() => {
    window.__pk = []; window.__t0 = performance.now();
    window.FogwoodGL = { postMessage: (ab) => { window.__pk.push(new Uint8Array(ab.slice(0))); window.__tDone = performance.now(); } };
    window.FogwoodNative = { haptic() {}, version: () => "dump" };   // no prepare(): the grids are built in JavaScript
    try { localStorage.clear(); } catch (_) {}
  });
  await p.goto(`http://127.0.0.1:${port}/index.html?rt=1#${st}`);
  await p.waitForFunction(() => window.__pk.length >= 2, null, { timeout: 180000 });
  const ms = await p.evaluate(() => window.__tDone - window.__t0);
  // out in base64 slices (one big array through the protocol runs node out of memory)
  const grab = async (k) => { const n = await p.evaluate((k) => window.__pk[k].length, k), parts = [];
    for (let o = 0; o < n; o += 4 << 20) parts.push(Buffer.from(await p.evaluate(([k, o]) => { const u = window.__pk[k].subarray(o, o + (4 << 20)); let s = "";
      for (let i = 0; i < u.length; i += 0x8000) s += String.fromCharCode.apply(null, u.subarray(i, i + 0x8000)); return btoa(s); }, [k, o]), "base64"));
    return Buffer.concat(parts); };
  const a = await grab(0), bb = await grab(1);
  fs.writeFileSync(path.join(out, st + ".fws"), a); fs.writeFileSync(path.join(out, st + ".frt"), bb);
  console.log(st, "built in", Math.round(ms), "ms (page load included);", a.length, "+", bb.length, "bytes");
  await p.close();
}
await b.close(); srv.close();
