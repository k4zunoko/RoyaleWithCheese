//! Per-thread runner functions used by the pipeline.

use crate::application::metrics::PipelineMetrics;
use crate::application::pipeline::{StatData, TimestampedDetection, TimestampedFrame};
use crate::application::recovery::{RecoveryState, RecoveryStrategy};
use crate::application::runtime_state::RuntimeState;
use crate::domain::config::{CaptureConfig, CommunicationConfig, PipelineConfig, ProcessConfig};
use crate::domain::error::DomainResult;
use crate::domain::ports::{
    apply_coordinate_transform, coordinates_to_hid_report, CapturePort, CommPort, InputPort,
    ProcessPort,
};
use crate::domain::types::{DetectionResult, HsvRange, Roi, VirtualKey};
use crate::infrastructure::processing::selector::ProcessSelector;
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TrySendError};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

#[inline]
fn process_roi_from_config(config: &ProcessConfig) -> Roi {
    Roi::new(0, 0, config.roi.width, config.roi.height)
}

#[inline]
fn hsv_range_from_config(config: &ProcessConfig) -> HsvRange {
    HsvRange::new(
        config.hsv_range.h_low,
        config.hsv_range.h_high,
        config.hsv_range.s_low,
        config.hsv_range.s_high,
        config.hsv_range.v_low,
        config.hsv_range.v_high,
    )
}

fn send_latest_only<T>(tx: &Sender<T>, item: T, metrics: &PipelineMetrics) {
    match tx.try_send(item) {
        Ok(_) => {}
        Err(TrySendError::Full(_)) => {
            metrics.record_frame_drop();
        }
        Err(TrySendError::Disconnected(_)) => {
            // channel closed — stop signal will fire shortly
        }
    }
}

pub struct ProcessThreadContext {
    pub runtime_state: Arc<RuntimeState>,
    pub config: ProcessConfig,
}

pub fn capture_thread(
    mut capture: Box<dyn CapturePort + 'static>,
    tx: Sender<TimestampedFrame>,
    metrics: Arc<PipelineMetrics>,
    runtime_state: Arc<RuntimeState>,
    stop: Arc<AtomicBool>,
    _config: CaptureConfig,
    roi: Roi,
) {
    let mut recovery = RecoveryState::new();
    let strategy = RecoveryStrategy::new(100, 3200, 5);

    while !stop.load(Ordering::Relaxed) {
        let _active = runtime_state.is_active();
        let t0 = Instant::now();
        match capture.capture_frame(&roi) {
            Ok(Some(frame)) => {
                strategy.record_success(&mut recovery);
                let captured_at = Instant::now();
                send_latest_only(&tx, TimestampedFrame { frame, captured_at }, &metrics);
                metrics.record_capture(t0.elapsed());
            }
            Ok(None) => {}
            Err(error) if error.is_recoverable() => {
                tracing::warn!(%error, "recoverable capture error");
                strategy.record_failure(&mut recovery);
                let backoff_ms = strategy.next_backoff_ms(&recovery);
                thread::sleep(Duration::from_millis(backoff_ms));
                if let Err(reinit_error) = capture.reinitialize() {
                    tracing::warn!(%reinit_error, "capture reinitialize failed during recovery");
                }
                if !strategy.should_attempt(&recovery) {
                    tracing::error!("capture recovery retries exceeded; stopping capture thread");
                    break;
                }
            }
            Err(error) => {
                tracing::error!(%error, "non-recoverable capture error");
                break;
            }
        }
    }
}

