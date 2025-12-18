/// 色検知処理アダプタ
/// 
/// OpenCVを使用したHSV色空間での物体検出実装。

use crate::domain::{
    config::DetectionMethod, DetectionResult, DomainError, DomainResult, Frame, HsvRange,
    ProcessPort, ProcessorBackend, Roi,
};
use opencv::{
    core::{self, Mat, Scalar},
    imgproc,
};

#[cfg(test)]
use opencv::prelude::{MatExprTraitConst, MatTraitConst};
use std::time::Instant;

/// 色検知処理アダプタ
pub struct ColorProcessAdapter {
    min_detection_area: u32,
    detection_method: DetectionMethod,
}

impl ColorProcessAdapter {
    /// 新しい色検知処理アダプタを作成
    /// 
    /// # Arguments
    /// - `min_detection_area`: 最小検出面積（ピクセル）
    /// - `detection_method`: 検出方法（moments/boundingbox）
    /// 
    /// # Returns
    /// ColorProcessAdapterインスタンス
    pub fn new(min_detection_area: u32, detection_method: DetectionMethod) -> DomainResult<Self> {
        // OpenCVのスレッド数を設定（0 = 自動、全コア使用）
        // cvtColor、moments等の一部関数で並列化が有効
        let _ = opencv::core::set_num_threads(0);
        
        #[cfg(debug_assertions)]
        {
            let num_threads = opencv::core::get_num_threads().unwrap_or(1);
            tracing::info!(
                "Color process adapter initialized with OpenCV (CPU/Mat, {} threads)",
                num_threads
            );
        }

        Ok(Self {
            min_detection_area,
            detection_method,
        })
    }

    /// フレームデータをMatに変換（CPU処理用）
    /// 
    /// # Arguments
    /// - `frame`: キャプチャされたフレーム（BGRA形式）
    /// 
    /// # Returns
    /// BGR形式のMat
    /// 
    /// # 低レイテンシ最適化
    /// - ゼロコピー戦略: frame.dataから直接Matを作成（shallow copy）
    /// - メモリコピーは1回のみ（BGRA→BGR変換時）
    /// - 中間バッファを使用しない
    fn frame_to_mat(&self, frame: &Frame) -> DomainResult<Mat> {
        use opencv::core::CV_8UC4;
        
        let rows = frame.height as i32;
        let cols = frame.width as i32;
        let step = (frame.width * 4) as usize; // BGRA = 4 bytes per pixel
        
        // BGRA（4チャンネル）データから直接Matを作成（ゼロコピー）
        // SAFETY: frame.dataは有効なBGRAデータを含み、サイズは width * height * 4
        // Matはframe.dataへの参照のみ保持（shallow copy）するため、
        // frameのライフタイムがこの関数スコープ内で保証される
        let bgra_mat = unsafe {
            Mat::new_rows_cols_with_data_unsafe(
                rows,
                cols,
                CV_8UC4,
                frame.data.as_ptr() as *mut std::ffi::c_void,
                step,
            )
            .map_err(|e| DomainError::Process(format!("Failed to create BGRA Mat: {:?}", e)))?
        };

        // BGRA → BGR変換（4チャンネル → 3チャンネル）
        // この時点でメモリコピーが1回発生（deep copy）
        let mut bgr_mat = Mat::default();
        imgproc::cvt_color_def(&bgra_mat, &mut bgr_mat, imgproc::COLOR_BGRA2BGR)
            .map_err(|e| DomainError::Process(format!("Failed to convert BGRA to BGR: {:?}", e)))?;

        Ok(bgr_mat)
    }



