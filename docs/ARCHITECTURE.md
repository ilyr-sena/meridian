# Meridian System Architecture

## Overview
Meridian is a high-performance, ultra-low-latency remote iPhone orchestration and interaction platform for **iOS 27** devices. It allows physical iPhones connected via USB to a Host machine (Windows primary target, Linux dev test) to be controlled seamlessly over the web via a centralized Next.js Control Center on the Cloud VPS with sub-10ms input latency and 60fps hardware GPU streaming.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              SIDE 1: THE HOST                               │
│       (Windows Host PC or Linux Dev Machine with USB-Connected iPhones)     │
│                                                                             │
│   [iPhone 1]            [iPhone 2]            ...     [iPhone N]            │
│       │                      │                             │                │
│       └──────────────────────┴──────────────┬──────────────┘                │
│                                      (Physical USB)                         │
│                                             ▼                               │
│                      ┌──────────────────────────────────────────────┐       │
│                      │            usbmuxd / AMDS (iTunes)           │       │
│                      │  • Windows: TCP 127.0.0.1:27015              │       │
│                      │  • Linux:   UNIX /var/run/usbmuxd            │       │
│                      └──────────────────────┬───────────────────────┘       │
│                                             ▼                               │
│   ┌─────────────────────────────────────────────────────────────────────┐   │
│   │               meridian-hub (Desktop GUI & CLI Daemon)               │   │
│   │                                                                     │   │
│   │  • Inspector: Validates Pairing, Trust, Passcode, iOS 27 & AMFI     │   │
│   │  • Sideload Pipeline: GSA auth + Anisette + zsign signer            │   │
│   │  • Slot Manager: Allocates isolated port blocks per USB order       │   │
│   │      Slot 0: WDA:8100 | Bridge:9001 | Stream:9200                   │   │
│   │      Slot 1: WDA:8101 | Bridge:9002 | Stream:9201                   │   │
│   │      Slot N: 8100+N   | 9001+N      | 9200+N                        │   │
│   │  • CoreDevice Bridge: 60Hz HID touch digitizer & 39-byte HW keys   │   │
│   │  • Heartbeat Worker: Syncs presence & host_ports to MongoDB         │   │
│   └─────────────────────────────────────────┬───────────────────────────┘   │
│                                             │                               │
│                                             ▼                               │
│                         Tailscale Node (100.101.105.127)                    │
└─────────────────────────────────────────────┬───────────────────────────────┘
                                              │  WireGuard Tailnet Mesh
                                              ▼  (Private & Encrypted)