pub fn process_thread(
    mut process: ProcessSelector,
    rx: Receiver<TimestampedFrame>,
    tx: Sender<TimestampedDetection>,
    stats_tx: Sender<StatData>,
    metrics: Arc<PipelineMetrics>,
    stop: Arc<AtomicBool>,
    context: ProcessThreadContext,
) {
    let runtime_state = context.runtime_state;
    let config = context.config;
    let roi = process_roi_from_config(&config);
    let hsv_range = hsv_range_from_config(&config);

    while !stop.load(Ordering::Relaxed) {
        let _active = runtime_state.is_active();
        match rx.recv_timeout(Duration::from_millis(10)) {
            Ok(timestamped_frame) => {
                let t0 = Instant::now();
                match process.process_frame(&timestamped_frame.frame, &roi, &hsv_range) {
                    Ok(result) => {
                        metrics.record_process(t0.elapsed());
                        let processed_at = Instant::now();
                        send_latest_only(
                            &tx,
                            TimestampedDetection {
                                result,
                                captured_at: timestamped_frame.captured_at,
                                processed_at,
                            },
                            &metrics,
                        );
                        let _ = stats_tx.try_send(StatData {
                            captured_at: timestamped_frame.captured_at,
                            processed_at,
                            hid_sent_at: processed_at,
                        });
                    }
                    Err(error) if error.is_recoverable() => {
                        tracing::warn!(%error, "recoverable process error");
                    }
                    Err(error) => {
                        tracing::error!(%error, "non-recoverable process error");
                        break;
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn send_detection_report(
    comm: &mut dyn CommPort,
    detection: &DetectionResult,
    roi: &Roi,
    sensitivity: f64,
    dead_zone: f64,
    x_clip_limit: f64,
    y_clip_limit: f64,
    metrics: &PipelineMetrics,
) -> DomainResult<()> {
    let t0 = Instant::now();
    let transformed = apply_coordinate_transform(
        detection,
        roi,
        sensitivity,
        dead_zone,
        x_clip_limit,
        y_clip_limit,
    );
    let report = coordinates_to_hid_report(&transformed);
    match comm.send(&report) {
        Ok(_) => {
            metrics.record_hid_send(t0.elapsed());
            Ok(())
        }
        Err(error) => {
            metrics.record_hid_error();
            Err(error)
        }
    }
}

pub fn hid_thread(
    mut comm: Box<dyn CommPort + 'static>,
    rx: Receiver<TimestampedDetection>,
    metrics: Arc<PipelineMetrics>,
    runtime_state: Arc<RuntimeState>,
    stop: Arc<AtomicBool>,
    config: CommunicationConfig,
    roi: Roi,
) {
    let hid_send_interval = Duration::from_millis(config.hid_send_interval_ms as u64);
    // CommunicationConfig has no sensitivity; use neutral multiplier.
    let sensitivity = 1.0_f64;
    let mut last_detection: Option<DetectionResult> = None;
    let mut recovery = RecoveryState::new();
    let strategy = RecoveryStrategy::new(100, 3200, 5);

    while !stop.load(Ordering::Relaxed) {
        match rx.recv_timeout(hid_send_interval) {
            Ok(timestamped_detection) => {
                last_detection = Some(timestamped_detection.result.clone());
                if !runtime_state.is_active() {
                    continue;
                }

                match send_detection_report(
                    &mut *comm,
                    &timestamped_detection.result,
                    &roi,
                    sensitivity,
                    0.0,
                    0.0,
                    0.0,
                    &metrics,
                ) {
                    Ok(()) => strategy.record_success(&mut recovery),
                    Err(error) if error.is_recoverable() => {
                        tracing::warn!(%error, "recoverable hid send error");
                        strategy.record_failure(&mut recovery);
                        let backoff_ms = strategy.next_backoff_ms(&recovery);
                        thread::sleep(Duration::from_millis(backoff_ms));
                        if let Err(reconnect_error) = comm.reconnect() {
                            tracing::warn!(%reconnect_error, "hid reconnect failed during recovery");
                        }
                        if !strategy.should_attempt(&recovery) {
                            tracing::error!("hid recovery retries exceeded; stopping hid thread");
                            break;
                        }
                    }
                    Err(error) => {
                        tracing::error!(%error, "non-recoverable hid send error");
                        break;
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                let detection = last_detection
                    .as_ref()
                    .cloned()
                    .unwrap_or_else(DetectionResult::not_detected);
                if !runtime_state.is_active() {
                    continue;
                }

                match send_detection_report(
                    &mut *comm,
                    &detection,
                    &roi,
                    sensitivity,
                    0.0,
                    0.0,
                    0.0,
                    &metrics,
                ) {
                    Ok(()) => strategy.record_success(&mut recovery),
                    Err(error) if error.is_recoverable() => {
                        tracing::warn!(%error, "recoverable hid send error");
                        strategy.record_failure(&mut recovery);
                        let backoff_ms = strategy.next_backoff_ms(&recovery);
                        thread::sleep(Duration::from_millis(backoff_ms));
                        if let Err(reconnect_error) = comm.reconnect() {
                            tracing::warn!(%reconnect_error, "hid reconnect failed during recovery");
                        }
                        if !strategy.should_attempt(&recovery) {
                            tracing::error!("hid recovery retries exceeded; stopping hid thread");
                            break;
                        }
                    }
                    Err(error) => {
                        tracing::error!(%error, "non-recoverable hid send error");
                        break;
                    }
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

pub fn stats_thread(
    stats_rx: Receiver<StatData>,
    metrics: Arc<PipelineMetrics>,
    runtime_state: Arc<RuntimeState>,
    stop: Arc<AtomicBool>,
    config: PipelineConfig,
) {
    let stats_interval = Duration::from_secs(config.stats_interval_sec as u64);
    while !stop.load(Ordering::Relaxed) {
        let _active = runtime_state.is_active();
        match stats_rx.recv_timeout(stats_interval) {
            Ok(_stat) => {}
            Err(RecvTimeoutError::Timeout) => {
                let snapshot = metrics.snapshot();
                tracing::info!("{}", snapshot.display());
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

pub fn toggle_thread(
    input: Arc<dyn InputPort>,
    toggle_key: VirtualKey,
    runtime_state: Arc<RuntimeState>,
    stop: Arc<AtomicBool>,
) {
    let mut prev_pressed = false;
    while !stop.load(Ordering::Relaxed) {
        let currently_pressed = input.is_key_pressed(toggle_key);
        if !prev_pressed && currently_pressed {
            // SAFETY: toggle() の load-store は非原子的だが、
            // この関数は単一スレッドからのみ呼び出されるため安全。
            runtime_state.toggle();
            if runtime_state.is_active() {
                crate::infrastructure::sound::play_toggle_on_sound();
                tracing::info!("toggle: active=true");
            } else {
                crate::infrastructure::sound::play_toggle_off_sound();
                tracing::info!("toggle: active=false");
            }
        }
        prev_pressed = currently_pressed;
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::runtime_state::RuntimeState;
    use crate::domain::config::{
        CaptureConfig, CommunicationConfig, CoordinateTransformConfig, HsvRangeConfig,
        ProcessConfig, ProcessMode, RoiConfig,
    };
    use crate::domain::error::DomainResult;
    use crate::domain::ports::CapturePort;
    use crate::domain::types::{DeviceInfo, Frame, GpuFrame, HsvRange, ProcessorBackend, Roi};
    use crate::infrastructure::processing::cpu::ColorProcessAdapter;
    use crossbeam_channel::bounded;
    use std::sync::atomic::AtomicUsize;
    use std::thread;

    struct SingleFrameCapture {
        sent_once: bool,
    }

    impl CapturePort for SingleFrameCapture {
        fn capture_frame(&mut self, _roi: &Roi) -> DomainResult<Option<Frame>> {
            if self.sent_once {
                Ok(None)
            } else {
                self.sent_once = true;
                Ok(Some(Frame::new(vec![1, 2, 3, 4], 1, 1)))
            }
        }

        fn capture_gpu_frame(&mut self, _roi: &Roi) -> DomainResult<Option<GpuFrame>> {
            Ok(None)
        }

        fn reinitialize(&mut self) -> DomainResult<()> {
            Ok(())
        }

        fn device_info(&self) -> DeviceInfo {
            DeviceInfo::new(1920, 1080, "mock".to_string())
        }
    }

    struct NoneCapture;

    impl CapturePort for NoneCapture {
        fn capture_frame(&mut self, _roi: &Roi) -> DomainResult<Option<Frame>> {
            Ok(None)
        }

        fn capture_gpu_frame(&mut self, _roi: &Roi) -> DomainResult<Option<GpuFrame>> {
            Ok(None)
        }

        fn reinitialize(&mut self) -> DomainResult<()> {
            Ok(())
        }

        fn device_info(&self) -> DeviceInfo {
            DeviceInfo::new(1920, 1080, "mock".to_string())
        }
    }

    struct RecordingComm {
        send_count: Arc<AtomicUsize>,
        notify_tx: std::sync::mpsc::Sender<()>,
    }

    impl CommPort for RecordingComm {
        fn send(&mut self, _data: &[u8]) -> DomainResult<()> {
            self.send_count.fetch_add(1, Ordering::Relaxed);
            let _ = self.notify_tx.send(());
            Ok(())
        }

        fn reconnect(&mut self) -> DomainResult<()> {
            Ok(())
        }

        fn is_connected(&self) -> bool {
            true
        }
    }

    fn build_process_selector() -> ProcessSelector {
        let adapter = ColorProcessAdapter::new().expect("ColorProcessAdapter should initialize");
        ProcessSelector::FastColor(adapter)
    }

    fn test_capture_config() -> CaptureConfig {
        CaptureConfig {
            source: "dda".to_string(),
            timeout_ms: 8,
            monitor_index: 0,
        }
    }

    fn test_process_config() -> ProcessConfig {
        ProcessConfig {
            mode: ProcessMode::FastColor,
            roi: RoiConfig {
                width: 460,
                height: 240,
            },
            hsv_range: HsvRangeConfig {
                h_low: 25,
                h_high: 45,
                s_low: 80,
                s_high: 255,
                v_low: 80,
                v_high: 255,
            },
            coordinate_transform: CoordinateTransformConfig {
                sensitivity: 1.0,
                x_clip_limit: 10.0,
                y_clip_limit: 10.0,
                dead_zone: 0.0,
            },
        }
    }

    fn test_communication_config() -> CommunicationConfig {
        CommunicationConfig {
            vendor_id: 0x1234,
            product_id: 0x5678,
            hid_send_interval_ms: 8,
        }
    }

    #[test]
    fn capture_thread_sends_frame_to_channel() {
        let metrics = PipelineMetrics::new();
        let runtime_state = Arc::new(RuntimeState::new());
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = bounded(1);
        let roi = Roi::new(0, 0, 1, 1);
        let capture = Box::new(SingleFrameCapture { sent_once: false });
        let capture_config = test_capture_config();

        let stop_for_thread = Arc::clone(&stop);
        let runtime_state_for_thread = Arc::clone(&runtime_state);
        let handle = thread::spawn(move || {
            capture_thread(
                capture,
                tx,
                metrics,
                runtime_state_for_thread,
                stop_for_thread,
                capture_config,
                roi,
            )
        });

        let msg = rx
            .recv_timeout(Duration::from_millis(100))
            .expect("captured frame should arrive");
        assert_eq!(msg.frame.width, 1);
        assert_eq!(msg.frame.height, 1);

        stop.store(true, Ordering::Relaxed);
        handle.join().expect("capture thread should exit");
    }

    #[test]
    fn process_thread_processes_received_frame() {
        let metrics = PipelineMetrics::new();
        let runtime_state = Arc::new(RuntimeState::new());
        let stop = Arc::new(AtomicBool::new(false));
        let (capture_tx, capture_rx) = bounded(1);
        let (process_tx, process_rx) = bounded(1);
        let (stats_tx, _stats_rx) = bounded(4);
        let process = build_process_selector();
        let process_config = test_process_config();

        let stop_for_thread = Arc::clone(&stop);
        let runtime_state_for_thread = Arc::clone(&runtime_state);
        let handle = thread::spawn(move || {
            process_thread(
                process,
                capture_rx,
                process_tx,
                stats_tx,
                metrics,
                stop_for_thread,
                ProcessThreadContext {
                    runtime_state: runtime_state_for_thread,
                    config: process_config,
                },
            )
        });

        let frame = Frame::new(vec![0, 255, 0, 255], 1, 1);
        capture_tx
            .send(TimestampedFrame {
                frame,
                captured_at: Instant::now(),
            })
            .expect("input frame send should succeed");

        let detection = process_rx
            .recv_timeout(Duration::from_millis(300))
            .expect("detection should arrive");
        assert!(detection.processed_at >= detection.captured_at);

        stop.store(true, Ordering::Relaxed);
        handle.join().expect("process thread should exit");
    }

    #[test]
    fn hid_thread_sends_hid_report() {
        let metrics = PipelineMetrics::new();
        let runtime_state = Arc::new(RuntimeState::new());
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = bounded(1);
        let (notify_tx, notify_rx) = std::sync::mpsc::channel();
        let send_count = Arc::new(AtomicUsize::new(0));
        let comm = Box::new(RecordingComm {
            send_count: Arc::clone(&send_count),
            notify_tx,
        });
        let config = test_communication_config();
        let roi = Roi::new(0, 0, 460, 240);

        let stop_for_thread = Arc::clone(&stop);
        let runtime_state_for_thread = Arc::clone(&runtime_state);
        let handle = thread::spawn(move || {
            hid_thread(
                comm,
                rx,
                metrics,
                runtime_state_for_thread,
                stop_for_thread,
                config,
                roi,
            )
        });

        tx.send(TimestampedDetection {
            result: DetectionResult::detected(10.0, 10.0, 0.5),
            captured_at: Instant::now(),
            processed_at: Instant::now(),
        })
        .expect("detection send should succeed");

        notify_rx
            .recv_timeout(Duration::from_millis(200))
            .expect("hid send should be called");
        assert!(send_count.load(Ordering::Relaxed) >= 1);

        stop.store(true, Ordering::Relaxed);
        handle.join().expect("hid thread should exit");
    }

    #[test]
    fn send_latest_only_drops_when_full() {
        let metrics = PipelineMetrics::new();
        let (tx, _rx) = bounded::<u32>(1);
        tx.try_send(1).expect("first item should fill channel");

        send_latest_only(&tx, 2, &metrics);

        let dropped = metrics.snapshot().frames_dropped;
        println!("frames_dropped={dropped}");
        assert_eq!(dropped, 1);
    }

    #[test]
    fn capture_thread_stops_on_signal() {
        let metrics = PipelineMetrics::new();
        let runtime_state = Arc::new(RuntimeState::new());
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, _rx) = bounded(1);
        let roi = Roi::new(0, 0, 1, 1);
        let capture = Box::new(NoneCapture);
        let capture_config = test_capture_config();
        let (done_tx, done_rx) = std::sync::mpsc::channel();

        let stop_for_thread = Arc::clone(&stop);
        let runtime_state_for_thread = Arc::clone(&runtime_state);
        let handle = thread::spawn(move || {
            capture_thread(
                capture,
                tx,
                metrics,
                runtime_state_for_thread,
                stop_for_thread,
                capture_config,
                roi,
            );
            let _ = done_tx.send(());
        });

        stop.store(true, Ordering::Relaxed);
        done_rx
            .recv_timeout(Duration::from_millis(200))
            .expect("capture thread should stop quickly");
        handle.join().expect("capture thread should join");
    }

    #[test]
    fn process_config_mapping_helpers_match_expected_values() {
        let config = test_process_config();
        let roi = process_roi_from_config(&config);
        let hsv = hsv_range_from_config(&config);
        assert_eq!(roi, Roi::new(0, 0, config.roi.width, config.roi.height));
        assert_eq!(hsv, HsvRange::new(25, 45, 80, 255, 80, 255));
    }

    #[test]
    fn process_selector_fast_color_backend_is_cpu() {
        let selector = build_process_selector();
        assert_eq!(selector.backend(), ProcessorBackend::Cpu);
        assert!(!selector.supports_gpu_processing());
    }
    struct MockInput {
        pressed: std::sync::Mutex<Vec<bool>>,
        call_index: std::sync::Mutex<usize>,
    }

    impl MockInput {
        fn always_released() -> Self {
            Self {
                pressed: std::sync::Mutex::new(vec![]),
                call_index: std::sync::Mutex::new(0),
            }
        }

        /// Returns true for the first `n` calls then false.
        fn press_for_n_calls(n: usize) -> Self {
            let values = (0..100).map(|i| i < n).collect();
            Self {
                pressed: std::sync::Mutex::new(values),
                call_index: std::sync::Mutex::new(0),
            }
        }
    }

    impl InputPort for MockInput {
        fn is_key_pressed(&self, _key: VirtualKey) -> bool {
            let values = self.pressed.lock().unwrap();
            let mut idx = self.call_index.lock().unwrap();
            let result = values.get(*idx).copied().unwrap_or(false);
            *idx += 1;
            result
        }
    }

    #[test]
    fn toggle_thread_stops_on_flag() {
        let input = Arc::new(MockInput::always_released());
        let runtime_state = Arc::new(RuntimeState::new());
        let stop = Arc::new(AtomicBool::new(false));
        let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();

        let input_c = Arc::clone(&input);
        let rs_c = Arc::clone(&runtime_state);
        let stop_c = Arc::clone(&stop);
        thread::spawn(move || {
            toggle_thread(input_c, VirtualKey::LeftButton, rs_c, stop_c);
            let _ = done_tx.send(());
        });

        thread::sleep(Duration::from_millis(30));
        stop.store(true, Ordering::Relaxed);
        done_rx
            .recv_timeout(Duration::from_millis(200))
            .expect("toggle_thread should exit after stop flag is set");
    }

    #[test]
    fn toggle_thread_edge_detection() {
        // Returns true for first 3 calls (rising edge at call 0), then false.
        // This produces exactly one rising edge → one toggle.
        let input = Arc::new(MockInput::press_for_n_calls(3));
        let runtime_state = Arc::new(RuntimeState::new());
        let stop = Arc::new(AtomicBool::new(false));
        let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();

        let initial_active = runtime_state.is_active(); // true

        let input_c = Arc::clone(&input);
        let rs_c = Arc::clone(&runtime_state);
        let stop_c = Arc::clone(&stop);
        thread::spawn(move || {
            toggle_thread(input_c, VirtualKey::LeftButton, rs_c, stop_c);
            let _ = done_tx.send(());
        });

        // Wait enough iterations for the mock values to be consumed and toggle to occur.
        thread::sleep(Duration::from_millis(100));
        stop.store(true, Ordering::Relaxed);
        done_rx
            .recv_timeout(Duration::from_millis(200))
            .expect("toggle_thread should exit");

        // Exactly one toggle: active flipped from initial.
        assert_ne!(
            runtime_state.is_active(),
            initial_active,
            "runtime_state should have been toggled exactly once"
        );
    }
}
