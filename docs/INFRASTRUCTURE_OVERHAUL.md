# Meridian Infrastructure Overhaul Plan

This document defines the architectural changes required to achieve stable 60 FPS streaming and sub-100ms latency by replacing the Tailscale DERP relay transport with direct TCP tunneling, and establishing a clear production-grade infrastructure path.

---

## 1. Problem Summary

| Metric | Expected | Actual (via VPS) | Actual (localhost) |
|--------|----------|-------------------|--------------------|
| FPS | 60 | 5–21 | 60 |
| Latency | <100ms | 140–10,000ms | <30ms |
| Codec | avc1.64002A (gpu) | avc1.64002A (gpu) | avc1.64002A (gpu) |
| Resolution | 720p | 720p | 720p |
| Bitrate | 2.5 Mbps | 1.77–8+ Mbps | 2.5 Mbps |

**The code is correct.** The localhost test proves the entire pipeline — ScreenCaptureKit capture, VideoToolbox H.264 encoding, fMP4 fragment building, WebSocket delivery, WebCodecs GPU decoding — works at full 60 FPS with sub-30ms latency. The bottleneck is **exclusively in the transport layer between Host PC and VPS**.

### Confirmed Bottlenecks (in order of severity)

1. **Tailscale DERP relay fallback** — Host PC behind residential NAT (Tenda router). When UDP hole-punching fails, all traffic routes through a public DERP relay that throttles streaming traffic and adds 150–500ms base jitter. Under motion bursts, DERP buffers delay TCP packets, driving latency to 10,000ms.

2. **VPS memory starvation** — 1 GB RAM with 506 MB active swap. Kernel socket buffer throttling and major page faults during high-throughput proxying. This is a compounding factor but NOT the primary cause.

3. **No congestion control** — WebSocket+TCP has no adaptive bitrate. Static 2.5 Mbps causes frame pileup on degraded networks.

---

## 2. Phase 1: Direct TCP Tunnel (Immediate Win)

### 2.1 Goal

Replace Tailscale DERP relay with a direct outbound TCP tunnel from hub-rust to VPS. This eliminates the relay bottleneck while keeping the current VPS (1 GB RAM, no upgrade).

### 2.2 Why rathole

rathole is a Rust-native lightweight reverse proxy designed for exactly this use case.

| Solution | Latency Added | NAT Handling | Complexity | Production Ready |
|----------|--------------|--------------|------------|-----------------|
| **rathole** | ~2ms | Outbound (no NAT issues) | Low | Yes |
| Custom Rust tunnel | ~1ms | You build it | High | If tested |
| FRP (KCP mode) | ~1.5ms | Outbound | Low | Yes |
| WireGuard direct | ~0.5ms | Manual port-forward | Medium | Yes |
| Cloudflare Tunnel | 10-50ms | Automatic | Low | Yes |
| SSH tunnel | 3-5ms | Automatic | No (for media) |

**rathole wins because:**
- Hub-rust initiates an **outbound** TCP connection to VPS — no inbound NAT issues, no UPnP, no port-forwarding on the Tenda router
- Written in Rust — same language as hub-rust, can be embedded as a library or run as a sidecar
- ~500KB binary, TCP_NODELAY by default, Noise Protocol encryption (no cert management)
- ~2ms overhead, 2-3x throughput over FRP at high QPS
- Handles reconnection, keepalive, and hot-reload natively

### 2.3 Architecture

