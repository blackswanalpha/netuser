const { invoke } = window.__TAURI__.core;
const appWindow = window.__TAURI__?.window?.getCurrentWindow?.();

const DEVICE_REFRESH_MS = 60_000;
const NET_REFRESH_MS = 2_000;
const PENCIL =
  '<svg class="pencil" viewBox="0 0 24 24" width="14" height="14" fill="none"><path d="M15.2141 5.98239L16.6158 4.58063C17.39 3.80646 18.6452 3.80646 19.4194 4.58063C20.1935 5.3548 20.1935 6.60998 19.4194 7.38415L18.0176 8.78591M15.2141 5.98239L6.98023 14.2163C5.93493 15.2616 5.41226 15.7842 5.05637 16.4211C4.70047 17.058 4.3424 18.5619 4 20C5.43809 19.6576 6.94199 19.2995 7.57889 18.9436C8.21579 18.5877 8.73844 18.0651 9.78375 17.0198L18.0176 8.78591M15.2141 5.98239L18.0176 8.78591" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.8"/></svg>';

const AVATAR_COLORS = ["#f1521f", "#3b6ef5", "#1f9d57", "#8b5cf6", "#e0a500", "#0ea5a3", "#e0457b"];
const el = (id) => document.getElementById(id);

/* ───────────── Window controls ───────────── */
el("win-min").addEventListener("click", () => appWindow?.minimize());
el("win-close").addEventListener("click", () => appWindow?.close());

/* ───────────── Screen toggle ───────────── */
let screen = "devices";
const tabs = { devices: el("tab-devices"), network: el("tab-network") };
const screens = { devices: el("screen-devices"), network: el("screen-network") };
let netTimer = null;

function show(which) {
  if (screen === which) return;
  screen = which;
  for (const k of ["devices", "network"]) {
    tabs[k].classList.toggle("active", k === which);
    screens[k].hidden = k !== which;
  }
  if (which === "network") {
    netScan();
    netTimer = setInterval(netScan, NET_REFRESH_MS);
  } else {
    clearInterval(netTimer);
    netTimer = null;
    if (Date.now() - lastScanAt > 10_000) scan();
  }
}
tabs.devices.addEventListener("click", () => show("devices"));
tabs.network.addEventListener("click", () => show("network"));

/* ───────────── Devices ───────────── */
const d = {
  subnet: el("subnet"), stamp: el("stamp"), error: el("error"), rows: el("rows"),
  button: el("scan"), label: el("scan-label"),
  total: el("s-total"), named: el("s-named"), unknown: el("s-unknown"),
  self: el("s-self"), selfIp: el("s-self-ip"),
};
let scanning = false, editing = null, last = null, lastScanAt = 0;

async function scan() {
  if (scanning) return;
  scanning = true;
  d.button.disabled = true;
  d.button.classList.add("spinning");
  d.label.textContent = "Scanning…";
  try {
    last = await invoke("scan");
    lastScanAt = Date.now();
    d.error.hidden = true;
    if (!editing) renderDevices(); // an open rename renders on commit

  } catch (e) {
    d.error.textContent = String(e);
    d.error.hidden = false;
  } finally {
    scanning = false;
    d.button.disabled = false;
    d.button.classList.remove("spinning");
    d.label.textContent = "Scan";
  }
}

function renderDevices() {
  if (!last) return;
  const self = last.devices.find((x) => x.is_self);
  d.subnet.textContent = `${last.interface} · ${last.subnet}`;
  d.stamp.textContent = `Last scan ${time(lastScanAt)}`;
  d.total.textContent = last.count;
  d.named.textContent = last.devices.filter((x) => x.name).length;
  d.unknown.textContent = last.devices.filter((x) => !x.is_router && !x.vendor).length;
  d.self.textContent = self ? "1" : "0";
  d.selfIp.textContent = self ? self.ip : "Your IP on this network";
  d.rows.replaceChildren(...last.devices.map(deviceRow));
}

function labelOf(x) {
  if (x.name) return x.name;
  if (x.is_self) return "This computer";
  if (x.is_router) return "Router";
  return x.vendor || "Unnamed device";
}
function avatarColor(x) {
  let h = 0;
  for (const c of x.mac) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return AVATAR_COLORS[h % AVATAR_COLORS.length];
}
function cell(text, cls) {
  const td = document.createElement("td");
  td.textContent = text;
  if (cls) td.className = cls;
  return td;
}

