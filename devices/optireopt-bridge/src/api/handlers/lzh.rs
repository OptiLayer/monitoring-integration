//! LZH/Optotech vacuum-machine endpoints. Drive a [`lzh_vm::LzhSession`] that
//! talks to the customer's PCS over two TCP connections (started in `main.rs`).
//!
//! All endpoints return 503 when LZH is disabled (no `--pcs-addr` configured).

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use lzh_vm::{CoatingPlan, LayerSpec, Measurement, SharedSession, State as LzhState};
use serde_json::Value;

use crate::api::lzh_broadcaster;
use crate::api::models::{LzhAckResponse, LzhMeasurementRequest, LzhRecipeRequest};
use crate::service::state::AppState;

fn state_name(s: LzhState) -> &'static str {
    match s {
        LzhState::Initializing => "initializing",
        LzhState::Ready => "ready",
        LzhState::Calibrating => "calibrating",
        LzhState::Depositing => "depositing",
        LzhState::EndOfLayerPulse => "end_of_layer_pulse",
        LzhState::Complete => "complete",
    }
}

fn lzh_or_503(state: &AppState) -> Result<SharedSession, StatusCode> {
    state.lzh.clone().ok_or(StatusCode::SERVICE_UNAVAILABLE)
}

pub async fn get_state(State(state): State<AppState>) -> Result<Json<Value>, StatusCode> {
    let lzh = lzh_or_503(&state)?;
    Ok(Json(lzh_broadcaster::snapshot(&lzh).await))
}

pub async fn set_recipe(
    State(state): State<AppState>,
    Json(req): Json<LzhRecipeRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    let lzh = lzh_or_503(&state)?;
    let n_layers = req.layers.len();
    let plan = CoatingPlan {
        layers: req
            .layers
            .into_iter()
            .map(|l| LayerSpec {
                design_thickness: l.design_thickness,
                design_rate: l.design_rate,
                n_index: l.n_index,
                central_wavelength: l.central_wavelength,
            })
            .collect(),
        current_test_glass: req.current_test_glass,
    };
    let mut s = lzh.lock().await;
    if !s.set_plan(plan) {
        tracing::warn!(state = ?s.state(), "POST /vacuum_chamber/lzh/recipe rejected: not in Initializing/Ready");
        return Err(StatusCode::CONFLICT);
    }
    tracing::info!(
        layers = n_layers,
        "POST /vacuum_chamber/lzh/recipe accepted"
    );
    Ok(Json(ack(&s)))
}

pub async fn start(State(state): State<AppState>) -> Result<Json<LzhAckResponse>, StatusCode> {
    let lzh = lzh_or_503(&state)?;
    let mut s = lzh.lock().await;
    s.mark_initialized();
    tracing::info!(state = ?s.state(), "POST /vacuum_chamber/lzh/start");
    Ok(Json(ack(&s)))
}

pub async fn calibration_complete(
    State(state): State<AppState>,
) -> Result<Json<LzhAckResponse>, StatusCode> {
    let lzh = lzh_or_503(&state)?;
    let mut s = lzh.lock().await;
    s.mark_calibration_complete();
    tracing::info!(state = ?s.state(), "POST /vacuum_chamber/lzh/calibration_complete");
    Ok(Json(ack(&s)))
}

pub async fn measurement(
    State(state): State<AppState>,
    Json(req): Json<LzhMeasurementRequest>,
) -> Result<Json<LzhAckResponse>, StatusCode> {
    let lzh = lzh_or_503(&state)?;
    let mut s = lzh.lock().await;
    s.update_measurement(Measurement {
        current_thickness: req.current_thickness,
        current_rate: req.current_rate,
        mean_rate: req.mean_rate,
        remaining_time: req.remaining_time,
    });
    tracing::debug!(
        current_thickness = req.current_thickness,
        current_rate = req.current_rate,
        state = ?s.state(),
        "POST /vacuum_chamber/lzh/measurement",
    );
    Ok(Json(ack(&s)))
}

pub async fn end_layer(State(state): State<AppState>) -> Result<Json<LzhAckResponse>, StatusCode> {
    let lzh = lzh_or_503(&state)?;
    let mut s = lzh.lock().await;
    s.force_end_of_layer();
    tracing::info!(state = ?s.state(), "POST /vacuum_chamber/lzh/end_layer");
    Ok(Json(ack(&s)))
}

fn ack(s: &lzh_vm::LzhSession) -> LzhAckResponse {
    LzhAckResponse {
        ok: true,
        state: state_name(s.state()),
        current_layer: s.current_layer(),
    }
}
