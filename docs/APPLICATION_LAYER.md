# Application層 設計詳細

## 概要

Application層は**ユースケースの実行**を担当し、Domain層のtraitを組み合わせてビジネス要件を実現します。

## モジュール構成

```
src/application/
├─ mod.rs             # モジュールエクスポート
├─ pipeline.rs        # パイプライン設定とエントリーポイント
├─ threads.rs         # 4スレッド実装（Capture/Process/HID/Stats）
├─ recovery.rs        # 再初期化ロジック（指数バックオフ）
├─ stats.rs           # 統計情報管理
├─ runtime_state.rs   # ランタイム状態管理（有効/無効切り替え）
└─ input_detector.rs  # キー押下検知（エッジ検出）
```

### モジュール分離の理由 (2025-12-18更新)

**pipeline.rsからthreads.rsを分離** (990行 → 250行 + 600行):
- **保守性向上**: 各ファイルが適切なサイズ（300-600行）に
- **責務の明確化**: "設定・エントリーポイント" vs "スレッド実装詳細"
- **低レイテンシ維持**: コンパイル時の分離のみ、実行時構造は不変

## 責務の明確化

### Application層の責務

✅ **含むもの**:
- パイプライン制御（スレッド管理、チャネル接続）
- **再初期化ロジック**（いつ・どのように再初期化するか）
- 統計情報収集と出力
- エラーハンドリングポリシー

❌ **含まないもの**:
- 具体的な外部技術（DDA/OpenCV/HID）
- traitの具体実装
- ハードウェア依存のコード

### Infrastructure層の責務

✅ **含むもの**:
- trait実装（CapturePort, ProcessPort, CommPort）
- エラー型の返却
- 外部ライブラリとのFFI

❌ **含まないもの**:
- **再初期化の判断**（エラーを返すのみ）
- ポリシー決定（バックオフ時間、再試行回数など）
- 統計情報の管理

## 再初期化ロジックの配置理由

### なぜApplication層か？

**DDA固有のリカバリーなのにApplication層に配置**する理由：

1. **ポリシー判断はビジネスロジック**
   - 「連続120回タイムアウトしたら再初期化」→ ビジネス要件
   - 「指数バックオフで100ms → 5秒」→ ビジネス要件
   - 「累積失敗1分で致命的エラー」→ ビジネス要件

2. **Infrastructure層はtraitの実装に徹する**
   - DDAアダプタは`capture_frame()`と`reinitialize()`を実装
   - エラーが発生したら適切なDomainErrorを返す
   - **いつreinitialize()を呼ぶかは知らない**

3. **関心の分離**
   - Infrastructure: 技術的詳細（どうやってキャプチャするか）
   - Application: ビジネスロジック（どう制御するか）

### win_desktop_duplicationのエラー型

Context7で調査したエラー型:

```rust
// win_desktop_duplicationクレートのエラー型
pub enum DDApiError {
    // Recoverable errors
    AccessLost,    // デスクトップモード変更（解像度、ロック画面）
    AccessDenied,  // Windowsがセキュア環境に移行
    
    // Non-recoverable error
    Unexpected(String),  // 予期しないエラー → インスタンス再作成必要
}
```

**Application層の判断フロー**:
```rust
match capture.capture_frame() {
    Ok(Some(frame)) => { /* 処理 */ },
    Ok(None) => { /* タイムアウト - カウント */ },
    Err(e) => {
        // エラー種別を判定してリカバリー戦略を決定
        if is_recoverable(&e) {
            // Recoverable: 即座に再試行
            recovery.record_timeout();
        } else {
            // Non-recoverable: インスタンス再作成
            recovery.record_reinitialization_attempt();
            capture.reinitialize()?;
        }
    }
}
```

## pipeline.rs - パイプライン設定とエントリーポイント

### 責務

