//! パイプライン制御モジュール
//!
//! Capture / Process / HID / Stats の4スレッド構成でパイプラインを制御します。
//! HID送信を統計処理から分離し、低レイテンシを実現します。

use crate::domain::{
    error::DomainResult,
    ports::{CapturePort, CommPort, InputPort, ProcessPort, VirtualKey},
    types::{DetectionResult, Frame, Roi, HsvRange},
};
use crate::application::{
    recovery::RecoveryState, 
    stats::{StatKind, StatsCollector},
    runtime_state::RuntimeState,
    input_detector::KeyPressDetector,
};
use crossbeam_channel::{bounded, unbounded, Sender, Receiver, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(feature = "opencv-debug-display")]
use opencv::{highgui, imgproc, core::{Mat, Point, Scalar}};

/// HIDアクティベーション条件（HIDスレッド内で状態管理）
#[derive(Debug, Clone)]
pub struct ActivationConditions {
    /// ROI中心からの最大距離（ピクセル、2乗で比較するため平方根計算を避ける）
    max_distance_squared: f32,
    /// アクティブウィンドウの持続時間（この時間内であればHID送信を許可）
    active_window_duration: Duration,
}

impl ActivationConditions {
    pub fn new(
        max_distance: f32,
        active_window_duration: Duration,
    ) -> Self {
        Self {
            max_distance_squared: max_distance * max_distance, // 平方根計算を避けるため2乗で保持
            active_window_duration,
        }
    }
}

/// HIDアクティベーション状態（HIDスレッド内で管理）
#[derive(Debug)]
struct ActivationState {
    /// 最後にアクティブ条件を満たした時刻
    last_activation_time: Option<Instant>,
}

impl ActivationState {
    fn new() -> Self {
        Self {
            last_activation_time: None,
        }
    }
    
    /// HID送信が許可されるか判定（低レイテンシ最優先）
    /// 
    /// # アクティベーションロジック
    /// 1. システム無効または検出なしの場合は即座にFalse
    /// 2. マウス左クリック押下 OR ROI中心からの距離が閾値以下の場合、アクティブ時刻を更新
    /// 3. 最後にアクティブになってから0.5秒以内であればTrue
    #[inline]
    fn should_activate(
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
        let within_distance = distance_squared <= conditions.max_distance_squared;
        
        // マウス左クリック押下 OR 距離条件を満たす場合、アクティブ時刻を更新
        if mouse_left_pressed || within_distance {
            self.last_activation_time = Some(Instant::now());
        }
        
        // 4. アクティブウィンドウ内かチェック
        if let Some(last_time) = self.last_activation_time {
            if last_time.elapsed() < conditions.active_window_duration {
                return true;
            }
        }
        
        false
    }
}

/// パイプライン設定
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PipelineConfig {
    /// 統計出力間隔
    pub stats_interval: Duration,
    /// DirtyRect最適化を有効化（未実装）
    pub enable_dirty_rect_optimization: bool,
    /// HID送信間隔（新しい値がない場合も直前の値を送信）
    pub hid_send_interval: Duration,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            stats_interval: Duration::from_secs(10),
            enable_dirty_rect_optimization: true,
            hid_send_interval: Duration::from_millis(8),  // 約144Hz
        }
    }
}

/// フレームとタイムスタンプのペア
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TimestampedFrame {
    pub frame: Frame,
    pub captured_at: Instant,
}

/// 検出結果とタイムスタンプのペア
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TimestampedDetection {
    pub result: DetectionResult,
    pub captured_at: Instant,
    pub processed_at: Instant,
}

/// 統計データ（Stats/UIスレッドへ送信用）
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct StatData {
    pub captured_at: Instant,
    pub processed_at: Instant,
    pub hid_sent_at: Instant,
}

/// パイプライン実行コンテキスト
#[allow(dead_code)]
pub struct PipelineRunner<C, P, H, I>
where
    C: CapturePort,
    P: ProcessPort,
    H: CommPort,
    I: InputPort,
{
    capture: Arc<Mutex<C>>,
    process: Arc<Mutex<P>>,
    comm: Arc<Mutex<H>>,
    input: Arc<I>,
    config: PipelineConfig,
    recovery: RecoveryState,
    stats: StatsCollector,
    runtime_state: RuntimeState,
    activation_conditions: ActivationConditions,
    
    roi: Roi,
    hsv_range: HsvRange,
    coordinate_transform: crate::domain::CoordinateTransformConfig,
}

