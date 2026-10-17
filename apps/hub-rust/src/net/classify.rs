//! Connection quality grading with hysteresis and fault localization.
//!
//! Pure functions plus a tiny state machine — no I/O, fully unit-testable.
//! The monitor feeds observations every roll-up cycle and publishes only
//! debounced transitions, so the UI can never flicker.

use super::{
    ANCHOR_DEGRADED_LOSS_PCT, ANCHOR_DEGRADED_RTT_MS, GRADE_CONFIRM_CYCLES,
    GRADE_DOWN_CONFIRM_CYCLES, SPEED_FLOOR_MBPS,
};

/// Everything the grader needs to know about one probe anchor.
#[derive(Debug, Clone)]
pub struct AnchorSummary {
    /// True while the anchor participates in grading. False = quarantined
    /// (hard-blocked, e.g. a corporate firewall filtering the target).
    pub active: bool,
    /// Whether this is the Meridian VPS anchor (the path the product uses).
    pub is_vps: bool,
    /// A successful probe landed within the reachability window.
    pub recent_success: bool,
    /// Median RTT over the sample window (`None` when no samples).
    pub median_rtt_ms: Option<u32>,
    /// Loss percentage over the outcome window.
    pub loss_pct: f32,
}

impl AnchorSummary {
    /// Healthy = responding recently with acceptable loss and latency.
    fn healthy(&self) -> bool {
        self.recent_success
            && self.loss_pct < ANCHOR_DEGRADED_LOSS_PCT
            && self
                .median_rtt_ms
                .map_or(false, |rtt| rtt < ANCHOR_DEGRADED_RTT_MS)
    }

    /// Unreachable = no successful probe within the reachability window.
    fn unreachable(&self) -> bool {
        !self.recent_success
    }
}

/// Published per-anchor reachability state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorHealth {
    Healthy,
    Degraded,
    Unreachable,
    /// Hard-blocked target; excluded from all verdicts and aggregates.
    Quarantined,
}

/// Overall internet connection grade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NetGrade {
    /// No data yet (monitor starting up).
    Unknown,
    Excellent,
    Good,
    Fair,
    Poor,
    Bad,
    /// Total connectivity loss (every active anchor unreachable).
    Down,
}

impl NetGrade {
    pub fn label(self) -> &'static str {
        match self {
            NetGrade::Unknown => "Measuring...",
            NetGrade::Excellent => "Excellent",
            NetGrade::Good => "Good",
            NetGrade::Fair => "Fair",
            NetGrade::Poor => "Poor",
            NetGrade::Bad => "Bad",
            NetGrade::Down => "No Internet",
        }
    }

    pub fn is_degraded(self) -> bool {
        matches!(self, NetGrade::Poor | NetGrade::Bad | NetGrade::Down)
    }
}

/// All metrics fed to [`score_grade`] for a single evaluation.
#[derive(Debug, Clone, Copy)]
pub struct GradeInput {
    /// Median RTT of the best active anchor. `None` = no data → `Unknown`.
    pub median_rtt_ms: Option<u32>,
    pub jitter_ms: f32,
    /// Aggregate packet loss across active anchors over the loss window.
    pub loss_pct: f32,
    /// Fresh download throughput, when available. Only ever *downgrades* the
    /// metric-based grade — a fast link cannot buy its way past packet loss.
    pub download_mbps: Option<f64>,
}

/// Score one observation into a grade. The worst offending metric wins
/// (loss / latency / jitter table below), then throughput caps the result.
///
/// | Grade  | RTT    | Jitter | Loss   |
/// |--------|--------|--------|--------|
/// | Bad    | ≥500ms | ≥40ms  | ≥15%   |
/// | Poor   | ≥250ms | ≥20ms  | ≥5%    |
/// | Fair   | ≥120ms | ≥10ms  | ≥2%    |
/// | Good   | ≥60ms  | ≥5ms   | ≥0.5%  |
pub fn score_grade(input: &GradeInput) -> NetGrade {
    let Some(rtt) = input.median_rtt_ms else {
        return NetGrade::Unknown;
    };

    let grade = if input.loss_pct >= 15.0 || rtt >= 500 || input.jitter_ms >= 40.0 {
        NetGrade::Bad
    } else if input.loss_pct >= 5.0 || rtt >= 250 || input.jitter_ms >= 20.0 {
        NetGrade::Poor
    } else if input.loss_pct >= 2.0 || rtt >= 120 || input.jitter_ms >= 10.0 {
        NetGrade::Fair
    } else if input.loss_pct >= 0.5 || rtt >= 60 || input.jitter_ms >= 5.0 {
        NetGrade::Good
    } else {
        NetGrade::Excellent
    };

    match input.download_mbps {
        Some(mbps) if mbps < SPEED_FLOOR_MBPS => {
            // Can't sustain even a single stream → at best "Poor".
            if grade.is_degraded() {
                grade
            } else {
                NetGrade::Poor
            }
        }
        _ => grade,
    }
}

