//! キャプチャ実装の共通ユーティリティ
//!
//! DDA/Spout両方で使用される共通処理を提供。
//! - ステージングテクスチャ管理
//! - ROIクランプ
//! - GPU→CPU転送

use crate::domain::{DomainError, DomainResult, Roi};
use std::mem;
use std::ptr;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;

/// ステージングテクスチャ管理
///
/// ROIサイズが同じ間は既存テクスチャを再利用し、
/// GPUリソースの再割り当てを最小化する。
///
/// # パフォーマンス
/// - 同一サイズ・フォーマットならテクスチャ再利用
/// - サイズ変更時のみ新規作成
pub struct StagingTextureManager {
    staging_tex: Option<ID3D11Texture2D>,
    staging_size: (u32, u32),
    staging_format: DXGI_FORMAT,
}

impl StagingTextureManager {
    /// 新しいステージングテクスチャマネージャを作成
    pub fn new() -> Self {
        Self {
            staging_tex: None,
            staging_size: (0, 0),
            staging_format: DXGI_FORMAT_UNKNOWN,
        }
    }

    /// ステージングテクスチャを確保または再利用
    ///
    /// # Arguments
    /// - `device`: D3D11デバイス
    /// - `width`: テクスチャ幅
    /// - `height`: テクスチャ高さ
    /// - `format`: ピクセルフォーマット（DXGI_FORMAT）
    ///
    /// # Returns
    /// - `Ok(ID3D11Texture2D)`: ステージングテクスチャ
    /// - `Err(DomainError)`: テクスチャ作成失敗
    pub fn ensure_texture(
        &mut self,
        device: &ID3D11Device,
        width: u32,
        height: u32,
        format: DXGI_FORMAT,
    ) -> DomainResult<ID3D11Texture2D> {
        // サイズとフォーマットが同じで既にテクスチャがあれば再利用
        if let Some(ref tex) = self.staging_tex {
            if self.staging_size == (width, height) && self.staging_format == format {
                return Ok(tex.clone());
            }
        }

        // 新しいステージングテクスチャを作成
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
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: D3D11_RESOURCE_MISC_FLAG(0).0 as u32,
        };

        let mut staging_tex: Option<ID3D11Texture2D> = None;
        unsafe {
            device
                .CreateTexture2D(&desc, None, Some(&mut staging_tex))
                .map_err(|e| {
                    DomainError::Capture(format!("Failed to create staging texture: {:?}", e))
                })?;
        }

        let tex = staging_tex.ok_or_else(|| {
            DomainError::Capture("Staging texture creation returned None".to_string())
        })?;

        // キャッシュに保存
        self.staging_tex = Some(tex.clone());
        self.staging_size = (width, height);
        self.staging_format = format;

        Ok(tex)
    }

    /// ステージングテクスチャをクリア
    ///
    /// サイズ変更時や再初期化時に呼び出す。
    pub fn clear(&mut self) {
        self.staging_tex = None;
        self.staging_size = (0, 0);
        self.staging_format = DXGI_FORMAT_UNKNOWN;
    }

    /// 現在のステージングサイズを取得
    #[allow(dead_code)]
    pub fn size(&self) -> (u32, u32) {
        self.staging_size
    }
}

impl Default for StagingTextureManager {
    fn default() -> Self {
        Self::new()
    }
}

/// ROIを境界内にクランプ
///
/// ROIが境界外にはみ出している場合、境界内に収まるように調整。
/// ROIが完全に境界外の場合はNoneを返す。
///
/// # Arguments
/// - `roi`: クランプ対象のROI
/// - `bounds_width`: 境界の幅
/// - `bounds_height`: 境界の高さ
///
/// # Returns
/// - `Some(Roi)`: クランプされたROI
/// - `None`: ROIが無効または完全に境界外
pub fn clamp_roi(roi: &Roi, bounds_width: u32, bounds_height: u32) -> Option<Roi> {
    // 境界またはROIのサイズが0なら無効
    if bounds_width == 0 || bounds_height == 0 || roi.width == 0 || roi.height == 0 {
        return None;
    }

    // ROIが完全に境界外ならNone
    if roi.x >= bounds_width || roi.y >= bounds_height {
        return None;
    }

    // 境界内に収まるようにクランプ
    let clamped_x = roi.x.min(bounds_width);
    let clamped_y = roi.y.min(bounds_height);
    let max_w = bounds_width.saturating_sub(clamped_x);
    let max_h = bounds_height.saturating_sub(clamped_y);
    let clamped_width = roi.width.min(max_w);
    let clamped_height = roi.height.min(max_h);

    // クランプ後のサイズが0なら無効
    if clamped_width == 0 || clamped_height == 0 {
        return None;
    }

    Some(Roi::new(clamped_x, clamped_y, clamped_width, clamped_height))
}

