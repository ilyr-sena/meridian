//! GSA `apptokens` identity probe — debug tool for Apple's rotating GrandSlam
//! gates (the -80045 / -80046 incident chain; see
//! docs/ANISETTE_GSA_80045_FIX.md).
//!
//! Logs in ONCE with the real hub flow (iTunes identity — the -80045 fix),
//! then requests the `xcode.auth` app token against several candidate client
//! identities inside that one session, printing each GSA response. This turns
//! "one 2FA login per theory" into "all theories in one 2FA login".
//!
//! Usage (credentials via environment — never argv):
//!   APPLE_ID=... APPLE_PASSWORD=... [ANISETTE_URL=http://meridianhub.cc:6969] \
//!   cargo run --bin gsa_probe
//!
//! The 2FA code is read from /tmp/meridian-2fa.txt — when Apple pushes the
//! code to your devices, write it there (e.g. `echo 123456 > /tmp/meridian-2fa.txt`).

use hmac::{Hmac, Mac};
use isideload::anisette::remote::RemoteAnisetteProvider;
use isideload::anisette::AnisetteClientInfo;
use isideload::auth::apple_account::{
    AppleAccount, TwoFactorCallbackParams, TwoFactorCallbackResponse,
};
use isideload::auth::grandslam::GrandSlam;
use isideload::dev::developer_session::DeveloperSession;
use isideload::dev::teams::TeamsApi;
use isideload::util::plist::PlistDataExtract;
use plist::Dictionary;
use sha2::Sha256;
use std::time::Duration;

const TWO_FA_FILE: &str = "/tmp/meridian-2fa.txt";
const APP: &str = "com.apple.gs.xcode.auth";

