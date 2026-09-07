//! Device presence and heartbeat sync worker with Meridian cloud API.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use serde_json::json;
use tracing::{debug, info};

use crate::device::models::DeviceReport;

pub const DEFAULT_API_URL: &str = "https://www.meridianhub.cc";

#[derive(Debug)]
pub struct HeartbeatWorker {
    should_run: Arc<AtomicBool>,
}

impl HeartbeatWorker {
    pub fn start(report: DeviceReport, mesh_ip: Option<String>, api_url: Option<String>) -> Self {
        let should_run = Arc::new(AtomicBool::new(true));
        let flag = should_run.clone();

        let base_url = api_url.unwrap_or_else(|| DEFAULT_API_URL.to_string());
        let endpoint = format!("{}/api/devices/heartbeat", base_url.trim_end_matches('/'));

        tokio::spawn(async move {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap();

            info!("Starting cloud heartbeat for device {} -> {}", report.udid, endpoint);

            while flag.load(Ordering::SeqCst) {
                let payload = json!({
                    "udid": report.udid,
                    "name": report.name,
                    "model": report.model,
                    "version": report.os_version,
                    "build": report.build_version,
                    "ports": {
                        "slot": report.ports.slot,
                        "wda": report.ports.wda,
                        "stream": report.ports.stream,
                        "bridge": report.ports.bridge,
                    },
                    "mesh_ip": mesh_ip,
                    "status": "online",
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                });

                match client.post(&endpoint).json(&payload).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        debug!("✓ Heartbeat synced for {}", report.udid);
                    }
                    Ok(resp) => {
                        debug!("Heartbeat non-200 for {}: {}", report.udid, resp.status());
                    }
                    Err(e) => {
                        debug!("Heartbeat send error for {}: {:?}", report.udid, e);
                    }
                }

                tokio::time::sleep(Duration::from_secs(10)).await;
            }

            // Send offline payload when stopped
            let offline_payload = json!({
                "udid": report.udid,
                "status": "offline",
                "timestamp": chrono::Utc::now().to_rfc3339(),
            });
            let _ = client.post(&endpoint).json(&offline_payload).send().await;
            info!("Sent offline heartbeat status for {}", report.udid);
        });

        Self { should_run }
    }

    pub fn stop(&self) {
        self.should_run.store(false, Ordering::SeqCst);
    }
}
