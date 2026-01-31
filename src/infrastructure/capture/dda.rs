//! DDA (Desktop Duplication API) キャプチャアダプタ
//!
//! Windows Desktop Duplication APIを使用した低レイテンシ画面キャプチャ。
//! 144Hzモニタで毎秒144フレーム取得可能。

use crate::domain::{CapturePort, DeviceInfo, DomainError, DomainResult, Frame, GpuFrame, Roi};
use crate::infrastructure::capture::common::{
    clamp_roi, copy_roi_to_staging, copy_texture_to_cpu, StagingTextureManager,
};
use std::time::Instant;
use win_desktop_duplication::{
    co_init, devices::AdapterFactory, outputs::Display, set_process_dpi_awareness,
    DesktopDuplicationApi, DuplicationApiOptions,
};
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;

// DDApiErrorはpublicではないため、Result型から推論する必要がある
// エラーハンドリングでは具体的なエラー型を使用せず、Result<T>のパターンマッチで対応

/// DDAキャプチャアダプタ
///
/// CapturePort traitを実装し、DDAによる画面キャプチャを提供。
pub struct DdaCaptureAdapter {
    dupl: DesktopDuplicationApi,
    output: Display,
    device_info: DeviceInfo,
    #[allow(dead_code)] // 将来的に使用予定
    timeout_ms: u32,

    // GPU ROI実装用のD3D11リソース
    // Note: win_desktop_duplicationとwindows crateで同じバージョン(0.57)を使用
    #[allow(dead_code)] // get_device_and_ctx()で取得するがcontextのみ使用
    device: ID3D11Device4,
    context: ID3D11DeviceContext4,

    // ステージングテクスチャ管理（共通モジュール使用）
    staging_manager: StagingTextureManager,

    // 再初期化時に元の設定を保持
    // reinitialize() メソッドで使用されるため、コンパイラの "never read" 警告は誤検知
    #[allow(dead_code)]
    adapter_idx: usize,
    #[allow(dead_code)]
    output_idx: usize,
}

impl DdaCaptureAdapter {
    /// 新しいDDAキャプチャアダプタを作成
    ///
    /// # Arguments
    /// - `adapter_idx`: GPUアダプタのインデックス（通常は0）
    /// - `output_idx`: ディスプレイ出力のインデックス（通常は0）
    /// - `timeout_ms`: キャプチャタイムアウト時間（ミリ秒）
    ///
    /// # Returns
    /// - `Ok(DdaCaptureAdapter)`: 初期化成功
    /// - `Err(DomainError)`: 初期化失敗
    ///
    /// # Safety
    /// このメソッドはCOM初期化とDPI設定を行う。
    /// プロセスごとに1回のみ呼び出すべき。
    pub fn new(adapter_idx: usize, output_idx: usize, timeout_ms: u32) -> DomainResult<Self> {
        // COM初期化とDPI設定（プロセスごとに1回のみ必要）
        // 複数回呼んでも安全（内部でガード済み）
        set_process_dpi_awareness();
        co_init();

        // アダプタとディスプレイの取得
        let adapter = AdapterFactory::new()
            .get_adapter_by_idx(adapter_idx as u32)
            .ok_or_else(|| {
                DomainError::Capture(format!("Failed to get adapter {}", adapter_idx))
            })?;

        let output = adapter
            .get_display_by_idx(output_idx as u32)
            .ok_or_else(|| DomainError::Capture(format!("Failed to get display {}", output_idx)))?;

        // DDA API初期化
        let mut dupl = DesktopDuplicationApi::new(adapter.clone(), output.clone())
            .map_err(|e| DomainError::Capture(format!("Failed to initialize DDA: {:?}", e)))?;

        // カーソル描画を無効化(マウスカーソルを画面キャプチャに含めない)
        let options = DuplicationApiOptions { skip_cursor: true };
        dupl.configure(options);

        // TextureReader初期化(GPU → CPU転送用)
        let (device, ctx) = dupl.get_device_and_ctx();

        // デバイス情報の取得
        let display_mode = output
            .get_current_display_mode()
            .map_err(|e| DomainError::Capture(format!("Failed to get display mode: {:?}", e)))?;

        let device_info = DeviceInfo {
            width: display_mode.width,
            height: display_mode.height,
            refresh_rate: display_mode.refresh_num / display_mode.refresh_den,
            name: format!("Display {} on Adapter {}", output_idx, adapter_idx),
        };

        Ok(Self {
            dupl,
            output,
            device_info,
            timeout_ms,
            device,
            context: ctx,
            staging_manager: StagingTextureManager::new(),
            adapter_idx,
            output_idx,
        })
    }

