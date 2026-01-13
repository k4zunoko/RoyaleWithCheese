//! Spout DX11 テクスチャ受信によるキャプチャアダプタ
//!
//! Spout送信されたDirectX 11テクスチャを受信し、
//! CapturePort traitを実装します。

use crate::domain::{CapturePort, DeviceInfo, DomainError, DomainResult, Frame, Roi};
use crate::infrastructure::capture::common::{
    clamp_roi, copy_roi_to_staging, copy_texture_to_cpu, StagingTextureManager,
};
use crate::infrastructure::capture::spout_ffi::*;
use std::ffi::CString;
use std::ptr;
use std::time::Instant;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;

/// Spoutキャプチャアダプタ
///
/// CapturePort traitを実装し、Spout送信されたDX11テクスチャを受信します。
/// USAGE_DLL.mdに従い、内部テクスチャ受信方式（spoutdx_receiver_receive + get_received_texture）を使用。
pub struct SpoutCaptureAdapter {
    // FFIハンドル（Send/Syncを実装するためのラッパー）
    receiver: SpoutDxReceiverHandle,

    // DirectX 11 デバイス（自前で作成、アダプタ整合性のため）
    #[allow(dead_code)] // SpoutDXのコンテキストを使用するため未使用だが保持が必要
    device: ID3D11Device,
    #[allow(dead_code)]
    context: ID3D11DeviceContext,

    // ステージングテクスチャ管理（共通モジュール使用）
    staging_manager: StagingTextureManager,

    // 送信者情報
    #[allow(dead_code)] // 再初期化時に使用
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
                "Failed to create Spout receiver".to_string(),
            ));
        }

        // D3D11デバイスをSpoutに渡す
        let device_ptr = device.as_raw();
        let result = unsafe { spoutdx_receiver_open_dx11(receiver, device_ptr) };
        if !SpoutDxResult::from_raw(result).is_ok() {
            unsafe {
                spoutdx_receiver_destroy(receiver);
            }
            return Err(DomainError::Initialization(format!(
                "Failed to open DX11 for Spout: {:?}",
                SpoutDxResult::from_raw(result)
            )));
        }

        // 送信者名を設定（指定があれば）
        if let Some(ref name) = sender_name {
            let c_name = CString::new(name.as_str())
                .map_err(|_| DomainError::Configuration("Invalid sender name".to_string()))?;
            unsafe {
                spoutdx_receiver_set_sender_name(receiver, c_name.as_ptr());
            }
        } else {
            // NULLで自動選択
            unsafe {
                spoutdx_receiver_set_sender_name(receiver, ptr::null());
            }
        }

        // 初期デバイス情報（接続後に更新）
        let device_info = DeviceInfo {
            width: 0,
            height: 0,
            refresh_rate: 0, // Spoutでは不明
            name: sender_name
                .clone()
                .unwrap_or_else(|| "Spout (auto)".to_string()),
        };

        #[cfg(debug_assertions)]
        tracing::info!(
            "Spout receiver initialized: sender_name={:?}",
            sender_name
        );

        Ok(Self {
            receiver,
            device,
            context,
            staging_manager: StagingTextureManager::new(),
            sender_name,
            sender_info: SpoutDxSenderInfo::default(),
            device_info,
        })
    }

    /// D3D11デバイスを作成（Spout固有）
    fn create_d3d11_device() -> DomainResult<(ID3D11Device, ID3D11DeviceContext)> {
        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;

        unsafe {
            D3D11CreateDevice(
                None, // デフォルトアダプタ
                D3D_DRIVER_TYPE_HARDWARE,
                None,
                D3D11_CREATE_DEVICE_FLAG(0),
                None, // 機能レベル自動選択
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .map_err(|e| {
                DomainError::Initialization(format!("Failed to create D3D11 device: {:?}", e))
            })?;
        }

        let device = device.ok_or_else(|| {
            DomainError::Initialization("D3D11 device creation returned None".to_string())
        })?;
        let context = context.ok_or_else(|| {
            DomainError::Initialization("D3D11 context creation returned None".to_string())
        })?;

        Ok((device, context))
    }

    /// フレームを受信（Spout固有処理）
    ///
    /// # Returns
    /// - `Ok(Some((texture, context, sender_info)))`: 新しいフレーム受信成功
    /// - `Ok(None)`: 更新なしまたは未接続
    /// - `Err(DomainError)`: 致命的エラー
    fn receive_frame(
        &mut self,
    ) -> DomainResult<Option<(ID3D11Texture2D, ID3D11DeviceContext, SpoutDxSenderInfo)>> {
        // 内部テクスチャへ受信（USAGE_DLL.md推奨方式）
        let result = unsafe { spoutdx_receiver_receive(self.receiver) };

        if !SpoutDxResult::from_raw(result).is_ok() {
            let err = SpoutDxResult::from_raw(result);
            return match err {
                SpoutDxResult::ErrorNotConnected => Ok(None),
                SpoutDxResult::ErrorReceiveFailed => Err(DomainError::DeviceNotAvailable),
                SpoutDxResult::ErrorNullHandle | SpoutDxResult::ErrorNullDevice => {
                    Err(DomainError::ReInitializationRequired)
                }
                _ => Err(DomainError::Capture(format!(
                    "Spout receive failed: {:?}",
                    err
                ))),
            };
        }

        // 新しいフレームがあるかチェック
        let is_new = unsafe { spoutdx_receiver_is_frame_new(self.receiver) };
        if is_new == 0 {
            return Ok(None); // 更新なし
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
                .ok_or_else(|| {
                    DomainError::Capture("Failed to get received texture".to_string())
                })?
                .clone()
        };

        // SpoutDX側のD3D11コンテキストを取得（USAGE_DLL.md: 同一コンテキストでコピー）
        let spout_context_ptr = unsafe { spoutdx_receiver_get_dx11_context(self.receiver) };
        if spout_context_ptr.is_null() {
            return Err(DomainError::Capture(
                "Failed to get SpoutDX context".to_string(),
            ));
        }
        let spout_context: ID3D11DeviceContext = unsafe {
            ID3D11DeviceContext::from_raw_borrowed(&spout_context_ptr)
                .ok_or_else(|| {
                    DomainError::Capture("Failed to wrap SpoutDX context".to_string())
                })?
                .clone()
        };

        // 送信者情報を取得
        let mut sender_info = SpoutDxSenderInfo::default();
        let result =
            unsafe { spoutdx_receiver_get_sender_info(self.receiver, &mut sender_info) };

        if !SpoutDxResult::from_raw(result).is_ok()
            || sender_info.width == 0
            || sender_info.height == 0
        {
            #[cfg(debug_assertions)]
            tracing::trace!("Spout sender info not available");
            return Ok(None);
        }

        Ok(Some((received_tex, spout_context, sender_info)))
    }

    /// 送信者情報の変更を検出して状態を更新
    fn update_sender_info(&mut self, sender_info: &SpoutDxSenderInfo) {
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
            self.staging_manager.clear();
        }
    }
}