```
┌──────────────────────────────────────────────────────┐
│  Host PC (Residential NAT — Tenda Router)            │
│                                                      │
│  iPhone (USB)                                        │
│       │                                              │
│       ▼                                              │
│  usbmuxd / AMDS                                      │
│       │                                              │
│       ▼                                              │
│  meridian-hub (Rust, Iced GUI)                       │
│       │                                              │
│       ├── TCP :8100 (WDA tunnel)                     │
│       ├── TCP :9200 (Stream tunnel)                  │
│       └── TCP :9001 (Bridge/Control)                 │
│       │                                              │
│       ▼                                              │
│  rathole client                                      │
│  (outbound TCP to VPS, Noise encrypted)              │
│       │                                              │
└───────┼──────────────────────────────────────────────┘
        │
        │ Direct TCP (outbound-initiated)
        │ TCP_NODELAY enabled
        │ Noise Protocol encryption
        ▼
┌──────────────────────────────────────────────────────┐
│  VPS (AWS Lightsail, 1 GB RAM, Debian 12)           │
│                                                      │
│  rathole server                                      │
│  (listens on mapped ports, accepts outbound)         │
│       │                                              │
│       ├── :8100 → rathole → Host :8100 (WDA)        │
│       ├── :9200 → rathole → Host :9200 (Stream)     │
│       └── :9001 → rathole → Host :9001 (Bridge)     │
│       │                                              │
│       ▼                                              │
│  Nginx (reverse proxy / stream proxy)                │
│       │                                              │
│       ├── :443 → Next.js :3000 (Web UI)             │
│       ├── :9200 SSL → Stream (WebCodecs Secure)     │
│       └── /dev/{ip}/{port} → Dynamic routing        │
│       │                                              │
│       ▼                                              │
│  Browser (WebCodecs GPU decode)                      │
└──────────────────────────────────────────────────────┘
```

### 2.4 How It Works

1. **Session Start**: When a user clicks "Start Session" in hub-rust, the hub:
   - Binds local TCP listeners on ports 8100, 9001, 9200 (existing behavior)
   - Launches rathole client which establishes an **outbound** TCP connection to the VPS rathole server
   - rathole server on VPS maps these connections to its own listening ports

2. **Data Flow**: Video/control traffic flows:
   - Host :9200 → rathole client → (outbound TCP) → VPS rathole server → VPS :9200 → Nginx → Browser
   - No Tailscale involved. No DERP relay. Direct path.

3. **Session Stop**: rathole client disconnects, server releases ports. Clean teardown.

4. **NAT Traversal**: Not needed. The host initiates the connection outbound. The VPS has a public IP. The Tenda router's NAT table maps the outbound connection, and rathole keeps it alive with TCP keepalive.

### 2.5 rathole Configuration

**Host side (rathole client)** — runs inside hub-rust or as a sidecar:

```toml
# rathole client config (embedded or file)
[client]
remote_addr = "meridianhub.cc:2333"  # VPS rathole server
 authentication = { token = "<shared-secret>" }

[client.services.wda]
    bind_addr = "127.0.0.1:8100"
    token = "<per-service-token>"

[client.services.stream]
    bind_addr = "127.0.0.1:9200"
    token = "<per-service-token>"

[client.services.bridge]
    bind_addr = "127.0.0.1:9001"
    token = "<per-service-token>"
```

**VPS side (rathole server)**:

```toml
# rathole server config
[server]
bind_addr = "0.0.0.0:2333"
 authentication = { token = "<shared-secret>" }

[server.services.wda]
    bind_addr = "0.0.0.0:8100"
    token = "<per-service-token>"

[server.services.stream]
    bind_addr = "0.0.0.0:9200"
    token = "<per-service-token>"

[server.services.bridge]
    bind_addr = "0.0.0.0:9001"
    token = "<per-service-token>"
```

### 2.6 Integration Strategy

Two options for integrating rathole into hub-rust:

**Option A: rathole as embedded library (preferred)**
- Use the `rathole` Rust crate directly in hub-rust
- On session start, spawn rathole client as a tokio task
- No external binary dependency
- Full control over lifecycle, logging, reconnection

**Option B: rathole as bundled sidecar binary**
- Bundle `rathole` binary in `apps/hub-rust/dist/bin/` (like `meridian-mesh`)
- Hub-rust spawns rathole as a child process on session start
- Simpler integration but adds a binary dependency
- Easier to update independently

### 2.7 VPS Changes Required

1. **Install rathole server** on VPS:
   ```bash
   # Download rathole binary
   curl -L https://github.com/youxihujing/rathole/releases/latest/download/rathole-linux-amd64 -o /usr/local/bin/rathole-server
   chmod +x /usr/local/bin/rathole-server
   ```

