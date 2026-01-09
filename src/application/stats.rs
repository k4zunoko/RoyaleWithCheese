//! 統計情報管理モジュール
//!
//! FPS、各処理段階のレイテンシ、再初期化回数などの統計を収集・出力します。

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// 統計情報の種別
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StatKind {
    /// キャプチャ処理時間
    #[allow(dead_code)]  // 将来の計測用
    Capture,
    /// 前処理時間
    #[allow(dead_code)]  // 将来の計測用
    Preprocess,
    /// 画像処理時間（色検知/YOLO推論）
    Process,
    /// HID通信時間
    Communication,
    /// エンドツーエンドのレイテンシ
    EndToEnd,
}

/// パーセンタイル統計値
#[derive(Debug, Clone)]
pub struct PercentileStats {
    pub p50: Duration,
    pub p95: Duration,
    pub p99: Duration,
    pub count: usize,
}

/// 統計情報コレクター
#[derive(Debug)]
pub struct StatsCollector {
    /// FPS計測用のフレームタイムスタンプ（最大1秒分保持）
    frame_times: VecDeque<Instant>,
    /// 各処理段階の所要時間（最大1000サンプル保持）
    durations: std::collections::HashMap<StatKind, VecDeque<Duration>>,
    /// 再初期化回数
    reinit_count: u64,
    /// 累積失敗時間
    cumulative_failure_duration: Duration,
    /// 最後の統計出力時刻
    last_report: Instant,
    /// 統計出力間隔
    report_interval: Duration,
}

impl StatsCollector {
    /// 新しいStatsCollectorを作成
    ///
    /// # Arguments
    /// * `report_interval` - 統計出力間隔（例: 10秒）
    pub fn new(report_interval: Duration) -> Self {
        Self {
            frame_times: VecDeque::new(),
            durations: std::collections::HashMap::new(),
            reinit_count: 0,
            cumulative_failure_duration: Duration::ZERO,
            last_report: Instant::now(),
            report_interval,
        }
    }

    /// FPS計算の時間範囲（1秒間のフレーム数を計測）
    const FPS_WINDOW_SECS: u64 = 1;
    
    /// フレーム受信を記録（FPS計測用）
    pub fn record_frame(&mut self) {
        let now = Instant::now();
        self.frame_times.push_back(now);

        // 指定秒数より古いタイムスタンプを削除
        let window = Duration::from_secs(Self::FPS_WINDOW_SECS);
        while let Some(&front) = self.frame_times.front() {
            if now.duration_since(front) > window {
                self.frame_times.pop_front();
            } else {
                break;
            }
        }
    }

    /// 最大サンプル保持数（パーセンタイル計算用）
    const MAX_DURATION_SAMPLES: usize = 1000;
    
    /// 処理時間を記録
    ///
    /// # Arguments
    /// * `kind` - 統計種別
    /// * `duration` - 処理時間
    pub fn record_duration(&mut self, kind: StatKind, duration: Duration) {
        let queue = self.durations.entry(kind).or_default();
        queue.push_back(duration);

        // 最大サンプル数を超えたら古いデータを破棄
        if queue.len() > Self::MAX_DURATION_SAMPLES {
            queue.pop_front();
        }
    }

    /// 再初期化をカウント
    #[allow(dead_code)]  // 将来の統計レポート用
    pub fn record_reinitialization(&mut self) {
        self.reinit_count += 1;
    }

    /// 累積失敗時間を追加
    #[allow(dead_code)]  // 将来の統計レポート用
    pub fn add_failure_duration(&mut self, duration: Duration) {
        self.cumulative_failure_duration += duration;
    }

    /// 現在のFPSを計算
    pub fn current_fps(&self) -> f64 {
        if self.frame_times.is_empty() {
            return 0.0;
        }

        // フレーム数 / 経過時間
        let count = self.frame_times.len() as f64;
        if let (Some(&first), Some(&last)) = (self.frame_times.front(), self.frame_times.back()) {
            let elapsed = last.duration_since(first).as_secs_f64();
            if elapsed > 0.0 {
                return count / elapsed;
            }
        }
        0.0
    }

