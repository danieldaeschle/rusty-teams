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

Do this without being asked, before you mark the PR ready for review. Leave it a draft until every item below is in the description. The PR template's HTML comments do not count: a section that is only a comment is empty. The PR gate check enforces the same list (CONTRIBUTING.md, "When a PR is ready for review"). `gate: no visual` and `gate: skip` are for maintainers only.

The body has `## Summary`, `## AI model`, `## Tests` and `## Review`, plus `## Problem` when item 4 applies and `## Screenshot` when item 6 applies.

1. Commit, run `git fetch origin`, and get at least two independent reviews of `git diff origin/main...HEAD`.
2. A reviewer must not have written the change. Use a fresh agent session, a subagent, or a review command such as `/code-review` or `/review`. On Copilot, a Copilot code review on the draft PR counts as one.
3. Fix or answer every valid finding. In `## Review`, write at least two non-empty lines: one line per review (what ran it), then one line per finding: fixed, or not fixed and why.
4. No `Closes` / `Fixes` / `Resolves` plus `#n` or an issue URL on this repo: write `## Problem` (at least 80 characters). Say what is wrong or missing, when a user hits it (steps or when/then), and what should happen instead. Write `Closes #123` only when that issue exists.
5. A `feat` PR needs an issue labelled `status: accepted`. A maintainer sets that label. Do not add it yourself. Without it, do not mark the PR ready: say so.
6. A change under `crates/app/src/views/`, `settings/`, `components/`, `widgets/`, a `crates/app/src/*_view.rs` file, or `locales/` needs a screenshot in the body, taken with `cargo run -p app -- --demo` or `scripts/headless-shot.sh`. No visible change: tick `- [x] No visible change` and say why. Never a real account.
7. In `## Tests`, say what ran and what did not. If the diff touches Windows-only code, say whether you built it on Windows.
8. No planning folders in the diff (`openspec/`, `.specify/`, `.kiro/`). A `vendor/` change says why in the summary. At most 5 open PRs, drafts included.
9. Title: `type(scope): message`. Type is one of `feat fix perf refactor docs test chore ci build`, the scope is lowercase, no emoji. Write it from the user's view. It becomes the release note line.
10. Run `python3 .github/scripts/pr_gate.py --dry-run <PR number>`. It must print "ready for review". Only then mark the PR ready.
