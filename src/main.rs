mod domain;
mod logging;

use crate::logging::init_logging;

fn main() {
    // ログシステムの初期化
    init_logging("info", false);

    tracing::info!("RoyaleWithCheese starting...");

    // TODO: Application層の実装後、パイプライン起動処理を追加

    tracing::info!("RoyaleWithCheese terminated.");
}