    /// HSVマスク処理（Mat版）
    /// 
    /// 低レイテンシのために以下の最適化を実施:
    /// - OpenCVの並列処理を活用（cvtColor、moments）
    /// - 条件付きパフォーマンスログ（performance-timing feature）
    fn process_with_mat(
        &self,
        bgr: &Mat,
        hsv_range: &HsvRange,
    ) -> DomainResult<DetectionResult> {
        #[cfg(feature = "performance-timing")]
        let start = Instant::now();
        
        // BGR → HSV変換
        let mut hsv = Mat::default();
        imgproc::cvt_color_def(bgr, &mut hsv, imgproc::COLOR_BGR2HSV)
            .map_err(|e| DomainError::Process(format!("Failed to convert BGR to HSV: {:?}", e)))?;

        #[cfg(feature = "performance-timing")]
        let hsv_time = start.elapsed();

        // HSVレンジでマスク生成
        let lower = Scalar::new(hsv_range.h_min as f64, hsv_range.s_min as f64, hsv_range.v_min as f64, 0.0);
        let upper = Scalar::new(hsv_range.h_max as f64, hsv_range.s_max as f64, hsv_range.v_max as f64, 0.0);
        
        let mut mask = Mat::default();
        core::in_range(&hsv, &lower, &upper, &mut mask)
            .map_err(|e| DomainError::Process(format!("Failed to create mask: {:?}", e)))?;

        #[cfg(feature = "performance-timing")]
        let mask_time = hsv_time.elapsed();

        // 検出方法に応じて処理を分岐
        let result = match self.detection_method {
            DetectionMethod::Moments => self.calculate_moments(&mask)?,
            DetectionMethod::BoundingBox => self.calculate_bounding_box(&mask)?,
        };

        // デバッグ表示：画像処理の中間結果を表示
        #[cfg(feature = "opencv-debug-display")]
        {
            self.display_debug_images(bgr, &hsv, &mask, hsv_range, &result)?;
        }
        
        #[cfg(feature = "performance-timing")]
        {
            let detection_time = mask_time.elapsed();
            let total_time = start.elapsed();
            tracing::debug!(
                "Color process breakdown - HSV: {:.2}ms | Mask: {:.2}ms | Detection ({}): {:.2}ms | Total: {:.2}ms",
                hsv_time.as_secs_f64() * 1000.0,
                mask_time.as_secs_f64() * 1000.0,
                match self.detection_method {
                    DetectionMethod::Moments => "moments",
                    DetectionMethod::BoundingBox => "bbox",
                },
                detection_time.as_secs_f64() * 1000.0,
                total_time.as_secs_f64() * 1000.0
            );
        }
        
        Ok(result)
    }

    /// デバッグ用：画像処理の中間結果を表示（拡張版）
    /// 
    /// opencv-debug-display featureが有効な場合のみコンパイルされます。
    /// BGR、マスク画像、デバッグ情報ウィンドウを表示します。
    /// 
    /// # 操作方法
    /// - ESCキーまたは'q'キー: 終了
    /// - その他: 継続（約30fps表示）
    #[cfg(feature = "opencv-debug-display")]
    fn display_debug_images(
        &self,
        bgr: &Mat,
        _hsv: &Mat,
        mask: &Mat,
        hsv_range: &HsvRange,
        detection: &DetectionResult,
    ) -> DomainResult<()> {
        use opencv::highgui;
        use opencv::prelude::MatTraitConst;

        // BGR画像に検出マーカー（十字と円）のみ描画
        let mut bgr_display = bgr.clone();
        self.draw_detection_markers(&mut bgr_display, detection)?;

        // デバッグ情報専用ウィンドウを作成
        let info_window = self.create_info_window(hsv_range, detection, bgr.cols(), bgr.rows())?;

        // ウィンドウを作成（初回のみ）
        // WINDOW_AUTOSIZEで等倍表示（リサイズ不可）
        let _ = highgui::named_window("Debug: BGR Capture", highgui::WINDOW_AUTOSIZE);
        let _ = highgui::named_window("Debug: Mask", highgui::WINDOW_AUTOSIZE);
        let _ = highgui::named_window("Debug: Info", highgui::WINDOW_AUTOSIZE);

        // 画像を表示
        highgui::imshow("Debug: BGR Capture", &bgr_display)
            .map_err(|e| DomainError::Process(format!("Failed to show BGR image: {:?}", e)))?;
        highgui::imshow("Debug: Mask", mask)
            .map_err(|e| DomainError::Process(format!("Failed to show Mask image: {:?}", e)))?;
        highgui::imshow("Debug: Info", &info_window)
            .map_err(|e| DomainError::Process(format!("Failed to show Info window: {:?}", e)))?;

        // キー入力を待つ（30ms待機 = 約33fps、ユーザーが画像を確認しやすい速度）
        // ESCキー(27)、'q'(113)で終了
        let key = highgui::wait_key(30)
            .map_err(|e| DomainError::Process(format!("Failed to wait for key: {:?}", e)))?;
        
        if key == 27 || key == 113 { // ESC or 'q'
            tracing::info!("Debug display: User requested exit (ESC or 'q' pressed)");
            // ウィンドウを破棄
            let _ = highgui::destroy_all_windows();
            // プログラム全体を終了
            std::process::exit(0);
        }

        Ok(())
    }

