use serde::{Deserialize, Serialize};

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
}

#[derive(Debug, Serialize)]
pub struct RegisterResponse {
    pub status: String,
    pub spectrometer_id: Option<String>,
    pub monitoring_api_url: String,
}

// ============= Vacuum Chamber (no-op) =============
//
// The bridge has no real vacuum chamber — the operator's external software owns
// the physical hardware. We expose these endpoints purely so OptiMonitor's
// AutomaticStrategy can drive layer-by-layer progression: when OptiReOpt's
// dt_switch reaches zero, OptiMonitor PUTs /vacuum-chambers/{id}/material which
// in turn POSTs here. We accept the call, log it, and let OptiMonitor advance
// its own layer counter on the strength of the 200 OK.

#[derive(Debug, Deserialize)]
pub struct SetMaterialRequest {
    pub material: String,
    #[serde(default)]
    pub fraction: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct MaterialResponse {
    pub material: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fraction: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct VacuumChamberStatusResponse {
    pub status: String,
    pub is_depositing: bool,
    pub current_material: String,
}

#[derive(Debug, Serialize)]
pub struct DepositionResponse {
    pub status: String,
}

// ============= LZH (real vacuum-machine integration) =============

#[derive(Debug, Deserialize, Serialize)]
pub struct LzhLayerSpec {
    pub design_thickness: f64,
    pub design_rate: f64,
    #[serde(default)]
    pub n_index: u16,
    #[serde(default)]
    pub central_wavelength: f64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LzhRecipeRequest {
    pub layers: Vec<LzhLayerSpec>,
    #[serde(default = "default_test_glass")]
    pub current_test_glass: u16,
}

fn default_test_glass() -> u16 {
    1
}

#[derive(Debug, Deserialize)]
pub struct LzhMeasurementRequest {
    pub current_thickness: f64,
    #[serde(default)]
    pub current_rate: f64,
    #[serde(default)]
    pub mean_rate: f64,
    #[serde(default)]
    pub remaining_time: f64,
}

#[derive(Debug, Serialize)]
pub struct LzhStateResponse {
    pub state: &'static str,
    pub current_layer: u16,
    pub heartbeat: u16,
    pub layers: u16,
    pub current_thickness: f64,
    pub current_rate: f64,
    pub mean_rate: f64,
    pub remaining_time: f64,
}

#[derive(Debug, Serialize)]
pub struct LzhAckResponse {
    pub ok: bool,
    pub state: &'static str,
    pub current_layer: u16,
}
