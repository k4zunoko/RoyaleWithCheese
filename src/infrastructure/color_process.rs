/// 色検知処理アダプタ
/// 
/// OpenCVを使用したHSV色空間での物体検出実装。

use crate::domain::{
    DetectionResult, DomainError, DomainResult, Frame, HsvRange, ProcessPort, ProcessorBackend, Roi,
};
use opencv::{
    core::{self, Mat, Scalar},
    imgproc,
};
use std::time::Instant;

/// 色検知処理アダプタ
pub struct ColorProcessAdapter {
    min_detection_area: u32,
}

impl ColorProcessAdapter {
    /// 新しい色検知処理アダプタを作成
    /// 
    /// # Arguments
    /// - `min_detection_area`: 最小検出面積（ピクセル）
    /// 
    /// # Returns
    /// ColorProcessAdapterインスタンス
    pub fn new(min_detection_area: u32) -> DomainResult<Self> {
        #[cfg(debug_assertions)]
        tracing::info!("Color process adapter initialized with OpenCV (CPU/Mat)");

        Ok(Self {
            min_detection_area,
        })
    }

    /// フレームデータをMatに変換（CPU処理用）
    /// 
    /// # Arguments
    /// - `frame`: キャプチャされたフレーム（BGRA形式）
    /// 
    /// # Returns
    /// BGR形式のMat
    fn frame_to_mat(&self, frame: &Frame) -> DomainResult<Mat> {
        // BGRA（4チャンネル）からBGR（3チャンネル）に変換
        let rows = frame.height as i32;
        let cols = frame.width as i32;

        // BGRAデータからMatを作成
        let bgra_mat = Mat::new_rows_cols_with_data(
            rows,
            cols,
            &frame.data,
        )
        .map_err(|e| DomainError::Process(format!("Failed to create Mat: {:?}", e)))?;

        // BGRA → BGR変換
        let mut bgr_mat = Mat::default();
        imgproc::cvt_color_def(&bgra_mat, &mut bgr_mat, imgproc::COLOR_BGRA2BGR)
            .map_err(|e| DomainError::Process(format!("Failed to convert BGRA to BGR: {:?}", e)))?;

        Ok(bgr_mat)
    }



    /// HSVマスク処理（Mat版）
    fn process_with_mat(
        &self,
        bgr: &Mat,
        hsv_range: &HsvRange,
    ) -> DomainResult<DetectionResult> {
        // BGR → HSV変換
        let mut hsv = Mat::default();
        imgproc::cvt_color_def(bgr, &mut hsv, imgproc::COLOR_BGR2HSV)
            .map_err(|e| DomainError::Process(format!("Failed to convert BGR to HSV: {:?}", e)))?;

        // HSVレンジでマスク生成
        let lower = Scalar::new(hsv_range.h_min as f64, hsv_range.s_min as f64, hsv_range.v_min as f64, 0.0);
        let upper = Scalar::new(hsv_range.h_max as f64, hsv_range.s_max as f64, hsv_range.v_max as f64, 0.0);
        
        let mut mask = Mat::default();
        core::in_range(&hsv, &lower, &upper, &mut mask)
            .map_err(|e| DomainError::Process(format!("Failed to create mask: {:?}", e)))?;

        // モーメント計算
        self.calculate_moments(&mask)
    }



    /// モーメント計算から重心と面積を取得
    fn calculate_moments(&self, mask: &Mat) -> DomainResult<DetectionResult> {
        let moments = imgproc::moments(mask, false)
            .map_err(|e| DomainError::Process(format!("Failed to calculate moments: {:?}", e)))?;

        let m00 = moments.m00;
        let coverage = m00 as u32;

        // 最小検出面積チェック
        if coverage < self.min_detection_area {
            return Ok(DetectionResult::none());
        }

        // 重心計算
        let center_x = if m00 > 0.0 {
            (moments.m10 / m00) as f32
        } else {
            0.0
        };

        let center_y = if m00 > 0.0 {
            (moments.m01 / m00) as f32
        } else {
            0.0
        };

        Ok(DetectionResult {
            timestamp: Instant::now(),
            detected: true,
            center_x,
            center_y,
            coverage,
        })
    }
}

impl ProcessPort for ColorProcessAdapter {
    fn process_frame(
        &mut self,
        frame: &Frame,
        _roi: &Roi,
        hsv_range: &HsvRange,
    ) -> DomainResult<DetectionResult> {
        // フレームは既にROI領域のみを含んでいる（DDAがROI切り出し済み）
        // そのため、ここではフレーム全体を処理すればよい
        let mat = self.frame_to_mat(frame)?;
        self.process_with_mat(&mat, hsv_range)
    }

    fn backend(&self) -> ProcessorBackend {
        ProcessorBackend::Cpu
    }
}
