use crate::domain::error::{DomainError, DomainResult};
use crate::domain::ports::CapturePort;
use crate::domain::types::{DeviceInfo, Frame, GpuFrame, Roi};
use crate::infrastructure::capture::common::{
    clamp_roi, copy_texture_to_cpu, StagingTextureManager,
};
use std::sync::{Arc, Mutex};
use windows::core::{factory, IUnknown, Interface, GUID};
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::{IDirect3DDevice, IDirect3DSurface};
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::{BOOL, LPARAM, RECT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D,
    D3D11_BIND_SHADER_RESOURCE, D3D11_BOX, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_CREATE_DEVICE_FLAG, D3D11_RESOURCE_MISC_FLAG, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

#[repr(C)]
#[derive(Clone, Debug)]
struct IGraphicsCaptureItemInterop(IUnknown);

unsafe impl Interface for IGraphicsCaptureItemInterop {
    type Vtable = IGraphicsCaptureItemInteropVtbl;
    const IID: GUID = GUID::from_u128(0x3628e81b_3cac_4c60_b7f4_23ce0e0c3356);
}

impl IGraphicsCaptureItemInterop {
    #[allow(non_snake_case)]
    unsafe fn CreateForMonitor(
        &self,
        monitor: HMONITOR,
    ) -> windows::core::Result<GraphicsCaptureItem> {
        let mut result = std::ptr::null_mut();
        // SAFETY: COM vtable call is made with valid `this`, monitor handle, iid, and out pointer.
        unsafe {
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
}

#[repr(C)]
#[allow(non_snake_case)]
struct IGraphicsCaptureItemInteropVtbl {
    base__: windows::core::IUnknown_Vtbl,
    CreateForWindow: unsafe extern "system" fn(
        this: *mut std::ffi::c_void,
        window: *mut std::ffi::c_void,
        iid: *const GUID,
        result: *mut *mut std::ffi::c_void,
    ) -> windows::core::HRESULT,
    CreateForMonitor: unsafe extern "system" fn(
        this: *mut std::ffi::c_void,
        monitor: HMONITOR,
        iid: *const GUID,
        result: *mut *mut std::ffi::c_void,
    ) -> windows::core::HRESULT,
}

#[derive(Clone)]
struct CapturedFrameData {
    texture: ID3D11Texture2D,
    width: u32,
    height: u32,
}

struct MonitorEnumData {
    monitors: Vec<HMONITOR>,
}

pub struct WgcCaptureAdapter {
    info: DeviceInfo,
    latest_frame: Arc<Mutex<Option<CapturedFrameData>>>,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    staging_manager: StagingTextureManager,
    _capture_item: GraphicsCaptureItem,
    _frame_pool: Direct3D11CaptureFramePool,
    _capture_session: GraphicsCaptureSession,
    _d3d_device: IDirect3DDevice,
}

// SAFETY: Adapter methods require `&mut self` for capture operations, so callers cannot
// concurrently use internal COM handles through this type.
unsafe impl Send for WgcCaptureAdapter {}

impl WgcCaptureAdapter {
    pub fn new(monitor_index: usize) -> DomainResult<Self> {
        // SAFETY: WinRT must be initialized before WGC APIs are used.
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };

        let monitors = Self::enumerate_monitors()?;
        let target_monitor = monitors.get(monitor_index).ok_or_else(|| {
            DomainError::Initialization(format!(
                "Monitor index {monitor_index} out of range (available: {})",
                monitors.len()
            ))
        })?;

        let (device, context) = Self::create_d3d11_device()?;
        let capture_item = Self::create_capture_item_for_monitor(*target_monitor)?;
        let size = capture_item.Size().map_err(|e| {
            DomainError::Initialization(format!("Failed to query capture item size: {e:?}"))
        })?;

        let d3d_device = Self::create_direct3d_device(&device)?;
        let frame_pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &d3d_device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            size,
        )
        .map_err(|e| DomainError::Initialization(format!("Failed to create frame pool: {e:?}")))?;

        let latest_frame = Arc::new(Mutex::new(None));
        let callback_latest_frame = Arc::clone(&latest_frame);

        frame_pool
            .FrameArrived(&TypedEventHandler::new(
                move |pool: &Option<Direct3D11CaptureFramePool>, _| {
                    if let Some(pool) = pool {
                        if let Ok(frame) = pool.TryGetNextFrame() {
                            if let Ok(surface) = frame.Surface() {
                                if let Ok(texture) = Self::get_texture_from_surface(&surface) {
                                    let mut desc = D3D11_TEXTURE2D_DESC::default();
                                    // SAFETY: `texture` is valid and `desc` is initialized output storage.
                                    unsafe {
                                        texture.GetDesc(&mut desc);
                                    }
                                    if let Ok(mut guard) = callback_latest_frame.lock() {
                                        *guard = Some(CapturedFrameData {
                                            texture,
                                            width: desc.Width,
                                            height: desc.Height,
                                        });
                                    }
                                }
                            }
                        }
                    }
                    Ok(())
                },
            ))
            .map_err(|e| {
                DomainError::Initialization(format!("Failed to register frame callback: {e:?}"))
            })?;

        let capture_session = frame_pool
            .CreateCaptureSession(&capture_item)
            .map_err(|e| {
                DomainError::Initialization(format!("Failed to create capture session: {e:?}"))
            })?;

        capture_session
            .SetIsBorderRequired(false)
            .map_err(|e| DomainError::Initialization(format!("Failed to disable border: {e:?}")))?;

        capture_session
            .SetIsCursorCaptureEnabled(false)
            .map_err(|e| DomainError::Initialization(format!("Failed to disable cursor: {e:?}")))?;

        capture_session.StartCapture().map_err(|e| {
            DomainError::Initialization(format!("Failed to start WGC capture: {e:?}"))
        })?;

        Ok(Self {
            info: DeviceInfo::new(
                size.Width as u32,
                size.Height as u32,
                format!("WGC Monitor {monitor_index}"),
            ),
            latest_frame,
            device,
            context,
            staging_manager: StagingTextureManager::new(),
            _capture_item: capture_item,
            _frame_pool: frame_pool,
            _capture_session: capture_session,
            _d3d_device: d3d_device,
        })
    }

    fn enumerate_monitors() -> DomainResult<Vec<HMONITOR>> {
        extern "system" fn enum_proc(
            hmonitor: HMONITOR,
            _hdc: HDC,
            _rect: *mut RECT,
            lparam: LPARAM,
        ) -> BOOL {
            // SAFETY: `lparam` is provided by caller as pointer to `MonitorEnumData` valid for callback lifetime.
            unsafe {
                let data = &mut *(lparam.0 as *mut MonitorEnumData);
                data.monitors.push(hmonitor);
            }
            BOOL(1)
        }

        let mut data = MonitorEnumData {
            monitors: Vec::new(),
        };
        // SAFETY: callback and context pointer remain valid for the duration of this call.
        unsafe {
            let _ = EnumDisplayMonitors(
                HDC(0),
                None,
                Some(enum_proc),
                LPARAM((&mut data as *mut MonitorEnumData) as isize),
            );
        }

        if data.monitors.is_empty() {
            Err(DomainError::Initialization("No monitor found".to_string()))
        } else {
            Ok(data.monitors)
        }
    }

    fn create_capture_item_for_monitor(hmonitor: HMONITOR) -> DomainResult<GraphicsCaptureItem> {
        let interop: IGraphicsCaptureItemInterop =
            factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().map_err(|e| {
                DomainError::Initialization(format!(
                    "Failed to acquire GraphicsCaptureItem interop: {e:?}"
                ))
            })?;

        // SAFETY: interop object and monitor handle are valid for this call.
        unsafe {
            interop
                .CreateForMonitor(hmonitor)
                .map_err(|e| DomainError::Initialization(format!("CreateForMonitor failed: {e:?}")))
        }
    }

    fn create_d3d11_device() -> DomainResult<(ID3D11Device, ID3D11DeviceContext)> {
        let mut device = None;
        let mut context = None;

        // SAFETY: all pointers and parameters satisfy D3D11CreateDevice contract.
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                None,
                D3D11_CREATE_DEVICE_FLAG(D3D11_CREATE_DEVICE_BGRA_SUPPORT.0),
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .map_err(|e| {
                DomainError::Initialization(format!("Failed to create D3D11 device: {e:?}"))
            })?;
        }

        let device = device.ok_or_else(|| {
            DomainError::Initialization("D3D11CreateDevice returned no device".to_string())
        })?;
        let context = context.ok_or_else(|| {
            DomainError::Initialization("D3D11CreateDevice returned no context".to_string())
        })?;

        Ok((device, context))
    }

    fn create_direct3d_device(d3d_device: &ID3D11Device) -> DomainResult<IDirect3DDevice> {
        let dxgi_device: windows::Win32::Graphics::Dxgi::IDXGIDevice =
            d3d_device.cast().map_err(|e| {
                DomainError::Initialization(format!("Failed to cast to IDXGIDevice: {e:?}"))
            })?;

        // SAFETY: API requires a valid DXGI device and returns a WinRT inspectable wrapper.
        let inspectable =
            unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device) }.map_err(|e| {
                DomainError::Initialization(format!(
                    "Failed to create WinRT Direct3D device: {e:?}"
                ))
            })?;

        inspectable.cast().map_err(|e| {
            DomainError::Initialization(format!("Failed to cast to IDirect3DDevice: {e:?}"))
        })
    }

    fn get_texture_from_surface(surface: &IDirect3DSurface) -> DomainResult<ID3D11Texture2D> {
        let access: IDirect3DDxgiInterfaceAccess = surface.cast().map_err(|e| {
            DomainError::Capture(format!(
                "Failed to cast surface to IDirect3DDxgiInterfaceAccess: {e:?}"
            ))
        })?;

        // SAFETY: WGC surfaces expose DXGI interface access to underlying D3D11 texture.
        unsafe { access.GetInterface::<ID3D11Texture2D>() }
            .map_err(|e| DomainError::Capture(format!("Failed to get frame texture: {e:?}")))
    }

    fn read_latest_frame(&self) -> DomainResult<Option<CapturedFrameData>> {
        let guard = self
            .latest_frame
            .lock()
            .map_err(|e| DomainError::Capture(format!("Failed to lock latest frame: {e}")))?;
        Ok(guard.clone())
    }

    fn create_roi_texture(
        &self,
        source: &ID3D11Texture2D,
        roi: &Roi,
    ) -> DomainResult<ID3D11Texture2D> {
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
        // SAFETY: descriptor is fully initialized and out pointer is valid.
        unsafe {
            self.device
                .CreateTexture2D(&desc, None, Some(&mut roi_texture))
                .map_err(|e| {
                    DomainError::GpuTexture(format!("Failed to create ROI texture: {e:?}"))
                })?;
        }

        let roi_texture = roi_texture.ok_or_else(|| {
            DomainError::GpuTexture("CreateTexture2D returned None for ROI texture".to_string())
        })?;

        let src_resource: ID3D11Resource = source.cast().map_err(|e| {
            DomainError::GpuTexture(format!("Failed to cast source texture: {e:?}"))
        })?;
        let dst_resource: ID3D11Resource = roi_texture
            .cast()
            .map_err(|e| DomainError::GpuTexture(format!("Failed to cast ROI texture: {e:?}")))?;

        let src_box = D3D11_BOX {
            left: roi.x,
            top: roi.y,
            front: 0,
            right: roi.x + roi.width,
            bottom: roi.y + roi.height,
            back: 1,
        };

        // SAFETY: source and destination resources are valid and ROI bounds are clamped by caller.
        unsafe {
            self.context.CopySubresourceRegion(
                &dst_resource,
                0,
                0,
                0,
                0,
                &src_resource,
                0,
                Some(&src_box),
            );
        }

        Ok(roi_texture)
    }
}