┌─────────────────────────────────────────────┴───────────────────────────────┐
│                              SIDE 2: THE SERVER                             │
│                  (Cloud VPS: 100.127.117.36 / meridianhub.cc)               │
│                                                                             │
│  ┌────────────────────────┐  ┌───────────────────────┐  ┌────────────────┐  │
│  │ Tailscale Daemon       │  │ MongoDB Atlas / Local │  │ Omnisette      │  │
│  │ WireGuard endpoint     │  │ Devices & Sessions DB │  │ Port 6969      │  │
│  └────────────────────────┘  └───────────────────────┘  └────────────────┘  │
│                                          ▲                                  │
│                                          │                                  │
│  ┌───────────────────────────────────────┴───────────────────────────────┐  │
│  │                  Next.js 15 Control Center (Port 3000)                │  │
│  │  • Live device presence & leasing engine                              │  │
│  │  • WebCodecs GPU stream receiver                                      │  │
│  │  • Real-time mouse coordinate & keyboard event normalizer             │  │
│  └───────────────────────────────────────▲───────────────────────────────┘  │
│                                          │                                  │
│  ┌───────────────────────────────────────┴───────────────────────────────┐  │
│  │                  Nginx Reverse Proxy (:80 / :443 SSL)                 │  │
│  │  • Terminates SSL (Let's Encrypt for meridianhub.cc)                  │  │
│  │  • Proxies Web UI: https://meridianhub.cc -> 127.0.0.1:3000           │  │
│  │  • Zero-Leak Device Proxy:                                            │  │
│  │      wss://meridianhub.cc/dev/{port}/ws        -> Host:{port}/ws      │  │
│  │      wss://meridianhub.cc/dev/{port}/stream.ws -> Host:{port}/stream  │  │
│  │      https://meridianhub.cc/dev/{port}/apps    -> Host:{port}/apps    │  │
│  └───────────────────────────────────────▲───────────────────────────────┘  │
└──────────────────────────────────────────┼──────────────────────────────────┘
                                           │  HTTPS / WSS with SSL
                                           ▼  Zero Leaked Tailscale IPs
┌─────────────────────────────────────────────────────────────────────────────┐
│                              END-USER BROWSER                               │
│  Any user accessing https://meridianhub.cc from anywhere in the world       │
│  Interacts with the remote iPhone with instant zero-latency feedback        │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## The Two Sides in Detail

### Side 1: The Host Machine (Windows Primary / Linux Dev)
1. **Device Detection & Verification**:
   - Communicates with iTunes `AppleMobileDeviceService` on Windows (port `27015`) or `/var/run/usbmuxd` on Linux.
   - Detects connected iPhones and runs the deterministic onboarding machine:
     - USB connection active.
     - Device paired & trusted.
     - Device passcode configured.
     - Target OS is strictly iOS 27 (`ProductVersion.startswith("27.")` or `"27"`).
     - AMFI Developer Mode enabled.
     - `MeridianRunner` sideloaded and present.
2. **Automated Sideload Pipeline**:
   - If `MeridianRunner` is not installed, Meridian Hub automatically prompts or triggers sideloading:
     - Fetches Apple anisette authentication headers from the VPS omnisette server (`http://100.127.117.36:6969`) or local fallback.
     - Performs Apple GrandSlam (GSA) SRP-6a login and requests Xcode delegation tokens.
     - Obtains a genuine development certificate and provisioning profile.
     - Invokes `zsign` (`zsign.exe` on Windows) to sign nested bundles deepest-first.
     - Installs signed IPA via CoreDevice `InstallationProxyService`.
3. **Port Slot Allocation**:
   - Dynamically reserves port blocks strictly in order of USB connection on the host:
     - **Slot 0**: WDA `8100`, Control Bridge `9001`, Screen Stream `9200`.
     - **Slot 1**: WDA `8101`, Control Bridge `9002`, Screen Stream `9201`.
     - **Slot N**: WDA `8100+N`, Control Bridge `9001+N`, Screen Stream `9200+N`.
4. **Zero-Latency Control Bridge**:
   - Listens on `localhost:9001+N`.
   - Mounts a permanent CoreDevice HID keyboard service (`kid=4294975489`) and capacitive touchscreen digitizer.
   - Dispatches keystrokes using genuine 39-byte HID reports with modifier bitmaps (automatically suppressing the iOS on-screen keyboard).
   - Relays 60Hz mouse taps, clicks, drags, swipes, and Home/Lock actions directly to the iPhone digitizer.
5. **Screen Streaming Relay**:
   - Launches `MeridianRunner` on the phone.
   - Creates persistent usbmux tunnel mapping host port `9200+N` -> iPhone port `9200`.
   - Exposes hardware H.264 video WebSocket (`/stream.ws`), MJPEG fallback (`/stream`), and WebCodecs player (`/stream.html`).
6. **Presence & Heartbeat**:
   - Sends periodic heartbeats (every 10s) to `https://meridianhub.cc/api/devices/heartbeat`.
   - Updates status in MongoDB (`online` / `offline`), model, OS version, and host ports.
   - On exit, sends `status: "offline"` and terminates `MeridianRunner` on the iPhone.

---

### Side 2: The Server / VPS (Cloud & Control Center)
1. **Tailscale Private Mesh**:
   - Connects the VPS (`100.127.117.36`) and Host PC (`100.101.105.127`) over an encrypted WireGuard tunnel.
2. **Omnisette Server (:6969)**:
   - High-throughput Apple anisette service supplying required cryptographic headers for Apple ID signing.
3. **MongoDB (:27017)**:
   - Houses the `devices` and `sessions` collections.
   - Devices maintain `status: "online"` or `"offline"`, leased session status, and port mappings.
4. **Nginx Reverse Proxy & Zero-Leak Gateway**:
   - Public users connect to `https://meridianhub.cc`.
   - Nginx handles SSL termination with Let's Encrypt certificates.
   - Routes `/dev/{port}/*` over the internal Tailscale network (`100.101.105.127`) to the host.
   - **Crucial**: Public web clients NEVER connect to or see the host's private Tailscale IP. No mixed-content errors, no port forwarding required on the host router.
5. **Next.js React Control Center (:3000)**:
   - Built on Next.js 15 + Tailwind CSS + Lucide icons.
   - Responsive dark-theme dashboard showing connected devices and session status.
   - Interactive Phone Stage:
     - Real-time WebCodecs GPU stream playback.
     - Direct mouse-to-digitizer coordinate mapping.
     - Hardware keyboard event forwarding.
     - Application launcher and task killer drawer.
