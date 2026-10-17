# BACKUP — pre -80045 fix (2026-10-17)

## What this is

An exact copy of `vendor/isideload/src/anisette/` **immediately before** the
GSA `-80045` identity fix was applied to `vendor/isideload/src/anisette/remote.rs`.

The `vendor/isideload` crate is **not tracked in git** (excluded via
.gitignore), so this directory is the only rollback point for the fork.

## Why the fix was needed

Apple began rejecting the MacBookPro/macOS `akd/1.0` GrandSlam client identity
with error `-80045 "This action cannot be completed at this time"` (community
wide, first reports 2026-10; same failure pattern as the earlier
`Xcode/3594.4.19` WAF block that this fork originally patched around).

Verified by direct A/B test against `gsa.apple.com/grandslam/GsService2`
(see `docs/ANISETTE_GSA_80045_FIX.md` in the repo root for the full matrix):
legacy identity → `-80045` (even with a fresh OTP and a dummy account);
Windows/iTunes identity → gate passes, Apple proceeds to credential
validation.

## What changed (only one file: `anisette/remote.rs`)

`get_client_info()` now returns the Windows/iTunes identity
(`<PC> <Windows;6.2(0,0);9200> <com.apple.AuthKit/1 (com.apple.iTunes/12.12.4.1)>`
+ `User-Agent: akd/1.0 CFNetwork/808.1.4`). The old
`AKD_CLIENT_INFO` / `PYTHON_USER_AGENT` constants remain defined in the
patched file (documented, marked `#[allow(dead_code)]`) for instant rollback.

## How to roll back

```bash
cp -f vendor/isideload-backup-2026-10-17/anisette-pre-80045-fix/remote.rs \
      vendor/isideload/src/anisette/remote.rs
./build.sh
```

(Or restore the whole directory: `rm -rf vendor/isideload/src/anisette &&
cp -a vendor/isideload-backup-2026-10-17/anisette-pre-80045-fix vendor/isideload/src/anisette`.)
