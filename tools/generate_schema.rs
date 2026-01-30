//! JSON Schema + Markdown生成ツール
//!
//! src/domain/config.rsの設定構造から以下を自動生成します：
//! 1. JSON Schema (schema/config.json)
//! 2. Markdownドキュメント (CONFIGURATION.md)
//!
//! 実行方法:
//! ```
//! cargo run --bin generate_schema
//! ```

use schemars::schema_for;
use serde_json::{Map, Value};
use std::fs;
use RoyaleWithCheese::domain::config::AppConfig;

fn main() {
    println!("JSON Schema + Markdown生成中...");

    // AppConfigからJSON Schemaを生成
    let schema = schema_for!(AppConfig);

    // JSON文字列に変換（prettify）
    let json = serde_json::to_string_pretty(&schema).expect("Failed to serialize schema to JSON");

    // schema/ディレクトリを作成
    fs::create_dir_all("schema").expect("Failed to create schema/ directory");

    // schema/config.jsonに書き出し
    fs::write("schema/config.json", json.clone()).expect("Failed to write schema/config.json");
    println!("  ✓ schema/config.json");

    // JSON Schemaをパースしてマークダウン生成
    let schema_value: Value =
        serde_json::from_str(&json).expect("Failed to parse generated schema");
    let markdown = generate_markdown(&schema_value);

    // CONFIGURATION.mdに書き出し
    fs::write("CONFIGURATION.md", markdown).expect("Failed to write CONFIGURATION.md");
    println!("  ✓ CONFIGURATION.md");

    println!("✅ 生成完了: schema/config.json + CONFIGURATION.md");
}

/// JSON Schemaからマークダウンドキュメントを生成
fn generate_markdown(schema: &Value) -> String {
    let mut md = String::new();

    // ヘッダー
    md.push_str("# 設定リファレンス (Configuration Reference)\n\n");

    md.push_str("## 概要\n\n");
    md.push_str("`config.toml`ファイルは、RoyaleWithCheeseの動作を制御する設定ファイルです。\n");
    md.push_str("JSON Schemaによる検証により、設定の正確性が保証されています。\n\n");

    md.push_str("**設定ファイルの場所**: `config.toml` (プロジェクトルート)  \n");
    md.push_str("**スキーマファイル**: `schema/config.json` (自動生成)  \n");
    md.push_str("**サンプル**: `config.toml.example`\n\n");

    md.push_str("⚠️ **注意**: このドキュメント（CONFIGURATION.md）は `cargo run --bin generate_schema` で自動生成されます。\n");
    md.push_str("設定項目の説明を変更する場合は、`src/domain/config.rs`のdoc commentsを編集してください。\n\n");

    md.push_str("## 設定ファイルの読み込み\n\n");
    md.push_str("- `config.toml`が存在する場合: ファイルから読み込み\n");
    md.push_str("- ファイルが存在しない場合: デフォルト値を使用（警告ログ出力）\n");
    md.push_str("- パース失敗時: デフォルト値を使用（警告ログ出力）\n\n");

    md.push_str("## 設定項目\n\n");

    // $defsを取得してマップを作成
    let defs = schema
        .get("$defs")
        .and_then(|d| d.as_object())
        .cloned()
        .unwrap_or_default();

    // トップレベルのプロパティを処理
    if let Some(props) = schema.get("properties").and_then(|p| p.as_object()) {
        for (key, prop) in props {
            generate_property_section(&mut md, key, prop, &defs);
        }
    }

    // フッター
    md.push_str("## 参考\n\n");
    md.push_str("- [docs/CLI_CONTRACT.md](docs/CLI_CONTRACT.md) - 実行時契約\n");
    md.push_str("- [README.md](README.md) - クイックスタート\n");

    md
}

/// プロパティセクションを生成
fn generate_property_section(
    md: &mut String,
    key: &str,
    schema: &Value,
    defs: &Map<String, Value>,
) {
    // セクション名をフォーマット
    let section_name = format_section_name(key);
    md.push_str(&format!("### [{}] - {}\n\n", key, section_name));

    // description取得
    if let Some(desc) = schema.get("description") {
        md.push_str(&format!("{}\n\n", desc.as_str().unwrap_or("")));
    }

    // $refの場合、定義を取得
    if let Some(ref_str) = schema.get("$ref").and_then(|r| r.as_str()) {
        if let Some(def_name) = ref_str.strip_prefix("#/$defs/") {
            if let Some(def_schema) = defs.get(def_name) {
                generate_properties_table(md, def_schema, defs, key);
            }
        }
    }

    // 直接プロパティを持つ場合
    if schema.get("properties").is_some() {
        generate_properties_table(md, schema, defs, key);
    }
}

