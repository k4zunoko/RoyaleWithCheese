//! ランタイム状態管理（Application層）
//!
//! Insertキーによる有効/無効切り替えやマウスボタン状態を管理します。
//! `Arc<AtomicBool>`を使用したロックフリー設計により、
//! 読み取り側スレッド（Capture/Process/HID）は数CPUサイクルで状態を確認できます。

use std::sync::{atomic::{AtomicBool, AtomicU64, Ordering}, Arc};
use std::time::Instant;

/// ランタイム状態（スレッド間で共有、ロックフリー）
/// 
/// Insertキーによる有効/無効切り替えやマウスボタン状態を管理します。
/// `Arc<AtomicBool>`を使用したロックフリー設計により、
/// 読み取り側スレッド（Capture/Process/HID）は数CPUサイクルで状態を確認できます。
/// 
/// # パフォーマンス特性
/// - 読み取り: `Ordering::Relaxed` - 数CPUサイクル、ロック不要
/// - 書き込み: stats_threadのみが実行（低頻度）
/// - メモリオーダー: Relaxed - 厳密な順序保証は不要（少し古い値でも無害）
#[derive(Clone)]
pub struct RuntimeState {
    /// プロジェクト全体の有効/無効（Insertキーで切り替え）
    enabled: Arc<AtomicBool>,
    /// マウス左ボタン押下状態
    mouse_left: Arc<AtomicBool>,
    /// マウス右ボタン押下状態
    mouse_right: Arc<AtomicBool>,
    /// 最後に無効状態ログを出力した時刻（ミリ秒単位のUnix時刻）
    last_disabled_log_ms: Arc<AtomicU64>,
    /// プログラム開始時刻（レートリミット計算用）
    start_time: Instant,
}

impl RuntimeState {
    /// 新しいRuntimeStateを作成（デフォルトで有効）
    pub fn new() -> Self {
        Self {
            enabled: Arc::new(AtomicBool::new(true)),
            mouse_left: Arc::new(AtomicBool::new(false)),
            mouse_right: Arc::new(AtomicBool::new(false)),
            last_disabled_log_ms: Arc::new(AtomicU64::new(0)),
            start_time: Instant::now(),
        }
    }
    
    // ===== 高速読み取り（Capture/Process/HIDスレッド用） =====
    
    /// システムが有効かどうかを確認（ロックフリー、超高速）
    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }
    
    /// マウス左ボタンが押下されているかを確認（ロックフリー、超高速）
    #[inline]
    #[allow(dead_code)] // 将来的に使用する可能性
    pub fn is_mouse_left_pressed(&self) -> bool {
        self.mouse_left.load(Ordering::Relaxed)
    }
    
    /// マウス右ボタンが押下されているかを確認（ロックフリー、超高速）
    #[inline]
    #[allow(dead_code)] // 将来的に使用する可能性
    pub fn is_mouse_right_pressed(&self) -> bool {
        self.mouse_right.load(Ordering::Relaxed)
    }
    
    // ===== 書き込み（stats_thread用） =====
    
    /// 有効/無効をトグル（新しい状態を返す）
    pub fn toggle_enabled(&self) -> bool {
        let current = self.enabled.load(Ordering::Relaxed);
        let new_value = !current;
        self.enabled.store(new_value, Ordering::Relaxed);
        new_value
    }
    
    /// マウスボタン状態を設定
    pub fn set_mouse_buttons(&self, left: bool, right: bool) {
        self.mouse_left.store(left, Ordering::Relaxed);
        self.mouse_right.store(right, Ordering::Relaxed);
    }
    
    /// 無効状態のログを出力すべきかを判定（5秒に1回のレートリミット）
    /// 
    /// HIDスレッドでシステム無効時のログレートリミットに使用します。
    /// ロックフリーのAtomic操作のみで実装されており、数CPUサイクルで完了します。
    /// 
    /// # Returns
    /// - `true`: ログを出力すべき（5秒以上経過または初回）
    /// - `false`: ログをスキップすべき（5秒未満）
    #[inline]
    pub fn should_log_disabled_status(&self) -> bool {
        let now_ms = self.start_time.elapsed().as_millis() as u64;
        let last_ms = self.last_disabled_log_ms.load(Ordering::Relaxed);
        
        // 初回または5秒以上経過した場合
        if last_ms == 0 || now_ms.saturating_sub(last_ms) >= 5000 {
            // Atomic CAS（Compare-And-Swap）で更新
            // 複数スレッドが同時に呼び出しても、1つのスレッドのみが成功する
            self.last_disabled_log_ms.compare_exchange(
                last_ms,
                now_ms,
                Ordering::Relaxed,
                Ordering::Relaxed
            ).is_ok()
        } else {
            false
        }
    }
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_state_toggle() {
        let state = RuntimeState::new();
        assert!(state.is_enabled());
        
        let new_state = state.toggle_enabled();
        assert!(!new_state);
        assert!(!state.is_enabled());
        
        let new_state = state.toggle_enabled();
        assert!(new_state);
        assert!(state.is_enabled());
    }

    #[test]
    fn test_runtime_state_mouse_buttons() {
        let state = RuntimeState::new();
        assert!(!state.is_mouse_left_pressed());
        assert!(!state.is_mouse_right_pressed());
        
        state.set_mouse_buttons(true, false);
        assert!(state.is_mouse_left_pressed());
        assert!(!state.is_mouse_right_pressed());
        
        state.set_mouse_buttons(false, true);
        assert!(!state.is_mouse_left_pressed());
        assert!(state.is_mouse_right_pressed());
    }
}
