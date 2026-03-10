# CONFIGURATION.md - RoyaleWithCheese 設定リファレンス

このドキュメントは `config.toml` のすべての設定項目を説明します。

## はじめに

設定ファイルは TOML 形式です。以下のセクションから構成されます：

- **[capture]** - 画面キャプチャ方式の設定
- **[process]** - 画像処理のパイプライン設定
- **[process.roi]** - 関心領域 (Region of Interest)
- **[process.hsv_range]** - HSV色検知の範囲設定
- **[process.coordinate_transform]** - 座標変換・感度設定
- **[communication]** - USB HID デバイス通信設定
- **[pipeline]** - パイプライン全体の最適化設定
- **[activation]** - マウスカーソル活性化条件
- **[audio_feedback]** - 音声フィードバック設定
- **[gpu]** - GPU 処理設定
- **[debug]** - デバッグ機能設定

---

## [capture] - キャプチャ設定

画面キャプチャの方式とタイムアウト動作を設定します。

### source
- **型**: 文字列
- **デフォルト**: `"dda"`
- **有効な値**: `"dda"`, `"wgc"`
- **説明**: キャプチャ方式を指定します
  - `"dda"` - Desktop Duplication API: 画面全体を標準的に処理（汎用）
  - `"wgc"` - Windows Graphics Capture: 低レイテンシモード（推奨、Windows 10 1803+）

### timeout_ms
- **型**: 整数 (u32)
- **デフォルト**: `8`
- **単位**: ミリ秒
- **有効な値**: `1` 以上
- **説明**: キャプチャタイムアウトの閾値。この時間内にキャプチャできない場合、タイムアウトと判定されます。

### max_consecutive_timeouts
- **型**: 整数 (u32)
- **デフォルト**: `120`
- **単位**: 回数
- **有効な値**: `1` 以上
- **説明**: 連続でタイムアウトした場合、この回数を超えたら再初期化を行います。

### reinit_initial_delay_ms
- **型**: 整数 (u32)
- **デフォルト**: `100`
- **単位**: ミリ秒
- **有効な値**: `1` 以上
- **説明**: 再初期化を最初に行う際の遅延時間。その後、指数バックオフで増加します。

### reinit_max_delay_ms
- **型**: 整数 (u32)
- **デフォルト**: `5000`
- **単位**: ミリ秒
- **有効な値**: `1` 以上
- **説明**: 再初期化の最大遅延時間。指数バックオフはこの値で上限します。

### monitor_index
- **型**: 整数 (u32)
- **デフォルト**: `0`
- **有効な値**: `0` 以上
- **説明**: キャプチャ対象のモニター番号（0 = プライマリモニター）。複数モニター環境で対象モニターを指定します。

### 設定例

```toml
[capture]
source = "wgc"              # 低レイテンシ推奨
timeout_ms = 8
max_consecutive_timeouts = 120
reinit_initial_delay_ms = 100
reinit_max_delay_ms = 5000
monitor_index = 0
```

---

## [process] - 画像処理パイプライン設定

画像処理全体の動作モードと検出方法を設定します。

### mode
- **型**: 文字列
- **デフォルト**: `"fast-color"`
- **有効な値**: `"fast-color"`
- **説明**: 画像処理のモード。現在は `"fast-color"` のみサポート。HSV色検知を使用した高速検出。

### min_detection_area
- **型**: 整数 (u32)
- **デフォルト**: `0`
- **単位**: ピクセル²（四角形の面積）
- **有効な値**: `0` 以上
- **説明**: 検出対象とする最小面積。この値より小さい領域は無視されます。ノイズ除去に使用。

### detection_method
- **型**: 文字列
- **デフォルト**: `"moments"`
- **有効な値**: `"moments"`
- **説明**: 検出した領域の代表点を求める方法。現在は `"moments"` （画像モーメント）のみサポート。

### 設定例

```toml
[process]
mode = "fast-color"
min_detection_area = 0
detection_method = "moments"
```

---

## [process.roi] - 関心領域（ROI）設定

キャプチャ画像の中から、処理対象とする矩形領域（関心領域）を定義します。

### width
- **型**: 整数 (u32)
- **デフォルト**: `460`
- **単位**: ピクセル
- **有効な値**: `1` 以上
- **説明**: ROI の幅。例：1920x1080 モニターの中央 460px 幅を処理。

