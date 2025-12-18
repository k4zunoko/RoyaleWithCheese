/// デバッグ表示モジュール
/// 
/// OpenCVを使用した視覚的デバッグ機能。
/// `opencv-debug-display` featureが有効な場合のみコンパイルされます。
///
/// このモジュールは開発・調整段階での設定確認用であり、
/// Release buildでは完全に除外されるため、実行時のレイテンシには影響しません。

use crate::domain::{DetectionResult, DomainError, DomainResult, HsvRange};
use opencv::{
    core::{Mat, Point, Rect, Scalar},
    highgui,
    imgproc::{self, FONT_HERSHEY_SIMPLEX, LINE_8},
    prelude::MatTraitConst,
};

/// デバッグ用：画像処理の中間結果を表示（拡張版）
/// 
/// BGR、マスク画像、デバッグ情報ウィンドウを表示します。
/// 
/// # Arguments
/// - `bgr`: BGR形式の元画像
/// - `_hsv`: HSV形式の画像（将来の拡張用に予約）
/// - `mask`: 2値化マスク画像
/// - `hsv_range`: HSV検出範囲
/// - `detection`: 検出結果
/// - `min_detection_area`: 最小検出面積
/// 
/// # 操作方法
/// - ESCキーまたは'q'キー: 終了
/// - その他: 継続（約30fps表示）
pub(crate) fn display_debug_images(
    bgr: &Mat,
    _hsv: &Mat,
    mask: &Mat,
    hsv_range: &HsvRange,
    detection: &DetectionResult,
    min_detection_area: u32,
) -> DomainResult<()> {
    // BGR画像に検出マーカー（十字と円）のみ描画
    let mut bgr_display = bgr.clone();
    draw_detection_markers(&mut bgr_display, detection)?;

    // デバッグ情報専用ウィンドウを作成
    let info_window = create_info_window(hsv_range, detection, bgr.cols(), bgr.rows(), min_detection_area)?;

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

    const DEBUG_DISPLAY_WAIT_MS: i32 = 30; // 約33fps
    const KEY_ESC: i32 = 27;
    const KEY_Q: i32 = 113;
    
    // キー入力を待つ（ユーザーが画像を確認しやすい速度）
    let key = highgui::wait_key(DEBUG_DISPLAY_WAIT_MS)
        .map_err(|e| DomainError::Process(format!("Failed to wait for key: {:?}", e)))?;
    
    if key == KEY_ESC || key == KEY_Q {
        tracing::info!("Debug display: User requested exit (ESC or 'q' pressed)");
        // ウィンドウを破棄
        let _ = highgui::destroy_all_windows();
        // プログラム全体を終了
        std::process::exit(0);
    }

    Ok(())
}

/// デバッグ情報専用ウィンドウを作成
fn create_info_window(
    hsv_range: &HsvRange,
    detection: &DetectionResult,
    img_width: i32,
    img_height: i32,
    min_detection_area: u32,
) -> DomainResult<Mat> {
    // 固定サイズのウィンドウ（幅400px、高さは内容に応じて調整）
    let window_width = 400;
    let window_height = 350; // BoundingBox情報追加のため高さを増やす
    
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

    let min_area_text = format!("Min Area: {} px", min_detection_area);
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

    // 検出方法
    if detection.detected {
        let method_text = if detection.bounding_box.is_some() {
            "Method: BoundingBox"
        } else {
            "Method: Moments"
        };
        imgproc::put_text(
            &mut info_img,
            method_text,
            Point::new(20, y),
            FONT_HERSHEY_SIMPLEX,
            font_scale,
            yellow,
            thickness,
            LINE_8,
            false,
        ).map_err(|e| DomainError::Process(format!("Failed to draw text: {:?}", e)))?;
        y += line_height;
    }

    // 中心座標
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

    // BoundingBox情報
    if let Some(bbox) = &detection.bounding_box {
        let bbox_text = format!("BBox: ({:.1}, {:.1})", bbox.x, bbox.y);
        imgproc::put_text(
            &mut info_img,
            &bbox_text,
            Point::new(20, y),
            FONT_HERSHEY_SIMPLEX,
            font_scale,
            white,
            thickness,
            LINE_8,
            false,
        ).map_err(|e| DomainError::Process(format!("Failed to draw text: {:?}", e)))?;
        y += line_height;

        let size_text = format!("Size: {:.1} x {:.1}", bbox.width, bbox.height);
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

/// 検出マーカーを描画
/// 
/// - Moments検出時: 重心に十字と円を描画（緑色）
/// - BoundingBox検出時: 矩形と中心に十字を描画（青色の矩形、緑色の十字）
fn draw_detection_markers(
    img: &mut Mat,
    detection: &DetectionResult,
) -> DomainResult<()> {
    if !detection.detected {
        return Ok(());
    }

    let green = Scalar::new(0.0, 255.0, 0.0, 0.0);
    let blue = Scalar::new(255.0, 0.0, 0.0, 0.0);
    let center_point = Point::new(
        detection.center_x as i32,
        detection.center_y as i32,
    );

    // BoundingBox情報がある場合は矩形を描画
    if let Some(bbox) = &detection.bounding_box {
        // 矩形を描画（青色、太線）
        let rect = Rect::new(
            bbox.x as i32,
            bbox.y as i32,
            bbox.width as i32,
            bbox.height as i32,
        );
        imgproc::rectangle(
            img,
            rect,
            blue,
            2,
            LINE_8,
            0,
        ).map_err(|e| DomainError::Process(format!("Failed to draw rectangle: {:?}", e)))?;

        // 中心に十字マーカーを描画（緑色）
        let marker_size = 8;
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
    } else {
        // Moments検出時: 重心に十字と円を描画（緑色）
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
    }

    Ok(())
}