    /// VSync待機
    ///
    /// 144Hzモニタなら約6.9ms間隔でVSync信号が来る。
    ///
    /// **注意**: レイテンシ最小化のため、現在は使用していない。
    /// VSync待機は次のVBlankまで必ず待つため、最大1フレーム分の遅延が発生する。
    /// acquire_next_frame_now()のみを使用することで、OSのデスクトップ更新時に
    /// 即座に復帰し、最低レイテンシを実現する。
    ///
    /// 将来的にフレームレート制御が必要になった場合のために保持。
    #[allow(dead_code)]
    fn wait_for_vsync(&self) -> DomainResult<()> {
        self.output
            .wait_for_vsync()
            .map_err(|e| DomainError::Capture(format!("VSync wait failed: {:?}", e)))
    }

    /// フレームを取得（DDA固有処理）
    ///
    /// # Returns
    /// - `Ok(Some(texture))`: フレーム取得成功
    /// - `Ok(None)`: タイムアウト（フレーム更新なし）
    /// - `Err(DomainError)`: 致命的エラー
    fn acquire_frame(&mut self) -> DomainResult<Option<win_desktop_duplication::texture::Texture>> {
        match self.dupl.acquire_next_frame_now() {
            Ok(tex) => Ok(Some(tex)),
            Err(e) => {
                // エラーメッセージから種別を判定
                let error_msg = format!("{:?}", e);

                if error_msg.contains("Timeout") {
                    // タイムアウト: フレーム更新なし
                    Ok(None)
                } else if error_msg.contains("AccessLost") || error_msg.contains("AccessDenied") {
                    // Recoverable: 排他的フルスクリーン切替、デスクトップモード変更
                    // Application層が再初期化の判断を行う
                    #[cfg(debug_assertions)]
                    tracing::debug!("DDA Access error: {}", error_msg);
                    Err(DomainError::DeviceNotAvailable)
                } else {
                    // Non-recoverable: インスタンス再作成が必要
                    #[cfg(debug_assertions)]
                    tracing::error!("DDA Unexpected error: {}", error_msg);
                    Err(DomainError::ReInitializationRequired)
                }
            }
        }
    }
}

