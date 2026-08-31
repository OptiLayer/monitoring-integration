# Solar LS SDK runtime files

Redistributable runtime files for the **Solar LS M266-IV** monochromator,
copied out of the vendor's `SDK.zip` (SDK Manual rev 1.2, DLLs dated 2022-04-07).
The service loads `SolarLS.SdkExport.dll` at runtime; the rest are its
dependency closure. The release workflow zips this folder next to the exe.

Nothing here is needed to *build* the service — only to run `--mono` against
real hardware. Linux development uses `--mono sim` and needs none of it.

## Provenance

| File | Source in `SDK.zip` |
|------|---------------------|
| `SolarLS.SdkExport.dll` | `SDK/Release/**x64**/SolarLS.SdkExport.dll` |
| everything else | `SDK/Release/` |

> `SolarLS.SdkExport.dll` exists twice in the SDK under the same name. The copy
> in `Release/` is an AnyCPU managed wrapper that exports no native symbols; the
> one in `Release/x64/` is the mixed-mode DLL that exports `sls_*`. Ours is the
> `x64` one — verify with `strings SolarLS.SdkExport.dll | grep -c '^sls_'`
> (104 for the correct file, 101 for the wrong one) or check for `PE32+`.

## Deliberately excluded

Detector support that this instrument does not use: `SolarLS.Camera.Andor`,
`SolarLS.Camera.Ormins`, `SolarLS.PMT.Hamamatsu`, `SolarLS.Bh.Spc130`,
`MccDaq`, and the native `atmcd*` / `cbw*` / `spcm*` / `H11890api` drivers.
`SolarLS.Sdk.dll` references them, but .NET resolves assemblies lazily and a
monochromator-only config never touches them — the SDK's own example `bin\`
folders ship without them too.

`SolarLS.PMT.Hamamatsu.dll` is additionally flagged 32BITREQUIRED, so an
instrument config containing a Hamamatsu PMT could not run in our x64 process
at all.

Also excluded, because they are build-time only and we bind at runtime rather
than link: `solarls_sdk.h`, `SolarLS.SdkExport.lib`, `.exp`, `.pdb`, the
examples, the LabVIEW `.llb` and the manual.

## Instrument configuration

`InstrumentCfg_M266#SM2-150.xml` describes **our** M266-IV (serial `#SM2-150`):
four gratings, USB board IDs, backlash and limits. Swap it if the instrument is
replaced — the SDK finds zero instruments without a matching config, and
`--mono` then fails at startup.

## Requirement on the target machine

.NET Framework 4.0 or later. Windows only.
