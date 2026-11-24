## 1. Notice action

- [x] 1.1 Mark a Google load failure as needing sign-in when the token cannot be refreshed or the client is missing.
- [x] 1.2 The notice shows Sign in for that failure and Retry otherwise.
- [x] 1.3 Sign in starts the existing Google connect flow.

## 2. Replace the same account

- [x] 2.1 A finished connect for an email that is already connected replaces that account and keeps its id.

## 3. Verify

- [x] 3.1 `cargo test -p app-state google_sign_in`
- [x] 3.2 `cargo test -p store connect_google_replaces`
- [x] 3.3 `openspec validate google-calendar-signin`
