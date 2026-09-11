//! Pure-Rust Developer Disk Image (DDI) detection and mounting.
//!
//! iOS 17 and later only advertises `com.apple.coredevice.appservice` — the
//! CoreDevice service the hub uses to launch apps — while a Developer Disk
//! Image is mounted. A device reboot unmounts it, which previously surfaced in
//! the hub as `CoreDevice AppService not advertised on RSD`.
//!
//! This module:
//!   * detects whether a Developer/Personalized image is already mounted,
//!   * downloads the matching image payload (from the public `doronz88/
//!     DeveloperDiskImage` repository) into a local cache when missing,
//!   * performs the personalized TSS handshake and mounts via the device's
//!     MobileImageMounter service, and
//!   * self-heals a stale cached image by refreshing it and retrying once.
//!
//! No external tooling is required — everything runs over usbmuxd in pure Rust.

use std::path::{Path, PathBuf};
use std::time::Duration;

use idevice::provider::{IdeviceProvider, UsbmuxdProvider};
use idevice::services::lockdown::LockdownClient;
use idevice::services::mobile_image_mounter::ImageMounter;
use idevice::usbmuxd::UsbmuxdAddr;
use idevice::IdeviceService;
use tracing::{info, warn};

const DDI_REPO_BASE: &str = "https://raw.githubusercontent.com/doronz88/DeveloperDiskImage/main";
const PERSONALIZED_DIR: &str = "PersonalizedImages/Xcode_iOS_DDI_Personalized";

/// Check whether a Developer Disk Image is currently mounted on the device.
pub async fn is_developer_image_mounted(udid: &str, device_id: u32) -> anyhow::Result<bool> {
    let provider = provider(udid, device_id);
    is_developer_image_mounted_for(&provider).await
}

/// Ensure a Developer Disk Image is mounted, mounting it if necessary.
///
/// Required before apps can be launched via CoreDevice on iOS 17+. When an
/// image is already mounted this is a cheap check that returns immediately.
pub async fn ensure_developer_image_mounted(udid: &str, device_id: u32) -> anyhow::Result<()> {
    let provider = provider(udid, device_id);

    if is_developer_image_mounted_for(&provider).await.unwrap_or(false) {
        info!("Developer Disk Image already mounted for {udid}");
        return Ok(());
    }

    let version = get_product_version(&provider).await.unwrap_or_default();
    info!("No Developer Disk Image mounted for {udid} (iOS {version}); mounting...",);

    let major: u32 = version
        .split('.')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    if major >= 17 {
        // Developer Mode must be enabled before a personalized DDI can mount.
        if let Ok(developer_mode) = developer_mode_enabled(&provider).await {
            if !developer_mode {
                anyhow::bail!(
                    "Developer Mode is not enabled on this device.\n\
                     Open Settings -> Privacy & Security -> Developer Mode, enable it, and reboot."
                );
            }
        }
        mount_personalized(&provider).await
    } else {
        mount_classic(&provider, &version).await
    }
}

async fn developer_mode_enabled(provider: &UsbmuxdProvider) -> anyhow::Result<bool> {
    let mut mounter = ImageMounter::connect(provider).await?;
    Ok(mounter.query_developer_mode_status().await?)
}

fn provider(udid: &str, device_id: u32) -> UsbmuxdProvider {
    UsbmuxdProvider {
        addr: UsbmuxdAddr::default(),
        tag: 1,
        udid: udid.to_string(),
        device_id,
        label: "meridian-hub".to_string(),
    }
}

async fn is_developer_image_mounted_for(provider: &UsbmuxdProvider) -> anyhow::Result<bool> {
    let mut mounter = ImageMounter::connect(provider).await?;
    let devices = mounter.copy_devices().await?;
    Ok(devices.iter().any(is_developer_entry))
}

/// A `CopyDevices` entry represents a mounted Developer image either as the
/// classic `Developer` type (iOS < 17) or the `Personalized` type (iOS 17+).
fn is_developer_entry(value: &plist::Value) -> bool {
    let Some(dict) = value.as_dictionary() else {
        return false;
    };
    matches!(
        dict.get("DiskImageType").and_then(|v| v.as_string()),
        Some("Developer" | "Personalized")
    )
}

async fn get_product_version(provider: &UsbmuxdProvider) -> anyhow::Result<String> {
    let mut lockdown = LockdownClient::connect(provider).await?;
    let value = lockdown.get_value(Some("ProductVersion"), None).await?;
    value
        .as_string()
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("device did not return ProductVersion"))
}

