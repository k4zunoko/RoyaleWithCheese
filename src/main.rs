mod domain;
mod logging;
mod application;
mod infrastructure;

use crate::logging::init_logging;
use std::path::PathBuf;

// #![windows_subsystem = "windows"] // ← これでコンソール非表示（GUIサブシステム）
fn main() {
    // ログシステムの初期化（非同期ファイル出力）
    // WindowsGUIサブシステムではコンソールが使えないため、ファイル出力必須
    let log_dir = PathBuf::from("logs");
    let _guard = init_logging("info", false, Some(log_dir));
    // 注意: _guardはmain終了まで保持する必要がある（Dropでログスレッドが終了）

    tracing::info!("RoyaleWithCheese starting...");

    // TODO: Application層の実装後、パイプライン起動処理を追加

    tracing::info!("RoyaleWithCheese terminated.");
}
