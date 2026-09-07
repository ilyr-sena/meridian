//! Pure Rust WDA client and CoreDevice HID action dispatcher.
//!
//! Provides WebDriverAgent automation (touch, keyboard, session management)
//! and native 60Hz CoreDevice HID hardware button / digitizer input.

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, info};

use idevice::{
    IdeviceService, ReadWrite,
    provider::UsbmuxdProvider,
    services::{
        core_device::{ButtonState, IndigoHidClient},
        core_device_proxy::CoreDeviceProxy,
        rsd::RsdHandshake,
    },
    usbmuxd::UsbmuxdAddr,
};

// ---------------------------------------------------------------------------
// WebDriverAgent (WDA) Client
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct WdaClient {
    base_url: String,
    http: reqwest::Client,
    session_id: Arc<RwLock<Option<String>>>,
}

impl WdaClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap_or_default();

        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            http: client,
            session_id: Arc::new(RwLock::new(None)),
        }
    }

    /// Check if WDA responds on /status.
    pub async fn alive(&self) -> bool {
        let url = format!("{}/status", self.base_url);
        match self.http.get(&url).send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    /// Retrieve or create a WDA session ID.
    pub async fn get_session_id(&self) -> anyhow::Result<String> {
        {
            let guard = self.session_id.read().await;
            if let Some(ref sid) = *guard {
                return Ok(sid.clone());
            }
        }

        let mut guard = self.session_id.write().await;
        if let Some(ref sid) = *guard {
            return Ok(sid.clone());
        }

        let url = format!("{}/session", self.base_url);
        let resp = self
            .http
            .post(&url)
            .json(&serde_json::json!({ "capabilities": {} }))
            .send()
            .await?;

        let body: serde_json::Value = resp.json().await?;
        let sid = body["value"]["sessionId"]
            .as_str()
            .or_else(|| body["sessionId"].as_str())
            .ok_or_else(|| anyhow::anyhow!("No sessionId returned in WDA /session response"))?
            .to_string();

        info!("Created WDA session: {}", sid);

        // Best effort: set waitForIdleTimeout=0 for low-latency touch actions
        let settings_url = format!("{}/session/{}/appium/settings", self.base_url, sid);
        let _ = self
            .http
            .post(&settings_url)
            .json(&serde_json::json!({ "settings": { "waitForIdleTimeout": 0 } }))
            .send()
            .await;

        *guard = Some(sid.clone());
        Ok(sid)
    }

    pub async fn invalidate(&self) {
        let mut guard = self.session_id.write().await;
        *guard = None;
    }

    /// Press hardware button via WDA (e.g. "home", "volumeUp", "volumeDown").
    pub async fn press_key(&self, key: &str) -> anyhow::Result<()> {
        let sid = self.get_session_id().await?;
        let url = format!("{}/session/{}/wda/pressKey", self.base_url, sid);
        let resp = self
            .http
            .post(&url)
            .json(&serde_json::json!({ "key": key }))
            .send()
            .await;

        if let Ok(r) = resp {
            if r.status().is_success() {
                return Ok(());
            }
        }

        // Retry with refreshed session
        self.invalidate().await;
        let new_sid = self.get_session_id().await?;
        let retry_url = format!("{}/session/{}/wda/pressKey", self.base_url, new_sid);
        self.http
            .post(&retry_url)
            .json(&serde_json::json!({ "key": key }))
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }

    /// Type text using WDA keys API.
    pub async fn type_text(&self, text: &str) -> anyhow::Result<()> {
        let sid = self.get_session_id().await?;
        let url = format!("{}/session/{}/wda/keys", self.base_url, sid);
        let resp = self
            .http
            .post(&url)
            .json(&serde_json::json!({ "value": [text] }))
            .send()
            .await;

        if let Ok(r) = resp {
            if r.status().is_success() {
                return Ok(());
            }
        }

        self.invalidate().await;
        let new_sid = self.get_session_id().await?;
        let retry_url = format!("{}/session/{}/wda/keys", self.base_url, new_sid);
        self.http
            .post(&retry_url)
            .json(&serde_json::json!({ "value": [text] }))
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }

    /// Tap coordinates using WDA touch/perform API.
    pub async fn tap(&self, x: f32, y: f32) -> anyhow::Result<()> {
        let sid = self.get_session_id().await?;
        let url = format!("{}/session/{}/wda/touch/perform", self.base_url, sid);
        let payload = serde_json::json!({
            "actions": [
                { "action": "press", "options": { "x": x, "y": y } },
                { "action": "wait", "options": { "ms": 50 } },
                { "action": "release" }
            ]
        });

        self.http.post(&url).json(&payload).send().await?.error_for_status()?;
        Ok(())
    }

    /// Navigate to Home Screen.
    pub async fn homescreen(&self) -> anyhow::Result<()> {
        let url = format!("{}/wda/homescreen", self.base_url);
        self.http.post(&url).send().await?.error_for_status()?;
        Ok(())
    }

    /// Check if software keyboard is displayed on screen.
    pub async fn is_keyboard_displayed(&self) -> anyhow::Result<bool> {
        let sid = self.get_session_id().await?;
        let url = format!("{}/session/{}/element", self.base_url, sid);
        let payload = serde_json::json!({
            "using": "class name",
            "value": "XCUIElementTypeKeyboard"
        });

        let resp = self.http.post(&url).json(&payload).send().await?;
        if !resp.status().is_success() {
            return Ok(false);
        }

        let body: serde_json::Value = resp.json().await?;
        let element_id = body["value"]["ELEMENT"]
            .as_str()
            .unwrap_or_default();

        if element_id.is_empty() {
            return Ok(false);
        }

        let disp_url = format!("{}/session/{}/element/{}/displayed", self.base_url, sid, element_id);
        let disp_resp = self.http.get(&disp_url).send().await?;
        let disp_body: serde_json::Value = disp_resp.json().await?;

        Ok(disp_body["value"].as_bool().unwrap_or(false))
    }
}

