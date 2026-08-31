use axum::response::Html;

pub async fn index() -> Html<&'static str> {
    Html(CALIBRATION_HTML)
}

const CALIBRATION_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>ATmega328P Spectrometer</title>
<style>
* { box-sizing: border-box; margin: 0; padding: 0; }
body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; background: #1a1a2e; color: #e0e0e0; display: flex; height: 100vh; }
.sidebar { width: 280px; background: #16213e; padding: 20px; overflow-y: auto; border-right: 1px solid #0f3460; }
.main { flex: 1; padding: 20px; display: flex; flex-direction: column; gap: 16px; overflow-y: auto; }
h1 { font-size: 18px; margin-bottom: 16px; color: #e94560; }
h2 { font-size: 13px; color: #a0a0b0; margin-bottom: 8px; text-transform: uppercase; letter-spacing: 1px; }
.badge { display: inline-block; padding: 2px 8px; border-radius: 4px; font-size: 12px; font-weight: bold; }
.badge-ok { background: #2d6a4f; color: #b7e4c7; }
.badge-warn { background: #9a6700; color: #ffd500; }
.badge-err { background: #6a2c2c; color: #f4a0a0; }
.section { margin-bottom: 20px; }
.stat { display: flex; justify-content: space-between; padding: 4px 0; font-size: 13px; border-bottom: 1px solid #0f3460; }
.stat-label { color: #a0a0b0; }
.stat-value { font-family: 'Courier New', monospace; }
select, input[type=number] { width: 100%; padding: 6px 8px; margin: 4px 0; background: #0f3460; border: 1px solid #1d4ed8; border-radius: 4px; color: #e0e0e0; font-size: 13px; }
button { width: 100%; padding: 10px; margin: 4px 0; border: none; border-radius: 6px; cursor: pointer; font-size: 13px; font-weight: bold; }
.btn-save { background: #1d4ed8; color: white; }
.btn-save:hover { background: #2563eb; }
.chart-container { background: #16213e; border-radius: 8px; padding: 16px; border: 1px solid #0f3460; flex: 1; min-height: 0; }
.chart-title { font-size: 13px; color: #a0a0b0; margin-bottom: 8px; }
canvas { width: 100%; height: 100%; display: block; }
.header { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; }
.header-badges { display: flex; gap: 6px; }
.formula { font-family: 'Courier New', monospace; font-size: 11px; color: #a0a0b0; background: #0f3460; padding: 8px; border-radius: 4px; margin-top: 8px; }
#cycle-counter { font-family: 'Courier New', monospace; font-size: 12px; color: #a0a0b0; }
.settings-note { font-size: 11px; color: #6b7280; margin-top: 4px; }
.log-panel { background: #0a0a1a; border-radius: 8px; border: 1px solid #0f3460; padding: 8px; font-family: 'Courier New', monospace; font-size: 11px; overflow-y: auto; max-height: 200px; min-height: 100px; }
.log-panel .cmd { color: #fbbf24; }
.log-panel .err { color: #ef4444; }
.log-panel .data { color: #a0a0b0; }
.log-panel .cycle { color: #22c55e; }
.wl-row { display: flex; gap: 6px; align-items: center; }
.wl-row input { flex: 1; margin: 0; }
.wl-row button { width: auto; padding: 6px 14px; margin: 0; }
.btn-go { background: #0f766e; color: white; }
.btn-go:hover { background: #14b8a6; }
.btn-go:disabled { background: #334155; color: #64748b; cursor: not-allowed; }
.mono-err { font-size: 11px; color: #f4a0a0; margin-top: 6px; word-break: break-word; }
</style>
</head>
<body>
<div class="sidebar">
  <h1>ATmega328P</h1>

  <div class="section">
    <h2>Monochromator</h2>
    <div class="wl-row">
      <input type="number" id="wl-input" min="1" step="0.1" placeholder="nm">
      <button class="btn-go" id="wl-go" onclick="setWavelength()">Go</button>
    </div>
    <div class="stat"><span class="stat-label">Setpoint</span><span class="stat-value" id="wl-set">-</span></div>
    <div class="stat"><span class="stat-label">Actual</span><span class="stat-value" id="wl-actual">-</span></div>
    <div class="stat"><span class="stat-label">Status</span><span class="stat-value"><span class="badge" id="mono-badge">-</span></span></div>
    <div class="settings-note" id="mono-instrument"></div>
    <div class="mono-err" id="mono-error" style="display:none"></div>
  </div>

  <div class="section">
    <h2>Device Settings</h2>
    <label class="stat-label" for="sel-gain">GAIN</label>
    <select id="sel-gain" onchange="saveSettings()">
      <option value="1">1</option><option value="2" selected>2</option>
      <option value="4">4</option><option value="8">8</option>
      <option value="16">16</option><option value="32">32</option>
      <option value="64">64</option><option value="128">128</option>
    </select>
    <label class="stat-label" for="sel-fadc">FADC (Hz)</label>
    <select id="sel-fadc" onchange="saveSettings()">
      <option value="500">500</option><option value="250" selected>250</option>
      <option value="125">125</option><option value="62.5">62.5</option>
      <option value="50">50</option><option value="39.2">39.2</option>
      <option value="33.3">33.3</option><option value="19.6">19.6</option>
      <option value="16.7">16.7</option><option value="12.5">12.5</option>
      <option value="10">10</option><option value="8.33">8.33</option>
      <option value="6.25">6.25</option><option value="4.17">4.17</option>
    </select>
    <label class="stat-label" for="sel-count">COUNT</label>
    <select id="sel-count" onchange="saveSettings()">
      <option value="1">1</option><option value="2">2</option>
      <option value="3">3</option><option value="4" selected>4</option>
      <option value="5">5</option><option value="6">6</option>
      <option value="7">7</option><option value="8">8</option>
      <option value="9">9</option><option value="10">10</option>
      <option value="11">11</option><option value="12">12</option>
    </select>
  </div>

  <div class="section">
    <h2>Series Mapping</h2>
    <div class="settings-note" style="margin-bottom:6px">Which SERIES is which channel?</div>
    <label class="stat-label" for="map-dark">Dark</label>
    <select id="map-dark" onchange="saveSettings()">
      <option value="1" selected>SERIES 1</option>
      <option value="2">SERIES 2</option>
      <option value="3">SERIES 3</option>
    </select>
    <label class="stat-label" for="map-full">Full (reference)</label>
    <select id="map-full" onchange="saveSettings()">
      <option value="1">SERIES 1</option>
      <option value="2" selected>SERIES 2</option>
      <option value="3">SERIES 3</option>
    </select>
    <label class="stat-label" for="map-sample">Sample</label>
    <select id="map-sample" onchange="saveSettings()">
      <option value="1">SERIES 1</option>
      <option value="2">SERIES 2</option>
      <option value="3" selected>SERIES 3</option>
    </select>
    <button class="btn-save" onclick="saveSettings()">Save Settings</button>
    <div class="settings-note">Mapping applies immediately. Save persists to disk.</div>
  </div>

  <div class="section">
    <h2>Live Values</h2>
    <div class="stat"><span class="stat-label">T%</span><span class="stat-value" id="v-t">-</span></div>
    <div class="stat"><span class="stat-label">Dark mean</span><span class="stat-value" id="v-dark">-</span></div>
    <div class="stat"><span class="stat-label">Full mean</span><span class="stat-value" id="v-full">-</span></div>
    <div class="stat"><span class="stat-label">Sample mean</span><span class="stat-value" id="v-sample">-</span></div>
  </div>

  <div class="formula">T% = (sample - dark) / (full - dark) * 100</div>
  <div style="margin-top: 12px"><span id="cycle-counter">Cycles: 0</span></div>
</div>

<div class="main">
  <div class="header">
    <h2>Live Monitoring</h2>
    <div class="header-badges">
      <span class="badge" id="clip-badge" style="display:none">CLIPPED</span>
      <span class="badge badge-warn" id="ws-badge">Connecting...</span>
    </div>
  </div>

  <div class="chart-container" style="flex:2">
    <div class="chart-title">Calibrated Transmittance (%) &mdash; <span style="color:#94a3b8">grey = grating moving</span></div>
    <canvas id="chart-t"></canvas>
  </div>

  <div class="chart-container" style="flex:1">
    <div class="chart-title">Raw Means &mdash; <span style="color:#ef4444">dark</span> <span style="color:#22c55e">full</span> <span style="color:#3b82f6">sample</span></div>
    <canvas id="chart-raw"></canvas>
  </div>

  <div style="flex:0 0 auto">
    <div class="chart-title">Serial Log</div>
    <div class="log-panel" id="serial-log"></div>
  </div>
</div>

<script>
const MAX = 300;
const D = { t: [], dark: [], full: [], sample: [], clip: [], moving: [] };
let ws, cycles = 0;

function connect() {
  const p = location.protocol === 'https:' ? 'wss:' : 'ws:';
  ws = new WebSocket(`${p}//${location.host}/ws`);
  ws.onopen = () => { el('ws-badge').className='badge badge-ok'; el('ws-badge').textContent='Connected'; };
  ws.onclose = () => { el('ws-badge').className='badge badge-warn'; el('ws-badge').textContent='Reconnecting...'; setTimeout(connect,2000); };
  ws.onmessage = (e) => {
    const m = JSON.parse(e.data);
    if (m.type==='init') onInit(m);
    else if (m.type==='cycle') onCycle(m);
    else if (m.type==='settings_updated') onSettingsUpdated(m);
    else if (m.type==='log') onLog(m);
    else if (m.type==='mono') onMono(m);
  };
}

function el(id) { return document.getElementById(id); }
function fmt(n) { return Math.abs(n)>1e5 ? n.toExponential(2) : n.toFixed(2); }

function onInit(m) {
  el('sel-gain').value = m.device_settings.gain;
  el('sel-fadc').value = m.device_settings.fadc;
  el('sel-count').value = m.device_settings.count;
  if (m.series_mapping) {
    el('map-dark').value = m.series_mapping.dark;
    el('map-full').value = m.series_mapping.full;
    el('map-sample').value = m.series_mapping.sample;
  }
}

function onCycle(m) {
  cycles++;
  el('cycle-counter').textContent = `Cycles: ${cycles}`;
  el('v-t').textContent = fmt(m.calibrated_reading) + '%';
  el('v-dark').textContent = fmt(m.dark_mean);
  el('v-full').textContent = fmt(m.full_mean);
  el('v-sample').textContent = fmt(m.sample_mean);

  D.t.push(m.calibrated_reading);
  D.dark.push(m.dark_mean);
  D.full.push(m.full_mean);
  D.sample.push(m.sample_mean);
  D.clip.push(m.is_clipped);
  D.moving.push(monoMoving);
  for (const k of Object.keys(D)) { if (D[k].length>MAX) D[k].shift(); }

  const cb = el('clip-badge');
  if (m.is_clipped) { cb.style.display=''; cb.className='badge badge-err'; }
  else { cb.style.display='none'; }

  draw();
}

function onSettingsUpdated(m) {
  el('sel-gain').value = m.gain;
  el('sel-fadc').value = m.fadc;
  el('sel-count').value = m.count;
  if (m.series_mapping) {
    el('map-dark').value = m.series_mapping.dark;
    el('map-full').value = m.series_mapping.full;
    el('map-sample').value = m.series_mapping.sample;
  }
}

let monoMoving = false;

function onMono(m) {
  monoMoving = m.moving;
  el('wl-set').textContent = m.control_wavelength.toFixed(1) + ' nm';
  el('wl-actual').textContent = m.actual_wavelength==null ? '-' : m.actual_wavelength.toFixed(1)+' nm';
  if (document.activeElement !== el('wl-input') && !el('wl-input').value) {
    el('wl-input').value = m.control_wavelength.toFixed(1);
  }
  el('mono-instrument').textContent = m.instrument || '';

  const b = el('mono-badge');
  if (!m.hardware) { b.className='badge badge-warn'; b.textContent='LABEL ONLY'; }
  else if (m.error) { b.className='badge badge-err'; b.textContent='ERROR'; }
  else if (m.moving) { b.className='badge badge-warn'; b.textContent='MOVING'; }
  else { b.className='badge badge-ok'; b.textContent='READY'; }

  const e = el('mono-error');
  e.style.display = m.error ? '' : 'none';
  e.textContent = m.error || '';

  el('wl-go').disabled = m.moving;
  el('wl-go').textContent = m.moving ? '...' : 'Go';
}

async function setWavelength() {
  const nm = parseFloat(el('wl-input').value);
  if (!isFinite(nm)) return;
  el('wl-go').disabled = true;
  try {
    const r = await fetch('/control_wavelength', {
      method:'POST', headers:{'Content-Type':'application/json'},
      body: JSON.stringify({ wavelength: nm }),
    });
    onMono({ ...(await r.json()), type:'mono', moving:false });
  } finally {
    el('wl-go').disabled = monoMoving;
  }
}

const MAX_LOG = 500;
function onLog(m) {
  const log = el('serial-log');
  const line = document.createElement('div');
  const t = m.line;

  if (t.startsWith('>')) {
    line.className='cmd';
    line.innerHTML = highlight(t);
  } else if (t.startsWith('ERROR') || t.includes('missing')) {
    line.className='err';
    line.textContent = t;
  } else if (t.startsWith('END_CYCLE')) {
    line.className='cycle';
    line.textContent = t;
  } else if (t.match(/^(OK )?(GAIN|FADC|COUNT)=/)) {
    line.className='cmd';
    line.innerHTML = highlight(t);
  } else if (t.startsWith('SERIES')) {
    line.className='data';
    line.innerHTML = highlight(t);
  } else {
    line.className='data';
    line.textContent = t;
  }
  log.appendChild(line);
  if (log.children.length > MAX_LOG) log.removeChild(log.firstChild);
  log.scrollTop = log.scrollHeight;
}

function highlight(t) {
  return t
    .replace(/^(&gt;|>)\s*/, '<span style="color:#fbbf24">&gt; </span>')
    .replace(/(SERIES[123])/g, '<span style="color:#60a5fa">$1</span>')
    .replace(/(GAIN|FADC|COUNT)(=)(\S+)/g, '<span style="color:#c084fc">$1</span>$2<span style="color:#34d399">$3</span>')
    .replace(/(OK)\s/g, '<span style="color:#22c55e">$1</span> ')
    .replace(/(END_CYCLE)/g, '<span style="color:#22c55e">$1</span>')
    .replace(/(16777215)/g, '<span style="color:#ef4444;font-weight:bold">$1</span>');
}

async function saveSettings() {
  const body = {
    gain: parseInt(el('sel-gain').value),
    fadc: parseFloat(el('sel-fadc').value),
    count: parseInt(el('sel-count').value),
    series_mapping: {
      dark: parseInt(el('map-dark').value),
      full: parseInt(el('map-full').value),
      sample: parseInt(el('map-sample').value),
    },
  };
  await fetch('/api/settings', { method:'POST', headers:{'Content-Type':'application/json'}, body:JSON.stringify(body) });
}

function draw() {
  drawLine('chart-t', D.t, '#e94560', true);
  drawRaw('chart-raw');
}

// Vertical band wherever `flags[i]` is set — marks samples that are not
// trustworthy (clipped ADC, or grating in motion).
function shade(ctx, p, pw, ph, flags, color) {
  ctx.fillStyle = color;
  for (let i=0;i<flags.length;i++) {
    if (!flags[i]) continue;
    const x=p.l+(i/(MAX-1))*pw;
    ctx.fillRect(x-1,p.t,3,ph);
  }
}

function drawLine(id, vals, color, fixedRange) {
  const c = el(id), ctx = c.getContext('2d'), dpr = devicePixelRatio||1;
  c.width = c.clientWidth*dpr; c.height = c.clientHeight*dpr;
  ctx.scale(dpr,dpr);
  const w=c.clientWidth, h=c.clientHeight, p={t:10,r:55,b:20,l:10}, pw=w-p.l-p.r, ph=h-p.t-p.b;
  ctx.clearRect(0,0,w,h);

  let yMin=0, yMax=100;
  if (!fixedRange && vals.length>0) { yMin=Math.min(...vals)*0.95; yMax=Math.max(...vals)*1.05; }

  grid(ctx,p,pw,ph,yMin,yMax,'');
  shade(ctx,p,pw,ph,D.moving,'rgba(148,163,184,0.25)');
  if (vals.length<2) return;
  ctx.strokeStyle=color; ctx.lineWidth=1.5; ctx.beginPath();
  for (let i=0;i<vals.length;i++) {
    const x=p.l+(i/(MAX-1))*pw, y=p.t+(1-(vals[i]-yMin)/(yMax-yMin))*ph;
    i===0?ctx.moveTo(x,y):ctx.lineTo(x,y);
  }
  ctx.stroke();
}

function drawRaw(id) {
  const c = el(id), ctx = c.getContext('2d'), dpr = devicePixelRatio||1;
  c.width = c.clientWidth*dpr; c.height = c.clientHeight*dpr;
  ctx.scale(dpr,dpr);
  const w=c.clientWidth, h=c.clientHeight, p={t:10,r:65,b:20,l:10}, pw=w-p.l-p.r, ph=h-p.t-p.b;
  ctx.clearRect(0,0,w,h);

  const all=[...D.dark,...D.full,...D.sample];
  if (!all.length) return;
  const yMin=Math.min(...all)*0.95, yMax=Math.max(...all)*1.05;
  grid(ctx,p,pw,ph,yMin,yMax,'');

  for (const [vals,col] of [[D.dark,'#ef4444'],[D.full,'#22c55e'],[D.sample,'#3b82f6']]) {
    if (vals.length<2) continue;
    ctx.strokeStyle=col; ctx.lineWidth=1.5; ctx.beginPath();
    for (let i=0;i<vals.length;i++) {
      const x=p.l+(i/(MAX-1))*pw, y=p.t+(1-(vals[i]-yMin)/(yMax-yMin))*ph;
      i===0?ctx.moveTo(x,y):ctx.lineTo(x,y);
    }
    ctx.stroke();
  }

  shade(ctx,p,pw,ph,D.moving,'rgba(148,163,184,0.25)');
  shade(ctx,p,pw,ph,D.clip,'rgba(233,69,96,0.3)');
}

function grid(ctx,p,pw,ph,yMin,yMax,sfx) {
  ctx.strokeStyle='#0f3460'; ctx.lineWidth=0.5;
  for (let i=0;i<=4;i++) {
    const y=p.t+(ph/4)*i;
    ctx.beginPath(); ctx.moveTo(p.l,y); ctx.lineTo(p.l+pw,y); ctx.stroke();
    ctx.fillStyle='#a0a0b0'; ctx.font='10px monospace'; ctx.textAlign='left';
    ctx.fillText(fmt(yMax-(yMax-yMin)*(i/4))+sfx, p.l+pw+4, y+4);
  }
}

connect();
</script>
</body>
</html>"##;
