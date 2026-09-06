# Meridian: High-Performance Remote iOS 27 Orchestration Platform

Meridian is a unified, all-in-one ecosystem for remote physical iPhone control, high-framerate hardware video streaming, and multi-device USB lifecycle automation.

---

## Repository Layout

```
MERIDIAN-PROJECT/
├── apps/
│   ├── hub/                 # Desktop Hub (PySide6 GUI + headless CLI daemon)
│   │   ├── bin/             # Bundled binaries: meridian-mesh, meridian-mesh.exe, zsign.exe
│   │   ├── packaging/       # PyInstaller single-binary build scripts (.spec, build_binary.py)
│   │   └── src/meridian_py/ # Core Python daemon, Inspector, Slots, Vault, Bridge, Sideload
│   └── web/                 # Next.js 15 React Control Center (Frontend Dashboard & API)
│       ├── app/             # App Router pages and REST/SSE API endpoints
│       └── components/      # PhoneStage, H264StreamPlayer (WebCodecs GPU), Sidebar
├── packages/
│   ├── ui/                  # Shared Tailwind & Radix UI design system
│   ├── eslint-config/       # ESLint configurations
│   └── typescript-config/   # TypeScript build configs
├── runner/                  # Unified on-device iOS app (MeridianRunner)
│   ├── ProbeApp/            # Swift ScreenCaptureKit H.264 engine & TinyHTTPServer
│   ├── prebuilt/            # Unsigned IPA asset (MeridianRunner-unsigned.ipa)
│   └── tools/               # merge_probe_wda.py (merges Probe into WebDriverAgent)
├── sidecar/                 # meridian-mesh Go tsnet userspace WireGuard source
├── infra/                   # VPS deployment configs
│   ├── nginx/               # Nginx SSL reverse proxy & /dev/{port} streaming configs
│   └── systemd/             # Next.js systemd service file
├── docs/                    # Architectural & operational documentation
│   ├── ARCHITECTURE.md      # Full 2-sided architectural map & byte flow
│   ├── PORTS.md             # Dynamic slot & port allocation matrix
│   └── WINDOWS_HOST_GUIDE.md# Windows host setup and device onboarding
└── .github/workflows/       # Automated CI/CD
    └── unified-runner.yml   # Xcode 27 macOS runner IPA build pipeline
```

---

## Key Features

1. **Strict iOS 27 Architecture**:
   - Engineered specifically for iOS 27 (`ProductVersion.startswith("27")`).
   - Automatically inspects USB trust, device passcode, and AMFI Developer Mode.
2. **Definitive Port Standard (9200 Video Stream)**:
   - Screen stream standard: **Port 9200** (H.264 WebSockets + WebCodecs hardware player).
   - WDA automation: **Port 8100**.
   - CoreDevice HID Touch/Keyboard Bridge: **Port 9001**.
3. **Dynamic Slot Allocation**:
   - Automatically assigns port blocks ordered strictly by USB connection sequence:
     - Slot 0: `WDA:8100`, `Bridge:9001`, `Stream:9200`
     - Slot 1: `WDA:8101`, `Bridge:9002`, `Stream:9201`
     - Slot N: `WDA:8100+N`, `Bridge:9001+N`, `Stream:9200+N`
4. **Zero-Leak Tailscale Security**:
   - Public end users connect only to `https://meridianhub.cc` and `wss://meridianhub.cc/dev/{port}/*`.
   - Host private Tailscale IPs are never exposed to the client browser.
   - Zero mixed-content warnings, full SSL encryption.
5. **Zero-Latency HID Digitizer & Keyboard**:
   - 60Hz native capacitive touchscreen digitizer.
   - 39-byte bitmap HID hardware keyboard reporting with modifier bitmasks.
   - Suppresses iOS on-screen keyboard automatically during typing.
6. **Cross-Platform Host Support**:
   - Windows 10/11: Native support with automated Windows Firewall rule creation and iTunes AMDS TCP 27015 detection.
   - Linux: Full support with `/var/run/usbmuxd`.

---

## Quickstart

### 1. Running the Hub (Host Machine)
```bash
cd apps/hub
pip install -e .
python -m meridian_py hub
```
*(On Windows, run `meridian.exe hub`)*

### 2. Running the Web Control Center (Server / Local Dev)
```bash
pnpm install
pnpm --filter web dev
```
Open [http://localhost:3000](http://localhost:3000)

### 3. Documentation
- Read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the complete two-sided architecture.
- Read [docs/PORTS.md](docs/PORTS.md) for port and slot details.
- Read [docs/WINDOWS_HOST_GUIDE.md](docs/WINDOWS_HOST_GUIDE.md) for Windows configuration.
