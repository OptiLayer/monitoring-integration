"""FastAPI device service — OptiMonitor integration + calibration web UI."""

from __future__ import annotations

import asyncio
import logging
from contextlib import asynccontextmanager
from pathlib import Path

import numpy as np
from fastapi import FastAPI, WebSocket, WebSocketDisconnect
from fastapi.responses import HTMLResponse
from pydantic import BaseModel

from .calibration import CalibrationState
from .driver import AcquisitionConfig, HoribaDriver
from .monitoring import MonitoringClient

logger = logging.getLogger(__name__)

# Downsample factor for WebSocket data
DOWNSAMPLE = 1


class RegisterRequest(BaseModel):
    spectrometer_id: str | None = None
    vacuum_chamber_id: str | None = None
    monitoring_api_url: str


class ConfigRequest(BaseModel):
    center_wavelength: float | None = None
    exposure_time_ms: float | None = None
    gain_token: int | None = None
    speed_token: int | None = None


DEFAULT_CALIBRATION_PATH = Path("calibration.json")


def create_app(driver: HoribaDriver, calibration_path: Path = DEFAULT_CALIBRATION_PATH) -> FastAPI:
    cal = CalibrationState()
    cal.load(calibration_path)
    monitoring = MonitoringClient()
    connected_clients: list[WebSocket] = []
    wavelengths: list[float] = []
    is_running = False
    acquisition_task: asyncio.Task | None = None

    async def broadcast(message: dict) -> None:
        dead = []
        for ws in connected_clients:
            try:
                await ws.send_json(message)
            except Exception:
                dead.append(ws)
        for ws in dead:
            connected_clients.remove(ws)

    async def acquisition_loop() -> None:
        nonlocal wavelengths
        await monitoring.start()

        while is_running:
            try:
                spectrum = await driver.acquire()
                wavelengths = spectrum.wavelengths
                intensities = np.array(spectrum.intensities, dtype=np.float64)

                msg: dict = {
                    "type": "scan",
                    "values": intensities[::DOWNSAMPLE].tolist(),
                    "wavelengths": spectrum.wavelengths[::DOWNSAMPLE] if isinstance(spectrum.wavelengths, list) else spectrum.wavelengths,
                    "mean": float(np.mean(intensities)),
                    "calibration": cal.status_dict(),
                }

                if cal.is_calibrated:
                    calibrated = cal.calibrate(intensities)
                    msg["calibrated"] = calibrated[::DOWNSAMPLE].tolist()
                    msg["calibrated_mean"] = float(np.mean(calibrated))

                    # Post to OptiMonitor
                    await monitoring.post_spectral_data(
                        calibrated_readings=calibrated.tolist(),
                        wavelengths=spectrum.wavelengths,
                    )

                await broadcast(msg)

            except asyncio.CancelledError:
                break
            except Exception as e:
                logger.error(f"Acquisition error: {e}")
                await asyncio.sleep(1.0)

        await monitoring.stop()

    @asynccontextmanager
    async def lifespan(_app: FastAPI):
        yield

    app = FastAPI(title="HORIBA iHR320 Device Service", lifespan=lifespan)

    # --- OptiMonitor device endpoints ---

    @app.get("/device/info")
    async def get_device_info():
        return {
            "type": "spectrometer",
            "name": "HORIBA iHR320",
            "capabilities": {
                "has_spectrometer": True,
                "has_vacuum_chamber": True,
                "process_type": "two-component",
                "is_monochromatic": False,
            },
        }

    @app.post("/register")
    async def register(request: RegisterRequest):
        monitoring.register(
            request.monitoring_api_url,
            request.spectrometer_id,
            request.vacuum_chamber_id,
        )
        return {
            "status": "registered",
            "spectrometer_id": request.spectrometer_id,
            "vacuum_chamber_id": request.vacuum_chamber_id,
            "monitoring_api_url": request.monitoring_api_url,
        }

    @app.post("/vacuum_chamber/start")
    async def start_deposition():
        nonlocal is_running, acquisition_task
        if is_running:
            return {"status": "already running"}
        is_running = True
        acquisition_task = asyncio.create_task(acquisition_loop())
        await monitoring.report_shutter(True)
        return {"status": "running"}

    @app.post("/vacuum_chamber/stop")
    async def stop_deposition():
        nonlocal is_running, acquisition_task
        is_running = False
        if acquisition_task:
            acquisition_task.cancel()
            try:
                await acquisition_task
            except asyncio.CancelledError:
                pass
            acquisition_task = None
        await monitoring.report_shutter(False)
        return {"status": "stopped"}

    @app.get("/vacuum_chamber/status")
    async def get_vacuum_chamber_status():
        return {"status": "running" if is_running else "stopped", "is_depositing": is_running}

    @app.get("/vacuum_chamber/material")
    async def get_material():
        return {"material": "H"}

    @app.post("/vacuum_chamber/material")
    async def set_material(payload: dict):
        # No chamber behind this service: play the shutter so OptiMonitor
        # completes the layer switch (closed → next layer, open → fitting).
        if is_running:
            asyncio.create_task(_dummy_switch())
        return {"material": payload.get("material", "H"), "fraction": payload.get("fraction", 100.0)}

    async def _dummy_switch() -> None:
        await monitoring.report_shutter(False)
        await asyncio.sleep(2.0)
        await monitoring.report_shutter(True)

    # --- Calibration endpoints ---
    #
    # Unlike the TECSpec chopper-wheel setup where scans auto-classify by
    # signal level, the HORIBA CCD requires explicit shutter control:
    #   - Dark: shutter CLOSED → CCD acquires detector noise only
    #   - White: shutter OPEN, no sample → full lamp reference
    # The driver handles shutter via acquisition_start(open_shutter=...).

    @app.get("/calibration/status")
    async def get_calibration_status():
        return cal.status_dict()

    class CaptureRequest(BaseModel):
        count: int = 10

    @app.post("/calibration/dark/capture")
    async def capture_dark(request: CaptureRequest = CaptureRequest()):
        """Acquire dark reference: closes shutter, averages N scans."""
        cal.capturing_dark = True
        await broadcast({"type": "capture_status", "capturing": "dark", "count": request.count})
        try:
            cal.dark_ref = await driver.acquire_dark(count=request.count)
            cal.save(calibration_path)
            await broadcast({"type": "dark_ref", "values": cal.dark_ref[::DOWNSAMPLE].tolist()})
            return {"status": "done", "scans_averaged": request.count}
        finally:
            cal.capturing_dark = False

    @app.post("/calibration/white/capture")
    async def capture_white(request: CaptureRequest = CaptureRequest()):
        """Acquire white/lamp reference: opens shutter (no sample), averages N scans."""
        cal.capturing_white = True
        await broadcast({"type": "capture_status", "capturing": "white", "count": request.count})
        try:
            cal.white_ref = await driver.acquire_white(count=request.count)
            cal.save(calibration_path)
            await broadcast({"type": "white_ref", "values": cal.white_ref[::DOWNSAMPLE].tolist()})
            return {"status": "done", "scans_averaged": request.count}
        finally:
            cal.capturing_white = False

    @app.post("/calibration/reset")
    async def reset_calibration():
        cal.reset()
        return {"status": "reset"}

    # --- Configuration endpoints ---

    @app.get("/config")
    async def get_config():
        c = driver.config
        return {
            "center_wavelength": c.center_wavelength,
            "exposure_time_ms": c.exposure_time_ms,
            "gain_token": c.gain_token,
            "speed_token": c.speed_token,
        }

    @app.post("/config")
    async def set_config(request: ConfigRequest):
        c = driver.config
        if request.center_wavelength is not None:
            c.center_wavelength = request.center_wavelength
        if request.exposure_time_ms is not None:
            c.exposure_time_ms = request.exposure_time_ms
        if request.gain_token is not None:
            c.gain_token = request.gain_token
        if request.speed_token is not None:
            c.speed_token = request.speed_token
        await driver.configure(c)
        return {"status": "configured", "config": get_config.__wrapped__() if hasattr(get_config, '__wrapped__') else await get_config()}

    # --- WebSocket + Web UI ---

    @app.websocket("/ws")
    async def websocket_endpoint(ws: WebSocket):
        await ws.accept()
        connected_clients.append(ws)
        await ws.send_json({"type": "init", "wavelengths": wavelengths[::DOWNSAMPLE] if wavelengths else []})
        try:
            while True:
                await ws.receive_text()
        except WebSocketDisconnect:
            if ws in connected_clients:
                connected_clients.remove(ws)

    @app.get("/", response_class=HTMLResponse)
    async def index():
        return HTML_PAGE

    return app