2. **Create systemd service** for rathole server:
   ```ini
   [Unit]
   Description=Rathole Server (Meridian Tunnel)
   After=network.target

   [Service]
   Type=simple
   User=admin
   ExecStart=/usr/local/bin/rathole-server --config /home/admin/meridian/rathole-server.toml
   Restart=always
   RestartSec=3

   [Install]
   WantedBy=multi-user.target
   ```

3. **Update Nginx stream.conf** — no changes needed if rathole binds to the same ports Nginx currently proxies. The flow becomes:
   ```
   Browser → Nginx :9200 → rathole server :9200 → (tunnel) → Host :9200
   ```
   Or alternatively, rathole binds to different ports and Nginx upstreams are updated.

4. **Open port 2333** on VPS firewall for rathole control channel:
   ```bash
   sudo ufw allow 2333/tcp
   ```

5. **VPS sysctl tuning** for high-throughput TCP:
   ```bash
   # /etc/sysctl.d/99-meridian.conf
   net.core.rmem_max = 16777216
   net.core.wmem_max = 16777216
   net.ipv4.tcp_rmem = 4096 87380 16777216
   net.ipv4.tcp_wmem = 4096 65536 16777216
   net.core.netdev_max_backlog = 5000
   net.ipv4.tcp_congestion_control = bbr
   net.core.default_qdisc = fq
   ```

6. **Memory optimization** (without VPS upgrade):
   ```bash
   # Lower swappiness
   sudo sysctl vm.swappiness=10

   # Kill dead root PM2 daemon
   sudo pm2 kill

   # Constrain MongoDB cache
   # Edit /etc/mongod.conf
   storage:
     wiredTiger:
       engineConfig:
         cacheSizeGB: 0.25
   ```

### 2.8 Tailscale Removal (or Demotion)

After rathole is working, Tailscale can be:

- **Removed entirely** from the streaming path (rathole replaces it)
- **Kept for SSH access** only (admin SSH to VPS via Tailscale IP)
- **Kept as fallback** (if rathole fails, fall back to Tailscale temporarily)

The `meridian-mesh` Go sidecar can be deprecated or repurposed.

### 2.9 Expected Results (Phase 1)

| Metric | Current (Tailscale DERP) | After rathole |
|--------|--------------------------|---------------|
| FPS | 5–21 | 40–60 (limited by VPS RAM) |
| Latency | 140–10,000ms | 50–200ms |
| Under motion | 10,000ms spikes | 150–300ms |
| Connection stability | Drops on DERP | Stable (outbound TCP) |

**Note**: The VPS swap thrashing will still limit performance. Phase 1 eliminates the DERP bottleneck but the 1 GB RAM constraint remains. This is expected — the stream should be noticeably better but not yet localhost-fast. Phase 2 (VPS upgrade) or Phase 3 (SFU) addresses the remaining bottleneck.

### 2.10 Implementation Checklist

- [ ] Add rathole crate to hub-rust `Cargo.toml`
- [ ] Implement rathole client lifecycle in `app.rs` (start on session, stop on teardown)
- [ ] Create rathole server config and systemd service for VPS
- [ ] Install rathole server on VPS
- [ ] Open port 2333 on VPS firewall
- [ ] Apply VPS sysctl tuning
- [ ] Apply VPS memory optimizations (swappiness, PM2, MongoDB cache)
- [ ] Test direct TCP tunnel without Tailscale
- [ ] Remove Tailscale from streaming path (keep for SSH if desired)
- [ ] Verify stream performance over public internet
- [ ] Update heartbeat.rs to report rathole tunnel status instead of Tailscale IP
- [ ] Update docs/PORTS.md with new port layout

---

## 3. Phase 2: VPS Upgrade (If Needed)

**Only proceed here if Phase 1 results are still unsatisfactory due to VPS memory constraints.**

### 3.1 When to Upgrade

