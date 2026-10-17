//! Raw network measurement primitives.
//!
//! Every primitive is a standalone async function with explicit timeouts so a
//! single wedged target can never stall the monitor. All measurement is pure
//! TCP — no ICMP, no raw sockets — so it works unprivileged on every platform.

use std::time::{Duration, Instant};

use futures::StreamExt;
use tracing::debug;

/// One completed throughput measurement.
#[derive(Debug, Clone)]
pub struct SpeedMeasurement {
    /// Downstream throughput in megabits per second, computed over the actual
    /// measured span (accurate even when the window overshoots by one chunk).
    pub mbps: f64,
    /// Bytes consumed inside the measurement window.
    pub measured_bytes: u64,
    /// Bytes discarded during warm-up (TLS handshake + TCP slow start).
    pub warmup_bytes: u64,
    /// Actual duration of the measurement window.
    pub elapsed: Duration,
}

/// Maximum wait for any single body chunk before the stream counts as stalled.
const CHUNK_STALL: Duration = Duration::from_secs(5);
/// Minimum data for an honest sample. Also lets a fast link that consumes the
/// whole payload early still produce a valid measurement.
const MIN_MEASURE_SECS: f64 = 1.0;
const MIN_MEASURE_BYTES: u64 = 128 * 1024;

/// Measure the round-trip time of a TCP connect (SYN → SYN-ACK) to `addr`.
///
/// The address is expected to be an `ip:port` literal — no DNS is involved,
/// keeping the sample a pure network-RTT measurement. A connect refusal or a
/// `timeout` expiry is returned as an error and counted as a loss event by
/// the caller.
pub async fn tcp_connect_rtt(addr: &str, timeout: Duration) -> std::io::Result<Duration> {
    let started = Instant::now();
    let connect = tokio::time::timeout(timeout, tokio::net::TcpStream::connect(addr)).await;
    let stream = match connect {
        Ok(result) => result?,
        Err(_) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("connect to {addr} timed out"),
            ));
        }
    };
    let rtt = started.elapsed();
    drop(stream);
    Ok(rtt)
}

/// Measure download throughput by streaming `url`: discard `warmup_bytes`
/// (TLS + slow-start ramp-up), then measure throughput for `measure`.
///
/// Robustness rules:
///   * each individual chunk read is bounded by [`CHUNK_STALL`]; the caller
///     bounds the whole test with an outer timeout,
///   * `keep_going` is consulted between chunks so a session starting mid-test
///     aborts it immediately (never compete with live video traffic),
///   * a stall, a stream error, or an early end-of-stream still returns the
///     partial measurement when it is already statistically usable — only
///     thin/empty samples error out.
pub async fn http_download_speed(
    url: &str,
    warmup_bytes: u64,
    measure: Duration,
    mut keep_going: impl FnMut() -> bool + Send,
) -> anyhow::Result<SpeedMeasurement> {
    let client = reqwest::Client::builder()
        .connect_timeout(CHUNK_STALL)
        .build()?;
    let resp = client.get(url).send().await?.error_for_status()?;
    let mut stream = resp.bytes_stream();

    // --- warm-up: discard until TCP slow start has opened the window ---
    let mut warmed = 0u64;
    while warmed < warmup_bytes {
        if !keep_going() {
            anyhow::bail!("speed test aborted (live session started)");
        }
        match tokio::time::timeout(CHUNK_STALL, stream.next()).await {
            Ok(Some(Ok(chunk))) => warmed += chunk.len() as u64,
            Ok(Some(Err(e))) => anyhow::bail!("download failed during warm-up: {e}"),
            Ok(None) => anyhow::bail!("payload smaller than warm-up ({warmed} bytes)"),
            Err(_) => anyhow::bail!("download stalled during warm-up"),
        }
    }

    // --- measurement window ---
    let started = Instant::now();
    let mut bytes: u64 = 0;

    loop {
        if !keep_going() {
            anyhow::bail!("speed test aborted (live session started)");
        }
        if started.elapsed() >= measure {
            break; // window complete — honest, full sample
        }

        match tokio::time::timeout(CHUNK_STALL, stream.next()).await {
            Ok(Some(Ok(chunk))) => bytes += chunk.len() as u64,
            Ok(Some(Err(_))) => {
                if usable_sample(bytes, started) {
                    debug!("speed stream errored mid-measure; keeping partial sample ({bytes} bytes)");
                    break;
                }
                anyhow::bail!("download failed mid-measure after {bytes} bytes");
            }
            Ok(None) => {
                if usable_sample(bytes, started) {
                    debug!("speed payload fully consumed; keeping partial sample ({bytes} bytes)");
                    break;
                }
                anyhow::bail!("download ended too early ({bytes} bytes)");
            }
            Err(_) => {
                if usable_sample(bytes, started) {
                    debug!("speed stream stalled; keeping partial sample ({bytes} bytes)");
                    break;
                }
                anyhow::bail!("download stalled mid-measure after {bytes} bytes");
            }
        }
    }

    let elapsed = started.elapsed();
    if !usable_sample(bytes, started) {
        anyhow::bail!(
            "insufficient speed sample: {bytes} bytes in {:.2}s",
            elapsed.as_secs_f64()
        );
    }

    let mbps = bytes as f64 * 8.0 / 1.0e6 / elapsed.as_secs_f64();
    Ok(SpeedMeasurement {
        mbps,
        measured_bytes: bytes,
        warmup_bytes: warmed,
        elapsed,
    })
}

/// Whether a (possibly partial) measurement window carries enough data to be
/// an honest throughput estimate.
fn usable_sample(bytes: u64, started: Instant) -> bool {
    bytes >= MIN_MEASURE_BYTES && started.elapsed().as_secs_f64() >= MIN_MEASURE_SECS
}
