/// Domain層: ビジネスロジックの中心
/// 
/// 外部依存を持たない純粋なRust型とtrait定義。
/// Applicationから注入され、Infrastructureで実装される。

pub mod types;
pub mod ports;
pub mod config;
pub mod error;

pub use types::*;
pub use ports::*;
pub use config::*;
pub use error::*;
