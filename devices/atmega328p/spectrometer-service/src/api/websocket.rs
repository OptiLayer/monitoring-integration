use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use tokio::sync::broadcast;

use crate::api::handlers::spectrometer;
use crate::service::state::AppState;

pub async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    // Send init message with current device settings
    let init_msg = {
        let cfg = state.config.read().await;
        let s = &cfg.config.device_settings;
        serde_json::json!({
            "type": "init",
            "device_settings": {
                "gain": s.gain,
                "fadc": s.fadc,
                "count": s.count,
            },
            "series_mapping": {
                "dark": s.series_mapping.dark,
                "full": s.series_mapping.full,
                "sample": s.series_mapping.sample,
            }
        })
    };

    if socket
        .send(Message::Text(init_msg.to_string().into()))
        .await
        .is_err()
    {
        return;
    }

    // Current monochromator state, so a fresh tab is not blank until the next move
    let mono_msg = spectrometer::mono_json(&state).await;
    if socket
        .send(Message::Text(mono_msg.to_string().into()))
        .await
        .is_err()
    {
        return;
    }

    // Subscribe to broadcast channel
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
                    _ => {} // Ignore other client messages
                }
            }
        }
    }
}
