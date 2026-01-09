//! Spout DX11 テクスチャ受信によるキャプチャアダプタ
//!
//! Spout送信されたDirectX 11テクスチャを受信し、
//! CapturePort traitを実装します。

use crate::domain::{CapturePort, DeviceInfo, DomainError, DomainResult, Frame, Roi};
use crate::infrastructure::capture::spout_ffi::*;
use std::ffi::CString;
use std::ptr;
use std::time::Instant;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::core::Interface;

/// Spoutキャプチャアダプタ
/// 
/// CapturePort traitを実装し、Spout送信されたDX11テクスチャを受信します。
/// USAGE_DLL.mdに従い、内部テクスチャ受信方式（spoutdx_receiver_receive + get_received_texture）を使用。
pub struct SpoutCaptureAdapter {
    // FFIハンドル（Send/Syncを実装するためのラッパー）
    receiver: SpoutDxReceiverHandle,
    
    // DirectX 11 デバイス（自前で作成、アダプタ整合性のため）
    #[allow(dead_code)]  // SpoutDXのコンテキストを使用するため未使用だが保持が必要
    device: ID3D11Device,
    #[allow(dead_code)]
    context: ID3D11DeviceContext,
    
    // ROI切り出し用ステージングテクスチャ
    staging_tex: Option<ID3D11Texture2D>,
    staging_size: (u32, u32),
    
    // 送信者情報
    #[allow(dead_code)]  // 再初期化時に使用
    sender_name: Option<String>,
    sender_info: SpoutDxSenderInfo,
    
    // デバイス情報（CapturePort用）
    device_info: DeviceInfo,
}

impl SpoutCaptureAdapter {
    /// 新しいSpoutキャプチャアダプタを作成
    ///
    /// # Arguments
    /// - `sender_name`: 接続する送信者名（Noneで自動選択）
    ///
    /// # Returns
    /// - `Ok(SpoutCaptureAdapter)`: 初期化成功
    /// - `Err(DomainError)`: 初期化失敗
    pub fn new(sender_name: Option<String>) -> DomainResult<Self> {
        // D3D11デバイスを作成
        let (device, context) = Self::create_d3d11_device()?;
        
        // Spoutレシーバーを作成
        let receiver = unsafe { spoutdx_receiver_create() };
        if receiver.is_null() {
            return Err(DomainError::Initialization(
                "Failed to create Spout receiver".to_string()
            ));
        }
        
        // D3D11デバイスをSpoutに渡す
        let device_ptr = device.as_raw() as *mut std::ffi::c_void;
        let result = unsafe { spoutdx_receiver_open_dx11(receiver, device_ptr) };
        if !SpoutDxResult::from_raw(result).is_ok() {
            unsafe { spoutdx_receiver_destroy(receiver); }
            return Err(DomainError::Initialization(
                format!("Failed to open DX11 for Spout: {:?}", SpoutDxResult::from_raw(result))
            ));
        }
        
        // 送信者名を設定（指定があれば）
        if let Some(ref name) = sender_name {
            let c_name = CString::new(name.as_str())
                .map_err(|_| DomainError::Configuration("Invalid sender name".to_string()))?;
            unsafe { spoutdx_receiver_set_sender_name(receiver, c_name.as_ptr()); }
        } else {
            // NULLで自動選択
            unsafe { spoutdx_receiver_set_sender_name(receiver, ptr::null()); }
        }
        
        // 初期デバイス情報（接続後に更新）
        let device_info = DeviceInfo {
            width: 0,
            height: 0,
            refresh_rate: 0,  // Spoutでは不明
            name: sender_name.clone().unwrap_or_else(|| "Spout (auto)".to_string()),
        };
        
        #[cfg(debug_assertions)]
        tracing::info!("Spout receiver initialized: sender_name={:?}", sender_name);
        
        Ok(Self {
            receiver,
            device,
            context,
            staging_tex: None,
            staging_size: (0, 0),
            sender_name,
            sender_info: SpoutDxSenderInfo::default(),
            device_info,
        })
    }
    
    /// D3D11デバイスを作成
    fn create_d3d11_device() -> DomainResult<(ID3D11Device, ID3D11DeviceContext)> {
        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;
        
        unsafe {
            D3D11CreateDevice(
                None,  // デフォルトアダプタ
                D3D_DRIVER_TYPE_HARDWARE,
                None,
                D3D11_CREATE_DEVICE_FLAG(0),
                None,  // 機能レベル自動選択
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            ).map_err(|e| DomainError::Initialization(
                format!("Failed to create D3D11 device: {:?}", e)
            ))?;
        }
        
        let device = device.ok_or_else(|| 
            DomainError::Initialization("D3D11 device creation returned None".to_string())
        )?;
        let context = context.ok_or_else(|| 
            DomainError::Initialization("D3D11 context creation returned None".to_string())
        )?;
        
        Ok((device, context))
    }
    