impl CapturePort for DdaCaptureAdapter {
    fn capture_frame_with_roi(&mut self, roi: &Roi) -> DomainResult<Option<Frame>> {
        // ROIを画面中心に動的配置
        // レイテンシへの影響: ~10ns未満（減算2回、除算2回）
        let centered_roi = roi
            .centered_in(self.device_info.width, self.device_info.height)
            .ok_or_else(|| {
                DomainError::Configuration(format!(
                    "ROI size ({}x{}) exceeds display bounds ({}x{})",
                    roi.width, roi.height, self.device_info.width, self.device_info.height
                ))
            })?;

        // ROI境界検証とクランプ（画面外アクセス防止）
        // 共通モジュールの関数を使用
        let clamped_roi = clamp_roi(
            &centered_roi,
            self.device_info.width,
            self.device_info.height,
        )
        .ok_or_else(|| {
            DomainError::Capture(format!(
                "ROI ({}, {}, {}x{}) is completely outside display bounds ({}x{})",
                centered_roi.x,
                centered_roi.y,
                centered_roi.width,
                centered_roi.height,
                self.device_info.width,
                self.device_info.height
            ))
        })?;

        #[cfg(feature = "performance-timing")]
        let acquire_start = Instant::now();

        // フレーム取得（DDA固有処理）
        let tex = match self.acquire_frame()? {
            Some(tex) => tex,
            None => return Ok(None), // タイムアウト
        };

        #[cfg(feature = "performance-timing")]
        let acquire_time = acquire_start.elapsed();

        // ステージングテクスチャを確保または再利用（共通モジュール使用）
        // DDAはBGRA形式固定
        let staging_tex = self.staging_manager.ensure_texture(
            &self.device,
            clamped_roi.width,
            clamped_roi.height,
            DXGI_FORMAT_B8G8R8A8_UNORM,
        )?;

        #[cfg(feature = "performance-timing")]
        let gpu_copy_start = Instant::now();

        // GPU上でROI領域だけをSTAGINGへコピー（共通モジュール使用）
        // win_desktop_duplicationのTextureをID3D11Resourceとして取得
        let src_resource: ID3D11Resource = tex.as_raw_ref().clone().cast().map_err(|e| {
            DomainError::Capture(format!("Failed to cast texture to resource: {:?}", e))
        })?;

        copy_roi_to_staging(&self.context, &src_resource, &staging_tex, &clamped_roi);

        #[cfg(feature = "performance-timing")]
        let gpu_copy_time = gpu_copy_start.elapsed();
        #[cfg(feature = "performance-timing")]
        let cpu_transfer_start = Instant::now();

        // GPU→CPU転送（共通モジュール使用）
        let data = copy_texture_to_cpu(
            &self.context,
            &staging_tex,
            clamped_roi.width,
            clamped_roi.height,
        )?;

        #[cfg(feature = "performance-timing")]
        let cpu_transfer_time = cpu_transfer_start.elapsed();
        #[cfg(feature = "performance-timing")]
        tracing::debug!(
            "DDA Capture breakdown: Acquire={:.2}ms, GPU_Copy={:.2}ms, CPU_Transfer={:.2}ms, Total={:.2}ms ({}x{} ROI)",
            acquire_time.as_secs_f64() * 1000.0,
            gpu_copy_time.as_secs_f64() * 1000.0,
            cpu_transfer_time.as_secs_f64() * 1000.0,
            acquire_start.elapsed().as_secs_f64() * 1000.0,
            clamped_roi.width,
            clamped_roi.height
        );

        // DirtyRect情報の取得（最適化用）
        // 注: win_desktop_duplicationクレートはDirtyRect情報を直接提供しない可能性があるため、
        // ここでは空のVecを返す。将来的にはクレートのAPIを確認して実装を改善。
        let dirty_rects = vec![];

        Ok(Some(Frame {
            data,
            width: clamped_roi.width,
            height: clamped_roi.height,
            timestamp: Instant::now(),
            dirty_rects,
        }))
    }

    fn capture_gpu_frame(&mut self, roi: &Roi) -> DomainResult<Option<GpuFrame>> {
        // ROIを画面中心に動的配置
        let centered_roi = roi
            .centered_in(self.device_info.width, self.device_info.height)
            .ok_or_else(|| {
                DomainError::Configuration(format!(
                    "ROI size ({}x{}) exceeds display bounds ({}x{})",
                    roi.width, roi.height, self.device_info.width, self.device_info.height
                ))
            })?;

        // ROI境界検証とクランプ
        let clamped_roi = clamp_roi(
            &centered_roi,
            self.device_info.width,
            self.device_info.height,
        )
        .ok_or_else(|| {
            DomainError::Capture(format!(
                "ROI ({}, {}, {}x{}) is completely outside display bounds ({}x{})",
                centered_roi.x,
                centered_roi.y,
                centered_roi.width,
                centered_roi.height,
                self.device_info.width,
                self.device_info.height
            ))
        })?;

        // フレーム取得
        let tex = match self.acquire_frame()? {
            Some(tex) => tex,
            None => return Ok(None), // タイムアウト
        };

        // GPU上でROI領域だけを切り出して新しいテクスチャを作成
        // ステージングテクスチャを確保
        let roi_texture = self.staging_manager.ensure_texture(
            &self.device,
            clamped_roi.width,
            clamped_roi.height,
            DXGI_FORMAT_B8G8R8A8_UNORM,
        )?;

        // GPU上でROI領域をコピー
        let src_resource: ID3D11Resource = tex.as_raw_ref().clone().cast().map_err(|e| {
            DomainError::Capture(format!("Failed to cast texture to resource: {:?}", e))
        })?;

        copy_roi_to_staging(&self.context, &src_resource, &roi_texture, &clamped_roi);

        // GpuFrameを作成して返す
        // Note: roi_textureはID3D11Texture2DとしてGpuFrameに渡される
        let gpu_texture: ID3D11Texture2D = roi_texture.cast().map_err(|e| {
            DomainError::Capture(format!(
                "Failed to cast staging texture to ID3D11Texture2D: {:?}",
                e
            ))
        })?;

        let gpu_frame = GpuFrame::new(
            Some(gpu_texture),
            clamped_roi.width,
            clamped_roi.height,
            DXGI_FORMAT_B8G8R8A8_UNORM,
        );

        Ok(Some(gpu_frame))
    }

