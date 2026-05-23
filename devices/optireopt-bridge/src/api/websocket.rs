use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use serde_json::json;
use tokio::sync::broadcast;

use crate::service::state::AppState;

pub async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    // Send init snapshot so the dashboard can render immediately.
    let init = {
        let device = state.device.read().await;
        let lzh = match &state.lzh {
            Some(s) => {
                let session = s.lock().await;
                let st = match session.state() {
                    lzh_vm::State::Initializing => "initializing",
                    lzh_vm::State::Ready => "ready",
                    lzh_vm::State::Calibrating => "calibrating",
                    lzh_vm::State::Depositing => "depositing",
                    lzh_vm::State::EndOfLayerPulse => "end_of_layer_pulse",
                    lzh_vm::State::Complete => "complete",
                };
                Some(json!({
                    "state": st,
                    "current_layer": session.current_layer(),
                    "heartbeat": session.heartbeat(),
                    "layers": session.plan().layers.len() as u16,
                }))
            }
            None => None,
        };
        json!({
            "type": "init",
            "scans_received": device.scans_received,
            "latest_frame": device.latest_frame,
            "last_frame_at": device.last_frame_at,
            "lzh": lzh,
        })
    };
    if socket
        .send(Message::Text(init.to_string().into()))
        .await
        .is_err()
    {
        return;
    }

    let mut rx = state.broadcast_tx.subscribe();
    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Ok(data) => {
                        if socket.send(Message::Text(data.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("WebSocket client lagged by {} messages", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}
