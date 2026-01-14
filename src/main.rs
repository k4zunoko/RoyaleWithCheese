#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod application;
mod domain;
mod infrastructure;
mod logging;

use crate::application::pipeline::{PipelineConfig, PipelineRunner};
use crate::application::recovery::{RecoveryState, RecoveryStrategy};
use crate::domain::config::AppConfig;
use crate::domain::config::CaptureSource;
use crate::domain::ports::CapturePort; // traitメソッド使用のため
use crate::domain::Roi; // ROI型を使用
use crate::infrastructure::audio_feedback::WindowsAudioFeedback;
use crate::infrastructure::capture::dda::DdaCaptureAdapter;
use crate::infrastructure::capture::spout::SpoutCaptureAdapter;
use crate::infrastructure::capture::wgc::WgcCaptureAdapter;
use crate::infrastructure::color_process::ColorProcessAdapter;
use crate::infrastructure::hid_comm::HidCommAdapter;
use crate::infrastructure::input::WindowsInputAdapter;
use crate::infrastructure::process_selector::ProcessSelector;
use crate::logging::init_logging;
use std::path::PathBuf;
use std::time::Duration;

// #![windows_subsystem = "windows"] // ← これでコンソール非表示（GUIサブシステム）
fn main() {
    // ログシステムの初期化（非同期ファイル出力）
    // WindowsGUIサブシステムではコンソールが使えないため、ファイル出力必須
    let log_dir = PathBuf::from("logs");
    let _guard = init_logging("debug", false, Some(log_dir));
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
    tracing::info!(
        "Capture: source={:?}, timeout={}ms, monitor={}",
        config.capture.source,
        config.capture.timeout_ms,
        config.capture.monitor_index
    );

    // キャプチャアダプタの初期化（設定に基づく選択）
    tracing::info!("Initializing capture adapter: {:?}", config.capture.source);
    match config.capture.source {
        CaptureSource::Dda => {
            tracing::info!("Using Desktop Duplication API (DDA)");
            let capture = DdaCaptureAdapter::new(
                0, // adapter_idx
                config.capture.monitor_index as usize,
                config.capture.timeout_ms as u32,
            )?;
            run_with_capture(capture, config)
        }
        CaptureSource::Spout => {
            tracing::info!("Using Spout DX11 texture receiver");
            if let Some(ref sender_name) = config.capture.spout_sender_name {
                tracing::info!("  Sender name: {}", sender_name);
            } else {
                tracing::info!("  Sender name: auto (first active sender)");
            }
            let capture = SpoutCaptureAdapter::new(config.capture.spout_sender_name.clone())?;
            run_with_capture(capture, config)
        }
        CaptureSource::Wgc => {
            tracing::info!("Using Windows Graphics Capture (WGC) - Low Latency Mode");
            let capture = WgcCaptureAdapter::new(config.capture.monitor_index as usize)?;
            run_with_capture(capture, config)
        }
    }
}

