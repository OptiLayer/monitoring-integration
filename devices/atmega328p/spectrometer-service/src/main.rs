use std::net::SocketAddr;

use clap::Parser;
use tokio::sync::{broadcast, mpsc};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

mod api;
mod config;
mod data_source;
mod error;
mod monitoring;
mod mono;
mod processing;
mod protocol;
mod service;

use config::Cli;
use data_source::serial::SerialDataSource;
use mono::Monochromator;
use service::calibration::create_shared_config;
use service::data_loop::DataProcessingLoop;
use service::state::{AppState, create_shared_state};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_ansi(false))
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "spectrometer_service=info".into()),
        )
        .init();

    let cli = Cli::parse();

    // Handle --list-ports
    if cli.list_ports {
        list_serial_ports();
        return Ok(());
    }

    // Load saved device config (before creating data source)
    let device_config = create_shared_config(cli.calibration_config.clone());
    let saved_settings = {
        let cfg = device_config.read().await;
        cfg.config.device_settings.clone()
    };

    // Require a mode if not listing ports
    let Some(data_source_config) = cli.to_data_source_config(&saved_settings) else {
        eprintln!("Error: Please specify a mode (serial or playback)");
        eprintln!("Use --help for usage information");
        std::process::exit(1);
    };

    tracing::info!(
        "Starting spectrometer service on {}:{}",
        cli.host,
        cli.listen
    );

    // Create shared state
    let device_state = create_shared_state();

    // Create broadcast channel for WebSocket
    let (broadcast_tx, _) = broadcast::channel(256);

    // Create device command channel (UI -> data source)
    let (device_cmd_tx, mut device_cmd_rx) = mpsc::channel::<String>(16);

    // Connect the monochromator, if one was configured
    let mono = match cli.mono.as_deref() {
        None => None,
        Some(target) => Some(std::sync::Arc::new(Monochromator::connect(
            target,
            cli.mono_index,
            cli.mono_grating,
        )?)),
    };

    // Seed the dashboard with where the instrument actually is right now
    if let Some(m) = &mono {
        match m.wavelength().await {
            Ok(nm) => {
                let mut s = device_state.write().await;
                s.control_wavelength = nm;
                s.actual_wavelength = Some(nm);
            }
            Err(e) => tracing::warn!("mono: could not read current wavelength: {e}"),
        }
    }

    // Composite app state
    let app_state = AppState {
        device: device_state.clone(),
        config: device_config.clone(),
        broadcast_tx: broadcast_tx.clone(),
        device_cmd_tx,
        mono,
    };

    // Create data source
    let mut data_source = data_source_config.create_source();

    // Create outlier excluder
    let outlier_method = cli.to_outlier_method();
    let outlier_excluder = outlier_method.create();

    tracing::info!("Using {} outlier exclusion", outlier_excluder.name());

    // Set up log channel (serial lines -> WebSocket broadcast)
    let (log_line_tx, mut log_line_rx) = mpsc::channel::<String>(256);
    data_source.set_log_channel(log_line_tx);

    let log_broadcast_tx = broadcast_tx.clone();
    let log_handle = tokio::spawn(async move {
        while let Some(line) = log_line_rx.recv().await {
            let _ = log_broadcast_tx.send(serde_json::json!({
                "type": "log",
                "line": line,
            }));
        }
    });

    // Start data source and get cycle receiver
    let cycle_rx = data_source.start().await?;

    // Spawn command forwarding task (forwards UI commands to data source)
    let cmd_handle = tokio::spawn(async move {
        while let Some(cmd) = device_cmd_rx.recv().await {
            if let Err(e) = data_source.send_command(&cmd).await {
                tracing::warn!("Device command '{cmd}' failed: {e}");
            }
        }
        // When cmd channel closes, stop the data source
        let _ = data_source.stop().await;
    });

    // Create and spawn data processing loop
    let processing_loop =
        DataProcessingLoop::new(device_state, device_config, broadcast_tx, outlier_excluder);

    let processing_handle = tokio::spawn(async move {
        if let Err(e) = processing_loop.run(cycle_rx).await {
            tracing::error!("Data processing loop error: {}", e);
        }
    });

    // Create and run HTTP server
    let router = api::create_router(app_state);
    let addr: SocketAddr = format!("{}:{}", cli.host, cli.listen).parse()?;

    tracing::info!("HTTP server listening on {}", addr);
    tracing::info!("Open http://localhost:{} for calibration UI", cli.listen);

    let listener = tokio::net::TcpListener::bind(addr).await?;

    // Run server with graceful shutdown
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    // Cleanup
    tracing::info!("Shutting down...");
    processing_handle.abort();
    cmd_handle.abort();
    log_handle.abort();

    Ok(())
}

/// List available serial ports
fn list_serial_ports() {
    match SerialDataSource::list_available_ports() {
        Ok(ports) => {
            if ports.is_empty() {
                println!("No serial ports found");
            } else {
                println!("Available serial ports:");
                for port in ports {
                    let port_type = match port.port_type {
                        serialport::SerialPortType::UsbPort(info) => {
                            format!(
                                "USB - {}",
                                info.product.unwrap_or_else(|| "Unknown".to_string())
                            )
                        }
                        serialport::SerialPortType::BluetoothPort => "Bluetooth".to_string(),
                        serialport::SerialPortType::PciPort => "PCI".to_string(),
                        serialport::SerialPortType::Unknown => "Unknown".to_string(),
                    };
                    println!("  {} - {}", port.port_name, port_type);
                }
            }
        }
        Err(e) => {
            eprintln!("Error listing serial ports: {}", e);
        }
    }
}

/// Wait for shutdown signal (Ctrl+C)
async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to install Ctrl+C handler");
    tracing::info!("Received shutdown signal");
}
