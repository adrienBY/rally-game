// node web/tools/turn_test.mjs <url of turn_test.html> [chrome]: does the TURN server relay a data channel? (prints the result)
import { createRequire } from "module";
const { chromium } = createRequire(process.env.PW_MODULES || import.meta.url)("playwright");
const b = await chromium.launch({ executablePath: process.argv[3] || "/usr/bin/google-chrome" });
let fail = false;
for (const policy of ["relay", "all"]) {
  const p = await b.newPage(); await p.goto(process.argv[2] + "?policy=" + policy);
  await p.waitForFunction(() => window.__res && (window.__res.rtts.length >= 30 || window.__res.error), null, { timeout: 25000 }).catch(() => {});
  if (policy === "all") await p.waitForTimeout(4000);   // relayed candidates still gathered once the direct path works?
  const r = await p.evaluate(() => window.__res); const s = [...r.rtts].sort((x, y) => x - y);
  console.log(`${policy}: relayed candidates ${r.relayCands}, channel ${r.open ? "open after " + r.openMs + " ms" : "NOT open"}, round trips ${s.length ? "median " + s[s.length >> 1] + " ms, max " + s[s.length - 1] + " ms" : "none"} ${r.error}`);
  if (policy === "relay" && !r.open) fail = true; await p.close(); }
await b.close(); process.exit(fail ? 1 : 0);
