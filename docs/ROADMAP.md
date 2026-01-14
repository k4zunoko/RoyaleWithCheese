# ロードマップ / 現状整理

## 現状（実装済み）

- Clean Architecture（Domain / Application / Infrastructure / Presentation）に沿った構成
- 4スレッドパイプライン（Capture / Process / HID / Stats(UI)）
- **キャプチャ**: 
  - **DDA (Desktop Duplication API)**: `win_desktop_duplication` を使用した画面全体キャプチャ
  - **Spout**: DX11テクスチャ受信によるアプリケーション間共有（2026-01-08完了）
  - **WGC (Windows Graphics Capture)**: 低レイテンシモード、処理レイテンシ0-1ms（2026-01-13完了）
- 画像処理: OpenCV による HSV 色検知（fast-color）
- 通信: hidapi による HID 送信（モック経路あり）
- 自動復旧: 連続タイムアウト監視、指数バックオフでの再初期化
- 音声フィードバック: Insertキー等の切り替え時に PlaySoundW
- 開発補助: `opencv-debug-display` による視覚デバッグ、`performance-timing` による計測

## 完了済みの大型タスク

### WGC (Windows Graphics Capture) キャプチャ実装 (2026-01-13)
- ✅ windows crate v0.57を使用した直接実装
- ✅ フレームプールバッファサイズ2で低レイテンシ化
- ✅ FrameArrivedイベントで即座にフレーム取得
- ✅ パフォーマンス: 処理レイテンシ**0-1ms**達成
- ✅ 60+ FPSで安定動作確認済み
- 詳細: [INFRASTRUCTURE_WGC.md](INFRASTRUCTURE_WGC.md)、[WGC_PHASE1_REPORT.md](WGC_PHASE1_REPORT.md)

### Spout DX11テクスチャ受信実装 (2026-01-08)
- ✅ spoutdx-ffi を使用したテクスチャ共有
- ✅ DDAの代替としてゼロコピーに近い低遅延実現
- ✅ 管理者権限不要、排他的フルスクリーン制約を回避
- 詳細: [INFRASTRUCTURE_SPOUT.md](INFRASTRUCTURE_SPOUT.md)

## 既知の未実装 / 制約

### 未実装機能
- **YOLO + ONNX Runtime検出**: `process.mode = "yolo-ort"` は未実装（現状はエラーで終了）
- **DirtyRect最適化**: `pipeline.enable_dirty_rect_optimization` は項目として存在するが、DirtyRect情報取得が未対応のため実質未実装

### キャプチャ方式別の制約
- **DDA**: 管理者権限が必要な場合あり、セキュアデスクトップ（UAC/ロック画面）は取得不可
- **Spout**: 送受信は同一GPUアダプタ必須、送信側アプリがSpout対応必要
- **WGC**: Windows 10 1803以降が必須、セキュアデスクトップは取得不可

## 次のステップ（計画中）

### パフォーマンス検証
- エンドツーエンドのレイテンシ計測（p95/p99等）と最適化
- DDA / Spout / WGC レイテンシベンチマーク比較
- 実機（実HIDデバイス）での動作確認とチューニング

### 運用・保守
- ログ/計測の運用方針の整理（保存期間、出力レベルなど）
- 長時間動作試験（安定性確認）

## 将来の拡張（構想）

### 画像処理
- **YOLO + ONNX Runtime検出**: `yolo-ort` モードの実装
  - 推論バックエンド選択（CPU/CUDA/TensorRT）
  - モデル選定とチューニング
  - ProcessPort trait の別実装として追加

### 最適化
- **DirtyRect最適化**: ROIとDirtyRectの交差判定によるスキップ
  - DDAのDirtyRect情報を活用
  - 静止フレーム時の処理削減

### 拡張性
- **マルチモニタ対応**: monitor_index による複数モニタ切り替え
- **HIDプロトコル拡張**: CommPort trait の別実装（シリアル通信、WebSocket等）

## Assumptions（前提）

- **DirtyRect取得**: `win_desktop_duplication` クレートの対応状況に依存
- **YOLO導入**: 推論バックエンド（CPU/CUDA/TensorRT）やモデル選定は未確定
- **実運用環境**: Windows 10 1803以降（WGC使用時）、Windows 8以降（DDA/Spoutのみ）

## Questions（要確認事項）

- 実運用での推奨キャプチャ方式（DDA / Spout / WGC）の選定基準
- HIDレポート仕様の詳細ドキュメント化（バイト配列の意味、座標スケール等）
- パフォーマンス目標値の最終確認（現状: End-to-End < 10ms @ 144Hz）
