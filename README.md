# RoyaleWithCheese

Windows 向けのリアルタイム画面キャプチャ／画像処理／USB HID 出力パイプラインです。  
画面上の指定色をフレームごとに検出し、その座標を USB HID デバイスへ送信することで、外部デバイスによるカーソル操作を実現します。

---

## 動作要件

| 要件 | バージョン |
|---|---|
| OS | Windows 10 1803 以降 (WGC 使用時) / Windows 10 以降 (DDA 使用時) |
| Rust ツールチェーン | 1.75 以降 (`rustup` 推奨) |
| LLVM / Clang | `third_party/llvm/` に同梱 |
| OpenCV | `third_party/opencv/` に同梱 |
| GPU (任意) | D3D11 対応 GPU（GPU 処理を使用する場合） |
| USB HID デバイス | 送信先デバイスが接続されていること |

> **注意**: ネイティブライブラリは `third_party/` 以下にバンドルされています。  
> `.cargo/config.toml` で `LIBCLANG_PATH`、`OPENCV_INCLUDE_PATHS` 等が自動設定されるため、環境変数の手動設定は不要です。

---

## セットアップ

### 1. リポジトリのクローン

```powershell
git clone <repository-url>
cd RoyaleWithCheese
```

### 2. 設定ファイルの作成

```powershell
Copy-Item config.toml.example config.toml
```

`config.toml` を開き、少なくとも以下の項目をご自身の環境に合わせて変更してください：

```toml
[communication]
vendor_id = 0xXXXX   # 使用する HID デバイスの Vendor ID
product_id = 0xYYYY  # 使用する HID デバイスの Product ID
```

VID/PID の確認方法はデバイスマネージャー → 該当デバイス → プロパティ → 詳細 → ハードウェア ID で確認できます（形式: `USB\VID_xxxx&PID_yyyy`）。

---

## ビルド

```powershell
# デバッグビルド（開発・検証用）
cargo build

# リリースビルド（本番実行用・最適化済み）
cargo build --release
```

ビルド成果物は `target/debug/` または `target/release/` に生成されます。

---

## 実行

### 基本起動

```powershell
# デバッグビルドで実行
cargo run

# リリースビルドで実行（推奨）
cargo run --release

# ビルド済みバイナリを直接実行
.\target\release\RoyaleWithCheese.exe
```

起動時に `config.toml` の読み込み・パース・検証のいずれかに失敗した場合、アプリケーションはエラー終了します。

### ログレベルの制御

`RUST_LOG` 環境変数でログレベルを制御できます：

```powershell
# 詳細なデバッグログ（開発時）
$env:RUST_LOG = "debug"
cargo run

# 警告以上のみ表示（本番相当）
$env:RUST_LOG = "warn"
.\target\release\RoyaleWithCheese.exe
```

| ビルド | デフォルトレベル |
|---|---|
| デバッグビルド | `debug` |
| リリースビルド | `warn` |
| リリース + `performance-timing` フィーチャ | `info` |

---

## Cargo フィーチャ

```powershell
# パフォーマンス計測ログを有効化（フレームごとの処理時間を INFO レベルで出力）
cargo build --release --features performance-timing

# OpenCV デバッグウィンドウを有効化（処理中の画像をリアルタイム表示）
cargo build --features opencv-debug-display
```

| フィーチャ | 説明 |
|---|---|
| `performance-timing` | キャプチャ・処理・HID 送信の各ステージの処理時間をログ出力 |
| `opencv-debug-display` | OpenCV ウィンドウで処理中のフレームとマスク画像をリアルタイム表示（開発・チューニング用） |

---

## 設定

設定は `config.toml`（TOML 形式）で管理します。起動時に読み込まれ、起動後の変更は反映されません。

### 主要セクション早見表

| セクション | 役割 |
|---|---|
| `[capture]` | 画面キャプチャ方式・タイムアウトの設定 |
| `[process]` | 処理モードの選択 |
| `[process.roi]` | 処理対象とする矩形領域（画面中央に自動配置） |
| `[process.hsv_range]` | 検出対象の HSV 色範囲 |
| `[process.coordinate_transform]` | 感度・デッドゾーン・軸ごとのクリップ/無効化 |
| `[communication]` | USB HID デバイスの VID/PID・送信間隔 |
| `[activation]` | ROI 中心距離にもとづくアクティベーション制御 |
| `[pipeline]` | 統計情報出力間隔 |
| `[debug]` | デバッグモードの有効化 |


### キャプチャソースの選択

```toml
[capture]
source = "wgc"   # 推奨: 低レイテンシ (Windows 10 1803+)
# source = "dda" # 互換性重視
```

### GPU 処理の有効化

```toml
[process]
mode = "fast-color-gpu"
```

`mode = "fast-color-gpu"` を設定すると GPU 処理が有効になります。D3D11 対応 GPU が必要です。GPU 初期化に失敗した場合はエラーで終了します。

### アクティベーション制御

```toml
[activation]
enabled = true
max_distance_from_center = 15.0
active_window_ms = 500
```

