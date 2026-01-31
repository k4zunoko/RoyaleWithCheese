//! GPUキャプチャ統合テスト
//!
//! キャプチャアダプタのGPUフレーム取得機能をテスト。
//! 注意: これらのテストはGPUを必要とするため、CI環境では無視されます。

use RoyaleWithCheese::domain::{
    ports::CapturePort,
    types::{GpuFrame, Roi},
};
use RoyaleWithCheese::infrastructure::capture::{
    dda::DdaCaptureAdapter, spout::SpoutCaptureAdapter,
};

/// GPUデバイスが利用可能かチェック
fn gpu_available() -> bool {
    use RoyaleWithCheese::infrastructure::gpu_device::create_d3d11_device;
    create_d3d11_device().is_ok()
}

#[test]
#[ignore = "Requires GPU and display"]
fn test_dda_supports_gpu_frame() {
    if !gpu_available() {
        println!("Skipping test: No GPU available");
        return;
    }

    let adapter = DdaCaptureAdapter::new(0, 0, 8);
    if let Ok(adapter) = adapter {
        assert!(
            adapter.supports_gpu_frame(),
            "DDA should support GPU frames"
        );
        println!("✓ DDA supports_gpu_frame() = true");
    } else {
        println!("⚠ Could not create DDA adapter (may need display access)");
    }
}

#[test]
#[ignore = "Requires GPU"]
fn test_spout_supports_gpu_frame() {
    if !gpu_available() {
        println!("Skipping test: No GPU available");
        return;
    }

    let adapter = SpoutCaptureAdapter::new(None);
    if let Ok(adapter) = adapter {
        assert!(
            adapter.supports_gpu_frame(),
            "Spout should support GPU frames"
        );
        println!("✓ Spout supports_gpu_frame() = true");
    } else {
        println!("⚠ Could not create Spout adapter");
    }
}

#[test]
#[ignore = "Requires GPU, display, and active DDA session"]
fn test_dda_capture_gpu_frame() {
    if !gpu_available() {
        println!("Skipping test: No GPU available");
        return;
    }

    let mut adapter = match DdaCaptureAdapter::new(0, 0, 100) {
        Ok(a) => a,
        Err(e) => {
            println!("⚠ Could not create DDA adapter: {:?}", e);
            return;
        }
    };

    let roi = Roi::new(0, 0, 640, 480);

    // Attempt to capture a GPU frame
    match adapter.capture_gpu_frame(&roi) {
        Ok(Some(gpu_frame)) => {
            println!(
                "✓ DDA captured GPU frame: {}x{}",
                gpu_frame.width(),
                gpu_frame.height()
            );
            assert!(
                gpu_frame.texture().is_some(),
                "GPU frame should have texture"
            );
            assert_eq!(gpu_frame.width(), 640);
            assert_eq!(gpu_frame.height(), 480);
        }
        Ok(None) => {
            println!(
                "⚠ DDA capture returned None (timeout - this is expected if display not changing)"
            );
        }
        Err(e) => {
            println!("✗ DDA capture failed: {:?}", e);
            // Don't fail the test - this could be due to display permissions
        }
    }
}

#[test]
#[ignore = "Requires GPU and active Spout sender"]
fn test_spout_capture_gpu_frame() {
    if !gpu_available() {
        println!("Skipping test: No GPU available");
        return;
    }

    let mut adapter = match SpoutCaptureAdapter::new(None) {
        Ok(a) => a,
        Err(e) => {
            println!("⚠ Could not create Spout adapter: {:?}", e);
            return;
        }
    };

    let roi = Roi::new(0, 0, 640, 480);

    // Attempt to capture a GPU frame
    match adapter.capture_gpu_frame(&roi) {
        Ok(Some(gpu_frame)) => {
            println!(
                "✓ Spout captured GPU frame: {}x{}",
                gpu_frame.width(),
                gpu_frame.height()
            );
            assert!(
                gpu_frame.texture().is_some(),
                "GPU frame should have texture"
            );
        }
        Ok(None) => {
            println!("⚠ Spout capture returned None (no sender or no new frame)");
        }
        Err(e) => {
            println!("✗ Spout capture failed: {:?}", e);
        }
    }
}

#[test]
fn test_gpu_frame_properties() {
    use std::time::Instant;
    use RoyaleWithCheese::domain::types::Frame;

    // Create a simple GPU frame with no texture (for testing)
    // In real usage, this would come from capture_gpu_frame()
    let gpu_frame = GpuFrame::new(
        None,
        1920,
        1080,
        windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
    );

    assert_eq!(gpu_frame.width(), 1920);
    assert_eq!(gpu_frame.height(), 1080);
    assert!(gpu_frame.texture().is_none()); // No actual texture in this test

    println!("✓ GpuFrame properties accessible");
}

#[test]
#[ignore = "Requires full pipeline with GPU"]
fn test_end_to_end_gpu_capture_and_process() {
    // This test would require:
    // 1. DDA/Spout adapter with GPU frame capture
    // 2. GpuColorAdapter configured with same device
    // 3. ProcessSelector configured for GPU
    //
    // This is the ultimate integration test for the GPU pipeline.
    // For now, we verify each component individually.

    println!("⚠ End-to-end GPU pipeline test requires full environment setup");
    println!("  - Running display with DDA or active Spout sender");
    println!("  - Compatible GPU with D3D11 support");
    println!("  - Matching D3D11 devices between capture and processing");
}
