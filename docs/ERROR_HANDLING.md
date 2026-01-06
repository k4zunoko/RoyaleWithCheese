# エラーハンドリング戦略

## 概要

RoyaleWithCheeseは**レイテンシが極めてクリティカル**なプロジェクトです。エラーハンドリング戦略は以下の原則に基づきます:

1. **予測可能性**: エラーは型システムで明示的に表現
2. **リカバリー**: 可能な限り自動復旧
3. **ゼロオーバーヘッド**: Release buildではエラーログも排除
4. **明確な責務分離**: レイヤごとにエラーの扱いを定義

## DomainError - 統一エラー型

### 定義

**場所**: `src/domain/error.rs`

```rust
use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum DomainError {
    #[error("Capture error: {0}")]
    Capture(String),

    #[error("Processing error: {0}")]
    Processing(String),

    #[error("Communication error: {0}")]
    Communication(String),

    #[error("Configuration error: {0}")]
    Configuration(String),

    #[error("Timeout occurred")]
    Timeout,

    #[error("Device not available")]
    DeviceNotAvailable,

    #[error("Reinitialization required")]
    ReInitializationRequired,
}

pub type DomainResult<T> = Result<T, DomainError>;
```

### 設計判断

**なぜthiserrorか？**
- `#[error]`属性でDisplay実装が自動生成
- `#[from]`属性で他のエラー型からの変換が簡潔
- パフォーマンスオーバーヘッド無し（ゼロコスト抽象化）

**なぜanyhowを使わないか？**
- anyhowは型安全でない（Box<dyn Error>）
- エラーハンドリングがパターンマッチできない
- DomainErrorは具体的な型として扱える

## エラー分類

### Recoverable vs Non-Recoverable

| エラー種別 | 説明 | リカバリー戦略 | 例 |
|----------|------|-------------|---|
| **Timeout** | 一時的なタイムアウト | カウント、閾値到達で再初期化 | DDA待機タイムアウト |
| **DeviceNotAvailable** | デバイス一時不可 | 即座に再試行 | ロック画面遷移 |
| **ReInitializationRequired** | インスタンス再作成必要 | 指数バックオフで再初期化 | Unexpected DDAエラー |
| **Configuration** | 設定エラー | 致命的（即終了） | 無効なROI、HSV範囲 |

### win_desktop_duplicationのマッピング

Context7で確認したDDAエラー型:

```rust
// win_desktop_duplicationクレート
pub enum DDApiError {
    AccessLost,    // モニタモード変更、ロック画面 → DeviceNotAvailable
    AccessDenied,  // セキュアデスクトップ → DeviceNotAvailable
    Unexpected(String),  // 予期しないエラー → ReInitializationRequired
}
```

**マッピング実装** (Infrastructure層):
```rust
impl From<DDApiError> for DomainError {
    fn from(err: DDApiError) -> Self {
        match err {
            DDApiError::AccessLost | DDApiError::AccessDenied => {
                DomainError::DeviceNotAvailable
            }
            DDApiError::Unexpected(msg) => {
                DomainError::ReInitializationRequired
            }
        }
    }
}
```

## リカバリー戦略

### 1. Timeout - 連続タイムアウト監視

**閾値**: 連続120回（144Hz想定で約0.8秒）

**実装** (`src/application/recovery.rs`):
```rust
pub fn record_timeout(&mut self) -> bool {
    self.consecutive_timeouts += 1;
    
    if self.consecutive_timeouts >= self.strategy.consecutive_timeout_threshold {
        self.consecutive_timeouts = 0;
        true  // 再初期化が必要
    } else {
        false
    }
}
```

**使用例** (Application層):
```rust
match capture.capture_frame() {
    Ok(Some(frame)) => {
        recovery.record_success();  // 成功時にカウンタリセット
        // 処理続行
    }
    Ok(None) => {
        // タイムアウト
        if recovery.record_timeout() {
            tracing::warn!("Consecutive timeout threshold reached - reinitializing");
            capture.reinitialize()?;
            recovery.record_reinitialization_attempt();
        }
    }
    Err(_) => { /* エラー処理 */ }
}
```

### 2. DeviceNotAvailable - 即座に再試行

**理由**: ロック画面やディスプレイモード変更は数フレームで復帰

**実装**:
```rust
match capture.capture_frame() {
    Err(DomainError::DeviceNotAvailable) => {
        tracing::debug!("Device temporarily unavailable - retrying immediately");
        // 次のフレームで自動的に再試行（カウンタのみ記録）
        recovery.record_timeout();
    }
    Err(e) => { /* 他のエラー */ }
}
```

