//! Processing module for image analysis.
//!
//! This module contains CPU and GPU processing implementations.
//! - `cpu/` - CPU-based processing using OpenCV
//! - `gpu/` - GPU-based processing using D3D11 compute shaders

pub mod cpu;
pub mod gpu;

// Re-export main processors for convenience
pub use cpu::ColorProcessAdapter;
pub use gpu::GpuColorProcessor;

// Re-export GPU adapter for ProcessPort integration
pub use gpu::adapter::GpuColorAdapter;