### height
- **型**: 整数 (u32)
- **デフォルト**: `240`
- **単位**: ピクセル
- **有効な値**: `1` 以上
- **説明**: ROI の高さ。例：1920x1080 モニターの中央 240px 高さを処理。

### 設定例

```toml
[process.roi]
width = 460
height = 240
```

---

## [process.hsv_range] - HSV色検知範囲設定

HSV色空間での検出対象色の範囲を指定します。例：黄色系、赤系など。

### h_low
- **型**: 整数 (u8)
- **デフォルト**: `25`
- **単位**: HSV Hue (0-180 in OpenCV)
- **有効な値**: `0` ～ `180`、かつ `h_low <= h_high`
- **説明**: Hue の最小値。検出する色相の下限。

### h_high
- **型**: 整数 (u8)
- **デフォルト**: `45`
- **単位**: HSV Hue (0-180 in OpenCV)
- **有効な値**: `0` ～ `180`、かつ `h_low <= h_high`
- **説明**: Hue の最大値。検出する色相の上限。

### s_low
- **型**: 整数 (u8)
- **デフォルト**: `80`
- **単位**: HSV Saturation (0-255)
- **有効な値**: `0` ～ `255`、かつ `s_low <= s_high`
- **説明**: Saturation の最小値。色の鮮やかさの下限。高い値ほど濃い色のみを検出。

### s_high
- **型**: 整数 (u8)
- **デフォルト**: `255`
- **単位**: HSV Saturation (0-255)
- **有効な値**: `0` ～ `255`、かつ `s_low <= s_high`
- **説明**: Saturation の最大値。色の鮮やかさの上限。通常は 255。

### v_low
- **型**: 整数 (u8)
- **デフォルト**: `80`
- **単位**: HSV Value (0-255)
- **有効な値**: `0` ～ `255`、かつ `v_low <= v_high`
- **説明**: Value の最小値。明度の下限。低い値は暗い色のみを検出。

### v_high
- **型**: 整数 (u8)
- **デフォルト**: `255`
- **単位**: HSV Value (0-255)
- **有効な値**: `0` ～ `255`、かつ `v_low <= v_high`
- **説明**: Value の最大値。明度の上限。通常は 255。

### 設定例

```toml
[process.hsv_range]
h_low = 25      # 黄色系の下限
h_high = 45     # 黄色系の上限
s_low = 80      # ある程度の彩度が必要
s_high = 255    # 最大彩度まで受け入れ
v_low = 80      # 暗すぎるものは除外
v_high = 255    # 最大明度まで受け入れ
```

---

## [process.coordinate_transform] - 座標変換・感度設定

キャプチャ画像上での座標を、マウスカーソル移動量に変換する際のパラメータです。

### sensitivity
- **型**: 浮動小数点 (f64)
- **デフォルト**: `1.0`
- **有効な値**: `0.0` より大きい
- **説明**: 座標変換の感度倍率。1.0 でスケール 1:1、2.0 で 2 倍の移動量になります。

### x_clip_limit
- **型**: 浮動小数点 (f64)
- **デフォルト**: `10.0`
- **単位**: ピクセル（または単位なし）
- **有効な値**: `0.0` 以上
- **説明**: X 軸（水平）の移動量の上限。急激な移動を制限してスムーズに。

### y_clip_limit
- **型**: 浮動小数点 (f64)
- **デフォルト**: `10.0`
- **単位**: ピクセル（または単位なし）
- **有効な値**: `0.0` 以上
- **説明**: Y 軸（垂直）の移動量の上限。急激な移動を制限してスムーズに。

### dead_zone
- **型**: 浮動小数点 (f64)
- **デフォルト**: `0.0`
- **単位**: ピクセル（または単位なし）
- **有効な値**: `0.0` 以上
- **説明**: 無応答ゾーンの半径。ROI 中央からこの距離以内の移動は無視。ジッターを除去。

### 設定例

```toml
[process.coordinate_transform]
sensitivity = 1.0
x_clip_limit = 10.0
y_clip_limit = 10.0
dead_zone = 0.0
```

---

## [communication] - USB HID 通信設定

HID デバイス（マウス、キーボードなど）への接続設定。

