//! Domain core types
//!
//! Core data structures used throughout the entire pipeline.
//! These types are shared across all layers and are immutable-first.

use std::time::Instant;
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT;

/// Region of Interest - pixel coordinates for capture and processing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Roi {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Roi {
    /// Create a new ROI
    pub fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Get the center coordinates of this ROI
    #[inline]
    pub fn center(&self) -> (u32, u32) {
        (self.x + self.width / 2, self.y + self.height / 2)
    }

    /// Get the area of this ROI
    #[inline]
    pub fn area(&self) -> u32 {
        self.width * self.height
    }

    /// Check if this ROI intersects with another ROI
    #[inline]
    pub fn intersects(&self, other: &Roi) -> bool {
        let self_x2 = self.x + self.width;
        let self_y2 = self.y + self.height;
        let other_x2 = other.x + other.width;
        let other_y2 = other.y + other.height;

        self.x < other_x2 && self_x2 > other.x && self.y < other_y2 && self_y2 > other.y
    }

    /// Create an ROI centered in the given screen dimensions
    ///
    /// Returns None if the ROI is larger than the screen.
    /// Formula: x = (screen_width - self.width) / 2
    #[inline]
    pub fn centered_in(self, screen_width: u32, screen_height: u32) -> Option<Roi> {
        if self.width > screen_width || self.height > screen_height {
            return None;
        }

        let x = (screen_width - self.width) / 2;
        let y = (screen_height - self.height) / 2;

        Some(Roi::new(x, y, self.width, self.height))
    }
}

/// HSV color range (OpenCV convention: H[0-180], S[0-255], V[0-255])
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HsvRange {
    pub h_low: u8,
    pub h_high: u8,
    pub s_low: u8,
    pub s_high: u8,
    pub v_low: u8,
    pub v_high: u8,
}

impl HsvRange {
    /// Create a new HSV range
    pub fn new(h_low: u8, h_high: u8, s_low: u8, s_high: u8, v_low: u8, v_high: u8) -> Self {
        Self {
            h_low,
            h_high,
            s_low,
            s_high,
            v_low,
            v_high,
        }
    }
}

/// CPU-resident frame data (BGR format, continuous memory)
#[derive(Debug, Clone)]
pub struct Frame {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub timestamp: Instant,
    pub dirty_rects: Vec<Roi>,
}

impl Frame {
    /// Create a new frame
    pub fn new(data: Vec<u8>, width: u32, height: u32) -> Self {
        Self {
            data,
            width,
            height,
            timestamp: Instant::now(),
            dirty_rects: Vec::new(),
        }
    }
}

/// GPU-resident texture frame for D3D11 compute processing
#[derive(Debug)]
pub struct GpuFrame {
    pub texture: Option<ID3D11Texture2D>,
    pub width: u32,
    pub height: u32,
    pub format: DXGI_FORMAT,
    pub timestamp: Instant,
}

impl GpuFrame {
    /// Create a new GPU frame
    pub fn new(
        texture: Option<ID3D11Texture2D>,
        width: u32,
        height: u32,
        format: DXGI_FORMAT,
    ) -> Self {
        Self {
            texture,
            width,
            height,
            format,
            timestamp: Instant::now(),
        }
    }
}

/// Bounding box with float coordinates
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl BoundingBox {
    /// Create a new bounding box
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

/// Result of object detection
#[derive(Debug, Clone)]
pub struct DetectionResult {
    pub detected: bool,
    pub center_x: f32,
    pub center_y: f32,
    pub coverage: f32,
    pub bounding_box: Option<BoundingBox>,
}

impl DetectionResult {
    /// Create a "not detected" result
    pub fn not_detected() -> Self {
        Self {
            detected: false,
            center_x: 0.0,
            center_y: 0.0,
            coverage: 0.0,
            bounding_box: None,
        }
    }

    /// Create a detection result with coordinates
    pub fn detected(center_x: f32, center_y: f32, coverage: f32) -> Self {
        Self {
            detected: true,
            center_x,
            center_y,
            coverage,
            bounding_box: None,
        }
    }
}

/// Transformed coordinates (delta from expected center)
#[derive(Debug, Clone, Copy)]
pub struct TransformedCoordinates {
    pub delta_x: f64,
    pub delta_y: f64,
    pub detected: bool,
}

impl TransformedCoordinates {
    /// Create new transformed coordinates
    pub fn new(delta_x: f64, delta_y: f64, detected: bool) -> Self {
        Self {
            delta_x,
            delta_y,
            detected,
        }
    }
}

/// Device display information
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub width: u32,
    pub height: u32,
    pub name: String,
}

