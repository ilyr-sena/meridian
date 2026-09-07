//! Direct Lockdown client to query device metadata and inspect pairing state.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{debug, info};
use crate::device::tunnel::connect_usbmuxd;

pub const LOCKDOWN_PORT: u16 = 62078;

#[derive(Debug, Clone, Default)]
pub struct LockdownInfo {
    pub name: String,
    pub model: String,
    pub os_version: String,
    pub build_version: String,
    pub serial_number: Option<String>,
}

pub async fn query_lockdown(device_id: u32, _udid: &str) -> anyhow::Result<LockdownInfo> {
    let mut mux = connect_usbmuxd().await?;

    // 1. Connect to lockdown port
    let mut dict = plist::Dictionary::new();
    dict.insert("MessageType".into(), plist::Value::String("Connect".into()));
    dict.insert("ClientVersion".into(), plist::Value::Integer(7.into()));
    dict.insert("ProgName".into(), plist::Value::String("meridian-hub".into()));
    dict.insert("DeviceID".into(), plist::Value::Integer((device_id as u64).into()));
    dict.insert("PortNumber".into(), plist::Value::Integer(((LOCKDOWN_PORT.to_be()) as u64).into()));

    let mut xml = Vec::new();
    plist::to_writer_xml(&mut xml, &dict)?;

    let total_len = (16 + xml.len()) as u32;
    let mut header = Vec::with_capacity(16);
    header.extend_from_slice(&total_len.to_le_bytes());
    header.extend_from_slice(&1u32.to_le_bytes());
    header.extend_from_slice(&8u32.to_le_bytes());
    header.extend_from_slice(&1u32.to_le_bytes());

    mux.write_all(&header).await?;
    mux.write_all(&xml).await?;
    mux.flush().await?;

    let mut resp_header = [0u8; 16];
    mux.read_exact(&mut resp_header).await?;
    let resp_len = u32::from_le_bytes([resp_header[0], resp_header[1], resp_header[2], resp_header[3]]) as usize;
    if resp_len > 16 {
        let mut buf = vec![0u8; resp_len - 16];
        mux.read_exact(&mut buf).await?;
        if let Ok(plist::Value::Dictionary(dict)) = plist::from_bytes(&buf) {
            if let Some(num) = dict.get("Number").and_then(|v| v.as_unsigned_integer()) {
                if num != 0 {
                    anyhow::bail!("Lockdown connect rejected with code {}", num);
                }
            }
        }
    }

    // 2. Query basic device values (GetValue query without pairing requirement)
    let mut req = plist::Dictionary::new();
    req.insert("Request".into(), plist::Value::String("GetValue".into()));
    req.insert("Label".into(), plist::Value::String("meridian-hub".into()));

    let mut req_xml = Vec::new();
    plist::to_writer_xml(&mut req_xml, &req)?;

    let msg_len = req_xml.len() as u32;
    mux.write_all(&msg_len.to_be_bytes()).await?;
    mux.write_all(&req_xml).await?;
    mux.flush().await?;

    // Read big-endian lockdown frame length (4 bytes)
    let mut len_buf = [0u8; 4];
    mux.read_exact(&mut len_buf).await?;
    let frame_len = u32::from_be_bytes(len_buf) as usize;

    let mut val_buf = vec![0u8; frame_len];
    mux.read_exact(&mut val_buf).await?;

    let mut info = LockdownInfo::default();
    if let Ok(plist::Value::Dictionary(root)) = plist::from_bytes(&val_buf) {
        if let Some(plist::Value::Dictionary(val)) = root.get("Value") {
            if let Some(name) = val.get("DeviceName").and_then(|v| v.as_string()) {
                info.name = name.to_string();
            }
            if let Some(model) = val.get("ProductType").and_then(|v| v.as_string()) {
                info.model = format_model(model);
            }
            if let Some(ver) = val.get("ProductVersion").and_then(|v| v.as_string()) {
                info.os_version = format!("iOS {}", ver);
            }
            if let Some(build) = val.get("BuildVersion").and_then(|v| v.as_string()) {
                info.build_version = build.to_string();
            }
            if let Some(sn) = val.get("SerialNumber").and_then(|v| v.as_string()) {
                info.serial_number = Some(sn.to_string());
            }
        }
    }

    Ok(info)
}

fn format_model(product_type: &str) -> String {
    match product_type {
        "iPhone14,5" => "iPhone 13".to_string(),
        "iPhone14,4" => "iPhone 13 mini".to_string(),
        "iPhone14,2" => "iPhone 13 Pro".to_string(),
        "iPhone14,3" => "iPhone 13 Pro Max".to_string(),
        "iPhone14,7" => "iPhone 14".to_string(),
        "iPhone14,8" => "iPhone 14 Plus".to_string(),
        "iPhone15,2" => "iPhone 14 Pro".to_string(),
        "iPhone15,3" => "iPhone 14 Pro Max".to_string(),
        "iPhone15,4" => "iPhone 15".to_string(),
        "iPhone15,5" => "iPhone 15 Plus".to_string(),
        "iPhone16,1" => "iPhone 15 Pro".to_string(),
        "iPhone16,2" => "iPhone 15 Pro Max".to_string(),
        "iPhone17,1" => "iPhone 16 Pro".to_string(),
        "iPhone17,2" => "iPhone 16 Pro Max".to_string(),
        "iPhone17,3" => "iPhone 16".to_string(),
        "iPhone17,4" => "iPhone 16 Plus".to_string(),
        other => other.to_string(),
    }
}

pub async fn check_runner_installed(udid: &str, device_id: u32) -> bool {
    use idevice::provider::UsbmuxdProvider;
    use idevice::services::installation_proxy::InstallationProxyClient;
    use idevice::usbmuxd::UsbmuxdAddr;
    use idevice::IdeviceService;

    let provider = UsbmuxdProvider {
        addr: UsbmuxdAddr::default(),
        tag: 1,
        udid: udid.to_string(),
        device_id,
        label: "meridian-hub".to_string(),
    };

    match InstallationProxyClient::connect(&provider).await {
        Ok(mut client) => {
            match client.get_apps(Some("User"), None).await {
                Ok(apps) => {
                    for bid in apps.keys() {
                        let lower = bid.to_lowercase();
                        if lower.contains("meridian") || lower.contains("runner") || lower.contains("xctrunner") {
                            info!("✓ Detected installed runner app: {}", bid);
                            return true;
                        }
                    }
                    info!("No runner app detected among {} user apps", apps.len());
                    false
                }
                Err(e) => {
                    debug!("Installation proxy get_apps error for {}: {:?}", udid, e);
                    false
                }
            }
        }
        Err(e) => {
            debug!("Installation proxy connect error for {}: {:?}", udid, e);
            false
        }
    }
}
