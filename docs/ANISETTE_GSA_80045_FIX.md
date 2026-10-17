# GSA `-80045` Incident & Fix — Sideload Apple ID Login (2026-10-17)

## Symptom

Sideloading via hub-rust failed at the Apple ID login step:

```
Sideload failed: Apple ID login failed:
 ● Failed to log in to Apple ID
 ● GrandSlam error during initial login request
 ● Auth error -80045: This action cannot be completed at this time.
```

The failure occurred on the **first** GSA request (`o: init` to
`https://gsa.apple.com/grandslam/GsService2`), before any password/SRP
verification — i.e. Apple's identity gate rejected the request outright.
The same error hit the wider anisette/Sideloadly community within days
(first Reddit reports 2026-10-13/14), ruling out an account problem.

## Root cause

Apple **rotated the accepted GrandSlam client identity**. The hub (via the
vendored `isideload` fork) was presenting:

```text
X-Mme-Client-Info: <MacBookPro13,2> <macOS;13.1;22C65> <com.apple.AuthKit/1 (com.apple.akd/1.0)>
User-Agent:         akd/1.0 CFNetwork/978.0.7 Darwin/18.7.0
```

That pair (era 2 — introduced to fix the earlier `Xcode/3594.4.19` WAF 503)
is now rejected at the login gate with `-80045`. This is the second such
rotation: era 1 (Xcode machine description) was 503-blocked before it.

## Evidence — live A/B test against Apple (2026-10-17)

Before touching any code, the exact `init` request isideload sends was
replicated with `curl` (same URL-bag lookup, same plist body with CPD, an
**RFC 2606 reserved dummy account** `meridian.gate.test@example.com`, and
fresh anisette headers from `http://meridianhub.cc:6969/` per attempt —
mirroring the hub's fresh-OTP-per-login behavior):

| Identity under test | HTTP | GSA result | Interpretation |
|---|---|---|---|
| Legacy MacBookPro/akd + `CFNetwork/978.0.7` | 200 | **`ec=-80045`** | Rejected at the identity gate — exact reproduction of the hub error, with a dummy account + fresh OTP → proves it's identity-gated, not account- or OTP-gated |
| **`<PC> <Windows;6.2(0,0);9200> <com.apple.AuthKit/1 (com.apple.iTunes/12.12.4.1)>` + `akd/1.0 CFNetwork/808.1.4`** | 200 | **`hsc=200`, `ec=-20970` "Your account information was entered incorrectly."** | **Identity gate and anisette data accepted** — Apple proceeded to real credential validation (the dummy account simply doesn't exist; a real account continues into SRP) |
| `<Pixel 7> <Android;14;UQ1A.240205.002> <com.apple.AuthKit/1 (com.apple.akd/1.0)>` + any CFNetwork UA | **503** | WAF block | The other community-reported identity gets hard-blocked at the WAF layer with these UA pairings — not usable for us |

Note from round 1 of testing: reusing one OTP across requests yields
`-22421` (anisette complaint with an `X-Apple-I-MD-DATA` response) — that is
an OTP-consumption artifact of the test harness, **not** a code issue; the
hub already fetches fresh anisette per login. With fresh OTPs it disappears.

Test artifacts: `/var/tmp/opencode/gsa_ab/` (ephemeral; recreated as needed).

## The fix (surgical)

**One file changed:** `apps/hub-rust/vendor/isideload/src/anisette/remote.rs`

- `get_client_info()` (and the unused `_device_description` field) now return
  the verified Windows/iTunes identity:

```rust
const ITUNES_CLIENT_INFO: &str =
    "<PC> <Windows;6.2(0,0);9200> <com.apple.AuthKit/1 (com.apple.iTunes/12.12.4.1)>";
const ITUNES_USER_AGENT: &str = "akd/1.0 CFNetwork/808.1.4";
```

- The old constants (`AKD_CLIENT_INFO`, `PYTHON_USER_AGENT`) remain defined,
  documented as era-2 legacy with `#[allow(dead_code)]` — nothing was deleted.
- Unchanged: the omnisette server on the VPS (`:6969` — no action needed),
  `X-Apple-App-Info: com.apple.gs.xcode.auth`, all other headers, CPD
  construction, SRP/2FA flows, and the hub's sideloader wiring.

Important detail: the client-info and User-Agent **must stay paired to the
same platform** — `CFNetwork/808.1.4` is the iTunes-era akd UA on Windows.
Mixing platforms (e.g. Android client-info with a Darwin UA) gets 503'd by
the WAF outright.

## Backups & rollback

- `vendor/isideload` is **not tracked in git** — the rollback point is the
  explicit backup at `apps/hub-rust/vendor/isideload-backup-2026-10-17/`
  (full pre-fix `anisette/` directory + README; verified byte-identical
  before patching).
- Rollback (e.g. if Apple rotates again and you want to compare):

```bash
cd apps/hub-rust
cp -f vendor/isideload-backup-2026-10-17/anisette-pre-80045-fix/remote.rs \
      vendor/isideload/src/anisette/remote.rs
./build.sh
```

- When Apple rotates identities *again*: add the new known-good pair as
  constants in the identity block of `remote.rs` (full history is documented
  there) and flip `get_client_info()`. The A/B curl test procedure above can
  re-verify candidates safely with a reserved `example.com` account — no
  real credentials needed to test the gate.

## Verification performed

1. A/B gate test against live Apple GSA (table above) — fix proven pre-patch.
2. `cargo check` — clean (only the 2 pre-existing vendor warnings).
3. `cargo test` — 22/22 pass.
4. `./build.sh` — fresh `dist/meridian` with the new identity.
5. Final validation: **sideload a real device from the hub** — expect the
   login to pass `-80045` and continue into SRP / 2FA as it did before.
