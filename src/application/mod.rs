//! Application Layer
//!
//! パイプライン制御、再初期化ロジック、統計管理などのユースケースを実装します。
//!
//! ## モジュール構成
//! - `pipeline`: 3スレッドパイプライン制御（Capture/Process/Communication）
//! - `recovery`: DDA再初期化ロジック（指数バックオフ）
//! - `stats`: 統計情報管理（FPS、レイテンシ、再初期化回数）
//! - `runtime_state`: ランタイム状態管理（Insertキー切り替え、マウスボタン状態）

pub mod pipeline;
pub mod recovery;
pub mod stats;
pub mod runtime_state;
pub mod input_detector;
