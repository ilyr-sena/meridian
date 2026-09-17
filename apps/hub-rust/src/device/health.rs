//! Real-time device health probing and combinable state interpretation.
//!
//! The hub must reflect on-device reality without a re-launch: the runner app
//! being installed/uninstalled, Developer Mode toggled, the screen locked/
//! unlocked, or the trust state changing all have to surface automatically.
//! [`probe_health`] gathers each dimension as an independent tri-state, and
//! [`health_state`] / [`status_message`] render every combination coherently.

use std::time::Duration;

use idevice::{
    IdeviceService,
    provider::{IdeviceProvider, UsbmuxdProvider},
    services::{
        installation_proxy::InstallationProxyClient,
        lockdown::LockdownClient,
        mobile_image_mounter::ImageMounter,
    },
    usbmuxd::UsbmuxdAddr,
};
use tokio::time::timeout;
use tracing::debug;

use crate::device::models::{DeviceHealth, DeviceState, TriState};

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

fn provider(udid: &str, device_id: u32) -> UsbmuxdProvider {
    UsbmuxdProvider {
        addr: UsbmuxdAddr::default(),
        tag: 1,
        udid: udid.to_string(),
        device_id,
        label: "meridian-hub".to_string(),
    }
}

/// Parse the major OS version out of strings like `"iOS 27.0"` / `"27.0"`.
pub fn os_major(os_version: &str) -> u32 {
    let mut num = String::new();
    for c in os_version.chars() {
        if c.is_ascii_digit() {
            num.push(c);
        } else if !num.is_empty() {
            break;
        }
    }
    num.parse().unwrap_or(0)
}

/// Returns `true` when an error message (lower-cased) indicates a locked
/// device — used to classify probe failures without inventing signals.
fn looks_locked(msg: &str) -> bool {
    let l = msg.to_lowercase();
    l.contains("lock")
        || l.contains("passcode")
        || l.contains("password")
        || l.contains("unlock")
}

/// Probe every combinator on one device and return the assembled health.
///
/// Ordering matters: pairing is local (no device round-trip), the locked check
/// is the cheapest service probe, and the other two follow only when useful.
/// Every probe is time-bounded so a wedged device can never hang the poller.
pub async fn probe_health(udid: &str, device_id: u32, os_major: u32) -> DeviceHealth {
    let provider = provider(udid, device_id);

    // 1. Pairing record — a local read; when absent the device was never
    //    trusted and nothing else will work.
    let paired = match timeout(PROBE_TIMEOUT, provider.get_pairing_file()).await {
        Ok(Ok(_)) => TriState::Yes,
        _ => TriState::No,
    };

    if paired.is_no() {
        return DeviceHealth {
            paired: TriState::No,
            ..Default::default()
        };
    }

    // 2. Screen lock.
    let mut locked = probe_locked(&provider).await;

    // 3. Runner installed (independent check — a responsive device proves it is
    //    not locked, so it also back-fills the lock signal when ambiguous).
    let runner_installed = probe_runner_installed(&provider).await;
    if locked.is_unknown() && !runner_installed.is_unknown() {
        locked = TriState::No; // get_apps succeeded → device is awake & unlocked
        debug!("{udid}: services reachable, marking unlocked");
    }

    // 4. Developer Mode — only an iOS 17+ concept; on older iOS the classic
    //    DDI path has no such toggle. The query is unreliable while locked.
    let developer_mode = if os_major >= 17 {
        if locked.is_yes() {
            TriState::Unknown
        } else {
            probe_developer_mode(&provider).await
        }
    } else {
        TriState::Yes
    };

    DeviceHealth {
        runner_installed,
        developer_mode,
        locked,
        paired: TriState::Yes,
    }
}

/// Locked screen detection: a lockdown session tells us the device answers; a
/// lockdown session on a passcode-locked unit fails. We keep `Unknown` unless
/// the error itself names the lock, and let other probes downgrade it after.
async fn probe_locked(provider: &UsbmuxdProvider) -> TriState {
    match timeout(PROBE_TIMEOUT, async {
        let mut lockdown = LockdownClient::connect(provider).await?;
        let pairing = provider.get_pairing_file().await?;
        lockdown.start_session(&pairing).await?;
        Ok::<(), anyhow::Error>(())
    })
    .await
    {
        Ok(Ok(())) => TriState::No,
        Ok(Err(e)) => {
            let msg = format!("{e:#}");
            if looks_locked(&msg) {
                TriState::Yes
            } else {
                TriState::Unknown
            }
        }
        Err(_) => TriState::Unknown,
    }
}

