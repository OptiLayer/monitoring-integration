use serde::{Deserialize, Serialize};

// ============= Device Endpoints =============

#[derive(Debug, Serialize)]
pub struct DeviceInfoResponse {
    #[serde(rename = "type")]
    pub device_type: String,
    pub name: String,
    pub capabilities: DeviceCapabilities,
}

#[derive(Debug, Serialize)]
pub struct DeviceCapabilities {
    pub has_spectrometer: bool,
    pub has_vacuum_chamber: bool,
    pub spectrometer_type: String,
    pub is_monochromatic: bool,
}

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub monitoring_api_url: String,
    pub spectrometer_id: Option<String>,
    pub vacuum_chamber_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RegisterResponse {
    pub status: String,
    pub spectrometer_id: Option<String>,
    pub vacuum_chamber_id: Option<String>,
    pub monitoring_api_url: String,
}

// ============= Spectrometer Endpoints =============

#[derive(Debug, Deserialize)]
pub struct ControlWavelengthRequest {
    pub wavelength: f64,
}

#[derive(Debug, Serialize)]
pub struct ControlWavelengthResponse {
    /// Requested wavelength (nm)
    pub control_wavelength: f64,
    /// What the monochromator reports it is at, when one is connected
    pub actual_wavelength: Option<f64>,
    /// False when no monochromator is configured — the value is a label only
    pub hardware: bool,
    /// Why the move failed, if it did
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ============= Vacuum Chamber Endpoints =============

#[derive(Debug, Serialize)]
pub struct MaterialResponse {
    pub material: String,
}

#[derive(Debug, Serialize)]
pub struct VacuumChamberStatusResponse {
    pub status: String,
    pub is_depositing: bool,
}

#[derive(Debug, Serialize)]
pub struct DepositionResponse {
    pub status: String,
}

// ============= Error Response =============

#[allow(dead_code)]
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}
