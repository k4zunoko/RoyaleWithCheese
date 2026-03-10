# RoyaleWithCheese

**Windows 向け低レイテンシ画面キャプチャ → HSV 処理 → HID 出力パイプライン（Rust 実装）**

---

## プロジェクト概要

RoyaleWithCheese は、Windows 環境でリアルタイム画面解析と USB HID デバイスへの出力を低レイテンシで実現する Rust 製アプリケーションです。

- **画面キャプチャ**: DDA または WGC バックエンドで画面の指定領域（ROI）を取得
- **HSV 色検知**: OpenCV を用いた高速 HSV カラーマスク処理でターゲットを検出
- **HID 出力**: 検出結果を USB HID レポートとしてデバイスへ送出
- **マルチスレッドパイプライン**: Capture → Process → HID → Stats の 4 スレッド構成

---

## アーキテクチャ概要

```
src/
├── main.rs              # エントリーポイント・DI 構成ルート
├── lib.rs               # モジュールエクスポート
├── logging.rs           # ロギング初期化
├── domain/              # ドメイン層（ビジネスロジック・型・設定）
│   ├── config.rs        # AppConfig TOML ロード・バリデーション
│   ├── types.rs         # Roi, Frame, DetectionResult など
│   ├── ports.rs         # CapturePort / ProcessPort / CommPort / InputPort
│   └── error.rs         # DomainError
├── application/         # アプリケーション層（パイプライン制御）
│   ├── pipeline.rs      # PipelineRunner（スレッド起動・結合）
│   ├── threads.rs       # capture / process / hid / stats スレッド実装
│   ├── recovery.rs      # 指数バックオフ再起動ロジック
│   ├── runtime_state.rs # アトミックフラグ（アクティブ・マウス状態）
│   └── metrics.rs       # lock-free AtomicU64 パフォーマンス指標
└── infrastructure/      # インフラ層（外部システムアダプター）
    ├── capture/         # dda.rs / wgc.rs / common.rs
    ├── processing/      # cpu/ gpu/ selector.rs
    ├── hid_comm.rs      # USB HID 送信アダプター
    └── input.rs         # Windows 入力状態読み取りアダプター
```

**レイヤー依存の方向**: `infrastructure` → `application` → `domain`（逆方向なし）

---

## キャプチャバックエンド

`config.toml` の `[capture]` セクションで `source` を指定します。

| 値 | 名称 | 説明 |
|---|---|---|
| `"dda"` | Desktop Duplication API | 画面全体を汎用的に取得（デフォルト） |
| `"wgc"` | Windows Graphics Capture | 低レイテンシモード（Windows 10 1803+、推奨） |

```toml
[capture]
source = "wgc"   # "dda" または "wgc"
```

---

## 処理モードと GPU パス

`config.toml` の `[process]` セクションと `[gpu]` セクションで制御します。

### 処理モード選択ロジック（`src/main.rs`）

| 条件 | 動作 |
|---|---|
| `process.mode = "fast-color-gpu"` | GPU アダプターを直接初期化。GPU が利用不可の場合はエラーで終了 |
| `process.mode = "fast-color"` + `gpu.enabled = true` | GPU を優先的に使用。初期化に失敗した場合は CPU へ自動フォールバック（警告ログ出力） |
| `process.mode = "fast-color"` + `gpu.enabled = false`（デフォルト） | CPU（OpenCV）処理のみ使用 |

```toml
[process]
mode = "fast-color"   # 通常はこれで十分

[gpu]
enabled = false       # true にすると GPU 優先 + CPU フォールバック
```

---

## クイックスタート

```powershell
# 1. 設定ファイルを作成（必須：HID デバイスの VID/PID を書き換えること）
Copy-Item config.toml.example config.toml

# 2. config.toml を編集し vendor_id / product_id を実デバイス値に設定
# [communication]
# vendor_id = 0xXXXX
# product_id = 0xYYYY

# 3. リリースビルド
cargo build --release

# 4. 実行
./target/release/RoyaleWithCheese.exe
```

> **注意**: `config.toml` が存在しない、または TOML のパースに失敗した場合、アプリケーションは **警告ログを出力してデフォルト設定で起動** します（終了しません）。デフォルト設定では `vendor_id = 0x1234` / `product_id = 0x5678` が使用されますが、これはプレースホルダーです。実際のデバイスに合わせて必ず書き換えてください。

