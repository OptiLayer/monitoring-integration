import argparse
import asyncio
import logging
from datetime import datetime
from typing import Optional

import httpx
import numpy as np
import uvicorn
from fastapi import Body, FastAPI
from pydantic import BaseModel

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)


class DeviceInfo(BaseModel):
    type: str
    name: str
    device_type: str | None = None
    capabilities: dict


class ControlWavelengthRequest(BaseModel):
    wavelength: float


class SpectralDataRequest(BaseModel):
    calibrated_readings: list[float]


class RegisterRequest(BaseModel):
    spectrometer_id: str | None = None
    vacuum_chamber_id: str | None = None
    monitoring_api_url: str


class VirtualSpectrometer:
    def __init__(
        self,
        name: str = "Virtual Spectrometer",
        num_points: int = 2048,
        update_interval: float = 0.5,
        is_monochromatic: bool = False,
        monitoring_api_url: Optional[str] = None,
        spectrometer_id: Optional[str] = None,
        vacuum_chamber_id: Optional[str] = None,
        device_type: str = "two-component",
        has_spectrometer: bool = True,
        has_vacuum_chamber: bool = True,
    ):
        self.name = name
        self.num_points = num_points
        self.update_interval = update_interval
        self.is_monochromatic = is_monochromatic
        self.monitoring_api_url = monitoring_api_url
        self.spectrometer_id = spectrometer_id
        self.vacuum_chamber_id = vacuum_chamber_id
        self.device_type = device_type
        self.has_spectrometer = has_spectrometer
        self.has_vacuum_chamber = has_vacuum_chamber
        self.control_wavelength = 550.0
        self.is_running = False
        self.data_task: Optional[asyncio.Task] = None
        self.time_offset = 0.0
        self.current_material = "H"
        self.current_fraction = 100.0
        self.wavelength = np.linspace(400.0, 900.0, num_points).tolist()

    def generate_spectral_data(self) -> np.ndarray:
        x = np.linspace(0, 2 * np.pi, self.num_points)
        sine_wave = np.sin(x + self.time_offset) * 30.0 + 50.0
        noise = np.random.normal(0, 2.0, self.num_points)
        calibrated_readings = sine_wave + noise
        calibrated_readings = np.clip(calibrated_readings, 0, 100)
        return calibrated_readings

    async def data_generation_loop(self):
        logger.info(f"Starting data generation loop (interval: {self.update_interval}s)")
        async with httpx.AsyncClient() as client:
            while self.is_running:
                try:
                    calibrated_readings = self.generate_spectral_data()
                    self.time_offset += 0.1

                    if self.monitoring_api_url and self.spectrometer_id:
                        url = f"{self.monitoring_api_url}/spectrometers/{self.spectrometer_id}/data"
                        payload = {
                            "calibrated_readings": calibrated_readings.tolist(),
                            "wavelengths": self.wavelength,
                            "timestamp": datetime.now().isoformat(),
                        }

                        try:
                            response = await client.post(url, json=payload, timeout=5.0)
                            if response.status_code == 200:
                                logger.debug(f"Posted data to {url}")
                            else:
                                logger.warning(f"Failed to post data: {response.status_code}")
                        except Exception as e:
                            logger.error(f"Error posting data: {e}")

                    await asyncio.sleep(self.update_interval)

                except asyncio.CancelledError:
                    logger.info("Data generation loop cancelled")
                    break
                except Exception as e:
                    logger.error(f"Error in data generation loop: {e}")
                    await asyncio.sleep(self.update_interval)

    async def report_shutter(self, open: bool) -> None:
        """Tell OptiMonitor the shutter physically closed or opened."""
        if not (self.monitoring_api_url and self.vacuum_chamber_id):
            return
        url = f"{self.monitoring_api_url}/vacuum-chambers/{self.vacuum_chamber_id}/shutter"
        async with httpx.AsyncClient() as client:
            try:
                await client.post(url, json={"open": open, "spectrometer_id": self.spectrometer_id}, timeout=5.0)
            except Exception as e:
                logger.error(f"Error reporting shutter: {e}")

    async def switch_layer(self, material: str, fraction: float) -> None:
        """Play the chamber: close, switch, reopen two seconds later."""
        await self.report_shutter(False)
        await asyncio.sleep(2.0)
        self.current_material = material
        self.current_fraction = fraction
        await self.report_shutter(True)

    async def start(self):
        if self.is_running:
            logger.warning("Already running")
            return

        self.is_running = True
        self.data_task = asyncio.create_task(self.data_generation_loop())
        logger.info("Virtual spectrometer started")

    async def stop(self):
        if not self.is_running:
            logger.warning("Not running")
            return

        self.is_running = False
        if self.data_task:
            self.data_task.cancel()
            try:
                await self.data_task
            except asyncio.CancelledError:
                pass
        logger.info("Virtual spectrometer stopped")


