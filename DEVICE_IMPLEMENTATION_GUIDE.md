# Device Implementation Guide

This guide explains how to implement your hardware device software to integrate with the OptiMonitor system.

## Overview

Your device needs to implement a simple REST API that the OptiMonitor system can communicate with. The monitoring system will:
1. Discover your device's capabilities
2. Register it in the system
3. Forward control commands (start/stop deposition, set material, etc.)
4. Receive spectral data from your device

## Required Endpoints

### 1. Device Information - `GET /device/info`

**Purpose**: Tell the monitoring system what your device can do.

**Response**:
```json
{
  "type": "spectrometer",
  "name": "Your Device Name",
  "capabilities": {
    "has_spectrometer": true,
    "has_vacuum_chamber": true,
    "spectrometer_type": "two-component",
    "is_monochromatic": false
  }
}
```

**Fields**:
- `type`: Always `"spectrometer"` (even if you have vacuum chamber too)
- `name`: Your device's display name
- `capabilities.has_spectrometer`: `true` if your device provides spectral measurements
- `capabilities.has_vacuum_chamber`: `true` if your device controls deposition
- `capabilities.spectrometer_type`: `"two-component"` or `"three-component"` 
- `capabilities.is_monochromatic`: `true` for single-wavelength, `false` for broadband

**Example Implementation**:
```python
@app.get("/device/info")
async def get_device_info():
    return {
        "type": "spectrometer",
        "name": "My Spectrometer",
        "capabilities": {
            "has_spectrometer": True,
            "has_vacuum_chamber": True,
            "spectrometer_type": "two-component",
            "is_monochromatic": False
        }
    }
```

---

### 2. Device Registration - `POST /register`

**Purpose**: Receive your assigned IDs from the monitoring system. Done by OptiMonitor.

**Request Body**:
```json
{
  "monitoring_api_url": "http://localhost:8200",
  "spectrometer_id": "uuid-assigned-by-monitoring",
  "vacuum_chamber_id": "uuid-assigned-by-monitoring"
}
```

**Response**:
```json
{
  "status": "registered",
  "spectrometer_id": "uuid-assigned-by-monitoring",
  "vacuum_chamber_id": "uuid-assigned-by-monitoring",
  "monitoring_api_url": "http://localhost:8200"
}
```

**What to do**:
- Store the `monitoring_api_url` - you'll send data here
- Store the `spectrometer_id` and/or `vacuum_chamber_id` - you'll use these when posting data

**Example Implementation**:
```python
monitoring_api_url = None
spectrometer_id = None
vacuum_chamber_id = None

@app.post("/register")
async def register(request: RegisterRequest):
    global monitoring_api_url, spectrometer_id, vacuum_chamber_id
    monitoring_api_url = request.monitoring_api_url
    spectrometer_id = request.spectrometer_id
    vacuum_chamber_id = request.vacuum_chamber_id

    logger.info(f"Registered with monitoring API: {monitoring_api_url}")
    return {
        "status": "registered",
        "spectrometer_id": spectrometer_id,
        "vacuum_chamber_id": vacuum_chamber_id,
        "monitoring_api_url": monitoring_api_url
    }
```

---

## Spectrometer Endpoints

### 3. Get Control Wavelength - `GET /control_wavelength`

**Purpose**: Return the current control wavelength (monochromatic spectrometers only).

**Response**:
```json
{
  "control_wavelength": 550.0
}
```

---

### 4. Set Control Wavelength - `POST /control_wavelength`

**Purpose**: Set a new control wavelength (monochromatic spectrometers only).

**Request Body**:
```json
{
  "wavelength": 550.0
}
```

**Response**:
```json
{
  "control_wavelength": 550.0
}
```

**Example Implementation**:
```python
current_wavelength = 550.0

@app.post("/control_wavelength")
async def set_control_wavelength(request: ControlWavelengthRequest):
    global current_wavelength
    current_wavelength = request.wavelength
    # Update your hardware here
    logger.info(f"Control wavelength set to {request.wavelength} nm")
    return {"control_wavelength": current_wavelength}

@app.get("/control_wavelength")
async def get_control_wavelength():
    return {"control_wavelength": current_wavelength}
```

