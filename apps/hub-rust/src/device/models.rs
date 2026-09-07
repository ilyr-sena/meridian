//! Device models, connection states, and status reports.

use serde::{Deserialize, Serialize};
use crate::core::slots::DevicePorts;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceState {
    Offline,
    Connected,
    NeedsSideload,
    Pairing,
    Ready,
    Starting,
    Running,
    Error,
}

impl DeviceState {
    pub fn label(&self) -> &'static str {
        match self {
            DeviceState::Offline => "Offline",
            DeviceState::Connected => "Connected",
            DeviceState::NeedsSideload => "Runner Not Installed",
            DeviceState::Pairing => "Pairing",
            DeviceState::Ready => "Ready",
            DeviceState::Starting => "Starting...",
            DeviceState::Running => "Live Streaming",
            DeviceState::Error => "Error",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceReport {
    pub udid: String,
    pub device_id: u32,
    pub name: String,
    pub model: String,
    pub os_version: String,
    pub build_version: String,
    pub serial_number: Option<String>,
    pub ports: DevicePorts,
    pub state: DeviceState,
    pub status_message: String,
    pub runner_installed: bool,
    pub battery_level: Option<i32>,
}

impl DeviceReport {
    pub fn new_empty(udid: String, device_id: u32, ports: DevicePorts) -> Self {
        Self {
            udid,
            device_id,
            name: "iPhone".to_string(),
            model: "Apple Device".to_string(),
            os_version: "iOS".to_string(),
            build_version: String::new(),
            serial_number: None,
            ports,
            state: DeviceState::Connected,
            status_message: "Detecting installed services...".to_string(),
            runner_installed: false,
            battery_level: None,
        }
    }

    /// Return masked name and UDID if sensitive mode is enabled.
    pub fn masked_name(&self, mask: bool) -> String {
        if !mask || self.name.len() <= 3 {
            return self.name.clone();
        }
        format!("{}•••", &self.name[..2])
    }

    pub fn masked_udid(&self, mask: bool) -> String {
        if !mask || self.udid.len() <= 8 {
            return self.udid.clone();
        }
        format!("{}••••••••{}", &self.udid[..4], &self.udid[self.udid.len() - 4..])
    }
}
