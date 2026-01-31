//! GPU統合テスト
//!
//! GPU処理パイプラインのend-to-endテスト。
//! 注意: これらのテストはGPUを必要とするため、CI環境では無視されます。

use std::time::Instant;
use RoyaleWithCheese::domain::{
    config::DetectionMethod,
    ports::ProcessPort,
    types::{Frame, HsvRange, Roi},
};
use RoyaleWithCheese::infrastructure::{
    gpu_device::create_d3d11_device,
    process_selector::ProcessSelector,
    processing::{ColorProcessAdapter, GpuColorAdapter},
};

/// GPUデバイスが利用可能かチェック
fn gpu_available() -> bool {
    create_d3d11_device().is_ok()
}

/// テスト用の黄色フレームを作成
fn create_yellow_frame(width: u32, height: u32) -> Frame {
    let size = (width * height * 4) as usize;
    let mut data = vec![0u8; size];

    let center_x = width / 2;
    let center_y = height / 2;
    let radius = 50;

    for y in 0..height {
        for x in 0..width {
            let dx = x as i32 - center_x as i32;
            let dy = y as i32 - center_y as i32;

            if dx * dx + dy * dy < radius * radius {
                let idx = ((y * width + x) * 4) as usize;
                data[idx] = 0; // B
                data[idx + 1] = 255; // G
                data[idx + 2] = 255; // R
                data[idx + 3] = 255; // A
            }
        }
    }

    Frame {
        data,
        width,
        height,
        timestamp: Instant::now(),
        dirty_rects: vec![],
    }
}

#[test]
#[ignore = "Requires GPU"]
fn test_gpu_process_selector_creation() {
    if !gpu_available() {
        println!("Skipping test: No GPU available");
        return;
    }

    let (device, context) = create_d3d11_device().expect("Failed to create D3D11 device");
    let gpu_adapter = GpuColorAdapter::with_device_context(device, context)
        .expect("Failed to create GPU adapter");

    let selector = ProcessSelector::new_gpu(gpu_adapter);

    assert!(selector.is_gpu());
    assert!(!selector.is_cpu());
    assert_eq!(selector.backend_type(), "GPU (D3D11 Compute Shader)");
}

#[test]
#[ignore = "Requires GPU"]
fn test_cpu_process_selector_creation() {
    let cpu_adapter = ColorProcessAdapter::new(100, DetectionMethod::Moments)
        .expect("Failed to create CPU adapter");

    let selector = ProcessSelector::new_cpu(cpu_adapter);

    assert!(!selector.is_gpu());
    assert!(selector.is_cpu());
    assert_eq!(selector.backend_type(), "CPU (OpenCV)");
}

#[test]
#[ignore = "Requires GPU"]
fn test_gpu_processes_frame() {
    if !gpu_available() {
        println!("Skipping test: No GPU available");
        return;
    }

    let (device, context) = create_d3d11_device().expect("Failed to create D3D11 device");
    let mut gpu_adapter = GpuColorAdapter::with_device_context(device, context)
        .expect("Failed to create GPU adapter");

    let frame = create_yellow_frame(640, 480);
    let roi = Roi::new(0, 0, 640, 480);
    let hsv_range = HsvRange::new(20, 40, 100, 255, 100, 255);

    let result = gpu_adapter
        .process_frame(&frame, &roi, &hsv_range)
        .expect("GPU processing failed");

    assert!(result.detected, "Should detect yellow color");
    assert!(result.coverage > 0, "Coverage should be > 0");

    // Center should be near frame center (with some tolerance)
    let expected_center_x = 320.0;
    let expected_center_y = 240.0;
    let tolerance = 50.0;

    assert!(
        (result.center_x - expected_center_x).abs() < tolerance,
        "Center X should be near frame center: expected ~{}, got {}",
        expected_center_x,
        result.center_x
    );
    assert!(
        (result.center_y - expected_center_y).abs() < tolerance,
        "Center Y should be near frame center: expected ~{}, got {}",
        expected_center_y,
        result.center_y
    );
}

