use std::sync::Arc;

use chrono::{DateTime, Utc};
use lzh_vm::SharedSession;
use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, broadcast};

use crate::monitoring::client::MonitoringClient;

#[derive(Debug, Clone, Default)]
pub struct DeviceState {
    pub monitoring_api_url: Option<String>,
    pub spectrometer_id: Option<String>,
    pub latest_frame: Option<ScanFrame>,
    pub last_frame_at: Option<DateTime<Utc>>,
    pub scans_received: u64,
    // Mirrored from OptiMonitor's auto-switch calls so /vacuum_chamber/status
    // returns something coherent. We don't drive any real hardware with this.
    pub current_material: String,
    pub current_fraction: Option<f64>,
    pub is_depositing: bool,
    pub material_switches: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanFrame {
    pub wavelength: Vec<f64>,
    pub values: Vec<f64>,
    pub rt_data: Option<String>,
    pub timestamp: String,
}

pub type SharedDevice = Arc<RwLock<DeviceState>>;

#[derive(Clone)]
pub struct AppState {
    pub device: SharedDevice,
    pub broadcast_tx: broadcast::Sender<serde_json::Value>,
    pub monitoring: Arc<MonitoringClient>,
    /// `None` when `--pcs-addr` was not provided; LZH endpoints respond 503.
    pub lzh: Option<SharedSession>,
}
