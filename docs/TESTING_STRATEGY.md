# テスト戦略

## テスト方針

### テストピラミッド

```
        ┌─────────────┐
        │  E2E Tests   │  少数・重要シナリオのみ
        ├─────────────┤
        │ Integration  │  trait実装の検証
        │    Tests     │
        ├─────────────┤
        │    Unit      │  多数・高速・詳細
        │    Tests     │
        └─────────────┘
```

## Domain層のテスト

### 単体テスト

**目標**: 100%カバレッジ

**実装場所**: 各モジュール内（`#[cfg(test)] mod tests`）

**例** (`src/domain/types.rs`):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_roi_center() {
        let roi = Roi::new(100, 100, 200, 200);
        assert_eq!(roi.center(), (200, 200));
    }
    
    #[test]
    fn test_roi_area() {
        let roi = Roi::new(0, 0, 100, 100);
        assert_eq!(roi.area(), 10000);
    }
    
    #[test]
    fn test_roi_intersects() {
        let roi1 = Roi::new(0, 0, 100, 100);
        let roi2 = Roi::new(50, 50, 100, 100);
        assert!(roi1.intersects(&roi2));
        
        let roi3 = Roi::new(200, 200, 100, 100);
        assert!(!roi1.intersects(&roi3));
    }
}
```

**実行**:
```powershell
cargo test --lib domain::types
```

## Application層のテスト

### モック実装

**実装場所**: `src/application/pipeline.rs`の`#[cfg(test)] mod tests`

**MockCapture**:
```rust
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
    
    fn reinitialize(&mut self) -> DomainResult<()> {
        Ok(())
    }
    
    fn device_info(&self) -> DeviceInfo {
        DeviceInfo {
            width: 1920,
            height: 1080,
            refresh_rate: 144,
            name: "Mock Display".to_string(),
        }
    }
}
```

**MockProcess**:
```rust
struct MockProcess;
impl ProcessPort for MockProcess {
    fn process_frame(
        &mut self,
        _frame: &Frame,
        _roi: &Roi,
        _hsv_range: &HsvRange,
    ) -> DomainResult<DetectionResult> {
        Ok(DetectionResult {
            timestamp: Instant::now(),
            detected: true,
            center_x: 960.0,
            center_y: 540.0,
            coverage: 1000,
        })
    }
    
    fn backend(&self) -> ProcessorBackend {
        ProcessorBackend::Cpu
    }
}
```

**MockComm**:
```rust
struct MockComm;
impl CommPort for MockComm {
    fn send(&mut self, _data: &[u8]) -> DomainResult<()> {
        Ok(())
    }
    
    fn is_connected(&self) -> bool {
        true
    }
    
    fn reconnect(&mut self) -> DomainResult<()> {
        Ok(())
    }
}
```

### 統合テスト

**パイプライン制御テスト**:
```rust
#[test]
fn test_pipeline_config_default() {
    let config = PipelineConfig::default();
    assert_eq!(config.stats_interval, Duration::from_secs(10));
    assert!(config.enable_dirty_rect_optimization);
    assert_eq!(config.hid_send_interval, Duration::from_millis(8));
}

#[test]
fn test_send_latest_only() {
    let (tx, rx) = bounded::<i32>(1);
    
    // 最初の送信は成功
    PipelineRunner::<MockCapture, MockProcess, MockComm>::send_latest_only(
        tx.clone(), 1
    );
    assert_eq!(rx.try_recv().unwrap(), 1);
    
    // キューを満たす
    tx.try_send(2).unwrap();
    
    // キューが満杯の状態で新しい値を送信（無視される）
    PipelineRunner::<MockCapture, MockProcess, MockComm>::send_latest_only(
        tx, 3
    );
    
    // キューには古い値（2）が残っている
    let value = rx.try_recv().unwrap();
    assert_eq!(value, 2);
}
```

