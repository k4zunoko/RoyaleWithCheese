//! Logging initialisation.
//!
//! Configures `tracing_subscriber` with `EnvFilter` for the pipeline.
//!
//! * Debug builds: full tracing with file/line info and DEBUG+ level.
//! * Release builds: WARN+ only (minimal noise on the hot path).
//! * Feature `performance-timing`: INFO level so per-stage timing macros fire.

use tracing_subscriber::{fmt, EnvFilter};

/// Initialise the global tracing subscriber.
///
/// Must be called once at the start of `main`. Subsequent calls are no-ops
/// because `tracing_subscriber::fmt::init` panics on re-init; we use
/// `try_init` to swallow that gracefully.
pub fn init_logging() {
    #[cfg(not(debug_assertions))]
    let default_filter = {
        #[cfg(feature = "performance-timing")]
        let f = "info";
        #[cfg(not(feature = "performance-timing"))]
        let f = "warn";
        f
    };

    #[cfg(debug_assertions)]
    let default_filter = {
        #[cfg(feature = "performance-timing")]
        let f = "debug";
        #[cfg(not(feature = "performance-timing"))]
        let f = "debug";
        f
    };

    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));

    #[cfg(debug_assertions)]
    let _ = fmt::Subscriber::builder()
        .with_env_filter(filter)
        .with_file(true)
        .with_line_number(true)
        .with_target(true)
        .try_init();

    #[cfg(not(debug_assertions))]
    let _ = fmt::Subscriber::builder()
        .with_env_filter(filter)
        .with_file(false)
        .with_line_number(false)
        .with_target(false)
        .try_init();
}
