//! Device models, connection states, and status reports.

use serde::{Deserialize, Serialize};
use crate::core::slots::DevicePorts;

/// Tri-state probe result. A probe either answered `Yes`/`No`, or could not be
/// determined right now (`Unknown`). The UI treats `Unknown` pessimistically so
/// it never *lies* — it simply refuses to enable an action until it knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TriState {
    Yes,
    No,
    #[default]
    Unknown,
}

impl TriState {
    pub fn is_yes(self) -> bool {
        self == TriState::Yes
    }

    pub fn is_no(self) -> bool {
        self == TriState::No
    }

    pub fn is_unknown(self) -> bool {
        self == TriState::Unknown
    }
}

/// Orthogonal, combinable device-capability flags gathered by the real-time
/// health poller. Every dimension is independent — any combination of
/// installed / developer-mode / locked / paired is meaningful and must render a
/// coherent UI (see `crate::device::health` for the interpreter).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DeviceHealth {
    /// MeridianRunner installed on the device.
    pub runner_installed: TriState,
    /// iOS Developer Mode toggle enabled (iOS 17+; treated `Yes` on older iOS).
    pub developer_mode: TriState,
    /// Passcode / screen lock currently active (safe-guards all actions).
    pub locked: TriState,
    /// Usbmuxd pairing record exists locally / device was trusted once.
    pub paired: TriState,
}

/// Session lifecycle of the MeridianRunner on a device. This is **app-level**
/// state (driven by Start/Stop flows) and is tracked separately from the
/// combinable `DeviceHealth` capability flags so the health poller never
/// clobbers a live session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SessionPhase {
    #[default]
    Idle,
    Starting,
    Running,
}

impl SessionPhase {
    /// Human label, or `None` when idle (the health/capability state is shown instead).
    pub fn label(self) -> Option<&'static str> {
        match self {
            SessionPhase::Idle => None,
            SessionPhase::Starting => Some("Starting Services..."),
            SessionPhase::Running => Some("Live Streaming"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceState {
    Offline,
    Connected,
    NeedsSideload,
    Unpaired,
    Locked,
    DeveloperModeOff,
    Ready,
    Starting,
    Running,
    Error,
}

impl DeviceState {
    pub fn label(&self) -> &'static str {
        match self {
            DeviceState::Offline => "Offline",
            DeviceState::Connected => "Checking...",
            DeviceState::NeedsSideload => "Runner Not Installed",
            DeviceState::Unpaired => "Trust Required",
            DeviceState::Locked => "Locked",
            DeviceState::DeveloperModeOff => "Developer Mode Off",
            DeviceState::Ready => "Ready",
            DeviceState::Starting => "Starting...",
            DeviceState::Running => "Live Streaming",
            DeviceState::Error => "Error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceReport {
    pub udid: String,
    pub device_id: u32,
    pub name: String,
    pub model: String,
    pub os_version: String,
    pub build_version: String,
    pub serial_number: Option<String>,
    pub ports: DevicePorts,
    /// Session lifecycle (Idle / Starting / Running) — app-level.
    pub session_phase: SessionPhase,
    /// Combinable capability flags — updated in real time by the health poller.
    pub health: DeviceHealth,
    /// Derived capability state (via `health_state`), plus transient `Error`.
    pub state: DeviceState,
    pub status_message: String,
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
            session_phase: SessionPhase::Idle,
            health: DeviceHealth::default(),
            state: DeviceState::Connected,
            status_message: "Detecting installed services...".to_string(),
            battery_level: None,
        }
    }

    /// True when the installed check answered `Yes` (used to gate Start).
    pub fn runner_installed(&self) -> bool {
        self.health.runner_installed.is_yes()
    }

    /// Merge a health-poll snapshot into `self` *without* clobbering the
    /// app-level session phase — except when the device health is a **hard
    /// regression** (uninstalled / locked / Developer Mode off / untrusted), in
    /// which case a live session cannot exist and the phase falls back to Idle.
    /// Transient `Error` states are preserved until health reports a concrete
    /// (non-Ready) condition.
    pub fn apply_health(&mut self, other: &DeviceReport) {
        self.health = other.health;
        self.state = other.state;
        if matches!(
            other.state,
            DeviceState::Locked
                | DeviceState::NeedsSideload
                | DeviceState::DeveloperModeOff
                | DeviceState::Unpaired
        ) {
            self.session_phase = SessionPhase::Idle;
        }
        if self.session_phase == SessionPhase::Idle && self.state != DeviceState::Error {
            self.status_message = other.status_message.clone();
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