//! Live touch injection via CoreDevice DisplayService + UniversalHIDService.
//!
//! backboardd silently drops synthetic HID digitizer reports unless a CoreDevice
//! `displayservice` media stream is active (the Xcode screen-mirror auth gate).
//! This module opens a discard-the-RTP screen video stream over the software
//! tunnel to hold that gate open, then drives the device's `mainTouchscreen`
//! HID surface with raw reports — giving real-time touch/drag instead of WDA's
//! buffered, forged `drag` command.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use idevice::{
    IdeviceService, RemoteXpcClient,
    provider::UsbmuxdProvider,
    services::{
        core_device::{
            CoreDeviceServiceClient, TOUCHSCREEN_STATE_CONTACT, TOUCHSCREEN_STATE_RELEASE,
            UniversalHidServiceClient,
            display_stream::{
                CallInfoBlob, DisplayServiceClient, build_screen_video_offer,
                build_start_video_parameters,
            },
        },
        core_device_proxy::CoreDeviceProxy,
        rsd::RsdHandshake,
    },
    usbmuxd::UsbmuxdAddr,
};
use tracing::{debug, info, warn};

/// Client-supported-features bitmask the device expects (matches Device Hub).
const CLIENT_SUPPORTED_FEATURES: u64 = 140;
const DISPLAY_ID: i64 = 1;

/// Which part of a gesture this event is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchPhase {
    Down,
    Move,
    Up,
}

/// An established live-touch channel: the tunnel handle (kept alive), the
/// display media-stream client (keeps the HID auth gate open), the RTP socket
/// being drained, and the Universal HID client used for touch reports.
pub struct TouchSession {
    handle: idevice::tcp::handle::AdapterHandle,
    display: DisplayServiceClient<idevice::tcp::handle::StreamHandle>,
    uhs: UniversalHidServiceClient<idevice::tcp::handle::StreamHandle>,
    _udp: Arc<idevice::tcp::handle::UdpSocketHandle>,
}

static TOUCH_CACHE: OnceLock<tokio::sync::Mutex<HashMap<String, TouchSession>>> = OnceLock::new();

fn touch_cache() -> &'static tokio::sync::Mutex<HashMap<String, TouchSession>> {
    TOUCH_CACHE.get_or_init(|| tokio::sync::Mutex::new(HashMap::new()))
}

fn provider(udid: &str, device_id: u32) -> UsbmuxdProvider {
    UsbmuxdProvider {
        addr: UsbmuxdAddr::default(),
        tag: 1,
        udid: udid.to_string(),
        device_id,
        label: "meridian-hub".to_string(),
    }
}

async fn establish(udid: &str, device_id: u32) -> anyhow::Result<TouchSession> {
    let provider = provider(udid, device_id);
    let proxy = CoreDeviceProxy::connect(&provider).await?;
    let rsd_port = proxy.tunnel_info().server_rsd_port;
    let client_ip = proxy.tunnel_info().client_address.clone();
    let server_ip = proxy.tunnel_info().server_address.clone();
    let adapter = proxy.create_software_tunnel()?;
    let mut handle = adapter.to_async_handle();

    let rsd_stream = handle.connect(rsd_port).await?;
    let rsd = RsdHandshake::new(rsd_stream).await?;

    // --- DisplayService media stream (opens the HID auth gate) ---
    let disp_entry = rsd
        .services
        .get("com.apple.coredevice.displayservice")
        .ok_or_else(|| anyhow::anyhow!("displayservice not advertised on RSD"))?;
    let disp_stream = handle.connect(disp_entry.port).await?;
    let core = CoreDeviceServiceClient::new(disp_stream).await?;
    let mut display = DisplayServiceClient::new(core);

    let udp = Arc::new(handle.bind_udp(0).await?);
    let receiver_port = udp.local_port();

    let call_info = CallInfoBlob {
        call_id: 0,
        client_version: 1,
        device_type: "Mac16,11".to_string(),
        framework_version: "2205.3.1".to_string(),
        os_version: "25F80".to_string(),
        device_name: None,
        audio_device_uid: None,
    };
    let call_id = uuid::Uuid::new_v4().to_string();
    let ssrc = rand::random::<u32>();
    let offer = build_screen_video_offer(&call_id, &call_info, ssrc)?;
    let params = build_start_video_parameters(
        &client_ip,
        receiver_port,
        &server_ip,
        0,
        offer,
        CLIENT_SUPPORTED_FEATURES,
        DISPLAY_ID,
        uuid::Uuid::new_v4(),
    );

    let answer = display.start_media_stream(params).await?;
    debug!("[TOUCH] media stream started: {:?}", answer);
    info!(
        "[TOUCH] CoreDevice display media stream active for {udid} (HID auth gate open)"
    );

    // --- Universal HID service (touch reports) ---
    let uhs_entry = rsd
        .services
        .get("com.apple.coredevice.hid.universalhidservice")
        .ok_or_else(|| anyhow::anyhow!("universalhidservice not advertised on RSD"))?;
    let uhs_stream = handle.connect(uhs_entry.port).await?;
    let mut xpc = RemoteXpcClient::new(uhs_stream).await?;
    xpc.do_handshake().await?;
    let uhs = UniversalHidServiceClient::new(xpc);

    // backboardd needs a moment to match the HID surfaces against the stream.
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Drain RTP payloads so the socket buffer never fills.
    let udp_drain = udp.clone();
    tokio::spawn(async move {
        loop {
            match udp_drain.recv().await {
                Ok(_) => {}
                Err(e) => {
                    debug!("[TOUCH] RTP drain ended: {e:?}");
                    break;
                }
            }
        }
    });

    Ok(TouchSession {
        handle,
        display,
        uhs,
        _udp: udp,
    })
}

/// Inject a single touch phase at normalized (fx, fy) in [0, 1].
pub async fn touch(
    udid: &str,
    device_id: u32,
    phase: TouchPhase,
    fx: f32,
    fy: f32,
) -> anyhow::Result<()> {
    let x = (fx.clamp(0.0, 1.0) * 65535.0).round() as u16;
    let y = (fy.clamp(0.0, 1.0) * 65535.0).round() as u16;

    let cache = touch_cache();
    let mut map = cache.lock().await;

    let session = match map.get_mut(udid) {
        Some(s) => s,
        None => {
            let s = establish(udid, device_id).await?;
            map.insert(udid.to_string(), s);
            map.get_mut(udid)
                .ok_or_else(|| anyhow::anyhow!("touch session missing"))?
        }
    };

    // Map phase -> report state (Down/Move CONTACT, Up RELEASE).
    let state = match phase {
        TouchPhase::Up => TOUCHSCREEN_STATE_RELEASE,
        TouchPhase::Down | TouchPhase::Move => TOUCHSCREEN_STATE_CONTACT,
    };

    match session.uhs.send_touchscreen(state, x, y, None).await {
        Ok(()) => Ok(()),
        Err(e) => {
            // Stale session (device re-lock / tunnel drop) — reconnect once and retry.
            warn!("[TOUCH] touch session stale ({e}); reconnecting");
            map.remove(udid);
            let s = establish(udid, device_id).await?;
            map.insert(udid.to_string(), s);
            let session = map
                .get_mut(udid)
                .ok_or_else(|| anyhow::anyhow!("touch session missing"))?;
            session.uhs.send_touchscreen(state, x, y, None).await?;
            Ok(())
        }
    }
}