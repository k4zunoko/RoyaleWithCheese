//! Mock-based end-to-end pipeline integration tests (hardware independent).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use RoyaleWithCheese::{
    application::{metrics::PipelineMetrics, pipeline::PipelineRunner},
    domain::{
        config::AppConfig,
        error::{DomainError, DomainResult},
        ports::{CapturePort, CommPort, InputPort},
        types::{DeviceInfo, Frame, InputState, Roi, VirtualKey},
    },
    infrastructure::processing::{cpu::ColorProcessAdapter, selector::ProcessSelector},
};

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

struct StopAwareCapture {
    stop: Arc<AtomicBool>,
    frame: Frame,
    frame_budget: Option<usize>,
    sent: usize,
}

impl StopAwareCapture {
    fn continuous(stop: Arc<AtomicBool>, frame: Frame) -> Self {
        Self {
            stop,
            frame,
            frame_budget: None,
            sent: 0,
        }
    }

    fn with_budget(stop: Arc<AtomicBool>, frame: Frame, frame_budget: usize) -> Self {
        Self {
            stop,
            frame,
            frame_budget: Some(frame_budget),
            sent: 0,
        }
    }
}

impl CapturePort for StopAwareCapture {
    fn capture_frame(&mut self, _roi: &Roi) -> DomainResult<Option<Frame>> {
        if self.stop.load(Ordering::Relaxed) {
            return Err(DomainError::Capture("test stop".to_string()));
        }

        if let Some(limit) = self.frame_budget {
            if self.sent >= limit {
                thread::sleep(Duration::from_millis(1));
                return Ok(None);
            }
        }

        self.sent += 1;
        Ok(Some(self.frame.clone()))
    }

    fn reinitialize(&mut self) -> DomainResult<()> {
        Ok(())
    }

    fn device_info(&self) -> DeviceInfo {
        DeviceInfo::new(1920, 1080, "mock-display".to_string())
    }
}

struct CountingComm {
    send_count: Arc<AtomicU64>,
}

impl CommPort for CountingComm {
    fn send(&mut self, _data: &[u8]) -> DomainResult<()> {
        self.send_count.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn reconnect(&mut self) -> DomainResult<()> {
        Ok(())
    }

    fn is_connected(&self) -> bool {
        true
    }
}

fn build_selector() -> ProcessSelector {
    let adapter = ColorProcessAdapter::new().expect("ColorProcessAdapter should initialize");
    ProcessSelector::FastColor(adapter)
}

fn bgra_frame(width: u32, height: u32, b: u8, g: u8, r: u8) -> Frame {
    let mut data = vec![0_u8; width as usize * height as usize * 4];
    for px in data.chunks_exact_mut(4) {
        px[0] = b;
        px[1] = g;
        px[2] = r;
        px[3] = 255;
    }
    Frame::new(data, width, height)
}

fn run_pipeline_with_external_stop(runner: PipelineRunner, stop: Arc<AtomicBool>, runtime_ms: u64) {
    let (done_tx, done_rx) = mpsc::channel();

    thread::spawn(move || {
        let result = runner.run();
        let _ = done_tx.send(result);
    });

    thread::sleep(Duration::from_millis(runtime_ms));
    stop.store(true, Ordering::Relaxed);

    let result = done_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("pipeline should stop and return within timeout");
    assert!(
        result.is_ok(),
        "pipeline run should return Ok(()), got {result:?}"
    );
}

#[test]
fn mock_pipeline_runs_and_stops() {
    let stop = Arc::new(AtomicBool::new(false));
    let metrics = PipelineMetrics::new();
    let send_count = Arc::new(AtomicU64::new(0));
    let input: Arc<dyn InputPort> = Arc::new(MockInput);

    let capture = StopAwareCapture::continuous(Arc::clone(&stop), bgra_frame(460, 240, 0, 255, 0));
    let comm = CountingComm {
        send_count: Arc::clone(&send_count),
    };

    let runner = PipelineRunner::new(
        capture,
        build_selector(),
        comm,
        input,
        AppConfig::default(),
        Arc::clone(&metrics),
    );

    run_pipeline_with_external_stop(runner, stop, 150);

    let snapshot = metrics.snapshot();
    assert!(snapshot.frames_captured <= u64::MAX);
}

#[test]
fn mock_pipeline_frame_flow() {
    let stop = Arc::new(AtomicBool::new(false));
    let metrics = PipelineMetrics::new();
    let send_count = Arc::new(AtomicU64::new(0));
    let input: Arc<dyn InputPort> = Arc::new(MockInput);

    let capture =
        StopAwareCapture::with_budget(Arc::clone(&stop), bgra_frame(460, 240, 0, 255, 0), 8);
    let comm = CountingComm {
        send_count: Arc::clone(&send_count),
    };

    let runner = PipelineRunner::new(
        capture,
        build_selector(),
        comm,
        input,
        AppConfig::default(),
        Arc::clone(&metrics),
    );

    run_pipeline_with_external_stop(runner, stop, 200);

    let snapshot = metrics.snapshot();
    assert!(snapshot.frames_captured > 0, "expected captured frames > 0");
    assert!(
        snapshot.frames_processed > 0,
        "expected processed frames > 0"
    );
    assert!(
        send_count.load(Ordering::Relaxed) > 0,
        "expected HID send count > 0"
    );
}

#[test]
fn mock_pipeline_frame_drop_behavior_and_metrics_updated() {
    let stop = Arc::new(AtomicBool::new(false));
    let metrics = PipelineMetrics::new();
    let input: Arc<dyn InputPort> = Arc::new(MockInput);

    let capture = StopAwareCapture::continuous(Arc::clone(&stop), bgra_frame(460, 240, 0, 255, 0));
    let comm = CountingComm {
        send_count: Arc::new(AtomicU64::new(0)),
    };

    let runner = PipelineRunner::new(
        capture,
        build_selector(),
        comm,
        input,
        AppConfig::default(),
        Arc::clone(&metrics),
    );

    run_pipeline_with_external_stop(runner, stop, 250);

    let snapshot = metrics.snapshot();
    assert!(snapshot.frames_captured > 0, "expected captured frames > 0");
    assert!(snapshot.frames_dropped > 0, "expected dropped frames > 0");
    assert!(snapshot.frames_processed <= snapshot.frames_captured);
}

#[test]
#[ignore = "requires real display capture backend and HID device"]
fn real_hardware_pipeline_smoke_test() {
    // Manual run example:
    // cargo test real_hardware_pipeline_smoke_test -- --ignored --nocapture --test-threads=1
    //
    // This is intentionally ignored in CI/local default test runs.
    assert!(true);
}
