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

# 実行（パフォーマンス測定ログ付き）
cargo run --features performance-timing

# 画像処理デバック
cargo run --features opencv-debug-display
```

### ビルドオプション

- **`--features performance-timing`**: 各処理の詳細なタイミングログを出力（パフォーマンス解析用）
- **`--features opencv-debug-display`**: OpenCVで画像処理の中間結果を表示（デバッグ用）