---

### 5. Sending Spectral Data to Monitoring

**When**: Your device should continuously send spectral data from the moment it is registered (after `POST /register`), regardless of whether deposition is running. OptiMonitor shows the raw signal before Start so the operator can verify the optical path, and only fits scans while the chamber is running.

**Endpoint**: `POST {monitoring_api_url}/spectrometers/{spectrometer_id}/data`

**Request Body**:
```json
{
  "calibrated_readings": [10.5, 25.3, 45.8, 67.2, 89.1, ...]
}
```

**Fields**:
- `calibrated_readings`: Array of floats representing spectral values (-100%)

**Example Implementation**:
```python
async def data_sending_loop():
    async with httpx.AsyncClient() as client:
        while is_running:
            # Get spectral data from your hardware
            readings = get_spectral_data_from_hardware()

            # Send to monitoring API
            url = f"{monitoring_api_url}/spectrometers/{spectrometer_id}/data"
            payload = {"calibrated_readings": readings.tolist()}

            try:
                response = await client.post(url, json=payload, timeout=5.0)
                if response.status_code == 200:
                    logger.debug("Data sent successfully")
            except Exception as e:
                logger.error(f"Error sending data: {e}")

            await asyncio.sleep(0.5)  # Update interval
```

---

## Vacuum Chamber Endpoints

### 6. Get Current Material - `GET /vacuum_chamber/material`

**Purpose**: Return the currently selected material.

**Response**:
```json
{
  "material": "H"
}
```

---

### 7. Set Material - `POST /vacuum_chamber/material`

**Purpose**: Set the material for the next deposition.

**Request Body**: Plain JSON string
```json
"H"
```

**Response**:
```json
{
  "material": "H"
}
```

**Example Implementation**:
```python
from fastapi import Body

current_material = "H"

@app.post("/vacuum_chamber/material")
async def set_material(material: str = Body(...)):
    global current_material
    current_material = material
    # Configure your hardware for this material
    logger.info(f"Material set to {material}")
    return {"material": current_material}

@app.get("/vacuum_chamber/material")
async def get_material():
    return {"material": current_material}
```

---

### 8. Start Deposition - `POST /vacuum_chamber/start`

**Purpose**: Begin the deposition process.

**Request Body**: None

**Response**:
```json
{
  "status": "running"
}
```

**What to do**:
- Open your deposition shutter / start your source
- Begin sending spectral data to the monitoring API
- Return success response

**Example Implementation**:
```python
is_depositing = False

@app.post("/vacuum_chamber/start")
async def start_deposition():
    global is_depositing
    is_depositing = True

    # Open shutter / start source
    hardware_start_deposition()

    # Start data sending loop if not already running
    if not data_task:
        asyncio.create_task(data_sending_loop())

    logger.info("Deposition started")
    return {"status": "running"}
```

---

### 9. Stop Deposition - `POST /vacuum_chamber/stop`

**Purpose**: End the deposition process.

**Request Body**: None

**Response**:
```json
{
  "status": "stopped"
}
```

**What to do**:
- Close your deposition shutter / stop your source
- Stop sending spectral data
- Return success response

**Example Implementation**:
```python
@app.post("/vacuum_chamber/stop")
async def stop_deposition():
    global is_depositing
    is_depositing = False

    # Close shutter / stop source
    hardware_stop_deposition()

    logger.info("Deposition stopped")
    return {"status": "stopped"}
```

---

### 10. Get Chamber Status - `GET /vacuum_chamber/status`

**Purpose**: Check if deposition is currently running.

**Response**:
```json
{
  "status": "running",
  "is_depositing": true
}
```

**Example Implementation**:
```python
@app.get("/vacuum_chamber/status")
async def get_status():
    return {
        "status": "running" if is_depositing else "stopped",
        "is_depositing": is_depositing
    }
```

---

## Complete Workflow

### 1. Device Startup
Your device starts its FastAPI server on a known port (e.g., 8100).

### 2. Connection to Monitoring
User connects your device through the monitoring UI or API:
```bash
POST http://localhost:8200/devices/connect
{
  "address": "localhost",
  "port": 8100
}
```

