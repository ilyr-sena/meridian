# BACKUP — vendor/isideload fixes for the 2026-10 Apple GSA breakages (2026-10-21)

Two snapshots live here, taken at different stages of the same incident:

## 1. `auth-pre-503-retry/` — before the GSA-edge-503 retry patch

Exact copy of `vendor/isideload/src/auth/` before `GrandSlam::plist_request`
gained its bounded 503/429 retry (Apple's edge intermittently sheds the
rapid `complete` request after `init`; an immediate resend of the identical
body succeeds — see the addendum in
`docs/ANISETTE_GSA_80045_FIX.md`).

Rollback:

```bash
cp -f vendor/isideload-backup-2026-10-21/auth-pre-503-retry/grandslam.rs \
      vendor/isideload/src/auth/grandslam.rs
```

## 2. `src-post-503-pre-80046/` — after the 503-retry patch, before the stale-OTP fix

Snapshot of the three files touched by the `-80046` fix (`anisette/mod.rs`,
`auth/apple_account.rs`, `dev/developer_session.rs`) with the 503-retry
patch applied but the fresh-anisette changes not yet. The `-80046` root
cause: `apptokens` (and each developer-API call) reused the 60s-cached
anisette OTP, which the deeper validation rejects after the ~30s server
rotation — fixed by `get_fresh_anisette_data` for `get_app_token` and
`DeveloperSession::get_headers`.

Rollback (per file):

```bash
cp -f vendor/isideload-backup-2026-10-21/src-post-503-pre-80046/mod.rs \
      vendor/isideload/src/anisette/mod.rs
cp -f vendor/isideload-backup-2026-10-21/src-post-503-pre-80046/apple_account.rs \
      vendor/isideload/src/auth/apple_account.rs
cp -f vendor/isideload-backup-2026-10-21/src-post-503-pre-80046/developer_session.rs \
      vendor/isideload/src/dev/developer_session.rs
```

Both snapshots were verified byte-identical to the tree at snapshot time.
Note: the `-80045` identity fix has its own backup at
`vendor/isideload-backup-2026-10-17/`.
