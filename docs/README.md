# docs/ 目次（まず読む）

このフォルダは **RoyaleWithCheese のプロジェクト情報を集約**するためのドキュメント群です。
詳細は docs/ 配下に置き、ルートの AGENTS.md は **索引（地図）**として最小限に保ちます。

## 読む順番（おすすめ）

1. [Architecture.md](Architecture.md) — プロジェクト全体の目的・構成の俯瞰
2. [DESIGN_PHILOSOPHY.md](DESIGN_PHILOSOPHY.md) — 設計原則（Clean Architecture / レイテンシ最適化）
3. [REQUIREMENTS.md](REQUIREMENTS.md) — 要求仕様（機能/非機能、スコープ）
4. [CLI_CONTRACT.md](CLI_CONTRACT.md) — 実行時のふるまい・設定ファイル契約
5. レイヤ別詳細
   - [DOMAIN_LAYER.md](DOMAIN_LAYER.md)
   - [APPLICATION_LAYER.md](APPLICATION_LAYER.md)
   - [INFRASTRUCTURE_CAPTURE.md](INFRASTRUCTURE_CAPTURE.md) — DDAキャプチャ実装
   - [INFRASTRUCTURE_SPOUT.md](INFRASTRUCTURE_SPOUT.md) — Spoutテクスチャ受信
6. 横断的トピック
   - [ERROR_HANDLING.md](ERROR_HANDLING.md)
   - [TESTING_STRATEGY.md](TESTING_STRATEGY.md)
   - [LOGGING.md](LOGGING.md)
   - [RELEASE_BUILD.md](RELEASE_BUILD.md)
7. 開発・調整
   - [VISUAL_DEBUG_GUIDE.md](VISUAL_DEBUG_GUIDE.md)
8. 現状と今後
   - [ROADMAP.md](ROADMAP.md)

## ルートファイルとの関係

- ルートの README.md: まず動かすためのコマンド例（クイックスタート）
- config.toml.example: 設定ファイルの例（TOML契約の実例）

> 注意: 同じ情報を複数ファイルに重複して書かない方針です。内容が重複しそうな場合は、本文は1箇所に集約し、他はリンク参照にします。
