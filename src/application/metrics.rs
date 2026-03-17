//! 軽量で低レイテンシなメトリクスモジュール
//!
//! AtomicU64 と Arc のみを使用したスレッドセーフなメトリクス管理。
//! Mutex不使用で、全ての操作は Ordering::Relaxed で実行される。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// パイプラインメトリクス構造体
///
/// 各処理段階のカウンターとレイテンシを記録する。
/// 全フィールドは AtomicU64 で、スレッドセーフに複数スレッドから
/// 同時アクセス可能。
pub struct PipelineMetrics {
    /// キャプチャされたフレーム数
    pub frames_captured: AtomicU64,
    /// ドロップされたフレーム数
    pub frames_dropped: AtomicU64,
    /// 処理されたフレーム数
    pub frames_processed: AtomicU64,
    /// HID送信成功数
    pub hid_sends: AtomicU64,
    /// HID通信エラー数
    pub hid_errors: AtomicU64,
    /// キャプチャレイテンシ累計（マイクロ秒）
    pub capture_latency_us: AtomicU64,
    /// 処理レイテンシ累計（マイクロ秒）
    pub process_latency_us: AtomicU64,
    /// HID通信レイテンシ累計（マイクロ秒）
    pub hid_latency_us: AtomicU64,
    /// process->HID 送信完了までのレイテンシ累計（マイクロ秒）
    pub process_to_hid_latency_us: AtomicU64,
    /// エンドツーエンドレイテンシ累計（マイクロ秒）
    pub total_latency_us: AtomicU64,
}

impl PipelineMetrics {
    /// 新しい PipelineMetrics インスタンスを Arc でラップして返す
    ///
    /// # 戻り値
    /// 全カウンターが0に初期化された新しいメトリクスインスタンス
    pub fn new() -> Arc<Self> {
        Arc::new(PipelineMetrics {
            frames_captured: AtomicU64::new(0),
            frames_dropped: AtomicU64::new(0),
            frames_processed: AtomicU64::new(0),
            hid_sends: AtomicU64::new(0),
            hid_errors: AtomicU64::new(0),
            capture_latency_us: AtomicU64::new(0),
            process_latency_us: AtomicU64::new(0),
            hid_latency_us: AtomicU64::new(0),
            process_to_hid_latency_us: AtomicU64::new(0),
            total_latency_us: AtomicU64::new(0),
        })
    }

    /// キャプチャ処理時間を記録
    ///
    /// # 引数
    /// * `duration` - キャプチャ処理にかかった時間
    pub fn record_capture(&self, duration: Duration) {
        let us = duration.as_micros() as u64;
        self.capture_latency_us.fetch_add(us, Ordering::Relaxed);
        self.frames_captured.fetch_add(1, Ordering::Relaxed);
    }

    /// フレームドロップを記録
    pub fn record_frame_drop(&self) {
        self.frames_dropped.fetch_add(1, Ordering::Relaxed);
    }

    /// 処理レイテンシを記録
    ///
    /// # 引数
    /// * `duration` - 処理にかかった時間
    pub fn record_process(&self, duration: Duration) {
        let us = duration.as_micros() as u64;
        self.process_latency_us.fetch_add(us, Ordering::Relaxed);
        self.frames_processed.fetch_add(1, Ordering::Relaxed);
    }

    /// HID送信処理を記録
    ///
    /// # 引数
    /// * `duration` - HID送信にかかった時間
    pub fn record_hid_send(&self, duration: Duration) {
        let us = duration.as_micros() as u64;
        self.hid_latency_us.fetch_add(us, Ordering::Relaxed);
        self.hid_sends.fetch_add(1, Ordering::Relaxed);
    }

    /// process->HID送信完了までのレイテンシを記録
    pub fn record_process_to_hid_latency(&self, duration: Duration) {
        let us = duration.as_micros() as u64;
        self.process_to_hid_latency_us
            .fetch_add(us, Ordering::Relaxed);
    }

    /// HID通信エラーを記録
    pub fn record_hid_error(&self) {
        self.hid_errors.fetch_add(1, Ordering::Relaxed);
    }

    /// キャプチャ開始からHID送信完了までの総レイテンシを記録
    pub fn record_total_latency(&self, duration: Duration) {
        let us = duration.as_micros() as u64;
        self.total_latency_us.fetch_add(us, Ordering::Relaxed);
    }

