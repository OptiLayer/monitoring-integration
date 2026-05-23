//! Periodic LZH-state broadcaster.
//!
//! Polls the [`SharedSession`] every 500 ms; when the snapshot changes
//! meaningfully (state name, current layer, plan size) it pushes a JSON event
//! over the shared broadcast bus. The dashboard subscribes via /ws and
//! re-renders LZH controls accordingly.

use std::time::Duration;

use lzh_vm::{SharedSession, State as LzhState};
use serde_json::json;
use tokio::sync::broadcast;
use tokio::time::sleep;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Snapshot {
    state: &'static str,
    current_layer: u16,
    heartbeat: u16,
    layers: u16,
}

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

async fn snapshot(session: &SharedSession) -> Snapshot {
    let s = session.lock().await;
    Snapshot {
        state: state_name(s.state()),
        current_layer: s.current_layer(),
        heartbeat: s.heartbeat(),
        layers: s.plan().layers.len() as u16,
    }
}

pub async fn run(session: SharedSession, tx: broadcast::Sender<serde_json::Value>) {
    let mut last: Option<Snapshot> = None;
    loop {
        let snap = snapshot(&session).await;
        let interesting = last
            .map(|p| {
                p.state != snap.state
                    || p.current_layer != snap.current_layer
                    || p.layers != snap.layers
            })
            .unwrap_or(true);
        if interesting {
            let _ = tx.send(json!({
                "type": "lzh_state",
                "state": snap.state,
                "current_layer": snap.current_layer,
                "heartbeat": snap.heartbeat,
                "layers": snap.layers,
            }));
        }
        last = Some(snap);
        sleep(Duration::from_millis(500)).await;
    }
}
