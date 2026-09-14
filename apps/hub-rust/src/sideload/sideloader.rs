//! Pure Rust sideloading: authenticate with the Apple ID, retrieve/create a
//! development certificate, sign the unsigned IPA, and install it over USB —
//! no Python or external `zsign` dependency.
//!
//! 2FA is handled by prompting the UI through a global channel.

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

#[derive(Debug, Clone)]
pub struct SideloadOptions {
    pub ipa_path: PathBuf,
    pub apple_id: String,
    pub password: String,
    pub anisette_url: String,
    pub udid: String,
    pub device_id: u32,
}

pub struct Sideloader;

impl Sideloader {
    /// Open native OS file dialog to select an IPA file.
    pub async fn pick_ipa_file() -> Option<PathBuf> {
        let file = rfd::AsyncFileDialog::new()
            .set_title("Select MeridianRunner IPA")
            .add_filter("iOS App Package (*.ipa)", &["ipa"])
            .pick_file()
            .await;

        file.map(|f| f.path().to_path_buf())
    }

    /// Authenticate, sign and install the IPA in pure Rust.
    pub async fn execute_sideload(
        opts: SideloadOptions,
        progress_callback: impl Fn(f32, &str) + Send + Sync + 'static,
    ) -> anyhow::Result<()> {
        if !opts.ipa_path.exists() {
            anyhow::bail!("Selected IPA file does not exist: {:?}", opts.ipa_path);
        }

        // --- persistent storage for anisette state, certs and profiles ---
        let storage_root = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("meridian")
            .join("isideload");
        let sideloader_storage = FsStorage::new(storage_root.join("sideloader"));

        progress_callback(0.03, "Authenticating with Apple ID...");

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

        progress_callback(0.15, "Provisioning certificate & profile...");

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
                cb(0.15 + pct * 0.60, "Signing application...");
            }
        };

        sideloader
            .install_app(&provider, opts.ipa_path.clone(), false, Some(isideload_progress))
            .await
            .map_err(|e| anyhow::anyhow!("sideload failed: {e:#}"))?;

        warn!("native signing completed for {}", opts.udid);

        cb(1.0, "Installed!");
        info!("✓ Pure-Rust sideload (signed + installed) successful for {}", opts.udid);

        Ok(())
    }
}