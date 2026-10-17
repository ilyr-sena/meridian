//! The live monitor: probe schedulers, analysis roll-up, and the published
//! [`NetHealthSnapshot`] the rest of the hub consumes.
//!
//! Three independent task families keep the data live at all times:
//!   * **probe tasks** — one per anchor; every 2s a TCP connect (timeout 3s).
//!     Fully isolated: a wedged anchor stalls only itself.
//!   * **roll-up task** — every 2s: recompute per-anchor stats, run the
//!     classifier + fault verdict, publish a fresh snapshot, and queue
//!     human-readable events (grade/verdict changes, quarantine transitions)
//!     for the Logs tab.
//!   * **speed task** — periodic real throughput measurement against the
//!     VPS-hosted runner IPA. Self-suppresses while a session is streaming
//!     and aborts mid-test if one starts.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use chrono::{DateTime, Local};
use tracing::{debug, info, warn};

use super::classify::{
    fault_verdict, score_grade, AnchorHealth, AnchorSummary, Classifier, FaultVerdict, GradeInput,
    GradeTransition, NetGrade,
};
use super::probes::{http_download_speed, tcp_connect_rtt};
use super::stats::{ewma, jitter_step, mean, percentile};
use super::{
    ANCHOR_DEGRADED_LOSS_PCT, ANCHOR_DEGRADED_RTT_MS, LOSS_WINDOW, PROBE_INTERVAL, PROBE_STAGGER,
    PROBE_TIMEOUT, QUARANTINE_AFTER_FAILS, QUARANTINE_RETRY_INTERVAL, ROLLUP_INTERVAL,
    RTT_EWMA_ALPHA, RTT_WINDOW, SPEED_FLOOR_MBPS, SPEED_MAX_TOTAL, SPEED_MEASURE,
    SPEED_STALE_AFTER, SPEED_TEST_BOOT_DELAY, SPEED_TEST_INTERVAL, SPEED_TEST_URL,
    SPEED_WARMUP_BYTES, UNREACHABLE_AFTER, VPS_PROBE_ADDR,
};

/// Which probe anchor a measurement belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorKind {
    Vps,
    Public1,
    Public2,
}

/// Static anchor registry: (kind, "ip:port"). IP literals only — no DNS
/// flakiness in the probe path.
const ANCHORS: [(AnchorKind, &str); 3] = [
    (AnchorKind::Vps, VPS_PROBE_ADDR),
    (AnchorKind::Public1, "1.1.1.1:443"),
    (AnchorKind::Public2, "8.8.8.8:443"),
];

impl AnchorKind {
    pub fn label(self) -> &'static str {
        match self {
            AnchorKind::Vps => "Meridian VPS",
            AnchorKind::Public1 => "Cloudflare",
            AnchorKind::Public2 => "Google DNS",
        }
    }
}

/// One anchor's live health as published in the snapshot.
#[derive(Debug, Clone)]
pub struct AnchorStatus {
    pub kind: AnchorKind,
    pub label: &'static str,
    pub addr: &'static str,
    /// EWMA-smoothed RTT of the last successful probes (`None` while
    /// unreachable/quarantined — stale numbers are never shown).
    pub current_rtt_ms: Option<u32>,
    pub median_rtt_ms: Option<u32>,
    pub p95_rtt_ms: Option<u32>,
    pub min_rtt_ms: Option<u32>,
    pub avg_rtt_ms: Option<u32>,
    pub jitter_ms: f32,
    pub loss_pct: f32,
    pub health: AnchorHealth,
}

/// A completed throughput measurement, as shown in the UI.
#[derive(Debug, Clone)]
pub struct SpeedSample {
    pub mbps: f64,
    /// When the measurement completed.
    pub at: DateTime<Local>,
}

