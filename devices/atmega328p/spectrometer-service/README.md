# ATmega328P Monochromatic Spectrometer Service

Rust service for the ATmega328P-based monochromatic spectrometer with AD7793 24-bit ADC. Provides a calibration web UI and integrates with OptiMonitor via REST API.

## Quick Start

```bash
# Build
cargo build --release

# Playback mode (replay captured data)
cargo run -- playback --file ../putty.log --loop-playback --cycle-interval 200

# Serial mode (real device)
cargo run -- serial --device /dev/ttyUSB0

# Open calibration UI
# http://localhost:8100
```

## Calibration Workflow

1. **Start the service** in serial mode (or playback for testing)
2. **Open `http://localhost:8100`** — the calibration web UI
3. **Adjust GAIN/FADC/COUNT** in the sidebar until:
   - No **CLIPPED** badge (no saturated ADC values at 16,777,215)
   - Stable T% readings with low noise
4. **Click "Save Settings"** — writes to `calibration.toml`
5. **Next startup** automatically uses saved settings (CLI args override if provided)

The service runs calibration and monitoring simultaneously — once settings are good, connect OptiMonitor to the same service.

## Device Settings

All values from the AD7793 datasheet:

| Setting | Values | Description |
|---------|--------|-------------|
| GAIN | 1, 2, 4, 8, 16, 32, 64, 128 | ADC amplification. Higher = more sensitive but clips easier |
| FADC | 500, 250, 125, 62.5, 50, 39.2, 33.3, 19.6, 16.7, 12.5, 10, 8.33, 6.25, 4.17 Hz | Sample rate. Lower = more accurate but slower |
| COUNT | 1–12 | Measurements per series. More = better averaging, must fit in ~40ms window |

Recommended starting point: **GAIN=2, FADC=250, COUNT=4** (~38ms, 0.003% error per spec).

In serial mode, settings are sent to the device immediately when changed in the UI.

## Operating Modes

### Serial (Real Hardware)

```bash
cargo run -- serial --device /dev/ttyUSB0 [--baud 38400] [--gain 4] [--fadc 500] [--count 3]
```

- Connects to ATmega328P over serial at 38400 baud
- `--gain`, `--fadc`, `--count` override saved config if provided
- Without those flags, uses values from `calibration.toml`
- Settings changes from the web UI are sent to the device in real-time

### Playback (Log File)

```bash
cargo run -- playback --file <path> [--speed 2.0] [--loop-playback] [--cycle-interval 100]
```

Supports two log formats (auto-detected):

