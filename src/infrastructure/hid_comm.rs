/// HID通信アダプタ
/// 
/// hidapiを使用したHIDデバイスとの通信実装。
/// 低レイテンシを重視し、非ブロッキング送信を行う。

use crate::domain::{CommPort, DomainError, DomainResult};
use hidapi::{HidApi, HidDevice};
use std::sync::Mutex;
use std::time::Duration;

/// HID通信アダプタ
/// 
/// HidDeviceはSync traitを実装していないため、Mutexでラップする。
/// これによりスレッド間で安全に共有できる。
pub struct HidCommAdapter {
    /// HIDデバイスハンドル（Mutexでラップ）
    device: Mutex<Option<HidDevice>>,
    /// HID API インスタンス（Mutexでラップ）
    api: Mutex<HidApi>,
    /// Vendor ID
    vendor_id: u16,
    /// Product ID
    product_id: u16,
    /// 送信タイムアウト
    #[allow(dead_code)]
    send_timeout: Duration,
}

impl HidCommAdapter {
    /// 新しいHID通信アダプタを作成
    /// 
    /// # Arguments
    /// - `vendor_id`: HIDデバイスのVendor ID
    /// - `product_id`: HIDデバイスのProduct ID
    /// - `send_timeout_ms`: 送信タイムアウト（ミリ秒）、現在は未使用
    /// 
    /// # Returns
    /// HidCommAdapterインスタンス
    /// 
    /// # Errors
    /// - HIDAPI初期化失敗
    /// - デバイスオープン失敗（初回接続時）
    pub fn new(
        vendor_id: u16,
        product_id: u16,
        send_timeout_ms: u64,
    ) -> DomainResult<Self> {
        let api = HidApi::new()
            .map_err(|e| DomainError::Communication(format!("Failed to initialize HIDAPI: {:?}", e)))?;
        
        let send_timeout = Duration::from_millis(send_timeout_ms);
        
        // デバイスのオープンを試行
        let device = match api.open(vendor_id, product_id) {
            Ok(dev) => {
                tracing::info!(
                    "HID device opened: VID=0x{:04X}, PID=0x{:04X}",
                    vendor_id,
                    product_id
                );
                
                Some(dev)
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to open HID device (VID=0x{:04X}, PID=0x{:04X}): {:?}. Will retry on reconnect.",
                    vendor_id,
                    product_id,
                    e
                );
                None
            }
        };
        
        Ok(Self {
            device: Mutex::new(device),
            api: Mutex::new(api),
            vendor_id,
            product_id,
            send_timeout,
        })
    }
    
    /// デバイス情報を取得して表示（デバッグ用）
    #[allow(dead_code)]
    pub fn print_device_info(&self) -> DomainResult<()> {
        let device_guard = self.device.lock().unwrap();
        if let Some(ref device) = *device_guard {
            let manufacturer = device.get_manufacturer_string()
                .unwrap_or_else(|_| Some("Unknown".to_string()))
                .unwrap_or_else(|| "N/A".to_string());
            
            let product = device.get_product_string()
                .unwrap_or_else(|_| Some("Unknown".to_string()))
                .unwrap_or_else(|| "N/A".to_string());
            
            let serial = device.get_serial_number_string()
                .unwrap_or_else(|_| Some("Unknown".to_string()))
                .unwrap_or_else(|| "N/A".to_string());
            
            tracing::info!(
                "HID Device Info - Manufacturer: {}, Product: {}, Serial: {}",
                manufacturer,
                product,
                serial
            );
        }
        
        Ok(())
    }
}

