/// ログ・トレーシング基盤
/// 
/// tracingを使用した統一的なログ出力と区間計測。
/// 
/// # ビルドモードとパフォーマンス
/// - **Release ビルド**: ログ関連コードが完全にコンパイルアウトされ、ゼロランタイムオーバーヘッドを実現
/// - **Debug ビルド**: 非同期ログ（tracing-appender）でメインロジックへの影響を最小化
/// 
/// # 設計意図
/// 低レイテンシを最優先し、ログ出力がHot Pathのパフォーマンスに影響しないように実装しています。

#[cfg(debug_assertions)] 
use std::path::PathBuf;
#[cfg(debug_assertions)] 
use tracing::info;
#[cfg(debug_assertions)] 
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// ログシステムを初期化
/// 
/// # ビルドモード別の動作
/// - **Release ビルド**: この関数自体が空関数にコンパイル最適化され、ゼロオーバーヘッド
/// - **Debug ビルド**: tracing-appenderで非同期ファイル出力（メインスレッドはメモリコピーのみ）
/// 
/// # Arguments
/// - `log_level`: ログレベル（"info", "debug", "trace"等）
/// - `json_format`: JSON形式で出力するか
/// - `log_dir`: ログファイル出力先（None = 標準出力）
/// 
/// # Returns
/// - Debug: `Some(WorkerGuard)` - プログラム終了まで保持必須（Drop時にログスレッド終了）
/// - Release: `None` - オーバーヘッドなし
/// 
/// # 重要
/// Debugビルドでは戻り値の`WorkerGuard`をmain関数終了まで保持する必要があります。
#[cfg(debug_assertions)] 
pub fn init_logging(
    log_level: &str,
    json_format: bool,
    log_dir: Option<PathBuf>,
) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(log_level));

    match log_dir {
        Some(dir) => {
            // ファイル出力（非同期）
            std::fs::create_dir_all(&dir).expect("Failed to create log directory");
            
            let file_appender = tracing_appender::rolling::daily(dir, "royale_with_cheese.log");
            let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

            let subscriber = tracing_subscriber::registry().with(env_filter);

            let result = if json_format {
                subscriber
                    .with(fmt::layer().json().with_writer(non_blocking))
                    .try_init()
            } else {
                subscriber
                    .with(
                        fmt::layer()
                            .with_target(true)
                            .with_thread_ids(true)
                            .with_line_number(true)
                            .with_ansi(false) // ファイル出力時はANSIエスケープ無効
                            .with_writer(non_blocking),
                    )
                    .try_init()
            };

            if result.is_err() {
                return None;
            }

            info!("Logging initialized (async file): level={}, format={}", log_level, if json_format { "json" } else { "text" });
            Some(guard)
        }
        None => {
            // 標準出力（デバッグ用）
            let subscriber = tracing_subscriber::registry().with(env_filter);

            let result = if json_format {
                subscriber.with(fmt::layer().json()).try_init()
            } else {
                subscriber
                    .with(
                        fmt::layer()
                            .with_target(true)
                            .with_thread_ids(true)
                            .with_line_number(true),
                    )
                    .try_init()
            };

            if result.is_ok() {
                info!("Logging initialized (stdout): level={}, format={}", log_level, if json_format { "json" } else { "text" });
            }
            None
        }
    }
}

/// Release ビルド時のスタブ実装
#[cfg(not(debug_assertions))] 
pub fn init_logging(
    _log_level: &str,
    _json_format: bool,
    _log_dir: Option<std::path::PathBuf>,
) -> Option<()> {
    // Release ビルド時は何もしない（ランタイムオーバーヘッドなし）
    None
}

/// 区間計測用のマクロ
/// 
/// Release ビルド時は完全にコンパイルアウト（ゼロコスト）
/// Debug ビルド時のみ計測を実行
/// 
/// # 使用例
/// ```ignore
/// use royale_with_cheese::measure_span;
/// 
/// fn process_frame() {
///     measure_span!("process_frame", {
///         // 処理内容
///     });
/// }
/// ```
#[macro_export]
macro_rules! measure_span {
    ($name:expr, $body:expr) => {
        #[cfg(debug_assertions)] 
        {
            let _span = tracing::info_span!($name).entered();
            let _start = std::time::Instant::now();
            let result = $body;
            let _elapsed = _start.elapsed();
            tracing::debug!(
                span = $name,
                elapsed_us = _elapsed.as_micros(),
                "Span completed"
            );
            result
        }
        #[cfg(not(debug_assertions))] 
        {
            $body
        }
    };
}

/// 処理段階別の計測ポイント
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeasurePoint {
    /// キャプチャ
    Capture,
    /// 前処理（ROI抽出等）
    Preprocess,
    /// メイン処理（色検知/YOLO）
    Process,
    /// HID送信
    Communication,
    /// エンドツーエンド（キャプチャ→送信）
    EndToEnd,
}