**Recovery戦略テスト**:
```rust
#[test]
fn test_exponential_backoff() {
    let strategy = RecoveryStrategy {
        initial_backoff: Duration::from_millis(100),
        max_backoff: Duration::from_secs(5),
        ..Default::default()
    };
    
    let mut state = RecoveryState::new(strategy);
    
    assert_eq!(state.current_backoff(), Duration::from_millis(100));
    
    state.record_reinitialization_attempt();
    assert_eq!(state.current_backoff(), Duration::from_millis(200));
    
    state.record_reinitialization_attempt();
    assert_eq!(state.current_backoff(), Duration::from_millis(400));
    
    // ... 800ms, 1600ms, 3200ms ...
    
    state.record_reinitialization_attempt();
    assert_eq!(state.current_backoff(), Duration::from_secs(5));  // 上限
}

#[test]
fn test_timeout_threshold() {
    let mut state = RecoveryState::with_default_strategy();
    
    // 閾値未満
    for _ in 0..119 {
        assert!(!state.record_timeout());
    }
    
    // 閾値到達
    assert!(state.record_timeout());
    assert_eq!(state.consecutive_timeouts(), 0);  // リセット
}
```

**統計収集テスト**:
```rust
#[test]
fn test_fps_calculation() {
    let mut stats = StatsCollector::new(Duration::from_secs(10));
    
    // 0.1秒間隔で4フレーム記録（期待FPS: ~10）
    for _ in 0..4 {
        stats.record_frame();
        std::thread::sleep(Duration::from_millis(100));
    }
    
    let fps = stats.current_fps();
    assert!(fps > 5.0 && fps < 15.0, "FPS should be around 10, got {}", fps);
}

#[test]
fn test_percentile_stats() {
    let mut stats = StatsCollector::new(Duration::from_secs(10));
    
    // 100サンプルの処理時間を記録
    for i in 0..100 {
        stats.record_duration(StatKind::Process, Duration::from_millis(i));
    }
    
    let percentile = stats.percentile_stats(StatKind::Process).unwrap();
    assert_eq!(percentile.count, 100);
    assert!(percentile.p50.as_millis() >= 45 && percentile.p50.as_millis() <= 55);
    assert!(percentile.p95.as_millis() >= 90 && percentile.p95.as_millis() <= 99);
}
```

## Infrastructure層のテスト

### trait契約の検証

**実装場所**: `tests/infrastructure_test.rs`（将来実装）

**例** (DDA Capture):
```rust
#[test]
fn test_dda_capture_implements_captureport() {
    // DdaCaptureAdapterがCapturePort traitを実装していることを確認
    let adapter = DdaCaptureAdapter::new().unwrap();
    
    // device_info()が有効な情報を返すか
    let info = adapter.device_info();
    assert!(info.width > 0);
    assert!(info.height > 0);
    assert!(info.refresh_rate > 0);
}

#[test]
fn test_dda_capture_frame_format() {
    let mut adapter = DdaCaptureAdapter::new().unwrap();
    
    if let Ok(Some(frame)) = adapter.capture_frame() {
        // BGRA形式（4バイト/ピクセル）
        assert_eq!(frame.data.len(), (frame.width * frame.height * 4) as usize);
    }
}

#[test]
fn test_dda_reinitialize() {
    let mut adapter = DdaCaptureAdapter::new().unwrap();
    
    // 再初期化が成功するか
    assert!(adapter.reinitialize().is_ok());
    
    // 再初期化後もキャプチャ可能か
    assert!(adapter.capture_frame().is_ok());
}
```

### 実環境テスト

**注意**: 管理者権限・GPU必須

```powershell
# 管理者権限でテスト実行
cargo test --test infrastructure_test -- --ignored
```

```rust
#[test]
#[ignore]  // CI環境ではスキップ
fn test_dda_capture_actual() {
    // 実際にDDAでキャプチャ
    let mut adapter = DdaCaptureAdapter::new()
        .expect("DDA initialization failed - admin rights required?");
    
    let frame = adapter.capture_frame()
        .expect("Capture failed");
    
    if let Some(frame) = frame {
        assert!(frame.width >= 1920);
        assert!(frame.height >= 1080);
    }
}
```

## E2Eテスト

### 合成画像テスト

**目的**: 画像処理精度の検証

**実装場所**: `tests/e2e_test.rs`（将来実装）

```rust
#[test]
fn test_yellow_detection_accuracy() {
    // 黄色の矩形を含む合成画像を生成
    let frame = generate_test_frame_with_yellow_rect(
        1920, 1080,
        Roi::new(800, 400, 320, 280)  // 黄色領域
    );
    
    let mut processor = ColorProcessAdapter::new(ProcessorBackend::Cpu);
    let roi = Roi::new(480, 270, 960, 540);  // 中心ROI
    let hsv_range = HsvRange::default();     // 黄色検出
    
    let result = processor.process_frame(&frame, &roi, &hsv_range)
        .expect("Process failed");
    
    assert!(result.detected);
    assert!((result.center_x - 960.0).abs() < 50.0);  // 許容誤差±50px
    assert!((result.center_y - 540.0).abs() < 50.0);
    assert!(result.coverage > 1000);  // 最小面積
}
```