#[tokio::main]
async fn main() {
    // Bootstrap the rustls crypto provider process-wide. isideload's reqwest
    // 0.13 is built with `rustls-no-provider` and panics at client-build time
    // unless a provider is installed; the hub process reaches an installed
    // state through its runtime startup, a bare probe does not. Ring is the
    // provider this graph already uses (via `idevice`). If one is already
    // installed the call is a harmless no-op.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let email = std::env::var("APPLE_ID").expect("APPLE_ID env required");
    let password = std::env::var("APPLE_PASSWORD").expect("APPLE_PASSWORD env required");
    let anisette_url =
        std::env::var("ANISETTE_URL").unwrap_or_else(|_| "http://meridianhub.cc:6969".to_string());

    eprintln!("[probe] logging in (anisette: {anisette_url}) — this pushes a 2FA code to trusted devices");

    let anisette = RemoteAnisetteProvider::new(&anisette_url).set_username(email.clone());

    let two_factor_cb = |params: TwoFactorCallbackParams| async move {
        if params.unknown {
            return Ok(TwoFactorCallbackResponse::SendToDevices);
        }
        let kind = if params.sms { "SMS" } else { "trusted-device" };
        eprintln!("[probe] 2FA required ({kind}). Apple pushed a code to your devices.");
        eprintln!("[probe] >>> write the code to {TWO_FA_FILE}  (echo <code> > {TWO_FA_FILE})");
        for _ in 0..480 {
            if let Ok(code) = std::fs::read_to_string(TWO_FA_FILE) {
                let _ = std::fs::remove_file(TWO_FA_FILE);
                let code = code.trim().to_string();
                if !code.is_empty() {
                    eprintln!("[probe] 2FA code accepted");
                    return Ok(TwoFactorCallbackResponse::SubmitCode(code));
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Ok(TwoFactorCallbackResponse::Abort)
    };

    let mut account = match AppleAccount::builder(&email)
        .anisette_provider(anisette)
        .login(&password, two_factor_cb)
        .await
    {
        Ok(a) => a,
        Err(e) => {
            eprintln!("[probe] LOGIN FAILED: {e:?}");
            std::process::exit(2);
        }
    };
    eprintln!("[probe] LOGIN OK");

    let spd = account.spd.clone().expect("spd missing after login");
    let dsid = spd.get_str("adsid").expect("adsid").to_string();
    let auth_token = spd.get_str("GsIdmsToken").expect("GsIdmsToken").to_string();
    let session_key = spd.get_data("sk").expect("sk");
    let continuation = spd.get_data("c").expect("c");
    eprintln!(
        "[probe] session ready (dsid={dsid}…, sk={}B, c={}B)",
        session_key.len(),
        continuation.len()
    );

    // Candidate client identities for the apptokens request. The first entry
    // is the hub's current identity (the -80045 login fix) and acts as the
    // control expected to reproduce -80046; the others are candidates for
    // what the deeper token-issuance check may accept.
    let candidates: Vec<(&str, AnisetteClientInfo)> = vec![
        (
            "itunes-windows-12.12.4 (current hub identity — control)",
            AnisetteClientInfo {
                client_info: "<PC> <Windows;6.2(0,0);9200> <com.apple.AuthKit/1 (com.apple.iTunes/12.12.4.1)>".to_string(),
                user_agent: "akd/1.0 CFNetwork/808.1.4".to_string(),
            },
        ),
        (
            "akd-macos-13.1-MBP13,2 (era-2 identity)",
            AnisetteClientInfo {
                client_info: "<MacBookPro13,2> <macOS;13.1;22C65> <com.apple.AuthKit/1 (com.apple.akd/1.0)>".to_string(),
                user_agent: "akd/1.0 CFNetwork/978.0.7 Darwin/18.7.0".to_string(),
            },
        ),
        (
            "akd-macos-14.1-MBP16,1",
            AnisetteClientInfo {
                client_info: "<MacBookPro16,1> <macOS;14.1;23B74> <com.apple.AuthKit/1 (com.apple.akd/1.0)>".to_string(),
                user_agent: "akd/1.0 CFNetwork/978.0.7 Darwin/18.7.0".to_string(),
            },
        ),
    ];

    for (name, ci) in candidates {
        eprintln!("\n[probe] === apptokens as: {name}");

        let gs = match GrandSlam::new(ci, false, None).await {
            Ok(g) => g,
            Err(e) => {
                eprintln!("[probe]   GrandSlam::new (URL bag fetch) failed: {e:?}");
                continue;
            }
        };

        // Fresh anisette per attempt — mirrors the patched hub (-80046 fix).
        let mut generator = account.anisette_generator.clone();
        let anisette_data = match generator
            .get_fresh_anisette_data(account.grandslam_client.clone())
            .await
        {
            Ok(a) => a,
            Err(e) => {
                eprintln!("[probe]   anisette fetch failed: {e:?}");
                continue;
            }
        };
        let cpd = anisette_data.get_client_provided_data();

        // checksum = HMAC-SHA256(session_key, "apptokens" || dsid || app)
        let mut mac = Hmac::<Sha256>::new_from_slice(&session_key).expect("hmac init");
        mac.update(b"apptokens");
        mac.update(dsid.as_bytes());
        mac.update(APP.as_bytes());
        let checksum = mac.finalize().into_bytes().to_vec();

        let mut header = Dictionary::new();
        header.insert("Version".into(), plist::Value::String("1.0.1".into()));
        let mut request = Dictionary::new();
        request.insert(
            "app".into(),
            plist::Value::Array(vec![plist::Value::String(APP.to_string())]),
        );
        request.insert("c".into(), plist::Value::Data(continuation.to_vec()));
        request.insert("checksum".into(), plist::Value::Data(checksum));
        request.insert("cpd".into(), plist::Value::Dictionary(cpd));
        request.insert("o".into(), plist::Value::String("apptokens".into()));
        request.insert("u".into(), plist::Value::String(dsid.clone()));
        request.insert("t".into(), plist::Value::String(auth_token.clone()));
        let mut body = Dictionary::new();
        body.insert("Header".into(), plist::Value::Dictionary(header));
        body.insert("Request".into(), plist::Value::Dictionary(request));

        let url = match gs.get_url("gsService") {
            Ok(u) => u,
            Err(e) => {
                eprintln!("[probe]   url-bag key missing: {e:?}");
                continue;
            }
        };

        match gs.plist_request(&url, &body, None).await {
            Ok(resp) => {
                let status = resp.get("Status").and_then(|v| v.as_dictionary());
                if let Some(st) = status {
                    let ec = st.get_signed_integer("ec").unwrap_or(0);
                    let em = st.get_str("em").unwrap_or("");
                    let et_len = resp.get("et").and_then(|v| v.as_data()).map(|d| d.len());
                    eprintln!(
                        "[probe]   GSA response: ec={ec} em={em:?} et bytes: {et_len:?}"
                    );
                    if ec == 0 {
                        eprintln!("[probe]   *** ACCEPTED for this identity ***");
                    }
                } else {
                    let et_len = resp
                        .get("et")
                        .and_then(|v| v.as_data())
                        .map(|d| d.len());
                    eprintln!(
                        "[probe]   no Status dict — keys: {:?}, et bytes: {et_len:?}",
                        resp.keys().collect::<Vec<_>>()
                    );
                    if et_len.is_some() {
                        eprintln!("[probe]   *** TOKEN ISSUED for this identity ***");
                    }
                }
            }
            Err(e) => eprintln!("[probe]   request failed: {e:?}"),
        }

        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    // --- ESX phase: the developer services (developerservices2.apple.com).
    // Mint the token via the REAL hub path (the vendored identity split is
    // in effect), then A/B the teams listing with the session's GrandSlam
    // client built from each identity — the ESX requests carry the client's
    // base headers, so the session identity is the variable.
    eprintln!("\n[probe] === minting xcode.auth token via the real hub path");
    let token = match account.get_app_token("xcode.auth").await {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[probe] get_app_token FAILED: {e:?}");
            std::process::exit(3);
        }
    };
    eprintln!(
        "[probe] token minted ({} chars, duration={}s, expiry={})",
        token.token.len(),
        token.duration,
        token.expiry
    );

    let session_candidates: Vec<(&str, AnisetteClientInfo)> = vec![
        (
            "itunes-session (current hub behavior — account's login client)",
            AnisetteClientInfo {
                client_info: "<PC> <Windows;6.2(0,0);9200> <com.apple.AuthKit/1 (com.apple.iTunes/12.12.4.1)>".to_string(),
                user_agent: "akd/1.0 CFNetwork/808.1.4".to_string(),
            },
        ),
        (
            "akd13-session (the identity the token was minted under)",
            AnisetteClientInfo {
                client_info: "<MacBookPro13,2> <macOS;13.1;22C65> <com.apple.AuthKit/1 (com.apple.akd/1.0)>".to_string(),
                user_agent: "akd/1.0 CFNetwork/978.0.7 Darwin/18.7.0".to_string(),
            },
        ),
    ];

    for (name, ci) in session_candidates {
        eprintln!("\n[probe] === list_teams via {name}");
        let gs = match GrandSlam::new(ci, false, None).await {
            Ok(g) => g,
            Err(e) => {
                eprintln!("[probe]   GrandSlam::new failed: {e:?}");
                continue;
            }
        };
        let mut session = DeveloperSession::new(
            token.clone(),
            dsid.clone(),
            std::sync::Arc::new(gs),
            account.anisette_generator.clone(),
        );
        match session.list_teams().await {
            Ok(teams) => {
                eprintln!(
                    "[probe]   *** SUCCESS — {} team(s) ***",
                    teams.len()
                );
                for t in teams.iter().take(5) {
                    eprintln!("[probe]     team: {:?} ({})", t.name, t.team_id);
                }
            }
            Err(e) => eprintln!("[probe]   FAILED: {e:?}"),
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    eprintln!("\n[probe] done");
}
