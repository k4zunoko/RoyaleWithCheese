# Infrastructure層: WGC (Windows Graphics Capture) 設計方針

このドキュメントは、Windows Graphics Capture API を使用した新しいキャプチャアダプタ (`WgcCaptureAdapter`) の設計方針、実装計画、および未決定事項を記載します。

## 概要

### WGCとは

Windows Graphics Capture (WGC) は、Windows 10 バージョン1803以降で利用可能な画面キャプチャAPIです。Desktop Duplication API (DDA) と同じ基盤技術を使用しつつ、いくつかの改善が加えられています。

### 導入目的

| 目的 | 説明 |
|------|------|
| DDAの代替手段 | DDAが使用不可な状況（他アプリがDDAを専有など）での代替 |
| クロスGPU対応 | OBSなどで報告されているDDAのGPU制約を回避 |
| MPO対応の可能性 | Multi-Plane Overlay の再合成問題への対応（要検証） |
| 技術的冗長性 | キャプチャ手段の多様化による安定性向上 |

## DDA vs WGC 比較

| 項目 | DDA | WGC |
|------|-----|-----|
| **最小Windows** | Windows 8 | Windows 10 1803 |
| **レイテンシ** | 非常に低い | 低い（1コピー削減の可能性） |
| **ウィンドウ単位キャプチャ** | ❌ 不可 | ✅ 可能 |
| **モニター単位キャプチャ** | ✅ 可能 | ✅ 可能 |
| **DirtyRect** | ✅ 提供 | ❌ 未提供（将来追加予定） |
| **Protected Content情報** | ✅ 提供 | ⚠️ 未提供（黒矩形に置換） |
| **UAC/セキュア画面** | ✅ SYSTEM権限で可能 | ❌ 不可 |
| **クロスGPU** | ⚠️ 同一GPUが必要 | ✅ クロスGPU対応 |
| **UWPアプリ対応** | ❌ Win32のみ | ✅ UWP/Win32両対応 |
| **権限要件** | 管理者権限推奨 | 標準ユーザー可 |

### このプロジェクトでの優先度

- **レイテンシ最小化が最優先** → DDAを引き続きデフォルトとする
- **WGCは代替手段として実装** → 設定で切り替え可能に
- **モニター単位キャプチャ** → DDAと同様の動作を目指す

## アーキテクチャ設計

### レイヤ責務（既存設計との整合）

```
Domain層:
  - CapturePort trait (既存、変更なし)
  - CaptureSource enum に Wgc を追加

Infrastructure層:
  - WgcCaptureAdapter: CapturePort を実装
  - 共通処理は capture/common.rs を再利用

Application層:
  - 変更なし（CapturePort経由で透過的に使用）

Presentation層 (main.rs):
  - CaptureSource::Wgc の分岐を追加
```

### ファイル構成

```
src/infrastructure/capture/
  mod.rs         # WgcCaptureAdapter の re-export 追加
  common.rs      # 共通処理（変更なし）
  dda.rs         # 既存DDA実装（変更なし）
  spout.rs       # 既存Spout実装（変更なし）
  wgc.rs         # 新規: WGC実装
```

### 依存クレート候補

| クレート | 説明 | 評価 |
|----------|------|------|
| `windows-capture` | WGC のハイレベルラッパー | ⭐ 推奨: 簡潔なAPI、活発にメンテナンス |
| `windows` crate (直接) | Windows RS の直接使用 | 選択肢: 細かい制御可能だが実装量増加 |

**推奨**: `windows-capture` クレートを使用（失敗時は `windows` crate 直接使用に切り替え）

## 実装設計

### WgcCaptureAdapter 構造

```rust
// 概念的な構造（実装時に調整）
pub struct WgcCaptureAdapter {
    // WGCキャプチャセッション（windows-capture使用時）
    capture: GraphicsCaptureSession,  // または同等のハンドル
    
    // D3D11デバイス（GPU→CPU転送用）
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    
    // ステージングテクスチャ管理（共通モジュール使用）
    staging_manager: StagingTextureManager,
    
    // デバイス情報
    device_info: DeviceInfo,
    
    // 再初期化用の設定保持
    monitor_index: usize,
}
```

### CapturePort 実装方針

```rust
impl CapturePort for WgcCaptureAdapter {
    fn capture_frame_with_roi(&mut self, roi: &Roi) -> DomainResult<Option<Frame>> {
        // 1. ROIを画面中心に動的配置（DDA/Spoutと同様）
        // 2. WGCからフレーム取得
        // 3. 共通モジュールでROI領域をステージングへコピー
        // 4. 共通モジュールでGPU→CPU転送
        // 5. Frame構築して返す
    }
    
    fn reinitialize(&mut self) -> DomainResult<()> {
        // WGCセッションの再作成
    }
    
    fn device_info(&self) -> DeviceInfo {
        self.device_info.clone()
    }
}
```

