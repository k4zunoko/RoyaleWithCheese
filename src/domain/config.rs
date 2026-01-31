//! 設定管理
//!
//! TOML設定ファイルの読み込みとDomain型への変換。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

use crate::domain::{DomainError, DomainResult, HsvRange, Roi};

/// 検出方法
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DetectionMethod {
    /// モーメントによる重心計算（デフォルト、高精度）
    Moments,
    /// バウンディングボックスの中心計算（高速）
    BoundingBox,
}

/// キャプチャソース
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CaptureSource {
    /// Desktop Duplication API（画面全体をキャプチャ）
    #[default]
    Dda,
    /// Spout DX11テクスチャ受信（送信側アプリケーションが必要）
    Spout,
    /// Windows Graphics Capture（低レイテンシモード、Win10 1803+）
    Wgc,
}

/// アプリケーション設定のルート構造
#[allow(dead_code)]
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct AppConfig {
    /// キャプチャ設定
    pub capture: CaptureConfig,
    /// 画像処理設定
    pub process: ProcessConfig,
    /// HID通信設定
    pub communication: CommunicationConfig,
    /// パイプライン設定
    pub pipeline: PipelineConfig,
    /// アクティベーション設定
    pub activation: ActivationConfig,
    /// 音声フィードバック設定
    pub audio_feedback: AudioFeedbackConfig,
    /// GPU処理設定（プレースホルダー）
    /// GPU processing settings (placeholder for future implementation)
    #[serde(default)]
    pub gpu: GpuConfig,
}

/// キャプチャ設定
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CaptureConfig {
    /// キャプチャソース
    ///
    /// 選択肢: "dda", "spout", "wgc"
    /// デフォルト: "dda"
    #[serde(default)]
    pub source: CaptureSource,

    /// Spout送信者名（source = "spout" の場合のみ有効）
    ///
    /// 空文字列または省略で最初のアクティブ送信者に自動接続
    #[serde(default)]
    pub spout_sender_name: Option<String>,

    /// キャプチャタイムアウト（ミリ秒）
    ///
    /// デフォルト: 8ms
    pub timeout_ms: u64,

    /// 連続タイムアウト許容回数
    ///
    /// この回数を超えたら再初期化を実行
    /// デフォルト: 120回
    pub max_consecutive_timeouts: u32,

    /// 再初期化時の初期待機時間（ミリ秒）
    ///
    /// デフォルト: 100ms
    pub reinit_initial_delay_ms: u64,

    /// 再初期化時の最大待機時間（ミリ秒、指数バックオフの上限）
    ///
    /// デフォルト: 5000ms
    pub reinit_max_delay_ms: u64,

    /// メインモニタのインデックス（DDAのみ有効）
    ///
    /// 通常は0
    pub monitor_index: u32,
}

impl CaptureConfig {
    /// デフォルトのキャプチャタイムアウト（ミリ秒）
    pub const DEFAULT_TIMEOUT_MS: u64 = 8;
    /// デフォルトの連続タイムアウト閉値（約1秒 @ 8ms）
    pub const DEFAULT_MAX_CONSECUTIVE_TIMEOUTS: u32 = 120;
    /// デフォルトの再初期化初期遅延（ミリ秒）
    pub const DEFAULT_REINIT_INITIAL_DELAY_MS: u64 = 100;
    /// デフォルトの再初期化最大遅延（ミリ秒）
    pub const DEFAULT_REINIT_MAX_DELAY_MS: u64 = 5000;
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            source: CaptureSource::default(),
            spout_sender_name: None,
            timeout_ms: Self::DEFAULT_TIMEOUT_MS,
            max_consecutive_timeouts: Self::DEFAULT_MAX_CONSECUTIVE_TIMEOUTS,
            reinit_initial_delay_ms: Self::DEFAULT_REINIT_INITIAL_DELAY_MS,
            reinit_max_delay_ms: Self::DEFAULT_REINIT_MAX_DELAY_MS,
            monitor_index: 0,
        }
    }
}

