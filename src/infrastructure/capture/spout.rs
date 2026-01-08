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
pub struct SpoutCaptureAdapter {
    // FFIハンドル（Send/Syncを実装するためのラッパー）
    receiver: SpoutDxReceiverHandle,
    
    // DirectX 11 デバイス（自前で作成）
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    
    // 受信用テクスチャ（送信者のサイズに合わせて再作成）
    receive_tex: Option<ID3D11Texture2D>,
    receive_tex_size: (u32, u32),
    
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
            receive_tex: None,
            receive_tex_size: (0, 0),
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
    
    /// 受信テクスチャのサイズを更新（送信者変更時）
    fn update_receive_texture(&mut self, width: u32, height: u32, format: u32) -> DomainResult<()> {
        if self.receive_tex_size == (width, height) {
            return Ok(());  // サイズ変更なし
        }
        
        #[cfg(debug_assertions)]
        tracing::debug!("Updating receive texture: {}x{}, format={}", width, height, format);
        
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT(format as i32),
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: D3D11_RESOURCE_MISC_SHARED.0 as u32,
        };
        
        let mut tex: Option<ID3D11Texture2D> = None;
        unsafe {
            self.device.CreateTexture2D(&desc, None, Some(&mut tex))
                .map_err(|e| DomainError::Capture(
                    format!("Failed to create receive texture: {:?}", e)
                ))?;
        }
        
        self.receive_tex = tex;
        self.receive_tex_size = (width, height);
        
        // デバイス情報を更新
        self.device_info.width = width;
        self.device_info.height = height;
        
