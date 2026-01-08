//! Spout DX11 FFI バインディング
//!
//! third_party/spoutdx-ffi のC APIをRustから呼び出すための
//! 安全なラッパーを提供します。

use std::ffi::{c_char, c_int, c_uint, c_void};

/// FFI戻り値
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpoutDxResult {
    Ok = 0,
    ErrorNullHandle = -1,
    ErrorNullDevice = -2,
    ErrorNotConnected = -3,
    ErrorInitFailed = -4,
    ErrorReceiveFailed = -5,
    ErrorInternal = -99,
}

impl SpoutDxResult {
    pub fn is_ok(self) -> bool {
        self == SpoutDxResult::Ok
    }
    
    pub fn from_raw(value: c_int) -> Self {
        match value {
            0 => SpoutDxResult::Ok,
            -1 => SpoutDxResult::ErrorNullHandle,
            -2 => SpoutDxResult::ErrorNullDevice,
            -3 => SpoutDxResult::ErrorNotConnected,
            -4 => SpoutDxResult::ErrorInitFailed,
            -5 => SpoutDxResult::ErrorReceiveFailed,
            _ => SpoutDxResult::ErrorInternal,
        }
    }
}

/// 送信者情報
#[repr(C)]
#[derive(Debug, Clone)]
pub struct SpoutDxSenderInfo {
    pub name: [c_char; 256],
    pub width: c_uint,
    pub height: c_uint,
    pub format: c_uint, // DXGI_FORMAT
}

impl Default for SpoutDxSenderInfo {
    fn default() -> Self {
        Self {
            name: [0; 256],
            width: 0,
            height: 0,
            format: 0,
        }
    }
}

impl SpoutDxSenderInfo {
    /// C文字列をRust Stringに変換
    pub fn name_as_string(&self) -> String {
        let bytes: Vec<u8> = self.name.iter()
            .take_while(|&&c| c != 0)
            .map(|&c| c as u8)
            .collect();
        String::from_utf8_lossy(&bytes).to_string()
    }
}

/// Spoutレシーバーハンドル（不透明ポインタ）
pub type SpoutDxReceiverHandle = *mut c_void;

#[link(name = "spoutdx_ffi")]
extern "C" {
    // バージョン情報
    #[allow(dead_code)]
    pub fn spoutdx_ffi_version() -> *const c_char;
    #[allow(dead_code)]
    pub fn spoutdx_ffi_get_sdk_version() -> c_int;
    #[allow(dead_code)]
    pub fn spoutdx_ffi_test_dx11_init() -> c_int;

    // ライフサイクル
    pub fn spoutdx_receiver_create() -> SpoutDxReceiverHandle;
    pub fn spoutdx_receiver_destroy(handle: SpoutDxReceiverHandle) -> c_int;

    // DirectX初期化
    pub fn spoutdx_receiver_open_dx11(
        handle: SpoutDxReceiverHandle,
        device: *mut c_void,
    ) -> c_int;
    pub fn spoutdx_receiver_close_dx11(handle: SpoutDxReceiverHandle) -> c_int;

    // 受信設定
    pub fn spoutdx_receiver_set_sender_name(
        handle: SpoutDxReceiverHandle,
        sender_name: *const c_char,
    ) -> c_int;

    // テクスチャ受信
    pub fn spoutdx_receiver_receive_texture(
        handle: SpoutDxReceiverHandle,
        dst_texture: *mut c_void,
    ) -> c_int;
    
    #[allow(dead_code)]
    pub fn spoutdx_receiver_release(handle: SpoutDxReceiverHandle) -> c_int;

    // 状態クエリ
    pub fn spoutdx_receiver_get_sender_info(
        handle: SpoutDxReceiverHandle,
        out_info: *mut SpoutDxSenderInfo,
    ) -> c_int;
    
    #[allow(dead_code)]
    pub fn spoutdx_receiver_is_updated(handle: SpoutDxReceiverHandle) -> c_int;
    #[allow(dead_code)]
    pub fn spoutdx_receiver_is_connected(handle: SpoutDxReceiverHandle) -> c_int;
    pub fn spoutdx_receiver_is_frame_new(handle: SpoutDxReceiverHandle) -> c_int;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spout_dx_result_conversion() {
        assert!(SpoutDxResult::from_raw(0).is_ok());
        assert_eq!(SpoutDxResult::from_raw(-3), SpoutDxResult::ErrorNotConnected);
        assert_eq!(SpoutDxResult::from_raw(-99), SpoutDxResult::ErrorInternal);
        assert_eq!(SpoutDxResult::from_raw(-999), SpoutDxResult::ErrorInternal);
    }

    #[test]
    fn test_sender_info_name_parsing() {
        let mut info = SpoutDxSenderInfo::default();
        info.name[0..4].copy_from_slice(&[b'T' as i8, b'e' as i8, b's' as i8, b't' as i8]);
        assert_eq!(info.name_as_string(), "Test");
        
        // 空文字列のテスト
        let empty = SpoutDxSenderInfo::default();
        assert_eq!(empty.name_as_string(), "");
    }
}
