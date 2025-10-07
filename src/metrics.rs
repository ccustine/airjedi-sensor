//! Lightweight metrics system for tracking ADS-B decoder performance
//!
//! Uses atomic counters for lock-free, zero-overhead metrics collection.
//! All operations are thread-safe and designed to have minimal performance impact.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Global metrics for the ADS-B decoder
pub struct GlobalMetrics {
    // Preamble detection
    pub preambles_detected: AtomicU64,

    // Decoder metrics
    pub packets_crc_passed: AtomicU64,
    pub packets_crc_failed: AtomicU64,
    pub packets_decoded: AtomicU64,
    pub packets_decode_failed: AtomicU64,

    // Message types (by ADS-B ME field)
    pub msg_identification: AtomicU64,
    pub msg_position: AtomicU64,
    pub msg_velocity: AtomicU64,
    pub msg_other: AtomicU64,

    // Tracker
    pub aircraft_tracked: AtomicU64,
    pub updates_processed: AtomicU64,

    // Output modules
    pub output_beast: AtomicU64,
    pub output_raw: AtomicU64,
    pub output_sbs1: AtomicU64,
    pub output_websocket: AtomicU64,
}

impl GlobalMetrics {
    /// Create a new GlobalMetrics instance
    pub const fn new() -> Self {
        Self {
            preambles_detected: AtomicU64::new(0),
            packets_crc_passed: AtomicU64::new(0),
            packets_crc_failed: AtomicU64::new(0),
            packets_decoded: AtomicU64::new(0),
            packets_decode_failed: AtomicU64::new(0),
            msg_identification: AtomicU64::new(0),
            msg_position: AtomicU64::new(0),
            msg_velocity: AtomicU64::new(0),
            msg_other: AtomicU64::new(0),
            aircraft_tracked: AtomicU64::new(0),
            updates_processed: AtomicU64::new(0),
            output_beast: AtomicU64::new(0),
            output_raw: AtomicU64::new(0),
            output_sbs1: AtomicU64::new(0),
            output_websocket: AtomicU64::new(0),
        }
    }

    /// Get a snapshot of all current metric values
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            preambles_detected: self.preambles_detected.load(Ordering::Relaxed),
            packets_crc_passed: self.packets_crc_passed.load(Ordering::Relaxed),
            packets_crc_failed: self.packets_crc_failed.load(Ordering::Relaxed),
            packets_decoded: self.packets_decoded.load(Ordering::Relaxed),
            packets_decode_failed: self.packets_decode_failed.load(Ordering::Relaxed),
            msg_identification: self.msg_identification.load(Ordering::Relaxed),
            msg_position: self.msg_position.load(Ordering::Relaxed),
            msg_velocity: self.msg_velocity.load(Ordering::Relaxed),
            msg_other: self.msg_other.load(Ordering::Relaxed),
            aircraft_tracked: self.aircraft_tracked.load(Ordering::Relaxed),
            updates_processed: self.updates_processed.load(Ordering::Relaxed),
            output_beast: self.output_beast.load(Ordering::Relaxed),
            output_raw: self.output_raw.load(Ordering::Relaxed),
            output_sbs1: self.output_sbs1.load(Ordering::Relaxed),
            output_websocket: self.output_websocket.load(Ordering::Relaxed),
            uptime: start_time().elapsed(),
        }
    }
}

/// Global metrics instance
static METRICS: GlobalMetrics = GlobalMetrics::new();

/// Start time for uptime calculation
static START_TIME: OnceLock<Instant> = OnceLock::new();

/// Get the start time, initializing it if necessary
fn start_time() -> &'static Instant {
    START_TIME.get_or_init(Instant::now)
}

/// Get reference to global metrics
pub fn metrics() -> &'static GlobalMetrics {
    &METRICS
}

/// Snapshot of metrics at a point in time
#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    pub preambles_detected: u64,
    pub packets_crc_passed: u64,
    pub packets_crc_failed: u64,
    pub packets_decoded: u64,
    pub packets_decode_failed: u64,
    pub msg_identification: u64,
    pub msg_position: u64,
    pub msg_velocity: u64,
    pub msg_other: u64,
    pub aircraft_tracked: u64,
    pub updates_processed: u64,
    pub output_beast: u64,
    pub output_raw: u64,
    pub output_sbs1: u64,
    pub output_websocket: u64,
    pub uptime: Duration,
}

