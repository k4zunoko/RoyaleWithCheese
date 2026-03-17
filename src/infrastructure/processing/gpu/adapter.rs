//! GPU processing adapter using D3D11 compute shader.

use crate::domain::error::{DomainError, DomainResult};
use crate::domain::ports::ProcessPort;
use crate::domain::types::{DetectionResult, Frame, GpuFrame, HsvRange, ProcessorBackend, Roi};
use crate::infrastructure::gpu_device::create_d3d11_device;
use std::ffi::CString;
use std::mem::size_of;
use windows::core::PCSTR;
use windows::Win32::Graphics::Direct3D::Fxc::{
    D3DCompile, D3DCOMPILE_DEBUG, D3DCOMPILE_ENABLE_STRICTNESS, D3DCOMPILE_OPTIMIZATION_LEVEL3,
};
use windows::Win32::Graphics::Direct3D::{ID3DBlob, D3D_SRV_DIMENSION_TEXTURE2D};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Buffer, ID3D11ComputeShader, ID3D11Device, ID3D11DeviceContext, ID3D11ShaderResourceView,
    ID3D11Texture2D, ID3D11UnorderedAccessView, D3D11_BIND_CONSTANT_BUFFER,
    D3D11_BIND_SHADER_RESOURCE, D3D11_BIND_UNORDERED_ACCESS, D3D11_BUFFER_DESC, D3D11_BUFFER_UAV,
    D3D11_CPU_ACCESS_READ, D3D11_CPU_ACCESS_WRITE, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ,
    D3D11_MAP_WRITE_DISCARD, D3D11_RESOURCE_MISC_BUFFER_STRUCTURED,
    D3D11_SHADER_RESOURCE_VIEW_DESC, D3D11_SHADER_RESOURCE_VIEW_DESC_0, D3D11_TEX2D_SRV,
    D3D11_TEXTURE2D_DESC, D3D11_UAV_DIMENSION_BUFFER, D3D11_UNORDERED_ACCESS_VIEW_DESC,
    D3D11_UNORDERED_ACCESS_VIEW_DESC_0, D3D11_USAGE_DEFAULT, D3D11_USAGE_DYNAMIC,
    D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC,
};

const SHADER_SOURCE: &str = include_str!("shader.hlsl");
const SHADER_ENTRY: &str = "CSMain";
const SHADER_TARGET: &str = "cs_5_0";
const THREAD_GROUP_SIZE: u32 = 16;

#[repr(C)]
#[derive(Clone, Copy)]
struct HsvParams {
    h_low: u32,
    h_high: u32,
    s_low: u32,
    s_high: u32,
    v_low: u32,
    v_high: u32,
    width: u32,
    height: u32,
}

pub struct GpuColorAdapter {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    compute_shader: ID3D11ComputeShader,
    constant_buffer: ID3D11Buffer,
    output_buffer: ID3D11Buffer,
    output_uav: ID3D11UnorderedAccessView,
    staging_buffer: ID3D11Buffer,
    upload_texture: Option<ID3D11Texture2D>,
    upload_srv: Option<ID3D11ShaderResourceView>,
    upload_width: u32,
    upload_height: u32,
}

impl GpuColorAdapter {
    pub fn new() -> DomainResult<Self> {
        let (device, context) = create_d3d11_device()?;
        Self::with_device_context(device, context)
    }

    pub fn with_device_context(
        device: ID3D11Device,
        context: ID3D11DeviceContext,
    ) -> DomainResult<Self> {
        let compute_shader = compile_compute_shader(&device, SHADER_SOURCE)?;
        let constant_buffer = create_constant_buffer(&device)?;
        let (output_buffer, output_uav, staging_buffer) = create_output_buffers(&device)?;

        Ok(Self {
            device,
            context,
            compute_shader,
            constant_buffer,
            output_buffer,
            output_uav,
            staging_buffer,
            upload_texture: None,
            upload_srv: None,
            upload_width: 0,
            upload_height: 0,
        })
    }

    fn ensure_upload_texture(&mut self, width: u32, height: u32) -> DomainResult<()> {
        if self.upload_texture.is_some()
            && self.upload_width == width
            && self.upload_height == height
        {
            return Ok(());
        }

        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DYNAMIC,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
            MiscFlags: 0,
        };

