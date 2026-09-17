# Meridian Hub — Windows Update & Packaging Architecture (Planned)

> **STATUS: NOT IMPLEMENTED.** This is the agreed-upon design for a future
> version. The current `apps/hub-rust` still ships as a plain release binary
> with **no installer and no auto-update** (the only updater ever written lived
> in the now-deleted legacy Python hub, `apps/hub/`). Implement this
> architecture in a dedicated future milestone; do not half-apply it to new
> features before that milestone exists.
>
> Locked decisions (approved): **D1** Inno Setup installer · **D2** forced
> update, no "Later" escape · **D3** machine-wide install to
> `C:\Program Files\Meridian` + auto-elevating scheduled task · **D4** best
> possible security for *free* (TLS + SHA-256 + Ed25519 signed manifest, pinned
> public key; no paid code-signing cert) · **D6** at-logon auto-start only (no
> pre-login service split).

---

## 1. Goals & Non-Goals

**Goals**
1. A real Windows installer that prompts UAC **exactly once** and then runs the
   hub as admin forever with **zero further prompts**.
2. Update delivery from the Meridian VPS (`meridianhub.cc`):
   - Hub's **very first action on launch** is to check the VPS manifest.
   - New version found → **forced** one-click "Download & Install" prompt
     (no "Later", no skip — this is a must per spec).
   - Download → verify → install → **the new version starts itself**.
   - Update is applied **at startup**, before any device/tunnel/session is
     active, so applying never interrupts a live session.
3. A clean, minimal product: the installer ships only `meridian.exe`,
   `meridian-updater.exe`, and the icon — no dead sidecars.
4. Rollback safety: keep one previous binary until the new one self-certifies.

**Non-Goals (for now)**
- Pre-login auto-start (would require a Windows-service + GUI split, D6).
- Paid Authenticode code signing / SmartScreen zero-warning (D4 keeps this free).
- MSI/GPO deployment (D1 default is Inno EXE; MSI noted as a future option).
- macOS/Linux auto-update (out of scope; Linux can later reuse the simple
  replace+execv pattern described in §7.5).

---

## 2. End-to-End Flows

### 2.1 Fresh install
1. User runs `MeridianHub-Setup-<ver>.exe` → **one UAC prompt**.
2. Installer writes `C:\Program Files\Meridian\{meridian.exe, meridian-updater.exe, meridian-icon.png}`,
   Start Menu (+ optional desktop) shortcut, uninstaller.
3. Installer registers scheduled task `MeridianHub` (logon, interactive user,
   `RunLevel=Highest`, program = `meridian.exe`). This is the single source of
   "always running, always admin, never prompt" — the shortcut and the auto-start
   both *run the task*, they never launch the raw exe.
4. Installer launches the task → hub starts → update gate (below) finds nothing
   or is offline → main UI.

### 2.2 Daily launch (up-to-date)
1. hub starts (via task ⇒ already admin).
2. App-level single-instance lock acquired (named mutex) — a second instance
   exits, forwarding focus.
3. Crash-recovery pass: finish any interrupted update; clean stale `.old`.
4. **Update gate**: GET `https://meridianhub.cc/updates/manifest.json`
   (hard timeout ≈ 3 s, static nginx ≈ tens of ms RTT).
5. Version equal or newer → continue normally into the UI.

### 2.3 Update available (forced)
1. Same steps 1–4 above; manifest lists a newer `windows-x86_64` build.
2. **Forced modal**: "Meridian Hub v0.5.0 is available — Download & Install".
   No dismiss. (User can close the window, but the app will not proceed past the
   gate this launch; next launch re-prompts.)
3. Click → streaming download to
   `%LOCALAPPDATA%\meridian\updates\meridian.exe.partial` with progress bar →
   SHA-256 + manifest signature verified → rename to `meridian.exe.new`,
   stamp file written.
4. hub spawns `meridian-updater.exe --wait-pid <hubPid> --target
   "C:\Program Files\Meridian\meridian.exe" --stage <staging>\meridian.exe.new
   --stamp <staging>\update.stamp` then exits.
