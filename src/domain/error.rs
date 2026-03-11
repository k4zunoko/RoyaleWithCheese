//! エラー型定義
//!
//! Domain層の統一エラー型。thiserrorを使用して型安全なエラー処理を提供します。
//!
//! # 設計方針
//! - unwrap()の使用を禁止し、明示的なエラーハンドリングを強制
//! - Result型でエラー伝播を明示化
//! - 回復可能性をエラー型で表現（DeviceNotAvailable vs ReInitializationRequired）

use thiserror::Error;

/// Domain層の統一エラー型
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
    #[allow(dead_code)] // 将来のエラーハンドリング用
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

    /// GPU device/adapter not available
    ///
    /// GPU が利用できない致命的エラー。
    /// Non-recoverable — GPU 初期化失敗はパイプライン停止を引き起こします。
    #[error("GPU not available: {0}")]
    GpuNotAvailable(String),

    /// GPU compute shader error
    ///
    /// GPU コンピュートシェーダ実行時のエラー。
    /// 致命的エラーとして扱われます。
    #[error("GPU compute error: {0}")]
    GpuCompute(String),

    /// GPU texture operation error
    ///
    /// GPU テクスチャ操作エラー。
    /// 致命的エラーとして扱われます。
    #[error("GPU texture error: {0}")]
    GpuTexture(String),
}

impl DomainError {
    /// エラーが回復可能かどうかを判定
    ///
    /// 回復可能なエラー：
    /// - DeviceNotAvailable: ロック画面やディスプレイモード変更は一時的
    ///
    /// それ以外は致命的エラー
    pub fn is_recoverable(&self) -> bool {
        matches!(self, DomainError::DeviceNotAvailable)
    }
}

/// Domain層の統一Result型
pub type DomainResult<T> = Result<T, DomainError>;

#[cfg(test)]
mod tests {
    use super::*;

    // ========== Test: Error Display Messages ==========

    #[test]
    fn test_capture_error_display() {
        let err = DomainError::Capture("test message".to_string());
        assert_eq!(err.to_string(), "Capture error: test message");
    }

    #[test]
    fn test_process_error_display() {
        let err = DomainError::Process("test message".to_string());
        assert_eq!(err.to_string(), "Process error: test message");
    }

    #[test]
    fn test_communication_error_display() {
        let err = DomainError::Communication("test message".to_string());
        assert_eq!(err.to_string(), "Communication error: test message");
    }

    #[test]
    fn test_configuration_error_display() {
        let err = DomainError::Configuration("test message".to_string());
        assert_eq!(err.to_string(), "Configuration error: test message");
    }

    #[test]
    fn test_timeout_error_display() {
        let err = DomainError::Timeout("test message".to_string());
        assert_eq!(err.to_string(), "Operation timed out: test message");
    }

    #[test]
    fn test_device_not_available_display() {
        let err = DomainError::DeviceNotAvailable;
        assert_eq!(err.to_string(), "Device temporarily unavailable");
    }

    #[test]
    fn test_reinitialization_required_display() {
        let err = DomainError::ReInitializationRequired;
        assert_eq!(err.to_string(), "Reinitialization required");
    }

    #[test]
    fn test_initialization_error_display() {
        let err = DomainError::Initialization("test message".to_string());
        assert_eq!(err.to_string(), "Initialization failed: test message");
    }

    #[test]
    fn test_gpu_not_available_display() {
        let err = DomainError::GpuNotAvailable("test message".to_string());
        assert_eq!(err.to_string(), "GPU not available: test message");
    }

    #[test]
    fn test_gpu_compute_error_display() {
        let err = DomainError::GpuCompute("test message".to_string());
        assert_eq!(err.to_string(), "GPU compute error: test message");
    }

    #[test]
    fn test_gpu_texture_error_display() {
        let err = DomainError::GpuTexture("test message".to_string());
        assert_eq!(err.to_string(), "GPU texture error: test message");
    }

    // ========== Test: DomainResult Type Alias ==========

    #[test]
    fn test_domain_result_ok() {
        let result: DomainResult<i32> = Ok(42);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_domain_result_err() {
        let result: DomainResult<i32> = Err(DomainError::Capture("test".to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn test_domain_result_string() {
        let result: DomainResult<String> = Ok("hello".to_string());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "hello");
    }

    // ========== Test: is_recoverable() ==========

    #[test]
    fn test_is_recoverable_device_not_available() {
        let err = DomainError::DeviceNotAvailable;
        assert!(err.is_recoverable());
    }

    #[test]
    fn test_is_recoverable_gpu_not_available() {
        let err = DomainError::GpuNotAvailable("test".to_string());
        assert!(!err.is_recoverable());
    }

    #[test]
    fn test_is_not_recoverable_capture() {
        let err = DomainError::Capture("test".to_string());
        assert!(!err.is_recoverable());
    }

    #[test]
    fn test_is_not_recoverable_process() {
        let err = DomainError::Process("test".to_string());
        assert!(!err.is_recoverable());
    }

    #[test]
    fn test_is_not_recoverable_communication() {
        let err = DomainError::Communication("test".to_string());
        assert!(!err.is_recoverable());
    }

    #[test]
    fn test_is_not_recoverable_configuration() {
        let err = DomainError::Configuration("test".to_string());
        assert!(!err.is_recoverable());
    }

    #[test]
    fn test_is_not_recoverable_timeout() {
        let err = DomainError::Timeout("test".to_string());
        assert!(!err.is_recoverable());
    }

    #[test]
    fn test_is_not_recoverable_reinitialization_required() {
        let err = DomainError::ReInitializationRequired;
        assert!(!err.is_recoverable());
    }

    #[test]
    fn test_is_not_recoverable_initialization() {
        let err = DomainError::Initialization("test".to_string());
        assert!(!err.is_recoverable());
    }

    #[test]
    fn test_is_not_recoverable_gpu_compute() {
        let err = DomainError::GpuCompute("test".to_string());
        assert!(!err.is_recoverable());
    }

    #[test]
    fn test_is_not_recoverable_gpu_texture() {
        let err = DomainError::GpuTexture("test".to_string());
        assert!(!err.is_recoverable());
    }
}
