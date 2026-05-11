"""HORIBA Monochromator + CCD integration test script.

Supports any HORIBA monochromator + CCD detector connected via ICL,
including iHR320, iHR550, and other models discoverable by the SDK.

Tests all functionality needed for OptiMonitor broadband spectrometer support:
1. Connect to ICL, discover and list all devices with capabilities
2. Initialize monochromator and CCD
3. Read full device configuration (model, chip size, gratings, gains, speeds)
4. Set wavelength, grating, slits
5. Acquire single spectrum (broadband CCD)
6. Acquire dark frame (shutter closed)
7. Acquire series of spectra (continuous monitoring simulation)
8. MultiAcq hardware-level series
9. Grating switch test (optional)

Prerequisites:
  - HORIBA ICL.exe installed, licensed, and running
  - pip install horiba-sdk numpy

Usage:
  horiba-test.exe [--icl-ip 127.0.0.1] [--icl-port 25010]
                  [--start-icl] [--wavelength 500]
                  [--exposure 1000] [--series-count 5]
"""

from __future__ import annotations

import argparse
import asyncio
import time
from dataclasses import dataclass
from typing import TYPE_CHECKING

import numpy as np
from horiba_sdk.core.acquisition_format import AcquisitionFormat
from horiba_sdk.core.timer_resolution import TimerResolution
from horiba_sdk.core.stitching import LinearSpectraStitch
from horiba_sdk.core.x_axis_conversion_type import XAxisConversionType
from horiba_sdk.devices.device_manager import DeviceManager
from horiba_sdk.devices.single_devices.monochromator import Monochromator

if TYPE_CHECKING:
    from horiba_sdk.devices.single_devices.ccd import ChargeCoupledDevice

# ---------------------------------------------------------------------------
# horiba_sdk bug: SpectrAcq3 discovery crashes with KeyError: 'devices' when
# no SpectrAcq3 hardware is present.  We don't use SpectrAcq3 (it's a single-
# channel detector interface), but DeviceManager.start() always runs its
# discovery.  No way to disable it, so patch _parse_devices to return [].
# ---------------------------------------------------------------------------
from typing import Any

from horiba_sdk.devices.spectracq3_discovery import SpectrAcq3Discovery

SpectrAcq3Discovery._parse_devices = lambda self, raw: []  # type: ignore[assignment]


@dataclass
class SpectrumData:
    wavelengths: np.ndarray
    intensities: np.ndarray
    timestamp: str
    center_wavelength: float
    exposure_time_ms: int


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


async def wait_mono_ready(mono: Monochromator, timeout: float = 120.0) -> None:
    """Poll monochromator until not busy, with timeout."""
    t0 = time.monotonic()
    while await mono.is_busy():
        if time.monotonic() - t0 > timeout:
            raise TimeoutError(f"Monochromator still busy after {timeout}s")
        await asyncio.sleep(0.5)


async def acquire_spectrum(
    ccd: ChargeCoupledDevice,
    exposure_time_ms: int,
    open_shutter: bool = True,
    timeout: float = 30.0,
) -> dict:
    """Run a single CCD acquisition and return raw data dict."""
    # Abort any stuck acquisition from a previous failure
    try:
        if await ccd.get_acquisition_busy():
            print("  Aborting stuck acquisition...")
            await ccd.acquisition_abort()
            await asyncio.sleep(1.0)
    except Exception:
        pass

    await ccd.set_exposure_time(exposure_time_ms)

    if not await ccd.get_acquisition_ready():
        raise RuntimeError("CCD not ready for acquisition")

    await ccd.acquisition_start(open_shutter=open_shutter)
    await asyncio.sleep(1.0)  # let CCD start before polling

    t0 = time.monotonic()
    while await ccd.get_acquisition_busy():
        if time.monotonic() - t0 > timeout:
            await ccd.acquisition_abort()
            raise TimeoutError(f"CCD acquisition timed out after {timeout}s")
        await asyncio.sleep(0.5)

    raw = await ccd.get_acquisition_data()
    await ccd.acquisition_abort()  # reset CCD state for next acquisition
    print(f"  Raw acquisition keys: {list(raw.keys())}")
    for i, acq in enumerate(raw.get("acquisition", [])):
        print(
            f"  acquisition[{i}]: acqIndex={acq.get('acqIndex')}, ROIs={len(acq.get('roi', []))}"
        )
        for j, roi in enumerate(acq.get("roi", [])):
            print(
                f"    roi[{j}]: xData={len(roi.get('xData', []))} pts, yData rows={len(roi.get('yData', []))}"
            )
    return raw


