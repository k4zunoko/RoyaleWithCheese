//! Domain層: ビジネスロジックの中心
//!
//! 外部依存を持たない純粋なRust型とtrait定義。
//! Applicationから注入され、Infrastructureで実装される。

pub mod config;
pub mod error;
pub mod gpu_ports;
pub mod ports;
pub mod types;

pub use config::*;
pub use error::*;
pub use ports::*;
pub use types::*;