impl CaptureConfig {
    #[allow(dead_code)]
    pub fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms)
    }

    #[allow(dead_code)]
    pub fn reinit_initial_delay(&self) -> Duration {
        Duration::from_millis(self.reinit_initial_delay_ms)
    }

    #[allow(dead_code)]
    pub fn reinit_max_delay(&self) -> Duration {
        Duration::from_millis(self.reinit_max_delay_ms)
    }
}

/// 処理設定
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProcessConfig {
    /// 処理モード
    ///
    /// 選択肢: "fast-color" (HSV色検知), "yolo-ort" (YOLO物体検出、将来実装)
    /// デフォルト: "fast-color"
    pub mode: String,

    /// ROI（Region of Interest）設定
    pub roi: RoiConfig,

    /// HSVレンジ設定（fast-colorモードのみ使用）
    pub hsv_range: HsvRangeConfig,

    /// 最小検出面積（ピクセル数、これ未満は無視）
    ///
    /// デフォルト: 0
    pub min_detection_area: u32,

    /// 検出方法（fast-colorモードのみ使用）
    ///
    /// 選択肢: "moments" (モーメントによる重心計算、高精度), "boundingbox" (バウンディングボックスの中心、高速)
    /// デフォルト: "moments"
    #[serde(default = "default_detection_method")]
    pub detection_method: DetectionMethod,

    /// 座標変換設定
    #[serde(default)]
    pub coordinate_transform: CoordinateTransformConfig,
}

fn default_detection_method() -> DetectionMethod {
    DetectionMethod::Moments
}

impl ProcessConfig {
    /// デフォルトの処理モード
    pub const DEFAULT_MODE: &'static str = "fast-color";
    /// デフォルトの最小検出面積（ピクセル）
    pub const DEFAULT_MIN_DETECTION_AREA: u32 = 100;
}

impl Default for ProcessConfig {
    fn default() -> Self {
        Self {
            mode: Self::DEFAULT_MODE.to_string(),
            roi: RoiConfig::default(),
            hsv_range: HsvRangeConfig::default(),
            min_detection_area: Self::DEFAULT_MIN_DETECTION_AREA,
            detection_method: DetectionMethod::Moments,
            coordinate_transform: CoordinateTransformConfig::default(),
        }
    }
}

/// ROI設定（サイズのみ、位置は画面中心に自動配置）
///
/// x, y座標は実行時に画面解像度から自動計算される。
/// プロジェクトの設計方針として、ROIは常に画面中心に配置される。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RoiConfig {
    /// ROI幅（ピクセル）
    ///
    /// 注意: 画面解像度を超える場合は起動時にエラーになります
    pub width: u32,

    /// ROI高さ（ピクセル）
    ///
    /// 注意: 画面解像度を超える場合は起動時にエラーになります
    pub height: u32,
}

impl RoiConfig {
    /// デフォルトROI: 1920x1080中心の960x540を想定したサイズ
    pub const DEFAULT_WIDTH: u32 = 960;
    pub const DEFAULT_HEIGHT: u32 = 540;

    /// 画面中心にROIを配置
    ///
    /// # Arguments
    /// - `screen_width`: 画面幅（ピクセル）
    /// - `screen_height`: 画面高さ（ピクセル）
    ///
    /// # Returns
    /// - `Ok(Roi)`: 画面中心に配置されたROI
    /// - `Err(DomainError)`: ROIサイズが画面サイズを超える場合
    ///
    /// # Example
    /// ```ignore
    /// let roi_config = RoiConfig { width: 960, height: 540 };
    /// let roi = roi_config.to_roi_centered(1920, 1080)?;
    /// // roi.x = 480, roi.y = 270
    /// ```
    pub fn to_roi_centered(&self, screen_width: u32, screen_height: u32) -> DomainResult<Roi> {
        // width/heightが画面サイズを超える場合はエラー
        if self.width > screen_width {
            return Err(DomainError::Configuration(format!(
                "ROI width {} exceeds screen width {}",
                self.width, screen_width
            )));
        }
        if self.height > screen_height {
            return Err(DomainError::Configuration(format!(
                "ROI height {} exceeds screen height {}",
                self.height, screen_height
            )));
        }

        // 中心座標を計算
        let x = (screen_width - self.width) / 2;
        let y = (screen_height - self.height) / 2;

        Ok(Roi::new(x, y, self.width, self.height))
    }
}

