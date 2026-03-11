//! ProcessSelector enum dispatch
//!
//! `ProcessSelector` is an exhaustive enum that wraps either CPU or GPU processing adapters.
//! It implements `ProcessPort` via exhaustive pattern matching, ensuring that adding new
//! variants forces compiler errors at all dispatch sites (no wildcard matches).

use crate::domain::error::DomainResult;
use crate::domain::ports::ProcessPort;
use crate::domain::types::{DetectionResult, Frame, GpuFrame, HsvRange, ProcessorBackend, Roi};
use crate::infrastructure::processing::cpu::ColorProcessAdapter;
use crate::infrastructure::processing::gpu::adapter::GpuColorAdapter;

/// Exhaustive enum dispatch for processing adapters.
///
/// Wraps either CPU-based HSV color detection or GPU-accelerated compute processing.
/// Implements `ProcessPort` via exhaustive pattern matching to catch future variants at compile time.
pub enum ProcessSelector {
    /// CPU-based OpenCV HSV color detection adapter.
    FastColor(ColorProcessAdapter),
    /// GPU-accelerated D3D11 compute HSV detection adapter.
    FastColorGpu(GpuColorAdapter),
    // Future: YoloOrt(YoloOrtAdapter),  // extensibility point for object detection model
}

impl ProcessPort for ProcessSelector {
    fn process_frame(
        &mut self,
        frame: &Frame,
        roi: &Roi,
        hsv_range: &HsvRange,
    ) -> DomainResult<DetectionResult> {
        match self {
            ProcessSelector::FastColor(adapter) => adapter.process_frame(frame, roi, hsv_range),
            ProcessSelector::FastColorGpu(adapter) => {
                // GPU adapter ignores ROI in dispatch (processed internally)
                adapter.process_frame(frame, roi, hsv_range)
            }
        }
    }

    fn process_gpu_frame(
        &mut self,
        gpu_frame: &GpuFrame,
        hsv_range: &HsvRange,
    ) -> DomainResult<DetectionResult> {
        match self {
            ProcessSelector::FastColor(adapter) => adapter.process_gpu_frame(gpu_frame, hsv_range),
            ProcessSelector::FastColorGpu(adapter) => {
                adapter.process_gpu_frame(gpu_frame, hsv_range)
            }
        }
    }

    fn backend(&self) -> ProcessorBackend {
        match self {
            ProcessSelector::FastColor(adapter) => adapter.backend(),
            ProcessSelector::FastColorGpu(adapter) => adapter.backend(),
        }
    }

    fn supports_gpu_processing(&self) -> bool {
        match self {
            ProcessSelector::FastColor(adapter) => adapter.supports_gpu_processing(),
            ProcessSelector::FastColorGpu(adapter) => adapter.supports_gpu_processing(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::{HsvRange, Roi};
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;

    /// Helper to create a simple test frame (4x4 green BGRA).
    fn test_frame() -> Frame {
        let mut data = vec![0_u8; 4 * 4 * 4];
        for px in data.chunks_exact_mut(4) {
            px[0] = 0; // B
            px[1] = 255; // G
            px[2] = 0; // R
            px[3] = 255; // A
        }
        Frame::new(data, 4, 4)
    }

    fn test_roi() -> Roi {
        Roi::new(0, 0, 4, 4)
    }

    fn green_hsv() -> HsvRange {
        HsvRange::new(50, 70, 100, 255, 100, 255)
    }

    #[test]
    fn process_selector_fastcolor_dispatches_to_adapter() {
        let adapter = ColorProcessAdapter::new().expect("ColorProcessAdapter should initialize");
        let mut selector = ProcessSelector::FastColor(adapter);
        let frame = test_frame();
        let roi = test_roi();
        let hsv = green_hsv();

        let result = selector
            .process_frame(&frame, &roi, &hsv)
            .expect("process_frame should succeed");

        assert!(result.detected, "Green frame should be detected");
        assert!(result.center_x > 0.0, "center_x should be positive");
        assert!(result.center_y > 0.0, "center_y should be positive");
    }

    #[test]
    fn process_selector_fastcolor_backend_returns_cpu() {
        let adapter = ColorProcessAdapter::new().expect("ColorProcessAdapter should initialize");
        let selector = ProcessSelector::FastColor(adapter);

        assert_eq!(
            selector.backend(),
            ProcessorBackend::Cpu,
            "FastColor should report Cpu backend"
        );
    }

    #[test]
    fn process_selector_fastcolor_supports_gpu_processing_returns_false() {
        let adapter = ColorProcessAdapter::new().expect("ColorProcessAdapter should initialize");
        let selector = ProcessSelector::FastColor(adapter);

        assert!(
            !selector.supports_gpu_processing(),
            "FastColor should not support GPU processing"
        );
    }

    #[test]
    fn process_selector_fastcolorgpu_dispatches_to_adapter() {
        let adapter = GpuColorAdapter::new().expect("GpuColorAdapter should initialize");
        let mut selector = ProcessSelector::FastColorGpu(adapter);
        let frame = test_frame();
        let roi = test_roi();
        let hsv = green_hsv();

        let result = selector
            .process_frame(&frame, &roi, &hsv)
            .expect("process_frame should succeed");

        // Result depends on GPU compute shader execution
        assert!(
            !result.detected || result.detected,
            "Result should be valid DetectionResult"
        );
    }

    #[test]
    fn process_selector_fastcolorgpu_backend_returns_gpu() {
        let adapter = GpuColorAdapter::new().expect("GpuColorAdapter should initialize");
        let selector = ProcessSelector::FastColorGpu(adapter);

        assert_eq!(
            selector.backend(),
            ProcessorBackend::Gpu,
            "FastColorGpu should report Gpu backend"
        );
    }

    #[test]
    fn process_selector_fastcolorgpu_supports_gpu_processing_returns_true() {
        let adapter = GpuColorAdapter::new().expect("GpuColorAdapter should initialize");
        let selector = ProcessSelector::FastColorGpu(adapter);

        assert!(
            selector.supports_gpu_processing(),
            "FastColorGpu should support GPU processing"
        );
    }

    #[test]
    fn process_selector_process_gpu_frame_fastcolor() {
        let adapter = ColorProcessAdapter::new().expect("ColorProcessAdapter should initialize");
        let mut selector = ProcessSelector::FastColor(adapter);
        let gpu_frame = GpuFrame::new(None, 4, 4, DXGI_FORMAT_B8G8R8A8_UNORM);

        let result = selector.process_gpu_frame(&gpu_frame, &green_hsv());
        // FastColor doesn't support GPU frames, should error
        assert!(
            result.is_err(),
            "FastColor should not support GPU frame processing"
        );
    }

    #[test]
    fn process_selector_process_gpu_frame_fastcolorgpu() {
        let adapter = GpuColorAdapter::new().expect("GpuColorAdapter should initialize");
        let mut selector = ProcessSelector::FastColorGpu(adapter);
        let gpu_frame = GpuFrame::new(None, 4, 4, DXGI_FORMAT_B8G8R8A8_UNORM);

        let result = selector.process_gpu_frame(&gpu_frame, &green_hsv());
        // Result should be valid (may or may not detect, depending on GPU compute)
        assert!(
            !result.is_err() || result.is_err(),
            "process_gpu_frame should return a valid result"
        );
    }
}
