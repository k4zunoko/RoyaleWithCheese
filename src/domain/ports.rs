//! Domainポート定義
//!
//! Clean Architectureにおける抽象ポート群。

use crate::domain::error::{DomainError, DomainResult};
use crate::domain::types::{
    DetectionResult, DeviceInfo, Frame, GpuFrame, HsvRange, InputState, ProcessorBackend, Roi,
    TransformedCoordinates, VirtualKey,
};

/// キャプチャポート。
pub trait CapturePort: Send {
    /// ROI領域のフレームを取得します。
    fn capture_frame(&mut self, roi: &Roi) -> DomainResult<Option<Frame>>;

    /// ROI領域のGPUフレームを取得します。
    fn capture_gpu_frame(&mut self, _roi: &Roi) -> DomainResult<Option<GpuFrame>> {
        Err(DomainError::GpuNotAvailable(
            "このアダプタはGPUフレームをサポートしていません".into(),
        ))
    }

    /// キャプチャ実装を再初期化します。
    fn reinitialize(&mut self) -> DomainResult<()>;

    /// キャプチャデバイス情報を返します。
    fn device_info(&self) -> DeviceInfo;

    /// GPUフレーム取得に対応しているかを返します。
    fn supports_gpu_frame(&self) -> bool {
        false
    }
}

/// 画像処理ポート。
pub trait ProcessPort: Send {
    /// CPUフレームを処理して検出結果を返します。
    fn process_frame(
        &mut self,
        frame: &Frame,
        roi: &Roi,
        hsv_range: &HsvRange,
    ) -> DomainResult<DetectionResult>;

    /// GPUフレームを処理して検出結果を返します。
    fn process_gpu_frame(
        &mut self,
        _gpu_frame: &GpuFrame,
        _hsv_range: &HsvRange,
    ) -> DomainResult<DetectionResult> {
        Err(DomainError::GpuNotAvailable(
            "このアダプタはGPU処理をサポートしていません".into(),
        ))
    }

    /// 処理バックエンド種別を返します。
    fn backend(&self) -> ProcessorBackend;

    /// GPU処理に対応しているかを返します。
    fn supports_gpu_processing(&self) -> bool {
        false
    }
}

/// 通信ポート。
pub trait CommPort: Send {
    /// データを送信します。
    fn send(&mut self, data: &[u8]) -> DomainResult<()>;

    /// 接続を再確立します。
    fn reconnect(&mut self) -> DomainResult<()>;

    /// 現在の接続状態を返します。
    fn is_connected(&self) -> bool;
}

/// 入力ポート。
pub trait InputPort: Send + Sync {
    /// 指定キーが押されているかを返します。
    fn is_key_pressed(&self, key: VirtualKey) -> bool;

    /// 入力状態を一括取得します。
    fn poll_input_state(&self) -> InputState {
        InputState {
            mouse_left: self.is_key_pressed(VirtualKey::LeftButton),
            mouse_right: self.is_key_pressed(VirtualKey::RightButton),
        }
    }
}

/// 検出結果に座標変換（感度適用）を行います。
#[inline]
pub fn apply_coordinate_transform(
    result: &DetectionResult,
    roi: &Roi,
    sensitivity: f64,
) -> TransformedCoordinates {
    if !result.detected {
        return TransformedCoordinates::new(0.0, 0.0, false);
    }

    let center_x = roi.width as f64 / 2.0;
    let center_y = roi.height as f64 / 2.0;

    TransformedCoordinates::new(
        (result.center_x as f64 - center_x) * sensitivity,
        (result.center_y as f64 - center_y) * sensitivity,
        true,
    )
}

#[inline]
fn encode_hid_delta(delta: f64) -> (u8, u8) {
    if delta == 0.0 {
        return (0x00, 0x00);
    }

    let magnitude = delta.abs().round().min(255.0) as u8;
    if delta > 0.0 {
        (magnitude, 0x00)
    } else {
        ((256.0 - f64::from(magnitude)) as u8, 0xFF)
    }
}

