//! Pure Rust Action & Control Bridge Server (Port 9001).
//!
//! Provides the HTTP endpoints and WebSocket server required by the Meridian web client:
//! - GET  /apps.json            -> List installed user/system apps
//! - GET  /apps/running.json    -> List active processes
//! - GET  /icon/:bundle_id.png  -> Fetch app icon PNG
//! - POST /app/launch/:bundle_id -> Launch an app via CoreDevice
//! - POST /app/kill/:pid        -> Terminate a process
//! - WS   /ws                   -> Low-latency 60Hz HID gestures, hardware buttons, keyboard

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, info, warn};

use idevice::{
    IdeviceService,
    provider::UsbmuxdProvider,
    services::{
        core_device::AppServiceClient,
        core_device_proxy::CoreDeviceProxy,
        rsd::RsdHandshake,
        springboardservices::SpringBoardServicesClient,
    },
    usbmuxd::UsbmuxdAddr,
};

use crate::device::actions::{CoreDeviceHid, WdaClient};

const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

#[derive(Debug)]
pub struct BridgeServer {
    is_running: Arc<AtomicBool>,
}

impl BridgeServer {
    /// Start the Bridge HTTP + WebSocket server on the specified port.
    pub fn start(
        port: u16,
        wda_port: u16,
        udid: String,
        device_id: u32,
    ) -> Self {
        let is_running = Arc::new(AtomicBool::new(true));
        let running_flag = is_running.clone();

        tokio::spawn(async move {
            let addr = SocketAddr::from(([0, 0, 0, 0], port));
            let listener = match TcpListener::bind(addr).await {
                Ok(l) => {
                    info!("✓ Action Bridge Server listening on http://0.0.0.0:{}", port);
                    l
                }
                Err(e) => {
                    warn!("Failed to bind Action Bridge on port {}: {:?}", port, e);
                    return;
                }
            };

            let wda = Arc::new(WdaClient::new(format!("http://127.0.0.1:{}", wda_port)));
            let udid = Arc::new(udid);

            while running_flag.load(Ordering::SeqCst) {
                match listener.accept().await {
                    Ok((stream, peer)) => {
                        debug!("Bridge connection from {}", peer);
                        let wda_clone = wda.clone();
                        let udid_clone = udid.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, wda_clone, udid_clone, device_id).await {
                                debug!("Bridge connection ended: {:?}", e);
                            }
                        });
                    }
                    Err(e) => {
                        debug!("Bridge accept error: {:?}", e);
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                }
            }
        });

        Self { is_running }
    }

    pub fn stop(&self) {
        self.is_running.store(false, Ordering::SeqCst);
    }
}

struct HttpRequest {
    method: String,
    path: String,
    headers: HashMap<String, String>,
}

fn parse_http_request(buf: &[u8]) -> Option<HttpRequest> {
    let req_str = String::from_utf8_lossy(buf);
    let mut lines = req_str.lines();
    let first_line = lines.next()?;
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }
    let method = parts[0].to_string();
    let path = parts[1].to_string();

    let mut headers = HashMap::new();
    for line in lines {
        if line.is_empty() || line == "\r" {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_lowercase(), v.trim().to_string());
        }
    }
    Some(HttpRequest { method, path, headers })
}

