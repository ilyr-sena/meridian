//! Fetch the latest unsigned MeridianRunner IPA from the Meridian VPS.
//!
//! The hub never asks the user for a local IPA — it always uses the freshest
//! `MeridianRunner-unsigned.ipa` published on the VPS. This module:
//!   1. reads `https://meridianhub.cc/runner/manifest.json`,
//!   2. downloads the IPA when the version changed,
//!   3. verifies SHA-256 + size,
//!   4. caches it locally for offline / repeat sideloads.

use std::path::PathBuf;

use futures::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tracing::{debug, info, warn};

/// Base URL where the VPS publishes the runner manifest + IPA (nginx `/runner/`).
pub const RUNNER_BASE_URL: &str = "https://meridianhub.cc/runner";

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
struct RunnerManifest {
    version: String,
    url: String,
    sha256: String,
    size: u64,
}

fn cache_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("meridian")
        .join("runners")
}

fn read_cached_manifest(path: &std::path::Path) -> Option<RunnerManifest> {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Ensure the latest runner IPA is cached and return its local path.
///
/// `progress` receives normalized (0.0..=0.5) updates; the remaining 0.5 is the
/// sign + install phase managed by the caller.
pub async fn fetch_or_update(
    progress: impl Fn(f32, &str) + Send + Sync,
) -> anyhow::Result<PathBuf> {
    let dir = cache_dir();
    std::fs::create_dir_all(&dir)?;
    let manifest_path = dir.join("manifest.json");
    let ipa_path = dir.join("MeridianRunner-unsigned.ipa");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    progress(0.0, "Fetching runner manifest from VPS...");
    let manifest_url = format!("{RUNNER_BASE_URL}/manifest.json");
    let remote = match client.get(&manifest_url).send().await {
        Ok(r) if r.status().is_success() => r.json::<RunnerManifest>().await.ok(),
        Ok(r) => {
            warn!("Runner manifest returned HTTP {}", r.status());
            None
        }
        Err(e) => {
            warn!("Runner manifest fetch failed: {e}");
            None
        }
    };

    if let Some(remote) = &remote {
        let cached = read_cached_manifest(&manifest_path);
        let uptodate = cached
            .as_ref()
            .map(|c| c.version == remote.version)
            .unwrap_or(false);
        if uptodate && ipa_path.exists() {
            debug!("Runner v{} already cached", remote.version);
            progress(0.5, format!("Runner v{} already downloaded", remote.version).as_str());
            return Ok(ipa_path);
        }

        // Fresh version → stream it down, verifying as we go.
        progress(
            0.02,
            format!("Downloading MeridianRunner v{}...", remote.version).as_str(),
        );
        let resp = client.get(&remote.url).send().await?.error_for_status()?;
        let total = resp.content_length().unwrap_or(remote.size).max(1);
        let mut stream = resp.bytes_stream();

        let partial = dir.join("MeridianRunner-unsigned.ipa.partial");
        let mut out = tokio::fs::File::create(&partial).await?;
        let mut hasher = Sha256::new();
        let mut got: u64 = 0;

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            hasher.update(&chunk);
            out.write_all(&chunk).await?;
            got += chunk.len() as u64;
            let p = 0.02 + 0.44 * (got as f32 / total as f32).min(1.0);
            progress(
                p,
                format!(
                    "Downloading v{} — {:.1} MB / {} MB",
                    remote.version,
                    got as f64 / 1_048_576.0,
                    total as f64 / 1_048_576.0,
                )
                .as_str(),
            );
        }
        out.flush().await?;

        let digest = hex(&hasher.finalize());
        if !digest.eq_ignore_ascii_case(&remote.sha256) {
            let _ = tokio::fs::remove_file(&partial).await;
            anyhow::bail!(
                "Runner IPA hash mismatch:\n  got      {digest}\n  expected {}",
                remote.sha256
            );
        }
        if got != remote.size {
            let _ = tokio::fs::remove_file(&partial).await;
            anyhow::bail!("Runner IPA size mismatch: got {got} bytes, expected {}", remote.size);
        }
        tokio::fs::rename(&partial, &ipa_path).await?;
        tokio::fs::write(&manifest_path, serde_json::to_vec(remote)?).await?;

        info!("✓ Runner v{} downloaded & verified ({})", remote.version, ipa_path.display());
        progress(0.5, format!("Runner v{} ready", remote.version).as_str());
        return Ok(ipa_path);
    }

    // VPS unreachable — fall back to a cached runner if we have one.
    if ipa_path.exists() {
        warn!("VPS manifest unreachable; using cached runner IPA");
        progress(0.5, "VPS unreachable — using cached Runner");
        return Ok(ipa_path);
    }

    anyhow::bail!(
        "Could not reach the Meridian VPS ({RUNNER_BASE_URL}) and no cached Runner IPA is available."
    )
}