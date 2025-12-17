/// Infrastructure層: 外部技術の統合
/// 
/// Domain層のtraitを実装し、外部ライブラリ（DDA/OpenCV/HID/ORT）と接続する。

pub mod capture;
pub mod color_process;
pub mod hid_comm;
pub mod process_selector;
