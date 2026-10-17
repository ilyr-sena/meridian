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

use std::sync::Arc;

use base64::prelude::*;
use reqwest::Client;
use rootcause::prelude::*;
use serde::Deserialize;
use web_time::SystemTime;

use crate::anisette::{AnisetteClientInfo, AnisetteData, AnisetteProvider};
use crate::auth::grandslam::GrandSlam;

/// `User-Agent` the Python hub sends to `gsa.apple.com` (seasoned value Apple accepts).
const PYTHON_USER_AGENT: &str = "akd/1.0 CFNetwork/978.0.7 Darwin/18.7.0";

/// `X-MMe-Client-Info` value Apple's GSA accepts.
///
/// The omnisette machine description ends in `<...> <com.apple.AuthKit/1
/// (com.apple.dt.Xcode/3594.4.19)>`. Apple's WAF has flagged that exact string and
/// rejects it with `503 Service Temporarily Unavailable` on `GsService2`. Modern
/// clients (Sideloadly/AltServer/anisette-v3-server) use `com.apple.AuthKit/1
/// (com.apple.akd/1.0)` instead, which is accepted. Only the HTTP header value
/// matters; the cpd body does not need a client-info entry.
const AKD_CLIENT_INFO: &str =
    "<MacBookPro13,2> <macOS;13.1;22C65> <com.apple.AuthKit/1 (com.apple.akd/1.0)>";

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
            _device_description: AKD_CLIENT_INFO.to_string(),
            device_unique_identifier: uuid::Uuid::new_v4().to_string().to_uppercase(),
            _local_user_id: local_user_id,
            generated_at: SystemTime::now(),
        })
    }

    async fn get_client_info(&self) -> Result<AnisetteClientInfo, Report> {
        // The client-info string must be the `akd/1.0` variant; the omnisette
        // `Xcode/3594.4.19` machine description gets 503'd by Apple's WAF.
        Ok(AnisetteClientInfo {
            client_info: AKD_CLIENT_INFO.to_string(),
            user_agent: PYTHON_USER_AGENT.to_string(),
        })
    }

    fn needs_provisioning(&self) -> Result<bool, Report> {
        Ok(false)
    }

    async fn provision(&mut self, _gs: Arc<GrandSlam>) -> Result<(), Report> {
        Ok(())
    }
}