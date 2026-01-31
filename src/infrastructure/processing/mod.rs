//! Processing module for image analysis.
//!
//! This module contains CPU and GPU processing implementations.
//! - `cpu/` - CPU-based processing using OpenCV
//! - `gpu/` - GPU-based processing using D3D11 compute shaders (placeholder)

pub mod cpu;
pub mod gpu;

// Re-export main processors for convenience
pub use cpu::ColorProcessAdapter;
pub use gpu::GpuColorProcessor;