#[test]
#[ignore = "Requires GPU"]
fn test_cpu_gpu_process_comparison() {
    if !gpu_available() {
        println!("Skipping test: No GPU available");
        return;
    }

    let frame = create_yellow_frame(640, 480);
    let roi = Roi::new(0, 0, 640, 480);
    let hsv_range = HsvRange::new(20, 40, 100, 255, 100, 255);

    // CPU processing
    let mut cpu_adapter = ColorProcessAdapter::new(100, DetectionMethod::Moments)
        .expect("Failed to create CPU adapter");
    let cpu_result = cpu_adapter
        .process_frame(&frame, &roi, &hsv_range)
        .expect("CPU processing failed");

    // GPU processing
    let (device, context) = create_d3d11_device().expect("Failed to create D3D11 device");
    let mut gpu_adapter = GpuColorAdapter::with_device_context(device, context)
        .expect("Failed to create GPU adapter");
    let gpu_result = gpu_adapter
        .process_frame(&frame, &roi, &hsv_range)
        .expect("GPU processing failed");

    // Both should detect
    assert!(cpu_result.detected, "CPU should detect yellow color");
    assert!(gpu_result.detected, "GPU should detect yellow color");

    // Results should be similar (not exact due to different algorithms)
    let center_tolerance = 20.0;
    assert!(
        (cpu_result.center_x - gpu_result.center_x).abs() < center_tolerance,
        "CPU and GPU center X should be similar: CPU={}, GPU={}",
        cpu_result.center_x,
        gpu_result.center_x
    );
    assert!(
        (cpu_result.center_y - gpu_result.center_y).abs() < center_tolerance,
        "CPU and GPU center Y should be similar: CPU={}, GPU={}",
        cpu_result.center_y,
        gpu_result.center_y
    );

    println!(
        "CPU result: center=({:.1}, {:.1}), coverage={}",
        cpu_result.center_x, cpu_result.center_y, cpu_result.coverage
    );
    println!(
        "GPU result: center=({:.1}, {:.1}), coverage={}",
        gpu_result.center_x, gpu_result.center_y, gpu_result.coverage
    );
}

#[test]
#[ignore = "Requires GPU"]
fn test_process_selector_enum_dispatch() {
    if !gpu_available() {
        println!("Skipping test: No GPU available");
        return;
    }

    let frame = create_yellow_frame(640, 480);
    let roi = Roi::new(0, 0, 640, 480);
    let hsv_range = HsvRange::new(20, 40, 100, 255, 100, 255);

    // Test GPU variant
    let (device, context) = create_d3d11_device().expect("Failed to create D3D11 device");
    let gpu_adapter = GpuColorAdapter::with_device_context(device, context)
        .expect("Failed to create GPU adapter");
    let mut gpu_selector = ProcessSelector::new_gpu(gpu_adapter);

    let gpu_result = gpu_selector
        .process_frame(&frame, &roi, &hsv_range)
        .expect("ProcessSelector GPU processing failed");
    assert!(gpu_result.detected);

    // Test CPU variant
    let cpu_adapter = ColorProcessAdapter::new(100, DetectionMethod::Moments)
        .expect("Failed to create CPU adapter");
    let mut cpu_selector = ProcessSelector::new_cpu(cpu_adapter);

    let cpu_result = cpu_selector
        .process_frame(&frame, &roi, &hsv_range)
        .expect("ProcessSelector CPU processing failed");
    assert!(cpu_result.detected);
}

#[test]
fn test_fallback_when_gpu_unavailable() {
    // This test simulates the fallback behavior when GPU is not available
    // We can't easily simulate GPU failure, but we can verify CPU fallback works

    let cpu_adapter = ColorProcessAdapter::new(100, DetectionMethod::Moments)
        .expect("Failed to create CPU adapter");
    let selector = ProcessSelector::new_cpu(cpu_adapter);

    // Verify CPU processing works
    let frame = create_yellow_frame(640, 480);
    let roi = Roi::new(0, 0, 640, 480);
    let hsv_range = HsvRange::new(20, 40, 100, 255, 100, 255);

    let mut selector = selector;
    let result = selector
        .process_frame(&frame, &roi, &hsv_range)
        .expect("CPU fallback processing failed");

    assert!(result.detected);
    assert_eq!(selector.backend_type(), "CPU (OpenCV)");
}
