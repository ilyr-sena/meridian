//! Pure Rust sideloading: fetch the latest runner IPA from the Meridian VPS,
//! authenticate with the Apple ID, retrieve/create a development certificate,
//! sign the IPA, and install it over USB — no local file picker, no Python, no
//! external `zsign` dependency.
//!
//! 2FA is handled by prompting the UI through a global channel; progress is
//! streamed to the UI through a second global channel.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use tokio::sync::mpsc;
use tracing::{info, warn};

use idevice::provider::UsbmuxdProvider;
use idevice::usbmuxd::UsbmuxdAddr;
use isideload::anisette::remote::RemoteAnisetteProvider;
use isideload::auth::apple_account::{
    AppleAccount, TwoFactorCallbackParams, TwoFactorCallbackResponse,
};
use isideload::dev::developer_session::DeveloperSession;
use isideload::sideload::builder::{MaxCertsBehavior, SideloaderBuilder};
use isideload::util::fs_storage::FsStorage;

use super::runner_ipa;

/// A pending 2FA request: a hint to display plus a slot the UI fills with the
/// user's 6-digit code. `Arc<Mutex<>>` so it is cheaply `Clone`-able into a
/// `Message`.
pub type TwoFactorPrompt = (String, Arc<Mutex<Option<String>>>);

static TWO_FACTOR_TX: OnceLock<mpsc::UnboundedSender<TwoFactorPrompt>> = OnceLock::new();
static TWO_FACTOR_RX: OnceLock<tokio::sync::Mutex<Option<mpsc::UnboundedReceiver<TwoFactorPrompt>>>> =
    OnceLock::new();

/// Sender used by the login callback to ask the UI for a 2FA code.
pub fn two_factor_tx() -> &'static mpsc::UnboundedSender<TwoFactorPrompt> {
    TWO_FACTOR_TX.get_or_init(|| {
        let (tx, rx) = mpsc::unbounded_channel();
        TWO_FACTOR_RX.get_or_init(|| tokio::sync::Mutex::new(Some(rx)));
        tx
    })
}

/// Await the next 2FA request (for the UI subscription to drain).
pub async fn next_two_factor_prompt() -> Option<TwoFactorPrompt> {
    let rx = TWO_FACTOR_RX.get()?;
    let mut guard = rx.lock().await;
    guard.as_mut()?.recv().await
}

/// A progress update: normalized 0.0..=1.0 plus a human-readable status.
pub type SideloadProgressUpdate = (f32, String);

static PROGRESS_TX: OnceLock<mpsc::UnboundedSender<SideloadProgressUpdate>> = OnceLock::new();
static PROGRESS_RX: OnceLock<tokio::sync::Mutex<Option<mpsc::UnboundedReceiver<SideloadProgressUpdate>>>> =
    OnceLock::new();

/// Sender the sideload task uses to stream progress to the UI.
pub fn sideload_progress_tx() -> &'static mpsc::UnboundedSender<SideloadProgressUpdate> {
    PROGRESS_TX.get_or_init(|| {
        let (tx, rx) = mpsc::unbounded_channel();
        PROGRESS_RX.get_or_init(|| tokio::sync::Mutex::new(Some(rx)));
        tx
    })
}

/// Await the next progress update (for the UI subscription to drain).
pub async fn next_sideload_progress() -> Option<SideloadProgressUpdate> {
    let rx = PROGRESS_RX.get()?;
    let mut guard = rx.lock().await;
    guard.as_mut()?.recv().await
}

#[derive(Debug, Clone)]
pub struct SideloadOptions {
    /// Apple ID email. Saved to the vault on success and reused afterwards.
    pub apple_id: String,
    /// Password / app-specific password. May be empty when a valid password is
    /// already stored in the vault (the caller resolves the fallback).
    pub password: String,
    pub anisette_url: String,
    pub udid: String,
    pub device_id: u32,
}

pub struct Sideloader;

impl Sideloader {
    /// Fetch the latest runner IPA from the VPS, authenticate, sign and install it.
    pub async fn execute_sideload(
        opts: SideloadOptions,
        progress_callback: impl Fn(f32, &str) + Send + Sync + 'static,
    ) -> anyhow::Result<()> {
        // --- 1. Latest unsigned IPA from the VPS (no local file picker) ---
        let ipa_path = runner_ipa::fetch_or_update(|p, s| progress_callback(p, s)).await?;

        // --- persistent storage for anisette state, certs and profiles ---
        let storage_root = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("meridian")
            .join("isideload");
        let sideloader_storage = FsStorage::new(storage_root.join("sideloader"));

        progress_callback(0.5, "Authenticating with Apple ID...");

        // --- anisette provider (seasons omnisette-server, GET / headers) ---
        let anisette = RemoteAnisetteProvider::new(&opts.anisette_url)
            .set_username(opts.apple_id.clone());

        // --- login (with 2FA callback that prompts the UI) ---
        let prompt_tx = two_factor_tx().clone();
        let two_factor_cb = move |params: TwoFactorCallbackParams| {
            let tx = prompt_tx.clone();
            async move {
                if params.unknown {
                    // No known method yet — request the trusted-device push.
                    return Ok(TwoFactorCallbackResponse::SendToDevices);
                }
                let hint = if params.sms {
                    "2FA: enter the SMS code sent to your phone".to_string()
                } else {
                    "2FA: enter the code shown on your trusted Apple devices".to_string()
                };
                let slot = Arc::new(Mutex::new(None));
                let _ = tx.send((hint, slot.clone()));
                // Poll for the code (up to ~3 minutes).
                for _ in 0..360 {
                    if let Some(code) = slot.lock().unwrap().clone() {
                        return Ok(TwoFactorCallbackResponse::SubmitCode(code));
                    }
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
                Ok(TwoFactorCallbackResponse::Abort)
            }
        };

        let mut account = AppleAccount::builder(&opts.apple_id)
            .anisette_provider(anisette)
            .login(&opts.password, two_factor_cb)
            .await
            .map_err(|e| anyhow::anyhow!("Apple ID login failed: {e:#}"))?;

        info!("Apple ID authenticated");

        // --- developer session (Xcode auth token) ---
        let session = DeveloperSession::from_account(&mut account)
            .await
            .map_err(|e| anyhow::anyhow!("developer session failed: {e:#}"))?;

        // --- build the signer/installer ---
        let mut sideloader = SideloaderBuilder::new(session, opts.apple_id.clone())
            .machine_name("meridian-hub".to_string())
            .max_certs_behavior(MaxCertsBehavior::Revoke)
            .storage(Box::new(sideloader_storage))
            .build();

        progress_callback(0.55, "Provisioning certificate & profile...");

        let provider = UsbmuxdProvider {
            addr: UsbmuxdAddr::default(),
            tag: 1,
            udid: opts.udid.clone(),
            device_id: opts.device_id,
            label: "meridian-hub".to_string(),
        };

        let cb = Arc::new(progress_callback);
        let cb_progress = cb.clone();
        let isideload_progress = move |pct: f32| {
            let cb = cb_progress.clone();
            async move {
                cb(0.55 + pct * 0.44, "Signing application...");
            }
        };

        sideloader
            .install_app(&provider, ipa_path, false, Some(isideload_progress))
            .await
            .map_err(|e| anyhow::anyhow!("sideload failed: {e:#}"))?;

        warn!("native signing completed for {}", opts.udid);

        cb(1.0, "Installed!");
        info!("✓ Pure-Rust sideload (signed + installed) successful for {}", opts.udid);

        Ok(())
    }
}