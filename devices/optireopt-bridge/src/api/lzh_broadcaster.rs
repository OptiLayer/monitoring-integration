//! Periodic LZH-state broadcaster and WS snapshot helper.
//!
//! [`snapshot`] is also used by the WS init handler so the page-load state
//! matches the live update format.

use std::time::Duration;

use lzh_vm::{SharedSession, State as LzhState};
use serde_json::{Value, json};
use tokio::sync::broadcast;
use tokio::time::sleep;

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

pub async fn snapshot(session: &SharedSession) -> Value {
    let s = session.lock().await;
    let m = s.measurement();
    let pcs = s.last_in_frame();
    let design = s
        .plan()
        .layers
        .get(s.current_layer().saturating_sub(1) as usize);
    json!({
        "state": state_name(s.state()),
        "current_layer": s.current_layer(),
        "heartbeat": s.heartbeat(),
        "layers": s.plan().layers.len() as u16,
        "pcs": {
            "deposit": pcs.deposit,
            "acp": pcs.acp,
            "auto_scaling": pcs.auto_scaling,
        },
        "measurement": {
            "current_thickness": m.current_thickness,
            "current_rate": m.current_rate,
            "mean_rate": m.mean_rate,
            "remaining_time": m.remaining_time,
        },
        "design": design.map(|d| json!({
            "design_thickness": d.design_thickness,
            "design_rate": d.design_rate,
            "n_index": d.n_index,
            "central_wavelength": d.central_wavelength,
        })),
    })
}

pub async fn run(session: SharedSession, tx: broadcast::Sender<Value>) {
    let mut last_key: Option<(String, u16, u16)> = None;
    loop {
        let snap = snapshot(&session).await;
        let key = (
            snap.get("state")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            snap.get("current_layer")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u16,
            snap.get("layers").and_then(|v| v.as_u64()).unwrap_or(0) as u16,
        );
        let changed = last_key.as_ref().map(|k| k != &key).unwrap_or(true);
        // Always re-broadcast every tick so the dashboard sees fresh
        // measurement and heartbeat values; the snapshot is small.
        let mut envelope = snap;
        envelope["type"] = Value::String("lzh_state".into());
        envelope["changed"] = Value::Bool(changed);
        let _ = tx.send(envelope);
        last_key = Some(key);
        sleep(Duration::from_millis(500)).await;
    }
}
