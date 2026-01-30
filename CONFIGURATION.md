# 設定リファレンス (Configuration Reference)

## 概要

`config.toml`ファイルは、RoyaleWithCheeseの動作を制御する設定ファイルです。
JSON Schemaによる検証により、設定の正確性が保証されています。

**設定ファイルの場所**: `config.toml` (プロジェクトルート)  
**スキーマファイル**: `schema/config.json`  
**サンプル**: `config.toml.example`

## 設定ファイルの読み込み

- `config.toml`が存在する場合: ファイルから読み込み
- ファイルが存在しない場合: デフォルト値を使用（警告ログ出力）
- パース失敗時: デフォルト値を使用（警告ログ出力）

## 設定項目

### [capture] - キャプチャ設定

画面キャプチャの動作を制御します。

| 設定項目 | 型 | デフォルト値 | 説明 | 制約 |
|---------|-----|------------|------|------|
| `source` | enum | `"dda"` | キャプチャソース<br>• `"dda"`: Desktop Duplication API（画面全体をキャプチャ）<br>• `"spout"`: Spout DX11テクスチャ受信（送信側アプリケーションが必要）<br>• `"wgc"`: Windows Graphics Capture（低レイテンシモード、Win10 1803+） | `dda`/`spout`/`wgc` |
| `timeout_ms` | u64 | `8` | キャプチャタイムアウト（ミリ秒） | > 0 |
| `max_consecutive_timeouts` | u32 | `120` | 連続タイムアウト許容回数<br>この回数を超えたら再初期化を実行 | - |
| `reinit_initial_delay_ms` | u64 | `100` | 再初期化時の初期待機時間（ミリ秒） | - |
| `reinit_max_delay_ms` | u64 | `5000` | 再初期化時の最大待機時間（ミリ秒、指数バックオフの上限） | - |
| `monitor_index` | u32 | `0` | メインモニタのインデックス<br>**DDAのみ有効**（通常は0） | - |
| `spout_sender_name` | string? | `null` | Spout送信者名（オプション）<br>空文字列または省略で最初のアクティブ送信者に自動接続<br>**spoutソースのみ有効** | - |

### [process] - 画像処理設定

画像処理およびROI（Region of Interest）の設定。

| 設定項目 | 型 | デフォルト値 | 説明 | 制約 |
|---------|-----|------------|------|------|
| `mode` | string | `"fast-color"` | 処理モード<br>• `"fast-color"`: HSV色検知による高速処理<br>• `"yolo-ort"`: YOLO + ONNX Runtime による物体検出（将来実装） | - |
| `min_detection_area` | u32 | `0` | 最小検出面積（ピクセル数、これ未満は無視） | - |
| `detection_method` | enum | `"moments"` | 検出方法（fast-colorモードのみ使用）<br>• `"moments"`: モーメントによる重心計算（高精度）<br>• `"boundingbox"`: バウンディングボックスの中心（高速） | `moments`/`boundingbox` |

### [process.roi] - ROI設定

画面中心を基準として、指定したサイズの領域をキャプチャします。  
x, y座標は実行時に画面解像度から自動計算されます。

| 設定項目 | 型 | デフォルト値 | 説明 | 制約 |
|---------|-----|------------|------|------|
| `width` | u32 | `960` | ROI幅（ピクセル） | 画面解像度以下 |
| `height` | u32 | `540` | ROI高さ（ピクセル） | 画面解像度以下 |

⚠️ **注意**: width/heightが画面解像度を超える場合は起動時にエラーになります。

### [process.hsv_range] - HSV色空間レンジ

fast-colorモードで使用するHSV色空間の検出範囲（OpenCV準拠）。

| 設定項目 | 型 | デフォルト値 | 説明 | 制約 |
|---------|-----|------------|------|------|
| `h_min` | u8 | `25` | H（色相）の最小値 | 0-180 |
| `h_max` | u8 | `45` | H（色相）の最大値 | 0-180 |
| `s_min` | u8 | `80` | S（彩度）の最小値 | 0-255 |
| `s_max` | u8 | `255` | S（彩度）の最大値 | 0-255 |
| `v_min` | u8 | `80` | V（明度）の最小値 | 0-255 |
| `v_max` | u8 | `255` | V（明度）の最大値 | 0-255 |

デフォルト設定は黄色系の検出を想定しています。

### [process.coordinate_transform] - 座標変換設定

検出座標の変換パラメータ。

| 設定項目 | 型 | デフォルト値 | 説明 | 制約 |
|---------|-----|------------|------|------|
| `sensitivity` | f32 | `1.0` | 感度（倍率、X/Y軸共通） | > 0.0 |
| `x_clip_limit` | f32 | `10.0` | X軸のクリッピング限界値（ピクセル） | ≥ 0.0 |
| `y_clip_limit` | f32 | `10.0` | Y軸のクリッピング限界値（ピクセル） | ≥ 0.0 |
| `dead_zone` | f32 | `0.0` | デッドゾーン（ピクセル） | ≥ 0.0 |

### [communication] - HID通信設定

HIDデバイスとの通信パラメータ。

