# Meridian Windows Host Setup Guide

This guide details setting up the Windows computer that has physical iPhones connected via USB.

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
   - Log in to the same Tailnet as your VPS (`jezudotdigital@...`).
   - Verify your Tailscale IP:
     ```cmd
     tailscale ip -4
     ```
     (Expected: `100.101.105.127` or similar).

---

## 2. iPhone Configuration (Strict iOS 27 Requirements)

1. Connect the iPhone to the Windows PC via a reliable Lightning or USB-C cable.
2. When prompted on the iPhone, tap **"Trust This Computer"** and enter your passcode.
3. Enable **Developer Mode**:
   - Go to **Settings > Privacy & Security > Developer Mode**.
   - Toggle **Developer Mode ON**.
   - The iPhone will restart. After restart, unlock the phone and tap **Turn On** when prompted.

---

## 3. Running Meridian Hub on Windows

### Option A: Running from Source (Python 3.11 / 3.12)
```cmd
cd apps\hub
python -m pip install -e .
python -m meridian_py hub
```

### Option B: Running the Standalone Executable
Double-click `meridian.exe` or run from PowerShell:
```powershell
.\meridian.exe hub
```

### Option C: Running Headless CLI Daemon (Background)
```powershell
.\meridian.exe hub --cli
```

---

## 4. First-Time Setup & Sideloading

1. On first launch, Meridian will prompt to create Windows Firewall inbound rules. Accept the UAC prompt to allow traffic on ports `8100-8131`, `9001-9032`, and `9200-9231`.
2. Connect your iPhone. The Meridian Hub UI will display a device card with a 6-step inspection checklist:
   - `USB Connection`
   - `Pairing / Trust`
   - `Passcode Check`
   - `iOS 27 Verification`
   - `Developer Mode Status`
   - `MeridianRunner Sideload Status`
3. If `MeridianRunner` is not installed, click **"Sideload Runner"**:
   - Enter your Apple ID and password (or use custom signing certificates).
   - Enter the 2FA verification code when prompted.
   - Meridian will automatically generate development provisioning profiles, sign the unified runner using the bundled `zsign.exe`, and install it onto the phone.
4. Once installed, Meridian launches the runner, establishes tunnels on **Port 9200** (video stream) and **Port 8100** (WDA), arms the **60Hz HID touch digitizer** on **Port 9001**, and pushes status `online` to MongoDB.

---

## 5. Controlling the Phone Remotely

Open your browser to:
```
https://meridianhub.cc
```
The connected phone will appear as **Online** and **Available**. Start a session to control the phone in real time with hardware-accelerated video and native capacitive touch.