def extract_spectrum(
    raw_data: dict, center_wl: float, exposure_ms: int
) -> SpectrumData:
    """Extract wavelengths and intensities from acquisition data dict."""
    acq = raw_data["acquisition"][0]
    roi = acq["roi"][0]
    return SpectrumData(
        wavelengths=np.array(roi["xData"], dtype=np.float64),
        intensities=np.array(roi["yData"][0], dtype=np.float64),
        timestamp=raw_data.get("timestamp", ""),
        center_wavelength=center_wl,
        exposure_time_ms=exposure_ms,
    )


def print_spectrum_summary(label: str, spectrum: SpectrumData) -> None:
    wl = spectrum.wavelengths
    intens = spectrum.intensities
    print(f"\n--- {label} ---")
    print(f"  Timestamp:        {spectrum.timestamp}")
    print(f"  Center WL:        {spectrum.center_wavelength:.1f} nm")
    print(f"  Exposure:         {spectrum.exposure_time_ms} ms")
    print(f"  Pixels:           {len(wl)}")
    if len(wl) > 0:
        print(f"  Wavelength range: {wl[0]:.2f} - {wl[-1]:.2f} nm")
        print(f"  Intensity range:  {intens.min():.1f} - {intens.max():.1f}")
        print(f"  Intensity mean:   {intens.mean():.1f}")
        print(f"  Intensity std:    {intens.std():.1f}")

        # Print every 50th point so the full shape is visible
        step = max(1, len(wl) // 50)
        print(f"\n  {'Wavelength (nm)':>16}  {'Intensity':>12}")
        print(f"  {'-' * 16}  {'-' * 12}")
        for i in range(0, len(wl), step):
            print(f"  {wl[i]:16.2f}  {intens[i]:12.1f}")
        # Always include the last point
        if (len(wl) - 1) % step != 0:
            print(f"  {wl[-1]:16.2f}  {intens[-1]:12.1f}")


# ---------------------------------------------------------------------------
# Test steps
# ---------------------------------------------------------------------------


async def test_connect_and_discover(args: argparse.Namespace) -> DeviceManager:
    """Step 1: Connect to ICL and discover devices."""
    print("=" * 60)
    print("STEP 1: Connect to ICL and discover devices")
    print("=" * 60)

    dm = DeviceManager(
        start_icl=args.start_icl,
        icl_ip=args.icl_ip,
        icl_port=str(args.icl_port),
        enable_binary_messages=False,
    )
    await dm.start()

    monos = dm.monochromators
    ccds = dm.charge_coupled_devices
    print(f"  Monochromators found: {len(monos)}")
    for i, m in enumerate(monos):
        print(f"    [{i}] id={m.id()}, type={type(m).__name__}")
    print(f"  CCDs found:          {len(ccds)}")
    for i, c in enumerate(ccds):
        print(f"    [{i}] id={c.id()}, type={type(c).__name__}")

    if len(monos) == 0:
        raise RuntimeError("No monochromator found. Check USB connection and ICL.")
    if len(ccds) == 0:
        raise RuntimeError("No CCD found. Check USB connection and ICL.")

    return dm


async def test_initialize_mono(
    mono: Monochromator,
    slit_a_mm: float = 0.2,
    slit_b_mm: float = 0.2,
) -> dict:
    """Step 2: Initialize monochromator, read configuration.

    Slit widths are set before reading positions. Pass a negative value to
    leave the corresponding slit at its current position.
    """
    print("\n" + "=" * 60)
    print("STEP 2: Initialize monochromator")
    print("=" * 60)

    await mono.open()

    if not await mono.is_initialized():
        print("  Initializing (homing)... this may take a minute")
        await mono.initialize()
        await wait_mono_ready(mono, timeout=180.0)
    print("  Monochromator initialized")

    config = await mono.configuration()
    print("  Configuration:")
    for key, val in config.items():
        print(f"    {key}: {val}")

    wl = await mono.get_current_wavelength()
    grating = await mono.get_turret_grating()
    print(f"  Current wavelength: {wl:.2f} nm")
    print(f"  Current grating:    {grating}")

    # List available gratings
    print("  Available gratings:")
    for g in Monochromator.Grating:
        try:
            print(f"    {g.name}: {g.value}")
        except Exception:
            pass

    # Set and read slit positions
    slit_targets = {
        Monochromator.Slit.A: slit_a_mm,
        Monochromator.Slit.B: slit_b_mm,
    }
    for slit, target_mm in slit_targets.items():
        if target_mm < 0:
            print(f"  Slit {slit.name}: leaving at current position")
        else:
            try:
                await mono.set_slit_position(slit, target_mm)
                await wait_mono_ready(mono)
                print(f"  Slit {slit.name}: set to {target_mm:.3f} mm")
            except Exception as e:
                print(f"  Slit {slit.name}: set failed ({e})")

        try:
            pos = await mono.get_slit_position_in_mm(slit)
            print(f"  Slit {slit.name} position:  {pos:.3f} mm")
        except Exception as e:
            print(f"  Slit {slit.name}: read failed ({e})")

    # Read shutter status
    try:
        shutter_pos = await mono.get_shutter_position(Monochromator.Shutter.FIRST)
        print(f"  Shutter status:     {shutter_pos}")
    except Exception as e:
        print(f"  Shutter: not available ({e})")

    # Read mirror positions
    for mirror in [Monochromator.Mirror.ENTRANCE, Monochromator.Mirror.EXIT]:
        try:
            pos = await mono.get_mirror_position(mirror)
            print(f"  Mirror {mirror.name}: {pos}")
        except Exception as e:
            print(f"  Mirror {mirror.name}: not available ({e})")

    return config


async def test_initialize_ccd(ccd: ChargeCoupledDevice) -> tuple[int, int]:
    """Step 3: Initialize CCD and do a minimal test acquisition.

    Follows the exact pattern from the SDK's own test_ccd_functionality:
    open -> restart -> sleep(10) -> set_format -> set_roi -> acquire
    """
    print("\n" + "=" * 60)
    print("STEP 3: Initialize CCD")
    print("=" * 60)

    await ccd.open()

    chip_size = await ccd.get_chip_size()
    temperature = await ccd.get_chip_temperature()
    print(f"  Chip size:        {chip_size.width} x {chip_size.height} pixels")
    print(f"  Chip temperature: {temperature:.1f} C")

    return chip_size.width, chip_size.height


async def test_set_wavelength(mono: Monochromator, wavelength: float) -> float:
    """Step 4: Move monochromator to target wavelength."""
    print("\n" + "=" * 60)
    print(f"STEP 4: Move to {wavelength:.1f} nm")
    print("=" * 60)

    t0 = time.monotonic()
    await mono.move_to_target_wavelength(wavelength)
    await wait_mono_ready(mono)
    elapsed = time.monotonic() - t0

    actual_wl = await mono.get_current_wavelength()
    print(f"  Requested:  {wavelength:.1f} nm")
    print(f"  Actual:     {actual_wl:.2f} nm")
    print(f"  Move time:  {elapsed:.1f} s")

    return actual_wl


async def test_setup_ccd_acquisition(
    ccd: ChargeCoupledDevice,
    mono: Monochromator | None,
    center_wavelength: float,
    chip_size: tuple[int, int],
    grating_index: int = 0,
) -> None:
    """Step 5: Configure CCD for acquisition."""
    print("\n" + "=" * 60)
    print("STEP 5: Configure CCD acquisition")
    print("=" * 60)

    chip_x, chip_y = chip_size

    # Set grating explicitly (ICL needs this for wavelength conversion)
    if mono is not None:
        grating = Monochromator.Grating(grating_index)
        await mono.set_turret_grating(grating)
        await wait_mono_ready(mono)
        actual = await mono.get_turret_grating()
        print(f"  Grating: {actual.name} (index {grating_index})")

    # Full chip, single ROI, vertical binning
    await ccd.set_acquisition_format(1, AcquisitionFormat.SPECTRA_IMAGE)
    await ccd.set_region_of_interest(
        roi_index=1,
        x_origin=0,
        y_origin=0,
        x_size=chip_x,
        y_size=chip_y,
        x_bin=1,
        y_bin=chip_y,  # full vertical binning
    )

    # Wavelength calibration
    if mono is not None:
        await ccd.set_center_wavelength(mono.id(), center_wavelength)
        await ccd.set_x_axis_conversion_type(XAxisConversionType.FROM_ICL_SETTINGS_INI)
        print("  X-axis: wavelength from ICL settings")
        print(f"  Center wavelength: {center_wavelength:.2f} nm")
    else:
        print("  X-axis: pixel index (no monochromator for wavelength calibration)")

    # Timing and readout
    await ccd.set_timer_resolution(TimerResolution.MILLISECONDS)
    await ccd.set_acquisition_count(1)
    await ccd.set_gain(0)
    await ccd.set_speed(0)

    print(f"  ROI: {chip_x} x {chip_y}, full vertical binning")
    print("  Gain: 0 (High Light), Speed: 0 (50 kHz)")


async def test_single_spectrum(
    ccd: ChargeCoupledDevice,
    center_wl: float,
    exposure_ms: int,
) -> SpectrumData:
    """Step 6: Acquire a single spectrum with shutter open."""
    print("\n" + "=" * 60)
    print(f"STEP 6: Single spectrum (exposure={exposure_ms} ms, shutter=OPEN)")
    print("=" * 60)

    raw = await acquire_spectrum(ccd, exposure_ms, open_shutter=True)
    spectrum = extract_spectrum(raw, center_wl, exposure_ms)
    print_spectrum_summary("Light spectrum", spectrum)
    return spectrum


async def test_dark_frame(
    ccd: ChargeCoupledDevice,
    center_wl: float,
    exposure_ms: int,
) -> SpectrumData:
    """Step 7: Acquire a dark frame with shutter closed."""
    print("\n" + "=" * 60)
    print(f"STEP 7: Dark frame (exposure={exposure_ms} ms, shutter=CLOSED)")
    print("=" * 60)

    raw = await acquire_spectrum(ccd, exposure_ms, open_shutter=False)
    spectrum = extract_spectrum(raw, center_wl, exposure_ms)
    print_spectrum_summary("Dark frame", spectrum)
    return spectrum


async def test_series_acquisition(
    ccd: ChargeCoupledDevice,
    center_wl: float,
    exposure_ms: int,
    count: int,
) -> list[SpectrumData]:
    """Step 8: Acquire a series of spectra (simulates continuous monitoring)."""
    print("\n" + "=" * 60)
    print(f"STEP 8: Series acquisition ({count} spectra, exposure={exposure_ms} ms)")
    print("=" * 60)

    spectra: list[SpectrumData] = []
    t0 = time.monotonic()

    for i in range(count):
        raw = await acquire_spectrum(ccd, exposure_ms, open_shutter=True)
        spectrum = extract_spectrum(raw, center_wl, exposure_ms)
        spectra.append(spectrum)
        elapsed = time.monotonic() - t0
        print(
            f"  [{i + 1}/{count}] t={elapsed:.1f}s  mean={spectrum.intensities.mean():.1f}  max={spectrum.intensities.max():.1f}"
        )

    total_time = time.monotonic() - t0
    rate = count / total_time if total_time > 0 else 0
    print(f"\n  Total time: {total_time:.1f} s")
    print(f"  Rate:       {rate:.2f} spectra/s")

    # Check stability across series
    means = np.array([s.intensities.mean() for s in spectra])
    print(f"  Mean intensity across series: {means.mean():.1f} +/- {means.std():.1f}")
    print(f"  Relative stability: {means.std() / means.mean() * 100:.2f}%")

    return spectra


async def test_multi_acquisition(
    ccd: ChargeCoupledDevice,
    center_wl: float,
    exposure_ms: int,
    count: int,
) -> list[SpectrumData]:
    """Step 9: Use CCD MultiAcq mode (hardware-level series)."""
    print("\n" + "=" * 60)
    print(f"STEP 9: MultiAcq mode ({count} acquisitions, exposure={exposure_ms} ms)")
    print("=" * 60)

    await ccd.set_acquisition_count(count)
    await ccd.set_exposure_time(exposure_ms)

    if not await ccd.get_acquisition_ready():
        raise RuntimeError("CCD not ready for multi-acquisition")

    t0 = time.monotonic()
    await ccd.acquisition_start(open_shutter=True)

    # Wait for all acquisitions to complete
    timeout = exposure_ms * count / 1000.0 + 30.0
    while await ccd.get_acquisition_busy():
        if time.monotonic() - t0 > timeout:
            await ccd.acquisition_abort()
            raise TimeoutError(f"MultiAcq timed out after {timeout:.0f}s")
        await asyncio.sleep(0.5)

    elapsed = time.monotonic() - t0
    raw = await ccd.get_acquisition_data()

    spectra: list[SpectrumData] = []
    for acq_entry in raw.get("acquisition", []):
        roi = acq_entry["roi"][0]
        spectrum = SpectrumData(
            wavelengths=np.array(roi["xData"], dtype=np.float64),
            intensities=np.array(roi["yData"][0], dtype=np.float64),
            timestamp=raw.get("timestamp", ""),
            center_wavelength=center_wl,
            exposure_time_ms=exposure_ms,
        )
        spectra.append(spectrum)
        print(f"  Acq {acq_entry['acqIndex']}: mean={spectrum.intensities.mean():.1f}")

    print(f"\n  Total time: {elapsed:.1f} s for {len(spectra)} acquisitions")

    # Reset to single acquisition mode
    await ccd.set_acquisition_count(1)

    return spectra


async def test_range_scan(
    ccd: ChargeCoupledDevice,
    mono: Monochromator,
    start_wl: float,
    end_wl: float,
    exposure_ms: int,
) -> SpectrumData | None:
    """Step 10: Range scan — acquire across a wide wavelength range and stitch."""
    print("\n" + "=" * 60)
    print(f"STEP 10: Range scan ({start_wl:.0f} - {end_wl:.0f} nm)")
    print("=" * 60)

    # Ask SDK to calculate center wavelengths needed to cover the range
    pixel_overlap = 50
    center_wavelengths = await ccd.range_mode_center_wavelengths(
        mono.id(), start_wl, end_wl, pixel_overlap
    )
    print(f"  Center wavelengths: {len(center_wavelengths)} positions")
    for i, cwl in enumerate(center_wavelengths):
        print(f"    [{i}] {cwl:.1f} nm")

    captures: list[list] = []
    t0 = time.monotonic()

    for i, cwl in enumerate(center_wavelengths):
        # Move mono
        await mono.move_to_target_wavelength(cwl)
        await wait_mono_ready(mono)
        actual_wl = await mono.get_current_wavelength()

        # Tell CCD about the new center wavelength
        await ccd.set_center_wavelength(mono.id(), actual_wl)

        # Acquire
        await ccd.set_exposure_time(exposure_ms)
        if not await ccd.get_acquisition_ready():
            print(f"  [{i + 1}/{len(center_wavelengths)}] CCD not ready, skipping")
            continue

        await ccd.acquisition_start(open_shutter=True)
        await asyncio.sleep(1.0)
        acq_t0 = time.monotonic()
        while await ccd.get_acquisition_busy():
            if time.monotonic() - acq_t0 > 30:
                await ccd.acquisition_abort()
                raise TimeoutError(f"Acquisition timed out at {cwl:.1f} nm")
            await asyncio.sleep(0.5)

        raw = await ccd.get_acquisition_data()
        await ccd.acquisition_abort()
        roi = raw["acquisition"][0]["roi"][0]
        x_data = roi["xData"]
        y_data = roi["yData"]
        captures.append([x_data, y_data])

        elapsed = time.monotonic() - t0
        print(
            f"  [{i + 1}/{len(center_wavelengths)}] center={actual_wl:.1f} nm  "
            f"range={x_data[0]:.1f}-{x_data[-1]:.1f} nm  t={elapsed:.1f}s"
        )

    total_time = time.monotonic() - t0
    print(f"\n  Total scan time: {total_time:.1f} s")

    if not captures:
        print("  No captures — range scan failed")
        return None

    # Stitch spectra together
    stitch = LinearSpectraStitch(captures)
    stitched = stitch.stitched_spectra()
    wl_all = np.array(stitched[0], dtype=np.float64)
    intens_all = np.array(stitched[1][0], dtype=np.float64)

    # Filter to requested range
    mask = (wl_all >= start_wl) & (wl_all <= end_wl)
    wl_filtered = wl_all[mask]
    intens_filtered = intens_all[mask]

    spectrum = SpectrumData(
        wavelengths=wl_filtered,
        intensities=intens_filtered,
        timestamp="",
        center_wavelength=(start_wl + end_wl) / 2,
        exposure_time_ms=exposure_ms,
    )
    print_spectrum_summary(f"Range scan {start_wl:.0f}-{end_wl:.0f} nm", spectrum)
    return spectrum


async def test_grating_switch(mono: Monochromator) -> None:
    """Step 10: Test grating switching."""
    print("\n" + "=" * 60)
    print("STEP 10: Grating switch test")
    print("=" * 60)

    current = await mono.get_turret_grating()
    print(f"  Current grating: {current}")

    # Try switching to each grating and back
    for grating in Monochromator.Grating:
        if grating == current:
            continue
        try:
            print(f"  Switching to grating {grating.name}...")
            t0 = time.monotonic()
            await mono.set_turret_grating(grating)
            await wait_mono_ready(mono, timeout=120.0)
            elapsed = time.monotonic() - t0
            print(f"  Switched to {grating.name} in {elapsed:.1f}s")
            break  # only test one switch
        except Exception as e:
            print(f"  Grating {grating.name} not available: {e}")

    # Switch back
    print(f"  Switching back to {current.name}...")
    await mono.set_turret_grating(current)
    await wait_mono_ready(mono)
    print(f"  Restored grating {current.name}")


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------


async def run_all_tests(args: argparse.Namespace) -> None:
    # Step 1 is critical — can't do anything without ICL connection
    dm = await test_connect_and_discover(args)

    mono = dm.monochromators[0]
    ccd = dm.charge_coupled_devices[0]

    results: list[tuple[str, str]] = [("1. Connect to ICL", "PASS")]
    mono_ok = False
    ccd_ok = False
    chip_size: tuple[int, int] = (1024, 256)
    center_wl = args.wavelength

    try:
        # Step 2: Init monochromator (optional — CCD can work without it for BBM)
        try:
            await test_initialize_mono(mono, args.slit_a, args.slit_b)
            mono_ok = True
            results.append(("2. Init monochromator", "PASS"))
        except Exception as e:
            results.append(("2. Init monochromator", f"FAIL: {e}"))
            print(f"\n  ** Monochromator init failed: {e}")
            print("  ** CCD tests will continue without monochromator")

        # Step 3: Init CCD
        try:
            chip_size = await test_initialize_ccd(ccd)
            ccd_ok = True
            results.append(("3. Init CCD", "PASS"))
        except Exception as e:
            results.append(("3. Init CCD", f"FAIL: {e}"))
            print(f"\n  ** CCD init failed: {e}")

        # Step 4: Move to wavelength (requires mono)
        if mono_ok:
            try:
                center_wl = await test_set_wavelength(mono, args.wavelength)
                results.append(("4. Set wavelength", "PASS"))
            except Exception as e:
                results.append(("4. Set wavelength", f"FAIL: {e}"))
                print(f"\n  ** Wavelength move failed: {e}")
        else:
            results.append(("4. Set wavelength", "SKIP (no monochromator)"))

        # Step 5: Configure CCD acquisition (requires CCD; mono optional for calibration)
        if ccd_ok:
            try:
                await test_setup_ccd_acquisition(
                    ccd,
                    mono if mono_ok else None,
                    center_wl,
                    chip_size,
                    grating_index=args.grating,
                )
                results.append(("5. Configure CCD", "PASS"))
            except Exception as e:
                results.append(("5. Configure CCD", f"FAIL: {e}"))
                print(f"\n  ** CCD configuration failed: {e}")
                ccd_ok = False
        else:
            results.append(("5. Configure CCD", "SKIP (CCD not initialized)"))

        # Step 6: Single spectrum
        if ccd_ok:
            try:
                light = await test_single_spectrum(ccd, center_wl, args.exposure)
                results.append(("6. Single spectrum", "PASS"))
            except Exception as e:
                results.append(("6. Single spectrum", f"FAIL: {e}"))
                print(f"\n  ** Single spectrum failed: {e}")
        else:
            results.append(("6. Single spectrum", "SKIP (CCD not ready)"))

        # Step 7: Range scan (requires mono + CCD)
        if ccd_ok and mono_ok:
            try:
                await test_range_scan(
                    ccd, mono, args.start_wl, args.end_wl, args.exposure
                )
                results.append(("7. Range scan", "PASS"))
            except Exception as e:
                results.append(("7. Range scan", f"FAIL: {e}"))
                print(f"\n  ** Range scan failed: {e}")
        else:
            results.append(("7. Range scan", "SKIP (need mono + CCD)"))

        # Step 8: Dark frame
        if ccd_ok:
            try:
                dark = await test_dark_frame(ccd, center_wl, args.exposure)
                results.append(("8. Dark frame", "PASS"))
            except Exception as e:
                results.append(("8. Dark frame", f"FAIL: {e}"))
                print(f"\n  ** Dark frame failed: {e}")
        else:
            results.append(("8. Dark frame", "SKIP (CCD not ready)"))

        # Step 9: Series acquisition
        if ccd_ok:
            try:
                await test_series_acquisition(
                    ccd, center_wl, args.exposure, args.series_count
                )
                results.append(("9. Series acquisition", "PASS"))
            except Exception as e:
                results.append(("9. Series acquisition", f"FAIL: {e}"))
                print(f"\n  ** Series acquisition failed: {e}")
        else:
            results.append(("9. Series acquisition", "SKIP (CCD not ready)"))

        # Step 10: MultiAcq mode
        if ccd_ok and args.series_count >= 2:
            try:
                await test_multi_acquisition(
                    ccd, center_wl, args.exposure, min(args.series_count, 5)
                )
                results.append(("10. MultiAcq mode", "PASS"))
            except Exception as e:
                results.append(("10. MultiAcq mode", f"FAIL: {e}"))
                print(f"\n  ** MultiAcq failed: {e}")
        elif not ccd_ok:
            results.append(("10. MultiAcq mode", "SKIP (CCD not ready)"))

        # Step 11: Grating switch (opt-in)
        if args.test_grating:
            if mono_ok:
                try:
                    await test_grating_switch(mono)
                    results.append(("11. Grating switch", "PASS"))
                except Exception as e:
                    results.append(("11. Grating switch", f"FAIL: {e}"))
                    print(f"\n  ** Grating switch failed: {e}")
            else:
                results.append(("11. Grating switch", "SKIP (no monochromator)"))

    finally:
        print("\nCleaning up...")
        try:
            await ccd.close()
        except Exception as e:
            print(f"  CCD close error: {e}")
        await asyncio.sleep(0.5)
        try:
            await mono.close()
        except Exception as e:
            print(f"  Mono close error: {e}")
        await dm.stop()

    # Print summary
    print("\n" + "=" * 60)
    print("TEST RESULTS SUMMARY")
    print("=" * 60)
    n_pass = sum(1 for _, s in results if s == "PASS")
    n_fail = sum(1 for _, s in results if s.startswith("FAIL"))
    n_skip = sum(1 for _, s in results if s.startswith("SKIP"))
    for name, status in results:
        print(f"  {name}: {status}")
    print(f"\n  {n_pass} passed, {n_fail} failed, {n_skip} skipped")
    if n_fail == 0:
        print("\n  ALL TESTS PASSED")
    print("=" * 60)


def main():
    parser = argparse.ArgumentParser(
        description="HORIBA Monochromator + CCD integration test for OptiMonitor (iHR320, iHR550, etc.)",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument("--icl-ip", default="127.0.0.1", help="ICL WebSocket IP")
    parser.add_argument(
        "--icl-port", type=int, default=25010, help="ICL WebSocket port"
    )
    parser.add_argument(
        "--start-icl",
        action="store_true",
        help="Auto-start ICL.exe (default: don't start)",
    )
    parser.add_argument(
        "--wavelength", type=float, default=500.0, help="Target wavelength in nm"
    )
    parser.add_argument(
        "--exposure", type=int, default=1000, help="Exposure time in ms"
    )
    parser.add_argument(
        "--series-count", type=int, default=5, help="Number of spectra in series test"
    )
    parser.add_argument(
        "--start-wl", type=float, default=400.0, help="Range scan start wavelength (nm)"
    )
    parser.add_argument(
        "--end-wl", type=float, default=600.0, help="Range scan end wavelength (nm)"
    )
    parser.add_argument(
        "--grating",
        type=int,
        default=0,
        help="Grating index: 0=FIRST(1800), 1=SECOND(1200), 2=THIRD(300)",
    )
    parser.add_argument(
        "--slit-a",
        type=float,
        default=0.2,
        help="Entrance slit A width in mm (negative to leave at current position)",
    )
    parser.add_argument(
        "--slit-b",
        type=float,
        default=0.2,
        help="Slit B width in mm (negative to leave at current position)",
    )
    parser.add_argument(
        "--test-grating", action="store_true", help="Test grating switching (slow)"
    )

    args = parser.parse_args()

    try:
        asyncio.run(run_all_tests(args))
    except ConnectionRefusedError:
        print(f"\nERROR: Cannot connect to ICL at {args.icl_ip}:{args.icl_port}")
        print("Make sure ICL.exe is running, or use --start-icl to auto-start it.")
    except KeyboardInterrupt:
        print("\nInterrupted by user.")
    except Exception as e:
        print(f"\nERROR: {e}")
        raise


if __name__ == "__main__":
    main()