#[allow(dead_code)]
impl<C, P, H, I> PipelineRunner<C, P, H, I>
where
    C: CapturePort + Send + Sync + 'static,
    P: ProcessPort + Send + Sync + 'static,
    H: CommPort + Send + Sync + 'static,
    I: InputPort + Send + Sync + 'static,
{
    /// 新しいPipelineRunnerを作成
    pub fn new(
        capture: C,
        process: P,
        comm: H,
        input: I,
        config: PipelineConfig,
        recovery: RecoveryState,
        roi: Roi,
        hsv_range: HsvRange,
        coordinate_transform: crate::domain::CoordinateTransformConfig,
        activation_conditions: ActivationConditions,
    ) -> Self {
        Self {
            capture: Arc::new(Mutex::new(capture)),
            process: Arc::new(Mutex::new(process)),
            comm: Arc::new(Mutex::new(comm)),
            input: Arc::new(input),
            stats: StatsCollector::new(config.stats_interval),
            runtime_state: RuntimeState::new(),
            activation_conditions,
            config,
            recovery,
            roi,
            hsv_range,
            coordinate_transform,
        }
    }
    
    /// RuntimeStateを取得（テスト用）
    #[cfg(test)]
    pub fn runtime_state(&self) -> &RuntimeState {
        &self.runtime_state
    }

    /// パイプラインを起動（ブロッキング）
    ///
    /// # Returns
    /// エラーが発生した場合のみ戻る
    pub fn run(mut self) -> DomainResult<()> {
        let (capture_tx, capture_rx) = bounded::<TimestampedFrame>(1);
        let (process_tx, process_rx) = bounded::<TimestampedDetection>(1);
        let (stats_tx, stats_rx) = unbounded::<StatData>();

        // Capture Thread
        let capture_handle = {
            let capture = Arc::clone(&self.capture);
            let tx = capture_tx.clone();
            let roi = self.roi.clone();
            std::thread::spawn(move || {
                Self::capture_thread(capture, tx, roi);
            })
        };

        // Process Thread
        let process_handle = {
            let process = Arc::clone(&self.process);
            let roi = self.roi.clone();
            let hsv_range = self.hsv_range.clone();
            let rx = capture_rx;
            let tx = process_tx;
            let stats_tx = stats_tx.clone();
            std::thread::spawn(move || {
                Self::process_thread(process, rx, tx, roi, hsv_range, stats_tx);
            })
        };

        // HID Thread
        let hid_handle = {
            let comm = Arc::clone(&self.comm);
            let rx = process_rx;
            let roi = self.roi.clone();
            let coordinate_transform = self.coordinate_transform.clone();
            let stats_tx = stats_tx.clone();
            let hid_send_interval = self.config.hid_send_interval;
            let runtime_state = self.runtime_state.clone();
            let activation_conditions = self.activation_conditions.clone();
            std::thread::spawn(move || {
                Self::hid_thread(comm, rx, roi, coordinate_transform, stats_tx, hid_send_interval, runtime_state, activation_conditions);
            })
        };

        // Stats/UI Thread（メインスレッドで実行）
        Self::stats_thread(
            stats_rx,
            &mut self.stats,
            &mut self.recovery,
            &self.runtime_state,
            &*self.input,
        );

        // スレッドの終了を待つ
        let _ = capture_handle.join();
        let _ = process_handle.join();
        let _ = hid_handle.join();

        Ok(())
    }

    /// Captureスレッドのメインループ
    fn capture_thread(capture: Arc<Mutex<C>>, tx: Sender<TimestampedFrame>, roi: Roi) {
        tracing::info!("Capture thread started with ROI: {}x{} at ({}, {})", 
            roi.width, roi.height, roi.x, roi.y);
        
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
                        if frame_count % 144 == 0 { // 144フレーム（約1秒@144Hz）に1回ログ出力
                            tracing::debug!("Frame captured: {}x{} (count: {})", frame.width, frame.height, frame_count);
                        }
                    }
                    
                    let timestamped = TimestampedFrame { frame, captured_at };
                    Self::send_latest_only(&tx, timestamped);
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
    fn process_thread(
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
        
        loop {
            match rx.recv() {
                Ok(timestamped) => {
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
                                if process_count % 144 == 0 { // 144フレーム（約1秒@144Hz）に1回ログ出力
                                    let latency = processed_at.duration_since(timestamped.captured_at);
                                    tracing::debug!("Frame processed: detected={}, latency={:?}ms, count={}", 
                                        detection_result.detected, latency.as_millis(), process_count);
                                }
                                // println!("x: {}, y: {}, coverage: {}", 
                                //     detection_result.center_x, detection_result.center_y, detection_result.coverage);
                            }
                            
                            Self::send_latest_only(&tx, detection);
                        }
                        Err(e) => {
                            #[cfg(debug_assertions)]
                            tracing::error!("Process error: {:?}", e);
                            #[cfg(not(debug_assertions))]
                            let _ = e;
                        }
                    }
                }
                Err(_) => {
                    // Channel closed
                    break;
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
    fn hid_thread(
        comm: Arc<Mutex<H>>,
        rx: Receiver<TimestampedDetection>,
        roi: Roi,
        coordinate_transform: crate::domain::CoordinateTransformConfig,
        stats_tx: Sender<StatData>,
        hid_send_interval: Duration,
        runtime_state: RuntimeState,
        activation_conditions: ActivationConditions,
    ) {
        tracing::info!("HID thread started with send interval: {:?}", hid_send_interval);
        tracing::info!(
            "Activation conditions: max_distance={:.1}px, active_window={:?}",
            activation_conditions.max_distance_squared.sqrt(),
            activation_conditions.active_window_duration
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
                            captured_at: Instant::now(),  // 現在時刻を使用
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
                // 注意: should_activateは&mut selfを取るため、状態を更新する
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
                            tracing::error!("HID send error (consecutive: {}): {:?}", consecutive_errors, e);
                            #[cfg(not(debug_assertions))]
                            let _ = e;
                            
                            // 再接続を試みる
                            if consecutive_errors <= MAX_RETRY {
                                // 指数バックオフの計算
                                let backoff_ms = (INITIAL_BACKOFF_MS * 2u64.pow(consecutive_errors - 1))
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
    fn stats_thread(
        stats_rx: Receiver<StatData>,
        stats: &mut StatsCollector,
        _recovery: &mut RecoveryState,
        runtime_state: &RuntimeState,
        input: &dyn InputPort,
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
                #[cfg(debug_assertions)]
                tracing::info!("System {}", if new_state { "ENABLED" } else { "DISABLED" });
            }
            
            // マウスボタン状態を更新（毎ポーリング）
            let input_state = input.poll_input_state();
            runtime_state.set_mouse_buttons(input_state.mouse_left, input_state.mouse_right);
            
            // デバッグウィンドウを更新（opencv-debug-display featureが有効な場合のみ）
            #[cfg(feature = "opencv-debug-display")]
            {
                if let Err(e) = Self::update_runtime_state_window(runtime_state) {
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
    fn update_runtime_state_window(runtime_state: &RuntimeState) -> DomainResult<()> {
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
        ).map_err(|e| DomainError::Process(format!("Failed to create runtime state window: {:?}", e)))?;

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
        ).map_err(|e| DomainError::Process(format!("Failed to draw text: {:?}", e)))?;
        
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
        ).map_err(|e| DomainError::Process(format!("Failed to draw text: {:?}", e)))?;
        
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
        ).map_err(|e| DomainError::Process(format!("Failed to draw text: {:?}", e)))?;
        
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
        ).map_err(|e| DomainError::Process(format!("Failed to draw text: {:?}", e)))?;

        // ウィンドウに表示
        highgui::imshow("Debug: Runtime State", &info_img)
            .map_err(|e| DomainError::Process(format!("Failed to show runtime state window: {:?}", e)))?;
        
        // キー入力を待つ（1ms、ノンブロッキング）
        let _ = highgui::wait_key(1);

        Ok(())
    }

    /// 最新のみ上書きポリシーで送信
    /// 
    /// bounded(1)キューを使用し、キューが満杯の場合は古いデータを破棄。
    /// これにより常に最新のデータのみが処理される（低レイテンシ最優先）。
    fn send_latest_only<T>(tx: &Sender<T>, value: T) {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        error::DomainError,
        ports::DeviceInfo,
        types::{DetectionResult, Frame, Roi, HsvRange, ProcessorBackend},
    };

    // モック実装
    struct MockCapture;
    impl CapturePort for MockCapture {
        fn capture_frame_with_roi(&mut self, roi: &Roi) -> DomainResult<Option<Frame>> {
            Ok(Some(Frame {
                data: vec![0u8; (roi.width * roi.height * 4) as usize],
                width: roi.width,
                height: roi.height,
                timestamp: std::time::Instant::now(),
                dirty_rects: vec![],
            }))
        }

        fn reinitialize(&mut self) -> DomainResult<()> {
            Ok(())
        }

        fn device_info(&self) -> DeviceInfo {
            DeviceInfo {
                width: 1920,
                height: 1080,
                refresh_rate: 144,
                name: "Mock Display".to_string(),
            }
        }
    }

    struct MockProcess;
    impl ProcessPort for MockProcess {
        fn process_frame(
            &mut self,
            _frame: &Frame,
            _roi: &Roi,
            _hsv_range: &HsvRange,
        ) -> DomainResult<DetectionResult> {
            Ok(DetectionResult {
                timestamp: std::time::Instant::now(),
                detected: true,
                center_x: 960.0,
                center_y: 540.0,
                coverage: 1000,
                bounding_box: None,
            })
        }

        fn backend(&self) -> ProcessorBackend {
            ProcessorBackend::Cpu
        }
    }

    struct MockComm;
    impl CommPort for MockComm {
        fn send(&mut self, _data: &[u8]) -> DomainResult<()> {
            Ok(())
        }

        fn is_connected(&self) -> bool {
            true
        }

        fn reconnect(&mut self) -> DomainResult<()> {
            Ok(())
        }
    }

    struct MockInput;
    impl InputPort for MockInput {
        fn is_key_pressed(&self, _key: VirtualKey) -> bool {
            false
        }

        fn poll_input_state(&self) -> crate::domain::ports::InputState {
            crate::domain::ports::InputState {
                mouse_left: false,
                mouse_right: false,
            }
        }
    }

    #[allow(dead_code)]
    struct FailingCapture;
    impl CapturePort for FailingCapture {
        fn capture_frame_with_roi(&mut self, _roi: &Roi) -> DomainResult<Option<Frame>> {
            Err(DomainError::Timeout("Test timeout".to_string()))
        }

        fn reinitialize(&mut self) -> DomainResult<()> {
            Err(DomainError::Capture("Reinit failed".to_string()))
        }

        fn device_info(&self) -> DeviceInfo {
            DeviceInfo {
                width: 1920,
                height: 1080,
                refresh_rate: 144,
                name: "Failing Capture".to_string(),
            }
        }
    }

    #[test]
    fn test_pipeline_config_default() {
        let config = PipelineConfig::default();
        assert_eq!(config.stats_interval, Duration::from_secs(10));
        assert!(config.enable_dirty_rect_optimization);
        assert_eq!(config.hid_send_interval, Duration::from_millis(8));
    }

    #[test]
    fn test_timestamped_frame_creation() {
        let frame = Frame {
            data: vec![0u8; 100],
            width: 10,
            height: 10,
            timestamp: Instant::now(),
            dirty_rects: vec![],
        };

        let captured_at = Instant::now();
        let timestamped = TimestampedFrame {
            frame: frame.clone(),
            captured_at,
        };

        assert_eq!(timestamped.frame.width, 10);
        assert_eq!(timestamped.frame.height, 10);
    }

    #[test]
    fn test_send_latest_only() {
        let (tx, rx) = bounded::<i32>(1);

        // 最初の送信は成功
        PipelineRunner::<MockCapture, MockProcess, MockComm, MockInput>::send_latest_only(&tx, 1);
        assert_eq!(rx.try_recv().unwrap(), 1);

        // キューを満たす
        tx.try_send(2).unwrap();

        // キューが満杯の状態で新しい値を送信（満杯なので無視される）
        PipelineRunner::<MockCapture, MockProcess, MockComm, MockInput>::send_latest_only(&tx, 3);

        // キューには古い値（2）が残っている
        let value = rx.try_recv().unwrap();
        assert_eq!(value, 2);
    }

    #[test]
    fn test_capture_with_roi_abstraction() {
        // CapturePort traitの抽象化を通じてcapture_frame_with_roi()を使用するテスト
        let mut capture = MockCapture;

        // テスト1: 小さいROI (400x300)
        let roi_small = Roi::new(100, 100, 400, 300);
        let frame = capture.capture_frame_with_roi(&roi_small)
            .expect("Capture should succeed")
            .expect("Frame should be present");

        assert_eq!(frame.width, roi_small.width, "Frame width should match ROI");
        assert_eq!(frame.height, roi_small.height, "Frame height should match ROI");
        assert_eq!(frame.data.len(), (roi_small.width * roi_small.height * 4) as usize, "Data size should match ROI");

        // テスト2: 設計目標サイズ (800x600)
        let roi_medium = Roi::new(560, 240, 800, 600);
        let frame = capture.capture_frame_with_roi(&roi_medium)
            .expect("Capture should succeed")
            .expect("Frame should be present");

        assert_eq!(frame.width, roi_medium.width, "Frame width should match ROI");
        assert_eq!(frame.height, roi_medium.height, "Frame height should match ROI");
        assert_eq!(frame.data.len(), (roi_medium.width * roi_medium.height * 4) as usize, "Data size should match ROI");

        // テスト3: フルスクリーンROI
        let roi_full = Roi::new(0, 0, 1920, 1080);
        let frame = capture.capture_frame_with_roi(&roi_full)
            .expect("Capture should succeed")
            .expect("Frame should be present");

        assert_eq!(frame.width, roi_full.width, "Frame width should match full screen ROI");
        assert_eq!(frame.height, roi_full.height, "Frame height should match full screen ROI");

        // テスト4: capture_frame()のデフォルト実装がフルスクリーンROIを使用することを確認
        let device_info = capture.device_info();
        let frame_default = capture.capture_frame()
            .expect("Capture should succeed")
            .expect("Frame should be present");

        assert_eq!(frame_default.width, device_info.width, "Default capture should return full screen width");
        assert_eq!(frame_default.height, device_info.height, "Default capture should return full screen height");
    }
}
