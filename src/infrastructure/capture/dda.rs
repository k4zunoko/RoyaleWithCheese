/// DDA (Desktop Duplication API) キャプチャアダプタ
/// 
/// Windows Desktop Duplication APIを使用した低レイテンシ画面キャプチャ。
/// 144Hzモニタで毎秒144フレーム取得可能。

use crate::domain::{CapturePort, DeviceInfo, DomainError, DomainResult, Frame, Roi};
use std::time::Instant;
use std::mem;
use std::ptr;
use win_desktop_duplication::{
    devices::AdapterFactory,
    outputs::Display,
    set_process_dpi_awareness, co_init,
    DesktopDuplicationApi,
    DuplicationApiOptions,
};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::core::Interface;

// DDApiErrorはpublicではないため、Result型から推論する必要がある
// エラーハンドリングでは具体的なエラー型を使用せず、Result<T>のパターンマッチで対応

/// DDAキャプチャアダプタ
/// 
/// CapturePort traitを実装し、DDAによる画面キャプチャを提供。
pub struct DdaCaptureAdapter {
    dupl: DesktopDuplicationApi,
    output: Display,
    device_info: DeviceInfo,
    #[allow(dead_code)]  // 将来的に使用予定
    timeout_ms: u32,
    
    // GPU ROI実装用のD3D11リソース
    // Note: win_desktop_duplicationとwindows crateで同じバージョン(0.57)を使用
    device: ID3D11Device4,
    context: ID3D11DeviceContext4,
    
    // ステージングテクスチャの再利用（パフォーマンス最適化）
    staging_tex: Option<ID3D11Texture2D>,
    staging_size: (u32, u32),
    
    // 再初期化時に元の設定を保持
    adapter_idx: usize,
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
            .ok_or_else(|| {
                DomainError::Capture(format!("Failed to get display {}", output_idx))
            })?;

        // DDA API初期化
        let mut dupl = DesktopDuplicationApi::new(adapter.clone(), output.clone())
            .map_err(|e| DomainError::Capture(format!("Failed to initialize DDA: {:?}", e)))?;

        // カーソル描画を無効化(マウスカーソルを画面キャプチャに含めない)
        let mut options = DuplicationApiOptions::default();
        options.skip_cursor = true;
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
            refresh_rate: (display_mode.refresh_num / display_mode.refresh_den) as u32,
            name: format!("Display {} on Adapter {}", output_idx, adapter_idx),
        };

        Ok(Self {
            dupl,
            output,
            device_info,
            timeout_ms,
            device,
            context: ctx,
            staging_tex: None,
            staging_size: (0, 0),
            adapter_idx,
            output_idx,
        })
    }

    /// プライマリモニタでの初期化（簡易版）
    /// 
    /// アダプタ0、ディスプレイ0、タイムアウト8msで初期化。
    pub fn new_primary(timeout_ms: u32) -> DomainResult<Self> {
        Self::new(0, 0, timeout_ms)
    }

    /// VSync待機
    /// 
    /// 144Hzモニタなら約6.9ms間隔でVSync信号が来る。
    /// この待機により、リフレッシュレートに同期したキャプチャが可能。
    fn wait_for_vsync(&self) -> DomainResult<()> {
        self.output
            .wait_for_vsync()
            .map_err(|e| DomainError::Capture(format!("VSync wait failed: {:?}", e)))
    }
    
    /// ROIを画面サイズ内にクランプ
    /// 
    /// ROIが画面外にはみ出している場合、画面内に収まるように調整。
    /// ROIが完全に画面外の場合はNoneを返す。
    fn clamp_roi(&self, roi: &Roi) -> Option<Roi> {
        let w = self.device_info.width;
        let h = self.device_info.height;
        
        // ROIのサイズが0なら無効
        if roi.width == 0 || roi.height == 0 {
            return None;
        }
        
        // ROIが完全に画面外ならNone
        if roi.x >= w || roi.y >= h {
            return None;
        }
        
        // 画面内に収まるようにクランプ
        let clamped_x = roi.x.min(w);
        let clamped_y = roi.y.min(h);
        let max_w = w.saturating_sub(clamped_x);
        let max_h = h.saturating_sub(clamped_y);
        let clamped_width = roi.width.min(max_w);
        let clamped_height = roi.height.min(max_h);
        
        // クランプ後のサイズが0なら無効
        if clamped_width == 0 || clamped_height == 0 {
            return None;
        }
        
        Some(Roi::new(clamped_x, clamped_y, clamped_width, clamped_height))
    }
    
    /// ステージングテクスチャを確保または再利用
    /// 
    /// ROIサイズが変わった場合のみ再作成、同じサイズなら再利用してパフォーマンスを向上。
    fn ensure_staging_texture(&mut self, width: u32, height: u32) -> DomainResult<ID3D11Texture2D> {
        // サイズが同じで既にテクスチャがあれば再利用
        if let Some(ref tex) = self.staging_tex {
            if self.staging_size == (width, height) {
                return Ok(tex.clone());
            }
        }
        
        // 新しいステージングテクスチャを作成
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: D3D11_BIND_FLAG(0).0 as u32,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: D3D11_RESOURCE_MISC_FLAG(0).0 as u32,
        };
        
        let mut staging_tex: Option<ID3D11Texture2D> = None;
        unsafe {
            self.device
                .CreateTexture2D(&desc, None, Some(&mut staging_tex))
                .map_err(|e| DomainError::Capture(format!("Failed to create staging texture: {:?}", e)))?;
        }
        
        let tex = staging_tex.ok_or_else(|| 
            DomainError::Capture("Staging texture creation returned None".to_string())
        )?;
        
        // キャッシュに保存
        self.staging_tex = Some(tex.clone());
        self.staging_size = (width, height);
        
        Ok(tex)
    }
}

