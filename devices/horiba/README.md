# HORIBA iHR320 Device Service

Broadband monitoring device service for OptiMonitor using a HORIBA iHR320 spectrometer + CCD detector.

## Architecture

```
[Linux or Windows]                      [Windows PC]
┌──────────────────────┐    WebSocket    ┌─────────────────┐    USB
│  This service        │ ◄────────────► │  ICL.exe         │ ◄──────► iHR320 + CCD
│  (FastAPI, port 8100)│  ws://...:25010 │  (HORIBA licensed)│
└────────┬─────────────┘                └─────────────────┘
         │ REST API
         ▼
   OptiMonitor (port 8200)
```

The service talks to HORIBA's ICL software over WebSocket. ICL handles USB communication with the hardware. The service can run on the same Windows machine as ICL, or remotely on Linux over the network.

## Prerequisites

- **ICL.exe** installed and running on the Windows machine connected to the spectrometer (HORIBA licensed software)
- **HORIBA EzSpec Python SDK** (`horiba-sdk` package)
- Python 3.11+

## Installation

```bash
cd devices/horiba
uv sync
```

## Usage

### On the same Windows machine as ICL

```bash
uv run python -m horiba_service.main --start-icl
```

This auto-starts ICL.exe and connects to it locally.

### Remote (service on Linux, ICL on Windows)

On the Windows machine, start ICL.exe manually. Then on the Linux box:

```bash
uv run python -m horiba_service.main --icl-host 192.168.1.50 --icl-port 25010
```

### Options

```
--port PORT        Service port (default: 8100)
--icl-host HOST    ICL WebSocket host (default: 127.0.0.1)
--icl-port PORT    ICL WebSocket port (default: 25010)
--start-icl        Auto-start ICL.exe (Windows only)
```

## Calibration

Open `http://localhost:8100` for the calibration web UI.

Before the service can send useful data to OptiMonitor, you need to capture dark and white references. Without calibration, the CCD returns raw counts that reflect lamp brightness + detector sensitivity — not sample transmittance.

### Calibration workflow

1. **Capture dark reference** — block the light path (or the service closes the CCD shutter automatically), click "Capture Dark" in the web UI. Averages 10 scans with shutter closed.

2. **Capture white reference** — ensure the light path is open with no sample (lamp reference only), click "Capture White". Averages 10 scans with shutter open.

3. **Done** — every subsequent scan is calibrated:
   ```
   T% = (scan - dark) / (white - dark) × 100
   ```

Calibration data is saved to `calibration.json` and **persists across service restarts**. You only need to recalibrate if the lamp or optical setup changes.

### Calibration API

```
POST /calibration/dark/capture   {"count": 10}   → capture dark (shutter closed)
POST /calibration/white/capture  {"count": 10}   → capture white (shutter open)
GET  /calibration/status                         → calibration state
POST /calibration/reset                          → clear references
```

## OptiMonitor Integration

The service implements the standard OptiMonitor device API. Connect it like any other device:

### 1. Start the service

```bash
uv run python -m horiba_service.main --icl-host 192.168.1.50
```

### 2. Connect to OptiMonitor

```bash
curl -X POST http://localhost:8200/devices/connect \
  -H "Content-Type: application/json" \
  -d '{"address": "localhost", "port": 8100}'
```

OptiMonitor will:
- Call `GET /device/info` → learns it's a broadband spectrometer
- Call `POST /register` → assigns spectrometer and vacuum chamber IDs
- The service stores these IDs for data posting

### 3. Calibrate

Open `http://localhost:8100` and capture dark + white references (see above).

### 4. Start monitoring

```bash
curl -X POST http://localhost:8200/vacuum-chambers/{chamber_id}/start
```

This triggers the service to:
- Continuously acquire spectra from the CCD
- Apply calibration (dark/white → T%)
- POST `calibrated_readings` (0-100%) + `wavelengths` to OptiMonitor

### 5. Stop monitoring

```bash
curl -X POST http://localhost:8200/vacuum-chambers/{chamber_id}/stop
```

## Device API Reference

### OptiMonitor endpoints

```
GET  /device/info              → device type, name, capabilities
POST /register                 → receive monitoring API URL + IDs
POST /vacuum_chamber/start     → begin acquisition + data posting
POST /vacuum_chamber/stop      → stop acquisition
GET  /vacuum_chamber/status    → running/stopped
GET  /vacuum_chamber/material  → current material
POST /vacuum_chamber/material  → set material
```

### Configuration

```
GET  /config                   → center wavelength, exposure time, gain, speed
POST /config                   → update settings (applies immediately)
```

Example:
```bash
curl -X POST http://localhost:8100/config \
  -H "Content-Type: application/json" \
  -d '{"center_wavelength": 600.0, "exposure_time_ms": 200.0}'
```

### Live data

```
GET  /                         → calibration web UI
WS   /ws                       → WebSocket stream of live spectra
```

## Development

### Run tests

```bash
uv sync --group dev
uv run pytest tests/ -v
```

Tests use mocked SDK components — no hardware or ICL needed.

### Project structure

```
devices/horiba/
├── horiba_service/
│   ├── main.py           # CLI entry point
│   ├── server.py         # FastAPI app + web UI
│   ├── driver.py         # HORIBA SDK wrapper
│   ├── calibration.py    # Dark/white reference management + persistence
│   └── monitoring.py     # OptiMonitor data posting client
├── tests/
│   ├── test_calibration.py   # Calibration math + save/load
│   ├── test_driver.py        # Driver with mocked SDK
│   └── test_server.py        # REST endpoint contracts
├── pyproject.toml
├── RANGE_SCAN.md       # Broadband range-scan shape: root cause + grating/overlap fix
└── README.md
```

## Broadband range scan

If the wide-range broadband spectrum comes out the wrong shape (inverted, seams, or a
sawtooth), see **[RANGE_SCAN.md](RANGE_SCAN.md)**. Short version: use grating 1
(1200 g/mm) with a high `--scan-overlap` (e.g. 800) so the stitch samples frame
centers, not the low-efficiency frame edges.
