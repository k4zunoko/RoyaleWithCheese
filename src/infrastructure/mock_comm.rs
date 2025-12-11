/// モック通信アダプタ
/// 
/// テスト・開発用のHID通信モック実装。
/// データをログに出力するのみで、実際のHID送信は行わない。

use crate::domain::{CommPort, DomainResult};

/// モック通信アダプタ
pub struct MockCommAdapter {
    connected: bool,
}

impl MockCommAdapter {
    /// 新しいモック通信アダプタを作成
    pub fn new() -> Self {
        Self { connected: true }
    }
}

impl Default for MockCommAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl CommPort for MockCommAdapter {
    fn send(&mut self, data: &[u8]) -> DomainResult<()> {
        // モック実装: ログに出力のみ
        #[cfg(debug_assertions)]
        tracing::debug!("MockComm: Sending {} bytes: {:02X?}", data.len(), &data[..data.len().min(16)]);
        
        #[cfg(not(debug_assertions))]
        let _ = data;
        
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    fn reconnect(&mut self) -> DomainResult<()> {
        self.connected = true;
        
        #[cfg(debug_assertions)]
        tracing::info!("MockComm: Reconnected");
        
        Ok(())
    }
}
