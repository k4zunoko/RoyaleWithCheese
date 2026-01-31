//! GPU-based image processing using D3D11 compute shaders.
//!
//! This module implements HSV detection on GPU textures and reads back only
//! aggregate detection results (count and summed coordinates).

pub mod adapter;

use crate::domain::error::{DomainError, DomainResult};
use crate::domain::gpu_ports::GpuProcessPort;
use crate::domain::types::{DetectionResult, GpuFrame, HsvRange, ProcessorBackend};
use std::ffi::CString;
use std::mem::size_of;
use windows::core::PCSTR;
use windows::Win32::Graphics::Direct3D::Fxc::{
    D3DCompile, D3DCOMPILE_DEBUG, D3DCOMPILE_ENABLE_STRICTNESS, D3DCOMPILE_OPTIMIZATION_LEVEL3,
};
use windows::Win32::Graphics::Direct3D::{ID3DBlob, ID3DInclude, D3D_SRV_DIMENSION_TEXTURE2D};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Buffer, ID3D11ComputeShader, ID3D11Device, ID3D11DeviceContext, ID3D11ShaderResourceView,
    ID3D11Texture2D, ID3D11UnorderedAccessView, D3D11_BIND_CONSTANT_BUFFER,
    D3D11_BIND_UNORDERED_ACCESS, D3D11_BUFFER_DESC, D3D11_BUFFER_UAV, D3D11_CPU_ACCESS_READ,
    D3D11_CPU_ACCESS_WRITE, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_MAP_WRITE_DISCARD,
    D3D11_RESOURCE_MISC_BUFFER_STRUCTURED, D3D11_SHADER_RESOURCE_VIEW_DESC,
    D3D11_SHADER_RESOURCE_VIEW_DESC_0, D3D11_TEX2D_SRV, D3D11_UAV_DIMENSION_BUFFER,
    D3D11_UNORDERED_ACCESS_VIEW_DESC, D3D11_UNORDERED_ACCESS_VIEW_DESC_0, D3D11_USAGE_DEFAULT,
    D3D11_USAGE_DYNAMIC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_B8G8R8A8_UNORM_SRGB, DXGI_FORMAT_UNKNOWN,
};

const HLSL_SOURCE: &str = include_str!("shaders/hsv_detect.hlsl");
const HLSL_ENTRY_POINT: &str = "CSMain";
const HLSL_TARGET: &str = "cs_5_0";
const THREAD_GROUP_SIZE_X: u32 = 16;
const THREAD_GROUP_SIZE_Y: u32 = 16;
const OUTPUT_BUFFER_ELEMENTS: usize = 3;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct HsvParams {
    h_min: u32,
    h_max: u32,
    s_min: u32,
    s_max: u32,
    v_min: u32,
    v_max: u32,
    img_width: u32,
    img_height: u32,
}

impl HsvParams {
    fn from_range(range: &HsvRange, width: u32, height: u32) -> Self {
        Self {
            h_min: range.h_min as u32,
            h_max: range.h_max as u32,
            s_min: range.s_min as u32,
            s_max: range.s_max as u32,
            v_min: range.v_min as u32,
            v_max: range.v_max as u32,
            img_width: width,
            img_height: height,
        }
    }
}

/// GPU color processor using D3D11 compute shaders.
///
/// Holds all D3D11 resources needed to dispatch the HSV detection shader and
/// read back aggregated results with minimal CPU transfer.
pub struct GpuColorProcessor {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    compute_shader: ID3D11ComputeShader,
    constant_buffer: ID3D11Buffer,
    output_buffer: ID3D11Buffer,
    output_uav: ID3D11UnorderedAccessView,
    staging_buffer: ID3D11Buffer,
}

// SAFETY: D3D11 device/context are thread-safe when used with external synchronization.
// The processor requires &mut self for processing, preventing concurrent access.
unsafe impl Send for GpuColorProcessor {}
// SAFETY: Internal GPU resources are immutable references and COM interfaces are thread-safe.
unsafe impl Sync for GpuColorProcessor {}

impl GpuColorProcessor {
    /// Create a new GPU processor using an existing D3D11 device.
    pub fn new(device: &ID3D11Device) -> DomainResult<Self> {
        // SAFETY: GetImmediateContext returns a valid context tied to this device.
        let context = unsafe { device.GetImmediateContext() }.map_err(|e| {
            DomainError::GpuNotAvailable(format!(
                "Failed to acquire D3D11 immediate context: {:?}",
                e
            ))
        })?;

        Self::new_with_context(device.clone(), context)
    }

