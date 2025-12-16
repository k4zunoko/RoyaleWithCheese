/// Port定義（Clean Architectureのインターフェース）
/// 
/// Domain層が外部実装に依存するための抽象trait。
/// Infrastructure層がこれらを実装し、Application層がDIで注入する。

use crate::domain::{
    CoordinateTransformConfig, DetectionResult, DomainResult, Frame, HsvRange,
    ProcessorBackend, Roi, TransformedCoordinates,
};

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

/// 座標変換を適用（感度・デッドゾーン・クリッピング）
/// 
/// ROI中心からの相対位置に対して、以下の処理を順次適用し、
/// 中心からの相対座標（Δx, Δy）を返します：
/// 1. デッドゾーン判定（中心からの距離がdead_zone未満なら(0, 0)に補正）
/// 2. 感度適用（x_sensitivity, y_sensitivityで倍率変更）
/// 3. クリッピング（±clip_limitで制限）
/// 
/// # 戻り値
/// ROI中心からの相対座標（Δx, Δy）。HIDデバイスへの相対移動量として使用。
/// 
/// # 低レイテンシ設計
/// - インライン展開可能な単純計算のみ
/// - メモリアロケーションなし（スタック上で完結）
/// - 分岐最小化
#[inline]
pub fn apply_coordinate_transform(
    result: &DetectionResult,
    roi: &Roi,
    transform: &CoordinateTransformConfig,
) -> TransformedCoordinates {
    if !result.detected {
        // 検出なしの場合は移動なし (Δx=0, Δy=0)
        return TransformedCoordinates::new(0.0, 0.0, false);
    }
    
    // ROI中心からの相対位置（ピクセル）
    let center_x = roi.width as f32 / 2.0;
    let center_y = roi.height as f32 / 2.0;
    let relative_x = result.center_x - center_x;
    let relative_y = result.center_y - center_y;
    
    // デッドゾーン判定
    let distance = (relative_x * relative_x + relative_y * relative_y).sqrt();
    if distance < transform.dead_zone {
        // デッドゾーン内: 移動なし (Δx=0, Δy=0)
        return TransformedCoordinates::new(0.0, 0.0, true);
    }
    
    // 感度適用
    let scaled_x = relative_x * transform.x_sensitivity;
    let scaled_y = relative_y * transform.y_sensitivity;
    
    // クリッピング（対称: ±clip_limit）
    let clipped_x = scaled_x.clamp(-transform.x_clip_limit, transform.x_clip_limit);
    let clipped_y = scaled_y.clamp(-transform.y_clip_limit, transform.y_clip_limit);
    
    // 中心からの相対座標として返す（Δx, Δy）
    TransformedCoordinates::new(clipped_x, clipped_y, true)
}

/// 変換座標をHIDレポートに変換
/// 
/// 中心からの相対座標（Δx, Δy）を符号付き16ビット整数に変換し、
/// HIDレポートとして送信します。
/// 
/// # レポート構造（8バイト）
/// - [0]: ReportID (固定 0x01)
/// - [1-2]: Reserved (0x00)
/// - [3-4]: Δx (i16, ビッグエンディアン, -32768 ~ 32767)
/// - [5-6]: Δy (i16, ビッグエンディアン, -32768 ~ 32767)
/// - [7]: Reserved (0xFF)
/// 
/// # C++実装との互換性
/// ```cpp
/// report[3] = xBytes.second; // 上位バイト
/// report[4] = xBytes.first;  // 下位バイト
/// ```
#[inline]
pub fn coordinates_to_hid_report(coords: &TransformedCoordinates) -> Vec<u8> {
    let mut report = vec![0u8; 8];

    // ReportID
    report[0] = 0x01;
    report[1] = 0x00;
    report[2] = 0x00;

    // Δx (i16, ビッグエンディアン)
    let delta_x = coords.x.clamp(-32768.0, 32767.0) as i16;
    let dx_bytes = delta_x.to_be_bytes();
    report[3] = dx_bytes[0]; // 上位バイト
    report[4] = dx_bytes[1]; // 下位バイト

    // Δy (i16, ビッグエンディアン)
    let delta_y = coords.y.clamp(-32768.0, 32767.0) as i16;
    let dy_bytes = delta_y.to_be_bytes();
    report[5] = dy_bytes[0]; // 上位バイト
    report[6] = dy_bytes[1]; // 下位バイト

    // Reserved
    report[7] = 0xFF;

    report
}

