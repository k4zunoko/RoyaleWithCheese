//! 再初期化ロジックモジュール
//!
//! DDAキャプチャの再初期化を指数バックオフで制御します。

use std::time::{Duration, Instant};

/// 再初期化戦略
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct RecoveryStrategy {
    /// 連続タイムアウト閾値（この回数を超えたら再初期化）
    pub consecutive_timeout_threshold: u32,
    /// 初期バックオフ時間
    pub initial_backoff: Duration,
    /// 最大バックオフ時間
    pub max_backoff: Duration,
    /// 累積失敗時間の上限（これを超えたら致命的エラー）
    pub max_cumulative_failure: Duration,
}

impl Default for RecoveryStrategy {
    fn default() -> Self {
        Self {
            consecutive_timeout_threshold: 120, // 約1秒（8ms * 120）
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(5),
            max_cumulative_failure: Duration::from_secs(60),
        }
    }
}

/// 再初期化状態管理
#[derive(Debug)]
#[allow(dead_code)]
pub struct RecoveryState {
    strategy: RecoveryStrategy,
    consecutive_timeouts: u32,
    current_backoff: Duration,
    cumulative_failure_start: Option<Instant>,
    total_reinitializations: u64,
}

#[allow(dead_code)]
impl RecoveryState {
    /// 新しいRecoveryStateを作成
    ///
    /// # Arguments
    /// * `strategy` - 再初期化戦略
    pub fn new(strategy: RecoveryStrategy) -> Self {
        Self {
            current_backoff: strategy.initial_backoff,
            strategy,
            consecutive_timeouts: 0,
            cumulative_failure_start: None,
            total_reinitializations: 0,
        }
    }

    /// デフォルト戦略でRecoveryStateを作成
    pub fn with_default_strategy() -> Self {
        Self::new(RecoveryStrategy::default())
    }

    /// タイムアウトを記録
    ///
    /// # Returns
    /// 再初期化が必要な場合は true
    pub fn record_timeout(&mut self) -> bool {
        self.consecutive_timeouts += 1;

        if self.consecutive_timeouts >= self.strategy.consecutive_timeout_threshold {
            self.consecutive_timeouts = 0;
            true
        } else {
            false
        }
    }

    /// 成功を記録（連続タイムアウトカウンターをリセット）
    pub fn record_success(&mut self) {
        self.consecutive_timeouts = 0;
        self.current_backoff = self.strategy.initial_backoff;
        self.cumulative_failure_start = None;
    }

    /// 再初期化試行を記録
    pub fn record_reinitialization_attempt(&mut self) {
        self.total_reinitializations += 1;

        // 指数バックオフ: 次回のバックオフ時間を2倍にする
        self.current_backoff = (self.current_backoff * 2).min(self.strategy.max_backoff);

        // 累積失敗時間の計測開始
        if self.cumulative_failure_start.is_none() {
            self.cumulative_failure_start = Some(Instant::now());
        }
    }

    /// 現在のバックオフ時間を取得
    pub fn current_backoff(&self) -> Duration {
        self.current_backoff
    }

    /// 累積失敗時間を取得
    ///
    /// # Returns
    /// 累積失敗時間。失敗していない場合は None
    pub fn cumulative_failure_duration(&self) -> Option<Duration> {
        self.cumulative_failure_start
            .map(|start| start.elapsed())
    }

    /// 累積失敗時間が上限を超えたか判定
    ///
    /// # Returns
    /// 上限を超えた場合は true
    pub fn is_cumulative_failure_exceeded(&self) -> bool {
        if let Some(duration) = self.cumulative_failure_duration() {
            duration >= self.strategy.max_cumulative_failure
        } else {
            false
        }
    }

    /// 総再初期化回数を取得
    pub fn total_reinitializations(&self) -> u64 {
        self.total_reinitializations
    }

    /// 連続タイムアウト回数を取得
    pub fn consecutive_timeouts(&self) -> u32 {
        self.consecutive_timeouts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timeout_threshold() {
        let mut state = RecoveryState::with_default_strategy();

        // 閾値未満
        for _ in 0..119 {
            assert!(!state.record_timeout());
        }

        // 閾値到達
        assert!(state.record_timeout());
        assert_eq!(state.consecutive_timeouts, 0);
    }

    #[test]
    fn test_success_resets_timeouts() {
        let mut state = RecoveryState::with_default_strategy();

        for _ in 0..50 {
            state.record_timeout();
        }

        assert_eq!(state.consecutive_timeouts, 50);

        state.record_success();

        assert_eq!(state.consecutive_timeouts, 0);
    }

    #[test]
    fn test_exponential_backoff() {
        let strategy = RecoveryStrategy {
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(5),
            ..Default::default()
        };

        let mut state = RecoveryState::new(strategy);

        assert_eq!(state.current_backoff(), Duration::from_millis(100));

        state.record_reinitialization_attempt();
        assert_eq!(state.current_backoff(), Duration::from_millis(200));

        state.record_reinitialization_attempt();
        assert_eq!(state.current_backoff(), Duration::from_millis(400));

        state.record_reinitialization_attempt();
        assert_eq!(state.current_backoff(), Duration::from_millis(800));

        state.record_reinitialization_attempt();
        assert_eq!(state.current_backoff(), Duration::from_millis(1600));

        state.record_reinitialization_attempt();
        assert_eq!(state.current_backoff(), Duration::from_millis(3200));

        // 最大値で固定
        state.record_reinitialization_attempt();
        assert_eq!(state.current_backoff(), Duration::from_secs(5));

        state.record_reinitialization_attempt();
        assert_eq!(state.current_backoff(), Duration::from_secs(5));
    }

    #[test]
    fn test_cumulative_failure_duration() {
        let mut state = RecoveryState::with_default_strategy();

        assert!(state.cumulative_failure_duration().is_none());

        state.record_reinitialization_attempt();
        std::thread::sleep(Duration::from_millis(100));

        let duration = state.cumulative_failure_duration().unwrap();
        assert!(duration >= Duration::from_millis(100));

        state.record_success();
        assert!(state.cumulative_failure_duration().is_none());
    }

    #[test]
    fn test_cumulative_failure_exceeded() {
        let strategy = RecoveryStrategy {
            max_cumulative_failure: Duration::from_millis(200),
            ..Default::default()
        };

        let mut state = RecoveryState::new(strategy);

        assert!(!state.is_cumulative_failure_exceeded());

        state.record_reinitialization_attempt();
        std::thread::sleep(Duration::from_millis(250));

        assert!(state.is_cumulative_failure_exceeded());
    }

    #[test]
    fn test_total_reinitializations() {
        let mut state = RecoveryState::with_default_strategy();

        assert_eq!(state.total_reinitializations(), 0);

        state.record_reinitialization_attempt();
        state.record_reinitialization_attempt();
        state.record_reinitialization_attempt();

        assert_eq!(state.total_reinitializations(), 3);
    }
}
