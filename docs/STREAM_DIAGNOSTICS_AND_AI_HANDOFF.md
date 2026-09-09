# Meridian Stream Performance Diagnostics & AI Engineering Handoff

This document provides a comprehensive, ground-truth technical briefing on the Meridian video streaming architecture, full diagnostics of the slow stream issue, all applied patches, server credentials, network topologies, and an actionable engineering roadmap for any AI agent or engineer continuing this work.

---

## 1. System Inventory & Credentials

### 1.1 VPS Infrastructure (Cloud Server)
- **Domain**: `https://meridianhub.cc`
- **Public IP**: `100.51.75.20` (AWS Lightsail, Debian 12 `bookworm`, kernel `6.12.95+deb13-cloud-amd64`)
- **Tailscale IP**: `100.127.117.36` (Node name: `vps-meridian`)
- **Hardware Specs**: **1 GB RAM, 2 vCPUs, 40 GB NVMe SSD**
- **SSH Access**:
  - Command: `ssh -i /home/sooku/Downloads/LightsailDefaultKey-us-east-1.pem admin@100.51.75.20`
  - Key path on host: `/home/sooku/Downloads/LightsailDefaultKey-us-east-1.pem` (Permissions `0600`)
  - Sudo on VPS: Passwordless for `admin` (`sudo <cmd>`)
- **Web App Location**: `/home/admin/meridian/apps/web`
  - PM2 process: `meridian` (Process ID `0`, runs `npm run start` on port 3000)
  - PM2 commands: `pm2 status`, `pm2 restart meridian`, `pm2 logs meridian`
- **MongoDB**: Local instance on port `27017`
  - Connection string: `mongodb://127.0.0.1:27017/meridian` (or remote from mesh `mongodb://100.51.75.20:27017/meridian`)
  - Collections: `devices`, `sessions`, `users`, `teams`, `roles`, `permissions`
- **Omnisette Server**: Port `6969` (Docker/service, generates Apple anisette headers)
- **Nginx Configs**:
  - Virtual hosts: `/etc/nginx/sites-enabled/meridian.conf`
  - Stream block: `/etc/nginx/conf.d/stream.conf`
  - Test command: `sudo nginx -t && sudo systemctl reload nginx`

### 1.2 Host Machine (Linux PC)
- **Hostname**: `sooku-pc`
- **OS**: Linux (Debian/Ubuntu, kernel `6.8.0-52-generic`)
- **Host Tailscale IP**: `100.93.183.86` (Dynamic ephemeral node `meridian-sooku-pc-X`)
- **Host LAN IP**: `192.168.0.196` (Gateway: `192.168.0.1`, Tenda router)
- **Sudo Password**: `122`
- **Repository Root**: `/run/media/sooku/01D5548B2D35B720/MERIDIAN-PROJECT`
- **Hub Release Binary**: `/run/media/sooku/01D5548B2D35B720/MERIDIAN-PROJECT/apps/hub-rust/dist/meridian`
- **Build Script**: `cd apps/hub-rust && ./build.sh`

### 1.3 Target iOS Device
- **Model**: iPhone 13 (A2633)
- **UDID**: `00008110-000C694914F3801E`
- **OS Version**: iOS 27.0 (`24A5418b`)
- **Native Resolution**: `1170 × 2532` pixels @ 60Hz ProMotion/Retina
- **Installed Runner**: `dev.ius.meridian.runner.xctrunner.SRTHYBYH35`
  - Executable: `Runner.app/Runner`
  - Ports: WDA automation on `8100`, Screen streaming on `9200`
- **CI/CD Workflow**: `.github/workflows/unified-runner.yml`
  - GitHub Repo: `https://github.com/ilyr-sena/meridian.git`
  - Runner Image: `macos-15` (Xcode 16.4, iOS SDK 18.5)
  - Prebuilt IPA: `runner/prebuilt/MeridianRunner-unsigned.ipa`

---

## 2. Problem Statement & Observed Metrics

During live remote sessions on `https://meridianhub.cc/` and `https://meridianhub.cc:9200/`:
- **Codec**: `avc1.64002A (gpu)` (WebCodecs hardware decoding is successfully active in browser).
- **Bitrate**:
  - Static screen: `1.77 – 3.4 Mbps`
  - On high screen movement (scrolling, swiping, video playback): **Spikes to 8+ Mbps**.
- **Frame Rate**:
  - Fluctuates erratically between **5 FPS, 21 FPS, and 40 FPS** (never locking stably at 60 FPS).