### エラーマッピング（WGC → Domain）

| WGCエラー | DomainError | 説明 |
|-----------|-------------|------|
| フレーム更新なし | `Ok(None)` | タイムアウトとして正常扱い |
| セッション終了/無効化 | `DeviceNotAvailable` | 再接続で復旧可能 |
| 致命的エラー | `ReInitializationRequired` | インスタンス再作成必要 |

## 設定変更

### config.toml の拡張

```toml
[capture]
source = "wgc"  # "dda" | "spout" | "wgc"
monitor_index = 0
```

### CaptureSource enum の拡張

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptureSource {
    #[default]
    Dda,
    Spout,
    Wgc,  // 新規追加
}
```

## 未決定事項・検討ポイント

### 1. クレート選定（決定済み）

**問題**: `windows-capture` vs `windows` crate 直接使用

**決定**: `windows` crate v0.57 を直接使用

**根拠** (Phase 1検証結果):
- `windows-capture` v1.5 は `windows` v0.61 に依存
- 既存プロジェクトは `windows` v0.57 を使用（`win_desktop_duplication` の要求）
- バージョン不整合によりコンパイル不可
- 直接実装により、細かい制御と最適化が可能
- 既存コード（DDA/Spout）への影響なし

**実装アプローチ**:
- `Graphics.Capture.GraphicsCaptureItem` でモニター選択
- `Graphics.Capture.Direct3D11CaptureFramePool` でフレームプール作成
- `FrameArrived` イベントハンドラでフレーム取得
- Arc<Mutex> パターンで最新フレームを保持

### 2. フレーム取得方式（優先度: 高）

**問題**: WGCはコールバックベースのAPI設計

- DDAの `acquire_next_frame_now()` のような同期取得ができない
- コールバックでフレームを受け取り、チャネル経由でスレッドに渡す必要がある可能性

**検討オプション**:
- (A) コールバック内でフレーム処理まで完結
- (B) コールバックでフレームを保存、`capture_frame_with_roi` で最新を取得
- (C) `windows-capture` の同期APIを使用（存在すれば）

**決定方針**: Phase 1 で調査・プロトタイプ作成

### 3. GPU/デバイス整合性（優先度: 中）

**問題**: WGCが使用するD3D11デバイスとステージング用デバイスの整合

- WGCが作成するデバイスを取得できるか？
- 自前デバイスを渡せるか？
- クロスデバイスコピーが必要になる場合のパフォーマンス影響

**決定方針**: `windows-capture` のAPIを調査して決定

### 4. レイテンシ計測（優先度: 中）

**問題**: DDAとWGCの実際のレイテンシ差の定量化

- 「1コピー削減」がどの程度の効果か？
- ROIコピー時のパフォーマンス差

**決定方針**: Phase 2 で比較ベンチマークを実施

### 5. Feature Flag（決定済み）

**問題**: WGCサポートをオプションにするか？

- `windows-capture` は追加の依存（Windows API）を持つ
- DDAのみでほとんどのユースケースをカバー

**決定**: (A) デフォルトで含める（DDA/Spoutと同様）

**根拠**:
- ユーザーは `config.toml` の `capture.source` で切り替え可能
- 実装・ドキュメント管理がシンプル
- DDA/Spoutと設計整合性が取れる

### 6. 最小Windowsバージョン（決定済み）

**問題**: Windows 10 1803以降が必要

- 現在のプロジェクトは最小バージョンを明示していない
- WGC追加で実質 Win10 1803+ が必要に

**決定**: REQUIREMENTS.md に最小バージョンを明記

**根拠**:
- WGCはWin10 1803+でのみ動作
- DDAはWin8+、Spoutも幅広く動作
- 実用上、Win10 1803+ は十分に普及している

## 実装フェーズ

### Phase 1: 調査・プロトタイプ（推定: 1-2日）

**目標**: WGC実装の実現可能性確認

1. `windows-capture` クレートの動作確認
   - Cargo.toml に依存追加
   - 最小限のモニターキャプチャ実装
   - フレーム取得方式の確認（同期/非同期）

2. GPU/デバイス整合性の調査
   - D3D11デバイスの取得方法
   - ステージングテクスチャとの互換性

3. 判断ポイント
   - `windows-capture` で要件を満たせるか？
   - 直接実装が必要か？

**成果物**: 技術検証レポート、クレート選定の最終決定

### Phase 2: 基本実装（推定: 2-3日）

**目標**: CapturePort を実装した動作する WgcCaptureAdapter

1. `WgcCaptureAdapter` 基本構造の実装
   - モニター列挙と選択
   - キャプチャセッション開始
   - フレーム取得

2. 共通モジュールとの統合
   - `StagingTextureManager` の使用
   - `copy_roi_to_staging` / `copy_texture_to_cpu` の使用

3. エラーハンドリング
   - WGCエラー → DomainError のマッピング
   - 再初期化ロジック

4. 設定の拡張
   - `CaptureSource::Wgc` の追加
   - `config.toml` 対応

**成果物**: 動作するWGCキャプチャ、統合テスト

### Phase 3: 統合・最適化（推定: 1-2日）

**目標**: 本番品質への仕上げ

1. パイプラインへの統合
   - `main.rs` での分岐追加
   - 復旧ロジックの動作確認

2. パフォーマンス計測
   - DDAとのレイテンシ比較
   - `performance-timing` フィーチャーでの計測追加

3. ドキュメント更新
   - このファイルの「実装済み」セクション追加
   - ROADMAP.md の更新
   - README.md / config.toml.example の更新

**成果物**: 本番リリース可能なWGC実装

### Phase 4: 検証・安定化（推定: 1日）

**目標**: 品質保証

1. エッジケーステスト
   - モニター切断/再接続
   - 解像度変更
   - スリープ/復帰

2. 長時間動作テスト
3. ドキュメント最終化

**成果物**: 安定したWGC実装、完全なドキュメント

## リスクと対策

| リスク | 影響 | 対策 |
|--------|------|------|
| `windows-capture` が要件を満たさない | Phase 1 遅延 | `windows` crate 直接実装に切り替え |
| コールバックAPIがレイテンシに影響 | パフォーマンス低下 | 最新フレーム保持方式で対応、レイテンシ計測で評価 |
| GPUデバイス不整合 | クロスデバイスコピー必要 | 共有テクスチャまたはGPUコピーで対応 |
| WGCがDDAより遅い場合 | 採用価値低下 | 代替手段として位置付け（デフォルトはDDA維持） |

## 参考資料

- [Windows Graphics Capture 公式ドキュメント](https://learn.microsoft.com/en-us/windows/uwp/audio-video-camera/screen-capture)
- [windows-capture crate](https://crates.io/crates/windows-capture)
- [robmikh/Win32CaptureSample](https://github.com/robmikh/Win32CaptureSample) - WGC Win32サンプル
- [DDA vs WGC 議論](https://github.com/robmikh/Win32CaptureSample/issues/24)

## 更新履歴

| 日付 | 内容 |
|------|------|
| 2026-01-13 | 初版作成（設計方針、実装計画、未決定事項の洗い出し） |
| 2026-01-13 | 未決定事項5,6を決定、Phase 1開始（プロトタイプ実装完了、ビルド検証は保留） |
| 2026-01-13 | Phase 1完了：windows-captureバージョン不整合により直接実装に切り替え決定 |

---

## Phase 1 進捗状況

**ステータス**: ✅ 完了

### 完了項目
- ✅ `windows-capture` クレートの評価
- ✅ バージョン不整合の発覚
- ✅ 直接実装への切り替え決定
- ✅ フレーム取得方式の設計（Arc<Mutex> パターン）
- ✅ Phase 1技術検証レポート完成

### 主要な判断
**問題**: `windows-capture` (windows 0.61) と `win_desktop_duplication` (windows 0.57) のバージョン不整合

**決定**: `windows` crate v0.57 を直接使用してWGCを実装

**影響**:
- 実装量が増加（許容範囲内）
- 既存コード（DDA/Spout）への影響なし
- より細かい制御と最適化が可能

### 次のステップ

**Phase 2**: 基本実装（✅ 完了）

**実装結果**:
- ✅ windows features 追加（Graphics_Capture等8個のfeature）
- ✅ `IGraphicsCaptureItemInterop` COMインターフェース定義
- ✅ モニター列挙 (`enumerate_monitors`)
- ✅ GraphicsCaptureItem作成 (`create_capture_item_for_monitor`)
- ✅ D3D11デバイス作成 (`create_d3d11_device`)
- ✅ Direct3D11CaptureFramePool作成（バッファサイズ2で低レイテンシ化）
- ✅ FrameArrivedイベントハンドリング
- ✅ capture_frame_with_roi実装（ROI処理対応）
- ✅ ビルド成功確認
- ✅ **動作確認完了** - フレームキャプチャ成功、処理レイテンシ**1ms**達成

**パフォーマンス結果** (実装テスト済み):
- フレームレート: 60+ FPS
- 処理レイテンシ: **0-1ms** (目標達成)
- キャプチャサイズ: 460x240 ROI
- 解像度: 1920x1080

**次のタスク (Phase 3-4)**:
- Phase 3: 最適化とDDA比較ベンチマーク
- Phase 4: エラーハンドリング強化と本番環境テスト

詳細は [WGC_PHASE1_REPORT.md](WGC_PHASE1_REPORT.md) を参照。