/// Retrieve the device ECID, which is required to personalize the image.
async fn get_ecid(provider: &UsbmuxdProvider) -> anyhow::Result<u64> {
    let mut lockdown = LockdownClient::connect(provider).await?;

    if let Ok(value) = lockdown.get_value(Some("UniqueChipID"), None).await {
        if let Some(ecid) = value.as_unsigned_integer() {
            return Ok(ecid);
        }
    }

    // Some devices only expose the value after establishing a session.
    let pairing = provider.get_pairing_file().await?;
    lockdown.start_session(&pairing).await?;
    let value = lockdown.get_value(Some("UniqueChipID"), None).await?;
    value
        .as_unsigned_integer()
        .ok_or_else(|| anyhow::anyhow!("device did not return UniqueChipID (ECID)"))
}

struct PersonalizedDdi {
    image: Vec<u8>,
    build_manifest: Vec<u8>,
    trust_cache: Vec<u8>,
}

async fn mount_personalized(provider: &UsbmuxdProvider) -> anyhow::Result<()> {
    let ecid = get_ecid(provider).await?;

    match try_mount_personalized(provider, ecid).await {
        Ok(()) => {
            info!("✓ Personalized Developer Disk Image mounted for {}", provider.udid);
            Ok(())
        }
        Err(first) => {
            // A stale cached image (e.g. after an iOS update) fails to mount;
            // refresh it from the repository once and retry.
            warn!("Personalized DDI mount failed ({first}); refreshing cache and retrying",);
            let _ = tokio::fs::remove_dir_all(ddi_cache_dir()).await;
            try_mount_personalized(provider, ecid).await.map(|()| {
                info!("✓ Personalized Developer Disk Image mounted (after refresh) for {}", provider.udid);
            })
        }
    }
}

async fn try_mount_personalized(provider: &UsbmuxdProvider, ecid: u64) -> anyhow::Result<()> {
    let ddi = obtain_personalized_ddi().await?;
    let mut mounter = ImageMounter::connect(provider).await?;
    mounter
        .mount_personalized(provider, ddi.image, ddi.trust_cache, &ddi.build_manifest, None, ecid)
        .await?;
    Ok(())
}

async fn obtain_personalized_ddi() -> anyhow::Result<PersonalizedDdi> {
    let dir = ddi_cache_dir();
    std::fs::create_dir_all(&dir)?;

    let image_path = dir.join("Image.dmg");
    let manifest_path = dir.join("BuildManifest.plist");
    let trust_path = dir.join("Image.trustcache");

    if !image_path.exists() || !manifest_path.exists() || !trust_path.exists() {
        info!("Downloading Personalized Developer Disk Image (~15–20 MB)...");
        let base = format!("{DDI_REPO_BASE}/{PERSONALIZED_DIR}");
        download(&format!("{base}/Image.dmg"), &image_path).await?;
        download(&format!("{base}/BuildManifest.plist"), &manifest_path).await?;
        download(&format!("{base}/Image.dmg.trustcache"), &trust_path).await?;
    }

    Ok(PersonalizedDdi {
        image: tokio::fs::read(&image_path).await?,
        build_manifest: tokio::fs::read(&manifest_path).await?,
        trust_cache: tokio::fs::read(&trust_path).await?,
    })
}

async fn mount_classic(provider: &UsbmuxdProvider, version: &str) -> anyhow::Result<()> {
    let mut parts = version.split('.');
    let major = parts.next().unwrap_or_default();
    let minor = parts.next().unwrap_or("0");
    let short = format!("{major}.{minor}");

    let dir = ddi_cache_dir().join("legacy").join(&short);
    std::fs::create_dir_all(&dir)?;

    let image_path = dir.join("DeveloperDiskImage.dmg");
    let sig_path = dir.join("DeveloperDiskImage.dmg.signature");

    if !image_path.exists() || !sig_path.exists() {
        download(
            &format!("{DDI_REPO_BASE}/DeveloperDiskImages/{short}/DeveloperDiskImage.dmg"),
            &image_path,
        )
        .await?;
        download(
            &format!("{DDI_REPO_BASE}/DeveloperDiskImages/{short}/DeveloperDiskImage.dmg.signature"),
            &sig_path,
        )
        .await?;
    }

    let image = tokio::fs::read(&image_path).await?;
    let signature = tokio::fs::read(&sig_path).await?;
    let mut mounter = ImageMounter::connect(provider).await?;
    mounter.mount_developer(&image, signature).await?;
    info!("✓ Developer Disk Image mounted for {}", provider.udid);
    Ok(())
}

async fn download(url: &str, dest: &Path) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(180))
        .build()?;
    let response = client.get(url).send().await?.error_for_status()?;
    let bytes = response.bytes().await?;
    tokio::fs::write(dest, &bytes).await?;
    Ok(())
}

fn ddi_cache_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("meridian")
        .join("ddi")
}