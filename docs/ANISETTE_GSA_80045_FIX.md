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

---

# Addendum — 2026-10-21: GSA edge intermittently 503s the `complete` request

## Symptom

After the `-80045` identity fix, sideloading progressed past `init` but
failed at the proof step with a raw HTTP error (no GSA error code):

```
● Failed to send proof login request
● Received error response from grandslam
● HTTP status server error (503 Service Temporarily Unavailable)
  for url (https://gsa.apple.com/grandslam/GsService2)
```

## Root cause — Apple's edge, not us

Reproduced and characterized by direct curl testing against the live GSA
endpoint (same harness: real URL-bag lookup, real plist bodies, fresh
anotomette headers from `:6969` per flow, reserved `@example.com`
account):

- The **exact hub pattern** (lookup → `init` → `complete`, back-to-back,
  shared CPD/OTP, separate connections) **passes** — unless it doesn't:
  **~1 in 5 `complete` requests gets a raw `503`** at the edge (2 of 14
  observed), while every `init` returned 200 in the same runs.
- After a 503, an **immediate resend of the identical body returns 200**
  — the SRP continuation stays valid; the 503 is generated *before* auth
  processing (edge/load-balancer flakiness on rapid consecutive requests,
  the second request of an exchange being the vulnerable one).
- Unrelated to identity (`init` passes consistently with the iTunes pair),
  account (dummy account hit it), and the anisette server (same machine
  data across passing and failing requests).

## Fix — bounded retry in `GrandSlam::plist_request`

`vendor/isideload/src/auth/grandslam.rs` only: `plist_request` — the
single choke point for every GSA POST (`init`, `complete`, and all 2FA
calls) — now retries on `503 Service Unavailable` / `429 Too Many
Requests`, up to 4 attempts with 1s/2s/3s backoff. Those statuses are
edge-only rejections (never auth results), so resending the same body is
always safe — proven by the immediate-resend test above. The original
transport-vs-status error context distinction is preserved; a `warn!`
log line records each retry.

Expected worst case with the observed ~14% flake rate: P(4 consecutive
failures) ≈ 0.05% — for practical purposes sideloading is back to always
working.

## Backups

- `vendor/isideload-backup-2026-10-21/` — full pre-fix copy of
  `src/auth/` + README with rollback commands (verified identical before
  patching).
- The `-80045` backup remains at `vendor/isideload-backup-2026-10-17/`.

## Re-verification

1. curl burst testing (documented above) — 503 reproduced + immediate
   resend recovery proven.
2. `cargo check` clean; `cargo test` 22/22.
3. `./build.sh` — fresh `dist/meridian`.
4. Final validation: sideload from the hub — retries will show as
   `grandslam … rejected with 503 — retrying in …` warnings in tracing
   logs when the edge flakes, and the login should complete regardless.

---

# Addendum — 2026-10-21 (later): `-80046` at the developer session (apptokens)

## Symptom

Login and 2FA now complete, but sideloading fails one stage later:

```
● Failed to get xcode token from Apple account
● GrandSlam error during app token request
● Auth error -80046: This action cannot be completed at this time.
```

## Root cause — stale one-time-password at the `apptokens` step

Direct curl testing (garbage `o: "apptokens"` body with `app: ["xcode.auth"]`)
showed every sane identity — including the Windows/iTunes pair — **passes the
apptokens gate** when the token is invalid (`-20356 Invalid Requester …
invalid/expired session`): the request reaches token validation. So the
identity is not rejected; `-80046` comes from a **deeper check that only
valid tokens reach**.

The deeper check that fails is OTP staleness:

* The VPS omnisette server **rotates the OTP (`X-Apple-I-MD`) every ~30s**
  (measured: identical values within a rotation, new value at each ~30s
  boundary — the server is healthy).
