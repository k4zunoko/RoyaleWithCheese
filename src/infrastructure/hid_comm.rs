//! HID communication adapter

use crate::domain::config::CommunicationConfig;
use crate::domain::error::{DomainError, DomainResult};
use crate::domain::ports::CommPort;
use hidapi::{HidApi, HidDevice};
use std::ffi::CString;

/// HID communication adapter.
///
/// Device open priority:
/// 1. device_path
/// 2. serial_number
/// 3. vendor_id + product_id
pub struct HidCommAdapter {
    api: HidApi,
    device: Option<HidDevice>,
    vendor_id: u16,
    product_id: u16,
    serial_number: Option<String>,
    device_path: Option<String>,
}

impl HidCommAdapter {
    /// Constructs adapter from communication config.
    pub fn new(config: CommunicationConfig) -> DomainResult<Self> {
        let vendor_id = u16::try_from(config.vendor_id).map_err(|_| {
            DomainError::Communication(format!(
                "communication.vendor_id out of u16 range: {}",
                config.vendor_id
            ))
        })?;
        let product_id = u16::try_from(config.product_id).map_err(|_| {
            DomainError::Communication(format!(
                "communication.product_id out of u16 range: {}",
                config.product_id
            ))
        })?;

        Self::with_identifiers(vendor_id, product_id, None, None)
    }

    /// Constructs adapter from explicit identifiers.
    pub fn with_identifiers(
        vendor_id: u16,
        product_id: u16,
        serial_number: Option<String>,
        device_path: Option<String>,
    ) -> DomainResult<Self> {
        let api = HidApi::new().map_err(|err| {
            DomainError::Communication(format!("failed to initialize hidapi: {}", err))
        })?;

        let device = Self::try_open_device(
            &api,
            vendor_id,
            product_id,
            serial_number.as_deref(),
            device_path.as_deref(),
        )?;

        Ok(Self {
            api,
            device,
            vendor_id,
            product_id,
            serial_number,
            device_path,
        })
    }

    fn try_open_device(
        api: &HidApi,
        vendor_id: u16,
        product_id: u16,
        serial_number: Option<&str>,
        device_path: Option<&str>,
    ) -> DomainResult<Option<HidDevice>> {
        if let Some(path) = device_path {
            let c_path = CString::new(path).map_err(|err| {
                DomainError::Communication(format!(
                    "invalid HID device path (contains NUL): {} ({})",
                    path, err
                ))
            })?;

            return match api.open_path(c_path.as_c_str()) {
                Ok(device) => Ok(Some(device)),
                Err(_err) => Ok(None),
            };
        }

        if let Some(serial) = serial_number {
            return match api.open_serial(vendor_id, product_id, serial) {
                Ok(device) => Ok(Some(device)),
                Err(_err) => Ok(None),
            };
        }

        match api.open(vendor_id, product_id) {
            Ok(device) => Ok(Some(device)),
            Err(_err) => Ok(None),
        }
    }

    fn open_for_reconnect(&self, api: &HidApi) -> DomainResult<HidDevice> {
        if let Some(path) = self.device_path.as_deref() {
            let c_path = CString::new(path).map_err(|err| {
                DomainError::Communication(format!(
                    "invalid HID device path (contains NUL): {} ({})",
                    path, err
                ))
            })?;

            return api.open_path(c_path.as_c_str()).map_err(|err| {
                DomainError::Communication(format!(
                    "failed to reconnect HID device by path '{}': {}",
                    path, err
                ))
            });
        }

        if let Some(serial) = self.serial_number.as_deref() {
            return api
                .open_serial(self.vendor_id, self.product_id, serial)
                .map_err(|err| {
                    DomainError::Communication(format!(
                        "failed to reconnect HID device (vid=0x{:04X}, pid=0x{:04X}, serial='{}'): {}",
                        self.vendor_id, self.product_id, serial, err
                    ))
                });
        }

        api.open(self.vendor_id, self.product_id).map_err(|err| {
            DomainError::Communication(format!(
                "failed to reconnect HID device (vid=0x{:04X}, pid=0x{:04X}): {}",
                self.vendor_id, self.product_id, err
            ))
        })
    }
}

impl CommPort for HidCommAdapter {
    fn send(&mut self, data: &[u8]) -> DomainResult<()> {
        let device = match self.device.as_mut() {
            Some(device) => device,
            None => {
                return Err(DomainError::Communication(format!(
                    "HID device not connected (vid=0x{:04X}, pid=0x{:04X})",
                    self.vendor_id, self.product_id
                )));
            }
        };

        if let Err(err) = device.write(data) {
            self.device = None;
            return Err(DomainError::Communication(format!(
                "HID write failed (vid=0x{:04X}, pid=0x{:04X}): {}",
                self.vendor_id, self.product_id, err
            )));
        }

        Ok(())
    }

    fn reconnect(&mut self) -> DomainResult<()> {
        self.device = None;

        let new_api = HidApi::new().map_err(|err| {
            DomainError::Communication(format!("failed to reinitialize hidapi: {}", err))
        })?;

        let device = self.open_for_reconnect(&new_api)?;
        self.api = new_api;
        self.device = Some(device);

        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.device.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> CommunicationConfig {
        CommunicationConfig {
            vendor_id: 0x1234,
            product_id: 0x5678,
            hid_send_interval_ms: 8,
        }
    }

    #[test]
    fn hid_adapter_construction_with_valid_config() {
        let adapter = HidCommAdapter::new(test_config());
        assert!(adapter.is_ok());
    }

    #[test]
    fn hid_adapter_is_connected_false_when_no_device() {
        let adapter = HidCommAdapter::new(test_config()).expect("construction should succeed");
        assert!(!adapter.is_connected());
    }

    #[test]
    fn hid_adapter_send_returns_error_when_not_connected() {
        let mut adapter = HidCommAdapter::new(test_config()).expect("construction should succeed");
        let err = adapter
            .send(&[0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF])
            .expect_err("send should fail while disconnected");

        assert!(matches!(err, DomainError::Communication(_)));
    }

    #[test]
    #[ignore]
    fn hid_adapter_reconnect_requires_actual_device() {
        let mut adapter = HidCommAdapter::new(test_config()).expect("construction should succeed");
        let _ = adapter.reconnect();
    }
}
