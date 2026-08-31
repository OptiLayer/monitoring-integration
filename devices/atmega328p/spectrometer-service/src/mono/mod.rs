//! Monochromator wavelength control.
//!
//! The real backend is the Solar LS SDK, which is Windows-only (mixed-mode
//! .NET 4.0 DLL over CyUSB). `--mono sim` selects a simulator so the endpoint
//! and dashboard can be developed and tested on Linux.

#[cfg(windows)]
mod sdk;
mod sim;

use std::sync::Arc;

use tokio::sync::Mutex;

enum Backend {
    Sim(sim::Sim),
    #[cfg(windows)]
    Sdk(sdk::Sdk),
}

/// Delegate a `&self -> Result<T, String>` call to whichever backend is active.
macro_rules! delegate {
    ($self:expr, $method:ident $(, $arg:expr)*) => {
        match $self {
            Backend::Sim(b) => b.$method($($arg),*),
            #[cfg(windows)]
            Backend::Sdk(b) => b.$method($($arg),*),
        }
    };
}

impl Backend {
    fn description(&self) -> &str {
        delegate!(self, description)
    }

    /// Select a grating if needed, move, and read back where we landed.
    /// Blocking: the SDK's `sls_SetWl` returns only once the grating settled.
    fn move_to(
        &self,
        nm: f64,
        gratings: &[Grating],
        pinned_grating: Option<i32>,
    ) -> Result<f64, String> {
        let grating = choose_grating(
            nm,
            gratings,
            delegate!(self, active_grating)?,
            pinned_grating,
        )?;

        if grating != delegate!(self, active_grating)? {
            tracing::info!("mono: switching to grating {grating} for {nm} nm");
            delegate!(self, set_active_grating, grating)?;
        }

        delegate!(self, set_wavelength, nm)?;
        delegate!(self, wavelength)
    }

    /// Read the installed gratings once. This is the call that touches the
    /// most of the SDK's device layer, so we do it at connect time — see the
    /// note on `Monochromator::connect`.
    fn read_gratings(&self) -> Result<Vec<Grating>, String> {
        let count = delegate!(self, grating_count)?;
        (0..count)
            .map(|index| {
                let (grooves, min_nm, max_nm) = delegate!(self, grating_prm, index)?;
                Ok(Grating {
                    index,
                    grooves,
                    min_nm,
                    max_nm,
                })
            })
            .collect()
    }

    fn wavelength(&self) -> Result<f64, String> {
        delegate!(self, wavelength)
    }

    #[cfg(test)]
    fn active_grating(&self) -> Result<i32, String> {
        delegate!(self, active_grating)
    }

    #[cfg(test)]
    fn set_active_grating(&self, grating: i32) -> Result<(), String> {
        delegate!(self, set_active_grating, grating)
    }
}

/// One installed diffraction grating and the range it can reach.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grating {
    pub index: i32,
    pub grooves: u32,
    pub min_nm: f64,
    pub max_nm: f64,
}

impl Grating {
    fn reaches(&self, nm: f64) -> bool {
        (self.min_nm..=self.max_nm).contains(&nm)
    }
}

/// Prefer staying on the active grating when it can reach the wavelength.
///
/// Switching gratings changes throughput and stray light, so an unnecessary
/// switch mid-scan puts a step in the spectrum. Only move when we must, and
/// then take the highest-dispersion (lowest-index) grating that can reach it.
fn choose_grating(
    nm: f64,
    gratings: &[Grating],
    active: i32,
    pinned: Option<i32>,
) -> Result<i32, String> {
    if let Some(g) = pinned {
        let Some(grating) = gratings.iter().find(|x| x.index == g) else {
            return Err(format!("pinned grating {g} is not installed"));
        };
        if !grating.reaches(nm) {
            return Err(format!(
                "{nm} nm is outside pinned grating {g} ({}-{} nm)",
                grating.min_nm, grating.max_nm
            ));
        }
        return Ok(g);
    }

    if gratings.iter().any(|g| g.index == active && g.reaches(nm)) {
        return Ok(active);
    }

    gratings
        .iter()
        .find(|g| g.reaches(nm))
        .map(|g| g.index)
        .ok_or_else(|| format!("{nm} nm is out of range for every installed grating"))
}

/// Handle to the instrument. Calls are serialised — the grating is one motor.
pub struct Monochromator {
    backend: Arc<Backend>,
    lock: Mutex<()>,
    gratings: Vec<Grating>,
    pinned_grating: Option<i32>,
}

impl Monochromator {
    /// `target` is either `"sim"` or a filesystem path to the folder holding
    /// the instrument's `InstrumentCfg*.xml` (empty string = executable folder).
    pub fn connect(
        target: &str,
        instrument_index: i32,
        pinned_grating: Option<i32>,
    ) -> Result<Self, String> {
        let backend = match target {
            "sim" => Backend::Sim(sim::Sim::open()),
            #[cfg(windows)]
            path => Backend::Sdk(sdk::Sdk::open(
                (!path.is_empty()).then_some(path),
                instrument_index,
            )?),
            #[cfg(not(windows))]
            _ => {
                let _ = instrument_index;
                return Err(
                    "the Solar LS SDK is Windows-only — use `--mono sim` for development"
                        .to_string(),
                );
            }
        };

        tracing::info!("mono: connected to {}", backend.description());

        // Read the grating table up front rather than per move. It never
        // changes, it saves a handful of FFI calls on every wavelength set,
        // and it front-loads the risk: with no instrument reachable the SDK
        // throws inside sls_GetGratingCount, and because that is an unhandled
        // exception in its own managed code it takes the process down rather
        // than returning an error we could report. Far better that happens now,
        // at startup, than three hours into a deposition run.
        let gratings = backend.read_gratings()?;
        if gratings.is_empty() {
            return Err("instrument reports no gratings".to_string());
        }
        for g in &gratings {
            tracing::info!(
                "mono: grating {} — {} gr/mm, {}-{} nm",
                g.index,
                g.grooves,
                g.min_nm,
                g.max_nm
            );
        }

        Ok(Self {
            backend: Arc::new(backend),
            lock: Mutex::new(()),
            gratings,
            pinned_grating,
        })
    }

