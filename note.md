# ドキュメント

プロジェクトの設計方針についてはAGENTS.mdとdocs/を参照し、必要であれば実際のコードも読む。また、技術的な情報はContext7とWebから集めてください。
このプロジェクトは低レイテンシを重視しています。

## パフォーマンスヒント

ビルド時にPATHに`\third_party\llvm\bin`を追加する必要があります。
実行時にPATHに`\third_party\opencv\build\x64\vc16\bin`を追加する必要があります。

レイテンシ最優先なら “YOLO11n（不十分なら s）+ TensorRT FP16” が第一候補。

```cpp
void Maintenance::MaintenanceThreadFunc() {
    bool lastState = false;

    auto last = std::chrono::steady_clock::now();
    int LastCaptureCount = 0;
    int LastReportCount = 0;

    while (running) {
        SHORT state = GetAsyncKeyState(VK_INSERT);
        bool pressed = (state & 0x8000) != 0;

        if (pressed && !lastState) {
            bool current = getFlag();
            PlaySound(!current ? L"C:\\Windows\\Media\\Speech On.wav" : L"C:\\Windows\\Media\\Speech Off.wav", NULL, SND_FILENAME | SND_ASYNC);
            setFlag(!current);
        }
        lastState = pressed;

        std::this_thread::sleep_for(std::chrono::milliseconds(50));
    }
}
```