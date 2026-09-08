# Meridian Windows Host Setup Guide

This guide details setting up a Windows computer hosting physical iOS devices connected via USB.

---

## 1. Prerequisites on Windows Host

1. **Apple Mobile Device Support**:
   - Install **iTunes** (direct standalone installer from Apple, **not** the Microsoft Store version) OR install **Apple Devices** / **iCloud**.
   - Verify that Apple Mobile Device Service (AMDS) is running in `services.msc`.
   - Verify that port `27015` is listening locally:
     ```cmd
     netstat -ano | findstr 27015
     ```
2. **Tailscale**:
   - Install Tailscale on the Windows machine.
   - Alternatively, Meridian Hub embeds the `meridian-mesh` Go sidecar (`meridian-mesh.exe`), which connects autonomously via pre-authenticated auth keys.
   - Verify your Tailscale IP:
     ```cmd
     tailscale ip -4
     ```

---

## 2. iPhone Configuration (iOS 17 through iOS 27)

1. Connect the iPhone to the Windows PC via a reliable Lightning or USB-C cable.
2. Tap **"Trust This Computer"** and enter the device passcode.
3. Enable **Developer Mode**:
   - Open **Settings > Privacy & Security > Developer Mode**.
   - Toggle **Developer Mode ON**.
   - Restart the iPhone and tap **Turn On** when prompted.

---

## 3. Running Meridian Hub (Pure Rust)

### Option A: Running the Standalone Executable
Download or build the standalone binary:
```powershell
.\apps\hub-rust\dist\meridian.exe
```

### Option B: Building from Source
Ensure Rust toolchain (1.80+) is installed:
```powershell
cd apps\hub-rust
.\build.bat
.\dist\meridian.exe
```

---

## 4. Sideloading the Runner

1. Connect your iPhone via USB.
2. The Meridian Hub UI displays the device card with automatic status checks:
   - USB connection active.
   - Device pairing and lockdown validated.
   - Runner installation detected.
3. If `MeridianRunner` is not installed, click **"Sideload Runner"**:
   - Enter your Apple ID and password (or use anisette + provisioning).
   - The pure Rust sideloader (`isideload`) signs the prebuilt IPA (`runner\prebuilt\MeridianRunner-unsigned.ipa`) and installs it directly over usbmuxd AFC staging.
4. Click **"Start Session"**:
   - Binds WDA (8100), Bridge (9001), and Stream (9200).
   - Launches `MeridianRunner` on the device via native CoreDevice DVT.
   - Reports presence and active ports to the cloud VPS database.

---

## 5. Remote Access

Open your browser to:
```
https://meridianhub.cc
```
The connected phone will appear as **Online** and **Available**. Click **Use Device** to begin controlling the phone in real time with WebCodecs hardware GPU decoding and responsive touch input.
