use axum::Json;
use axum::extract::State;
use serde_json::json;

use crate::api::models::{
    DepositionResponse, MaterialResponse, SetMaterialRequest, VacuumChamberStatusResponse,
};
use crate::service::state::AppState;

pub async fn get_material(State(state): State<AppState>) -> Json<MaterialResponse> {
    let device = state.device.read().await;
    Json(MaterialResponse {
        material: device.current_material.clone(),
        fraction: device.current_fraction,
    })
}

pub async fn set_material(
    State(state): State<AppState>,
    Json(request): Json<SetMaterialRequest>,
) -> Json<MaterialResponse> {
    {
        let mut device = state.device.write().await;
        device.current_material = request.material.clone();
        device.current_fraction = request.fraction;
        device.material_switches = device.material_switches.saturating_add(1);
    }

    // OptiMonitor's dt_switch reaching zero is the layer-end decision. Push it
    // straight into the LZH session so PCS sees a TerminateLayer pulse on the
    // wire. force_end_of_layer() is a no-op outside Depositing, so calls before
    // the first deposit (e.g. setting initial material) cost nothing.
    if let Some(lzh) = &state.lzh {
        let mut s = lzh.lock().await;
        let before = s.state();
        s.force_end_of_layer();
        tracing::info!(
            material = %request.material,
            fraction = ?request.fraction,
            lzh_before = ?before,
            lzh_after = ?s.state(),
            "auto-switch from OptiMonitor -> LZH force_end_of_layer",
        );
    } else {
        tracing::info!(
            material = %request.material,
            fraction = ?request.fraction,
            "auto-switch from OptiMonitor (no LZH transport configured)",
        );
    }

    let _ = state.broadcast_tx.send(json!({
        "type": "material_changed",
        "material": request.material,
        "fraction": request.fraction,
    }));

    Json(MaterialResponse {
        material: request.material,
        fraction: request.fraction,
    })
}

pub async fn start_deposition(State(state): State<AppState>) -> Json<DepositionResponse> {
    {
        let mut device = state.device.write().await;
        device.is_depositing = true;
    }
    if let Some(lzh) = &state.lzh {
        let mut s = lzh.lock().await;
        s.mark_initialized();
        tracing::info!(
            "deposition start -> LZH mark_initialized, state={:?}",
            s.state()
        );
    } else {
        tracing::info!("deposition start (no LZH transport configured)");
    }
    Json(DepositionResponse {
        status: "running".to_string(),
    })
}

pub async fn stop_deposition(State(state): State<AppState>) -> Json<DepositionResponse> {
    {
        let mut device = state.device.write().await;
        device.is_depositing = false;
    }
    if let Some(lzh) = &state.lzh {
        let mut s = lzh.lock().await;
        s.mark_process_complete();
        tracing::info!("deposition stop -> LZH mark_process_complete");
    } else {
        tracing::info!("deposition stop (no LZH transport configured)");
    }
    Json(DepositionResponse {
        status: "stopped".to_string(),
    })
}

pub async fn get_status(State(state): State<AppState>) -> Json<VacuumChamberStatusResponse> {
    let device = state.device.read().await;
    Json(VacuumChamberStatusResponse {
        status: if device.is_depositing {
            "running"
        } else {
            "stopped"
        }
        .to_string(),
        is_depositing: device.is_depositing,
        current_material: device.current_material.clone(),
    })
}