/// ROI領域をソーステクスチャからステージングテクスチャへコピー
///
/// GPU上でROI領域のみをCopySubresourceRegionでコピーする。
///
/// # Arguments
/// - `context`: D3D11デバイスコンテキスト
/// - `src_resource`: ソースリソース（テクスチャ）
/// - `staging_tex`: 宛先ステージングテクスチャ
/// - `roi`: コピーするROI領域
///
/// # Safety
/// - `context`と`src_resource`、`staging_tex`は同じデバイスに属している必要がある
pub fn copy_roi_to_staging(
    context: &ID3D11DeviceContext,
    src_resource: &ID3D11Resource,
    staging_tex: &ID3D11Texture2D,
    roi: &Roi,
) {
    unsafe {
        let src_box = D3D11_BOX {
            left: roi.x,
            top: roi.y,
            front: 0,
            right: roi.x + roi.width,
            bottom: roi.y + roi.height,
            back: 1,
        };

        context.CopySubresourceRegion(staging_tex, 0, 0, 0, 0, src_resource, 0, Some(&src_box));
    }
}

/// GPUからCPUへのテクスチャデータ転送
///
/// RowPitchを考慮してステージングテクスチャからVec<u8>にコピー。
///
/// # Arguments
/// - `context`: D3D11デバイスコンテキスト
/// - `staging_tex`: ステージングテクスチャ（D3D11_USAGE_STAGING）
/// - `width`: テクスチャ幅
/// - `height`: テクスチャ高さ
///
/// # Returns
/// - `Ok(Vec<u8>)`: BGRA形式のピクセルデータ
/// - `Err(DomainError)`: Map/Unmap失敗
///
/// # Safety
/// - `context`と`staging_tex`は同じデバイスに属している必要がある
/// - `staging_tex`はD3D11_USAGE_STAGINGで作成されている必要がある
pub fn copy_texture_to_cpu(
    context: &ID3D11DeviceContext,
    staging_tex: &ID3D11Texture2D,
    width: u32,
    height: u32,
) -> DomainResult<Vec<u8>> {
    // データサイズを計算（BGRA形式: 4バイト/ピクセル）
    let data_size = (width * height * 4) as usize;
    let mut data = vec![0u8; data_size];

    unsafe {
        let mut mapped: D3D11_MAPPED_SUBRESOURCE = mem::zeroed();
        context
            .Map(staging_tex, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .map_err(|e| {
                DomainError::Capture(format!("Failed to map staging texture: {:?}", e))
            })?;

        // RowPitchを考慮してデータをコピー
        let row_pitch = mapped.RowPitch as usize;
        let row_size = (width * 4) as usize;

        for y in 0..height as usize {
            let src_offset = y * row_pitch;
            let dst_offset = y * row_size;

            ptr::copy_nonoverlapping(
                (mapped.pData as *const u8).add(src_offset),
                data.as_mut_ptr().add(dst_offset),
                row_size,
            );
        }

        context.Unmap(staging_tex, 0);
    }

    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clamp_roi_valid() {
        let roi = Roi::new(100, 100, 400, 300);
        let clamped = clamp_roi(&roi, 1920, 1080);

        assert!(clamped.is_some());
        let c = clamped.unwrap();
        assert_eq!(c.x, 100);
        assert_eq!(c.y, 100);
        assert_eq!(c.width, 400);
        assert_eq!(c.height, 300);
    }

    #[test]
    fn test_clamp_roi_exceeds_bounds() {
        let roi = Roi::new(1800, 1000, 400, 300);
        let clamped = clamp_roi(&roi, 1920, 1080);

        assert!(clamped.is_some());
        let c = clamped.unwrap();
        assert_eq!(c.x, 1800);
        assert_eq!(c.y, 1000);
        assert_eq!(c.width, 120); // 1920 - 1800
        assert_eq!(c.height, 80); // 1080 - 1000
    }

    #[test]
    fn test_clamp_roi_completely_outside() {
        let roi = Roi::new(2000, 1200, 400, 300);
        let clamped = clamp_roi(&roi, 1920, 1080);

        assert!(clamped.is_none());
    }

    #[test]
    fn test_clamp_roi_zero_size() {
        let roi = Roi::new(100, 100, 0, 0);
        let clamped = clamp_roi(&roi, 1920, 1080);

        assert!(clamped.is_none());
    }

    #[test]
    fn test_clamp_roi_zero_bounds() {
        let roi = Roi::new(100, 100, 400, 300);
        let clamped = clamp_roi(&roi, 0, 0);

        assert!(clamped.is_none());
    }

    #[test]
    fn test_staging_texture_manager_default() {
        let manager = StagingTextureManager::default();
        assert_eq!(manager.size(), (0, 0));
    }

    #[test]
    fn test_staging_texture_manager_clear() {
        let mut manager = StagingTextureManager::new();
        manager.staging_size = (100, 100);
        manager.clear();
        assert_eq!(manager.size(), (0, 0));
    }
}
