# Infrastructure層: Capture実装（DDA）

このドキュメントは、現在実装されている **DDA (Desktop Duplication API)** キャプチャ（`DdaCaptureAdapter`）の実装概要と、Application層との境界・エラーマッピングをまとめます。

注意: ここには「実装済みの内容のみ」を記載します（未実装のキャプチャ方式や将来構想は書きません）。

## 対象範囲

- 実装: `DdaCaptureAdapter`（`CapturePort` 実装）
- 主な関心: ROI付きキャプチャ、GPU→CPU転送、エラーのDomain変換、再初期化

## 関連ファイル

```
src/infrastructure/capture/
  mod.rs
  dda.rs
```

## レイヤ責務（要点）

- Infrastructure: Windows/DDA API 呼び出し・データ変換・`DomainError`への変換、`reinitialize()` の具体実装
- Application: どのエラーでいつ `reinitialize()` するか等の復旧ポリシー
- Domain: `CapturePort`/`Frame`/`Roi`/`DomainError` といった抽象

## `DdaCaptureAdapter` の内部状態

`DdaCaptureAdapter` はDDA本体と、ROIコピー用の D3D11 リソース（ステージングテクスチャ含む）を保持します。

- `dupl: DesktopDuplicationApi`
- `output: Display`
- `device_info: DeviceInfo`
- `device/context: ID3D11Device4 / ID3D11DeviceContext4`
- `staging_tex` と `staging_size`（ROIサイズが同じなら再利用）

## キャプチャフロー（`capture_frame_with_roi`）

実装は `src/infrastructure/capture/dda.rs` の `CapturePort for DdaCaptureAdapter` を参照します。

1. ROIを検証・クランプ
   - `clamp_roi()` で画面外アクセスを防止
2. フレーム取得
   - `dupl.acquire_next_frame_now()` を使用
   - フレーム更新がない場合は「タイムアウト」として扱い、`Ok(None)` を返す
3. GPU上でROIだけコピー
   - `CopySubresourceRegion()` でROI領域のみをステージングテクスチャへコピー
4. GPU→CPU転送
   - `Map(D3D11_MAP_READ)` → RowPitch を考慮して `Vec<u8>` に詰め替え
5. `Frame` を構築して返す
   - 現状 `dirty_rects` は空 `vec![]`（将来の最適化余地）

## VSync待機について

`wait_for_vsync()` は実装されていますが、現在は **レイテンシ最小化のため未使用**です。

- 実際のキャプチャは `acquire_next_frame_now()` のみを使用します
- VSync待機を入れると最短でも次のVBlankまで待つため、遅延が増える可能性があります

## エラーマッピング（DDA → Domain）

`win_desktop_duplication` 側のエラー型が直接公開されていないため、実装では `{:?}` 文字列を見て分類しています。

- `Timeout` を含む → `Ok(None)`（フレーム更新なしとして正常扱い）
- `AccessLost` / `AccessDenied` を含む → `Err(DomainError::DeviceNotAvailable)`
- それ以外 → `Err(DomainError::ReInitializationRequired)`

この結果、排他的フルスクリーン等で `AccessLost` が出るケースでは、Application層が復旧ポリシーに従って `reinitialize()` を呼ぶ前提になります。

## 再初期化（`reinitialize`）

`reinitialize()` は、元の `adapter_idx` / `output_idx` で `DesktopDuplicationApi` を作り直し、
ディスプレイモードを再取得して `device_info` を更新します。

- `DuplicationApiOptions { skip_cursor: true }` を再適用
- ステージングテクスチャはサイズ変化の可能性があるためクリア（次回キャプチャで再確保）

## テスト

`src/infrastructure/capture/dda.rs` には `#[ignore]` のテストがあります（GPU/権限/環境依存のため通常は無視されます）。

- 初期化テスト
- 単発キャプチャ
- 複数フレームキャプチャ

## 既知の制限（実装由来）

- テストや実行の同時並行によっては DDA 初期化が失敗する可能性があります（テスト内コメント参照）
- セキュアな画面（UAC/ロック画面等）はWindowsの制約で取得できない可能性があります

## 今後の改善余地（実装コメントに基づく）

- DirtyRect の活用（現状 `dirty_rects` は空）
- エラー判定の堅牢化（文字列分類のため）

## 参考

- Desktop Duplication API: https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/desktop-dup-api
- win_desktop_duplication (docs.rs): https://docs.rs/win_desktop_duplication/

## 更新履歴

| 日付 | 内容 |
|------|------|
| 2026-01-05 | 未実装仕様と推測記述を除去し、現行実装（`dda.rs`）に合わせて全面整理 |

