use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;

use crate::api::models::*;
use crate::service::state::{AppState, DeviceState};

/// Wider than any grating on the M266, but catches nonsense (0, negative, NaN)
/// before it reaches the SDK.
const WL_LIMITS_NM: std::ops::RangeInclusive<f64> = 1.0..=1_000_000.0;

/// GET /control_wavelength - Get current control wavelength
pub async fn get_control_wavelength(
    State(state): State<AppState>,
) -> Json<ControlWavelengthResponse> {
    let device = state.device.read().await;
    Json(response(&device, state.mono.is_some()))
}

/// POST /control_wavelength - Move the monochromator to the requested wavelength.
///
/// Without `--mono` the value is stored as a label for the monitoring API only,
/// which is what playback and bench calibration runs want.
pub async fn set_control_wavelength(
    State(state): State<AppState>,
    Json(request): Json<ControlWavelengthRequest>,
) -> (StatusCode, Json<ControlWavelengthResponse>) {
    let nm = request.wavelength;

    if !nm.is_finite() || !WL_LIMITS_NM.contains(&nm) {
        let device = state.device.read().await;
        let mut resp = response(&device, state.mono.is_some());
        resp.error = Some(format!("wavelength {nm} is not a plausible value in nm"));
        return (StatusCode::BAD_REQUEST, Json(resp));
    }

    let Some(mono) = state.mono.clone() else {
        let mut device = state.device.write().await;
        device.control_wavelength = nm;
        tracing::info!("Control wavelength label set to {nm} nm (no monochromator configured)");
        let resp = response(&device, false);
        drop(device);
        broadcast(&state).await;
        return (StatusCode::OK, Json(resp));
    };

    {
        let mut device = state.device.write().await;
        device.mono_moving = true;
    }
    broadcast(&state).await;

    let result = mono.set_wavelength(nm).await;

    let mut device = state.device.write().await;
    device.mono_moving = false;

    match result {
        Ok(actual) => {
            device.control_wavelength = nm;
            device.actual_wavelength = Some(actual);
            device.mono_error = None;
            tracing::info!("Monochromator at {actual:.2} nm (requested {nm} nm)");
        }
        Err(e) => {
            // Leave control_wavelength alone: mislabelling the spectral data we
            // push to monitoring is worse than reporting the failure.
            device.mono_error = Some(e.clone());
            tracing::error!("Monochromator move to {nm} nm failed: {e}");
        }
    }

    let status = if device.mono_error.is_some() {
        StatusCode::BAD_GATEWAY
    } else {
        StatusCode::OK
    };
    let resp = response(&device, true);
    drop(device);
    broadcast(&state).await;

    (status, Json(resp))
}

fn response(device: &DeviceState, hardware: bool) -> ControlWavelengthResponse {
    ControlWavelengthResponse {
        control_wavelength: device.control_wavelength,
        actual_wavelength: device.actual_wavelength,
        hardware,
        error: device.mono_error.clone(),
    }
}

/// Monochromator state as the dashboard consumes it.
pub async fn mono_json(state: &AppState) -> serde_json::Value {
    let device = state.device.read().await;
    serde_json::json!({
        "type": "mono",
        "control_wavelength": device.control_wavelength,
        "actual_wavelength": device.actual_wavelength,
        "moving": device.mono_moving,
        "hardware": state.mono.is_some(),
        "instrument": state.mono.as_ref().map(|m| m.description()),
        "error": device.mono_error,
    })
}

/// Push monochromator state to the dashboard, so a move driven by OptiMonitor
/// shows up there too.
pub async fn broadcast(state: &AppState) {
    let _ = state.broadcast_tx.send(mono_json(state).await);
}

#[cfg(test)]
mod tests {

    use tokio::sync::{broadcast, mpsc};

    use super::*;
    use crate::mono::Monochromator;
    use crate::service::calibration::create_shared_config;
    use crate::service::state::create_shared_state;

    fn test_state(mono: Option<Monochromator>) -> (AppState, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let (tx, _) = broadcast::channel(16);
        let (cmd_tx, _) = mpsc::channel(16);
        let state = AppState {
            device: create_shared_state(),
            config: create_shared_config(dir.path().join("cfg.toml")),
            broadcast_tx: tx,
            device_cmd_tx: cmd_tx,
            mono: mono.map(std::sync::Arc::new),
        };
        (state, dir)
    }

    #[tokio::test]
    async fn test_get_control_wavelength() {
        let (state, _dir) = test_state(None);
        let response = get_control_wavelength(State(state)).await;
        assert_eq!(response.control_wavelength, 550.0);
        assert!(!response.hardware);
    }

    #[tokio::test]
    async fn test_set_control_wavelength_without_hardware() {
        let (state, _dir) = test_state(None);

        let request = ControlWavelengthRequest { wavelength: 600.0 };
        let (status, response) = set_control_wavelength(State(state.clone()), Json(request)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(response.control_wavelength, 600.0);
        assert!(!response.hardware);

        let device = state.device.read().await;
        assert_eq!(device.control_wavelength, 600.0);
    }

    #[tokio::test]
    async fn test_set_control_wavelength_moves_hardware() {
        let mono = Monochromator::connect("sim", 0, None).unwrap();
        let (state, _dir) = test_state(Some(mono));

        let request = ControlWavelengthRequest { wavelength: 750.0 };
        let (status, response) = set_control_wavelength(State(state.clone()), Json(request)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(response.actual_wavelength, Some(750.0));
        assert!(response.hardware);

        let device = state.device.read().await;
        assert_eq!(device.control_wavelength, 750.0);
        assert!(!device.mono_moving);
    }

    #[tokio::test]
    async fn test_failed_move_keeps_previous_setpoint() {
        // Pinned to grating 0, which tops out at 540 nm.
        let mono = Monochromator::connect("sim", 0, Some(0)).unwrap();
        let (state, _dir) = test_state(Some(mono));

        let request = ControlWavelengthRequest { wavelength: 900.0 };
        let (status, response) = set_control_wavelength(State(state.clone()), Json(request)).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert!(response.error.is_some());
        assert_eq!(response.control_wavelength, 550.0);

        let device = state.device.read().await;
        assert_eq!(device.control_wavelength, 550.0);
        assert!(!device.mono_moving);
    }

    #[tokio::test]
    async fn test_rejects_nonsense_wavelength() {
        let (state, _dir) = test_state(None);
        for bad in [0.0, -5.0, f64::NAN] {
            let (status, _) = set_control_wavelength(
                State(state.clone()),
                Json(ControlWavelengthRequest { wavelength: bad }),
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "accepted {bad}");
        }
        assert_eq!(state.device.read().await.control_wavelength, 550.0);
    }
}
