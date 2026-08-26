use std::sync::Arc;

use tokio::sync::{RwLock, broadcast, mpsc};

use crate::protocol::ProcessedMeasurement;
use crate::service::calibration::SharedConfig;

/// Application state for the spectrometer service
#[derive(Debug, Clone)]
pub struct DeviceState {
    pub monitoring_api_url: Option<String>,
    pub spectrometer_id: Option<String>,
    pub vacuum_chamber_id: Option<String>,
    pub control_wavelength: f64,
    pub is_running: bool,
    pub current_material: String,
    pub is_depositing: bool,
    pub latest_reading: Option<ProcessedMeasurement>,
}

impl Default for DeviceState {
    fn default() -> Self {
        Self {
            monitoring_api_url: None,
            spectrometer_id: None,
            vacuum_chamber_id: None,
            control_wavelength: 550.0,
            is_running: false,
            current_material: "H".to_string(),
            is_depositing: false,
            latest_reading: None,
        }
    }
}

impl DeviceState {
    #[allow(dead_code)]
    pub fn is_registered(&self) -> bool {
        self.monitoring_api_url.is_some() && self.spectrometer_id.is_some()
    }

    /// Stream to the monitoring API whenever registered — shutter state is
    /// irrelevant, the operator needs the live signal before pressing Start.
    pub fn should_process_data(&self) -> bool {
        self.is_registered()
    }
}

pub type SharedState = Arc<RwLock<DeviceState>>;

pub fn create_shared_state() -> SharedState {
    Arc::new(RwLock::new(DeviceState::default()))
}

/// Composite application state for axum handlers
#[derive(Clone)]
pub struct AppState {
    pub device: SharedState,
    pub config: SharedConfig,
    pub broadcast_tx: broadcast::Sender<serde_json::Value>,
    /// Channel for sending commands to the device (GAIN=, FADC=, COUNT=)
    pub device_cmd_tx: mpsc::Sender<String>,
}

impl AppState {
    /// Send a device command (e.g., "GAIN=4")
    pub async fn send_device_command(&self, cmd: &str) -> Result<(), String> {
        self.device_cmd_tx
            .send(cmd.to_string())
            .await
            .map_err(|_| "Device command channel closed".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_state_default() {
        let state = DeviceState::default();
        assert!(state.monitoring_api_url.is_none());
        assert_eq!(state.control_wavelength, 550.0);
        assert!(!state.is_running);
        assert_eq!(state.current_material, "H");
    }

    #[test]
    fn test_is_registered() {
        let mut state = DeviceState::default();
        assert!(!state.is_registered());
        state.monitoring_api_url = Some("http://localhost:8200".to_string());
        state.spectrometer_id = Some("test-id".to_string());
        assert!(state.is_registered());
    }

    #[test]
    fn test_should_process_data_when_registered_regardless_of_shutter() {
        let mut state = DeviceState::default();
        assert!(!state.should_process_data());
        state.monitoring_api_url = Some("http://localhost:8200".to_string());
        state.spectrometer_id = Some("test-id".to_string());
        assert!(state.should_process_data());
        state.is_running = true;
        assert!(state.should_process_data());
    }
}
