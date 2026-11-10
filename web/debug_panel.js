/* ================= DEBUG BUILD: live tuning panel ================= */
{
  const DEFAULTS = JSON.parse(JSON.stringify({ TUNE, SUSP: { K: SUSP.K, C: SUSP.C, KARB: SUSP.KARB }, VEH: { Iz: VEH.Iz } }));
  const SPEC = [
    ["Arcade", [
      ["TUNE", "arcade", "Arcade amount (0 = pure sim)", 0, 1, 0.01],
      ["TUNE", "aGrip", "Extra side grip", 0, 8, 0.1],
      ["TUNE", "aYaw", "Yaw follows steering", 0, 15, 0.1],
      ["TUNE", "aYawCap", "Max turn rate vs grip", 0.8, 3, 0.05],
      ["TUNE", "aBrake", "Extra braking (m/s2)", 0, 15, 0.1],
      ["TUNE", "aAccel", "Extra acceleration (m/s2)", 0, 10, 0.1],
      ["TUNE", "tc", "Traction control", 0, 1, 0.01],
      ["TUNE", "aHbYaw", "Handbrake rotation", 0, 8, 0.1],
    ]],
    ["Steering", [
      ["TUNE", "lock", "Max lock (rad)", 0.2, 1.3, 0.01],
      ["TUNE", "lockFall", "Lock falloff speed (m/s)", 10, 150, 1],
      ["TUNE", "steerRate", "Steer-in rate (rad/s)", 0.5, 10, 0.1],
      ["TUNE", "returnRate", "Return-to-centre rate", 0.5, 12, 0.1],
      ["TUNE", "rateFall", "Rate falloff speed (m/s)", 5, 150, 1],
      ["TUNE", "assist", "Countersteer assist", 0, 1.5, 0.05],
      ["TUNE", "csExtra", "Extra lock when countersteering", 0, 0.6, 0.01],
      ["TUNE", "csRate", "Countersteer speed x", 1, 4, 0.05],
    ]],
    ["Tyres", [
      ["TUNE", "gripF", "Front grip x", 0.6, 1.6, 0.01],
      ["TUNE", "gripR", "Rear grip x", 0.6, 1.6, 0.01],
      ["TUNE", "By", "Lateral stiffness (B)", 2, 16, 0.1],
      ["TUNE", "Cy", "Lateral curve shape (C)", 1.0, 1.9, 0.01],
      ["TUNE", "latKeep", "Side grip kept while spinning", 0, 1, 0.01],
      ["TUNE", "muRoad", "Gravel friction", 0.4, 1.5, 0.01],
      ["TUNE", "muGrass", "Grass friction", 0.3, 1.3, 0.01],
    ]],
    ["Drive + brakes", [
      ["TUNE", "power", "Engine power x", 0.3, 2.5, 0.05],
      ["TUNE", "split", "Torque to front (0 RWD, 1 FWD)", 0, 1, 0.01],
      ["TUNE", "brake", "Brake torque front (Nm)", 500, 5000, 50],
      ["TUNE", "brakeRear", "Rear brake ratio", 0.2, 1.2, 0.01],
      ["TUNE", "abs", "Anti-lock slip (0 = off)", 0, 0.5, 0.01],
      ["TUNE", "hb", "Handbrake torque (Nm)", 0, 6000, 50],
    ]],
    ["Chassis", [
      ["VEH", "Iz", "Yaw inertia (lower = twitchier)", 600, 4000, 10],
      ["SUSP", "K", "Spring rate (N/m)", 8000, 70000, 500],
      ["SUSP", "C", "Damping (Ns/m)", 300, 6000, 50],
      ["SUSP", "KARB", "Anti-roll bar (N/m)", 0, 40000, 500],
      ["TUNE", "camFollow", "Camera stiffness", 1, 40, 0.5],
    ]],
  ];
  const OBJ = { TUNE, SUSP, VEH };
  const KEY = MAPN === "forest" ? "fogwood-rally-tune" : "fogwood-rally-tune-" + MAPN;
  const st = document.createElement("style");
  st.textContent = `
    #tuneBtn { position: fixed; top: calc(6px + env(safe-area-inset-top, 0px)); left: 50%; transform: translateX(-50%); z-index: 20;
      font: 600 11px/1 ui-monospace, Menlo, monospace; letter-spacing: .08em; color: #f3cf62; background: rgba(8,12,8,.82);
      border: 1px solid rgba(243,207,98,.6); border-radius: 4px; padding: 7px 10px; cursor: pointer; }
    #tunePanel { position: fixed; z-index: 19; left: 50%; transform: translateX(-50%); top: calc(34px + env(safe-area-inset-top, 0px));
      width: min(360px, 46vw); max-height: calc(100% - 150px); overflow: auto; overscroll-behavior: contain; touch-action: pan-y;
      background: rgba(8,12,8,.9); color: #e9ece2; border: 1px solid rgba(233,236,226,.3); border-radius: 6px;
      font: 11px/1.35 ui-monospace, Menlo, monospace; padding: 8px 10px 10px; box-sizing: border-box; -webkit-user-select: text; user-select: text; }
    #tunePanel h3 { margin: 10px 0 4px; font-size: 10px; letter-spacing: .1em; text-transform: uppercase; color: #f3cf62; }
    #tunePanel .row { display: grid; grid-template-columns: 1fr auto; gap: 0 8px; align-items: center; margin: 3px 0; }
    #tunePanel .row label { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: #c9cfc0; }
    #tunePanel .row output { font-variant-numeric: tabular-nums; color: #e9ece2; }
    #tunePanel .row output.changed { color: #f3cf62; }
    #tunePanel input[type=range] { grid-column: 1 / -1; width: 100%; margin: 2px 0 4px; accent-color: #f3cf62; height: 22px; }
    #tunePanel .live { font-variant-numeric: tabular-nums; color: #9fb39a; white-space: pre; margin-bottom: 4px; }
    #tunePanel .btns { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 10px; }
    #tunePanel button { font: 600 11px/1 ui-monospace, Menlo, monospace; color: #e9ece2; background: rgba(233,236,226,.08);
      border: 1px solid rgba(233,236,226,.35); border-radius: 4px; padding: 8px 10px; cursor: pointer; }
    #tunePanel button.primary { color: #10140f; background: #f3cf62; border-color: #f3cf62; }
    #tunePanel textarea { width: 100%; box-sizing: border-box; min-height: 90px; margin-top: 8px; font: 10px/1.35 ui-monospace, Menlo, monospace;
      color: #e9ece2; background: rgba(0,0,0,.5); border: 1px solid rgba(233,236,226,.3); border-radius: 4px; padding: 6px; resize: vertical; }
    #tunePanel .note { color: #9fb39a; margin-top: 6px; min-height: 1.3em; }
    #tunePanel :focus-visible { outline: 2px solid #f3cf62; outline-offset: 1px; }`;
  st.id = "tuneStyle"; document.head.appendChild(st);
  const btn = document.createElement("button"); btn.id = "tuneBtn"; btn.type = "button"; btn.textContent = "TUNE ▾";
  const panel = document.createElement("div"); panel.id = "tunePanel";
  document.body.append(btn, panel);
  const live = document.createElement("div"); live.className = "live"; panel.appendChild(live);
  const fields = [];
  const fmtv = (v, step) => step >= 1 ? String(Math.round(v)) : v.toFixed(step >= 0.1 ? 1 : 2);
  for (const [group, rows] of SPEC) {
    const h = document.createElement("h3"); h.textContent = group; panel.appendChild(h);
    for (const [o, k, label, min, max, step] of rows) {
      const row = document.createElement("div"); row.className = "row";
      const id = "t_" + o + "_" + k;
      const lab = document.createElement("label"); lab.htmlFor = id; lab.textContent = label;
      const out = document.createElement("output");
      const inp = document.createElement("input"); inp.type = "range"; inp.id = id; inp.min = min; inp.max = max; inp.step = step;
      row.append(lab, out, inp); panel.appendChild(row);
      const f = { o, k, inp, out, step, def: DEFAULTS[o][k] };
      f.sync = () => { inp.value = OBJ[o][k]; out.value = fmtv(OBJ[o][k], step); out.classList.toggle("changed", Math.abs(OBJ[o][k] - f.def) > step / 2); };
      inp.addEventListener("input", () => { OBJ[o][k] = parseFloat(inp.value); f.sync(); save(); });
      inp.addEventListener("change", () => inp.blur());
      fields.push(f);
    }
  }
  const btns = document.createElement("div"); btns.className = "btns";
  const mk = (t, cls) => { const b = document.createElement("button"); b.type = "button"; b.textContent = t; if (cls) b.className = cls; btns.appendChild(b); return b; };
  const bExport = mk("Export settings", "primary"), bImport = mk("Import"), bReset = mk("Reset to defaults");
  panel.appendChild(btns);
  const ta = document.createElement("textarea"); ta.id = "tuneText"; ta.placeholder = "Exported settings appear here. Paste settings here and press Import."; ta.spellcheck = false;
  const note = document.createElement("div"); note.className = "note";
  panel.append(ta, note);
  const snapshot = () => { const v = { TUNE: {}, SUSP: {}, VEH: {} }; for (const f of fields) v[f.o][f.k] = +OBJ[f.o][f.k]; return v; };
  function exportText() {
    const v = snapshot(), changed = [];
    for (const f of fields) if (Math.abs(OBJ[f.o][f.k] - f.def) > f.step / 2) changed.push(f.o + "." + f.k);
    return JSON.stringify({ fogwoodRallyTune: 1, changed, ...v });
  }
  function apply(v) {
    for (const f of fields) { const x = v && v[f.o] && v[f.o][f.k]; if (typeof x === "number" && isFinite(x)) OBJ[f.o][f.k] = x; }
    fields.forEach(f => f.sync());
  }
  function save() { try { localStorage.setItem(KEY, JSON.stringify(snapshot())); } catch (_) {} }
  try { const s = localStorage.getItem(KEY); if (s) apply(JSON.parse(s)); } catch (_) {}
  fields.forEach(f => f.sync());
  bExport.addEventListener("click", () => {
    const t = exportText(); ta.value = t;
    const done = () => { note.textContent = "Copied. Paste it into the chat and I'll make it the default."; };
    const fallback = () => { ta.focus(); ta.select(); note.textContent = "Selected below. Copy it and paste it into the chat."; };
    try { navigator.clipboard.writeText(t).then(done, fallback); } catch (_) { fallback(); }
  });
  bImport.addEventListener("click", () => {
    try { apply(JSON.parse(ta.value)); save(); note.textContent = "Settings applied."; }
    catch (_) { note.textContent = "That text isn't valid settings JSON. Paste the whole exported block."; }
  });
  bReset.addEventListener("click", () => { apply(DEFAULTS); save(); note.textContent = "Back to defaults."; });
  let open = true;
  const setOpen = (v) => { open = v; panel.hidden = !v; btn.textContent = v ? "TUNE ▴" : "TUNE ▾"; };
  btn.addEventListener("click", () => { setOpen(!open); btn.blur(); });
  for (const el of [panel, btn]) for (const ev of ["pointerdown", "touchstart"]) el.addEventListener(ev, (e) => e.stopPropagation(), { passive: true });
  setOpen(true);
  const ivT = setInterval(() => {
    if (!open) return;
    const sp = Math.hypot(S.vx, S.vz), slip = sp > 2 ? wrapA(Math.atan2(S.vx, S.vz) - S.h) * 57.3 : 0;
    live.textContent = `speed ${(S.u * 3.6).toFixed(0).padStart(3)} km/h   gear ${S.gear < 0 ? "R" : S.gear}\n` +
      `steer ${(S.steer * 57.3).toFixed(1).padStart(5)}°  lock ${(steerLock(S.u) * 57.3).toFixed(1)}°\n` +
      `yaw ${(S.r * 57.3).toFixed(0).padStart(4)}°/s  slip ${slip.toFixed(1).padStart(5)}°  lat ${(S.ayF / 9.81).toFixed(2)} g\n` +
      `loads ${WH.map(w => (w.fz / 1000).toFixed(1)).join(" ")} kN  air ${S.contacts === 0 ? "yes" : "no"}`;
  }, 200);
  ABORT.signal.addEventListener("abort", () => clearInterval(ivT));
}
