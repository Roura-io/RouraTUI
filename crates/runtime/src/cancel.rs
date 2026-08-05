//! Cooperative cancellation signal for an in-flight turn.
//!
//! Deliberately separate from `HookAbortSignal` (`hooks.rs`): hooks are a
//! narrower, pre/post-tool-use lifecycle concern with their own tests, and
//! reusing that type here would risk today's hook-abort behavior. This
//! signal instead spans a whole turn -- the conversation loop, the
//! permission-prompt wait, and any in-flight bash child process -- and,
//! unlike the hook signal, is reset between turns so the same instance can
//! be reused for the life of an interactive session.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Debug, Clone, Default)]
pub struct TurnCancelSignal {
    cancelled: Arc<AtomicBool>,
}

impl TurnCancelSignal {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Clears the flag so the same signal can be reused for the next turn.
    pub fn reset(&self) {
        self.cancelled.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_not_cancelled() {
        assert!(!TurnCancelSignal::new().is_cancelled());
    }

    #[test]
    fn cancel_is_observed_through_clones() {
        let signal = TurnCancelSignal::new();
        let clone = signal.clone();
        clone.cancel();
        assert!(signal.is_cancelled());
    }

    #[test]
    fn reset_clears_a_cancelled_signal() {
        let signal = TurnCancelSignal::new();
        signal.cancel();
        signal.reset();
        assert!(!signal.is_cancelled());
    }
}
