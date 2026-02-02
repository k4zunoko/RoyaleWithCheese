//! 処理アダプタのセレクタ（実行時選択用）
//!
//! ビルド時のfeatureフラグではなく、実行時に設定で処理方式を選択するための列挙型。
//! vtableのオーバーヘッドを避けるため、trait objectではなくenumでディスパッチ。

use crate::domain::gpu_ports::GpuProcessPort;
use crate::domain::{
    DetectionResult, DomainError, DomainResult, Frame, GpuFrame, HsvRange, ProcessPort,
    ProcessorBackend, Roi,
};
use crate::infrastructure::processing::cpu::ColorProcessAdapter;
use crate::infrastructure::processing::GpuColorAdapter;

/// 処理アダプタの選択
pub enum ProcessSelector {
    /// HSV色検知（高速、CPU版）
    FastColor(ColorProcessAdapter),
    /// HSV色検知（高速、GPU版）
    FastColorGpu(GpuColorAdapter),
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
            ProcessSelector::FastColorGpu(adapter) => adapter.process_frame(frame, roi, hsv_range),
            ProcessSelector::YoloOrt => {
                // 将来の実装
                unimplemented!("YoloOrt is not yet implemented")
            }
        }
    }

    fn backend(&self) -> ProcessorBackend {
        match self {
            ProcessSelector::FastColor(adapter) => adapter.backend(),
            ProcessSelector::FastColorGpu(adapter) => adapter.backend(),
            ProcessSelector::YoloOrt => ProcessorBackend::Cpu, // 将来の実装: YOLOのバックエンドを返す
        }
    }

    fn supports_gpu_processing(&self) -> bool {
        matches!(self, ProcessSelector::FastColorGpu(_))
    }

    fn process_gpu_frame(
        &mut self,
        gpu_frame: &GpuFrame,
        hsv_range: &HsvRange,
    ) -> DomainResult<DetectionResult> {
        match self {
            ProcessSelector::FastColorGpu(adapter) => {
                adapter.process_gpu_frame(gpu_frame, hsv_range)
            }
            ProcessSelector::FastColor(adapter) => adapter.process_gpu_frame(gpu_frame, hsv_range),
            ProcessSelector::YoloOrt => {
                unimplemented!("YoloOrt is not yet implemented")
            }
        }
    }
}

impl ProcessSelector {
    /// Check if GPU backend is being used
    pub fn is_gpu(&self) -> bool {
        matches!(self, ProcessSelector::FastColorGpu(_))
    }

    /// Check if CPU backend is being used
    pub fn is_cpu(&self) -> bool {
        matches!(self, ProcessSelector::FastColor(_))
    }

    /// Get the backend type
    pub fn backend_type(&self) -> &'static str {
        match self {
            ProcessSelector::FastColor(_) => "CPU (OpenCV)",
            ProcessSelector::FastColorGpu(_) => "GPU (D3D11 Compute Shader)",
            ProcessSelector::YoloOrt => "CPU (YOLO/ONNX - not implemented)",
        }
    }

    /// Create a new CPU-based selector
    pub fn new_cpu(adapter: ColorProcessAdapter) -> Self {
        ProcessSelector::FastColor(adapter)
    }

    /// Create a new GPU-based selector
    pub fn new_gpu(adapter: GpuColorAdapter) -> Self {
        ProcessSelector::FastColorGpu(adapter)
    }

    /// Process a GPU frame directly (zero-copy GPU pipeline)
    ///
    /// This method is only available when the selector is in GPU mode.
    /// It processes the frame directly on GPU without CPU transfer.
    pub fn process_gpu_frame(
        &mut self,
        gpu_frame: &GpuFrame,
        hsv_range: &HsvRange,
    ) -> DomainResult<DetectionResult> {
        match self {
            ProcessSelector::FastColorGpu(adapter) => {
                adapter.process_gpu_frame(gpu_frame, hsv_range)
            }
            ProcessSelector::FastColor(_) => Err(DomainError::Process(
                "CPU adapter cannot process GPU frames. Use process_frame() instead.".to_string(),
            )),
            ProcessSelector::YoloOrt => {
                unimplemented!("YoloOrt is not yet implemented")
            }
        }
    }
}