    /// デバッグ情報専用ウィンドウを作成
    #[cfg(feature = "opencv-debug-display")]
    fn create_info_window(
        &self,
        hsv_range: &HsvRange,
        detection: &DetectionResult,
        img_width: i32,
        img_height: i32,
    ) -> DomainResult<Mat> {
        use opencv::imgproc::{self, FONT_HERSHEY_SIMPLEX, LINE_8};
        use opencv::core::{Point, Scalar};

        // 固定サイズのウィンドウ（幅400px、高さは内容に応じて調整）
        let window_width = 400;
        let window_height = 300;
        
        // 黒背景のMatを作成
        let mut info_img = Mat::new_rows_cols_with_default(
            window_height,
            window_width,
            opencv::core::CV_8UC3,
            Scalar::new(0.0, 0.0, 0.0, 0.0),
        ).map_err(|e| DomainError::Process(format!("Failed to create info window: {:?}", e)))?;

        let font_scale = 0.6;
        let thickness = 1;
        let white = Scalar::new(255.0, 255.0, 255.0, 0.0);
        let green = Scalar::new(0.0, 255.0, 0.0, 0.0);
        let red = Scalar::new(0.0, 0.0, 255.0, 0.0);
        let yellow = Scalar::new(0.0, 255.0, 255.0, 0.0);
        
        let mut y = 30;
        let line_height = 25;

        // タイトル
        imgproc::put_text(
            &mut info_img,
            "=== Detection Info ===",
            Point::new(20, y),
            FONT_HERSHEY_SIMPLEX,
            0.7,
            yellow,
            2,
            LINE_8,
            false,
        ).map_err(|e| DomainError::Process(format!("Failed to draw text: {:?}", e)))?;
        y += line_height + 5;

        // ROIサイズ
        let size_text = format!("ROI Size: {}x{} px", img_width, img_height);
        imgproc::put_text(
            &mut info_img,
            &size_text,
            Point::new(20, y),
            FONT_HERSHEY_SIMPLEX,
            font_scale,
            white,
            thickness,
            LINE_8,
            false,
        ).map_err(|e| DomainError::Process(format!("Failed to draw text: {:?}", e)))?;
        y += line_height;

        // HSV設定
        let hsv_text = format!("HSV Range:");
        imgproc::put_text(
            &mut info_img,
            &hsv_text,
            Point::new(20, y),
            FONT_HERSHEY_SIMPLEX,
            font_scale,
            white,
            thickness,
            LINE_8,
            false,
        ).map_err(|e| DomainError::Process(format!("Failed to draw text: {:?}", e)))?;
        y += line_height;

        let hsv_detail = format!("  H: [{:3} - {:3}]", hsv_range.h_min, hsv_range.h_max);
        imgproc::put_text(
            &mut info_img,
            &hsv_detail,
            Point::new(20, y),
            FONT_HERSHEY_SIMPLEX,
            font_scale,
            white,
            thickness,
            LINE_8,
            false,
        ).map_err(|e| DomainError::Process(format!("Failed to draw text: {:?}", e)))?;
        y += line_height;

        let hsv_detail = format!("  S: [{:3} - {:3}]", hsv_range.s_min, hsv_range.s_max);
        imgproc::put_text(
            &mut info_img,
            &hsv_detail,
            Point::new(20, y),
            FONT_HERSHEY_SIMPLEX,
            font_scale,
            white,
            thickness,
            LINE_8,
            false,
        ).map_err(|e| DomainError::Process(format!("Failed to draw text: {:?}", e)))?;
        y += line_height;

        let hsv_detail = format!("  V: [{:3} - {:3}]", hsv_range.v_min, hsv_range.v_max);
        imgproc::put_text(
            &mut info_img,
            &hsv_detail,
            Point::new(20, y),
            FONT_HERSHEY_SIMPLEX,
            font_scale,
            white,
            thickness,
            LINE_8,
            false,
        ).map_err(|e| DomainError::Process(format!("Failed to draw text: {:?}", e)))?;
        y += line_height + 5;

        // 検出状態
        let (status_text, status_color) = if detection.detected {
            ("Status: DETECTED", green)
        } else {
            ("Status: NOT DETECTED", red)
        };
        imgproc::put_text(
            &mut info_img,
            status_text,
            Point::new(20, y),
            FONT_HERSHEY_SIMPLEX,
            0.7,
            status_color,
            2,
            LINE_8,
            false,
        ).map_err(|e| DomainError::Process(format!("Failed to draw text: {:?}", e)))?;
        y += line_height + 5;

        // 検出面積
        let area_text = format!("Coverage: {} px", detection.coverage);
        imgproc::put_text(
            &mut info_img,
            &area_text,
            Point::new(20, y),
            FONT_HERSHEY_SIMPLEX,
            font_scale,
            white,
            thickness,
            LINE_8,
            false,
        ).map_err(|e| DomainError::Process(format!("Failed to draw text: {:?}", e)))?;
        y += line_height;

        let min_area_text = format!("Min Area: {} px", self.min_detection_area);
        imgproc::put_text(
            &mut info_img,
            &min_area_text,
            Point::new(20, y),
            FONT_HERSHEY_SIMPLEX,
            font_scale,
            white,
            thickness,
            LINE_8,
            false,
        ).map_err(|e| DomainError::Process(format!("Failed to draw text: {:?}", e)))?;
        y += line_height + 5;

        // 重心座標
        if detection.detected {
            let center_text = format!("Center: ({:.1}, {:.1})", 
                detection.center_x, 
                detection.center_y);
            imgproc::put_text(
                &mut info_img,
                &center_text,
                Point::new(20, y),
                FONT_HERSHEY_SIMPLEX,
                font_scale,
                white,
                thickness,
                LINE_8,
                false,
            ).map_err(|e| DomainError::Process(format!("Failed to draw text: {:?}", e)))?;
            y += line_height;
        }

        y += 10;

        // 操作方法
        imgproc::put_text(
            &mut info_img,
            "Press ESC or 'q' to quit",
            Point::new(20, y),
            FONT_HERSHEY_SIMPLEX,
            0.5,
            white,
            thickness,
            LINE_8,
            false,
        ).map_err(|e| DomainError::Process(format!("Failed to draw text: {:?}", e)))?;

        Ok(info_img)
    }

