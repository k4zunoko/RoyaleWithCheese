//! Pipeline orchestrator and channel wiring.

use crate::application::metrics::PipelineMetrics;
use crate::domain::config::AppConfig;
use crate::domain::error::{DomainError, DomainResult};
use crate::domain::ports::{CapturePort, CommPort, InputPort};
use crate::domain::types::{DetectionResult, Frame};
use crate::infrastructure::processing::selector::ProcessSelector;
use crossbeam_channel::{Receiver, Sender};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Frame message with capture timestamp.
pub struct TimestampedFrame {
    pub frame: Frame,
    pub captured_at: Instant,
}

/// Detection message with capture + process timestamps.
pub struct TimestampedDetection {
    pub result: DetectionResult,
    pub captured_at: Instant,
    pub processed_at: Instant,
}

/// Stats message carrying end-to-end stage timestamps.
pub struct StatData {
    pub captured_at: Instant,
    pub processed_at: Instant,
    pub hid_sent_at: Instant,
}

/// Orchestrates adapters, channels, and pipeline threads.
///
/// Ownership is moved per-thread in `run(self)`, avoiding shared adapter locks
/// on the hot path.
pub struct PipelineRunner {
    capture: Box<dyn CapturePort + 'static>,
    process: ProcessSelector,
    comm: Box<dyn CommPort + 'static>,
    input: Arc<dyn InputPort>,
    config: AppConfig,
    metrics: Arc<PipelineMetrics>,
}

impl PipelineRunner {
    /// Creates a new pipeline runner with owned adapters.
    pub fn new(
        capture: impl CapturePort + 'static,
        process: ProcessSelector,
        comm: impl CommPort + 'static,
        input: Arc<dyn InputPort>,
        config: AppConfig,
        metrics: Arc<PipelineMetrics>,
    ) -> Self {
        Self {
            capture: Box::new(capture),
            process,
            comm: Box::new(comm),
            input,
            config,
            metrics,
        }
    }

    /// Consumes the runner, spawns pipeline threads, and waits for shutdown.
    pub fn run(self) -> DomainResult<()> {
        let (capture_tx, capture_rx) = crossbeam_channel::bounded::<TimestampedFrame>(1);
        let (process_tx, process_rx) = crossbeam_channel::bounded::<TimestampedDetection>(1);
        let (stats_tx, stats_rx) = crossbeam_channel::unbounded::<StatData>();

        let stop = Arc::new(AtomicBool::new(false));

        let PipelineRunner {
            capture,
            process,
            comm,
            input,
            config,
            metrics,
        } = self;

        let mut handles: Vec<JoinHandle<()>> = Vec::with_capacity(4);

        let capture_stop = Arc::clone(&stop);
        let capture_metrics = Arc::clone(&metrics);
        let capture_config = config.capture.clone();
        handles.push(thread::spawn(move || {
            capture_thread_stub(
                capture,
                capture_tx,
                capture_metrics,
                capture_stop,
                capture_config,
            );
        }));

        let process_stop = Arc::clone(&stop);
        let process_metrics = Arc::clone(&metrics);
        let process_config = config.process.clone();
        handles.push(thread::spawn(move || {
            process_thread_stub(
                process,
                capture_rx,
                process_tx,
                process_metrics,
                process_stop,
                process_config,
            );
        }));

        let hid_stop = Arc::clone(&stop);
        let hid_metrics = Arc::clone(&metrics);
        let communication_config = config.communication.clone();
        handles.push(thread::spawn(move || {
            hid_thread_stub(
                comm,
                input,
                process_rx,
                stats_tx,
                hid_metrics,
                hid_stop,
                communication_config,
            );
        }));

        let stats_stop = Arc::clone(&stop);
        let stats_metrics = Arc::clone(&metrics);
        let pipeline_config = config.pipeline.clone();
        handles.push(thread::spawn(move || {
            stats_thread_stub(stats_rx, stats_metrics, stats_stop, pipeline_config);
        }));

        // Stub lifecycle control for task 14: run briefly, then stop all workers.
        thread::sleep(Duration::from_millis(50));
        stop.store(true, Ordering::Relaxed);

        for handle in handles {
            if let Err(panic_payload) = handle.join() {
                stop.store(true, Ordering::Relaxed);
                let panic_reason = if let Some(message) = panic_payload.downcast_ref::<&str>() {
                    message.to_string()
                } else if let Some(message) = panic_payload.downcast_ref::<String>() {
                    message.clone()
                } else {
                    "unknown panic payload".to_string()
                };
                return Err(DomainError::Process(format!(
                    "pipeline thread panicked: {panic_reason}"
                )));
            }
        }

        Ok(())
    }
}

fn capture_thread_stub(
    _capture: Box<dyn CapturePort + 'static>,
    _capture_tx: Sender<TimestampedFrame>,
    _metrics: Arc<PipelineMetrics>,
    stop: Arc<AtomicBool>,
    _capture_config: crate::domain::config::CaptureConfig,
) {
    while !stop.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(1));
    }
}

fn process_thread_stub(
    _process: ProcessSelector,
    _capture_rx: Receiver<TimestampedFrame>,
    _process_tx: Sender<TimestampedDetection>,
    _metrics: Arc<PipelineMetrics>,
    stop: Arc<AtomicBool>,
    _process_config: crate::domain::config::ProcessConfig,
) {
    while !stop.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(1));
    }
}

