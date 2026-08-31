use std::time::Duration;

use reqwest::Client;
use tokio::sync::broadcast;
use tokio::time::MissedTickBehavior;

use crate::service::state::SharedState;

const POLL_INTERVAL_MS: u64 = 500;
const HTTP_TIMEOUT_SECS: u64 = 3;

/// Long-running task that polls a running `vm-service` for status and
/// forwards every response into:
///   1. the spectrometer service's broadcast channel (so the calibration HTML
///      WebSocket clients see a `{type: "vm_state", ...}` message)
///   2. `DeviceState.latest_vm_state` (so the data loop can gate on the VM
///      phase and the `/vacuum_chamber/status` endpoint can proxy it)
///
/// We poll instead of subscribing via WebSocket to avoid pulling in a new
/// dependency — 500 ms is well within human-facing latency requirements.
pub async fn run(
    vm_service_url: String,
    state: SharedState,
    broadcast_tx: broadcast::Sender<serde_json::Value>,
) {
    let client = match Client::builder()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("vm-bridge: failed to build HTTP client: {e}");
            return;
        }
    };

    let status_url = format!(
        "{}/vacuum_chamber/status",
        vm_service_url.trim_end_matches('/')
    );
    tracing::info!("vm-bridge: polling {}", status_url);

    let mut ticker = tokio::time::interval(Duration::from_millis(POLL_INTERVAL_MS));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut was_reachable = false;

    loop {
        ticker.tick().await;

        match client.get(&status_url).send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<serde_json::Value>().await {
                    Ok(mut json) => {
                        if let Some(obj) = json.as_object_mut() {
                            obj.insert("type".to_string(), serde_json::json!("vm_state"));
                        }
                        {
                            let mut s = state.write().await;
                            s.latest_vm_state = Some(json.clone());
                        }
                        let _ = broadcast_tx.send(json);
                        was_reachable = true;
                    }
                    Err(e) => {
                        tracing::warn!("vm-bridge: failed to parse status JSON: {e}");
                    }
                }
            }
            Ok(resp) => {
                tracing::warn!("vm-bridge: unexpected HTTP status {}", resp.status());
                if was_reachable {
                    emit_unreachable(&state, &broadcast_tx).await;
                    was_reachable = false;
                }
            }
            Err(e) => {
                tracing::debug!("vm-bridge: request failed: {e}");
                if was_reachable {
                    emit_unreachable(&state, &broadcast_tx).await;
                    was_reachable = false;
                }
            }
        }
    }
}

async fn emit_unreachable(
    state: &SharedState,
    broadcast_tx: &broadcast::Sender<serde_json::Value>,
) {
    let msg = serde_json::json!({
        "type": "vm_state",
        "connection": "unreachable",
    });
    {
        let mut s = state.write().await;
        s.latest_vm_state = Some(msg.clone());
    }
    let _ = broadcast_tx.send(msg);
}