impl Default for RoiConfig {
    fn default() -> Self {
        Self {
            width: Self::DEFAULT_WIDTH,
            height: Self::DEFAULT_HEIGHT,
        }
    }
}

/// HSVレンジ設定
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HsvRangeConfig {
    /// H（色相）の最小値
    ///
    /// OpenCV準拠: H [0-180]
    pub h_min: u8,

    /// H（色相）の最大値
    ///
    /// OpenCV準拠: H [0-180]
    pub h_max: u8,

    /// S（彩度）の最小値
    ///
    /// OpenCV準拠: S [0-255]
    pub s_min: u8,

    /// S（彩度）の最大値
    ///
    /// OpenCV準拠: S [0-255]
    pub s_max: u8,

    /// V（明度）の最小値
    ///
    /// OpenCV準拠: V [0-255]
    pub v_min: u8,

    /// V（明度）の最大値
    ///
    /// OpenCV準拠: V [0-255]
    pub v_max: u8,
}

impl Default for HsvRangeConfig {
    fn default() -> Self {
        // デフォルト: 黄色系（H:25-45, S:80-255, V:80-255）
        Self {
            h_min: 25,
            h_max: 45,
            s_min: 80,
            s_max: 255,
            v_min: 80,
            v_max: 255,
        }
    }
}

impl From<HsvRangeConfig> for HsvRange {
    fn from(config: HsvRangeConfig) -> Self {
        HsvRange::new(
            config.h_min,
            config.h_max,
            config.s_min,
            config.s_max,
            config.v_min,
            config.v_max,
        )
    }
}

/// 座標変換設定
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CoordinateTransformConfig {
    /// 感度（倍率、X/Y軸共通）
    ///
    /// デフォルト: 1.0
    pub sensitivity: f32,

    /// X軸のクリッピング限界値（ピクセル）
    ///
    /// デフォルト: 10.0
    pub x_clip_limit: f32,

    /// Y軸のクリッピング限界値（ピクセル）
    ///
    /// デフォルト: 10.0
    pub y_clip_limit: f32,

    /// デッドゾーン（ピクセル）
    ///
    /// デフォルト: 0.0
    pub dead_zone: f32,
}

impl Default for CoordinateTransformConfig {
    fn default() -> Self {
        Self {
            sensitivity: 1.0,
            x_clip_limit: f32::MAX, // クリッピングなし
            y_clip_limit: f32::MAX, // クリッピングなし
            dead_zone: 0.0,         // デッドゾーンなし
        }
    }
}

/// 通信設定
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CommunicationConfig {
    /// HIDデバイスのVendor ID（16進数で指定する場合は 0x1234 の形式）
    ///
    /// `cargo test test_enumerate_hid_devices -- --nocapture` で取得できます
    pub vendor_id: u16,

    /// HIDデバイスのProduct ID
    pub product_id: u16,

    /// デバイスのシリアル番号（オプション）
    #[serde(default)]
    pub serial_number: Option<String>,

    /// デバイスパス（オプション、最も確実な識別方法）
    ///
    /// 例 (Windows): "\\\\?\\hid#vid_2341&pid_8036#..."
    #[serde(default)]
    pub device_path: Option<String>,

    /// HIDレポート送信間隔（ミリ秒）
    ///
    /// HIDスレッドで新しい検出結果を待つタイムアウト時間であり、HIDパケットの送信頻度を決定します。
    /// 新しい検出結果がない場合でも、この間隔で直前の値を送信し続けます。
    /// 例: 8ms = 約125Hz、7ms = 約143Hz、16ms = 約62Hz
    pub hid_send_interval_ms: u64,
}