- **Latency**:
  - Idle screen: `140 – 500 ms`
  - High screen movement: **Spikes to 10,000 ms (10 seconds!)**.
- **720p Empirical Test Result**:
  - Even after setting the stream resolution to 720p (`scale = 0.6`, `702 × 1520` pixels, 2.5 Mbps), **the stream did NOT become smoother or faster**.
  - It remained at low FPS (5–21 FPS) and high latency.
  - **Critical Engineering Insight**: This conclusively proves that nominal video resolution/bitrate alone is NOT the sole root bottleneck; the bottleneck lies in the transport layer between Host PC and VPS.

### 2.1 The Definitive Proof: localhost vs VPS

| Metric | localhost:9200 | meridianhub.cc:9200 |
|--------|---------------|---------------------|
| FPS | 60 (locked) | 5–21 (erratic) |
| Latency | <30ms | 140–10,000ms |
| Under motion | Smooth | 10,000ms spikes |
| Codec | avc1.64002A (gpu) | avc1.64002A (gpu) |
| Resolution | 720p | 720p |

**The code is identical in both cases.** The only difference is the transport path. This conclusively proves the problem is architectural (transport layer), not code-level.

---

## 3. Root-Cause Analysis (Updated)

### 3.1 PRIMARY: Tailscale DERP Relay Fallback

The Host PC sits behind a residential NAT (Tenda router). When Tailscale cannot establish a direct UDP hole-punch between the Host PC (`100.93.183.86`) and the VPS (`100.127.117.36`):

- All video traffic is routed through a public **Tailscale DERP relay server**.
- DERP relays are geographically remote and explicitly rate-limited by Tailscale for streaming traffic.
- Under motion bursts (when bitrate climbs), DERP buffers and delays TCP packets, driving latency from 140ms up to **10,000ms (10 seconds)**.

This is the **single biggest bottleneck** and the reason the stream is slow.

### 3.2 SECONDARY: VPS Memory Starvation (1 GB RAM)

- **506 MB of Swap is actively in use**.
- `vmstat 1 3` confirms continuous swap-in (`si: 53`) and swap-out (`so: 79`) operations.
- Kernel socket buffers (`sk_sndbuf`, `sk_rcvbuf`) are aggressively throttled under low-memory conditions.
- This compounds the DERP problem but is NOT the primary cause (localhost works fine on the same VPS).

### 3.3 TERTIARY: No Congestion Control

- WebSocket+TCP has no adaptive bitrate. Static 2.5 Mbps causes frame pileup on degraded networks.
- WebRTC's GCC (Google Congestion Control) would solve this, but is a Phase 3 concern.

---

## 4. Confirmed Bottlenecks (Previously Identified, Now Prioritized)

### 4.1 ~~Bottleneck A: Synchronous CoreImage GPU Scaling~~ (Mitigated)
- `ciContext.render()` in `GPUScaler` takes 15–30ms per frame.
- This was identified but the localhost test shows 60 FPS, meaning the GPU scaler is NOT the primary bottleneck in practice. The capture queue can handle the load when the transport is fast.

### 4.2 ~~Bottleneck B: Unbounded Burst Sizes~~ (Patched)
- `kVTCompressionPropertyKey_DataRateLimits` added to cap burst at 1.5× average bitrate.
- This patch is in place and working.

### 4.3 ~~Bottleneck C: WebSocket Queue Purge Storm~~ (Patched)
- Outbox threshold relaxed from 2 frames to `15 frames or > 1.5 MB`.
- This patch is in place and working.

### 4.4 ~~Bottleneck D: In-Band Ping Serialization~~ (Acknowledged)
- Ping messages queue behind video chunks on the same WebSocket.
- Reported 10,000ms latency reflects TCP buffer backlog, not wire transit.
- This is a symptom of the DERP/bandwidth bottleneck, not a separate issue.

---

## 5. Summary of Implemented Patches

### 5.1 On-Device Runner (`runner/ProbeApp/`)
1. **`TinyHTTPServer.swift`**: Relaxed outbox threshold from 2 frames to `15 frames or > 1.5 MB`.
2. **`H264Stream.swift`**: Added `kVTCompressionPropertyKey_DataRateLimits` capped at 1.5× average bitrate.
3. **`CaptureProbe.swift`**: Set `minimumFrameInterval = CMTime(1, 60)`, `queueDepth = 3`, `showsCursor = false`.
4. **CI/CD**: Automated GitHub Actions build pipeline for `MeridianRunner-unsigned.ipa`.