/// A human-readable event for the hub's Logs tab.
#[derive(Debug, Clone)]
pub struct NetEvent {
    pub kind: NetEventKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetEventKind {
    Info,
    Warning,
    Critical,
}

/// The complete, ready-to-render picture of the internet connection.
#[derive(Debug, Clone)]
pub struct NetHealthSnapshot {
    pub grade: NetGrade,
    pub verdict: FaultVerdict,
    /// Live (EWMA-smoothed) RTT of the best active anchor — the headline
    /// "latency now" shown in the titlebar and status tab.
    pub current_latency_ms: Option<u32>,
    pub median_latency_ms: Option<u32>,
    pub p95_latency_ms: Option<u32>,
    pub min_latency_ms: Option<u32>,
    pub avg_latency_ms: Option<u32>,
    pub jitter_ms: f32,
    /// Aggregate packet loss across active anchors over the loss window.
    /// An indicator of *local-link* health; per-anchor detail lives in
    /// `anchors` and the verdict localizes path-specific trouble.
    pub loss_pct: f32,
    /// Fresh download throughput sample, if one is available.
    pub download: Option<SpeedSample>,
    /// True while a speed measurement is in flight.
    pub speed_running: bool,
    /// True while speed tests are suppressed by a live streaming session.
    pub speed_paused: bool,
    pub anchors: Vec<AnchorStatus>,
    /// Wall-clock time of the last grade change.
    pub last_change: DateTime<Local>,
}

impl NetHealthSnapshot {
    fn initial() -> Self {
        Self {
            grade: NetGrade::Unknown,
            verdict: FaultVerdict::Measuring,
            current_latency_ms: None,
            median_latency_ms: None,
            p95_latency_ms: None,
            min_latency_ms: None,
            avg_latency_ms: None,
            jitter_ms: 0.0,
            loss_pct: 0.0,
            download: None,
            speed_running: false,
            speed_paused: false,
            anchors: Vec::new(),
            last_change: Local::now(),
        }
    }
}

// ---------------------------------------------------------------------------
// Internal shared state
// ---------------------------------------------------------------------------

/// Probe outcomes and derived state for one anchor — written by its probe
/// task, read by the roll-up. All locks are held only for microseconds.
struct AnchorState {
    kind: AnchorKind,
    addr: &'static str,
    /// Recent successful RTTs (ms), bounded to `RTT_WINDOW` samples.
    rtts_ms: VecDeque<u32>,
    /// Recent probe outcomes for loss math, bounded to `LOSS_WINDOW` by age.
    outcomes: VecDeque<(Instant, bool)>,
    /// EWMA-smoothed RTT for the jitter-free "current latency" display.
    smoothed_ms: Option<f64>,
    jitter_ms: f32,
    last_rtt_ms: Option<f32>,
    last_success_at: Option<Instant>,
    consecutive_failures: u32,
    quarantined: bool,
    /// Set when a probe succeeds while quarantined (roll-up reinstates).
    success_since_quarantine: bool,
}

impl AnchorState {
    fn new(kind: AnchorKind, addr: &'static str) -> Self {
        Self {
            kind,
            addr,
            rtts_ms: VecDeque::with_capacity(RTT_WINDOW),
            outcomes: VecDeque::with_capacity(96),
            smoothed_ms: None,
            jitter_ms: 0.0,
            last_rtt_ms: None,
            last_success_at: None,
            consecutive_failures: 0,
            quarantined: false,
            success_since_quarantine: false,
        }
    }
}

struct Shared {
    snapshot: RwLock<NetHealthSnapshot>,
    /// Log events queued for the UI tick to drain (bounded).
    events: Mutex<Vec<NetEvent>>,
    anchors: Vec<Arc<Mutex<AnchorState>>>,
    /// Set by the app while any device session is streaming — speed tests
    /// must never compete with live video traffic.
    session_active: AtomicBool,
    speed_in_flight: AtomicBool,
    last_speed: Mutex<Option<(SpeedSample, Instant)>>,
    should_run: AtomicBool,
}

/// Read-only working copy of one anchor's state, gathered under a short lock.
struct AnchorView {
    kind: AnchorKind,
    addr: &'static str,
    rtts: Vec<u32>,
    jitter_ms: f32,
    smoothed_ms: Option<f64>,
    current_ms: Option<u32>,
    loss_pct: f32,
    failures_window: u32,
    total_window: u32,
    recent_success: bool,
    consecutive_failures: u32,
    quarantined: bool,
    success_since_quarantine: bool,
}

// ---------------------------------------------------------------------------
// Public monitor
// ---------------------------------------------------------------------------

pub struct NetHealthMonitor {
    shared: Arc<Shared>,
}

impl NetHealthMonitor {
    /// Start the monitor and its background tasks. Returns immediately; the
    /// first snapshot is published within a couple of seconds.
    pub fn start() -> Arc<Self> {
        let anchors = ANCHORS
            .iter()
            .map(|(kind, addr)| Arc::new(Mutex::new(AnchorState::new(*kind, addr))))
            .collect();

        let shared = Arc::new(Shared {
            snapshot: RwLock::new(NetHealthSnapshot::initial()),
            events: Mutex::new(Vec::new()),
            anchors,
            session_active: AtomicBool::new(false),
            speed_in_flight: AtomicBool::new(false),
            last_speed: Mutex::new(None),
            should_run: AtomicBool::new(true),
        });

        // One isolated probe task per anchor.
        for (index, state) in shared.anchors.iter().enumerate() {
            tokio::spawn(run_anchor_probes(shared.clone(), state.clone(), index as u32));
        }
        // Analysis roll-up — the only writer of the published snapshot.
        tokio::spawn(run_rollup(shared.clone()));
        // Periodic real throughput measurement.
        tokio::spawn(run_speed_tests(shared.clone()));

        info!(
            "[NET] Internet health monitor started — {} anchors, {}s probes",
            ANCHORS.len(),
            PROBE_INTERVAL.as_secs()
        );

        Arc::new(Self { shared })
    }

