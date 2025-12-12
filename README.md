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

# 実行（通常）
cargo run --release

# 実行（パフォーマンス測定ログ付き）
cargo run --release --features performance-timing
```

### ビルドオプション

- **`--features fast-color`** (デフォルト): OpenCVベースの色検出処理を使用
- **`--features performance-timing`**: 各処理の詳細なタイミングログを出力（パフォーマンス解析用）
- **`--features yolo-ort`**: YOLO + ONNX Runtimeベースの物体検出（未実装）

## 現在の実装状況

- ✅ **Domain層**: 型定義、Ports、エラーハンドリング、設定管理
- ✅ **Application層**: 4スレッドパイプライン、再初期化ロジック、統計情報管理
- ✅ **Infrastructure/Capture**: DDA実装（60-144Hz対応、GPU ROI実装）
- 🔄 **Infrastructure/Process**: モック実装（OpenCV統合は未実装）
- 🔄 **Infrastructure/Comm**: モック実装（HID統合は未実装）
- ✅ **Presentation/main.rs**: 初期化処理、設定読み込み、パイプライン起動