impl DeviceInfo {
    /// Create new device info
    pub fn new(width: u32, height: u32, name: String) -> Self {
        Self {
            width,
            height,
            name,
        }
    }
}

/// Processor backend selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessorBackend {
    Cpu,
    Gpu,
}

/// Input state (mouse/keyboard buttons)
#[derive(Debug, Clone, Default)]
pub struct InputState {
    pub mouse_left: bool,
    pub mouse_right: bool,
}

/// Virtual key codes for HID output
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VirtualKey {
    Insert,
    LeftButton,
    RightButton,
    LeftControl,
    LeftAlt,
}

impl VirtualKey {
    /// Convert to Windows virtual key code (u16)
    pub fn to_vk_code(&self) -> u16 {
        match self {
            VirtualKey::Insert => 0x2D,
            VirtualKey::LeftButton => 0x01,
            VirtualKey::RightButton => 0x02,
            VirtualKey::LeftControl => 0xA2,
            VirtualKey::LeftAlt => 0xA4,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roi_centered_in_standard() {
        let roi = Roi::new(0, 0, 200, 200);
        let result = roi.centered_in(1920, 1080);
        assert_eq!(
            result,
            Some(Roi {
                x: 860,
                y: 440,
                width: 200,
                height: 200
            })
        );
    }

    #[test]
    fn roi_centered_in_roi_larger_than_screen() {
        let roi = Roi::new(0, 0, 200, 200);
        let result = roi.centered_in(100, 100);
        assert_eq!(result, None);
    }

    #[test]
    fn roi_centered_in_roi_wider_than_screen() {
        let roi = Roi::new(0, 0, 200, 100);
        let result = roi.centered_in(100, 500);
        assert_eq!(result, None);
    }

    #[test]
    fn roi_centered_in_roi_taller_than_screen() {
        let roi = Roi::new(0, 0, 100, 200);
        let result = roi.centered_in(500, 100);
        assert_eq!(result, None);
    }

    #[test]
    fn roi_area() {
        let roi = Roi::new(10, 20, 300, 400);
        assert_eq!(roi.area(), 120_000);
    }

    #[test]
    fn roi_center() {
        let roi = Roi::new(100, 200, 80, 60);
        assert_eq!(roi.center(), (140, 230));
    }

    #[test]
    fn roi_intersects_overlapping() {
        let roi1 = Roi::new(0, 0, 100, 100);
        let roi2 = Roi::new(50, 50, 100, 100);
        assert!(roi1.intersects(&roi2));
        assert!(roi2.intersects(&roi1));
    }

    #[test]
    fn roi_intersects_non_overlapping_horizontal() {
        let roi1 = Roi::new(0, 0, 100, 100);
        let roi2 = Roi::new(100, 0, 100, 100);
        assert!(!roi1.intersects(&roi2));
        assert!(!roi2.intersects(&roi1));
    }

    #[test]
    fn roi_intersects_non_overlapping_vertical() {
        let roi1 = Roi::new(0, 0, 100, 100);
        let roi2 = Roi::new(0, 100, 100, 100);
        assert!(!roi1.intersects(&roi2));
        assert!(!roi2.intersects(&roi1));
    }

    #[test]
    fn roi_intersects_diagonal() {
        let roi1 = Roi::new(0, 0, 100, 100);
        let roi2 = Roi::new(80, 80, 100, 100);
        assert!(roi1.intersects(&roi2));
    }

    #[test]
    fn roi_intersects_touching_edge_no_overlap() {
        let roi1 = Roi::new(0, 0, 100, 100);
        let roi2 = Roi::new(100, 100, 100, 100);
        assert!(!roi1.intersects(&roi2));
    }

    #[test]
    fn roi_new() {
        let roi = Roi::new(10, 20, 300, 400);
        assert_eq!(roi.x, 10);
        assert_eq!(roi.y, 20);
        assert_eq!(roi.width, 300);
        assert_eq!(roi.height, 400);
    }

    #[test]
    fn hsv_range_new() {
        let range = HsvRange::new(0, 180, 50, 255, 100, 255);
        assert_eq!(range.h_low, 0);
        assert_eq!(range.h_high, 180);
        assert_eq!(range.s_low, 50);
        assert_eq!(range.s_high, 255);
        assert_eq!(range.v_low, 100);
        assert_eq!(range.v_high, 255);
    }

    #[test]
    fn frame_new() {
        let data = vec![1, 2, 3, 4];
        let frame = Frame::new(data.clone(), 2, 2);
        assert_eq!(frame.data, data);
        assert_eq!(frame.width, 2);
        assert_eq!(frame.height, 2);
        assert_eq!(frame.dirty_rects.len(), 0);
    }

    #[test]
    fn bounding_box_new() {
        let bbox = BoundingBox::new(10.5, 20.3, 100.0, 200.0);
        assert_eq!(bbox.x, 10.5);
        assert_eq!(bbox.y, 20.3);
        assert_eq!(bbox.width, 100.0);
        assert_eq!(bbox.height, 200.0);
    }

    #[test]
    fn detection_result_not_detected() {
        let result = DetectionResult::not_detected();
        assert!(!result.detected);
        assert_eq!(result.center_x, 0.0);
        assert_eq!(result.center_y, 0.0);
        assert_eq!(result.coverage, 0.0);
        assert!(result.bounding_box.is_none());
    }

    #[test]
    fn detection_result_detected() {
        let result = DetectionResult::detected(500.5, 600.3, 0.75);
        assert!(result.detected);
        assert_eq!(result.center_x, 500.5);
        assert_eq!(result.center_y, 600.3);
        assert_eq!(result.coverage, 0.75);
        assert!(result.bounding_box.is_none());
    }

    #[test]
    fn transformed_coordinates_new() {
        let tc = TransformedCoordinates::new(1.5, -2.3, true);
        assert_eq!(tc.delta_x, 1.5);
        assert_eq!(tc.delta_y, -2.3);
        assert!(tc.detected);
    }

    #[test]
    fn device_info_new() {
        let info = DeviceInfo::new(1920, 1080, "Primary Display".to_string());
        assert_eq!(info.width, 1920);
        assert_eq!(info.height, 1080);
        assert_eq!(info.name, "Primary Display");
    }

    #[test]
    fn processor_backend_equality() {
        assert_eq!(ProcessorBackend::Cpu, ProcessorBackend::Cpu);
        assert_eq!(ProcessorBackend::Gpu, ProcessorBackend::Gpu);
        assert_ne!(ProcessorBackend::Cpu, ProcessorBackend::Gpu);
    }

    #[test]
    fn input_state_default() {
        let state = InputState::default();
        assert!(!state.mouse_left);
        assert!(!state.mouse_right);
    }

    #[test]
    fn virtual_key_to_vk_code() {
        assert_eq!(VirtualKey::Insert.to_vk_code(), 0x2D);
        assert_eq!(VirtualKey::LeftButton.to_vk_code(), 0x01);
        assert_eq!(VirtualKey::RightButton.to_vk_code(), 0x02);
        assert_eq!(VirtualKey::LeftControl.to_vk_code(), 0xA2);
        assert_eq!(VirtualKey::LeftAlt.to_vk_code(), 0xA4);
    }

    #[test]
    fn virtual_key_equality() {
        assert_eq!(VirtualKey::Insert, VirtualKey::Insert);
        assert_ne!(VirtualKey::Insert, VirtualKey::LeftButton);
    }

    #[test]
    fn gpu_frame_new() {
        let gpu_frame = GpuFrame::new(None, 1920, 1080, DXGI_FORMAT(0));
        assert_eq!(gpu_frame.width, 1920);
        assert_eq!(gpu_frame.height, 1080);
        assert!(gpu_frame.texture.is_none());
    }
}