impl CapturePort for WgcCaptureAdapter {
    fn capture_frame(&mut self, roi: &Roi) -> DomainResult<Option<Frame>> {
        let frame_data = match self.read_latest_frame()? {
            Some(data) => data,
            None => return Ok(None),
        };

        let clamped = clamp_roi(roi, frame_data.width, frame_data.height);
        if clamped.width == 0 || clamped.height == 0 {
            return Ok(None);
        }

        let staging = self.staging_manager.ensure_texture(
            &self.device,
            clamped.width,
            clamped.height,
            DXGI_FORMAT_B8G8R8A8_UNORM,
        )?;

        let src_resource: ID3D11Resource = frame_data.texture.cast().map_err(|e| {
            DomainError::Capture(format!("Failed to cast source frame texture: {e:?}"))
        })?;
        let dst_resource: ID3D11Resource = staging
            .cast()
            .map_err(|e| DomainError::Capture(format!("Failed to cast staging texture: {e:?}")))?;

        let src_box = D3D11_BOX {
            left: clamped.x,
            top: clamped.y,
            front: 0,
            right: clamped.x + clamped.width,
            bottom: clamped.y + clamped.height,
            back: 1,
        };

        // SAFETY: source and destination resources are compatible and ROI was clamped to frame bounds.
        unsafe {
            self.context.CopySubresourceRegion(
                &dst_resource,
                0,
                0,
                0,
                0,
                &src_resource,
                0,
                Some(&src_box),
            );
        }

        let data = copy_texture_to_cpu(&self.context, &staging, clamped.width, clamped.height)?;
        Ok(Some(Frame::new(data, clamped.width, clamped.height)))
    }

