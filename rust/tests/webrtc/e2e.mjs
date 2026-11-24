// node e2e.mjs <game page url> <secs> <code file>: the real browser build in headless Chrome joins the room whose code
// the native host wrote to <code file>, then prints its view of the room (links, data channels) every 2 s
import { createRequire } from "module"; import fs from "fs";
const { chromium } = createRequire("/opt/npm-tools/node_modules/")("playwright");
const [url, secs, codeFile] = [process.argv[2], +(process.argv[3] || 40), process.argv[4] || "/tmp/fw_code"];
const b = await chromium.launch({ executablePath: "/usr/bin/google-chrome",
  args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist", "--autoplay-policy=no-user-gesture-required"] });
const p = await b.newPage({ viewport: { width: 800, height: 450 } });
const errs = [];
p.on("console", (m) => { const t = m.text(); if (m.type() === "error" || /panic|rtc|webrtc/i.test(t)) errs.push(m.type() + ": " + t.slice(0, process.env.FULL ? 4000 : 200)); if (process.env.VERBOSE) console.log("page:", t); });
p.on("pageerror", (e) => errs.push("pageerror: " + String(e).slice(0, 200)));
await p.goto(url);
const t0 = Date.now();
await p.waitForFunction(() => window.__fw && typeof window.__fw.web_test_join === "function", null, { timeout: 60000 });
console.log(`page ready after ${((Date.now() - t0) / 1000).toFixed(1)} s`);
let code = ""; for (let i = 0; i < 200 && !code; i++) { try { code = fs.readFileSync(codeFile, "utf8").trim(); } catch (_) {} if (!code) await p.waitForTimeout(100); }
await p.evaluate((c) => window.__fw.web_test_join(+c), code); console.log("joined", code);
for (let s = 0; s < secs; s += 2) {
  await p.waitForTimeout(2000);
  let v; try { v = await p.evaluate(() => [window.__fw.web_test_net(), window.__fw.web_room(), (window.__fw.web_status() || "").slice(0, 120)]); }
  catch (e) { console.log(`t=${s + 2} evaluate failed: ${String(e).split("\n")[0]}`); console.log("console:", JSON.stringify(errs.slice(-12))); console.log("first:", errs[0]); break; }
  console.log(`t=${s + 2} net=[${v[0]}] room=${v[1]} status=${v[2].replace(/\n/g, " | ")}`);
}
await p.screenshot({ path: "/tmp/e2e.jpg", type: "jpeg", quality: 80 });
console.log("console:", JSON.stringify(errs.slice(-15)));
await b.close();
