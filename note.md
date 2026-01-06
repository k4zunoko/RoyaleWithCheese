# ドキュメント

プロジェクトの設計方針についてはAGENTS.mdとdocs/を参照し、必要であれば実際のコードも読む。また、技術的な情報はContext7とWebから集めてください。
このプロジェクトは低レイテンシを重視しています。

## パフォーマンスヒント

ビルド時にPATHに`\third_party\llvm\bin`を追加する必要があります。

レイテンシ最優先なら “YOLO11n（不十分なら s）+ TensorRT FP16” が第一候補。

## OBS Studio と Spoutプラグイン
[OBS Studio 31.1.2](https://github.com/obsproject/obs-studio/releases/tag/31.1.2)
[Spout2 Plugin for OBS](https://github.com/Off-World-Live/obs-spout2-plugin/releases/tag/1.10.0)