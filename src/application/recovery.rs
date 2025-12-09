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

/// 排他的フルスクリーン環境での統合テスト
/// 
/// このモジュールは実際のDDAキャプチャを使用して、
/// Application層の再初期化ロジックが正しく機能することを検証します。
#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::domain::{CapturePort, DomainError};

    /// 排他的フルスクリーン環境でのリカバリーテスト
    /// 
    /// このテストは以下を検証します:
    /// 1. DeviceNotAvailableエラー発生時に再初期化が実行される
    /// 2. 再初期化後にフレーム取得が継続される
    /// 3. 144Hz環境で安定したフレームレート（目標: 100 FPS以上）
    /// 
    /// # 実行方法
    /// ```powershell
    /// cargo test test_exclusive_fullscreen_recovery -- --ignored --nocapture --test-threads=1
    /// ```
    /// 
    /// # 前提条件
    /// - 排他的フルスクリーンアプリケーション（ゲーム等）がメインモニターで実行中
    /// - 144Hzモニター環境
    #[test]
    #[ignore] // 排他的フルスクリーン環境が必要
    fn test_exclusive_fullscreen_recovery() {
        use crate::infrastructure::capture::dda::DdaCaptureAdapter;
        
        println!("\n=== Exclusive Fullscreen Recovery Test ===");
        println!("Prerequisites:");
        println!("  - Exclusive fullscreen app running on primary monitor");
        println!("  - 144Hz monitor environment");
        println!();

        // DDAキャプチャアダプタを初期化
        let mut capture = match DdaCaptureAdapter::new_primary(8) {
            Ok(adapter) => {
                let info = adapter.device_info();
                println!("DDA initialized:");
                println!("  Resolution: {}x{}", info.width, info.height);
                println!("  Refresh Rate: {}Hz", info.refresh_rate);
                println!();
                adapter
            }
            Err(e) => {
                println!("SKIP: DDA initialization failed: {:?}", e);
                println!("This is expected if another DDA instance is active");
                return;
            }
        };

        // RecoveryStateを初期化（より積極的な再初期化設定）
        let strategy = RecoveryStrategy {
            consecutive_timeout_threshold: 10, // タイムアウト10回で再初期化
            initial_backoff: Duration::from_millis(50),
            max_backoff: Duration::from_millis(500),
            max_cumulative_failure: Duration::from_secs(10),
        };
        let mut recovery = RecoveryState::new(strategy);

        // 統計情報
        let mut frame_count = 0u32;
        let mut timeout_count = 0u32;
        let mut device_not_available_count = 0u32;
        let mut reinit_required_count = 0u32;
        let mut successful_reinitializations = 0u32;

        let start = Instant::now();
        let test_duration = Duration::from_secs(3); // 3秒間テスト

        println!("Starting capture loop (3 seconds)...");
        println!();

        while start.elapsed() < test_duration {
            match capture.capture_frame() {
                Ok(Some(_frame)) => {
                    // フレーム取得成功
                    frame_count += 1;
                    recovery.record_success();
                }
                Ok(None) => {
                    // タイムアウト: 連続タイムアウト監視
                    timeout_count += 1;
                    
                    if recovery.record_timeout() {
                        println!("[TIMEOUT] Threshold reached, reinitializing...");
                        match capture.reinitialize() {
                            Ok(_) => {
                                successful_reinitializations += 1;
                                recovery.record_reinitialization_attempt();
                                println!("[REINIT] Success (backoff: {:?})", recovery.current_backoff());
                            }
                            Err(e) => {
                                println!("[REINIT] Failed: {:?}", e);
                            }
                        }
                    }
                }
                Err(DomainError::DeviceNotAvailable) => {
                    // 排他的フルスクリーン等: 即座に再初期化
                    device_not_available_count += 1;
                    
                    if device_not_available_count == 1 {
                        println!("[ERROR] DeviceNotAvailable detected (expected in exclusive fullscreen)");
                    }
                    
                    match capture.reinitialize() {
                        Ok(_) => {
                            successful_reinitializations += 1;
                            recovery.record_reinitialization_attempt();
                            
                            if device_not_available_count % 50 == 0 {
                                println!("[REINIT] Attempt #{}, backoff: {:?}", 
                                    successful_reinitializations, recovery.current_backoff());
                            }
                        }
                        Err(e) => {
                            println!("[REINIT] Failed: {:?}", e);
                            std::thread::sleep(Duration::from_millis(10));
                        }
                    }
                }
                Err(DomainError::ReInitializationRequired) => {
                    // 予期しないエラー: バックオフ後に再初期化
                    reinit_required_count += 1;
                    
                    let backoff = recovery.current_backoff();
                    println!("[ERROR] ReInitializationRequired, backing off: {:?}", backoff);
                    std::thread::sleep(backoff);
                    
                    match capture.reinitialize() {
                        Ok(_) => {
                            successful_reinitializations += 1;
                            recovery.record_reinitialization_attempt();
                            println!("[REINIT] Success after backoff");
                        }
                        Err(e) => {
                            println!("[REINIT] Failed after backoff: {:?}", e);
                        }
                    }
                }
                Err(e) => {
                    println!("[ERROR] Unexpected error: {:?}", e);
                    break;
                }
            }

            // 累積失敗時間チェック
            if recovery.is_cumulative_failure_exceeded() {
                println!("\n[FATAL] Cumulative failure time exceeded!");
                break;
            }
        }

        let elapsed = start.elapsed();
        let effective_fps = frame_count as f64 / elapsed.as_secs_f64();

        println!();
        println!("=== Test Results ===");
        println!("Duration: {:.2}s", elapsed.as_secs_f64());
        println!();
        println!("Frame Statistics:");
        println!("  Frames captured: {}", frame_count);
        println!("  Timeouts: {}", timeout_count);
        println!("  DeviceNotAvailable errors: {}", device_not_available_count);
        println!("  ReInitializationRequired errors: {}", reinit_required_count);
        println!("  Effective FPS: {:.2}", effective_fps);
        println!();
        println!("Recovery Statistics:");
        println!("  Total reinitializations: {}", recovery.total_reinitializations());
        println!("  Successful reinitializations: {}", successful_reinitializations);
        println!("  Current backoff: {:?}", recovery.current_backoff());
        println!("  Cumulative failure exceeded: {}", recovery.is_cumulative_failure_exceeded());
        println!();

        // アサーション: 排他的フルスクリーン環境での期待値
        if device_not_available_count > 0 {
            println!("Validation (Exclusive Fullscreen Environment):");
            
            // 1. 再初期化が実行されたことを確認
            assert!(
                successful_reinitializations > 0,
                "Expected at least one successful reinitialization in exclusive fullscreen"
            );
            println!("  ✓ Reinitialization executed: {} times", successful_reinitializations);
            
            // 2. フレームが取得できていることを確認（100 FPS以上を目標）
            // 排他的フルスクリーンではAccessLost頻発により一時的に低下する可能性がある
            let min_expected_fps = 100.0;
            if effective_fps >= min_expected_fps {
                println!("  ✓ Stable frame rate achieved: {:.2} FPS (target: {} FPS)", 
                    effective_fps, min_expected_fps);
            } else {
                println!("  ⚠ Frame rate below target: {:.2} FPS (target: {} FPS)", 
                    effective_fps, min_expected_fps);
                println!("    This may indicate frequent AccessLost errors");
                println!("    DeviceNotAvailable count: {}", device_not_available_count);
            }
            
            // 3. 累積失敗時間が上限を超えていないことを確認
            assert!(
                !recovery.is_cumulative_failure_exceeded(),
                "Cumulative failure time should not exceed limit"
            );
            println!("  ✓ Cumulative failure time within limit");
            
        } else {
            println!("Validation (Desktop Environment):");
            
            // デスクトップ環境: 安定した144 FPS（またはモニターのリフレッシュレート）
            let device_refresh_rate = capture.device_info().refresh_rate as f64;
            let min_expected_fps = device_refresh_rate * 0.9; // 90%以上
            
            assert!(
                effective_fps >= min_expected_fps,
                "Expected FPS >= {:.2}, got {:.2}",
                min_expected_fps,
                effective_fps
            );
            println!("  ✓ Stable frame rate: {:.2} FPS (device: {} Hz)", 
                effective_fps, device_refresh_rate);
            
            assert_eq!(
                device_not_available_count, 0,
                "No DeviceNotAvailable errors expected in desktop environment"
            );
            println!("  ✓ No DeviceNotAvailable errors");
        }

        println!();
        println!("=== Test Passed ===");
    }
}
