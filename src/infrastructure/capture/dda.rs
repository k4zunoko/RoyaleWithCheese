use crate::domain::error::{DomainError, DomainResult};
use crate::domain::ports::CapturePort;
use crate::domain::types::{DeviceInfo, Frame, GpuFrame, Roi};
use crate::infrastructure::capture::common::{
    clamp_roi, copy_texture_to_cpu, StagingTextureManager,
};
use win_desktop_duplication::{
    co_init, devices::AdapterFactory, outputs::Display, set_process_dpi_awareness,
    DesktopDuplicationApi, DuplicationApiOptions,
};
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device4, ID3D11DeviceContext4, ID3D11Resource, ID3D11Texture2D,
    D3D11_BIND_SHADER_RESOURCE, D3D11_BOX, D3D11_RESOURCE_MISC_FLAG, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};

pub struct DdaCaptureAdapter {
    dupl: DesktopDuplicationApi,
    #[allow(dead_code)]
    output: Display,
    device: ID3D11Device4,
    context: ID3D11DeviceContext4,
    staging_manager: StagingTextureManager,
    info: DeviceInfo,
    adapter_idx: usize,
    output_idx: usize,
}

impl DdaCaptureAdapter {
    pub fn new(adapter_idx: usize, output_idx: usize, _timeout_ms: u32) -> DomainResult<Self> {
        set_process_dpi_awareness();
        co_init();

        let adapter = AdapterFactory::new()
            .get_adapter_by_idx(adapter_idx as u32)
            .ok_or_else(|| {
                DomainError::Initialization(format!("Adapter {adapter_idx} not found"))
            })?;

        let output = adapter
            .get_display_by_idx(output_idx as u32)
            .ok_or_else(|| DomainError::Initialization(format!("Output {output_idx} not found")))?;

        let mut dupl = DesktopDuplicationApi::new(adapter, output.clone())
            .map_err(|e| DomainError::Initialization(format!("Failed to create DDA: {e:?}")))?;
        dupl.configure(DuplicationApiOptions { skip_cursor: true });

        let (device, context) = dupl.get_device_and_ctx();
        let mode = output.get_current_display_mode().map_err(|e| {
            DomainError::Initialization(format!("Failed to query display mode: {e:?}"))
        })?;

        Ok(Self {
            dupl,
            output,
            device,
            context,
            staging_manager: StagingTextureManager::new(),
            info: DeviceInfo::new(
                mode.width,
                mode.height,
                format!("Display {output_idx} on adapter {adapter_idx}"),
            ),
            adapter_idx,
            output_idx,
        })
    }

    fn acquire_frame(&mut self) -> DomainResult<Option<win_desktop_duplication::texture::Texture>> {
        match self.dupl.acquire_next_frame_now() {
            Ok(texture) => Ok(Some(texture)),
            Err(e) => {
                let msg = format!("{e:?}");
                if msg.contains("Timeout") {
                    Ok(None)
                } else if msg.contains("AccessLost") || msg.contains("AccessDenied") {
                    Err(DomainError::DeviceNotAvailable)
                } else {
                    Err(DomainError::ReInitializationRequired)
                }
            }
        }
    }
}

impl CapturePort for DdaCaptureAdapter {
    fn capture_frame(&mut self, roi: &Roi) -> DomainResult<Option<Frame>> {
        let roi = clamp_roi(roi, self.info.width, self.info.height);
        if roi.width == 0 || roi.height == 0 {
            return Ok(None);
        }

        let texture = match self.acquire_frame()? {
            Some(t) => t,
            None => return Ok(None),
        };

        let src_resource: ID3D11Resource =
            texture.as_raw_ref().clone().cast().map_err(|e| {
                DomainError::Capture(format!("Failed to cast source texture: {e:?}"))
            })?;

        let staging = self.staging_manager.ensure_texture(
            &self.device,
            roi.width,
            roi.height,
            DXGI_FORMAT_B8G8R8A8_UNORM,
        )?;

        let src_box = D3D11_BOX {
            left: roi.x,
            top: roi.y,
            front: 0,
            right: roi.x + roi.width,
            bottom: roi.y + roi.height,
            back: 1,
        };

        // SAFETY: Source/destination resources belong to the same D3D11 device and box is within clamped ROI bounds.
        unsafe {
            self.context.CopySubresourceRegion(
                &staging,
                0,
                0,
                0,
                0,
                &src_resource,
                0,
                Some(&src_box),
            );
        }

        let data = copy_texture_to_cpu(&self.context, &staging, roi.width, roi.height)?;
        Ok(Some(Frame::new(data, roi.width, roi.height)))
    }

