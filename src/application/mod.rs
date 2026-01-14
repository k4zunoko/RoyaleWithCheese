//! Application Layer
//!
//! パイプライン制御、再初期化ロジック、統計管理などのユースケースを実装します。
//!
//! ## モジュール構成
//! - `pipeline`: パイプライン設定とエントリーポイント
//! - `threads`: 4スレッド実装（Capture/Process/HID/Stats）
//! - `recovery`: DDA再初期化ロジック（指数バックオフ）
//! - `stats`: 統計情報管理（FPS、レイテンシ、再初期化回数）
//! - `runtime_state`: ランタイム状態管理（Insertキー切り替え、マウスボタン状態）
//! - `input_detector`: キー押下検知（エッジ検出）

pub mod input_detector;
pub mod pipeline;
pub mod recovery;
pub mod runtime_state;
pub mod stats;
pub mod threads;
