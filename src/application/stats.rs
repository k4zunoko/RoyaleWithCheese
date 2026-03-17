//! Pipeline statistics aggregation and periodic reporting.

use crate::application::metrics::PipelineMetrics;
use crate::application::runtime_state::RuntimeState;
use crate::domain::config::PipelineConfig;
use crossbeam_channel::{Receiver, RecvTimeoutError};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Stats message carrying end-to-end stage timestamps.
#[derive(Debug, Clone, Copy)]
pub struct StatData {
    pub captured_at: Instant,
    pub processed_at: Instant,
    pub hid_sent_at: Instant,
}

impl StatData {
    pub fn total_latency(&self) -> Duration {
        self.hid_sent_at.saturating_duration_since(self.captured_at)
    }

    pub fn process_to_hid_latency(&self) -> Duration {
        self.hid_sent_at
            .saturating_duration_since(self.processed_at)
    }
}

#[inline]
pub fn advance_stats_report_deadline(
    mut next_report_at: Instant,
    stats_interval: Duration,
    now: Instant,
) -> Instant {
    while next_report_at <= now {
        next_report_at += stats_interval;
    }
    next_report_at
}

#[inline]
pub fn report_stats_if_due(
    metrics: &PipelineMetrics,
    next_report_at: &mut Instant,
    stats_interval: Duration,
) {
    let now = Instant::now();
    if now < *next_report_at {
        return;
    }

    let snapshot = metrics.snapshot();
    tracing::info!(
        process_to_hid_latency_us = snapshot.process_to_hid_latency_us,
        "{}",
        snapshot.display()
    );
    *next_report_at = advance_stats_report_deadline(*next_report_at, stats_interval, now);
}

pub fn stats_thread(
    stats_rx: Receiver<StatData>,
    metrics: Arc<PipelineMetrics>,
    runtime_state: Arc<RuntimeState>,
    stop: Arc<AtomicBool>,
    config: PipelineConfig,
) {
    let stats_interval = Duration::from_secs(config.stats_interval_sec as u64);
    let mut next_report_at = Instant::now() + stats_interval;

    while !stop.load(Ordering::Relaxed) {
        let _active = runtime_state.is_active();
        report_stats_if_due(&metrics, &mut next_report_at, stats_interval);

        let timeout = next_report_at.saturating_duration_since(Instant::now());
        match stats_rx.recv_timeout(timeout) {
            Ok(stat) => {
                metrics.record_total_latency(stat.total_latency());
                metrics.record_process_to_hid_latency(stat.process_to_hid_latency());
                report_stats_if_due(&metrics, &mut next_report_at, stats_interval);
            }
            Err(RecvTimeoutError::Timeout) => {
                report_stats_if_due(&metrics, &mut next_report_at, stats_interval);
            }
            Err(RecvTimeoutError::Disconnected) => {
                stop.store(true, Ordering::Relaxed);
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::runtime_state::RuntimeState;
    use std::thread;

    #[test]
    fn stat_data_reports_total_and_hid_stage_latencies() {
        let captured_at = Instant::now();
        let processed_at = captured_at + Duration::from_millis(5);
        let hid_sent_at = processed_at + Duration::from_millis(3);
        let stat = StatData {
            captured_at,
            processed_at,
            hid_sent_at,
        };

        assert_eq!(stat.total_latency(), Duration::from_millis(8));
        assert_eq!(stat.process_to_hid_latency(), Duration::from_millis(3));
    }

    #[test]
    fn advance_stats_report_deadline_moves_to_next_interval() {
        let base = Instant::now();
        let interval = Duration::from_millis(100);

        let next = advance_stats_report_deadline(base + interval, interval, base + interval);

        assert_eq!(next, base + (interval * 2));
    }

    #[test]
    fn advance_stats_report_deadline_skips_missed_intervals() {
        let base = Instant::now();
        let interval = Duration::from_millis(100);

        let next = advance_stats_report_deadline(
            base + interval,
            interval,
            base + Duration::from_millis(350),
        );

        assert_eq!(next, base + Duration::from_millis(400));
    }

    #[test]
    fn stats_thread_applies_stat_data_to_metrics() {
        let metrics = PipelineMetrics::new();
        let runtime_state = Arc::new(RuntimeState::new());
        let stop = Arc::new(AtomicBool::new(false));
        let (stats_tx, stats_rx) = crossbeam_channel::unbounded();
        let config = PipelineConfig {
            stats_interval_sec: 10,
        };

        let stop_for_thread = Arc::clone(&stop);
        let rs_for_thread = Arc::clone(&runtime_state);
        let metrics_for_thread = Arc::clone(&metrics);
        let handle = thread::spawn(move || {
            stats_thread(
                stats_rx,
                metrics_for_thread,
                rs_for_thread,
                stop_for_thread,
                config,
            );
        });

        let captured_at = Instant::now();
        let processed_at = captured_at + Duration::from_millis(4);
        let hid_sent_at = processed_at + Duration::from_millis(6);
        stats_tx
            .send(StatData {
                captured_at,
                processed_at,
                hid_sent_at,
            })
            .expect("stat data send should succeed");

        thread::sleep(Duration::from_millis(20));
        drop(stats_tx);
        handle
            .join()
            .expect("stats thread should exit on disconnect");

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.total_latency_us, 10_000);
        assert_eq!(snapshot.process_to_hid_latency_us, 6_000);
        assert!(stop.load(Ordering::Relaxed));
    }

    #[test]
    fn stats_thread_sets_stop_on_disconnect() {
        let metrics = PipelineMetrics::new();
        let runtime_state = Arc::new(RuntimeState::new());
        let stop = Arc::new(AtomicBool::new(false));
        let (stats_tx, stats_rx) = crossbeam_channel::unbounded();
        let config = PipelineConfig {
            stats_interval_sec: 10,
        };

        let stop_for_thread = Arc::clone(&stop);
        let rs_for_thread = Arc::clone(&runtime_state);
        let handle = thread::spawn(move || {
            stats_thread(stats_rx, metrics, rs_for_thread, stop_for_thread, config);
        });

        drop(stats_tx);

        handle
            .join()
            .expect("stats thread should exit on disconnect");
        assert!(
            stop.load(Ordering::Relaxed),
            "stop should be set after stats channel disconnect"
        );
    }
}