### vendor_id
- **型**: 16進数整数 (u32)
- **デフォルト**: `0x1234`（コード内デフォルト）
- **有効な値**: `0x0001` ～ `0xFFFF`（0 より大きい必要）
- **説明**: HID デバイスの Vendor ID。USB VID リストで確認可能。
  - **注意**: config.toml.example では `0x0000` を使用していますが、これはプレースホルダーです。実際に使用する際は自分のデバイスの VID を設定してください。

### product_id
- **型**: 16進数整数 (u32)
- **デフォルト**: `0x5678`（コード内デフォルト）
- **有効な値**: `0x0001` ～ `0xFFFF`（0 より大きい必要）
- **説明**: HID デバイスの Product ID。USB PID リストで確認可能。
  - **注意**: config.toml.example では `0x0000` を使用していますが、これはプレースホルダーです。実際に使用する際は自分のデバイスの PID を設定してください。

### hid_send_interval_ms
- **型**: 整数 (u32)
- **デフォルト**: `4`
- **単位**: ミリ秒
- **有効な値**: `1` 以上
- **説明**: HID メッセージを送信する間隔。低い値ほど応答性が向上しますが、レポートレートに注意。

### 設定例

```toml
[communication]
vendor_id = 0x1234         # あなたのデバイスの VID に置き換え
product_id = 0x5678        # あなたのデバイスの PID に置き換え
hid_send_interval_ms = 4
```

### デバイス ID の確認方法

Windows 環境で VID・PID を確認する手順：

1. デバイスマネージャーを開く
2. 該当 HID デバイスを右クリック → プロパティ
3. 詳細タブで「ハードウェア ID」を確認
4. 形式：`USB\VID_xxxx&PID_yyyy` の xx、yy の部分が VID、PID

---

## [pipeline] - パイプライン全体設定

キャプチャ → 処理 → HID → 統計情報 のパイプライン全体の動作を設定します。

### enable_dirty_rect_optimization
- **型**: 真偽値 (bool)
- **デフォルト**: `false`
- **有効な値**: `true`, `false`
- **説明**: ダーティレクト最適化を有効にするか。`true` の場合、変更領域のみ処理。パフォーマンス向上の可能性がある一方、処理の複雑さが増加。

### stats_interval_sec
- **型**: 整数 (u32)
- **デフォルト**: `10`
- **単位**: 秒
- **有効な値**: `1` 以上
- **説明**: パフォーマンス統計情報を出力する間隔。`performance-timing` フィーチャ有効時のみ動作。

### 設定例

```toml
[pipeline]
enable_dirty_rect_optimization = false
stats_interval_sec = 10
```

---

## [activation] - マウスカーソル活性化条件

検出結果に基づいてマウスカーソル移動を開始・停止する条件。

### max_distance_from_center
- **型**: 浮動小数点 (f64)
- **デフォルト**: `5.0`
- **単位**: ピクセル（または単位なし）
- **有効な値**: `0.0` 以上
- **説明**: ROI 中央からの最大距離。検出点がこの距離を超えた場合、マウス移動の活性化を解除。

### active_window_ms
- **型**: 整数 (u32)
- **デフォルト**: `500`
- **単位**: ミリ秒
- **有効な値**: `1` 以上
- **説明**: マウス移動の活性状態を保つ時間窓。この時間内に新しい検出がない場合、非活性化。

### 設定例

```toml
[activation]
max_distance_from_center = 5.0
active_window_ms = 500
```

---

## [audio_feedback] - 音声フィードバック設定

マウス移動の活性化・非活性化に伴う音声フィードバック。

### enabled
- **型**: 真偽値 (bool)
- **デフォルト**: `true`
- **有効な値**: `true`, `false`
- **説明**: 音声フィードバックを有効にするか。`false` で無音。

### on_sound
- **型**: 文字列（ファイルパス）
- **デフォルト**: `"C:\\Windows\\Media\\Speech On.wav"`
- **有効な値**: WAV ファイルへの絶対パス
- **説明**: マウス移動が活性化した際に再生する音声ファイル。パスはバックスラッシュでエスケープ。

### off_sound
- **型**: 文字列（ファイルパス）
- **デフォルト**: `"C:\\Windows\\Media\\Speech Off.wav"`
- **有効な値**: WAV ファイルへの絶対パス
- **説明**: マウス移動が非活性化した際に再生する音声ファイル。パスはバックスラッシュでエスケープ。

