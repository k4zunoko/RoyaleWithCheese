//! JSON Schema生成ツール
//!
//! src/domain/config.rsの設定構造からJSON Schemaを生成します。
//!
//! 実行方法:
//! ```
//! cargo run --bin generate_schema
//! ```

use schemars::schema_for;
use std::fs;
use RoyaleWithCheese::domain::config::AppConfig;

fn main() {
    println!("JSON Schema生成中...");

    // AppConfigからJSON Schemaを生成
    let schema = schema_for!(AppConfig);

    // JSON文字列に変換（prettify）
    let json = serde_json::to_string_pretty(&schema).expect("Failed to serialize schema to JSON");

    // schema/ディレクトリを作成
    fs::create_dir_all("schema").expect("Failed to create schema/ directory");

    // schema/config.jsonに書き出し
    fs::write("schema/config.json", json).expect("Failed to write schema/config.json");

    println!("✅ Schema生成完了: schema/config.json");
}