    /// The installed gratings and their ranges, read at connect time.
    pub fn gratings(&self) -> &[Grating] {
        &self.gratings
    }

    pub fn description(&self) -> &str {
        self.backend.description()
    }

    /// Move to `nm` and return the wavelength the instrument reports afterwards.
    pub async fn set_wavelength(&self, nm: f64) -> Result<f64, String> {
        let _guard = self.lock.lock().await;
        let backend = self.backend.clone();
        let gratings = self.gratings.clone();
        let pinned = self.pinned_grating;

        tokio::task::spawn_blocking(move || backend.move_to(nm, &gratings, pinned))
            .await
            .map_err(|e| format!("mono task panicked: {e}"))?
    }

    pub async fn wavelength(&self) -> Result<f64, String> {
        let backend = self.backend.clone();
        tokio::task::spawn_blocking(move || backend.wavelength())
            .await
            .map_err(|e| format!("mono task panicked: {e}"))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The M266-IV table, as `sim` reports it.
    fn m266() -> Vec<Grating> {
        vec![
            Grating {
                index: 0,
                grooves: 1800,
                min_nm: 0.0,
                max_nm: 540.0,
            },
            Grating {
                index: 1,
                grooves: 1200,
                min_nm: 0.0,
                max_nm: 800.0,
            },
            Grating {
                index: 2,
                grooves: 600,
                min_nm: 0.0,
                max_nm: 1800.0,
            },
            Grating {
                index: 3,
                grooves: 200,
                min_nm: 0.0,
                max_nm: 5400.0,
            },
        ]
    }

    #[test]
    fn keeps_active_grating_when_it_can_reach() {
        // 500 nm is reachable by every grating — stay on the active one
        // rather than stepping the spectrum for no reason.
        assert_eq!(choose_grating(500.0, &m266(), 2, None).unwrap(), 2);
    }

    #[test]
    fn switches_only_when_active_cannot_reach() {
        // 900 nm: gratings 0 (<=540) and 1 (<=800) cannot; 2 is the next up.
        assert_eq!(choose_grating(900.0, &m266(), 1, None).unwrap(), 2);
    }

    #[test]
    fn rejects_wavelength_no_grating_reaches() {
        assert!(choose_grating(9000.0, &m266(), 0, None).is_err());
    }

    #[test]
    fn pinned_grating_is_never_switched_away_from() {
        assert_eq!(choose_grating(500.0, &m266(), 2, Some(0)).unwrap(), 0);
        let err = choose_grating(900.0, &m266(), 2, Some(0)).unwrap_err();
        assert!(err.contains("pinned grating 0"), "{err}");
    }

    #[test]
    fn pinned_grating_must_be_installed() {
        assert!(choose_grating(500.0, &m266(), 0, Some(9)).is_err());
    }

    #[tokio::test]
    async fn sim_reads_grating_table_at_connect() {
        let mono = Monochromator::connect("sim", 0, None).unwrap();
        assert_eq!(mono.gratings(), m266());
    }

    #[tokio::test]
    async fn sim_moves_and_reads_back() {
        let mono = Monochromator::connect("sim", 0, None).unwrap();
        let actual = mono.set_wavelength(632.8).await.unwrap();
        assert!((actual - 632.8).abs() < 1e-9);
        assert!((mono.wavelength().await.unwrap() - 632.8).abs() < 1e-9);
    }

    #[tokio::test]
    async fn sim_switches_grating_only_when_forced() {
        let mono = Monochromator::connect("sim", 0, None).unwrap();

        // Park on grating 1, which reaches 800 nm at most.
        mono.backend.set_active_grating(1).unwrap();

        // 700 nm is within grating 1 — stay put.
        mono.set_wavelength(700.0).await.unwrap();
        assert_eq!(mono.backend.active_grating().unwrap(), 1);

        // 900 nm is not — grating 2 has to take over.
        mono.set_wavelength(900.0).await.unwrap();
        assert_eq!(mono.backend.active_grating().unwrap(), 2);
    }

    #[tokio::test]
    async fn sim_rejects_out_of_range_for_pinned_grating() {
        let mono = Monochromator::connect("sim", 0, Some(0)).unwrap();
        // Grating 0 tops out at 540 nm.
        assert!(mono.set_wavelength(900.0).await.is_err());
    }

    #[tokio::test]
    async fn sim_rejects_beyond_every_grating() {
        let mono = Monochromator::connect("sim", 0, None).unwrap();
        assert!(mono.set_wavelength(9000.0).await.is_err());
    }
}
