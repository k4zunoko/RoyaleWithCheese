/// エラー型定義
/// 
/// Domain層のエラー型。thiserrorを使用して統一的なエラー処理を提供。

use thiserror::Error;

/// Domain層の統一エラー型
#[allow(dead_code)]
#[derive(Error, Debug)]
pub enum DomainError {
    /// キャプチャ関連のエラー
    #[error("Capture error: {0}")]
    Capture(String),

    /// 処理（画像処理）関連のエラー
    #[error("Process error: {0}")]
    Process(String),

    /// 通信（HID送信）関連のエラー
    #[error("Communication error: {0}")]
    Communication(String),

    /// 設定関連のエラー
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// タイムアウトエラー
    #[error("Operation timed out: {0}")]
    Timeout(String),

    /// デバイス一時不可（Recoverable）
    /// 
    /// ロック画面遷移やディスプレイモード変更など、
    /// すぐに復旧可能なエラー。
    #[error("Device temporarily unavailable")]
    DeviceNotAvailable,

    /// 再初期化必要（Non-recoverable）
    /// 
    /// インスタンス再作成が必要な致命的エラー。
    #[error("Reinitialization required")]
    ReInitializationRequired,

    /// 初期化エラー
    #[error("Initialization failed: {0}")]
    Initialization(String),

    /// リソース不足エラー
    #[error("Resource unavailable: {0}")]
    ResourceUnavailable(String),

    /// その他のエラー
    #[error("Unexpected error: {0}")]
    Other(String),
}

/// Domain層の統一Result型
pub type DomainResult<T> = Result<T, DomainError>;