    /// 全メトリクスのスナップショットを取得
    ///
    /// # 戻り値
    /// 現在のメトリクス値を plain u64 でコピーしたスナップショット
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            frames_captured: self.frames_captured.load(Ordering::Relaxed),
            frames_dropped: self.frames_dropped.load(Ordering::Relaxed),
            frames_processed: self.frames_processed.load(Ordering::Relaxed),
            hid_sends: self.hid_sends.load(Ordering::Relaxed),
            hid_errors: self.hid_errors.load(Ordering::Relaxed),
            capture_latency_us: self.capture_latency_us.load(Ordering::Relaxed),
            process_latency_us: self.process_latency_us.load(Ordering::Relaxed),
            hid_latency_us: self.hid_latency_us.load(Ordering::Relaxed),
            process_to_hid_latency_us: self.process_to_hid_latency_us.load(Ordering::Relaxed),
            total_latency_us: self.total_latency_us.load(Ordering::Relaxed),
        }
    }
}

/// メトリクススナップショット
///
/// PipelineMetrics から snapshot() で得られる、plain u64 フィールドを持つ構造体。
#[derive(Debug, Clone, Copy)]
pub struct MetricsSnapshot {
    /// キャプチャされたフレーム数
    pub frames_captured: u64,
    /// ドロップされたフレーム数
    pub frames_dropped: u64,
    /// 処理されたフレーム数
    pub frames_processed: u64,
    /// HID送信成功数
    pub hid_sends: u64,
    /// HID通信エラー数
    pub hid_errors: u64,
    /// キャプチャレイテンシ累計（マイクロ秒）
    pub capture_latency_us: u64,
    /// 処理レイテンシ累計（マイクロ秒）
    pub process_latency_us: u64,
    /// HID通信レイテンシ累計（マイクロ秒）
    pub hid_latency_us: u64,
    /// process->HID送信完了までのレイテンシ累計（マイクロ秒）
    pub process_to_hid_latency_us: u64,
    /// エンドツーエンドレイテンシ累計（マイクロ秒）
    pub total_latency_us: u64,
}

