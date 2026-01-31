# アーキテクチャ概要

このドキュメントは、RoyaleWithCheese の全体像（構成・データフロー・責務分離）を短時間で把握するための **俯瞰**です。
設計原則や根拠は [DESIGN_PHILOSOPHY.md](DESIGN_PHILOSOPHY.md) に集約します。

## 目的

Windows 環境で、画面キャプチャ → 画像処理 → HID送信のパイプラインを低レイテンシで継続実行します。

## レイヤ構成（Clean Architecture）

```
┌───────────────────────────────┐
│     Presentation (main.rs)     │  設定読込・DI・起動制御
└──────────────▲────────────────┘
               │ DI注入
┌──────────────┴────────────────┐
│  Application (UseCase)         │  パイプライン制御・ポリシー・復旧
└──────────────▲────────────────┘
               │ trait依存
┌──────────────┴────────────────┐
│  Domain (Core)                 │  型・trait・エラー・設定
└──────────────▲────────────────┘
               │ trait実装
┌──────────────┴────────────────┐
│  Infrastructure/Adapters       │  DDA/OpenCV/HID/入力/音
└───────────────────────────────┘
```

レイヤ責務の詳細は以下を参照してください。

- Domain: [DOMAIN_LAYER.md](DOMAIN_LAYER.md)
- Application: [APPLICATION_LAYER.md](APPLICATION_LAYER.md)
- エラーハンドリング: [ERROR_HANDLING.md](ERROR_HANDLING.md)

## 実装上のディレクトリ構成

```
src/
  main.rs
  domain/
  application/
  infrastructure/
  logging.rs
```

Infrastructure の詳細（DDAの制約・復旧）は [INFRASTRUCTURE_CAPTURE.md](INFRASTRUCTURE_CAPTURE.md) を参照してください。

## ランタイムのデータフロー

### 4スレッドパイプライン

```
┌─────────────┐   bounded(1)    ┌─────────────┐   bounded(1)    ┌─────────────┐
│   Capture    │ ──────────────> │   Process    │ ──────────────> │     HID      │
│   Thread     │                 │   Thread     │                 │   Thread     │
└─────────────┘                  └──────┬───────┘                 └─────────────┘
                                        │
                                        │ unbounded（統計）
                                        ↓
                                  ┌─────────────┐
                                  │  Stats/UI    │
                                  │   Thread     │
                                  └─────────────┘
```

- Capture: DDA で ROI を取り込み
- Process: fast-color（OpenCV HSV 色検知）
- HID: 最新の検出結果を一定間隔で送信
- Stats/UI: 統計集計・有効/無効の切替・音声フィードバック

詳細は [APPLICATION_LAYER.md](APPLICATION_LAYER.md) を参照してください。

### 「最新のみ」ポリシー

Capture→Process、Process→HID は `bounded(1)` を使い、古いデータを溜めずに最新へ追従します。

## 設定の扱い

- 実行時に `config.toml` を読み込み（存在しない場合はデフォルト設定で継続）
- 読み込んだ設定は起動時に検証します
- ROI は `width/height` のみ設定し、実行時に画面中心へ自動配置します

設定ファイル契約は [CLI_CONTRACT.md](CLI_CONTRACT.md) を参照してください。

## キャプチャ方式の実装状況

- **DDA (Desktop Duplication API)**: ✅ 実装済み - 画面全体の高速キャプチャ
- **Spout**: ✅ 実装済み（2026-01-08） - DX11テクスチャ受信
- **WGC (Windows Graphics Capture)**: ✅ 実装済み（2026-01-13） - 低レイテンシモード

## process.mode の実装状況

- `fast-color`: ✅ 実装済み（HSV 色検知）
- `yolo-ort`: ❌ 未実装（指定するとエラーで終了）

詳細は [ROADMAP.md](ROADMAP.md) を参照してください。

## Cargo features

- `performance-timing`: 計測ログ
- `opencv-debug-display`: 視覚デバッグ表示

使い方は [VISUAL_DEBUG_GUIDE.md](VISUAL_DEBUG_GUIDE.md) を参照してください。

## Processing モジュール構成

画像処理 Infrastructure は CPU/GPU 実装に分離されています:

```
src/infrastructure/processing/
├── mod.rs              # モジュールエクスポート
├── cpu/
│   └── mod.rs          # ColorProcessAdapter (OpenCV ベース)
└── gpu/
    ├── mod.rs          # GpuColorProcessor (D3D11 Compute Shader)
    └── shaders/
        └── hsv_detect.hlsl  # HLSL Compute Shader
```

### 現在のデータフロー (CPU)

```
Capture (DDA/WGC/Spout)
    ↓
GPU Texture
    ↓ CopySubresourceRegion (ROI のみ)
Staging Texture
    ↓ Map/Unmap
Frame { data: Vec<u8> }
    ↓
ColorProcessAdapter (OpenCV)
    ↓ BGRA→BGR→HSV 変換
DetectionResult
    ↓
HID Thread
```

### GPU データフロー (実装済み、統合待ち)

```
Capture (DDA/WGC/Spout)
    ↓
GPU Texture
    ↓ (GPU 常駐のまま)
GpuFrame { texture: ID3D11Texture2D }
    ↓
GpuColorProcessor
    ↓ D3D11 Compute Shader (hsv_detect.hlsl)
    │   - BGRA→HSV 変換 (GPU 上)
    │   - 色範囲判定
    │   - 並列リダクション (検出ピクセル数、座標合計)
    ↓
Staging Buffer (12 bytes: count, sum_x, sum_y)
    ↓ Map/Unmap
DetectionResult (座標のみ CPU へ)
    ↓
HID Thread
```

**利点**: GPU→CPU コピーを 12 バイトの結果バッファのみに削減し、レイテンシを大幅に改善

### GPU Compute Shader 詳細

HLSL シェーダ (`hsv_detect.hlsl`) の処理フロー:

1. **スレッドグループ**: 16x16 = 256 スレッド/グループ
2. **BGRA→HSV 変換**: OpenCV 互換 (H: 0-180, S/V: 0-255)
3. **範囲判定**: Hue ラップアラウンド対応 (赤色検出: h_min=170, h_max=10)
4. **ローカル集約**: グループ共有メモリで検出数・座標合計を集計
5. **グローバル集約**: Atomic 演算で全グループ結果を統合
6. **結果読み戻し**: Staging Buffer 経由で CPU へ (12 バイト)