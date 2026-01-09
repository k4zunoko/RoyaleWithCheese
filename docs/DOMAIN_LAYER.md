# Domain層 設計詳細

## 概要

Domain層は**ビジネスロジックの中核**であり、外部依存を一切持たない純粋なRustコードで構成されます。

## モジュール構成

```
src/domain/
├─ mod.rs       # モジュールエクスポート
├─ types.rs     # コア型定義
├─ ports.rs     # trait定義（Clean Architectureのインターフェース）
├─ error.rs     # 統一エラー型
└─ config.rs    # 設定構造体とバリデーション
```

## types.rs - コア型定義

### Roi (Region of Interest)

**目的**: ピクセル座標系でのROIを表現

```rust
pub struct Roi {
    pub x: u32,      // 左上のX座標
    pub y: u32,      // 左上のY座標
    pub width: u32,  // 幅
    pub height: u32, // 高さ
}
```

**提供メソッド**:
- `new(x, y, width, height)`: 新しいROIを作成
- `center()`: ROI中心座標を計算
- `area()`: ROI面積を計算
- `intersects(&other)`: 別のROIとの交差判定

**設計判断**:
- **u32を使用**: 画面座標は負値を取らない
- **中心座標は計算で求める**: メモリ効率とデータ一貫性
- **不変性**: 一度作成したら変更不可（Rustのムーブセマンティクスで自然に実現）

### RoiConfig（config.rs）と自動中心配置

**プロジェクトの設計方針**: ROIは常に画面中心に配置される

**目的**: ユーザビリティと設定の意図明確化

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoiConfig {
    pub width: u32,   // ROIの幅のみ指定
    pub height: u32,  // ROIの高さのみ指定
    // x, y は実行時に毎フレーム動的に計算
}

impl RoiConfig {
    pub fn to_roi_centered(&self, screen_width: u32, screen_height: u32) -> DomainResult<Roi> {
        // ROIサイズが画面サイズを超える場合はエラー（初期化時の妥当性検証用）
        if self.width > screen_width || self.height > screen_height {
            return Err(DomainError::Configuration(/* ... */));
        }
        
        // 中心座標を計算（初期化時の妥当性検証用）
        let x = (screen_width - self.width) / 2;
        let y = (screen_height - self.height) / 2;
        
        Ok(Roi::new(x, y, self.width, self.height))
    }
}
```

**Roi::centered_in() による動的中心配置**:

```rust
impl Roi {
    /// 指定された画面サイズの中心に配置されるROIを作成
    /// 
    /// 毎フレーム実行されるが、レイテンシへの影響は無視できるレベル（~10ns未満）
    pub fn centered_in(&self, screen_width: u32, screen_height: u32) -> Option<Self> {
        if self.width > screen_width || self.height > screen_height {
            return None;
        }
        
        let x = (screen_width - self.width) / 2;
        let y = (screen_height - self.height) / 2;
        
        Some(Roi::new(x, y, self.width, self.height))
    }
}
```

**実行時の動作**:

1. **初期化時（1回のみ）**: `to_roi_centered()` でROIサイズの妥当性を検証
2. **毎フレーム**: `centered_in()` でテクスチャサイズに応じた中心位置を動的計算
   - DDA: ディスプレイ解像度に対して中心配置
   - Spout: 受信したテクスチャサイズに対して中心配置（送信者が変わっても自動追従）

**設計根拠**:

1. **ユーザビリティ向上**
   - 異なる解像度（1920×1080 / 2560×1440 / 3840×2160）で同じ設定ファイルが使える
   - Spoutで送信者が変わっても常に中心からキャプチャ
   - ユーザーが手動でx, y座標を計算する必要がない
   - 設定の意図が明確（「画面中心の460×240領域をキャプチャ」）

2. **低レイテンシ維持**
   - 計算コスト: 毎フレーム2回の減算、2回の除算（~10ns未満、<0.01ms）
   - GPU ROI切り出しは変更なし（DDA/Spoutレイヤーで実行）
   - パフォーマンスへの影響は無視できるレベル

3. **エラー処理の簡素化**
   - 画面解像度検証が初期化時に行われる（明確なエラーメッセージ）
   - `clamp_roi()`による境界検証と組み合わせて安全性確保

4. **拡張性**
   - 将来、異なる配置戦略（左上、右下、カスタム）への拡張が容易
   - 現在の実装を変更せずに追加可能

**config.toml例**:
```toml
[process.roi]
# スクリーン中心を基準として、指定したサイズの領域をキャプチャ
# 毎フレーム、受信したテクスチャのサイズから中心位置を動的計算
# 例: 1920x1080 画面 → x=730, y=420 に自動配置
# 例: 2560x1440 画面 → x=1050, y=600 に自動配置
# Spoutで送信者が変わっても自動追従
width = 460
height = 240
```

**テスト**:
```rust
#[test]
fn test_roi_center() {
    let roi = Roi::new(100, 100, 200, 200);
    assert_eq!(roi.center(), (200, 200));
}