impl MetricsSnapshot {
    /// メトリクススナップショットを文字列フォーマットで出力
    ///
    /// # 戻り値
    /// 各メトリクス値を含む整形された文字列
    pub fn display(&self) -> String {
        format!(
            "Frames: captured={}, dropped={}, processed={} | HID: sends={}, errors={} | Latency (us): capture={}, process={}, hid={}, process_to_hid={}, total={}",
            self.frames_captured,
            self.frames_dropped,
            self.frames_processed,
            self.hid_sends,
            self.hid_errors,
            self.capture_latency_us,
            self.process_latency_us,
            self.hid_latency_us,
            self.process_to_hid_latency_us,
            self.total_latency_us,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::thread;

    #[test]
    fn test_new_initializes_to_zero() {
        let metrics = PipelineMetrics::new();
        let snap = metrics.snapshot();

        assert_eq!(snap.frames_captured, 0);
        assert_eq!(snap.frames_dropped, 0);
        assert_eq!(snap.frames_processed, 0);
        assert_eq!(snap.hid_sends, 0);
        assert_eq!(snap.hid_errors, 0);
        assert_eq!(snap.capture_latency_us, 0);
        assert_eq!(snap.process_latency_us, 0);
        assert_eq!(snap.hid_latency_us, 0);
        assert_eq!(snap.process_to_hid_latency_us, 0);
        assert_eq!(snap.total_latency_us, 0);
    }

    #[test]
    fn test_record_capture() {
        let metrics = PipelineMetrics::new();

        metrics.record_capture(Duration::from_micros(100));
        metrics.record_capture(Duration::from_micros(50));

        let snap = metrics.snapshot();
        assert_eq!(snap.frames_captured, 2);
        assert_eq!(snap.capture_latency_us, 150);
    }

    #[test]
    fn test_record_frame_drop() {
        let metrics = PipelineMetrics::new();

        metrics.record_frame_drop();
        metrics.record_frame_drop();
        metrics.record_frame_drop();

        let snap = metrics.snapshot();
        assert_eq!(snap.frames_dropped, 3);
    }

    #[test]
    fn test_record_process() {
        let metrics = PipelineMetrics::new();

        metrics.record_process(Duration::from_micros(200));
        metrics.record_process(Duration::from_micros(300));

        let snap = metrics.snapshot();
        assert_eq!(snap.frames_processed, 2);
        assert_eq!(snap.process_latency_us, 500);
    }

    #[test]
    fn test_record_hid_send() {
        let metrics = PipelineMetrics::new();

        metrics.record_hid_send(Duration::from_micros(10));
        metrics.record_hid_send(Duration::from_micros(15));

        let snap = metrics.snapshot();
        assert_eq!(snap.hid_sends, 2);
        assert_eq!(snap.hid_latency_us, 25);
    }

    #[test]
    fn test_record_hid_error() {
        let metrics = PipelineMetrics::new();

        metrics.record_hid_error();
        metrics.record_hid_error();

        let snap = metrics.snapshot();
        assert_eq!(snap.hid_errors, 2);
    }

    #[test]
    fn test_snapshot_returns_copy() {
        let metrics = PipelineMetrics::new();

        metrics.record_capture(Duration::from_micros(100));
        let snap1 = metrics.snapshot();

        metrics.record_capture(Duration::from_micros(50));
        let snap2 = metrics.snapshot();

        // snap1 는 캡처할 때의 값
        assert_eq!(snap1.frames_captured, 1);
        assert_eq!(snap1.capture_latency_us, 100);

        // snap2 는 현재의 값
        assert_eq!(snap2.frames_captured, 2);
        assert_eq!(snap2.capture_latency_us, 150);
    }

    #[test]
    fn test_metrics_snapshot_display() {
        let snap = MetricsSnapshot {
            frames_captured: 100,
            frames_dropped: 5,
            frames_processed: 95,
            hid_sends: 90,
            hid_errors: 2,
            capture_latency_us: 10000,
            process_latency_us: 5000,
            hid_latency_us: 500,
            process_to_hid_latency_us: 4500,
            total_latency_us: 15500,
        };

        let display_str = snap.display();
        assert!(display_str.contains("captured=100"));
        assert!(display_str.contains("dropped=5"));
        assert!(display_str.contains("sends=90"));
        assert!(display_str.contains("process_to_hid=4500"));
    }

    #[test]
    fn test_thread_safety_4threads_1000events() {
        let metrics = PipelineMetrics::new();
        let thread_count = 4;
        let events_per_thread = 1000;

        let mut handles = vec![];

        for _ in 0..thread_count {
            let metrics_clone = Arc::clone(&metrics);
            let handle = thread::spawn(move || {
                for _ in 0..events_per_thread {
                    metrics_clone.record_capture(Duration::from_micros(1));
                    metrics_clone.record_frame_drop();
                    metrics_clone.record_process(Duration::from_micros(2));
                    metrics_clone.record_hid_send(Duration::from_micros(1));
                    metrics_clone.record_hid_error();
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().expect("Thread panic");
        }

        let snap = metrics.snapshot();

        // 各スレッドが1000回ずつ記録したので、合計4000回
        assert_eq!(snap.frames_captured, 4000);
        assert_eq!(snap.frames_dropped, 4000);
        assert_eq!(snap.frames_processed, 4000);
        assert_eq!(snap.hid_sends, 4000);
        assert_eq!(snap.hid_errors, 4000);

        // レイテンシも期待値通り
        assert_eq!(snap.capture_latency_us, 4000); // 4000 threads * 1 us
        assert_eq!(snap.process_latency_us, 8000); // 4000 threads * 2 us
        assert_eq!(snap.hid_latency_us, 4000); // 4000 threads * 1 us
        assert_eq!(snap.process_to_hid_latency_us, 0);
    }

    #[test]
    fn test_concurrent_reads_and_writes() {
        let metrics = PipelineMetrics::new();
        let done = Arc::new(AtomicBool::new(false));

        // Writer thread
        let metrics_w = Arc::clone(&metrics);
        let done_w = Arc::clone(&done);
        let writer = thread::spawn(move || {
            for i in 0..1000 {
                metrics_w.record_capture(Duration::from_micros(i % 100));
                thread::yield_now();
            }
            done_w.store(true, Ordering::Relaxed);
        });

        // Reader threads
        let mut readers = vec![];
        for _ in 0..3 {
            let metrics_r = Arc::clone(&metrics);
            let done_r = Arc::clone(&done);
            let reader = thread::spawn(move || {
                while !done_r.load(Ordering::Relaxed) {
                    let _snap = metrics_r.snapshot();
                    thread::yield_now();
                }
            });
            readers.push(reader);
        }

        writer.join().expect("Writer panic");
        for reader in readers {
            reader.join().expect("Reader panic");
        }

        let final_snap = metrics.snapshot();
        assert_eq!(final_snap.frames_captured, 1000);
    }

    #[test]
    fn test_duration_conversion() {
        let metrics = PipelineMetrics::new();

        metrics.record_capture(Duration::from_millis(1)); // 1000 us
        metrics.record_capture(Duration::from_secs(1)); // 1_000_000 us

        let snap = metrics.snapshot();
        assert_eq!(snap.capture_latency_us, 1_001_000);
    }

    #[test]
    fn test_record_total_latency() {
        let metrics = PipelineMetrics::new();

        metrics.record_total_latency(Duration::from_micros(100));
        metrics.record_total_latency(Duration::from_micros(250));

        let snap = metrics.snapshot();
        assert_eq!(snap.total_latency_us, 350);
    }

    #[test]
    fn test_record_process_to_hid_latency() {
        let metrics = PipelineMetrics::new();

        metrics.record_process_to_hid_latency(Duration::from_micros(40));
        metrics.record_process_to_hid_latency(Duration::from_micros(60));

        let snap = metrics.snapshot();
        assert_eq!(snap.process_to_hid_latency_us, 100);
    }
}