/// HIDアクティベーション設定
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ActivationConfig {
    /// ROI中心からの最大距離（ピクセル）
    ///
    /// 検出対象がROI中心からこの距離以内にある場合、アクティブ状態として記録される
    pub max_distance_from_center: f32,

    /// アクティブウィンドウの持続時間（ミリ秒）
    ///
    /// 最後にアクティブ条件を満たしてからこの時間内であればHID送信を許可する
    pub active_window_ms: u64,
}

impl Default for CommunicationConfig {
    fn default() -> Self {
        Self {
            vendor_id: 0x0000,
            product_id: 0x0000,
            serial_number: None,
            device_path: None,
            hid_send_interval_ms: 8, // 約144Hz（8ms間隔）
        }
    }
}

impl Default for ActivationConfig {
    fn default() -> Self {
        Self {
            max_distance_from_center: 50.0, // ROI中心から50ピクセル
            active_window_ms: 500,          // 500ms = 0.5秒
        }
    }
}

impl ActivationConfig {
    /// アクティブウィンドウの持続時間をDurationとして取得
    pub fn active_window(&self) -> Duration {
        Duration::from_millis(self.active_window_ms)
    }
}

/// パイプライン設定
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PipelineConfig {
    /// DirtyRect最適化を有効にするか（未実装）
    ///
    /// true の場合、ROI と交差しない DirtyRect は処理をスキップ
    /// 注: 現在、win_desktop_duplication クレートから DirtyRect 情報を取得していないため機能しません
    pub enable_dirty_rect_optimization: bool,

    /// 統計情報の出力間隔（秒）
    pub stats_interval_sec: u64,
}

/// 音声フィードバック設定
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AudioFeedbackConfig {
    /// Insertキー押下時の音声フィードバックを有効にする
    pub enabled: bool,

    /// 有効化時の音声ファイルパス（Windowsシステム音を使用）
    pub on_sound: String,

    /// 無効化時の音声ファイルパス
    pub off_sound: String,

    /// 音声ファイルが見つからない場合は静かに失敗する（ログのみ）
    pub fallback_to_silent: bool,
}

impl Default for AudioFeedbackConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            on_sound: "C:\\Windows\\Media\\Speech On.wav".to_string(),
            off_sound: "C:\\Windows\\Media\\Speech Off.wav".to_string(),
            fallback_to_silent: true,
        }
    }
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            enable_dirty_rect_optimization: true,
            stats_interval_sec: 10,
        }
    }
}

/// GPU処理設定
///
/// GPU-based processing configuration for D3D11 compute shaders.
/// Currently a placeholder for future GPU processing implementation.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GpuConfig {
    /// GPU処理を有効にするかどうか
    /// Whether to enable GPU-based processing.
    /// Default: false (use CPU processing)
    #[serde(default)]
    pub enabled: bool,

    /// 使用するGPUデバイスのインデックス
    /// GPU device index to use (0 = primary GPU).
    /// Default: 0
    #[serde(default)]
    pub device_index: u32,

    /// GPUが利用可能な場合に優先的に使用するか
    /// Whether to prefer GPU processing when available.
    /// If GPU fails, will fall back to CPU processing.
    /// Default: false
    #[serde(default)]
    pub prefer_gpu: bool,
}

impl Default for GpuConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            device_index: 0,
            prefer_gpu: false,
        }
    }
}