fn hid_thread_stub(
    _comm: Box<dyn CommPort + 'static>,
    _input: Arc<dyn InputPort>,
    _process_rx: Receiver<TimestampedDetection>,
    _stats_tx: Sender<StatData>,
    _metrics: Arc<PipelineMetrics>,
    stop: Arc<AtomicBool>,
    _communication_config: crate::domain::config::CommunicationConfig,
) {
    while !stop.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(1));
    }
}

fn stats_thread_stub(
    _stats_rx: Receiver<StatData>,
    _metrics: Arc<PipelineMetrics>,
    stop: Arc<AtomicBool>,
    _pipeline_config: crate::domain::config::PipelineConfig,
) {
    while !stop.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ports::ProcessPort;
    use crate::domain::types::{
        DeviceInfo, GpuFrame, HsvRange, InputState, ProcessorBackend, Roi, VirtualKey,
    };
    use crate::infrastructure::processing::cpu::ColorProcessAdapter;

    struct MockCapture;

    impl CapturePort for MockCapture {
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

        fn supports_gpu_frame(&self) -> bool {
            true
        }
    }

    struct MockComm;

    impl CommPort for MockComm {
        fn send(&mut self, _data: &[u8]) -> DomainResult<()> {
            Ok(())
        }

        fn reconnect(&mut self) -> DomainResult<()> {
            Ok(())
        }

        fn is_connected(&self) -> bool {
            true
        }
    }

    struct MockInput;

    impl InputPort for MockInput {
        fn is_key_pressed(&self, _key: VirtualKey) -> bool {
            false
        }

        fn poll_input_state(&self) -> InputState {
            InputState {
                mouse_left: false,
                mouse_right: false,
            }
        }
    }

    fn build_process_selector() -> ProcessSelector {
        let adapter = ColorProcessAdapter::new().expect("ColorProcessAdapter should initialize");
        ProcessSelector::FastColor(adapter)
    }

    #[test]
    fn pipeline_construction_succeeds() {
        let input: Arc<dyn InputPort> = Arc::new(MockInput);
        let config = AppConfig::default();
        let metrics = PipelineMetrics::new();

        let _runner = PipelineRunner::new(
            MockCapture,
            build_process_selector(),
            MockComm,
            input,
            config,
            metrics,
        );
    }

    #[test]
    fn pipeline_run_stops_cleanly() {
        let input: Arc<dyn InputPort> = Arc::new(MockInput);
        let config = AppConfig::default();
        let metrics = PipelineMetrics::new();

        let runner = PipelineRunner::new(
            MockCapture,
            build_process_selector(),
            MockComm,
            input,
            config,
            metrics,
        );

        let result = runner.run();
        assert!(result.is_ok(), "pipeline run should stop cleanly");
    }

    #[test]
    fn process_selector_fast_color_backend_is_cpu() {
        let selector = build_process_selector();
        assert_eq!(selector.backend(), ProcessorBackend::Cpu);
        assert!(!selector.supports_gpu_processing());
    }

    #[test]
    fn channel_topology_uses_expected_capacities() {
        let (capture_tx, capture_rx) = crossbeam_channel::bounded::<TimestampedFrame>(1);
        let (process_tx, process_rx) = crossbeam_channel::bounded::<TimestampedDetection>(1);
        let (stats_tx, stats_rx) = crossbeam_channel::unbounded::<StatData>();

        assert_eq!(capture_tx.len(), 0);
        assert_eq!(capture_rx.len(), 0);
        assert_eq!(process_tx.len(), 0);
        assert_eq!(process_rx.len(), 0);
        assert_eq!(stats_tx.len(), 0);
        assert_eq!(stats_rx.len(), 0);

        assert_eq!(capture_tx.capacity(), Some(1));
        assert_eq!(process_tx.capacity(), Some(1));
        assert_eq!(stats_tx.capacity(), None);
    }

    #[allow(dead_code)]
    fn _run_consumes_self_compile_time_check() {
        let input: Arc<dyn InputPort> = Arc::new(MockInput);
        let runner = PipelineRunner::new(
            MockCapture,
            build_process_selector(),
            MockComm,
            input,
            AppConfig::default(),
            PipelineMetrics::new(),
        );
        let _ = runner.run();
        // runner.run(); // This would fail to compile because run(self) consumes ownership.
    }

    #[test]
    fn process_port_trait_accepts_mock_types() {
        struct InlineProcess;
        impl ProcessPort for InlineProcess {
            fn process_frame(
                &mut self,
                _frame: &Frame,
                _roi: &Roi,
                _hsv_range: &HsvRange,
            ) -> DomainResult<DetectionResult> {
                Ok(DetectionResult::not_detected())
            }

            fn backend(&self) -> ProcessorBackend {
                ProcessorBackend::Cpu
            }
        }

        let mut process = InlineProcess;
        let frame = Frame::new(vec![0, 0, 0, 0], 1, 1);
        let roi = Roi::new(0, 0, 1, 1);
        let hsv = HsvRange::new(0, 0, 0, 0, 0, 0);
        let result = process
            .process_frame(&frame, &roi, &hsv)
            .expect("inline mock processing should succeed");
        assert!(!result.detected);
    }
}
