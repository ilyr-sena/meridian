//! 100% Pure Rust iOS application launcher with verification loop and diagnostics.
//!
//! MeridianRunner is an XCTest UI runner (.xctrunner), so launching it is NOT a
//! plain CoreDevice app launch: the runner's UI-automation init connects to
//! `com.apple.testmanagerd`, which only answers once testmanagerd is running —
//! and testmanagerd only launches when an IDE-style session connects to its
//! RemoteXPC service (`com.apple.dt.testmanagerd.remote`) over the CoreDevice
//! RSD tunnel. A plain launch therefore SIGABRTs at startup with
//! "Failed to initiate daemon session ... No such process".
//!
//! This module drives the full testmanagerd orchestration (via the vendored
//! idevice `dvt::xctest` flow) and waits until WDA answers on its device-side
//! HTTP port. It also keeps the Developer Disk Image mounted: the runner
//! links XCTest from /System/Developer, so an unmounted DDI fails earlier at
//! dyld load time ("Library missing").

use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

use idevice::{
    IdeviceService,
    provider::UsbmuxdProvider,
    services::{
        core_device::AppServiceClient,
        core_device_proxy::CoreDeviceProxy,
        dvt::xctest::{TestConfig, XCUITestService},
        installation_proxy::InstallationProxyClient,
        rsd::RsdHandshake,
    },
    usbmuxd::UsbmuxdAddr,
};

pub const DEFAULT_RUNNER_BUNDLE: &str = "dev.ius.meridian.runner.xctrunner.SRTHYBYH35";

/// Attempt to launch MeridianRunner on the device and verify via HTTP probe.
pub async fn launch_meridian_runner(
    udid: String,
    device_id: u32,
    stream_port: u16,
    bundle_id_hint: Option<String>,
) -> anyhow::Result<()> {
    tokio::spawn(async move {
        launch_meridian_runner_inner(udid, device_id, stream_port, bundle_id_hint).await
    })
    .await
    .map_err(|e| anyhow::anyhow!("Task join error: {}", e))?
}

