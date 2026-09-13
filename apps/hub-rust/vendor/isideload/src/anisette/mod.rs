pub mod remote_v3;

use crate::auth::grandslam::GrandSlam;
use plist::Dictionary;
use plist_macro::plist;
use reqwest::header::HeaderMap;
use rootcause::prelude::*;
use serde::Deserialize;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;
use tracing::warn;
use web_time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Deserialize, Debug, Clone)]
pub struct AnisetteClientInfo {
    pub client_info: String,
    pub user_agent: String,
}

#[derive(Debug, Clone)]
pub struct AnisetteData {
    machine_id: String,
    one_time_password: String,
    pub routing_info: String,
    _device_description: String,
    device_unique_identifier: String,
    _local_user_id: String,
    generated_at: SystemTime,
}

// Some headers don't seem to be required. I guess not including them is technically more efficient soooo
impl AnisetteData {
    pub fn get_headers(&self) -> HashMap<String, String> {
        // 503 workaround: Apple rejects the shared omnisette anisette unless the
        // full header set (notably X-Apple-I-MD-LU + routing info) is present.
        // Also present a fresh device id instead of the shared omnisette id.
        HashMap::from_iter([
            (
                "X-Apple-I-Client-Time".to_string(),
                apple_client_time(),
            ),
            ("X-Apple-I-SRL-NO".to_string(), "0".to_string()),
            ("X-Apple-I-TimeZone".to_string(), "UTC".to_string()),
            ("X-Apple-Locale".to_string(), "en_US".to_string()),
            (
                "X-Apple-I-MD-RINFO".to_string(),
                self.routing_info.clone(),
            ),
            (
                "X-Apple-I-MD-LU".to_string(),
                self._local_user_id.clone(),
            ),
            (
                "X-Mme-Device-Id".to_string(),
                Uuid::new_v4().to_string().to_uppercase(),
            ),
            ("X-Apple-I-MD".to_string(), self.one_time_password.clone()),
            ("X-Apple-I-MD-M".to_string(), self.machine_id.clone()),
        ])
    }

    pub fn get_header_map(&self) -> Result<HeaderMap, Report> {
        let headers_map = self.get_headers();
        let mut header_map = HeaderMap::new();

        for (key, value) in headers_map {
            header_map.insert(
                reqwest::header::HeaderName::from_bytes(key.as_bytes())?,
                reqwest::header::HeaderValue::from_str(&value)?,
            );
        }

        Ok(header_map)
    }

    pub fn get_client_provided_data(&self) -> Dictionary {
        let headers = self.get_headers();

        let mut cpd = plist!(dict {
            "bootstrap": "true",
            "icscrec": "true",
            "loc": "en_US",
            "pbe": "false",
            "prkgen": "true",
            "svct": "iCloud"
        });

        for (key, value) in headers {
            cpd.insert(key.to_string(), plist::Value::String(value));
        }

        cpd
    }

    pub fn needs_refresh(&self) -> bool {
        let elapsed = self.generated_at.elapsed();
        match elapsed {
            Ok(elapsed) => elapsed.as_secs() > 60,
            Err(_) => {
                warn!("Unable to determine anisette data age, treating as expired");
                true
            }
        }
    }
}

/// Produce an `X-Apple-I-Client-Time` value in the format Apple expects
/// (`YYYY-MM-DDTHH:MM:SSZ`, UTC).
fn apple_client_time() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (y, mo, d) = civil_from_days(days);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

/// Howard Hinnant's civil-from-days algorithm (days since epoch -> Y/M/D).
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    (if mo <= 2 { y + 1 } else { y }, mo, d)
}

#[cfg_attr(feature = "wasm", async_trait::async_trait(?Send))]
#[cfg_attr(not(feature = "wasm"), async_trait::async_trait)]
pub trait AnisetteProvider {
    async fn get_anisette_data(&self) -> Result<AnisetteData, Report>;

    async fn get_client_info(&self) -> Result<AnisetteClientInfo, Report>;

    async fn provision(&mut self, gs: Arc<GrandSlam>) -> Result<(), Report>;

    fn needs_provisioning(&self) -> Result<bool, Report>;
}

#[derive(Clone)]
pub struct AnisetteDataGenerator {
    provider: Arc<RwLock<dyn AnisetteProvider + Send + Sync>>,
    data: Option<Arc<AnisetteData>>,
}

impl AnisetteDataGenerator {
    pub fn new(provider: Arc<RwLock<dyn AnisetteProvider + Send + Sync>>) -> Self {
        AnisetteDataGenerator {
            provider,
            data: None,
        }
    }

    pub async fn get_anisette_data(
        &mut self,
        gs: Arc<GrandSlam>,
    ) -> Result<Arc<AnisetteData>, Report> {
        if let Some(data) = &self.data
            && !data.needs_refresh()
        {
            return Ok(data.clone());
        }

        // trying to avoid locking as write unless necessary to promote concurrency
        let provider = self.provider.read().await;

        if provider.needs_provisioning()? {
            drop(provider);
            let mut provider_write = self.provider.write().await;
            provider_write.provision(gs).await?;
            drop(provider_write);

            let provider = self.provider.read().await;
            let data = provider.get_anisette_data().await?;
            let arc_data = Arc::new(data);
            self.data = Some(arc_data.clone());
            Ok(arc_data)
        } else {
            let data = provider.get_anisette_data().await?;
            let arc_data = Arc::new(data);
            self.data = Some(arc_data.clone());
            Ok(arc_data)
        }
    }

    pub async fn get_client_info(&self) -> Result<AnisetteClientInfo, Report> {
        let provider = self.provider.read().await;
        provider.get_client_info().await
    }
}
