//! Application configuration
//!
//! Manages loading and validation of application configuration from TOML files.
//! Configuration is immutable after startup and covers all pipeline components.

use crate::domain::error::{DomainError, DomainResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::Path;

// ============================================================================
// Configuration Sections
// ============================================================================

/// キャプチャ設定
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CaptureConfig {
    /// キャプチャソース: "dda" または "wgc"
    pub source: String,
    /// タイムアウト (ミリ秒)
    pub timeout_ms: u32,
    /// モニター番号
    pub monitor_index: u32,
}

/// 処理設定内の ROI (Region of Interest)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RoiConfig {
    /// ROI幅 (ピクセル)
    pub width: u32,
    /// ROI高さ (ピクセル)
    pub height: u32,
}

/// HSV色範囲設定
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HsvRangeConfig {
    /// Hue 最小値 [0-180]
    pub h_low: u8,
    /// Hue 最大値 [0-180]
    pub h_high: u8,
    /// Saturation 最小値 [0-255]
    pub s_low: u8,
    /// Saturation 最大値 [0-255]
    pub s_high: u8,
    /// Value 最小値 [0-255]
    pub v_low: u8,
    /// Value 最大値 [0-255]
    pub v_high: u8,
}

/// 座標変換設定
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CoordinateTransformConfig {
    /// 感度倍率
    pub sensitivity: f64,
    /// X軸クリップリミット
    pub x_clip_limit: f64,
    /// Y軸クリップリミット
    pub y_clip_limit: f64,
}

/// 処理モード
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ProcessMode {
    FastColor,
    FastColorGpu,
}

impl std::fmt::Display for ProcessMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProcessMode::FastColor => write!(f, "fast-color"),
            ProcessMode::FastColorGpu => write!(f, "fast-color-gpu"),
        }
    }
}

/// 画像処理設定
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProcessConfig {
    /// 処理モード: "fast-color" など
    pub mode: ProcessMode,
    /// ROI設定
    pub roi: RoiConfig,
    /// HSV色範囲設定
    pub hsv_range: HsvRangeConfig,
    /// 座標変換設定
    pub coordinate_transform: CoordinateTransformConfig,
}

/// 通信設定
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CommunicationConfig {
    /// USB Vendor ID
    pub vendor_id: u32,
    /// USB Product ID
    pub product_id: u32,
    /// HID送信間隔 (ミリ秒)
    pub hid_send_interval_ms: u32,
}

/// パイプライン設定
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PipelineConfig {
    /// 統計情報出力間隔 (秒)
    pub stats_interval_sec: u32,
}

/// デバッグ設定
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DebugConfig {
    /// デバッグモードを有効にするか
    pub enabled: bool,
}

// ============================================================================
// Application Configuration (Root)
// ============================================================================

/// アプリケーション全体の設定構造
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AppConfig {
    /// キャプチャ設定
    pub capture: CaptureConfig,
    /// 処理設定
    pub process: ProcessConfig,
    /// 通信設定
    pub communication: CommunicationConfig,
    /// パイプライン設定
    pub pipeline: PipelineConfig,
    /// デバッグ設定
    pub debug: DebugConfig,
}

