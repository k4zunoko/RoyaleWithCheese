# 設定リファレンス (Configuration Reference)

## 概要

`config.toml`ファイルは、RoyaleWithCheeseの動作を制御する設定ファイルです。
JSON Schemaによる検証により、設定の正確性が保証されています。

**設定ファイルの場所**: `config.toml` (プロジェクトルート)  
**スキーマファイル**: `schema/config.json` (自動生成)  
**サンプル**: `config.toml.example`

⚠️ **注意**: このドキュメント（CONFIGURATION.md）は `cargo run --bin generate_schema` で自動生成されます。
設定項目の説明を変更する場合は、`src/domain/config.rs`のdoc commentsを編集してください。

## 設定ファイルの読み込み

- `config.toml`が存在する場合: ファイルから読み込み
- ファイルが存在しない場合: デフォルト値を使用（警告ログ出力）
- パース失敗時: デフォルト値を使用（警告ログ出力）

## 設定項目

### [activation] - アクティベーション設定

アクティベーション設定

| 設定項目 | 型 | デフォルト | 説明 |
|---------|-----|---------|---------|
| `active_window_ms` | uint64 | - | アクティブウィンドウの持続時間（ミリ秒）<br><br>最後にアクティブ条件を満たしてからこの時間内であればHID送信を許可する |
| `max_distance_from_center` | float | - | ROI中心からの最大距離（ピクセル）<br><br>検出対象がROI中心からこの距離以内にある場合、アクティブ状態として記録される |

### [audio_feedback] - 音声フィードバック設定

音声フィードバック設定

| 設定項目 | 型 | デフォルト | 説明 |
|---------|-----|---------|---------|
| `enabled` | bool | - | Insertキー押下時の音声フィードバックを有効にする |
| `fallback_to_silent` | bool | - | 音声ファイルが見つからない場合は静かに失敗する（ログのみ） |
| `off_sound` | string | - | 無効化時の音声ファイルパス |
| `on_sound` | string | - | 有効化時の音声ファイルパス（Windowsシステム音を使用） |

### [capture] - キャプチャ設定

キャプチャ設定

| 設定項目 | 型 | デフォルト | 説明 |
|---------|-----|---------|---------|
| `max_consecutive_timeouts` | uint32 | - | 連続タイムアウト許容回数<br><br>この回数を超えたら再初期化を実行 デフォルト: 120回 |
| `monitor_index` | uint32 | - | メインモニタのインデックス（DDAのみ有効）<br><br>通常は0 |
| `reinit_initial_delay_ms` | uint64 | - | 再初期化時の初期待機時間（ミリ秒）<br><br>デフォルト: 100ms |
| `reinit_max_delay_ms` | uint64 | - | 再初期化時の最大待機時間（ミリ秒、指数バックオフの上限）<br><br>デフォルト: 5000ms |
| `source` | CaptureSource | `"dda"` | キャプチャソース<br><br>選択肢: "dda", "spout", "wgc" デフォルト: "dda" |
| `spout_sender_name` | string \| null | `null` | Spout送信者名（source = "spout" の場合のみ有効）<br><br>空文字列または省略で最初のアクティブ送信者に自動接続 |
| `timeout_ms` | uint64 | - | キャプチャタイムアウト（ミリ秒）<br><br>デフォルト: 8ms |

### [communication] - HID通信設定

HID通信設定

| 設定項目 | 型 | デフォルト | 説明 |
|---------|-----|---------|---------|
| `device_path` | string \| null | `null` | デバイスパス（オプション、最も確実な識別方法）<br><br>例 (Windows): "\\\\?\\hid#vid_2341&pid_8036#..." |
| `hid_send_interval_ms` | uint64 | - | HIDレポート送信間隔（ミリ秒）<br><br>HIDスレッドで新しい検出結果を待つタイムアウト時間であり、HIDパケットの送信頻度を決定します。 新しい検出結果がない場合でも、この間隔で直前の値を送信し続けます。 例: 8ms = 約125Hz、7ms = 約143Hz、16ms = 約62Hz |
| `product_id` | uint16 | - | HIDデバイスのProduct ID |
| `serial_number` | string \| null | `null` | デバイスのシリアル番号（オプション） |
| `vendor_id` | uint16 | - | HIDデバイスのVendor ID（16進数で指定する場合は 0x1234 の形式）<br><br>`cargo test test_enumerate_hid_devices -- --nocapture` で取得できます |