### 3. ReInitializationRequired - 指数バックオフ

**目的**: DDAインスタンス再作成の頻度を制限（システム負荷軽減）

**バックオフシーケンス**:
```
100ms → 200ms → 400ms → 800ms → 1600ms → 3200ms → 5000ms（上限）
```

**実装**:
```rust
match capture.capture_frame() {
    Err(DomainError::ReInitializationRequired) => {
        let backoff = recovery.current_backoff();
        tracing::warn!("Reinitialization required - waiting {:?}", backoff);
        
        std::thread::sleep(backoff);  // バックオフ待機
        
        capture.reinitialize()?;
        recovery.record_reinitialization_attempt();  // バックオフ時間を2倍にする
    }
    Err(e) => { /* 他のエラー */ }
}
```

**成功時のリセット**:
```rust
pub fn record_success(&mut self) {
    self.consecutive_timeouts = 0;
    self.current_backoff = self.strategy.initial_backoff;  // 100msに戻す
    self.cumulative_failure_start = None;
}
```

### 4. Configuration - 致命的エラー

**理由**: 設定エラーは実行時に自動復旧不可

**実装**:
```rust
match AppConfig::from_file("config.toml") {
    Ok(config) => { /* 使用 */ }
    Err(DomainError::Configuration(msg)) => {
        eprintln!("Configuration error: {}", msg);
        eprintln!("Fix config.toml and restart");
        std::process::exit(1);
    }
    Err(e) => { /* 他のエラー */ }
}
```

## 累積失敗監視

### 目的

短時間の再初期化は許容するが、**長期間の失敗は異常**として検出

**閾値**: 累積失敗時間60秒

### 実装

```rust
pub fn record_reinitialization_attempt(&mut self) {
    self.total_reinitializations += 1;
    self.current_backoff = (self.current_backoff * 2).min(self.strategy.max_backoff);
    
    // 累積失敗時間の計測開始
    if self.cumulative_failure_start.is_none() {
        self.cumulative_failure_start = Some(Instant::now());
    }
}

pub fn is_cumulative_failure_exceeded(&self) -> bool {
    if let Some(duration) = self.cumulative_failure_duration() {
        duration >= self.strategy.max_cumulative_failure
    } else {
        false
    }
}
```

### 使用例

```rust
loop {
    match capture.capture_frame() {
        Ok(Some(frame)) => {
            recovery.record_success();  // 累積失敗タイマーリセット
            // 処理続行
        }
        Err(e) => {
            // エラー処理...
            
            if recovery.is_cumulative_failure_exceeded() {
                tracing::error!("Cumulative failure exceeded 60s - terminating");
                break;  // または待機継続（設定可能）
            }
        }
    }
}
```

## エラーログのゼロオーバーヘッド化

### Release buildでの完全排除

**実装** (`src/logging.rs`):
```rust
#[cfg(debug_assertions)]
pub fn try_init() -> Option<()> {
    // tracing-appenderで非ブロッキング書き込み
    let file_appender = tracing_appender::rolling::daily("logs", "app.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    
    let subscriber = tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_max_level(Level::DEBUG)
        .finish();
    
    tracing::subscriber::set_global_default(subscriber).ok()?;
    Some(())
}

#[cfg(not(debug_assertions))]
pub fn try_init() -> Option<()> {
    None  // Release buildではログ初期化しない
}
```

**使用時の注意**:
```rust
#[cfg(debug_assertions)]
tracing::error!("Capture error: {:?}", e);

// Release buildではコンパイルされない → 0% オーバーヘッド
```

### Debug buildでの詳細ログ

**レイヤ別ログレベル**:
- **ERROR**: 致命的エラー（Configuration, 累積失敗超過）
- **WARN**: リカバリー必要なエラー（ReInitializationRequired, 連続タイムアウト）
- **INFO**: 統計情報（FPS, パーセンタイル）
- **DEBUG**: 詳細トレース（フレームキャプチャ、処理結果）

**実装例**:
```rust
#[cfg(debug_assertions)]
{
    tracing::debug!("Frame captured: {}x{}", frame.width, frame.height);
    tracing::info!("FPS: {:.1}", stats.current_fps());
    tracing::warn!("Reinitialization attempt #{}", recovery.total_reinitializations());
    tracing::error!("Configuration validation failed: {:?}", e);
}
```

## エラー伝播の原則

### Infrastructure → Application