**pipeline.rs（約250行）**:
- `PipelineConfig`: 統計出力間隔、HID送信間隔、DirtyRect最適化設定
- `ActivationConditions`: HIDアクティベーション条件（距離、持続時間）
- `PipelineRunner`: パイプラインの設定とスレッド起動
  - `new()`: 依存性注入による初期化
  - `run()`: 4スレッドを起動しメインスレッドでStats/UIを実行

**threads.rs（約600行）**:
- 内部データ型: `TimestampedFrame`, `TimestampedDetection`, `StatData`, `ActivationState`
- スレッド関数: `capture_thread`, `process_thread`, `hid_thread`, `stats_thread`
- ヘルパー: `send_latest_only`, `update_runtime_state_window`

### 4スレッド構成

```
┌─────────────┐     bounded(1)      ┌─────────────┐     bounded(1)      ┌─────────────┐
│   Capture   │ ──────────────────> │   Process   │ ──────────────────> │     HID     │
│   Thread    │  TimestampedFrame   │   Thread    │ TimestampedDetection│   Thread    │
└─────────────┘                     └──────┬──────┘                     └─────────────┘
                                           │ mpsc
                                           │ StatData
                                           ↓
                                    ┌─────────────┐
                                    │  Stats/UI   │
                                    │   Thread    │
                                    └─────────────┘
```

**スレッド役割**:
- **Capture Thread**: `CapturePort::capture_frame()` を呼び出し、フレームを取得
- **Process Thread**: `ProcessPort::process_frame()` で画像処理、統計データをStats/UIスレッドに送信
- **HID Thread**: `CommPort::send()` でHID送信のみに特化（低レイテンシ最優先）
- **Stats/UI Thread**: 統計情報の収集・計算・出力、将来的にユーザー対話を担当

**設計理由**:
- HID送信を統計処理から完全に分離し、レイテンシを最小化
- 統計計算（メモリコピー、集計）がHID送信の遅延に影響しない
- 関心の分離: HID（リアルタイム性重視）とStats/UI（スループット重視）

## threads.rs - スレッド実装の詳細

### スレッド関数（threads.rsに実装）

**`capture_thread()`**:
- `CapturePort::capture_frame_with_roi()` を呼び出し、フレームを取得
- 成功時: `TimestampedFrame` を `send_latest_only()` で送信
- タイムアウト: 1ms sleep後に再試行
- エラー: 10ms sleep後に再試行

**`process_thread()`**:
- `ProcessPort::process_frame()` で画像処理
- 検出結果を `TimestampedDetection` として送信
- 統計データ（処理時間）をStats/UIスレッドに送信

**`hid_thread()`**:
- アクティベーション条件をチェック（`ActivationState::should_activate()`）
- 条件満たす場合: `CommPort::send()` でHID送信
- 送信エラー時: 指数バックオフで再接続（100ms → 5秒）
- タイムアウト時: 直前の値を送信（144Hz固定レート）

**`stats_thread()`**:
- 統計データを受信し、FPS・レイテンシを計算
- Insertキー検知でシステム有効/無効を切り替え
- マウスボタン状態をポーリング（100Hz）
- opencv-debug-display feature有効時: RuntimeState表示

### bounded(1)キューの「最新のみ」ポリシー

**実装（threads.rsの`send_latest_only()`）**:
```rust
pub(crate) fn send_latest_only<T>(tx: Sender<T>, value: T) {
    match tx.try_send(value) {
        Ok(_) => {}
        Err(TrySendError::Full(_)) => {
            // キューが満杯 - 古いデータは受信側が破棄する
            // Senderからは取り出せないため、単に無視
        }
        Err(TrySendError::Disconnected(_)) => {
            // Channel closed
        }
    }
}
```

**設計判断**:
- **try_send()でFull時は無視**: crossbeam-channelの仕様上、Senderから古いデータを取り出せない
- **受信側が破棄**: 次のrecv()で自動的に古いデータを破棄して新しいデータを受信
- **トレードオフ**: 満杯時の1フレーム遅延 vs メモリ安全性 → 後者を優先

