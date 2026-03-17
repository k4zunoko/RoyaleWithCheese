//! Windows システム音声再生（Infrastructure層）
//!
//! Win32 PlaySoundW API を使用してサウンドファイルを再生します。

use windows::core::w;
use windows::Win32::Foundation::FALSE;
use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME};

fn play_sound_file(path: windows::core::PCWSTR, context: &'static str) {
    // SAFETY: PlaySoundW は Win32 API。文字列リテラルは 'static かつ null-terminated。
    // SND_ASYNC で呼び出しスレッドをブロックせず、SND_FILENAME でファイルパスを直接再生する。
    let result = unsafe { PlaySoundW(path, None, SND_FILENAME | SND_ASYNC) };
    if result == FALSE {
        tracing::warn!(
            "{context}: PlaySoundW failed for hardcoded audio file path, continuing without audio"
        );
    }
}

/// トグルON時の音声を再生する。
///
/// `C:\Windows\Media\Speech On.wav` を非同期で再生する。
/// 再生に失敗した場合は warn ログを出力して続行する（パニックしない）。
pub fn play_toggle_on_sound() {
    play_sound_file(
        w!(r"C:\Windows\Media\Speech On.wav"),
        "play_toggle_on_sound",
    );
}

/// トグルOFF時の音声を再生する。
///
/// `C:\Windows\Media\Speech Off.wav` を非同期で再生する。
/// 再生に失敗した場合は warn ログを出力して続行する（パニックしない）。
pub fn play_toggle_off_sound() {
    play_sound_file(
        w!(r"C:\Windows\Media\Speech Off.wav"),
        "play_toggle_off_sound",
    );
}