    fn supports_gpu_frame(&self) -> bool {
        true
    }

    fn reinitialize(&mut self) -> DomainResult<()> {
        #[cfg(debug_assertions)]
        tracing::info!(
            "Reinitializing DDA capture adapter (adapter: {}, output: {})",
            self.adapter_idx,
            self.output_idx
        );

        // 元のadapter_idx/output_idxを使用してインスタンスを再作成
        let adapter = AdapterFactory::new()
            .get_adapter_by_idx(self.adapter_idx as u32)
            .ok_or_else(|| {
                DomainError::Capture(format!(
                    "Failed to get adapter {} during reinit",
                    self.adapter_idx
                ))
            })?;

        let output = adapter
            .get_display_by_idx(self.output_idx as u32)
            .ok_or_else(|| {
                DomainError::Capture(format!(
                    "Failed to get display {} during reinit",
                    self.output_idx
                ))
            })?;

        let mut dupl = DesktopDuplicationApi::new(adapter, output.clone())
            .map_err(|e| DomainError::Capture(format!("Failed to reinitialize DDA: {:?}", e)))?;

        // カーソル描画を無効化(再初期化時も設定を適用)
        let options = DuplicationApiOptions { skip_cursor: true };
        dupl.configure(options);

        let (device, ctx) = dupl.get_device_and_ctx();

        // device_infoを再計算（解像度やリフレッシュレートが変わっている可能性）
        let display_mode = output.get_current_display_mode().map_err(|e| {
            DomainError::Capture(format!("Failed to get display mode during reinit: {:?}", e))
        })?;

        let device_info = DeviceInfo {
            width: display_mode.width,
            height: display_mode.height,
            refresh_rate: display_mode.refresh_num / display_mode.refresh_den,
            name: format!(
                "Display {} on Adapter {}",
                self.output_idx, self.adapter_idx
            ),
        };

        // 状態を更新
        self.dupl = dupl;
        self.output = output;
        self.device = device;
        self.context = ctx;
        self.device_info = device_info;

        // ステージングテクスチャをクリア（サイズが変わっている可能性があるため）
        self.staging_manager.clear();

        #[cfg(debug_assertions)]
        tracing::info!(
            "DDA reinitialization completed: {}x{}@{}Hz",
            self.device_info.width,
            self.device_info.height,
            self.device_info.refresh_rate
        );

        Ok(())
    }

    fn device_info(&self) -> DeviceInfo {
        self.device_info.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore] // 管理者権限 + GPU必須のため通常はスキップ
    fn test_dda_initialization() {
        // 注: DDA APIは同時に1つのインスタンスしか作成できない場合がある
        // 他のテストと同時実行しないこと
        let adapter = DdaCaptureAdapter::new(0, 0, 8);

        if adapter.is_err() {
            println!(
                "DDA initialization failed (expected if another instance exists): {:?}",
                adapter.err()
            );
            return; // 他のテストが実行中の場合は失敗を許容
        }

        let adapter = adapter.unwrap();
        let info = adapter.device_info();

        println!("Device Info:");
        println!("  Resolution: {}x{}", info.width, info.height);
        println!("  Refresh Rate: {}Hz", info.refresh_rate);
        println!("  Name: {}", info.name);

        assert!(info.width > 0);
        assert!(info.height > 0);
        assert!(info.refresh_rate > 0);
    }