    /// 検出マーカー（十字と円）を描画
    #[cfg(feature = "opencv-debug-display")]
    fn draw_detection_markers(
        &self,
        img: &mut Mat,
        detection: &DetectionResult,
    ) -> DomainResult<()> {
        use opencv::imgproc::{self, LINE_8};
        use opencv::core::{Point, Scalar};

        if !detection.detected {
            return Ok(());
        }

        let green = Scalar::new(0.0, 255.0, 0.0, 0.0);
        let center_point = Point::new(
            detection.center_x as i32,
            detection.center_y as i32,
        );
        let marker_size = 10;
        
        // 縦線
        imgproc::line(
            img,
            Point::new(center_point.x, center_point.y - marker_size),
            Point::new(center_point.x, center_point.y + marker_size),
            green,
            2,
            LINE_8,
            0,
        ).map_err(|e| DomainError::Process(format!("Failed to draw line: {:?}", e)))?;
        
        // 横線
        imgproc::line(
            img,
            Point::new(center_point.x - marker_size, center_point.y),
            Point::new(center_point.x + marker_size, center_point.y),
            green,
            2,
            LINE_8,
            0,
        ).map_err(|e| DomainError::Process(format!("Failed to draw line: {:?}", e)))?;

        // 重心周りに円を描画
        imgproc::circle(
            img,
            center_point,
            5,
            green,
            2,
            LINE_8,
            0,
        ).map_err(|e| DomainError::Process(format!("Failed to draw circle: {:?}", e)))?;

        Ok(())
    }



