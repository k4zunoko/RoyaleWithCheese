//! Windows 入力監視実装（Infrastructure層）n//!
//! Win32 APIのGetAsyncKeyStateを使用してInputPort traitを実装します。
//!
//! # 低レイテンシ設計
//! - ポーリング方式（イベントドリブンより高速）
//! - 直接Windows API呼び出し（中間レイヤーなし）
//! - unsafeブロックはInfrastructure層に限定

use crate::domain::ports::InputPort;
use crate::domain::types::VirtualKey;
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

/// Windows入力アダプタ（Infrastructure層の実装）
///
/// GetAsyncKeyStateを使用してキー状態をポーリングします。
/// ステートレス（OSが真実の源）で、Send + Sync を実装します。
pub struct WindowsInputAdapter;

impl WindowsInputAdapter {
    /// 新しいWindowsInputAdapterを作成
    pub fn new() -> Self {
        Self
    }
}

impl Default for WindowsInputAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl InputPort for WindowsInputAdapter {
    fn is_key_pressed(&self, key: VirtualKey) -> bool {
        unsafe {
            // SAFETY: GetAsyncKeyStateは非同期キー状態をOS側で保持している。
            // 戻り値の最上位ビット（0x8000）が立っていれば現在押下中。
            // 戻り値はi16なので0x8000i16とマスク。
            let state = GetAsyncKeyState(key.to_vk_code() as i32);
            (state as u16 & 0x8000u16) != 0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_input_adapter_construction() {
        let adapter = WindowsInputAdapter::new();
        let adapter_default = WindowsInputAdapter::default();
        // Test that both constructions work (no panic)
        drop(adapter);
        drop(adapter_default);
    }

    #[test]
    fn windows_input_adapter_key_released_bit_encoding() {
        // Test the bit encoding logic when key is released (bit 15 = 0)
        // GetAsyncKeyState returns i16, but high bit is in bit 15 (0x8000)
        // When key is released: 0x0000 & 0x8000 = 0 (false)
        let key_released = 0x0000i16;
        let is_pressed = (key_released as u16 & 0x8000u16) != 0;
        assert!(!is_pressed, "Released key (0x0000) should not be pressed");
    }

    #[test]
    fn windows_input_adapter_key_held_bit_encoding() {
        // Test the bit encoding logic when key is held (bit 15 = 1)
        // When key is held: 0x8000 & 0x8000 = 0x8000 (true)
        let key_held = -32768i16; // 0x8000 as signed i16
        let is_pressed = (key_held as u16 & 0x8000u16) != 0;
        assert!(is_pressed, "Held key (0x8000) should be pressed");
    }

    #[test]
    fn windows_input_adapter_implements_send_sync() {
        // Compile-time check: WindowsInputAdapter must implement Send + Sync
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<WindowsInputAdapter>();
    }

    #[test]
    fn windows_input_adapter_input_port_trait_object() {
        // Verify that WindowsInputAdapter can be used as InputPort trait object
        let adapter = WindowsInputAdapter::new();
        let _port: &dyn InputPort = &adapter;
        // Test that InputPort trait object compiles and works
    }
}
