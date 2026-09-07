//! Remote Tailscale Auth Key Fetcher.
//!
//! Automatically retrieves the active Tailscale AuthKey from a remote URL
//! so expiration (90 days) can be resolved by updating the remote file/API
//! without recompiling the Meridian Hub.

use std::time::Duration;
use tracing::{debug, info, warn};
use crate::core::vault::Vault;

pub const DEFAULT_KEY_URL: &str = "https://meridianhub.cc/api/mesh/authkey";

#[derive(Debug, Clone)]
pub struct KeyFetcher {
    vault: Vault,
}

impl KeyFetcher {
    pub fn new(vault: Vault) -> Self {
        Self { vault }
    }

    /// Fetch the active Tailscale auth key from vault override or remote web endpoint.
    pub async fn fetch_active_key(&self) -> Option<String> {
        let data = self.vault.load();

        // 1. Check for manual override directly in vault
        if let Some(ref key) = data.tailscale_auth_key {
            let trimmed = key.trim();
            if !trimmed.is_empty() && trimmed.starts_with("tskey-auth-") {
                info!("Using manual Tailscale auth key from local settings");
                return Some(trimmed.to_string());
            }
        }

        // 2. Fetch from configured or default remote URL
        let url = data.tailscale_key_url
            .clone()
            .unwrap_or_else(|| DEFAULT_KEY_URL.to_string());

        info!("Fetching dynamic Tailscale auth key from {} ...", url);

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .build()
            .ok()?;

        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(text) = resp.text().await {
                    let cleaned = text.trim().to_string();
                    if cleaned.starts_with("tskey-auth-") {
                        info!("✓ Successfully fetched Tailscale auth key from remote endpoint");
                        let mut data_copy = data.clone();
                        data_copy.tailscale_auth_key = Some(cleaned.clone());
                        let _ = self.vault.save(&data_copy);
                        return Some(cleaned);
                    } else if let Ok(json) = serde_json::from_str::<serde_json::Value>(&cleaned) {
                        if let Some(key) = json.get("authkey").or_else(|| json.get("key")).and_then(|v| v.as_str()) {
                            if key.starts_with("tskey-auth-") {
                                info!("✓ Successfully parsed Tailscale auth key from JSON response");
                                let mut data_copy = data.clone();
                                data_copy.tailscale_auth_key = Some(key.to_string());
                                let _ = self.vault.save(&data_copy);
                                return Some(key.to_string());
                            }
                        }
                    }
                    warn!("Remote endpoint returned non-authkey payload: {}", cleaned);
                }
            }
            Ok(resp) => {
                warn!("Remote key fetch returned HTTP {}: {}", resp.status(), url);
            }
            Err(e) => {
                debug!("Failed to reach remote key endpoint {}: {:?}", url, e);
            }
        }

        None
    }
}