function deviceRow(x) {
  const tr = document.createElement("tr");
  tr.dataset.mac = x.mac;
  if (x.is_self) tr.className = "me";
  else if (x.is_router) tr.className = "router";

  const nameTd = document.createElement("td");
  const wrap = document.createElement("div");
  wrap.className = "device";
  const av = document.createElement("span");
  av.className = "avatar";
  av.style.background = avatarColor(x);
  av.textContent = (labelOf(x).trim()[0] || "?").toUpperCase();
  const box = document.createElement("div");
  box.className = "name-box";
  const line = document.createElement("span");
  line.className = "name-line" + (x.name ? "" : " unnamed");
  line.innerHTML = `<span class="name-text"></span>${PENCIL}`;
  line.querySelector(".name-text").textContent = labelOf(x);
  line.title = "Click to rename";
  line.addEventListener("click", () => openEditor(x.mac));
  const sub = document.createElement("p");
  sub.className = "name-sub";
  sub.textContent = x.name ? x.vendor || (x.private_mac ? "Private MAC" : "") : "";
  box.append(line, sub);
  wrap.append(av, box);
  nameTd.append(wrap);
  tr.append(nameTd);

  tr.append(cell(x.ip, "mono"));
  tr.append(cell(x.mac, "mono"));
  tr.append(cell(x.vendor || (x.private_mac ? "Private MAC" : "Unknown"), x.vendor ? "" : "vendor-muted"));

  if (!x.online && !x.is_self) tr.classList.add("away");
  const statusTd = document.createElement("td");
  const pill = document.createElement("span");
  if (x.is_self) { pill.className = "pill pill-self"; pill.textContent = "This device"; }
  else if (x.is_router) { pill.className = "pill pill-router"; pill.textContent = "Router"; }
  else if (!x.online) {
    pill.className = "pill pill-router";
    pill.textContent = "Away";
    pill.title = `Last seen ${agoText(x.last_seen_secs)}`;
  } else { pill.className = "pill pill-online"; pill.textContent = "Online"; }
  statusTd.append(pill);
  tr.append(statusTd);

  // Block helper: only for other people's devices.
  const actionTd = document.createElement("td");
  actionTd.className = "col-action";
  if (!x.is_self && !x.is_router) {
    const b = document.createElement("button");
    b.className = "ghost-btn";
    b.type = "button";
    b.textContent = "Block";
    b.title = "Copy MAC and open the router page to block it";
    b.addEventListener("click", () => block(x));
    actionTd.append(b);
  }
  tr.append(actionTd);
  return tr;
}

async function block(x) {
  try {
    const url = await invoke("block_helper", { mac: x.mac });
    const who = x.name || x.vendor || x.ip;
    toast(`MAC of ${who} copied. Paste it into the block/MAC-filter list on ${url}.`);
  } catch (e) {
    toast(String(e), true);
  }
}

function agoText(secs) {
  if (secs < 60) return "just now";
  const m = Math.round(secs / 60);
  return m < 60 ? `${m} min ago` : `${Math.round(m / 60)} h ago`;
}

let toastTimer = null;
function toast(msg, isError) {
  const t = el("toast");
  t.textContent = msg;
  t.classList.toggle("toast-error", !!isError);
  t.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { t.hidden = true; }, 6000);
}

function openEditor(mac) {
  const tr = d.rows.querySelector(`tr[data-mac="${CSS.escape(mac)}"]`);
  const line = tr?.querySelector(".name-line");
  const dev = last?.devices.find((x) => x.mac === mac);
  if (!line || !dev) return;
  editing = mac; // set only once the editor really opens, or auto-refresh stalls

  const input = document.createElement("input");
  input.className = "name-input";
  input.value = dev.name || "";
  input.placeholder = "e.g. Tyra's phone";
  input.maxLength = 40;
  let done = false;
  const commit = async (save) => {
    if (done) return;
    done = true;
    if (save) {
      const name = input.value.trim();
      try {
        // Apply the saved map to the current `last`: a scan may have replaced
        // it while this editor was open.
        const names = await invoke("set_name", { mac, name });
        for (const x of last.devices) x.name = names[x.mac] ?? null;
      } catch (e) {
        d.error.textContent = String(e);
        d.error.hidden = false;
      }
    }
    if (editing === mac) editing = null;
    if (!editing) {
      renderDevices();
    } else {
      // Another row's editor opened meanwhile: refresh only this row.
      const cur = last.devices.find((x) => x.mac === mac);
      const row = input.closest("tr");
      if (cur && row) row.replaceWith(deviceRow(cur));
    }
  };
  input.addEventListener("keydown", (ev) => {
    if (ev.key === "Enter") commit(true);
    else if (ev.key === "Escape") commit(false);
  });
  input.addEventListener("blur", () => commit(true));
  line.replaceWith(input);
  input.focus();
  input.select();
}