    /// モーメントから検出結果を計算（内部ヘルパー）
    fn calculate_detection_from_moments(&self, moments: &opencv::core::Moments) -> DetectionResult {
        let m00 = moments.m00;
        let coverage = m00 as u32;

        // 最小検出面積チェック
        if coverage <= self.min_detection_area {
            return DetectionResult::none();
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

        DetectionResult {
            timestamp: Instant::now(),
            detected: true,
            center_x,
            center_y,
            coverage,
        }
    }

    /// モーメント計算から重心と面積を取得
    fn calculate_moments(&self, mask: &Mat) -> DomainResult<DetectionResult> {
        let moments = imgproc::moments(mask, true)
            .map_err(|e| DomainError::Process(format!("Failed to calculate moments: {:?}", e)))?;
        
        Ok(self.calculate_detection_from_moments(&moments))
    }

    /// バウンディングボックス計算から中心と面積を取得
    /// 
    /// # レイテンシ特性
    /// - momentsに比べて計算が単純（重心計算なし）
    /// - findContoursはOpenCVの並列処理を活用
    fn calculate_bounding_box(&self, mask: &Mat) -> DomainResult<DetectionResult> {
        use opencv::core::Vector;

        // 輪郭検出
        let mut contours: Vector<Vector<opencv::core::Point>> = Vector::new();
        imgproc::find_contours(
            mask,
            &mut contours,
            imgproc::RETR_EXTERNAL,
            imgproc::CHAIN_APPROX_SIMPLE,
            opencv::core::Point::new(0, 0),
        )
        .map_err(|e| DomainError::Process(format!("Failed to find contours: {:?}", e)))?;

        // 輪郭がない場合
        if contours.is_empty() {
            return Ok(DetectionResult::none());
        }

        // 最大面積の輪郭を探す
        let mut max_area = 0.0;
        let mut max_contour_idx = 0;
        for i in 0..contours.len() {
            let area = imgproc::contour_area(&contours.get(i).unwrap(), false)
                .map_err(|e| DomainError::Process(format!("Failed to calculate contour area: {:?}", e)))?;
            if area > max_area {
                max_area = area;
                max_contour_idx = i;
            }
        }

        let coverage = max_area as u32;

        // 最小検出面積チェック
        if coverage <= self.min_detection_area {
            return Ok(DetectionResult::none());
        }

        // バウンディングボックスを計算
        let rect = imgproc::bounding_rect(&contours.get(max_contour_idx).unwrap())
            .map_err(|e| DomainError::Process(format!("Failed to calculate bounding rect: {:?}", e)))?;

        // 中心座標を計算
        let center_x = rect.x as f32 + rect.width as f32 / 2.0;
        let center_y = rect.y as f32 + rect.height as f32 / 2.0;

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
        
        #[cfg(feature = "performance-timing")]
        let start = Instant::now();
        
        let mat = self.frame_to_mat(frame)?;
        
        #[cfg(feature = "performance-timing")]
        let frame_to_mat_time = start.elapsed();
        
        let result = self.process_with_mat(&mat, hsv_range)?;
        
        #[cfg(feature = "performance-timing")]
        {
            let processing_time = frame_to_mat_time.elapsed();
            let total_time = start.elapsed();
            tracing::debug!(
                "Frame processing - Mat conversion: {:.2}ms | Color detection: {:.2}ms | Total: {:.2}ms ({}x{} px)",
                frame_to_mat_time.as_secs_f64() * 1000.0,
                processing_time.as_secs_f64() * 1000.0,
                total_time.as_secs_f64() * 1000.0,
                frame.width,
                frame.height
            );
        }
        
        Ok(result)
    }

    fn backend(&self) -> ProcessorBackend {
        ProcessorBackend::Cpu
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// テスト用のダミーフレームを作成
    fn create_test_frame(width: usize, height: usize) -> Frame {
        // 黄色のBGRA画像（B=0, G=255, R=255, A=255）
        let size = width * height * 4;
        let mut data = vec![0u8; size];
        
        // 中心部分を黄色で塗りつぶす
        let center_x = width / 2;
        let center_y = height / 2;
        let radius = 50;
        
        for y in 0..height {
            for x in 0..width {
                let dx = x as i32 - center_x as i32;
                let dy = y as i32 - center_y as i32;
                
                if dx * dx + dy * dy < radius * radius {
                    let idx = (y * width + x) * 4;
                    data[idx] = 0;      // B
                    data[idx + 1] = 255; // G
                    data[idx + 2] = 255; // R
                    data[idx + 3] = 255; // A
                }
            }
        }
        
        Frame {
            data,
            width: width as u32,
            height: height as u32,
            timestamp: Instant::now(),
            dirty_rects: vec![],
        }
    }

    #[test]
    fn test_adapter_creation() {
        let adapter = ColorProcessAdapter::new(100, DetectionMethod::Moments);
        assert!(adapter.is_ok());
        let adapter = adapter.unwrap();
        assert_eq!(adapter.min_detection_area, 100);
    }

    #[test]
    fn test_backend() {
        let adapter = ColorProcessAdapter::new(100, DetectionMethod::Moments).unwrap();
        assert_eq!(adapter.backend(), ProcessorBackend::Cpu);
    }

    #[test]
    fn test_process_frame_with_detection() {
        let mut adapter = ColorProcessAdapter::new(100, DetectionMethod::Moments).unwrap();
        let frame = create_test_frame(640, 480);
        let roi = Roi::new(0, 0, 640, 480);
        
        // 黄色を検出するHSV範囲
        let hsv_range = HsvRange {
            h_min: 20,
            h_max: 40,
            s_min: 100,
            s_max: 255,
            v_min: 100,
            v_max: 255,
        };

        let result = adapter.process_frame(&frame, &roi, &hsv_range);
        assert!(result.is_ok());
        
        let detection = result.unwrap();
        assert!(detection.detected, "Should detect yellow color");
        assert!(detection.coverage >= 100, "Coverage should be at least min_detection_area");
        
        // 重心が中心付近にあることを確認（誤差を考慮）
        let center_x = 640.0 / 2.0;
        let center_y = 480.0 / 2.0;
        assert!((detection.center_x - center_x).abs() < 50.0, 
            "Center X should be near frame center: expected {}, got {}", 
            center_x, detection.center_x);
        assert!((detection.center_y - center_y).abs() < 50.0,
            "Center Y should be near frame center: expected {}, got {}",
            center_y, detection.center_y);
    }

    #[test]
    fn test_process_frame_no_detection() {
        let mut adapter = ColorProcessAdapter::new(100, DetectionMethod::Moments).unwrap();
        
        // 黒いフレーム（検出なし）
        let frame = Frame {
            data: vec![0u8; 640 * 480 * 4],
            width: 640,
            height: 480,
            timestamp: Instant::now(),
            dirty_rects: vec![],
        };
        
        let roi = Roi::new(0, 0, 640, 480);
        
        // 黄色を検出するHSV範囲
        let hsv_range = HsvRange {
            h_min: 20,
            h_max: 40,
            s_min: 100,
            s_max: 255,
            v_min: 100,
            v_max: 255,
        };

        let result = adapter.process_frame(&frame, &roi, &hsv_range);
        assert!(result.is_ok());
        
        let detection = result.unwrap();
        assert!(!detection.detected, "Should not detect anything in black frame");
        assert_eq!(detection.coverage, 0, "Coverage should be 0");
    }

    #[test]
    fn test_process_frame_small_area() {
        // 検出はされるが、カバレッジが記録されることを確認
        let mut adapter = ColorProcessAdapter::new(100, DetectionMethod::Moments).unwrap(); // 低い閾値で検出
        let frame = create_test_frame(640, 480);
        let roi = Roi::new(0, 0, 640, 480);
        
        // 黄色を検出するHSV範囲
        let hsv_range = HsvRange {
            h_min: 20,
            h_max: 40,
            s_min: 100,
            s_max: 255,
            v_min: 100,
            v_max: 255,
        };

        let result = adapter.process_frame(&frame, &roi, &hsv_range);
        assert!(result.is_ok());
        
        let detection = result.unwrap();
        // 検出されることを確認
        assert!(detection.detected, "Should detect yellow color");
        // カバレッジが記録されていることを確認（0より大きい）
        assert!(detection.coverage > 0, "Coverage should be greater than 0");
    }

    #[test]
    fn test_frame_to_mat_conversion() {
        let adapter = ColorProcessAdapter::new(100, DetectionMethod::Moments).unwrap();
        let frame = create_test_frame(320, 240);
        
        let result = adapter.frame_to_mat(&frame);
        if let Err(e) = &result {
            eprintln!("Error in frame_to_mat: {:?}", e);
        }
        assert!(result.is_ok(), "Frame to Mat conversion should succeed");
        
        let mat = result.unwrap();
        assert_eq!(mat.rows(), 240);
        assert_eq!(mat.cols(), 320);
    }

    #[test]
    fn test_calculate_moments_empty_mask() {
        let adapter = ColorProcessAdapter::new(100, DetectionMethod::Moments).unwrap();
        
        // 空のマスク（全て0）
        let mask = Mat::zeros(100, 100, opencv::core::CV_8UC1)
            .unwrap()
            .to_mat()
            .unwrap();
        
        let result = adapter.calculate_moments(&mask);
        assert!(result.is_ok());
        
        let detection = result.unwrap();
        assert!(!detection.detected);
        assert_eq!(detection.coverage, 0);
        assert_eq!(detection.center_x, 0.0);
        assert_eq!(detection.center_y, 0.0);
    }

    #[test]
    fn test_bounding_box_detection() {
        // BoundingBox検出メソッドのテスト
        let mut adapter = ColorProcessAdapter::new(100, DetectionMethod::BoundingBox).unwrap();
        let frame = create_test_frame(640, 480);
        let roi = Roi::new(0, 0, 640, 480);
        
        // 黄色を検出するHSV範囲
        let hsv_range = HsvRange {
            h_min: 20,
            h_max: 40,
            s_min: 100,
            s_max: 255,
            v_min: 100,
            v_max: 255,
        };

        let result = adapter.process_frame(&frame, &roi, &hsv_range);
        assert!(result.is_ok());
        
        let detection = result.unwrap();
        assert!(detection.detected, "Should detect yellow color with BoundingBox method");
        assert!(detection.coverage > 0, "Coverage should be greater than 0");
        
        // バウンディングボックスの中心はフレームの中心付近であるべき
        let center_x = frame.width as f32 / 2.0;
        let center_y = frame.height as f32 / 2.0;
        assert!((detection.center_x - center_x).abs() < 100.0,
            "Center X should be near frame center with BoundingBox: expected {}, got {}",
            center_x, detection.center_x);
        assert!((detection.center_y - center_y).abs() < 100.0,
            "Center Y should be near frame center with BoundingBox: expected {}, got {}",
            center_y, detection.center_y);
    }

    #[test]
    fn test_bounding_box_empty_mask() {
        // BoundingBoxで空のマスクをテスト
        let adapter = ColorProcessAdapter::new(100, DetectionMethod::BoundingBox).unwrap();
        
        // 空のマスク（全て0）
        let mask = Mat::zeros(100, 100, opencv::core::CV_8UC1)
            .unwrap()
            .to_mat()
            .unwrap();
        
        let result = adapter.calculate_bounding_box(&mask);
        assert!(result.is_ok());
        
        let detection = result.unwrap();
        assert!(!detection.detected);
    }
}