        let mut texture: Option<ID3D11Texture2D> = None;
        // SAFETY: Valid desc/output pointers and device lifetime.
        unsafe {
            self.device
                .CreateTexture2D(&desc, None, Some(&mut texture))
                .map_err(|e| {
                    DomainError::GpuTexture(format!("failed to create upload texture: {e:?}"))
                })?;
        }
        let texture = texture.ok_or_else(|| {
            DomainError::GpuTexture("CreateTexture2D returned null texture".to_string())
        })?;

        let srv = create_srv(&self.device, &texture, DXGI_FORMAT_B8G8R8A8_UNORM)?;
        self.upload_texture = Some(texture);
        self.upload_srv = Some(srv);
        self.upload_width = width;
        self.upload_height = height;

        Ok(())
    }

    fn upload_frame(&mut self, frame: &Frame) -> DomainResult<()> {
        let expected = frame.width as usize * frame.height as usize * 4;
        if frame.data.len() != expected {
            return Err(DomainError::Process(format!(
                "invalid frame length: expected {expected}, got {}",
                frame.data.len()
            )));
        }

        self.ensure_upload_texture(frame.width, frame.height)?;
        let texture = self
            .upload_texture
            .as_ref()
            .ok_or_else(|| DomainError::GpuTexture("upload texture unavailable".to_string()))?;

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: Texture created as D3D11_USAGE_DYNAMIC + CPU_WRITE.
        unsafe {
            self.context
                .Map(texture, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped))
                .map_err(|e| {
                    DomainError::GpuTexture(format!("failed to map upload texture: {e:?}"))
                })?;

            if mapped.pData.is_null() {
                self.context.Unmap(texture, 0);
                return Err(DomainError::GpuTexture(
                    "mapped upload texture was null".to_string(),
                ));
            }

            let src_pitch = (frame.width * 4) as usize;
            let dst_pitch = mapped.RowPitch as usize;
            let dst = mapped.pData as *mut u8;

            for row in 0..frame.height as usize {
                let src_off = row * src_pitch;
                let dst_off = row * dst_pitch;
                std::ptr::copy_nonoverlapping(
                    frame.data[src_off..src_off + src_pitch].as_ptr(),
                    dst.add(dst_off),
                    src_pitch,
                );
            }

            self.context.Unmap(texture, 0);
        }

        Ok(())
    }

    fn update_hsv_params(&self, width: u32, height: u32, hsv: &HsvRange) -> DomainResult<()> {
        let params = HsvParams {
            h_low: hsv.h_low as u32,
            h_high: hsv.h_high as u32,
            s_low: hsv.s_low as u32,
            s_high: hsv.s_high as u32,
            v_low: hsv.v_low as u32,
            v_high: hsv.v_high as u32,
            width,
            height,
        };

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: Constant buffer created as dynamic + CPU write.
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
                    DomainError::GpuCompute(format!("failed to map constant buffer: {e:?}"))
                })?;

            if mapped.pData.is_null() {
                self.context.Unmap(&self.constant_buffer, 0);
                return Err(DomainError::GpuCompute(
                    "mapped constant buffer was null".to_string(),
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

    fn run_compute(
        &mut self,
        srv: &ID3D11ShaderResourceView,
        width: u32,
        height: u32,
        hsv: &HsvRange,
    ) -> DomainResult<DetectionResult> {
        if width == 0 || height == 0 {
            return Ok(DetectionResult::not_detected());
        }

        self.update_hsv_params(width, height, hsv)?;

        let clear = [0_u32; 4];
        // SAFETY: UAV is valid and clear values are correctly sized.
        unsafe {
            self.context
                .ClearUnorderedAccessViewUint(&self.output_uav, &clear);
        }

        let srvs = [Some(srv.clone())];
        let uavs = [Some(self.output_uav.clone())];
        let cbs = [Some(self.constant_buffer.clone())];

        // SAFETY: Bound resources are valid and owned by this adapter.
        unsafe {
            self.context.CSSetShaderResources(0, Some(&srvs));
            self.context
                .CSSetUnorderedAccessViews(0, 1, Some(uavs.as_ptr()), None);
            self.context.CSSetConstantBuffers(0, Some(&cbs));
            self.context.CSSetShader(&self.compute_shader, None);

            self.context.Dispatch(
                width.div_ceil(THREAD_GROUP_SIZE),
                height.div_ceil(THREAD_GROUP_SIZE),
                1,
            );

            let null_srvs = [None];
            let null_uavs = [None];
            self.context.CSSetShaderResources(0, Some(&null_srvs));
            self.context
                .CSSetUnorderedAccessViews(0, 1, Some(null_uavs.as_ptr()), None);
            self.context
                .CopyResource(&self.staging_buffer, &self.output_buffer);
        }

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: Staging buffer is CPU readable.
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
                    DomainError::GpuCompute(format!("failed to map readback buffer: {e:?}"))
                })?;

            if mapped.pData.is_null() {
                self.context.Unmap(&self.staging_buffer, 0);
                return Err(DomainError::GpuCompute(
                    "mapped readback buffer was null".to_string(),
                ));
            }

            let result = std::slice::from_raw_parts(mapped.pData as *const u32, 3);
            let pixel_count = result[0];
            let sum_x = result[1];
            let sum_y = result[2];
            self.context.Unmap(&self.staging_buffer, 0);

            if pixel_count == 0 {
                return Ok(DetectionResult::not_detected());
            }

            let px = pixel_count as f32;
            let center_x = sum_x as f32 / px;
            let center_y = sum_y as f32 / px;
            let coverage = (px / (width as f32 * height as f32)).clamp(0.0, 1.0);

            Ok(DetectionResult::detected(center_x, center_y, coverage))
        }
    }
}

