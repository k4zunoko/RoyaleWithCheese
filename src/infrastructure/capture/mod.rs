/// Capture実装: 画面キャプチャの具体実装

pub mod dda;
pub mod spout;
pub mod spout_ffi;

#[allow(unused_imports)]  // main.rsで使用予定
pub use dda::DdaCaptureAdapter;
#[allow(unused_imports)]  // main.rsで使用予定
pub use spout::SpoutCaptureAdapter;