    /// Latest live snapshot. Cheap — clones a small struct.
    pub fn snapshot(&self) -> NetHealthSnapshot {
        self.shared.snapshot.read().unwrap().clone()
    }

    /// Drain queued log events (called by the UI tick).
    pub fn drain_events(&self) -> Vec<NetEvent> {
        std::mem::take(&mut *self.shared.events.lock().unwrap())
    }

    /// Snapshot + drained events in one call for the UI tick.
    pub fn collect(&self) -> (NetHealthSnapshot, Vec<NetEvent>) {
        (self.snapshot(), self.drain_events())
    }

    /// Live-session flag: while any session is streaming, synthetic speed
    /// tests are suppressed (and an in-flight one aborts) so they never
    /// compete with the real video traffic.
    pub fn set_session_active(&self, active: bool) {
        self.shared.session_active.store(active, Ordering::Relaxed);
    }

    pub fn stop(&self) {
        self.shared.should_run.store(false, Ordering::SeqCst);
    }
}

// ---------------------------------------------------------------------------
// Probe tasks
// ---------------------------------------------------------------------------

/// Fixed-cadence probe loop for one anchor. Never drifts, never overlaps, and
/// slows to `QUARANTINE_RETRY_INTERVAL` while the anchor is quarantined.
async fn run_anchor_probes(shared: Arc<Shared>, state: Arc<Mutex<AnchorState>>, index: u32) {
    // Stagger anchors so they never fire as a synchronized burst.
    tokio::time::sleep(PROBE_STAGGER * index).await;

    let mut next_due = Instant::now();
    while shared.should_run.load(Ordering::Relaxed) {
        let (addr, quarantined) = {
            let s = state.lock().unwrap();
            (s.addr, s.quarantined)
        };
        let cadence = if quarantined {
            QUARANTINE_RETRY_INTERVAL
        } else {
            PROBE_INTERVAL
        };

        let rtt = match tcp_connect_rtt(addr, PROBE_TIMEOUT).await {
            Ok(rtt) => Some(rtt),
            Err(_) => None, // refused or timed out — a loss event
        };
        record_probe(&state, rtt);

        next_due += cadence;
        let now = Instant::now();
        if next_due > now {
            tokio::time::sleep(next_due - now).await;
        } else {
            next_due = now; // fell behind (long stall) — reset the cadence
        }
    }
}

/// Record one probe outcome into the anchor's shared state.
fn record_probe(state: &Mutex<AnchorState>, rtt: Option<Duration>) {
    let mut s = state.lock().unwrap();
    let now = Instant::now();

    match rtt {
        Some(d) => {
            let ms = d.as_millis().min(u32::MAX as u128) as u32;
            s.rtts_ms.push_back(ms);
            while s.rtts_ms.len() > RTT_WINDOW {
                s.rtts_ms.pop_front();
            }
            s.jitter_ms = jitter_step(s.jitter_ms, s.last_rtt_ms, ms as f32);
            s.smoothed_ms = Some(ewma(s.smoothed_ms, ms as f64, RTT_EWMA_ALPHA));
            s.last_rtt_ms = Some(ms as f32);
            s.last_success_at = Some(now);
            s.consecutive_failures = 0;
            s.outcomes.push_back((now, true));
            if s.quarantined {
                s.success_since_quarantine = true;
            }
        }
        None => {
            s.consecutive_failures += 1;
            s.outcomes.push_back((now, false));
        }
    }

    while s
        .outcomes
        .front()
        .map_or(false, |(t, _)| now.duration_since(*t) > LOSS_WINDOW)
    {
        s.outcomes.pop_front();
    }
}

// ---------------------------------------------------------------------------
// Analysis roll-up
// ---------------------------------------------------------------------------

async fn run_rollup(shared: Arc<Shared>) {
    // Give the first probe wave a moment to land before the first analysis.
    tokio::time::sleep(Duration::from_millis(1200)).await;

    let mut classifier = Classifier::new();
    let mut last_verdict = FaultVerdict::Measuring;
    let mut last_change_at = Local::now();

    while shared.should_run.load(Ordering::Relaxed) {
        rollup_once(&shared, &mut classifier, &mut last_verdict, &mut last_change_at);
        tokio::time::sleep(ROLLUP_INTERVAL).await;
    }

    debug!("[NET] roll-up task exited");
}

/// One analysis cycle. Synchronous by design — it only touches tiny in-memory
/// state under microsecond locks, never holding one across an await.
fn rollup_once(
    shared: &Shared,
    classifier: &mut Classifier,
    last_verdict: &mut FaultVerdict,
    last_change_at: &mut DateTime<Local>,
) {
    let now = Instant::now();

    // ---- Pass 1: gather per-anchor views under short locks ----
    let views: Vec<AnchorView> = shared
        .anchors
        .iter()
        .map(|st| {
            let s = st.lock().unwrap();
            let total = s.outcomes.len() as u32;
            let failures = s.outcomes.iter().filter(|(_, ok)| !ok).count() as u32;
            AnchorView {
                kind: s.kind,
                addr: s.addr,
                rtts: s.rtts_ms.iter().copied().collect(),
                jitter_ms: s.jitter_ms,
                smoothed_ms: s.smoothed_ms,
                current_ms: s.last_rtt_ms.map(|m| m as u32),
                loss_pct: if total > 0 {
                    failures as f32 * 100.0 / total as f32
                } else {
                    0.0
                },
                failures_window: failures,
                total_window: total,
                recent_success: s
                    .last_success_at
                    .map_or(false, |t| now.duration_since(t) <= UNREACHABLE_AFTER),
                consecutive_failures: s.consecutive_failures,
                quarantined: s.quarantined,
                success_since_quarantine: s.success_since_quarantine,
            }
        })
        .collect();

    // ---- Pass 2: quarantine transitions (need cross-anchor knowledge) ----
    // A target failing hard while another anchor is provably healthy is a
    // blocked *target* (firewall), not a dead local link — exclude it from
    // all verdicts and retry it slowly until it answers again.
    let mut flags: Vec<bool> = views.iter().map(|v| v.quarantined).collect();
    for (idx, v) in views.iter().enumerate() {
        if v.quarantined {
            if v.success_since_quarantine {
                flags[idx] = false;
                {
                    let mut s = shared.anchors[idx].lock().unwrap();
                    s.quarantined = false;
                    s.success_since_quarantine = false;
                    // Fresh start: stale pre-quarantine samples would poison
                    // the median/loss window of a just-reinstated anchor.
                    s.rtts_ms.clear();
                    s.outcomes.clear();
                    s.jitter_ms = 0.0;
                    s.last_rtt_ms = None;
                    s.smoothed_ms = None;
                }
                push_event(
                    shared,
                    NetEventKind::Info,
                    format!("Probe anchor {} ({}) reachable again — reinstated", v.kind.label(), v.addr),
                );
                info!("[NET] anchor {} reinstated", v.addr);
            }
        } else if v.consecutive_failures >= QUARANTINE_AFTER_FAILS
            && views.iter().enumerate().any(|(i, o)| {
                i != idx
                    && !flags[i]
                    && o.recent_success
                    && o.loss_pct < ANCHOR_DEGRADED_LOSS_PCT
            })
        {
            flags[idx] = true;
            {
                let mut s = shared.anchors[idx].lock().unwrap();
                s.quarantined = true;
                s.success_since_quarantine = false;
            }
            push_event(
                shared,
                NetEventKind::Warning,
                format!(
                    "Probe anchor {} ({}) unreachable for {}s — quarantined (target blocked?), retrying every 60s",
                    v.kind.label(),
                    v.addr,
                    v.consecutive_failures * PROBE_INTERVAL.as_secs() as u32
                ),
            );
            warn!(
                "[NET] anchor {} quarantined after {} consecutive failures",
                v.addr,
                v.consecutive_failures
            );
        }
    }

    // ---- Published per-anchor rows (stale stats are blanked, never shown) ----
    let statuses: Vec<AnchorStatus> = views
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let median = percentile(&v.rtts, 50.0);
            let health = if flags[i] {
                AnchorHealth::Quarantined
            } else if !v.recent_success {
                AnchorHealth::Unreachable
            } else if v.loss_pct >= ANCHOR_DEGRADED_LOSS_PCT
                || median.map_or(false, |m| m >= ANCHOR_DEGRADED_RTT_MS)
            {
                AnchorHealth::Degraded
            } else {
                AnchorHealth::Healthy
            };

            let show = health == AnchorHealth::Healthy || health == AnchorHealth::Degraded;
            AnchorStatus {
                kind: v.kind,
                label: v.kind.label(),
                addr: v.addr,
                current_rtt_ms: show.then(|| v.smoothed_ms.map(|m| m.round() as u32)).flatten(),
                median_rtt_ms: show.then_some(median).flatten(),
                p95_rtt_ms: show.then(|| percentile(&v.rtts, 95.0)).flatten(),
                min_rtt_ms: show.then(|| v.rtts.iter().copied().min()).flatten(),
                avg_rtt_ms: show
                    .then(|| mean(&v.rtts).map(|m| m.round() as u32))
                    .flatten(),
                jitter_ms: if show { v.jitter_ms } else { 0.0 },
                loss_pct: v.loss_pct,
                health,
            }
        })
        .collect();

    // ---- Fresh speed sample (if any) ----
    let speed_fresh = shared
        .last_speed
        .lock()
        .unwrap()
        .as_ref()
        .filter(|(_, at)| now.duration_since(*at) < SPEED_STALE_AFTER)
        .map(|(s, _)| s.clone());

    let active_idx: Vec<usize> = (0..views.len()).filter(|&i| !flags[i]).collect();

    // Defensive fallback: every anchor quarantined (should be impossible —
    // quarantine requires another healthy anchor). Keep the last grade.
    if active_idx.is_empty() {
        *shared.snapshot.write().unwrap() = NetHealthSnapshot {
            grade: classifier.current(),
            verdict: *last_verdict,
            anchors: statuses,
            download: speed_fresh,
            speed_running: shared.speed_in_flight.load(Ordering::Relaxed),
            speed_paused: shared.session_active.load(Ordering::Relaxed),
            last_change: *last_change_at,
            ..NetHealthSnapshot::initial()
        };
        return;
    }

    // ---- Aggregates over active anchors ----
    let all_down = active_idx.iter().all(|&i| !views[i].recent_success);

    let total: u32 = active_idx.iter().map(|&i| views[i].total_window).sum();
    let fails: u32 = active_idx.iter().map(|&i| views[i].failures_window).sum();
    let agg_loss = if total > 0 {
        fails as f32 * 100.0 / total as f32
    } else {
        0.0
    };

    // Best anchor = lowest median RTT among responsive active anchors; every
    // headline number comes from that one anchor so they stay coherent.
    let best: Option<&AnchorView> = active_idx
        .iter()
        .filter_map(|&i| {
            let v = &views[i];
            (v.recent_success && !v.rtts.is_empty()).then_some(v)
        })
        .min_by_key(|v| percentile(&v.rtts, 50.0).unwrap_or(u32::MAX));

    let (cur, med, p95, min, avg, jit) = match best {
        Some(b) => (
            b.smoothed_ms.map(|m| m.round() as u32),
            percentile(&b.rtts, 50.0),
            percentile(&b.rtts, 95.0),
            b.rtts.iter().copied().min(),
            mean(&b.rtts).map(|m| m.round() as u32),
            b.jitter_ms,
        ),
        None => (None, None, None, None, None, 0.0),
    };

    // ---- Grade (with hysteresis) ----
    let mut transition: Option<GradeTransition> = None;
    if all_down {
        transition = Some(classifier.observe(NetGrade::Down));
    } else if let Some(median) = med {
        let raw = score_grade(&GradeInput {
            median_rtt_ms: Some(median),
            jitter_ms: jit,
            loss_pct: agg_loss,
            download_mbps: speed_fresh.as_ref().map(|s| s.mbps),
        });
        transition = Some(classifier.observe(raw));
    }

    if let Some(GradeTransition::Changed { from, to }) = transition {
        *last_change_at = Local::now();
        let med_str = med.map(|m| format!("{m}ms")).unwrap_or_else(|| "n/a".into());
        let (kind, message) = if to == NetGrade::Down {
            (
                NetEventKind::Critical,
                "Internet connection lost — all probe paths failing".to_string(),
            )
        } else if from == NetGrade::Down {
            (
                NetEventKind::Info,
                format!("Internet connection restored ({})", to.label()),
            )
        } else if to.is_degraded() {
            (
                NetEventKind::Warning,
                format!(
                    "Internet connection degraded to {} — loss {:.1}%, median {}, jitter {:.1}ms",
                    to.label(),
                    agg_loss,
                    med_str,
                    jit
                ),
            )
        } else {
            (
                NetEventKind::Info,
                format!("Internet connection improved to {}", to.label()),
            )
        };
        push_event(shared, kind, message);
    }

    // ---- Fault localization ----
    let summaries: Vec<AnchorSummary> = views
        .iter()
        .enumerate()
        .map(|(i, v)| AnchorSummary {
            active: !flags[i],
            is_vps: v.kind == AnchorKind::Vps,
            recent_success: v.recent_success,
            median_rtt_ms: percentile(&v.rtts, 50.0),
            loss_pct: v.loss_pct,
        })
        .collect();
    let new_verdict = fault_verdict(&summaries);

    if new_verdict != *last_verdict && !matches!(new_verdict, FaultVerdict::Measuring | FaultVerdict::NetDown)
    {
        // NetDown already announced itself via the grade event.
        let kind = match new_verdict {
            FaultVerdict::Healthy => NetEventKind::Info,
            FaultVerdict::VpsUnreachable
            | FaultVerdict::LocalLinkDegraded
            | FaultVerdict::VpsPathDegraded => NetEventKind::Warning,
            FaultVerdict::Measuring | FaultVerdict::NetDown => NetEventKind::Info,
        };
        push_event(
            shared,
            kind,
            format!("Network diagnosis: {}", new_verdict.label()),
        );
    }
    *last_verdict = new_verdict;

    // ---- Publish ----
    *shared.snapshot.write().unwrap() = NetHealthSnapshot {
        grade: classifier.current(),
        verdict: new_verdict,
        current_latency_ms: cur,
        median_latency_ms: med,
        p95_latency_ms: p95,
        min_latency_ms: min,
        avg_latency_ms: avg,
        jitter_ms: jit,
        loss_pct: agg_loss,
        download: speed_fresh,
        speed_running: shared.speed_in_flight.load(Ordering::Relaxed),
        speed_paused: shared.session_active.load(Ordering::Relaxed),
        anchors: statuses,
        last_change: *last_change_at,
    };
}

