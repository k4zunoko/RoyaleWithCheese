# ロードマップ / 現状整理

## 現状（実装済み）

- Clean Architecture（Domain / Application / Infrastructure / Presentation）に沿った構成
- 4スレッドパイプライン（Capture / Process / HID / Stats(UI)）
- キャプチャ: DDA（`win_desktop_duplication`）
- 画像処理: OpenCV による HSV 色検知（fast-color）
- 通信: hidapi による HID 送信（モック経路あり）
- 自動復旧: 連続タイムアウト監視、指数バックオフでの再初期化
- 音声フィードバック: Insertキー等の切り替え時に PlaySoundW
- 開発補助: `opencv-debug-display` による視覚デバッグ、`performance-timing` による計測

## 既知の未実装 / 制約

- `process.mode = "yolo-ort"` は未実装（現状はエラーで終了）
- `pipeline.enable_dirty_rect_optimization` は項目として存在するが、DirtyRect情報取得が未対応のため実質未実装

## 次のステップ（計画中）

### Spout DX11テクスチャ受信 🆕

**概要**: DDAの代替としてSpout送信されたDX11テクスチャを受信する機能

**ユースケース**:
- ゲーム側がSpout送信をサポートしている場合、より効率的なテクスチャ取得
- DDA利用時の制約（管理者権限、排他的フルスクリーン等）を回避

**設計**:
- `CapturePort` traitの新規実装（`SpoutCaptureAdapter`）
- config.tomlで `capture.source = "spout"` で選択可能
- 詳細は [INFRASTRUCTURE_SPOUT.md](INFRASTRUCTURE_SPOUT.md) を参照

**ステータス**: 設計完了、実装待ち

### その他

- 実機（実HIDデバイス）での動作確認とチューニング
- エンドツーエンドのレイテンシ計測（p95/p99等）と最適化
- ログ/計測の運用方針の整理（保存期間、出力レベルなど）

## 将来（構想）

- YOLO + ONNX Runtime による検出（`yolo-ort` モード）
- DirtyRect 最適化の実装（ROIとDirtyRectの交差判定によるスキップ）

## Assumptions / Questions

- DirtyRect の取得方法は `win_desktop_duplication` 側の対応状況に依存する
- YOLO導入時の推論バックエンド（CPU/CUDA/TensorRT）やモデル選定は未確定
