"use strict";

// ── Config ────────────────────────────────────────────────────────────────────
// The API is same-origin in production (Rust serves both this page and /api/*).
// Override with ?api=https://host for local testing against a remote backend.
const API_BASE = new URLSearchParams(location.search).get("api") || "";
const DEFAULT_COUNTRY = "DE";

// Line colours (match the previous Flutter chart)
const C_GEN = "#2E7D32";
const C_LOAD = "#1565C0";
const C_SURPLUS = "#F57C00";
const FILL_POS = "rgba(129,199,132,0.30)";
const FILL_NEG = "rgba(239,154,154,0.28)";

// ── State ───────────────────────────────────────────────────────────────────
let country = DEFAULT_COUNTRY;
let hours = 24;
let series = []; // [{t: Date, gen, load, surplus}]

// ── DOM ─────────────────────────────────────────────────────────────────────
const $ = (id) => document.getElementById(id);
const els = {
  country: $("country"), hours: $("hours"), refresh: $("refresh"),
  refreshIcon: $("refresh-icon"), retry: $("retry"),
  loading: $("loading"), error: $("error"), errorMsg: $("error-msg"),
  dashboard: $("dashboard"),
  gauge: $("gauge"), chart: $("chart"), tooltip: $("tooltip"), range: $("range"),
};

// ── Helpers ───────────────────────────────────────────────────────────────────
function fmtMW(v) {
  const a = Math.abs(v), s = v < 0 ? "-" : "";
  return a >= 1000 ? `${s}${(a / 1000).toFixed(1)} GW` : `${s}${a.toFixed(0)} MW`;
}
function fmtTime(d) {
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}
function fmtDay(d) {
  return d.toLocaleDateString(undefined, { weekday: "short", day: "numeric", month: "short" });
}
function clamp(v, lo, hi) { return Math.max(lo, Math.min(hi, v)); }
function lerp(a, b, t) { return a + (b - a) * t; }

// ── Derived metrics (mirrors the old EnergyData model) ────────────────────────
function currentPoint() {
  if (!series.length) return null;
  const now = Date.now();
  return series.reduce((a, b) =>
    Math.abs(a.t - now) < Math.abs(b.t - now) ? a : b);
}
function peakSurplusPoint() {
  return series.reduce((a, b) => (a.surplus > b.surplus ? a : b));
}
function coveragePct(p) { return p.load > 0 ? Math.min(p.gen / p.load, 1) * 100 : 0; }
function surplusPct(p) { return p.gen > 0 ? (p.surplus / p.gen) * 100 : 0; }

// 0 = worst surplus of the window, 1 = best — drives the gauge needle.
function normalizedCurrentSurplus() {
  const cur = currentPoint();
  if (!cur || !series.length) return 0.5;
  let lo = Infinity, hi = -Infinity;
  for (const p of series) { lo = Math.min(lo, p.surplus); hi = Math.max(hi, p.surplus); }
  if (hi === lo) return 0.5;
  return clamp((cur.surplus - lo) / (hi - lo), 0, 1);
}

// 3h window before "now" vs after — falls back to first/second half.
function renewableTrend() {
  if (series.length < 4) return "flat";
  const now = Date.now(), W = 3 * 3600 * 1000;
  const past = series.filter((p) => p.t < now && p.t > now - W).map((p) => p.gen);
  const future = series.filter((p) => p.t > now && p.t < now + W).map((p) => p.gen);
  const avg = (a) => a.reduce((x, y) => x + y, 0) / a.length;
  let pa, fa;
  if (!past.length || !future.length) {
    const mid = Math.floor(series.length / 2);
    pa = avg(series.slice(0, mid).map((p) => p.gen));
    fa = avg(series.slice(mid).map((p) => p.gen));
  } else { pa = avg(past); fa = avg(future); }
  if (pa <= 0) return fa > 0 ? "rising" : "flat";
  const ch = (fa - pa) / pa;
  if (ch > 0.05) return "rising";
  if (ch < -0.05) return "falling";
  return "flat";
}

