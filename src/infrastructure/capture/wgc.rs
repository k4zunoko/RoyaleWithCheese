//! WGC (Windows Graphics Capture) キャプチャアダプタ
//!
//! Windows Graphics Capture APIを使用した画面キャプチャ。
//! Windows 10 バージョン 1803 以降で動作。
//!
//! # Phase 2: windows crate v0.57 による直接実装
//!
//! windows-capture クレートのバージョン不整合により、
//! windows crate を直接使用してWGC APIを実装します。

use crate::domain::{CapturePort, DeviceInfo, DomainError, DomainResult, Frame, Roi};
use crate::infrastructure::capture::common::{
    clamp_roi, copy_roi_to_staging, copy_texture_to_cpu, StagingTextureManager,
};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use windows::core::{factory, Interface, IUnknown, GUID};
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::{BOOL, LPARAM, RECT};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
use windows::Win32::System::WinRT::Direct3D11::CreateDirect3D11DeviceFromDXGIDevice;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

// IGraphicsCaptureItemInterop COM interface
#[repr(C)]
#[derive(Clone, Debug)]
pub struct IGraphicsCaptureItemInterop(IUnknown);

unsafe impl Interface for IGraphicsCaptureItemInterop {
    type Vtable = IGraphicsCaptureItemInterop_Vtbl;
    const IID: GUID = GUID::from_u128(0x3628e81b_3cac_4c60_b7f4_23ce0e0c3356);
}

impl IGraphicsCaptureItemInterop {
    #[allow(non_snake_case)]
    pub unsafe fn CreateForMonitor(
        &self,
        monitor: HMONITOR,
    ) -> windows::core::Result<GraphicsCaptureItem> {
        let mut result: *mut std::ffi::c_void = std::ptr::null_mut();
        (self.vtable().CreateForMonitor)(
            self.as_raw(),
            monitor,
            &GraphicsCaptureItem::IID,
            &mut result,
        )
        .ok()?;
        Ok(GraphicsCaptureItem::from_raw(result))
    }
}

#[repr(C)]
#[allow(non_snake_case)]
pub struct IGraphicsCaptureItemInterop_Vtbl {
    pub base__: windows::core::IUnknown_Vtbl,
    pub CreateForWindow: unsafe extern "system" fn(
        this: *mut std::ffi::c_void,
        window: *mut std::ffi::c_void,
        iid: *const GUID,
        result: *mut *mut std::ffi::c_void,
    ) -> windows::core::HRESULT,
    pub CreateForMonitor: unsafe extern "system" fn(
        this: *mut std::ffi::c_void,
        monitor: HMONITOR,
        iid: *const GUID,
        result: *mut *mut std::ffi::c_void,
    ) -> windows::core::HRESULT,
}

/// WGCキャプチャアダプタ（Phase 2: 直接実装）
///
/// CapturePort traitを実装し、WGCによる画面キャプチャを提供します。
pub struct WgcCaptureAdapter {
    // デバイス情報
    device_info: DeviceInfo,
    _monitor_index: usize,

    // 最新フレームの保持（コールバックから更新）
    latest_frame: Arc<Mutex<Option<CapturedFrameData>>>,

    // D3D11デバイス（ステージング用）
    device: ID3D11Device,
    context: ID3D11DeviceContext,

    // ステージングテクスチャ管理
    staging_manager: StagingTextureManager,

    // WGCセッション（ドロップ防止のため保持）
    _capture_item: GraphicsCaptureItem,
    _frame_pool: Direct3D11CaptureFramePool,
    _d3d_device: IDirect3DDevice,
    _capture_session: windows::Graphics::Capture::GraphicsCaptureSession,
}

// WGCオブジェクトはスレッド安全に使用するためSend + Syncを実装
// WinRTのCOM呼び出しは内部的にスレッドセーフに設計されている
unsafe impl Send for WgcCaptureAdapter {}
unsafe impl Sync for WgcCaptureAdapter {}

