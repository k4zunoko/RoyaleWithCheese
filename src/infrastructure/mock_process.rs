/// モック画像処理アダプタ
/// 
/// テスト・開発用の画像処理モック実装。
/// 常に検出成功を返す。

use crate::domain::{DetectionResult, DomainResult, Frame, HsvRange, ProcessPort, ProcessorBackend, Roi};
use std::time::Instant;

/// モック画像処理アダプタ
pub struct MockProcessAdapter {
    backend: ProcessorBackend,
}

impl MockProcessAdapter {
    /// 新しいモック処理アダプタを作成
    pub fn new() -> Self {
        Self {
            backend: ProcessorBackend::Cpu,
        }
    }
}

impl Default for MockProcessAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessPort for MockProcessAdapter {
    fn process_frame(
        &mut self,
        _frame: &Frame,
        roi: &Roi,
        _hsv_range: &HsvRange,
    ) -> DomainResult<DetectionResult> {
        // モック実装: ROI中心を検出結果として返す
        let center = roi.center();
        
        Ok(DetectionResult {
            timestamp: Instant::now(),
            detected: true,
            center_x: center.0 as f32,
            center_y: center.1 as f32,
            coverage: (roi.width * roi.height / 10) as u32, // ROI面積の10%を検出したと仮定
        })
    }

    fn backend(&self) -> ProcessorBackend {
        self.backend
    }
}
