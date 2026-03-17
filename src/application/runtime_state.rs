use std::sync::atomic::{AtomicBool, Ordering};

/// RuntimeState manages pipeline runtime flags with atomic operations.
/// No Mutex needed — all operations are non-blocking (Relaxed ordering).
pub struct RuntimeState {
    active: AtomicBool,
    mouse_left: AtomicBool,
    mouse_right: AtomicBool,
}

impl RuntimeState {
    /// Create a new RuntimeState with default values:
    /// - active: true
    /// - mouse_left: false
    /// - mouse_right: false
    pub fn new() -> Self {
        RuntimeState {
            active: AtomicBool::new(true),
            mouse_left: AtomicBool::new(false),
            mouse_right: AtomicBool::new(false),
        }
    }

    /// Toggle the active flag (flip active state).
    pub fn toggle(&self) {
        let current = self.active.load(Ordering::Relaxed);
        self.active.store(!current, Ordering::Relaxed);
    }

    /// Check if the pipeline is active.
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    /// Update left mouse button state.
    pub fn update_mouse_left(&self, pressed: bool) {
        self.mouse_left.store(pressed, Ordering::Relaxed);
    }

    /// Update right mouse button state.
    pub fn update_mouse_right(&self, pressed: bool) {
        self.mouse_right.store(pressed, Ordering::Relaxed);
    }

    /// Check if left mouse button is pressed.
    pub fn is_mouse_left_pressed(&self) -> bool {
        self.mouse_left.load(Ordering::Relaxed)
    }

    /// Check if right mouse button is pressed.
    pub fn is_mouse_right_pressed(&self) -> bool {
        self.mouse_right.load(Ordering::Relaxed)
    }
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn new_starts_active() {
        let state = RuntimeState::new();
        assert!(state.is_active(), "RuntimeState should start active");
    }

    #[test]
    fn toggle_flips_active() {
        let state = RuntimeState::new();
        assert!(state.is_active(), "Should start active");

        state.toggle();
        assert!(!state.is_active(), "Should be inactive after first toggle");

        state.toggle();
        assert!(state.is_active(), "Should be active after second toggle");
    }

    #[test]
    fn mouse_state_tracks_updates() {
        let state = RuntimeState::new();

        // Left button
        assert!(
            !state.is_mouse_left_pressed(),
            "Left button should start unpressed"
        );
        state.update_mouse_left(true);
        assert!(
            state.is_mouse_left_pressed(),
            "Left button should be pressed"
        );
        state.update_mouse_left(false);
        assert!(
            !state.is_mouse_left_pressed(),
            "Left button should be unpressed"
        );

        // Right button
        assert!(
            !state.is_mouse_right_pressed(),
            "Right button should start unpressed"
        );
        state.update_mouse_right(true);
        assert!(
            state.is_mouse_right_pressed(),
            "Right button should be pressed"
        );
        state.update_mouse_right(false);
        assert!(
            !state.is_mouse_right_pressed(),
            "Right button should be unpressed"
        );
    }

    #[test]
    fn arc_shared_toggle_visible_across_clones() {
        let state = Arc::new(RuntimeState::new());
        let state_clone = Arc::clone(&state);

        assert!(state.is_active(), "Original should start active");
        assert!(state_clone.is_active(), "Clone should start active");

        // Toggle from original
        state.toggle();
        assert!(
            !state.is_active(),
            "Original should be inactive after toggle"
        );
        assert!(
            !state_clone.is_active(),
            "Clone should see toggle from original"
        );

        // Toggle from clone
        state_clone.toggle();
        assert!(state.is_active(), "Original should see toggle from clone");
        assert!(
            state_clone.is_active(),
            "Clone should be active after toggle"
        );
    }
}
