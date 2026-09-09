//! Rathole Direct TCP Tunnel Supervisor.
//!
//! Manages the rathole client process for direct TCP forwarding from VPS to host PC.
//! Replaces the previous Tailscale mesh sidecar (Go binary + DERP relay).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{error, info, warn};

use crate::core::vault::Vault;

const VPS_ADDR: &str = "100.51.75.20:2333";

#[derive(Debug, Clone)]
pub struct TunnelStatus {
    pub is_running: bool,
    pub status_text: String,
}

impl TunnelStatus {
    pub fn offline() -> Self {
        Self {
            is_running: false,
            status_text: "Initializing tunnel...".to_string(),
        }
    }
}

pub struct TunnelSupervisor {
    status: Arc<tokio::sync::Mutex<TunnelStatus>>,
    should_run: Arc<AtomicBool>,
    shutdown_tx: broadcast::Sender<bool>,
}

impl TunnelSupervisor {
    pub fn new(_vault: Vault) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            status: Arc::new(tokio::sync::Mutex::new(TunnelStatus::offline())),
            should_run: Arc::new(AtomicBool::new(true)),
            shutdown_tx,
        }
    }

    pub async fn get_status(&self) -> TunnelStatus {
        self.status.lock().await.clone()
    }

    pub fn start(&self) {
        let status = self.status.clone();
        let should_run = self.should_run.clone();
        let shutdown_tx = self.shutdown_tx.clone();

        tokio::spawn(async move {
            run_rathole_loop(status, should_run, shutdown_tx).await;
        });
    }

    pub async fn stop(&self) {
        self.should_run.store(false, Ordering::SeqCst);
        let _ = self.shutdown_tx.send(true);
    }
}

async fn run_rathole_loop(
    status: Arc<tokio::sync::Mutex<TunnelStatus>>,
    should_run: Arc<AtomicBool>,
    shutdown_tx: broadcast::Sender<bool>,
) {
    while should_run.load(Ordering::SeqCst) {
        let config_toml = generate_client_config();
        let tmp_path = std::env::temp_dir().join("meridian-rathole-client.toml");

        if let Err(e) = std::fs::write(&tmp_path, &config_toml) {
            error!("Failed to write rathole config to {:?}: {:?}", tmp_path, e);
            {
                let mut st = status.lock().await;
                st.status_text = format!("Config write failed: {}", e);
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
            continue;
        }

        {
            let mut st = status.lock().await;
            st.is_running = false;
            st.status_text = "Connecting to VPS...".to_string();
        }

        info!("Starting rathole client → {}", VPS_ADDR);

        let shutdown_rx = shutdown_tx.subscribe();
        let cli = rathole::Cli {
            config_path: Some(tmp_path.clone()),
            server: false,
            client: true,
            genkey: None,
        };

        let rathole_handle = tokio::spawn(async move {
            if let Err(e) = rathole::run(cli, shutdown_rx).await {
                error!("rathole client exited with error: {:?}", e);
            }
        });

        {
            let mut st = status.lock().await;
            st.is_running = true;
            st.status_text = "Tunnel online (rathole)".to_string();
        }
        info!("✓ rathole client connected to {}", VPS_ADDR);

        // Wait for shutdown signal or rathole exit
        tokio::select! {
            _ = rathole_handle => {
                warn!("rathole process exited, restarting in 3s...");
            }
            _ = should_run_changed(&should_run) => {
                info!("Tunnel supervisor shutting down");
                break;
            }
        }

        {
            let mut st = status.lock().await;
            st.is_running = false;
            st.status_text = "Reconnecting in 3s...".to_string();
        }

        tokio::time::sleep(Duration::from_secs(3)).await;
    }

    let _ = std::fs::remove_file(std::env::temp_dir().join("meridian-rathole-client.toml"));
}

async fn should_run_changed(flag: &Arc<AtomicBool>) {
    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if !flag.load(Ordering::SeqCst) {
            return;
        }
    }
}

fn generate_client_config() -> String {
    format!(
        r#"[client]
remote_addr = "{vps_addr}"

[client.services.wda]
type = "tcp"
local_addr = "127.0.0.1:8100"
remote_addr = "0.0.0.0:18100"
token = "{wda_token}"

[client.services.bridge]
type = "tcp"
local_addr = "127.0.0.1:9001"
remote_addr = "0.0.0.0:19001"
token = "{bridge_token}"

[client.services.stream]
type = "tcp"
local_addr = "127.0.0.1:9200"
remote_addr = "0.0.0.0:19200"
token = "{stream_token}"
"#,
        vps_addr = VPS_ADDR,
        wda_token = "meridian-wda-token",
        bridge_token = "meridian-bridge-token",
        stream_token = "meridian-stream-token",
    )
}