**代替案と却下理由**:
- unbounded + 定期的なクリア → メモリ増加リスク
- 複数キュー → 同期コストが高い
- Senderからtry_recv() → crossbeam-channelにはこのメソッドが存在しない

### Arc<Mutex>による共有

**実装**:
```rust
pub struct PipelineRunner<C, P, H>
where
    C: CapturePort,
    P: ProcessPort,
    H: CommPort,
{
    capture: Arc<Mutex<C>>,  // スレッド間共有
    process: Arc<Mutex<P>>,
    comm: Arc<Mutex<H>>,
    // ...
}
```

**設計判断**:
- **Arc<Mutex>**: traitが`&mut self`を要求するため
- **ロック粒度**: メソッド呼び出し単位（短時間）
- **デッドロック回避**: 複数ロックを同時に取得しない

**パフォーマンス考慮**:
- Mutexのロックはナノ秒オーダー（競合がない場合）
- 各スレッドは独立したPortにアクセス → 競合なし
- クリティカルセクションを最小化

### タイムスタンプ管理

**実装**:
```rust
pub struct TimestampedFrame {
    pub frame: Frame,
    pub captured_at: Instant,  // キャプチャ時刻
}

pub struct TimestampedDetection {
    pub result: DetectionResult,
    pub captured_at: Instant,   // キャプチャ時刻（伝播）
    pub processed_at: Instant,  // 処理完了時刻
}
```

**レイテンシ計測**:
```rust
// HID Threadでタイムスタンプを記録
let hid_sent_at = Instant::now();

// Stats/UI Threadで集計計測
let end_to_end = hid_sent_at.duration_since(detection.captured_at);
let process_time = detection.processed_at.duration_since(detection.captured_at);
let comm_time = hid_sent_at.duration_since(detection.processed_at);

stats.record_duration(StatKind::Process, process_time);
stats.record_duration(StatKind::Communication, comm_time);
stats.record_duration(StatKind::EndToEnd, end_to_end);
```

## recovery.rs - 再初期化ロジック

### RecoveryStrategy

**設定可能なパラメータ**:
```rust
pub struct RecoveryStrategy {
    pub consecutive_timeout_threshold: u32,  // 連続タイムアウト閾値
    pub initial_backoff: Duration,           // 初期バックオフ時間
    pub max_backoff: Duration,               // 最大バックオフ時間
    pub max_cumulative_failure: Duration,    // 累積失敗時間上限
}
```

**デフォルト値**:
```rust
impl Default for RecoveryStrategy {
    fn default() -> Self {
        Self {
            consecutive_timeout_threshold: 120,  // 約1秒（8ms × 120）
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(5),
            max_cumulative_failure: Duration::from_secs(60),
        }
    }
}
```

### 指数バックオフ

**実装**:
```rust
impl RecoveryState {
    pub fn record_reinitialization_attempt(&mut self) {
        self.total_reinitializations += 1;
        
        // 指数バックオフ: 次回のバックオフ時間を2倍にする
        self.current_backoff = (self.current_backoff * 2)
            .min(self.strategy.max_backoff);
        
        // 累積失敗時間の計測開始
        if self.cumulative_failure_start.is_none() {
            self.cumulative_failure_start = Some(Instant::now());
        }
    }
}
```

**バックオフシーケンス**:
```
100ms → 200ms → 400ms → 800ms → 1600ms → 3200ms → 5000ms（上限）
```

### 連続タイムアウト監視

**実装**:
```rust
impl RecoveryState {
    pub fn record_timeout(&mut self) -> bool {
        self.consecutive_timeouts += 1;
        
        if self.consecutive_timeouts >= self.strategy.consecutive_timeout_threshold {
            self.consecutive_timeouts = 0;
            true  // 再初期化が必要
        } else {
            false
        }
    }
    
    pub fn record_success(&mut self) {
        self.consecutive_timeouts = 0;
        self.current_backoff = self.strategy.initial_backoff;
        self.cumulative_failure_start = None;
    }
}
```

