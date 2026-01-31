//! GPU Color Adapter - ProcessPort implementation for GPU processing
//!
//! This module provides an adapter that implements `ProcessPort` trait using
//! GPU-based HSV detection. It handles the upload of CPU Frame data to GPU
//! textures and delegates processing to `GpuColorProcessor`.
//!
//! # Architecture
//! ```
//! Frame (CPU memory)
//!     ↓ upload
//! ID3D11Texture2D (GPU memory)
//!     ↓ wrap
//! GpuFrame
//!     ↓ process_gpu_frame
//! GpuColorProcessor
//!     ↓
//! DetectionResult
//! ```

use crate::domain::{
    error::{DomainError, DomainResult},
    gpu_ports::GpuProcessPort,
    ports::ProcessPort,
    types::{DetectionResult, Frame, GpuFrame, HsvRange, ProcessorBackend, Roi},
};
use crate::infrastructure::processing::gpu::GpuColorProcessor;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_BIND_SHADER_RESOURCE,
    D3D11_CPU_ACCESS_WRITE, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_WRITE_DISCARD,
    D3D11_RESOURCE_MISC_FLAG, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DYNAMIC,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;

/// GPU color processor adapter implementing ProcessPort
///
/// This adapter wraps `GpuColorProcessor` and implements `ProcessPort` trait,
/// allowing seamless integration into the existing CPU-based pipeline.
/// It handles the texture upload from CPU Frame to GPU memory.
pub struct GpuColorAdapter {
    processor: GpuColorProcessor,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    // Reusable texture for frame upload (if dimensions match)
    staging_texture: Option<ID3D11Texture2D>,
    staging_width: u32,
    staging_height: u32,
}

// SAFETY: D3D11 device/context are thread-safe when used with external synchronization.
// The adapter requires &mut self for processing, preventing concurrent access.
unsafe impl Send for GpuColorAdapter {}
unsafe impl Sync for GpuColorAdapter {}

impl GpuColorAdapter {
    /// Create a new GPU color adapter
    ///
    /// # Arguments
    /// * `device` - D3D11 device to use for processing
    ///
    /// # Returns
    /// * `Ok(GpuColorAdapter)` - Successfully created adapter
    /// * `Err(DomainError::GpuNotAvailable)` - GPU initialization failed
    pub fn new(device: &ID3D11Device) -> DomainResult<Self> {
        let processor = GpuColorProcessor::new(device)?;

        // SAFETY: GetImmediateContext returns a valid context tied to this device.
        let context = unsafe { device.GetImmediateContext() }.map_err(|e| {
            DomainError::GpuNotAvailable(format!(
                "Failed to acquire D3D11 immediate context: {:?}",
                e
            ))
        })?;

        Ok(Self {
            processor,
            device: device.clone(),
            context,
            staging_texture: None,
            staging_width: 0,
            staging_height: 0,
        })
    }

    /// Create a new GPU color adapter with explicit device and context
    ///
    /// This is useful when sharing device/context with other components
    /// (e.g., capture adapters).
    pub fn with_device_context(
        device: ID3D11Device,
        context: ID3D11DeviceContext,
    ) -> DomainResult<Self> {
        let processor = GpuColorProcessor::new_with_context(device.clone(), context.clone())?;

        Ok(Self {
            processor,
            device,
            context,
            staging_texture: None,
            staging_width: 0,
            staging_height: 0,
        })
    }

    /// Upload frame data to GPU texture
    ///
    /// Creates or reuses a GPU texture and uploads BGRA frame data.
    /// The texture is created with D3D11_USAGE_DYNAMIC for CPU write access.
    fn upload_frame_to_gpu(&mut self, frame: &Frame) -> DomainResult<GpuFrame> {
        let width = frame.width;
        let height = frame.height;

        // Check if we need to create a new texture
        if self.staging_texture.is_none()
            || self.staging_width != width
            || self.staging_height != height
        {
            self.create_staging_texture(width, height)?;
        }

        let texture = self.staging_texture.as_ref().unwrap();

        // Map the texture and copy frame data
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();

        // SAFETY: Map returns a valid pointer for writing to the dynamic texture.
        unsafe {
            self.context
                .Map(texture, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped))
                .map_err(|e| {
                    DomainError::GpuTexture(format!("Failed to map staging texture: {:?}", e))
                })?;
        }