impl ProcessPort for GpuColorAdapter {
    fn process_frame(
        &mut self,
        frame: &Frame,
        _roi: &Roi,
        hsv_range: &HsvRange,
    ) -> DomainResult<DetectionResult> {
        self.upload_frame(frame)?;
        let srv = self
            .upload_srv
            .as_ref()
            .ok_or_else(|| DomainError::GpuTexture("upload SRV unavailable".to_string()))?
            .clone();
        self.run_compute(&srv, frame.width, frame.height, hsv_range)
    }

    fn process_gpu_frame(
        &mut self,
        gpu_frame: &GpuFrame,
        hsv_range: &HsvRange,
    ) -> DomainResult<DetectionResult> {
        let texture = gpu_frame.texture.as_ref().ok_or_else(|| {
            DomainError::GpuNotAvailable("GPU texture not available in frame".to_string())
        })?;
        let srv = create_srv(&self.device, texture, gpu_frame.format)?;
        self.run_compute(&srv, gpu_frame.width, gpu_frame.height, hsv_range)
    }

    fn backend(&self) -> ProcessorBackend {
        ProcessorBackend::Gpu
    }

    fn supports_gpu_processing(&self) -> bool {
        true
    }
}

fn compile_shader_blob(source: &str, entry: &str, target: &str) -> DomainResult<ID3DBlob> {
    let entry_c = CString::new(entry)
        .map_err(|_| DomainError::GpuCompute("shader entry contains NUL byte".to_string()))?;
    let target_c = CString::new(target)
        .map_err(|_| DomainError::GpuCompute("shader target contains NUL byte".to_string()))?;

    let mut flags = D3DCOMPILE_ENABLE_STRICTNESS;
    if cfg!(debug_assertions) {
        flags |= D3DCOMPILE_DEBUG;
    } else {
        flags |= D3DCOMPILE_OPTIMIZATION_LEVEL3;
    }

    let mut shader_blob: Option<ID3DBlob> = None;
    let mut error_blob: Option<ID3DBlob> = None;

    // SAFETY: Pointers and lengths are valid for source and C strings.
    let result = unsafe {
        D3DCompile(
            source.as_ptr() as *const _,
            source.len(),
            PCSTR::null(),
            None,
            None,
            PCSTR::from_raw(entry_c.as_ptr() as *const u8),
            PCSTR::from_raw(target_c.as_ptr() as *const u8),
            flags,
            0,
            &mut shader_blob,
            Some(&mut error_blob),
        )
    };

    if let Err(err) = result {
        let details = error_blob
            .as_ref()
            .map(blob_to_string)
            .unwrap_or_else(|| format!("{err:?}"));
        return Err(DomainError::GpuCompute(format!(
            "failed to compile shader (entry={entry}, target={target}): {details}"
        )));
    }

    shader_blob.ok_or_else(|| DomainError::GpuCompute("D3DCompile returned no blob".to_string()))
}

