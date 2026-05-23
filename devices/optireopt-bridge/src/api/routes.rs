use axum::Router;
use axum::routing::{get, post};

use super::handlers::{device, ingest, lzh, vacuum_chamber};
use super::{web_ui, websocket};
use crate::service::state::AppState;

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(web_ui::index))
        .route("/ws", get(websocket::ws_handler))
        .route("/device/info", get(device::get_device_info))
        .route("/register", post(device::register))
        .route("/latest", get(device::get_latest))
        .route("/ingest", post(ingest::ingest))
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
        .route("/vacuum_chamber/lzh/state", get(lzh::get_state))
        .route("/vacuum_chamber/lzh/recipe", post(lzh::set_recipe))
        .route("/vacuum_chamber/lzh/start", post(lzh::start))
        .route(
            "/vacuum_chamber/lzh/calibration_complete",
            post(lzh::calibration_complete),
        )
        .route("/vacuum_chamber/lzh/measurement", post(lzh::measurement))
        .route("/vacuum_chamber/lzh/end_layer", post(lzh::end_layer))
        .with_state(state)
}
