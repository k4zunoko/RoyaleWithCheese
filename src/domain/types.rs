/// コア型定義
/// 
/// Domain層の中心となるデータ構造。
/// すべての処理で共有される不変の型。

use std::time::Instant;

/// ピクセル座標で指定されるROI（Region of Interest）
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Roi {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Roi {
    /// 新しいROIを作成
    #[allow(dead_code)]
    pub fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self { x, y, width, height }
    }

    /// ROIの中心座標を取得
    #[allow(dead_code)]
    pub fn center(&self) -> (u32, u32) {
        (self.x + self.width / 2, self.y + self.height / 2)
    }

    /// ROIの面積を取得
    #[allow(dead_code)]
    pub fn area(&self) -> u32 {
        self.width * self.height
    }

    /// 指定された矩形との交差判定
    #[allow(dead_code)]
    pub fn intersects(&self, other: &Roi) -> bool {
        let self_x2 = self.x + self.width;
        let self_y2 = self.y + self.height;
        let other_x2 = other.x + other.width;
        let other_y2 = other.y + other.height;

        self.x < other_x2 && self_x2 > other.x && self.y < other_y2 && self_y2 > other.y
    }
}

/// HSV色空間のレンジ（OpenCV準拠: H[0-180], S[0-255], V[0-255]）
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HsvRange {
    pub h_min: u8,
    pub h_max: u8,
    pub s_min: u8,
    pub s_max: u8,
    pub v_min: u8,
    pub v_max: u8,
}

impl HsvRange {
    /// 新しいHSVレンジを作成
    #[allow(dead_code)]
    pub fn new(h_min: u8, h_max: u8, s_min: u8, s_max: u8, v_min: u8, v_max: u8) -> Self {
        Self {
            h_min,
            h_max,
            s_min,
            s_max,
            v_min,
            v_max,
        }
    }

    /// OpenCVのScalar形式で下限を取得 [H, S, V]
    #[allow(dead_code)]
    pub fn lower_bound(&self) -> [u8; 3] {
        [self.h_min, self.s_min, self.v_min]
    }

    /// OpenCVのScalar形式で上限を取得 [H, S, V]
    #[allow(dead_code)]
    pub fn upper_bound(&self) -> [u8; 3] {
        [self.h_max, self.s_max, self.v_max]
    }
}

/// キャプチャされたフレームデータ
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Frame {
    /// フレーム取得時刻
    pub timestamp: Instant,
    /// フレーム画像データ（BGR形式、連続メモリ）
    pub data: Vec<u8>,
    /// 画像の幅
    pub width: u32,
    /// 画像の高さ
    pub height: u32,
    /// 更新領域（DirtyRect）のリスト
    pub dirty_rects: Vec<Roi>,
}

impl Frame {
    /// 新しいフレームを作成
    #[allow(dead_code)]
    pub fn new(data: Vec<u8>, width: u32, height: u32) -> Self {
        Self {
            timestamp: Instant::now(),
            data,
            width,
            height,
            dirty_rects: Vec::new(),
        }
    }

    /// DirtyRectsを設定
    #[allow(dead_code)]
    pub fn with_dirty_rects(mut self, rects: Vec<Roi>) -> Self {
        self.dirty_rects = rects;
        self
    }

    /// 指定されたROIとDirtyRectsが交差するか判定
    #[allow(dead_code)]
    pub fn roi_is_dirty(&self, roi: &Roi) -> bool {
        if self.dirty_rects.is_empty() {
            // DirtyRect情報がない場合は常に更新されたと見なす
            return true;
        }
        self.dirty_rects.iter().any(|rect| roi.intersects(rect))
    }
}

/// 色検知の結果
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetectionResult {
    /// 検出時刻
    pub timestamp: Instant,
    /// 検出された重心X座標（ROI内の相対座標）
    pub center_x: f32,
    /// 検出された重心Y座標（ROI内の相対座標）
    pub center_y: f32,
    /// 検出された領域の面積（ピクセル数）
    pub coverage: u32,
    /// 検出フラグ（true: 検出あり, false: 検出なし）
    pub detected: bool,
}

impl DetectionResult {
    /// 検出なしの結果を作成
    #[allow(dead_code)]
    pub fn none() -> Self {
        Self {
            timestamp: Instant::now(),
            center_x: 0.0,
            center_y: 0.0,
            coverage: 0,
            detected: false,
        }
    }

    /// 検出ありの結果を作成
    #[allow(dead_code)]
    pub fn some(center_x: f32, center_y: f32, coverage: u32) -> Self {
        Self {
            timestamp: Instant::now(),
            center_x,
            center_y,
            coverage,
            detected: true,
        }
    }
}

/// 変換後の座標（感度・クリッピング・デッドゾーン適用済み）
/// 
/// ROI中心からの相対座標（Δx, Δy）を表します。
/// HIDデバイスへの相対移動量として使用されます。
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransformedCoordinates {
    /// ROI中心からの相対X座標（ピクセル、±値）
    pub x: f32,
    /// ROI中心からの相対Y座標（ピクセル、±値）
    pub y: f32,
    /// 検出フラグ
    pub detected: bool,
}

impl TransformedCoordinates {
    /// 新しい変換座標を作成
    #[allow(dead_code)]
    pub fn new(x: f32, y: f32, detected: bool) -> Self {
        Self { x, y, detected }
    }
}

/// 処理バックエンドの種類
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessorBackend {
    /// CPU処理（OpenCV Mat使用）
    Cpu,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roi_center() {
        let roi = Roi::new(100, 200, 50, 60);
        assert_eq!(roi.center(), (125, 230));
    }

    #[test]
    fn test_roi_area() {
        let roi = Roi::new(0, 0, 100, 200);
        assert_eq!(roi.area(), 20000);
    }

    #[test]
    fn test_roi_intersects() {
        let roi1 = Roi::new(10, 10, 50, 50);
        let roi2 = Roi::new(40, 40, 50, 50);
        let roi3 = Roi::new(100, 100, 50, 50);

        assert!(roi1.intersects(&roi2));
        assert!(roi2.intersects(&roi1));
        assert!(!roi1.intersects(&roi3));
    }

    #[test]
    fn test_hsv_range_bounds() {
        let range = HsvRange::new(25, 45, 80, 255, 80, 255);
        assert_eq!(range.lower_bound(), [25, 80, 80]);
        assert_eq!(range.upper_bound(), [45, 255, 255]);
    }

    #[test]
    fn test_frame_roi_is_dirty() {
        let frame = Frame::new(vec![0; 1920 * 1080 * 3], 1920, 1080)
            .with_dirty_rects(vec![Roi::new(100, 100, 200, 200)]);

        let roi_dirty = Roi::new(150, 150, 100, 100);
        let roi_clean = Roi::new(500, 500, 100, 100);

        assert!(frame.roi_is_dirty(&roi_dirty));
        assert!(!frame.roi_is_dirty(&roi_clean));
    }

    #[test]
    fn test_detection_result_none() {
        let result = DetectionResult::none();
        assert!(!result.detected);
        assert_eq!(result.coverage, 0);
    }

    #[test]
    fn test_detection_result_some() {
        let result = DetectionResult::some(100.5, 200.3, 1500);
        assert!(result.detected);
        assert_eq!(result.center_x, 100.5);
        assert_eq!(result.center_y, 200.3);
        assert_eq!(result.coverage, 1500);
    }
}
