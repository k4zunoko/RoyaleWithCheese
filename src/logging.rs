/// ログ・トレーシング基盤
/// 
/// tracingを使用した統一的なログ出力と区間計測。

use tracing::{info, Level};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// ログシステムを初期化
/// 
/// # Arguments
/// - `log_level`: ログレベル（例: "info", "debug", "trace"）
/// - `json_format`: JSON形式で出力するか
pub fn init_logging(log_level: &str, json_format: bool) {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(log_level));

    let subscriber = tracing_subscriber::registry().with(env_filter);

    if json_format {
        subscriber
            .with(fmt::layer().json())
            .init();
    } else {
        subscriber
            .with(
                fmt::layer()
                    .with_target(true)
                    .with_thread_ids(true)
                    .with_line_number(true)
            )
            .init();
    }

    info!("Logging initialized at level: {}", log_level);
}

/// 区間計測用のマクロ
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
    ($name:expr, $body:expr) => {{
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
    }};
}

/// 処理段階別の計測ポイント
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

    pub fn add_sample(&mut self, elapsed_us: u64) {
        self.count += 1;
        self.total_us += elapsed_us;
        self.min_us = self.min_us.min(elapsed_us);
        self.max_us = self.max_us.max(elapsed_us);
        self.avg_us = self.total_us / self.count;
    }

    pub fn reset(&mut self) {
        self.count = 0;
        self.total_us = 0;
        self.min_us = u64::MAX;
        self.max_us = 0;
        self.avg_us = 0;
    }
}

/// 区間計測ヘルパー
pub struct SpanTimer {
    name: &'static str,
    start: std::time::Instant,
}

impl SpanTimer {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            start: std::time::Instant::now(),
        }
    }

    pub fn elapsed_us(&self) -> u64 {
        self.start.elapsed().as_micros() as u64
    }
}

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
}