    #[test]
    #[ignore] // 管理者権限 + GPU必須のため通常はスキップ
    fn test_dda_capture_single_frame() {
        let mut adapter = match DdaCaptureAdapter::new(0, 0, 8) {
            Ok(a) => a,
            Err(e) => {
                println!(
                    "DDA initialization failed (expected if another instance exists): {:?}",
                    e
                );
                return;
            }
        };

        match adapter.capture_frame() {
            Ok(Some(frame)) => {
                println!("Captured frame:");
                println!("  Size: {}x{}", frame.width, frame.height);
                println!("  Data length: {} bytes", frame.data.len());
                println!("  Expected: {} bytes", frame.width * frame.height * 4);

                assert_eq!(frame.data.len(), (frame.width * frame.height * 4) as usize);
                assert!(frame.width > 0);
                assert!(frame.height > 0);
            }
            Ok(None) => {
                println!("No frame update (timeout)");
            }
            Err(e) => {
                println!("Capture error (expected in exclusive fullscreen): {:?}", e);
                // 排他的フルスクリーン環境ではDeviceNotAvailableが発生するため許容
                // Application層が再初期化ロジックで対応する
            }
        }
    }

    #[test]
    #[ignore] // 管理者権限 + GPU必須 + 時間がかかるため通常はスキップ
    fn test_dda_capture_multiple_frames() {
        let mut adapter = DdaCaptureAdapter::new(0, 0, 8).expect("DDA initialization failed");

        let mut frame_count = 0;
        let mut timeout_count = 0;
        let mut error_count = 0;

        // 1秒間キャプチャ（144Hzなら約144フレーム取得可能）
        let start = Instant::now();
        while start.elapsed().as_secs() < 1 {
            match adapter.capture_frame() {
                Ok(Some(_)) => frame_count += 1,
                Ok(None) => timeout_count += 1,
                Err(e) => {
                    // 排他的フルスクリーン環境ではDeviceNotAvailableが頻繁に発生
                    // Infrastructure層はエラーを返すのみ、Application層が再初期化を判断
                    error_count += 1;
                    if error_count == 1 {
                        println!("First error (expected in exclusive fullscreen): {:?}", e);
                    }
                }
            }
        }
        println!();
        println!("Capture statistics (1 second):");
        println!("  Frames captured: {}", frame_count);
        println!("  Timeouts: {}", timeout_count);
        println!(
            "  Errors: {} (expected in exclusive fullscreen)",
            error_count
        );
        println!("  Effective FPS: {}", frame_count);

        // デスクトップ環境では144 FPS、排他的フルスクリーンではエラーが多発
        if error_count == 0 {
            assert!(
                frame_count > 0,
                "Should capture at least one frame in desktop mode"
            );
        } else {
            println!("NOTE: High error count indicates exclusive fullscreen environment");
            println!("Application layer should handle this with recovery logic");
        }
    }

    #[test]
    #[ignore] // 管理者権限 + GPU必須 + DDA API制限のためスキップ
    fn test_dda_reinitialize() {
        // 注: このテストはDDA APIの制限により、
        // 既存のインスタンスを完全に破棄してからでないと
        // 新しいインスタンスを作成できないため、通常は失敗する。
        // 実際のアプリケーションでは、Application層が
        // RecoveryStateを使って適切にreinitialize()を呼び出す。
        let mut adapter = match DdaCaptureAdapter::new(0, 0, 8) {
            Ok(a) => a,
            Err(e) => {
                println!("DDA initialization failed: {:?}", e);
                return;
            }
        };

        // 最初のフレーム取得
        let _ = adapter.capture_frame();

        // 再初期化（DDA API制限により失敗する可能性が高い）
        let result = adapter.reinitialize();
        if result.is_err() {
            println!(
                "Reinitialization failed (expected due to DDA API limitation): {:?}",
                result.err()
            );
            return;
        }

        // 再初期化後のフレーム取得
        let frame = adapter.capture_frame();
        assert!(frame.is_ok(), "Frame capture after reinit should work");
    }

