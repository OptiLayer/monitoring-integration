//! Simulated monochromator so the endpoint and the dashboard can be developed
//! on Linux, where the Windows-only Solar LS SDK cannot be loaded.
//!
//! Grating table mirrors `InstrumentCfg_M266#SM2-150.xml` (M266-IV): four
//! gratings at 1800/1200/600/200 grooves/mm.

use std::sync::Mutex;

/// (grooves/mm, min nm, max nm) per grating, in SDK index order.
const GRATINGS: [(u32, f64, f64); 4] = [
    (1800, 0.0, 540.0),
    (1200, 0.0, 800.0),
    (600, 0.0, 1800.0),
    (200, 0.0, 5400.0),
];

/// Rough M266 slew rate; enough to make the settling flag observable in the UI.
const NM_PER_SEC: f64 = 400.0;

pub struct Sim {
    state: Mutex<(f64, i32)>, // (wavelength nm, active grating)
}

impl Sim {
    pub fn open() -> Self {
        Self {
            state: Mutex::new((550.0, 2)),
        }
    }

    pub fn description(&self) -> &str {
        "M266-IV #SIMULATED"
    }

    pub fn set_wavelength(&self, nm: f64) -> Result<(), String> {
        let travel = {
            let mut s = self.state.lock().unwrap();
            let travel = (nm - s.0).abs();
            s.0 = nm;
            travel
        };
        std::thread::sleep(std::time::Duration::from_secs_f64(travel / NM_PER_SEC));
        Ok(())
    }

    pub fn wavelength(&self) -> Result<f64, String> {
        Ok(self.state.lock().unwrap().0)
    }

    pub fn grating_count(&self) -> Result<i32, String> {
        Ok(GRATINGS.len() as i32)
    }

    pub fn active_grating(&self) -> Result<i32, String> {
        Ok(self.state.lock().unwrap().1)
    }

    pub fn set_active_grating(&self, grating: i32) -> Result<(), String> {
        if !(0..GRATINGS.len() as i32).contains(&grating) {
            return Err(format!("grating {grating} out of range"));
        }
        self.state.lock().unwrap().1 = grating;
        Ok(())
    }

    pub fn grating_prm(&self, grating: i32) -> Result<(u32, f64, f64), String> {
        let Some(&(grooves, min, max)) = GRATINGS.get(grating as usize) else {
            return Err(format!("grating {grating} out of range"));
        };
        Ok((grooves, min, max))
    }
}