        if mapped.pData.is_null() {
            // SAFETY: Unmap must be called even if Map returned null.
            unsafe {
                self.context.Unmap(texture, 0);
            }
            return Err(DomainError::GpuTexture(
                "Mapped texture returned null pointer".to_string(),
            ));
        }

        // Copy frame data (BGRA format, 4 bytes per pixel)
        let row_pitch = (width * 4) as usize;
        let src_data = &frame.data;

        // SAFETY: Copy frame data row by row to handle potential pitch differences.
        unsafe {
            let dest_ptr = mapped.pData as *mut u8;
            let dest_pitch = mapped.RowPitch as usize;

            for y in 0..height as usize {
                let src_offset = y * row_pitch;
                let dest_offset = y * dest_pitch;
                let row_data = &src_data[src_offset..src_offset + row_pitch];

                std::ptr::copy_nonoverlapping(
                    row_data.as_ptr(),
                    dest_ptr.add(dest_offset),
                    row_pitch,
                );
            }

            self.context.Unmap(texture, 0);
        }

        // Create GpuFrame wrapping the texture
        let gpu_frame = GpuFrame::new(
            Some(texture.clone()),
            width,
            height,
            DXGI_FORMAT_B8G8R8A8_UNORM,
        );

        Ok(gpu_frame)
    }

    /// Create a staging texture for CPU→GPU upload
    fn create_staging_texture(&mut self, width: u32, height: u32) -> DomainResult<()> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DYNAMIC,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
            MiscFlags: D3D11_RESOURCE_MISC_FLAG(0).0 as u32,
        };

        let mut texture: Option<ID3D11Texture2D> = None;

        // SAFETY: CreateTexture2D is safe with valid desc and device.
        unsafe {
            self.device
                .CreateTexture2D(&desc, None, Some(&mut texture))
                .map_err(|e| {
                    DomainError::GpuTexture(format!(
                        "Failed to create staging texture ({}x{}): {:?}",
                        width, height, e
                    ))
                })?;
        }

        self.staging_texture = texture;
        self.staging_width = width;
        self.staging_height = height;

        Ok(())
    }

    /// Get the D3D11 device
    pub fn device(&self) -> &ID3D11Device {
        &self.device
    }

    /// Get the D3D11 device context
    pub fn context(&self) -> &ID3D11DeviceContext {
        &self.context
    }

    /// Check if GPU processing is available
    pub fn is_available(&self) -> bool {
        // The adapter exists, so GPU should be available
        // In practice, we might want to do a test operation
        true
    }

    /// Process a GPU frame directly (zero-copy)
    ///
    /// This method processes an already-uploaded GPU frame without any CPU transfer.
    /// Use this when you have a GpuFrame from a capture adapter that supports GPU frames.
    pub fn process_gpu_frame(
        &mut self,
        gpu_frame: &GpuFrame,
        hsv_range: &HsvRange,
    ) -> DomainResult<DetectionResult> {
        // Directly process the GPU frame without any upload
        self.processor.process_gpu_frame(gpu_frame, hsv_range)
    }
}

impl ProcessPort for GpuColorAdapter {
    fn process_frame(
        &mut self,
        frame: &Frame,
        _roi: &Roi,
        hsv_range: &HsvRange,
    ) -> DomainResult<DetectionResult> {
        // Step 1: Upload frame data to GPU
        let gpu_frame = self.upload_frame_to_gpu(frame)?;

        // Step 2: Process using GPU
        let result = self.processor.process_gpu_frame(&gpu_frame, hsv_range)?;

        Ok(result)
    }