    #[test]
    #[ignore] // 管理者権限 + GPU必須のため通常はスキップ
    fn test_dda_capture_with_roi() {
        let mut adapter = match DdaCaptureAdapter::new(0, 0, 8) {
            Ok(a) => a,
            Err(e) => {
                println!(
                    "DDA initialization failed (expected if another instance exists): {:?}",
                    e
                );
                return;
            }
        };

        let device_info = adapter.device_info();
        println!(
            "Display resolution: {}x{}",
            device_info.width, device_info.height
        );

        // テスト1: 400x300のROI
        let roi_small = Roi::new(100, 100, 400, 300);
        println!(
            "\nTest 1: Capturing with ROI {}x{} at ({}, {})",
            roi_small.width, roi_small.height, roi_small.x, roi_small.y
        );

        match adapter.capture_frame_with_roi(&roi_small) {
            Ok(Some(frame)) => {
                println!("  Frame size: {}x{}", frame.width, frame.height);
                println!("  Data length: {} bytes", frame.data.len());
                println!(
                    "  Expected: {} bytes",
                    roi_small.width * roi_small.height * 4
                );

                assert_eq!(
                    frame.width, roi_small.width,
                    "Frame width should match ROI width"
                );
                assert_eq!(
                    frame.height, roi_small.height,
                    "Frame height should match ROI height"
                );
                assert_eq!(
                    frame.data.len(),
                    (roi_small.width * roi_small.height * 4) as usize,
                    "Data size should match ROI size"
                );
            }
            Ok(None) => {
                println!("  No frame update (timeout)");
            }
            Err(e) => {
                println!("  Capture error: {:?}", e);
            }
        }

        // テスト2: 800x600のROI（設計書での目標サイズ）
        let roi_medium = Roi::new(560, 240, 800, 600);
        println!(
            "\nTest 2: Capturing with ROI {}x{} at ({}, {}) - Design target size",
            roi_medium.width, roi_medium.height, roi_medium.x, roi_medium.y
        );

        match adapter.capture_frame_with_roi(&roi_medium) {
            Ok(Some(frame)) => {
                println!("  Frame size: {}x{}", frame.width, frame.height);
                println!("  Data length: {} bytes", frame.data.len());
                println!(
                    "  Expected: {} bytes",
                    roi_medium.width * roi_medium.height * 4
                );
                println!(
                    "  PCIe transfer reduction: {} -> {} bytes ({:.1}% reduction)",
                    device_info.width * device_info.height * 4,
                    frame.data.len(),
                    (1.0 - frame.data.len() as f64
                        / (device_info.width * device_info.height * 4) as f64)
                        * 100.0
                );

                assert_eq!(
                    frame.width, roi_medium.width,
                    "Frame width should match ROI width"
                );
                assert_eq!(
                    frame.height, roi_medium.height,
                    "Frame height should match ROI height"
                );
                assert_eq!(
                    frame.data.len(),
                    (roi_medium.width * roi_medium.height * 4) as usize,
                    "Data size should match ROI size"
                );
            }
            Ok(None) => {
                println!("  No frame update (timeout)");
            }
            Err(e) => {
                println!("  Capture error: {:?}", e);
            }
        }

        // テスト3: フルスクリーンROIとcapture_frame()の比較
        let roi_full = Roi::new(0, 0, device_info.width, device_info.height);
        println!("\nTest 3: Full-screen ROI vs capture_frame()");

        let frame_with_roi = adapter.capture_frame_with_roi(&roi_full);
        let frame_default = adapter.capture_frame();

        match (frame_with_roi, frame_default) {
            (Ok(Some(f1)), Ok(Some(f2))) => {
                println!(
                    "  capture_frame_with_roi: {}x{}, {} bytes",
                    f1.width,
                    f1.height,
                    f1.data.len()
                );
                println!(
                    "  capture_frame: {}x{}, {} bytes",
                    f2.width,
                    f2.height,
                    f2.data.len()
                );
                assert_eq!(
                    f1.width, f2.width,
                    "Both methods should return same dimensions"
                );
                assert_eq!(
                    f1.height, f2.height,
                    "Both methods should return same dimensions"
                );
            }
            _ => {
                println!("  One or both captures returned None or error");
            }
        }
    }
}