### [pipeline] - パイプライン設定

パイプライン設定

| 設定項目 | 型 | デフォルト | 説明 |
|---------|-----|---------|---------|
| `enable_dirty_rect_optimization` | bool | - | DirtyRect最適化を有効にするか（未実装）<br><br>true の場合、ROI と交差しない DirtyRect は処理をスキップ 注: 現在、win_desktop_duplication クレートから DirtyRect 情報を取得していないため機能しません |
| `stats_interval_sec` | uint64 | - | 統計情報の出力間隔（秒） |

### [process] - 画像処理設定

画像処理設定

| 設定項目 | 型 | デフォルト | 説明 |
|---------|-----|---------|---------|
| `coordinate_transform` | object | - | 座標変換設定 |
| `detection_method` | DetectionMethod | `"moments"` | 検出方法（fast-colorモードのみ使用）<br><br>選択肢: "moments" (モーメントによる重心計算、高精度), "boundingbox" (バウンディングボックスの中心、高速) デフォルト: "moments" |
| `hsv_range` | object | - | HSVレンジ設定（fast-colorモードのみ使用） |
| `min_detection_area` | uint32 | - | 最小検出面積（ピクセル数、これ未満は無視）<br><br>デフォルト: 0 |
| `mode` | string | - | 処理モード<br><br>選択肢: "fast-color" (HSV色検知), "yolo-ort" (YOLO物体検出、将来実装) デフォルト: "fast-color" |
| `roi` | object | - | ROI（Region of Interest）設定 |

#### [coordinate_transform] - 座標変換設定

座標変換設定

| 設定項目 | 型 | デフォルト | 説明 |
|---------|-----|---------|---------|
| `dead_zone` | float | - | デッドゾーン（ピクセル）<br><br>デフォルト: 0.0 |
| `sensitivity` | float | - | 感度（倍率、X/Y軸共通）<br><br>デフォルト: 1.0 |
| `x_clip_limit` | float | - | X軸のクリッピング限界値（ピクセル）<br><br>デフォルト: 10.0 |
| `y_clip_limit` | float | - | Y軸のクリッピング限界値（ピクセル）<br><br>デフォルト: 10.0 |

#### [hsv_range] - HSV色空間レンジ

HSVレンジ設定

| 設定項目 | 型 | デフォルト | 説明 |
|---------|-----|---------|---------|
| `h_max` | uint8 | - | H（色相）の最大値<br><br>OpenCV準拠: H [0-180] |
| `h_min` | uint8 | - | H（色相）の最小値<br><br>OpenCV準拠: H [0-180] |
| `s_max` | uint8 | - | S（彩度）の最大値<br><br>OpenCV準拠: S [0-255] |
| `s_min` | uint8 | - | S（彩度）の最小値<br><br>OpenCV準拠: S [0-255] |
| `v_max` | uint8 | - | V（明度）の最大値<br><br>OpenCV準拠: V [0-255] |
| `v_min` | uint8 | - | V（明度）の最小値<br><br>OpenCV準拠: V [0-255] |

#### [roi] - ROI設定

ROI設定（サイズのみ、位置は画面中心に自動配置）

x, y座標は実行時に画面解像度から自動計算される。
プロジェクトの設計方針として、ROIは常に画面中心に配置される。

| 設定項目 | 型 | デフォルト | 説明 |
|---------|-----|---------|---------|
| `height` | uint32 | - | ROI高さ（ピクセル）<br><br>注意: 画面解像度を超える場合は起動時にエラーになります |
| `width` | uint32 | - | ROI幅（ピクセル）<br><br>注意: 画面解像度を超える場合は起動時にエラーになります |

## 参考

- [docs/CLI_CONTRACT.md](docs/CLI_CONTRACT.md) - 実行時契約
- [README.md](README.md) - クイックスタート
