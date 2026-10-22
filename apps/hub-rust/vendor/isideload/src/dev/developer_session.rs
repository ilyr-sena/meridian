use std::sync::Arc;

use plist::Dictionary;
use plist_macro::{plist, plist_to_xml_string};
use reqwest::header::{HeaderMap, HeaderValue};
use rootcause::prelude::*;
use serde::de::DeserializeOwned;
use tracing::{error, warn};
use uuid::Uuid;

use crate::{
    SideloadError,
    anisette::AnisetteDataGenerator,
    auth::{
        apple_account::{AppToken, AppleAccount},
        grandslam::GrandSlam,
    },
    util::plist::PlistDataExtract,
};

pub use super::app_groups::*;
pub use super::app_ids::*;
pub use super::certificates::*;
pub use super::device_type::DeveloperDeviceType;
pub use super::devices::*;
pub use super::teams::*;

#[derive(Clone)]
pub struct DeveloperSession {
    token: AppToken,
    adsid: String,
    client: Arc<GrandSlam>,
    anisette_generator: AnisetteDataGenerator,
}

impl DeveloperSession {
    pub fn new(
        token: AppToken,
        adsid: String,
        client: Arc<GrandSlam>,
        anisette_generator: AnisetteDataGenerator,
    ) -> Self {
        DeveloperSession {
            token,
            adsid,
            client,
            anisette_generator,
        }
    }

    pub async fn from_account(account: &mut AppleAccount) -> Result<Self, Report> {
        let token = account
            .get_app_token("xcode.auth")
            .await
            .context("Failed to get xcode token from Apple account")?;

        let spd = account
            .spd
            .as_ref()
            .ok_or_else(|| report!("SPD not available, cannot get adsid"))?;

        // The ESX (developer services) requests must present the same
        // client identity the app token was minted under — the akd macOS
        // identity from `get_app_token_client_info`. Sending them with the
        // account's login client (Windows/iTunes) is rejected with
        // `Developer error 1100: Your session has expired. Please log in.`
        // (verified live 2026-10-21 with a real session: iTunes-identity
        // session → 1100 on listTeams, akd-identity session → teams
        // listed). The ESX calls carry the GrandSlam client's base headers,
        // so the client below is what they present.
        let client_info = account
            .anisette_generator
            .get_app_token_client_info()
            .await
            .context("Failed to get developer session client info")?;
        let client = Arc::new(
            GrandSlam::new(client_info, false, None)
                .await
                .context("Failed to build developer session client")?,
        );

        Ok(DeveloperSession::new(
            token,
            spd.get_string("adsid")?,
            client,
            account.anisette_generator.clone(),
        ))
    }

    pub async fn get_headers(&mut self) -> Result<HeaderMap, Report> {
        // Fresh anisette per call: every developer-API request consumes an
        // OTP, and reusing one fetched for a previous request risks the
        // same stale-OTP rejection (-80046) the apptokens step hit.
        // Mirrors real akd, which regenerates anisette for every request.
        let mut headers = self
            .anisette_generator
            .get_fresh_anisette_data(self.client.clone())
            .await?
            .get_header_map()?;

        headers.insert(
            "X-Apple-GS-Token",
            HeaderValue::from_str(&self.token.token)?,
        );
        headers.insert("X-Apple-I-Identity-Id", HeaderValue::from_str(&self.adsid)?);

        Ok(headers)
    }

    pub fn get_grandslam_client(&self) -> Arc<GrandSlam> {
        self.client.clone()
    }

    async fn send_dev_request_internal(
        &mut self,
        url: &str,
        body: impl Into<Option<Dictionary>>,
    ) -> Result<(Dictionary, Option<SideloadError>), Report> {
        let body = body.into().unwrap_or_else(Dictionary::new);

        let base = plist!(dict {
            "clientId": "XABBG36SBA",
            "protocolVersion": "QH65B2",
            "requestId": Uuid::new_v4().to_string().to_uppercase(),
            "userLocale": ["en_US"],
        });

        let body = base.into_iter().chain(body.into_iter()).collect();

        let text = self
            .client
            .post(url)?
            .body(plist_to_xml_string(&body))
            .headers(
                self.get_headers()
                    .await
                    .context("Failed to get anisette headers")?,
            )
            .send()
            .await?
            .error_for_status()
            .context("Developer request failed")?
            .text()
            .await
            .context("Failed to read developer request response text")?;

        let dict: Dictionary = plist::from_bytes(text.as_bytes())
            .context("Failed to parse developer request plist")?;

        // All this error handling is here to ensure that:
        // 1. We always warn/log errors from the server even if it returns the expected data
        // 2. We return server errors if the expected data is missing
        // 3. We return parsing errors if there is no server error but the expected data is missing
        let response_code = dict.get("resultCode").and_then(|v| v.as_signed_integer());
        let mut server_error: Option<SideloadError> = None;
        if let Some(code) = response_code {
            if code != 0 {
                let result_string = dict
                    .get("resultString")
                    .and_then(|v| v.as_string())
                    .unwrap_or("No error message given.");
                let user_string = dict
                    .get("userString")
                    .and_then(|v| v.as_string())
                    .unwrap_or(result_string);
                server_error = Some(SideloadError::DeveloperError(code, user_string.to_string()));

                error!(
                    "Developer request returned error code {}: {} ({})",
                    code, user_string, result_string
                );
            }
        } else {
            warn!("No resultCode in developer request response");
        }

        Ok((dict, server_error))
    }

    pub async fn send_dev_request<T: DeserializeOwned>(
        &mut self,
        url: &str,
        body: impl Into<Option<Dictionary>>,
        response_key: &str,
    ) -> Result<T, Report> {
        let (dict, server_error) = self.send_dev_request_internal(url, body).await?;

        let result: Result<T, _> = dict.get_struct(response_key);

        if result.is_err()
            && let Some(err) = server_error
        {
            bail!(err);
        }

        Ok(result.context("Failed to extract developer request result")?)
    }

    pub async fn send_dev_request_no_response(
        &mut self,
        url: &str,
        body: impl Into<Option<Dictionary>>,
    ) -> Result<Dictionary, Report> {
        let (dict, server_error) = self.send_dev_request_internal(url, body).await?;

        if let Some(err) = server_error {
            bail!(err);
        }

        Ok(dict)
    }
}
