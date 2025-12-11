/// 色検知処理アダプタ
/// 
/// OpenCVを使用したHSV色空間での物体検出実装。
/// OpenCL（UMat）による高速化に対応。

use crate::domain::{
    DetectionResult, DomainError, DomainResult, Frame, HsvRange, ProcessPort, ProcessorBackend, Roi,
};
use opencv::{
    core::{self, Mat, MatTraitConst, Scalar, Size, UMat, UMatUsageFlags, Vector},
    imgproc,
    prelude::*,
};
use std::time::Instant;

/// 色検知処理アダプタ
pub struct ColorProcessAdapter {
    backend: ProcessorBackend,
    min_detection_area: u32,
}

impl ColorProcessAdapter {
    /// 新しい色検知処理アダプタを作成
    /// 
    /// # Arguments
    /// - `use_opencl`: OpenCL（UMat）を使用するか
    /// - `min_detection_area`: 最小検出面積（ピクセル）
    /// 
    /// # Returns
    /// ColorProcessAdapterインスタンス
    pub fn new(use_opencl: bool, min_detection_area: u32) -> DomainResult<Self> {
        let backend = if use_opencl {
            // OpenCL利用可能性を確認
            if Self::check_opencl_available() {
                #[cfg(debug_assertions)]
                tracing::info!("OpenCL available, using UMat for GPU acceleration");
                ProcessorBackend::OpenCl
            } else {
                #[cfg(debug_assertions)]
                tracing::warn!("OpenCL not available, fallback to CPU (Mat)");
                ProcessorBackend::Cpu
            }
        } else {
            #[cfg(debug_assertions)]
            tracing::info!("OpenCL disabled by config, using CPU (Mat)");
            ProcessorBackend::Cpu
        };

        Ok(Self {
            backend,
            min_detection_area,
        })
    }

    /// OpenCLが利用可能かチェック
    fn check_opencl_available() -> bool {
        match core::have_opencl() {
            Ok(true) => {
                // OpenCLを有効化
                if let Err(e) = core::set_use_opencl(true) {
                    #[cfg(debug_assertions)]
                    tracing::warn!("Failed to enable OpenCL: {:?}", e);
                    return false;
                }
                true
            }
            _ => false,
        }
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
        let bgra_mat = unsafe {
            Mat::new_rows_cols_with_data(
                rows,
                cols,
                core::CV_8UC4, // BGRA形式
                frame.data.as_ptr() as *mut core::c_void,
                core::Mat_AUTO_STEP,
            )
            .map_err(|e| DomainError::Process(format!("Failed to create Mat: {:?}", e)))?
        };

        // BGRA → BGR変換
        let mut bgr_mat = Mat::default();
        imgproc::cvt_color(&bgra_mat, &mut bgr_mat, imgproc::COLOR_BGRA2BGR, 0)
            .map_err(|e| DomainError::Process(format!("Failed to convert BGRA to BGR: {:?}", e)))?;

        Ok(bgr_mat)
    }

    /// フレームデータをUMatに変換（OpenCL処理用）
    /// 
    /// # Arguments
    /// - `frame`: キャプチャされたフレーム（BGRA形式）
    /// 
    /// # Returns
    /// BGR形式のUMat
    fn frame_to_umat(&self, frame: &Frame) -> DomainResult<UMat> {
        // まずMatを作成
        let mat = self.frame_to_mat(frame)?;

        // MatをUMatに変換（GPU転送）
        let mut umat = UMat::new(UMatUsageFlags::USAGE_DEFAULT);
        mat.copy_to(&mut umat)
            .map_err(|e| DomainError::Process(format!("Failed to convert Mat to UMat: {:?}", e)))?;

        Ok(umat)
    }

    /// HSVマスク処理（Mat版）
    fn process_with_mat(
        &self,
        bgr: &Mat,
        hsv_range: &HsvRange,
    ) -> DomainResult<DetectionResult> {
        // BGR → HSV変換
        let mut hsv = Mat::default();
        imgproc::cvt_color(bgr, &mut hsv, imgproc::COLOR_BGR2HSV, 0)
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

    /// HSVマスク処理（UMat版）
    fn process_with_umat(
        &self,
        bgr: &UMat,
        hsv_range: &HsvRange,
    ) -> DomainResult<DetectionResult> {
        // BGR → HSV変換
        let mut hsv = UMat::new(UMatUsageFlags::USAGE_DEFAULT);
        imgproc::cvt_color(bgr, &mut hsv, imgproc::COLOR_BGR2HSV, 0)
            .map_err(|e| DomainError::Process(format!("Failed to convert BGR to HSV: {:?}", e)))?;

        // HSVレンジでマスク生成
        let lower = Scalar::new(hsv_range.h_min as f64, hsv_range.s_min as f64, hsv_range.v_min as f64, 0.0);
        let upper = Scalar::new(hsv_range.h_max as f64, hsv_range.s_max as f64, hsv_range.v_max as f64, 0.0);
        
        let mut mask = UMat::new(UMatUsageFlags::USAGE_DEFAULT);
        core::in_range(&hsv, &lower, &upper, &mut mask)
            .map_err(|e| DomainError::Process(format!("Failed to create mask: {:?}", e)))?;

        // UMat → Mat変換してモーメント計算（momentsはMatのみ対応）
        let mut mask_mat = Mat::default();
        mask.copy_to(&mut mask_mat)
            .map_err(|e| DomainError::Process(format!("Failed to convert UMat to Mat: {:?}", e)))?;

        self.calculate_moments(&mask_mat)
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

        match self.backend {
            ProcessorBackend::Cpu => {
                let mat = self.frame_to_mat(frame)?;
                self.process_with_mat(&mat, hsv_range)
            }
            ProcessorBackend::OpenCl => {
                let umat = self.frame_to_umat(frame)?;
                self.process_with_umat(&umat, hsv_range)
            }
        }
    }

    fn backend(&self) -> ProcessorBackend {
        self.backend
    }
}
