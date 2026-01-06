# ログ実装の詳細とレイテンシ対策

## 概要
本プロジェクトは**超低レイテンシ**を要求するため、ログ出力がメインロジックの実行速度に影響を与えないよう設計されています。

---

## 非同期ログの仕組み

### 実装: `tracing-appender` の `non_blocking`
```rust
let file_appender = tracing_appender::rolling::daily(dir, "royale_with_cheese.log");
let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
```

### 動作原理
1. **メインスレッド**（キャプチャ・処理・HID送信）
   - `tracing::info!()` などのマクロ呼び出し
   - **即座に復帰**（ログデータはバッファに書き込むだけ）
   - **I/O待機なし**（ファイル書き込みは別スレッド）

2. **ログスレッド**（バックグラウンド）
   - バッファからログデータを取得
   - ファイルへの書き込み（ブロッキングI/O）
   - ディスク同期などの重い処理を担当

### レイテンシへの影響
- **メインスレッドのオーバーヘッド**: ~数十ナノ秒（メモリコピーのみ）
- **I/O待機**: なし（別スレッドで実行）
- **バッファ満杯時**: 古いログを破棄（ドロップポリシー）
  - メインスレッドは**絶対にブロックしない**

---

## ファイル出力の設定

### ログディレクトリ
- **パス**: `logs/royale_with_cheese.log`
- **ローテーション**: 日次（`tracing_appender::rolling::daily`）
- **ファイル名**: `royale_with_cheese.log.YYYY-MM-DD`

### WindowsGUIサブシステム対応
```rust
// #![windows_subsystem = "windows"] を有効化時
// コンソールが存在しないため、ファイル出力必須
let log_dir = PathBuf::from("logs");
let _guard = init_logging("info", false, Some(log_dir));
```

### WorkerGuard の保持
```rust
let _guard = init_logging(...);
// _guardはmain()の終了まで保持
// Dropでログスレッドが終了し、バッファがフラッシュされる
```

---

## 計測スパンの使用方法

### マクロ: `measure_span!`
```rust
use royale_with_cheese::measure_span;

fn process_frame() {
    measure_span!("process_frame", {
        // 処理内容
        // 自動的に経過時間がログ出力される
    });
}
```

### RAII: `SpanTimer`
```rust
use royale_with_cheese::logging::SpanTimer;

fn capture_loop() {
    let _timer = SpanTimer::new("capture");
    // 処理
    // Dropで自動的に経過時間がログ出力
}
```

### ログ出力例
```
2025-12-05T12:34:56.789Z DEBUG capture: Span completed elapsed_us=1234
2025-12-05T12:34:56.791Z DEBUG process: Span completed elapsed_us=567
```

---

## 計測統計の集計

### `MeasurementStats` の使用
```rust
use royale_with_cheese::logging::MeasurementStats;

let mut stats = MeasurementStats::new("capture".to_string());

// サンプル追加
stats.add_sample(1234); // マイクロ秒
stats.add_sample(1456);
stats.add_sample(1123);

// 統計情報
println!("Count: {}", stats.count);
println!("Avg: {}us", stats.avg_us);
println!("Min: {}us", stats.min_us);
println!("Max: {}us", stats.max_us);

// リセット
stats.reset();
```

---

## パフォーマンスベンチマーク

### 計測方法
```bash
# Criterion ベンチマーク実行
cargo bench

# プロファイリング（Windows Performance Analyzer推奨）
# または perf (Linux)
```

### 目標値
- **キャプチャ**: <1ms (144Hz対応: 6.94ms周期)
- **前処理（ROI抽出）**: <100us
- **色検知（HSV→マスク→モーメント）**: <500us
- **HID送信**: <100us
- **エンドツーエンド**: <2ms

### ログオーバーヘッド
- **目標**: <1% (非同期ログで実現)
- **測定**: ログ無効時とログ有効時の比較

---

## トラブルシューティング

### ログファイルが作成されない
- `logs/` ディレクトリの書き込み権限を確認
- `_guard` を main() 終了まで保持しているか確認

### ログが途中で切れる
- プログラム異常終了時は `_guard` の Drop が呼ばれない
- バッファに残ったログはフラッシュされない
- **対策**: シグナルハンドラで `drop(_guard)` を明示的に呼ぶ

### ディスク容量の圧迫
- 日次ローテーションで古いログが残る
- **対策**: 定期的な古ログ削除（例: 7日以上前を削除）
- または `tracing_appender::rolling::Rotation` でサイズベースローテーション

---

## まとめ

✅ **非同期ログでレイテンシ影響なし**
✅ **ファイル出力でWindowsGUIサブシステム対応**
✅ **日次ローテーションで管理容易**
✅ **WorkerGuard保持で確実なフラッシュ**
✅ **計測スパンで統一的なパフォーマンス計測**