/// Fault localization verdict — *where* the problem is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultVerdict {
    /// No data yet.
    Measuring,
    /// Everything answers cleanly.
    Healthy,
    /// A public anchor is degraded/unreachable — the local internet link is
    /// the suspect (the VPS path shares it, so mixed evidence points here too).
    LocalLinkDegraded,
    /// VPS unreachable while public anchors are healthy.
    VpsUnreachable,
    /// VPS degraded (loss/latency) while public anchors are healthy.
    VpsPathDegraded,
    /// Every active anchor is unreachable.
    NetDown,
}

impl FaultVerdict {
    pub fn label(self) -> &'static str {
        match self {
            FaultVerdict::Measuring => "Establishing baseline...",
            FaultVerdict::Healthy => "All probe paths healthy",
            FaultVerdict::LocalLinkDegraded => "Local internet link degraded",
            FaultVerdict::VpsUnreachable => "VPS unreachable — public internet OK",
            FaultVerdict::VpsPathDegraded => "Degraded path to Meridian VPS",
            FaultVerdict::NetDown => "No internet connectivity detected",
        }
    }
}

/// Localize the fault from per-anchor evidence. Pure.
///
/// Logic, in priority order:
///   * every active anchor unreachable → `NetDown`,
///   * any public anchor not healthy → `LocalLinkDegraded` (a bad local link
///     drags every path with it, so that is the actionable answer),
///   * all public anchors healthy (or none remain active) → judge the VPS
///     path alone: unreachable → `VpsUnreachable`, degraded →
///     `VpsPathDegraded`, else `Healthy`.
pub fn fault_verdict(anchors: &[AnchorSummary]) -> FaultVerdict {
    let active: Vec<&AnchorSummary> = anchors.iter().filter(|a| a.active).collect();
    if active.is_empty() {
        return FaultVerdict::Measuring;
    }

    if active.iter().all(|a| a.unreachable()) {
        return FaultVerdict::NetDown;
    }

    let public: Vec<&&AnchorSummary> = active.iter().filter(|a| !a.is_vps).collect();
    let vps = active.iter().find(|a| a.is_vps);

    if !public.is_empty() && public.iter().any(|a| !a.healthy()) {
        return FaultVerdict::LocalLinkDegraded;
    }

    match vps {
        Some(v) if v.unreachable() => FaultVerdict::VpsUnreachable,
        Some(v) if !v.healthy() => FaultVerdict::VpsPathDegraded,
        _ => FaultVerdict::Healthy,
    }
}

/// Result of one classifier observation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GradeTransition {
    /// Observation absorbed; published grade unchanged.
    Unchanged,
    /// Published grade changed (`from` is `Unknown` on the first observation).
    Changed { from: NetGrade, to: NetGrade },
}

/// Hysteresis state machine over [`score_grade`] observations.
///
/// A candidate grade must be observed for [`GRADE_CONFIRM_CYCLES`]
/// consecutive roll-ups before it is published; total-loss (`Down`) gets the
/// faster [`GRADE_DOWN_CONFIRM_CYCLES`] path in both directions so outages
/// and recoveries surface quickly. The first real observation after startup
/// is accepted immediately.
#[derive(Debug)]
pub struct Classifier {
    current: NetGrade,
    pending: Option<NetGrade>,
    pending_count: u32,
}

impl Classifier {
    pub fn new() -> Self {
        Self {
            current: NetGrade::Unknown,
            pending: None,
            pending_count: 0,
        }
    }

    pub fn current(&self) -> NetGrade {
        self.current
    }

    pub fn observe(&mut self, raw: NetGrade) -> GradeTransition {
        // Startup: accept the first real observation immediately so the UI
        // stops saying "Measuring..." as soon as evidence exists.
        if self.current == NetGrade::Unknown && raw != NetGrade::Unknown {
            let from = self.current;
            self.current = raw;
            self.reset();
            return GradeTransition::Changed { from, to: raw };
        }

        if raw == self.current {
            self.reset();
            return GradeTransition::Unchanged;
        }

        let required = if raw == NetGrade::Down || self.current == NetGrade::Down {
            GRADE_DOWN_CONFIRM_CYCLES
        } else {
            GRADE_CONFIRM_CYCLES
        };

        if self.pending == Some(raw) {
            self.pending_count += 1;
        } else {
            self.pending = Some(raw);
            self.pending_count = 1;
        }

        if self.pending_count >= required {
            let from = self.current;
            self.current = raw;
            self.reset();
            return GradeTransition::Changed { from, to: raw };
        }

        GradeTransition::Unchanged
    }

