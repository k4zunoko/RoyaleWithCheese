# 視覚的デバッグガイド（opencv-debug-display）

## 概要

`opencv-debug-display`フィーチャーは、**config.tomlの設定が正しく適用されているか**を視覚的に確認するための動作テスト機能です。

低レイテンシを重視したこのプロジェクトでは、通常の実行時はログ出力を最小限に抑えています。しかし、開発・調整段階では、以下のような確認が必要です：

- **ROI（Region of Interest）が期待した範囲か**
- **HSV色範囲が検出したい色を正しく捉えているか**
- **最小検出面積の設定が適切か**
- **重心計算が正しく動作しているか**

このフィーチャーを使用することで、これらの設定を**リアルタイムで視覚的に確認**できます。

## 実装構成

デバッグ表示機能は、保守性向上のため専用モジュールに分離されています：

- **[src/infrastructure/debug_display.rs](../src/infrastructure/debug_display.rs)** (400行)
  - OpenCVを使用した視覚的デバッグ機能
  - `#[cfg(feature = "opencv-debug-display")]`で全体をガード
  - Release buildでは完全に除外されるため、実行時パフォーマンスへの影響はゼロ

- **[src/infrastructure/color_process.rs](../src/infrastructure/color_process.rs)** (513行)
  - HSV色検知のコア処理
  - デバッグ機能は`debug_display::display_debug_images()`を呼び出し

この分離により、コア処理とデバッグ機能の責務が明確になり、保守性が向上しています。

## 使用方法

### 1. config.tomlの準備

テストしたい設定を`config.toml`に記述します：

```toml
[process.roi]
# テストしたいROI設定
# ROIは画面中心に自動配置されます（x/yは指定しません）
width = 960
height = 540

[process.hsv_range]
# テストしたいHSV範囲（例：黄色）
h_min = 25
h_max = 45
s_min = 80
s_max = 255
v_min = 80
v_max = 255

[process]
# 最小検出面積
min_detection_area = 100
```

### 2. 実行

```powershell
cargo run --features opencv-debug-display
```

### 3. 表示内容の確認

#### ウィンドウ1: Debug: BGR Capture

キャプチャされた元画像を**等倍表示**します。

**視覚的マーカー**：
- 検出時は重心位置に**緑色の十字マーク**と**円**を描画
- 検出されていない場合はマーカー表示なし

#### ウィンドウ2: Debug: Mask

HSV範囲でフィルタリングされた結果を**等倍表示**：
- **白**: HSV範囲内（検出対象）
- **黒**: HSV範囲外（非検出）

このマスク画像で、期待した領域が白く表示されているか確認します。

#### ウィンドウ3: Debug: Info

デバッグ情報を専用ウィンドウに表示（固定サイズ 400x300px）：

**表示される情報**：
- **ROI Size**: 処理されているROIのサイズ（例: `960x540 px`）
- **HSV Range**: config.tomlから読み込まれたHSV範囲
  - H、S、Vの範囲を個別に表示
- **Status**: 検出状態
  - `DETECTED` (緑色) または `NOT DETECTED` (赤色)
- **Coverage**: 検出された面積（ピクセル数）
- **Min Area**: 最小検出面積（config.tomlの`min_detection_area`）
- **Center**: 検出された重心座標（ROI内の相対座標）
- **操作方法**: 「Press ESC or 'q' to quit」

### 4. 操作方法

- **自動更新**: 約30fps（33ms間隔）で画像が自動更新されます
- **終了**: **ESCキー** または **'q'キー**を押す

## テストシナリオ

### シナリオ1: ROI設定の確認

**目的**: config.tomlで設定したROIが、期待した画面領域をキャプチャしているか確認する。

1. config.tomlのROI設定を変更
   ```toml
   [process.roi]
   # ROIは常に画面中心へ自動配置されます
   # ここではサイズを変えて、キャプチャ範囲が期待通りか確認します
   width = 1920
   height = 1080
   ```

2. 実行して「ROI Size」を確認
3. 期待したサイズが表示されているか確認

### シナリオ2: HSV範囲の調整

**目的**: 特定の色を検出するために、HSV範囲を調整する。

#### 黄色を検出する例：

1. テスト用の黄色い物体を画面に表示
2. 初期設定で実行
   ```toml
   [process.hsv_range]
   h_min = 20
   h_max = 40
   s_min = 100
   s_max = 255
   v_min = 100
   v_max = 255
   ```

3. **Debug: Mask**ウィンドウを確認
   - 黄色い部分が白く表示されているか
   - ノイズ（不要な検出）が多くないか

4. 検出が不十分な場合、範囲を広げる
   ```toml
   h_min = 15  # H範囲を広げる
   h_max = 45
   s_min = 80   # S下限を下げる（薄い色も検出）
   s_max = 255
   v_min = 80   # V下限を下げる（暗い色も検出）
   v_max = 255
   ```

