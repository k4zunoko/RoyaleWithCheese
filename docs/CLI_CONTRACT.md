# CLI / 実行時契約（CLI Contract）

## 概要

RoyaleWithCheese は引数を取らない実行バイナリで、カレントディレクトリの `config.toml` を読み込んで起動します。

- エントリポイント: `src/main.rs`
- 設定: `config.toml`（例は `config.toml.example`）

## 起動シーケンス

1. ログ初期化（デフォルトは `logs/` への非同期ファイル出力）
2. `config.toml` の読み込み
   - 成功: 読み込んだ設定を使用
   - 失敗: **デフォルト設定**で起動を継続
3. 設定の妥当性検証（不正なら終了）
4. DDAキャプチャ初期化 → 画面中心ROIの確定
5. `process.mode` に応じて処理アダプタを選択
   - `fast-color`: 実装済み
   - `yolo-ort`: 現状未実装（エラーで終了）
6. パイプライン（4スレッド）起動

## 設定ファイル契約

- ファイル名は固定: `config.toml`
- スキーマの実例: `config.toml.example`
- **詳細な設定項目説明**: [../CONFIGURATION.md](../CONFIGURATION.md) を参照
- 主な契約ポイント:
  - ROI は `width/height` のみ設定し、実行時に画面中心へ自動配置
  - `communication.vendor_id/product_id` が `0x0000` の場合、モック通信を選べる

## 終了コード

- `0`: 正常終了（通常は長時間常駐を想定）
- `1`: 致命的エラー（例: 設定検証失敗、未実装モード指定、初期化失敗）

## Cargo features

- `performance-timing`: パフォーマンス計測ログを有効化
- `opencv-debug-display`: OpenCVの視覚デバッグ表示を有効化

## 実行例

```powershell
# 設定ファイルを作成
Copy-Item config.toml.example config.toml

# 通常実行
cargo run --release

# 視覚デバッグ（パフォーマンスは低下）
cargo run --features opencv-debug-display
```

## 注意事項

- DDA の実機テストは管理者権限や環境条件が必要な場合があります（テストは `#[ignore]` で隔離）
- ログは `logs/` に出力されます（ディレクトリ作成権限が必要）