    /// Create a new GPU processor with explicit device and context.
    pub fn new_with_context(
        device: ID3D11Device,
        context: ID3D11DeviceContext,
    ) -> DomainResult<Self> {
        let compute_shader = Self::compile_compute_shader(&device)?;
        let constant_buffer = Self::create_constant_buffer(&device)?;
        let (output_buffer, output_uav, staging_buffer) = Self::create_output_buffers(&device)?;

        Ok(Self {
            device,
            context,
            compute_shader,
            constant_buffer,
            output_buffer,
            output_uav,
            staging_buffer,
        })
    }

    fn compile_compute_shader(device: &ID3D11Device) -> DomainResult<ID3D11ComputeShader> {
        let entry_point = CString::new(HLSL_ENTRY_POINT).map_err(|_| {
            DomainError::GpuCompute("HLSL entry point contains a null byte".to_string())
        })?;
        let target = CString::new(HLSL_TARGET).map_err(|_| {
            DomainError::GpuCompute("HLSL target string contains a null byte".to_string())
        })?;

        let mut flags = D3DCOMPILE_ENABLE_STRICTNESS;
        if cfg!(debug_assertions) {
            flags |= D3DCOMPILE_DEBUG;
        } else {
            flags |= D3DCOMPILE_OPTIMIZATION_LEVEL3;
        }

        let mut shader_blob: Option<ID3DBlob> = None;
        let mut error_blob: Option<ID3DBlob> = None;

        // SAFETY: D3DCompile expects valid pointers to the source and C strings for the call.
        let compile_result = unsafe {
            D3DCompile(
                HLSL_SOURCE.as_ptr() as *const _,
                HLSL_SOURCE.len(),
                PCSTR::null(),
                None,
                None::<&ID3DInclude>,
                PCSTR::from_raw(entry_point.as_ptr() as *const u8),
                PCSTR::from_raw(target.as_ptr() as *const u8),
                flags,
                0,
                &mut shader_blob,
                Some(&mut error_blob),
            )
        };

        if let Err(err) = compile_result {
            let details = error_blob
                .as_ref()
                .map(blob_to_string)
                .unwrap_or_else(|| format!("{:?}", err));
            return Err(DomainError::GpuCompute(format!(
                "Failed to compile HSV compute shader: {}",
                details
            )));
        }

        let shader_blob = match shader_blob {
            Some(blob) => blob,
            None => {
                return Err(DomainError::GpuCompute(
                    "Shader compilation returned no bytecode".to_string(),
                ));
            }
        };

        let shader_bytes = unsafe {
            // SAFETY: Blob exposes a valid pointer/size for the bytecode.
            std::slice::from_raw_parts(
                shader_blob.GetBufferPointer() as *const u8,
                shader_blob.GetBufferSize(),
            )
        };

        let mut compute_shader: Option<ID3D11ComputeShader> = None;
        // SAFETY: The bytecode pointer is valid for the duration of this call.
        unsafe {
            device
                .CreateComputeShader(shader_bytes, None, Some(&mut compute_shader))
                .map_err(|e| {
                    DomainError::GpuCompute(format!("Failed to create compute shader: {:?}", e))
                })?;
        }

        compute_shader.ok_or_else(|| {
            DomainError::GpuCompute("Compute shader creation returned null".to_string())
        })
    }

    fn create_constant_buffer(device: &ID3D11Device) -> DomainResult<ID3D11Buffer> {
        debug_assert_eq!(size_of::<HsvParams>() % 16, 0);

        let desc = D3D11_BUFFER_DESC {
            ByteWidth: size_of::<HsvParams>() as u32,
            Usage: D3D11_USAGE_DYNAMIC,
            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
            MiscFlags: 0,
            StructureByteStride: 0,
        };

        let mut buffer: Option<ID3D11Buffer> = None;
        // SAFETY: D3D11 device is valid; desc and output pointer live for call.
        unsafe {
            device
                .CreateBuffer(&desc, None, Some(&mut buffer))
                .map_err(|e| {
                    DomainError::GpuCompute(format!(
                        "Failed to create HSV constant buffer: {:?}",
                        e
                    ))
                })?;
        }

        buffer.ok_or_else(|| {
            DomainError::GpuCompute("Constant buffer creation returned null".to_string())
        })
    }

