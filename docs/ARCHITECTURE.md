# Meridian System Architecture

## 1. Executive Summary
Meridian is an ultra-low-latency physical iPhone orchestration and remote control system supporting **iOS 17 through iOS 27**. It connects physical iPhones via USB to a Host machine (Linux or Windows) and streams the live display over the web to a centralized Next.js Control Center on the Cloud VPS (`meridianhub.cc`) with sub-frame input response and hardware GPU decoding.

---

## 2. End-to-End System Diagram

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              SIDE 1: THE HOST                               │
│       (Linux PC or Windows Machine with USB-Connected Physical iPhones)     │
│                                                                             │
│   [iPhone 13 (iOS 27)]       [iPhone 14]       ...      [iPhone N]          │
│            │                      │                          │              │
│            └──────────────────────┴─────────────┬────────────┘              │
│                                           (USB) │                           │
│                                                 ▼                           │
│                      ┌──────────────────────────────────────┐               │
│                      │       usbmuxd / AMDS (iTunes)        │               │
│                      │  • Linux:   /var/run/usbmuxd         │               │
│                      │  • Windows: TCP 127.0.0.1:27015      │               │
│                      └──────────────────┬───────────────────┘               │
│                                         ▼                                   │
│   ┌─────────────────────────────────────────────────────────────────────┐   │
│   │           meridian-hub (100% Pure Rust Desktop App & Daemon)        │   │
│   │                                                                     │   │
│   │  • Pure Rust CoreDevice: DVT app launch & process killer (Zero Py)  │   │
│   │  • Pure Rust Sideload: isideload (GSA SRP-6a + Anisette + zsign)    │   │
│   │  • Slot Manager: Allocates isolated port blocks per USB order       │   │
│   │      Slot 0: WDA :8100 | Bridge :9001 | Stream :9200                │   │
│   │      Slot 1: WDA :8101 | Bridge :9002 | Stream :9201                │   │
│   │  • Action Bridge Server (:9001): Native apps, icons, touch gestures │   │
│   │  • Low-Latency TCP Splicer: usbmuxd tunnel with TCP_NODELAY enabled │   │
│   │  • Dynamic Heartbeat: Syncs presence, tailscale_ip & host_ports     │   │
│   └─────────────────────────────────────┬───────────────────────────────┘   │
│                                         │                                   │
│                                         ▼                                   │
│                     meridian-mesh sidecar (Tailscale WireGuard)             │
│                         Node IP: 100.93.183.86 (dynamic)                    │
└─────────────────────────────────────────┬───────────────────────────────────┘
                                          │  WireGuard Tailnet Mesh
                                          ▼  (Private & Encrypted)
┌─────────────────────────────────────────────────────────────────────────────┐
│                              SIDE 2: THE SERVER                             │
│                  (Cloud VPS: 98.84.189.148 / meridianhub.cc)                 │
│                                                                             │
│  ┌────────────────────────┐  ┌───────────────────────┐  ┌────────────────┐  │
│  │ Tailscale Daemon       │  │ MongoDB (:27017)      │  │ Omnisette      │  │
│  │ WireGuard endpoint     │  │ Devices & Sessions DB │  │ Port 6969      │  │
│  │ IP: 100.127.117.36     │  │                       │  │                │  │
│  └────────────────────────┘  └───────────────────────┘  └────────────────┘  │
│                                          ▲                                  │
│                                          │ Change Streams + SSE             │
│  ┌───────────────────────────────────────┴───────────────────────────────┐  │
│  │                  Next.js 16 Control Center (Port 3000)                │  │
│  │  • Real-time device leasing & session lifecycle state machine         │  │
│  │  • WebCodecs GPU stream decoder (avc1.64002a hardware acceleration)   │  │
│  │  • Normalized coordinate gesture tracking (taps, drags, swipes)       │  │
│  │  • Real-time session teardown & resource cleanup                      │  │
│  └───────────────────────────────────────▲───────────────────────────────┘  │
│                                          │                                  │
│  ┌───────────────────────────────────────┴───────────────────────────────┐  │
│  │                  Nginx Reverse Proxy (:80 / :443 / :9200 SSL)         │  │
│  │  • Let's Encrypt SSL termination (meridianhub.cc)                     │  │
│  │  • Web UI: https://meridianhub.cc -> 127.0.0.1:3000                   │  │
│  │  • Dynamic Node Proxy: /dev/<tailscale_ip>/<port>/<path>              │  │
│  │  • Stream SSL Proxy: https://meridianhub.cc:9200 (secure WebCodecs)   │  │
│  └───────────────────────────────────────▲───────────────────────────────┘  │
└──────────────────────────────────────────┼──────────────────────────────────┘
                                           │  HTTPS / WSS (Secure Context)
                                           ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                              END-USER BROWSER                               │
│         WebCodecs VideoDecoder (Hardware GPU) + WebSocket Control           │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 3. Subsystem Breakdown