async fn handle_connection(
    mut stream: TcpStream,
    wda: Arc<WdaClient>,
    udid: Arc<String>,
    device_id: u32,
) -> anyhow::Result<()> {
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).await?;
    if n == 0 {
        return Ok(());
    }

    let req = match parse_http_request(&buf[..n]) {
        Some(r) => r,
        None => return Ok(()),
    };

    // 1. WebSocket Upgrade on /ws
    if req.headers.get("upgrade").map(|s| s.to_lowercase()).as_deref() == Some("websocket") {
        if let Some(sec_key) = req.headers.get("sec-websocket-key") {
            let sec_key_owned = sec_key.clone();
            return handle_websocket(stream, sec_key_owned, wda, udid, device_id).await;
        }
    }

    // 2. HTTP Endpoints
    if req.method == "OPTIONS" {
        send_cors_response(&mut stream, 204, "No Content", "text/plain", Vec::new()).await?;
        return Ok(());
    }

    if req.method == "GET" && (req.path == "/apps.json" || req.path.starts_with("/apps.json?")) {
        let apps = fetch_apps(udid.clone(), device_id).await.unwrap_or_default();
        let body = serde_json::to_vec(&serde_json::json!({ "apps": apps }))?;
        send_cors_response(&mut stream, 200, "OK", "application/json", body).await?;
        return Ok(());
    }

    if req.method == "GET" && (req.path == "/apps/running.json" || req.path.starts_with("/apps/running.json?")) {
        let procs = fetch_running_processes(udid.clone(), device_id).await.unwrap_or_default();
        let body = serde_json::to_vec(&serde_json::json!({ "running": procs }))?;
        send_cors_response(&mut stream, 200, "OK", "application/json", body).await?;
        return Ok(());
    }

    if req.method == "GET" && req.path.starts_with("/icon/") {
        let bundle_id = req.path
            .trim_start_matches("/icon/")
            .trim_end_matches(".png");
        let decoded_bundle = url_decode(bundle_id);

        if let Ok(icon_png) = fetch_icon(udid.clone(), device_id, decoded_bundle).await {
            send_cors_response(&mut stream, 200, "OK", "image/png", icon_png).await?;
            return Ok(());
        } else {
            send_cors_response(&mut stream, 404, "Not Found", "text/plain", b"Icon not found".to_vec()).await?;
            return Ok(());
        }
    }

    if req.method == "POST" && req.path.starts_with("/app/launch/") {
        let bundle_id = req.path.trim_start_matches("/app/launch/");
        let decoded_bundle = url_decode(bundle_id);
        let res = launch_app(udid.clone(), device_id, decoded_bundle).await;
        let body = match res {
            Ok(pid) => serde_json::to_vec(&serde_json::json!({ "ok": true, "pid": pid }))?,
            Err(e) => serde_json::to_vec(&serde_json::json!({ "ok": false, "error": e.to_string() }))?,
        };
        send_cors_response(&mut stream, 200, "OK", "application/json", body).await?;
        return Ok(());
    }

    if req.method == "POST" && req.path.starts_with("/app/kill/") {
        let pid_str = req.path.trim_start_matches("/app/kill/");
        if let Ok(pid) = pid_str.parse::<u32>() {
            let _ = kill_process(udid.clone(), device_id, pid).await;
        }
        let body = serde_json::to_vec(&serde_json::json!({ "ok": true }))?;
        send_cors_response(&mut stream, 200, "OK", "application/json", body).await?;
        return Ok(());
    }

    send_cors_response(&mut stream, 404, "Not Found", "text/plain", b"Not found".to_vec()).await?;
    Ok(())
}

