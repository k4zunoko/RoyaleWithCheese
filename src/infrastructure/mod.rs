//! Infrastructure層: 外部技術の統合
//!
//! Domain層のtraitを実装し、外部ライブラリ（DDA/OpenCV/HID/ORT）と接続する。

pub mod capture;
pub mod color_process;
pub mod hid_comm;
pub mod process_selector;
pub mod input;
pub mod audio_feedback;

// デバッグ表示モジュール（opencv-debug-display feature有効時のみ）
#[cfg(feature = "opencv-debug-display")]
pub mod debug_display;