    fn create_output_buffers(
        device: &ID3D11Device,
    ) -> DomainResult<(ID3D11Buffer, ID3D11UnorderedAccessView, ID3D11Buffer)> {
        let stride = size_of::<u32>() as u32;
        let byte_width = stride * OUTPUT_BUFFER_ELEMENTS as u32;

        let output_desc = D3D11_BUFFER_DESC {
            ByteWidth: byte_width,
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_UNORDERED_ACCESS.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: D3D11_RESOURCE_MISC_BUFFER_STRUCTURED.0 as u32,
            StructureByteStride: stride,
        };

        let mut output_buffer: Option<ID3D11Buffer> = None;
        // SAFETY: D3D11 device is valid; desc and output pointer live for call.
        unsafe {
            device
                .CreateBuffer(&output_desc, None, Some(&mut output_buffer))
                .map_err(|e| {
                    DomainError::GpuCompute(format!("Failed to create output buffer: {:?}", e))
                })?;
        }

        let output_buffer = output_buffer.ok_or_else(|| {
            DomainError::GpuCompute("Output buffer creation returned null".to_string())
        })?;

        let uav_desc = D3D11_UNORDERED_ACCESS_VIEW_DESC {
            Format: DXGI_FORMAT_UNKNOWN,
            ViewDimension: D3D11_UAV_DIMENSION_BUFFER,
            Anonymous: D3D11_UNORDERED_ACCESS_VIEW_DESC_0 {
                Buffer: D3D11_BUFFER_UAV {
                    FirstElement: 0,
                    NumElements: OUTPUT_BUFFER_ELEMENTS as u32,
                    Flags: 0,
                },
            },
        };

        let mut uav: Option<ID3D11UnorderedAccessView> = None;
        // SAFETY: Output buffer is valid; desc and output pointer live for call.
        unsafe {
            device
                .CreateUnorderedAccessView(&output_buffer, Some(&uav_desc), Some(&mut uav))
                .map_err(|e| {
                    DomainError::GpuCompute(format!("Failed to create output UAV: {:?}", e))
                })?;
        }

        let output_uav = uav.ok_or_else(|| {
            DomainError::GpuCompute("Output UAV creation returned null".to_string())
        })?;

        let staging_desc = D3D11_BUFFER_DESC {
            ByteWidth: byte_width,
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: D3D11_RESOURCE_MISC_BUFFER_STRUCTURED.0 as u32,
            StructureByteStride: stride,
        };

        let mut staging_buffer: Option<ID3D11Buffer> = None;
        // SAFETY: D3D11 device is valid; desc and output pointer live for call.
        unsafe {
            device
                .CreateBuffer(&staging_desc, None, Some(&mut staging_buffer))
                .map_err(|e| {
                    DomainError::GpuCompute(format!("Failed to create staging buffer: {:?}", e))
                })?;
        }

        let staging_buffer = staging_buffer.ok_or_else(|| {
            DomainError::GpuCompute("Staging buffer creation returned null".to_string())
        })?;

        Ok((output_buffer, output_uav, staging_buffer))
    }

    fn create_texture_srv(
        &self,
        texture: &ID3D11Texture2D,
        format: DXGI_FORMAT,
    ) -> DomainResult<ID3D11ShaderResourceView> {
        let desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
            Format: format,
            ViewDimension: D3D_SRV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_SRV {
                    MostDetailedMip: 0,
                    MipLevels: 1,
                },
            },
        };

        let mut srv: Option<ID3D11ShaderResourceView> = None;
        // SAFETY: Texture is valid; desc and output pointer live for call.
        unsafe {
            self.device
                .CreateShaderResourceView(texture, Some(&desc), Some(&mut srv))
                .map_err(|e| {
                    DomainError::GpuTexture(format!(
                        "Failed to create SRV for input texture: {:?}",
                        e
                    ))
                })?;
        }

        srv.ok_or_else(|| {
            DomainError::GpuTexture("Shader resource view creation returned null".to_string())
        })
    }

    fn update_constant_buffer(
        &self,
        hsv_range: &HsvRange,
        width: u32,
        height: u32,
    ) -> DomainResult<()> {
        let params = HsvParams::from_range(hsv_range, width, height);
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();

        // SAFETY: Map returns a valid pointer to the constant buffer data.
        unsafe {
            self.context
                .Map(
                    &self.constant_buffer,
                    0,
                    D3D11_MAP_WRITE_DISCARD,
                    0,
                    Some(&mut mapped),
                )
                .map_err(|e| {
                    DomainError::GpuCompute(format!("Failed to map HSV constant buffer: {:?}", e))
                })?;

            if mapped.pData.is_null() {
                self.context.Unmap(&self.constant_buffer, 0);
                return Err(DomainError::GpuCompute(
                    "Mapped constant buffer returned null pointer".to_string(),
                ));
            }

            std::ptr::copy_nonoverlapping(
                &params as *const HsvParams as *const u8,
                mapped.pData as *mut u8,
                size_of::<HsvParams>(),
            );
            self.context.Unmap(&self.constant_buffer, 0);
        }

        Ok(())
    }
}

