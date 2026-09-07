//! Device presence and heartbeat sync worker with Meridian cloud API.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use serde_json::json;
use tracing::{debug, info};

use crate::core::slots::DevicePorts;
use crate::device::models::DeviceReport;

pub const DEFAULT_API_URL: &str = "https://www.meridianhub.cc";

#[derive(Debug)]
pub struct HeartbeatWorker {
    should_run: Arc<AtomicBool>,
    session_active: Arc<AtomicBool>,
}

impl HeartbeatWorker {
    pub fn start(
        report: DeviceReport,
        shared_mesh_ip: Arc<RwLock<Option<String>>>,
        session_active: Arc<AtomicBool>,
        api_url: Option<String>,
    ) -> Self {
        let should_run = Arc::new(AtomicBool::new(true));
        let flag = should_run.clone();
        let is_session_active = session_active.clone();

        let base_url = api_url.unwrap_or_else(|| DEFAULT_API_URL.to_string());
        let endpoint = format!("{}/api/devices/heartbeat", base_url.trim_end_matches('/'));

        tokio::spawn(async move {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap();

            info!("Starting cloud heartbeat for device {} -> {}", report.udid, endpoint);

            while flag.load(Ordering::SeqCst) {
                let current_mesh_ip = {
                    shared_mesh_ip.read().ok().and_then(|guard| guard.clone())
                };

                let active = is_session_active.load(Ordering::SeqCst);
                let host_ports = if active {
                    json!({
                        "wda": report.ports.wda,
                        "stream": report.ports.stream,
                        "bridge": report.ports.bridge,
                    })
                } else {
                    serde_json::Value::Null
                };

                let payload = json!({
                    "udid": report.udid,
                    "name": report.name,
                    "model": report.model,
                    "version": report.os_version,
                    "build": report.build_version,
                    "tailscale_ip": current_mesh_ip,
                    "mesh_ip": current_mesh_ip,
                    "host_ports": host_ports,
                    "status": "online",
                    "session_active": active,
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                });

                match client.post(&endpoint).json(&payload).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        debug!("✓ Cloud heartbeat synced for {} (active: {})", report.udid, active);
                    }
                    Ok(resp) => {
                        debug!("Cloud heartbeat status {}: {}", report.udid, resp.status());
                    }
                    Err(e) => {
                        debug!("Cloud heartbeat error {}: {:?}", report.udid, e);
                    }
                }

                tokio::time::sleep(Duration::from_secs(6)).await;
            }

            // Send offline payload when stopped
            let offline_payload = json!({
                "udid": report.udid,
                "status": "offline",
                "host_ports": serde_json::Value::Null,
                "end_active_session": true,
                "end_reason": "device_detached",
                "timestamp": chrono::Utc::now().to_rfc3339(),
            });
            let _ = client.post(&endpoint).json(&offline_payload).send().await;
            info!("Sent offline heartbeat status for {}", report.udid);
        });

        Self { should_run, session_active }
    }

    pub fn set_session_active(&self, active: bool) {
        self.session_active.store(active, Ordering::SeqCst);
    }

    pub fn stop(&self) {
        self.should_run.store(false, Ordering::SeqCst);
    }
}

/// Send an immediate session state update to the cloud (start or stop).
pub async fn sync_session_state(
    udid: &str,
    active: bool,
    ports: Option<DevicePorts>,
    mesh_ip: Option<String>,
    api_url: Option<&str>,
) {
    let base_url = api_url.unwrap_or(DEFAULT_API_URL);
    let endpoint = format!("{}/api/devices/heartbeat", base_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build();

    if let Ok(c) = client {
        let host_ports = if active {
            if let Some(p) = ports {
                json!({
                    "wda": p.wda,
                    "stream": p.stream,
                    "bridge": p.bridge,
                })
            } else {
                json!({ "wda": 8100, "stream": 9200, "bridge": 9001 })
            }
        } else {
            serde_json::Value::Null
        };

        let mut payload = json!({
            "udid": udid,
            "status": "online",
            "tailscale_ip": mesh_ip,
            "mesh_ip": mesh_ip,
            "host_ports": host_ports,
            "session_active": active,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });

        if !active {
            payload["end_active_session"] = json!(true);
            payload["end_reason"] = json!("hub_stopped");
            payload["session_state"] = json!("ended");
        }

        let res = c.post(&endpoint).json(&payload).send().await;
        match res {
            Ok(resp) if resp.status().is_success() => {
                info!("✓ Session state synced to cloud: {} active={}", udid, active);
            }
            Ok(resp) => {
                debug!("Session sync returned status {}: {}", udid, resp.status());
            }
            Err(e) => {
                debug!("Session sync error for {}: {:?}", udid, e);
            }
        }
    }
}

pub async fn send_offline_sync(udid: &str, api_url: Option<&str>) {
    let base_url = api_url.unwrap_or(DEFAULT_API_URL);
    let endpoint = format!("{}/api/devices/heartbeat", base_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build();
    if let Ok(c) = client {
        let payload = json!({
            "udid": udid,
            "status": "offline",
            "host_ports": serde_json::Value::Null,
            "end_active_session": true,
            "end_reason": "device_detached",
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });
        let _ = c.post(&endpoint).json(&payload).send().await;
        debug!("Sent offline presence update for {}", udid);
    }
}
