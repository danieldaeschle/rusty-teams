//! Sign-in progress for the managed Chrome window (Linux/macOS).
//!
//! The session transport waits on CDP while Chrome shows Microsoft login.
//! This state drives splash/banner copy so the user is not stuck on a generic
//! «Signing in» line with no pointer to the browser window.

use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum BrowserSignInState {
    #[default]
    Idle,
    /// Chrome is starting or the login window is being opened.
    Opening,
    /// Visible managed Chrome is waiting for the user to finish Microsoft login.
    AwaitingSignIn,
    /// Login succeeded; Chrome is moving headless / session is finishing.
    Finishing,
    /// Chrome missing or launch failed.
    Failed(String),
}

fn slot() -> &'static Mutex<BrowserSignInState> {
    static STATE: OnceLock<Mutex<BrowserSignInState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(BrowserSignInState::Idle))
}

pub fn current() -> BrowserSignInState {
    slot().lock().map(|guard| guard.clone()).unwrap_or_default()
}

pub fn set(state: BrowserSignInState) {
    if let Ok(mut guard) = slot().lock() {
        *guard = state;
    }
}

#[cfg(test)]
mod tests {
    use super::{BrowserSignInState, current, set};

    #[test]
    fn round_trips_through_the_slot() {
        set(BrowserSignInState::Opening);
        assert_eq!(current(), BrowserSignInState::Opening);
        set(BrowserSignInState::AwaitingSignIn);
        assert_eq!(current(), BrowserSignInState::AwaitingSignIn);
        set(BrowserSignInState::Idle);
        assert_eq!(current(), BrowserSignInState::Idle);
    }
}