### fallback_to_silent
- **型**: 真偽値 (bool)
- **デフォルト**: `true`
- **有効な値**: `true`, `false`
- **説明**: 音声ファイルが見つからない場合、エラーで停止せず無音で続行するか。

### 設定例

```toml
[audio_feedback]
enabled = true
on_sound = "C:\\Windows\\Media\\Speech On.wav"
off_sound = "C:\\Windows\\Media\\Speech Off.wav"
fallback_to_silent = true
```

---

## [gpu] - GPU 処理設定

D3D11 コンピュートシェーダを使用した GPU 処理。

### enabled
- **型**: 真偽値 (bool)
- **デフォルト**: `false`
- **有効な値**: `true`, `false`
- **説明**: GPU 処理を有効にするか。
  - `true` - GPU 処理を試行し、失敗時は自動的に CPU にフォールバック
  - `false` - CPU 処理のみを使用（推奨）

### device_index
- **型**: 整数 (u32)
- **デフォルト**: `0`
- **有効な値**: `0` 以上
- **説明**: 使用する GPU デバイスのインデックス。0 = プライマリ GPU。複数 GPU 環境で対象 GPU を指定。`enabled = true` のみ有効。

### prefer_gpu
- **型**: 真偽値 (bool)
- **デフォルト**: `false`
- **有効な値**: `true`, `false`
- **説明**: 将来の拡張用。現在は `enabled` と同じ動作。GPU 失敗時は常に CPU へ自動フォールバック。

### 設定例

```toml
[gpu]
enabled = false
device_index = 0
prefer_gpu = false
```

---

## [debug] - デバッグ機能設定

デバッグ機能を有効・無効にします。

### enabled
- **型**: 真偽値 (bool)
- **デフォルト**: `false`
- **有効な値**: `true`, `false`
- **説明**: デバッグ機能を有効にするか。`true` の場合、詳細なログ出力や OpenCV ウィンドウ表示など。

### 設定例

```toml
[debug]
enabled = false
```

---

## 完全な設定ファイル例

以下は、すべてのセクションを含む完全な設定例です：

```toml
# RoyaleWithCheese 設定ファイル例
# このファイルをコピーして config.toml として使用してください
# 詳細な説明は CONFIGURATION.md を参照してください

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
h_low = 25
h_high = 45
s_low = 80
s_high = 255
v_low = 80
v_high = 255

[process.coordinate_transform]
sensitivity = 1.0
x_clip_limit = 10.0
y_clip_limit = 10.0
dead_zone = 0.0

[communication]
vendor_id = 0x1234
product_id = 0x5678
hid_send_interval_ms = 4

[pipeline]
enable_dirty_rect_optimization = false
stats_interval_sec = 10

[activation]
max_distance_from_center = 5.0
active_window_ms = 500

[audio_feedback]
enabled = true
on_sound = "C:\\Windows\\Media\\Speech On.wav"
off_sound = "C:\\Windows\\Media\\Speech Off.wav"
fallback_to_silent = true

[gpu]
enabled = false
device_index = 0
prefer_gpu = false

[debug]
enabled = false
```

---

## トラブルシューティング

### 設定ファイルが見つからないエラー
- ファイル名が `config.toml` であることを確認
- `config.toml.example` をコピーして作成してください

### TOML パースエラー
- ファイルに構文エラーがないか確認
- TOML 形式のバリデータを使用（例：[toml-lint.com](https://www.toml-lint.com/)）

### 検出できない場合
- HSV 範囲を調整。`process.hsv_range` の値を見直してください
- ROI サイズが適切か確認。`process.roi` で調整

### マウスが移動しない場合
- `communication` の vendor_id/product_id が正しいか確認
- HID デバイスがシステムに接続されているか確認

### パフォーマンス最適化
- `capture.source` を `"wgc"` に変更（低レイテンシ推奨）
- `pipeline.enable_dirty_rect_optimization` を `true` にする（場合によっては効果あり）
- `debug.enabled` を `false` に確認（ログ出力はオーバーヘッド）

---

## 参考資料

- OpenCV HSV 色空間: [HSV in OpenCV](https://docs.opencv.org/4.0.0/df/d9d/tutorial_py_colorspaces.html)
- USB Vendor/Product ID: [USB ID Repository](http://www.linux-usb.org/usb.ids)
- Windows Graphics Capture: [Microsoft Docs](https://docs.microsoft.com/en-us/windows/win32/direct3d11/direct3d-11-3-features)
