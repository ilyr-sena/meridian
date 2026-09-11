# VPS Rebuild Plan

**New Instance**: t3.small EC2 (2 vCPU, 2GB RAM, 16GB storage, Debian 13)
**IP**: 98.84.189.148 (Elastic IP)
**SSH Key**: `/home/sooku/Downloads/001-KEY.pem`
**SSH Port**: 122
**Username**: admin
**Domain**: meridianhub.cc (A record → 98.84.189.148)

---

## Credentials

### MongoDB
| User | Password | Roles | Auth DB |
|------|----------|-------|---------|
| `meridian_app` | `Rb2Tz#dfyY81Qm` | readWrite, dbAdmin on `meridian` | admin |
| `meridian_admin` | `gpeIlJX%b5e@Wz` | userAdminAnyDatabase, readWriteAnyDatabase, dbAdminAnyDatabase | admin |

MongoDB URI (for web app): `mongodb://meridian_app:Rb2Tz%23dfyY81Qm@127.0.0.1:27017/meridian?authSource=admin`

### SSH
- Key: `/home/sooku/Downloads/001-KEY.pem` (port 122, key-only auth, root disabled)

---

## Port Allocation Scheme

Ports are **dynamically assigned** by hub-rust based on connected devices. NOT fixed to UDIDs.

Each device gets 3 ports: Stream, WDA, Bridge.

| Device # | Stream Port | WDA Port | Bridge Port |
|----------|-------------|----------|-------------|
| 1        | 19200       | 18100    | 19001       |
| 2        | 19201       | 18101    | 19002       |
| 3        | 19202       | 18102    | 19003       |
| ...      | ...         | ...      | ...         |
| N        | 19200+(N-1) | 18100+(N-1) | 19000+N |

**Pattern:**
- Stream:  **19200 + device_index** (19200, 19201, 19202, ...)
- WDA:     **18100 + device_index** (18100, 18101, 18102, ...)
- Bridge:  **19001 + device_index** (19001, 19002, 19003, ...)

**Port range needed**: 18100–18199, 19001–19099, 19200–19299 (supports up to 100 devices)

**Control port**: 2333 (single, shared by all clients)

**Omnisette**: 6969 (Apple anisette headers for sideloading — needs provider files to function)

---

## EC2 Security Group Inbound Rules

| Port | Protocol | Purpose |
|------|----------|---------|
| 122  | TCP      | SSH |
| 80   | TCP      | HTTP (certbot redirect) |
| 443  | TCP      | HTTPS (web app + stream) |
| 6969 | TCP      | Omnisette server |
| 2333 | TCP      | Rathole control (single, shared) |
| 18100-18199 | TCP | Rathole WDA (per device) |
| 19001-19099 | TCP | Rathole Bridge (per device) |
| 19200-19299 | TCP | Rathole Stream (per device) |

**Note**: Port 3000 is NOT exposed. Nginx proxies 443→localhost:3000 internally.

---

## Implementation Status

### Phase 1: Fresh Instance Setup & Hardening ✅
- [x] 1.1 SSH into new instance, verify connectivity
- [x] 1.2 Admin user exists with sudo
- [x] 1.3 SSH key deployed
- [x] 1.4 Disable root login
- [x] 1.5 Change SSH port from 22 → 122
- [x] 1.6 Enable UFW firewall
- [x] 1.7 Allow only specific ports
- [x] 1.8 Enable automatic security updates
- [x] 1.9 Install base packages (curl, wget, git, build-essential, unzip)

### Phase 2: MongoDB Hardening ✅
- [x] 2.1 Install MongoDB 8.0.30 (bookworm repo, compatible with Debian 13)
- [x] 2.2 Enable authentication (SCRAM-SHA-256)
- [x] 2.3 Bind to 127.0.0.1 only (NOT exposed to internet)
- [x] 2.4 Create admin + app users with correct roles
- [x] 2.5 Restore from Atlas dump (meridian database, 14 documents)
- [x] 2.6 Systemd service (`mongod.service`, type=simple, enabled)

**Note**: `storage.journal.enabled` removed (deprecated in MongoDB 8.0). `systemLog.pidFilePath` removed (unrecognized in 8.0).

### Phase 3: Services Setup ✅
- [x] 3.1 Install Node.js 22.x (v22.23.2)
- [x] 3.2 Install pnpm (12.3.4)
- [x] 3.3 Install Nginx
- [x] 3.4 Install rathole server binary (v0.5.0, x86_64-unknown-linux-gnu)
- [x] 3.5 Install Omnisette server (SideStore/omnisette-server 0.2.0, systemd service created, NOT started — needs Apple anisette provider data files)
- [x] 3.6 Install certbot (4.0.0)