// ---------------------------------------------------------------------------
// Speed tests
// ---------------------------------------------------------------------------

/// Periodic real-throughput measurement loop. Wakes frequently so a session
/// ending mid-interval is picked up quickly; each attempt is guarded by
/// eligibility (never while streaming, never while the link is down).
async fn run_speed_tests(shared: Arc<Shared>) {
    tokio::time::sleep(SPEED_TEST_BOOT_DELAY).await;

    loop {
        if !shared.should_run.load(Ordering::Relaxed) {
            return;
        }

        let due = shared
            .last_speed
            .lock()
            .unwrap()
            .as_ref()
            .map_or(true, |(_, at)| at.elapsed() >= SPEED_TEST_INTERVAL);

        let link_up = shared.snapshot.read().unwrap().grade != NetGrade::Down;
        let streaming = shared.session_active.load(Ordering::Relaxed);

        if due && link_up && !streaming {
            run_one_speed_test(shared.clone()).await;
        }

        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}

/// One single-flight throughput measurement against the VPS payload.
async fn run_one_speed_test(shared: Arc<Shared>) {
    if shared.speed_in_flight.swap(true, Ordering::SeqCst) {
        return; // already running — never overlap tests
    }

    info!("[NET] starting download speed measurement");
    let guard = shared.clone();
    let result = tokio::time::timeout(
        SPEED_MAX_TOTAL,
        http_download_speed(
            SPEED_TEST_URL,
            SPEED_WARMUP_BYTES,
            SPEED_MEASURE,
            move || {
                !guard.session_active.load(Ordering::Relaxed)
                    && guard.should_run.load(Ordering::Relaxed)
            },
        ),
    )
    .await;

    let measurement = match result {
        Ok(Ok(m)) => m,
        Ok(Err(e)) => {
            debug!("[NET] speed measurement failed: {e:#}");
            shared.speed_in_flight.store(false, Ordering::SeqCst);
            return;
        }
        Err(_) => {
            debug!("[NET] speed measurement exceeded the total budget");
            shared.speed_in_flight.store(false, Ordering::SeqCst);
            return;
        }
    };

    let sample = SpeedSample {
        mbps: measurement.mbps,
        at: Local::now(),
    };
    *shared.last_speed.lock().unwrap() = Some((sample, Instant::now()));
    shared.speed_in_flight.store(false, Ordering::SeqCst);

    let (kind, message) = if measurement.mbps < SPEED_FLOOR_MBPS {
        (
            NetEventKind::Warning,
            format!(
                "Download speed low: {:.1} Mbps — below the {:.0} Mbps streaming floor",
                measurement.mbps, SPEED_FLOOR_MBPS
            ),
        )
    } else {
        (
            NetEventKind::Info,
            format!("Download speed: {:.1} Mbps", measurement.mbps),
        )
    };
    push_event(&shared, kind, message);
    info!(
        "[NET] download speed: {:.1} Mbps ({} bytes in {:.2}s)",
        measurement.mbps,
        measurement.measured_bytes,
        measurement.elapsed.as_secs_f64()
    );
}

/// Queue a log event for the UI tick to drain (bounded, so a pathological
/// burst can never grow the queue unbounded).
fn push_event(shared: &Shared, kind: NetEventKind, message: String) {
    let mut q = shared.events.lock().unwrap();
    if q.len() >= 64 {
        q.remove(0);
    }
    q.push(NetEvent { kind, message });
}
