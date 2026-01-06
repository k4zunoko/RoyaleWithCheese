# 設計理念とアーキテクチャ指針

## プロジェクト概要

**RoyaleWithCheese**は、Windows環境でDXGI Desktop Duplication APIを使用した低レイテンシ画面キャプチャ・画像処理・HID通信を行うRustプロジェクトです。

## 設計理念

### 1. Clean Architecture

**依存の方向**: 常に内側（Domain）へ

```
┌───────────────────────────────┐
│     Presentation (main.rs)     │  設定読込・DI・起動制御
└──────────────▲────────────────┘
               │ DI注入
┌──────────────┴────────────────┐
│  Application (UseCase)         │  パイプライン制御・ポリシー
└──────────────▲────────────────┘
               │ trait依存
┌──────────────┴────────────────┐
│  Domain (Core)                 │  純粋ビジネスロジック
└──────────────▲────────────────┘
               │ trait実装
┌──────────────┴────────────────┐
│  Infrastructure/Adapters       │  外部技術（DDA/OpenCV/HID）
└───────────────────────────────┘
```

**重要な原則**:
- **Domain層**: 外部依存なし、純粋なRust型のみ
- **Application層**: Domainのtraitにのみ依存
- **Infrastructure層**: Domainのtraitを実装するが、Application/Domainに依存しない
- **Presentation層**: DIでInfrastructureの実装を注入

### 2. レイテンシ最適化戦略

このプロジェクトの**最優先事項**はレイテンシ最小化です。

#### 最適化技術

**ROI限定処理**:
- 全画面ではなく中心領域のみ処理
- 処理ピクセル数を大幅削減

**DirtyRect最適化**:
- 更新がない場合は処理スキップ
- 静止フレームのオーバーヘッドを削減

**最新のみポリシー** (`bounded(1)` キュー):
- 古いフレームは破棄して最新のみ処理
- バックプレッシャを回避
- メモリ使用量を一定に保つ

**非同期ログ出力**:
- メインスレッドはメモリコピーのみ（~数十ns）
- ファイルI/Oは別スレッドで実行
- Debug buildのみ有効、Release buildは完全削除

**OpenCL加速**:
- 初期化時に一度だけ判定
- 実行時の分岐コストなし
- UMat/Matを使い分け

### 3. 型安全性と表現力

**強い型付け**:
```rust
pub struct Roi { /* ... */ }
pub struct HsvRange { /* ... */ }
pub struct Frame { /* ... */ }
```
- プリミティブ型の直接使用を避ける
- ドメイン概念を型で表現
- コンパイル時に誤用を防止

**Result型の一貫使用**:
```rust
pub type DomainResult<T> = Result<T, DomainError>;
```
- unwrap()禁止（Presentationの試行コードのみ例外）
- エラー伝播を明示的に
- 回復可能性を型で表現

### 4. trait による抽象化

**Port（Clean Architectureのインターフェース）**:
```rust
pub trait CapturePort: Send + Sync {
    fn capture_frame(&mut self) -> DomainResult<Option<Frame>>;
    fn reinitialize(&mut self) -> DomainResult<()>;
    fn device_info(&self) -> DeviceInfo;
}

pub trait ProcessPort: Send + Sync {
    fn process_frame(&mut self, frame: &Frame, roi: &Roi, hsv_range: &HsvRange) 
        -> DomainResult<DetectionResult>;
    fn backend(&self) -> ProcessorBackend;
}

pub trait CommPort: Send + Sync {
    fn send(&mut self, data: &[u8]) -> DomainResult<()>;
    fn is_connected(&self) -> bool;
    fn reconnect(&mut self) -> DomainResult<()>;
}
```

**利点**:
- テスト時にモック注入が容易
- 実装の差し替えが可能（DDA → WGC、OpenCV → YOLO）
- Infrastructure層の技術選択が自由

## レイヤー責務の詳細

### Domain層
- **責務**: ビジネスロジックとドメイン知識
- **含むもの**: 型定義、trait定義、エラー型、設定構造体
- **含まないもの**: 外部ライブラリ依存、I/O、並行処理
- **テスト**: 純粋関数の単体テスト、100%カバレッジ目標

### Application層
- **責務**: ユースケース実行、パイプライン制御、リカバリーポリシー
- **含むもの**: スレッド管理、チャネル制御、統計収集、再初期化ロジック
- **含まないもの**: 具体的な外部技術（DDA/OpenCV/HID）
- **テスト**: モック注入による統合テスト

### Infrastructure層
- **責務**: 外部技術との接続、trait実装
- **含むもの**: DDA/OpenCV/HID/ORTの具体的な実装
- **含まないもの**: ビジネスロジック、ポリシー決定
- **テスト**: trait契約の検証、実環境での動作確認

### Presentation層
- **責務**: 起動制御、設定読込、DI組み立て
- **含むもの**: main.rs、設定ファイル読込、ログ初期化
- **含まないもの**: ビジネスロジック、技術的詳細
- **テスト**: 起動シーケンスの検証

## 技術選択の理由

