//! 音声フィードバック実装（Infrastructure層）
//!
//! Windows PlaySoundW APIを使用して、システム有効/無効切り替え時に音声を再生します。
//! 低レイテンシ設計: SND_ASYNCフラグにより非同期再生、呼び出し元はブロックされません。

use crate::domain::config::AudioFeedbackConfig;

/// Windows音声フィードバック実装
/// 
/// PlaySoundW APIを使用してシステム音を非同期再生します。
/// 
/// # 低レイテンシ設計
/// - **非同期再生**: SND_ASYNCフラグにより、音声再生は別スレッドで実行
/// - **超高速**: PlaySoundW呼び出しは数マイクロ秒で完了（ファイルI/Oは別スレッド）
/// - **低頻度イベント**: トグル時のみ実行（秒単位のイベント）
/// - **Stats/UIスレッドで実行**: Capture/Process/HIDスレッドには影響なし
pub struct WindowsAudioFeedback {
    config: AudioFeedbackConfig,
}

impl WindowsAudioFeedback {
    /// 新しいWindowsAudioFeedbackを作成
    pub fn new(config: AudioFeedbackConfig) -> Self {
        Self { config }
    }

    /// トグル時の音声を再生
    /// 
    /// # Arguments
    /// * `enabled` - トグル後の状態（true=有効化、false=無効化）
    /// 
    /// # パフォーマンス
    /// - SND_ASYNCフラグにより非同期再生（数マイクロ秒で復帰）
    /// - 音声ファイルが見つからない場合でもブロックせず即座に復帰
    /// - エラーはログに記録のみ（致命的でない）
    pub fn play_toggle_sound(&self, enabled: bool) {
        if !self.config.enabled {
            return;
        }

        let path = if enabled {
            &self.config.on_sound
        } else {
            &self.config.off_sound
        };

        // Windows APIを使用して音声再生
        #[cfg(target_os = "windows")]
        {
            use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_NODEFAULT};
            use windows::core::PCWSTR;

            // UTF-16に変換（null終端を含む）
            let wide_path: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
            
            // フラグ設定
            // - SND_FILENAME: ファイルパスとして解釈
            // - SND_ASYNC: 非同期再生（即座に復帰）
            // - SND_NODEFAULT: ファイルが見つからない場合、デフォルトシステムサウンドを再生しない
            let mut flags = SND_FILENAME | SND_ASYNC;
            if self.config.fallback_to_silent {
                flags |= SND_NODEFAULT;
            }
            
            unsafe {
                let result = PlaySoundW(PCWSTR(wide_path.as_ptr()), None, flags);
                if !result.as_bool() {
                    // 音声再生失敗は致命的ではないため、ログのみ
                    #[cfg(debug_assertions)]
                    tracing::warn!("Failed to play sound '{}'", path);
                }
            }
        }

        // Windows以外のプラットフォームでは何もしない
        #[cfg(not(target_os = "windows"))]
        {
            #[cfg(debug_assertions)]
            tracing::debug!("Audio feedback not supported on this platform");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_feedback_creation() {
        let config = AudioFeedbackConfig::default();
        let feedback = WindowsAudioFeedback::new(config);
        assert!(feedback.config.enabled);
    }

    #[test]
    fn test_audio_feedback_disabled() {
        let mut config = AudioFeedbackConfig::default();
        config.enabled = false;
        let feedback = WindowsAudioFeedback::new(config);
        
        // 無効時は何も実行されない（パニックしないことを確認）
        feedback.play_toggle_sound(true);
        feedback.play_toggle_sound(false);
    }

    #[test]
    #[ignore] // 実機でのみ実行（音声が実際に再生される）
    fn test_play_toggle_sound() {
        use std::thread;
        use std::time::Duration;

        let config = AudioFeedbackConfig::default();
        let feedback = WindowsAudioFeedback::new(config);
        
        println!("Playing 'enabled' sound...");
        feedback.play_toggle_sound(true);
        thread::sleep(Duration::from_millis(1500));
        
        println!("Playing 'disabled' sound...");
        feedback.play_toggle_sound(false);
        thread::sleep(Duration::from_millis(1500));
    }

    #[test]
    fn test_invalid_sound_path() {
        let mut config = AudioFeedbackConfig::default();
        config.on_sound = "C:\\NonExistent\\Sound.wav".to_string();
        config.fallback_to_silent = true;
        let feedback = WindowsAudioFeedback::new(config);
        
        // fallback_to_silent=true なので、エラーでもパニックしない
        feedback.play_toggle_sound(true);
    }

    #[test]
    fn test_custom_sound_paths() {
        let config = AudioFeedbackConfig {
            enabled: true,
            on_sound: "C:\\Windows\\Media\\tada.wav".to_string(),
            off_sound: "C:\\Windows\\Media\\chimes.wav".to_string(),
            fallback_to_silent: true,
        };
        let feedback = WindowsAudioFeedback::new(config);
        
        // カスタム音声パスでもパニックしない
        feedback.play_toggle_sound(true);
        feedback.play_toggle_sound(false);
    }
}
