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

## process.mode の実装状況

- `fast-color`: 実装済み（HSV 色検知）
- `yolo-ort`: 現状未実装（指定するとエラーで終了）

未実装/制約は [ROADMAP.md](ROADMAP.md) にまとめます。

## Cargo features

- `performance-timing`: 計測ログ
- `opencv-debug-display`: 視覚デバッグ表示

使い方は [VISUAL_DEBUG_GUIDE.md](VISUAL_DEBUG_GUIDE.md) を参照してください。