fn compile_compute_shader(
    device: &ID3D11Device,
    source: &str,
) -> DomainResult<ID3D11ComputeShader> {
    let blob = compile_shader_blob(source, SHADER_ENTRY, SHADER_TARGET)?;
    let bytes = unsafe {
        // SAFETY: Blob exposes stable pointer/size for lifetime of blob.
        std::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize())
    };

    let mut shader: Option<ID3D11ComputeShader> = None;
    // SAFETY: Bytecode is valid from D3DCompile.
    unsafe {
        device
            .CreateComputeShader(bytes, None, Some(&mut shader))
            .map_err(|e| {
                DomainError::GpuCompute(format!("failed to create compute shader: {e:?}"))
            })?;
    }

    shader.ok_or_else(|| DomainError::GpuCompute("CreateComputeShader returned null".to_string()))
}

fn create_constant_buffer(device: &ID3D11Device) -> DomainResult<ID3D11Buffer> {
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: size_of::<HsvParams>() as u32,
        Usage: D3D11_USAGE_DYNAMIC,
        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
        CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
        MiscFlags: 0,
        StructureByteStride: 0,
    };

    let mut buffer: Option<ID3D11Buffer> = None;
    // SAFETY: Valid desc/output pointers and device lifetime.
    unsafe {
        device
            .CreateBuffer(&desc, None, Some(&mut buffer))
            .map_err(|e| {
                DomainError::GpuCompute(format!("failed to create constant buffer: {e:?}"))
            })?;
    }

    buffer.ok_or_else(|| {
        DomainError::GpuCompute("CreateBuffer returned null constant buffer".to_string())
    })
}

fn create_output_buffers(
    device: &ID3D11Device,
) -> DomainResult<(ID3D11Buffer, ID3D11UnorderedAccessView, ID3D11Buffer)> {
    let stride = size_of::<u32>() as u32;
    let size = stride * 3;

    let output_desc = D3D11_BUFFER_DESC {
        ByteWidth: size,
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_UNORDERED_ACCESS.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: D3D11_RESOURCE_MISC_BUFFER_STRUCTURED.0 as u32,
        StructureByteStride: stride,
    };

    let mut output_buffer: Option<ID3D11Buffer> = None;
    // SAFETY: Valid desc/output pointers and device lifetime.
    unsafe {
        device
            .CreateBuffer(&output_desc, None, Some(&mut output_buffer))
            .map_err(|e| {
                DomainError::GpuCompute(format!("failed to create output buffer: {e:?}"))
            })?;
    }
    let output_buffer = output_buffer.ok_or_else(|| {
        DomainError::GpuCompute("CreateBuffer returned null output buffer".to_string())
    })?;

    let uav_desc = D3D11_UNORDERED_ACCESS_VIEW_DESC {
        Format: DXGI_FORMAT_UNKNOWN,
        ViewDimension: D3D11_UAV_DIMENSION_BUFFER,
        Anonymous: D3D11_UNORDERED_ACCESS_VIEW_DESC_0 {
            Buffer: D3D11_BUFFER_UAV {
                FirstElement: 0,
                NumElements: 3,
                Flags: 0,
            },
        },
    };

    let mut uav: Option<ID3D11UnorderedAccessView> = None;
    // SAFETY: Valid desc/output pointers and resource lifetime.
    unsafe {
        device
            .CreateUnorderedAccessView(&output_buffer, Some(&uav_desc), Some(&mut uav))
            .map_err(|e| DomainError::GpuCompute(format!("failed to create output UAV: {e:?}")))?;
    }
    let uav = uav.ok_or_else(|| {
        DomainError::GpuCompute("CreateUnorderedAccessView returned null".to_string())
    })?;

    let staging_desc = D3D11_BUFFER_DESC {
        ByteWidth: size,
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: D3D11_RESOURCE_MISC_BUFFER_STRUCTURED.0 as u32,
        StructureByteStride: stride,
    };

    let mut staging: Option<ID3D11Buffer> = None;
    // SAFETY: Valid desc/output pointers and device lifetime.
    unsafe {
        device
            .CreateBuffer(&staging_desc, None, Some(&mut staging))
            .map_err(|e| {
                DomainError::GpuCompute(format!("failed to create staging buffer: {e:?}"))
            })?;
    }
    let staging = staging.ok_or_else(|| {
        DomainError::GpuCompute("CreateBuffer returned null staging buffer".to_string())
    })?;

    Ok((output_buffer, uav, staging))
}