/// 共通のパイプライン起動ロジック（ジェネリック版）
fn run_with_capture<C: CapturePort + Send + Sync + 'static>(
    capture: C,
    config: AppConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let device_info = capture.device_info();
    tracing::info!(
        "Capture initialized: {}x{} @ {}Hz - {}",
        device_info.width,
        device_info.height,
        device_info.refresh_rate,
        device_info.name
    );

    // ROI設定（サイズのみ、位置は毎フレーム動的に中心配置される）
    // width/heightの妥当性のみ検証（初期化時の画面サイズに対して）
    let roi = if device_info.width > 0 && device_info.height > 0 {
        // 画面サイズが既知の場合、ROIサイズの妥当性を検証
        config
            .process
            .roi
            .to_roi_centered(device_info.width, device_info.height)?
    } else {
        // Spout未接続時など、画面サイズが不明な場合はサイズのみ設定
        // 実際の位置は毎フレーム動的に計算される
        tracing::warn!("Capture device size unknown (width={}, height={}), ROI position will be dynamically calculated",
            device_info.width, device_info.height);
        Roi::new(0, 0, config.process.roi.width, config.process.roi.height)
    };
    tracing::info!(
        "Process: mode={}, ROI={}x{} (dynamically centered each frame)",
        config.process.mode,
        roi.width,
        roi.height
    );

    // 処理アダプタの初期化（config.process.modeに基づく）
    tracing::info!(
        "Initializing process adapter with mode: {}",
        config.process.mode
    );
    let process = match config.process.mode.as_str() {
        "fast-color" => {
            tracing::info!(
                "Using fast-color (HSV color detection) mode with {:?} detection method",
                config.process.detection_method
            );
            let adapter = ColorProcessAdapter::new(
                config.process.min_detection_area,
                config.process.detection_method,
            )?;
            ProcessSelector::FastColor(adapter)
        }
        "yolo-ort" => {
            // 将来の実装: YOLO + ONNX Runtime
            // let adapter = YoloProcessAdapter::new(...)?;
            // ProcessSelector::YoloOrt(adapter)
            return Err(format!(
                "Process mode '{}' is not yet implemented. Currently only 'fast-color' is supported. \
                Please set process.mode = \"fast-color\" in config.toml.",
                config.process.mode
            ).into());
        }
        _ => {
            return Err(format!(
                "Unknown process mode: '{}'. Supported modes are 'fast-color' or 'yolo-ort' (not yet implemented). \
                Please check process.mode in config.toml.",
                config.process.mode
            ).into());
        }
    };

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
        enable_dirty_rect_optimization: config.pipeline.enable_dirty_rect_optimization,
        hid_send_interval: Duration::from_millis(config.communication.hid_send_interval_ms),
    };

    // HSVレンジと座標変換の設定
    let hsv_range = config.process.hsv_range.into();
    let coordinate_transform = config.process.coordinate_transform.clone();

    tracing::info!(
        "Coordinate transform: sensitivity={:.2}, clip_limit=({:.1}, {:.1}), dead_zone={:.1}",
        coordinate_transform.sensitivity,
        coordinate_transform.x_clip_limit,
        coordinate_transform.y_clip_limit,
        coordinate_transform.dead_zone
    );

    tracing::info!("Starting pipeline with 4-thread architecture...");
    tracing::info!("Threads: Capture -> Process -> HID -> Stats/UI");

    // HID通信アダプタの初期化
    tracing::info!("Initializing HID communication adapter...");
    let hid_comm = HidCommAdapter::new(
        config.communication.vendor_id,
        config.communication.product_id,
        config.communication.serial_number.clone(),
        config.communication.device_path.clone(),
    )?;
    tracing::info!(
        "HID adapter initialized: VID=0x{:04X}, PID=0x{:04X}",
        config.communication.vendor_id,
        config.communication.product_id
    );

    // 入力アダプタの初期化（Insertキー検知とマウスボタン状態）
    tracing::info!("Initializing input adapter...");
    let input = WindowsInputAdapter::new();

    // アクティベーション条件の設定
    use crate::application::pipeline::ActivationConditions;
    let activation_conditions = ActivationConditions::new(
        config.activation.max_distance_from_center,
        config.activation.active_window(),
    );
    tracing::info!(
        "Activation: max_distance={:.1}px, active_window={}ms",
        config.activation.max_distance_from_center,
        config.activation.active_window_ms
    );

    // 音声フィードバックの初期化
    let audio_feedback = if config.audio_feedback.enabled {
        tracing::info!("Audio feedback enabled");
        tracing::info!("  On sound: {}", config.audio_feedback.on_sound);
        tracing::info!("  Off sound: {}", config.audio_feedback.off_sound);
        Some(WindowsAudioFeedback::new(config.audio_feedback.clone()))
    } else {
        tracing::info!("Audio feedback disabled");
        None
    };

    // パイプラインの起動（ブロッキング）
    let runner = PipelineRunner::new(
        capture,
        process,
        hid_comm,
        input,
        pipeline_config,
        recovery,
        roi,
        hsv_range,
        coordinate_transform,
        activation_conditions,
        audio_feedback,
    );
    runner.run()?;

    Ok(())
}