**Timestamped** (from the service's own logging):
```
2025-01-15T10:30:00.000 SERIES1 = [1000000 1000100 1000050]
2025-01-15T10:30:00.040 SERIES2 = [8000000 8000200 8000100]
2025-01-15T10:30:00.080 SERIES3 = [4000000 4000100 4000050]
2025-01-15T10:30:00.100 END_CYCLE
```

**Raw serial capture** (e.g., PuTTY log):
```
SERIES1 = 16777215 16777215 16777215
SERIES2 = 0 213 7
SERIES3 = 16777215 16777215 16777215
GAIN=4
FADC=500.00
COUNT=3
END_CYCLE
```

Raw logs use `--cycle-interval` (default 100ms) for pacing since there are no timestamps.

## Monochromator Control

The ATmega328P board is readout only. Wavelength selection is done by a **Solar
LS M266-IV** monochromator driven through the vendor SDK, enabled with `--mono`:

```bash
# Windows (production): folder holding InstrumentCfg*.xml, or "" for the exe's folder
spectrometer-service.exe --mono "C:\\ProgramData\\SolarLS" serial --device COM3

# Linux (development): simulated M266 with the same four gratings
cargo run -- --mono sim playback --file ../putty.log --loop-playback
```

| Flag | Description |
|------|-------------|
| `--mono <sim\|path>` | Enable wavelength control. `sim` = simulator, otherwise the SDK config folder |
| `--mono-index <N>` | Instrument index within the SDK config (default 0) |
| `--mono-grating <N>` | Pin one grating instead of auto-selecting per wavelength |

Without `--mono`, `POST /control_wavelength` keeps its previous behaviour: it
stores the value as a label for the monitoring API and reports
`"hardware": false`. Playback and bench calibration runs want exactly that.

### Grating selection

The M266 has four gratings and the instrument config sets `AutoSelGrating=no`,
so the service picks one. It **keeps the active grating whenever that grating
can reach the requested wavelength**, and only switches when it cannot —
switching changes throughput and stray light, which puts a step in the
spectrum mid-scan. Use `--mono-grating` to forbid switching entirely.

The table is read from the instrument at startup and logged, so it follows the
instrument config rather than being hardcoded. For our M266-IV:

| Grating | Grooves/mm | Max λ |
|---------|-----------|-------|
| 0 | 1800 | 540 nm |
| 1 | 1200 | 800 nm |
| 2 | 600 | 1800 nm |
| 3 | 200 | 5400 nm |

> **If the instrument is not reachable, `--mono` takes the process down at
> startup.** Grating enumeration throws an unhandled `NullReferenceException`
> inside the SDK's own managed code (`sls_GetGratingCount`), which the CLR
> treats as fatal — there is no error for us to catch and report. We therefore
> read the grating table during connect rather than on the first move, so this
> surfaces immediately at startup instead of hours into a deposition run.
> Observed under wine with no instrument attached; not yet confirmed against
> real hardware on Windows.

### Settling

`POST /control_wavelength` blocks until the grating has settled (the SDK's
synchronous `sls_SetWl`). While it is travelling the detector sees a smear of
every wavelength it sweeps past, so readings taken during the move are **not**
pushed to the monitoring API, and the dashboard greys out that span.

### Windows deployment

Download `atmega328p-spectrometer-<tag>.zip` from the GitHub release and unpack
it anywhere. It contains the exe and the whole Solar LS runtime side by side —
nothing else to install except **.NET Framework 4.0**.

The SDK is loaded at runtime, not linked. Two consequences:

- **CI needs nothing from the SDK to build.** No header, no `.lib`. The release
  workflow compiles the exe and then copies `vendor/solarls/` next to it.
- **A missing or broken SDK does not stop the service.** It still starts and the
  calibration UI still works; only `--mono` fails, with the reason in the log.

The runtime files live in [`vendor/solarls/`](vendor/solarls/) — 13 DLLs (~4 MB)
plus the instrument config. That folder's README documents where each file came
from, why the detector assemblies are excluded, and the `SolarLS.SdkExport.dll`
name collision between the SDK's `Release\` and `Release\x64\` folders. The
release workflow checks the package is complete and that the shipped
`SolarLS.SdkExport.dll` really is the x64 build before publishing.

To run against hardware from a local `cargo build`, copy that folder next to the
binary:

```powershell
Copy-Item vendor/solarls/* target/release/ -Exclude README.md
target/release/spectrometer-service.exe --mono "" serial --device COM3
```

`--mono ""` means "load the instrument config from the executable's folder".

Cross-compiling from Linux is not needed for development — `--mono sim` covers
the endpoint, the grating logic and the dashboard. The Windows binary is built
on `windows-latest` in CI.

## Calibration Formula

```
T% = (sample - dark) / (full - dark) × 100
```

- **SERIES1** = dark (light blocked)
- **SERIES2** = full (100% light reference)
- **SERIES3** = sample (through material)

The AD7793 reads higher ADC values for less light (dark ~14M, full ~300). The formula handles this correctly — both numerator and denominator are negative, so they cancel out.

## Web UI

Available at `http://localhost:<port>` (default 8100).

- **Monochromator panel** — wavelength setpoint + Go, actual readback, READY/MOVING/ERROR badge
- **Transmittance chart** — live T% over time (last 300 cycles), greyed while the grating moves
- **Raw means chart** — dark (red), full (green), sample (blue) with clipping markers
- **Settings controls** — GAIN, FADC, COUNT dropdowns with Save button
- **Live values** — current T%, dark/full/sample means
- **Clipping detection** — red CLIPPED badge when any ADC value hits 16,777,215

## API Endpoints

### Calibration/Settings

| Method | Path | Description |
|--------|------|-------------|
| GET | `/` | Calibration web UI |
| GET | `/ws` | WebSocket for live data streaming |
| GET | `/api/settings` | Current device settings |
| POST | `/api/settings` | Update settings (sends to device + saves to TOML) |

### OptiMonitor Integration

| Method | Path | Description |
|--------|------|-------------|
| GET | `/device/info` | Device capabilities |
| POST | `/register` | Register with monitoring API |
| GET/POST | `/control_wavelength` | Wavelength control — moves the monochromator when `--mono` is set |
| GET/POST | `/vacuum_chamber/material` | Material setting |
| POST | `/vacuum_chamber/start` | Start deposition |
| POST | `/vacuum_chamber/stop` | Stop deposition |
| GET | `/vacuum_chamber/status` | Chamber status |

## Config Persistence

Settings are saved to `calibration.toml` (configurable via `--calibration-config`):

```toml
[device_settings]
gain = 2
fadc = 250.0
count = 4

last_updated = "2026-03-23T12:00:00Z"
```

Priority: CLI args > calibration.toml > hardcoded defaults.

## Building & Testing

```bash
cargo build              # Debug build
cargo build --release    # Release build
cargo test               # 109 tests
cargo clippy --tests     # Zero warnings
```

### Prerequisites

- Rust 2024 edition
- Linux: `libudev-dev` (`apt install libudev-dev` or `dnf install systemd-devel`)

### Serial Port Access (Linux)

```bash
sudo usermod -a -G dialout $USER
# Re-login for group change to take effect
```