def create_app(spectrometer: VirtualSpectrometer) -> FastAPI:
    app = FastAPI(title="Virtual Spectrometer with Vacuum Chamber", version="1.0.0")

    @app.get("/device/info", response_model=DeviceInfo)
    async def get_device_info():
        capabilities = {
            "has_spectrometer": spectrometer.has_spectrometer,
            "has_vacuum_chamber": spectrometer.has_vacuum_chamber,
        }
        if spectrometer.has_spectrometer:
            capabilities["process_type"] = spectrometer.device_type
            capabilities["is_monochromatic"] = spectrometer.is_monochromatic
        return DeviceInfo(
            type="spectrometer",
            name=spectrometer.name,
            device_type=None,
            capabilities=capabilities,
        )

    @app.post("/register")
    async def register(request: RegisterRequest):
        spectrometer.monitoring_api_url = request.monitoring_api_url
        if request.spectrometer_id:
            spectrometer.spectrometer_id = request.spectrometer_id
            logger.info(f"Registered with spectrometer_id: {request.spectrometer_id}")
        if request.vacuum_chamber_id:
            spectrometer.vacuum_chamber_id = request.vacuum_chamber_id
            logger.info(f"Registered with vacuum_chamber_id: {request.vacuum_chamber_id}")
        logger.info(f"Registered with monitoring API: {request.monitoring_api_url}")
        return {
            "status": "registered",
            "spectrometer_id": spectrometer.spectrometer_id,
            "vacuum_chamber_id": spectrometer.vacuum_chamber_id,
            "monitoring_api_url": spectrometer.monitoring_api_url,
        }

    @app.post("/control_wavelength")
    async def set_control_wavelength(request: ControlWavelengthRequest):
        spectrometer.control_wavelength = request.wavelength
        logger.info(f"Control wavelength set to {request.wavelength} nm")
        return {"control_wavelength": spectrometer.control_wavelength}

    @app.get("/control_wavelength")
    async def get_control_wavelength():
        return {"control_wavelength": spectrometer.control_wavelength}

    @app.post("/start")
    async def start_acquisition():
        await spectrometer.start()
        return {"running": spectrometer.is_running}

    @app.post("/stop")
    async def stop_acquisition():
        await spectrometer.stop()
        return {"running": spectrometer.is_running}

    @app.get("/status")
    async def get_status():
        return {
            "running": spectrometer.is_running,
            "control_wavelength": spectrometer.control_wavelength,
            "name": spectrometer.name,
        }

    @app.get("/data")
    async def get_data():
        calibrated_readings = spectrometer.generate_spectral_data()
        return {
            "timestamp": datetime.now().isoformat(),
            "calibrated_readings": calibrated_readings.tolist(),
            "wavelengths": spectrometer.wavelength,
        }

    @app.post("/vacuum_chamber/start")
    async def start_vacuum_chamber():
        await spectrometer.start()
        await spectrometer.report_shutter(True)
        logger.info("Vacuum chamber started - shutter open, beginning deposition")
        return {"status": "running"}

    @app.post("/vacuum_chamber/stop")
    async def stop_vacuum_chamber():
        await spectrometer.stop()
        await spectrometer.report_shutter(False)
        logger.info("Vacuum chamber stopped - shutter closed, deposition ended")
        return {"status": "stopped"}

    @app.get("/vacuum_chamber/status")
    async def get_vacuum_chamber_status():
        return {
            "status": "running" if spectrometer.is_running else "stopped",
            "is_depositing": spectrometer.is_running,
        }

    @app.get("/vacuum_chamber/material")
    async def get_vacuum_chamber_material():
        return {"material": spectrometer.current_material, "fraction": spectrometer.current_fraction}

    @app.post("/vacuum_chamber/material")
    async def set_vacuum_chamber_material(payload: dict = Body(...)):
        material = payload.get("material", "H")
        fraction = payload.get("fraction", 100.0)
        if spectrometer.is_running:
            asyncio.create_task(spectrometer.switch_layer(material, fraction))
            logger.info(f"Layer switch to {material} requested")
            return {"material": material, "fraction": fraction}
        spectrometer.current_material = material
        spectrometer.current_fraction = fraction
        logger.info(f"Material set to {spectrometer.current_material} with fraction {spectrometer.current_fraction}%")
        return {"material": spectrometer.current_material, "fraction": spectrometer.current_fraction}

    return app


def main():
    parser = argparse.ArgumentParser(description="Virtual Spectrometer with Vacuum Chamber")
    parser.add_argument("--port", type=int, default=8100, help="Port to run on")
    parser.add_argument("--name", type=str, default="Virtual Spectrometer", help="Device name")
    parser.add_argument("--num-points", type=int, default=2048, help="Number of spectral points")
    parser.add_argument("--update-interval", type=float, default=0.5, help="Data update interval (seconds)")
    parser.add_argument("--monitoring-api", type=str, help="Monitoring API URL (e.g., http://localhost:8200)")
    parser.add_argument("--spectrometer-id", type=str, help="Spectrometer ID in monitoring system")
    parser.add_argument("--vacuum-chamber-id", type=str, help="Vacuum chamber ID in monitoring system")
    parser.add_argument("--device-type", type=str, default="two-component", help="Spectrometer type")

    args = parser.parse_args()

    spectrometer = VirtualSpectrometer(
        name=args.name,
        num_points=args.num_points,
        update_interval=args.update_interval,
        monitoring_api_url=args.monitoring_api,
        spectrometer_id=args.spectrometer_id,
        vacuum_chamber_id=args.vacuum_chamber_id,
        device_type=args.device_type,
        has_spectrometer=True,
        has_vacuum_chamber=True,
    )

    app = create_app(spectrometer)

    logger.info(f"Starting Virtual Spectrometer with Vacuum Chamber on port {args.port}")
    logger.info(f"Device name: {args.name}")
    logger.info(f"Spectral points: {args.num_points}")
    logger.info(f"Update interval: {args.update_interval}s")
    logger.info("Capabilities: Spectrometer=True, Vacuum Chamber=True")
    if args.monitoring_api:
        logger.info(f"Monitoring API: {args.monitoring_api}")

    uvicorn.run(app, host="0.0.0.0", port=args.port)


if __name__ == "__main__":
    main()
