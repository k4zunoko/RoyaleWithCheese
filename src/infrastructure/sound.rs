//! Windows システム音声再生（Infrastructure層）
//!
//! Win32 PlaySoundW API を使用してシステム音声を再生します。

use windows::core::w;
use windows::Win32::Foundation::FALSE;
use windows::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC};

/// トグルON時の音声を再生する。
///
/// "Speech On" Windows システム音声を非同期で再生する。
/// 再生に失敗した場合は warn ログを出力して続行する（パニックしない）。
pub fn play_toggle_on_sound() {
    // SAFETY: PlaySoundW は Win32 API。文字列リテラルは 'static かつ null-terminated。
    // SND_ASYNC で呼び出しスレッドをブロックしない。
    let result = unsafe { PlaySoundW(w!("Speech On"), None, SND_ALIAS | SND_ASYNC) };
    if result == FALSE {
        tracing::warn!("play_toggle_on_sound: PlaySoundW failed, continuing without audio");
    }
}

/// トグルOFF時の音声を再生する。
///
/// "Speech Off" Windows システム音声を非同期で再生する。
/// 再生に失敗した場合は warn ログを出力して続行する（パニックしない）。
pub fn play_toggle_off_sound() {
    // SAFETY: PlaySoundW は Win32 API。文字列リテラルは 'static かつ null-terminated。
    // SND_ASYNC で呼び出しスレッドをブロックしない。
    let result = unsafe { PlaySoundW(w!("Speech Off"), None, SND_ALIAS | SND_ASYNC) };
    if result == FALSE {
        tracing::warn!("play_toggle_off_sound: PlaySoundW failed, continuing without audio");
    }
}