// ── Data fetching ─────────────────────────────────────────────────────────────
async function loadCountries() {
  try {
    const r = await fetch(`${API_BASE}/api/v1/countries`);
    const j = await r.json();
    const list = (j.data || []).slice().sort();
    if (!list.length) return;
    if (!list.includes(country)) country = list[0];
    els.country.innerHTML = list
      .map((c) => `<option value="${c}"${c === country ? " selected" : ""}>${c}</option>`)
      .join("");
  } catch (_) { /* country list is optional */ }
}

async function loadData() {
  showLoading();
  els.refreshIcon.classList.add("animate-spin");
  try {
    const url = `${API_BASE}/api/v1/renewable-surplus/${country}/plot-json?hours=${hours}`;
    const r = await fetch(url);
    if (!r.ok) throw new Error(`Server returned HTTP ${r.status}`);
    const j = await r.json();
    if (j.success !== true || !j.data) throw new Error(j.error || "No data returned by server");
    const d = j.data;
    series = d.timestamps.map((ts, i) => ({
      t: new Date(ts), gen: +d.generation[i], load: +d.load[i], surplus: +d.surplus[i],
    }));
    render();
    showDashboard();
  } catch (e) {
    showError(e.message || String(e));
  } finally {
    els.refreshIcon.classList.remove("animate-spin");
  }
}

// ── View switching ────────────────────────────────────────────────────────────
function showLoading() {
  els.loading.classList.remove("hidden");
  els.error.classList.add("hidden"); els.error.classList.remove("flex");
  els.dashboard.classList.add("hidden");
}
function showError(msg) {
  els.errorMsg.textContent = msg;
  els.loading.classList.add("hidden");
  els.error.classList.remove("hidden"); els.error.classList.add("flex");
  els.dashboard.classList.add("hidden");
}
function showDashboard() {
  els.loading.classList.add("hidden");
  els.error.classList.add("hidden"); els.error.classList.remove("flex");
  els.dashboard.classList.remove("hidden");
}

// ── Render ────────────────────────────────────────────────────────────────────
function render() {
  renderGauge();
  renderCards();
  renderRange();
  renderChart();
  renderHint();
}

// ── Gauge (speedometer arc) ─────────────────────────────────────────────────
const GAUGE_START = 150, GAUGE_SWEEP = 240; // degrees, clockwise from start

function polar(cx, cy, r, deg) {
  const a = (deg * Math.PI) / 180;
  return [cx + r * Math.cos(a), cy + r * Math.sin(a)];
}
function arcPath(cx, cy, r, startDeg, sweepDeg) {
  const [x0, y0] = polar(cx, cy, r, startDeg);
  const [x1, y1] = polar(cx, cy, r, startDeg + sweepDeg);
  const large = sweepDeg > 180 ? 1 : 0;
  return `M ${x0.toFixed(2)} ${y0.toFixed(2)} A ${r} ${r} 0 ${large} 1 ${x1.toFixed(2)} ${y1.toFixed(2)}`;
}
// red → amber → green by value (0..1)
function gaugeColor(t) {
  const stops = [[0xE5, 0x39, 0x35], [0xFB, 0x8C, 0x00], [0x2E, 0x7D, 0x32]];
  const [a, b] = t < 0.5 ? [stops[0], stops[1]] : [stops[1], stops[2]];
  const k = t < 0.5 ? t / 0.5 : (t - 0.5) / 0.5;
  const c = a.map((v, i) => Math.round(lerp(v, b[i], k)));
  return `rgb(${c[0]},${c[1]},${c[2]})`;
}

function renderGauge() {
  const cur = currentPoint();
  const value = normalizedCurrentSurplus();
  const pct = cur ? coveragePct(cur) : 0;
  const cx = 100, cy = 92, r = 76, sw = 13;
  const color = gaugeColor(value);
  const [tx, ty] = polar(cx, cy, r, GAUGE_START + GAUGE_SWEEP * value);

  els.gauge.innerHTML = `
    <svg viewBox="0 0 200 150" class="w-full" role="img" aria-label="${pct.toFixed(0)}% renewable now">
      <path d="${arcPath(cx, cy, r, GAUGE_START, GAUGE_SWEEP)}" fill="none"
            stroke="#e2e8f0" stroke-width="${sw}" stroke-linecap="round"/>
      ${value > 0.01 ? `<path d="${arcPath(cx, cy, r, GAUGE_START, GAUGE_SWEEP * value)}" fill="none"
            stroke="${color}" stroke-width="${sw}" stroke-linecap="round"/>
      <circle cx="${tx.toFixed(2)}" cy="${ty.toFixed(2)}" r="${sw * 0.42}" fill="#fff" stroke="${color}" stroke-width="2"/>` : ""}
      <text x="${cx}" y="${cy - 4}" text-anchor="middle" font-size="40" font-weight="700"
            fill="#0f172a" letter-spacing="-1.5">${pct.toFixed(0)}%</text>
      <text x="${cx}" y="${cy + 16}" text-anchor="middle" font-size="11" font-weight="500"
            fill="#94a3b8" letter-spacing="0.4">renewable now</text>
    </svg>`;
}

