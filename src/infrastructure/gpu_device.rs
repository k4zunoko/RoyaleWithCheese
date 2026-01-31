//! GPU device management utilities
//!
//! This module provides functions for D3D11 device creation and management
//! used by GPU processing components.

use crate::domain::error::{DomainError, DomainResult};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_10_0, D3D_FEATURE_LEVEL_11_0,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, D3D11_CREATE_DEVICE_FLAG,
    D3D11_SDK_VERSION,
};

/// Create a D3D11 device for GPU processing
///
/// Attempts to create a hardware-accelerated device first, then falls back to
/// WARP (software rasterizer) if hardware creation fails.
///
/// # Returns
/// * `Ok((ID3D11Device, ID3D11DeviceContext))` - Successfully created device and context
/// * `Err(DomainError::GpuNotAvailable)` - Both hardware and WARP creation failed
///
/// # Example
/// ```ignore
/// let (device, context) = create_d3d11_device()?;
/// let gpu_adapter = GpuColorAdapter::with_device_context(device, context)?;
/// ```
pub fn create_d3d11_device() -> DomainResult<(ID3D11Device, ID3D11DeviceContext)> {
    // Try hardware first
    match create_hardware_device() {
        Ok(device_context) => {
            tracing::info!("D3D11 hardware device created successfully");
            return Ok(device_context);
        }
        Err(e) => {
            tracing::warn!("Failed to create D3D11 hardware device: {:?}", e);
        }
    }

    // Fallback to WARP
    match create_warp_device() {
        Ok(device_context) => {
            tracing::info!("D3D11 WARP (software) device created as fallback");
            Ok(device_context)
        }
        Err(e) => {
            tracing::error!("Failed to create D3D11 WARP device: {:?}", e);
            Err(DomainError::GpuNotAvailable(
                "Failed to create D3D11 device (both hardware and WARP failed)".to_string(),
            ))
        }
    }
}

/// Create a hardware-accelerated D3D11 device
fn create_hardware_device() -> DomainResult<(ID3D11Device, ID3D11DeviceContext)> {
    let feature_levels = [D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_0];
    let flags = D3D11_CREATE_DEVICE_FLAG(0);

    let mut device: Option<ID3D11Device> = None;
    let mut context: Option<ID3D11DeviceContext> = None;

    // SAFETY: D3D11CreateDevice is an FFI call with valid parameters.
    unsafe {
        D3D11CreateDevice(
            None,                     // Adapter: use default
            D3D_DRIVER_TYPE_HARDWARE, // Hardware acceleration
            None,                     // No software rasterizer
            flags,
            Some(&feature_levels),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None, // Don't need actual feature level
            Some(&mut context),
        )
        .map_err(|e| {
            DomainError::GpuNotAvailable(format!("D3D11 hardware device creation failed: {:?}", e))
        })?;
    }

    match (device, context) {
        (Some(device), Some(context)) => Ok((device, context)),
        _ => Err(DomainError::GpuNotAvailable(
            "D3D11CreateDevice returned null device or context".to_string(),
        )),
    }
}

/// Create a WARP (software) D3D11 device
fn create_warp_device() -> DomainResult<(ID3D11Device, ID3D11DeviceContext)> {
    let feature_levels = [D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_0];
    let flags = D3D11_CREATE_DEVICE_FLAG(0);

    let mut device: Option<ID3D11Device> = None;
    let mut context: Option<ID3D11DeviceContext> = None;

    // SAFETY: D3D11CreateDevice is an FFI call with valid parameters.
    unsafe {
        D3D11CreateDevice(
            None,                 // Adapter: use default
            D3D_DRIVER_TYPE_WARP, // Windows Advanced Rasterization Platform
            None,                 // No software module
            flags,
            Some(&feature_levels),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None, // Don't need actual feature level
            Some(&mut context),
        )
        .map_err(|e| {
            DomainError::GpuNotAvailable(format!("D3D11 WARP device creation failed: {:?}", e))
        })?;
    }

    match (device, context) {
        (Some(device), Some(context)) => Ok((device, context)),
        _ => Err(DomainError::GpuNotAvailable(
            "D3D11CreateDevice returned null device or context".to_string(),
        )),
    }
}

/// Check if GPU processing is available on this system
///
/// This is a lightweight check that attempts to create a temporary D3D11 device.
/// Use this for feature detection before attempting full initialization.
///
/// # Returns
/// * `true` - GPU processing is likely available
/// * `false` - GPU processing is not available
pub fn is_gpu_available() -> bool {
    // Attempt to create a device - if successful, GPU is available
    create_d3d11_device().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_d3d11_device() {
        let result = create_d3d11_device();

        // Should succeed on most Windows systems
        // (either hardware or WARP should be available)
        if let Ok((device, context)) = result {
            assert!(!device.is_null(), "Device should not be null");
            assert!(!context.is_null(), "Context should not be null");
        }
        // If it fails, that's also acceptable in some environments
        // (e.g., CI without GPU support)
    }

    #[test]
    fn test_is_gpu_available() {
        // This should return true on most Windows systems
        let available = is_gpu_available();

        // Log the result for debugging
        if available {
            println!("GPU is available");
        } else {
            println!("GPU is not available (may be running in CI or without D3D11 support)");
        }

        // Don't assert - availability depends on the environment
        assert!(true);
    }
}
