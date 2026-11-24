## 1. Open the window

- [x] 1.1 When Teams needs a sign-in and the managed window is hidden, open it once.
- [x] 1.2 Do not open a second window while one is visible, and do not reopen automatically after the user closes it.
- [x] 1.3 Unit-test the hidden, visible, in-flight, and forced-reopen decisions.

## 2. Banner action

- [x] 2.1 The sign-in banner shows Sign in. Choosing it opens the managed window when none is visible.
- [x] 2.2 The live-updates banner still shows Retry.

## 3. Verify

- [x] 3.1 `cargo test -p app --bin teams login_launch_decision`
- [x] 3.2 `cargo test -p app-state connection_banner`
- [x] 3.3 `openspec validate linux-signin-window`