impl GpuProcessPort for GpuColorProcessor {
    fn process_gpu_frame(
        &mut self,
        frame: &GpuFrame,
        hsv_range: &HsvRange,
    ) -> DomainResult<DetectionResult> {
        let texture = frame.texture().ok_or_else(|| {
            DomainError::GpuNotAvailable("GPU texture not available in frame".to_string())
        })?;

        let width = frame.width();
        let height = frame.height();
        if width == 0 || height == 0 {
            return Err(DomainError::GpuTexture(
                "GPU frame dimensions must be non-zero".to_string(),
            ));
        }

        let format = frame.format();
        if !is_supported_format(format) {
            return Err(DomainError::GpuTexture(format!(
                "Unsupported GPU texture format: {:?}",
                format
            )));
        }

        let srv = self.create_texture_srv(texture, format)?;
        self.update_constant_buffer(hsv_range, width, height)?;

        let clear_values = [0u32; 4];
        // SAFETY: output_uav is a valid UAV owned by this processor.
        unsafe {
            self.context
                .ClearUnorderedAccessViewUint(&self.output_uav, &clear_values);
        }

        let srvs = [Some(srv)];
        let uavs = [Some(self.output_uav.clone())];
        let constant_buffers = [Some(self.constant_buffer.clone())];

        // SAFETY: Binding resources and dispatching uses valid D3D11 objects.
        unsafe {
            self.context.CSSetShaderResources(0, Some(&srvs));
            self.context
                .CSSetUnorderedAccessViews(0, uavs.len() as u32, Some(uavs.as_ptr()), None);
            self.context
                .CSSetConstantBuffers(0, Some(&constant_buffers));
            self.context.CSSetShader(&self.compute_shader, None);
            self.context.Dispatch(
                (width + THREAD_GROUP_SIZE_X - 1) / THREAD_GROUP_SIZE_X,
                (height + THREAD_GROUP_SIZE_Y - 1) / THREAD_GROUP_SIZE_Y,
                1,
            );
        }

        let null_srvs = [None];
        let null_uavs = [None];

        // SAFETY: Unbinding resources uses valid pipeline slots.
        unsafe {
            self.context.CSSetShaderResources(0, Some(&null_srvs));
            self.context
                .CSSetUnorderedAccessViews(0, 1, Some(null_uavs.as_ptr()), None);
        }

        // SAFETY: Copying between buffers is valid for same-sized resources.
        unsafe {
            self.context
                .CopyResource(&self.staging_buffer, &self.output_buffer);
        }

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: Map returns a valid pointer for the staging buffer.
        unsafe {
            self.context
                .Map(
                    &self.staging_buffer,
                    0,
                    D3D11_MAP_READ,
                    0,
                    Some(&mut mapped),
                )
                .map_err(|e| {
                    DomainError::GpuCompute(format!("Failed to map output buffer: {:?}", e))
                })?;
        }

        if mapped.pData.is_null() {
            // SAFETY: Unmap must be called when Map succeeded.
            unsafe {
                self.context.Unmap(&self.staging_buffer, 0);
            }
            return Err(DomainError::GpuCompute(
                "Mapped output buffer returned null pointer".to_string(),
            ));
        }

        let results = unsafe {
            // SAFETY: The staging buffer holds OUTPUT_BUFFER_ELEMENTS u32 values.
            std::slice::from_raw_parts(mapped.pData as *const u32, OUTPUT_BUFFER_ELEMENTS)
        };

        let detected_count = results[0];
        let sum_x = results[1];
        let sum_y = results[2];

        // SAFETY: Unmap must be called when Map succeeded.
        unsafe {
            self.context.Unmap(&self.staging_buffer, 0);
        }

        if detected_count == 0 {
            return Ok(DetectionResult {
                timestamp: frame.timestamp(),
                center_x: 0.0,
                center_y: 0.0,
                coverage: 0,
                detected: false,
                bounding_box: None,
            });
        }

        let count_f = detected_count as f32;
        Ok(DetectionResult {
            timestamp: frame.timestamp(),
            center_x: sum_x as f32 / count_f,
            center_y: sum_y as f32 / count_f,
            coverage: detected_count,
            detected: true,
            bounding_box: None,
        })
    }

