//! Dynamic port and slot management for multiple connected iPhones.
//!
//! Standard port layout per slot:
//! - WDA / WebDriverAgent: 8100 + slot (8100..8131)
//! - Stream / H.264 & MJPEG: 9200 + slot (9200..9231)
//! - Remote Bridge / Touch: 9001 + slot (9001..9032)

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use serde::{Deserialize, Serialize};

pub const BASE_WDA_PORT: u16 = 8100;
pub const BASE_STREAM_PORT: u16 = 9200;
pub const BASE_BRIDGE_PORT: u16 = 9001;
pub const MAX_SLOTS: u16 = 32;

// Published (rathole / VPS-facing) port layout per slot. These are the ports the
// remote web client reaches, and are what the heartbeat reports. WDA and bridge
// are plain ports on the VPS loopback served through nginx (`/dev/<port>`);
// stream is the stunnel TLS port (browser talks `:19200+slot` directly, which
// stunnel wraps to the rathole-internal `19100+slot`).
pub const RATHOLE_BASE_WDA_PORT: u16 = 18100;
pub const RATHOLE_BASE_BRIDGE_PORT: u16 = 19001;
pub const RATHOLE_BASE_STREAM_PORT: u16 = 19200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevicePorts {
    pub slot: u16,
    pub wda: u16,
    pub stream: u16,
    pub bridge: u16,
}

impl DevicePorts {
    pub fn for_slot(slot: u16) -> Self {
        Self {
            slot,
            wda: BASE_WDA_PORT + slot,
            stream: BASE_STREAM_PORT + slot,
            bridge: BASE_BRIDGE_PORT + slot,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RatholePorts {
    pub slot: u16,
    pub wda: u16,
    pub bridge: u16,
    pub stream: u16,
}

impl RatholePorts {
    pub fn for_slot(slot: u16) -> Self {
        Self {
            slot,
            wda: RATHOLE_BASE_WDA_PORT + slot,
            bridge: RATHOLE_BASE_BRIDGE_PORT + slot,
            stream: RATHOLE_BASE_STREAM_PORT + slot,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SlotManager {
    allocations: Arc<Mutex<HashMap<String, u16>>>,
}

impl SlotManager {
    pub fn new() -> Self {
        Self {
            allocations: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Allocate or retrieve an existing slot for a given device UDID.
    pub fn allocate(&self, udid: &str) -> DevicePorts {
        let mut map = self.allocations.lock().unwrap();
        if let Some(&slot) = map.get(udid) {
            return DevicePorts::for_slot(slot);
        }

        // Find the lowest free slot index (0..MAX_SLOTS)
        let used_slots: Vec<u16> = map.values().copied().collect();
        for slot in 0..MAX_SLOTS {
            if !used_slots.contains(&slot) {
                map.insert(udid.to_string(), slot);
                return DevicePorts::for_slot(slot);
            }
        }

        // Fallback if full: wrap around
        let slot = (map.len() as u16) % MAX_SLOTS;
        map.insert(udid.to_string(), slot);
        DevicePorts::for_slot(slot)
    }

    /// Release slot when device is disconnected.
    pub fn release(&self, udid: &str) {
        let mut map = self.allocations.lock().unwrap();
        map.remove(udid);
    }
}
