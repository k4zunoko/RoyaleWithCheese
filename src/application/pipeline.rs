//! パイプライン制御モジュール
//!
//! Capture / Process / HID / Stats の4スレッド構成でパイプラインを制御します。
//! HID送信を統計処理から分離し、低レイテンシを実現します。

use crate::domain::{
    error::DomainResult,
    ports::{CapturePort, CommPort, ProcessPort},
    types::{DetectionResult, Frame, Roi, HsvRange},
};
use crate::application::{recovery::RecoveryState, stats::{StatKind, StatsCollector}};
use crossbeam_channel::{bounded, unbounded, Sender, Receiver, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// パイプライン設定
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PipelineConfig {
    /// 統計出力間隔
    pub stats_interval: Duration,
    /// キャプチャタイムアウト
    pub capture_timeout: Duration,
    /// DirtyRect最適化を有効化
    pub enable_dirty_rect_optimization: bool,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            stats_interval: Duration::from_secs(10),
            capture_timeout: Duration::from_millis(8),
            enable_dirty_rect_optimization: true,
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
pub struct PipelineRunner<C, P, H>
where
    C: CapturePort,
    P: ProcessPort,
    H: CommPort,
{
    capture: Arc<Mutex<C>>,
    process: Arc<Mutex<P>>,
    comm: Arc<Mutex<H>>,
    config: PipelineConfig,
    recovery: RecoveryState,
    stats: StatsCollector,
    // TODO: 設定から読み込む
    roi: Roi,
    hsv_range: HsvRange,
}

#[allow(dead_code)]
impl<C, P, H> PipelineRunner<C, P, H>
where
    C: CapturePort + Send + Sync + 'static,
    P: ProcessPort + Send + Sync + 'static,
    H: CommPort + Send + Sync + 'static,
{
    /// 新しいPipelineRunnerを作成
    pub fn new(
        capture: C,
        process: P,
        comm: H,
        config: PipelineConfig,
        recovery: RecoveryState,
        roi: Roi,
        hsv_range: HsvRange,
    ) -> Self {
        Self {
            capture: Arc::new(Mutex::new(capture)),
            process: Arc::new(Mutex::new(process)),
            comm: Arc::new(Mutex::new(comm)),
            stats: StatsCollector::new(config.stats_interval),
            config,
            recovery,
            roi,
            hsv_range,
        }
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
            let stats_tx = stats_tx.clone();
            std::thread::spawn(move || {
                Self::hid_thread(comm, rx, stats_tx);
            })
        };

        // Stats/UI Thread（メインスレッドで実行）
        Self::stats_thread(
            stats_rx,
            &mut self.stats,
            &mut self.recovery,
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
        loop {
            let captured_at = Instant::now();

            let result = {
                let mut guard = capture.lock().unwrap();
                guard.capture_frame_with_roi(&roi)
            };

            match result {
                Ok(Some(frame)) => {
                    #[cfg(debug_assertions)]
                    if captured_at.elapsed().as_millis() % 1000 < 20 { // 約1秒に1回ログ出力
                        tracing::debug!("Frame captured: {}x{}", frame.width, frame.height);
                    }
                    
                    let timestamped = TimestampedFrame { frame, captured_at };
                    Self::send_latest_only(tx.clone(), timestamped);
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
                            if processed_at.elapsed().as_millis() % 1000 < 20 { // 約1秒に1回ログ出力
                                let latency = processed_at.duration_since(timestamped.captured_at);
                                tracing::debug!("Frame processed: detected={}, latency={:?}", 
                                    detection_result.detected, latency);
                            }
                            
                            Self::send_latest_only(tx.clone(), detection);
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
    fn hid_thread(
        comm: Arc<Mutex<H>>,
        rx: Receiver<TimestampedDetection>,
        stats_tx: Sender<StatData>,
    ) {
        tracing::info!("HID thread started");
        loop {
            match rx.recv() {
                Ok(detection) => {
                    // HID送信（低レイテンシ最優先）
                    let hid_report = crate::domain::ports::detection_to_hid_report(&detection.result);
                    let send_result = {
                        let mut guard = comm.lock().unwrap();
                        guard.send(&hid_report)
                    };

                    let hid_sent_at = Instant::now();

                    if let Err(e) = send_result {
                        #[cfg(debug_assertions)]
                        tracing::error!("HID send error: {:?}", e);
                        #[cfg(not(debug_assertions))]
                        let _ = e;
                    }

                    // 統計データをStats/UIスレッドに送信（非ブロッキング）
                    let stat_data = StatData {
                        captured_at: detection.captured_at,
                        processed_at: detection.processed_at,
                        hid_sent_at,
                    };
                    let _ = stats_tx.try_send(stat_data);
                }
                Err(_) => {
                    // Channel closed
                    break;
                }
            }
        }
    }

    /// Stats/UIスレッド（統計情報管理とユーザー対話）
    fn stats_thread(
        stats_rx: Receiver<StatData>,
        stats: &mut StatsCollector,
        _recovery: &mut RecoveryState,
    ) {
        tracing::info!("Stats/UI thread started");
        loop {
            match stats_rx.recv() {
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
                Err(_) => {
                    // Channel closed
                    break;
                }
            }
        }
    }

    /// 最新のみ上書きポリシーで送信
    fn send_latest_only<T>(tx: Sender<T>, value: T) {
        match tx.try_send(value) {            Ok(_) => {}
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
        assert_eq!(config.capture_timeout, Duration::from_millis(8));
        assert!(config.enable_dirty_rect_optimization);
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
        PipelineRunner::<MockCapture, MockProcess, MockComm>::send_latest_only(tx.clone(), 1);
        assert_eq!(rx.try_recv().unwrap(), 1);

        // キューを満たす
        tx.try_send(2).unwrap();

        // キューが満杯の状態で新しい値を送信（満杯なので無視される）
        PipelineRunner::<MockCapture, MockProcess, MockComm>::send_latest_only(tx, 3);

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