/// Whether the MeridianRunner bundle is present among user apps.
async fn probe_runner_installed(provider: &UsbmuxdProvider) -> TriState {
    let connect = timeout(PROBE_TIMEOUT, InstallationProxyClient::connect(provider)).await;
    let mut client = match connect {
        Ok(Ok(c)) => c,
        Ok(Err(_)) | Err(_) => return TriState::Unknown,
    };

    match timeout(PROBE_TIMEOUT, client.get_apps(Some("User"), None)).await {
        Ok(Ok(apps)) => {
            let found = apps.keys().any(|bid| {
                let l = bid.to_lowercase();
                l.contains("meridian") || l.contains("runner") || l.contains("xctrunner")
            });
            if found {
                TriState::Yes
            } else {
                TriState::No
            }
        }
        Ok(Err(e)) => {
            let msg = format!("{e:#}");
            if looks_locked(&msg) {
                debug!("runner probe: device appears locked: {msg}");
            }
            TriState::Unknown
        }
        Err(_) => TriState::Unknown,
    }
}

/// Query the Developer Mode toggle (iOS 17+).
async fn probe_developer_mode(provider: &UsbmuxdProvider) -> TriState {
    let connect = timeout(PROBE_TIMEOUT, ImageMounter::connect(provider)).await;
    let mut mounter = match connect {
        Ok(Ok(m)) => m,
        Ok(Err(_)) | Err(_) => return TriState::Unknown,
    };

    match timeout(PROBE_TIMEOUT, mounter.query_developer_mode_status()).await {
        Ok(Ok(true)) => TriState::Yes,
        Ok(Ok(false)) => TriState::No,
        Ok(Err(_)) => TriState::Unknown,
        Err(_) => TriState::Unknown,
    }
}

/// Derive the "capability" device state from the health flags alone.
/// Priority (highest blocker first): trust → lock → sideload → developer mode →
/// ready. `Unknown` is never reported as a concrete problem — it degrades to
/// `Connected` (still diagnosing), which disables actions, never enables them.
pub fn health_state(health: &DeviceHealth) -> DeviceState {
    if health.paired.is_no() {
        return DeviceState::Unpaired;
    }
    if health.locked.is_yes() {
        return DeviceState::Locked;
    }
    if health.runner_installed.is_no() {
        return DeviceState::NeedsSideload;
    }
    if !health.runner_installed.is_yes() {
        return DeviceState::Connected; // still diagnosing
    }
    if health.developer_mode.is_no() {
        return DeviceState::DeveloperModeOff;
    }
    if !health.developer_mode.is_yes() {
        return DeviceState::Connected;
    }
    DeviceState::Ready
}

/// Human-readable, actionable multi-line message describing every condition at
/// once — whatever the combination. Empty message ⇒ ready to stream.
pub fn status_message(health: &DeviceHealth) -> String {
    let mut lines = Vec::new();
    if health.paired.is_no() {
        lines.push("Trust this computer: unlock the iPhone and tap \"Trust\".");
    }
    if health.locked.is_yes() {
        lines.push("Unlock the iPhone to continue (enter your passcode).");
    }
    match health.runner_installed {
        TriState::No => lines.push(
            "MeridianRunner is not installed — click \"Sideload Runner\" to fetch the latest build from the VPS and install it.",
        ),
        TriState::Unknown => lines.push("Checking whether MeridianRunner is installed..."),
        TriState::Yes => {}
    }
    match health.developer_mode {
        TriState::No => lines.push(
            "Developer Mode is OFF — enable it in Settings → Privacy & Security → Developer Mode, then reboot the iPhone.",
        ),
        TriState::Unknown => lines.push("Checking Developer Mode..."),
        TriState::Yes => {}
    }
    if lines.is_empty() {
        "Ready to stream".to_string()
    } else {
        lines.join("\n")
    }
}