// ── Summary cards ─────────────────────────────────────────────────────────────
function trendBadge(trend) {
  const map = {
    rising: ["#15803d", "Rising", "M3 17l6-6 4 4 7-7M14 8h5v5"],
    falling: ["#b91c1c", "Falling", "M3 7l6 6 4-4 7 7M14 16h5v-5"],
    flat: ["#475569", "Steady", "M3 12h18M16 7l5 5-5 5"],
  };
  const [color, label, d] = map[trend];
  return `<span style="color:${color}" class="inline-flex items-center gap-1">
    <svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="${d}"/></svg>${label}</span>`;
}

function renderCards() {
  const cur = currentPoint();
  const peak = peakSurplusPoint();

  // Current status
  if (cur) {
    const surplus = cur.surplus >= 0;
    const card = $("card-current");
    card.style.background = surplus ? "#ecfdf5" : "#fef2f2";
    card.style.borderColor = surplus ? "#a7f3d0" : "#fecaca";
    const color = surplus ? "#047857" : "#b91c1c";
    const bolt = '<svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/></svg>';
    const warn = '<svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>';
    $("current-icon").innerHTML = `<span style="color:${color}">${surplus ? bolt : warn}</span>`;
    $("current-title").textContent = surplus ? "Surplus now" : "Deficit now";
    const v = $("current-value");
    v.textContent = fmtMW(Math.abs(cur.surplus)); v.style.color = color;
    $("current-sub").textContent = `${surplusPct(cur).toFixed(0)}% of generation`;
    $("current-trend").innerHTML = trendBadge(renewableTrend());
  }

  // Peak surplus
  $("peak-value").textContent = fmtTime(peak.t);
  $("peak-sub").textContent = fmtMW(peak.surplus);

  // Green coverage
  if (cur) {
    const pct = coveragePct(cur);
    const color = pct >= 80 ? "#15803d" : pct >= 50 ? "#4d7c0f" : pct >= 30 ? "#b45309" : "#b91c1c";
    const card = $("card-coverage");
    card.style.borderColor = color + "40";
    card.style.background = color + "14";
    const v = $("coverage-value");
    v.textContent = `${pct.toFixed(0)}%`; v.style.color = color;
    $("coverage-icon-color").style.color = color;
  }
}

function renderRange() {
  if (!series.length) { els.range.textContent = ""; return; }
  const a = series[0].t, b = series[series.length - 1].t;
  els.range.textContent = `${fmtDay(a)}, ${fmtTime(a)} → ${fmtTime(b)} (${series.length} data points)`;
}

// ── Chart (SVG) ───────────────────────────────────────────────────────────────
const CH = { h: 260, padT: 10, padR: 8, padB: 26, padL: 54 };
let chartGeom = null; // saved for hover hit-testing