fn create_srv(
    device: &ID3D11Device,
    texture: &ID3D11Texture2D,
    format: DXGI_FORMAT,
) -> DomainResult<ID3D11ShaderResourceView> {
    if format != DXGI_FORMAT_B8G8R8A8_UNORM {
        return Err(DomainError::GpuTexture(format!(
            "unsupported texture format for GPU processing: {format:?}"
        )));
    }

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
    // SAFETY: Valid desc/output pointers and resource lifetime.
    unsafe {
        device
            .CreateShaderResourceView(texture, Some(&desc), Some(&mut srv))
            .map_err(|e| DomainError::GpuTexture(format!("failed to create SRV: {e:?}")))?;
    }

    srv.ok_or_else(|| DomainError::GpuTexture("CreateShaderResourceView returned null".to_string()))
}

fn blob_to_string(blob: &ID3DBlob) -> String {
    // SAFETY: Blob returns valid pointer + byte count.
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
    use windows::Win32::Graphics::Direct3D11::D3D11_BIND_SHADER_RESOURCE;

    #[test]
    fn backend_returns_gpu() {
        let adapter = GpuColorAdapter::new();
        if let Ok(adapter) = adapter {
            assert_eq!(adapter.backend(), ProcessorBackend::Gpu);
        }
    }

    #[test]
    fn supports_gpu_processing_returns_true() {
        let adapter = GpuColorAdapter::new();
        if let Ok(adapter) = adapter {
            assert!(adapter.supports_gpu_processing());
        }
    }

    #[test]
    fn d3dcompile_failure_returns_gpu_compute_error() {
        let err = compile_shader_blob("this is not hlsl", SHADER_ENTRY, SHADER_TARGET)
            .expect_err("invalid shader source should fail");

        match err {
            DomainError::GpuCompute(message) => {
                assert!(message.contains("failed to compile shader"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    #[ignore = "Requires D3D11 runtime and GPU/WARP availability"]
    fn process_gpu_frame_without_texture_returns_not_available() {
        let mut adapter = GpuColorAdapter::new().expect("adapter creation should succeed");
        let frame = GpuFrame::new(None, 64, 64, DXGI_FORMAT_B8G8R8A8_UNORM);
        let hsv = HsvRange::new(20, 40, 100, 255, 100, 255);

        let err = adapter
            .process_gpu_frame(&frame, &hsv)
            .expect_err("missing texture should error");
        assert!(matches!(err, DomainError::GpuNotAvailable(_)));
    }

    #[test]
    #[ignore = "Requires D3D11 runtime and GPU/WARP availability"]
    fn process_frame_executes_compute_path() {
        let mut adapter = GpuColorAdapter::new().expect("adapter creation should succeed");
        let frame = Frame::new(vec![0; 32 * 32 * 4], 32, 32);
        let roi = Roi::new(0, 0, 32, 32);
        let hsv = HsvRange::new(20, 40, 100, 255, 100, 255);

        let result = adapter
            .process_frame(&frame, &roi, &hsv)
            .expect("processing should succeed");
        assert!(result.coverage >= 0.0);
    }

    #[test]
    #[ignore = "Requires D3D11 runtime and GPU/WARP availability"]
    fn create_srv_rejects_non_bgra_texture() {
        let (device, _context) = create_d3d11_device().expect("device should be available");
        let desc = D3D11_TEXTURE2D_DESC {
            Width: 4,
            Height: 4,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_UNKNOWN,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut texture = None;
        // SAFETY: Valid CreateTexture2D invocation for test resource.
        unsafe {
            device
                .CreateTexture2D(&desc, None, Some(&mut texture))
                .expect("texture create should succeed");
        }
        let texture = texture.expect("texture should exist");
        let err = create_srv(&device, &texture, DXGI_FORMAT_UNKNOWN)
            .expect_err("unsupported format should fail");
        assert!(matches!(err, DomainError::GpuTexture(_)));
    }
}
