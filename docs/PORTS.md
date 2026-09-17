# Meridian Port Matrix & Dynamic Allocation Guide

There are **two** port planes, both derived from the same per-device **slot**:

1. **Host-local ports** — where the hub actually binds on the host PC (USB side). Never exposed to the internet.
2. **Published (rathole) ports** — where the traffic lands on the VPS and is served to the browser through nginx.

---

## 1. Host-Local Ports (host PC, internal)

| Function | Base Port | Formula | Handled By |
| :--- | :--- | :--- | :--- |
| WDA Automation | `8100` | `8100 + slot` | WebDriverAgent runner on iPhone |
| Control Bridge | `9001` | `9001 + slot` | `meridian-hub` (pure Rust bridge) |
| Screen Stream | `9200` | `9200 + slot` | MeridianRunner H.264 engine |

---

## 2. Published Rathole Ports (VPS, browser-facing)

WDA and bridge reach the browser only through nginx: `https://meridianhub.cc/dev/<port>/<path>`
(nginx `location ~ ^/dev/(\d+)/(.*)` proxies to `http://127.0.0.1:<port>/<path>`).
Stream is the exception: it is served over its own stunnel TLS port and the
browser talks to `https://meridianhub.cc:<stream_port>/...` directly.

| Function | Browser Port | Formula | rathole internal bind | stunnel |
| :--- | :--- | :--- | :--- | :--- |
| WDA Automation | `18100` | `18100 + slot` | `127.0.0.1:18100+slot` | — (via nginx) |
| Control Bridge | `19001` | `19001 + slot` | `127.0.0.1:19001+slot` | — (via nginx) |
| Screen Stream | `19200` | `19200 + slot` | `127.0.0.1:19100+slot` | `19200+slot → 19100+slot` |

Rathole control channel: `98.84.189.148:2333`.

---

## 3. Dynamic Slot Allocation (strictly USB connection order)

Slots are assigned dynamically by `SlotManager`, in thread-safe order of physical
USB connection. Slot `N` maps to the port triple above (both planes share `N`).

| Slot # | USB Order | Host-local | Published (rathole) |
| :---: | :--- | :--- | :--- |
| 0 | 1st iPhone | 8100/9001/9200 | 18100/19001/19200 |
| 1 | 2nd iPhone | 8101/9002/9201 | 18101/19002/19201 |
| N | (N+1)th | 8100+N / 9001+N / 9200+N | 18100+N / 19001+N / 19200+N |

When a device detaches:
1. The hub stops its WDA/stream tunnels and bridge server.
2. The hub sends a heartbeat with `host_ports: null` (DB updated in real-time).
3. The slot is released for the next USB-attached device.

---

## 4. Components That Must Agree

Three places encode the port scheme; keep them in sync:

- `apps/hub-rust/src/core/slots.rs` — `BASE_*` and `RATHOLE_*` constants, `RatholePorts`.
- `apps/hub-rust/src/remote/mesh.rs` — rathole client service tokens `meridian-{wda|bridge|stream}-{slot}` and local addresses.
- `infra/rathole-server.toml` — rathole server services with identical tokens, bound to `127.0.0.1` published ports.

---

## 5. Heartbeat Contract

`POST /api/devices/heartbeat` — sent every 6s while a device is online, and once on
state changes. The `host_ports` value carries the **published rathole ports** (not
the host-local ports), and always includes `udid` for `udid ↔ ports` correlation:

```json
{
  "udid": "00008110-…",
  "name": "iPhone",
  "model": "iPhone 13",
  "version": "iOS 27.0",
  "host_ports": { "wda": 18100, "bridge": 19001, "stream": 19200 },
  "status": "online",
  "session_active": true
}
```

The React client keys off `host_ports` (no IP/UUID needed in the URL).

Rathole control channel: `98.84.189.148:2333`.

---

## 6. Runner IPA Distribution (VPS Static Serving)

The unsigned MeridianRunner IPA is served statically from the VPS via nginx for
the hub to fetch during sideload — no companion tool or local file required.

| Function | Browser URL | Served From |
| :--- | :--- | :--- |
| Runner Manifest | `https://meridianhub.cc/runner/manifest.json` | `/var/www/meridian-runner/manifest.json` |
| Runner IPA | `https://meridianhub.cc/runner/MeridianRunner-unsigned.ipa` | `/var/www/meridian-runner/MeridianRunner-unsigned.ipa` |

**Manifest format** (`/runner/manifest.json`):
```json
{
  "version": "1.0",
  "url": "https://meridianhub.cc/runner/MeridianRunner-unsigned.ipa",
  "sha256": "3aafd9bcf14bd8504f18ac2f6397ffc46962acc8e19238fe8c62953558a5bfda",
  "size": 11687731
}
```

The hub polls this manifest on every sideload attempt, downloads the IPA only
when the version changes, and verifies SHA-256 + size before signing + install.

**Nginx config** (inside the 443 server block):
```
location /runner/ {
    alias /var/www/meridian-runner/;
    add_header Cache-Control "no-cache" always;
}
```

**Publish helper**: `runner/tools/publish_ipa.sh` — uploads a freshly-built IPA to
the VPS and regenerates `manifest.json` with correct version + SHA-256.

---

## 7. Public Endpoints Exposed to the Browser

- **Stream (H.264 + MJPG)**: `https://meridianhub.cc:19200/stream.ws` / `…:19200/stream?…` (stunnel TLS)
- **Control WebSocket**: `wss://meridianhub.cc/dev/19001/ws`
- **Installed Apps**: `https://meridianhub.cc/dev/19001/apps.json`
- **App Icon**: `https://meridianhub.cc/dev/19001/icon/<bundle_id>.png`
- **WDA Automation**: `https://meridianhub.cc/dev/18100/status`
- **Runner Manifest**: `https://meridianhub.cc/runner/manifest.json`
- **Runner IPA**: `https://meridianhub.cc/runner/MeridianRunner-unsigned.ipa`