impl CommPort for HidCommAdapter {
    /// HIDレポートを送信
    /// 
    /// # Arguments
    /// - `data`: 送信データ（最初のバイトはReport ID）
    /// 
    /// # Returns
    /// - `Ok(())`: 送信成功
    /// - `Err(DomainError)`: 送信失敗（デバイス切断等）
    /// 
    /// # 低レイテンシ最適化
    /// - 非ブロッキングモードで送信
    /// - エラー時は自動再接続を試行せず、即座にエラーを返す
    ///   （再接続は明示的なreconnect()呼び出しで実行）
    fn send(&mut self, data: &[u8]) -> DomainResult<()> {
        if data.is_empty() {
            return Err(DomainError::Communication("Empty data".to_string()));
        }
        
        let mut device_guard = self.device.lock().unwrap();
        let result = if let Some(ref mut device) = *device_guard {
            device.write(data)
        } else {
            Err(hidapi::HidError::HidApiError {
                message: "Device not connected".to_string(),
            })
        };
        
        match result {
            Ok(bytes_written) => {
                #[cfg(debug_assertions)]
                {
                    if bytes_written != data.len() {
                        tracing::warn!(
                            "Partial write: {} bytes written out of {}",
                            bytes_written,
                            data.len()
                        );
                    }
                }
                Ok(())
            }
            Err(e) => {
                #[cfg(debug_assertions)]
                tracing::error!("HID write failed: {:?}", e);
                
                // デバイス切断と判断
                *device_guard = None;
                
                Err(DomainError::Communication(format!("HID write failed: {:?}", e)))
            }
        }
    }
    
    /// デバイスとの接続状態を確認
    /// 
    /// # Returns
    /// - `true`: 接続中
    /// - `false`: 未接続
    fn is_connected(&self) -> bool {
        self.device.lock().unwrap().is_some()
    }
    
    /// デバイスとの接続を再試行
    /// 
    /// # Returns
    /// - `Ok(())`: 再接続成功
    /// - `Err(DomainError)`: 再接続失敗
    /// 
    /// # 設計ノート
    /// - レート制限や指数バックオフはApplication層で実装
    /// - Infrastructure層はシンプルに再接続のみ行う
    fn reconnect(&mut self) -> DomainResult<()> {
        tracing::info!(
            "Attempting to reconnect HID device (VID=0x{:04X}, PID=0x{:04X})...",
            self.vendor_id,
            self.product_id
        );
        
        // HID APIを再初期化（デバイス列挙を更新）
        let new_api = HidApi::new()
            .map_err(|e| DomainError::Communication(format!("Failed to reinitialize HIDAPI: {:?}", e)))?;
        
        // デバイスをオープン
        let device = new_api.open(self.vendor_id, self.product_id)
            .map_err(|e| DomainError::Communication(format!("Failed to open HID device: {:?}", e)))?;
        
        // 更新
        *self.api.lock().unwrap() = new_api;
        *self.device.lock().unwrap() = Some(device);
        
        tracing::info!("HID device reconnected successfully");
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_adapter_creation() {
        // ダミーのVID/PIDで作成（実デバイスなしでも成功する設計）
        let adapter = HidCommAdapter::new(0x0000, 0x0000, 10);
        assert!(adapter.is_ok());
        
        let adapter = adapter.unwrap();
        // デバイスが接続されていない場合はNone
        assert!(!adapter.is_connected());
    }
    
    #[test]
    fn test_send_without_device() {
        let mut adapter = HidCommAdapter::new(0x0000, 0x0000, 10).unwrap();
        
        // デバイス未接続の状態で送信
        let data = vec![0x01, 0x02, 0x03];
        let result = adapter.send(&data);
        
        // エラーが返されることを確認
        assert!(result.is_err());
    }
    
    #[test]
    fn test_send_empty_data() {
        let mut adapter = HidCommAdapter::new(0x0000, 0x0000, 10).unwrap();
        
        // 空データを送信
        let data = vec![];
        let result = adapter.send(&data);
        
        // エラーが返されることを確認
        assert!(result.is_err());
    }
    
    #[test]
    fn test_reconnect_without_device() {
        let mut adapter = HidCommAdapter::new(0x0000, 0x0000, 10).unwrap();
        
        // デバイスが存在しないので再接続は失敗する
        let result = adapter.reconnect();
        assert!(result.is_err());
    }
}
