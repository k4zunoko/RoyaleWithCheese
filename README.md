# RoyaleWithCheese

**Windows環境で低レイテンシのリアルタイム画面解析とHID出力を実現するRustプロジェクト**

## クイックスタート

```powershell
# 設定ファイル作成
Copy-Item config.toml.example config.toml

# 通常のビルド（パフォーマンス測定ログなし）
cargo build --release

# パフォーマンス測定ログ付きビルド
cargo build --features performance-timing

# テスト（単体テスト）
cargo test -- --test-threads=1

# Infrastructure層のキャプチャテスト（管理者権限必要）
cargo test dda -- --ignored --nocapture --test-threads=1

# Application層の統合テスト（排他的フルスクリーン環境）
cargo test test_exclusive_fullscreen_recovery -- --ignored --nocapture --test-threads=1

# HIDデバイス列挙テスト
cargo test test_enumerate_hid_devices -- --nocapture

# HID通信確認テスト（実デバイス必須、要デバイスパス設定）
cargo test test_hid_communication -- --ignored --nocapture

# 実行（通常）
cargo run --release

# 実行（パフォーマンス測定ログ付き）
cargo run --features performance-timing

# 画像処理デバック
cargo run --features opencv-debug-display
```

### ビルドオプション

- **`--features fast-color`** (デフォルト): OpenCVベースの色検出処理を使用
- **`--features performance-timing`**: 各処理の詳細なタイミングログを出力（パフォーマンス解析用）
- **`--features opencv-debug-display`**: OpenCVで画像処理の中間結果を表示（デバッグ用）
- **`--features yolo-ort`**: YOLO + ONNX Runtimeベースの物体検出（未実装）

### 動作テスト機能

**opencv-debug-display**: config.tomlの設定が正しく適用されているかを視覚的に確認

```powershell
# デバッグ表示を有効化して実行
cargo run --features opencv-debug-display
```

このFeatureを有効にすると、以下の3つのウィンドウが表示されます：

1. **Debug: BGR Capture**: キャプチャされた元画像（等倍表示）
   - 検出時は重心位置に緑色の十字マークと円を表示

2. **Debug: Mask**: HSV範囲でフィルタリングされたマスク画像（等倍表示、白=検出、黒=非検出）

3. **Debug: Info**: デバッグ情報専用ウィンドウ（固定サイズ）
   - ROIサイズ、HSV範囲設定値、検出状態、検出面積、重心座標を表示

**操作**: ESCキーまたは'q'キーで終了（約30fps表示）

**確認項目**:
- ROI設定が期待通りか
- HSV範囲が適切に設定されているか
- 検出したい色が正しく検出されているか
- 重心が期待した位置にあるか

**注意**: 画像表示により処理速度が大幅に低下するため、デバッグ・動作確認目的でのみ使用してください。