impl MetricsSnapshot {
    /// Calculate total packets received
    pub fn total_packets(&self) -> u64 {
        self.packets_crc_passed + self.packets_crc_failed
    }

    /// Calculate CRC pass rate as a percentage
    pub fn crc_pass_rate(&self) -> f64 {
        let total = self.total_packets();
        if total == 0 {
            0.0
        } else {
            (self.packets_crc_passed as f64 / total as f64) * 100.0
        }
    }

    /// Calculate decode success rate as a percentage
    pub fn decode_success_rate(&self) -> f64 {
        let total = self.packets_decoded + self.packets_decode_failed;
        if total == 0 {
            0.0
        } else {
            (self.packets_decoded as f64 / total as f64) * 100.0
        }
    }

    /// Calculate messages per second
    pub fn messages_per_second(&self) -> f64 {
        let secs = self.uptime.as_secs_f64();
        if secs == 0.0 {
            0.0
        } else {
            self.packets_decoded as f64 / secs
        }
    }

    /// Calculate total messages sent to all outputs
    pub fn total_output_messages(&self) -> u64 {
        self.output_beast + self.output_raw + self.output_sbs1 + self.output_websocket
    }

    /// Format a compact summary string for logging
    pub fn format_summary(&self) -> String {
        format!(
            "{} pkts ({:.1}% CRC OK), {} decoded, {} aircraft, Types: {} ID, {} Pos, {} Vel | {:.0} msg/s",
            self.total_packets(),
            self.crc_pass_rate(),
            self.packets_decoded,
            self.aircraft_tracked,
            self.msg_identification,
            self.msg_position,
            self.msg_velocity,
            self.messages_per_second()
        )
    }

    /// Format detailed metrics for logging
    pub fn format_detailed(&self) -> String {
        format!(
            "Metrics Summary:\n\
             ├─ Decoder: {} packets ({:.1}% CRC OK), {} decoded ({:.1}% success)\n\
             ├─ Messages: {} ID, {} Pos, {} Vel, {} Other\n\
             ├─ Aircraft: {} tracked, {} updates processed\n\
             ├─ Outputs: {} BEAST, {} Raw, {} SBS-1, {} WebSocket\n\
             └─ Performance: {:.0} msg/s over {:.0}s uptime",
            self.total_packets(),
            self.crc_pass_rate(),
            self.packets_decoded,
            self.decode_success_rate(),
            self.msg_identification,
            self.msg_position,
            self.msg_velocity,
            self.msg_other,
            self.aircraft_tracked,
            self.updates_processed,
            self.output_beast,
            self.output_raw,
            self.output_sbs1,
            self.output_websocket,
            self.messages_per_second(),
            self.uptime.as_secs_f64()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_increment() {
        let m = GlobalMetrics::new();
        assert_eq!(m.packets_crc_passed.load(Ordering::Relaxed), 0);

        m.packets_crc_passed.fetch_add(1, Ordering::Relaxed);
        assert_eq!(m.packets_crc_passed.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_snapshot() {
        let m = GlobalMetrics::new();
        m.packets_crc_passed.fetch_add(100, Ordering::Relaxed);
        m.packets_crc_failed.fetch_add(5, Ordering::Relaxed);

        let snap = m.snapshot();
        assert_eq!(snap.total_packets(), 105);
        assert!((snap.crc_pass_rate() - 95.238).abs() < 0.01);
    }

    #[test]
    fn test_format_summary() {
        let snap = MetricsSnapshot {
            preambles_detected: 0,
            packets_crc_passed: 1000,
            packets_crc_failed: 50,
            packets_decoded: 980,
            packets_decode_failed: 20,
            msg_identification: 100,
            msg_position: 600,
            msg_velocity: 280,
            msg_other: 0,
            aircraft_tracked: 45,
            updates_processed: 980,
            output_beast: 0,
            output_raw: 0,
            output_sbs1: 0,
            output_websocket: 0,
            uptime: Duration::from_secs(10),
        };

        let summary = snap.format_summary();
        assert!(summary.contains("1050 pkts"));
        assert!(summary.contains("95.2% CRC OK"));
        assert!(summary.contains("980 decoded"));
        assert!(summary.contains("45 aircraft"));
    }
}