impl MeasurePoint {
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Capture => "capture",
            Self::Preprocess => "preprocess",
            Self::Process => "process",
            Self::Communication => "communication",
            Self::EndToEnd => "end_to_end",
        }
    }
}

/// 計測結果の統計
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct MeasurementStats {
    pub name: String,
    pub count: u64,
    pub total_us: u64,
    pub min_us: u64,
    pub max_us: u64,
    pub avg_us: u64,
}

impl MeasurementStats {
    #[allow(dead_code)]
    pub fn new(name: String) -> Self {
        Self {
            name,
            count: 0,
            total_us: 0,
            min_us: u64::MAX,
            max_us: 0,
            avg_us: 0,
        }
    }

    #[allow(dead_code)]
    pub fn add_sample(&mut self, elapsed_us: u64) {
        self.count += 1;
        self.total_us += elapsed_us;
        self.min_us = self.min_us.min(elapsed_us);
        self.max_us = self.max_us.max(elapsed_us);
        self.avg_us = self.total_us / self.count;
    }

    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.count = 0;
        self.total_us = 0;
        self.min_us = u64::MAX;
        self.max_us = 0;
        self.avg_us = 0;
    }
}

/// 区間計測ヘルパー
/// 
/// Release ビルド時は `Instant::now()` でオーバーヘッドが生じる可能性があるため、
#[allow(dead_code)]
pub struct SpanTimer {
    name: &'static str,
    start: std::time::Instant,
}

impl SpanTimer {
    #[cfg(debug_assertions)] 
    #[allow(dead_code)]
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            start: std::time::Instant::now(),
        }
    }

    #[cfg(not(debug_assertions))] 
    #[allow(dead_code)]
    pub fn new(_name: &'static str) -> Self {
        // Release ビルド時は計測しない
        Self {
            name: "",
            start: std::time::Instant::now(), // 副作用をなくすためだけ
        }
    }

    #[allow(dead_code)]
    pub fn elapsed_us(&self) -> u64 {
        self.start.elapsed().as_micros() as u64
    }
}

#[cfg(debug_assertions)] 
impl Drop for SpanTimer {
    fn drop(&mut self) {
        let elapsed = self.elapsed_us();
        tracing::debug!(
            span = self.name,
            elapsed_us = elapsed,
            "Span completed"
        );
    }
}

#[cfg(not(debug_assertions))] 
impl Drop for SpanTimer {
    fn drop(&mut self) {
        // Release ビルド時は何もしない
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_measurement_stats() {
        let mut stats = MeasurementStats::new("test".to_string());
        
        stats.add_sample(100);
        stats.add_sample(200);
        stats.add_sample(300);

        assert_eq!(stats.count, 3);
        assert_eq!(stats.total_us, 600);
        assert_eq!(stats.min_us, 100);
        assert_eq!(stats.max_us, 300);
        assert_eq!(stats.avg_us, 200);

        stats.reset();
        assert_eq!(stats.count, 0);
    }

    #[test]
    fn test_span_timer() {
        let timer = SpanTimer::new("test_span");
        thread::sleep(Duration::from_millis(10));
        let elapsed = timer.elapsed_us();
        
        // 10ms = 10000us 以上経過しているはず
        assert!(elapsed >= 10000);
    }

    #[test]
    fn test_measure_point_as_str() {
        assert_eq!(MeasurePoint::Capture.as_str(), "capture");
        assert_eq!(MeasurePoint::Process.as_str(), "process");
        assert_eq!(MeasurePoint::EndToEnd.as_str(), "end_to_end");
    }

    #[test]
    fn test_init_logging_stdout() {
        // 標準出力モード（デバッグ用）
        let guard = init_logging("debug", false, None);
        assert!(guard.is_none());
        
        tracing::info!("Test log message");
        // ログが出力されることを確認（エラーにならないこと）
    }

    #[test]
    fn test_init_logging_file() {
        // ファイル出力モード
        let temp_dir = std::env::temp_dir().join("royale_test_logs");
        
        // グローバルsubscriberが既に設定されている場合はスキップ
        // （他のテストで設定済みの可能性がある）
        let guard = init_logging("info", false, Some(temp_dir.clone()));
        
        if guard.is_none() {
            // 既に設定済み - スキップ
            return;
        }
        
        assert!(temp_dir.exists());
        
        tracing::info!("Test file log");
        
        // guardをDropしてログをフラッシュ
        drop(guard);
        
        // ログファイルが作成されていることを確認
        let log_files: Vec<_> = std::fs::read_dir(&temp_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(!log_files.is_empty(), "Log file should be created");
        
        // クリーンアップ
        std::fs::remove_dir_all(temp_dir).ok();
    }
}
