/// HID通信アダプタ
/// 
/// hidapiを使用したHIDデバイスとの通信実装。
/// 低レイテンシを重視し、非ブロッキング送信を行う。

use crate::domain::{CommPort, DomainError, DomainResult};
use hidapi::{HidApi, HidDevice};
use std::sync::Mutex;

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
    /// シリアル番号（オプション）
    serial_number: Option<String>,
    /// デバイスパス（オプション）
    device_path: Option<String>,
}

impl HidCommAdapter {
    /// 新しいHID通信アダプタを作成
    /// 
    /// # Arguments
    /// - `vendor_id`: HIDデバイスのVendor ID
    /// - `product_id`: HIDデバイスのProduct ID
    /// - `serial_number`: デバイスのシリアル番号（オプション）
    /// - `device_path`: デバイスパス（オプション、最優先）
    /// 
    /// # Returns
    /// HidCommAdapterインスタンス
    /// 
    /// # Errors
    /// - HIDAPI初期化失敗
    /// - デバイスオープン失敗（初回接続時）
    /// 
    /// # デバイス識別の優先順位
    /// 1. device_path（最も確実）
    /// 2. vendor_id + product_id + serial_number
    /// 3. vendor_id + product_id（最初にマッチしたデバイス）
    pub fn new(
        vendor_id: u16,
        product_id: u16,
        serial_number: Option<String>,
        device_path: Option<String>,
    ) -> DomainResult<Self> {
        let api = HidApi::new()
            .map_err(|e| DomainError::Communication(format!("Failed to initialize HIDAPI: {:?}", e)))?;
        
        // デバイスのオープンを試行（優先順位: device_path > serial_number > vid/pid）
        let device = if let Some(ref path) = device_path {
            // デバイスパスで開く（最も確実）
            match api.open_path(std::ffi::CString::new(path.as_bytes()).unwrap().as_c_str()) {
                Ok(dev) => {
                    tracing::info!("HID device opened by path: {}", path);
                    Some(dev)
                }
                Err(e) => {
                    tracing::warn!("Failed to open HID device by path '{}': {:?}. Will retry on reconnect.", path, e);
                    None
                }
            }
        } else if let Some(ref serial) = serial_number {
            // シリアル番号で開く
            match api.open_serial(vendor_id, product_id, serial) {
                Ok(dev) => {
                    tracing::info!(
                        "HID device opened: VID=0x{:04X}, PID=0x{:04X}, SN={}",
                        vendor_id,
                        product_id,
                        serial
                    );
                    Some(dev)
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to open HID device (VID=0x{:04X}, PID=0x{:04X}, SN={}): {:?}. Will retry on reconnect.",
                        vendor_id,
                        product_id,
                        serial,
                        e
                    );
                    None
                }
            }
        } else {
            // VID/PIDのみで開く
            match api.open(vendor_id, product_id) {
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
            }
        };
        