This triggers:
1. Monitoring calls `GET /device/info` on your device
2. Monitoring auto-creates spectrometer and/or vacuum chamber entries
3. Monitoring calls `POST /register` with assigned IDs
4. Your device stores the IDs and monitoring URL

### 3. Device Activation
User activates your spectrometer:
```bash
POST http://localhost:8200/spectrometers/{id}/activate
```

### 4. Material Configuration (Optional)
User may set material before starting:
```bash
PUT http://localhost:8200/vacuum-chambers/{id}/material
{
  "material": "H"
}
```

This forwards to your device: `POST /vacuum_chamber/material`

### 5. Start Deposition
User starts deposition:
```bash
POST http://localhost:8200/vacuum-chambers/{id}/start
```

This triggers:
1. Monitoring calls `POST /vacuum_chamber/start` on your device
2. Your device starts the deposition process
3. Monitoring begins fitting the spectral data your device is already sending

### 6. During Deposition
Your device repeatedly sends data:
```python
POST {monitoring_api_url}/spectrometers/{spectrometer_id}/data
{
  "calibrated_readings": [...]
}
```

### 7. Stop Deposition
User stops deposition:
```bash
POST http://localhost:8200/vacuum-chambers/{id}/stop
```

This triggers:
1. Monitoring calls `POST /vacuum_chamber/stop` on your device
2. Your device stops the deposition process
3. Your device stops sending spectral data

---

## Complete Example

See `virtual_spectrometer.py` in this directory for a complete working example that:
- Implements all required endpoints
- Generates random spectral data
- Starts/stops data generation based on vacuum chamber commands
- Handles registration and ID storage

**Key code structure**:
```python
from fastapi import FastAPI, Body
import httpx
import asyncio

app = FastAPI()

# State
monitoring_api_url = None
spectrometer_id = None
vacuum_chamber_id = None
is_depositing = False
current_material = "H"

# Device info
@app.get("/device/info")
async def get_device_info():
    return {
        "type": "spectrometer",
        "name": "My Device",
        "capabilities": {
            "has_spectrometer": True,
            "has_vacuum_chamber": True,
            "spectrometer_type": "two-component",
            "is_monochromatic": False
        }
    }

# Registration
@app.post("/register")
async def register(request: RegisterRequest):
    global monitoring_api_url, spectrometer_id, vacuum_chamber_id
    monitoring_api_url = request.monitoring_api_url
    spectrometer_id = request.spectrometer_id
    vacuum_chamber_id = request.vacuum_chamber_id
    return {
        "status": "registered",
        "spectrometer_id": spectrometer_id,
        "vacuum_chamber_id": vacuum_chamber_id,
        "monitoring_api_url": monitoring_api_url
    }

# Material endpoints
@app.get("/vacuum_chamber/material")
async def get_material():
    return {"material": current_material}

@app.post("/vacuum_chamber/material")
async def set_material(material: str = Body(...)):
    global current_material
    current_material = material
    return {"material": current_material}

# Deposition control
@app.post("/vacuum_chamber/start")
async def start_deposition():
    global is_depositing
    is_depositing = True
    asyncio.create_task(data_sending_loop())
    return {"status": "running"}

@app.post("/vacuum_chamber/stop")
async def stop_deposition():
    global is_depositing
    is_depositing = False
    return {"status": "stopped"}

# Data sending
async def data_sending_loop():
    async with httpx.AsyncClient() as client:
        while is_depositing:
            readings = get_hardware_data()
            url = f"{monitoring_api_url}/spectrometers/{spectrometer_id}/data"
            await client.post(url, json={"calibrated_readings": readings})
            await asyncio.sleep(0.5)
```

---

## Testing Your Implementation

1. **Start Monitoring Server**:
   ```bash
   python -m monitoring.server --port 8200
   ```

2. **Start Your Device**:
   ```bash
   python your_device.py --port 8100
   ```

3. **Connect Device**:
   ```bash
   curl -X POST http://localhost:8200/devices/connect \
     -H "Content-Type: application/json" \
     -d '{"address": "localhost", "port": 8100}'
   ```

4. **Run the Example Script**:
   ```bash
   ./run_example.sh 8200 8100
   ```

This will walk through the complete workflow and verify your implementation.

---
