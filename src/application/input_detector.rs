//! 入力検出ユーティリティ（Application層）
//!
//! キー押下のエッジ検出（立ち上がり/立ち下がり）を提供します。
//!
//! # 使用例
//! Insertキーのトグル検出（押し続けではなく、押した瞬間のみ検出）。

use crate::domain::ports::{InputPort, VirtualKey};

/// キーの押下状態を検知（エッジ検出用）
///
/// 前回の状態と比較して、キーが押された瞬間（立ち上がりエッジ）を検知します。
pub struct KeyPressDetector {
    previous_state: bool,
}

impl KeyPressDetector {
    /// 新しいKeyPressDetectorを作成
    pub fn new() -> Self {
        Self {
            previous_state: false,
        }
    }

    /// キーが押された瞬間かをチェック（立ち上がりエッジ検出）
    ///
    /// # Arguments
    /// - `input`: InputPort trait実装（抽象化されたキーボード入力）
    /// - `key`: チェックするキー
    ///
    /// # Returns
    /// - `true`: 前回チェック時は押されておらず、今回押されている（立ち上がりエッジ）
    /// - `false`: それ以外（押され続けている、離されている、押されていない）
    pub fn is_key_just_pressed(&mut self, input: &dyn InputPort, key: VirtualKey) -> bool {
        let current_state = input.is_key_pressed(key);
        let edge = !self.previous_state && current_state;
        self.previous_state = current_state;
        edge
    }

    /// 現在の状態をリセット
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.previous_state = false;
    }
}

impl Default for KeyPressDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ports::InputState;

    struct MockInput {
        pressed: bool,
    }

    impl InputPort for MockInput {
        fn is_key_pressed(&self, _key: VirtualKey) -> bool {
            self.pressed
        }

        fn poll_input_state(&self) -> InputState {
            InputState {
                mouse_left: false,
                mouse_right: false,
            }
        }
    }

    #[test]
    fn test_edge_detection() {
        let mut detector = KeyPressDetector::new();

        // 初期状態: 押されていない
        let input = MockInput { pressed: false };
        assert!(!detector.is_key_just_pressed(&input, VirtualKey::Insert));

        // 押された瞬間: エッジ検出
        let input = MockInput { pressed: true };
        assert!(detector.is_key_just_pressed(&input, VirtualKey::Insert));

        // 押され続けている: エッジなし
        let input = MockInput { pressed: true };
        assert!(!detector.is_key_just_pressed(&input, VirtualKey::Insert));

        // 離された
        let input = MockInput { pressed: false };
        assert!(!detector.is_key_just_pressed(&input, VirtualKey::Insert));

        // 再度押された: エッジ検出
        let input = MockInput { pressed: true };
        assert!(detector.is_key_just_pressed(&input, VirtualKey::Insert));
    }

    #[test]
    fn test_reset() {
        let mut detector = KeyPressDetector::new();

        // 押された瞬間
        let input = MockInput { pressed: true };
        assert!(detector.is_key_just_pressed(&input, VirtualKey::Insert));

        // リセット
        detector.reset();

        // 再度押された瞬間として検出される
        let input = MockInput { pressed: true };
        assert!(detector.is_key_just_pressed(&input, VirtualKey::Insert));
    }
}
