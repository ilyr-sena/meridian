//! Pure statistics primitives — no I/O, fully unit-testable.

/// Linear-interpolation percentile (`p` in `0..=100`) of a sample set.
/// Copies and sorts internally; returns `None` when there are no samples.
pub fn percentile(values: &[u32], p: f64) -> Option<u32> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    percentile_sorted(&sorted, p)
}

/// Percentile of an already-sorted slice (avoids re-sorting when the caller
/// keeps a sorted copy).
pub fn percentile_sorted(sorted: &[u32], p: f64) -> Option<u32> {
    if sorted.is_empty() {
        return None;
    }
    if sorted.len() == 1 {
        return Some(sorted[0]);
    }
    let rank = (p.clamp(0.0, 100.0) / 100.0) * (sorted.len() - 1) as f64;
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    if lo == hi {
        return Some(sorted[lo]);
    }
    let frac = rank - lo as f64;
    let value = sorted[lo] as f64 + (sorted[hi] - sorted[lo]) as f64 * frac;
    Some(value.round() as u32)
}

/// Exponentially-weighted moving average. A `prev` of `None` seeds directly
/// with the first sample.
pub fn ewma(prev: Option<f64>, sample: f64, alpha: f64) -> f64 {
    match prev {
        Some(p) => p + alpha * (sample - p),
        None => sample,
    }
}

/// RFC 3550 inter-arrival jitter update: `J += (|D| - J) / 16`, where `D` is
/// the difference between consecutive RTT samples. Produces a smooth,
/// window-independent deviation estimate in milliseconds.
pub fn jitter_step(prev_jitter_ms: f32, prev_rtt_ms: Option<f32>, rtt_ms: f32) -> f32 {
    match prev_rtt_ms {
        Some(prev) => {
            let d = (rtt_ms - prev).abs();
            prev_jitter_ms + (d - prev_jitter_ms) / 16.0
        }
        None => prev_jitter_ms,
    }
}

/// Arithmetic mean; `None` when empty.
pub fn mean(values: &[u32]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    Some(values.iter().map(|&v| v as f64).sum::<f64>() / values.len() as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_interpolates() {
        // 10, 20, 30, ... 100
        let v: Vec<u32> = (1..=10u32).map(|x| x * 10).collect();
        assert_eq!(percentile(&v, 0.0), Some(10));
        assert_eq!(percentile(&v, 50.0), Some(55)); // rank 4.5 → 50 + 0.5*10
        assert_eq!(percentile(&v, 75.0), Some(78)); // rank 6.75 → 70 + 0.75*10
        assert_eq!(percentile(&v, 100.0), Some(100));
    }

    #[test]
    fn percentile_sorts_unsorted_input() {
        let v = [100u32, 10, 50, 30, 70];
        assert_eq!(percentile(&v, 50.0), Some(50));
        assert_eq!(percentile(&v, 0.0), Some(10));
        assert_eq!(percentile(&v, 100.0), Some(100));
    }

    #[test]
    fn percentile_handles_empty_and_single() {
        assert_eq!(percentile(&[], 50.0), None);
        assert_eq!(percentile(&[42], 50.0), Some(42));
        assert_eq!(percentile(&[42], 99.0), Some(42));
    }

    #[test]
    fn ewma_seeds_and_smooths() {
        assert_eq!(ewma(None, 40.0, 0.3), 40.0);
        assert_eq!(ewma(Some(100.0), 50.0, 0.3), 85.0);
        assert_eq!(ewma(Some(85.0), 50.0, 0.3), 74.5);
    }

    #[test]
    fn jitter_follows_rfc3550() {
        // No previous sample → no deviation information yet.
        assert_eq!(jitter_step(0.0, None, 100.0), 0.0);
        // |120 - 100| = 20 → J = 0 + (20 - 0)/16
        assert!((jitter_step(0.0, Some(100.0), 120.0) - 1.25).abs() < 1e-6);
        // |100 - 120| = 20 → J = 1.25 + (20 - 1.25)/16
        assert!((jitter_step(1.25, Some(120.0), 100.0) - 2.421875).abs() < 1e-6);
    }

    #[test]
    fn mean_is_none_when_empty() {
        assert_eq!(mean(&[]), None);
        assert_eq!(mean(&[10, 20, 30]), Some(20.0));
    }
}
