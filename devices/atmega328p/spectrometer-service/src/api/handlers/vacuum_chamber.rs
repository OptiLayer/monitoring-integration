use axum::Json;
use axum::extract::State;

use crate::api::models::*;
use crate::service::state::AppState;

/// GET /vacuum_chamber/material - Get current material
pub async fn get_material(State(state): State<AppState>) -> Json<MaterialResponse> {
    let device = state.device.read().await;

    Json(MaterialResponse {
        material: device.current_material.clone(),
    })
}

/// POST /vacuum_chamber/material - Set material
pub async fn set_material(State(state): State<AppState>, body: String) -> Json<MaterialResponse> {
    let mut device = state.device.write().await;

    let material = body.trim().trim_matches('"').to_string();
    device.current_material = material.clone();

    tracing::info!("Material set to {}", material);

    Json(MaterialResponse { material })
}

/// POST /vacuum_chamber/start - Start deposition
pub async fn start_deposition(State(state): State<AppState>) -> Json<DepositionResponse> {
    let mut device = state.device.write().await;

    device.is_depositing = true;
    device.is_running = true;

    tracing::info!("Deposition started");

    Json(DepositionResponse {
        status: "running".to_string(),
    })
}

/// POST /vacuum_chamber/stop - Stop deposition
pub async fn stop_deposition(State(state): State<AppState>) -> Json<DepositionResponse> {
    let mut device = state.device.write().await;

    device.is_depositing = false;
    device.is_running = false;

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

    #[tokio::test]
    async fn test_get_material() {
        let (state, _dir) = test_state();
        let response = get_material(State(state)).await;
        assert_eq!(response.material, "H");
    }

    #[tokio::test]
    async fn test_set_material() {
        let (state, _dir) = test_state();
        let response = set_material(State(state.clone()), "L".to_string()).await;
        assert_eq!(response.material, "L");

        let device = state.device.read().await;
        assert_eq!(device.current_material, "L");
    }

    #[tokio::test]
    async fn test_set_material_json_string() {
        let (state, _dir) = test_state();
        let response = set_material(State(state.clone()), "\"H\"".to_string()).await;
        assert_eq!(response.material, "H");
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
