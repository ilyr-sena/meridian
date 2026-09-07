//! 100% Pure Rust iOS application launcher with verification loop and diagnostics.
//!
//! Controls CoreDevice and DVT instruments protocols directly over usbmuxd
//! without any Python or external dependencies.

use std::time::Duration;
use tracing::{debug, info, warn};

use idevice::{
    IdeviceService,
    provider::UsbmuxdProvider,
    services::{
        core_device::AppServiceClient,
        core_device_proxy::CoreDeviceProxy,
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

/// Pure Rust CoreDevice / DVT application launcher
async fn invoke_native_launch(udid: &str, device_id: u32, bundle_id: &str) -> anyhow::Result<()> {
    debug!("Invoking native pure-Rust launch for {} on {} (ID: {})", bundle_id, udid, device_id);

    let provider = UsbmuxdProvider {
        addr: UsbmuxdAddr::default(),
        tag: 1,
        udid: udid.to_string(),
        device_id,
        label: "meridian-hub".to_string(),
    };

    // Primary: iOS 17+ CoreDeviceProxy over usbmuxd + in-process jktcp TCP stack
    match CoreDeviceProxy::connect(&provider).await {
        Ok(proxy) => {
            let rsd_port = proxy.tunnel_info().server_rsd_port;
            let adapter = proxy.create_software_tunnel()?;
            let mut handle = adapter.to_async_handle();

            let rsd_stream = handle.connect(rsd_port).await?;
            let rsd = RsdHandshake::new(rsd_stream).await?;

            let app_entry = rsd
                .services
                .get("com.apple.coredevice.appservice")
                .ok_or_else(|| anyhow::anyhow!("CoreDevice AppService not advertised on RSD"))?;

            let app_stream = handle.connect(app_entry.port).await?;
            let mut app_service = AppServiceClient::new(app_stream).await?;

            debug!("Connected to CoreDevice AppService via RSD on port {}. Launching...", app_entry.port);
            const EMPTY_ARGS: &[&'static str] = &[];
            let resp = app_service
                .launch_application(bundle_id, EMPTY_ARGS, true, false, None, None, None)
                .await?;

            info!("✓ Native CoreDevice launched {} with PID {}", bundle_id, resp.pid);
            return Ok(());
        }
        Err(e) => {
            debug!("CoreDeviceProxy connection failed ({:?})", e);
        }
    }

    anyhow::bail!("Failed to launch application {} via native iOS protocols", bundle_id);
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
                                            if url.relative.contains("meridian")
                                                || url.relative.contains("runner")
                                                || url.relative.contains(bundle_id)
                                            {
                                                let _ = app_service.send_signal(p.pid, 9).await;
                                                info!("✓ Sent SIGKILL to PID {}", p.pid);
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
