# Meridian: Ultra-Low Latency Remote iOS Orchestration Platform

Meridian is a high-performance, unified platform for physical iPhone remote control, hardware-accelerated 60fps video streaming, and automated USB device lifecycle management.

Built for **iOS 17 through iOS 27**, Meridian eliminates third-party dependencies with a **100% pure Rust host hub**, a unified **ScreenCaptureKit + VideoToolbox + WebDriverAgent** on-device runner, and a modern **Next.js 16 WebCodecs GPU** web stage.

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              SIDE 1: THE HOST                               │
│            (Host PC with USB-connected physical iOS devices)                │
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
│   │           meridian-hub (Pure Rust Desktop GUI & Daemon)             │   │
│   │                                                                     │   │
│   │  • Pure Rust CoreDevice: DVT app launch & process killer (Zero Py)  │   │
│   │  • Pure Rust Sideload: isideload (GSA SRP-6a + Anisette + zsign)    │   │
│   │  • Slot Manager: Allocates isolated port blocks per USB order       │   │
│   │      Slot 0: WDA :8100 | Bridge :9001 | Stream :9200                │   │
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
│                    (Cloud VPS: 98.84.189.148 / meridianhub.cc)               │
│                                                                             │
│  ┌────────────────────────┐  ┌───────────────────────┐  ┌────────────────┐  │
│  │ Tailscale Daemon       │  │ MongoDB (:27017)      │  │ Omnisette      │  │
│  │ WireGuard endpoint     │  │ Devices & Sessions DB │  │ Port 6969      │  │
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

## Repository Structure

```
MERIDIAN-PROJECT/
├── apps/
│   ├── hub-rust/            # Production Pure Rust Desktop App (Iced GUI + Tokio daemon)
│   │   ├── src/
│   │   │   ├── device/      # CoreDevice DVT launcher, lockdown, monitor, actions, tunnel
│   │   │   ├── remote/      # Action bridge (:9001), heartbeat sync, mesh sidecar
│   │   │   ├── sideload/    # Pure Rust isideload installer
│   │   │   ├── core/        # Slots, vault, privilege management
│   │   │   └── ui/          # Iced GUI tabs (devices, status, settings, logs, sideload)
│   │   ├── dist/            # Self-contained distribution bundle
│   │   │   ├── meridian     # Standalone Linux binary
│   │   │   └── bin/         # Bundled meridian-mesh sidecar
│   │   ├── build.sh         # Linux release build script
│   │   └── build.bat        # Windows release build script
│   └── web/                 # Next.js 16 React Web Control Center
│       ├── app/             # App Router pages and REST/SSE API endpoints
│       └── components/      # PhoneStage, H264StreamPlayer (WebCodecs GPU), Sidebar
├── runner/                  # Unified on-device iOS app (MeridianRunner)
│   ├── ProbeApp/            # Swift ScreenCaptureKit H.264 engine & TinyHTTPServer
│   ├── prebuilt/            # Precompiled IPA asset (MeridianRunner-unsigned.ipa)
│   └── tools/               # merge_probe_wda.py (merges Probe into WebDriverAgent)
├── sidecar/                 # meridian-mesh Go tsnet WireGuard source
├── infra/                   # VPS deployment configs
│   ├── nginx/               # Nginx SSL reverse proxy configs (meridian.conf, stream.conf)
│   └── systemd/             # Systemd service units
├── docs/                    # Technical documentation
│   ├── ARCHITECTURE.md      # Detailed system architecture and data flows
│   ├── PORTS.md             # Port matrix and dynamic slot assignment
│   ├── WINDOWS_HOST_GUIDE.md# Windows host setup and AMDS configuration
│   └── STREAM_DIAGNOSTICS_AND_AI_HANDOFF.md # Complete findings, diagnostics & handoff
└── .github/workflows/
    └── unified-runner.yml   # macOS-15 Xcode 16 automated IPA build pipeline
```

---

## Key Features & Capabilities

