/// Port定義（Clean Architectureのインターフェース）
/// 
/// Domain層が外部実装に依存するための抽象trait。
/// Infrastructure層がこれらを実装し、Application層がDIで注入する。

use crate::domain::{DetectionResult, DomainResult, Frame, HsvRange, ProcessorBackend, Roi};

/// キャプチャポート: 画面フレームの取得を抽象化
#[allow(dead_code)]
pub trait CapturePort: Send + Sync {
    /// ROI指定でフレームをキャプチャする（GPU ROI実装）
    /// 
    /// 指定されたROI領域のみをGPU上で切り出し、CPU転送量を削減します。
    /// 
    /// # Arguments
    /// - `roi`: キャプチャするROI領域（デバイス座標系）
    /// 
    /// # Returns
    /// - `Ok(Some(Frame))`: フレームの取得成功（ROI領域のみ、Frame.width/heightはROIサイズ）
    /// - `Ok(None)`: タイムアウト（フレーム更新なし）
    /// - `Err(DomainError)`: 致命的エラー（再初期化が必要）
    fn capture_frame_with_roi(&mut self, roi: &Roi) -> DomainResult<Option<Frame>>;

    /// フレームをキャプチャする（全画面、デフォルト実装）
    /// 
    /// 内部的にはcapture_frame_with_roi()を全画面ROIで呼び出します。
    /// 
    /// # Returns
    /// - `Ok(Some(Frame))`: フレームの取得成功
    /// - `Ok(None)`: タイムアウト（フレーム更新なし）
    /// - `Err(DomainError)`: 致命的エラー（再初期化が必要）
    fn capture_frame(&mut self) -> DomainResult<Option<Frame>> {
        let info = self.device_info();
        let full_roi = Roi::new(0, 0, info.width, info.height);
        self.capture_frame_with_roi(&full_roi)
    }

    /// キャプチャセッションを再初期化
    /// 
    /// DDA接続が切断された場合などに呼び出される。
    fn reinitialize(&mut self) -> DomainResult<()>;

    /// キャプチャデバイスの情報を取得
    fn device_info(&self) -> DeviceInfo;
}

/// デバイス情報
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub width: u32,
    pub height: u32,
    pub refresh_rate: u32,
    pub name: String,
}

/// 処理ポート: 画像処理（色検知/YOLO等）を抽象化
#[allow(dead_code)]
pub trait ProcessPort: Send + Sync {
    /// フレームを処理して検出結果を返す
    /// 
    /// # Arguments
    /// - `frame`: 処理対象のフレーム
    /// - `roi`: 処理領域
    /// - `hsv_range`: 色検知の場合のHSVレンジ（YOLOの場合は無視）
    /// 
    /// # Returns
    /// - `Ok(DetectionResult)`: 検出結果
    /// - `Err(DomainError)`: 処理エラー
    fn process_frame(
        &mut self,
        frame: &Frame,
        roi: &Roi,
        hsv_range: &HsvRange,
    ) -> DomainResult<DetectionResult>;

    /// 処理バックエンドを取得
    fn backend(&self) -> ProcessorBackend;

    /// 処理統計を取得（オプション）
    fn stats(&self) -> ProcessStats {
        ProcessStats::default()
    }
}

/// 処理統計情報
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct ProcessStats {
    pub total_frames: u64,
    pub detected_frames: u64,
    pub avg_process_time_us: u64,
}

/// 通信ポート: HID送信を抽象化
#[allow(dead_code)]
pub trait CommPort: Send + Sync {
    /// 検出結果をデバイスに送信
    /// 
    /// # Arguments
    /// - `data`: 送信データ（8バイトを想定）
    /// 
    /// # Returns
    /// - `Ok(())`: 送信成功
    /// - `Err(DomainError)`: 送信エラー（デバイス切断等）
    fn send(&mut self, data: &[u8]) -> DomainResult<()>;

    /// デバイスとの接続状態を確認
    fn is_connected(&self) -> bool;

    /// デバイスとの接続を再試行
    fn reconnect(&mut self) -> DomainResult<()>;
}

/// 検出結果をHIDレポートに変換するヘルパー
/// 
/// # レポート構造（8バイト）
/// - [0]: ReportID (固定 0x01)
/// - [3-4]: Center X (u16, ビッグエンディアン)
/// - [5-6]: Center Y (u16, ビッグエンディアン)
/// - [7]: Reserved (0xFF)
pub fn detection_to_hid_report(result: &DetectionResult) -> Vec<u8> {
    let mut report = vec![0u8; 8];

    // ReportID
    report[0] = 0x01;

    report[1] = 0x00;
    report[2] = 0x00;

    // Center X (u16, ビッグエンディアン)
    let cx = result.center_x.clamp(0.0, 65535.0) as u16;
    let cx_bytes = cx.to_be_bytes();
    report[3] = cx_bytes[0]; // 上位バイト
    report[4] = cx_bytes[1]; // 下位バイト

    // Center Y (u16, ビッグエンディアン)
    let cy = result.center_y.clamp(0.0, 65535.0) as u16;
    let cy_bytes = cy.to_be_bytes();
    report[5] = cy_bytes[0]; // 上位バイト
    report[6] = cy_bytes[1]; // 下位バイト

    // Reserved
    report[7] = 0xFF;

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn test_detection_to_hid_report() {
        let result = DetectionResult {
            timestamp: Instant::now(),
            center_x: 123.5,
            center_y: 456.7,
            coverage: 9999,
            detected: true,
        };

        let report = detection_to_hid_report(&result);

        assert_eq!(report.len(), 8);
        assert_eq!(report[0], 0x01); // ReportID
        assert_eq!(report[1], 0x00); // Detection flag

        // Center X (ビッグエンディアン)
        let cx = u16::from_be_bytes([report[3], report[4]]);
        assert_eq!(cx, 123);

        // Center Y (ビッグエンディアン)
        let cy = u16::from_be_bytes([report[5], report[6]]);
        assert_eq!(cy, 456);

        // Reserved bytes
        assert_eq!(report[2], 0x00);
        assert_eq!(report[7], 0xFF);
    }

    #[test]
    fn test_detection_to_hid_report_none() {
        let result = DetectionResult::none();
        let report = detection_to_hid_report(&result);

        assert_eq!(report.len(), 8);
        assert_eq!(report[0], 0x01); // ReportID
        assert_eq!(report[1], 0); // Detection flag = 0
        
        // Center X/Y は0
        let cx = u16::from_be_bytes([report[3], report[4]]);
        let cy = u16::from_be_bytes([report[5], report[6]]);
        assert_eq!(cx, 0);
        assert_eq!(cy, 0);
    }
}