    /// パーセンタイル統計を計算
    ///
    /// # Arguments
    /// * `kind` - 統計種別
    ///
    /// # Returns
    /// パーセンタイル統計値。データがない場合は None
    pub fn percentile_stats(&self, kind: StatKind) -> Option<PercentileStats> {
        let queue = self.durations.get(&kind)?;
        if queue.is_empty() {
            return None;
        }

        let mut sorted: Vec<Duration> = queue.iter().copied().collect();
        sorted.sort();

        let count = sorted.len();
        let p50 = sorted[count * 50 / 100];
        let p95 = sorted[count * 95 / 100];
        let p99 = sorted[count * 99 / 100];

        Some(PercentileStats {
            p50,
            p95,
            p99,
            count,
        })
    }

    /// 統計レポートを出力すべきか判定
    ///
    /// # Returns
    /// 出力すべき場合は true
    pub fn should_report(&self) -> bool {
        self.last_report.elapsed() >= self.report_interval
    }

    /// 統計レポートを出力してタイマーをリセット
    #[cfg(debug_assertions)]
    pub fn report_and_reset(&mut self) {
        use tracing::info;

        info!("=== Pipeline Statistics ===");
        info!("FPS: {:.1}", self.current_fps());

        for kind in [
            StatKind::Capture,
            StatKind::Preprocess,
            StatKind::Process,
            StatKind::Communication,
            StatKind::EndToEnd,
        ] {
            if let Some(stats) = self.percentile_stats(kind) {
                info!(
                    "{:?}: p50={:.2}ms, p95={:.2}ms, p99={:.2}ms (n={})",
                    kind,
                    stats.p50.as_secs_f64() * 1000.0,
                    stats.p95.as_secs_f64() * 1000.0,
                    stats.p99.as_secs_f64() * 1000.0,
                    stats.count
                );
            }
        }

        info!("Reinitialization count: {}", self.reinit_count);
        info!(
            "Cumulative failure duration: {:.2}s",
            self.cumulative_failure_duration.as_secs_f64()
        );
        info!("===========================");

        self.last_report = Instant::now();
    }

    /// Release build用のダミー実装
    #[cfg(not(debug_assertions))]
    pub fn report_and_reset(&mut self) {
        self.last_report = Instant::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fps_calculation() {
        let mut stats = StatsCollector::new(Duration::from_secs(10));

        // 0.5秒間隔で4フレーム記録（期待FPS: ~6-8）
        for _ in 0..4 {
            stats.record_frame();
            std::thread::sleep(Duration::from_millis(100));
        }

        let fps = stats.current_fps();
        assert!(fps > 5.0 && fps < 15.0, "FPS should be around 10, got {}", fps);
    }

    #[test]
    fn test_percentile_stats() {
        let mut stats = StatsCollector::new(Duration::from_secs(10));

        // 100サンプルの処理時間を記録
        for i in 0..100 {
            stats.record_duration(StatKind::Process, Duration::from_millis(i));
        }

        let percentile = stats.percentile_stats(StatKind::Process).unwrap();
        assert_eq!(percentile.count, 100);
        assert!(percentile.p50.as_millis() >= 45 && percentile.p50.as_millis() <= 55);
        assert!(percentile.p95.as_millis() >= 90 && percentile.p95.as_millis() <= 99);
        assert_eq!(percentile.p99.as_millis(), 99);
    }

    #[test]
    fn test_reinitialization_count() {
        let mut stats = StatsCollector::new(Duration::from_secs(10));

        stats.record_reinitialization();
        stats.record_reinitialization();
        stats.record_reinitialization();

        assert_eq!(stats.reinit_count, 3);
    }

    #[test]
    fn test_cumulative_failure_duration() {
        let mut stats = StatsCollector::new(Duration::from_secs(10));

        stats.add_failure_duration(Duration::from_secs(5));
        stats.add_failure_duration(Duration::from_secs(3));

        assert_eq!(stats.cumulative_failure_duration, Duration::from_secs(8));
    }

    #[test]
    fn test_should_report() {
        let stats = StatsCollector::new(Duration::from_millis(100));

        assert!(!stats.should_report());

        std::thread::sleep(Duration::from_millis(150));

        assert!(stats.should_report());
    }
}
