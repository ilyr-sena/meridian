# Phase 1 Implementation Tracker

**Goal**: Replace Tailscale DERP relay with rathole direct TCP tunnel. Eliminate transport bottleneck.

**Status**: HUB-RUST COMPLETE. Docs update remaining.

---

## VPS Side (meridianhub.cc)

| # | Task | Status | Notes |
|---|------|--------|-------|
| 1 | Install rathole server binary | DONE | v0.5.0 at `/usr/local/bin/rathole-server` |
| 2 | Create rathole server config | DONE | `/home/admin/meridian/rathole-server.toml`, ports 2333/18100/19001/19200 |
| 3 | Create systemd service | DONE | `rathole-server.service` enabled + running |
| 4 | Open firewall ports | DONE | 2333, 18100, 19001, 19200/tcp |
| 5 | Remove Tailscale | DONE | Binary + state + config purged |
| 6 | Apply sysctl tuning | DONE | BBR, 16MB buffers, swappiness=10 |
| 7 | Optimize memory | DONE | PM2 killed, MongoDB cache=0.25GB |
| 8 | Update Nginx meridian.conf | DONE | 5 server blocks: 8100→18100, 9001→19001, 9200 SSL→19200, 443→3000, 80→443 |
| 9 | Remove Nginx stream.conf | DONE | Removed, include commented out in nginx.conf |
| 10 | Verify Nginx reload | DONE | `nginx -t` passed, reloaded |

**VPS Status**: COMPLETE. Rathole server listening on :2333. Awaiting client connections for service ports.

**Pipeline Status**: All services verified working:
- PM2 meridian app → port 3000 ✅
- MongoDB → port 27017 ✅ (fixed bad config from memory optimization)
- Omnisette → port 6969 ✅
- Heartbeat API → /api/devices/heartbeat ✅
- Rathole server → port 2333 ✅
- Nginx → ports 80/443/8100/9001/9200 ✅ (502 on 9001/8100/9200 expected until rathole client connects)

---

## Hub-Rust Side (Host PC)

| # | Task | Status | Notes |
|---|------|--------|-------|
| 11 | Add rathole crate to Cargo.toml | DONE | `rathole = { version = "0.5", features = ["client"] }`, `toml = "0.8"` |
| 12 | Rewrite mesh.rs → rathole tunnel supervisor | DONE | `TunnelSupervisor` spawns rathole client via lib API, writes temp config, broadcast shutdown |
| 13 | Update app.rs (remove mesh_ip, MeshSupervisor → TunnelSupervisor) | DONE | Removed shared_mesh_ip, KeyUrlChanged, AuthKeyChanged, RefreshKeyNow, KeyRefreshed messages |
| 14 | Update heartbeat.rs (remove tailscale_ip fields) | DONE | Removed shared_mesh_ip param, tailscale_ip/mesh_ip from payloads |
| 15 | Clean vault.rs (remove tailscale_key_url, tailscale_auth_key) | DONE | Fields removed from VaultData |
| 16 | Remove key_fetcher.rs | DONE | File deleted |
| 17 | Update remote/mod.rs | DONE | Removed key_fetcher module |
| 18 | Update UI: titlebar.rs (remove "Mesh Offline" text) | DONE | Replaced with tunnel_online bool, "Tunnel Online"/"Tunnel Offline" pill |
| 19 | Update UI: status.rs (remove "TAILSCALE MESH OVERVIEW") | DONE | Now "RATHOLE DIRECT TUNNEL" with VPS endpoint display |
| 20 | Update UI: settings.rs (remove tailscale section) | DONE | Entire TAILSCALE MESH CONFIGURATION section removed |
| 21 | Update UI: app.rs (remove KeyUrlChanged, AuthKeyChanged, RefreshKeyNow messages) | DONE | Removed from Message enum and update handler |
| 22 | Build and verify compilation | DONE | Clean build, zero warnings, 9m 31s |
| 23 | Commit all hub-rust changes | DONE | `60bc27b` — 14 files changed, 1264 insertions, 637 deletions |

---

## Docs

| # | Task | Status | Notes |
|---|------|--------|-------|
| 24 | Update PORTS.md | PENDING | |
| 25 | Update ARCHITECTURE.md | PENDING | |
| 26 | Update STREAM_DIAGNOSTICS_AND_AI_HANDOFF.md | PENDING | |

---

## Architecture

```
Host PC (sooku-pc)                          VPS (meridianhub.cc)
┌──────────────────────┐                    ┌──────────────────────┐
│ iPhone → usbmuxd     │                    │ rathole server       │
│       ↓              │                    │ :2333 (control)      │
│ meridian-hub (Rust)  │                    │ :18100 → host :8100  │
│       ↓              │   outbound TCP     │ :19001 → host :9001  │
│ rathole client ──────│───────────────────→│ :19200 → host :9200  │
│       ↓              │                    │       ↓              │
│ Local ports:         │                    │ Nginx                │
│  :8100 (WDA)         │                    │  :8100 → :18100      │
│  :9001 (Bridge)      │                    │  :9001 → :19001      │
│  :9200 (Stream)      │                    │  :9200 SSL → :19200  │
└──────────────────────┘                    │  :443 → :3000        │
                                            └──────────┬───────────┘
                                                       │
                                                       ▼
                                              Browser connects to
                                              VPS public IP:9200
```

---

## Rathole Token

Shared secret for client-server auth (embedded in hub-rust, configured in VPS):
```
token: "meridian-rathole-2026"
```

---

*Last updated: 2026-09-09 — Hub-rust code complete, VPS pipeline verified, MongoDB fixed*
