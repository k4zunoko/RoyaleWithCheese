/// Port定義（Clean Architectureのインターフェース）
/// 
/// Domain層が外部実装に依存するための抽象trait。
/// Infrastructure層がこれらを実装し、Application層がDIで注入する。

use crate::domain::{DetectionResult, DomainResult, Frame, HsvRange, ProcessorBackend, Roi};

/// キャプチャポート: 画面フレームの取得を抽象化
pub trait CapturePort: Send + Sync {
    /// フレームをキャプチャする
    /// 
    /// # Returns
    /// - `Ok(Some(Frame))`: フレームの取得成功
    /// - `Ok(None)`: タイムアウト（フレーム更新なし）
    /// - `Err(DomainError)`: 致命的エラー（再初期化が必要）
    fn capture_frame(&mut self) -> DomainResult<Option<Frame>>;

    /// キャプチャセッションを再初期化
    /// 
    /// DDA接続が切断された場合などに呼び出される。
    fn reinitialize(&mut self) -> DomainResult<()>;

    /// キャプチャデバイスの情報を取得
    fn device_info(&self) -> DeviceInfo;
}

/// デバイス情報
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub width: u32,
    pub height: u32,
    pub refresh_rate: u32,
    pub name: String,
}

/// 処理ポート: 画像処理（色検知/YOLO等）を抽象化
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
#[derive(Debug, Clone, Default)]
pub struct ProcessStats {
    pub total_frames: u64,
    pub detected_frames: u64,
    pub avg_process_time_us: u64,
}

/// 通信ポート: HID送信を抽象化
pub trait CommPort: Send + Sync {
    /// 検出結果をデバイスに送信
    /// 
    /// # Arguments
    /// - `data`: 送信データ（最大16バイト程度を想定）
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
/// # レポート構造（16バイト）
/// - [0]: ReportID (固定 0x01)
/// - [1-4]: Timestamp (u32, ms)
/// - [5-6]: Center X (u16)
/// - [7-8]: Center Y (u16)
/// - [9-10]: Coverage area (u16)
/// - [11]: Detection flag (0/1)
/// - [12-15]: Reserved (0x00)
pub fn detection_to_hid_report(result: &DetectionResult) -> Vec<u8> {
    let mut report = vec![0u8; 16];

    // ReportID
    report[0] = 0x01;

    // Timestamp (ms since epoch - 下位32bit)
    let ts_ms = result.timestamp.elapsed().as_millis() as u32;
    report[1..5].copy_from_slice(&ts_ms.to_le_bytes());

    // Center X (u16)
    let cx = result.center_x.clamp(0.0, 65535.0) as u16;
    report[5..7].copy_from_slice(&cx.to_le_bytes());

    // Center Y (u16)
    let cy = result.center_y.clamp(0.0, 65535.0) as u16;
    report[7..9].copy_from_slice(&cy.to_le_bytes());

    // Coverage (u16)
    let coverage = result.coverage.clamp(0, 65535) as u16;
    report[9..11].copy_from_slice(&coverage.to_le_bytes());

    // Detection flag
    report[11] = if result.detected { 1 } else { 0 };

    // Reserved [12-15] は0のまま

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

        assert_eq!(report.len(), 16);
        assert_eq!(report[0], 0x01); // ReportID

        // Center X
        let cx = u16::from_le_bytes([report[5], report[6]]);
        assert_eq!(cx, 123);

        // Center Y
        let cy = u16::from_le_bytes([report[7], report[8]]);
        assert_eq!(cy, 456);

        // Coverage
        let coverage = u16::from_le_bytes([report[9], report[10]]);
        assert_eq!(coverage, 9999);

        // Detection flag
        assert_eq!(report[11], 1);
    }

    #[test]
    fn test_detection_to_hid_report_none() {
        let result = DetectionResult::none();
        let report = detection_to_hid_report(&result);

        assert_eq!(report[11], 0); // Detection flag = 0
        assert_eq!(report[9], 0); // Coverage = 0
        assert_eq!(report[10], 0);
    }
}