- If stream FPS is still below 40 after Phase 1
- If `vmstat` still shows swap activity during streaming
- If Next.js + MongoDB + rathole server exceed available RAM

### 3.2 Upgrade Options

| Provider | RAM | vCPU | Storage | Bandwidth | Price |
|----------|-----|------|---------|-----------|-------|
| AWS Lightsail 2GB | 2GB | 2 | 60GB | 2TB | $5/mo |
| AWS Lightsail 4GB | 4GB | 2 | 80GB | 3TB | $10/mo |
| Hetzner CX22 | 4GB | 2 | 40GB | 20TB | €4.49/mo |

### 3.3 Expected Impact

- 2GB RAM eliminates swap thrashing entirely
- MongoDB can cache indexes properly
- OS has ~1.5GB free for buffer cache
- TCP socket buffers are no longer throttled by kernel low-memory pressure

---

## 4. Phase 3: WebRTC SFU (Future — Production Grade)

**Only implement if Phase 1 + Phase 2 still don't meet latency targets, or if multi-viewer support is needed.**

### 4.1 Why WebRTC

| Dimension | WebSocket+fMP4+WebCodecs | WebRTC (SFU) |
|-----------|--------------------------|--------------|
| Congestion control | None (static 2.5 Mbps) | GCC/BBR (adaptive) |
| Head-of-line blocking | Yes (TCP) | No (RTP/UDP) |
| Packet loss recovery | TCP retransmit (stalls all) | NACK/PLI (only lost frame) |
| Jitter buffer | None | Built-in, configurable |
| Under network stress | 300-500ms (TCP collapse) | ~150ms (adaptive) |
| Multi-viewer | Manual duplication | Native SFU fan-out |

### 4.2 Recommended SFU: mediasoup

- C++ media worker (no GC pauses)
- ~100MB RAM (fits alongside Next.js + MongoDB on 2GB+ VPS)
- Node.js API (integrates with existing stack)
- Battle-tested (Whereby, Hopin, Clubhouse)

### 4.3 Architecture (Phase 3)

```
Host PC → WHIP (HTTP POST) → VPS mediasoup SFU → WHEP → Browser
                                                       → Browser 2
                                                       → Browser N
```

### 4.4 Key Features

- **Adaptive bitrate**: SFU sends RTCP REMB → hub adjusts VideoToolbox bitrate in real-time
- **DataChannel control**: Touch/keyboard via WebRTC DataChannel (sub-10ms, replaces WebSocket :9001)
- **Multi-viewer**: Native producer/consumer model
- **Encryption**: DTLS-SRTP end-to-end

---

## 5. Decision Log

| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-09-08 | Bottleneck confirmed as transport layer, not code | localhost:9200 is fast; meridianhub.cc:9200 is slow |
| 2026-09-08 | Tailscale DERP relay identified as primary cause | Host behind NAT → UDP hole-punch fails → DERP fallback |
| 2026-09-08 | Phase 1: rathole direct TCP tunnel | Eliminates DERP, outbound-initiated (no NAT issues), Rust-native |
| 2026-09-08 | VPS upgrade deferred to Phase 2 | Phase 1 is the immediate win; upgrade only if needed after |
| 2026-09-08 | WebRTC SFU deferred to Phase 3 | Only if Phase 1+2 insufficient or multi-viewer needed |

---

## 6. Success Criteria

### Phase 1 Success
- [ ] Stream FPS consistently above 30 (target: 40-60) over public internet
- [ ] Latency consistently below 300ms (target: <200ms)
- [ ] No 10,000ms latency spikes under motion
- [ ] Connection stable for >30 minutes without drops
- [ ] Tailscale removed from streaming path

### Phase 2 Success (if needed)
- [ ] Swap usage drops to 0 MB
- [ ] Stream FPS consistently above 50
- [ ] Latency consistently below 150ms

### Phase 3 Success (if needed)
- [ ] Adaptive bitrate responds to network degradation within 200ms
- [ ] Multi-viewer fan-out works with <50ms additional latency per hop
- [ ] DataChannel control latency <10ms
