# ドキュメント

プロジェクトの設計方針についてはAGENTS.mdとdocs/を参照し、必要であれば実際のコードも読む。また、技術的な情報はContext7とWebから集めてください。
このプロジェクトは低レイテンシを重視しています。

## パフォーマンスヒント

ビルド時にPATHに`\third_party\llvm\bin`を追加する必要があります。

レイテンシ最優先なら “YOLO11n（不十分なら s）+ TensorRT FP16” が第一候補。

## OBS Studio と Spoutプラグイン
[OBS Studio 31.1.2](https://github.com/obsproject/obs-studio/releases/tag/31.1.2)

[Spout2 Plugin for OBS](https://github.com/Off-World-Live/obs-spout2-plugin/releases/tag/1.10.0)



## キャプチャ部分のボトルネック
ddaについても、spoutについても、capture_frame_with_roi関数が遅い理由としては、GPU→CPU 転送（Map/Unmap）とその同期が大きい
**ただこれは関数のみの速度についてのみ議論しているので、リングバッファ実装は1,2フレーム過去の画面を処理するため、画面生成から本プロジェクトでの出力までの総レイテンシ画像化します。**
レイテンシ、安定性も両立するには処理をすべてGPU上で完結させるしかない

### Map(D3D11_MAP_READ) が強制同期を起こしている
```Rust
spout_context.Map(
    &staging_tex,
    0,
    D3D11_MAP_READ,
    0,
    Some(&mut mapped),
)
```
これは GPUパイプラインを完全に止めます。
- CopySubresourceRegion は非同期
- しかし Map(READ) は
  - その staging_tex に対する GPU 処理が終わるまで CPU を待たせる
- Spout の受信・コピー直後なので、毎フレーム確実に stall

実測ではここだけで：
- 数百 µs ～ 数 ms
- フレームレート依存でスパイクが出る
この関数が遅い最大要因はここです。

## 対策
### staging をリングバッファ化
- 2～3 枚の staging_tex
- フレーム N：
  - GPU: Copy → staging[N]
  - CPU: Map → staging[N-1]

### Map を別スレッドへ
- Capture スレッド：Acquire + Copy
- Readback スレッド：Map + memcpy

完全非同期ではないが、フレーム落ちが緩和