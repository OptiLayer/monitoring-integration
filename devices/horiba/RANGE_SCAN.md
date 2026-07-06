# Range-Scan Shape: Why the Broadband Spectrum Looked Wrong

Field notes from debugging the iHR320 + CCD broadband range scan against a
customer's stock **SynapsePlus** software. Written after a multi-day
investigation at a customer site, so the next person doesn't repeat it.

## The symptom

Measuring a **halogen lamp** (smooth continuum, no lines) with `horiba_test.py`'s
range scan (step 7) produced a **wrong shape** — inverted in the blue, with a hard
seam mid-range — while SynapsePlus on the same PC, same lamp, same settings showed
a clean halogen rising to a broad peak at ~735 nm.

Both outputs are **raw counts** (SynapsePlus metadata: `Units: Counts`, no dark
subtract). So this was never a units or calibration difference — same hardware,
same raw quantity, different shape.

## Root cause

The range scan is not one measurement. It moves the grating to several **center
wavelengths**, takes one CCD frame at each, and **stitches** them into a wide
spectrum. Two things combine to distort the result:

1. **Per-frame efficiency droop.** A single CCD frame does *not* measure light
   evenly across its width. Instrument efficiency (grating throughput × CCD QE) is
   highest near the frame center and falls off toward the edges. So each raw frame,
   on its own, is warped — it does not represent the lamp. **Only the pixels near a
   frame's center track the true lamp shape.**

2. **Coarse stitching kept the droopy edges.** With few, barely-overlapping frames,
   most output wavelengths came from low-efficiency frame *edges*, and the stitch
   glued whole frames together — droop included — producing a sawtooth (one tooth
   per frame) whose overall shape was wrong.

### The proof (frame centers vs edges)

Four single frames were captured at centers 465 / 595 / 725 / 855 nm (grating 2).
Each frame's shape was different and disagreed with its neighbors by up to ~40× in
the overlap regions — clearly not a faithful slice of the lamp. **But the net value
at each frame's dead center** (465→388, 595→2407, 725→4256, 855→1856 counts) traced
a clean halogen peaking at ~725 nm — matching SynapsePlus's ~735 nm within 10 nm.

Conclusion: frame *centers* are trustworthy; frame *edges* are warped. Reproducing
SynapsePlus is a **sampling** problem (use the centers), not a response-correction
problem.

## What controls it

### Grating → number of frames

`ccd.range_mode_center_wavelengths(mono, start, end, pixel_overlap)` decides how many
frames cover the range, based on the **grating's dispersion**. Higher dispersion =
narrower frames = more of them = every output wavelength lands nearer a frame center.

Grating index (`--grating`): `0 = 1800 g/mm`, `1 = 1200 g/mm`, `2 = 300 g/mm`.

| Grating   | Frames over 500–900 | Shape result                                    |
| --------- | ------------------- | ----------------------------------------------- |
| 2 (300)   | ~2 (wide)           | Inverted / wrong — mostly warped edge pixels     |
| **1 (1200)** | **~7**           | **Correct halogen, peak ~735 nm** ✅              |
| 0 (1800)  | ~14 (narrow)        | Finest sampling, but blazed for blue: red-blind, slopes downhill. Wrong |

**Grating 1 is the sweet spot** — enough frames to sample near centers, enough
red/NIR efficiency to see the lamp where it's bright. Grating 0 samples finest but
its efficiency craters in the red (where the halogen peaks), so its shape is wrong
for a different reason.

### Overlap → whether the teeth survive

`--scan-overlap` (v0.28+, `pixel_overlap`, default 400) sets how many pixels adjacent
frames share. Bigger overlap places frames closer together, so each output wavelength
is dominated by frame *centers* and the droopy edges get down-weighted.

- **v0.27:** overlap hardcoded at **50** — no flag. Every grating still showed teeth,
  because the overlap that removes them was frozen low. Grating only changed *how
  many* teeth, not whether they existed.
- **v0.28:** overlap tunable. `400` softened the sharp seams into gentle ~50 nm waves;
  `800` flattened the waves into a clean curve.

### Stitch algorithm (SDK)

The SDK ships four stitchers. For per-frame droop, the weighted-average one is best;
the others are equal or worse. **None of them do any efficiency/response correction —
they only combine frames.**

| Algorithm         | Overlap behavior                          | Verdict for droop            |
| ----------------- | ----------------------------------------- | ---------------------------- |
| **LabSpec6** (v0.28) | Weighted average, ramped across overlap | **Best** — no hard seam       |
| Linear (v0.27)    | Plain 50/50 average                       | Softer than cut but still dips; has a stray debug `print` |
| SimpleCut         | Cut at seam, keep one frame               | Hard stairs — worse           |
| YDisplacement     | Manual Y offset then cut                  | For DC-offset frames; not this |

## The fix

**Grating 1 + high overlap, on v0.28+:**

```
horiba-test-v0.28.0.exe --grating 1 --start-wl 500 --end-wl 900 \
    --slit-a 0.5 --slit-b 0.5 --exposure 200 --scan-overlap 800 --output scan.csv
```

This produces a clean halogen straight from the file — peak plateau 730–760 nm,
red half sitting on the SynapsePlus curve — with **no post-processing**.

Signal notes:
- Higher-dispersion gratings spread the same light over more pixels, so open the
  **slit** (≈0.5 mm) rather than cranking exposure — the live monitor can't afford
  long exposures. Slit width doesn't matter for a smooth continuum (no lines to
  resolve).
- Shape is independent of slit and exposure; those only scale signal.

## Known remaining limitation

Even with the sawtooth fixed, the **blue-green flank (560–680 nm) reads low** — the
measured curve climbs to the peak more slowly than SynapsePlus. This is **not**
stitching; it's a residual **instrument-response / efficiency difference** (grating 1
is less efficient in the blue-green). No stitch algorithm or overlap setting fixes
it, because none of them correct for response.

Correcting the flank needs a **white-reference** normalization, which the production
`horiba_service` already does (dark/white → T%, see `calibration.py` and README
"Calibration"). The bare `horiba_test.py` tool does not — it is a raw-counts debug
tool by design.

So:
- Need "does it look like a halogen (smooth hump peaking in the red)?" → grating 1 +
  overlap 800 is enough.
- Need the flank to match a reference too → use the calibrated service, not the raw
  test tool.

## Gotchas

- **Mercury line at ~546 nm.** A sharp narrow spike near 546 nm in the blue is the
  room's overhead **fluorescent lighting** (Hg emission), not the lamp. Ignore it, or
  mask 543–560 nm. A halogen is a smooth continuum with no lines.
- **Grating 2 single frames peak at the frame edge**, not center, depending on where
  the center sits relative to the lamp+efficiency product — which is exactly why a
  single frame or a 2-frame stitch is misleading. Don't judge shape from one frame.

## What we could NOT verify

SynapsePlus's internal range-mode settings (its overlap, stitch method, frame count)
are **unknown** — we only ever saw its *output* (a smooth halogen) and its metadata
(raw counts). We reproduced the same result independently; we did not reverse-engineer
theirs. Don't state their internals as fact.
