mod domain;
mod logging;
mod application;
mod infrastructure;

use crate::domain::config::AppConfig;
use crate::domain::ports::CapturePort; // traitメソッド使用のため
use crate::infrastructure::capture::dda::DdaCaptureAdapter;
use crate::infrastructure::mock_process::MockProcessAdapter;
use crate::infrastructure::mock_comm::MockCommAdapter;
use crate::application::pipeline::{PipelineRunner, PipelineConfig};
use crate::application::recovery::{RecoveryState, RecoveryStrategy};
use crate::logging::init_logging;
use std::path::PathBuf;
use std::time::Duration;

// #![windows_subsystem = "windows"] // ← これでコンソール非表示（GUIサブシステム）
fn main() {
    // ログシステムの初期化（非同期ファイル出力）
    // WindowsGUIサブシステムではコンソールが使えないため、ファイル出力必須
    let log_dir = PathBuf::from("logs");
    let _guard = init_logging("info", false, Some(log_dir));
    // 注意: _guardはmain終了まで保持する必要がある（Dropでログスレッドが終了）

    tracing::info!("RoyaleWithCheese starting...");

    // 初期化処理を実行
    match run() {
        Ok(_) => {
            tracing::info!("RoyaleWithCheese terminated gracefully.");
        }
        Err(e) => {
            tracing::error!("Fatal error: {:?}", e);
            std::process::exit(1);
        }
    }
}

/// アプリケーションのメイン処理
fn run() -> Result<(), Box<dyn std::error::Error>> {
    // 設定ファイルの読み込み（存在しない場合はデフォルト設定を使用）
    let config = match AppConfig::from_file("config.toml") {
        Ok(config) => {
            tracing::info!("Loaded configuration from config.toml");
            config
        }
        Err(e) => {
            tracing::warn!("Failed to load config.toml: {:?}, using defaults", e);
            AppConfig::default()
        }
    };

    // 設定の検証
    config.validate()?;

    tracing::info!("Configuration validated successfully");
    tracing::info!("Capture: timeout={}ms, monitor={}", 
        config.capture.timeout_ms, 
        config.capture.monitor_index
    );
    tracing::info!("Process: mode={}, ROI={}x{} at ({},{})", 
        config.process.mode,
        config.process.roi.width,
        config.process.roi.height,
        config.process.roi.x,
        config.process.roi.y
    );

    // DDAキャプチャアダプタの初期化
    tracing::info!("Initializing DDA capture adapter...");
    let capture = DdaCaptureAdapter::new(
        0, // adapter_idx
        config.capture.monitor_index as usize,
        config.capture.timeout_ms as u32,
    )?;
    
    let device_info = capture.device_info();
    tracing::info!("DDA initialized: {}x{} @ {}Hz - {}",
        device_info.width,
        device_info.height,
        device_info.refresh_rate,
        device_info.name
    );

    // モック処理アダプタの初期化（実際の処理は未実装）
    tracing::info!("Initializing mock process adapter...");
    let process = MockProcessAdapter::new();

    // モック通信アダプタの初期化（実際のHIDは未実装）
    tracing::info!("Initializing mock communication adapter...");
    let comm = MockCommAdapter::new();

    // 再初期化戦略の設定
    let recovery_strategy = RecoveryStrategy {
        consecutive_timeout_threshold: config.capture.max_consecutive_timeouts,
        initial_backoff: config.capture.reinit_initial_delay(),
        max_backoff: config.capture.reinit_max_delay(),
        max_cumulative_failure: Duration::from_secs(60),
    };
    let recovery = RecoveryState::new(recovery_strategy);

    // パイプライン設定
    let pipeline_config = PipelineConfig {
        stats_interval: Duration::from_secs(config.pipeline.stats_interval_sec),
        capture_timeout: config.capture.timeout(),
        enable_dirty_rect_optimization: config.pipeline.enable_dirty_rect_optimization,
    };

    // ROIとHSVレンジの変換
    let roi = config.process.roi.into();
    let hsv_range = config.process.hsv_range.into();

    tracing::info!("Starting pipeline with 4-thread architecture...");
    tracing::info!("Threads: Capture -> Process -> HID -> Stats/UI");

    // パイプラインの起動（ブロッキング）
    let runner = PipelineRunner::new(
        capture,
        process,
        comm,
        pipeline_config,
        recovery,
        roi,
        hsv_range,
    );

    runner.run()?;

    Ok(())
}