### 3.1 Host Hub (`apps/hub-rust`)
- **Technology**: 100% Pure Rust, Iced GUI, Tokio async runtime, `idevice`, `isideload`.
- **Zero Python**: Replaced all legacy Python wrappers (`pymobiledevice3`, `sideload-engine.py`, `meridian_py`).
- **Device Lifecycle & Real-Time Health**:
  - `monitor.rs`: Usbmuxd monitor stream listens for device attach/detach.
  - `health.rs`: Real-time health poller (3s interval, debounced) probes `runner_installed`, `developer_mode`, `locked`, `paired` independently; emits `Updated` events on debounced changes.
  - `launcher.rs`: Connects to `CoreDeviceProxy` over usbmuxd and uses DVT/XCTest orchestration (`XCUITestService::run_until_wda_ready`) — a plain CoreDevice app launch SIGABRTs the UI-test runner at startup.
  - `actions.rs`: WebDriverAgent (WDA) client with zero animation cooldown (`animationCoolOffTimeout = 0`, `waitForIdleTimeout = 0`) and `CoreDeviceHid` hardware button dispatcher.
  - `keyboard.rs`: Pure Rust scancode keyboard (`IndigoKeyboardButtonEvent`s, HID page `0x07`) — phone decodes usages with its own layout, dead keys compose on-device; non-ASCII direct chars fall back to exact-text injection; Ctrl+V reads clipboard directly.
  - `touch.rs`: CoreDevice DisplayService media stream + UniversalHIDService for live touch injection.
  - `tunnel.rs`: Zero-copy asynchronous TCP forwarder over usbmuxd with `TCP_NODELAY`.
  - `bridge.rs`: HTTP/WebSocket server on port 9001 providing `/apps.json`, `/apps/running.json`, `/icon/:bid.png`, `/ws` touch/keyboard relay.
  - `heartbeat.rs`: Reports presence + dynamic rathole `host_ports` + session state to cloud API every 6s.
- **Vault & Sideload**:
  - `vault.rs`: AES-256-GCM + Argon2id encrypted vault (`~/.meridian/vault.bin`), stores Apple ID + password + anisette URL.
  - `sideloader.rs`: Pure-Rust IPA signer/installer via `isideload` (Apple ID SRP-6a + 2FA + anisette + cert/profile + sign + install). No external `zsign` / Python.
  - **VPS IPA Source**: `runner_ipa.rs` fetches latest `MeridianRunner-unsigned.ipa` from `https://meridianhub.cc/runner/manifest.json`, verifies SHA-256 + size, caches locally. No local file picker.
  - **Auto-Save Credentials**: On successful sideload, Apple ID + password + anisette URL saved to vault; next sideload auto-uses vault password when field left blank — zero re-prompt.
  - **No Re-Sideload Button**: Uninstalling the runner on-device auto-flips UI to "Sideload Runner" via health poller.

### 3.2 Unified On-Device Runner (`runner/ProbeApp`)
- **Technology**: Swift ScreenCaptureKit + VideoToolbox + WebDriverAgent merged into a single bundle (via `merge_probe_wda.py`).
- **Port 8100**: WebDriverAgent HTTP automation engine.
- **Port 9200**: Video streaming server (`TinyHTTPServer.swift` + `H264Stream.swift`).
  - ScreenCaptureKit captures screen frames with `queueDepth = 3`, `minimumFrameInterval = CMTime(1, 60)`.
  - VideoToolbox hardware encoder produces H.264 AVCC format.
  - `DataRateLimits` caps burst data rate to 1.5× average to prevent network bufferbloat.
  - WebSocket (`/stream.ws`) dispatches fMP4 fragments (`moof` + `mdat`) to connected viewers.
  - Auto-restart capture on stream stop using cached picker filter (no picker re-prompt).

### 3.3 Server & Cloud VPS (`meridianhub.cc` / `98.84.189.148`)
- **Hardware Specs**: 1 GB RAM, 2 vCPUs, 40 GB NVMe SSD (AWS Lightsail Debian 12).
- **MongoDB (:27017)**: `devices` and `sessions` collections. Change Streams broadcast database mutations in real time via SSE (`/api/events`).
- **Next.js 16 Web App (:3000)**: Managed by PM2 (`meridian`). Consumes SSE events to reflect device availability and session leases instantly.
- **Nginx Reverse Proxy**:
  - Port 443: Web UI + Dynamic node routing (`/dev/<port>/<path>` → loopback rathole service) + **Runner IPA endpoint** (`/runner/` → `/var/www/meridian-runner/`).
  - Port 80: Let's Encrypt ACME challenge + HTTPS redirect.
  - Stream SSL not needed; stream port served via rathole/STunnel on separate TLS port.
- **Rathole Direct TCP Tunnel**:
  - Control: `98.84.189.148:2333`.
  - Published (loopback): WDA `18100+slot`, Bridge `19001+slot`, Stream `19100+slot`.
  - Rathole client in-process (pure Rust `rathole` crate), one outbound TCP connection.

### 3.4 Web Frontend (`apps/web`)
- **H264StreamPlayer**:
  - WebCodecs `VideoDecoder` hardware GPU decoding.
  - Multi-tier configuration fallback with lowercase codec normalization (`avc1.64002a`) and clean AVCC stripping.
  - Decoupled `requestAnimationFrame` render loop synchronizing canvas paints with display VSync.
  - Drops pre-keyframe delta chunks to satisfy browser decoder requirements.
- **PhoneStage**:
  - Normalized touch pad capturing `down`, `move`, and `release` gestures.
  - Scancode keyboard relay: physical keys relayed as `KeyboardEvent.code` — phone decodes with its own layout; dead keys compose on-device; Ctrl+V reads clipboard directly (`navigator.clipboard.readText()`).
  - Enforces `isDeviceActive`: automatically closes connections, dismisses drawer, and displays the "No active session" empty state on session stop.
  - Real-time device status pills reflect `DeviceHealth` flags (installed / dev-mode / locked / paired) instantly.

### 3.5 VPS Runner IPA Distribution
- **Endpoint**: `https://meridianhub.cc/runner/manifest.json` + `https://meridianhub.cc/runner/MeridianRunner-unsigned.ipa`
- **Manifest**: `{ version, url, sha256, size }` — hub polls on sideload, downloads only when version changed, verifies SHA-256 + size.
- **Nginx**: `location /runner/ { alias /var/www/meridian-runner/; add_header Cache-Control "no-cache" always; }` — static serving with no cache.
- **Publish helper**: `runner/tools/publish_ipa.sh` — uploads IPA + regenerates manifest on VPS.
