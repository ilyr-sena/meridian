//! Simple remote anisette provider.
//!
//! Mirrors the proven Apple GrandSlam workaround used by the Python hub: a plain
//! `GET {url}` against a long-running omnisette-server returns a seasoned
//! (`X-Apple-I-MD` / `X-Apple-I-MD-M` / `X-Apple-I-MD-RINFO`) machine identity
//! that GSA accepts. We then override `X-Apple-I-MD-LU` with the base64 of the
//! Apple ID and `X-Mme-Device-Id` with a fresh UUID, exactly like Python's
//! `fetch_local_anisette_headers`.
//!
//! No websocket provisioning is performed; Apple rejects the freshly-provisioned
//! identity produced by the v3 flow with `503 Service Temporarily Unavailable`.
//!
//! 2026-10: Apple rotated the accepted GrandSlam client identity (error
//! `-80045 "This action cannot be completed at this time"` on login init).
//! This file now presents a Windows/iTunes akd identity — see the "GSA client
//! identity" block below, `docs/ANISETTE_GSA_80045_FIX.md`, and
//! `vendor/isideload-backup-2026-10-17/` for history, evidence and rollback.

use std::sync::Arc;

use base64::prelude::*;
use reqwest::Client;
use rootcause::prelude::*;
use serde::Deserialize;
use web_time::SystemTime;

use crate::anisette::{AnisetteClientInfo, AnisetteData, AnisetteProvider};
use crate::auth::grandslam::GrandSlam;

// ---------------------------------------------------------------------------
// GSA client identity — see docs/ANISETTE_GSA_80045_FIX.md for full history.
//
// Apple rotates which `X-Mme-Client-Info` / `User-Agent` pairs their GrandSlam
// gate accepts. Eras so far:
//
//   1. omnisette's raw machine description (`…Xcode/3594.4.19`) → hard
//      `503` by Apple's WAF. Only the HTTP header value matters; the cpd
//      body does not need a client-info entry. Fixed by switching to the
//      akd client-info variant (era-2 constants below — this fork's
//      original patch).
//   2. 2026-10: the MacBookPro/macOS akd pair began failing the login gate
//      with `ec=-80045 "This action cannot be completed at this time"`
//      (community-wide; anisette/Sideloadly users hit the same wall within
//      days). Verified by direct A/B test against GsService2 with a dummy
//      account + fresh OTP: era-2 pair → -80045 every time; the Windows/
//      iTunes pair below → gate + anisette accepted, Apple proceeded to
//      real credential validation (`-20970 "account information was
//      entered incorrectly"` for the nonexistent test account).
//
// The active identity MUST keep its client-info and User-Agent paired to the
// same platform: `CFNetwork/808.1.4` is the iTunes-era akd user agent on
// Windows. A mismatched pair (e.g. the Android client-info variant reported
// in the community, `<Pixel 7> <Android;14;UQ1A.240205.002>
// <com.apple.AuthKit/1 (com.apple.akd/1.0)>`, with any CFNetwork UA) gets
// 503'd by the WAF outright — verified empirically 2026-10.
// ---------------------------------------------------------------------------

/// Active GSA identity (accepted as of 2026-10): akd speaking for
/// iTunes 12.12.4.1 on Windows 6.2 (Server 2012 family).
const ITUNES_CLIENT_INFO: &str =
    "<PC> <Windows;6.2(0,0);9200> <com.apple.AuthKit/1 (com.apple.iTunes/12.12.4.1)>";
/// User-Agent paired with [`ITUNES_CLIENT_INFO`] (same platform, same era).
const ITUNES_USER_AGENT: &str = "akd/1.0 CFNetwork/808.1.4";

/// LEGACY (era 2, broken by -80045 since 2026-10) — the `X-MMe-Client-Info`
/// value that fixed the Xcode/3594.4.19 WAF 503 of era 1. Kept for reference
/// and instant rollback: point `get_client_info()` back at this +
/// [`PYTHON_USER_AGENT`] if the iTunes pair above gets rotated out too.
#[allow(dead_code)]
const AKD_CLIENT_INFO: &str =
    "<MacBookPro13,2> <macOS;13.1;22C65> <com.apple.AuthKit/1 (com.apple.akd/1.0)>";
/// LEGACY (era 2) — `User-Agent` the Python hub sent to `gsa.apple.com`
/// (the seasoned value that was accepted through era 2).
#[allow(dead_code)]
const PYTHON_USER_AGENT: &str = "akd/1.0 CFNetwork/978.0.7 Darwin/18.7.0";

/// Raw header JSON returned by an omnisette-server `GET /`.
#[derive(Deserialize)]
struct OmnisetteHeaders {
    #[serde(rename = "X-Apple-I-MD")]
    one_time_password: String,
    #[serde(rename = "X-Apple-I-MD-M")]
    machine_id: String,
    #[serde(rename = "X-Apple-I-MD-RINFO")]
    routing_info: String,
    #[serde(rename = "X-Apple-I-MD-LU")]
    local_user_id: String,
}

pub struct RemoteAnisetteProvider {
    url: String,
    client: Client,
    username: Option<String>,
}

impl RemoteAnisetteProvider {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.trim_end_matches('/').to_string(),
            client: Client::new(),
            username: None,
        }
    }

    /// Set the Apple ID email; used to fill `X-Apple-I-MD-LU` with base64 of the
    /// username (Apple's GSA 503 workaround).
    pub fn set_username(mut self, username: String) -> Self {
        self.username = Some(username.trim().to_lowercase());
        self
    }

    async fn fetch(&self) -> Result<OmnisetteHeaders, Report> {
        Ok(self
            .client
            .get(&self.url)
            .send()
            .await
            .context("Failed to reach omnisette server")?
            .error_for_status()
            .context("omnisette server returned an error")?
            .json::<OmnisetteHeaders>()
            .await
            .context("Failed to parse omnisette headers")?)
    }
}

#[cfg_attr(feature = "wasm", async_trait::async_trait(?Send))]
#[cfg_attr(not(feature = "wasm"), async_trait::async_trait)]
impl AnisetteProvider for RemoteAnisetteProvider {
    async fn get_anisette_data(&self) -> Result<AnisetteData, Report> {
        let headers = self.fetch().await?;

        let local_user_id = match &self.username {
            Some(u) => BASE64_STANDARD.encode(u.as_bytes()),
            // Fall back to the seasoned md_lu hash from omnisette when no username
            // is known.
            None => headers.local_user_id.clone(),
        };

        Ok(AnisetteData {
            machine_id: headers.machine_id,
            one_time_password: headers.one_time_password,
            routing_info: headers.routing_info,
            _device_description: ITUNES_CLIENT_INFO.to_string(),
            device_unique_identifier: uuid::Uuid::new_v4().to_string().to_uppercase(),
            _local_user_id: local_user_id,
            generated_at: SystemTime::now(),
        })
    }

    async fn get_client_info(&self) -> Result<AnisetteClientInfo, Report> {
        // Client identity presented to GSA. 2026-10: the macOS akd pair is
        // rejected with -80045 at the login gate; the Windows/iTunes pair is
        // the verified-working one (see the identity block above for
        // history, evidence and rollback).
        Ok(AnisetteClientInfo {
            client_info: ITUNES_CLIENT_INFO.to_string(),
            user_agent: ITUNES_USER_AGENT.to_string(),
        })
    }

    fn needs_provisioning(&self) -> Result<bool, Report> {
        Ok(false)
    }

    async fn provision(&mut self, _gs: Arc<GrandSlam>) -> Result<(), Report> {
        Ok(())
    }
}