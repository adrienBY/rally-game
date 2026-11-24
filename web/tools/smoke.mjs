// Browser smoke test of the WebAssembly build: node web/tools/smoke.mjs <url of dist-web/index.html> [chrome path]
// Opens the page in headless Chrome (software WebGL), lets the title screen run, then joins a room (no server reachable:
// the online code still runs every frame) and fails if the engine trapped (a Rust panic aborts in the browser and freezes
// the game) or the page threw.
import { createRequire } from "module";
const req = createRequire(process.env.PW_MODULES || import.meta.url);
const { chromium } = req("playwright");
const [url, chrome] = [process.argv[2], process.argv[3] || "/usr/bin/google-chrome"];
const b = await chromium.launch({ executablePath: chrome, args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"] });
const p = await b.newPage({ viewport: { width: 800, height: 450 } });
const bad = [];
p.on("console", (m) => { const t = m.text(); if (/RuntimeError|panicked|unreachable/.test(t)) bad.push(t.split("\n").slice(0, 12).join("\n")); });
p.on("pageerror", (e) => bad.push("page error: " + String(e)));
const sep = url.includes("?") ? "&" : "?";
await p.goto(url + sep + "relay=ws://127.0.0.1:9/ws&ws=ws://127.0.0.1:9/mqtt");
await p.waitForFunction(() => window.__fw && typeof window.__fw.web_test_join === "function", null, { timeout: 60000 });
await p.waitForTimeout(10000);
await p.evaluate(() => window.__fw.web_test_join(123456)).catch((e) => bad.push("join: " + String(e).split("\n").slice(0, 6).join("\n")));
await p.waitForTimeout(8000);
const room = await p.evaluate(() => window.__fw.web_room()).catch((e) => "error " + e);
await b.close();
if (bad.length || room !== 123456) { console.error(`smoke test FAILED (room ${room}):\n` + bad.slice(0, 2).join("\n---\n")); process.exit(1); }
console.log("smoke test passed: 18 s in the browser, title screen and an online room, no engine trap");