/* ───────────── Network ───────────── */
const n = {
  dbm: el("n-dbm"), quality: el("n-quality"), bars: el("n-bars"),
  ssid: el("n-ssid"), band: el("n-band"),
  down: el("n-down"), up: el("n-up"), link: el("n-link"),
  chan: el("n-chan"), freq: el("n-freq"), rx: el("n-rx"), tx: el("n-tx"),
  error: el("net-error"),
};
// Five static bars, filled by level.
n.bars.innerHTML = "<i></i><i></i><i></i><i></i><i></i>";

let netBusy = false;
async function netScan() {
  if (netBusy) return; // each call blocks ~1s; don't overlap
  netBusy = true;
  try {
    const s = await invoke("net_stats");
    n.error.hidden = true;
    renderNet(s);
  } catch (e) {
    n.error.textContent = String(e);
    n.error.hidden = false;
  } finally {
    netBusy = false;
  }
}

function level(s) {
  if (s.signal_dbm != null) {
    const v = s.signal_dbm;
    return v >= -55 ? 5 : v >= -65 ? 4 : v >= -72 ? 3 : v >= -80 ? 2 : 1;
  }
  const p = s.signal_pct ?? 0;
  return p >= 80 ? 5 : p >= 60 ? 4 : p >= 40 ? 3 : p >= 20 ? 2 : 1;
}

function renderNet(s) {
  n.dbm.textContent = s.signal_dbm != null ? s.signal_dbm : "–";
  const lv = level(s);
  n.quality.textContent = s.quality || "Unknown";
  n.quality.className = "pill " + (lv >= 4 ? "pill-online" : lv === 3 ? "pill-fair" : "pill-weak");
  [...n.bars.children].forEach((b, i) => b.classList.toggle("on", i < lv));

  n.ssid.textContent = s.ssid || "Unknown network";
  n.band.textContent = [s.band, s.freq_mhz ? `${s.freq_mhz} MHz` : null]
    .filter(Boolean).join(" · ") || "–";

  n.down.textContent = fmtRate(s.rx_bps);
  n.up.textContent = fmtRate(s.tx_bps);
  n.link.textContent = s.link_mbps != null ? `${trim(s.link_mbps)} Mb/s` : "–";
  n.chan.textContent = s.channel ? `Ch ${s.channel}` : "–";
  n.freq.textContent = s.band ? `${s.band} band` : "Frequency";
  n.rx.textContent = fmtBytes(s.rx_total);
  n.tx.textContent = fmtBytes(s.tx_total);
}

/* ───────────── Formatting ───────────── */
function time(ms) {
  return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}
function trim(x) { return Number.isInteger(x) ? x : Math.round(x * 10) / 10; }

// bytes/second -> bits/second, human readable (network convention).
function fmtRate(bps) {
  const bits = bps * 8;
  if (bits >= 1e9) return `${(bits / 1e9).toFixed(1)} Gb/s`;
  if (bits >= 1e6) return `${(bits / 1e6).toFixed(1)} Mb/s`;
  if (bits >= 1e3) return `${Math.round(bits / 1e3)} Kb/s`;
  return `${Math.round(bits)} b/s`;
}
// total bytes, human readable.
function fmtBytes(b) {
  if (b >= 1e9) return `${(b / 1e9).toFixed(2)} GB`;
  if (b >= 1e6) return `${(b / 1e6).toFixed(1)} MB`;
  if (b >= 1e3) return `${Math.round(b / 1e3)} KB`;
  return `${b} B`;
}

/* ───────────── Boot ───────────── */
setInterval(() => { if (screen === "devices" && !editing) scan(); }, DEVICE_REFRESH_MS);
scan();
