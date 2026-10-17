//! Rathole Direct TCP Tunnel Supervisor.
//!
//! Manages the rathole client process for direct TCP forwarding from VPS to host PC.
//! Replaces the previous Tailscale mesh sidecar (Go binary + DERP relay).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

use crate::core::slots::{BASE_BRIDGE_PORT, BASE_STREAM_PORT, BASE_WDA_PORT};
use crate::core::vault::Vault;

const VPS_ADDR: &str = "98.84.189.148:2333";

#[derive(Debug, Clone)]
pub struct TunnelStatus {
    /// Whether the rathole control channel is currently connected. Internet
    /// link quality (latency, loss, speed) is tracked separately by the
    /// `net` module's `NetHealthMonitor`.
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
    slots: Arc<Mutex<Vec<u16>>>,
    shutdown_tx: broadcast::Sender<bool>,
}

impl TunnelSupervisor {
    pub fn new(_vault: Vault) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            status: Arc::new(tokio::sync::Mutex::new(TunnelStatus::offline())),
            should_run: Arc::new(AtomicBool::new(true)),
            slots: Arc::new(Mutex::new(Vec::new())),
            shutdown_tx,
        }
    }

    pub async fn get_status(&self) -> TunnelStatus {
        self.status.lock().await.clone()
    }

    pub fn start(&self) {
        let status = self.status.clone();
        let should_run = self.should_run.clone();
        let slots = self.slots.clone();
        let shutdown_tx = self.shutdown_tx.clone();

        info!("[TUNNEL] TunnelSupervisor::start() called — spawning rathole loop");
        tokio::spawn(async move {
            info!("[TUNNEL] Background task spawned, entering run_rathole_loop");
            run_rathole_loop(status, should_run, slots, shutdown_tx).await;
        });
    }

    /// Atomically replace the set of active device slots and restart the tunnel
    /// so the new per-slot services take effect. No-op when unchanged.
    pub fn update_slots(&self, mut slots: Vec<u16>) {
        slots.sort_unstable();
        slots.dedup();
        let changed = {
            let mut current = self.slots.lock().unwrap();
            if *current == slots {
                false
            } else {
                *current = slots.clone();
                true
            }
        };
        if changed {
            info!("[TUNNEL] Slots changed to {:?} — restarting tunnel", slots);
            let _ = self.shutdown_tx.send(true);
        }
    }

    pub async fn stop(&self) {
        self.should_run.store(false, Ordering::SeqCst);
        let _ = self.shutdown_tx.send(true);
    }
}

async fn run_rathole_loop(
    status: Arc<tokio::sync::Mutex<TunnelStatus>>,
    should_run: Arc<AtomicBool>,
    slots: Arc<Mutex<Vec<u16>>>,
    shutdown_tx: broadcast::Sender<bool>,
) {
    info!("[TUNNEL] run_rathole_loop entered");

    while should_run.load(Ordering::SeqCst) {
        let slot_list = slots.lock().unwrap().clone();
        let config_toml = generate_client_config(&slot_list);
        info!("[TUNNEL] Generated client config for {} slots:\n{}", slot_list.len(), config_toml);
        let tmp_path = std::env::temp_dir().join("meridian-rathole-client.toml");

        if let Err(e) = std::fs::write(&tmp_path, &config_toml) {
            error!("[TUNNEL] Failed to write config to {:?}: {:?}", tmp_path, e);
            {
                let mut st = status.lock().await;
                st.status_text = format!("Config write failed: {}", e);
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
            continue;
        }
        debug!("[TUNNEL] Config written to {:?}", tmp_path);

        {
            let mut st = status.lock().await;
            st.is_running = false;
            st.status_text = "Connecting to VPS...".to_string();
        }

        info!("[TUNNEL] Starting rathole client → {}", VPS_ADDR);

        let shutdown_rx = shutdown_tx.subscribe();
        let cli = rathole::Cli {
            config_path: Some(tmp_path.clone()),
            server: false,
            client: true,
            genkey: None,
        };
        debug!("[TUNNEL] Cli struct created: {:?}", cli);

        let rathole_handle = tokio::spawn(async move {
            info!("[TUNNEL] rathole::run() calling...");
            let result = rathole::run(cli, shutdown_rx).await;
            info!("[TUNNEL] rathole::run() returned: {:?}", result);
            if let Err(e) = result {
                error!("[TUNNEL] rathole client error: {:?}", e);
            }
        });

        tokio::time::sleep(Duration::from_millis(500)).await;

        {
            let mut st = status.lock().await;
            st.is_running = true;
            st.status_text = "Tunnel online (rathole)".to_string();
        }
        info!("[TUNNEL] ✓ Marked as online, waiting for rathole to exit or shutdown signal");

        tokio::select! {
            _ = rathole_handle => {
                warn!("[TUNNEL] rathole process exited, restarting in 3s...");
            }
            _ = should_run_changed(&should_run) => {
                info!("[TUNNEL] Tunnel supervisor shutting down");
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
    info!("[TUNNEL] run_rathole_loop exited");
}

async fn should_run_changed(flag: &Arc<AtomicBool>) {
    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if !flag.load(Ordering::SeqCst) {
            return;
        }
    }
}

/// Build the rathole client config for the given active slots. Each slot gets
/// three services (WDA, bridge, stream) with deterministic tokens that must
/// match the VPS `rathole-server.toml`.
fn generate_client_config(slots: &[u16]) -> String {
    let mut out = format!(
        r#"[client]
remote_addr = "{vps_addr}"
retry_interval = 1
heartbeat_timeout = 10

[client.transport]
type = "tcp"

[client.transport.tcp]
nodelay = true
keepalive_secs = 10
keepalive_interval = 3
"#,
        vps_addr = VPS_ADDR,
    );

    for &slot in slots {
        out.push_str(&format!(
            r#"
[client.services.wda-{slot}]
type = "tcp"
token = "meridian-wda-{slot}"
local_addr = "127.0.0.1:{wda_local}"
nodelay = true
retry_interval = 1

[client.services.bridge-{slot}]
type = "tcp"
token = "meridian-bridge-{slot}"
local_addr = "127.0.0.1:{bridge_local}"
nodelay = true
retry_interval = 1

[client.services.stream-{slot}]
type = "tcp"
token = "meridian-stream-{slot}"
local_addr = "127.0.0.1:{stream_local}"
nodelay = true
retry_interval = 1
"#,
            wda_local = BASE_WDA_PORT + slot,
            bridge_local = BASE_BRIDGE_PORT + slot,
            stream_local = BASE_STREAM_PORT + slot,
        ));
    }

    out
}
