# Decisions — zero-from-scratch-redesign

## [2026-03-10] Architecture Decisions

### Ownership-per-thread
- Each pipeline thread owns its adapter exclusively
- NO Arc<Mutex<T>> wrapping CapturePort, ProcessPort, or CommPort on hot path
- Channels only for inter-thread communication: crossbeam_channel::bounded(1)

### ProcessPort Trait Design
- Merge GpuProcessPort INTO ProcessPort (Metis recommendation)
- CapturePort, ProcessPort, CommPort: Send only (no Sync)
- InputPort: Send + Sync (stateless, read-only)

### ProcessSelector Enum Dispatch
- Use ProcessSelector enum instead of dyn ProcessPort (avoids vtable overhead)
- No wildcard match `_` — force explicit handling of all variants

### WGC Internal Mutex Exception
- WGC adapter INTERNALLY uses Arc<Mutex<Option<CapturedFrameData>>> for handler→adapter
- This is allowed because it's encapsulated inside the adapter
- Public API must NOT expose Mutex

### Metrics
- PipelineMetrics: AtomicU64 fields only, shared via Arc<PipelineMetrics>
- NO Mutex for metrics — pure atomic operations
- Available in ALL builds (not feature-gated)
- Use Ordering::Relaxed for all counter ops

### Error Recovery
- Thread failure → pipeline shutdown (simplest, safest)
- RecoveryState with exponential backoff (configurable)
- Recovery is per-thread, not coordinated

### Config
- Startup-only TOML loading (no hot-reload)
- Config sections: capture, process, communication, roi, gpu, debug

## [2026-03-10] Task 20 Testing Decisions

- Added `tests/pipeline_integration.rs` as hardware-independent end-to-end coverage for pipeline orchestration.
- Chose stop strategy: test-owned `Arc<AtomicBool>` embedded in mock capture adapter; adapter emits non-recoverable `DomainError::Capture("test stop")` when signaled.
- Included one `#[ignore]` hardware smoke test in integration file to document real-device execution path while keeping default CI/local runs hardware-free.
- Updated legacy `tests/gpu_integration.rs` and `tests/gpu_capture_integration.rs` import/API usage so full test compilation works with current module layout.