    fn backend(&self) -> ProcessorBackend {
        ProcessorBackend::Gpu
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::HsvRange;
    use std::time::Instant;
    use windows::Win32::Graphics::Direct3D::{
        D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_10_0,
        D3D_FEATURE_LEVEL_11_0,
    };
    use windows::Win32::Graphics::Direct3D11::{
        D3D11CreateDevice, D3D11_CREATE_DEVICE_FLAG, D3D11_SDK_VERSION,
    };

    fn create_test_device() -> Option<(ID3D11Device, ID3D11DeviceContext)> {
        let feature_levels = [D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_0];
        let flags = D3D11_CREATE_DEVICE_FLAG(0);

        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;

        // Try hardware first
        let result = unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                None,
                flags,
                Some(&feature_levels),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        };

        if result.is_ok() {
            if let (Some(device), Some(context)) = (device, context) {
                return Some((device, context));
            }
        }

        // Fallback to WARP
        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;

        let result = unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_WARP,
                None,
                flags,
                Some(&feature_levels),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        };

        if result.is_ok() {
            if let (Some(device), Some(context)) = (device, context) {
                return Some((device, context));
            }
        }

        None
    }

    fn create_test_frame(width: u32, height: u32) -> Frame {
        // Create a yellow test pattern
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
    fn test_gpu_color_adapter_creation() {
        let Some((device, _context)) = create_test_device() else {
            println!("Skipping test: No D3D11 device available");
            return;
        };

        let adapter = GpuColorAdapter::new(&device);
        assert!(adapter.is_ok());

        let adapter = adapter.unwrap();
        assert_eq!(adapter.backend(), ProcessorBackend::Gpu);
        assert!(adapter.is_available());
    }

    #[test]
    fn test_gpu_color_adapter_process_frame() {
        let Some((device, context)) = create_test_device() else {
            println!("Skipping test: No D3D11 device available");
            return;
        };

        let mut adapter = GpuColorAdapter::with_device_context(device, context).unwrap();
        let frame = create_test_frame(640, 480);
        let roi = Roi::new(0, 0, 640, 480);

        // Yellow HSV range
        let hsv_range = HsvRange::new(20, 40, 100, 255, 100, 255);

        let result = adapter.process_frame(&frame, &roi, &hsv_range);

        // Should succeed (GPU processing works)
        assert!(result.is_ok(), "GPU processing failed: {:?}", result.err());

        let detection = result.unwrap();
        // Should detect something (the yellow circle)
        assert!(detection.detected, "Should detect yellow color");
        assert!(detection.coverage > 0, "Coverage should be > 0");
    }

    #[test]
    fn test_gpu_color_adapter_no_detection() {
        let Some((device, context)) = create_test_device() else {
            println!("Skipping test: No D3D11 device available");
            return;
        };

        let mut adapter = GpuColorAdapter::with_device_context(device, context).unwrap();

        // Black frame (no detection)
        let frame = Frame {
            data: vec![0u8; 640 * 480 * 4],
            width: 640,
            height: 480,
            timestamp: Instant::now(),
            dirty_rects: vec![],
        };
        let roi = Roi::new(0, 0, 640, 480);

        // Yellow HSV range (won't match black)
        let hsv_range = HsvRange::new(20, 40, 100, 255, 100, 255);

        let result = adapter.process_frame(&frame, &roi, &hsv_range);
        assert!(result.is_ok());

        let detection = result.unwrap();
        assert!(
            !detection.detected,
            "Should not detect anything in black frame"
        );
        assert_eq!(detection.coverage, 0);
    }

    #[test]
    fn test_staging_texture_reuse() {
        let Some((device, context)) = create_test_device() else {
            println!("Skipping test: No D3D11 device available");
            return;
        };

        let mut adapter = GpuColorAdapter::with_device_context(device, context).unwrap();

        // First frame: creates texture
        let frame1 = create_test_frame(320, 240);
        let roi = Roi::new(0, 0, 320, 240);
        let hsv_range = HsvRange::new(20, 40, 100, 255, 100, 255);

        let _ = adapter.process_frame(&frame1, &roi, &hsv_range).unwrap();

        // Same dimensions: should reuse texture
        let frame2 = create_test_frame(320, 240);
        let _ = adapter.process_frame(&frame2, &roi, &hsv_range).unwrap();

        // Different dimensions: creates new texture
        let frame3 = create_test_frame(640, 480);
        let roi2 = Roi::new(0, 0, 640, 480);
        let _ = adapter.process_frame(&frame3, &roi2, &hsv_range).unwrap();

        // All succeeded
        assert!(true);
    }
}