        Ok(Self {
            device: Mutex::new(device),
            api: Mutex::new(api),
            vendor_id,
            product_id,
            serial_number,
            device_path,
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
        let log_msg = if let Some(ref path) = self.device_path {
            format!("Attempting to reconnect HID device by path: {}", path)
        } else if let Some(ref serial) = self.serial_number {
            format!(
                "Attempting to reconnect HID device (VID=0x{:04X}, PID=0x{:04X}, SN={})...",
                self.vendor_id, self.product_id, serial
            )
        } else {
            format!(
                "Attempting to reconnect HID device (VID=0x{:04X}, PID=0x{:04X})...",
                self.vendor_id, self.product_id
            )
        };
        tracing::info!("{}", log_msg);
        
        // HID APIを再初期化（デバイス列挙を更新）
        let new_api = HidApi::new()
            .map_err(|e| DomainError::Communication(format!("Failed to reinitialize HIDAPI: {:?}", e)))?;
        
        // デバイスをオープン（優先順位: device_path > serial_number > vid/pid）
        let device = if let Some(ref path) = self.device_path {
            new_api.open_path(std::ffi::CString::new(path.as_bytes()).unwrap().as_c_str())
                .map_err(|e| DomainError::Communication(format!("Failed to open HID device by path: {:?}", e)))?
        } else if let Some(ref serial) = self.serial_number {
            new_api.open_serial(self.vendor_id, self.product_id, serial)
                .map_err(|e| DomainError::Communication(format!("Failed to open HID device with serial: {:?}", e)))?
        } else {
            new_api.open(self.vendor_id, self.product_id)
                .map_err(|e| DomainError::Communication(format!("Failed to open HID device: {:?}", e)))?
        };
        
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
    use std::time::Duration;
    
    #[test]
    fn test_adapter_creation() {
        // ダミーのVID/PIDで作成（実デバイスなしでも成功する設計）
        let adapter = HidCommAdapter::new(0x0000, 0x0000, None, None);
        assert!(adapter.is_ok());
        
        let adapter = adapter.unwrap();
        // デバイスが接続されていない場合はNone
        assert!(!adapter.is_connected());
    }
    
    #[test]
    fn test_send_without_device() {
        let mut adapter = HidCommAdapter::new(0x0000, 0x0000, None, None).unwrap();
        
        // デバイス未接続の状態で送信
        let data = vec![0x01, 0x02, 0x03];
        let result = adapter.send(&data);
        
        // エラーが返されることを確認
        assert!(result.is_err());
    }
    
    #[test]
    fn test_send_empty_data() {
        let mut adapter = HidCommAdapter::new(0x0000, 0x0000, None, None).unwrap();
        
        // 空データを送信
        let data = vec![];
        let result = adapter.send(&data);
        
        // エラーが返されることを確認
        assert!(result.is_err());
    }
    
    #[test]
    fn test_reconnect_without_device() {
        let mut adapter = HidCommAdapter::new(0x0000, 0x0000, None, None).unwrap();
        
        // デバイスが存在しないので再接続は失敗する
        let result = adapter.reconnect();
        assert!(result.is_err());
    }
    
    /// HID通信確認テスト（実デバイス必須）
    /// # 事前準備
    /// 1. `test_enumerate_hid_devices`でデバイスパスを取得
    /// 2. 以下のコード内の`DEVICE_PATH`を実際のパスに変更
    #[test]
    #[ignore]
    fn test_hid_communication() {
        use std::thread;
        
        // ===== テスト設定 =====
        // ここに実際のデバイスパスを設定してください
        // 例 (Windows): r"\\?\hid#vid_2341&pid_8036#..."
        // 例 (Linux):   "/dev/hidraw0"
        const DEVICE_PATH: &str = r"\\?\\HID#VID_258A&PID_1007&MI_02#8&7c5162e&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}";
        
        // 送信するテストパケット（8バイト）
        let test_packet: [u8; 8] = [
            0x01, 0x00, 0x00, 0x0F, 0xFF, 0x00, 0x00, 0xFF,
        ];
        
        const SEND_COUNT: usize = 10;
        const SEND_INTERVAL_MS: u64 = 1000;
        // ===== テスト設定ここまで =====
        
        println!("\n========== HID Communication Test ==========");
        println!("Device Path: {}", DEVICE_PATH);
        println!("Packet Size: {} bytes", test_packet.len());
        println!("Send Count:  {}", SEND_COUNT);
        println!("Interval:    {} ms", SEND_INTERVAL_MS);
        println!("===========================================\n");
        
        // HIDアダプタを作成（デバイスパスを使用）
        let mut adapter = match HidCommAdapter::new(
            0x0000, // VID（パスで開くため不使用）
            0x0000, // PID（パスで開くため不使用）
            None,   // シリアル番号（不使用）
            Some(DEVICE_PATH.to_string()),
        ) {
            Ok(adapter) => adapter,
            Err(e) => {
                panic!("Failed to create HID adapter: {:?}\nPlease check DEVICE_PATH is correct.", e);
            }
        };
        
        // デバイスが接続されているか確認
        if !adapter.is_connected() {
            panic!("Device is not connected. Please check the device path.");
        }
        
        println!("Device connected successfully.\n");
        
        // 10回送信
        let mut success_count = 0;
        let mut error_count = 0;
        
        for i in 1..=SEND_COUNT {
            println!("[{}/{}] Sending packet...", i, SEND_COUNT);
            
            match adapter.send(&test_packet.to_vec()) {
                Ok(_) => {
                    println!("  ✓ Sent successfully");
                    success_count += 1;
                }
                Err(e) => {
                    println!("  ✗ Error: {:?}", e);
                    error_count += 1;
                }
            }
            
            // 最後の送信以外は待機
            if i < SEND_COUNT {
                thread::sleep(Duration::from_millis(SEND_INTERVAL_MS));
            }
        }
        
        println!("\n========== Test Results ==========");
        println!("Success: {} / {}", success_count, SEND_COUNT);
        println!("Error:   {} / {}", error_count, SEND_COUNT);
        println!("==================================\n");
        
        // 少なくとも1回は成功することを確認
        assert!(success_count > 0, "At least one packet should be sent successfully");
    }
    
    /// PCに接続されているHIDデバイスをすべて列挙するテスト
    /// 
    /// `cargo test test_enumerate_hid_devices -- --nocapture` で実行してください。
    /// 実際のデバイス情報が出力されます。
    #[test]
    fn test_enumerate_hid_devices() {
        use hidapi::HidApi;
        
        println!("\n========== Enumerating HID Devices ==========\n");
        
        let api = match HidApi::new() {
            Ok(api) => api,
            Err(e) => {
                println!("Failed to initialize HIDAPI: {:?}", e);
                return;
            }
        };
        
        let devices = api.device_list();
        let mut count = 0;
        
        for device in devices {
            count += 1;
            println!("Device #{}:", count);
            println!("  Vendor ID:       0x{:04X}", device.vendor_id());
            println!("  Product ID:      0x{:04X}", device.product_id());
            println!("  Path:            {:?}", device.path());
            
            if let Some(serial) = device.serial_number() {
                println!("  Serial Number:   {}", serial);
            } else {
                println!("  Serial Number:   (none)");
            }
            
            if let Some(manufacturer) = device.manufacturer_string() {
                println!("  Manufacturer:    {}", manufacturer);
            }
            
            if let Some(product) = device.product_string() {
                println!("  Product:         {}", product);
            }
            
            println!("  Release Number:  {}", device.release_number());
            println!("  Interface:       {}", device.interface_number());
            println!("  Usage Page:      0x{:04X}", device.usage_page());
            println!("  Usage:           0x{:04X}", device.usage());
            println!();
        }
        
        println!("========== Total: {} HID devices found ==========\n", count);
        
        // このテストは常に成功（列挙のみ）
        assert!(true);
    }
}
