use axum::response::Html;

pub async fn index() -> Html<&'static str> {
    Html(DASHBOARD_HTML)
}

const DASHBOARD_HTML: &str = r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>OptiReOpt Bridge — Live Spectrum</title>
<style>
  :root {
    --bg: #0e1116;
    --panel: #161b22;
    --border: #2a313c;
    --text: #d6dde6;
    --muted: #8a94a3;
    --green: #3fb950;
    --red: #f85149;
    --accent: #58a6ff;
  }
  html, body { background: var(--bg); color: var(--text); margin: 0; height: 100%; font: 14px/1.4 -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; }
  body { display: flex; flex-direction: column; }
  header { padding: 12px 16px; background: var(--panel); border-bottom: 1px solid var(--border); display: flex; align-items: center; gap: 16px; flex-wrap: wrap; }
  h1 { font-size: 15px; margin: 0; font-weight: 600; }
  .pill { display: inline-flex; align-items: center; gap: 6px; padding: 3px 10px; border-radius: 999px; background: #20262d; font-size: 12px; }
  .pill::before { content: ""; width: 8px; height: 8px; border-radius: 50%; background: var(--muted); }
  .pill.connected::before { background: var(--green); }
  .pill.disconnected::before { background: var(--red); }
  .pill.lzh-initializing::before, .pill.lzh-calibrating::before { background: #d29922; }
  .pill.lzh-ready::before { background: var(--accent); }
  .pill.lzh-depositing::before { background: var(--green); }
  .pill.lzh-end_of_layer_pulse::before { background: #db61a2; }
  .pill.lzh-complete::before { background: var(--muted); }
  .meta { color: var(--muted); font-size: 12px; display: flex; gap: 14px; flex-wrap: wrap; }
  .meta b { color: var(--text); font-weight: 500; }
  .lzh-controls { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  .lzh-controls button { background: #20262d; color: var(--text); border: 1px solid var(--border); border-radius: 6px; padding: 4px 10px; font-size: 12px; cursor: pointer; }
  .lzh-controls button:hover { background: #2a313c; }
  .lzh-controls button:disabled { opacity: 0.35; cursor: not-allowed; }
  .lzh-controls button.primary { background: var(--accent); color: #0e1116; border-color: var(--accent); }
  .lzh-controls button.danger { background: var(--red); color: #0e1116; border-color: var(--red); }
  .lzh-controls.hidden { display: none; }
  main { flex: 1; padding: 16px; display: flex; flex-direction: column; gap: 12px; min-height: 0; }
  #chart-wrap { flex: 1; background: var(--panel); border: 1px solid var(--border); border-radius: 8px; padding: 8px; min-height: 0; }
  #chart { display: block; width: 100%; height: 100%; }
  footer { color: var(--muted); font-size: 11px; padding: 8px 16px; border-top: 1px solid var(--border); }
  code { background: #20262d; padding: 1px 6px; border-radius: 4px; }
</style>
</head>
<body>
<header>
  <h1>OptiReOpt Bridge</h1>
  <span id="src-pill" class="pill">source: connecting…</span>
  <span id="lzh-pill" class="pill" style="display:none">LZH: —</span>
  <span id="lzh-link-pill" class="pill" style="display:none">PCS link: —</span>
  <div class="meta">
    <span>last frame: <b id="m-ts">—</b></span>
    <span>n: <b id="m-n">0</b></span>
    <span>min: <b id="m-min">—</b></span>
    <span>mean: <b id="m-mean">—</b></span>
    <span>max: <b id="m-max">—</b></span>
    <span>rate: <b id="m-rate">0.0</b> scan/s</span>
    <span>rt: <b id="m-rt">—</b></span>
    <span>material: <b id="m-mat">—</b></span>
    <span>switches: <b id="m-sw">0</b></span>
    <span id="lzh-layer-meta" style="display:none">layer: <b id="m-layer">—</b>/<b id="m-layers">—</b></span>
    <span id="lzh-hb-meta" style="display:none">hb: <b id="m-hb">—</b></span>
  </div>
  <div id="lzh-controls" class="lzh-controls hidden">
    <button id="btn-cal-complete">Mark Cal Complete</button>
    <button id="btn-end-layer">End Layer Now</button>
    <button id="btn-stop" class="danger">Stop</button>
  </div>
</header>
<section id="lzh-panel" style="display:none; padding: 12px 16px; background: var(--panel); border-bottom: 1px solid var(--border);">
  <div style="display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 12px;">
    <div>
      <div style="color: var(--muted); font-size: 11px; margin-bottom: 4px;">PCS → bridge</div>
      <div>Deposit: <b id="pcs-deposit">—</b></div>
      <div>ACP: <b id="pcs-acp">—</b></div>
      <div>AutoScaling: <b id="pcs-autoscaling">—</b></div>
    </div>
    <div>
      <div style="color: var(--muted); font-size: 11px; margin-bottom: 4px;">measurement</div>
      <div>thickness: <b id="meas-thick">—</b> nm</div>
      <div>rate: <b id="meas-rate">—</b> nm/s</div>
      <div>mean rate: <b id="meas-mean">—</b> nm/s</div>
      <div>remaining: <b id="meas-rem">—</b> s</div>
    </div>
    <div>
      <div style="color: var(--muted); font-size: 11px; margin-bottom: 4px;">current layer design</div>
      <div>thickness: <b id="dsn-thick">—</b> nm</div>
      <div>rate: <b id="dsn-rate">—</b> nm/s</div>
      <div>n: <b id="dsn-n">—</b></div>
      <div>λ: <b id="dsn-cwl">—</b> nm</div>
    </div>
  </div>
</section>
<main>
  <div id="chart-wrap"><canvas id="chart"></canvas></div>
</main>
<footer>
  Endpoints: <code>GET /device/info</code> · <code>GET /latest</code> · <code>POST /ingest</code> · <code>POST /register</code> · <code>GET /ws</code>
</footer>
<script>
(function () {
  const cv = document.getElementById('chart');
  const ctx = cv.getContext('2d');
  const srcPill = document.getElementById('src-pill');
  const mTs = document.getElementById('m-ts');
  const mN = document.getElementById('m-n');
  const mMin = document.getElementById('m-min');
  const mMean = document.getElementById('m-mean');
  const mMax = document.getElementById('m-max');
  const mRate = document.getElementById('m-rate');
  const mRt = document.getElementById('m-rt');
  const mMat = document.getElementById('m-mat');
  const mSw = document.getElementById('m-sw');
  const lzhPill = document.getElementById('lzh-pill');
  const lzhLinkPill = document.getElementById('lzh-link-pill');
  const lzhLayerMeta = document.getElementById('lzh-layer-meta');
  const lzhHbMeta = document.getElementById('lzh-hb-meta');
  const lzhControls = document.getElementById('lzh-controls');
  const mLayer = document.getElementById('m-layer');
  const mLayers = document.getElementById('m-layers');
  const mHb = document.getElementById('m-hb');
  const btnCalComplete = document.getElementById('btn-cal-complete');
  const btnEndLayer = document.getElementById('btn-end-layer');
  const btnStop = document.getElementById('btn-stop');
  let switchCount = 0;
  let lzhState = null;
  let lzhLastInFrames = 0;
  let lzhLastInFramesAt = 0;
  function refreshLzhLink() {
    if (!lzhState) return;
    const elapsed = lzhLastInFramesAt > 0 ? (performance.now() - lzhLastInFramesAt) : Infinity;
    const stale = elapsed > 1500;
    lzhLinkPill.style.display = '';
    lzhLinkPill.classList.toggle('connected', !stale);
    lzhLinkPill.classList.toggle('disconnected', stale);
    lzhLinkPill.textContent = stale
      ? 'PCS link: no frames'
      : `PCS link: ${(elapsed/1000).toFixed(1)}s ago`;
  }
  setInterval(refreshLzhLink, 500);

  const lzhPanel = document.getElementById('lzh-panel');
  const pcsDeposit = document.getElementById('pcs-deposit');
  const pcsAcp = document.getElementById('pcs-acp');
  const pcsAutoscaling = document.getElementById('pcs-autoscaling');
  const measThick = document.getElementById('meas-thick');
  const measRate = document.getElementById('meas-rate');
  const measMean = document.getElementById('meas-mean');
  const measRem = document.getElementById('meas-rem');
  const dsnThick = document.getElementById('dsn-thick');
  const dsnRate = document.getElementById('dsn-rate');
  const dsnN = document.getElementById('dsn-n');
  const dsnCwl = document.getElementById('dsn-cwl');
  function bool(v) { return v === true ? 'T' : v === false ? 'F' : '—'; }
  function num(v, digits) {
    if (v === null || v === undefined || !isFinite(v)) return '—';
    return Number(v).toFixed(digits ?? 3);
  }
  function applyLzh(snap) {
    if (!snap) {
      lzhPill.style.display = 'none';
      lzhLayerMeta.style.display = 'none';
      lzhHbMeta.style.display = 'none';
      lzhControls.classList.add('hidden');
      lzhPanel.style.display = 'none';
      lzhLinkPill.style.display = 'none';
      lzhState = null;
      return;
    }
    lzhState = snap.state;
    if (typeof snap.in_frames_received === 'number' && snap.in_frames_received !== lzhLastInFrames) {
      lzhLastInFrames = snap.in_frames_received;
      lzhLastInFramesAt = performance.now();
    }
    refreshLzhLink();
    lzhPill.style.display = '';
    lzhLayerMeta.style.display = '';
    lzhHbMeta.style.display = '';
    lzhPanel.style.display = '';
    lzhPill.className = 'pill lzh-' + snap.state;
    lzhPill.textContent = 'LZH: ' + snap.state.replace(/_/g, ' ');
    mLayer.textContent = snap.current_layer;
    mLayers.textContent = snap.layers || '—';
    mHb.textContent = snap.heartbeat;
    lzhControls.classList.remove('hidden');
    btnCalComplete.disabled = snap.state !== 'calibrating';
    btnEndLayer.disabled = snap.state !== 'depositing';
    btnStop.disabled = snap.state === 'initializing' || snap.state === 'complete';

    const pcs = snap.pcs || {};
    pcsDeposit.textContent = bool(pcs.deposit);
    pcsAcp.textContent = bool(pcs.acp);
    pcsAutoscaling.textContent = bool(pcs.auto_scaling);

    const m = snap.measurement || {};
    measThick.textContent = num(m.current_thickness);
    measRate.textContent = num(m.current_rate, 4);
    measMean.textContent = num(m.mean_rate, 4);
    measRem.textContent = num(m.remaining_time, 1);

    const d = snap.design;
    if (d) {
      dsnThick.textContent = num(d.design_thickness);
      dsnRate.textContent = num(d.design_rate, 4);
      dsnN.textContent = d.n_index ?? '—';
      dsnCwl.textContent = num(d.central_wavelength);
    } else {
      dsnThick.textContent = '—'; dsnRate.textContent = '—';
      dsnN.textContent = '—'; dsnCwl.textContent = '—';
    }
  }

  async function lzhPost(path) {
    try {
      const r = await fetch(path, { method: 'POST' });
      if (!r.ok) console.warn(path, r.status);
    } catch (e) { console.warn(path, e); }
  }
  btnCalComplete.addEventListener('click', () => lzhPost('/vacuum_chamber/lzh/calibration_complete'));
  btnEndLayer.addEventListener('click', () => lzhPost('/vacuum_chamber/lzh/end_layer'));
  btnStop.addEventListener('click', () => lzhPost('/vacuum_chamber/stop'));

  let latest = null;          // { wavelengths, values, rt_data, timestamp }
  let scansReceived = 0;
  let lastFrameAt = 0;        // performance.now() of last received frame
  const recentTs = [];        // wall-clock arrival times for rate calc
  const FRESH_MS = 3000;      // pill is "live" while frames arrive within this window

  function fmt(x) {
    if (x === null || x === undefined || !isFinite(x)) return '—';
    if (Math.abs(x) >= 1000 || (Math.abs(x) > 0 && Math.abs(x) < 0.01)) return x.toExponential(2);
    return x.toFixed(3);
  }

  function setSourceStatus(live) {
    srcPill.classList.toggle('connected', live);
    srcPill.classList.toggle('disconnected', !live);
    srcPill.textContent = 'source: ' + (live ? 'live' : 'idle');
  }

  function refreshLiveness() {
    const live = lastFrameAt > 0 && (performance.now() - lastFrameAt) < FRESH_MS;
    setSourceStatus(live);
  }
  setInterval(refreshLiveness, 500);

  function resizeCanvas() {
    const r = cv.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    cv.width = Math.max(1, Math.floor(r.width * dpr));
    cv.height = Math.max(1, Math.floor(r.height * dpr));
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    draw();
  }
  window.addEventListener('resize', resizeCanvas);

  function draw() {
    const r = cv.getBoundingClientRect();
    const W = r.width, H = r.height;
    ctx.clearRect(0, 0, W, H);
    if (!latest || !latest.wavelengths || latest.wavelengths.length === 0) {
      ctx.fillStyle = '#8a94a3';
      ctx.font = '13px sans-serif';
      ctx.fillText('Waiting for first frame…', 16, 24);
      return;
    }

    const wl = latest.wavelengths;
    const vs = latest.values;
    const n = Math.min(wl.length, vs.length);
    if (n < 2) return;

    let xMin = Infinity, xMax = -Infinity, yMin = Infinity, yMax = -Infinity;
    for (let i = 0; i < n; i++) {
      const x = wl[i], y = vs[i];
      if (x < xMin) xMin = x;
      if (x > xMax) xMax = x;
      if (y < yMin) yMin = y;
      if (y > yMax) yMax = y;
    }
    if (yMin === yMax) { yMin -= 0.5; yMax += 0.5; }
    const yPad = (yMax - yMin) * 0.05;
    yMin -= yPad; yMax += yPad;

    const padL = 56, padR = 12, padT = 10, padB = 30;
    const plotW = W - padL - padR;
    const plotH = H - padT - padB;

    // Axes
    ctx.strokeStyle = '#2a313c';
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(padL, padT);
    ctx.lineTo(padL, padT + plotH);
    ctx.lineTo(padL + plotW, padT + plotH);
    ctx.stroke();

    ctx.fillStyle = '#8a94a3';
    ctx.font = '11px sans-serif';

    // Y gridlines
    const yTicks = 5;
    ctx.strokeStyle = '#1f262e';
    for (let i = 0; i <= yTicks; i++) {
      const t = i / yTicks;
      const y = padT + plotH - t * plotH;
      const yv = yMin + t * (yMax - yMin);
      ctx.beginPath();
      ctx.moveTo(padL, y); ctx.lineTo(padL + plotW, y);
      ctx.stroke();
      ctx.textAlign = 'right'; ctx.textBaseline = 'middle';
      ctx.fillText(fmt(yv), padL - 6, y);
    }
    // X ticks
    const xTicks = 6;
    for (let i = 0; i <= xTicks; i++) {
      const t = i / xTicks;
      const x = padL + t * plotW;
      const xv = xMin + t * (xMax - xMin);
      ctx.textAlign = 'center'; ctx.textBaseline = 'top';
      ctx.fillText(xv.toFixed(0), x, padT + plotH + 4);
    }

    // Line
    ctx.strokeStyle = '#58a6ff';
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    for (let i = 0; i < n; i++) {
      const x = padL + ((wl[i] - xMin) / (xMax - xMin)) * plotW;
      const y = padT + plotH - ((vs[i] - yMin) / (yMax - yMin)) * plotH;
      if (i === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
    }
    ctx.stroke();

    // Axis labels
    ctx.fillStyle = '#8a94a3';
    ctx.textAlign = 'center'; ctx.textBaseline = 'alphabetic';
    ctx.fillText('wavelength (nm)', padL + plotW / 2, H - 6);
    ctx.save();
    ctx.translate(14, padT + plotH / 2);
    ctx.rotate(-Math.PI / 2);
    ctx.fillText('value', 0, 0);
    ctx.restore();
  }

  function applyFrame(frame) {
    latest = frame;
    scansReceived += 1;
    const now = performance.now();
    lastFrameAt = now;
    recentTs.push(now);
    while (recentTs.length > 0 && now - recentTs[0] > 5000) recentTs.shift();

    let mn = Infinity, mx = -Infinity, sum = 0, k = 0;
    for (const v of frame.values) {
      if (!isFinite(v)) continue;
      if (v < mn) mn = v;
      if (v > mx) mx = v;
      sum += v; k++;
    }
    mTs.textContent = frame.timestamp || '—';
    mN.textContent = String(scansReceived);
    mMin.textContent = k ? fmt(mn) : '—';
    mMean.textContent = k ? fmt(sum / k) : '—';
    mMax.textContent = k ? fmt(mx) : '—';
    mRt.textContent = frame.rt_data || '—';
    const rate = recentTs.length > 1
      ? (recentTs.length - 1) / ((recentTs[recentTs.length - 1] - recentTs[0]) / 1000)
      : 0;
    mRate.textContent = rate.toFixed(2);
    draw();
  }

  function connect() {
    const proto = location.protocol === 'https:' ? 'wss' : 'ws';
    const url = proto + '://' + location.host + '/ws';
    const ws = new WebSocket(url);
    ws.onmessage = (ev) => {
      let msg; try { msg = JSON.parse(ev.data); } catch (e) { return; }
      if (msg.type === 'init') {
        scansReceived = msg.scans_received || 0;
        mN.textContent = String(scansReceived);
        applyLzh(msg.lzh);
        if (msg.latest_frame) {
          // Init latest_frame uses the ingest-side names (wavelength/values).
          applyFrame({
            wavelengths: msg.latest_frame.wavelength,
            values: msg.latest_frame.values,
            rt_data: msg.latest_frame.rt_data,
            timestamp: msg.latest_frame.timestamp,
          });
          // applyFrame increments scansReceived and stamps lastFrameAt; rewind
          // the count, and zero lastFrameAt so the pill reflects current
          // ingest activity rather than a stale snapshot at page load.
          scansReceived = msg.scans_received || 0;
          mN.textContent = String(scansReceived);
          lastFrameAt = 0;
        }
        refreshLiveness();
      } else if (msg.type === 'scan') {
        applyFrame(msg);
      } else if (msg.type === 'material_changed') {
        switchCount += 1;
        const frac = (msg.fraction === null || msg.fraction === undefined) ? '' : ` (${msg.fraction}%)`;
        mMat.textContent = (msg.material || '—') + frac;
        mSw.textContent = String(switchCount);
      } else if (msg.type === 'lzh_state') {
        applyLzh(msg);
      }
    };
    ws.onclose = () => {
      lastFrameAt = 0;
      refreshLiveness();
      setTimeout(connect, 1000);
    };
    ws.onerror = () => { try { ws.close(); } catch (e) {} };
  }

  resizeCanvas();
  connect();
})();
</script>
</body>
</html>
"##;