#[test]
fn test_roi_intersects() {
    let roi1 = Roi::new(0, 0, 100, 100);
    let roi2 = Roi::new(50, 50, 100, 100);
    assert!(roi1.intersects(&roi2));
}

#[test]
fn test_roi_centered_normal() {
    // 1920x1080画面の中心に960x540のROI
    let roi_config = RoiConfig { width: 960, height: 540 };
    let roi = roi_config.to_roi_centered(1920, 1080).unwrap();
    assert_eq!(roi.x, 480);  // (1920 - 960) / 2
    assert_eq!(roi.y, 270);  // (1080 - 540) / 2
}

#[test]
fn test_roi_centered_width_exceeds() {
    // ROI幅が画面幅を超える場合はエラー
    let roi_config = RoiConfig { width: 2000, height: 540 };
    assert!(roi_config.to_roi_centered(1920, 1080).is_err());
}
```

### HsvRange

**目的**: OpenCV準拠のHSV色空間範囲を表現

```rust
pub struct HsvRange {
    pub h_min: u8,  // 0-180 (OpenCV形式)
    pub h_max: u8,
    pub s_min: u8,  // 0-255
    pub s_max: u8,
    pub v_min: u8,  // 0-255
    pub v_max: u8,
}
```

**設計判断**:
- **OpenCV準拠**: H: 0-180（8bit）、S/V: 0-255
  - 理由: OpenCVがこの形式を使用（メモリ効率のため）
  - 注意: 一般的なHSVはH: 0-360だが、OpenCVは半分のスケール
- **バリデーション**: 範囲外の値を拒否（config.rsで実施）

**使用例**:
```rust
// 黄色系の検出（デフォルト設定）
HsvRange {
    h_min: 25,   // 黄色の開始
    h_max: 45,   // 黄色の終了
    s_min: 80,   // 彩度の最小値（鮮やかさ）
    s_max: 255,  // 彩度の最大値
    v_min: 80,   // 明度の最小値
    v_max: 255,  // 明度の最大値
}
```

### Frame

**目的**: キャプチャされたフレームを表現

```rust
pub struct Frame {
    pub data: Vec<u8>,              // BGRA形式のピクセルデータ
    pub width: u32,
    pub height: u32,
    pub timestamp: Instant,          // キャプチャ時刻
    pub dirty_rects: Vec<Rect>,     // 更新領域（DirtyRect最適化用）
}
```

**設計判断**:
- **BGRA形式**: DDAが返す形式をそのまま使用（変換コスト削減）
- **dirty_rectsをVec**: DDAは複数のDirtyRectを返す可能性がある
  - 変更許容: パフォーマンス問題があれば最大N個に制限も可
- **timestampをInstant**: レイテンシ計測に必要

**DirtyRect最適化**:
```rust
impl Frame {
    pub fn roi_is_dirty(&self, roi: &Roi) -> bool {
        if self.dirty_rects.is_empty() {
            return true; // DirtyRect情報がない場合は常に処理
        }
        
        // ROIとDirtyRectの交差判定
        self.dirty_rects.iter().any(|rect| roi.intersects(rect))
    }
}
```

### DetectionResult

**目的**: 画像処理の検出結果を表現

```rust
pub struct DetectionResult {
    pub timestamp: Instant,
    pub detected: bool,     // 検出フラグ
    pub center_x: f32,      // 重心X座標（サブピクセル精度）
    pub center_y: f32,      // 重心Y座標
    pub coverage: u32,      // 検出領域の面積（ピクセル数）
}
```

**設計判断**:
- **center_x/center_yをf32**: OpenCVのmoments()がサブピクセル精度の重心を返す
  - 変更許容: パフォーマンス問題があればu32に変更可（精度は低下）
- **coverageをu32**: ピクセル数は整数値
- **detectedフラグ**: 検出なしの場合でもResultを返す（エラーではない）

**HID変換**:
```rust
// domain/ports.rsで定義
pub fn detection_to_hid_report(result: &DetectionResult) -> Vec<u8> {
    let mut report = vec![0u8; 16];
    report[0] = 0x01; // ReportID
    
    if result.detected {
        // タイムスタンプ（ミリ秒）
        let ts_ms = result.timestamp.elapsed().as_millis() as u32;
        report[1..5].copy_from_slice(&ts_ms.to_le_bytes());
        
        // 重心座標（u16にキャスト）
        let cx = result.center_x as u16;
        let cy = result.center_y as u16;
        report[5..7].copy_from_slice(&cx.to_le_bytes());
        report[7..9].copy_from_slice(&cy.to_le_bytes());
        
        // カバレッジ
        let coverage = result.coverage as u16;
        report[9..11].copy_from_slice(&coverage.to_le_bytes());
        
        report[11] = 1; // 検出フラグ
    }
    
    report
}
```

### ProcessorBackend

**目的**: 画像処理バックエンドを表現

```rust
pub enum ProcessorBackend {
    Cpu,      // CPU処理（Mat使用）
    OpenCl,   // OpenCL加速（UMat使用）
}
```

**設計判断**:
- **初期化時に判定**: 実行時の分岐コストを避ける
- **enumで表現**: 将来の拡張（CUDA, DirectML等）に対応可能

## ports.rs - trait定義

### CapturePort

**目的**: 画面キャプチャを抽象化

```rust
pub trait CapturePort: Send + Sync {
    fn capture_frame(&mut self) -> DomainResult<Option<Frame>>;
    fn reinitialize(&mut self) -> DomainResult<()>;
    fn device_info(&self) -> DeviceInfo;
}
```

**設計判断**:
- **capture_frame() → Option<Frame>**: タイムアウト時はNoneを返す（エラーではない）
- **&mut self**: キャプチャセッションは内部状態を持つ
- **reinitialize()**: 再初期化が必要な場合に呼び出し
  - 注意: **いつ呼び出すかはApplication層の責務**

**実装例（Infrastructure層）**:
```rust
pub struct DdaCaptureAdapter {
    dupl: Arc<Mutex<DesktopDuplicationApi>>,
    // ...
}