* isideload caches anisette for **60 seconds**
  (`AnisetteDataGenerator::get_anisette_data` / `needs_refresh`), and the
  whole login shares it: `init`/`complete` consume the OTP fetched at
  t≈0, the user then spends 30s+ on the 2FA code, and by the time
  `apptokens` runs it is still reusing the same OTP — now **1–2 rotations
  (~30–65s) stale**.
* The login and 2FA endpoints tolerate that reuse; the **deeper apptokens
  validation does not**, and answers with `-80046`. Real `akd` never trips
  this because it regenerates anisette for **every** request.

## Fix — fresh anisette for every post-login request

Three surgical changes in the vendored crate:

1. `anisette/mod.rs`: new `AnisetteDataGenerator::get_fresh_anisette_data` —
   identical to `get_anisette_data` but always fetches (bypassing the 60s
   cache) and updates the cache with the result.
2. `auth/apple_account.rs::get_app_token`: uses the fresh fetch — the
   failing `apptokens` request now always carries a current OTP
   (≤ one ~30s rotation old, typically ~15s).
3. `dev/developer_session.rs::get_headers`: same fresh fetch — every
   developer-API call (certificates, profiles, app IDs — they all flow
   through this) gets its own current OTP, preventing the identical
   failure at the next stage.

`login_inner` is deliberately **unchanged**: `init`/`complete` sharing one
CPD is the protocol-blessed pattern every implementation uses, and it is
demonstrably accepted.

## Backup

`vendor/isideload-backup-2026-10-21/src-post-503-pre-80046/` — snapshot of
the three files immediately before this fix (i.e. with the 503-retry patch
applied). See the README there for per-file rollback.

## Re-verification

1. curl: apptokens gate matrix (4 identities, garbage tokens) — identity
   ruled out; OTP rotation cadence measured directly.
2. `cargo check` clean; `cargo test` 22/22.
3. `./build.sh` — fresh `dist/meridian`.
4. Final validation: sideload from the hub — expect the flow to pass
   `-80046` and proceed to cert/profile provisioning.

---

# Addendum — 2026-10-21 (final): the gates are PER-ENDPOINT — identity split

## Symptom

Even with the fresh-OTP fix, `apptokens` still failed with `-80046` after a
fully successful login + 2FA.

## Root cause — proven with a real session

A new debug tool (`apps/hub-rust/src/bin/gsa_probe.rs`) performed ONE real
login (2FA included) and then requested the `xcode.auth` app token against
several client identities inside that single session:

| Identity at `apptokens` | GSA response |
|---|---|
| `<PC> <Windows;6.2(0,0);9200> <com.apple.AuthKit/1 (com.apple.iTunes/12.12.4.1)>` + `akd/1.0 CFNetwork/808.1.4` (the `-80045` login fix) | **`ec=-80046`** — exact reproduction |
| `<MacBookPro13,2> <macOS;13.1;22C65> <com.apple.AuthKit/1 (com.apple.akd/1.0)>` + `akd/1.0 CFNetwork/978.0.7 Darwin/18.7.0` (era-2 pair — rejected at LOGIN with -80045) | **`ec=0` — accepted** |
| `<MacBookPro16,1> <macOS;14.1;23B74> <com.apple.AuthKit/1 (com.apple.akd/1.0)>` + Darwin UA | **`ec=0` — accepted** |

Apple's GrandSlam gates are now **per-endpoint and mirrored**: login
requires the Windows/iTunes identity (the era-2 akd pair gets `-80045`
there), while `apptokens` requires an akd macOS identity (the iTunes pair
gets `-80046` there). The old single-identity flow could never satisfy
both gates at once after the 2026-10 rotations.

The stale-OTP condition fixed earlier is real but was not the `-80046`
discriminator — the fresh-OTP fix stays (it is protocol-correct; real akd
regenerates anisette per request).

## Fix — per-endpoint identity split (vendored isideload)

1. `anisette/mod.rs`: new `AnisetteProvider::get_app_token_client_info`
   (defaults to the login identity) + `AnisetteDataGenerator` passthrough.
