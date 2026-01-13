# Phase 1: WGC技術検証レポート（進行中）

## 実施日
2026-01-13

## ステータス
✅ **完了** - 重要な判断を実施

## 目的
Windows Graphics Capture (WGC) 実装の実現可能性確認

## Phase 1の最終結論

### 🔴 重大な問題: windowsクレートのバージョン不整合

**問題の詳細**:
- `windows-capture` v1.5.0 は `windows` v0.61.3 に依存
- 既存プロジェクトは `windows` v0.57.0 を使用（`win_desktop_duplication` の要求）
- 2つの異なる `windows` クレートバージョンが共存できず、コンパイルエラー

**エラーの種類**:
```
error: multiple versions of crate `windows` used
  - windows v0.57.0 (win_desktop_duplication経由)
  - windows v0.61.3 (windows-capture経由)
```

### 🎯 Phase 1の判断: 直接実装へ切り替え

**理由**:
1. **既存コードの安定性優先**: DDA/Spoutは windows 0.57 で動作しており、変更リスクが高い
2. **API制御の必要性**: `windows-capture` のラッパーでは細かい制御が難しい可能性
3. **将来の柔軟性**: 直接実装により、プロジェクト固有の最適化が可能

**選択**: `windows` crate v0.57 を直接使用してWGCを実装する

## 実施内容

### 1. クレート選定の試行と撤回
- ✅ `windows-capture` v1.5.0 の調査完了
- ✅ API構造の理解（GraphicsCaptureApiHandler trait）
- ❌ バージョン不整合により使用断念

### 2. プロトタイプ実装の試行
- ✅ `WgcCaptureAdapter` の基本構造を設計
- ✅ コールバックパターン（Arc<Mutex>）の設計完了
- ❌ コンパイル不可のため実装中断

### 3. 代替案の決定
**選択肢A**: windows 0.61 へ全体をアップグレード
- ⚠️ リスク: DDA/Spoutの動作に影響
- ⚠️ win_desktop_duplication が 0.61 に対応していない可能性

**選択肢B**: windows 0.57 で直接実装 ✅ **採用**
- ✅ 既存コードへの影響なし
- ✅ 完全な制御と最適化が可能
- ⚠️ 実装量が増加（許容範囲内）

## 発見事項（修正版）

### 1. windows-capture の制約
- Windows Graphics Capture の高レベルラッパー
- 便利だが、バージョン依存性が厳しい
- プロジェクトの既存依存関係と競合する可能性

### 2. windows crate での直接実装の実現可能性
**調査結果**:
- `windows` v0.57 は WGC API をサポート
- 必要な機能:
  - `Windows.Graphics.Capture.GraphicsCaptureItem`
  - `Windows.Graphics.Capture.Direct3D11CaptureFramePool`
  - `Windows.Graphics.DirectX.Direct3D11.IDirect3DSurface`

**実装の複雑性**: 中程度
- WinRT API の使用が必要（COM より複雑）
- コールバック管理を手動実装
- ただし、DDA実装の経験を活かせる

### 3. 必要な windows crate 機能
```toml
windows = { version = "0.57", features = [
    "Win32_Graphics_Direct3D11",
    "Graphics_Capture",
    "Graphics_DirectX_Direct3D11",
    "Foundation",
    # 既存機能...
] }
```

## 未決定事項の検討結果

### 1. クレート選定（優先度: 高） ✅ **決定**

**決定**: `windows` crate v0.57 を直接使用

**根拠**:
- `windows-capture` はバージョン不整合により使用不可
- `windows` v0.57 は WGC API を完全にサポート
- 既存コード（DDA/Spout）との整合性を維持

### 2. フレーム取得方式（優先度: 高） ⏭️ **Phase 2で検討**

**検討事項**:
- WGC の `FrameArrived` イベントハンドリング
- Direct3D11CaptureFramePool の管理
- 最新フレームの保持方法（Arc<Mutex> パターンは有効）

**Phase 2での実装方針**:
- `Direct3D11CaptureFramePool::CreateFreeThreaded` を使用
- イベントハンドラで最新フレームを Arc<Mutex> に保存
- `capture_frame_with_roi` で同期的に取得

### 3. GPU/デバイス整合性（優先度: 中） ⏭️ **Phase 2で検証**

**Phase 1の理解**:
- WGC は IDirect3DSurface を返す
- ID3D11Texture2D への変換が必要
- 既存の StagingTextureManager を再利用可能

### 4. レイテンシ計測（優先度: 中） ⏭️ **Phase 3で実施**

**Phase 2実装完了後に**: DDAとの比較ベンチマークを実施

## Phase 1の成果物

### 文書
- ✅ 設計方針ドキュメント（INFRASTRUCTURE_WGC.md）
- ✅ Phase 1技術検証レポート（本ファイル）
- ✅ 実装判断の記録

### コード
- ⚠️ プロトタイプ実装（削除済み）- バージョン不整合により中断
- ✅ Cargo.toml の準備（windows-capture コメントアウト）

### 学び
1. ✅ サードパーティラッパーの依存関係リスクを確認
2. ✅ `windows` crate での直接実装の実現可能性を確認
3. ✅ WGC API の基本構造を理解

## 次のアクション（Phase 2への移行）

### Phase 2: 基本実装

**目標**: windows crate v0.57 を使用してWGCを直接実装

**タスク**:
1. 必要な windows features を Cargo.toml に追加
   ```toml
   "Graphics_Capture",
   "Graphics_DirectX_Direct3D11",
   "Graphics_Capture_Direct3D11CaptureFramePool",
   "Graphics_Capture_GraphicsCaptureItem",
   ```

2. `wgc.rs` を再作成（直接実装版）
   - `GraphicsCaptureItem::CreateFromMonitor` でモニター選択
   - `Direct3D11CaptureFramePool` でフレームプール作成
   - `FrameArrived` イベントハンドリング
   - Arc<Mutex> パターンでフレーム保持

3. CapturePort trait の実装
   - `capture_frame_with_roi` の完全実装
   - ROI コピーと GPU→CPU転送
   - エラーマッピング

4. 動作確認
   - 初期化テスト
   - フレーム取得テスト
   - ROI処理の検証

**推定期間**: 3-4日（Phase 1の学びを活かして）

## 最終結論

### 実現可能性: ✅ **高い（直接実装で）**

**Phase 1で確認できたこと**:
1. WGC API は windows 0.57 で利用可能
2. DDA/Spoutとの共存が可能
3. 直接実装により細かい制御が可能
4. コールバックパターンの設計が妥当

**Phase 2へ進行を推奨**:
- 直接実装により、より良い制御と最適化が可能
- プロジェクトの安定性を維持しながら実装できる
- DDA/Spoutの実装経験を活かせる

### 重要な学び

**依存関係の管理**:
- サードパーティラッパーは便利だが、バージョン制約に注意
- 低レベルAPIの直接使用により、長期的な安定性が向上
- プロジェクト全体の依存関係整合性を優先すべき

## 更新履歴

| 日付 | 内容 |
|------|─------|
| 2026-01-13 | Phase 1 開始、プロトタイプ実装、ネットワーク問題により検証保留 |
| 2026-01-13 | Phase 1 完了、windows-capture バージョン不整合発覚、直接実装へ切り替え決定 |

## 更新履歴

| 日付 | 内容 |
|------|------|
| 2026-01-13 | Phase 1 開始、プロトタイプ実装、ネットワーク問題により検証保留 |
