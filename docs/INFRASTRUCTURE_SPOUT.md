````markdown
# Infrastructure層: Spout受信実装（SpoutCaptureAdapter）

このドキュメントは、**Spout DX11テクスチャ受信**による代替キャプチャ実装の設計をまとめます。

## 概要

### 目的

DDAキャプチャの代替として、Spout送信されたDX11テクスチャを受信する機能です。

**利点**:
- DDAよりも低遅延（ゼロコピーに近いGPU転送）
- 管理者権限不要
- 排他的フルスクリーンの制約を回避
- アプリケーション間のテクスチャ共有に対応

### Spoutとは

Windows環境でDirectX 11テクスチャをプロセス間で共有するフレームワーク。GPU上で効率的な転送が可能ですが、送受信は同一GPUアダプタである必要があります。

## アーキテクチャ

```
Domain (CapturePort trait)
    │ trait実装
    ├─ DdaCaptureAdapter
    └─ SpoutCaptureAdapter
```

**Clean Architectureの原則**:
- Domain層の`CapturePort` traitは不変
- Infrastructure層に`SpoutCaptureAdapter`を追加
- Application層はDIで切り替え
- 設定: `config.toml`の`capture.source`で選択

## 実装構成

### ファイル構成

```
src/infrastructure/capture/
├── mod.rs            # CaptureSource enum、エクスポート
├── dda.rs            # DDA実装
├── spout.rs          # SpoutCaptureAdapter実装
└── spout_ffi.rs      # FFIバインディング（spoutdx-ffi）
```

### FFI API（spoutdx_ffi）

`third_party/spoutdx_ffi` を使用。主要なAPI：

**ライフサイクル**:
- `spoutdx_receiver_create()` / `destroy()`

**初期化**:
- `spoutdx_receiver_open_dx11(handle, ID3D11Device*)`
- `spoutdx_receiver_set_sender_name(handle, name)` - 送信者指定（NULL=自動）

**受信（推奨: 内部テクスチャ方式）**:
- `spoutdx_receiver_receive(handle)` - 内部テクスチャへ受信
- `spoutdx_receiver_get_received_texture(handle)` - 内部テクスチャ取得（ID3D11Texture2D*）
- `spoutdx_receiver_get_dx11_context(handle)` - SpoutDX側のD3D11コンテキスト取得
- `spoutdx_receiver_get_sender_info(handle, SpoutDxSenderInfo*)` - サイズ/フォーマット取得
- `spoutdx_receiver_is_frame_new(handle)` - 更新チェック

**旧方式（非推奨）**:
- `spoutdx_receiver_receive_texture(handle, ID3D11Texture2D*)` - 外部テクスチャへ直接受信

### SpoutCaptureAdapter の責務

1. **D3D11デバイス管理**: 自前でデバイス・コンテキストを作成（アダプタ整合性のため）
2. **内部テクスチャ受信**: `spoutdx_receiver_receive` → `get_received_texture` で受信
3. **SpoutDXコンテキスト使用**: ROIコピー時は `spoutdx_receiver_get_dx11_context` で取得したコンテキストを使用
4. **ステージングテクスチャ管理**: 送信者のフォーマットに合わせたステージングテクスチャ（ROI切り出し用）
5. **フレーム受信**: 新しいフレームをポーリング、ROI切り出し
   - **ROI動的中心配置**: 毎フレーム受信テクスチャのサイズから中心位置を計算（~10ns未満）
   - 送信者の解像度が変わっても自動的に中心からキャプチャ
6. **再初期化**: レシーバー再作成でリカバリ

### ROI動的中心配置の利点（Spoutの場合）

- **送信者変更に自動追従**: 解像度が異なる送信者に切り替わっても常に中心からキャプチャ
- **設定ファイルの汎用性**: 異なる送信者で同じconfig.tomlを使用可能
- **低レイテンシ維持**: 計算コスト~10ns未満（減算2回、除算2回）で影響なし

### 設定例（config.toml）

```toml
[capture]
source = "spout"               # "dda" または "spout"
spout_sender_name = "MyGame"   # 送信者名（省略時は自動選択）
```

## エラーマッピング

| SpoutDxResult | DomainError | 扱い |
|--------------|-------------|------|
| `OK` | - | 成功 |
| `ERROR_NOT_CONNECTED` | `Ok(None)` | 送信者未接続（正常） |
| `ERROR_NULL_HANDLE` / `NULL_DEVICE` | `ReInitializationRequired` | ハンドル無効 |
| `ERROR_INIT_FAILED` | `Initialization` | 初期化失敗 |
| `ERROR_RECEIVE_FAILED` | `DeviceNotAvailable` | 受信失敗（リトライ可能） |
| `ERROR_INTERNAL` | `Capture` | 内部エラー |

## DDAとSpoutの比較

| 項目 | DDA | Spout |
|------|-----|-------|
| ソース | 画面全体 | 特定アプリの送信テクスチャ |
| 遅延 | 1フレーム程度 | ゼロコピー（最小） |
| 解像度 | モニタ解像度 | 送信者設定に依存 |
| 権限 | 管理者権限が必要な場合あり | 不要 |
| 排他フルスクリーン | 対応（再初期化必要） | N/A（アプリ側対応次第） |
| フォーマット | BGRA固定 | 送信者設定に依存 |
| GPU制約 | なし | 送受信は同一GPUアダプタ必須 |

## 既知の制限

1. **同一GPUアダプタ制約**: 送信側・受信側は同じGPUアダプタが必要
2. **フォーマット依存**: 送信者のテクスチャフォーマットに依存
3. **リフレッシュレート不明**: Spoutではリフレッシュレート情報なし
4. **送信者依存**: 送信側がSpout対応している必要がある

## 参考リンク

- [Spout2 GitHub](https://github.com/leadedge/Spout2)
- [Spout DirectX Texture Sharing](https://spout.zeal.co/)

---

**更新履歴**:
- 2026-01-08: 初版作成
- 2026-01-08: 実装完了後に簡潔化

````
