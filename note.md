# ドキュメント

プロジェクトの設計方針についてはAGENT.mdとdocs/を参照し、必要であれば実際のコードも読む。また、技術的な情報はContext7とWebから集めてください。
このプロジェクトは開発途中で低レイテンシを重視しています。

## パフォーマンスヒント

// #![windows_subsystem = "windows"] // ← これでコンソール非表示（GUIサブシステム）

dda.rs 237行目のコメントアウトを外すと、VSync待機が有効になります。
```rust
// self.wait_for_vsync()?;
```

ビルド時にPATHに`\third_party\llvm\bin`を追加する必要があります。
実行時にPATHに`\third_party\opencv\build\x64\vc16\bin`を追加する必要があります。

レイテンシ最優先なら “YOLO11n（不十分なら s）+ TensorRT FP16” が第一候補。

デバック機能付きの実行では少なくともconfig.tomlが適用されていない
HIDデバイスへのパケットは8byte

```cpp
bool HIDDevice::Move(int x, int y) {
    std::vector<unsigned char> report(8, 0x00);
    auto xBytes = encodeIntToBytes(x);
    auto yBytes = encodeIntToBytes(y);

    report[0] = 0x01;   
    report[1] = 0x00;   
    report[2] = 0x00;   
    report[3] = xBytes.second; 
    report[4] = xBytes.first;  
    report[5] = yBytes.second; 
    report[6] = yBytes.first;
    report[7] = 0xFF;

    int res = sendReport(report);
    if (res < 0) {
        return false;
    }
    return true;
}
```