1. **Pure Rust Host (Zero Python)**:
   - Replaced all legacy Python scripts (`pymobiledevice3`, `sideload-engine.py`, `meridian_py`).
   - App launching, process termination, device enrichment, and sideloading run natively via `idevice`, `isideload`, `tokio`, and `iced`.

2. **Real-Time Device Health (Orthogonal State + 3s Poller)**:
   - Combinable `DeviceHealth` model (`installed × developer_mode × locked × paired`) with tri-state logic (`Yes/No/Unknown`).
   - Real-time health poller (3s interval, debounced) emits `Updated` events for install/uninstall, Developer Mode toggle, lock/unlock — UI reflects changes instantly without re-launch.
   - Guarded session states: live sessions only downgraded on hard health regressions (uninstalled/locked/Developer Mode off).

3. **VPS IPA Sideload (No Local File Picker)**:
   - Hub fetches the latest unsigned `MeridianRunner.ipa` from the VPS (`/runner/manifest.json` + `MeridianRunner-unsigned.ipa`).
   - SHA-256 + size verification, local cache, no local file picker ever.
   - One-click "Sideload Runner" button; no "Re-Sideload" button (uninstall → auto-detected → Sideload button reappears).

4. **Vault Credential Auto-Save & Reuse**:
   - Apple ID + password + anisette URL saved to encrypted vault on successful sideload.
   - Next sideload auto-uses vault password when field left blank — no re-prompt unless credentials expired.
   - Works identically on Linux and Windows.

5. **WebCodecs Hardware GPU Streaming (`avc1.64002a`)**:
   - Ultra-low latency H.264 video decoding using browser WebCodecs API (`window.VideoDecoder`).
   - Delivered over secure HTTPS/WSS contexts to eliminate software MSE buffering and drift.
   - `requestAnimationFrame` render loop synchronizes paints with display VSync.

6. **Dynamic Multi-Node Rathole Mesh**:
   - Replaced Tailscale with pure-Rust in-process rathole client; one outbound TCP connection.
   - Dynamic routing: nginx `/dev/<port>/<path>` → loopback rathole service.
   - Rathole control channel `98.84.189.148:2333`; published ports `18100+/19001+/19200+`.

7. **Action & Control Bridge Server (Port 9001)**:
   - HTTP endpoints: `GET /apps.json`, `GET /apps/running.json`, `GET /icon/:bid.png`, `POST /app/launch/:bid`, `POST /app/kill/:pid`.
   - WebSocket (`/ws`): Bidirectional touch coordinate normalization (taps, drags, swipes), keyboard scancode relay (phone uses its own layout), hardware button dispatch, clipboard paste via `navigator.clipboard.readText()`.

8. **Real-Time Session Lifecycle & Port Management**:
   - MongoDB Change Streams coupled with Server-Sent Events (`/api/events`) drive instant UI state changes.
   - Session stop cleanly clears allocated `host_ports` and terminates runner instances on device.

---

## Quickstart

### 1. Build and Run the Pure Rust Hub (Host Machine)
```bash
cd apps/hub-rust
./build.sh
./dist/meridian
```

### 2. Run the Web Control Center (Local Development)
```bash
pnpm install
pnpm --filter web dev
```
Open [http://localhost:3000](http://localhost:3000)

### 3. Production Deployment (VPS)
- Web App runs via PM2: `pm2 status` (process `meridian` on port 3000).
- Nginx terminates SSL for `meridianhub.cc` and proxies ports 80, 443, and 9200.
- MongoDB runs locally on port 27017 (`mongodb://127.0.0.1:27017/meridian`).

---

## Technical Documentation

- **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)**: Comprehensive architectural breakdown.
- **[docs/PORTS.md](docs/PORTS.md)**: Port allocation and slot mapping matrix.
- **[docs/WINDOWS_HOST_GUIDE.md](docs/WINDOWS_HOST_GUIDE.md)**: Windows host onboarding guide.
- **[docs/STREAM_DIAGNOSTICS_AND_AI_HANDOFF.md](docs/STREAM_DIAGNOSTICS_AND_AI_HANDOFF.md)**: Complete root-cause diagnostics, network analysis, credentials, and future engineering roadmap.
