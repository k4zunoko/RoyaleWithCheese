# Infrastructure層: WGC (Windows Graphics Capture) 実装

このドキュメントは、Windows Graphics Capture API を使用したキャプチャアダプタ (`WgcCaptureAdapter`) の実装方針と制約をまとめます。

## 概要

### 目的

DDAキャプチャの代替として、Windows Graphics Capture (WGC) によるキャプチャ機能です。

**利点**:
- 標準ユーザー権限で動作（管理者権限不要）
- クロスGPU対応（異なるGPUアダプタ間でも動作）
- ウィンドウ単位/モニター単位キャプチャが可能
- 処理レイテンシ **0-1ms** を達成

### WGCとは

Windows 10 バージョン1803以降で利用可能な画面キャプチャAPIです。Desktop Duplication API (DDA) と同じ基盤技術を使用しつつ、より柔軟な設計になっています。

## DDA vs WGC 比較

| 項目 | DDA | WGC |
|------|-----|-----|
| **最小Windows** | Windows 8 | Windows 10 1803 |
| **レイテンシ** | 非常に低い | 低い（0-1ms） |
| **ウィンドウ単位** | ❌ 不可 | ✅ 可能 |
| **モニター単位** | ✅ 可能 | ✅ 可能 |
| **DirtyRect** | ✅ 提供 | ❌ 未提供 |
| **セキュア画面** | ⚠️ SYSTEM権限で可能 | ❌ 不可 |
| **クロスGPU** | ❌ 同一GPU必須 | ✅ 対応 |
| **権限要件** | 管理者権限推奨 | 標準ユーザー可 |

## アーキテクチャ

```
Domain (CapturePort trait)
    │ trait実装
    ├─ DdaCaptureAdapter
    ├─ SpoutCaptureAdapter
    └─ WgcCaptureAdapter
```

**Clean Architectureの原則**:
- Domain層の`CapturePort` traitは不変
- Infrastructure層に`WgcCaptureAdapter`を追加
- Application層はDIで切り替え
- 設定: `config.toml`の`capture.source`で選択

## 実装構成

### ファイル構成

```
src/infrastructure/capture/
├── mod.rs            # CaptureSource enum、エクスポート
├── dda.rs            # DDA実装
├── spout.rs          # Spout実装
├── wgc.rs            # WGC実装
└── common.rs         # 共通処理（ROI切り出し、GPU→CPU転送）
```

### 依存クレート

**採用**: `windows` crate v0.57 を直接使用

**理由**:
- 既存の`win_desktop_duplication`が`windows` v0.57に依存
- `windows-capture`クレート（v1.5）は`windows` v0.61に依存し、バージョン不整合
- 直接実装により細かい制御と最適化が可能

### WgcCaptureAdapter の責務

1. **WGCセッション管理**: GraphicsCaptureItem、FramePool、Session の作成と管理
2. **フレーム取得**: FrameArrivedイベントで非同期に受信、Arc<Mutex>で最新フレームを保持
3. **ROI処理**: 共通モジュール（`common.rs`）を使用してROI切り出し
4. **GPU→CPU転送**: ステージングテクスチャ経由でフレームデータを取得
5. **再初期化**: セッション再作成でリカバリ
6. **ROI動的中心配置**: 毎フレーム受信テクスチャのサイズから中心位置を計算（~10ns未満）

### フレーム取得の仕組み

**WGCの特性**: コールバックベースのAPI

**実装アプローチ**:
```rust
// 1. FrameArrivedイベントハンドラで最新フレームを保存
let latest_frame = Arc::new(Mutex::new(None));
let latest_frame_clone = latest_frame.clone();

frame_pool.FrameArrived(&TypedEventHandler::new(move |pool, _| {
    if let Some(frame) = pool.TryGetNextFrame().ok() {
        *latest_frame_clone.lock().unwrap() = Some(frame);
    }
    Ok(())
}))?;

// 2. capture_frame_with_roiで同期的に取得
fn capture_frame_with_roi(&mut self, roi: &Roi) -> DomainResult<Option<Frame>> {
    let frame = self.latest_frame.lock().unwrap().take();
    // ROI処理...
}
```

**設計判断**:
- フレームプールバッファサイズ: **2**（低レイテンシ優先）
- 最新フレームのみを保持（古いフレームは破棄）
- DDAと同様の同期的インターフェース（`CapturePort` trait）

### ROI動的中心配置

**DDA/Spoutと同じ方針**: 毎フレーム中心位置を動的計算

**利点**:
- 送信者の解像度が変わっても常に中心からキャプチャ
- 設定ファイルの汎用性（異なる環境で同じ設定を使用可能）
- 低レイテンシ維持（計算コスト~10ns未満）

### 設定例（config.toml）

```toml
[capture]
source = "wgc"          # "dda" | "spout" | "wgc"
monitor_index = 0       # WGCではモニター単位キャプチャに使用
```

## エラーマッピング

| WGCエラー | DomainError | 扱い |
|-----------|-------------|------|
| フレーム更新なし | `Ok(None)` | タイムアウト（正常） |
| セッション終了 | `DeviceNotAvailable` | 再接続可能 |
| 致命的エラー | `ReInitializationRequired` | インスタンス再作成必要 |

## 既知の制限

1. **Windows 10 1803以降が必須**: それ以前のWindowsでは動作不可
2. **DirtyRect未対応**: WGC APIがDirtyRect情報を提供しない
3. **セキュア画面**: UAC/ロック画面は取得不可（Windowsの制約）
4. **フレームレート**: モニターのリフレッシュレートに依存

## パフォーマンス

**実測値**（2026-01-13実装テスト）:
- フレームレート: 60+ FPS
- 処理レイテンシ: **0-1ms**
- キャプチャサイズ: 460×240 ROI
- 解像度: 1920×1080

**DDAとの比較**: Phase 3で詳細ベンチマーク実施予定

## 技術的詳細

### 使用するWinRT API

- `Windows.Graphics.Capture.GraphicsCaptureItem`
- `Windows.Graphics.Capture.Direct3D11CaptureFramePool`
- `Windows.Graphics.Capture.GraphicsCaptureSession`
- `Windows.Graphics.DirectX.Direct3D11.IDirect3DSurface`

### COMインターフェース

```rust
// IGraphicsCaptureItemInterop: HMONITORからGraphicsCaptureItemを作成
#[repr(C)]
#[interface("3628E81B-3CAC-4C60-B7F4-23CE0E0C3356")]
unsafe interface IGraphicsCaptureItemInterop: IUnknown {
    fn CreateForMonitor(
        monitor: HMONITOR,
        riid: *const GUID,
        result: *mut *mut c_void,
    ) -> HRESULT;
}
```

## 参考リンク

- [Windows Graphics Capture 公式ドキュメント](https://learn.microsoft.com/en-us/windows/uwp/audio-video-camera/screen-capture)
- [robmikh/Win32CaptureSample](https://github.com/robmikh/Win32CaptureSample) - WGC Win32サンプル
- [DDA vs WGC 議論](https://github.com/robmikh/Win32CaptureSample/issues/24)

---

**更新履歴**:
- 2026-01-13: 初版作成（実装完了後）