### パイプライン継続実行テスト

**目的**: 長時間安定性の検証

```rust
#[test]
#[ignore]  // 時間がかかるため通常はスキップ
fn test_pipeline_stability_1hour() {
    let config = PipelineConfig::default();
    let recovery = RecoveryState::with_default_strategy();
    let roi = Roi::new(480, 270, 960, 540);
    let hsv_range = HsvRange::default();
    
    let capture = DdaCaptureAdapter::new().unwrap();
    let process = ColorProcessAdapter::new(ProcessorBackend::Cpu);
    let comm = HidCommAdapter::new().unwrap();
    
    let runner = PipelineRunner::new(
        capture, process, comm,
        config, recovery, roi, hsv_range,
    );
    
    // 1時間実行
    std::thread::spawn(move || {
        runner.run().unwrap();
    });
    
    std::thread::sleep(Duration::from_secs(3600));
    
    // メモリリーク・クラッシュがないことを確認
    // 統計情報を出力して異常値がないか確認
}
```

## パフォーマンステスト

### ベンチマーク

**ツール**: criterion

**実装場所**: `benches/benchmark.rs`（将来実装）

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_roi_processing(c: &mut Criterion) {
    let frame = generate_test_frame(1920, 1080);
    let roi = Roi::new(480, 270, 960, 540);
    let hsv_range = HsvRange::default();
    let mut processor = ColorProcessAdapter::new(ProcessorBackend::Cpu);
    
    c.bench_function("color_detection_cpu", |b| {
        b.iter(|| {
            processor.process_frame(
                black_box(&frame),
                black_box(&roi),
                black_box(&hsv_range)
            )
        });
    });
}

criterion_group!(benches, bench_roi_processing);
criterion_main!(benches);
```

**実行**:
```powershell
cargo bench
```

### レイテンシ計測

**目標**:
- End-to-End: < 10ms @ 144Hz
- Capture: < 2ms
- Process: < 5ms
- Communication: < 1ms

**計測方法**:
```rust
// Debug buildで自動計測（tracing span）
measure_span!(MeasurePoint::EndToEnd, {
    let frame = capture.capture_frame()?;
    let result = process.process_frame(&frame, &roi, &hsv_range)?;
    comm.send(&detection_to_hid_report(&result))?;
});
```

**統計出力**:
```
[INFO] EndToEnd: p50=3.5ms, p95=5.8ms, p99=8.2ms
```

## CI/CD

### GitHub Actions

**設定案** (`.github/workflows/rust.yml`):
```yaml
name: Rust CI

on: [push, pull_request]

jobs:
  test:
    runs-on: windows-latest
    
    steps:
    - uses: actions/checkout@v3
    
    - name: Setup Rust
      uses: actions-rust-lang/setup-rust-toolchain@v1
    
    - name: Run tests
      run: cargo test --verbose
    
    - name: Run clippy
      run: cargo clippy -- -D warnings
    
    - name: Check formatting
      run: cargo fmt -- --check
    
    - name: Build Release
      run: cargo build --release --verbose
```

### ローカルCI

**実行**:
```powershell
# 全テスト
cargo test

# カバレッジ（tarpaulin使用）
cargo tarpaulin --out Html

# Clippy
cargo clippy -- -D warnings

# フォーマットチェック
cargo fmt -- --check

# ベンチマーク
cargo bench

# Release build
cargo build --release
```

## テスト実行状況

### 現在のテスト数

```
Domain層:    18 tests ✅
Application層: 11 tests ✅
Logging:      3 tests ✅
─────────────────────────
合計:        32 tests ✅
```

### カバレッジ目標

- Domain層: 100% (純粋関数のみ)
- Application層: 80% (モック注入)
- Infrastructure層: 60% (実環境依存)

## まとめ

テスト戦略のポイント:
- **Domain層**: 単体テスト100%、高速・詳細
- **Application層**: モック注入で統合テスト
- **Infrastructure層**: trait契約検証 + 実環境テスト
- **E2E**: 合成画像で精度検証、継続実行で安定性検証
- **パフォーマンス**: criterion + tracing span計測
