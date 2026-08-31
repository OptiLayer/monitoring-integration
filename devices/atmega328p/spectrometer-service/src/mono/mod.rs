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
    fn move_to(&self, nm: f64, pinned_grating: Option<i32>) -> Result<f64, String> {
        let grating = self.choose_grating(nm, pinned_grating)?;

        if grating != delegate!(self, active_grating)? {
            tracing::info!("mono: switching to grating {grating} for {nm} nm");
            delegate!(self, set_active_grating, grating)?;
        }

        delegate!(self, set_wavelength, nm)?;
        delegate!(self, wavelength)
    }

    fn choose_grating(&self, nm: f64, pinned: Option<i32>) -> Result<i32, String> {
        if let Some(g) = pinned {
            if !delegate!(self, is_valid_wl, g, nm)? {
                return Err(format!("{nm} nm is out of range for pinned grating {g}"));
            }
            return Ok(g);
        }

        let active = delegate!(self, active_grating)?;
        let count = delegate!(self, grating_count)?;
        let mut valid = Vec::new();
        for g in 0..count {
            if delegate!(self, is_valid_wl, g, nm)? {
                valid.push(g);
            }
        }

        pick_grating(active, &valid)
            .ok_or_else(|| format!("{nm} nm is out of range for every installed grating"))
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

/// Prefer staying on the active grating when it can reach the wavelength.
///
/// Switching gratings changes throughput and stray light, so an unnecessary
/// switch mid-scan puts a step in the spectrum. Only move when we must, and
/// then take the highest-dispersion (lowest-index) grating that can reach it.
fn pick_grating(active: i32, valid: &[i32]) -> Option<i32> {
    if valid.contains(&active) {
        return Some(active);
    }
    valid.first().copied()
}

/// Handle to the instrument. Calls are serialised — the grating is one motor.
pub struct Monochromator {
    backend: Arc<Backend>,
    lock: Mutex<()>,
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

        Ok(Self {
            backend: Arc::new(backend),
            lock: Mutex::new(()),
            pinned_grating,
        })
    }

    pub fn description(&self) -> &str {
        self.backend.description()
    }

    /// Move to `nm` and return the wavelength the instrument reports afterwards.
    pub async fn set_wavelength(&self, nm: f64) -> Result<f64, String> {
        let _guard = self.lock.lock().await;
        let backend = self.backend.clone();
        let pinned = self.pinned_grating;

        tokio::task::spawn_blocking(move || backend.move_to(nm, pinned))
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

    #[test]
    fn keeps_active_grating_when_it_can_reach() {
        // 500 nm is reachable by gratings 0, 1, 2 and 3 — stay put on 2.
        assert_eq!(pick_grating(2, &[0, 1, 2, 3]), Some(2));
    }

    #[test]
    fn switches_only_when_active_cannot_reach() {
        // 900 nm: gratings 0 (<=540) and 1 (<=800) are out.
        assert_eq!(pick_grating(1, &[2, 3]), Some(2));
    }

    #[test]
    fn no_grating_reaches() {
        assert_eq!(pick_grating(0, &[]), None);
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

        // 700 nm is within grating 1 — stay put rather than change throughput.
        mono.set_wavelength(700.0).await.unwrap();
        assert_eq!(mono.backend.active_grating().unwrap(), 1);

        // 900 nm is not — the next grating up (2, 600 gr/mm) has to take over.
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
