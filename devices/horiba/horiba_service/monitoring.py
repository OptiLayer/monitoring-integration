"""Posts calibrated spectral data to OptiMonitor."""

from __future__ import annotations

import logging
from datetime import datetime

import httpx

logger = logging.getLogger(__name__)


class MonitoringClient:
    """HTTP client for sending data to OptiMonitor's monitoring API."""

    def __init__(self) -> None:
        self.monitoring_api_url: str | None = None
        self.spectrometer_id: str | None = None
        self.vacuum_chamber_id: str | None = None
        self._client: httpx.AsyncClient | None = None

    @property
    def registered(self) -> bool:
        return self.monitoring_api_url is not None and self.spectrometer_id is not None

    async def start(self) -> None:
        self._client = httpx.AsyncClient(timeout=5.0)

    async def stop(self) -> None:
        if self._client:
            await self._client.aclose()
            self._client = None

    def register(self, monitoring_api_url: str, spectrometer_id: str | None, vacuum_chamber_id: str | None) -> None:
        self.monitoring_api_url = monitoring_api_url
        self.spectrometer_id = spectrometer_id
        self.vacuum_chamber_id = vacuum_chamber_id
        logger.info(f"Registered with monitoring API: {monitoring_api_url}")

    async def report_shutter(self, open: bool) -> None:
        """Tell OptiMonitor the shutter physically closed or opened."""
        if self.monitoring_api_url is None or self.vacuum_chamber_id is None or not self._client:
            return
        url = f"{self.monitoring_api_url}/vacuum-chambers/{self.vacuum_chamber_id}/shutter"
        try:
            response = await self._client.post(url, json={"open": open, "spectrometer_id": self.spectrometer_id})
            if response.status_code != 200:
                logger.warning(f"Failed to report shutter: {response.status_code}")
        except Exception as e:
            logger.error(f"Error reporting shutter: {e}")

    async def post_spectral_data(self, calibrated_readings: list[float], wavelengths: list[float]) -> None:
        """Post calibrated spectral data to OptiMonitor."""
        if not self.registered or not self._client:
            return

        url = f"{self.monitoring_api_url}/spectrometers/{self.spectrometer_id}/data"
        payload = {
            "calibrated_readings": calibrated_readings,
            "wavelengths": wavelengths,
            "timestamp": datetime.now().isoformat(),
        }

        try:
            response = await self._client.post(url, json=payload)
            if response.status_code != 200:
                logger.warning(f"Failed to post data: {response.status_code}")
        except Exception as e:
            logger.error(f"Error posting data: {e}")