impl AppConfig {
    /// TOMLファイルから設定を読み込む
    #[allow(dead_code)]
    pub fn from_file<P: AsRef<Path>>(path: P) -> DomainResult<Self> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            DomainError::Configuration(format!("Failed to read config file: {}", e))
        })?;

        toml::from_str(&content)
            .map_err(|e| DomainError::Configuration(format!("Failed to parse config file: {}", e)))
    }

    /// デフォルト設定をTOMLファイルに書き出す
    #[allow(dead_code)]
    pub fn write_default<P: AsRef<Path>>(path: P) -> DomainResult<()> {
        let config = Self::default();
        let content = toml::to_string_pretty(&config).map_err(|e| {
            DomainError::Configuration(format!("Failed to serialize config: {}", e))
        })?;

        std::fs::write(path, content)
            .map_err(|e| DomainError::Configuration(format!("Failed to write config file: {}", e)))
    }

    /// 設定の妥当性を検証
    #[allow(dead_code)]
    pub fn validate(&self) -> DomainResult<()> {
        // ROIの検証
        if self.process.roi.width == 0 || self.process.roi.height == 0 {
            return Err(DomainError::Configuration(
                "ROI width and height must be greater than 0".to_string(),
            ));
        }

        // HSVレンジの検証
        let hsv = &self.process.hsv_range;
        if hsv.h_min > 180 || hsv.h_max > 180 || hsv.h_min > hsv.h_max {
            return Err(DomainError::Configuration(
                "Invalid HSV H range (must be 0-180, min <= max)".to_string(),
            ));
        }
        if hsv.s_min > hsv.s_max || hsv.v_min > hsv.v_max {
            return Err(DomainError::Configuration(
                "Invalid HSV S/V range (min must be <= max)".to_string(),
            ));
        }

        // タイムアウトの検証
        if self.capture.timeout_ms == 0 {
            return Err(DomainError::Configuration(
                "Capture timeout must be greater than 0".to_string(),
            ));
        }

        // 座標変換設定の検証
        let transform = &self.process.coordinate_transform;
        if transform.sensitivity <= 0.0 {
            return Err(DomainError::Configuration(
                "Sensitivity value must be positive".to_string(),
            ));
        }
        if transform.x_clip_limit < 0.0 || transform.y_clip_limit < 0.0 {
            return Err(DomainError::Configuration(
                "Clip limit values must be non-negative".to_string(),
            ));
        }
        if transform.dead_zone < 0.0 {
            return Err(DomainError::Configuration(
                "Dead zone must be non-negative".to_string(),
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert_eq!(config.capture.timeout_ms, 8);
        assert_eq!(config.process.mode, "fast-color");
        assert_eq!(config.process.roi.width, 960);
    }

    #[test]
    fn test_audio_feedback_config_default() {
        let config = AudioFeedbackConfig::default();
        assert!(config.enabled);
        assert_eq!(config.on_sound, "C:\\Windows\\Media\\Speech On.wav");
        assert_eq!(config.off_sound, "C:\\Windows\\Media\\Speech Off.wav");
        assert!(config.fallback_to_silent);
    }

    #[test]
    fn test_config_validation() {
        let mut config = AppConfig::default();
        assert!(config.validate().is_ok());

        // 不正なROI
        config.process.roi.width = 0;
        assert!(config.validate().is_err());

        config.process.roi.width = 960;

        // 不正なHSV範囲
        config.process.hsv_range.h_min = 200;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_roi_centered_normal() {
        // 正常系: 1920x1080画面の中心に960x540のROI
        let roi_config = RoiConfig {
            width: 960,
            height: 540,
        };
        let roi = roi_config.to_roi_centered(1920, 1080).unwrap();
        assert_eq!(roi.x, 480); // (1920 - 960) / 2
        assert_eq!(roi.y, 270); // (1080 - 540) / 2
        assert_eq!(roi.width, 960);
        assert_eq!(roi.height, 540);
    }

    #[test]
    fn test_roi_centered_width_exceeds() {
        // 異常系: ROI幅が画面幅を超える
        let roi_config = RoiConfig {
            width: 2000,
            height: 540,
        };
        let result = roi_config.to_roi_centered(1920, 1080);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), DomainError::Configuration(_)));
    }

    #[test]
    fn test_roi_centered_height_exceeds() {
        // 異常系: ROI高さが画面高さを超える
        let roi_config = RoiConfig {
            width: 960,
            height: 1200,
        };
        let result = roi_config.to_roi_centered(1920, 1080);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), DomainError::Configuration(_)));
    }

    #[test]
    fn test_roi_centered_exact_size() {
        // 境界値: ROIが画面と同じサイズ
        let roi_config = RoiConfig {
            width: 1920,
            height: 1080,
        };
        let roi = roi_config.to_roi_centered(1920, 1080).unwrap();
        assert_eq!(roi.x, 0);
        assert_eq!(roi.y, 0);
        assert_eq!(roi.width, 1920);
        assert_eq!(roi.height, 1080);
    }

    #[test]
    fn test_roi_centered_small_screen() {
        // 異なる解像度: 2560x1440画面
        let roi_config = RoiConfig {
            width: 1280,
            height: 720,
        };
        let roi = roi_config.to_roi_centered(2560, 1440).unwrap();
        assert_eq!(roi.x, 640); // (2560 - 1280) / 2
        assert_eq!(roi.y, 360); // (1440 - 720) / 2
        assert_eq!(roi.width, 1280);
        assert_eq!(roi.height, 720);
    }

    #[test]
    fn test_hsv_range_conversion() {
        let hsv_config = HsvRangeConfig {
            h_min: 10,
            h_max: 20,
            s_min: 30,
            s_max: 40,
            v_min: 50,
            v_max: 60,
        };
        let hsv: HsvRange = hsv_config.into();
        assert_eq!(hsv.h_min, 10);
        assert_eq!(hsv.h_max, 20);
    }

    #[test]
    fn test_config_loads() {
        // config.tomlが正常に読み込めることを確認
        let config = AppConfig::from_file("config.toml").expect("config.tomlが読み込めません");

        // 基本的なバリデーション
        config
            .validate()
            .expect("設定値のバリデーションに失敗しました");

        // 各セクションが存在することを確認
        assert!(
            config.capture.timeout_ms > 0,
            "timeout_msは0より大きい必要があります"
        );
        assert!(
            config.process.roi.width > 0,
            "ROI幅は0より大きい必要があります"
        );
        assert!(
            config.process.roi.height > 0,
            "ROI高さは0より大きい必要があります"
        );
        assert!(
            config.process.coordinate_transform.sensitivity > 0.0,
            "sensitivityは0より大きい必要があります"
        );
    }

    #[test]
    fn test_config_example_loads() {
        // config.toml.exampleが正常に読み込めることを確認
        let config = AppConfig::from_file("config.toml.example")
            .expect("config.toml.exampleが読み込めません");

        // 基本的なバリデーション
        config
            .validate()
            .expect("設定値のバリデーションに失敗しました");
    }

    #[test]
    fn test_gpu_config_defaults() {
        let config = GpuConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.device_index, 0);
        assert!(!config.prefer_gpu);
    }

    #[test]
    fn test_gpu_config_parsing() {
        let toml = r#"
            enabled = true
            device_index = 1
            prefer_gpu = true
        "#;
        let config: GpuConfig = toml::from_str(toml).unwrap();
        assert!(config.enabled);
        assert_eq!(config.device_index, 1);
        assert!(config.prefer_gpu);
    }

    #[test]
    fn test_config_with_gpu_section() {
        // Test that AppConfig can parse a [gpu] section
        let toml = r#"
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
            width = 960
            height = 540

            [process.hsv_range]
            h_min = 25
            h_max = 45
            s_min = 80
            s_max = 255
            v_min = 80
            v_max = 255

            [process.coordinate_transform]
            sensitivity = 1.0
            x_clip_limit = 10.0
            y_clip_limit = 10.0
            dead_zone = 0.0

            [communication]
            vendor_id = 0x0000
            product_id = 0x0000
            hid_send_interval_ms = 8

            [pipeline]
            enable_dirty_rect_optimization = false
            stats_interval_sec = 10

            [activation]
            max_distance_from_center = 50.0
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
        "#;
        let config: AppConfig = toml::from_str(toml).unwrap();
        assert!(!config.gpu.enabled);
        assert_eq!(config.gpu.device_index, 0);
        assert!(!config.gpu.prefer_gpu);
    }
}
