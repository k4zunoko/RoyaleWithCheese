# Release ビルド時のログ無効化

## 概要

Release ビルド時にはログ出力を完全に無効化し、ランタイムオーバーヘッドを0にしています。

## 実装方式

### `#[cfg(debug_assertions)]` による条件付きコンパイル

Rust標準の`debug_assertions`属性を使用して自動判定：

| ビルドモード | debug_assertions | ログ出力 | ランタイムオーバーヘッド |
|-----------|:---:|---------|:---:|
| **Debug** (`cargo build`) | 有効 | 非同期ファイル出力 | ~数十ns（メモリコピーのみ） |
| **Release** (`cargo build --release`) | 無効 | コンパイルアウト | **0** |

### ログ無効化の仕組み

**初期化関数の分岐**:
```rust
#[cfg(debug_assertions)]
pub fn init_logging(...) -> Option<WorkerGuard> { /* 実装 */ }

#[cfg(not(debug_assertions))]
pub fn init_logging(...) -> Option<()> { /* スタブ */ }
```

**ログマクロのコンパイルアウト**:
```rust
#[macro_export]
macro_rules! measure_span {
    ($name:expr, $body:expr) => {
        #[cfg(debug_assertions)]
        { let _span = tracing::info_span!($name).entered(); $body }
        #[cfg(not(debug_assertions))]
        { $body }  // Release時: 計測なし
    };
}
```

**計測ヘルパー**:
- Debug: `SpanTimer`は`Instant`を記録、`Drop`でログ出力
- Release: ダミー実装（オーバーヘッドなし）

### `debug_assertions`について

Rustが自動設定する条件属性。Feature Flagの明示的管理が不要で、標準的な慣行です。

[参考: Rust Reference - debug_assertions](https://doc.rust-lang.org/reference/conditional-compilation.html#debug_assertions)

## Release Profileの設定

```toml
[profile.release]
opt-level = 3          # 最大最適化
lto = true             # Link Time Optimization
codegen-units = 1      # 単一ユニット最適化
strip = true           # 不要なシンボル削除
```

**効果**:
- LTO: クロスモジュール最適化で未使用コード削除
- strip: デバッグシンボル削除（バイナリサイズ: 1.6MB → 110KB）
- opt-level=3: ループ展開、関数インライン化

## 効果まとめ

**Release時に削除されるもの**:
- ログ出力呼び出し（`tracing::info!`, `tracing::debug!`）
- 区間計測（`measure_span!`の計測処理）
- 統計計算（`MeasurementStats::add_sample()`）
- ファイルI/O、ログスレッド

**結果**:
- ランタイムオーバーヘッド: 0%
- バイナリサイズ削減: 88%（1.6MB → 110KB）
- Feature Flag不要（自動判定）

**注意**: `measure_span!`のbody部は実行されます（計測のみがコンパイルアウト）。

---

**更新履歴**:
- 2025-12-18: 初版作成
- 2026-01-08: 簡潔化