function renderChart() {
  if (!series.length) return;
  const W = els.chart.clientWidth || 600;
  const { h, padT, padR, padB, padL } = CH;
  const plotW = W - padL - padR;
  const plotH = h - padT - padB;
  const n = series.length;

  let maxV = 0, minS = 0;
  for (const p of series) { maxV = Math.max(maxV, p.gen, p.load); minS = Math.min(minS, p.surplus); }
  const maxY = maxV * 1.15;
  const minY = minS < 0 ? minS * 1.2 : -(maxY * 0.08);

  const xAt = (i) => padL + (n === 1 ? plotW / 2 : (i / (n - 1)) * plotW);
  const yAt = (v) => padT + (1 - (v - minY) / (maxY - minY)) * plotH;
  const zeroY = yAt(0);

  // "Now" — fractional position between bracketing points
  const now = Date.now();
  let nowX = null;
  if (series[0].t > now) nowX = xAt(0);
  else if (series[n - 1].t < now) nowX = xAt(n - 1);
  else for (let i = 1; i < n; i++) {
    if (series[i].t >= now) {
      const f = (now - series[i - 1].t) / (series[i].t - series[i - 1].t);
      nowX = xAt(i - 1 + f); break;
    }
  }

  const line = (key) => series.map((p, i) => `${xAt(i).toFixed(1)},${yAt(p[key]).toFixed(1)}`).join(" ");
  // Surplus area down to the zero baseline (clipped above/below for green/red).
  const areaPts = `${xAt(0).toFixed(1)},${zeroY.toFixed(1)} ` +
    series.map((p, i) => `${xAt(i).toFixed(1)},${yAt(p.surplus).toFixed(1)}`).join(" ") +
    ` ${xAt(n - 1).toFixed(1)},${zeroY.toFixed(1)}`;

  // Y axis labels (skip the very top/bottom to avoid clipping)
  let yTicks = "";
  const steps = 5;
  for (let i = 1; i < steps; i++) {
    const v = minY + ((maxY - minY) * i) / steps;
    const y = yAt(v);
    yTicks += `<line x1="${padL}" y1="${y.toFixed(1)}" x2="${W - padR}" y2="${y.toFixed(1)}" stroke="#e2e8f0" stroke-width="1"/>
      <text x="${padL - 6}" y="${(y + 3).toFixed(1)}" text-anchor="end" font-size="10" fill="#94a3b8">${fmtMW(v)}</text>`;
  }

  // X axis labels (~8 across)
  let xTicks = "";
  const every = Math.max(1, Math.ceil(n / 8));
  for (let i = 0; i < n; i += every) {
    const x = xAt(i);
    xTicks += `<text x="${x.toFixed(1)}" y="${h - padB + 16}" text-anchor="middle" font-size="10" fill="#94a3b8">${fmtTime(series[i].t)}</text>`;
  }

  const nowMarker = nowX != null ? `
    <line x1="${nowX.toFixed(1)}" y1="${padT}" x2="${nowX.toFixed(1)}" y2="${h - padB}" stroke="#ea580c" stroke-width="1.5" stroke-dasharray="6 3"/>
    <text x="${nowX.toFixed(1)}" y="${padT + 9}" text-anchor="middle" font-size="10" font-weight="600" fill="#c2410c">Now</text>` : "";

  els.chart.innerHTML = `
    <svg viewBox="0 0 ${W} ${h}" width="${W}" height="${h}" id="chart-svg" style="touch-action:none">
      <defs>
        <clipPath id="clip-pos"><rect x="0" y="0" width="${W}" height="${zeroY.toFixed(1)}"/></clipPath>
        <clipPath id="clip-neg"><rect x="0" y="${zeroY.toFixed(1)}" width="${W}" height="${(h - zeroY).toFixed(1)}"/></clipPath>
      </defs>
      ${yTicks}
      <line x1="${padL}" y1="${zeroY.toFixed(1)}" x2="${W - padR}" y2="${zeroY.toFixed(1)}" stroke="#cbd5e1" stroke-width="1" stroke-dasharray="6 4"/>
      <polygon points="${areaPts}" fill="${FILL_POS}" clip-path="url(#clip-pos)"/>
      <polygon points="${areaPts}" fill="${FILL_NEG}" clip-path="url(#clip-neg)"/>
      <polyline points="${line("gen")}" fill="none" stroke="${C_GEN}" stroke-width="2.5" stroke-linejoin="round"/>
      <polyline points="${line("load")}" fill="none" stroke="${C_LOAD}" stroke-width="2.5" stroke-linejoin="round"/>
      <polyline points="${line("surplus")}" fill="none" stroke="${C_SURPLUS}" stroke-width="2" stroke-linejoin="round"/>
      ${nowMarker}
      <line id="hover-line" x1="0" y1="${padT}" x2="0" y2="${h - padB}" stroke="#64748b" stroke-width="1" stroke-dasharray="3 3" visibility="hidden"/>
      <rect x="${padL}" y="${padT}" width="${plotW}" height="${plotH}" fill="transparent" id="hover-zone"/>
      ${xTicks}
    </svg>`;

  chartGeom = { W, padL, padR, plotW, n, xAt, yAt };
  attachHover();
}