/// 検出結果を直接HIDレポートに変換（後方互換性のため残す）
/// 
/// # Deprecated
/// 新しいコードでは `apply_coordinate_transform()` + `coordinates_to_hid_report()` を使用してください。
/// 
/// # レポート構造（8バイト）
/// - [0]: ReportID (固定 0x01)
/// - [3-4]: Center X (u16, ビッグエンディアン)
/// - [5-6]: Center Y (u16, ビッグエンディアン)
/// - [7]: Reserved (0xFF)

#[allow(dead_code)]
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

    #[test]
    fn test_apply_coordinate_transform_no_detection() {
        let result = DetectionResult::none();
        let roi = Roi::new(0, 0, 100, 100);
        let transform = CoordinateTransformConfig::default();
        
        let coords = apply_coordinate_transform(&result, &roi, &transform);
        
        assert!(!coords.detected);
        assert_eq!(coords.x, 0.0); // 移動なし (Δx=0)
        assert_eq!(coords.y, 0.0); // 移動なし (Δy=0)
    }

    #[test]
    fn test_apply_coordinate_transform_no_transform() {
        // 感度1.0、クリッピングなし、デッドゾーンなし
        let result = DetectionResult::some(60.0, 70.0, 100); // 中心(50,50)から(+10, +20)
        let roi = Roi::new(0, 0, 100, 100);
        let transform = CoordinateTransformConfig {
            x_sensitivity: 1.0,
            y_sensitivity: 1.0,
            x_clip_limit: f32::MAX,
            y_clip_limit: f32::MAX,
            dead_zone: 0.0,
        };
        
        let coords = apply_coordinate_transform(&result, &roi, &transform);
        
        assert!(coords.detected);
        assert_eq!(coords.x, 10.0);  // Δx = +10
        assert_eq!(coords.y, 20.0);  // Δy = +20
    }

    #[test]
    fn test_apply_coordinate_transform_sensitivity() {
        // 感度2.0: 中心からの距離が2倍になる
        let result = DetectionResult::some(60.0, 70.0, 100); // 中心(50,50)から(+10, +20)
        let roi = Roi::new(0, 0, 100, 100);
        let transform = CoordinateTransformConfig {
            x_sensitivity: 2.0,
            y_sensitivity: 2.0,
            x_clip_limit: f32::MAX,
            y_clip_limit: f32::MAX,
            dead_zone: 0.0,
        };
        
        let coords = apply_coordinate_transform(&result, &roi, &transform);
        
        assert!(coords.detected);
        // Δx = 10 * 2.0 = 20, Δy = 20 * 2.0 = 40
        assert_eq!(coords.x, 20.0);
        assert_eq!(coords.y, 40.0);
    }

    #[test]
    fn test_apply_coordinate_transform_clipping() {
        // クリッピング: ±15でクリップ
        let result = DetectionResult::some(80.0, 90.0, 100); // 中心(50,50)から(+30, +40)
        let roi = Roi::new(0, 0, 100, 100);
        let transform = CoordinateTransformConfig {
            x_sensitivity: 1.0,
            y_sensitivity: 1.0,
            x_clip_limit: 15.0,
            y_clip_limit: 15.0,
            dead_zone: 0.0,
        };
        
        let coords = apply_coordinate_transform(&result, &roi, &transform);
        
        assert!(coords.detected);
        // Δx = clamp(30, -15, 15) = 15, Δy = clamp(40, -15, 15) = 15
        assert_eq!(coords.x, 15.0);
        assert_eq!(coords.y, 15.0);
    }

    #[test]
    fn test_apply_coordinate_transform_dead_zone() {
        // デッドゾーン: 中心て5.0未満は移動なし
        let result = DetectionResult::some(52.0, 53.0, 100); // 中心(50,50)から(+2, +3)、距離3.6
        let roi = Roi::new(0, 0, 100, 100);
        let transform = CoordinateTransformConfig {
            x_sensitivity: 1.0,
            y_sensitivity: 1.0,
            x_clip_limit: f32::MAX,
            y_clip_limit: f32::MAX,
            dead_zone: 5.0,
        };
        
        let coords = apply_coordinate_transform(&result, &roi, &transform);
        
        assert!(coords.detected); // 検出はされているがデッドゾーン内
        assert_eq!(coords.x, 0.0); // 移動なし (Δx=0)
        assert_eq!(coords.y, 0.0); // 移動なし (Δy=0)
    }

    #[test]
    fn test_apply_coordinate_transform_combined() {
        // 感度2.0 + クリッピング20.0 + デッドゾーン3.0
        let result = DetectionResult::some(65.0, 60.0, 100); // 中心(50,50)から(+15, +10)
        let roi = Roi::new(0, 0, 100, 100);
        let transform = CoordinateTransformConfig {
            x_sensitivity: 2.0,
            y_sensitivity: 2.0,
            x_clip_limit: 20.0,
            y_clip_limit: 20.0,
            dead_zone: 3.0,
        };
        
        let coords = apply_coordinate_transform(&result, &roi, &transform);
        
        assert!(coords.detected);
        // 距離 = sqrt(15^2 + 10^2) = 18.0 > 3.0 (デッドゾーン外)
        // スケール後: (15*2, 10*2) = (30, 20)
        // クリップ後: clamp(30, -20, 20) = 20, clamp(20, -20, 20) = 20
        // 結果: Δx = 20, Δy = 20
        assert_eq!(coords.x, 20.0);
        assert_eq!(coords.y, 20.0);
    }

    #[test]
    fn test_coordinates_to_hid_report_positive() {
        let coords = TransformedCoordinates::new(123.5, 456.7, true);
        let report = coordinates_to_hid_report(&coords);
        
        assert_eq!(report.len(), 8);
        assert_eq!(report[0], 0x01);
        
        let dx = i16::from_be_bytes([report[3], report[4]]);
        let dy = i16::from_be_bytes([report[5], report[6]]);
        assert_eq!(dx, 123);
        assert_eq!(dy, 456);
        assert_eq!(report[7], 0xFF);
    }

    #[test]
    fn test_coordinates_to_hid_report_negative() {
        // 負の値をテスト (左・上方向の移動)
        let coords = TransformedCoordinates::new(-50.3, -100.8, true);
        let report = coordinates_to_hid_report(&coords);
        
        assert_eq!(report.len(), 8);
        assert_eq!(report[0], 0x01);
        
        let dx = i16::from_be_bytes([report[3], report[4]]);
        let dy = i16::from_be_bytes([report[5], report[6]]);
        assert_eq!(dx, -50);
        assert_eq!(dy, -100);
        assert_eq!(report[7], 0xFF);
    }

    #[test]
    fn test_coordinates_to_hid_report_zero() {
        // 移動なし (Δx=0, Δy=0)
        let coords = TransformedCoordinates::new(0.0, 0.0, true);
        let report = coordinates_to_hid_report(&coords);
        
        assert_eq!(report.len(), 8);
        assert_eq!(report[0], 0x01);
        
        let dx = i16::from_be_bytes([report[3], report[4]]);
        let dy = i16::from_be_bytes([report[5], report[6]]);
        assert_eq!(dx, 0);
        assert_eq!(dy, 0);
        assert_eq!(report[7], 0xFF);
    }
}