**Infrastructure層**: エラーを返すのみ
```rust
impl CapturePort for DdaCaptureAdapter {
    fn capture_frame(&mut self) -> DomainResult<Option<Frame>> {
        match self.inner.acquire_next_frame_now() {
            Ok(frame) => Ok(Some(self.convert_frame(frame))),
            Err(DDApiError::Timeout) => Ok(None),
            Err(e) => Err(e.into()),  // DomainErrorに変換して返す
        }
    }
}
```

**Application層**: エラーを受け取り、リカバリーを決定
```rust
match capture.capture_frame() {
    Ok(Some(frame)) => { /* 処理 */ },
    Ok(None) => { /* Timeout処理 */ },
    Err(DomainError::DeviceNotAvailable) => { /* 即座に再試行 */ },
    Err(DomainError::ReInitializationRequired) => { /* バックオフ → 再初期化 */ },
    Err(e) => {
        tracing::error!("Unhandled error: {:?}", e);
        break;
    }
}
```

### Application → main.rs

**Application層**: 致命的エラーはResult<(), DomainError>で返す
```rust
impl<C, P, H> PipelineRunner<C, P, H>
where
    C: CapturePort,
    P: ProcessPort,
    H: CommPort,
{
    pub fn run(self) -> DomainResult<()> {
        // パイプライン実行
        // 致命的エラー（累積失敗超過など）はErrで返す
    }
}
```

**main.rs**: エラーハンドリングと終了処理
```rust
fn main() {
    logging::try_init();
    
    let config = match AppConfig::from_file("config.toml") {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to load config: {}", e);
            std::process::exit(1);
        }
    };
    
    // ... パイプライン初期化 ...
    
    if let Err(e) = runner.run() {
        eprintln!("Pipeline terminated with error: {}", e);
        std::process::exit(1);
    }
}
```

## パニック回避

### unwrapを使わない

❌ **避けるべき**:
```rust
let frame = capture.capture_frame().unwrap();  // パニック
```

✅ **推奨**:
```rust
let frame = match capture.capture_frame() {
    Ok(Some(f)) => f,
    Ok(None) => continue,  // タイムアウト - 次のフレーム
    Err(e) => {
        tracing::error!("Capture error: {:?}", e);
        return Err(e);
    }
};
```

### Mutexのロック

❌ **避けるべき**:
```rust
let guard = mutex.lock().unwrap();  // Poison時にパニック
```

✅ **推奨**:
```rust
let guard = mutex.lock().map_err(|_| {
    DomainError::Processing("Mutex poisoned".to_string())
})?;
```

### expect()の使用

**許可される場合**:
- 明らかに失敗しないケース（論理的に不可能）
- 失敗時は即終了が適切なケース

```rust
let (tx, rx) = bounded(1);  // 絶対に失敗しない
```

## テストでのエラー検証

### エラー型のテスト

```rust
#[test]
fn test_domain_error_display() {
    let err = DomainError::Timeout;
    assert_eq!(err.to_string(), "Timeout occurred");
    
    let err = DomainError::Capture("DDA failed".to_string());
    assert_eq!(err.to_string(), "Capture error: DDA failed");
}

#[test]
fn test_error_conversion() {
    let dda_err = DDApiError::AccessLost;
    let domain_err: DomainError = dda_err.into();
    
    assert!(matches!(domain_err, DomainError::DeviceNotAvailable));
}
```

### リカバリーロジックのテスト

```rust
#[test]
fn test_timeout_recovery() {
    let mut recovery = RecoveryState::with_default_strategy();
    
    // 119回はfalse
    for _ in 0..119 {
        assert!(!recovery.record_timeout());
    }
    
    // 120回目でtrue
    assert!(recovery.record_timeout());
    assert_eq!(recovery.consecutive_timeouts(), 0);  // リセット確認
}

#[test]
fn test_cumulative_failure_detection() {
    let strategy = RecoveryStrategy {
        max_cumulative_failure: Duration::from_secs(1),
        ..Default::default()
    };
    let mut recovery = RecoveryState::new(strategy);
    
    recovery.record_reinitialization_attempt();
    std::thread::sleep(Duration::from_millis(1100));
    
    assert!(recovery.is_cumulative_failure_exceeded());
}
```

## まとめ

エラーハンドリングのポイント:
- **DomainError**: 統一エラー型、thiserrorで実装
- **エラー分類**: Recoverable vs Non-Recoverable
- **リカバリー戦略**: Timeout監視、指数バックオフ、累積失敗検出
- **ゼロオーバーヘッド**: Release buildでログ完全排除
- **パニック回避**: unwrap禁止、Result伝播
- **責務分離**: Infrastructure層はエラー返却、Application層がリカバリー判断
