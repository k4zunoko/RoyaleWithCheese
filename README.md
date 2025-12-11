```
cargo test  dda -- --ignored --nocapture --test-threads=1
cargo test test_exclusive_fullscreen_recovery -- --ignored --nocapture --test-threads=1
```

dda.rs 237行目のコメントアウトを外すと、VSync待機が有効になります。
```
// self.wait_for_vsync()?;
```

プロジェクトの設計方針についてはAGENT.mdとdocs/を参照し、必要であれば実際のコードも読む。また、技術的な情報はContext7とWebから集めてください。
このプロジェクトは開発途中で低レイテンシを重視しています。