//! スレッド実装の詳細
//!
//! Capture / Process / HID / Stats の4スレッドの実装を含みます。
//! pipeline.rsから分離され、低レイテンシのスレッド間通信を実現します。

use crate::domain::{
    ports::{CapturePort, CommPort, InputPort, ProcessPort, VirtualKey},
    types::{DetectionResult, Frame, HsvRange, Roi},
};

use crate::application::pipeline::ActivationConditions;
use crate::application::{
    input_detector::KeyPressDetector,
    recovery::RecoveryState,
    runtime_state::RuntimeState,
    stats::{StatKind, StatsCollector},
};
#[cfg(feature = "opencv-debug-display")]
use crate::domain::error::DomainResult;
use crossbeam_channel::{Receiver, Sender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(feature = "opencv-debug-display")]
use opencv::{
    core::{Mat, Point, Scalar},
    highgui, imgproc,
};

/// フレームとタイムスタンプのペア
#[derive(Debug, Clone)]
pub(crate) struct TimestampedFrame {
    pub frame: Frame,
    pub captured_at: Instant,
}

/// 検出結果とタイムスタンプのペア
#[derive(Debug, Clone)]
pub(crate) struct TimestampedDetection {
    pub result: DetectionResult,
    pub captured_at: Instant,
    pub processed_at: Instant,
}

/// 統計データ（Stats/UIスレッドへ送信用）
#[derive(Debug, Clone)]
pub(crate) struct StatData {
    pub captured_at: Instant,
    pub processed_at: Instant,
    pub hid_sent_at: Instant,
}

/// HIDアクティベーション状態（HIDスレッド内で管理）
#[derive(Debug)]
pub(crate) struct ActivationState {
    /// 最後にアクティブ条件を満たした時刻
    last_activation_time: Option<Instant>,
}

impl ActivationState {
    pub(crate) fn new() -> Self {
        Self {
            last_activation_time: None,
        }
    }

    /// HID送信が許可されるか判定（低レイテンシ最優先）
    ///
    /// # アクティベーションロジック
    /// 1. システム無効または検出なしの場合は即座にFalse
    /// 2. マウス左クリック押下 OR ROI中心からの距離が閾値以下の場合、アクティブ時刻を更新
    /// 3. 最後にアクティブになってから0.5秒未満であればTrue
    #[inline]
    pub(crate) fn should_activate(
        &mut self,
        runtime_state: &RuntimeState,
        detection: &DetectionResult,
        roi: &Roi,
        conditions: &ActivationConditions,
    ) -> bool {
        // 1. システムが有効か（最も頻繁に失敗する条件を先に評価）
        if !runtime_state.is_enabled() {
            return false;
        }

        // 2. 検出されているか
        if !detection.detected {
            return false;
        }

        // 3. アクティブ条件のチェックと時刻更新
        let mouse_left_pressed = runtime_state.is_mouse_left_pressed();

        // ROI中心からの距離を計算（平方根計算を避けるため2乗で比較）
        let roi_center_x = roi.width as f32 / 2.0;
        let roi_center_y = roi.height as f32 / 2.0;
        let dx = detection.center_x - roi_center_x;
        let dy = detection.center_y - roi_center_y;
        let distance_squared = dx * dx + dy * dy;
        let within_distance = distance_squared <= conditions.max_distance_squared();

        // マウス左クリック押下 OR 距離条件を満たす場合、アクティブ時刻を更新
        if mouse_left_pressed || within_distance {
            self.last_activation_time = Some(Instant::now());
        }

        // 4. アクティブウィンドウ内かチェック
        if let Some(last_time) = self.last_activation_time {
            if last_time.elapsed() < conditions.active_window_duration() {
                return true;
            }
        }

        false
    }
}