### 累積失敗時間監視

**実装**:
```rust
impl RecoveryState {
    pub fn cumulative_failure_duration(&self) -> Option<Duration> {
        self.cumulative_failure_start.map(|start| start.elapsed())
    }
    
    pub fn is_cumulative_failure_exceeded(&self) -> bool {
        if let Some(duration) = self.cumulative_failure_duration() {
            duration >= self.strategy.max_cumulative_failure
        } else {
            false
        }
    }
}
```

**使用例**:
```rust
if recovery.is_cumulative_failure_exceeded() {
    tracing::error!("Cumulative failure time exceeded 60s - fatal error");
    // 終了 or 待機継続（設定可能）
}
```

## stats.rs - 統計情報管理

### StatsCollector

**4スレッド構成での役割**:
- **Stats/UIスレッド**: 独立したスレッドで統計情報を収集・計算・出力
- **Process/HIDスレッドから統計データを受信**: mpscチャネル経由で非ブロッキング
- **HID送信への影響ゼロ**: 統計処理はStats/UIスレッドで完結

**管理する統計**:
```rust
pub struct StatsCollector {
    frame_times: VecDeque<Instant>,          // FPS計測用
    durations: HashMap<StatKind, VecDeque<Duration>>,  // 処理時間
    reinit_count: u64,                       // 再初期化回数
    cumulative_failure_duration: Duration,   // 累積失敗時間
    last_report: Instant,                    // 最後の出力時刻
    report_interval: Duration,               // 出力間隔
}

pub struct StatData {
    pub captured_at: Instant,
    pub processed_at: Instant,
    pub hid_sent_at: Instant,
}
```

### FPS計測

**実装**:
```rust
impl StatsCollector {
    pub fn record_frame(&mut self) {
        let now = Instant::now();
        self.frame_times.push_back(now);
        
        // 1秒より古いタイムスタンプを削除
        while let Some(&front) = self.frame_times.front() {
            if now.duration_since(front) > Duration::from_secs(1) {
                self.frame_times.pop_front();
            } else {
                break;
            }
        }
    }
    
    pub fn current_fps(&self) -> f64 {
        if self.frame_times.is_empty() {
            return 0.0;
        }
        
        let count = self.frame_times.len() as f64;
        if let (Some(&first), Some(&last)) = (self.frame_times.front(), self.frame_times.back()) {
            let elapsed = last.duration_since(first).as_secs_f64();
            if elapsed > 0.0 {
                return count / elapsed;
            }
        }
        0.0
    }
}
```

### パーセンタイル統計

**実装**:
```rust
impl StatsCollector {
    pub fn percentile_stats(&self, kind: StatKind) -> Option<PercentileStats> {
        let queue = self.durations.get(&kind)?;
        if queue.is_empty() {
            return None;
        }
        
        let mut sorted: Vec<Duration> = queue.iter().copied().collect();
        sorted.sort();
        
        let count = sorted.len();
        let p50 = sorted[count * 50 / 100];
        let p95 = sorted[count * 95 / 100];
        let p99 = sorted[count * 99 / 100];
        
        Some(PercentileStats { p50, p95, p99, count })
    }
}
```

### 統計出力

**実装** (Debug buildのみ):
```rust
#[cfg(debug_assertions)]
pub fn report_and_reset(&mut self) {
    info!("=== Pipeline Statistics ===");
    info!("FPS: {:.1}", self.current_fps());
    
    for kind in [StatKind::Capture, StatKind::Process, /*...*/] {
        if let Some(stats) = self.percentile_stats(kind) {
            info!(
                "{:?}: p50={:.2}ms, p95={:.2}ms, p99={:.2}ms (n={})",
                kind,
                stats.p50.as_secs_f64() * 1000.0,
                stats.p95.as_secs_f64() * 1000.0,
                stats.p99.as_secs_f64() * 1000.0,
                stats.count
            );
        }
    }
    
    info!("Reinitialization count: {}", self.reinit_count);
    self.last_report = Instant::now();
}
```

