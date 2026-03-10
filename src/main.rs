#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::process;
use std::sync::Arc;

use RoyaleWithCheese::application::metrics::PipelineMetrics;
use RoyaleWithCheese::application::pipeline::PipelineRunner;
use RoyaleWithCheese::application::runtime_state::RuntimeState;
use RoyaleWithCheese::domain::config::AppConfig;
use RoyaleWithCheese::domain::ports::CapturePort;
use RoyaleWithCheese::domain::types::Roi;
use RoyaleWithCheese::infrastructure::capture::dda::DdaCaptureAdapter;
use RoyaleWithCheese::infrastructure::capture::wgc::WgcCaptureAdapter;
use RoyaleWithCheese::infrastructure::hid_comm::HidCommAdapter;
use RoyaleWithCheese::infrastructure::input::WindowsInputAdapter;
use RoyaleWithCheese::infrastructure::processing::cpu::ColorProcessAdapter;
use RoyaleWithCheese::infrastructure::processing::gpu::GpuColorAdapter;
use RoyaleWithCheese::infrastructure::processing::selector::ProcessSelector;

mod logging;

fn run() -> Result<(), Box<dyn std::error::Error>> {
    logging::init_logging();

    // Load configuration — fall back to defaults if config.toml is absent.
    let config = AppConfig::from_file("config.toml").unwrap_or_else(|err| {
        tracing::warn!(%err, "config.toml not found or invalid, using defaults");
        AppConfig::default()
    });
    config.validate()?;

    tracing::info!(
        source = %config.capture.source,
        mode   = %config.process.mode,
        "pipeline starting"
    );

    // ── Infrastructure adapters (built before capture so they can be moved) ─
    let comm = HidCommAdapter::new(config.communication.clone())
        .map_err(|e| format!("HID init failed: {e}"))?;
    let input = Arc::new(WindowsInputAdapter::new());
    let metrics = PipelineMetrics::new();
    let runtime_state = Arc::new(RuntimeState::new());

    // ── Process adapter ───────────────────────────────────────────────────────
    let process: ProcessSelector = match config.process.mode.as_str() {
        "fast-color-gpu" => {
            let adapter = GpuColorAdapter::new().map_err(|e| format!("GPU init failed: {e}"))?;
            ProcessSelector::FastColorGpu(adapter)
        }
        _ => {
            // "fast-color" — honour gpu.enabled flag for optional GPU upgrade
            if config.gpu.enabled {
                match GpuColorAdapter::new() {
                    Ok(adapter) => ProcessSelector::FastColorGpu(adapter),
                    Err(e) => {
                        tracing::warn!(%e, "GPU unavailable, falling back to CPU");
                        let cpu = ColorProcessAdapter::new()
                            .map_err(|e2| format!("CPU init failed: {e2}"))?;
                        ProcessSelector::FastColor(cpu)
                    }
                }
            } else {
                let adapter =
                    ColorProcessAdapter::new().map_err(|e| format!("CPU init failed: {e}"))?;
                ProcessSelector::FastColor(adapter)
            }
        }
    };

    // ── Capture adapter — branch on source, passing concrete type to runner ─
    match config.capture.source.as_str() {
        "wgc" => {
            let capture = WgcCaptureAdapter::new(config.capture.monitor_index as usize)
                .map_err(|e| format!("WGC init failed: {e}"))?;
            run_with_capture(
                capture,
                process,
                comm,
                input,
                config,
                metrics,
                runtime_state,
            )
        }
        _ => {
            // Default: "dda"
            let capture = DdaCaptureAdapter::new(
                0,
                config.capture.monitor_index as usize,
                config.capture.timeout_ms,
            )
            .map_err(|e| format!("DDA init failed: {e}"))?;
            run_with_capture(
                capture,
                process,
                comm,
                input,
                config,
                metrics,
                runtime_state,
            )
        }
    }
}

/// Generic helper so `PipelineRunner::new` receives a concrete `CapturePort`.
fn run_with_capture<C: CapturePort + 'static>(
    capture: C,
    process: ProcessSelector,
    comm: HidCommAdapter,
    input: Arc<WindowsInputAdapter>,
    config: AppConfig,
    metrics: Arc<PipelineMetrics>,
    runtime_state: Arc<RuntimeState>,
) -> Result<(), Box<dyn std::error::Error>> {
    let device_info = capture.device_info();
    tracing::info!(
        width  = device_info.width,
        height = device_info.height,
        name   = %device_info.name,
        "capture device"
    );

    let roi = Roi::new(0, 0, config.process.roi.width, config.process.roi.height)
        .centered_in(device_info.width, device_info.height)
        .unwrap_or_else(|| {
            tracing::warn!(
                roi_w = config.process.roi.width,
                roi_h = config.process.roi.height,
                screen_w = device_info.width,
                screen_h = device_info.height,
                "ROI larger than screen, using top-left origin"
            );
            Roi::new(0, 0, config.process.roi.width, config.process.roi.height)
        });

    tracing::info!(x = roi.x, y = roi.y, w = roi.width, h = roi.height, "ROI");

    let runner = PipelineRunner::new(
        capture,
        process,
        comm,
        input,
        config,
        metrics,
        runtime_state,
    );
    runner.run()?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        tracing::error!(%e, "fatal pipeline error");
        process::exit(1);
    }
}
