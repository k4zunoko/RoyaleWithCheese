//! パイプライン制御モジュール
//!
//! Capture / Process / HID / Stats の4スレッド構成でパイプラインを制御します。
//! HID送信を統計処理から分離し、低レイテンシを実現します。

use crate::domain::{
    error::DomainResult,
    ports::{CapturePort, CommPort, InputPort, ProcessPort},
    types::{Roi, HsvRange},
};
use crate::application::{
    recovery::RecoveryState, 
    stats::StatsCollector,
    threads,
};
use crossbeam_channel::{bounded, unbounded};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
    
    /// 最大距離の2乗を取得（threads.rsからアクセス用）
    pub(crate) fn max_distance_squared(&self) -> f32 {
        self.max_distance_squared
    }
    
    /// アクティブウィンドウの持続時間を取得（threads.rsからアクセス用）
    pub(crate) fn active_window_duration(&self) -> Duration {
        self.active_window_duration
    }
}

/// パイプライン設定
#[derive(Debug, Clone)]
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

/// パイプライン実行コンテキスト
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
    runtime_state: crate::application::runtime_state::RuntimeState,
    activation_conditions: ActivationConditions,
    
    roi: Roi,
    hsv_range: HsvRange,
    coordinate_transform: crate::domain::CoordinateTransformConfig,
}

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
            runtime_state: crate::application::runtime_state::RuntimeState::new(),
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
    pub fn runtime_state(&self) -> &crate::application::runtime_state::RuntimeState {
        &self.runtime_state
    }

    /// パイプラインを起動（ブロッキング）
    ///
    /// # Returns
    /// エラーが発生した場合のみ戻る
    pub fn run(mut self) -> DomainResult<()> {
        let (capture_tx, capture_rx) = bounded::<threads::TimestampedFrame>(1);
        let (process_tx, process_rx) = bounded::<threads::TimestampedDetection>(1);
        let (stats_tx, stats_rx) = unbounded::<threads::StatData>();

        // Capture Thread
        let capture_handle = {
            let capture = Arc::clone(&self.capture);
            let tx = capture_tx;
            let roi = self.roi;
            std::thread::spawn(move || {
                threads::capture_thread(capture, tx, roi);
            })
        };

        // Process Thread
        let process_handle = {
            let process = Arc::clone(&self.process);
            let roi = self.roi;
            let hsv_range = self.hsv_range;
            let rx = capture_rx;
            let tx = process_tx;
            let stats_tx_clone = stats_tx.clone();
            std::thread::spawn(move || {
                threads::process_thread(process, rx, tx, roi, hsv_range, stats_tx_clone);
            })
        };

        // HID Thread
        let hid_handle = {
            let comm = Arc::clone(&self.comm);
            let rx = process_rx;
            let roi = self.roi;
            let coordinate_transform = self.coordinate_transform.clone();
            let stats_tx_clone = stats_tx.clone();
            let hid_send_interval = self.config.hid_send_interval;
            let runtime_state = self.runtime_state.clone();
            let activation_conditions = self.activation_conditions.clone();
            std::thread::spawn(move || {
                threads::hid_thread(comm, rx, roi, coordinate_transform, stats_tx_clone, hid_send_interval, runtime_state, activation_conditions);
            })
        };

        // Stats/UI Thread（メインスレッドで実行）
        threads::stats_thread(
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

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        error::DomainError,
        ports::{DeviceInfo, VirtualKey},
        types::{DetectionResult, Frame, Roi, HsvRange, ProcessorBackend},
    };
    use crate::application::threads::TimestampedFrame;
    use std::time::Instant;

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
        threads::send_latest_only(&tx, 1);
        assert_eq!(rx.try_recv().unwrap(), 1);

        // キューを満たす
        tx.try_send(2).unwrap();

        // キューが満杯の状態で新しい値を送信（満杯なので無視される）
        threads::send_latest_only(&tx, 3);

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