**出力例**:
```
[INFO] === Pipeline Statistics ===
[INFO] FPS: 144.2
[INFO] Capture: p50=0.8ms, p95=1.2ms, p99=1.8ms (n=1442)
[INFO] Process: p50=2.1ms, p95=3.5ms, p99=5.2ms (n=1442)
[INFO] Communication: p50=0.3ms, p95=0.5ms, p99=0.8ms (n=1442)
[INFO] EndToEnd: p50=3.5ms, p95=5.8ms, p99=8.2ms (n=1442)
[INFO] Reinitialization count: 0
[INFO] ===========================
```

## テスト戦略

### モック注入による統合テスト

**実装**:
```rust
#[cfg(test)]
mod tests {
    struct MockCapture;
    impl CapturePort for MockCapture {
        fn capture_frame(&mut self) -> DomainResult<Option<Frame>> {
            Ok(Some(Frame {
                data: vec![0u8; 1920 * 1080 * 4],
                width: 1920,
                height: 1080,
                timestamp: Instant::now(),
                dirty_rects: vec![],
            }))
        }
        
        fn reinitialize(&mut self) -> DomainResult<()> { Ok(()) }
        fn device_info(&self) -> DeviceInfo { /* ... */ }
    }
    
    // 同様にMockProcess, MockCommを実装
    
    #[test]
    fn test_pipeline_with_mocks() {
        let config = PipelineConfig::default();
        let recovery = RecoveryState::with_default_strategy();
        let roi = Roi::new(480, 270, 960, 540);
        let hsv_range = HsvRange::default();
        
        let runner = PipelineRunner::new(
            MockCapture,
            MockProcess,
            MockComm,
            config,
            recovery,
            roi,
            hsv_range,
        );
        
        // パイプライン起動（短時間で終了）
        // runner.run(); // 実際のテストでは短時間で終了させる
    }
}
```

## 実装時の注意点

### デッドロック回避

❌ **避けるべきパターン**:
```rust
// 複数ロックの同時取得
let guard1 = self.capture.lock().unwrap();
let guard2 = self.process.lock().unwrap();  // デッドロックリスク
```

✅ **推奨パターン**:
```rust
// ロックスコープを最小化
{
    let mut guard = self.capture.lock().unwrap();
    guard.capture_frame()?
} // ここでロック解放

{
    let mut guard = self.process.lock().unwrap();
    guard.process_frame(&frame, &roi, &hsv_range)?
}
```

### エラー伝播

❌ **避けるべきパターン**:
```rust
let frame = capture.capture_frame().unwrap();  // パニック
```

✅ **推奨パターン**:
```rust
match capture.capture_frame() {
    Ok(Some(frame)) => { /* 処理 */ },
    Ok(None) => { /* タイムアウト */ },
    Err(e) => {
        tracing::error!("Capture error: {:?}", e);
        // リカバリー処理
    }
}
```

### ログ出力

✅ **Debug buildでのみ有効**:
```rust
#[cfg(debug_assertions)]
tracing::info!("Frame captured: {}x{}", frame.width, frame.height);

#[cfg(not(debug_assertions))]
let _ = frame;  // 未使用変数警告を抑制
```

## まとめ

Application層の設計ポイント:
- **再初期化ロジック**: ポリシー判断はApplication層の責務
- **bounded(1)キュー**: 最新のみポリシーでバックプレッシャ回避
- **Arc<Mutex>**: スレッド間共有、デッドロック回避に注意
- **統計収集**: パフォーマンス計測と最適化の基盤
- **モックテスト**: Infrastructure実装前に動作確認可能