### DXGI Desktop Duplication API
- **選択理由**: 低レイテンシ、フルスクリーンDirectXアプリ対応
- **代替案**: Windows Graphics Capture (WGC) - MPO再合成に強いがレイテンシが高い
- **トレードオフ**: 管理者権限必要、DRM保護コンテンツは黒画面

### crossbeam-channel
- **選択理由**: bounded(1)で最新のみポリシーを実現
- **代替案**: std::sync::mpsc - unboundedのため制御が難しい
- **トレードオフ**: 外部依存だが、性能とAPIの利便性が高い

### tracing + tracing-subscriber
- **選択理由**: 構造化ログ、非同期出力、条件付きコンパイル対応
- **代替案**: log + env_logger - シンプルだが構造化ログ不可
- **トレードオフ**: やや複雑だが、パフォーマンス計測に最適

### thiserror
- **選択理由**: 派生マクロでエラー型定義が簡潔
- **代替案**: anyhow - Context追加に便利だが型安全性が低い
- **トレードオフ**: Domain層はthiserror、Application層以降はanyhowも併用可能

## 設計判断の記録

### Domain層の型設計

**決定**: Frame型にdirty_rectsをVec<Rect>で保持
**理由**: DDAは複数のDirtyRectを返す可能性がある
**代替案**: Option<Rect>（最初の実装）- 複数対応が困難
**変更許容**: 実装時に不都合があれば変更OK（例: 最大N個に制限、Iterator化など）

**決定**: DetectionResultのcenter_x/center_yをf32で保持
**理由**: サブピクセル精度の重心計算に対応
**代替案**: u32整数型 - 精度が不足
**変更許容**: パフォーマンス上の問題があればu32に変更可

### Application層の再初期化ロジック配置

**決定**: Application層でDDAの再初期化ロジックを実装
**理由**:
- **いつ・どのように再初期化するか**はビジネスロジック（ポリシー判断）
- Infrastructure層はtraitの実装に徹し、エラー型を返すのみ
- 指数バックオフ・累積失敗監視などのポリシーはApplication層の責務

**代替案**: Infrastructure層に再初期化ロジックを含める
**却下理由**: ポリシー判断が外部技術層に漏れ出す（関心の分離違反）

**根拠**: win_desktop_duplicationクレートのエラー型
- Recoverable: AccessLost (デスクトップモード変更), AccessDenied (セキュア環境) → 再試行可能
- Non-recoverable: Unexpected (予期しないエラー) → インスタンス再作成必要

### bounded(1)キューの「最新のみ」実装

**決定**: try_send()でFull時は単に無視（受信側が古いデータを破棄）
**理由**: Senderからは古いデータを取り出せない（crossbeam-channelの仕様）
**代替案**: unbounded + 定期的なクリア - メモリ増加リスク
**トレードオフ**: 満杯時の1フレーム遅延 vs メモリ安全性 → 後者を優先

## 実装ガイドライン

### パフォーマンス指針

**目標**: End-to-End < 6.9ms @ 144Hz（10ms @ 100Hz）

**最適化優先順位**:
1. アルゴリズム選択（O記法レベル）
2. メモリアロケーション削減
3. キャッシュ効率
4. マイクロ最適化

**避けるべきパターン**:
- ホットパスでのallocation
- 不要なclone()
- 実行時の型判定（初期化時に判定してキャッシュ）
- 同期ログ出力

### コーディング原則

- **unwrap()禁止**: Result型で明示的なエラー処理
- **unsafe境界**: FFI部分のみに限定し、`// SAFETY:` コメント必須
- **テスト**: Domain層100%カバレッジ目標、Application層はモック注入
- **ロギング**: Debug buildのみ有効、Release buildは完全削除（#[cfg(debug_assertions)]）

## 将来の拡張性

### YOLO統合（Phase 4）
- **切り替え**: feature flagで fast-color / yolo-ort を選択
- **共通化**: 同じDDA capture、同じROI抽出を再利用
- **実装**: ProcessPort traitの別実装として追加
- **EP選択**: CPU → CUDA → TensorRT の段階的移行

### マルチモニタ対応
- **設定**: monitor_index を追加
- **実装**: CapturePort::device_info()でモニタ列挙
- **制約**: 同時キャプチャは1モニタのみ（DXGI制約）

### HIDプロトコル拡張
- **抽象化**: CommPort traitで既に抽象化済み
- **実装**: 新しいHID実装を追加してDI注入
- **例**: シリアル通信、WebSocket、TCP/IP

## 参考資料

- [DXGI Desktop Duplication API (Microsoft Learn)](https://learn.microsoft.com/en-us/windows-hardware/drivers/display/desktop-duplication-api)
- [DXGI vs WGC 比較](https://sageinfinity.github.io/docs/FAQ/dxgiwgc)
- [OpenCV OpenCL (UMat)](https://docs.opencv.org/4.x/dc/d83/group__core__opencl.html)
- [ONNX Runtime Rust (ort)](https://docs.rs/ort/latest/ort/)
- [Clean Architecture (Robert C. Martin)](https://blog.cleancoder.com/uncle-bob/2012/08/13/the-clean-architecture.html)
