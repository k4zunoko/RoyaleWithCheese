# RoyaleWithCheese

**Windows環境で低レイテンシのリアルタイム画面解析とHID出力を実現するRustプロジェクト**

## クイックスタート

```powershell
# 設定ファイル作成
Copy-Item config.toml.example config.toml

# ビルド
cargo build --release

# テスト（単体テスト）
cargo test -- --test-threads=1

# Infrastructure層のキャプチャテスト（管理者権限必要）
cargo test dda -- --ignored --nocapture --test-threads=1

# Application層の統合テスト（排他的フルスクリーン環境）
cargo test test_exclusive_fullscreen_recovery -- --ignored --nocapture --test-threads=1

# 実行（開発中: 現在はキャプチャスレッドのみ動作）
cargo run --release
```

## 現在の実装状況

- ✅ **Domain層**: 型定義、Ports、エラーハンドリング、設定管理
- ✅ **Application層**: 4スレッドパイプライン、再初期化ロジック、統計情報管理
- ✅ **Infrastructure/Capture**: DDA実装（60-144Hz対応、GPU ROI実装）
- 🔄 **Infrastructure/Process**: モック実装（OpenCV統合は未実装）
- 🔄 **Infrastructure/Comm**: モック実装（HID統合は未実装）
- ✅ **Presentation/main.rs**: 初期化処理、設定読み込み、パイプライン起動

## パフォーマンスヒント

dda.rs 237行目のコメントアウトを外すと、VSync待機が有効になります。
```rust
// self.wait_for_vsync()?;
```

## ドキュメント

プロジェクトの設計方針についてはAGENT.mdとdocs/を参照し、必要であれば実際のコードも読む。また、技術的な情報はContext7とWebから集めてください。
このプロジェクトは開発途中で低レイテンシを重視しています。