impl CapturePort for DdaCaptureAdapter {
    fn capture_frame(&mut self) -> DomainResult<Option<Frame>> {
        let mut guard = self.dupl.lock().unwrap();
        match guard.acquire_next_frame_now() {
            Ok(texture) => {
                // テクスチャをFrameに変換
                Ok(Some(frame))
            }
            Err(DDApiError::AccessLost | DDApiError::AccessDenied) => {
                // Recoverable error - Application層で再試行判断
                Err(DomainError::Capture("Access lost".to_string()))
            }
            Err(DDApiError::Unexpected(msg)) => {
                // Non-recoverable - Application層でreinitialize()呼び出し
                Err(DomainError::Capture(format!("Unexpected: {}", msg)))
            }
        }
    }
    
    fn reinitialize(&mut self) -> DomainResult<()> {
        // DDAセッションを再作成
        // ...
    }
}
```

### ProcessPort

**目的**: 画像処理を抽象化

```rust
pub trait ProcessPort: Send + Sync {
    fn process_frame(
        &mut self,
        frame: &Frame,
        roi: &Roi,
        hsv_range: &HsvRange,
    ) -> DomainResult<DetectionResult>;
    
    fn backend(&self) -> ProcessorBackend;
}
```

**設計判断**:
- **引数でROI/HsvRangeを受け取る**: 設定の変更を容易にする
- **&mut self**: OpenCL初期化などの内部状態を持つ可能性
- **backend()**: デバッグ/統計情報用

### CommPort

**目的**: HID通信を抽象化

```rust
pub trait CommPort: Send + Sync {
    fn send(&mut self, data: &[u8]) -> DomainResult<()>;
    fn is_connected(&self) -> bool;
    fn reconnect(&mut self) -> DomainResult<()>;
}
```

**設計判断**:
- **&[u8]で抽象化**: HIDレポート形式に依存しない
- **is_connected()**: ポーリングで接続状態確認
- **reconnect()**: デバイス再接続の試行

## error.rs - エラー型

### DomainError

**目的**: Domain層の統一エラー型

```rust
#[derive(Error, Debug)]
pub enum DomainError {
    #[error("Capture error: {0}")]
    Capture(String),
    
