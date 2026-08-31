//! Windows-only binding to the Solar LS SDK (`SolarLS.SdkExport.dll`).
//!
//! The DLL is a mixed-mode .NET Framework 4.0 assembly, so it can only be
//! loaded on Windows. We load it at runtime rather than linking the import
//! library so that a missing/unpowered instrument degrades to an error on the
//! endpoint instead of preventing the service from starting at all.
//!
//! Deployment: `SolarLS.SdkExport.dll` must be the **x64** build from the SDK's
//! `Release\x64\` folder — the same-named file in `Release\` is the AnyCPU
//! managed wrapper and exports no native symbols. See the README for the full
//! list of assemblies that have to sit next to the executable.

use std::ffi::{CString, c_char, c_double, c_int};

use libloading::{Library, Symbol};

const DLL: &str = "SolarLS.SdkExport.dll";
const LOG_LEVEL_WARN: c_int = 4;

/// Loaded SDK plus the instrument index we drive.
pub struct Sdk {
    lib: Library,
    idx: c_int,
    description: String,
}

macro_rules! sym {
    ($self:expr, $name:literal, $ty:ty) => {
        unsafe { $self.lib.get::<$ty>($name.as_bytes()) }.map_err(|e| {
            format!(
                "{DLL}: missing symbol {}: {e} \
                     (is this the x64 build from the SDK's Release\\x64 folder?)",
                $name
            )
        })?
    };
}

impl Sdk {
    /// Load the DLL, initialise it against `config_path` (the folder holding
    /// the instrument XML; `None` = the executable's folder) and bind to
    /// instrument `idx`.
    pub fn open(config_path: Option<&str>, idx: i32) -> Result<Self, String> {
        let lib = unsafe { Library::new(DLL) }
            .map_err(|e| format!("failed to load {DLL}: {e} (is it next to the executable?)"))?;

        let mut sdk = Self {
            lib,
            idx,
            description: String::new(),
        };

        // Best effort: SDK log file next to ours. Not fatal if it fails.
        let log = CString::new("solarls_sdk.log").unwrap();
        let set_logging = sym!(
            sdk,
            "sls_SetLogging",
            unsafe extern "C" fn(c_int, *const c_char) -> c_int
        );
        unsafe { set_logging(LOG_LEVEL_WARN, log.as_ptr()) };

        let cfg = config_path
            .map(|p| CString::new(p).map_err(|_| "config path contains a NUL byte".to_string()))
            .transpose()?;
        let cfg_ptr = cfg.as_ref().map_or(std::ptr::null(), |c| c.as_ptr());

        let init = sym!(
            sdk,
            "sls_Init",
            unsafe extern "C" fn(*const c_char) -> c_int
        );
        if unsafe { init(cfg_ptr) } == 0 {
            return Err(format!("sls_Init failed: {}", sdk.last_error()));
        }

        let count = sdk.call_out_int("sls_GetInstrumentCount")?;
        if idx >= count {
            return Err(format!(
                "instrument index {idx} out of range: SDK loaded {count} instrument config(s)"
            ));
        }

        let name = sdk.string_of("sls_GetInstrumentName")?;
        let serial = sdk.string_of("sls_GetInstrumentSerial")?;
        sdk.description = format!("{name} {serial}");

        Ok(sdk)
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    /// Blocking move — the SDK's non-Async variant returns once the grating
    /// has settled, which saves us a status-polling loop.
    pub fn set_wavelength(&self, nm: f64) -> Result<(), String> {
        let f = sym!(
            self,
            "sls_SetWl",
            unsafe extern "C" fn(c_int, c_double) -> c_int
        );
        if unsafe { f(self.idx, nm) } == 0 {
            return Err(format!("sls_SetWl({nm}) failed: {}", self.last_error()));
        }
        Ok(())
    }

    pub fn wavelength(&self) -> Result<f64, String> {
        let f = sym!(
            self,
            "sls_GetWl",
            unsafe extern "C" fn(c_int, *mut c_double) -> c_int
        );
        let mut out: c_double = 0.0;
        if unsafe { f(self.idx, &mut out) } == 0 {
            return Err(format!("sls_GetWl failed: {}", self.last_error()));
        }
        Ok(out)
    }

    pub fn grating_count(&self) -> Result<i32, String> {
        self.call_out_int("sls_GetGratingCount")
    }

    pub fn active_grating(&self) -> Result<i32, String> {
        let f = sym!(
            self,
            "sls_GetActiveGrating",
            unsafe extern "C" fn(c_int, *mut c_int) -> c_int
        );
        let mut out: c_int = 0;
        if unsafe { f(self.idx, &mut out) } == 0 {
            return Err(format!(
                "sls_GetActiveGrating failed: {}",
                self.last_error()
            ));
        }
        Ok(out)
    }

    pub fn set_active_grating(&self, grating: i32) -> Result<(), String> {
        let f = sym!(
            self,
            "sls_SetActiveGrating",
            unsafe extern "C" fn(c_int, c_int) -> c_int
        );
        if unsafe { f(self.idx, grating) } == 0 {
            return Err(format!(
                "sls_SetActiveGrating({grating}) failed: {}",
                self.last_error()
            ));
        }
        Ok(())
    }

    pub fn is_valid_wl(&self, grating: i32, nm: f64) -> Result<bool, String> {
        let f = sym!(
            self,
            "sls_IsValidWlGrating",
            unsafe extern "C" fn(c_int, c_int, c_double, *mut c_int) -> c_int
        );
        let mut valid: c_int = 0;
        if unsafe { f(self.idx, grating, nm, &mut valid) } == 0 {
            return Err(format!(
                "sls_IsValidWlGrating failed: {}",
                self.last_error()
            ));
        }
        Ok(valid != 0)
    }

    /// `int fn(int* out)` — the shape shared by the various *Count getters.
    fn call_out_int(&self, name: &str) -> Result<i32, String> {
        let f: Symbol<unsafe extern "C" fn(*mut c_int) -> c_int> =
            unsafe { self.lib.get(name.as_bytes()) }
                .map_err(|e| format!("{DLL}: missing symbol {name}: {e}"))?;
        let mut out: c_int = 0;
        if unsafe { f(&mut out) } == 0 {
            return Err(format!("{name} failed: {}", self.last_error()));
        }
        Ok(out)
    }

    /// `int fn(int idx, char* buf, int len)` — name/serial getters.
    fn string_of(&self, name: &str) -> Result<String, String> {
        let f: Symbol<unsafe extern "C" fn(c_int, *mut c_char, c_int) -> c_int> =
            unsafe { self.lib.get(name.as_bytes()) }
                .map_err(|e| format!("{DLL}: missing symbol {name}: {e}"))?;
        let mut buf = [0u8; 128];
        if unsafe { f(self.idx, buf.as_mut_ptr().cast(), buf.len() as c_int) } == 0 {
            return Err(format!("{name} failed: {}", self.last_error()));
        }
        Ok(cstr(&buf))
    }

    fn last_error(&self) -> String {
        let Ok(f) = (unsafe {
            self.lib
                .get::<unsafe extern "C" fn(*mut c_char, c_int)>(b"sls_GetLastErrorText")
        }) else {
            return "<no error text available>".to_string();
        };
        let mut buf = [0u8; 2048];
        unsafe { f(buf.as_mut_ptr().cast(), buf.len() as c_int) };
        cstr(&buf)
    }
}

fn cstr(buf: &[u8]) -> String {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).trim().to_string()
}
