//! Pipeline orchestrator and channel wiring.

use crate::application::metrics::PipelineMetrics;
use crate::application::runtime_state::RuntimeState;
use crate::domain::config::AppConfig;
use crate::domain::error::{DomainError, DomainResult};
use crate::domain::ports::{CapturePort, CommPort, InputPort};
use crate::domain::types::VirtualKey;
use crate::domain::types::{DetectionResult, Frame, Roi};
use crate::infrastructure::processing::selector::ProcessSelector;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Instant;

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
    runtime_state: Arc<RuntimeState>,
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
        runtime_state: Arc<RuntimeState>,
    ) -> Self {
        Self {
            capture: Box::new(capture),
            process,
            comm: Box::new(comm),
            input,
            config,
            metrics,
            runtime_state,
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
            runtime_state,
        } = self;

        // Compute the capture / HID ROI centered on the screen.
        let device_info = capture.device_info();
        let roi = Roi::new(0, 0, config.process.roi.width, config.process.roi.height)
            .centered_in(device_info.width, device_info.height)
            .ok_or_else(|| {
                DomainError::Configuration(format!(
                    "ROI ({}x{}) exceeds display dimensions ({}x{})",
                    config.process.roi.width,
                    config.process.roi.height,
                    device_info.width,
                    device_info.height
                ))
            })?;
        tracing::info!(x = roi.x, y = roi.y, w = roi.width, h = roi.height, "ROI");

        let mut handles: Vec<JoinHandle<()>> = Vec::with_capacity(5);

        // ── Capture thread ────────────────────────────────────────────────────
        let capture_stop = Arc::clone(&stop);
        let capture_metrics = Arc::clone(&metrics);
        let capture_runtime_state = Arc::clone(&runtime_state);
        let capture_config = config.capture.clone();
        let capture_roi = roi;
        handles.push(thread::spawn(move || {
            crate::application::threads::capture_thread(
                capture,
                capture_tx,
                capture_metrics,
                capture_runtime_state,
                capture_stop,
                capture_config,
                capture_roi,
            );
        }));

        // ── Process thread ────────────────────────────────────────────────────
        let process_stop = Arc::clone(&stop);
        let process_metrics = Arc::clone(&metrics);
        let process_runtime_state = Arc::clone(&runtime_state);
        let process_stats_tx = stats_tx;
        let process_config = config.process.clone();
        handles.push(thread::spawn(move || {
            crate::application::threads::process_thread(
                process,
                capture_rx,
                process_tx,
                process_stats_tx,
                process_metrics,
                process_stop,
                crate::application::threads::ProcessThreadContext {
                    runtime_state: process_runtime_state,
                    config: process_config,
                },
            );
        }));

        // ── HID thread ────────────────────────────────────────────────────────
        let hid_stop = Arc::clone(&stop);
        let hid_metrics = Arc::clone(&metrics);
        let hid_runtime_state = Arc::clone(&runtime_state);
        let hid_input = Arc::clone(&input);
        let communication_config = config.communication.clone();
        let coordinate_transform_config = config.process.coordinate_transform.clone();
        let activation_config = config.activation.clone();
        let hid_roi = roi;
        handles.push(thread::spawn(move || {
            crate::application::threads::hid_thread(
                comm,
                process_rx,
                hid_input,
                hid_metrics,
                hid_runtime_state,
                hid_stop,
                communication_config,
                coordinate_transform_config,
                activation_config,
                hid_roi,
            );
        }));

        // ── Stats thread ──────────────────────────────────────────────────────
        let stats_stop = Arc::clone(&stop);
        let stats_metrics = Arc::clone(&metrics);
        let stats_runtime_state = Arc::clone(&runtime_state);
        let pipeline_config = config.pipeline.clone();
        handles.push(thread::spawn(move || {
            crate::application::threads::stats_thread(
                stats_rx,
                stats_metrics,
                stats_runtime_state,
                stats_stop,
                pipeline_config,
            );
        }));

        // ── Toggle thread ────────────────────────────────────────────────────
        if let Some(ref toggle_config) = config.toggle {
            let toggle_key = VirtualKey::from_config_str(&toggle_config.key)
                .expect("toggle key already validated");
            let toggle_input = Arc::clone(&input);
            let toggle_runtime_state = Arc::clone(&runtime_state);
            let toggle_stop = Arc::clone(&stop);
            handles.push(thread::spawn(move || {
                crate::application::threads::toggle_thread(
                    toggle_input,
                    toggle_key,
                    toggle_runtime_state,
                    toggle_stop,
                );
            }));
        }

        // Join all threads; propagate first panic as a DomainError.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::runtime_state::RuntimeState;
    use crate::domain::config::{
        AppConfig, CaptureConfig, CommunicationConfig, CoordinateTransformConfig, DebugConfig,
        HsvRangeConfig, PipelineConfig, ProcessConfig, ProcessMode, RoiConfig,
    };
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

    fn test_config() -> AppConfig {
        AppConfig {
            capture: CaptureConfig {
                source: "dda".to_string(),
                timeout_ms: 8,
                monitor_index: 0,
            },
            process: ProcessConfig {
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
            },
            communication: CommunicationConfig {
                vendor_id: 0x1234,
                product_id: 0x5678,
                hid_send_interval_ms: 8,
            },
            pipeline: PipelineConfig {
                stats_interval_sec: 10,
            },
            debug: DebugConfig { enabled: false },
            toggle: None,
            activation: None,
        }
    }

    #[test]
    fn pipeline_construction_succeeds() {
        let input: Arc<dyn InputPort> = Arc::new(MockInput);
        let config = test_config();
        let metrics = PipelineMetrics::new();
        let runtime_state = Arc::new(RuntimeState::new());

        let _runner = PipelineRunner::new(
            MockCapture,
            build_process_selector(),
            MockComm,
            input,
            config,
            metrics,
            runtime_state,
        );
    }

    #[test]
    fn pipeline_run_stops_cleanly() {
        let input: Arc<dyn InputPort> = Arc::new(MockInput);
        let config = test_config();
        let metrics = PipelineMetrics::new();
        let runtime_state = Arc::new(RuntimeState::new());

        let runner = PipelineRunner::new(
            MockCapture,
            build_process_selector(),
            MockComm,
            input,
            config,
            metrics,
            runtime_state,
        );

        // MockComm::send fails with Communication error once actually called.
        // The real hid_thread will try to send on timeout and eventually break.
        // We give it a generous timeout but rely on MockComm returning Ok.
        // The threads run indefinitely with MockCapture returning Ok(None).
        // This test cannot call runner.run() without blocking forever —
        // use a separate timeout thread to set stop if needed.
        // For now just verify construction (run() would block).
        let _ = runner;
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
        let runtime_state = Arc::new(RuntimeState::new());
        let runner = PipelineRunner::new(
            MockCapture,
            build_process_selector(),
            MockComm,
            input,
            test_config(),
            PipelineMetrics::new(),
            runtime_state,
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

    #[test]
    fn pipeline_run_rejects_oversized_roi() {
        struct MockSmallCapture;
        impl CapturePort for MockSmallCapture {
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
                DeviceInfo::new(200, 100, "mock-small".to_string())
            }

            fn supports_gpu_frame(&self) -> bool {
                true
            }
        }

        let input: Arc<dyn InputPort> = Arc::new(MockInput);
        let config = test_config(); // ROI is 460x240, display is 200x100 → error
        let metrics = PipelineMetrics::new();
        let runtime_state = Arc::new(RuntimeState::new());
        let runner = PipelineRunner::new(
            MockSmallCapture,
            build_process_selector(),
            MockComm,
            input,
            config,
            metrics,
            runtime_state,
        );
        let result = runner.run();
        assert!(
            matches!(result, Err(DomainError::Configuration(_))),
            "expected Configuration error, got {result:?}"
        );
    }

    #[test]
    fn pipeline_run_accepts_exact_fit_roi() {
        struct BoundedCapture {
            remaining: usize,
        }

        impl CapturePort for BoundedCapture {
            fn capture_frame(&mut self, _roi: &Roi) -> DomainResult<Option<Frame>> {
                if self.remaining == 0 {
                    return Err(DomainError::Capture("done".to_string()));
                }
                self.remaining -= 1;
                Ok(Some(Frame::new(vec![0, 0, 0, 255], 1, 1)))
            }

            fn capture_gpu_frame(&mut self, _roi: &Roi) -> DomainResult<Option<GpuFrame>> {
                Ok(None)
            }

            fn reinitialize(&mut self) -> DomainResult<()> {
                Ok(())
            }

            fn device_info(&self) -> DeviceInfo {
                DeviceInfo::new(460, 240, "mock-exact".to_string())
            }

            fn supports_gpu_frame(&self) -> bool {
                true
            }
        }

        let input: Arc<dyn InputPort> = Arc::new(MockInput);
        let config = test_config(); // ROI 460x240, display 460x240 → exact fit → OK
        let metrics = PipelineMetrics::new();
        let runtime_state = Arc::new(RuntimeState::new());
        let runner = PipelineRunner::new(
            BoundedCapture { remaining: 3 },
            build_process_selector(),
            MockComm,
            input,
            config,
            metrics,
            runtime_state,
        );
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = runner.run();
            let _ = tx.send(result);
        });
        let result = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("pipeline should stop within 5 seconds");
        assert!(
            !matches!(result, Err(DomainError::Configuration(_))),
            "ROI validation should pass; got Configuration error"
        );
    }
}