    fn reset(&mut self) {
        self.pending = None;
        self.pending_count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchor(active: bool, is_vps: bool, recent: bool, median: Option<u32>, loss: f32) -> AnchorSummary {
        AnchorSummary {
            active,
            is_vps,
            recent_success: recent,
            median_rtt_ms: median,
            loss_pct: loss,
        }
    }

    fn input(rtt: u32, jitter: f32, loss: f32) -> GradeInput {
        GradeInput {
            median_rtt_ms: Some(rtt),
            jitter_ms: jitter,
            loss_pct: loss,
            download_mbps: None,
        }
    }

    // ---- score_grade ----

    #[test]
    fn score_unknown_without_data() {
        assert_eq!(
            score_grade(&GradeInput {
                median_rtt_ms: None,
                jitter_ms: 0.0,
                loss_pct: 0.0,
                download_mbps: None,
            }),
            NetGrade::Unknown
        );
    }

    #[test]
    fn score_table_edges() {
        // Excellent zone
        assert_eq!(score_grade(&input(59, 4.9, 0.49)), NetGrade::Excellent);
        // Good boundaries
        assert_eq!(score_grade(&input(60, 5.0, 0.5)), NetGrade::Good);
        assert_eq!(score_grade(&input(119, 9.9, 1.9)), NetGrade::Good);
        // Fair boundaries
        assert_eq!(score_grade(&input(120, 10.0, 2.0)), NetGrade::Fair);
        assert_eq!(score_grade(&input(249, 19.9, 4.9)), NetGrade::Fair);
        // Poor boundaries
        assert_eq!(score_grade(&input(250, 20.0, 5.0)), NetGrade::Poor);
        assert_eq!(score_grade(&input(499, 39.9, 14.9)), NetGrade::Poor);
        // Bad boundaries
        assert_eq!(score_grade(&input(500, 40.0, 15.0)), NetGrade::Bad);
    }

    #[test]
    fn score_worst_metric_wins() {
        // Clean latency, terrible loss.
        assert_eq!(score_grade(&input(30, 1.0, 20.0)), NetGrade::Bad);
        // Clean loss, terrible latency.
        assert_eq!(score_grade(&input(300, 1.0, 0.0)), NetGrade::Poor);
        // Clean loss & latency, terrible jitter.
        assert_eq!(score_grade(&input(30, 25.0, 0.0)), NetGrade::Poor);
    }

    #[test]
    fn speed_floor_caps_grade_but_never_upgrades() {
        let mut slow = input(30, 1.0, 0.0);
        slow.download_mbps = Some(3.0);
        assert_eq!(score_grade(&slow), NetGrade::Poor);

        let mut fast = input(400, 30.0, 8.0);
        fast.download_mbps = Some(500.0);
        assert_eq!(score_grade(&fast), NetGrade::Poor); // still Poor — speed can't buy off loss

        let mut ok = input(30, 1.0, 0.0);
        ok.download_mbps = Some(25.0);
        assert_eq!(score_grade(&ok), NetGrade::Excellent);
    }

    // ---- fault_verdict ----

    #[test]
    fn verdict_healthy_when_all_good() {
        let anchors = [
            anchor(true, true, true, Some(40), 0.0),
            anchor(true, false, true, Some(12), 0.0),
            anchor(true, false, true, Some(14), 0.0),
        ];
        assert_eq!(fault_verdict(&anchors), FaultVerdict::Healthy);
    }

    #[test]
    fn verdict_localizes_vps_unreachable() {
        let anchors = [
            anchor(true, true, false, None, 100.0),
            anchor(true, false, true, Some(12), 0.0),
            anchor(true, false, true, Some(14), 0.0),
        ];
        assert_eq!(fault_verdict(&anchors), FaultVerdict::VpsUnreachable);
    }

    #[test]
    fn verdict_localizes_vps_path_degraded() {
        let anchors = [
            anchor(true, true, true, Some(400), 8.0),
            anchor(true, false, true, Some(12), 0.0),
            anchor(true, false, true, Some(14), 0.0),
        ];
        assert_eq!(fault_verdict(&anchors), FaultVerdict::VpsPathDegraded);
    }

    #[test]
    fn verdict_public_trouble_is_local_link() {
        let anchors = [
            anchor(true, true, true, Some(40), 0.0),
            anchor(true, false, true, Some(300), 6.0), // public degraded
            anchor(true, false, true, Some(14), 0.0),
        ];
        assert_eq!(fault_verdict(&anchors), FaultVerdict::LocalLinkDegraded);
    }

    #[test]
    fn verdict_mixed_public_trouble_and_vps_down_is_local() {
        let anchors = [
            anchor(true, true, false, None, 100.0),
            anchor(true, false, true, Some(300), 6.0),
            anchor(true, false, true, Some(14), 0.0),
        ];
        assert_eq!(fault_verdict(&anchors), FaultVerdict::LocalLinkDegraded);
    }

    #[test]
    fn verdict_net_down_when_everything_fails() {
        let anchors = [
            anchor(true, true, false, None, 100.0),
            anchor(true, false, false, None, 100.0),
            anchor(true, false, false, None, 100.0),
        ];
        assert_eq!(fault_verdict(&anchors), FaultVerdict::NetDown);
    }

    #[test]
    fn verdict_quarantined_anchor_is_excluded() {
        let anchors = [
            anchor(true, true, true, Some(40), 0.0),
            anchor(false, false, false, None, 100.0), // quarantined public
            anchor(true, false, true, Some(14), 0.0),
        ];
        assert_eq!(fault_verdict(&anchors), FaultVerdict::Healthy);
    }

    #[test]
    fn verdict_judges_vps_alone_when_public_quarantined() {
        let healthy = [
            anchor(true, true, true, Some(40), 0.0),
            anchor(false, false, false, None, 100.0),
            anchor(false, false, false, None, 100.0),
        ];
        assert_eq!(fault_verdict(&healthy), FaultVerdict::Healthy);

        let vps_down = [
            anchor(true, true, false, None, 100.0),
            anchor(false, false, false, None, 100.0),
            anchor(false, false, false, None, 100.0),
        ];
        // No working public anchor remains to vouch for the local link, so
        // an unreachable VPS is reported honestly as NetDown rather than
        // claiming "public internet OK" without evidence.
        assert_eq!(fault_verdict(&vps_down), FaultVerdict::NetDown);
    }

    #[test]
    fn verdict_measuring_with_no_data() {
        assert_eq!(fault_verdict(&[]), FaultVerdict::Measuring);
    }

    // ---- Classifier hysteresis ----

    #[test]
    fn classifier_accepts_first_observation() {
        let mut c = Classifier::new();
        assert_eq!(
            c.observe(NetGrade::Good),
            GradeTransition::Changed {
                from: NetGrade::Unknown,
                to: NetGrade::Good
            }
        );
        assert_eq!(c.current(), NetGrade::Good);
    }

    #[test]
    fn classifier_requires_consecutive_confirmations() {
        let mut c = Classifier::new();
        c.observe(NetGrade::Good);

        // Two consecutive Poor sightings are not enough (need 3).
        assert_eq!(c.observe(NetGrade::Poor), GradeTransition::Unchanged);
        assert_eq!(c.observe(NetGrade::Poor), GradeTransition::Unchanged);

        // A revert breaks the streak.
        assert_eq!(c.observe(NetGrade::Good), GradeTransition::Unchanged);
        assert_eq!(c.observe(NetGrade::Poor), GradeTransition::Unchanged);
        assert_eq!(c.observe(NetGrade::Poor), GradeTransition::Unchanged);

        // Third consecutive Poor publishes the transition.
        assert_eq!(
            c.observe(NetGrade::Poor),
            GradeTransition::Changed {
                from: NetGrade::Good,
                to: NetGrade::Poor
            }
        );
    }

    #[test]
    fn classifier_escalates_and_recovers_down_quickly() {
        let mut c = Classifier::new();
        c.observe(NetGrade::Good);

        // Down path: only 2 consecutive sightings required.
        assert_eq!(c.observe(NetGrade::Down), GradeTransition::Unchanged);
        assert_eq!(
            c.observe(NetGrade::Down),
            GradeTransition::Changed {
                from: NetGrade::Good,
                to: NetGrade::Down
            }
        );

        // Recovery from Down is also fast (2).
        assert_eq!(c.observe(NetGrade::Good), GradeTransition::Unchanged);
        assert_eq!(
            c.observe(NetGrade::Good),
            GradeTransition::Changed {
                from: NetGrade::Down,
                to: NetGrade::Good
            }
        );
    }
}
