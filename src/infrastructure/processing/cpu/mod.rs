//! CPU HSV color processing adapter.

use crate::domain::error::{DomainError, DomainResult};
use crate::domain::ports::ProcessPort;
use crate::domain::types::{DetectionResult, Frame, HsvRange, ProcessorBackend, Roi};
use opencv::core::{self, Mat, Scalar};
use opencv::imgproc;
use opencv::prelude::*;

/// OpenCV-based CPU adapter for HSV color detection.
pub struct ColorProcessAdapter {
    bgr: Mat,
    hsv: Mat,
    mask: Mat,
}

impl ColorProcessAdapter {
    /// Creates a new CPU color processing adapter.
    pub fn new() -> DomainResult<Self> {
        core::set_num_threads(1).map_err(|e| DomainError::Process(e.to_string()))?;

        Ok(Self {
            bgr: Mat::default(),
            hsv: Mat::default(),
            mask: Mat::default(),
        })
    }

    fn process_with_opencv(
        &mut self,
        frame: &Frame,
        roi: &Roi,
        hsv_range: &HsvRange,
    ) -> DomainResult<DetectionResult> {
        let expected_len = frame.width as usize * frame.height as usize * 4;
        if frame.data.len() != expected_len {
            return Err(DomainError::Process(format!(
                "invalid frame length: expected {expected_len}, got {}",
                frame.data.len()
            )));
        }

        let bgra_flat =
            Mat::from_slice(&frame.data).map_err(|e| DomainError::Process(e.to_string()))?;
        let bgra = bgra_flat
            .reshape(4, frame.height as i32)
            .map_err(|e| DomainError::Process(e.to_string()))?;

        imgproc::cvt_color_def(&bgra, &mut self.bgr, imgproc::COLOR_BGRA2BGR)
            .map_err(|e| DomainError::Process(e.to_string()))?;
        imgproc::cvt_color_def(&self.bgr, &mut self.hsv, imgproc::COLOR_BGR2HSV)
            .map_err(|e| DomainError::Process(e.to_string()))?;

        let lower = Scalar::new(
            hsv_range.h_low as f64,
            hsv_range.s_low as f64,
            hsv_range.v_low as f64,
            0.0,
        );
        let upper = Scalar::new(
            hsv_range.h_high as f64,
            hsv_range.s_high as f64,
            hsv_range.v_high as f64,
            0.0,
        );

        core::in_range(&self.hsv, &lower, &upper, &mut self.mask)
            .map_err(|e| DomainError::Process(e.to_string()))?;

        #[cfg(feature = "opencv-debug-display")]
        {
            opencv::highgui::imshow("cpu_bgr", &self.bgr)
                .map_err(|e| DomainError::Process(e.to_string()))?;
            opencv::highgui::imshow("cpu_hsv", &self.hsv)
                .map_err(|e| DomainError::Process(e.to_string()))?;
            opencv::highgui::imshow("cpu_mask", &self.mask)
                .map_err(|e| DomainError::Process(e.to_string()))?;
            opencv::highgui::wait_key(1).map_err(|e| DomainError::Process(e.to_string()))?;
        }

        let moments =
            imgproc::moments(&self.mask, true).map_err(|e| DomainError::Process(e.to_string()))?;
        if moments.m00 <= f64::EPSILON {
            return Ok(DetectionResult::not_detected());
        }

        let center_x = (moments.m10 / moments.m00) as f32;
        let center_y = (moments.m01 / moments.m00) as f32;
        let matched_pixels = (moments.m00 / 255.0) as f32;
        let roi_area = (roi.width as f32 * roi.height as f32).max(1.0);
        let coverage = (matched_pixels / roi_area).clamp(0.0, 1.0);

        Ok(DetectionResult::detected(center_x, center_y, coverage))
    }
}

impl ProcessPort for ColorProcessAdapter {
    fn process_frame(
        &mut self,
        frame: &Frame,
        roi: &Roi,
        hsv_range: &HsvRange,
    ) -> DomainResult<DetectionResult> {
        self.process_with_opencv(frame, roi, hsv_range)
    }

    fn backend(&self) -> ProcessorBackend {
        ProcessorBackend::Cpu
    }

    fn supports_gpu_processing(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_bgra_frame(width: u32, height: u32, b: u8, g: u8, r: u8, a: u8) -> Frame {
        let mut data = vec![0_u8; width as usize * height as usize * 4];
        for px in data.chunks_exact_mut(4) {
            px[0] = b;
            px[1] = g;
            px[2] = r;
            px[3] = a;
        }
        Frame::new(data, width, height)
    }

    #[test]
    fn process_frame_detects_solid_color_and_returns_center() {
        let mut adapter = ColorProcessAdapter::new().expect("adapter should initialize");
        let frame = solid_bgra_frame(4, 4, 0, 255, 0, 255);
        let roi = Roi::new(0, 0, 4, 4);
        let green_hsv = HsvRange::new(50, 70, 100, 255, 100, 255);

        let result = adapter
            .process_frame(&frame, &roi, &green_hsv)
            .expect("processing should succeed");

        assert!(result.detected);
        assert!(
            (result.center_x - 1.5).abs() < 0.2,
            "center_x={}",
            result.center_x
        );
        assert!(
            (result.center_y - 1.5).abs() < 0.2,
            "center_y={}",
            result.center_y
        );
        assert!(result.coverage > 0.0, "coverage={}", result.coverage);
    }

    #[test]
    fn process_frame_returns_not_detected_when_no_color_match() {
        let mut adapter = ColorProcessAdapter::new().expect("adapter should initialize");
        let frame = solid_bgra_frame(4, 4, 255, 0, 0, 255);
        let roi = Roi::new(0, 0, 4, 4);
        let green_hsv = HsvRange::new(50, 70, 100, 255, 100, 255);

        let result = adapter
            .process_frame(&frame, &roi, &green_hsv)
            .expect("processing should succeed");

        assert!(!result.detected);
        assert_eq!(result.center_x, 0.0);
        assert_eq!(result.center_y, 0.0);
        assert_eq!(result.coverage, 0.0);
    }

    #[test]
    fn backend_returns_cpu() {
        let adapter = ColorProcessAdapter::new().expect("adapter should initialize");
        assert_eq!(adapter.backend(), ProcessorBackend::Cpu);
    }

    #[test]
    fn supports_gpu_processing_returns_false() {
        let adapter = ColorProcessAdapter::new().expect("adapter should initialize");
        assert!(!adapter.supports_gpu_processing());
    }
}