async fn launch_meridian_runner_inner(
    udid: String,
    device_id: u32,
    stream_port: u16,
    bundle_id_hint: Option<String>,
) -> anyhow::Result<()> {
    let target_bundle = bundle_id_hint.unwrap_or_else(|| DEFAULT_RUNNER_BUNDLE.to_string());
    info!("🚀 Launching MeridianRunner ({target_bundle}) on device {udid} (device_id: {device_id})...");

    // 0. Ensure the Developer Disk Image is mounted. The runner links XCTest
    //    from /System/Developer, so without it the launch fails at dyld load
    //    time ("Library missing") before main() is ever reached.
    if let Err(e) = crate::device::ddi::ensure_developer_image_mounted(&udid, device_id).await {
        anyhow::bail!("Developer Disk Image unavailable: {e}");
    }

    // 1. Issue launch command to iOS CoreDevice in pure Rust
    let launch_result = invoke_native_launch(&udid, device_id, &target_bundle).await;
    if let Err(ref e) = launch_result {
        let err_str = e.to_string();
        if err_str.contains("profile has not been explicitly trusted") {
            anyhow::bail!(
                "Developer Profile Untrusted!\nOpen iPhone Settings -> General -> VPN & Device Management and tap 'Trust'"
            );
        } else if err_str.contains("Developer Mode") {
            anyhow::bail!(
                "Developer Mode Disabled!\nOpen iPhone Settings -> Privacy & Security -> Developer Mode and turn it On"
            );
        } else if err_str.contains("not installed") {
            anyhow::bail!(
                "Runner application not installed!\nPlease sideload MeridianRunner first."
            );
        }
        warn!("CoreDevice launch message: {}. Probing port :{}...", err_str, stream_port);
    }

    // 2. Verification Probe: check http://127.0.0.1:{stream_port}/status
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()?;

    let status_url = format!("http://127.0.0.1:{}/status", stream_port);
    let start_time = std::time::Instant::now();

    while start_time.elapsed() < Duration::from_secs(5) {
        match client.get(&status_url).send().await {
            Ok(resp) if resp.status().is_success() => {
                info!("✓ MeridianRunner is confirmed running and responding on port {}", stream_port);
                return Ok(());
            }
            _ => {
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
        }
    }

    if let Err(e) = launch_result {
        return Err(e);
    }

    anyhow::bail!("MeridianRunner did not answer on port {} after launch", stream_port)
}

/// Terminate the MeridianRunner process on the iPhone.
pub async fn kill_meridian_runner(
    udid: String,
    device_id: u32,
    bundle_id_hint: Option<String>,
) -> anyhow::Result<()> {
    tokio::spawn(async move {
        kill_meridian_runner_inner(udid, device_id, bundle_id_hint).await
    })
    .await
    .map_err(|e| anyhow::anyhow!("Task join error: {}", e))?
}

async fn kill_meridian_runner_inner(
    udid: String,
    device_id: u32,
    bundle_id_hint: Option<String>,
) -> anyhow::Result<()> {
    let target_bundle = bundle_id_hint.unwrap_or_else(|| DEFAULT_RUNNER_BUNDLE.to_string());
    info!("⏹ Terminating MeridianRunner ({}) on device {}", target_bundle, udid);
    let _ = invoke_native_kill(&udid, device_id, &target_bundle).await;
    Ok(())
}

/// Pure Rust testmanagerd launch: establishes the IDE-style session over the
/// CoreDevice RSD tunnel (which starts testmanagerd on demand), launches the
/// runner inside a real XCTest session and waits until WDA answers on its
/// device-side HTTP port. A plain CoreDevice app launch is NOT sufficient —
/// the runner's UI-automation init aborts without a live testmanagerd daemon.
async fn invoke_native_launch(udid: &str, device_id: u32, bundle_id: &str) -> anyhow::Result<()> {
    debug!(
        "Starting xctrunner test-session launch for {} on {} (ID: {})",
        bundle_id, udid, device_id
    );

    let provider = UsbmuxdProvider {
        addr: UsbmuxdAddr::default(),
        tag: 1,
        udid: udid.to_string(),
        device_id,
        label: "meridian-hub".to_string(),
    };

    // Runner bundle info (on-device paths + executable) from installation proxy.
    let mut install = InstallationProxyClient::connect(&provider).await?;
    let cfg = TestConfig::from_installation_proxy(&mut install, bundle_id, None).await?;

    // Full testmanagerd orchestration (connect -> session -> launch -> test
    // plan). WDA readiness is polled on the device-side HTTP port; the
    // orchestration task keeps running detached so the runner stays alive
    // after this call returns.
    let service = XCUITestService::new(Arc::new(provider));
    service
        .run_until_wda_ready(cfg, Duration::from_secs(180))
        .await?;

    info!("✓ XCTest session established, WDA ready for {bundle_id}");
    Ok(())
}

/// Pure Rust CoreDevice application terminator
async fn invoke_native_kill(udid: &str, device_id: u32, bundle_id: &str) -> anyhow::Result<()> {
    debug!("Invoking native kill for {} on {}", bundle_id, udid);

    let provider = UsbmuxdProvider {
        addr: UsbmuxdAddr::default(),
        tag: 1,
        udid: udid.to_string(),
        device_id,
        label: "meridian-hub".to_string(),
    };

    if let Ok(proxy) = CoreDeviceProxy::connect(&provider).await {
        let rsd_port = proxy.tunnel_info().server_rsd_port;
        if let Ok(adapter) = proxy.create_software_tunnel() {
            let mut handle = adapter.to_async_handle();
            if let Ok(rsd_stream) = handle.connect(rsd_port).await {
                if let Ok(rsd) = RsdHandshake::new(rsd_stream).await {
                    if let Some(app_entry) = rsd.services.get("com.apple.coredevice.appservice") {
                        if let Ok(app_stream) = handle.connect(app_entry.port).await {
                            if let Ok(mut app_service) = AppServiceClient::new(app_stream).await {
                                if let Ok(procs) = app_service.list_processes().await {
                                    for p in procs {
                                        if let Some(ref url) = p.executable_url {
                                            let lower = url.relative.to_lowercase();
                                            if lower.contains("runner")
                                                || lower.contains("meridian")
                                                || lower.contains("webdriver")
                                                || lower.contains(&bundle_id.to_lowercase())
                                            {
                                                let _ = app_service.send_signal(p.pid, 9).await;
                                                info!("✓ Sent SIGKILL to runner PID: {} ({})", p.pid, url.relative);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