function attachHover() {
  const zone = $("hover-zone");
  const hline = $("hover-line");
  if (!zone) return;
  const { padL, plotW, n } = chartGeom;
  const move = (clientX) => {
    const rect = $("chart-svg").getBoundingClientRect();
    const scale = chartGeom.W / rect.width;
    const sx = (clientX - rect.left) * scale;
    const frac = clamp((sx - padL) / plotW, 0, 1);
    const i = Math.round(frac * (n - 1));
    const p = series[i];
    const x = chartGeom.xAt(i);
    hline.setAttribute("x1", x); hline.setAttribute("x2", x);
    hline.setAttribute("visibility", "visible");
    const surplus = p.surplus >= 0;
    els.tooltip.innerHTML =
      `${fmtTime(p.t)}\n` +
      `<span style="color:#81C784">● Gen:  ${fmtMW(p.gen)}</span>\n` +
      `<span style="color:#64B5F6">● Load: ${fmtMW(p.load)}</span>\n` +
      `<span style="color:${surplus ? "#81C784" : "#EF9A9A"}">● ${surplus ? "Surplus" : "Deficit"}: ${fmtMW(Math.abs(p.surplus))}</span>`;
    els.tooltip.style.whiteSpace = "pre";
    els.tooltip.classList.remove("hidden");
    // Position within the chart section (tooltip is absolutely placed)
    const left = clamp((x / chartGeom.W) * rect.width + 12, 0, rect.width - 120);
    els.tooltip.style.left = `${left}px`;
    els.tooltip.style.top = `8px`;
  };
  zone.addEventListener("mousemove", (e) => move(e.clientX));
  zone.addEventListener("touchstart", (e) => move(e.touches[0].clientX), { passive: true });
  zone.addEventListener("touchmove", (e) => move(e.touches[0].clientX), { passive: true });
  const hide = () => { hline.setAttribute("visibility", "hidden"); els.tooltip.classList.add("hidden"); };
  zone.addEventListener("mouseleave", hide);
  zone.addEventListener("touchend", hide);
}

// ── Interpretation hint ─────────────────────────────────────────────────────
function renderHint() {
  const peak = peakSurplusPoint();
  const good = peak.surplus > 0;
  const box = $("hint");
  box.style.background = good ? "#ecfdf5" : "#fffbeb";
  box.style.borderColor = good ? "#a7f3d0" : "#fde68a";
  const color = good ? "#15803d" : "#b45309";
  const bulb = '<svg class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 18h6M10 22h4M12 2a7 7 0 0 0-4 12.7c.6.5 1 1.3 1 2.3h6c0-1 .4-1.8 1-2.3A7 7 0 0 0 12 2z"/></svg>';
  const info = '<svg class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><line x1="12" y1="16" x2="12" y2="12"/><line x1="12" y1="8" x2="12.01" y2="8"/></svg>';
  $("hint-icon").innerHTML = `<span style="color:${color}">${good ? bulb : info}</span>`;
  $("hint-text").textContent = good
    ? `Best time to charge an EV or run high-energy appliances: ${fmtTime(peak.t)} — that's when renewables exceed demand by the most.`
    : `Renewables don't cover full demand in this window. Highest renewable share is around ${fmtTime(peak.t)}.`;
}

// ── Events ────────────────────────────────────────────────────────────────────
els.country.addEventListener("change", (e) => { country = e.target.value; loadData(); });
els.hours.addEventListener("change", (e) => { hours = +e.target.value; loadData(); });
els.refresh.addEventListener("click", () => loadData());
els.retry.addEventListener("click", () => loadData());

let resizeTimer;
window.addEventListener("resize", () => {
  clearTimeout(resizeTimer);
  resizeTimer = setTimeout(() => { if (series.length && !els.dashboard.classList.contains("hidden")) renderChart(); }, 150);
});

// ── Boot ────────────────────────────────────────────────────────────────────
(async () => {
  await loadCountries();
  await loadData();
})();
