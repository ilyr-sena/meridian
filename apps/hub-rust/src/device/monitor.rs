//! Real-time Usbmuxd device monitor and event streamer.
//!
//! Streams real-time attach and detach events to UI components without polling.

use std::collections::HashMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::core::slots::SlotManager;
use crate::device::lockdown::query_lockdown;
use crate::device::models::{DeviceReport, DeviceState};
use crate::device::tunnel::connect_usbmuxd;

#[derive(Debug, Clone)]
pub enum DeviceEvent {
    Attached(DeviceReport),
    Updated(DeviceReport),
    Detached(String), // UDID
}

pub struct DeviceMonitor {
    slot_mgr: SlotManager,
    tx: mpsc::UnboundedSender<DeviceEvent>,
}

impl DeviceMonitor {
    pub fn new(slot_mgr: SlotManager, tx: mpsc::UnboundedSender<DeviceEvent>) -> Self {
        Self { slot_mgr, tx }
    }

    /// Spawns the persistent background usbmuxd watcher task with auto-reconnect.
    pub fn start(self) {
        tokio::spawn(async move {
            loop {
                info!("Connecting to usbmuxd monitor stream...");
                if let Err(e) = self.run_listener().await {
                    warn!("Usbmuxd monitor stream disconnected: {:?}. Retrying in 2s...", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                }
            }
        });
    }

    async fn run_listener(&self) -> anyhow::Result<()> {
        let mut mux = connect_usbmuxd().await?;

        // Send Listen request
        let mut dict = plist::Dictionary::new();
        dict.insert("MessageType".into(), plist::Value::String("Listen".into()));
        dict.insert("ClientVersion".into(), plist::Value::Integer(7.into()));
        dict.insert("ProgName".into(), plist::Value::String("meridian-hub".into()));
        dict.insert("kLibUSBMuxVersion".into(), plist::Value::Integer(3.into()));

        let mut xml = Vec::new();
        plist::to_writer_xml(&mut xml, &dict)?;

        let total_len = (16 + xml.len()) as u32;
        let mut header = Vec::with_capacity(16);
        header.extend_from_slice(&total_len.to_le_bytes());
        header.extend_from_slice(&1u32.to_le_bytes());
        header.extend_from_slice(&8u32.to_le_bytes());
        header.extend_from_slice(&1u32.to_le_bytes());

        mux.write_all(&header).await?;
        mux.write_all(&xml).await?;
        mux.flush().await?;

        let mut id_to_udid: HashMap<u32, String> = HashMap::new();

        loop {
            let mut resp_header = [0u8; 16];
            mux.read_exact(&mut resp_header).await?;
            let resp_len = u32::from_le_bytes([resp_header[0], resp_header[1], resp_header[2], resp_header[3]]) as usize;
            if resp_len < 16 {
                continue;
            }

            let mut payload = vec![0u8; resp_len - 16];
            mux.read_exact(&mut payload).await?;

            if let Ok(plist::Value::Dictionary(dict)) = plist::from_bytes(&payload) {
                let msg_type = dict.get("MessageType").and_then(|v| v.as_string()).unwrap_or("");

                match msg_type {
                    "Attached" => {
                        let device_id = dict.get("DeviceID").and_then(|v| v.as_unsigned_integer()).unwrap_or(0) as u32;
                        if let Some(plist::Value::Dictionary(props)) = dict.get("Properties") {
                            let udid = props.get("SerialNumber").and_then(|v| v.as_string()).unwrap_or("").to_string();
                            let conn_type = props.get("ConnectionType").and_then(|v| v.as_string()).unwrap_or("");

                            if !udid.is_empty() && conn_type == "USB" {
                                info!("🔌 USB device attached: {} (ID: {})", udid, device_id);
                                id_to_udid.insert(device_id, udid.clone());

                                let ports = self.slot_mgr.allocate(&udid);
                                let report = DeviceReport::new_empty(udid.clone(), device_id, ports);

                                // Query lockdown in background to enrich metadata
                                let tx_clone = self.tx.clone();
                                let mut report_for_enrich = report.clone();
                                tokio::spawn(async move {
                                    // Give usbmuxd and device 300ms to settle
                                    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
                                    match query_lockdown(device_id, &udid).await {
                                        Ok(info) => {
                                            report_for_enrich.name = info.name;
                                            report_for_enrich.model = info.model;
                                            report_for_enrich.os_version = info.os_version;
                                            report_for_enrich.build_version = info.build_version;
                                            report_for_enrich.serial_number = info.serial_number;
                                            report_for_enrich.state = DeviceState::Ready;
                                            report_for_enrich.status_message = format!("Ready (Slot {})", ports.slot);
                                            info!("✓ Enriched device: {} ({}, {})", report_for_enrich.name, report_for_enrich.model, report_for_enrich.os_version);
                                            let _ = tx_clone.send(DeviceEvent::Updated(report_for_enrich));
                                        }
                                        Err(e) => {
                                            warn!("Lockdown query deferred for {}: {:?}", udid, e);
                                            report_for_enrich.state = DeviceState::Ready;
                                            let _ = tx_clone.send(DeviceEvent::Updated(report_for_enrich));
                                        }
                                    }
                                });

                                let _ = self.tx.send(DeviceEvent::Attached(report));
                            }
                        }
                    }
                    "Detached" => {
                        let device_id = dict.get("DeviceID").and_then(|v| v.as_unsigned_integer()).unwrap_or(0) as u32;
                        if let Some(udid) = id_to_udid.remove(&device_id) {
                            info!("🔌 USB device detached: {} (ID: {})", udid, device_id);
                            self.slot_mgr.release(&udid);
                            let _ = self.tx.send(DeviceEvent::Detached(udid));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