    fn capture_gpu_frame(&mut self, roi: &Roi) -> DomainResult<Option<GpuFrame>> {
        let frame_data = match self.read_latest_frame()? {
            Some(data) => data,
            None => return Ok(None),
        };

        let clamped = clamp_roi(roi, frame_data.width, frame_data.height);
        if clamped.width == 0 || clamped.height == 0 {
            return Ok(None);
        }

        let roi_texture = self.create_roi_texture(&frame_data.texture, &clamped)?;
        Ok(Some(GpuFrame::new(
            Some(roi_texture),
            clamped.width,
            clamped.height,
            DXGI_FORMAT_B8G8R8A8_UNORM,
        )))
    }

    fn reinitialize(&mut self) -> DomainResult<()> {
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
    fn wgc_adapter_implements_capture_port() {
        fn assert_capture_port_impl<T: CapturePort>() {}
        assert_capture_port_impl::<WgcCaptureAdapter>();
    }

    #[test]
    #[ignore]
    fn supports_gpu_frame_is_true_on_initialized_adapter() {
        if let Ok(adapter) = WgcCaptureAdapter::new(0) {
            assert!(adapter.supports_gpu_frame());
        }
    }

    #[test]
    #[ignore]
    fn reinitialize_returns_ok() {
        if let Ok(mut adapter) = WgcCaptureAdapter::new(0) {
            assert!(adapter.reinitialize().is_ok());
        }
    }

    #[test]
    #[ignore]
    fn device_info_has_dimensions() {
        if let Ok(adapter) = WgcCaptureAdapter::new(0) {
            let info = adapter.device_info();
            assert!(info.width > 0);
            assert!(info.height > 0);
            assert!(!info.name.is_empty());
        }
    }
}
