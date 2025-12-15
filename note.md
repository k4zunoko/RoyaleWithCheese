# ドキュメント

プロジェクトの設計方針についてはAGENT.mdとdocs/を参照し、必要であれば実際のコードも読む。また、技術的な情報はContext7とWebから集めてください。
このプロジェクトは開発途中で低レイテンシを重視しています。

## パフォーマンスヒント

dda.rs 237行目のコメントアウトを外すと、VSync待機が有効になります。
```rust
// self.wait_for_vsync()?;
```

ビルド時にPATHに`\third_party\llvm\bin`を追加する必要があります。
実行時にPATHに`\third_party\opencv\build\x64\vc16\bin`を追加する必要があります。

レイテンシ最優先なら “YOLO11n（不十分なら s）+ TensorRT FP16” が第一候補。