/// キャプチャされたフレームデータ
#[derive(Clone)]
struct CapturedFrameData {
    texture: ID3D11Texture2D,
    width: u32,
    height: u32,
    timestamp: Instant,
}

/// モニター列挙用のコールバックデータ
struct MonitorEnumData {
    monitors: Vec<HMONITOR>,
}

impl WgcCaptureAdapter {
    /// 新しいWGCキャプチャアダプタを作成（Phase 2: 直接実装）
    ///
    /// # Arguments
    /// - `monitor_index`: モニターのインデックス（通常は0）
    ///
    /// # Returns
    /// - `Ok(WgcCaptureAdapter)`: 初期化成功
    /// - `Err(DomainError)`: 初期化失敗
    pub fn new(monitor_index: usize) -> DomainResult<Self> {
        // WinRTの初期化
        unsafe {
            RoInitialize(RO_INIT_MULTITHREADED).map_err(|e| {
                DomainError::Initialization(format!("Failed to initialize WinRT: {:?}", e))
            })?;
        }

        // モニターを列挙
        let monitors = Self::enumerate_monitors()?;

        if monitors.is_empty() {
            return Err(DomainError::Initialization(
                "No monitors found".to_string(),
            ));
        }

        let hmonitor = monitors.get(monitor_index).ok_or_else(|| {
            DomainError::Initialization(format!(
                "Monitor index {} not found (available: {})",
                monitor_index,
                monitors.len()
            ))
        })?;

        // D3D11デバイスを作成
        let (device, context) = Self::create_d3d11_device()?;

        // GraphicsCaptureItemを作成
        let capture_item = Self::create_capture_item_for_monitor(*hmonitor)?;

        // サイズ情報を取得
        let size = capture_item.Size().map_err(|e| {
            DomainError::Initialization(format!("Failed to get capture item size: {:?}", e))
        })?;

        let device_info = DeviceInfo {
            width: size.Width as u32,
            height: size.Height as u32,
            refresh_rate: 0, // WGCでは取得不可
            name: format!("WGC Monitor {}", monitor_index),
        };

        // IDirect3DDeviceを作成（WGC用）
        let d3d_device = Self::create_direct3d_device(&device)?;

        // レイテンシ最小化: バッファサイズ2（最小推奨値）でフレームプール作成
        let frame_pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &d3d_device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2, // 最小バッファでレイテンシ削減
            size,
        )
        .map_err(|e| {
            DomainError::Initialization(format!("Failed to create frame pool: {:?}", e))
        })?;

        // FrameArrivedイベントハンドラを設定（レイテンシ最小化のため即座に処理）
        let latest_frame = Arc::new(Mutex::new(None));
        let latest_frame_for_callback = Arc::clone(&latest_frame);
        let device_clone = device.clone();
        
