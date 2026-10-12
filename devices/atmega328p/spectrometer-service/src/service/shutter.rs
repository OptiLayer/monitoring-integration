//! The shutter between the vacuum chamber and OptiMonitor.
//!
//! OptiMonitor keeps fitting a layer until the chamber reports the shutter
//! closed and resumes on the open report. With `--chamber` the service forwards
//! each layer switch to the chamber (`POST /switch_layer`) and relays the
//! chamber's shutter reports (`POST /shutter` on the shutter port) to
//! OptiMonitor. Without one it plays a chamber whose shutter closes at once and
//! reopens after a fixed switch time.

use std::time::Duration;

use serde_json::json;

use crate::service::state::SharedState;

/// How long the dummy chamber keeps the shutter closed between layers.
const DUMMY_SWITCH_TIME: Duration = Duration::from_secs(2);

/// Tell OptiMonitor the shutter physically closed or opened.
pub async fn report_shutter(state: &SharedState, open: bool) {
    let (api_url, chamber_id, spectrometer_id) = {
        let s = state.read().await;
        (
            s.monitoring_api_url.clone(),
            s.vacuum_chamber_id.clone(),
            s.spectrometer_id.clone(),
        )
    };
    let (Some(api_url), Some(chamber_id)) = (api_url, chamber_id) else {
        tracing::info!(
            open,
            "not registered as a vacuum chamber; shutter state not reported"
        );
        return;
    };
    let url = format!("{api_url}/vacuum-chambers/{chamber_id}/shutter");
    let body = json!({ "open": open, "spectrometer_id": spectrometer_id });
    match client().post(&url).json(&body).send().await {
        Ok(r) if r.status().is_success() => tracing::info!(open, "shutter reported to OptiMonitor"),
        Ok(r) => {
            tracing::error!(open, status = %r.status(), "OptiMonitor rejected the shutter report")
        }
        Err(e) => tracing::error!(open, error = %e, "could not deliver the shutter report"),
    }
}

/// Ask the chamber to end the current layer: close the shutter, switch the
/// source and reopen — or, after the last layer, close and stay closed.
pub async fn switch_layer(state: &SharedState, last_layer: bool) {
    let chamber_url = state.read().await.chamber_url.clone();
    match chamber_url {
        Some(chamber_url) => forward_switch(&chamber_url, last_layer).await,
        None => play_dummy_switch(state.clone(), last_layer),
    }
}

/// Start: a real chamber opens its shutter when it is ready and reports it;
/// the dummy opens at once.
pub async fn arm(state: &SharedState) {
    if state.read().await.chamber_url.is_some() {
        return;
    }
    let state = state.clone();
    tokio::spawn(async move { report_shutter(&state, true).await });
}

async fn forward_switch(chamber_url: &str, last_layer: bool) {
    let url = format!("{chamber_url}/switch_layer");
    match client()
        .post(&url)
        .json(&json!({ "last_layer": last_layer }))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => {
            tracing::info!(last_layer, "switch_layer sent to the chamber")
        }
        Ok(r) => tracing::error!(last_layer, status = %r.status(), "chamber rejected switch_layer"),
        Err(e) => tracing::error!(last_layer, error = %e, "could not reach the chamber"),
    }
}

fn play_dummy_switch(state: SharedState, last_layer: bool) {
    tokio::spawn(async move {
        report_shutter(&state, false).await;
        if last_layer {
            return;
        }
        tokio::time::sleep(DUMMY_SWITCH_TIME).await;
        report_shutter(&state, true).await;
    });
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("reqwest client")
}