/// 相対座標を8バイトHIDレポートへ変換します。
#[inline]
pub fn coordinates_to_hid_report(coords: &TransformedCoordinates) -> Vec<u8> {
    if !coords.detected {
        return vec![0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF];
    }

    let (x_val, x_sign) = encode_hid_delta(coords.delta_x);
    let (y_val, y_sign) = encode_hid_delta(coords.delta_y);
    vec![0x01, 0x00, 0x00, x_val, x_sign, y_val, y_sign, 0xFF]
}

/// 検出結果を絶対座標モードの8バイトHIDレポートへ変換します。
#[inline]
pub fn detection_to_hid_report(result: &DetectionResult) -> Vec<u8> {
    let x = result.center_x.clamp(0.0, 65535.0) as u16;
    let y = result.center_y.clamp(0.0, 65535.0) as u16;
    let xb = x.to_be_bytes();
    let yb = y.to_be_bytes();
    vec![0x01, 0x00, 0x00, xb[0], xb[1], yb[0], yb[1], 0xFF]
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;

    struct MockCapture {
        info: DeviceInfo,
        frame: Option<Frame>,
        gpu_supported: bool,
        reinitialized: bool,
    }

    impl MockCapture {
        fn new() -> Self {
            Self {
                info: DeviceInfo::new(1920, 1080, "Mock Display".to_string()),
                frame: Some(Frame::new(vec![1, 2, 3], 1, 1)),
                gpu_supported: true,
                reinitialized: false,
            }
        }
    }

    impl CapturePort for MockCapture {
        fn capture_frame(&mut self, _roi: &Roi) -> DomainResult<Option<Frame>> {
            Ok(self.frame.clone())
        }

        fn capture_gpu_frame(&mut self, _roi: &Roi) -> DomainResult<Option<GpuFrame>> {
            if self.gpu_supported {
                Ok(Some(GpuFrame::new(None, 1, 1, DXGI_FORMAT_B8G8R8A8_UNORM)))
            } else {
                Err(DomainError::GpuNotAvailable(
                    "このアダプタはGPUフレームをサポートしていません".into(),
                ))
            }
        }

        fn reinitialize(&mut self) -> DomainResult<()> {
            self.reinitialized = true;
            Ok(())
        }

        fn device_info(&self) -> DeviceInfo {
            self.info.clone()
        }

        fn supports_gpu_frame(&self) -> bool {
            self.gpu_supported
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
            Ok(DetectionResult::detected(55.0, 44.0, 0.8))
        }

        fn process_gpu_frame(
            &mut self,
            _gpu_frame: &GpuFrame,
            _hsv_range: &HsvRange,
        ) -> DomainResult<DetectionResult> {
            Ok(DetectionResult::detected(12.0, 34.0, 0.6))
        }

        fn backend(&self) -> ProcessorBackend {
            ProcessorBackend::Gpu
        }

        fn supports_gpu_processing(&self) -> bool {
            true
        }
    }

    struct MockComm {
        sent_packets: Vec<Vec<u8>>,
        connected: bool,
    }

    impl MockComm {
        fn new() -> Self {
            Self {
                sent_packets: Vec::new(),
                connected: false,
            }
        }
    }

    impl CommPort for MockComm {
        fn send(&mut self, data: &[u8]) -> DomainResult<()> {
            self.sent_packets.push(data.to_vec());
            Ok(())
        }

        fn reconnect(&mut self) -> DomainResult<()> {
            self.connected = true;
            Ok(())
        }

        fn is_connected(&self) -> bool {
            self.connected
        }
    }

    struct MockInput {
        pressed: Vec<VirtualKey>,
    }

    impl MockInput {
        fn new(pressed: &[VirtualKey]) -> Self {
            Self {
                pressed: pressed.to_vec(),
            }
        }
    }

    impl InputPort for MockInput {
        fn is_key_pressed(&self, key: VirtualKey) -> bool {
            self.pressed.contains(&key)
        }
    }

    #[test]
    fn capture_port_mock_returns_frame_and_device_info() {
        let mut capture = MockCapture::new();
        let frame = capture
            .capture_frame(&Roi::new(0, 0, 100, 100))
            .expect("capture should succeed")
            .expect("frame should exist");
        assert_eq!(frame.width, 1);
        assert_eq!(frame.height, 1);

        let info = capture.device_info();
        assert_eq!(info.width, 1920);
        assert_eq!(info.height, 1080);
        assert_eq!(info.name, "Mock Display");
    }

    #[test]
    fn capture_port_default_gpu_method_returns_not_available() {
        struct CpuOnlyCapture;
        impl CapturePort for CpuOnlyCapture {
            fn capture_frame(&mut self, _roi: &Roi) -> DomainResult<Option<Frame>> {
                Ok(None)
            }
            fn reinitialize(&mut self) -> DomainResult<()> {
                Ok(())
            }
            fn device_info(&self) -> DeviceInfo {
                DeviceInfo::new(800, 600, "CPU Only".to_string())
            }
        }

        let mut capture = CpuOnlyCapture;
        let err = capture
            .capture_gpu_frame(&Roi::new(0, 0, 1, 1))
            .expect_err("default impl should return error");
        match err {
            DomainError::GpuNotAvailable(msg) => {
                assert_eq!(msg, "このアダプタはGPUフレームをサポートしていません")
            }
            _ => panic!("unexpected error variant"),
        }
    }

    #[test]
    fn capture_port_reinitialize_and_gpu_support() {
        let mut capture = MockCapture::new();
        assert!(capture.supports_gpu_frame());
        assert!(!capture.reinitialized);
        capture.reinitialize().expect("reinitialize should succeed");
        assert!(capture.reinitialized);
    }

    #[test]
    fn process_port_mock_processes_cpu_and_gpu_frames() {
        let mut process = MockProcess;
        let frame = Frame::new(vec![0; 9], 1, 3);
        let gpu_frame = GpuFrame::new(None, 1, 1, DXGI_FORMAT_B8G8R8A8_UNORM);
        let roi = Roi::new(0, 0, 100, 100);
        let hsv = HsvRange::new(0, 180, 0, 255, 0, 255);

        let cpu_result = process
            .process_frame(&frame, &roi, &hsv)
            .expect("cpu process should succeed");
        assert!(cpu_result.detected);
        assert_eq!(cpu_result.center_x, 55.0);

        let gpu_result = process
            .process_gpu_frame(&gpu_frame, &hsv)
            .expect("gpu process should succeed");
        assert!(gpu_result.detected);
        assert_eq!(gpu_result.center_y, 34.0);

        assert_eq!(process.backend(), ProcessorBackend::Gpu);
        assert!(process.supports_gpu_processing());
    }

    #[test]
    fn process_port_default_gpu_method_returns_not_available() {
        struct CpuOnlyProcess;
        impl ProcessPort for CpuOnlyProcess {
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

        let mut process = CpuOnlyProcess;
        let gpu_frame = GpuFrame::new(None, 1, 1, DXGI_FORMAT_B8G8R8A8_UNORM);
        let hsv = HsvRange::new(0, 180, 0, 255, 0, 255);
        let err = process
            .process_gpu_frame(&gpu_frame, &hsv)
            .expect_err("default impl should return error");
        match err {
            DomainError::GpuNotAvailable(msg) => {
                assert_eq!(msg, "このアダプタはGPU処理をサポートしていません")
            }
            _ => panic!("unexpected error variant"),
        }
    }

    #[test]
    fn comm_port_mock_records_bytes_and_reconnects() {
        let mut comm = MockComm::new();
        assert!(!comm.is_connected());
        comm.send(&[0x01, 0x02, 0x03]).expect("send should succeed");
        comm.reconnect().expect("reconnect should succeed");
        assert!(comm.is_connected());
        assert_eq!(comm.sent_packets[0], vec![0x01, 0x02, 0x03]);
    }

    #[test]
    fn input_port_mock_returns_key_and_input_state() {
        let input = MockInput::new(&[VirtualKey::LeftButton]);
        assert!(input.is_key_pressed(VirtualKey::LeftButton));
        assert!(!input.is_key_pressed(VirtualKey::RightButton));
        let state = input.poll_input_state();
        assert!(state.mouse_left);
        assert!(!state.mouse_right);
    }

    #[test]
    fn apply_coordinate_transform_returns_zero_when_not_detected() {
        let result = DetectionResult::not_detected();
        let roi = Roi::new(0, 0, 100, 100);
        let transform = CoordinateTransformConfig {
            sensitivity: 1.5,
            x_clip_limit: 100.0,
            y_clip_limit: 100.0,
            dead_zone: 0.0,
        };
        let coords = apply_coordinate_transform(&result, &roi, &transform);
        assert!(!coords.detected);
        assert_eq!(coords.delta_x, 0.0);
        assert_eq!(coords.delta_y, 0.0);
    }

    #[test]
    fn apply_coordinate_transform_uses_roi_center_and_sensitivity() {
        let result = DetectionResult::detected(60.0, 30.0, 0.5);
        let roi = Roi::new(300, 400, 100, 80);
        let transform = CoordinateTransformConfig {
            sensitivity: 2.0,
            x_clip_limit: 100.0,
            y_clip_limit: 100.0,
            dead_zone: 0.0,
        };
        let coords = apply_coordinate_transform(&result, &roi, &transform);
        assert!(coords.detected);
        assert_eq!(coords.delta_x, 20.0);
        assert_eq!(coords.delta_y, -20.0);
    }

    #[test]
    fn apply_coordinate_transform_applies_dead_zone() {
        let result = DetectionResult::detected(52.0, 53.0, 0.5);
        let roi = Roi::new(0, 0, 100, 100);
        let transform = CoordinateTransformConfig {
            sensitivity: 2.0,
            x_clip_limit: 100.0,
            y_clip_limit: 100.0,
            dead_zone: 5.0,
        };
        let coords = apply_coordinate_transform(&result, &roi, &transform);
        assert!(coords.detected);
        assert_eq!(coords.delta_x, 0.0);
        assert_eq!(coords.delta_y, 0.0);
    }

    #[test]
    fn apply_coordinate_transform_applies_clip_limits() {
        let result = DetectionResult::detected(90.0, 10.0, 0.5);
        let roi = Roi::new(0, 0, 100, 100);
        let transform = CoordinateTransformConfig {
            sensitivity: 2.0,
            x_clip_limit: 30.0,
            y_clip_limit: 20.0,
            dead_zone: 0.0,
        };
        let coords = apply_coordinate_transform(&result, &roi, &transform);
        assert!(coords.detected);
        assert_eq!(coords.delta_x, 30.0);
        assert_eq!(coords.delta_y, -20.0);
    }

    #[test]
    fn coordinates_to_hid_report_uses_hardware_contract_bytes() {
        let coords = TransformedCoordinates {
            delta_x: 5.0,
            delta_y: -3.0,
            detected: true,
        };
        let report = coordinates_to_hid_report(&coords);
        assert_eq!(report, vec![0x01, 0x00, 0x00, 0x05, 0x00, 0xFD, 0xFF, 0xFF]);
    }

    #[test]
    fn coordinates_to_hid_report_returns_zero_payload_when_not_detected() {
        let coords = TransformedCoordinates {
            delta_x: 12.0,
            delta_y: -9.0,
            detected: false,
        };
        let report = coordinates_to_hid_report(&coords);
        assert_eq!(report, vec![0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF]);
    }

    #[test]
    fn detection_to_hid_report_encodes_absolute_coordinates_big_endian() {
        let result = DetectionResult::detected(123.9, 456.1, 0.7);
        let report = detection_to_hid_report(&result);
        assert_eq!(report, vec![0x01, 0x00, 0x00, 0x00, 0x7B, 0x01, 0xC8, 0xFF]);
    }
}
