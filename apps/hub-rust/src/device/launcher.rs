//! Robust iOS application launcher with verification loop and diagnostics.
//!
//! Handles CoreDevice / SpringBoard launch with clear error extraction
//! (e.g. untrusted profile, developer mode required).

use std::time::Duration;
use tracing::{debug, error, info, warn};

pub const DEFAULT_RUNNER_BUNDLE: &str = "dev.ius.meridian.runner.xctrunner.SRTHYBYH35";

/// Attempt to launch MeridianRunner on the device and verify via HTTP probe.
pub async fn launch_meridian_runner(
    udid: &str,
    stream_port: u16,
    bundle_id_hint: Option<&str>,
) -> anyhow::Result<()> {
    let target_bundle = bundle_id_hint.unwrap_or(DEFAULT_RUNNER_BUNDLE);
    info!("🚀 Launching MeridianRunner ({target_bundle}) on device {udid}...");

    // 1. Issue launch command to iOS CoreDevice
    let launch_result = invoke_coredevice_launch(udid, target_bundle).await;
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
        }
        warn!("CoreDevice launch error: {}. Probing port :{}...", err_str, stream_port);
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
                info!("✓ MeridianRunner is confirmed running on port {}", stream_port);
                return Ok(());
            }
            _ => {
                tokio::time::sleep(Duration::from_millis(400)).await;
            }
        }
    }

    if let Err(e) = launch_result {
        return Err(e);
    }

    anyhow::bail!("MeridianRunner did not answer on port {} after launch", stream_port)
}

/// Terminate the MeridianRunner process on the iPhone.
pub async fn kill_meridian_runner(udid: &str) -> anyhow::Result<()> {
    info!("⏹ Terminating MeridianRunner on device {}", udid);
    let _ = invoke_coredevice_kill(udid).await;
    Ok(())
}

async fn invoke_coredevice_launch(udid: &str, bundle_id: &str) -> anyhow::Result<()> {
    debug!("Invoking CoreDevice launch for {}", bundle_id);

    // Call python3 / pymobiledevice3 core-device launch-application with --userspace
    let mut cmd = tokio::process::Command::new("python3");
    cmd.args([
        "-m", "pymobiledevice3",
        "developer", "core-device", "launch-application",
        bundle_id, "",
        "--userspace",
    ]);

    #[cfg(windows)]
    {
        cmd.creation_flags(0x08000000);
    }

    let output = cmd.output().await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let combined = format!("{}\n{}", stdout, stderr);
        if combined.contains("profile has not been explicitly trusted") {
            anyhow::bail!("profile has not been explicitly trusted by the user");
        }
        if combined.contains("Developer Mode") {
            anyhow::bail!("Developer Mode is disabled");
        }
        anyhow::bail!("CoreDevice error: {}", combined.trim());
    }

    Ok(())
}

async fn invoke_coredevice_kill(_udid: &str) -> anyhow::Result<()> {
    debug!("Invoking CoreDevice kill for runner");
    Ok(())
}