    fn backend(&self) -> ProcessorBackend {
        ProcessorBackend::Gpu
    }
}

fn is_supported_format(format: DXGI_FORMAT) -> bool {
    matches!(
        format,
        DXGI_FORMAT_B8G8R8A8_UNORM | DXGI_FORMAT_B8G8R8A8_UNORM_SRGB
    )
}

fn blob_to_string(blob: &ID3DBlob) -> String {
    // SAFETY: Blob provides a valid pointer/size for its contents.
    let bytes = unsafe {
        std::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize())
    };
    String::from_utf8_lossy(bytes)
        .trim_end_matches('\0')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::DomainError;
    use windows::Win32::Graphics::Direct3D::{
        D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_10_0,
        D3D_FEATURE_LEVEL_11_0,
    };
    use windows::Win32::Graphics::Direct3D11::{
        D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, D3D11_CREATE_DEVICE_FLAG,
        D3D11_SDK_VERSION,
    };
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;

    fn create_test_device() -> Option<(ID3D11Device, ID3D11DeviceContext)> {
        let feature_levels = [D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_0];
        let flags = D3D11_CREATE_DEVICE_FLAG(0);

        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;

        // SAFETY: D3D11CreateDevice is an FFI call; parameters are valid.
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

        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;

        // SAFETY: D3D11CreateDevice is an FFI call; parameters are valid.
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

    #[test]
    fn test_gpu_color_processor_creation() {
        let Some((device, _context)) = create_test_device() else {
            return;
        };

        let processor = GpuColorProcessor::new(&device);
        assert!(processor.is_ok());

        if let Ok(processor) = processor {
            assert_eq!(processor.backend(), ProcessorBackend::Gpu);
        }
    }

    #[test]
    fn test_gpu_color_processor_default() {
        let Some((device, context)) = create_test_device() else {
            return;
        };

        let processor = GpuColorProcessor::new_with_context(device, context);
        assert!(processor.is_ok());
    }

    #[test]
    fn test_gpu_color_processor_returns_not_available() {
        let Some((device, _context)) = create_test_device() else {
            return;
        };

        let mut processor = match GpuColorProcessor::new(&device) {
            Ok(processor) => processor,
            Err(err) => {
                panic!("Failed to create GPU processor: {:?}", err);
            }
        };
        let frame = GpuFrame::new(None, 100, 100, DXGI_FORMAT_B8G8R8A8_UNORM);
        let hsv_range = HsvRange::new(0, 100, 100, 10, 255, 255);

        let result = processor.process_gpu_frame(&frame, &hsv_range);

        assert!(result.is_err());
        match result {
            Err(DomainError::GpuNotAvailable(msg)) => {
                assert!(msg.contains("GPU texture"));
            }
            Err(err) => panic!("Unexpected error: {:?}", err),
            Ok(_) => panic!("Expected error for missing GPU texture"),
        }
    }

    #[test]
    fn test_gpu_color_processor_as_trait_object() {
        let Some((device, context)) = create_test_device() else {
            return;
        };

        let processor = match GpuColorProcessor::new_with_context(device, context) {
            Ok(processor) => processor,
            Err(err) => {
                panic!("Failed to create GPU processor: {:?}", err);
            }
        };

        let processor: Box<dyn GpuProcessPort> = Box::new(processor);
        assert_eq!(processor.backend(), ProcessorBackend::Gpu);
    }
}