5. ノイズが多い場合、範囲を狭める
   ```toml
   h_min = 25
   h_max = 35
   s_min = 120  # S下限を上げる（鮮やかな色のみ）
   s_max = 255
   v_min = 120  # V下限を上げる（明るい色のみ）
   v_max = 255
   ```

#### 赤色を検出する例：

赤色はHSVのHue値が0付近（0-10）と170-180に分布するため、2回に分けて設定が必要です。

**方法1**: 低Hue側のみ（0-10）
```toml
[process.hsv_range]
h_min = 0
h_max = 10
s_min = 100
s_max = 255
v_min = 100
v_max = 255
```

**方法2**: 高Hue側のみ（170-180）
```toml
[process.hsv_range]
h_min = 170
h_max = 180
s_min = 100
s_max = 255
v_min = 100
v_max = 255
```

### シナリオ3: 最小検出面積の調整

**目的**: ノイズを除去するための最小検出面積を調整する。

1. 小さな物体を検出したい場合（閾値を下げる）
   ```toml
   [process]
   min_detection_area = 50
   ```

2. ノイズを除去したい場合（閾値を上げる）
   ```toml
   [process]
   min_detection_area = 500
   ```

3. **Coverage**の値を確認
   - 検出されている場合、`Coverage: XXX px`が表示される
   - この値が`min_detection_area`以上であれば検出される

### シナリオ4: 重心計算の確認

**目的**: 重心が期待した位置に計算されているか確認する。

1. 画面中央に物体を配置
2. **Debug: Detection Result**ウィンドウで重心マーカー（緑の十字と円）を確認
3. **Center**座標が期待した値になっているか確認

## トラブルシューティング

### 問題1: 何も検出されない（Status: NOT DETECTED）

**確認項目**：
1. **Debug: HSV**ウィンドウで、検出したい色が存在するか
2. **Debug: Mask**ウィンドウで、白い領域があるか
3. HSV範囲が狭すぎないか → 範囲を広げてテスト
4. 最小検出面積が大きすぎないか → 閾値を下げてテスト

### 問題2: ノイズが多すぎる（不要な部分が検出される）

**対策**：
1. HSV範囲を狭める（特にS、Vの下限を上げる）
2. 最小検出面積を大きくする

### 問題3: ROIが期待と異なる

**確認項目**：
1. `config.toml`の`[process.roi]`設定が正しいか
2. `ROI Size`の表示値が期待通りか
3. キャプチャ対象モニタ（`capture.monitor_index`）が意図したものか

### 問題4: 重心がずれている

**原因**：
- 検出領域が複数ある場合、全体の重心が計算される
- ノイズが含まれている場合、重心がずれる

**対策**：
- HSV範囲を調整してノイズを除去
- 最小検出面積を大きくする

## パフォーマンスへの影響

**注意**: このフィーチャーは**画像表示のため大幅に処理速度が低下**します。

- 通常実行: 144Hz（約7ms/フレーム）
- デバッグ実行: 約30fps（約33ms/フレーム）

**レイテンシ増加の理由**：
- OpenCVのウィンドウ表示処理
- テキストとグラフィックのオーバーレイ描画
- キー入力待機（30ms）

**使用ガイドライン**：
- ✅ 設定調整時の動作確認
- ✅ 開発・デバッグ時の視覚的確認
- ❌ 実運用（リリースビルドでは無効）
- ❌ パフォーマンス測定

## 実装の詳細

### 低レイテンシへの配慮

デバッグ表示は`#[cfg(feature = "opencv-debug-display")]`で条件付きコンパイルされています。

**通常ビルド時**（featureなし）：
```rust
// デバッグコードは完全に削除される（条件付きコンパイル）
#[cfg(feature = "opencv-debug-display")]
self.display_debug_images(...); // コンパイルされない
```

**デバッグビルド時**（feature有効）：
```rust
// デバッグコードが有効化される
#[cfg(feature = "opencv-debug-display")]
self.display_debug_images(...); // コンパイルされる
```

この設計により、**通常実行時のオーバーヘッドは完全にゼロ**です。

### コードの配置

- **実装**: `src/infrastructure/color_process.rs`
  - `display_debug_images()`: デバッグウィンドウ表示
  - `draw_detection_overlay()`: オーバーレイ描画
- **設定**: `Cargo.toml`
  ```toml
  [features]
  opencv-debug-display = []
  ```

## まとめ

`opencv-debug-display`フィーチャーは、config.tomlの設定を視覚的に検証するための強力なツールです。

**活用シーン**：
1. 初期設定時のROI・HSV範囲の決定
2. 環境変化（照明、画面輝度）への対応
3. 新しい検出対象色への対応
4. パラメータ調整の効果確認

**設計哲学との整合性**：
- 条件付きコンパイルによりリリースビルドへの影響ゼロ
- 低レイテンシ重視の設計を損なわない
- 開発効率とパフォーマンスの両立

このツールを活用して、最適な設定を見つけてください！