        Ok(())
    }
    
    /// ステージングテクスチャを確保（ROIサイズ用）
    /// 
    /// # パフォーマンス最適化
    /// - ROIサイズが同じであれば既存のテクスチャを再利用
    /// - サイズ変更時のみ再作成し、GPUリソースの再割り当てを最小化
    fn ensure_staging_texture(&mut self, width: u32, height: u32) -> DomainResult<ID3D11Texture2D> {
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
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: D3D11_BIND_FLAG(0).0 as u32,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: D3D11_RESOURCE_MISC_FLAG(0).0 as u32,
        };
        
        let mut tex: Option<ID3D11Texture2D> = None;
        unsafe {
            self.device.CreateTexture2D(&desc, None, Some(&mut tex))
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
        // 新しいフレームがあるかチェック（これが接続を試行する）
        let is_new = unsafe { spoutdx_receiver_is_frame_new(self.receiver) };
        if is_new == 0 {
            return Ok(None);  // 更新なし
        }
        
        // 送信者情報を取得
        let mut sender_info = SpoutDxSenderInfo::default();
        let result = unsafe { 
            spoutdx_receiver_get_sender_info(self.receiver, &mut sender_info) 
        };
        
        // 初回または送信者変更時: receive_textureを呼ぶことで接続確立
        // sender_infoが0x0の場合は初回接続を試みる必要がある
        let is_first_connection = sender_info.width == 0 || sender_info.height == 0;
        
        if is_first_connection {
            #[cfg(debug_assertions)]
            tracing::debug!("First Spout connection attempt, calling receive_texture...");
            
            // ダミーテクスチャを作成して接続を確立
            if self.receive_tex.is_none() {
                // デフォルトサイズのテクスチャを作成（接続後に正しいサイズで再作成される）
                self.update_receive_texture(1920, 1080, 87)?;  // 87 = DXGI_FORMAT_B8G8R8A8_UNORM
            }
            
            let receive_tex = self.receive_tex.as_ref()
                .ok_or_else(|| DomainError::Capture("Receive texture not initialized".to_string()))?
                .clone();
            
            let tex_ptr = receive_tex.as_raw() as *mut std::ffi::c_void;
            let result = unsafe { spoutdx_receiver_receive_texture(self.receiver, tex_ptr) };
            
            if !SpoutDxResult::from_raw(result).is_ok() {
                #[cfg(debug_assertions)]
                tracing::trace!("Spout receive_texture failed (not connected yet): {:?}", SpoutDxResult::from_raw(result));
                return Ok(None);
            }
            
            // 接続成功後、sender infoを再取得
            let result = unsafe { 
                spoutdx_receiver_get_sender_info(self.receiver, &mut sender_info) 
            };
            
            if !SpoutDxResult::from_raw(result).is_ok() {
                #[cfg(debug_assertions)]
                tracing::warn!("Failed to get sender info after connection");
                return Ok(None);
            }
            
            #[cfg(debug_assertions)]
            tracing::info!(
                "Spout connected: {} ({}x{})",
                sender_info.name_as_string(),
                sender_info.width,
                sender_info.height
            );
            
            // 正しいサイズでテクスチャを再作成
            self.update_receive_texture(
                sender_info.width, 
                sender_info.height, 
                sender_info.format
            )?;
            self.sender_info = sender_info;
            
            // ステージングテクスチャもクリア（次回作成）
            self.staging_tex = None;
            self.staging_size = (0, 0);
        } else {
            // 既存接続: sender infoチェック
            if !SpoutDxResult::from_raw(result).is_ok() {
                // 送信者未接続
                #[cfg(debug_assertions)]
                tracing::trace!("Spout sender not connected");
                return Ok(None);
            }
            
            // 送信者のサイズが変わったらテクスチャを再作成
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
                
                self.update_receive_texture(
                    sender_info.width, 
                    sender_info.height, 
                    sender_info.format
                )?;
                self.sender_info = sender_info;
                
                // ステージングテクスチャもクリア（次回作成）
                self.staging_tex = None;
                self.staging_size = (0, 0);
            }
        }
        
        // テクスチャを受信
        let receive_tex = self.receive_tex.as_ref()
            .ok_or_else(|| DomainError::Capture("Receive texture not initialized".to_string()))?
            .clone();
        
        let tex_ptr = receive_tex.as_raw() as *mut std::ffi::c_void;
        let result = unsafe { spoutdx_receiver_receive_texture(self.receiver, tex_ptr) };
        
        if !SpoutDxResult::from_raw(result).is_ok() {
            let err = SpoutDxResult::from_raw(result);
            
            // エラー種別に応じてマッピング
            return match err {
                SpoutDxResult::ErrorNotConnected => Ok(None),
                SpoutDxResult::ErrorReceiveFailed => Err(DomainError::DeviceNotAvailable),
                SpoutDxResult::ErrorNullHandle | SpoutDxResult::ErrorNullDevice => {
                    Err(DomainError::ReInitializationRequired)
                }
                _ => Err(DomainError::Capture(format!("Failed to receive texture: {:?}", err)))
            };
        }
        
        // ROIのクランプ
        let clamped_roi = self.clamp_roi(roi).ok_or_else(|| {
            DomainError::Capture(format!(
                "ROI ({}, {}, {}x{}) is outside texture bounds ({}x{})",
                roi.x, roi.y, roi.width, roi.height,
                self.device_info.width, self.device_info.height
            ))
        })?;
        
        // ステージングテクスチャへROI領域をコピー
        let staging_tex = self.ensure_staging_texture(clamped_roi.width, clamped_roi.height)?;
        
        unsafe {
            let src_box = D3D11_BOX {
                left: clamped_roi.x,
                top: clamped_roi.y,
                front: 0,
                right: clamped_roi.x + clamped_roi.width,
                bottom: clamped_roi.y + clamped_roi.height,
                back: 1,
            };
            
            let src_resource: ID3D11Resource = receive_tex.cast()
                .map_err(|e| DomainError::Capture(format!("Cast error: {:?}", e)))?;
            
            self.context.CopySubresourceRegion(
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
            self.context.Map(
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
            
            self.context.Unmap(&staging_tex, 0);
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
        self.receive_tex = None;
        self.receive_tex_size = (0, 0);
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
