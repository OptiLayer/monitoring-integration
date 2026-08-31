use axum::Router;
use axum::routing::{get, post};

use super::handlers::{calibration, device, spectrometer, vacuum_chamber};
use super::{web_ui, websocket};
use crate::service::state::AppState;

/// Create the API router with all endpoints
pub fn create_router(state: AppState) -> Router {
    Router::new()
        // Web UI
        .route("/", get(web_ui::index))
        // WebSocket
        .route("/ws", get(websocket::ws_handler))
        // Device settings API
        .route(
            "/api/settings",
            get(calibration::get_settings).post(calibration::update_settings),
        )
        // Device info and registration
        .route("/device/info", get(device::get_device_info))
        .route("/register", post(device::register))
        // Spectrometer control
        .route(
            "/control_wavelength",
            get(spectrometer::get_control_wavelength).post(spectrometer::set_control_wavelength),
        )
        // Vacuum chamber control
        .route(
            "/vacuum_chamber/material",
            get(vacuum_chamber::get_material).post(vacuum_chamber::set_material),
        )
        .route(
            "/vacuum_chamber/start",
            post(vacuum_chamber::start_deposition),
        )
        .route(
            "/vacuum_chamber/stop",
            post(vacuum_chamber::stop_deposition),
        )
        .route("/vacuum_chamber/status", get(vacuum_chamber::get_status))
        .with_state(state)
}

#[cfg(test)]
mod tests {

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tokio::sync::{broadcast, mpsc};
    use tower::util::ServiceExt;

    use super::*;
    use crate::service::calibration::create_shared_config;
    use crate::service::state::create_shared_state;

    fn test_app_state() -> (AppState, tempfile::TempDir) {
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
    async fn test_device_info_route() {
        let app = create_router(test_app_state().0);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/device/info")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_settings_get() {
        let app = create_router(test_app_state().0);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/settings")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_web_ui_route() {
        let app = create_router(test_app_state().0);
        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
