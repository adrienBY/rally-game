// node web/tools/car_preview.mjs [out.png] [lowres-scale]
// Renders the car block of rally_template.html on its own (four views: chase, 3/4 front, side, rear 3/4), at the
// game's low resolution upscaled with nearest filtering plus one sharp view, and saves a screenshot.
import { readFileSync, writeFileSync, mkdirSync, copyFileSync } from "fs";
import { dirname, join } from "path";
import { fileURLToPath } from "url";
import { createRequire } from "module";
const require = createRequire("/opt/node22/lib/node_modules/");
const { chromium } = require("playwright");
const D = join(dirname(fileURLToPath(import.meta.url)), "..");
const src = readFileSync(join(D, "rally_template.html"), "utf8");
const cut = (a, b) => { const i = src.indexOf(a), j = src.indexOf(b, i); if (i < 0 || j < 0) throw new Error("marker " + a); return src.slice(i, j); };
const helpers = [cut("function hash2(", "/* ================= map:"), cut("function canvasTex(", "const leafTex"), cut("/* ================= pixel font", "/* ================= wind sway")].join("\n");
const carBlock = cut("/* ================= the car", "/* ================= dust");
const out = process.argv[2] || "/tmp/car_preview.png", only = process.argv[3], W = 220, H = 124, K = 3;
const dir = "/tmp/carprev"; mkdirSync(dir, { recursive: true });
copyFileSync(join(D, "../android/app/src/main/assets/three.module.min.js"), join(dir, "three.module.min.js"));
writeFileSync(join(dir, "index.html"), `<!doctype html><html><body style="margin:0;background:#283828">
<div id="grid" style="display:grid;grid-template-columns:repeat(2,${W * K}px);gap:4px"></div>
<script type="module">
import * as THREE from "./three.module.min.js";
THREE.ColorManagement.enabled = false;
const scene = new THREE.Scene();
scene.add(new THREE.HemisphereLight(0xc8d2cf, 0x2b3420, 2.3));
const sunL = new THREE.DirectionalLight(0xffefc8, 1.9); sunL.position.set(0.55, 0.75, 0.35); scene.add(sunL);
const ground = new THREE.Mesh(new THREE.PlaneGeometry(40, 40), new THREE.MeshLambertMaterial({ color: 0x8a7458 })); ground.rotation.x = -Math.PI / 2; ground.position.y = -0.075; scene.add(ground);
${helpers}
${carBlock}
console.log("PARTS " + JSON.stringify(car.body.children.filter(m => m.isMesh).map(m => [m.material.type, m.material.vertexColors ? "vc" : (m.material.color && m.material.color.getHexString()), m.geometry.attributes.position.count])));
for (const w of car.wheels) w.pivot.position.y = 0.235;   // suspension at rest, as in the game
const r = new THREE.WebGLRenderer({ antialias: false, preserveDrawingBuffer: true }); r.outputColorSpace = THREE.LinearSRGBColorSpace;
const grid = document.getElementById("grid");
const shot = (v, w, h, k) => { r.setSize(w, h, false); const cam = new THREE.PerspectiveCamera(46, w / h, 0.1, 100); cam.position.set(v[0], v[1], v[2]); cam.lookAt(v[3], v[4], v[5]);
  r.setClearColor(0x9fb0a8); r.render(scene, cam); const im = new Image(); im.src = r.domElement.toDataURL(); im.style.cssText = "width:" + w * k + "px;height:" + h * k + "px;image-rendering:pixelated"; grid.appendChild(im); };
const only = new URLSearchParams(location.search).get("m");
for (let m = 0; m < car.models.length; m++) { if (only !== null && +only !== m) continue; setCarModel(m);
  shot([-3.6, 1.9, -4.8, 0, 0.75, -0.2], ${W}, ${H}, ${K}); shot([3.7, 1.5, 4.0, 0, 0.7, 0.1], ${W * K}, ${H * K}, 1);
  if (only !== null) { shot([6.4, 0.9, 0, 0, 0.7, 0], ${W * K}, ${H * K}, 1); shot([0.01, 7.5, 0.01, 0, 0, 0], ${W * K}, ${H * K}, 1); } }
document.title = "done";
</script></body></html>`);
const b = await chromium.launch({ executablePath: "/usr/bin/google-chrome", args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"] });
const p = await b.newPage({ viewport: { width: W * K * 2 + 4, height: H * K * 5 + 20 } });
p.on("pageerror", e => console.log("pageerror", e.message)); p.on("console", m => { if (m.type() === "error" || m.text().startsWith("PARTS")) console.log(m.text()); });
await p.route("http://carprev/**", (rt) => { const f = join(dir, new URL(rt.request().url()).pathname); rt.fulfill({ body: readFileSync(f), contentType: f.endsWith(".js") ? "text/javascript" : "text/html" }); });
await p.goto("http://carprev/index.html" + (only !== undefined ? "?m=" + only : "")); await p.waitForFunction(() => document.title === "done", null, { timeout: 60000 });
await p.screenshot({ path: out, fullPage: true }); await b.close(); console.log("saved", out);
