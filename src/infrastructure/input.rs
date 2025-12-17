//! Windows 入力監視実装（Infrastructure層）
//!
//! GetAsyncKeyState APIを使用してInputPort traitを実装します。

use crate::domain::ports::{InputPort, InputState, VirtualKey};
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

/// Windows入力アダプタ（Infrastructure層の実装）
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
            // GetAsyncKeyStateの最上位ビット（0x8000）が立っていれば現在押下中
            // 戻り値はi16なので0x8000i16とマスク
            (GetAsyncKeyState(key.to_vk_code()) & 0x8000u16 as i16) != 0
        }
    }

    fn poll_input_state(&self) -> InputState {
        InputState {
            mouse_left: self.is_key_pressed(VirtualKey::LeftButton),
            mouse_right: self.is_key_pressed(VirtualKey::RightButton),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 注: これらのテストはWindows環境でのみ有効で、
    // 実際のキー入力がある場合にのみパスする可能性があります。
    // CI環境では#[ignore]を使用することを推奨します。

    #[test]
    #[ignore] // 手動テスト用
    fn test_is_key_pressed() {
        let adapter = WindowsInputAdapter::new();
        
        // Insertキーを押してテストを実行
        println!("Press INSERT key...");
        std::thread::sleep(std::time::Duration::from_secs(2));
        
        // この時点でInsertキーが押されていればtrue
        let pressed = adapter.is_key_pressed(VirtualKey::Insert);
        println!("INSERT key pressed: {}", pressed);
    }
}
