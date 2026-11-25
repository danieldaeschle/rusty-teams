# AGENTS.md

Rusty Teams: a native Microsoft Teams client in Rust (GPUI). It must behave like the Teams web client.

## Read first

- `GUARDRAILS.md`: where the app must match Teams and where it may differ. It wins over everything else.
- `CONTRIBUTING.md` before you start: scope, when to open an issue first, PR rules.

## Commands

| Task | Command |
|---|---|
| Run with demo data | `cargo run -p app -- --demo` |
| Format | `cargo fmt`. Never `cargo fmt --all`. It rewrites `vendor/`. |
| Test | `cargo test -p <package> <module>`: only the changed module. The package is `name` in the crate's `Cargo.toml` (`crates/core` is `teams-core`). |
| UI check | `cargo test -p app layout_guard`, then `cargo build -p app && scripts/headless-shot.sh "$(mktemp --suffix=.png)"`. Linux only, needs Xvfb, xdotool and ImageMagick. The script header lists the input options. |

Code under `cfg(windows)` is not compiled by tests on Linux or macOS. CI builds it on Windows. If you change it and cannot build it, say so in the PR.

## Rules

- No data from a real Teams account in code, tests, logs or screenshots. Use `--demo`.
- Testing on a real account follows the table in `GUARDRAILS.md`: nothing to other people without the account owner's yes, clean up afterwards.
- Teams behaviour you copy needs a source. `docs/research/` is local and may be missing. Name the source in the PR (Teams docs, a screenshot, the web client). If you have none, ask in the issue.
- `vendor/` holds our patched forks. Change it only when the fix belongs there, and say so in the commit body.
- Do not commit planning or process folders (OpenSpec, task lists, notes). The PR description carries the why.

## Before the PR leaves draft

1. Commit, run `git fetch origin`, and get at least two independent reviews of `git diff origin/main...HEAD`.
2. A reviewer must not have written the change. Use a fresh agent session, a subagent, or a review command such as `/code-review` or `/review`. On Copilot, a Copilot code review on the draft PR counts as one.
3. Fix or answer every valid finding. In the PR's Review section, write one line per review (what ran it), then one line per finding: fixed, or not fixed and why.
4. Check the PR against the table in CONTRIBUTING.md, "When a PR is ready for review". The PR gate check enforces it.
5. Commit and PR title: `type(scope): message`. Type is one of `feat fix perf refactor docs test chore ci build`, the scope is lowercase, no emoji. Write it from the user's view. It becomes the release note line.