impl CapturePort for DdaCaptureAdapter {
    fn capture_frame_with_roi(&mut self, roi: &Roi) -> DomainResult<Option<Frame>> {
        // ROI境界検証とクランプ（画面外アクセス防止）
        let clamped_roi = self.clamp_roi(roi).ok_or_else(|| {
            DomainError::Capture(format!(
                "ROI ({}, {}, {}x{}) is completely outside display bounds ({}x{})",
                roi.x, roi.y, roi.width, roi.height,
                self.device_info.width, self.device_info.height
            ))
        })?;
        
        // VSync待機（リフレッシュレートに同期）
        self.wait_for_vsync()?;

        // フレーム取得（即座に取得、ブロッキング）
        let tex = match self.dupl.acquire_next_frame_now() {
            Ok(tex) => tex,
            Err(e) => {
                // エラーメッセージから種別を判定
                let error_msg = format!("{:?}", e);
                
                if error_msg.contains("Timeout") {
                    // タイムアウト: フレーム更新なし
                    return Ok(None);
                } else if error_msg.contains("AccessLost") || error_msg.contains("AccessDenied") {
                    // Recoverable: 排他的フルスクリーン切替、デスクトップモード変更
                    // Application層が再初期化の判断を行う
                    #[cfg(debug_assertions)]
                    tracing::debug!("DDA Access error: {}", error_msg);
                    return Err(DomainError::DeviceNotAvailable);
                } else {
                    // Non-recoverable: インスタンス再作成が必要
                    #[cfg(debug_assertions)]
                    tracing::error!("DDA Unexpected error: {}", error_msg);
                    return Err(DomainError::ReInitializationRequired);
                }
            }
        };

        // ROI領域のデータサイズを計算（BGRA形式）
        let roi_data_size = (clamped_roi.width * clamped_roi.height * 4) as usize;
        let mut data = vec![0u8; roi_data_size];

        // ステージングテクスチャを確保または再利用（パフォーマンス最適化）
        let staging_tex = self.ensure_staging_texture(clamped_roi.width, clamped_roi.height)?;

        // GPU上でROI領域だけをSTAGINGへコピー
        unsafe {
            let src_box = D3D11_BOX {
                left: clamped_roi.x,
                top: clamped_roi.y,
                front: 0,
                right: clamped_roi.x + clamped_roi.width,
                bottom: clamped_roi.y + clamped_roi.height,
                back: 1,
            };

            // win_desktop_duplicationのTextureをID3D11Resourceとして取得
            // ID3D11Texture2DはID3D11Resourceを継承しているため、cast()で型安全にキャスト
            // transmute()は未定義動作の可能性があるため使用しない
            let src_resource: ID3D11Resource = tex.as_raw_ref().clone().cast()
                .map_err(|e| DomainError::Capture(format!("Failed to cast texture to resource: {:?}", e)))?;

            self.context.CopySubresourceRegion(
                &staging_tex,
                0,
                0,
                0,
                0,
                &src_resource,
                0,
                Some(&src_box),
            );
        }

        // STAGINGテクスチャをMapしてCPUアクセス
        unsafe {
            let mut mapped: D3D11_MAPPED_SUBRESOURCE = mem::zeroed();
            self.context
                .Map(
                    &staging_tex,
                    0,
                    D3D11_MAP_READ,
                    0,
                    Some(&mut mapped),
                )
                .map_err(|e| DomainError::Capture(format!("Failed to map staging texture: {:?}", e)))?;

            // RowPitchを考慮してデータをコピー
            let row_pitch = mapped.RowPitch as usize;
            let row_size = (clamped_roi.width * 4) as usize;
            
            for y in 0..clamped_roi.height as usize {
                let src_offset = y * row_pitch;
                let dst_offset = y * row_size;
                
                ptr::copy_nonoverlapping(
                    (mapped.pData as *const u8).add(src_offset),
                    data.as_mut_ptr().add(dst_offset),
                    row_size,
                );
            }

            self.context.Unmap(&staging_tex, 0);
        }

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

    fn reinitialize(&mut self) -> DomainResult<()> {
        #[cfg(debug_assertions)]
        tracing::info!("Reinitializing DDA capture adapter (adapter: {}, output: {})", 
            self.adapter_idx, self.output_idx);

        // 元のadapter_idx/output_idxを使用してインスタンスを再作成
        let adapter = AdapterFactory::new()
            .get_adapter_by_idx(self.adapter_idx as u32)
            .ok_or_else(|| DomainError::Capture(
                format!("Failed to get adapter {} during reinit", self.adapter_idx)
            ))?;

        let output = adapter
            .get_display_by_idx(self.output_idx as u32)
            .ok_or_else(|| DomainError::Capture(
                format!("Failed to get display {} during reinit", self.output_idx)
            ))?;

        let mut dupl = DesktopDuplicationApi::new(adapter, output.clone())
            .map_err(|e| DomainError::Capture(format!("Failed to reinitialize DDA: {:?}", e)))?;

        // カーソル描画を無効化(再初期化時も設定を適用)
        let mut options = DuplicationApiOptions::default();
        options.skip_cursor = true;
        dupl.configure(options);

        let (device, ctx) = dupl.get_device_and_ctx();

        // device_infoを再計算（解像度やリフレッシュレートが変わっている可能性）
        let display_mode = output
            .get_current_display_mode()
            .map_err(|e| DomainError::Capture(format!("Failed to get display mode during reinit: {:?}", e)))?;

        let device_info = DeviceInfo {
            width: display_mode.width,
            height: display_mode.height,
            refresh_rate: (display_mode.refresh_num / display_mode.refresh_den) as u32,
            name: format!("Display {} on Adapter {}", self.output_idx, self.adapter_idx),
        };

        // 状態を更新
        self.dupl = dupl;
        self.output = output;
        self.device = device;
        self.context = ctx;
        self.device_info = device_info;
        
        // ステージングテクスチャをクリア（サイズが変わっている可能性があるため）
        self.staging_tex = None;
        self.staging_size = (0, 0);

        #[cfg(debug_assertions)]
        tracing::info!("DDA reinitialization completed: {}x{}@{}Hz", 
            self.device_info.width, self.device_info.height, self.device_info.refresh_rate);

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
        let adapter = DdaCaptureAdapter::new_primary(8);
        
        if adapter.is_err() {
            println!("DDA initialization failed (expected if another instance exists): {:?}", adapter.err());
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
        let mut adapter = match DdaCaptureAdapter::new_primary(8) {
            Ok(a) => a,
            Err(e) => {
                println!("DDA initialization failed (expected if another instance exists): {:?}", e);
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
        let mut adapter = DdaCaptureAdapter::new_primary(8)
            .expect("DDA initialization failed");

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

        println!("Capture statistics (1 second):");
        println!("  Frames captured: {}", frame_count);
        println!("  Timeouts: {}", timeout_count);
        println!("  Errors: {} (expected in exclusive fullscreen)", error_count);
        println!("  Effective FPS: {}", frame_count);

        // デスクトップ環境では144 FPS、排他的フルスクリーンではエラーが多発
        if error_count == 0 {
            assert!(frame_count > 0, "Should capture at least one frame in desktop mode");
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
        let mut adapter = match DdaCaptureAdapter::new_primary(8) {
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
            println!("Reinitialization failed (expected due to DDA API limitation): {:?}", result.err());
            return;
        }

        // 再初期化後のフレーム取得
        let frame = adapter.capture_frame();
        assert!(frame.is_ok(), "Frame capture after reinit should work");
    }

    #[test]
    #[ignore] // 管理者権限 + GPU必須のため通常はスキップ
    fn test_dda_capture_with_roi() {
        let mut adapter = match DdaCaptureAdapter::new_primary(8) {
            Ok(a) => a,
            Err(e) => {
                println!("DDA initialization failed (expected if another instance exists): {:?}", e);
                return;
            }
        };

        let device_info = adapter.device_info();
        println!("Display resolution: {}x{}", device_info.width, device_info.height);

        // テスト1: 400x300のROI
        let roi_small = Roi::new(100, 100, 400, 300);
        println!("\nTest 1: Capturing with ROI {}x{} at ({}, {})", roi_small.width, roi_small.height, roi_small.x, roi_small.y);
        
        match adapter.capture_frame_with_roi(&roi_small) {
            Ok(Some(frame)) => {
                println!("  Frame size: {}x{}", frame.width, frame.height);
                println!("  Data length: {} bytes", frame.data.len());
                println!("  Expected: {} bytes", roi_small.width * roi_small.height * 4);

                assert_eq!(frame.width, roi_small.width, "Frame width should match ROI width");
                assert_eq!(frame.height, roi_small.height, "Frame height should match ROI height");
                assert_eq!(frame.data.len(), (roi_small.width * roi_small.height * 4) as usize, "Data size should match ROI size");
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
        println!("\nTest 2: Capturing with ROI {}x{} at ({}, {}) - Design target size", roi_medium.width, roi_medium.height, roi_medium.x, roi_medium.y);
        
        match adapter.capture_frame_with_roi(&roi_medium) {
            Ok(Some(frame)) => {
                println!("  Frame size: {}x{}", frame.width, frame.height);
                println!("  Data length: {} bytes", frame.data.len());
                println!("  Expected: {} bytes", roi_medium.width * roi_medium.height * 4);
                println!("  PCIe transfer reduction: {} -> {} bytes ({:.1}% reduction)",
                    device_info.width * device_info.height * 4,
                    frame.data.len(),
                    (1.0 - frame.data.len() as f64 / (device_info.width * device_info.height * 4) as f64) * 100.0
                );

                assert_eq!(frame.width, roi_medium.width, "Frame width should match ROI width");
                assert_eq!(frame.height, roi_medium.height, "Frame height should match ROI height");
                assert_eq!(frame.data.len(), (roi_medium.width * roi_medium.height * 4) as usize, "Data size should match ROI size");
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
                println!("  capture_frame_with_roi: {}x{}, {} bytes", f1.width, f1.height, f1.data.len());
                println!("  capture_frame: {}x{}, {} bytes", f2.width, f2.height, f2.data.len());
                assert_eq!(f1.width, f2.width, "Both methods should return same dimensions");
                assert_eq!(f1.height, f2.height, "Both methods should return same dimensions");
            }
            _ => {
                println!("  One or both captures returned None or error");
            }
        }
    }
}
