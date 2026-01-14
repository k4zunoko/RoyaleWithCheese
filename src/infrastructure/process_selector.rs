//! 処理アダプタのセレクタ（実行時選択用）
//!
//! ビルド時のfeatureフラグではなく、実行時に設定で処理方式を選択するための列挙型。
//! vtableのオーバーヘッドを避けるため、trait objectではなくenumでディスパッチ。

use crate::domain::{
    DetectionResult, DomainResult, Frame, HsvRange, ProcessPort, ProcessorBackend, Roi,
};
use crate::infrastructure::color_process::ColorProcessAdapter;

/// 処理アダプタの選択
pub enum ProcessSelector {
    /// HSV色検知（高速）
    FastColor(ColorProcessAdapter),
    /// YOLO + ONNX Runtime（将来実装）
    #[allow(dead_code)]
    YoloOrt, // 将来: YoloOrt(YoloProcessAdapter)
}

impl ProcessPort for ProcessSelector {
    fn process_frame(
        &mut self,
        frame: &Frame,
        roi: &Roi,
        hsv_range: &HsvRange,
    ) -> DomainResult<DetectionResult> {
        match self {
            ProcessSelector::FastColor(adapter) => adapter.process_frame(frame, roi, hsv_range),
            ProcessSelector::YoloOrt => {
                // 将来の実装
                unimplemented!("YoloOrt is not yet implemented")
            }
        }
    }

    fn backend(&self) -> ProcessorBackend {
        match self {
            ProcessSelector::FastColor(adapter) => adapter.backend(),
            ProcessSelector::YoloOrt => ProcessorBackend::Cpu, // 将来の実装: YOLOのバックエンドを返す
        }
    }
}