    #[error("Process error: {0}")]
    Process(String),
    
    #[error("Communication error: {0}")]
    Communication(String),
    
    #[error("Configuration error: {0}")]
    Configuration(String),
    
    #[error("Operation timed out: {0}")]
    Timeout(String),
}

pub type DomainResult<T> = Result<T, DomainError>;
```

**設計判断**:
- **thiserror使用**: 派生マクロでDisplay/Error実装が自動生成
- **String含む**: 詳細情報を保持（Context追加）
- **カテゴリ分け**: エラー種別で処理を分岐可能

## config.rs - 設定構造体

### AppConfig

**目的**: TOML設定ファイルの構造を表現

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppConfig {
    pub capture: CaptureConfig,
    pub process: ProcessConfig,
    pub communication: CommunicationConfig,
    pub pipeline: PipelineConfig,
}
```

**バリデーション**:
```rust
impl AppConfig {
    pub fn validate(&self) -> DomainResult<()> {
        // ROI範囲チェック
        if self.process.roi.width == 0 || self.process.roi.height == 0 {
            return Err(DomainError::Configuration(
                "ROI dimensions must be positive".to_string()
            ));
        }
        
        // HSV範囲チェック
        if self.process.hsv_range.h_min > 180 || self.process.hsv_range.h_max > 180 {
            return Err(DomainError::Configuration(
                "HSV H must be 0-180".to_string()
            ));
        }
        
        Ok(())
    }
}
```

## テスト戦略

### 単体テスト

**目標**: 100%カバレッジ

```rust
// types.rsの例
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_roi_area() {
        let roi = Roi::new(0, 0, 100, 100);
        assert_eq!(roi.area(), 10000);
    }
    
    #[test]
    fn test_hsv_range_bounds() {
        let hsv = HsvRange {
            h_min: 0,
            h_max: 180,
            s_min: 0,
            s_max: 255,
            v_min: 0,
            v_max: 255,
        };
        assert!(hsv.h_max <= 180);
    }
}
```

### 統合テスト

**目的**: trait実装の契約検証

```rust
// tests/domain_integration_test.rs
#[test]
fn test_detection_to_hid_report_format() {
    let result = DetectionResult {
        timestamp: Instant::now(),
        detected: true,
        center_x: 960.5,
        center_y: 540.3,
        coverage: 1000,
    };
    
    let report = detection_to_hid_report(&result);
    
    assert_eq!(report.len(), 16);
    assert_eq!(report[0], 0x01); // ReportID
    assert_eq!(report[11], 1);   // 検出フラグ
}
```

## 実装変更の許容範囲

### 変更OK（実装時の判断）

1. **Frame::dirty_rectsの実装**
   - Vec<Rect> → 最大N個のスタックバッファ
   - SmallVec使用でヒープアロケーション削減

2. **DetectionResult::center精度**
   - f32 → u32（パフォーマンス優先時）

3. **HIDレポート形式**
   - 16バイト → 可変長（デバイス仕様に応じて）

### 変更NG（アーキテクチャ違反）

1. **外部ライブラリ依存の追加**
   - Domain層は純粋Rustのみ

2. **traitのビジネスロジック実装**
   - traitは抽象化のみ、実装はInfrastructure層

3. **unwrap()の使用**
   - テスト以外では禁止

## まとめ

Domain層は**変更に強い**設計を目指しています：
- 外部依存なし → 技術選択の変更に影響されない
- 強い型付け → コンパイル時にエラー検出
- trait抽象化 → 実装の差し替えが容易
- 100%テスト → リファクタリングの安全性

実装時に不都合があれば、**このドキュメントも含めて変更OK**です。重要なのは、変更理由を記録することです。