impl CapturePort for SpoutCaptureAdapter {
    fn capture_frame_with_roi(&mut self, roi: &Roi) -> DomainResult<Option<Frame>> {
        // フレーム受信（Spout固有処理）
        let (received_tex, spout_context, sender_info) = match self.receive_frame()? {
            Some(data) => data,
            None => return Ok(None),
        };

        // 送信者情報の変更を検出して状態を更新
        self.update_sender_info(&sender_info);

        // テクスチャからデバイスを取得（ステージング作成用）
        let tex_device: ID3D11Device = unsafe {
            received_tex.GetDevice().map_err(|e| {
                DomainError::Capture(format!("Failed to get device from texture: {:?}", e))
            })?
        };

        // ROIを画面中心に動的配置
        // レイテンシへの影響: ~10ns未満（減算2回、除算2回）
        let centered_roi =
            roi.centered_in(self.device_info.width, self.device_info.height)
                .ok_or_else(|| {
                    DomainError::Configuration(format!(
                        "ROI size ({}x{}) exceeds texture bounds ({}x{})",
                        roi.width, roi.height, self.device_info.width, self.device_info.height
                    ))
                })?;

        // ROIのクランプ（共通モジュール使用）
        let clamped_roi =
            clamp_roi(&centered_roi, self.device_info.width, self.device_info.height).ok_or_else(
                || {
                    DomainError::Capture(format!(
                        "ROI ({}, {}, {}x{}) is outside texture bounds ({}x{})",
                        centered_roi.x,
                        centered_roi.y,
                        centered_roi.width,
                        centered_roi.height,
                        self.device_info.width,
                        self.device_info.height
                    ))
                },
            )?;

        // ステージングテクスチャを確保（送信者のフォーマットに合わせる）
        // 共通モジュール使用
        let tex_format = DXGI_FORMAT(sender_info.format as i32);
        let staging_tex = self.staging_manager.ensure_texture(
            &tex_device,
            clamped_roi.width,
            clamped_roi.height,
            tex_format,
        )?;

        // SpoutDX側のコンテキストを使ってROI領域をステージングへコピー
        // USAGE_DLL.md: 受信テクスチャから staging への CopyResource は SpoutDX側のcontextを使う
        // 共通モジュール使用
        let src_resource: ID3D11Resource = received_tex
            .cast()
            .map_err(|e| DomainError::Capture(format!("Cast error: {:?}", e)))?;

        copy_roi_to_staging(&spout_context, &src_resource, &staging_tex, &clamped_roi);

        // GPU→CPU転送（共通モジュール使用）
        let data = copy_texture_to_cpu(
            &spout_context,
            &staging_tex,
            clamped_roi.width,
            clamped_roi.height,
        )?;

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

        let device_ptr = self.device.as_raw();
        let result = unsafe { spoutdx_receiver_open_dx11(receiver, device_ptr) };
        if !SpoutDxResult::from_raw(result).is_ok() {
            unsafe {
                spoutdx_receiver_destroy(receiver);
            }
            return Err(DomainError::ReInitializationRequired);
        }

        // 送信者名を再設定
        if let Some(ref name) = self.sender_name {
            if let Ok(c_name) = CString::new(name.as_str()) {
                unsafe {
                    spoutdx_receiver_set_sender_name(receiver, c_name.as_ptr());
                }
            }
        } else {
            unsafe {
                spoutdx_receiver_set_sender_name(receiver, ptr::null());
            }
        }

        self.receiver = receiver;
        self.staging_manager.clear();

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
        let mut adapter =
            SpoutCaptureAdapter::new(None).expect("Failed to create Spout adapter");

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