/// Captureスレッドのメインループ
pub(crate) fn capture_thread<C: CapturePort>(
    capture: Arc<Mutex<C>>,
    tx: Sender<TimestampedFrame>,
    roi: Roi,
) {
    tracing::info!(
        "Capture thread started with ROI: {}x{} at ({}, {})",
        roi.width,
        roi.height,
        roi.x,
        roi.y
    );

    #[cfg(debug_assertions)]
    let mut frame_count = 0u64;

    loop {
        let captured_at = Instant::now();

        let result = {
            let mut guard = capture.lock().unwrap();
            guard.capture_frame_with_roi(&roi)
        };

        match result {
            Ok(Some(frame)) => {
                #[cfg(debug_assertions)]
                {
                    frame_count += 1;
                    if frame_count.is_multiple_of(144) {
                        // 144フレーム（約1秒@144Hz）に1回ログ出力
                        tracing::debug!(
                            "Frame captured: {}x{} (count: {})",
                            frame.width,
                            frame.height,
                            frame_count
                        );
                    }
                }

                let timestamped = TimestampedFrame { frame, captured_at };
                send_latest_only(&tx, timestamped);
            }
            Ok(None) => {
                // Timeout - no new frame
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(e) => {
                #[cfg(debug_assertions)]
                tracing::warn!("Capture error: {:?}", e);
                #[cfg(not(debug_assertions))]
                let _ = e;

                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

/// Processスレッドのメインループ
pub(crate) fn process_thread<P: ProcessPort>(
    process: Arc<Mutex<P>>,
    rx: Receiver<TimestampedFrame>,
    tx: Sender<TimestampedDetection>,
    roi: Roi,
    hsv_range: HsvRange,
    _stats_tx: Sender<StatData>,
) {
    tracing::info!("Process thread started");

    #[cfg(debug_assertions)]
    let mut process_count = 0u64;

    while let Ok(timestamped) = rx.recv() {
        let result = {
            let mut guard = process.lock().unwrap();
            guard.process_frame(&timestamped.frame, &roi, &hsv_range)
        };

        match result {
            Ok(detection_result) => {
                let processed_at = Instant::now();
                let detection = TimestampedDetection {
                    result: detection_result,
                    captured_at: timestamped.captured_at,
                    processed_at,
                };

                #[cfg(debug_assertions)]
                {
                    process_count += 1;
                    if process_count.is_multiple_of(144) {
                        // 144フレーム（約1秒@144Hz）に1回ログ出力
                        let latency = processed_at.duration_since(timestamped.captured_at);
                        tracing::debug!(
                            "Frame processed: detected={}, latency={:?}ms, count={}",
                            detection_result.detected,
                            latency.as_millis(),
                            process_count
                        );
                    }
                }

                send_latest_only(&tx, detection);
            }
            Err(e) => {
                #[cfg(debug_assertions)]
                tracing::error!("Process error: {:?}", e);
                #[cfg(not(debug_assertions))]
                let _ = e;
            }
        }
    }
}

/// HIDスレッド（低レイテンシ送信専用）
///
/// # 送信戦略
/// - 新しい検出結果を受信したら、アクティベーション条件を満たす場合に送信
/// - 新しい値がない場合は、hid_send_interval間隔で直前の値を送信し続ける
///
/// # アクティベーション条件
/// - システムが有効（runtime_state.is_enabled()）
/// - マウス左クリック押下 OR ROI中心から指定距離以内に検出対象が存在
/// - 上記条件を満たしてからactive_window_duration（0.5秒）以内
///
/// # 再接続戦略
/// - 送信エラー時、指数バックオフで再接続を試みる
/// - 初回: 100ms, 2回目: 200ms, 3回目: 400ms, ...最大10秒
/// - 最大リトライ回数: 10回
#[allow(clippy::too_many_arguments)]
pub(crate) fn hid_thread<H: CommPort>(
    comm: Arc<Mutex<H>>,
    rx: Receiver<TimestampedDetection>,
    roi: Roi,
    coordinate_transform: crate::domain::CoordinateTransformConfig,
    stats_tx: Sender<StatData>,
    hid_send_interval: Duration,
    runtime_state: RuntimeState,
    activation_conditions: ActivationConditions,
) {
    tracing::info!(
        "HID thread started with send interval: {:?}",
        hid_send_interval
    );
    tracing::info!(
        "Activation conditions: max_distance={:.1}px, active_window={:?}",
        activation_conditions.max_distance_squared().sqrt(),
        activation_conditions.active_window_duration()
    );

    let mut consecutive_errors = 0u32;
    let mut last_reconnect_attempt = None::<Instant>;
    const MAX_RETRY: u32 = 10;
    const INITIAL_BACKOFF_MS: u64 = 100;
    const MAX_BACKOFF_MS: u64 = 10_000;

    // アクティベーション状態管理
    let mut activation_state = ActivationState::new();

    // 直前の検出結果を保持（初期値: 検出なし）
    let mut last_detection: Option<DetectionResult> = None;

    loop {
        // タイムアウト付きでrecv（新しい値がない場合は直前の値を使用）
        let detection = match rx.recv_timeout(hid_send_interval) {
            Ok(new_detection) => {
                // 新しい検出結果を受信
                last_detection = Some(new_detection.result);
                Some(new_detection)
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                // タイムアウト: 直前の値を使用
                if let Some(last_result) = last_detection {
                    Some(TimestampedDetection {
                        result: last_result,
                        captured_at: Instant::now(), // 現在時刻を使用
                        processed_at: Instant::now(),
                    })
                } else {
                    // まだ一度も検出結果を受信していない場合はスキップ
                    continue;
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                // Channel closed
                break;
            }
        };

        if let Some(detection) = detection {
            let hid_sent_at: Instant;

            // アクティベーション条件チェック（低レイテンシ最優先）
            let should_send = activation_state.should_activate(
                &runtime_state,
                &detection.result,
                &roi,
                &activation_conditions,
            );

            if !should_send {
                // アクティベーション条件を満たさない場合はHID送信をスキップ（統計は記録）
                hid_sent_at = Instant::now();

                #[cfg(debug_assertions)]
                {
                    // システム無効の場合のみログ出力（レートリミット付き）
                    if !runtime_state.is_enabled() && runtime_state.should_log_disabled_status() {
                        tracing::debug!("System disabled - skipping HID transmission");
                    }
                }
            } else {
                // アクティベーション条件を満たした場合にHID送信を実行
                // 2段階変換: DetectionResult → TransformedCoordinates → HIDレポート
                let transformed = crate::domain::ports::apply_coordinate_transform(
                    &detection.result,
                    &roi,
                    &coordinate_transform,
                );
                let hid_report = crate::domain::ports::coordinates_to_hid_report(&transformed);
                let send_result = {
                    let mut guard = comm.lock().unwrap();
                    guard.send(&hid_report)
                };

                hid_sent_at = Instant::now();

                match send_result {
                    Ok(_) => {
                        // エラーカウントをリセット
                        if consecutive_errors > 0 {
                            tracing::info!("HID communication recovered");
                            consecutive_errors = 0;
                        }
                    }
                    Err(e) => {
                        consecutive_errors += 1;

                        #[cfg(debug_assertions)]
                        tracing::error!(
                            "HID send error (consecutive: {}): {:?}",
                            consecutive_errors,
                            e
                        );
                        #[cfg(not(debug_assertions))]
                        let _ = e;

                        // 再接続を試みる
                        if consecutive_errors <= MAX_RETRY {
                            // 指数バックオフの計算
                            let backoff_ms = (INITIAL_BACKOFF_MS
                                * 2u64.pow(consecutive_errors - 1))
                            .min(MAX_BACKOFF_MS);

                            // レート制限: 前回の再接続試行から十分な時間が経過しているか確認
                            let should_retry = if let Some(last_attempt) = last_reconnect_attempt {
                                last_attempt.elapsed() >= Duration::from_millis(backoff_ms)
                            } else {
                                true
                            };

                            if should_retry {
                                tracing::info!(
                                    "Attempting to reconnect HID device (retry {}/{}, backoff: {}ms)",
                                    consecutive_errors,
                                    MAX_RETRY,
                                    backoff_ms
                                );

                                last_reconnect_attempt = Some(Instant::now());

                                let reconnect_result = {
                                    let mut guard = comm.lock().unwrap();
                                    guard.reconnect()
                                };

                                match reconnect_result {
                                    Ok(_) => {
                                        tracing::info!("HID device reconnected successfully");
                                        consecutive_errors = 0;
                                    }
                                    Err(reconnect_err) => {
                                        tracing::warn!("Reconnect failed: {:?}", reconnect_err);
                                        // 次のフレームで再試行（バックオフを適用）
                                        std::thread::sleep(Duration::from_millis(backoff_ms));
                                    }
                                }
                            }
                        } else {
                            tracing::error!(
                                "Max retry count exceeded ({}), giving up on HID communication",
                                MAX_RETRY
                            );
                            // 最大リトライ回数を超えた場合もスレッドは継続
                            // （デバイスが復帰した場合に備えて）
                        }
                    }
                }
            }

            // 統計データをStats/UIスレッドに送信（非ブロッキング）
            // 無効時も統計は記録する（パフォーマンスモニタリングのため）
            let stat_data = StatData {
                captured_at: detection.captured_at,
                processed_at: detection.processed_at,
                hid_sent_at,
            };
            let _ = stats_tx.try_send(stat_data);
        }
    }
}

/// Stats/UIスレッド（統計情報管理とユーザー対話）
pub(crate) fn stats_thread(
    stats_rx: Receiver<StatData>,
    stats: &mut StatsCollector,
    _recovery: &mut RecoveryState,
    runtime_state: &RuntimeState,
    input: &dyn InputPort,
    audio_feedback: Option<&crate::infrastructure::audio_feedback::WindowsAudioFeedback>,
) {
    tracing::info!("Stats/UI thread started");

    let mut insert_detector = KeyPressDetector::new();
    let poll_interval = Duration::from_millis(10); // 入力ポーリング間隔: 10ms (100Hz)

    #[cfg(feature = "opencv-debug-display")]
    {
        // デバッグウィンドウを初期化
        let _ = highgui::named_window("Debug: Runtime State", highgui::WINDOW_AUTOSIZE);
        tracing::info!("Debug: Runtime State window initialized");
    }

    loop {
        // 非ブロッキング受信でタイムアウト付き
        match stats_rx.recv_timeout(poll_interval) {
            Ok(stat_data) => {
                // 統計記録
                stats.record_frame();

                let process_time = stat_data.processed_at.duration_since(stat_data.captured_at);
                let comm_time = stat_data.hid_sent_at.duration_since(stat_data.processed_at);
                let end_to_end = stat_data.hid_sent_at.duration_since(stat_data.captured_at);

                stats.record_duration(StatKind::Process, process_time);
                stats.record_duration(StatKind::Communication, comm_time);
                stats.record_duration(StatKind::EndToEnd, end_to_end);

                // 定期的に統計出力
                if stats.should_report() {
                    stats.report_and_reset();
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                // タイムアウト - 入力チェックを続行
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                // Channel closed
                break;
            }
        }

        // Insertキーの押下検知（エッジ検出）
        if insert_detector.is_key_just_pressed(input, VirtualKey::Insert) {
            let new_state = runtime_state.toggle_enabled();

            // 音声フィードバック再生（非同期、数マイクロ秒で復帰）
            if let Some(audio) = audio_feedback {
                audio.play_toggle_sound(new_state);
            }

            #[cfg(debug_assertions)]
            tracing::info!("System {}", if new_state { "ENABLED" } else { "DISABLED" });
        }

        // マウスボタン状態を更新（毎ポーリング）
        let input_state = input.poll_input_state();
        runtime_state.set_mouse_buttons(input_state.mouse_left, input_state.mouse_right);

        // デバッグウィンドウを更新（opencv-debug-display featureが有効な場合のみ）
        #[cfg(feature = "opencv-debug-display")]
        {
            if let Err(e) = update_runtime_state_window(runtime_state) {
                tracing::warn!("Failed to update runtime state window: {:?}", e);
            }
        }
    }

    #[cfg(feature = "opencv-debug-display")]
    {
        // スレッド終了時にウィンドウを破棄
        let _ = highgui::destroy_window("Debug: Runtime State");
        tracing::info!("Debug: Runtime State window closed");
    }
}

/// RuntimeState情報を表示するデバッグウィンドウを更新
///
/// opencv-debug-display featureが有効な場合のみコンパイルされます。
/// システムの有効/無効状態とマウスボタンの押下状態をリアルタイムで表示します。
#[cfg(feature = "opencv-debug-display")]
pub(crate) fn update_runtime_state_window(runtime_state: &RuntimeState) -> DomainResult<()> {
    use crate::domain::error::DomainError;

    // 固定サイズのウィンドウ
    let window_width = 350;
    let window_height = 200;

    // 黒背景のMatを作成
    let mut info_img = Mat::new_rows_cols_with_default(
        window_height,
        window_width,
        opencv::core::CV_8UC3,
        Scalar::new(0.0, 0.0, 0.0, 0.0),
    )
    .map_err(|e| DomainError::Process(format!("Failed to create runtime state window: {:?}", e)))?;

    let font_scale = 0.7;
    let thickness = 2;
    let white = Scalar::new(255.0, 255.0, 255.0, 0.0);
    let green = Scalar::new(0.0, 255.0, 0.0, 0.0);
    let red = Scalar::new(0.0, 0.0, 255.0, 0.0);
    let yellow = Scalar::new(0.0, 255.0, 255.0, 0.0);

    let mut y = 40;
    let line_height = 35;

    // タイトル
    imgproc::put_text(
        &mut info_img,
        "=== Runtime State ===",
        Point::new(20, y),
        imgproc::FONT_HERSHEY_SIMPLEX,
        font_scale,
        white,
        thickness,
        imgproc::LINE_8,
        false,
    )
    .map_err(|e| DomainError::Process(format!("Failed to draw text: {:?}", e)))?;

    y += line_height;

    // システム有効/無効状態
    let enabled = runtime_state.is_enabled();
    let status_text = if enabled { "ENABLED" } else { "DISABLED" };
    let status_color = if enabled { green } else { red };

    imgproc::put_text(
        &mut info_img,
        &format!("System: {}", status_text),
        Point::new(20, y),
        imgproc::FONT_HERSHEY_SIMPLEX,
        font_scale,
        status_color,
        thickness,
        imgproc::LINE_8,
        false,
    )
    .map_err(|e| DomainError::Process(format!("Failed to draw text: {:?}", e)))?;

    y += line_height;

    // マウス左ボタン状態
    let mouse_left = runtime_state.is_mouse_left_pressed();
    let left_text = if mouse_left { "PRESSED" } else { "Released" };
    let left_color = if mouse_left { yellow } else { white };

    imgproc::put_text(
        &mut info_img,
        &format!("Mouse L: {}", left_text),
        Point::new(20, y),
        imgproc::FONT_HERSHEY_SIMPLEX,
        font_scale,
        left_color,
        thickness,
        imgproc::LINE_8,
        false,
    )
    .map_err(|e| DomainError::Process(format!("Failed to draw text: {:?}", e)))?;

    y += line_height;

    // マウス右ボタン状態
    let mouse_right = runtime_state.is_mouse_right_pressed();
    let right_text = if mouse_right { "PRESSED" } else { "Released" };
    let right_color = if mouse_right { yellow } else { white };

    imgproc::put_text(
        &mut info_img,
        &format!("Mouse R: {}", right_text),
        Point::new(20, y),
        imgproc::FONT_HERSHEY_SIMPLEX,
        font_scale,
        right_color,
        thickness,
        imgproc::LINE_8,
        false,
    )
    .map_err(|e| DomainError::Process(format!("Failed to draw text: {:?}", e)))?;

    // ウィンドウに表示
    highgui::imshow("Debug: Runtime State", &info_img).map_err(|e| {
        DomainError::Process(format!("Failed to show runtime state window: {:?}", e))
    })?;

    // キー入力を待つ（1ms、ノンブロッキング）
    let _ = highgui::wait_key(1);

    Ok(())
}

/// 最新のみ上書きポリシーで送信
///
/// bounded(1)キューを使用し、キューが満杯の場合は古いデータを破棄。
/// これにより常に最新のデータのみが処理される（低レイテンシ最優先）。
pub(crate) fn send_latest_only<T>(tx: &Sender<T>, value: T) {
    match tx.try_send(value) {
        Ok(_) => {}
        Err(TrySendError::Full(_)) => {
            // キューが満杯 - 古いデータは受信側が破棄する
            // Senderからは取り出せないため、単に無視
        }
        Err(TrySendError::Disconnected(_)) => {
            // Channel closed
        }
    }
}
