use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use crate::api::models::{
    DepositionResponse, MaterialResponse, SetMaterialRequest, ShutterReport,
    VacuumChamberStatusResponse,
};
use crate::service::shutter;
use crate::service::state::AppState;

/// GET /vacuum_chamber/material - Get current material
pub async fn get_material(State(state): State<AppState>) -> Json<MaterialResponse> {
    let device = state.device.read().await;
    Json(MaterialResponse {
        material: device.current_material.clone(),
        fraction: device.current_fraction,
    })
}

/// POST /vacuum_chamber/material - Set material; while depositing this is
/// OptiMonitor's layer-end decision and goes to the chamber as a layer switch.
pub async fn set_material(
    State(state): State<AppState>,
    Json(request): Json<SetMaterialRequest>,
) -> Json<MaterialResponse> {
    let depositing = {
        let mut device = state.device.write().await;
        device.current_material = request.material.clone();
        device.current_fraction = request.fraction;
        device.is_depositing
    };
    if depositing {
        shutter::switch_layer(&state.device, false).await;
    }
    tracing::info!(material = %request.material, depositing, "Material set");
    Json(MaterialResponse {
        material: request.material,
        fraction: request.fraction,
    })
}

/// POST /vacuum_chamber/start - Arm the run; fitting begins when the shutter opens
pub async fn start_deposition(State(state): State<AppState>) -> Json<DepositionResponse> {
    {
        let mut device = state.device.write().await;
        device.is_depositing = true;
        device.is_running = true;
    }
    shutter::arm(&state.device).await;
    tracing::info!("Deposition started");
    Json(DepositionResponse {
        status: "running".to_string(),
    })
}

/// POST /vacuum_chamber/stop - Last layer: the chamber closes the shutter for good
pub async fn stop_deposition(State(state): State<AppState>) -> Json<DepositionResponse> {
    {
        let mut device = state.device.write().await;
        device.is_depositing = false;
        device.is_running = false;
    }
    shutter::switch_layer(&state.device, true).await;
    tracing::info!("Deposition stopped");
    Json(DepositionResponse {
        status: "stopped".to_string(),
    })
}

/// GET /vacuum_chamber/status - Get chamber status
pub async fn get_status(State(state): State<AppState>) -> Json<VacuumChamberStatusResponse> {
    let device = state.device.read().await;
    Json(VacuumChamberStatusResponse {
        status: if device.is_depositing {
            "running".to_string()
        } else {
            "stopped".to_string()
        },
        is_depositing: device.is_depositing,
    })
}

/// POST /shutter (shutter port) - The chamber reports a physical shutter transition
pub async fn shutter_report(
    State(state): State<AppState>,
    Json(report): Json<ShutterReport>,
) -> Json<Value> {
    tracing::info!(open = report.open, "Chamber reported shutter");
    let device = state.device.clone();
    tokio::spawn(async move { shutter::report_shutter(&device, report.open).await });
    Json(json!({ "status": "ok" }))
}

#[cfg(test)]
mod tests {

    use tokio::sync::{broadcast, mpsc};

    use super::*;
    use crate::service::calibration::create_shared_config;
    use crate::service::state::create_shared_state;

    fn test_state() -> (AppState, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let (tx, _) = broadcast::channel(16);
        let (cmd_tx, _) = mpsc::channel(16);
        let state = AppState {
            device: create_shared_state(),
            config: create_shared_config(dir.path().join("cfg.toml")),
            broadcast_tx: tx,
            device_cmd_tx: cmd_tx,
            mono: None,
        };
        (state, dir)
    }

    fn material(name: &str) -> Json<SetMaterialRequest> {
        Json(SetMaterialRequest {
            material: name.to_string(),
            fraction: Some(100.0),
        })
    }

    #[tokio::test]
    async fn test_get_material() {
        let (state, _dir) = test_state();
        let response = get_material(State(state)).await;
        assert_eq!(response.material, "H");
    }

    #[tokio::test]
    async fn test_set_material_stores_optimonitors_json_body() {
        let (state, _dir) = test_state();
        let response = set_material(State(state.clone()), material("L")).await;
        assert_eq!(response.material, "L");
        assert_eq!(response.fraction, Some(100.0));

        let device = state.device.read().await;
        assert_eq!(device.current_material, "L");
        assert_eq!(device.current_fraction, Some(100.0));
    }

    #[tokio::test]
    async fn test_start_stop_deposition() {
        let (state, _dir) = test_state();

        let response = start_deposition(State(state.clone())).await;
        assert_eq!(response.status, "running");
        {
            let s = state.device.read().await;
            assert!(s.is_depositing);
            assert!(s.is_running);
        }

        let response = stop_deposition(State(state.clone())).await;
        assert_eq!(response.status, "stopped");
        {
            let s = state.device.read().await;
            assert!(!s.is_depositing);
            assert!(!s.is_running);
        }
    }

    #[tokio::test]
    async fn test_shutter_report_is_acknowledged_without_a_registration() {
        let (state, _dir) = test_state();
        let response = shutter_report(State(state), Json(ShutterReport { open: false })).await;
        assert_eq!(response.0["status"], "ok");
    }

    #[tokio::test]
    async fn test_get_status() {
        let (state, _dir) = test_state();

        let response = get_status(State(state.clone())).await;
        assert_eq!(response.status, "stopped");
        assert!(!response.is_depositing);

        let _ = start_deposition(State(state.clone())).await;

        let response = get_status(State(state)).await;
        assert_eq!(response.status, "running");
        assert!(response.is_depositing);
    }
}
