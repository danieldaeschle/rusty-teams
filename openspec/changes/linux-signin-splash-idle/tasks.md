## 1. Sign-in state after launch

- [x] 1.1 When a managed-browser launch succeeds and the window is not a visible login window, publish idle so the opening splash ends.
- [x] 1.2 Keep awaiting sign-in when the login window is visible, and failed when the launch fails.
- [x] 1.3 Unit-test the headless-success and visible-login outcomes.

## 2. Verify

- [x] 2.1 `cargo test -p app --bin teams sign_in_after_browser_ready`
- [x] 2.2 `openspec validate linux-signin-splash-idle`