    /// ステージングテクスチャを確保（ROIサイズ用）
    /// 
    /// # パフォーマンス最適化
    /// - ROIサイズが同じであれば既存のテクスチャを再利用
    /// - サイズ変更時のみ再作成し、GPUリソースの再割り当てを最小化
    /// 
    /// # 注意
    /// USAGE_DLL.mdに従い、送信者のフォーマットに合わせたステージングテクスチャを作成
    fn ensure_staging_texture(
        &mut self, 
        width: u32, 
        height: u32, 
        format: DXGI_FORMAT,
        device: &ID3D11Device,
    ) -> DomainResult<ID3D11Texture2D> {
        if let Some(ref tex) = self.staging_tex {
            if self.staging_size == (width, height) {
                return Ok(tex.clone());
            }
        }
        
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: format,  // 送信者のフォーマットに合わせる
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: D3D11_BIND_FLAG(0).0 as u32,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: D3D11_RESOURCE_MISC_FLAG(0).0 as u32,
        };
        
        let mut tex: Option<ID3D11Texture2D> = None;
        unsafe {
            device.CreateTexture2D(&desc, None, Some(&mut tex))
                .map_err(|e| DomainError::Capture(
                    format!("Failed to create staging texture: {:?}", e)
                ))?;
        }
        
        let tex = tex.ok_or_else(||
            DomainError::Capture("Staging texture creation returned None".to_string())
        )?;
        
        self.staging_tex = Some(tex.clone());
        self.staging_size = (width, height);
        
        Ok(tex)
    }
    
    /// ROIを画面サイズ内にクランプ
    /// 
    /// ROIが画面外にはみ出している場合、画面内に収まるように調整。
    /// ROIが完全に画面外の場合はNoneを返す。
    fn clamp_roi(&self, roi: &Roi) -> Option<Roi> {
        let w = self.device_info.width;
        let h = self.device_info.height;
        
        if w == 0 || h == 0 || roi.width == 0 || roi.height == 0 {
            return None;
        }
        if roi.x >= w || roi.y >= h {
            return None;
        }
        
        let clamped_x = roi.x.min(w);
        let clamped_y = roi.y.min(h);
        let max_w = w.saturating_sub(clamped_x);
        let max_h = h.saturating_sub(clamped_y);
        let clamped_width = roi.width.min(max_w);
        let clamped_height = roi.height.min(max_h);
        
        if clamped_width == 0 || clamped_height == 0 {
            return None;
        }
        
        Some(Roi::new(clamped_x, clamped_y, clamped_width, clamped_height))
    }
}

