"use strict";
// ---------- helpers ----------
const $ = (s, r = document) => r.querySelector(s);
const api = async (p, opt) => { const r = await fetch("/api" + p, opt); if (!r.ok) throw new Error((await r.text()) || r.status); return r.json(); };
const esc = s => String(s ?? "").replace(/[&<>"]/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));
const fB = n => { n = +n || 0; return n >= 1e9 ? (n / 1e9).toFixed(2) + " GB" : n >= 1e6 ? (n / 1e6).toFixed(1) + " MB" : n >= 1e3 ? (n / 1e3).toFixed(0) + " kB" : n + " B"; };
const fD = s => { s = Math.round(+s || 0); const h = Math.floor(s / 3600), m = Math.floor(s % 3600 / 60); return h ? `${h}h ${String(m).padStart(2, "0")}m` : `${m}m ${String(s % 60).padStart(2, "0")}s`; };
const fKm = m => ((+m || 0) / 1000).toFixed(1) + " km";
const fKmh = v => ((+v || 0) * 3.6).toFixed(1) + " km/h";
const fDate = t => t ? new Date(t * 1000).toLocaleString("en-GB", { dateStyle: "medium", timeStyle: "short" }) : "–";
const fDay = t => t ? new Date(t * 1000).toLocaleDateString("en-GB", { dateStyle: "medium" }) : "–";
const pct = (a, b) => b ? Math.round(a / b * 100) : 0;
const kpi = (l, v, s = "") => `<div class="card kpi"><div class="l">${l}</div><div class="v">${v}</div><div class="s">${s}</div></div>`;
const card = (t, body, cls = "") => `<div class="card ${cls}"><h4>${t}</h4>${body}</div>`;
const kv = o => `<div class="kv">${Object.entries(o).filter(([, v]) => v !== null && v !== undefined && v !== "").map(([k, v]) => `<div>${esc(k)}</div><div>${typeof v === "object" && v && v.html ? v.html : esc(v)}</div>`).join("")}</div>`;
const COLORS = ["#2f9bff", "#7c5cff", "#2ecc8f", "#f5b83d", "#ff5c6c", "#35d0e0", "#e46bd6", "#9aa7bd"];
const cache = {};
const get = (key, sec) => (cache[key + sec] ??= api(`/devices/${encodeURIComponent(key)}/${sec}`));

// ---------- charts (canvas, dependency free) ----------
function setup(c, h) { const dpr = devicePixelRatio || 1, w = c.clientWidth || 600; c.width = w * dpr; c.height = h * dpr; c.style.height = h + "px"; const x = c.getContext("2d"); x.scale(dpr, dpr); x.clearRect(0, 0, w, h); return [x, w, h]; }
function lineChart(c, xs, series, { h = 180, unit = "", xfmt = v => v, fill = true } = {}) {
  const [x, w] = setup(c, h); const L = 44, R = 10, T = 10, B = 22;
  const vals = series.flatMap(s => s.data.filter(v => v != null)); if (!vals.length || xs.length < 2) { x.fillStyle = "#8a99b3"; x.fillText("No data", 10, 20); return; }
  let lo = Math.min(...vals), hi = Math.max(...vals); if (lo === hi) { lo -= 1; hi += 1; } const pad = (hi - lo) * .08; lo -= pad; hi += pad;
  const x0 = xs[0], x1 = xs[xs.length - 1], X = v => L + (v - x0) / ((x1 - x0) || 1) * (w - L - R), Y = v => T + (1 - (v - lo) / (hi - lo)) * (h - T - B);
  x.strokeStyle = "#233047"; x.fillStyle = "#8a99b3"; x.font = "11px Segoe UI"; x.lineWidth = 1;
  for (let i = 0; i <= 4; i++) { const v = lo + (hi - lo) * i / 4, y = Y(v); x.beginPath(); x.moveTo(L, y); x.lineTo(w - R, y); x.stroke(); x.fillText(v.toFixed(Math.abs(hi) < 10 ? 1 : 0) + unit, 2, y + 4); }
  for (let i = 0; i <= 4; i++) { const v = x0 + (x1 - x0) * i / 4; x.fillText(xfmt(v), Math.min(X(v) - 14, w - 48), h - 6); }
  series.forEach((s, k) => { const col = s.color || COLORS[k]; x.beginPath(); let st = false; xs.forEach((xv, i) => { const v = s.data[i]; if (v == null) { st = false; return; } st ? x.lineTo(X(xv), Y(v)) : x.moveTo(X(xv), Y(v)); st = true; }); x.strokeStyle = col; x.lineWidth = 1.8; x.stroke();
    if (fill && series.length === 1) { x.lineTo(X(x1), Y(lo)); x.lineTo(X(x0), Y(lo)); x.fillStyle = col + "22"; x.fill(); } });
}
function barChart(c, labels, values, { h = 170, color = "#2f9bff", fmt = v => v } = {}) {
  const [x, w] = setup(c, h); const L = 8, B = 24, T = 18, n = values.length; if (!n) return; const mx = Math.max(...values, 1), bw = (w - L * 2) / n;
  x.font = "11px Segoe UI"; const step = Math.ceil(n / Math.floor(w / 46));
  values.forEach((v, i) => { const bh = v / mx * (h - T - B), bx = L + i * bw + bw * .15; const g = x.createLinearGradient(0, h - B - bh, 0, h - B); g.addColorStop(0, color); g.addColorStop(1, color + "55"); x.fillStyle = g; x.beginPath(); x.roundRect?.(bx, h - B - bh, bw * .7, bh, 3); x.fill();
    x.fillStyle = "#8a99b3"; if (i % step === 0) x.fillText(labels[i], bx - 2, h - 8); if (n <= 14 && v) { x.fillStyle = "#c8d4e8"; x.fillText(fmt(v), bx, h - B - bh - 4); } });
}

// ---------- maps ----------
const OSM = "https://tiles.openfreemap.org/styles/positron"; // OpenFreeMap vector basemap
function newMap(el, opt = {}) { if (typeof maplibregl === "undefined") { el.innerHTML = '<div class="empty">Map library could not load (offline?)</div>'; return null; } return new maplibregl.Map({ container: el, style: OSM, center: [5, 52], zoom: 5, attributionControl: { compact: true }, ...opt }); }
function fit(map, b, pad = 40) { map.fitBounds([[b[0], b[1]], [b[2], b[3]]], { padding: pad, duration: 0, maxZoom: 16 }); }

// ---------- app state ----------
let devices = [], detection = null, current = null, tab = "overview";
const TABS = [["overview", "Overview"], ["battery", "Battery"], ["storage", "Storage"], ["maps", "Maps"], ["routing", "Routing"], ["sensors", "Sensors"], ["routes", "Routes"], ["plans", "Workouts"], ["alerts", "Alerts"], ["history", "History"], ["stats", "Statistics"], ["debug", "Technical"]];

async function refreshStatus() {
  try {
    detection = await api("/status"); const el = $("#conn"), s = detection.status;
    el.className = "pill " + ({ online: "pill-green", usb_no_mtp: "pill-amber", open_failed: "pill-red", absent: "pill-grey" }[s] || "pill-grey");
    el.textContent = ({ online: "● ", usb_no_mtp: "⚠ ", open_failed: "✖ ", absent: "○ " }[s] || "") + detection.message; el.title = detection.message;
    $("#syncBtn").disabled = s !== "online";
    $("#syncBtn").title = s === "online" ? "Read the device over USB" : detection.message;
  } catch { $("#conn").textContent = "Server not reachable"; $("#conn").className = "pill pill-red"; }
}
async function loadDevices() {
  const r = await api("/devices"); devices = r.devices; detection = r.detection; renderDevices();
  const h = location.hash.split("/"); const key = decodeURIComponent(h[1] || "");
  const pick = devices.find(d => d.key === key) || devices[0];
  if (pick) openDevice(pick.key, h[2] || tab, h[3]); else $("#content").innerHTML = emptyState();
}
function emptyState() {
  const d = detection; const msg = d?.status === "usb_no_mtp" ? `<p class="warn">${esc(d.message)}</p>` : "";
  return `<div class="empty"><h2>No Wahoo synced yet</h2>${msg}<p>Connect your ELEMNT with a USB data cable, switch it on and press <b>Sync device</b>.</p></div>`;
}
function renderDevices() {
  $("#devices").innerHTML = devices.map(d => {
    const st = d.storage, used = pct(st.used, st.capacity);
    return `<div class="dev ${current === d.key ? "active" : ""}" data-key="${esc(d.key)}"><h3><span class="dot ${d.online ? "on" : ""}"></span>${esc(d.name)}</h3>
    <div class="sub">${esc(d.descriptor || d.model)} · FW ${esc(d.firmware)}</div>
    <div class="row"><span>🔋 Battery</span><b>${d.power_level != null ? d.power_level + "%" : "–"}</b></div>
    <div class="bar"><i style="width:${d.power_level || 0}%;background:${(d.power_level || 0) < 20 ? "var(--red)" : "var(--green)"}"></i></div>
    <div class="row"><span>💾 ${fB(st.free)} free</span><b>${used}% used</b></div>
    <div class="bar"><i style="width:${used}%;background:var(--accent)"></i></div>
    <div class="row"><span>${d.activities} rides</span><span>${d.online ? "Online" : "Cached"}</span></div></div>`;
  }).join("") || `<div class="empty">No devices</div>`;
  document.querySelectorAll(".dev").forEach(e => e.onclick = () => openDevice(e.dataset.key, tab));
}
function openDevice(key, t = "overview", id) {
  current = key; tab = TABS.some(x => x[0] === t) ? t : "overview"; location.hash = `#/${encodeURIComponent(key)}/${tab}`; renderDevices();
  $("#content").innerHTML = `<div class="tabs">${TABS.map(([k, n]) => `<div class="tab ${k === tab ? "active" : ""}" data-t="${k}">${n}</div>`).join("")}</div><div id="pane"><div class="empty">Loading…</div></div>`;
  document.querySelectorAll(".tab").forEach(e => e.onclick = () => openDevice(key, e.dataset.t));
  window.scrollTo(0, 0);
  const fn = VIEWS[tab]; Promise.resolve(fn(key, $("#pane"))).catch(e => $("#pane").innerHTML = `<div class="empty">Error: ${esc(e.message)}</div>`);
  if (id) showActivity(key, id);
}

// ---------- map packs (download) ----------
let lastJob = 0; // start time of the pack job whose result already triggered a sync
async function mapPacks(key) {
  const box = $("#mpk"); if (!box) return;
  let u; try { u = await get(key, "mappacks"); } catch (e) { box.textContent = "Could not load packs: " + e.message; return; }
  if (u.status !== "ok") { box.textContent = u.message; return; }
  let all = false;
  const node = (p, d) => {
    if (!all && !p.installed_count) return "";
    const kids = p.children.map(c => node(c, d + 1)).join("");
    const has = p.installed_version != null;
    const stale = has && p.installed_version < u.latest_version;
    const opts = (u.versions || [u.latest_version]).slice().reverse().map(v => `<option value="${v}" ${v === (has ? p.installed_version : u.latest_version) ? "selected" : ""}>v${v}${v === p.installed_version ? " (installed)" : v === u.latest_version ? " (newest)" : ""}</option>`).join("");
    const act = p.downloadable ? `<select class="mpk-ver" data-id="${p.id}">${opts}</select> <button class="btn mpk-inst ${has ? "" : "primary"}" data-id="${p.id}" data-name="${esc(p.name)}" data-cur="${has ? p.installed_version : ""}">${has ? "Switch" : "Install"}</button>${has ? ` <button class="btn mpk-rm" data-id="${p.id}" data-name="${esc(p.name)}">Remove</button>` : ""}` : "";
    return `<tr><td style="padding-left:${d * 18 + 4}px">${esc(p.name)}</td><td class="num" style="${stale ? "color:#f5a623" : ""}">${has ? "v" + p.installed_version : ""}</td><td class="num">${has ? esc(p.installed_date || "") : ""}</td><td class="num">${p.downloadable ? p.tiles : ""}</td><td class="num">${fB(p.size_bytes)}</td><td class="num" style="white-space:nowrap">${act}</td></tr>${kids}`;
  };
  const instState = async () => {
    const s = await api("/mapinstall").catch(() => null), el = $("#mpk-job"); if (!s || !el) return;
    const p = s.bytes_total ? Math.min(100, Math.round(s.bytes_done / s.bytes_total * 100)) : (s.tiles_total ? Math.round(s.tiles_done / s.tiles_total * 100) : 0);
    const what = s.action === "remove" ? "Removing" : "Installing";
    if (!s.running && (!s.started || Date.now() / 1000 - s.finished_at > 600)) return;
    if (s.running) {
      const el_s = Date.now() / 1000 - s.started, eta = p > 3 ? Math.round(el_s * (100 - p) / p) : null;
      el.innerHTML = `<b>${what} ${esc(s.pack_name)}${s.version ? " (v" + s.version + ")" : ""}: ${p}%</b> · tile ${s.tiles_done}/${s.tiles_total} ${esc(s.current)}${eta != null ? " · about " + Math.ceil(eta / 60) + " min left" : ""} <button class="btn" id="mpk-cancel">Cancel</button><div class="bar" style="height:14px"><i style="width:${p}%;background:var(--accent)"></i></div><div class="note">Keep the USB cable connected.</div>`;
      $("#mpk-cancel").onclick = () => api("/mapinstall/cancel", { method: "POST" }).catch(() => {});
      box.querySelectorAll(".mpk-inst,.mpk-rm").forEach(b => b.disabled = true);
      setTimeout(instState, 700);
    } else if (s.error) {
      el.innerHTML = `<b style="color:#e5484d">✖ ${esc(s.pack_name)}: ${esc(s.error)}</b>${s.failed_tile ? `<br>Failed at tile ${esc(s.failed_tile)}; a failed replacement is restored automatically when possible (see the message).` : ""}`;
      if (lastJob !== s.started) { lastJob = s.started; $("#syncBtn").click(); }
    } else if (s.finished) {
      el.innerHTML = `<b style="color:#2ecc8f">✓ ${esc(s.pack_name)}: ${s.written} new, ${s.replaced} replaced, ${s.removed} removed, ${s.skipped} unchanged.</b> <b>Restart the ELEMNT</b> to use the maps.${s.backup ? ` <span class="note">Backup: <code>${esc(s.backup)}</code></span>` : ""}`;
      if (lastJob !== s.started) { lastJob = s.started; $("#syncBtn").click(); }
    }
  };
  const start = async (id, name, ver, cur) => {
    const q = ver === "remove" ? "remove" : ver, el = $("#mpk-job");
    el.innerHTML = `<b>⏳ Calculating plan for ${esc(name)}…</b> checking tile sizes on the server, this can take up to a minute.<div class="bar" style="height:14px"><i style="width:40%;background:var(--accent);animation:mpkpulse 1.2s ease-in-out infinite alternate"></i></div>`;
    box.querySelectorAll(".mpk-inst,.mpk-rm").forEach(b => b.disabled = true);
    const done = () => { box.querySelectorAll(".mpk-inst,.mpk-rm").forEach(b => b.disabled = false); box.querySelectorAll(".mpk-ver").forEach(s => s.onchange()); };
    let pl; try { pl = await api(`/devices/${encodeURIComponent(key)}/mappacks/${id}/plan?version=${q}`); } catch (e) { el.textContent = "Plan failed: " + e.message; done(); return; }
    if (pl.nothing_to_do) { el.textContent = `${name} is already on v${ver}.`; done(); return; }
    const c = pl.counts, min = Math.max(1, Math.ceil(pl.estimated_seconds / 60));
    const title = ver === "remove" ? `Remove ${name} from the device` : cur ? `Switch ${name} from v${cur} to v${ver}` : `Install ${name} v${ver}`;
    const row = (k, v) => `<tr><td>${k}</td><td class="num">${v}</td></tr>`;
    el.innerHTML = `<div class="card" style="margin:8px 0;border:1px solid var(--accent)"><h4 style="margin-top:0">${esc(title)}</h4>
      <table style="max-width:460px">${row("Tiles", `${c.write} new · ${c.replace} replaced · ${c.remove} removed · ${c.skip} unchanged${c.kept_shared ? " · " + c.kept_shared + " shared (kept)" : ""}`)}${row("Download", fB(pl.download_bytes))}${row("Device space", `${pl.net_bytes < 0 ? "−" : "+"}${fB(Math.abs(pl.net_bytes))} (${fB(pl.free_bytes)} free)`)}${row("Duration", `about ${min} min`)}</table>
      ${pl.warnings.map(w => `<div class="warn" style="margin-top:8px">⚠ ${esc(w)}</div>`).join("")}
      ${pl.enough_space ? "" : `<div class="warn" style="margin-top:8px;color:#e5484d"><b>Not enough free space on the device.</b></div>`}
      <div class="note">Keep the USB cable connected; restart the ELEMNT afterwards.</div>
      <div style="margin-top:8px"><button class="btn primary" id="mpk-go" ${pl.enough_space ? "" : "disabled"}>${ver === "remove" ? "Remove pack" : cur ? "Switch now" : "Install now"}</button> <button class="btn" id="mpk-no">Cancel</button></div></div>`;
    $("#mpk-no").onclick = () => { el.textContent = ""; done(); };
    $("#mpk-go").onclick = async () => {
      $("#mpk-go").disabled = true; $("#mpk-no").disabled = true;
      try { await api(`/devices/${encodeURIComponent(key)}/mappacks/${id}/install?version=${q}`, { method: "POST" }); instState(); }
      catch (e) { el.textContent = "Could not start: " + e.message; done(); }
    };
  };
  const draw = () => {
    const byVer = {};
    const collect = p => { if (p.installed_version != null) { const e = byVer[p.installed_version] ??= { n: 0, date: p.installed_date }; e.n++; } p.children.forEach(collect); };
    u.packs.forEach(collect);
    const summary = Object.keys(byVer).sort((a, b) => b - a).map(v => `v${v}${byVer[v].date ? " (" + esc(byVer[v].date) + ")" : ""}: ${byVer[v].n}`).join(", ");
    box.innerHTML = `<div class="note" style="margin-top:0">${u.installed_total} packs installed: ${summary || "none"}. Newest available: v${u.latest_version}${u.latest_date ? " (" + esc(u.latest_date) + ")" : ""}. Pick a version and press Install / Switch to write a pack to the device,     or Remove to delete it.
         <label style="float:right"><input type="checkbox" id="mpk-all" ${all ? "checked" : ""}> show all packs</label></div>
             <div id="mpk-job" class="note"></div>
             <div class="scroll" style="max-height:420px"><table><tr><th>Pack</th><th class="num">Version</th><th class="num">Map date</th><th class="num">Tiles</th><th class="num">Size</th><th></th></tr>${u.packs.map(p => node(p, 0)).join("")}</table></div>`;
           $("#mpk-all").onchange = e => { all = e.target.checked; draw(); };
           const ver = id => box.querySelector(`.mpk-ver[data-id="${id}"]`).value;
           box.querySelectorAll(".mpk-inst").forEach(b => b.onclick = () => start(b.dataset.id, b.dataset.name, ver(b.dataset.id), b.dataset.cur));
           box.querySelectorAll(".mpk-rm").forEach(b => b.onclick = () => start(b.dataset.id, b.dataset.name, "remove", ""));
           const sync = sel => {
             const b = box.querySelector(`.mpk-inst[data-id="${sel.dataset.id}"]`);
             if (b && b.dataset.cur) b.disabled = sel.value === b.dataset.cur;
           };
           box.querySelectorAll(".mpk-ver").forEach(s => { sync(s); s.onchange = () => sync(s); });
           instState();
     };
     draw();
}

// ---------- views ----------
const VIEWS = {
  async overview(key, el) {
    const [info, bat, st, stats, al, maps] = await Promise.all(["info", "battery", "storage", "stats", "alerts", "maps"].map(s => get(key, s)));
    const dev = devices.find(d => d.key === key);
    el.innerHTML = `<div class="grid g4">
      ${kpi("Firmware", esc(info.firmware), `Hardware rev ${esc(info.hardware || "–")}`)}
      ${kpi("Battery", bat.level != null ? bat.level + "%" : "–", bat.estimated_hours ? `≈ ${bat.estimated_hours.toFixed(0)} h riding` : "live over USB")}
      ${kpi("Storage free", fB(st.free), `${fB(st.used)} of ${fB(st.capacity)} used`)}
      ${kpi("Map tiles", maps.tile_count, fB(maps.tile_bytes))}
      ${kpi("Rides", stats.rides, `${stats.active_days} active days`)}
      ${kpi("Distance", Math.round(stats.distance / 1000) + " km", `${(stats.moving_time / 3600).toFixed(0)} h moving`)}
      ${kpi("Ascent", Math.round(stats.ascent).toLocaleString() + " m")}
      ${kpi("Alerts", al.items.length, al.items.filter(a => a.level !== "info").length + " need attention")}</div>
    <div class="grid g2" style="margin-top:14px">
      ${card("Device", kv({ Name: info.name, Model: info.descriptor || info.model, Manufacturer: info.manufacturer, "Serial number": info.serial, "Factory id": info.factory_id, Firmware: info.firmware, Hardware: info.hardware, "First ride": fDay(info.first_activity), "Last ride": fDay(info.last_activity), "Last sync": fDate(Date.parse(info.synced_at) / 1000), Connection: dev?.online ? "Online (USB / MTP)" : "Offline – showing cached data" }))}
      ${card("Where you ride", `<div id="ovmap" class="map"></div>`)}</div>
    <div class="grid g2" style="margin-top:14px">${card("Attention", al.items.map(alertHtml).join("") || '<div class="empty">All good ✓</div>')}${card("Distance per month", `<canvas id="ovm"></canvas>`)}</div>`;
    get(key, "firmware").then(f => {
      const sub = [...el.querySelectorAll(".kpi")].find(k => k.textContent.includes("Firmware"))?.querySelector(".s");
      if (!sub) return;
      const icon = { up_to_date: "✓ ", update_available: "⬆ ", newer: "✓ " }[f.status] || "";
      sub.textContent = icon + f.message;
      if (f.status === "update_available") sub.style.color = "#f5a623"; else if (f.status === "up_to_date" || f.status === "newer") sub.style.color = "#2ecc8f";
    }).catch(() => {});
    const m = stats.by_month.slice(-24); barChart($("#ovm"), m.map(x => x.month.slice(2)), m.map(x => x.distance / 1000), { fmt: v => Math.round(v) });
    const map = newMap($("#ovmap")); if (map && stats.points.length) { map.on("load", () => { map.addSource("p", { type: "geojson", data: { type: "FeatureCollection", features: stats.points.map(p => ({ type: "Feature", geometry: { type: "Point", coordinates: [p[1], p[0]] } })) } }); map.addLayer({ id: "p", type: "circle", source: "p", paint: { "circle-radius": 5, "circle-color": "#2f9bff", "circle-opacity": .35, "circle-stroke-width": 1, "circle-stroke-color": "#7c5cff" } }); const lons = stats.points.map(p => p[1]), lats = stats.points.map(p => p[0]); fit(map, [Math.min(...lons), Math.min(...lats), Math.max(...lons), Math.max(...lats)]); }); }
  },
  async battery(key, el) {
    const b = await get(key, "battery"), info = await get(key, "info");
    el.innerHTML = `<div class="grid g4">${kpi("Level now", b.level != null ? b.level + "%" : "–", "read over USB (WPD power level)")}${kpi("Drain while riding", b.drain_per_hour ? b.drain_per_hour.toFixed(2) + " %/h" : "–", "from battery_start → battery_end per ride")}${kpi("Estimated runtime", b.estimated_hours ? b.estimated_hours.toFixed(1) + " h" : "–", "on a full charge")}${kpi("Data points", b.history.length, "start/end of every ride")}</div>
    <div style="margin-top:14px">${card("Battery level at ride start / end", `<canvas id="bc"></canvas><div class="note">Each ride records the device battery percentage. Jumps up between rides are charges.</div>`)}</div>`;
    const h = b.history; lineChart($("#bc"), h.map(x => x[0]), [{ data: h.map(x => x[1]), color: "#2ecc8f" }], { h: 260, unit: "%", xfmt: v => new Date(v * 1000).toLocaleDateString("en-GB", { month: "short", year: "2-digit" }) });
  },
  async storage(key, el) {
    const s = await get(key, "storage"); const cols = {}; s.categories.forEach((c, i) => cols[c.name] = COLORS[i % COLORS.length]);
    el.innerHTML = `<div class="grid g4">${kpi("Capacity", fB(s.capacity), esc(s.name) + " · " + esc(s.fs))}${kpi("Used", fB(s.used), pct(s.used, s.capacity) + "%")}${kpi("Free", fB(s.free), pct(s.free, s.capacity) + "%")}${kpi("Unaccounted", fB(s.unaccounted), "system, routing graphs")}</div>
    <div style="margin-top:14px">${card("Usage breakdown", `<div class="bar" style="height:22px">${s.categories.map(c => `<i title="${esc(c.name)} ${fB(c.bytes)}" style="width:${c.bytes / s.capacity * 100}%;background:${cols[c.name]}"></i>`).join("")}<i title="Unaccounted" style="width:${s.unaccounted / s.capacity * 100}%;background:#44506a"></i></div>
    <div class="legend">${s.categories.map(c => `<span><i style="background:${cols[c.name]}"></i>${esc(c.name)} ${fB(c.bytes)}</span>`).join("")}<span><i style="background:#44506a"></i>unaccounted ${fB(s.unaccounted)}</span></div>
    <table><tr><th>Folder</th><th class="num">Files</th><th class="num">Size</th><th class="num">% of used</th></tr>${s.categories.map(c => `<tr><td>${esc(c.name)}</td><td class="num">${c.files}</td><td class="num">${fB(c.bytes)}</td><td class="num">${pct(c.bytes, s.used)}%</td></tr>`).join("")}</table><div class="note">${esc(s.note)}</div>`)}</div>`;
  },
  async maps(key, el) {
    const m = await get(key, "maps");
    el.innerHTML = `<div class="grid g4">${kpi("Tiles installed", m.tile_count, fB(m.tile_bytes))}${kpi("Regions", m.regions.length, "covered areas")}${kpi("Map versions", Object.keys(m.versions).join(", ") || "–", Object.entries(m.versions).map(([v, n]) => { const d = (m.version_dates || {})[v]; const r = d ? (fDay(d.oldest) === fDay(d.newest) ? fDay(d.oldest) : `${fDay(d.oldest)} – ${fDay(d.newest)}`) : ""; return `v${v}: ${n}${r ? " (" + r + ")" : ""}`; }).join(" · ") + (Object.keys(m.version_dates || {}).length ? "" : " · date unknown"))}</div>
    ${m.warnings.map(w => `<div class="warn" style="margin-top:10px">⚠ ${esc(w)}${w.startsWith("maps/temp holds") ? ` <button class="btn" id="cleanTemp" style="margin-left:8px">Clean up</button><span id="cleanMsg" class="note"></span>` : ""}</div>`)    .join("")}
        <div class="grid g2" style="margin-top:14px"><div class="card" style="grid-column:1/-1"><h4>Coverage map <span style="float:right;text-transform:none">Colour by <select id="mc"><option value="size">size</option><option value="version">version</option><option value="marker">missing marker</option></select></span></h4><div id="mmap" class="map tall"></div><div class="legend" id="mleg"></div></div>
        <div class="card" style="grid-column:1/-1"><h4>Map packs</h4><div id="mpk" class="note" style="margin-top:0">Loading pack list…</div></div></div>`;
        const ct = $("#cleanTemp");
        if (ct) ct.onclick = async () => {
          if (!confirm("Delete the leftover files in maps/temp on the device?\n\nThe ELEMNT re-creates a render copy when it needs one. Keep the USB cable connected.")) return;
          ct.disabled = true; $("#cleanMsg").textContent = "Deleting…";
          try { const r = await api(`/devices/${encodeURIComponent(key)}/maps/clean`, { method: "POST" }); $("#cleanMsg").textContent = `Deleted ${r.deleted} file(s), ${fB(r.bytes)} freed. Syncing…`; $("#syncBtn").click(); }
          catch (e) { $("#cleanMsg").textContent = "Failed: " + e.message; ct.disabled = false; }
        };
    mapPacks(key);
    const map = newMap($("#mmap"), { zoom: 2, center: [10, 30] }); if (!map) return;
    const sizeColor = ["step", ["get", "bytes"], "#1d4e89", 200000, "#2f9bff", 1e6, "#7c5cff", 4e6, "#e46bd6", 1.2e7, "#ff5c6c"];
    const colorExpr = { size: sizeColor, version: ["match", ["to-string", ["get", "version"]], "18", "#2ecc8f", "12", "#2f9bff", "null", "#ff5c6c", "#f5b83d"], marker: ["case", ["==", ["get", "version"], null], "#ff5c6c", "#2f9bff"] };
    const leg = { size: [["#1d4e89", "<200 kB"], ["#2f9bff", "<1 MB"], ["#7c5cff", "<4 MB"], ["#e46bd6", "<12 MB"], ["#ff5c6c", "≥12 MB"]], version: [["#2ecc8f", "v18"], ["#2f9bff", "v12"], ["#f5b83d", "other"]], marker: [["#2f9bff", "has version marker"], ["#ff5c6c", "no marker"]] };
    const draw = () => $("#mleg").innerHTML = leg[$("#mc").value].map(([c, l]) => `<span><i style="background:${c}"></i>${l}</span>`).join("");
    map.on("load", () => {
      const feats = m.tiles.map(t => ({ type: "Feature", properties: { ...t, bbox: undefined, version: t.version }, geometry: { type: "Polygon", coordinates: [[[t.bbox[0], t.bbox[1]], [t.bbox[2], t.bbox[1]], [t.bbox[2], t.bbox[3]], [t.bbox[0], t.bbox[3]], [t.bbox[0], t.bbox[1]]]] } }));
      map.addSource("t", { type: "geojson", data: { type: "FeatureCollection", features: feats } });
      map.addLayer({ id: "tf", type: "fill", source: "t", paint: { "fill-color": colorExpr.size, "fill-opacity": .55 } });
      map.addLayer({ id: "tl", type: "line", source: "t", paint: { "line-color": "#ffffff", "line-opacity": .25, "line-width": .6 } });
      const b = [180, 90, -180, -90]; m.tiles.forEach(t => { b[0] = Math.min(b[0], t.bbox[0]); b[1] = Math.min(b[1], t.bbox[1]); b[2] = Math.max(b[2], t.bbox[2]); b[3] = Math.max(b[3], t.bbox[3]); }); if (m.tiles.length) fit(map, b, 30);
      $("#mc").onchange = () => { map.setPaintProperty("tf", "fill-color", colorExpr[$("#mc").value]); draw(); }; draw();
      map.on("click", "tf", e => { const p = e.features[0].properties; new maplibregl.Popup().setLngLat(e.lngLat).setHTML(`<b>Tile z${p.z}/${p.x}/${p.y}</b><br>${esc(p.region)}<br>${fB(p.bytes)}<br>version: ${p.version == "null" || p.version == null ? "–" : p.version}<br>made/installed: ${p.modified && p.modified != "null" ? fDay(+p.modified) : "unknown"}`).addTo(map); });
      map.on("mouseenter", "tf", () => map.getCanvas().style.cursor = "pointer"); map.on("mouseleave", "tf", () => map.getCanvas().style.cursor = "");
    });
  },
  async routing(key, el) {
    const r = await get(key, "routing"), c = r.config || {}, g = r.graph_tiles, l2 = g?.levels.find(l => l.level === 2);
    el.innerHTML = `<div class="grid g4">${kpi("Graph tiles", g ? g.tiles.toLocaleString() : "–", "listed in maps/routing_md5")}${kpi("Local-road tiles", l2 ? l2.tiles.toLocaleString() : "–", "level 2, 0.25° tiles")}${kpi("Checksum list", r.checksums ? fB(r.checksums.bytes) : "–", "maps/routing_md5" + (r.checksums?.modified ? " · " + fDay(r.checksums.modified) : ""))}${kpi("Service config", r.config_file ? fB(r.config_file.bytes) : "–", "routing-config.json" + (r.config_file?.modified ? " · " + fDay(r.config_file.modified) : ""))}</div>
    ${r.warnings.map(w => `<div class="warn" style="margin-top:10px">⚠ ${esc(w)}</div>`).join("")}
    <div class="grid g2" style="margin-top:14px">
      <div class="card" style="grid-column:1/-1"><h4>Routing coverage (1° cells, colour = number of local-road tiles)</h4><div id="rmap" class="map tall"></div></div>
      ${card("Graph tiles per level", `<table><tr><th>Level</th><th>Meaning</th><th class="num">Tiles</th></tr>${(g?.levels || []).map(l => `<tr><td>${l.level}</td><td>${esc(l.name || "")}</td><td class="num">${l.tiles.toLocaleString()}</td></tr>`).join("") || '<tr><td colspan="3">no tile list synced yet</td></tr>'}</table><div class="note">${esc(r.layout)}</div>`)}
      ${card("Valhalla service", kv({ Listen: c.httpd?.service?.listen, "Search cutoff": c.loki?.service_defaults?.search_cutoff != null ? c.loki.service_defaults.search_cutoff + " m" : null, "Node snap tolerance": c.loki?.service_defaults?.node_snap_tolerance, "Loki actions": (c.loki?.actions || []).join(", "), "Map-matching (meili) profiles": Object.keys(c.meili || {}).join(", "), "Leftover temp files": r.temp.files ? `${r.temp.files} (${fB(r.temp.bytes)})` : "none" }) + `<div class="note">The ELEMNT runs an embedded <a href="https://valhalla.github.io/valhalla/" target="_blank">Valhalla</a> routing engine that reads these graph tiles to calculate routes and re-route on the device.</div>`)}
      <div class="card" style="grid-column:1/-1"><h4>Routing data of installed map packs (from Wahoo's map server)</h4><div id="rpk" class="note" style="margin-top:0">Loading…</div></div></div>
    <details style="margin-top:14px"><summary>routing-config.json</summary><pre>${esc(JSON.stringify(c, null, 1))}</pre></details>`;
    const rm = newMap($("#rmap"), { zoom: 2, center: [10, 30] });
    if (rm && g?.coverage.length) rm.on("load", () => {
      rm.addSource("r", { type: "geojson", data: { type: "FeatureCollection", features: g.coverage.map(t => ({ type: "Feature", properties: { tiles: t.tiles }, geometry: { type: "Polygon", coordinates: [[[t.bbox[0], t.bbox[1]], [t.bbox[2], t.bbox[1]], [t.bbox[2], t.bbox[3]], [t.bbox[0], t.bbox[3]], [t.bbox[0], t.bbox[1]]]] } })) } });
      rm.addLayer({ id: "rf", type: "fill", source: "r", paint: { "fill-color": ["step", ["get", "tiles"], "#1d4e89", 4, "#2f9bff", 9, "#7c5cff", 14, "#e46bd6"], "fill-opacity": .6 } });
      rm.addLayer({ id: "rl", type: "line", source: "r", paint: { "line-color": "#ffffff", "line-opacity": .25, "line-width": .6 } });
      const b = [180, 90, -180, -90]; g.coverage.forEach(t => { b[0] = Math.min(b[0], t.bbox[0]); b[1] = Math.min(b[1], t.bbox[1]); b[2] = Math.max(b[2], t.bbox[2]); b[3] = Math.max(b[3], t.bbox[3]); }); fit(rm, b, 30);
      rm.on("click", "rf", e => new maplibregl.Popup().setLngLat(e.lngLat).setHTML(`<b>${e.features[0].properties.tiles}</b> local-road tiles in this 1° cell`).addTo(rm));
    });
    get(key, "mappacks").then(u => {
      const box = $("#rpk"); if (!box) return;
      if (u.status !== "ok") { box.textContent = u.message; return; }
      const rows = []; const walk = (ps, pre) => ps.forEach(p => { const n = pre ? pre + " › " + p.name : p.name; if (p.installed_version != null) rows.push([n, p]); walk(p.children, n); }); walk(u.packs, "");
      const tot = rows.reduce((a, [, p]) => a + (p.routing_bytes || 0), 0);
      box.innerHTML = `<div class="scroll" style="max-height:320px"><table><tr><th>Pack</th><th class="num">Installed version</th><th class="num">Routing tiles</th><th class="num">Routing size (v${u.latest_version})</th></tr>${rows.map(([n, p]) => `<tr><td>${esc(n)}</td><td class="num" style="${p.installed_version < u.latest_version ? "color:#f5a623" : ""}">v${p.installed_version}${p.installed_date ? " (" + esc(p.installed_date) + ")" : ""}</td><td class="num">${p.routing_tiles}</td><td class="num">${fB(p.routing_bytes)}</td></tr>`).join("")}<tr><th>Total</th><th></th><th></th><th class="num">${fB(tot)}</th></tr></table></div><div class="note">Sizes are those published for the newest map version; Wahoo's pack list also holds the routing tile ids per pack.</div>`;
    }).catch(e => { const b = $("#rpk"); if (b) b.textContent = "Could not load: " + e.message; });
  },
  async sensors(key, el) {
    const s = await get(key, "sensors"), ext = s.devices.filter(d => !d.internal), int = s.devices.filter(d => d.internal);
    const row = d => `<tr><td>${esc(d.name)}</td><td>${esc(d.kind || d.source || "–")}</td><td>${esc(d.manufacturer || "–")}</td><td>${esc(d.serial || "–")}</td><td>${esc(d.software || "–")}</td><td>${d.battery_pct != null ? d.battery_pct + "%" : d.battery_status || "–"}</td><td>${d.battery_voltage != null ? d.battery_voltage.toFixed(2) + " V" : "–"}</td></tr>`;
    const head = "<tr><th>Name</th><th>Type</th><th>Manufacturer</th><th>Serial</th><th>Firmware</th><th>Battery</th><th>Voltage</th></tr>";
    const R = s.readings || {}, S = s.series, rk = (label, x, u, f = v => v.toFixed(1)) => x ? kpi(label, f(x.last) + u, `${f(x.min)}–${f(x.max)}${u} · avg ${f(x.avg)}${u} · ${fDate(x.last_at)}`) : "";
    const rd = rk("🌡 Temperature (last reading)", R.temperature, " °C") + rk("⛰ Altitude (barometer)", R.altitude, " m", v => Math.round(v)) + rk("Heart rate", R.heart_rate, " bpm", v => Math.round(v)) + rk("Power", R.power, " W", v => Math.round(v)) + rk("Cadence", R.cadence, " rpm", v => Math.round(v));
    el.innerHTML = `<div class="grid g4">${rd}</div><div style="height:14px"></div><div class="grid">${card("Connected / paired sensors (external)", ext.length ? `<table>${head}${ext.map(row).join("")}</table>` : `<div class="empty">${esc(s.note)}</div>`)}${card("Internal sensors & device", `<table>${head}${int.map(row).join("")}</table><div class="note">From the latest activity (${esc(s.from_activity || "–")}). External sensors (HR, power, cadence, …) show up here once they have been used in a ride.</div>`)}
    ${S ? card("🌡 Temperature during latest ride (°C)", `<canvas id="g1"></canvas>`) + card("⛰ Altitude during latest ride (m)", `<canvas id="g2"></canvas>`) + card("↯ Acceleration during latest ride (m/s²)", `<canvas id="g3"></canvas><div class="note">The device stores no raw accelerometer samples in its ride files. This is derived from the speed trace (positive = speeding up, negative = braking), sampled about every ${Math.round((S.t[S.t.length - 1] - S.t[0]) / Math.max(S.t.length - 1, 1))} s.</div>`) : ""}
    ${s.temp_history.length > 1 ? card("🌡 Average temperature per ride (°C)", `<canvas id="tc"></canvas><div class="note">Recorded by the internal temperature sensor; the ELEMNT keeps no live value over USB, only what was logged during rides.</div>`) : ""}
    ${Object.keys(s.external_usage).length ? card("External sensor usage (rides)", `<table>${Object.entries(s.external_usage).map(([k, v]) => `<tr><td>${esc(k)}</td><td class="num">${v} rides</td></tr>`).join("")}</table>`) : ""}</div>`;
    if (S) { const f = v => new Date(v * 1000).toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" }); [["#g1", S.temp, "#f5b83d", " °"], ["#g2", S.alt, "#2ecc8f", " m"], ["#g3", S.accel, "#7c5cff", ""]].forEach(([id, data, color, unit]) => lineChart($(id), S.t, [{ data, color }], { h: 200, unit, xfmt: f })); }
    if (s.temp_history.length > 1) { const h = s.temp_history; lineChart($("#tc"), h.map(x => x[0]), [{ data: h.map(x => x[1]), color: "#f5b83d" }, { data: h.map(x => x[2]), color: "#ff5c6c" }], { h: 220, fill: false, xfmt: v => new Date(v * 1000).toLocaleDateString("en-GB", { month: "short", year: "2-digit" }) }); }
  },
  async routes(key, el) {
    const r = await get(key, "routes"), dev = devices.find(d => d.key === key);
    el.innerHTML = card("Add route to device", `<div id="drop" class="drop"><div style="font-size:30px">⬆</div><div><b>Drop a .gpx or .fit file here</b> or <label class="link"><input id="rf" type="file" accept=".gpx,.fit" multiple hidden>browse…</label></div><div class="note">GPX is converted to a FIT course and copied to <code>/routes</code> on the Wahoo. ${dev?.online ? "" : '<span class="warn-text">The device must be connected and switched on.</span>'}</div><div id="ru"></div></div>`)
      + '<div style="height:14px"></div>' + card("Saved routes", r.items.length ? `<table><tr><th>Name</th><th>Modified</th><th class="num">Size</th></tr>${r.items.map(i => `<tr class="click" data-n="${esc(i.name)}"><td>🗺 ${esc(i.name)}</td><td>${i.modified ? fDate(i.modified) : "–"}</td><td class="num">${fB(i.bytes)}</td></tr>`).join("")}</table>` : `<div class="empty">No routes are stored on the device right now.<div class="note">Add one above; click a route to see it on a map.</div></div>`);
    el.querySelectorAll("tr.click").forEach(tr => tr.onclick = async () => { const d = await api(`/devices/${encodeURIComponent(key)}/route/${encodeURIComponent(tr.dataset.n)}`); modal(activityHtml(d, true)); drawActivity(d); });
    const drop = $("#drop"), log = $("#ru");
    const send = async files => {
      let ok = 0;
      for (const f of files) {
        log.innerHTML += `<div><span class="spin">⟳</span> Uploading ${esc(f.name)}…</div>`;
        try { const res = await api(`/devices/${encodeURIComponent(key)}/routes/upload?name=${encodeURIComponent(f.name)}`, { method: "POST", body: await f.arrayBuffer() });
          const c = res.converted; log.lastChild.innerHTML = `<span class="ok">✔</span> ${esc(res.file)}${c ? ` – ${c.points} points, ${fKm(c.distance_m)}` : ""}`; ok++;
        } catch (e) { let m = e.message; try { m = JSON.parse(m).error; } catch { } log.lastChild.innerHTML = `<span class="err">✖</span> ${esc(f.name)}: ${esc(m)}`; }
      }
      if (ok) { for (const k in cache) delete cache[k]; await loadDevices(); }
    };
    $("#rf").onchange = e => send([...e.target.files]);
    ["dragenter", "dragover"].forEach(t => drop.addEventListener(t, e => { e.preventDefault(); drop.classList.add("over"); }));
    ["dragleave", "drop"].forEach(t => drop.addEventListener(t, e => { e.preventDefault(); drop.classList.remove("over"); }));
    drop.addEventListener("drop", e => send([...e.dataTransfer.files]));
  },
  async plans(key, el) {
    const r = await get(key, "plans");
    el.innerHTML = card("Planned workouts", r.items.length ? `<table><tr><th>File</th><th class="num">Size</th></tr>${r.items.map(i => `<tr><td>${esc(i.name)}</td><td class="num">${fB(i.bytes)}</td></tr>`).join("")}</table>` : `<div class="empty">No planned workouts on the device.<div class="note">Workouts planned in TrainingPeaks / ELEMNT app are downloaded to <code>/plans</code>.</div></div>`);
  },
  async alerts(key, el) { const a = await get(key, "alerts"); el.innerHTML = card("Alerts & warnings", a.items.map(alertHtml).join("") || '<div class="empty">No alerts ✓</div>') + `<div class="note">${esc(a.note)}</div>`; },
  async history(key, el) {
    const h = (await get(key, "history")).items; let sort = ["start", -1], q = "";
    const cols = [["start", "Date", r => fDate(r.start)], ["sport", "Type", r => esc(r.sport + (r.sub_sport && r.sub_sport !== "generic" ? " · " + r.sub_sport : ""))], ["distance", "Distance", r => fKm(r.distance), 1], ["timer", "Moving", r => fD(r.timer), 1], ["avg_speed", "Avg speed", r => fKmh(r.avg_speed), 1], ["ascent", "Ascent", r => Math.round(r.ascent) + " m", 1], ["avg_hr", "Avg HR", r => r.avg_hr ? Math.round(r.avg_hr) : "–", 1], ["avg_power", "Avg power", r => r.avg_power ? Math.round(r.avg_power) + " W" : "–", 1], ["battery_end", "Battery", r => r.battery_start != null ? `${r.battery_start}→${r.battery_end}%` : "–", 1]];
    el.innerHTML = `<div class="toolbar"><input id="hq" type="text" placeholder="Filter (date, type…)"><span class="note" id="hn"></span></div><div class="card"><div class="scroll"><table id="ht"></table></div></div>`;
    const draw = () => { let r = h.filter(x => !q || (fDate(x.start) + x.sport + x.id).toLowerCase().includes(q)); r.sort((a, b) => ((a[sort[0]] ?? -1) > (b[sort[0]] ?? -1) ? 1 : -1) * sort[1]); $("#hn").textContent = `${r.length} of ${h.length} activities`;
      $("#ht").innerHTML = `<tr>${cols.map(c => `<th data-k="${c[0]}" class="${c[3] ? "num" : ""}">${c[1]}${sort[0] === c[0] ? (sort[1] > 0 ? " ▲" : " ▼") : ""}</th>`).join("")}</tr>` + r.slice(0, 600).map(x => `<tr class="click" data-id="${esc(x.id)}">${cols.map(c => `<td class="${c[3] ? "num" : ""}">${c[2](x)}</td>`).join("")}</tr>`).join("");
      $("#ht").querySelectorAll("th").forEach(t => t.onclick = () => { sort = [t.dataset.k, sort[0] === t.dataset.k ? -sort[1] : -1]; draw(); }); $("#ht").querySelectorAll("tr.click").forEach(t => t.onclick = () => showActivity(key, t.dataset.id)); };
    $("#hq").oninput = e => { q = e.target.value.toLowerCase(); draw(); }; draw();
  },
  async stats(key, el) {
    const s = await get(key, "stats"), R = s.records, rec = (l, r, f) => r ? `<tr class="click" data-id="${esc(r.id)}"><td>${l}</td><td class="num"><b>${f(r.value)}</b></td><td class="num">${fDay(r.start)}</td></tr>` : "";
    el.innerHTML = `<div class="grid g4">${kpi("Rides", s.rides)}${kpi("Distance", Math.round(s.distance / 1000).toLocaleString() + " km")}${kpi("Moving time", Math.round(s.moving_time / 3600) + " h", `elapsed ${Math.round(s.elapsed_time / 3600)} h`)}${kpi("Ascent", Math.round(s.ascent).toLocaleString() + " m")}${kpi("Avg speed", fKmh(s.avg_speed))}${kpi("Calories", Math.round(s.calories).toLocaleString())}${kpi("Active days", s.active_days)}${kpi("Best streak", s.best_streak_days + " days")}</div>
    <div class="grid g2" style="margin-top:14px">${card("Distance per month (km)", `<canvas id="s1"></canvas>`)}${card("Distance per year (km)", `<canvas id="s2"></canvas>`)}${card("Rides per weekday", `<canvas id="s3"></canvas>`)}${card("Rides per start hour", `<canvas id="s4"></canvas>`)}
    ${card("Personal records", `<table>${rec("Longest ride", R.longest, v => fKm(v))}${rec("Longest time", R.longest_time, v => fD(v))}${rec("Most ascent", R.most_ascent, v => Math.round(v) + " m")}${rec("Fastest average", R.fastest_avg, v => fKmh(v))}${rec("Top speed", R.top_speed, v => fKmh(v))}${R.highest_hr?.value ? rec("Highest HR", R.highest_hr, v => Math.round(v) + " bpm") : ""}${R.max_power?.value ? rec("Max power", R.max_power, v => Math.round(v) + " W") : ""}</table>`)}
    ${card("Activity types", `<table>${Object.entries(s.sports).map(([k, v]) => `<tr><td>${esc(k)}</td><td class="num">${v}</td></tr>`).join("")}</table>`)}</div>`;
    const m = s.by_month; barChart($("#s1"), m.map(x => x.month.slice(2)), m.map(x => x.distance / 1000), { fmt: v => Math.round(v) });
    barChart($("#s2"), s.by_year.map(x => x.year), s.by_year.map(x => x.distance / 1000), { color: "#7c5cff", fmt: v => Math.round(v) });
    barChart($("#s3"), ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"], s.by_weekday, { color: "#2ecc8f" }); barChart($("#s4"), [...Array(24).keys()].map(String), s.by_hour, { color: "#f5b83d" });
    el.querySelectorAll("tr.click").forEach(t => t.onclick = () => showActivity(key, t.dataset.id));
  },
  async debug(key, el) {
    const d = await get(key, "debug"), e = (await get(key, "entries")).entries;
    el.innerHTML = `<div class="grid g2">${card("Connection & sync", kv({ "USB / WPD id": d.device.pnp_id, Manufacturer: d.device.manufacturer, Model: d.device.model, "MTP serial": d.device.serial, "MTP firmware string": d.device.firmware, "Power level": d.device.power_level != null ? d.device.power_level + "%" : "–", "Objects on device": d.objects, "Tree walk": d.walk_ms + " ms", "Files copied last sync": d.copied_last_sync, "Activities parsed": `${d.parse_cached} cached · ${d.parse_ms} ms`, "Cache folder": d.cache_dir, "Last sync": d.synced_at, "system_info.txt": d.system_info.replace(/\n/g, " | "), "serial_number.txt": d.serial_txt }))}
    ${card("Storage object (MTP)", kv(Object.fromEntries(d.storages.flatMap(s => Object.entries(s).map(([k, v]) => [k, typeof v === "number" && v > 1e5 ? `${v} (${fB(v)})` : v])))))}
    ${card("Top-level folders", `<table><tr><th>Path</th><th class="num">Objects</th><th class="num">Size</th></tr>${d.top_level.map(t => `<tr><td>${esc(t.path)}</td><td class="num">${t.objects}</td><td class="num">${fB(t.bytes)}</td></tr>`).join("")}</table>`)}
    ${card("Tile pack versions (raw)", `<pre>${esc(JSON.stringify(d.tilepacks))}</pre>`)}</div>
    <div class="card" style="margin-top:14px"><h4>File browser (raw MTP listing)</h4><div class="toolbar"><input id="fq" type="text" placeholder="Path prefix, e.g. maps/tiles/8/131" style="width:340px"><span class="note" id="fn"></span></div><div class="scroll"><table id="ft"></table></div></div>
    <details style="margin-top:14px"><summary>routing-config.json</summary><pre>${esc(JSON.stringify(d.routing_config, null, 1))}</pre></details>`;
    const draw = () => { const q = $("#fq").value.toLowerCase(); const r = e.filter(x => !q || x.path.toLowerCase().startsWith(q) || x.path.toLowerCase().includes(q)); $("#fn").textContent = `${r.length} of ${e.length} objects`; $("#ft").innerHTML = r.slice(0, 500).map(x => `<tr><td>${x.dir ? "📁" : "📄"} ${esc(x.path)}</td><td class="num">${x.dir ? "" : fB(x.size)}</td></tr>`).join(""); }; $("#fq").oninput = draw; draw();
  },
};

const alertHtml = a => `<div class="alert"><span class="badge b-${a.level}">${a.level}</span><div><b>${esc(a.title)}</b>${a.when ? ` <span class="note" style="margin:0">· ${fDate(a.when)}</span>` : ""}<div class="note" style="margin:0">${esc(a.detail)}</div></div></div>`;

// ---------- activity detail ----------
async function showActivity(key, id) {
  modal('<div class="empty">Loading activity…</div>');
  try { const d = await api(`/devices/${encodeURIComponent(key)}/activity/${encodeURIComponent(id)}`); modal(activityHtml(d)); drawActivity(d); } catch (e) { modal(`<div class="empty">Error: ${esc(e.message)}</div>`); }
}
function activityHtml(d, route) {
  const s = d.summary, sensors = d.sensors.filter(x => !x.internal);
  return `<h2 style="margin-top:0">${route ? "🗺 " : "🚴 "}${esc(s.name || s.sport)} <span class="note">${esc(s.id)}</span></h2>
  <div class="grid g4">${kpi("Start", fDate(s.start))}${kpi("End", fDate(s.end))}${kpi("Distance", fKm(s.distance))}${kpi("Moving / elapsed", fD(s.timer), "elapsed " + fD(s.elapsed))}${kpi("Avg / max speed", fKmh(s.avg_speed), "max " + fKmh(s.max_speed))}${kpi("Ascent / descent", Math.round(s.ascent) + " / " + Math.round(s.descent) + " m", s.min_alt != null ? `alt ${Math.round(s.min_alt)}–${Math.round(s.max_alt)} m` : "")}${kpi("Type", esc(s.sport), esc(s.sub_sport))}${kpi("Temperature", s.avg_temp != null ? s.avg_temp + "°C" : "–", s.max_temp != null ? "max " + s.max_temp + "°C" : "")}
  ${s.avg_hr ? kpi("Heart rate", Math.round(s.avg_hr) + " bpm", "max " + Math.round(s.max_hr)) : ""}${s.avg_power ? kpi("Power", Math.round(s.avg_power) + " W", "max " + Math.round(s.max_power)) : ""}${s.calories ? kpi("Calories", Math.round(s.calories)) : ""}${s.battery_start != null ? kpi("Battery", `${s.battery_start}% → ${s.battery_end}%`) : ""}</div>
  <div class="grid g2" style="margin-top:14px"><div class="card" style="grid-column:1/-1"><h4>Map</h4><div id="amap" class="map tall"></div></div>
  ${card("Speed (km/h)", `<canvas id="c1"></canvas>`)}${card("Altitude (m)", `<canvas id="c2"></canvas>`)}${d.series.hr.some(v => v != null) ? card("Heart rate (bpm)", `<canvas id="c3"></canvas>`) : ""}${d.series.power.some(v => v != null) ? card("Power (W)", `<canvas id="c4"></canvas>`) : ""}
  ${d.laps.length ? card("Laps", `<table><tr><th>#</th><th>Start</th><th class="num">Dist</th><th class="num">Time</th><th class="num">Avg</th><th class="num">Ascent</th></tr>${d.laps.map((l, i) => `<tr><td>${i + 1}</td><td>${fDate(l.start)}</td><td class="num">${fKm(l.distance)}</td><td class="num">${fD(l.timer)}</td><td class="num">${fKmh(l.avg_speed)}</td><td class="num">${Math.round(l.ascent)} m</td></tr>`).join("")}</table>`) : ""}
  ${card("Device during ride", kv({ Device: s.device, Firmware: s.firmware, Hardware: s.hardware, "External sensors": sensors.map(x => x.name).join(", ") || "none", Records: s.records, "Track points (shown)": d.track.length, "File": s.file, "File size": fB(s.bytes), "Battery samples": d.battery.length }))}</div>
  <details><summary>Technical: FIT messages (${d.raw.length} types)</summary><div class="scroll"><table><tr><th>Message</th><th class="num">Count</th><th>Sample</th></tr>${d.raw.map(r => `<tr><td>${esc(r.kind)}</td><td class="num">${r.count}</td><td style="white-space:normal;font-size:11.5px;color:var(--dim)">${esc(Object.entries(r.sample).map(([k, v]) => k + "=" + v).join(", ").slice(0, 400))}</td></tr>`).join("")}</table></div></details>
  <details><summary>Technical: events (${d.events.length})</summary><div class="scroll" style="max-height:260px"><table>${d.events.map(e => `<tr><td>${fDate(e.t)}</td><td>${esc(e.event)}</td><td>${esc(e.event_type)}</td><td>${esc(Object.entries(e.extra).map(([k, v]) => k + "=" + v).join(" "))}</td></tr>`).join("")}</table></div></details>`;
}
function drawActivity(d) {
  const S = d.series, x = S.t, f = v => new Date(v * 1000).toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" });
  const ch = (id, data, color, unit) => $(id) && lineChart($(id), x, [{ data, color }], { unit, xfmt: f });
  ch("#c1", S.speed.map(v => v == null ? null : v * 3.6), "#2f9bff", ""); ch("#c2", S.alt, "#2ecc8f", " m"); ch("#c3", S.hr, "#ff5c6c", ""); ch("#c4", S.power, "#f5b83d", "");
  const map = newMap($("#amap")); if (!map) return; const s = d.summary;
  map.on("load", () => { if (d.track.length > 1) { const line = d.track.map(p => [p[1], p[0]]); map.addSource("r", { type: "geojson", data: { type: "Feature", geometry: { type: "LineString", coordinates: line } } });
    map.addLayer({ id: "rc", type: "line", source: "r", paint: { "line-color": "#0b0f17", "line-width": 7, "line-opacity": .6 }, layout: { "line-join": "round" } }); map.addLayer({ id: "r", type: "line", source: "r", paint: { "line-color": "#2f9bff", "line-width": 4 }, layout: { "line-join": "round", "line-cap": "round" } });
    new maplibregl.Marker({ color: "#2ecc8f" }).setLngLat(line[0]).addTo(map); new maplibregl.Marker({ color: "#ff5c6c" }).setLngLat(line[line.length - 1]).addTo(map); }
    if (s.bbox) fit(map, s.bbox, 50); });
}

// ---------- modal ----------
function modal(html) { $("#modalBody").innerHTML = html; $("#modal").classList.remove("hidden"); }
$("#modalClose").onclick = () => $("#modal").classList.add("hidden");
$("#modal").onclick = e => { if (e.target.id === "modal") $("#modal").classList.add("hidden"); };
document.addEventListener("keydown", e => { if (e.key === "Escape") $("#modal").classList.add("hidden"); });

// ---------- sync ----------
$("#syncBtn").onclick = async () => {
  const p = $("#syncPanel"); p.classList.remove("hidden"); p.innerHTML = '<span class="spin">⟳</span> Starting…'; $("#syncBtn").disabled = true;
  await api("/sync", { method: "POST" });
  for (;;) { await new Promise(r => setTimeout(r, 500)); const s = await api("/sync/state");
    p.innerHTML = (s.running ? '<span class="spin">⟳</span> ' : "") + s.log.map(esc).join("<br>") + (s.error ? `<br><span class="err">✖ ${esc(s.error)}</span>` : "") + (!s.running && !s.error ? '<br><span class="ok">✔ Sync complete</span>' : "");
    if (!s.running) { for (const k in cache) delete cache[k]; await loadDevices(); refreshStatus(); setTimeout(() => p.classList.add("hidden"), s.error ? 15000 : 4000); break; } }
};

refreshStatus(); loadDevices(); setInterval(refreshStatus, 4000);
