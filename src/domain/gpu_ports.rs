//! GPU processing port definitions.
//!
//! This module defines the `GpuProcessPort` trait for GPU-based image processing.
//! It mirrors `ProcessPort` but operates on GPU-resident textures (`GpuFrame`)
//! instead of CPU-accessible pixel data (`Frame`).

use crate::domain::error::DomainResult;
use crate::domain::types::{DetectionResult, GpuFrame, HsvRange, ProcessorBackend};

/// GPU-based image processing port.
///
/// This trait defines the interface for GPU compute shader-based image processing.
/// Implementations receive GPU-resident textures and perform detection using
/// D3D11 compute shaders, returning only the detection coordinates to CPU.
///
/// # Example (future implementation)
/// ```ignore
/// struct GpuHsvProcessor { /* D3D11 compute shader resources */ }
///
/// impl GpuProcessPort for GpuHsvProcessor {
///     fn process_gpu_frame(&mut self, frame: &GpuFrame, hsv_range: &HsvRange) -> DomainResult<DetectionResult> {
///         // Dispatch compute shader, read back coordinates only
///     }
/// }
/// ```
pub trait GpuProcessPort: Send + Sync {
    /// Process a GPU-resident texture frame and return detection result.
    ///
    /// # Arguments
    /// * `frame` - GPU texture to process
    /// * `hsv_range` - HSV color range for detection
    ///
    /// # Returns
    /// Detection result with coordinates (if detected)
    ///
    /// # Errors
    /// * `DomainError::GpuNotAvailable` - GPU device not available
    /// * `DomainError::GpuCompute` - Compute shader execution failed
    /// * `DomainError::GpuTexture` - Texture operation failed
    fn process_gpu_frame(
        &mut self,
        frame: &GpuFrame,
        hsv_range: &HsvRange,
    ) -> DomainResult<DetectionResult>;

    /// Returns the processing backend type.
    fn backend(&self) -> ProcessorBackend {
        ProcessorBackend::Gpu
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;

    /// Mock GPU processor for testing trait object creation
    struct MockGpuProcessor;

    impl GpuProcessPort for MockGpuProcessor {
        fn process_gpu_frame(
            &mut self,
            _frame: &GpuFrame,
            _hsv_range: &HsvRange,
        ) -> DomainResult<DetectionResult> {
            // Mock implementation returns GpuNotAvailable
            Err(crate::domain::error::DomainError::GpuNotAvailable(
                "Mock processor".to_string(),
            ))
        }
    }

    #[test]
    fn test_gpu_process_port_mock_implementation() {
        // Verify mock implementation compiles and works
        let mut processor = MockGpuProcessor;
        let frame = GpuFrame::new(None, 100, 100, DXGI_FORMAT_B8G8R8A8_UNORM);
        let hsv_range = HsvRange::new(0, 180, 0, 255, 0, 255);

        let result = processor.process_gpu_frame(&frame, &hsv_range);
        assert!(result.is_err());
    }

    #[test]
    fn test_gpu_process_port_backend() {
        let processor = MockGpuProcessor;
        assert_eq!(processor.backend(), ProcessorBackend::Gpu);
    }

    #[test]
    fn test_gpu_process_port_trait_object() {
        // Verify trait can be used as trait object (dyn GpuProcessPort)
        let processor: Box<dyn GpuProcessPort> = Box::new(MockGpuProcessor);
        assert_eq!(processor.backend(), ProcessorBackend::Gpu);
    }
}
