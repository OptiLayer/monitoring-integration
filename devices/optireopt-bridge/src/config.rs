use std::net::SocketAddr;

use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "optireopt-bridge",
    about = "Virtual spectrometer that ingests pushed scans from OptiReOpt"
)]
pub struct Cli {
    /// Bind address for the HTTP/WS service.
    #[arg(long, env = "BRIDGE_HOST", default_value = "0.0.0.0")]
    pub host: String,

    /// HTTP port. Must match OPTIREOPT_BROADCAST_URL on the OptiReOpt side
    /// (default 8473).
    #[arg(long, env = "BRIDGE_PORT", default_value_t = 8473)]
    pub port: u16,

    /// PCS server `IP:port` (the customer's vacuum-machine LZH endpoint). When
    /// unset, LZH integration is disabled and the bridge runs in its previous
    /// no-op vacuum-chamber mode.
    #[arg(long, env = "BRIDGE_PCS_ADDR")]
    pub pcs_addr: Option<SocketAddr>,

    /// Local `IP:port` for accepting the PCS-side connection that pushes
    /// InFrames at us. Defaults to `0.0.0.0:5054`, matching the customer's
    /// default Spa configuration.
    #[arg(long, env = "BRIDGE_LZH_LISTEN", default_value = "0.0.0.0:5054")]
    pub lzh_listen: SocketAddr,
}
