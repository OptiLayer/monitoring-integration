//! Tokio transport for the LZH protocol.
//!
//! Two TCP connections per PCS (per the customer's protocol):
//! * we listen on [`TransportConfig::listen_addr`] (default 5054) and read
//!   242-byte [`InFrame`]s sent by PCS,
//! * we connect to [`TransportConfig::pcs_addr`] (default port 9052) and emit
//!   242-byte [`OutFrame`]s on a fixed interval.
//!
//! Both halves reconnect on failure with simple constant backoff. The shared
//! [`LzhSession`] is held under a `Mutex`; lock holds are microseconds long so
//! contention is a non-issue at the ~5 Hz emit cadence.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::time::{MissedTickBehavior, interval, sleep};

use crate::frame::{FRAME_LEN, InFrame, OutFrame};
use crate::state::{CoatingPlan, LzhSession};

#[derive(Debug, Clone)]
pub struct TransportConfig {
    /// PCS server address. Our service connects here as a client and writes
    /// `OutFrame`s. Default port 9052.
    pub pcs_addr: SocketAddr,
    /// Local address where we accept the PCS client. Default port 5054.
    pub listen_addr: SocketAddr,
    /// How often to emit an `OutFrame`. Observed cadence on the wire is
    /// ~215 ms; pick something close.
    pub emit_interval: Duration,
    /// Wait this long before reconnecting after a TCP failure.
    pub reconnect_backoff: Duration,
}

impl TransportConfig {
    pub fn with_default_ports(pcs_host: std::net::IpAddr, listen_host: std::net::IpAddr) -> Self {
        Self {
            pcs_addr: SocketAddr::new(pcs_host, 9052),
            listen_addr: SocketAddr::new(listen_host, 5054),
            emit_interval: Duration::from_millis(215),
            reconnect_backoff: Duration::from_secs(3),
        }
    }
}

pub type SharedSession = Arc<Mutex<LzhSession>>;

/// Run the LZH transport forever. Spawns reader and emitter tasks and never
/// returns. Use `tokio::spawn(transport.run())` from the host service.
pub struct LzhTransport {
    session: SharedSession,
    config: TransportConfig,
}

impl LzhTransport {
    pub fn new(plan: CoatingPlan, config: TransportConfig) -> Self {
        Self {
            session: Arc::new(Mutex::new(LzhSession::new(plan))),
            config,
        }
    }

    pub fn session(&self) -> SharedSession {
        self.session.clone()
    }

    pub async fn run(self) {
        let session = self.session.clone();
        let cfg = self.config.clone();
        let reader = tokio::spawn(reader_loop(session.clone(), cfg.clone()));
        let emitter = tokio::spawn(emitter_loop(session, cfg));
        let _ = tokio::join!(reader, emitter);
    }
}

async fn reader_loop(session: SharedSession, cfg: TransportConfig) {
    loop {
        match run_reader_once(&session, &cfg).await {
            Ok(()) => tracing::info!("PCS reader connection closed cleanly"),
            Err(e) => tracing::warn!("PCS reader error: {e}"),
        }
        sleep(cfg.reconnect_backoff).await;
    }
}

async fn run_reader_once(session: &SharedSession, cfg: &TransportConfig) -> std::io::Result<()> {
    let listener = TcpListener::bind(cfg.listen_addr).await?;
    tracing::info!("LZH reader listening on {}", cfg.listen_addr);
    let (mut stream, peer) = listener.accept().await?;
    tracing::info!("PCS connected from {peer}");

    let mut buf = [0u8; FRAME_LEN];
    loop {
        stream.read_exact(&mut buf).await?;
        let frame = match InFrame::decode(&buf) {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("dropping malformed PCS frame: {e}");
                continue;
            }
        };
        session.lock().await.on_in_frame(frame);
    }
}

async fn emitter_loop(session: SharedSession, cfg: TransportConfig) {
    loop {
        match run_emitter_once(&session, &cfg).await {
            Ok(()) => tracing::info!("PCS emitter connection closed"),
            Err(e) => tracing::warn!("PCS emitter error: {e}"),
        }
        sleep(cfg.reconnect_backoff).await;
    }
}

async fn run_emitter_once(session: &SharedSession, cfg: &TransportConfig) -> std::io::Result<()> {
    let mut stream = TcpStream::connect(cfg.pcs_addr).await?;
    tracing::info!("LZH emitter connected to {}", cfg.pcs_addr);

    let mut tick = interval(cfg.emit_interval);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tick.tick().await;
        let frame = session.lock().await.emit();
        stream.write_all(&frame.encode()).await?;
    }
}

#[allow(dead_code)]
fn _assert_outframe_sendable(_: OutFrame) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};
    use std::time::Duration;

    use crate::frame::{InFrame, OutFrame};
    use crate::state::{CoatingPlan, LayerSpec, State};

    fn one_layer_plan() -> CoatingPlan {
        CoatingPlan {
            layers: vec![LayerSpec {
                design_thickness: 100.0,
                design_rate: 0.5,
                n_index: 0,
                central_wavelength: 0.0,
            }],
            current_test_glass: 1,
        }
    }

    async fn unused_tcp_port() -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        l.local_addr().unwrap().port()
    }

    #[tokio::test]
    async fn end_to_end_roundtrip_over_tcp() {
        // Stand up a fake PCS:
        //   * accepts our emitter on 9052_eq and reads OutFrames,
        //   * connects to our listener on 5054_eq and writes InFrames.
        let pcs_port = unused_tcp_port().await;
        let listen_port = unused_tcp_port().await;

        let cfg = TransportConfig {
            pcs_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), pcs_port),
            listen_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), listen_port),
            emit_interval: Duration::from_millis(20),
            reconnect_backoff: Duration::from_millis(10),
        };

        // Fake PCS server side: listens on pcs_port for our emitter to connect.
        let pcs_listener = TcpListener::bind(("127.0.0.1", pcs_port)).await.unwrap();
        let pcs_recv_task = tokio::spawn(async move {
            let (mut stream, _) = pcs_listener.accept().await.unwrap();
            let mut buf = [0u8; FRAME_LEN];
            let mut frames = Vec::new();
            for _ in 0..5 {
                stream.read_exact(&mut buf).await.unwrap();
                frames.push(OutFrame::decode(&buf).unwrap());
            }
            frames
        });

        let transport = LzhTransport::new(one_layer_plan(), cfg.clone());
        let session = transport.session();
        session.lock().await.mark_initialized();
        let _run = tokio::spawn(transport.run());

        // Fake PCS client side: connect to our listener and push InFrames.
        let mut pcs_sender = loop {
            match TcpStream::connect(("127.0.0.1", listen_port)).await {
                Ok(s) => break s,
                Err(_) => tokio::time::sleep(Duration::from_millis(10)).await,
            }
        };
        let push = InFrame {
            deposit: true,
            acp: true,
            auto_scaling: false,
        };
        pcs_sender.write_all(&push.encode()).await.unwrap();

        let frames = pcs_recv_task.await.unwrap();
        assert_eq!(frames.len(), 5);
        assert!(frames.last().unwrap().heartbeat >= 2);
        assert!(frames.iter().all(|f| f.ready));

        // The InFrame we pushed should have transitioned the session into
        // Depositing within a few emitter ticks.
        tokio::time::sleep(Duration::from_millis(50)).await;
        let state = session.lock().await.state();
        assert_eq!(state, State::Depositing);
    }
}
