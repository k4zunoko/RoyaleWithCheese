//! Capture実装: 画面キャプチャの具体実装
//!
//! DDA、Spout、WGC（Phase 2実装中）の3つのキャプチャ方式を提供。
//! 共通処理は`common`モジュールに集約されている。

pub mod common;
pub mod dda;
pub mod spout;
pub mod spout_ffi;
pub mod wgc;

// main.rsで直接infrastructure::capture::dda::DdaCaptureAdapterを使用しているため、
// このre-exportは主に外部APIとしての利便性のため
#[allow(unused_imports)]
pub use dda::DdaCaptureAdapter;
#[allow(unused_imports)]
pub use spout::SpoutCaptureAdapter;
#[allow(unused_imports)]
pub use wgc::WgcCaptureAdapter;