// ---------------------------------------------------------------------------
// Native CoreDevice HID Action Dispatcher
// ---------------------------------------------------------------------------

pub struct CoreDeviceHid;

impl CoreDeviceHid {
    /// Dispatch a named hardware button action directly via CoreDevice HID.
    ///
    /// Supported button names:
    /// - "home": UsagePage 0x0C, UsageCode 0x40
    /// - "lock": UsagePage 0x0C, UsageCode 0x30
    /// - "volume-up": UsagePage 0x0C, UsageCode 0xE9
    /// - "volume-down": UsagePage 0x0C, UsageCode 0xEA
    /// - "mute": UsagePage 0x0C, UsageCode 0xE2
    /// - "siri": UsagePage 0x0C, UsageCode 0xCF
    /// - "keyboard-toggle": UsagePage 0x0C, UsageCode 0x01AE
    pub async fn send_hardware_button(
        udid: &str,
        device_id: u32,
        action: &str,
    ) -> anyhow::Result<()> {
        let (usage_page, usage_code, hold_ms) = match action {
            "home" => (0x0Cu64, 0x40u64, 50u64),
            "lock" => (0x0Cu64, 0x30u64, 400u64),
            "volume-up" => (0x0Cu64, 0xE9u64, 50u64),
            "volume-down" => (0x0Cu64, 0xEAu64, 50u64),
            "mute" => (0x0Cu64, 0xE2u64, 50u64),
            "siri" => (0x0Cu64, 0xCFu64, 800u64),
            "keyboard-toggle" => (0x0Cu64, 0x01AEu64, 50u64),
            other => anyhow::bail!("Unknown hardware button action: {}", other),
        };

        debug!("Dispatching CoreDevice button action '{}' to {}", action, udid);

        let provider = UsbmuxdProvider {
            addr: UsbmuxdAddr::default(),
            tag: 1,
            udid: udid.to_string(),
            device_id,
            label: "meridian-hub".to_string(),
        };

        let proxy = CoreDeviceProxy::connect(&provider).await?;
        let rsd_port = proxy.tunnel_info().server_rsd_port;
        let adapter = proxy.create_software_tunnel()?;
        let mut handle = adapter.to_async_handle();

        let rsd_stream = handle.connect(rsd_port).await?;
        let mut rsd = RsdHandshake::new(rsd_stream).await?;
        let mut hid: IndigoHidClient<Box<dyn ReadWrite>> = rsd.connect(&mut handle).await?;

        hid.send_button(usage_page, usage_code, ButtonState::Down).await?;
        tokio::time::sleep(Duration::from_millis(hold_ms)).await;
        hid.send_button(usage_page, usage_code, ButtonState::Up).await?;

        info!("✓ CoreDevice button '{}' dispatched successfully to {}", action, udid);
        Ok(())
    }
}