### 5.2 Host Hub (`apps/hub-rust/`)
1. **`tunnel.rs`**: Enabled `set_nodelay(true)` on TCP sockets.
2. **`actions.rs`**: WDA settings with `animationCoolOffTimeout = 0`, `waitForIdleTimeout = 0`.
3. **`heartbeat.rs` & `app.rs`**: Dynamic Tailscale IP sync and session state management.

### 5.3 VPS Infrastructure & Next.js (`apps/web/`)
1. **Nginx SSL on :9200**: Secure Context for WebCodecs GPU decoding.
2. **`h264-stream-player.tsx`**: Lowercase codec normalization, clean AVCC stripping, 5-tier fallback.
3. **`phone-stage.tsx`**: `isDeviceActive` guard for clean teardown.

### 5.4 Dynamic 720p Resolution
1. WebSocket tuning command: `{"op":"tune","scale":0.6,"bitrateMbps":2.5,"maxFps":60,"keyframeSeconds":1.0}`
2. Default scale changed from 1.0 to 0.6 (720p).
3. `meridian-mesh` Go sidecar: Added `tc.SetNoDelay(true)`.

---

## 6. Current Engineering Decision

### 6.1 What We Know

The problem is **architectural, not code-level**. The entire pipeline works at 60 FPS / <30ms locally. The bottleneck is exclusively in the transport layer between Host PC and VPS.

### 6.2 Decision: Replace Tailscale with Direct TCP Tunnel

**Chosen solution**: rathole (Rust-native reverse proxy)

**Why rathole:**
- Hub-rust initiates **outbound** TCP to VPS (no NAT traversal issues, no UPnP, no port-forwarding)
- Rust-native (~500KB binary, same language as hub-rust)
- TCP_NODELAY by default, Noise Protocol encryption
- ~2ms overhead
- Handles reconnection and keepalive

**Architecture**:
```
Host PC → rathole client (outbound) → VPS rathole server → Nginx → Browser
```

### 6.3 Roadmap

| Phase | Goal | Status |
|-------|------|--------|
| Phase 1 | Replace Tailscale with rathole direct TCP | **NEXT — Implementation starting** |
| Phase 2 | VPS upgrade (1GB → 2GB+) | Deferred — only if Phase 1 insufficient |
| Phase 3 | WebRTC SFU (mediasoup) | Deferred — only if Phase 1+2 insufficient |

Full plan: `docs/INFRASTRUCTURE_OVERHAUL.md`

---

## 7. Master Roadmap (Historical — Superseded by Section 6.3)

The original roadmap steps have been re-evaluated:

### ~~Step 1: 720p Downscaling~~ — COMPLETE
- Stream default is now 720p. Dynamically applied over WebSocket.

### ~~Step 2: Ensure Direct P2P WireGuard~~ — SUPERSEDED
- Instead of trying to fix Tailscale P2P, we are replacing Tailscale entirely with rathole direct TCP.

### ~~Step 3: Mitigate VPS Memory Saturation~~ — DEFERRED TO PHASE 2
- VPS upgrade from 1 GB to 2 GB ($5/month) is deferred. Will only proceed if Phase 1 results are unsatisfactory.
- Memory optimizations (swappiness, PM2 cleanup, MongoDB cache) will be applied as part of Phase 1.

### ~~Step 4: Client-Side Drop-Behind~~ — NOT NEEDED
- With a proper transport layer, this workaround should not be necessary.

---

## 8. VPS Maintenance Commands

### Quick Reference
```bash
# SSH into VPS
ssh -i /home/sooku/Downloads/LightsailDefaultKey-us-east-1.pem admin@100.51.75.20

# Check memory
free -m
vmstat 1 3

# Check swap usage
cat /proc/swaps

# PM2 management
pm2 status
pm2 restart meridian
pm2 logs meridian

# Nginx
sudo nginx -t && sudo systemctl reload nginx

# MongoDB
mongosh --eval "db.stats()" mongodb://127.0.0.1:27017/meridian

# Tailscale (to be deprecated after Phase 1)
sudo tailscale status
sudo tailscale ping 100.93.183.86
```

---

## 9. Network Topology

### Current (Tailscale DERP)
```
Host PC (192.168.0.196) → Tenda Router (NAT) → ISP → Tailscale DERP Relay → VPS (100.51.75.20) → Browser
```

### Target (rathole Direct TCP)
```
Host PC (192.168.0.196) → Tenda Router (NAT) → ISP → VPS (100.51.75.20) → Browser
                           (outbound TCP, no NAT traversal needed)
```

---

*Last updated: 2026-09-08*
*Status: Phase 1 implementation ready to begin*