---

## ビルドオプション

```powershell
# パフォーマンス計測ログ付きビルド
cargo build --features performance-timing

# OpenCV デバッグ表示付きビルド（処理中間結果をウィンドウ表示）
cargo build --features opencv-debug-display

# パフォーマンス計測付きで即時実行
cargo run --features performance-timing
```

---

## テストコマンド

```powershell
# 単体テスト（全テスト、シングルスレッド実行）
cargo test --lib -- --test-threads=1

# パイプライン統合テスト（モックアダプター使用、ハードウェア不要）
cargo test --test pipeline_integration -- --test-threads=1

# GPU 処理統合テスト（GPU 必須、#[ignore] テスト）
cargo test --test gpu_integration -- --ignored --nocapture --test-threads=1

# DDA + GPU キャプチャ統合テスト（GPU + ディスプレイ必須、#[ignore] テスト）
cargo test --test gpu_capture_integration -- --ignored --nocapture --test-threads=1

# 実ハードウェアパイプラインスモークテスト（HID デバイス + ディスプレイ必須）
cargo test real_hardware_pipeline_smoke_test -- --ignored --nocapture --test-threads=1
```

---

## 設定リファレンス

設定ファイルは **TOML 形式**で、実行ファイルと同じディレクトリの `config.toml` から読み込まれます。

| ファイル | 用途 |
|---|---|
| [`config.toml.example`](config.toml.example) | コピーして使うテンプレート。全設定項目が記載済み |
| [`CONFIGURATION.md`](CONFIGURATION.md) | 全設定項目の詳細リファレンス（型・デフォルト値・バリデーション条件） |

### 主要設定セクション

| セクション | 内容 |
|---|---|
| `[capture]` | キャプチャソース（`dda` / `wgc`）・タイムアウト・再初期化設定 |
| `[process]` | 処理モード・最小検出面積・検出方法 |
| `[process.roi]` | 処理対象領域のサイズ（幅 × 高さ、デフォルト 460 × 240 px） |
| `[process.hsv_range]` | HSV カラー検知の色相・彩度・明度の範囲 |
| `[process.coordinate_transform]` | 座標変換の感度・クリップ量・デッドゾーン |
| `[communication]` | HID デバイスの VID / PID・送信間隔 |
| `[pipeline]` | ダーティレクト最適化・統計ログ間隔 |
| `[activation]` | 中心距離閾値・アクティブウィンドウ時間 |
| `[audio_feedback]` | 活性化音・非活性化音の WAV ファイルパス |
| `[gpu]` | GPU 処理の有効化・デバイスインデックス |
| `[debug]` | デバッグモード有効化 |

---

## リポジトリ構成

```
RoyaleWithCheese/
├── src/                  # Rust ソースコード（上記アーキテクチャ参照）
├── tests/                # 統合テスト
├── tools/                # 開発用ツール
├── scripts/              # ビルド補助スクリプト
├── third_party/          # OpenCV DLL など外部バイナリ
├── Cargo.toml            # クレート定義・依存関係
├── Cargo.lock            # 依存関係ロックファイル
├── build.rs              # ビルドスクリプト（OpenCV DLL コピー）
├── config.toml.example   # 設定ファイルテンプレート
└── CONFIGURATION.md      # 設定リファレンスドキュメント
```

---

## 技術スタック

| 項目 | 詳細 |
|---|---|
| 言語 | Rust 2021 edition |
| プラットフォーム | Windows 10 1803+ 専用 |
| 画像処理 | OpenCV 4.x（`opencv` crate v0.92） |
| D3D11 / WGC | `windows` crate v0.57 |
| HID 通信 | `hidapi` crate v2.6 |
| スレッド間通信 | `crossbeam-channel`（bounded、所有権移転モデル） |
| ロギング | `tracing` + `tracing-subscriber` |
| 設定 | `serde` + `toml` + `schemars` |

---

## ライセンス・注意事項

本プロジェクトは個人用途の実験的ツールです。  
USB HID デバイスへの書き込みを伴うため、使用するデバイスの仕様を十分確認したうえでご利用ください。