async fn send_cors_response(
    stream: &mut TcpStream,
    status_code: u16,
    reason: &'static str,
    content_type: &'static str,
    body: Vec<u8>,
) -> anyhow::Result<()> {
    let response = format!(
        "HTTP/1.1 {} {}\r\n\
         Content-Type: {}\r\n\
         Content-Length: {}\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
         Access-Control-Allow-Headers: *\r\n\
         Connection: close\r\n\r\n",
        status_code,
        reason,
        content_type,
        body.len()
    );

    stream.write_all(response.as_bytes()).await?;
    if !body.is_empty() {
        stream.write_all(&body).await?;
    }
    stream.flush().await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Native WebSocket Handler
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
struct GestureState {
    start: Option<(f32, f32, std::time::Instant)>,
    last: (f32, f32),
}

async fn handle_websocket(
    mut stream: TcpStream,
    sec_key: String,
    wda: Arc<WdaClient>,
    udid: Arc<String>,
    device_id: u32,
) -> anyhow::Result<()> {
    use sha1::{Digest, Sha1};

    // Calculate Sec-WebSocket-Accept
    let mut hasher = Sha1::new();
    hasher.update(format!("{}{}", sec_key, WS_GUID).as_bytes());
    let accept_hash = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, hasher.finalize());

    let handshake = format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {}\r\n\r\n",
        accept_hash
    );
    stream.write_all(handshake.as_bytes()).await?;
    stream.flush().await?;
    debug!("✓ WebSocket connected on /ws");

    let gesture_state = Arc::new(tokio::sync::Mutex::new(GestureState::default()));
    let mut buf = [0u8; 4096];

    loop {
        let n = match stream.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };

        let mut offset = 0;
        while offset < n {
            if offset + 2 > n {
                break;
            }
            let b0 = buf[offset];
            let b1 = buf[offset + 1];
            let opcode = b0 & 0x0F;
            let masked = (b1 & 0x80) != 0;
            let mut payload_len = (b1 & 0x7F) as usize;
            offset += 2;

            if opcode == 0x08 {
                // Connection Close
                return Ok(());
            }

            if payload_len == 126 {
                if offset + 2 > n { break; }
                payload_len = u16::from_be_bytes([buf[offset], buf[offset + 1]]) as usize;
                offset += 2;
            } else if payload_len == 127 {
                if offset + 8 > n { break; }
                payload_len = u64::from_be_bytes(buf[offset..offset + 8].try_into().unwrap()) as usize;
                offset += 8;
            }

            let mask = if masked {
                if offset + 4 > n { break; }
                let m = [buf[offset], buf[offset + 1], buf[offset + 2], buf[offset + 3]];
                offset += 4;
                Some(m)
            } else {
                None
            };

            if offset + payload_len > n {
                break;
            }

            let mut payload = buf[offset..offset + payload_len].to_vec();
            if let Some(m) = mask {
                for (i, byte) in payload.iter_mut().enumerate() {
                    *byte ^= m[i % 4];
                }
            }
            offset += payload_len;

            if opcode == 0x01 {
                // Text frame parsed directly from payload bytes and dispatched asynchronously
                if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&payload) {
                    let w = wda.clone();
                    let u = udid.clone();
                    let g = gesture_state.clone();
                    tokio::spawn(async move {
                        dispatch_ws_message(val, w, u, device_id, g).await;
                    });
                }
            }
        }
    }

    Ok(())
}