impl AppConfig {
    /// TOMLファイルから設定を読み込む
    ///
    /// # Arguments
    /// * `path` - 設定ファイルのパス
    ///
    /// # Returns
    /// * `Ok(AppConfig)` - 読み込み成功
    /// * `Err(DomainError::Configuration)` - パース失敗または検証失敗
    pub fn from_file<P: AsRef<Path>>(path: P) -> DomainResult<Self> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path).map_err(|e| {
            DomainError::Configuration(format!("Failed to read config file: {}", e))
        })?;

        let config: AppConfig = toml::from_str(&content).map_err(|e| {
            DomainError::Configuration(format!("Failed to parse TOML config: {}", e))
        })?;

        config.validate()?;
        Ok(config)
    }

    /// 設定を検証する
    ///
    /// # Returns
    /// * `Ok(())` - 検証成功
    /// * `Err(DomainError::Configuration)` - 検証失敗
    pub fn validate(&self) -> DomainResult<()> {
        // キャプチャ設定の検証
        let valid_sources = ["dda", "wgc"];
        if !valid_sources.contains(&self.capture.source.as_str()) {
            return Err(DomainError::Configuration(format!(
                "Invalid capture source: {}. Must be one of: {}",
                self.capture.source,
                valid_sources.join(", ")
            )));
        }

        if self.capture.timeout_ms == 0 {
            return Err(DomainError::Configuration(
                "capture.timeout_ms must be > 0".to_string(),
            ));
        }

        // 処理設定の検証
        if self.process.roi.width == 0 || self.process.roi.height == 0 {
            return Err(DomainError::Configuration(
                "process.roi width and height must be > 0".to_string(),
            ));
        }

        // HSV範囲の検証
        if self.process.hsv_range.h_low > self.process.hsv_range.h_high {
            return Err(DomainError::Configuration(
                "process.hsv_range.h_low must be <= h_high".to_string(),
            ));
        }

        if self.process.hsv_range.s_low > self.process.hsv_range.s_high {
            return Err(DomainError::Configuration(
                "process.hsv_range.s_low must be <= s_high".to_string(),
            ));
        }

        if self.process.hsv_range.v_low > self.process.hsv_range.v_high {
            return Err(DomainError::Configuration(
                "process.hsv_range.v_low must be <= v_high".to_string(),
            ));
        }

        // HSV値が有効な範囲内か
        if self.process.hsv_range.h_high > 180 {
            return Err(DomainError::Configuration(
                "process.hsv_range.h_high must be <= 180".to_string(),
            ));
        }

        // 座標変換設定の検証
        if self.process.coordinate_transform.x_clip_limit <= 0.0 {
            return Err(DomainError::Configuration(
                "process.coordinate_transform.x_clip_limit must be > 0".to_string(),
            ));
        }

        if self.process.coordinate_transform.y_clip_limit <= 0.0 {
            return Err(DomainError::Configuration(
                "process.coordinate_transform.y_clip_limit must be > 0".to_string(),
            ));
        }

        if self.process.coordinate_transform.sensitivity <= 0.0 {
            return Err(DomainError::Configuration(
                "process.coordinate_transform.sensitivity must be > 0".to_string(),
            ));
        }

        // 通信設定の検証
        if self.communication.vendor_id == 0 || self.communication.product_id == 0 {
            return Err(DomainError::Configuration(
                "communication.vendor_id and product_id must be > 0".to_string(),
            ));
        }

        if self.communication.hid_send_interval_ms == 0 {
            return Err(DomainError::Configuration(
                "communication.hid_send_interval_ms must be > 0".to_string(),
            ));
        }

        // パイプライン設定の検証
        if self.pipeline.stats_interval_sec == 0 {
            return Err(DomainError::Configuration(
                "pipeline.stats_interval_sec must be > 0".to_string(),
            ));
        }

        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_config() -> AppConfig {
        let config: AppConfig = toml::from_str(include_str!("../../config.toml.example"))
            .expect("valid example config");
        config.validate().expect("example config should validate");
        config
    }

    // ========== Test: Valid Example Configuration ==========

    #[test]
    fn test_example_config_validates() {
        let config = valid_config();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_example_config_capture_section() {
        let config = valid_config();
        assert_eq!(config.capture.source, "dda");
        assert_eq!(config.capture.timeout_ms, 8);
    }

    #[test]
    fn test_example_config_process_mode() {
        let config = valid_config();
        assert_eq!(config.process.mode, ProcessMode::FastColor);
    }

    #[test]
    fn test_example_config_roi() {
        let config = valid_config();
        assert_eq!(config.process.roi.width, 460);
        assert_eq!(config.process.roi.height, 240);
    }

    #[test]
    fn test_example_config_hsv_range() {
        let config = valid_config();
        assert_eq!(config.process.hsv_range.h_low, 25);
        assert_eq!(config.process.hsv_range.h_high, 45);
        assert_eq!(config.process.hsv_range.s_low, 80);
        assert_eq!(config.process.hsv_range.s_high, 255);
        assert_eq!(config.process.hsv_range.v_low, 80);
        assert_eq!(config.process.hsv_range.v_high, 255);
    }

    #[test]
    fn test_example_config_communication() {
        let config = valid_config();
        assert_eq!(config.communication.vendor_id, 0x1234);
        assert_eq!(config.communication.product_id, 0x5678);
        assert_eq!(config.communication.hid_send_interval_ms, 8);
    }

    // ========== Test: TOML Parsing (Valid) ==========

    #[test]
    fn test_parse_minimal_valid_toml() {
        let toml_str = r#"
[capture]
source = "dda"
timeout_ms = 8
monitor_index = 0

[process]
mode = "fast-color"

[process.roi]
width = 200
height = 200

[process.hsv_range]
h_low = 25
h_high = 45
s_low = 80
s_high = 255
v_low = 80
v_high = 255

[process.coordinate_transform]
sensitivity = 1.0
x_clip_limit = 10.0
y_clip_limit = 10.0

[communication]
vendor_id = 0x1234
product_id = 0x5678
hid_send_interval_ms = 8

[pipeline]
stats_interval_sec = 10

[debug]
enabled = false
"#;

        let config: AppConfig = toml::from_str(toml_str).expect("Failed to parse TOML");
        assert_eq!(config.capture.source, "dda");
        assert_eq!(config.process.roi.width, 200);
        assert_eq!(config.communication.vendor_id, 0x1234);
    }

    #[test]
    fn test_parse_wgc_source() {
        let toml_str = r#"
[capture]
source = "wgc"
timeout_ms = 8
monitor_index = 0

[process]
mode = "fast-color"

[process.roi]
width = 200
height = 200

[process.hsv_range]
h_low = 25
h_high = 45
s_low = 80
s_high = 255
v_low = 80
v_high = 255

[process.coordinate_transform]
sensitivity = 1.0
x_clip_limit = 10.0
y_clip_limit = 10.0

[communication]
vendor_id = 0x1234
product_id = 0x5678
hid_send_interval_ms = 8

[pipeline]
stats_interval_sec = 10

[debug]
enabled = false
"#;

        let config: AppConfig = toml::from_str(toml_str).expect("Failed to parse TOML");
        assert_eq!(config.capture.source, "wgc");
    }

    #[test]
    fn test_parse_invalid_process_mode() {
        let toml_str = r#"
[capture]
source = "dda"
timeout_ms = 8
monitor_index = 0

[process]
mode = "invalid"

[process.roi]
width = 200
height = 200

[process.hsv_range]
h_low = 25
h_high = 45
s_low = 80
s_high = 255
v_low = 80
v_high = 255

[process.coordinate_transform]
sensitivity = 1.0
x_clip_limit = 10.0
y_clip_limit = 10.0

[communication]
vendor_id = 0x1234
product_id = 0x5678
hid_send_interval_ms = 8

[pipeline]
stats_interval_sec = 10

[debug]
enabled = false
"#;

        let result: Result<AppConfig, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }

    // ========== Test: Validation - Capture Source ==========

    #[test]
    fn test_validate_invalid_capture_source() {
        let mut config = valid_config();
        config.capture.source = "spout".to_string();
        let result = config.validate();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Invalid capture source"));
    }

    #[test]
    fn test_validate_capture_timeout_zero() {
        let mut config = valid_config();
        config.capture.timeout_ms = 0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("timeout_ms must be > 0"));
    }

    // ========== Test: Validation - ROI ==========

    #[test]
    fn test_validate_roi_width_zero() {
        let mut config = valid_config();
        config.process.roi.width = 0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("width and height must be > 0"));
    }

    #[test]
    fn test_validate_roi_height_zero() {
        let mut config = valid_config();
        config.process.roi.height = 0;
        let result = config.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_roi_both_positive() {
        let mut config = valid_config();
        config.process.roi.width = 1;
        config.process.roi.height = 1;
        assert!(config.validate().is_ok());
    }

    // ========== Test: Validation - HSV Range ==========

    #[test]
    fn test_validate_hsv_h_low_greater_than_h_high() {
        let mut config = valid_config();
        config.process.hsv_range.h_low = 50;
        config.process.hsv_range.h_high = 40;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("h_low must be <= h_high"));
    }

    #[test]
    fn test_validate_hsv_s_low_greater_than_s_high() {
        let mut config = valid_config();
        config.process.hsv_range.s_low = 200;
        config.process.hsv_range.s_high = 100;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("s_low must be <= s_high"));
    }

    #[test]
    fn test_validate_hsv_v_low_greater_than_v_high() {
        let mut config = valid_config();
        config.process.hsv_range.v_low = 200;
        config.process.hsv_range.v_high = 100;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("v_low must be <= v_high"));
    }

    #[test]
    fn test_validate_hsv_h_high_exceeds_180() {
        let mut config = valid_config();
        config.process.hsv_range.h_high = 181;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("h_high must be <= 180"));
    }

    // ========== Test: Validation - Communication ==========

    #[test]
    fn test_validate_communication_vendor_id_zero() {
        let mut config = valid_config();
        config.communication.vendor_id = 0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("vendor_id and product_id must be > 0"));
    }

    #[test]
    fn test_validate_communication_product_id_zero() {
        let mut config = valid_config();
        config.communication.product_id = 0;
        let result = config.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_communication_hid_send_interval_zero() {
        let mut config = valid_config();
        config.communication.hid_send_interval_ms = 0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("hid_send_interval_ms must be > 0"));
    }

    // ========== Test: Validation - Coordinate Transform ==========

    #[test]
    fn test_validate_coordinate_transform_x_clip_zero() {
        let mut config = valid_config();
        config.process.coordinate_transform.x_clip_limit = 0.0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("x_clip_limit must be > 0"));
    }

    #[test]
    fn test_validate_coordinate_transform_y_clip_zero() {
        let mut config = valid_config();
        config.process.coordinate_transform.y_clip_limit = 0.0;
        let result = config.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_coordinate_transform_sensitivity_zero() {
        let mut config = valid_config();
        config.process.coordinate_transform.sensitivity = 0.0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("sensitivity must be > 0"));
    }

    // ========== Test: Validation - Pipeline ==========

    #[test]
    fn test_validate_pipeline_stats_interval_zero() {
        let mut config = valid_config();
        config.pipeline.stats_interval_sec = 0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("stats_interval_sec must be > 0"));
    }

    // ========== Test: Clone and Debug Traits ==========

    #[test]
    fn test_config_clone() {
        let config = valid_config();
        let cloned = config.clone();
        assert_eq!(config.capture.source, cloned.capture.source);
        assert_eq!(config.process.roi.width, cloned.process.roi.width);
    }

    #[test]
    fn test_config_debug_format() {
        let config = valid_config();
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("AppConfig"));
        assert!(debug_str.contains("dda"));
    }
}