/// プロパティテーブルを生成
fn generate_properties_table(
    md: &mut String,
    schema: &Value,
    defs: &Map<String, Value>,
    _parent_key: &str,
) {
    if let Some(props) = schema.get("properties").and_then(|p| p.as_object()) {
        if props.is_empty() {
            return;
        }

        // テーブルヘッダー
        md.push_str("| 設定項目 | 型 | デフォルト | 説明 |\n");
        md.push_str("|---------|-----|---------|---------|\n");

        for (prop_key, prop_schema) in props {
            let field_name = format!("`{}`", prop_key);
            let type_str = get_type_string(prop_schema, defs);
            let default = get_default_value(prop_schema);
            let description = get_description(prop_schema);

            // Escape pipes in type_str to prevent markdown table parsing issues
            let type_str_escaped = type_str.replace("|", "\\|");

            md.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                field_name, type_str_escaped, default, description
            ));
        }
        md.push_str("\n");

        // ネストされたオブジェクト（$ref を持つプロパティ）をサブセクションとして処理
        for (prop_key, prop_schema) in props {
            if let Some(ref_str) = prop_schema.get("$ref").and_then(|r| r.as_str()) {
                if let Some(def_name) = ref_str.strip_prefix("#/$defs/") {
                    // 定義が実際のオブジェクト（プロパティを持つ）かどうか確認
                    if let Some(def_schema) = defs.get(def_name) {
                        if def_schema.get("properties").is_some() {
                            let subsection_name = format_section_name(prop_key);
                            md.push_str(&format!("#### [{}] - {}\n\n", prop_key, subsection_name));

                            if let Some(desc) = def_schema.get("description") {
                                md.push_str(&format!("{}\n\n", desc.as_str().unwrap_or("")));
                            }

                            // 再帰的にネストされたプロパティを処理
                            generate_properties_table(md, def_schema, defs, prop_key);
                        }
                    }
                }
            }
        }
    }
}

/// 型を文字列で取得
fn get_type_string(schema: &Value, defs: &Map<String, Value>) -> String {
    // $ref の場合、参照先の型を確認
    if let Some(ref_str) = schema.get("$ref").and_then(|r| r.as_str()) {
        if let Some(def_name) = ref_str.strip_prefix("#/$defs/") {
            if let Some(def_schema) = defs.get(def_name) {
                // enum型の場合
                if def_schema.get("enum").is_some() {
                    return "enum".to_string();
                }
                // object型の場合
                if let Some(type_val) = def_schema.get("type").and_then(|t| t.as_str()) {
                    if type_val == "object" {
                        return "object".to_string();
                    }
                }
                // それ以外は参照名を返す
                return def_name.to_string();
            }
        }
    }

    // enum型の場合
    if let Some(enum_vals) = schema.get("enum").and_then(|e| e.as_array()) {
        if !enum_vals.is_empty() {
            return "enum".to_string();
        }
    }

    if let Some(type_val) = schema.get("type") {
        match type_val {
            Value::String(type_str) => {
                return match type_str.as_str() {
                    "string" => "string".to_string(),
                    "integer" => {
                        if let Some(format) = schema.get("format").and_then(|f| f.as_str()) {
                            format.to_string()
                        } else {
                            "integer".to_string()
                        }
                    }
                    "number" => {
                        if let Some(format) = schema.get("format").and_then(|f| f.as_str()) {
                            format.to_string()
                        } else {
                            "number".to_string()
                        }
                    }
                    "boolean" => "bool".to_string(),
                    "object" => "object".to_string(),
                    "array" => "array".to_string(),
                    _ => type_str.to_string(),
                };
            }
            Value::Array(types) => {
                // Union type (e.g., ["string", "null"])
                let type_strs: Vec<String> = types
                    .iter()
                    .filter_map(|t| {
                        t.as_str().and_then(|s| {
                            if s == "null" {
                                None
                            } else {
                                Some(s.to_string())
                            }
                        })
                    })
                    .collect();
                if !type_strs.is_empty() {
                    // Check if null is in the array (making it optional)
                    let has_null = types.iter().any(|t| t.as_str() == Some("null"));
                    let type_str = type_strs.join(" | ");
                    return if has_null {
                        format!("{} | null", type_str)
                    } else {
                        type_str
                    };
                }
            }
            _ => {}
        }
    }

    "unknown".to_string()
}

/// デフォルト値を取得
fn get_default_value(schema: &Value) -> String {
    if let Some(default) = schema.get("default") {
        match default {
            Value::String(s) => format!("`\"{}\"`", s),
            Value::Number(n) => format!("`{}`", n),
            Value::Bool(b) => format!("`{}`", b),
            Value::Null => "`null`".to_string(),
            _ => "-".to_string(),
        }
    } else {
        "-".to_string()
    }
}

/// 説明文を取得
fn get_description(schema: &Value) -> String {
    if let Some(desc) = schema.get("description") {
        if let Some(desc_str) = desc.as_str() {
            // 改行を<br>に、パイプをエスケープ
            let formatted = desc_str
                .replace("\n\n", "<br><br>")
                .replace("\n", " ")
                .replace("|", "\\|");
            return formatted;
        }
    }

    if let Some(enum_vals) = schema.get("enum").and_then(|e| e.as_array()) {
        let vals: Vec<String> = enum_vals
            .iter()
            .filter_map(|v| v.as_str().map(|s| format!("`{}`", s)))
            .collect();
        if !vals.is_empty() {
            return format!("値: {}", vals.join(", "));
        }
    }

    "-".to_string()
}

/// セクション名をフォーマット
fn format_section_name(key: &str) -> String {
    match key {
        "capture" => "キャプチャ設定".to_string(),
        "process" => "画像処理設定".to_string(),
        "communication" => "HID通信設定".to_string(),
        "activation" => "アクティベーション設定".to_string(),
        "audio_feedback" => "音声フィードバック設定".to_string(),
        "pipeline" => "パイプライン設定".to_string(),
        "roi" => "ROI設定".to_string(),
        "hsv_range" => "HSV色空間レンジ".to_string(),
        "coordinate_transform" => "座標変換設定".to_string(),
        _ => key.to_string(),
    }
}
