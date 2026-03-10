use crate::domain::error::{DomainError, DomainResult};
use crate::domain::types::Roi;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_BIND_FLAG, D3D11_CPU_ACCESS_FLAG,
    D3D11_CPU_ACCESS_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_RESOURCE_MISC_FLAG,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};

pub fn clamp_roi(roi: &Roi, screen_width: u32, screen_height: u32) -> Roi {
    if screen_width == 0 || screen_height == 0 || roi.width == 0 || roi.height == 0 {
        return Roi::new(0, 0, 0, 0);
    }

    if roi.x >= screen_width || roi.y >= screen_height {
        return Roi::new(0, 0, 0, 0);
    }

    let clamped_width = roi.width.min(screen_width.saturating_sub(roi.x));
    let clamped_height = roi.height.min(screen_height.saturating_sub(roi.y));

    Roi::new(roi.x, roi.y, clamped_width, clamped_height)
}

pub struct StagingTextureManager {
    texture: Option<ID3D11Texture2D>,
    width: u32,
    height: u32,
    format: DXGI_FORMAT,
}

impl StagingTextureManager {
    pub fn new() -> Self {
        Self {
            texture: None,
            width: 0,
            height: 0,
            format: DXGI_FORMAT_UNKNOWN,
        }
    }

    pub fn ensure_texture(
        &mut self,
        device: &ID3D11Device,
        width: u32,
        height: u32,
        format: DXGI_FORMAT,
    ) -> DomainResult<ID3D11Texture2D> {
        if let Some(existing) = &self.texture {
            if self.width == width && self.height == height && self.format == format {
                return Ok(existing.clone());
            }
        }

        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: D3D11_BIND_FLAG(0).0 as u32,
            CPUAccessFlags: D3D11_CPU_ACCESS_FLAG(D3D11_CPU_ACCESS_READ.0).0 as u32,
            MiscFlags: D3D11_RESOURCE_MISC_FLAG(0).0 as u32,
        };

        let mut texture = None;
        // SAFETY: COM out-parameter is valid for this call, and `desc` points to initialized data.
        unsafe {
            device
                .CreateTexture2D(&desc, None, Some(&mut texture))
                .map_err(|e| {
                    DomainError::Capture(format!("Failed to create staging texture: {e:?}"))
                })?;
        }

        let created = texture.ok_or_else(|| {
            DomainError::Capture("CreateTexture2D returned None for staging texture".to_string())
        })?;

        self.texture = Some(created.clone());
        self.width = width;
        self.height = height;
        self.format = format;
        Ok(created)
    }

    pub fn clear(&mut self) {
        self.texture = None;
        self.width = 0;
        self.height = 0;
        self.format = DXGI_FORMAT_UNKNOWN;
    }
}

impl Default for StagingTextureManager {
    fn default() -> Self {
        Self::new()
    }
}

pub fn copy_texture_to_cpu(
    context: &ID3D11DeviceContext,
    staging_tex: &ID3D11Texture2D,
    width: u32,
    height: u32,
) -> DomainResult<Vec<u8>> {
    let row_size = (width as usize) * 4;
    let mut buffer = vec![0_u8; row_size * height as usize];
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();

    // SAFETY: `staging_tex` was created as CPU-readable staging texture; `mapped` is valid out storage.
    unsafe {
        context
            .Map(staging_tex, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .map_err(|e| DomainError::Capture(format!("Failed to map staging texture: {e:?}")))?;

        let src = mapped.pData as *const u8;
        let src_pitch = mapped.RowPitch as usize;

        for row in 0..(height as usize) {
            let src_row = src.add(row * src_pitch);
            let dst_row = buffer.as_mut_ptr().add(row * row_size);
            std::ptr::copy_nonoverlapping(src_row, dst_row, row_size);
        }

        context.Unmap(staging_tex, 0);
    }

    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_roi_truncates_to_bounds() {
        let roi = Roi::new(1800, 1000, 400, 300);
        let clamped = clamp_roi(&roi, 1920, 1080);

        assert_eq!(clamped.x, 1800);
        assert_eq!(clamped.y, 1000);
        assert_eq!(clamped.width, 120);
        assert_eq!(clamped.height, 80);
    }

    #[test]
    fn clamp_roi_returns_zero_when_outside() {
        let roi = Roi::new(2000, 1200, 300, 300);
        let clamped = clamp_roi(&roi, 1920, 1080);

        assert_eq!(clamped.width, 0);
        assert_eq!(clamped.height, 0);
    }

    #[test]
    fn staging_texture_manager_default_is_empty() {
        let manager = StagingTextureManager::new();
        assert!(manager.texture.is_none());
        assert_eq!(manager.width, 0);
        assert_eq!(manager.height, 0);
        assert_eq!(manager.format, DXGI_FORMAT_UNKNOWN);
    }
}
