//! D3D11 device creation helper for GPU processing.

use crate::domain::error::{DomainError, DomainResult};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_10_0, D3D_FEATURE_LEVEL_11_0,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, D3D11_CREATE_DEVICE_FLAG,
    D3D11_SDK_VERSION,
};

pub fn create_d3d11_device() -> DomainResult<(ID3D11Device, ID3D11DeviceContext)> {
    create_device_for_driver(D3D_DRIVER_TYPE_HARDWARE)
}

fn create_device_for_driver(
    driver_type: D3D_DRIVER_TYPE,
) -> DomainResult<(ID3D11Device, ID3D11DeviceContext)> {
    let feature_levels = [D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_0];
    let flags = D3D11_CREATE_DEVICE_FLAG(0);

    let mut device: Option<ID3D11Device> = None;
    let mut context: Option<ID3D11DeviceContext> = None;

    // SAFETY: D3D11CreateDevice call with valid pointers and options.
    unsafe {
        D3D11CreateDevice(
            None,
            driver_type,
            None,
            flags,
            Some(&feature_levels),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
        .map_err(|e| DomainError::GpuNotAvailable(format!("D3D11CreateDevice failed: {e:?}")))?;
    }

    let device =
        device.ok_or_else(|| DomainError::GpuNotAvailable("D3D11 device is null".to_string()))?;
    let context =
        context.ok_or_else(|| DomainError::GpuNotAvailable("D3D11 context is null".to_string()))?;

    Ok((device, context))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_d3d11_device_returns_result() {
        let _ = create_d3d11_device();
    }
}