        frame_pool
            .FrameArrived(&windows::Foundation::TypedEventHandler::new(
                move |pool: &Option<Direct3D11CaptureFramePool>, _args| {
                    if let Some(pool) = pool {
                        // 即座にフレームを取得（レイテンシ削減）
                        if let Ok(frame) = pool.TryGetNextFrame() {
                            if let Ok(surface) = frame.Surface() {
                                // IDirect3DSurfaceからID3D11Texture2Dを取得
                                if let Ok(texture) =
                                    Self::get_texture_from_surface(&surface, &device_clone)
                                {
                                    unsafe {
                                        let mut desc = D3D11_TEXTURE2D_DESC::default();
                                        texture.GetDesc(&mut desc);
                                        
                                        // 最新フレームを更新
                                        if let Ok(mut guard) = latest_frame_for_callback.lock() {
                                            *guard = Some(CapturedFrameData {
                                                texture,
                                                width: desc.Width,
                                                height: desc.Height,
                                                timestamp: Instant::now(),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Ok(())
                },
            ))
            .map_err(|e| {
                DomainError::Initialization(format!("Failed to set FrameArrived handler: {:?}", e))
            })?;

        // キャプチャセッション開始
        let capture_session = frame_pool
            .CreateCaptureSession(&capture_item)
            .map_err(|e| {
                DomainError::Initialization(format!("Failed to create capture session: {:?}", e))
            })?;
        
        // 黄色枠を無効化（レイテンシ重視）
        capture_session
            .SetIsBorderRequired(false)
            .map_err(|e| {
                DomainError::Initialization(format!("Failed to disable border: {:?}", e))
            })?;
        
        // カーソルキャプチャを無効化
        capture_session
            .SetIsCursorCaptureEnabled(false)
            .map_err(|e| {
                DomainError::Initialization(format!("Failed to disable cursor capture: {:?}", e))
            })?;
        
        capture_session
            .StartCapture()
            .map_err(|e| {
                DomainError::Initialization(format!("Failed to start capture: {:?}", e))
            })?;

        #[cfg(debug_assertions)]
        tracing::info!(
            "WGC adapter initialized: {}x{} - {} (low-latency mode)",
            device_info.width,
            device_info.height,
            device_info.name
        );

        Ok(Self {
            device_info,
            _monitor_index: monitor_index,
            latest_frame,
            device,
            context,
            staging_manager: StagingTextureManager::new(),
            _capture_item: capture_item,
            _frame_pool: frame_pool,
            _d3d_device: d3d_device,
            _capture_session: capture_session,
        })
    }

    /// モニターを列挙
    fn enumerate_monitors() -> DomainResult<Vec<HMONITOR>> {
        let mut data = MonitorEnumData {
            monitors: Vec::new(),
        };

        unsafe {
            extern "system" fn enum_proc(
                hmonitor: HMONITOR,
                _hdc: HDC,
                _lprect: *mut RECT,
                lparam: LPARAM,
            ) -> BOOL {
                unsafe {
                    let data = &mut *(lparam.0 as *mut MonitorEnumData);
                    data.monitors.push(hmonitor);
                    BOOL(1) // TRUE
                }
            }

            let _ = EnumDisplayMonitors(
                HDC(0),
                None,
                Some(enum_proc),
                LPARAM(&mut data as *mut _ as isize),
            );
        }

        if data.monitors.is_empty() {
            Err(DomainError::Initialization(
                "No monitors found".to_string(),
            ))
        } else {
            Ok(data.monitors)
        }
    }

    /// HMONITORからGraphicsCaptureItemを作成
    fn create_capture_item_for_monitor(
        hmonitor: HMONITOR,
    ) -> DomainResult<GraphicsCaptureItem> {
        unsafe {
            let interop: IGraphicsCaptureItemInterop =
                factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
                    .map_err(|e| {
                        DomainError::Initialization(format!(
                            "Failed to get IGraphicsCaptureItemInterop factory: {:?}",
                            e
                        ))
                    })?;

            interop
                .CreateForMonitor(hmonitor)
                .map_err(|e| {
                    DomainError::Initialization(format!(
                        "CreateForMonitor failed: {:?}",
                        e
                    ))
                })
        }
    }

    /// D3D11DeviceからIDirect3DDeviceを作成（WGC用）
    fn create_direct3d_device(
        d3d_device: &ID3D11Device,
    ) -> DomainResult<IDirect3DDevice> {
        unsafe {
            // DXGIデバイスを取得
            let dxgi_device: windows::Win32::Graphics::Dxgi::IDXGIDevice =
                d3d_device.cast().map_err(|e| {
                    DomainError::Initialization(format!("Failed to cast to IDXGIDevice: {:?}", e))
                })?;

            // WinRT IDirect3DDevice を作成
            let inspectable = CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device).map_err(|e| {
                DomainError::Initialization(format!(
                    "Failed to create IDirect3DDevice: {:?}",
                    e
                ))
            })?;

            // IDirect3DDevice にキャスト
            let direct3d_device: IDirect3DDevice = inspectable.cast().map_err(|e| {
                DomainError::Initialization(format!(
                    "Failed to cast to IDirect3DDevice: {:?}",
                    e
                ))
            })?;

            Ok(direct3d_device)
        }
    }

    /// ID3D11Texture2DをIDirect3DSurfaceから取得
    fn get_texture_from_surface(
        surface: &windows::Graphics::DirectX::Direct3D11::IDirect3DSurface,
        _device: &ID3D11Device,
    ) -> DomainResult<ID3D11Texture2D> {
        unsafe {
            use windows::Win32::System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess;

            let access: IDirect3DDxgiInterfaceAccess = surface.cast().map_err(|e| {
                DomainError::Capture(format!("Failed to get interface access: {:?}", e))
            })?;

            let texture: ID3D11Texture2D = access.GetInterface().map_err(|e| {
                DomainError::Capture(format!("Failed to get texture: {:?}", e))
            })?;

            Ok(texture)
        }
    }

    /// D3D11デバイスを作成
    fn create_d3d11_device() -> DomainResult<(ID3D11Device, ID3D11DeviceContext)> {
        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;

        unsafe {
            D3D11CreateDevice(
                None,
                windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE,
                None,
                D3D11_CREATE_DEVICE_FLAG(0),
                None,
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
}

impl CapturePort for WgcCaptureAdapter {
    fn capture_frame_with_roi(&mut self, roi: &Roi) -> DomainResult<Option<Frame>> {
        // レイテンシ最小化: イベントハンドラで既に取得されたフレームを使用
        let frame_data = {
            let guard = self.latest_frame.lock().map_err(|e| {
                DomainError::Capture(format!("Failed to lock latest frame: {:?}", e))
            })?;
            
            match &*guard {
                Some(data) => data.clone(),
                None => {
                    // まだフレームが到着していない
                    return Ok(None);
                }
            }
        };

        // ROI処理（レイテンシ最小化のため、ステージングテクスチャへ直接コピー）
        // ROIをクランプ
        let clamped_roi = match clamp_roi(roi, frame_data.width, frame_data.height) {
            Some(r) => r,
            None => return Ok(None),
        };

        let staging = self.staging_manager.ensure_texture(
            &self.device,
            clamped_roi.width,
            clamped_roi.height,
            windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
        )?;

        // ROI領域をステージングテクスチャにコピー（最小コピー量）
        copy_roi_to_staging(
            &self.context,
            &frame_data.texture,
            &staging,
            &clamped_roi,
        );

        // CPUメモリへコピー
        let data = copy_texture_to_cpu(
            &self.context,
            &staging,
            clamped_roi.width,
            clamped_roi.height,
        )?;

        Ok(Some(Frame {
            data,
            width: clamped_roi.width,
            height: clamped_roi.height,
            timestamp: frame_data.timestamp,
            dirty_rects: vec![],
        }))
    }

    fn reinitialize(&mut self) -> DomainResult<()> {
        // WGCセッションは自動的に再接続するため、特別な処理は不要
        #[cfg(debug_assertions)]
        tracing::info!("WGC reinitialize: no action needed (auto-recovery)");
        Ok(())
    }

    fn device_info(&self) -> DeviceInfo {
        self.device_info.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore] // Phase 2: 実環境でのみ動作
    fn test_wgc_initialization() {
        // 初期化テスト
        let adapter = WgcCaptureAdapter::new(0);
        assert!(adapter.is_ok(), "Failed to create WGC adapter");

        let adapter = adapter.unwrap();
        let info = adapter.device_info();
        println!(
            "WGC adapter initialized: {}x{} - {}",
            info.width, info.height, info.name
        );
    }

    #[test]
    #[ignore]
    fn test_d3d11_device_creation() {
        // D3D11デバイス作成のテスト
        let result = WgcCaptureAdapter::create_d3d11_device();
        assert!(result.is_ok(), "Failed to create D3D11 device");
    }
}