`enabled = true` のときだけアクティベーション制御が有効になります。検出座標が ROI 中心から `max_distance_from_center` 以内に入ったときだけ `active_window_ms` 分だけアクティブ時間が加算され、そのウィンドウが残っている間は HID 出力を継続します。

### HSV 色範囲の調整

検出対象の色に合わせて HSV 範囲を調整します。OpenCV の HSV 色空間は H: 0–180、S/V: 0–255 です。

```toml
[process.hsv_range]
# 例: 黄色系
h_low = 25
h_high = 45
s_low = 80
s_high = 255
v_low = 80
v_high = 255
```

`opencv-debug-display` フィーチャを有効にしてビルドすると、実際の検出状況をリアルタイムで確認できます。

---

## テスト

```powershell
# 全ユニットテストを実行
cargo test --lib

# 全統合テストを実行
cargo test --tests

# 全テストを実行
cargo test

# 特定のテストを実行
cargo test domain::config::tests::test_example_config_validates -- --exact --nocapture

# 特定の統合テストファイルを実行
cargo test --test pipeline_integration

# ハードウェア依存テスト（GPU/ディスプレイが必要）
cargo test real_hardware_pipeline_smoke_test -- --ignored --nocapture --test-threads=1
```

> ハードウェア依存テストは `#[ignore]` が付与されており、通常のテスト実行では除外されます。  
> GPU/ディスプレイが接続されていない環境では実行しないでください。

---

## 検証フロー

変更後は以下のコマンドで品質確認を行ってください：

```powershell
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

---

## アーキテクチャ

```
src/
├── main.rs               # エントリーポイント・アダプタ配線
├── logging.rs            # tracing ロギング初期化
├── domain/               # コア型・ポート・設定・エラーモデル（抽象層）
│   ├── config.rs         # AppConfig (全設定構造体 + 検証)
│   ├── ports.rs          # CapturePort / ProcessPort / CommPort / InputPort
│   ├── types.rs          # Roi / Frame / GpuFrame / DetectionResult など
│   └── error.rs          # DomainError / DomainResult
├── application/          # パイプラインオーケストレーション層
│   ├── pipeline.rs       # PipelineRunner (スレッド管理・チャンネル配線)
│   ├── threads.rs        # 各スレッドのループ実装
│   ├── metrics.rs        # PipelineMetrics (フレームレート等の計測)
│   ├── recovery.rs       # キャプチャ再初期化ロジック
│   └── runtime_state.rs  # RuntimeState (Arc<AtomicBool> による制御フラグ)
└── infrastructure/       # 具体的なアダプタ実装層
    ├── capture/
    │   ├── dda.rs        # Desktop Duplication API アダプタ
    │   └── wgc.rs        # Windows Graphics Capture アダプタ
    ├── processing/
    │   ├── cpu/          # OpenCV HSV 色検出 (CPU)
    │   ├── gpu/          # D3D11 コンピュートシェーダ (GPU)
    │   └── selector.rs   # ProcessSelector (CPU/GPU 列挙ディスパッチ)
    ├── hid_comm.rs       # HidCommAdapter (hidapi)
    ├── input.rs          # WindowsInputAdapter (Win32 GetAsyncKeyState)
    └── gpu_device.rs     # D3D11 デバイス共有ユーティリティ
tests/
├── pipeline_integration.rs     # モックベースのパイプライン統合テスト
├── gpu_integration.rs          # GPU アダプタ統合テスト
└── gpu_capture_integration.rs  # GPU キャプチャ統合テスト
```

### データフロー

```
[キャプチャスレッド]
    DdaCaptureAdapter / WgcCaptureAdapter
        ↓ Frame (crossbeam-channel)
[処理スレッド]
    ProcessSelector (CPU: ColorProcessAdapter / GPU: GpuColorAdapter)
        ↓ DetectionResult
[HID 送信スレッド]
    HidCommAdapter → USB HID デバイス
        ↓
[統計スレッド] (performance-timing フィーチャ時)
    PipelineMetrics → ログ出力
```

---

## トラブルシューティング

### 起動時に `HID init failed` が出る

`config.toml` の `vendor_id` / `product_id` が実際のデバイスと一致していない可能性があります。  
デバイスマネージャーで VID/PID を確認し、正しい値を設定してください（`0x0000` は無効です）。

### キャプチャが始まらない / タイムアウトが多発する

- `capture.source = "wgc"` を試してください（低レイテンシ）
- `capture.timeout_ms` を少し大きくしてください（例: `16`）
- WGC を使用する場合は Windows 10 1803 以降が必要です

### 検出ができない

- `opencv-debug-display` フィーチャを有効にしてビルドし、実際の色範囲を確認してください
- `process.hsv_range` の H/S/V 範囲を調整してください
- `process.roi` のサイズが小さすぎないか確認してください（画面外の場合は左上原点にフォールバックします）

### GPU 処理が有効にならない

- D3D11 対応 GPU が接続されているか確認してください
- `process.mode = "fast-color-gpu"` を設定してください
- GPU 初期化に失敗した場合はエラーで終了します
