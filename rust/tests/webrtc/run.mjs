// node run.mjs <page url> <secs> [--no-mdns]: opens the harness in headless Chrome and prints window.__res at the end
import { createRequire } from "module"; const { chromium } = createRequire("/opt/npm-tools/node_modules/")("playwright");
const [url, secs] = [process.argv[2], +(process.argv[3] || 20)];
const args = process.argv.includes("--no-mdns") ? ["--disable-features=WebRtcHideLocalIpsWithMdns"] : [];
const b = await chromium.launch({ executablePath: "/usr/bin/google-chrome", args });
const p = await b.newPage(); p.on("console", (m) => { if (process.env.VERBOSE) console.log("page:", m.text()); });
await p.goto(url); await p.waitForTimeout(secs * 1000);
const r = await p.evaluate(() => window.__res); console.log(JSON.stringify(r)); await b.close();
