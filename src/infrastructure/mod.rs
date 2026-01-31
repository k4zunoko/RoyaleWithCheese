//! Infrastructure層: 外部技術の統合
//!
//! Domain層のtraitを実装し、外部ライブラリ（DDA/OpenCV/HID/ORT）と接続する。

pub mod audio_feedback;
pub mod capture;
#[cfg(target_os = "windows")]
pub mod gpu_device;
pub mod hid_comm;
pub mod input;
pub mod process_selector;
pub mod processing;

// デバッグ表示モジュール（opencv-debug-display feature有効時のみ）
#[cfg(feature = "opencv-debug-display")]
pub mod debug_display;
