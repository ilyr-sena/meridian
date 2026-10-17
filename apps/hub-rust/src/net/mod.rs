//! Real-time internet connection health monitoring.
//!
//! Continuously measures the quality of the host's internet link — latency,
//! jitter, packet loss, and download throughput — against multiple probe
//! anchors so degradation can be *localized* (local link vs. the path to the
//! Meridian VPS) rather than just observed.
//!
//! Architecture:
//!   * `probes`   — raw measurement primitives (TCP-connect RTT, HTTP download)
//!   * `stats`    — pure math: percentiles, EWMA, RFC-3550 jitter
//!   * `classify` — grade scoring + hysteresis state machine + fault verdict
//!   * `monitor`  — tokio schedulers that keep a live [`NetHealthSnapshot`]
//!                  published at all times (probes every 2s, roll-ups every
//!                  2s; the app reads the snapshot via its 1s UI tick)
//!
//! Design constraints:
//!   * Pure TCP — no raw sockets, no ICMP, no root/admin requirement; works
//!     identically on Linux and Windows.
//!   * Zero new dependencies.
//!   * Every network operation is timeout-boxed; one wedged anchor can never
//!     stall the others.
//!   * Synthetic speed tests suppress themselves while a live session is
//!     streaming so they never compete with real video traffic.

pub mod classify;
pub mod monitor;
pub mod probes;
pub mod stats;

pub use classify::{AnchorHealth, NetGrade};
pub use monitor::{NetEvent, NetEventKind, NetHealthMonitor, NetHealthSnapshot};

use std::time::Duration;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// TCP-connect probe cadence per anchor (the heartbeat of the system).
pub const PROBE_INTERVAL: Duration = Duration::from_secs(2);
/// Hard timeout for one TCP connect probe. A timeout or refusal counts as a
/// loss event for that anchor.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
/// Stagger between anchor probe tasks so they never fire as a burst.
pub const PROBE_STAGGER: Duration = Duration::from_millis(300);
/// Analysis roll-up cadence: recomputes stats, runs the classifier and
/// publishes a fresh snapshot.
pub const ROLLUP_INTERVAL: Duration = Duration::from_secs(2);
/// Smoothing factor for the live ("current") RTT display — fast enough to
/// feel real-time, slow enough to never flicker between probes.
pub const RTT_EWMA_ALPHA: f64 = 0.3;
/// RTT samples retained per anchor for percentile math (≈3 min at 2s cadence).
pub const RTT_WINDOW: usize = 90;
/// Probe outcomes retained per anchor, by age (≈3 min at 2s cadence).
pub const LOSS_WINDOW: Duration = Duration::from_secs(180);
/// An anchor with no successful probe in this window counts as unreachable.
pub const UNREACHABLE_AFTER: Duration = Duration::from_secs(30);
/// Per-anchor loss % that marks it "degraded" rather than healthy.
pub const ANCHOR_DEGRADED_LOSS_PCT: f32 = 2.0;
/// Per-anchor median RTT (ms) that marks it "degraded" rather than healthy.
pub const ANCHOR_DEGRADED_RTT_MS: u32 = 250;
/// Consecutive total failures before an anchor is quarantined — only applied
/// while another anchor is provably healthy, because a hard firewall block
/// (e.g. corporate net filtering 1.1.1.1:443) looks identical to a dead path.
pub const QUARANTINE_AFTER_FAILS: u32 = 30; // ≈60s of consecutive failures
/// While quarantined, an anchor is retried at this cadence until it answers.
pub const QUARANTINE_RETRY_INTERVAL: Duration = Duration::from_secs(60);

/// Time between synthetic download speed tests while no session is streaming.
pub const SPEED_TEST_INTERVAL: Duration = Duration::from_secs(600);
/// Grace period after boot before the first speed test (let probes establish
/// a latency baseline first).
pub const SPEED_TEST_BOOT_DELAY: Duration = Duration::from_secs(15);
/// Warm-up discarded before the speed measurement window (TLS handshake +
/// TCP slow-start ramp-up — the measured phase must reflect steady state).
pub const SPEED_WARMUP_BYTES: u64 = 256 * 1024;
/// Length of the speed measurement window.
pub const SPEED_MEASURE: Duration = Duration::from_secs(4);
/// Hard budget for one whole speed test (connect + warm-up + measurement).
pub const SPEED_MAX_TOTAL: Duration = Duration::from_secs(45);
/// A speed sample older than this stops counting toward the grade / UI.
pub const SPEED_STALE_AFTER: Duration = Duration::from_secs(1800);
/// Measured throughput below this cannot sustain a single stream (the runner
/// encodes ≈2.5 Mbps) and caps the connection grade at `Poor`.
pub const SPEED_FLOOR_MBPS: f64 = 5.0;
/// URL of the VPS-hosted payload streamed for speed measurements. The runner
/// IPA is served with no-cache and is large enough to saturate any reasonable
/// link for the measurement window — no server-side changes required.
pub const SPEED_TEST_URL: &str = "https://meridianhub.cc/runner/MeridianRunner-unsigned.ipa";

/// Consecutive roll-ups a candidate grade must hold before a transition is
/// published (hysteresis — the UI must never flicker).
pub const GRADE_CONFIRM_CYCLES: u32 = 3;
/// Fast-path confirmation count for entering/leaving the `Down` state — total
/// connectivity loss should surface (and recover) quickly.
pub const GRADE_DOWN_CONFIRM_CYCLES: u32 = 2;

/// The VPS probe target. Deliberately the nginx HTTPS port (`:443`), not the
/// rathole control channel (`:2333`) — probing the control channel floods it
/// with half-open connections. The browser-facing stream path traverses the
/// same route, so this RTT is the one that matters for the product.
pub const VPS_PROBE_ADDR: &str = "98.84.189.148:443";
