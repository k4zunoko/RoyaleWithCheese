// Recovery strategy

use std::time::Instant;

/// Mutable state tracking recovery progress for a single thread.
///
/// Stack-allocated per thread — no Arc/Mutex needed.
#[derive(Debug)]
pub struct RecoveryState {
    /// Number of consecutive failures since the last success (or start).
    pub consecutive_failures: u32,
    /// When the last recovery attempt was started.
    pub last_attempt: Option<Instant>,
}

impl RecoveryState {
    /// Create a fresh RecoveryState with no failures recorded.
    pub fn new() -> Self {
        Self {
            consecutive_failures: 0,
            last_attempt: None,
        }
    }
}

impl Default for RecoveryState {
    fn default() -> Self {
        Self::new()
    }
}

/// Stateless configuration for exponential-backoff recovery.
///
/// The strategy itself carries no mutable state; all tracking lives in
/// `RecoveryState` which is owned by the calling thread.
#[derive(Debug, Clone)]
pub struct RecoveryStrategy {
    /// First backoff delay in milliseconds.
    pub initial_backoff_ms: u64,
    /// Upper bound on the backoff delay in milliseconds.
    pub max_backoff_ms: u64,
    /// Maximum number of consecutive failures before giving up.
    pub max_retries: u32,
}

impl RecoveryStrategy {
    /// Create a new RecoveryStrategy with explicit parameters.
    pub fn new(initial_backoff_ms: u64, max_backoff_ms: u64, max_retries: u32) -> Self {
        Self {
            initial_backoff_ms,
            max_backoff_ms,
            max_retries,
        }
    }

    /// Returns `true` if another attempt should be made given the current state.
    ///
    /// Returns `false` once `consecutive_failures >= max_retries`.
    pub fn should_attempt(&self, state: &RecoveryState) -> bool {
        state.consecutive_failures < self.max_retries
    }

    /// Computes the next backoff delay in milliseconds using exponential scaling.
    ///
    /// Formula: `initial_backoff_ms * 2^consecutive_failures`, capped at
    /// `max_backoff_ms`.  Uses saturating arithmetic to avoid overflow.
    pub fn next_backoff_ms(&self, state: &RecoveryState) -> u64 {
        let exponent = state.consecutive_failures as u32;
        // 2^exponent, capped — checked_shl returns None on overflow.
        let multiplier: u64 = 1_u64.checked_shl(exponent).unwrap_or(u64::MAX);
        let backoff = self.initial_backoff_ms.saturating_mul(multiplier);
        backoff.min(self.max_backoff_ms)
    }

    /// Records one failure: increments the counter and timestamps the attempt.
    pub fn record_failure(&self, state: &mut RecoveryState) {
        state.consecutive_failures = state.consecutive_failures.saturating_add(1);
        state.last_attempt = Some(Instant::now());
    }