| 設定項目 | 型 | デフォルト値 | 説明 | 制約 |
|---------|-----|------------|------|------|
| `vendor_id` | u16 | `0x0000` | HIDデバイスのVendor ID（16進数で指定する場合は `0x1234` の形式） | - |
| `product_id` | u16 | `0x0000` | HIDデバイスのProduct ID | - |
| `serial_number` | string? | `null` | デバイスのシリアル番号（オプション） | - |
| `device_path` | string? | `null` | デバイスパス（オプション、最も確実な識別方法）<br>例 (Windows): `"\\\\?\\hid#vid_2341&pid_8036#..."` | - |
| `hid_send_interval_ms` | u64 | `4` | HIDレポート送信間隔（ミリ秒）<br>HIDスレッドで新しい検出結果を待つタイムアウト時間<br>例: 8ms ≈ 125Hz、7ms ≈ 143Hz、16ms ≈ 62Hz | - |

💡 **ヒント**: `cargo test test_enumerate_hid_devices -- --nocapture` でVendor ID/Product IDを取得できます。

### [activation] - アクティベーション設定

HID送信のアクティベーション条件。

| 設定項目 | 型 | デフォルト値 | 説明 | 制約 |
|---------|-----|------------|------|------|
| `max_distance_from_center` | f32 | `5.0` | ROI中心からの最大距離（ピクセル）<br>検出対象がROI中心からこの距離以内にある場合、アクティブ状態として記録される | - |
| `active_window_ms` | u64 | `500` | アクティブウィンドウの持続時間（ミリ秒）<br>最後にアクティブ条件を満たしてからこの時間内であればHID送信を許可する | - |

### [pipeline] - パイプライン設定

処理パイプラインの動作制御。

| 設定項目 | 型 | デフォルト値 | 説明 | 制約 |
|---------|-----|------------|------|------|
| `enable_dirty_rect_optimization` | bool | `false` | DirtyRect最適化を有効にするか（**未実装**）<br>true の場合、ROI と交差しない DirtyRect は処理をスキップ<br>⚠️ 現在、win_desktop_duplication クレートから DirtyRect 情報を取得していないため機能しません | - |
| `stats_interval_sec` | u64 | `10` | 統計情報の出力間隔（秒） | - |

### [audio_feedback] - 音声フィードバック設定

Insertキー押下時の音声フィードバック。

| 設定項目 | 型 | デフォルト値 | 説明 | 制約 |
|---------|-----|------------|------|------|
| `enabled` | bool | `true` | Insertキー押下時の音声フィードバックを有効にする | - |
| `on_sound` | string | `"C:\\Windows\\Media\\Speech On.wav"` | 有効化時の音声ファイルパス（Windowsシステム音を使用） | - |
| `off_sound` | string | `"C:\\Windows\\Media\\Speech Off.wav"` | 無効化時の音声ファイルパス | - |
| `fallback_to_silent` | bool | `true` | 音声ファイルが見つからない場合は静かに失敗する（ログのみ） | - |

---

## 使用例

### 基本設定（WGC + 低レイテンシ）

```toml
[capture]
source = "wgc"
timeout_ms = 8
max_consecutive_timeouts = 120
reinit_initial_delay_ms = 100
reinit_max_delay_ms = 5000
monitor_index = 0

[process]
mode = "fast-color"
min_detection_area = 0
detection_method = "moments"

[process.roi]
width = 460
height = 240

[process.hsv_range]
h_min = 25
h_max = 45
s_min = 80
s_max = 255
v_min = 80
v_max = 255

[process.coordinate_transform]
sensitivity = 1.0
x_clip_limit = 10.0
y_clip_limit = 10.0
dead_zone = 0.0

[communication]
vendor_id = 0x0000
product_id = 0x0000
hid_send_interval_ms = 4

[activation]
max_distance_from_center = 5.0
active_window_ms = 500

[pipeline]
enable_dirty_rect_optimization = false
stats_interval_sec = 10

[audio_feedback]
enabled = true
on_sound = "C:\\Windows\\Media\\Speech On.wav"
off_sound = "C:\\Windows\\Media\\Speech Off.wav"
fallback_to_silent = true
```

### DDA設定（複数モニタ環境）

```toml
[capture]
source = "dda"
monitor_index = 1
timeout_ms = 8
```

### Spout受信設定

```toml
[capture]
source = "spout"
spout_sender_name = "MyGame"
```

---

## 注意事項

### ROI設定

- **ROIは常に画面中心に配置されます**（プロジェクトの設計方針）
- width/heightが画面解像度を超える場合は起動時エラー
- 実行時に画面解像度から自動的にx, y座標が計算されます

### キャプチャソース選択

- **DDA**: 画面全体をキャプチャ、`monitor_index`で対象モニタ指定可能
- **Spout**: 別アプリケーションからのDX11テクスチャ受信、送信側アプリケーションが必要
- **WGC**: 低レイテンシモード（Win10 1803+）、処理レイテンシ0-1ms

### 検証エラー

設定ファイルの検証は`AppConfig::validate()`で行われます。  
以下のケースでエラーが発生します:

- ROI width/height が 0
- HSV H範囲が 0-180 の外、またはmin > max
- HSV S/V範囲で min > max
- capture timeout_ms が 0
- sensitivity が 0 以下
- clip_limit が負の値
- dead_zone が負の値

検証エラー時はプログラムが起動時に終了します。

---

## 関連ドキュメント

- [CLI_CONTRACT.md](CLI_CONTRACT.md) — 実行時契約とconfig.tomlの動作
- [DOMAIN_LAYER.md](DOMAIN_LAYER.md) — 設定の内部表現とDomain層の実装
- `schema/config.json` — JSON Schema定義ファイル

---

**更新履歴**:
- 2026-01-30: 初版作成（schema/config.jsonから自動生成）