2. `anisette/remote.rs`: the provider overrides it to return the era-2
   akd pair (`AKD_CLIENT_INFO` + `PYTHON_USER_AGENT`, revived from legacy
   with the full history documented in the identity block).
3. `auth/grandslam.rs`: `plist_request` refactored into a shared core
   (`plist_request_inner`, retry logic unchanged) + new public
   `plist_request_as(url, body, headers, client_info)` for per-request
   identity.
4. `auth/apple_account.rs::get_app_token`: fetches the app-token identity
   from the provider and sends the `apptokens` request via
   `plist_request_as`. Login/2FA requests are unchanged (iTunes identity).

The probe itself (`gsa_probe`) stays in the repo — future gate rotations
can be characterized in one 2FA login instead of one theory per login.

## Backups

- `vendor/isideload-backup-2026-10-21/src-post-otp-pre-identity-split/` —
  the four files touched by this fix, snapshotted immediately before it
  (with the 503-retry and fresh-OTP patches applied).
- Earlier snapshots: `auth-pre-503-retry/` and `src-post-503-pre-80046/`
  in the same directory; `-80045` identity backup in
  `vendor/isideload-backup-2026-10-17/`.

## Re-verification

1. `gsa_probe` with a real session — the table above (identity is the
   discriminator; both akd macOS variants accepted).
2. `cargo check` clean; `cargo test` 22/22.
3. `./build.sh` — fresh `dist/meridian`.
4. Final validation: sideload from the hub — login (iTunes identity) →
   2FA → `apptokens` (akd identity) should pass `-80046` and the flow
   should continue into cert/profile provisioning.

---

# Addendum — 2026-10-21 (ESX): developer services also require the akd identity

## Symptom

With the identity split in place, the token minted fine but the first
developer-API call failed:

```
● Failed to list developer teams
● Developer error 1100: Your session has expired. Please log in.
```

## Root cause — proven with a real session (gsa_probe, ESX phase)

`DeveloperSession::from_account` passed the account's login client
(Windows/iTunes identity) into the session, and every ESX request carries
that client's base headers (`send_dev_request_internal` →
`self.client.post`). The ESX gate demands the same context the token was
minted under:

| ESX session identity | `listTeams` result |
|---|---|
| iTunes/Windows (the then-current session client) | **`1100 Your session has expired`** — exact reproduction |
| akd macOS 13.1 (the token-mint identity) | **SUCCESS — teams listed** |

A garbage-token curl matrix against ESX first showed `1100` for *every*
identity — i.e. it is ESX's generic invalid-session response, useless for
gate testing — so the real-session probe was required. The probe also
confirmed the `apptokens` success shape includes the encrypted token
(`et`: 488 bytes; minted token: 196 chars, 1-year duration).

## The complete per-endpoint identity map (2026-10-21)

| Endpoint | Required identity | Wrong identity → |
|---|---|---|
| Login / 2FA (GsService2 init/complete) | Windows/iTunes | akd → `-80045` |
| `apptokens` (Xcode developer token) | akd macOS | iTunes → `-80046` |
| ESX (developerservices2.apple.com) | akd macOS | iTunes → `1100 session expired` |

## Fix

`dev/developer_session.rs::from_account` now builds the session's GrandSlam
client from `get_app_token_client_info()` (the akd identity) instead of
cloning the account's login client — matching the context the token was
minted under for every ESX call (teams, certificates, profiles, app IDs —
they all flow through this session).

## Backups

`vendor/isideload-backup-2026-10-21/src-post-split-pre-esx/` — snapshot of
`developer_session.rs` immediately before this change.

## Re-verification

1. `gsa_probe` real session — the ESX table above.
2. `cargo check` clean; `cargo test` 22/22.
3. `./build.sh` — fresh `dist/meridian`.
4. Final validation: full sideload from the hub — login → token → teams →
   certificates/profiles → install.
