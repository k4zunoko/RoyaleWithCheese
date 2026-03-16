# AGENTS.md

`C:\Programs\RoyaleWithCheese` で作業するコーディングエージェント向けのリポジトリガイダンス。

## Review guidelines
- Always review in Japanese

---

## スコープ

-   このガイドはリポジトリ全体に適用されます。
-   これは Windows 向けのキャプチャ／処理／HID パイプラインのための Rust 2021 Cargo プロジェクトです。
-   このプロジェクトは内部コードを理解できるプログラマが利用することを想定しているので、内部の状態や処理構造を、そのまま理解できることを優先したシステム中心設計になっています。

---

## リポジトリ構成

-   `src/application/` — オーケストレーション、スレッド、メトリクス、リカバリー、ランタイム状態
-   `src/domain/` — コア型、ポート、設定、エラーモデル
-   `src/infrastructure/` — キャプチャ、処理、入力、HID、および GPU の具体的アダプタ
-   `tests/` — GPU／ディスプレイなどのハードウェア依存テストを含む統合テスト
-   `config.toml.example` および `CONFIGURATION.md` — ランタイム設定ドキュメント
-   `.cargo/config.toml` — LLVM / libclang / OpenCV のローカル環境設定

---

## 環境およびネイティブ依存関係

-   コマンドはリポジトリのルートから実行してください。
-   Cargo は `.cargo/config.toml` から必要な環境変数を継承します。
-   主な環境変数（既に設定済み）:
    -   `LIBCLANG_PATH`
    -   `LLVM_CONFIG_PATH`
    -   `CLANG_PATH`
    -   `OPENCV_INCLUDE_PATHS`
    -   `OPENCV_LINK_PATHS`
    -   `OPENCV_LINK_LIBS`
-   このリポジトリは Windows 向けであり、`third_party/` 配下のアセットに依存しています。

---

## デフォルトコマンド

Cargo を基本のコマンドインターフェースとして使用してください。

```powershell
# 高速コンパイルチェック
cargo check

# デバッグおよびリリースビルド
cargo build
cargo build --release

# フォーマット
cargo fmt
cargo fmt -- --check

# Lint
cargo clippy --all-targets -- -D warnings

# テスト
cargo test
cargo test --lib
cargo test --tests
```

---

## 単一テストの実行

このリポジトリには `src/--` 内のユニットテストと、`tests/-.rs` の統合テストがあります。

### 単一ユニットテスト

```powershell
cargo test application::pipeline::tests::pipeline_construction_succeeds -- --exact --nocapture
```

-   `--` の前は Cargo のテストフィルタです。
-   `--exact` は部分一致を防ぎます。
-   `--nocapture` はデバッグ出力に有用です。

### 単一の統合テストクレート

```powershell
cargo test --test pipeline_integration
```

### 統合テスト内の単一テスト

```powershell
cargo test --test pipeline_integration mock_pipeline_runs_and_stops -- --exact --nocapture
```

### 無効化されたハードウェアテスト

```powershell
cargo test real_hardware_pipeline_smoke_test -- --ignored --nocapture --test-threads=1
```

タイミング依存またはハードウェア依存テストには `--test-threads=1` を使用してください。

---

## テスト規約

-   ユニットテストは通常 `#[cfg(test)] mod tests` 内に配置されます。
-   統合テストは `tests/` に配置されます。
-   ハードウェア依存テストは `#[ignore = "..."]` が付与されています。
-   CI 対応のため、モックベースのテストを推奨します。
-   モックは一般的に以下を実装します:
    -   `CapturePort`
    -   `ProcessPort`
    -   `CommPort`
    -   `InputPort`
-   GPU／ディスプレイ／HID テストは、環境が対応していない限り有効化しないでください。

---

## インポートとファイル構成

-   インポートは論理グループごとに空行で区切ります。
-   一般的な順序:
    1.  `std::...`
    2.  クレート内 (`crate::...` または `RoyaleWithCheese::...`)
    3.  サードパーティクレート
-   一部のファイルでは順序が異なるため、周囲のスタイルに合わせてください。
-   ワイルドカードインポートは避けてください。
-   例外: `opencv::prelude::-`
-   アーキテクチャ境界を維持してください:
    -   抽象: `domain`
    -   オーケストレーション: `application`
    -   実装: `infrastructure`
-   `main.rs` は起動および配線処理のみに限定してください。

---

## フォーマット規約

-   rustfmt のデフォルトに従います。
-   複数行の構造体／列挙型／呼び出しには末尾カンマを使用します。
-   文にはセミコロン、末尾式には付けません。
-   文字列はダブルクォートを使用します。
-   コメントは短く、対象コードの近くに配置します。

---

## 命名規約

-   型、列挙型、トレイト: `PascalCase`
-   関数、メソッド、モジュール、フィールド: `snake_case`
-   設定構造体: `-Config`
-   アダプタ実装: `-Adapter`
-   トレイト抽象: `-Port`
-   テストは説明的な `snake_case`
-   クレート名 `RoyaleWithCheese` は意図的なもの（`src/lib.rs` に例外あり）

---

## 型および API 設計

-   フォールバック可能な API には `DomainError` と `DomainResult<T>` を優先します。
-   小さなコンストラクタを使用:
    -   `Roi::new`
    -   `HsvRange::new`
    -   `Frame::new`
    -   `DeviceInfo::new`
-   `#[derive(Debug, Clone)]` を優先し、必要な場合のみ `Copy` 等を追加。
-   ランタイム共有状態は `Arc` + アトミックで管理。
-   トレイトオブジェクトや enum dispatch は既存設計に従う。
-   網羅的な enum マッチを維持し、ワイルドカードは避ける。

---

## エラーハンドリング

-   本番コードでの `unwrap()` や `expect()` は避けてください。
-   外部ライブラリエラーは `map_err(...)` で `DomainError` に変換。
-   その後 `?` を使用。
-   GPU 初期化失敗時の CPU フォールバック等、既存挙動を維持。
-   問題は `tracing` でログ出力。
-   テストや `build.rs` は例外的に `unwrap` 可（本番コードには持ち込まない）。

---

## ログと設定

-   構造化ログには `tracing` を使用。
-   Debug ビルドは詳細、Release は簡潔。
-   `performance-timing` feature でタイミングログ有効化。
-   設定は `config.toml` からロード。
-   `config.toml` の読み込み・パース・検証に失敗した場合はエラー終了させ、暗黙のデフォルトフォールバックを追加しないでください。
-   検証は `AppConfig::validate()` を使用。
-   対応キャプチャソース:
    -   `dda`
    -   `wgc`

---

## リポジトリ固有の注意

-   `.github/workflows/` に CI は存在しません。
-   `scripts/create-release-tag.ps1` は `package.json` を参照しますが、このリポジトリには存在しません（古いスクリプト）。
-   ネイティブ／GPU／ディスプレイテストは環境によって失敗する可能性があります。

---

## 推奨検証フロー

```powershell
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

変更箇所が限定的な場合は、該当テストのみ実行してください。

---

## コード編集時

-   インポート順序やコメント量は周囲に合わせる。
-   可能なら変更箇所付近にテストを追加／更新。
-   パイプライン挙動にはモックテストを優先。
-   新しいコマンドやワークフローを追加した場合は、このファイルを更新してください。