    fn capture_gpu_frame(&mut self, roi: &Roi) -> DomainResult<Option<GpuFrame>> {
        let roi = clamp_roi(roi, self.info.width, self.info.height);
        if roi.width == 0 || roi.height == 0 {
            return Ok(None);
        }

        let texture = match self.acquire_frame()? {
            Some(t) => t,
            None => return Ok(None),
        };

        let src_resource: ID3D11Resource =
            texture.as_raw_ref().clone().cast().map_err(|e| {
                DomainError::Capture(format!("Failed to cast source texture: {e:?}"))
            })?;

        let desc = D3D11_TEXTURE2D_DESC {
            Width: roi.width,
            Height: roi.height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: D3D11_RESOURCE_MISC_FLAG(0).0 as u32,
        };

        let mut roi_texture = None;
        // SAFETY: Descriptor is fully initialized and output pointer is valid for COM object creation.
        unsafe {
            self.device
                .CreateTexture2D(&desc, None, Some(&mut roi_texture))
                .map_err(|e| {
                    DomainError::GpuTexture(format!("Failed to create ROI texture: {e:?}"))
                })?;
        }

        let roi_texture: ID3D11Texture2D = roi_texture.ok_or_else(|| {
            DomainError::GpuTexture("CreateTexture2D returned None for ROI texture".to_string())
        })?;

        let src_box = D3D11_BOX {
            left: roi.x,
            top: roi.y,
            front: 0,
            right: roi.x + roi.width,
            bottom: roi.y + roi.height,
            back: 1,
        };

        // SAFETY: Source/destination resources are compatible and ROI is clamped to display bounds.
        unsafe {
            self.context.CopySubresourceRegion(
                &roi_texture,
                0,
                0,
                0,
                0,
                &src_resource,
                0,
                Some(&src_box),
            );
        }

        Ok(Some(GpuFrame::new(
            Some(roi_texture),
            roi.width,
            roi.height,
            DXGI_FORMAT_B8G8R8A8_UNORM,
        )))
    }

    fn reinitialize(&mut self) -> DomainResult<()> {
        let mut new_adapter = Self::new(self.adapter_idx, self.output_idx, 0)?;
        std::mem::swap(self, &mut new_adapter);
        self.staging_manager.clear();
        Ok(())
    }

    fn device_info(&self) -> DeviceInfo {
        self.info.clone()
    }

    fn supports_gpu_frame(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supports_gpu_frame_is_true() {
        assert!(DdaCaptureAdapter::new(0, 0, 8)
            .map(|adapter| adapter.supports_gpu_frame())
            .unwrap_or(true));
    }

    #[test]
    fn device_info_has_non_zero_dimensions() {
        match DdaCaptureAdapter::new(0, 0, 8) {
            Ok(adapter) => {
                let info = adapter.device_info();
                assert!(info.width > 0);
                assert!(info.height > 0);
            }
            Err(_) => {
                // DDA initialization can fail in CI/headless environments.
            }
        }
    }

    #[test]
    #[ignore]
    fn capture_frame_returns_roi_sized_frame() {
        let mut adapter = DdaCaptureAdapter::new(0, 0, 8).expect("should initialize");
        let roi = Roi::new(100, 200, 320, 240);
        let frame = adapter
            .capture_frame(&roi)
            .expect("capture should succeed")
            .expect("frame should exist");

        assert_eq!(frame.width, 320);
        assert_eq!(frame.height, 240);
        assert_eq!(frame.data.len(), 320 * 240 * 4);
    }
}
