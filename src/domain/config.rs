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
    /// 最大連続タイムアウト回数
    pub max_consecutive_timeouts: u32,
    /// 再初期化初期遅延 (ミリ秒)
    pub reinit_initial_delay_ms: u32,
    /// 再初期化最大遅延 (ミリ秒)
    pub reinit_max_delay_ms: u32,
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
    /// デッドゾーン
    pub dead_zone: f64,
}

/// 画像処理設定
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProcessConfig {
    /// 処理モード: "fast-color" など
    pub mode: String,
    /// 最小検出エリア
    pub min_detection_area: u32,
    /// 検出方法: "moments" など
    pub detection_method: String,
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
    /// ダーティレクト最適化の有効化
    pub enable_dirty_rect_optimization: bool,
    /// 統計情報出力間隔 (秒)
    pub stats_interval_sec: u32,
}

/// アクティベーション設定
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ActivationConfig {
    /// 中心からの最大距離
    pub max_distance_from_center: f64,
    /// アクティブウィンドウ期間 (ミリ秒)
    pub active_window_ms: u32,
}

/// 音声フィードバック設定
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AudioFeedbackConfig {
    /// 音声フィードバック有効化
    pub enabled: bool,
    /// オン効果音パス
    pub on_sound: String,
    /// オフ効果音パス
    pub off_sound: String,
    /// 無音モードへのフォールバック
    pub fallback_to_silent: bool,
}

/// GPU処理設定
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GpuConfig {
    /// GPU処理を有効にするか
    pub enabled: bool,
    /// 使用するGPUデバイスインデックス
    pub device_index: u32,
    /// GPU処理を優先するか
    pub prefer_gpu: bool,
}