5. Helper (inherits the hub's elevated token ⇒ no UAC) waits for the hub PID to
   die, re-verifies the stamp (sha256), renames old → `meridian.exe.old`, moves
   new into place, deletes stale `.old` unless rollback needed, then relaunches
   the app through the `MeridianHub` scheduled task.
6. New version starts → its own update gate → self-certifies by writing
   `last_ok_version` into the state file for ≥ 1 clean run → stale `.old`
   cleaned on the next start.
7. If the new exe exits immediately (crash-before-serve), the helper restores
   `meridian.exe.old` and relaunches the old version.

---

## 3. Windows Packaging & UAC (the core mechanism)

- **Manifest (`build.rs`)** keeps `requireAdministrator` only as a *fallback*
  for direct double-clicks. Primary launch path is the scheduled task.
- **Scheduled task** is what makes "never ask for UAC again" true and reliable:
  `schtasks /create /tn MeridianHub /tr <path> /sc onlogon /ru <user> /rl highest /f`.
  Interactive-user + Highest = process runs with an admin token in the user
  session, no prompt, no service.
- **Single-instance mutex** (`core/single_instance.rs`, new): named mutex
  (e.g. `Local\MeridianHub-{user}`) so the logon task and any manual launch or
  post-update relaunch cannot run two hubs that fight over ports / tunnels / the
  update lock.
- **Data location**: keep user data where it already goes —
  `%LOCALAPPDATA%\meridian` (vault, DDI cache, update staging) — so the install
  dir stays effectively read-only at runtime except for the update swap.

---

## 4. Installer (Inno Setup) — D1, D3

Layout under `apps/hub-rust/`:

```
installer/
  meridian-hub.iss       # Inno script
  assets/                # icon, optional banner
build-installer.bat      # cargo release --target x86_64-pc-windows-msvc; bump version; ISCC
```

Installer responsibilities:
- Install files to `C:\Program Files\Meridian\`.
- Create Start Menu + optional desktop shortcut; shortcut compact uses
  `schtasks /run /tn MeridianHub` (no UAC on manual launches).
- Register the `MeridianHub` scheduled task.
- Write installer-declared version + install path to `HKLM\Software\Meridian\Hub`
  (uninstaller already handled by Inno).
- Offer optional "keep user data" toggle — default **keep**
  `%LOCALAPPDATA%\meridian` (vault/DDI/session state).
- (Future) embed the Ed25519 public key into `meridian.exe` builds at compile
  time so the key can't be swapped on disk. See §6.

---

## 5. Update Server (VPS)

### 5.1 Artifact location
`/var/www/meridian-updates/` on `meridianhub.cc`, nginx:

```nginx
location /updates/ {
    alias /var/www/meridian-updates/;
    expires 1d;
}
```

Add to `infra/nginx/meridian.conf` (existing 443 TLS already applies).

### 5.2 Manifest (`/updates/manifest.json`) — the client contract

```json
{
  "signature": "<base64 Ed25519 over the canonical JSON of `latest` below>",
  "latest": {
    "version": "0.5.0",
    "published_at": "2026-09-16T12:00:00Z",
    "notes": "short changelog shown in the forced prompt",
    "platforms": {
      "windows-x86_64": {
        "url":   "https://meridianhub.cc/updates/meridian-0.5.0-windows-x86_64.exe",
        "sha256": "<hex sha256 of the exe>",
        "size":   47123456
      }
    }
  }
}
```

Rules: `version` strict semver; client accepts only exact `sha256`/`size`;
client treats manifest as authoritative (downgrades never offered).

### 5.3 Publish step
Small script (PowerShell for the Windows builder; CI job later) at
`scripts/publish-update.ps1`:
1. Build release (`build.bat`).
2. Rename to `meridian-<ver>-windows-x86_64.exe`.
3. Compute `sha256` + `size`, sign the `latest` block with the VPS-held Ed25519
   private key, write `manifest.json`.
4. `scp` both files to the VPS updates dir (old versions pruned, keep ≈2).
5. (Optional) bump `apps/web` heartbeat to report `hub_version` so stale hubs
   are visible in the web UI — see §7.6.

---

## 6. Security-for-free (D4)

- **Transport:** all update traffic over existing HTTPS (nginx TLS) — never plain HTTP.
- **Integrity:** SHA-256 pinned in the manifest and re-verified at every step
  (download, pre-apply, helper apply).
- **Authenticity (free):** the manifest `latest` block is signed with the
  **VPS-held Ed25519 private key**; the *public* key is embedded in
  `meridian.exe` at compile time. The client verifies the signature before
  trusting `version`/`url`/`sha256`. This holds even if someone tricks TLS via a
  poisoned local root store, since a forked manifest fails signature check.
  - Key management: private key lives only on the VPS (never in repo or CI logs);
    key rotation documented separately; stale-keys rollback risk noted.
- **Explicitly out (paid):** Authenticode signing (EV/OV) so SmartScreen stops
  warning "unknown publisher". Free mitigations today: HTTPS, signed manifest,
  unique clearly-named artifact, consistent versioning. Document for budget-time.
- **SmartScreen note:** unsigned downloads may warn once per device; this is
  cosmetic, not a security hole, given the signed manifest.

---

## 7. Client-Side Design (`apps/hub-rust/src/autoupdate/`)

```
autoupdate/
  mod.rs        # orchestrates the startup update gate; exposed Messages to app.rs
  manifest.rs   # fetch + parse + signature-verify manifest.json
  version.rs    # strict semver compare
  download.rs   # streaming download → .partial → verify → atomic rename → .new
  state.rs      # %LOCALAPPDATA%\meridian\updates\state.json
  apply.rs      # spawn meridian-updater.exe helper (Windows); replace+execv (Unix)
```

### 7.1 Startup gate order (before the Iced main window is interactive)
1. Single-instance lock.
2. **Crash recovery / pending apply:** if a `.new` + valid stamp exists (an
   interrupted previous update), run the apply now (§7.4) before anything else.
3. State cleanup: delete `meridian.exe.old` once `last_ok_version >=" applied`
   for a full clean start.
4. **Verbatim forced gate:** brief "Checking for updates…" splash (≈≤3 s hard
   timeout; static endpoint is fast). Offline → proceed to UI silently.
5. Newer version present → **forced modal** (no dismiss) → download+verify →
   apply → relaunch. Older/equal/offline → normal UI.

### 7.2 State file (`state.json`)
```json
{
  "running_version": "0.4.0",
  "last_ok_version": null,
  "pending": { "version": "0.5.0", "sha256": "…", "status": "downloaded" },
  "applied_version": null
}
```
Single source of truth for recovery; written transactionally (write-temp-rename).
An `install.lock` (separate file, `create_new`) prevents double-apply races.

### 7.3 Download & verify
- Stream to `<staging>/meridian.exe.partial` (tokio + reqwest `stream()`), report
  progress to the modal.
- Verify `sha256(partial) == manifest.latest.platforms.windows-x86_64.sha256`
  and `size`; on success rename `.partial` → `.new` and persist `pending`.
- Any failure → delete `.partial`, show error, retry (forced — no exit hatch).

### 7.4 Apply (Windows) — helper-based, no interruption
Because the gate runs before any service is started, there is **no active
session/tunnel/heartbeat** to interrupt — that is the whole point of applying at
startup.
1. hub re-verifies the stamp, spawns detached `meridian-updater.exe` with
   `--wait-pid <pid> --target <install>\meridian.exe --stage <staging>\meridian.exe.new --stamp <staging>\update.stamp`, exits.
2. Helper polls parent PID → dies → re-verify sha256 from stamp → rename
   `meridian.exe` → `meridian.exe.old` (delete any stale `.old` first) → move
   `.new` → `meridian.exe`.
3. If new exe exits non-zero/crashes immediately → restore `.old`, relaunch old.
4. Normal case → relaunch through `schtasks /run /tn MeridianHub`.
5. Rollback retention: `.old` survives until the new version records
   `last_ok_version` (i.e., it survived one complete gate pass); then it is
   deleted on a subsequent start.

### 7.5 Unix (future, thin)
Single binary: download to state dir → `atomic replace + os::exit`/`execv` with
the staged path (pattern proven by the old Python updater). No helper needed.

### 7.6 Telemetry hook
Add `hub_version: env!("CARGO_PKG_VERSION")` to the existing heartbeat payload
(`remote/heartbeat.rs`) and persist it in the `devices`-adjacent hub record in
`apps/web` so the Control Center can flag outdated hubs.

---

## 8. Edge Cases / Failure Matrix

| Case | Behavior |
|---|---|
| VPS unreachable at launch | Gate times out (≈3 s) → open normally, no nag. |
| Partial/corrupt download | `.partial` discarded; retry at next launch (forced). |
| Signature mismatch / forged manifest | Abort update, log, run current version. |
| Interrupted apply (kill during swap) | Next launch: recovery pass re-applies or restores from `.old`. |
| New binary crashes on start | Helper restores `.old`; old version relaunches. |
| Two instances racing to update | Named mutex (one hub) + `install.lock` (one apply). |
| Downgrade attempt | Semver guard — never install lower/equal. |
| AV/SmartScreen warning | Cosmetic; security covered by signed manifest + TLS. |
| Guarded write to `Program Files` | Always elevated via task; helper inherits token. |

---

## 9. Phased Implementation Checklist

- [ ] **Phase 1 — VPS infra**
  - nginx `/updates/` block (`infra/nginx/meridian.conf`).
  - Ed25519 keypair provisioning on VPS (private key off-repo).
  - `scripts/publish-update.ps1` + manifest template; seed a placeholder artifact.
- [ ] **Phase 2 — client core**
  - `autoupdate/` module: manifest fetch+verify, semver, download, state.
  - Startup gate in `app.rs` with splash + forced modal + progress.
  - `core/single_instance.rs` named mutex.
  - Heartbeat `hub_version` + web storage of it.
- [ ] **Phase 3 — apply & helper**
  - `src/bin/meridian-updater.rs` (tiny, ~200 KB release, elevated-inherit).
  - `apply.rs` spawn/verify/relaunch + rollback.
  - Crash-recovery pass + `.old` cleanup in the gate.
- [ ] **Phase 4 — installer**
  - `installer/meridian-hub.iss`, `build-installer.bat`, icon/shortcuts.
  - Scheduled task registration; shortcut runs the task.
  - Uninstaller: remove task + registry + optional data.
- [ ] **Phase 5 — dead-weight verification**
  - Confirm final installer payload = `meridian.exe` + `meridian-updater.exe` + icon only.
  - Confirm `build.sh`/`build.bat` no longer copy sidecars (already cleaned in the
    repo-cleanup commit that created this document).
- [ ] **Phase 6 — hardening (budget-time)**
  - Authenticode cert; SmartScreen reputation; optional MSI/GPO variants.

---

## 10. Appendix — Repo Cleanup already performed (creation commit)

This document was created as part of a repo-cleanup commit that **removed**:
- `apps/hub/` — legacy Python orchestrator (`meridian_py`).
- `sidecar/` — Go Tailscale `tsnet` mesh client (superseded by in-process rathole).
- `apps/hub-rust/bin/` — dead sidecar binaries (`meridian-mesh*`, `zsign*`).
- Sidecar copy steps in `apps/hub-rust/build.sh` and `build.bat`.

Remaining known-stale text (left untouched because it lives in uncommitted WIP
files): `README.md` lines ~45/104 and `docs/ARCHITECTURE.md` ~line 43 still
mention the `meridian-mesh` sidecar and `dist/bin/`. Update those once the WIP is
committed.