impl CapturePort for SpoutCaptureAdapter {
    fn capture_frame_with_roi(&mut self, roi: &Roi) -> DomainResult<Option<Frame>> {
        // 内部テクスチャへ受信（USAGE_DLL.md推奨方式）
        let result = unsafe { spoutdx_receiver_receive(self.receiver) };
        
        if !SpoutDxResult::from_raw(result).is_ok() {
            let err = SpoutDxResult::from_raw(result);
            match err {
                SpoutDxResult::ErrorNotConnected => return Ok(None),
                SpoutDxResult::ErrorReceiveFailed => return Err(DomainError::DeviceNotAvailable),
                SpoutDxResult::ErrorNullHandle | SpoutDxResult::ErrorNullDevice => {
                    return Err(DomainError::ReInitializationRequired)
                }
                _ => return Err(DomainError::Capture(format!("Spout receive failed: {:?}", err)))
            };
        }
        
        // 新しいフレームがあるかチェック
        let is_new = unsafe { spoutdx_receiver_is_frame_new(self.receiver) };
        if is_new == 0 {
            return Ok(None);  // 更新なし
        }
        
        // 内部受信テクスチャを取得
        let received_tex_ptr = unsafe { spoutdx_receiver_get_received_texture(self.receiver) };
        if received_tex_ptr.is_null() {
            #[cfg(debug_assertions)]
            tracing::trace!("No received texture available");
            return Ok(None);
        }
        
        // ID3D11Texture2Dとして扱う
        let received_tex: ID3D11Texture2D = unsafe {
            ID3D11Texture2D::from_raw_borrowed(&received_tex_ptr)
                .ok_or_else(|| DomainError::Capture("Failed to get received texture".to_string()))?
                .clone()
        };
        
        // SpoutDX側のD3D11コンテキストを取得（USAGE_DLL.md: 同一コンテキストでコピー）
        let spout_context_ptr = unsafe { spoutdx_receiver_get_dx11_context(self.receiver) };
        if spout_context_ptr.is_null() {
            return Err(DomainError::Capture("Failed to get SpoutDX context".to_string()));
        }
        let spout_context: ID3D11DeviceContext = unsafe {
            ID3D11DeviceContext::from_raw_borrowed(&spout_context_ptr)
                .ok_or_else(|| DomainError::Capture("Failed to wrap SpoutDX context".to_string()))?
                .clone()
        };
        
        // テクスチャからデバイスを取得（ステージング作成用）
        let tex_device: ID3D11Device = unsafe {
            received_tex.GetDevice()
                .map_err(|e| DomainError::Capture(format!("Failed to get device from texture: {:?}", e)))?
        };
        
        // 送信者情報を取得
        let mut sender_info = SpoutDxSenderInfo::default();
        let result = unsafe { 
            spoutdx_receiver_get_sender_info(self.receiver, &mut sender_info) 
        };
        
        if !SpoutDxResult::from_raw(result).is_ok() || sender_info.width == 0 || sender_info.height == 0 {
            #[cfg(debug_assertions)]
            tracing::trace!("Spout sender info not available");
            return Ok(None);
        }
        
        // 送信者情報が変わったか確認
        if sender_info.width != self.sender_info.width 
           || sender_info.height != self.sender_info.height 
        {
            #[cfg(debug_assertions)]
            tracing::info!(
                "Spout sender changed: {} ({}x{}) -> {} ({}x{})",
                self.sender_info.name_as_string(),
                self.sender_info.width,
                self.sender_info.height,
                sender_info.name_as_string(),
                sender_info.width,
                sender_info.height
            );
            
            // デバイス情報を更新
            self.device_info.width = sender_info.width;
            self.device_info.height = sender_info.height;
            self.sender_info = sender_info.clone();
            
            // ステージングテクスチャをクリア（次回作成）
            self.staging_tex = None;
            self.staging_size = (0, 0);
        }
        
        // ROIを画面中心に動的配置
        // レイテンシへの影響: ~10ns未満（減算2回、除算2回）
        let centered_roi = roi.centered_in(self.device_info.width, self.device_info.height)
            .ok_or_else(|| {
                DomainError::Configuration(format!(
                    "ROI size ({}x{}) exceeds texture bounds ({}x{})",
                    roi.width, roi.height,
                    self.device_info.width, self.device_info.height
                ))
            })?;
        
        // ROIのクランプ
        let clamped_roi = self.clamp_roi(&centered_roi).ok_or_else(|| {
            DomainError::Capture(format!(
                "ROI ({}, {}, {}x{}) is outside texture bounds ({}x{})",
                centered_roi.x, centered_roi.y, centered_roi.width, centered_roi.height,
                self.device_info.width, self.device_info.height
            ))
        })?;
        
        // ステージングテクスチャを確保（送信者のフォーマットに合わせる）
        let tex_format = DXGI_FORMAT(sender_info.format as i32);
        let staging_tex = self.ensure_staging_texture(
            clamped_roi.width, 
            clamped_roi.height, 
            tex_format,
            &tex_device,
        )?;
        
        // SpoutDX側のコンテキストを使ってROI領域をステージングへコピー
        // USAGE_DLL.md: 受信テクスチャから staging への CopyResource は SpoutDX側のcontextを使う
        unsafe {
            let src_box = D3D11_BOX {
                left: clamped_roi.x,
                top: clamped_roi.y,
                front: 0,
                right: clamped_roi.x + clamped_roi.width,
                bottom: clamped_roi.y + clamped_roi.height,
                back: 1,
            };
            
            let src_resource: ID3D11Resource = received_tex.cast()
                .map_err(|e| DomainError::Capture(format!("Cast error: {:?}", e)))?;
            
            spout_context.CopySubresourceRegion(
                &staging_tex,
                0, 0, 0, 0,
                &src_resource,
                0,
                Some(&src_box),
            );
        }
        
        // GPU→CPU転送
        let data_size = (clamped_roi.width * clamped_roi.height * 4) as usize;
        let mut data = vec![0u8; data_size];
        
        unsafe {
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            spout_context.Map(
                &staging_tex,
                0,
                D3D11_MAP_READ,
                0,
                Some(&mut mapped),
            ).map_err(|e| DomainError::Capture(format!("Map failed: {:?}", e)))?;
            
            // RowPitchを考慮してコピー
            let src_ptr = mapped.pData as *const u8;
            let row_pitch = mapped.RowPitch as usize;
            let row_bytes = (clamped_roi.width * 4) as usize;
            
            for y in 0..clamped_roi.height as usize {
                let src_offset = y * row_pitch;
                let dst_offset = y * row_bytes;
                std::ptr::copy_nonoverlapping(
                    src_ptr.add(src_offset),
                    data.as_mut_ptr().add(dst_offset),
                    row_bytes,
                );
            }
            
            spout_context.Unmap(&staging_tex, 0);
        }
        
        Ok(Some(Frame {
            data,
            width: clamped_roi.width,
            height: clamped_roi.height,
            timestamp: Instant::now(),
            dirty_rects: vec![],
        }))
    }
    