### Phase 4: SSL + Nginx ✅
- [x] 4.1 Nginx configured: 80→443 redirect, 443→Next.js:3000
- [x] 4.2 Self-signed cert active (temporary)
- [x] 4.3 SSL cert — Let's Encrypt, valid until 2026-12-10, auto-renewal configured

### Phase 5: Web App Deploy ✅
- [x] 5.1 Clone repo from github.com/ilyr-sena/meridian.git
- [x] 5.2 Install dependencies (pnpm install)
- [x] 5.3 Build Next.js app
- [x] 5.4 Setup PM2 (`pm2 start npm --name meridian -- run start -- -p 3000`)
- [x] 5.5 Configure .env.production with MongoDB auth + production URLs
- [x] 5.6 PM2 startup configured for boot persistence

### Phase 6: Rathole Setup ✅
- [x] 6.1 Install rathole server binary at `/usr/local/bin/rathole-server`
- [x] 6.2 Create config at `/home/admin/meridian/rathole-server.toml`
- [x] 6.3 Create systemd service (`rathole-server.service`, enabled)
- [x] 6.4 Start and verify listening on control port 2333
- [x] 6.5 Data ports (18100, 19001, 19200) will open when hub-rust client connects

### Phase 7: Sysctl TCP Tuning ✅
- [x] 7.1 BBR congestion control
- [x] 7.2 TCP buffer sizes (16MB max)
- [x] 7.3 Low latency options (tcp_low_latency, tcp_no_metrics_save)
- [x] 7.4 Persist to `/etc/sysctl.d/99-meridian.conf`

### Phase 8: Automated Backups ✅
- [x] 8.1 Cron job at 3 AM UTC daily (`/etc/cron.d/mongodb-backup`)
- [x] 8.2 Backups stored in `/home/admin/meridian/backups/`
- [x] 8.3 7-day retention (auto-cleanup)
- [x] 8.4 First backup completed (76KB)

### Phase 9: Final Verification ✅
- [x] 9.1 All services running (MongoDB, Nginx, PM2, Rathole)
- [x] 9.2 Web app accessible (HTTP 200 in 8ms)
- [x] 9.3 Rathole control port listening on 2333
- [x] 9.4 MongoDB accessible with auth
- [x] 9.5 UFW firewall active with only planned ports
- [x] 9.6 All documentation updated with new IP

---

## Hub-Rust Changes

Updated in this session:
- `apps/hub-rust/src/remote/mesh.rs`: VPS_ADDR + VPS_HOST → `98.84.189.148`
- `apps/hub-rust/src/ui/tabs/status.rs`: VPS endpoint display → `98.84.189.148:2333`

---

## Docs Updated

All references to `100.51.75.20` replaced with `98.84.189.148`:
- `README.md`
- `docs/ARCHITECTURE.md`
- `docs/PORTS.md`
- `docs/STREAM_DIAGNOSTICS_AND_AI_HANDOFF.md`
- `docs/VPS_REBUILD_PLAN.md`

SSH key references updated: `LightsailDefaultKey-us-east-1.pem` → `001-KEY.pem`
SSH port references updated: 22 → 122

---

## Known Issues

1. **Omnisette** — Binary installed at `/usr/local/bin/omnisette-server`, systemd service created but NOT started. Needs Apple anisette provider data files to generate genuine FairPlay headers. Without these files, it panics at startup.

---

## Service Management Commands

```bash
# SSH
ssh -i /home/sooku/Downloads/001-KEY.pem -p 122 admin@98.84.189.148

# MongoDB
sudo systemctl status mongod
mongosh --host 127.0.0.1 --port 27017 --username meridian_app --password 'Rb2Tz#dfyY81Qm' --authenticationDatabase admin

# PM2 (web app)
pm2 status
pm2 restart meridian
pm2 logs meridian

# Rathole
sudo systemctl status rathole-server
sudo journalctl -u rathole-server -f

# Nginx
sudo nginx -t && sudo systemctl reload nginx

# Backups
ls -la /home/admin/meridian/backups/
mongodump --uri='mongodb://meridian_app:Rb2Tz%23dfyY81Qm@127.0.0.1:27017/meridian?authSource=admin' --out=/home/admin/meridian/backups/manual-$(date +%Y%m%d)
```

---

*Last updated: 2026-09-11 — All phases complete, SSL live*