async fn dispatch_ws_message(
    val: serde_json::Value,
    wda: Arc<WdaClient>,
    udid: Arc<String>,
    device_id: u32,
    gesture: Arc<tokio::sync::Mutex<GestureState>>,
) {
    let kind = val["kind"].as_str().unwrap_or_default().to_string();
    match kind.as_str() {
        "action" => {
            if let Some(name) = val["name"].as_str() {
                let name = name.to_string();
                debug!("Received WS action: {}", name);
                if let Err(e) = CoreDeviceHid::send_hardware_button((*udid).clone(), device_id, name.clone()).await {
                    debug!("CoreDevice hardware button failed ({:?}); trying WDA fallback", e);
                    match name.as_str() {
                        "home" => { let _ = wda.homescreen().await; }
                        "lock" => { let _ = wda.press_key("lock").await; }
                        "volume-up" => { let _ = wda.press_key("volumeUp").await; }
                        "volume-down" => { let _ = wda.press_key("volumeDown").await; }
                        _ => {}
                    }
                }
            }
        }
        "down" => {
            if let (Some(fx), Some(fy)) = (val["fx"].as_f64(), val["fy"].as_f64()) {
                let (fx, fy) = (fx as f32, fy as f32);
                let mut st = gesture.lock().await;
                st.start = Some((fx, fy, std::time::Instant::now()));
                st.last = (fx, fy);
            }
        }
        "move" => {
            if let (Some(fx), Some(fy)) = (val["fx"].as_f64(), val["fy"].as_f64()) {
                let mut st = gesture.lock().await;
                st.last = (fx as f32, fy as f32);
            }
        }
        "release" => {
            let (start_opt, last_pos) = {
                let mut st = gesture.lock().await;
                (st.start.take(), st.last)
            };

            if let Some((start_fx, start_fy, start_time)) = start_opt {
                let (end_fx, end_fy) = if let (Some(x), Some(y)) = (val["fx"].as_f64(), val["fy"].as_f64()) {
                    (x as f32, y as f32)
                } else {
                    last_pos
                };

                let dx = (end_fx - start_fx) * 390.0;
                let dy = (end_fy - start_fy) * 844.0;
                let dist = (dx * dx + dy * dy).sqrt();
                let elapsed = start_time.elapsed().as_secs_f32();

                if dist < 15.0 {
                    let x = start_fx * 390.0;
                    let y = start_fy * 844.0;
                    debug!("📍 Gesture TAP at ({:.1}, {:.1})", x, y);
                    let _ = wda.tap(x, y).await;
                } else {
                    let x1 = start_fx * 390.0;
                    let y1 = start_fy * 844.0;
                    let x2 = end_fx * 390.0;
                    let y2 = end_fy * 844.0;
                    let duration = elapsed.clamp(0.08, 0.35);
                    debug!("📍 Gesture SWIPE from ({:.1}, {:.1}) to ({:.1}, {:.1}) duration {:.2}s", x1, y1, x2, y2, duration);
                    let _ = wda.drag(x1, y1, x2, y2, duration).await;
                }
            }
        }
        "tap" => {
            if let (Some(fx), Some(fy)) = (val["fx"].as_f64(), val["fy"].as_f64()) {
                let x = fx as f32 * 390.0;
                let y = fy as f32 * 844.0;
                let _ = wda.tap(x, y).await;
            }
        }
        "key_down" | "key" => {
            if let Some(key) = val["key"].as_str() {
                let _ = wda.type_text(key).await;
            }
        }
        "paste" => {
            if let Some(text) = val["text"].as_str() {
                let _ = wda.type_text(text).await;
            }
        }
        "keyboard" => {
            let _ = CoreDeviceHid::send_hardware_button((*udid).clone(), device_id, "keyboard-toggle".to_string()).await;
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Pure Rust CoreDevice Helpers for Bridge
// ---------------------------------------------------------------------------

async fn fetch_apps(udid: Arc<String>, device_id: u32) -> anyhow::Result<Vec<serde_json::Value>> {
    let provider = UsbmuxdProvider {
        addr: UsbmuxdAddr::default(),
        tag: 1,
        udid: (*udid).clone(),
        device_id,
        label: "meridian-hub".to_string(),
    };

    use idevice::services::installation_proxy::InstallationProxyClient;

    let mut inst = InstallationProxyClient::connect(&provider).await?;
    let raw_apps = inst.get_apps(Some("User"), None).await?;
    let mut out = Vec::new();
    for (bid, meta) in raw_apps {
        let name = meta
            .as_dictionary()
            .and_then(|d| d.get("CFBundleDisplayName").or_else(|| d.get("CFBundleName")))
            .and_then(|v| v.as_string())
            .unwrap_or(&bid)
            .to_string();

        out.push(serde_json::json!({
            "bundleId": bid,
            "name": name,
        }));
    }

    out.sort_by(|a, b| {
        let na = a["name"].as_str().unwrap_or_default();
        let nb = b["name"].as_str().unwrap_or_default();
        na.cmp(nb)
    });

    info!("✓ Loaded {} user apps via InstallationProxy", out.len());
    Ok(out)
}

async fn fetch_running_processes(udid: Arc<String>, device_id: u32) -> anyhow::Result<Vec<serde_json::Value>> {
    let provider = UsbmuxdProvider {
        addr: UsbmuxdAddr::default(),
        tag: 1,
        udid: (*udid).clone(),
        device_id,
        label: "meridian-hub".to_string(),
    };

    let proxy = CoreDeviceProxy::connect(&provider).await?;
    let rsd_port = proxy.tunnel_info().server_rsd_port;
    let adapter = proxy.create_software_tunnel()?;
    let mut handle = adapter.to_async_handle();

    let rsd_stream = handle.connect(rsd_port).await?;
    let rsd = RsdHandshake::new(rsd_stream).await?;

    let app_entry = rsd.services.get("com.apple.coredevice.appservice")
        .ok_or_else(|| anyhow::anyhow!("AppService not found"))?;

    let app_stream = handle.connect(app_entry.port).await?;
    let mut app_service = AppServiceClient::new(app_stream).await?;

    let procs = app_service.list_processes().await?;
    let mut out = Vec::new();
    for p in procs {
        let name = p.executable_url.as_ref().map(|u| u.relative.split('/').last().unwrap_or_default()).unwrap_or_default();
        out.push(serde_json::json!({
            "pid": p.pid,
            "name": name,
            "bundleId": name,
        }));
    }
    Ok(out)
}

async fn fetch_icon(udid: Arc<String>, device_id: u32, bundle_id: String) -> anyhow::Result<Vec<u8>> {
    let cache_dir = dirs::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("meridian")
        .join("icons");
    let icon_file = cache_dir.join(format!("{}.png", bundle_id));
    if icon_file.exists() {
        if let Ok(bytes) = tokio::fs::read(&icon_file).await {
            if !bytes.is_empty() {
                return Ok(bytes);
            }
        }
    }

    let provider = UsbmuxdProvider {
        addr: UsbmuxdAddr::default(),
        tag: 1,
        udid: (*udid).clone(),
        device_id,
        label: "meridian-hub".to_string(),
    };

    let mut sb = SpringBoardServicesClient::connect(&provider).await?;
    let png = sb.get_icon_pngdata(bundle_id).await?;

    let _ = std::fs::create_dir_all(&cache_dir);
    let _ = tokio::fs::write(&icon_file, &png).await;
    Ok(png)
}

async fn launch_app(udid: Arc<String>, device_id: u32, bundle_id: String) -> anyhow::Result<u32> {
    let provider = UsbmuxdProvider {
        addr: UsbmuxdAddr::default(),
        tag: 1,
        udid: (*udid).clone(),
        device_id,
        label: "meridian-hub".to_string(),
    };

    let proxy = CoreDeviceProxy::connect(&provider).await?;
    let rsd_port = proxy.tunnel_info().server_rsd_port;
    let adapter = proxy.create_software_tunnel()?;
    let mut handle = adapter.to_async_handle();

    let rsd_stream = handle.connect(rsd_port).await?;
    let rsd = RsdHandshake::new(rsd_stream).await?;

    let app_entry = rsd.services.get("com.apple.coredevice.appservice")
        .ok_or_else(|| anyhow::anyhow!("AppService not found"))?;

    let app_stream = handle.connect(app_entry.port).await?;
    let mut app_service = AppServiceClient::new(app_stream).await?;

    const EMPTY_ARGS: &[&'static str] = &[];
    let resp = app_service.launch_application(bundle_id, EMPTY_ARGS, true, false, None, None, None).await?;
    Ok(resp.pid)
}

async fn kill_process(udid: Arc<String>, device_id: u32, pid: u32) -> anyhow::Result<()> {
    let provider = UsbmuxdProvider {
        addr: UsbmuxdAddr::default(),
        tag: 1,
        udid: (*udid).clone(),
        device_id,
        label: "meridian-hub".to_string(),
    };

    let proxy = CoreDeviceProxy::connect(&provider).await?;
    let rsd_port = proxy.tunnel_info().server_rsd_port;
    let adapter = proxy.create_software_tunnel()?;
    let mut handle = adapter.to_async_handle();

    let rsd_stream = handle.connect(rsd_port).await?;
    let rsd = RsdHandshake::new(rsd_stream).await?;

    let app_entry = rsd.services.get("com.apple.coredevice.appservice")
        .ok_or_else(|| anyhow::anyhow!("AppService not found"))?;

    let app_stream = handle.connect(app_entry.port).await?;
    let mut app_service = AppServiceClient::new(app_stream).await?;

    let _ = app_service.send_signal(pid, 9).await?;
    Ok(())
}

fn url_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '%' {
            let h1 = chars.next();
            let h2 = chars.next();
            if let (Some(c1), Some(c2)) = (h1, h2) {
                let hex_str = format!("{}{}", c1, c2);
                if let Ok(byte) = u8::from_str_radix(&hex_str, 16) {
                    result.push(byte as char);
                    continue;
                }
            }
        } else if ch == '+' {
            result.push(' ');
        } else {
            result.push(ch);
        }
    }
    result
}