/// デバッグ設定
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DebugConfig {
    /// デバッグモードを有効にするか
    #[serde(default)]
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
    /// アクティベーション設定
    pub activation: ActivationConfig,
    /// 音声フィードバック設定
    pub audio_feedback: AudioFeedbackConfig,
    /// GPU処理設定
    pub gpu: GpuConfig,
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
        let valid_sources = vec!["dda", "wgc"];
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

        if self.capture.max_consecutive_timeouts == 0 {
            return Err(DomainError::Configuration(
                "capture.max_consecutive_timeouts must be > 0".to_string(),
            ));
        }

        if self.capture.reinit_initial_delay_ms == 0 {
            return Err(DomainError::Configuration(
                "capture.reinit_initial_delay_ms must be > 0".to_string(),
            ));
        }

        if self.capture.reinit_max_delay_ms == 0 {
            return Err(DomainError::Configuration(
                "capture.reinit_max_delay_ms must be > 0".to_string(),
            ));
        }

        if self.capture.reinit_max_delay_ms < self.capture.reinit_initial_delay_ms {
            return Err(DomainError::Configuration(
                "capture.reinit_max_delay_ms must be >= reinit_initial_delay_ms".to_string(),
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

        // アクティベーション設定の検証
        if self.activation.max_distance_from_center <= 0.0 {
            return Err(DomainError::Configuration(
                "activation.max_distance_from_center must be > 0".to_string(),
            ));
        }

        if self.activation.active_window_ms == 0 {
            return Err(DomainError::Configuration(
                "activation.active_window_ms must be > 0".to_string(),
            ));
        }

        // GPU設定の検証
        if self.gpu.device_index > 15 {
            // Typically 16 GPUs max
            return Err(DomainError::Configuration(
                "gpu.device_index must be <= 15".to_string(),
            ));
        }

        Ok(())
    }

    /// デフォルト設定を取得する
    ///
    /// すべてのデフォルト値は validate() を通過します。
    pub fn default() -> Self {
        Self {
            capture: CaptureConfig {
                source: "dda".to_string(),
                timeout_ms: 8,
                max_consecutive_timeouts: 120,
                reinit_initial_delay_ms: 100,
                reinit_max_delay_ms: 5000,
                monitor_index: 0,
            },
            process: ProcessConfig {
                mode: "fast-color".to_string(),
                min_detection_area: 0,
                detection_method: "moments".to_string(),
                roi: RoiConfig {
                    width: 460,
                    height: 240,
                },
                hsv_range: HsvRangeConfig {
                    h_low: 25,
                    h_high: 45,
                    s_low: 80,
                    s_high: 255,
                    v_low: 80,
                    v_high: 255,
                },
                coordinate_transform: CoordinateTransformConfig {
                    sensitivity: 1.0,
                    x_clip_limit: 10.0,
                    y_clip_limit: 10.0,
                    dead_zone: 0.0,
                },
            },
            communication: CommunicationConfig {
                vendor_id: 0x1234,
                product_id: 0x5678,
                hid_send_interval_ms: 8,
            },
            pipeline: PipelineConfig {
                enable_dirty_rect_optimization: false,
                stats_interval_sec: 10,
            },
            activation: ActivationConfig {
                max_distance_from_center: 5.0,
                active_window_ms: 500,
            },
            audio_feedback: AudioFeedbackConfig {
                enabled: true,
                on_sound: "C:\\Windows\\Media\\Speech On.wav".to_string(),
                off_sound: "C:\\Windows\\Media\\Speech Off.wav".to_string(),
                fallback_to_silent: true,
            },
            gpu: GpuConfig {
                enabled: false,
                device_index: 0,
                prefer_gpu: false,
            },
            debug: DebugConfig { enabled: false },
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // Helper: Create a temporary test config TOML file
    fn create_temp_toml(content: &str) -> (tempfile::NamedTempFile, std::path::PathBuf) {
        use std::io::Write;
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();
        file.flush().unwrap();
        let path = file.path().to_path_buf();
        (file, path)
    }

    // ========== Test: Default Configuration ==========

    #[test]
    fn test_default_config_validates() {
        let config = AppConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_default_config_capture_section() {
        let config = AppConfig::default();
        assert_eq!(config.capture.source, "dda");
        assert_eq!(config.capture.timeout_ms, 8);
        assert_eq!(config.capture.max_consecutive_timeouts, 120);
    }

    #[test]
    fn test_default_config_roi() {
        let config = AppConfig::default();
        assert_eq!(config.process.roi.width, 460);
        assert_eq!(config.process.roi.height, 240);
    }

    #[test]
    fn test_default_config_hsv_range() {
        let config = AppConfig::default();
        assert_eq!(config.process.hsv_range.h_low, 25);
        assert_eq!(config.process.hsv_range.h_high, 45);
        assert_eq!(config.process.hsv_range.s_low, 80);
        assert_eq!(config.process.hsv_range.s_high, 255);
        assert_eq!(config.process.hsv_range.v_low, 80);
        assert_eq!(config.process.hsv_range.v_high, 255);
    }

    #[test]
    fn test_default_config_communication() {
        let config = AppConfig::default();
        assert_eq!(config.communication.vendor_id, 0x1234);
        assert_eq!(config.communication.product_id, 0x5678);
        assert_eq!(config.communication.hid_send_interval_ms, 8);
    }

    #[test]
    fn test_default_config_gpu_disabled() {
        let config = AppConfig::default();
        assert!(!config.gpu.enabled);
        assert_eq!(config.gpu.device_index, 0);
    }

    // ========== Test: TOML Parsing (Valid) ==========

    #[test]
    fn test_parse_minimal_valid_toml() {
        let toml_str = r#"
[capture]
source = "dda"
timeout_ms = 8
max_consecutive_timeouts = 120
reinit_initial_delay_ms = 100
reinit_max_delay_ms = 5000
monitor_index = 0

[process]
mode = "fast-color"
min_detection_area = 0
detection_method = "moments"

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
dead_zone = 0.0

[communication]
vendor_id = 0x1234
product_id = 0x5678
hid_send_interval_ms = 8

[pipeline]
enable_dirty_rect_optimization = false
stats_interval_sec = 10

[activation]
max_distance_from_center = 5.0
active_window_ms = 500

[audio_feedback]
enabled = true
on_sound = "C:\\Windows\\Media\\Speech On.wav"
off_sound = "C:\\Windows\\Media\\Speech Off.wav"
fallback_to_silent = true

[gpu]
enabled = false
device_index = 0
prefer_gpu = false

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
max_consecutive_timeouts = 120
reinit_initial_delay_ms = 100
reinit_max_delay_ms = 5000
monitor_index = 0

[process]
mode = "fast-color"
min_detection_area = 0
detection_method = "moments"

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
dead_zone = 0.0

[communication]
vendor_id = 0x1234
product_id = 0x5678
hid_send_interval_ms = 8

[pipeline]
enable_dirty_rect_optimization = false
stats_interval_sec = 10

[activation]
max_distance_from_center = 5.0
active_window_ms = 500

[audio_feedback]
enabled = true
on_sound = "C:\\Windows\\Media\\Speech On.wav"
off_sound = "C:\\Windows\\Media\\Speech Off.wav"
fallback_to_silent = true

[gpu]
enabled = false
device_index = 0
prefer_gpu = false

[debug]
enabled = false
"#;

        let config: AppConfig = toml::from_str(toml_str).expect("Failed to parse TOML");
        assert_eq!(config.capture.source, "wgc");
    }

    // ========== Test: Validation - Capture Source ==========

    #[test]
    fn test_validate_invalid_capture_source() {
        let mut config = AppConfig::default();
        config.capture.source = "spout".to_string();
        let result = config.validate();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Invalid capture source"));
    }

    #[test]
    fn test_validate_capture_timeout_zero() {
        let mut config = AppConfig::default();
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
        let mut config = AppConfig::default();
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
        let mut config = AppConfig::default();
        config.process.roi.height = 0;
        let result = config.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_roi_both_positive() {
        let mut config = AppConfig::default();
        config.process.roi.width = 1;
        config.process.roi.height = 1;
        assert!(config.validate().is_ok());
    }

    // ========== Test: Validation - HSV Range ==========

    #[test]
    fn test_validate_hsv_h_low_greater_than_h_high() {
        let mut config = AppConfig::default();
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
        let mut config = AppConfig::default();
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
        let mut config = AppConfig::default();
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
        let mut config = AppConfig::default();
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
        let mut config = AppConfig::default();
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
        let mut config = AppConfig::default();
        config.communication.product_id = 0;
        let result = config.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_communication_hid_send_interval_zero() {
        let mut config = AppConfig::default();
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
        let mut config = AppConfig::default();
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
        let mut config = AppConfig::default();
        config.process.coordinate_transform.y_clip_limit = 0.0;
        let result = config.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_coordinate_transform_sensitivity_zero() {
        let mut config = AppConfig::default();
        config.process.coordinate_transform.sensitivity = 0.0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("sensitivity must be > 0"));
    }

    // ========== Test: Validation - GPU ==========

    #[test]
    fn test_validate_gpu_device_index_valid() {
        let mut config = AppConfig::default();
        config.gpu.device_index = 0;
        assert!(config.validate().is_ok());
        config.gpu.device_index = 15;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_gpu_device_index_too_high() {
        let mut config = AppConfig::default();
        config.gpu.device_index = 16;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("device_index must be <= 15"));
    }

    // ========== Test: Validation - Activation ==========

    #[test]
    fn test_validate_activation_max_distance_zero() {
        let mut config = AppConfig::default();
        config.activation.max_distance_from_center = 0.0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("max_distance_from_center must be > 0"));
    }

    #[test]
    fn test_validate_activation_active_window_zero() {
        let mut config = AppConfig::default();
        config.activation.active_window_ms = 0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("active_window_ms must be > 0"));
    }

    // ========== Test: Validation - Pipeline ==========

    #[test]
    fn test_validate_pipeline_stats_interval_zero() {
        let mut config = AppConfig::default();
        config.pipeline.stats_interval_sec = 0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("stats_interval_sec must be > 0"));
    }

    // ========== Test: Validation - Capture Reinit ==========

    #[test]
    fn test_validate_capture_reinit_max_less_than_initial() {
        let mut config = AppConfig::default();
        config.capture.reinit_max_delay_ms = 100;
        config.capture.reinit_initial_delay_ms = 500;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("reinit_max_delay_ms must be >= reinit_initial_delay_ms"));
    }

    #[test]
    fn test_validate_capture_reinit_max_equals_initial() {
        let mut config = AppConfig::default();
        config.capture.reinit_max_delay_ms = 100;
        config.capture.reinit_initial_delay_ms = 100;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_capture_reinit_max_greater_than_initial() {
        let mut config = AppConfig::default();
        config.capture.reinit_max_delay_ms = 5000;
        config.capture.reinit_initial_delay_ms = 100;
        assert!(config.validate().is_ok());
    }

    // ========== Test: Clone and Debug Traits ==========

    #[test]
    fn test_config_clone() {
        let config = AppConfig::default();
        let cloned = config.clone();
        assert_eq!(config.capture.source, cloned.capture.source);
        assert_eq!(config.process.roi.width, cloned.process.roi.width);
    }

    #[test]
    fn test_config_debug_format() {
        let config = AppConfig::default();
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("AppConfig"));
        assert!(debug_str.contains("dda"));
    }
}