HTML_PAGE = """<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<title>HORIBA iHR320 — Calibration</title>
<style>
  * { margin: 0; padding: 0; box-sizing: border-box; }
  body { font-family: system-ui, sans-serif; background: #1a1a2e; color: #e0e0e0; }
  .header { background: #16213e; padding: 10px 20px; display: flex; align-items: center; gap: 12px;
            border-bottom: 1px solid #0f3460; }
  .header h1 { font-size: 16px; }
  .badge { padding: 3px 10px; border-radius: 10px; font-size: 11px; font-weight: 700; }
  .badge-ok { background: #43a047; color: #fff; }
  .badge-warn { background: #e53935; color: #fff; }
  .header-right { margin-left: auto; font-size: 11px; color: #555; font-family: monospace; }
  .layout { display: grid; grid-template-columns: 1fr 280px; height: calc(100vh - 45px); }
  .charts { padding: 12px 16px; overflow-y: auto; display: flex; flex-direction: column; gap: 4px; }
  .chart-box { flex: 1; min-height: 130px; }
  .chart-header { display: flex; align-items: center; gap: 8px; margin-bottom: 4px; }
  .chart-title { font-size: 12px; font-weight: 600; color: #aaa; }
  .chart-value { font-size: 11px; font-family: monospace; color: #666; }
  canvas { width: 100%; height: calc(100% - 20px); min-height: 110px; background: #0a0a1a; border-radius: 6px; }
  .sidebar { background: #16213e; padding: 14px; border-left: 1px solid #0f3460; overflow-y: auto;
             display: flex; flex-direction: column; gap: 14px; }
  .section { padding-bottom: 12px; border-bottom: 1px solid #0f3460; }
  .section:last-child { border-bottom: none; }
  .section h3 { font-size: 13px; margin-bottom: 8px; color: #4fc3f7; }
  .btn { display: block; width: 100%; padding: 8px; margin-bottom: 4px; border: none; border-radius: 5px;
         font-size: 12px; font-weight: 600; cursor: pointer; }
  .btn:hover { filter: brightness(1.15); }
  .btn-dark { background: #444; color: #fff; }
  .btn-white { background: #f5a623; color: #000; }
  .btn-reset { background: #c62828; color: #fff; }
  .btn-capturing { outline: 2px solid #4fc3f7; animation: pulse 0.8s infinite; }
  @keyframes pulse { 0%,100%{opacity:1} 50%{opacity:0.6} }
  .capture-info { font-size: 11px; color: #4fc3f7; min-height: 14px; margin-bottom: 4px; font-family: monospace; }
  .capture-info.done { color: #43a047; }
  .stat { display: flex; justify-content: space-between; font-size: 11px; padding: 2px 0; }
  .stat-k { color: #666; }
  .stat-v { color: #ccc; font-family: monospace; }
  .formula { font-size: 11px; color: #555; line-height: 1.7; }
  .formula code { color: #aaa; background: #111; padding: 1px 4px; border-radius: 3px; }
</style>
</head>
<body>
<div class="header">
  <h1>HORIBA iHR320</h1>
  <span id="badgeCal" class="badge badge-warn">NOT CALIBRATED</span>
  <span class="header-right">Scan <span id="hdrN">0</span> | mean <span id="hdrMean">—</span></span>
</div>
<div class="layout">
  <div class="charts">
    <div class="chart-box">
      <div class="chart-header"><span class="chart-title">Raw Spectrum</span><span class="chart-value" id="rawInfo">—</span></div>
      <canvas id="cRaw"></canvas>
    </div>
    <div class="chart-box">
      <div class="chart-header"><span class="chart-title">Dark Reference</span><span class="chart-value" id="darkInfo">live</span></div>
      <canvas id="cDark"></canvas>
    </div>
    <div class="chart-box">
      <div class="chart-header"><span class="chart-title">White Reference</span><span class="chart-value" id="whiteInfo">live</span></div>
      <canvas id="cWhite"></canvas>
    </div>
    <div class="chart-box">
      <div class="chart-header"><span class="chart-title">Calibrated T%</span><span class="chart-value" id="calInfo">—</span></div>
      <canvas id="cCal"></canvas>
    </div>
  </div>
  <div class="sidebar">
    <div class="section">
      <h3>Calibration</h3>
      <button id="btnDark" class="btn btn-dark" onclick="toggleDark()">Capture Dark</button>
      <div id="darkCapInfo" class="capture-info"></div>
      <button id="btnWhite" class="btn btn-white" onclick="toggleWhite()">Capture White</button>
      <div id="whiteCapInfo" class="capture-info"></div>
      <button class="btn btn-reset" onclick="doReset()">Reset</button>
    </div>
    <div class="section">
      <h3>Status</h3>
      <div class="stat"><span class="stat-k">Dark ref</span><span class="stat-v" id="sDark">no</span></div>
      <div class="stat"><span class="stat-k">White ref</span><span class="stat-v" id="sWhite">no</span></div>
      <div class="stat"><span class="stat-k">Calibrated</span><span class="stat-v" id="sCal">no</span></div>
      <div class="stat"><span class="stat-k">T% (mean)</span><span class="stat-v" id="sTpct">—</span></div>
    </div>
    <div class="section">
      <h3>Formula</h3>
      <div class="formula"><code>T% = (scan - dark) / (white - dark) × 100</code></div>
    </div>
  </div>
</div>
<script>
const C = {raw:document.getElementById('cRaw'), dark:document.getElementById('cDark'),
           white:document.getElementById('cWhite'), cal:document.getElementById('cCal')};
let wl=[], scanN=0, capDark=false, capWhite=false, hasDark=false, hasWhite=false;
let darkRef=null, whiteRef=null;

function draw(canvas, values, color, yMin, yMax, overlay, oColor) {
  const ctx=canvas.getContext('2d'), dpr=devicePixelRatio||1, r=canvas.getBoundingClientRect();
  canvas.width=r.width*dpr; canvas.height=r.height*dpr; ctx.scale(dpr,dpr);
  const w=r.width, h=r.height;
  ctx.clearRect(0,0,w,h);
  ctx.strokeStyle='#1a1a2a'; ctx.lineWidth=0.5; ctx.fillStyle='#333'; ctx.font='9px monospace';
  for(let i=0;i<=4;i++){const y=h*i/4;ctx.beginPath();ctx.moveTo(0,y);ctx.lineTo(w,y);ctx.stroke();
    const v=yMax-(yMax-yMin)*i/4;ctx.fillText(v>=1000?(v/1000).toFixed(0)+'k':v.toFixed(0),3,y+10);}
  function line(d,c,lw){if(!d||!d.length)return;ctx.strokeStyle=c;ctx.lineWidth=lw;ctx.beginPath();
    for(let i=0;i<d.length;i++){const x=i/(d.length-1)*w,y=h-((d[i]-yMin)/(yMax-yMin))*h;
      i===0?ctx.moveTo(x,y):ctx.lineTo(x,y);}ctx.stroke();}
  if(overlay)line(overlay,oColor||'rgba(67,160,71,0.5)',1.5);
  line(values,color,1);
}

const ws=new WebSocket('ws://'+location.host+'/ws');
ws.onmessage=e=>{const m=JSON.parse(e.data);
  if(m.type==='init'){wl=m.wavelengths;return;}
  if(m.type==='dark_ref'){darkRef=m.values;document.getElementById('darkInfo').textContent='captured';return;}
  if(m.type==='white_ref'){whiteRef=m.values;document.getElementById('whiteInfo').textContent='captured';return;}
  if(m.type!=='scan')return;
  scanN++; document.getElementById('hdrN').textContent=scanN;
  document.getElementById('hdrMean').textContent=m.mean.toFixed(0);
  document.getElementById('rawInfo').textContent='mean '+m.mean.toFixed(0);
  const vals=m.values, mn=Math.min(...vals), mx=Math.max(...vals);
  draw(C.raw,vals,'#4fc3f7',Math.min(mn,-10),Math.max(mx,10));
  draw(C.dark,vals,'#888',Math.min(mn,-10),Math.max(mx,10),darkRef,'rgba(67,160,71,0.5)');
  draw(C.white,vals,'#f5a623',Math.min(mn,-10),Math.max(mx,10),whiteRef,'rgba(67,160,71,0.5)');
  if(m.calibrated){draw(C.cal,m.calibrated,'#4fc3f7',0,100);
    document.getElementById('calInfo').textContent='T%='+m.calibrated_mean.toFixed(1)+'%';
    document.getElementById('sTpct').textContent=m.calibrated_mean.toFixed(1)+'%';}
  const c=m.calibration;hasDark=c.has_dark;hasWhite=c.has_white;capDark=c.capturing_dark;capWhite=c.capturing_white;
  document.getElementById('sDark').textContent=hasDark?'yes':'no';
  document.getElementById('sWhite').textContent=hasWhite?'yes':'no';
  document.getElementById('sCal').textContent=c.is_calibrated?'yes':'no';
  const bc=document.getElementById('badgeCal');
  bc.textContent=c.is_calibrated?'CALIBRATED':'NOT CALIBRATED';
  bc.className='badge '+(c.is_calibrated?'badge-ok':'badge-warn');
  const bd=document.getElementById('btnDark');
  bd.textContent=capDark?'Stop Dark':hasDark?'Recapture Dark':'Capture Dark';
  bd.classList.toggle('btn-capturing',capDark);
  const bw=document.getElementById('btnWhite');
  bw.textContent=capWhite?'Stop White':hasWhite?'Recapture White':'Capture White';
  bw.classList.toggle('btn-capturing',capWhite);
  if(capDark)document.getElementById('darkCapInfo').textContent='collecting... '+c.dark_scans_collected;
  if(capWhite)document.getElementById('whiteCapInfo').textContent='collecting... '+c.white_scans_collected;
};

async function toggleDark(){
  if(capDark) return; // already capturing
  document.getElementById('darkCapInfo').textContent='capturing (shutter closed)...';
  document.getElementById('btnDark').classList.add('btn-capturing');
  const r=await(await fetch('/calibration/dark/capture',{method:'POST',headers:{'Content-Type':'application/json'},body:'{"count":10}'})).json();
  const e=document.getElementById('darkCapInfo');e.textContent='averaged '+r.scans_averaged+' scans';e.className='capture-info done';
}
async function toggleWhite(){
  if(capWhite) return;
  document.getElementById('whiteCapInfo').textContent='capturing (shutter open, remove sample)...';
  document.getElementById('btnWhite').classList.add('btn-capturing');
  const r=await(await fetch('/calibration/white/capture',{method:'POST',headers:{'Content-Type':'application/json'},body:'{"count":10}'})).json();
  const e=document.getElementById('whiteCapInfo');e.textContent='averaged '+r.scans_averaged+' scans';e.className='capture-info done';
}
async function doReset(){await fetch('/calibration/reset',{method:'POST'});darkRef=null;whiteRef=null;
  document.getElementById('darkCapInfo').textContent='';document.getElementById('whiteCapInfo').textContent='';
  document.getElementById('sTpct').textContent='—';document.getElementById('calInfo').textContent='—';
  document.getElementById('darkInfo').textContent='live';document.getElementById('whiteInfo').textContent='live';}
</script>
</body>
</html>
"""