    fn reinitialize(&mut self) -> DomainResult<()> {
        #[cfg(debug_assertions)]
        tracing::info!("Reinitializing Spout receiver");
        
        // Spoutレシーバーを再作成
        unsafe {
            spoutdx_receiver_close_dx11(self.receiver);
            spoutdx_receiver_destroy(self.receiver);
        }
        
        // 新しいレシーバーを作成
        let receiver = unsafe { spoutdx_receiver_create() };
        if receiver.is_null() {
            return Err(DomainError::ReInitializationRequired);
        }
        
        let device_ptr = self.device.as_raw() as *mut std::ffi::c_void;
        let result = unsafe { spoutdx_receiver_open_dx11(receiver, device_ptr) };
        if !SpoutDxResult::from_raw(result).is_ok() {
            unsafe { spoutdx_receiver_destroy(receiver); }
            return Err(DomainError::ReInitializationRequired);
        }
        
        // 送信者名を再設定
        if let Some(ref name) = self.sender_name {
            if let Ok(c_name) = CString::new(name.as_str()) {
                unsafe { spoutdx_receiver_set_sender_name(receiver, c_name.as_ptr()); }
            }
        } else {
            unsafe { spoutdx_receiver_set_sender_name(receiver, ptr::null()); }
        }
        
        self.receiver = receiver;
        self.staging_tex = None;
        self.staging_size = (0, 0);
        
        #[cfg(debug_assertions)]
        tracing::info!("Spout receiver reinitialization completed");
        
        Ok(())
    }
    
    fn device_info(&self) -> DeviceInfo {
        self.device_info.clone()
    }
}

impl Drop for SpoutCaptureAdapter {
    fn drop(&mut self) {
        unsafe {
            spoutdx_receiver_close_dx11(self.receiver);
            spoutdx_receiver_destroy(self.receiver);
        }
    }
}

// Safety: SpoutCaptureAdapter is safe to send between threads
// as the receiver handle is opaque and managed by the Spout library
unsafe impl Send for SpoutCaptureAdapter {}
unsafe impl Sync for SpoutCaptureAdapter {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore] // Spout送信者が必要
    fn test_spout_initialization() {
        let adapter = SpoutCaptureAdapter::new(None);
        
        // Spout DLLが存在しない環境では失敗する
        match adapter {
            Ok(adapter) => {
                let info = adapter.device_info();
                println!("Spout Device Info:");
                println!("  Name: {}", info.name);
                println!("  Resolution: {}x{}", info.width, info.height);
            }
            Err(e) => {
                println!("Spout initialization failed (expected without sender): {:?}", e);
            }
        }
    }

    #[test]
    #[ignore] // Spout送信者が必要
    fn test_spout_capture_with_sender() {
        let mut adapter = SpoutCaptureAdapter::new(None)
            .expect("Failed to create Spout adapter");
        
        let roi = Roi::new(0, 0, 100, 100);
        
        match adapter.capture_frame_with_roi(&roi) {
            Ok(Some(frame)) => {
                println!("Captured frame:");
                println!("  Size: {}x{}", frame.width, frame.height);
                println!("  Data length: {} bytes", frame.data.len());
                assert_eq!(frame.data.len(), (frame.width * frame.height * 4) as usize);
            }
            Ok(None) => {
                println!("No sender connected or no new frame");
            }
            Err(e) => {
                println!("Capture error: {:?}", e);
            }
        }
    }
}