    /// Records a success: resets the consecutive failure counter.
    ///
    /// `last_attempt` is intentionally left as-is because the caller may want
    /// to know when the last operation occurred.
    pub fn record_success(&self, state: &mut RecoveryState) {
        state.consecutive_failures = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_strategy() -> RecoveryStrategy {
        RecoveryStrategy::new(100, 3200, 5)
    }

    // ──────────────────────────────────────────────────────────────────────
    // 1. should_attempt returns true on a fresh state
    // ──────────────────────────────────────────────────────────────────────

    #[test]
    fn should_attempt_returns_true_initially() {
        let strategy = default_strategy();
        let state = RecoveryState::new();
        assert!(
            strategy.should_attempt(&state),
            "fresh state should be allowed to attempt"
        );
    }

    // ──────────────────────────────────────────────────────────────────────
    // 2. should_attempt returns false when max_retries exceeded
    // ──────────────────────────────────────────────────────────────────────

    #[test]
    fn should_attempt_returns_false_when_max_retries_exceeded() {
        let strategy = RecoveryStrategy::new(100, 6400, 3);
        let mut state = RecoveryState::new();

        // Record exactly max_retries failures.
        for _ in 0..3 {
            strategy.record_failure(&mut state);
        }

        assert_eq!(state.consecutive_failures, 3);
        assert!(
            !strategy.should_attempt(&state),
            "should not attempt after max_retries failures"
        );
    }

    // ──────────────────────────────────────────────────────────────────────
    // 3. Exponential backoff doubles each retry
    // ──────────────────────────────────────────────────────────────────────

    #[test]
    fn exponential_backoff_doubles_each_retry() {
        let strategy = RecoveryStrategy::new(100, u64::MAX, 10);
        let mut state = RecoveryState::new();

        // 0 failures → first backoff = initial * 2^0 = 100
        let first = strategy.next_backoff_ms(&state);
        assert_eq!(first, 100, "first backoff should equal initial_backoff_ms");

        strategy.record_failure(&mut state);
        let second = strategy.next_backoff_ms(&state);
        assert_eq!(second, 200, "second backoff should be 2× initial");

        strategy.record_failure(&mut state);
        let third = strategy.next_backoff_ms(&state);
        assert_eq!(third, 400, "third backoff should be 4× initial");

        strategy.record_failure(&mut state);
        let fourth = strategy.next_backoff_ms(&state);
        assert_eq!(fourth, 800, "fourth backoff should be 8× initial");
    }

    // ──────────────────────────────────────────────────────────────────────
    // 4. Backoff is capped at max_backoff_ms
    // ──────────────────────────────────────────────────────────────────────

    #[test]
    fn backoff_capped_at_max_backoff_ms() {
        let cap = 500_u64;
        let strategy = RecoveryStrategy::new(100, cap, 100);
        let mut state = RecoveryState::new();

        // Drive the failure count high enough that the raw exponential would
        // exceed the cap by a large margin.
        for _ in 0..20 {
            strategy.record_failure(&mut state);
        }

        let backoff = strategy.next_backoff_ms(&state);
        assert_eq!(backoff, cap, "backoff must never exceed max_backoff_ms");
    }

    // ──────────────────────────────────────────────────────────────────────
    // 5. record_success resets consecutive_failures
    // ──────────────────────────────────────────────────────────────────────

    #[test]
    fn record_success_resets_consecutive_failures() {
        let strategy = default_strategy();
        let mut state = RecoveryState::new();

        strategy.record_failure(&mut state);
        strategy.record_failure(&mut state);
        strategy.record_failure(&mut state);
        assert_eq!(state.consecutive_failures, 3);

        strategy.record_success(&mut state);
        assert_eq!(
            state.consecutive_failures, 0,
            "record_success must reset consecutive_failures to 0"
        );
        assert!(
            strategy.should_attempt(&state),
            "should be allowed to attempt again after success reset"
        );
    }

    // ──────────────────────────────────────────────────────────────────────
    // Extra: record_failure sets last_attempt
    // ──────────────────────────────────────────────────────────────────────

    #[test]
    fn record_failure_sets_last_attempt() {
        let strategy = default_strategy();
        let mut state = RecoveryState::new();

        assert!(state.last_attempt.is_none(), "last_attempt starts as None");
        strategy.record_failure(&mut state);
        assert!(
            state.last_attempt.is_some(),
            "last_attempt should be set after record_failure"
        );
    }

    // ──────────────────────────────────────────────────────────────────────
    // Extra: saturating overflow safety (extreme failure count)
    // ──────────────────────────────────────────────────────────────────────

    #[test]
    fn backoff_does_not_overflow_at_extreme_failure_count() {
        let strategy = RecoveryStrategy::new(1, 9999, u32::MAX);
        let mut state = RecoveryState::new();

        // Set consecutive_failures to a value that would cause 2^n to overflow u64.
        state.consecutive_failures = 70;
        let backoff = strategy.next_backoff_ms(&state);
        assert_eq!(
            backoff, 9999,
            "backoff must be capped even with extreme failure count"
        );
    }
}
