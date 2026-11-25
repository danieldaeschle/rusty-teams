# Contributing to Rusty Teams

Most of this code is written with AI agents, and that is welcome here. Just say which model did the work.

## Before you start

| Step | What |
|---|---|
| 1 | Read [GUARDRAILS.md](GUARDRAILS.md): where the app must match Teams and where it may differ. It wins over any habit. |
| 2 | Check [ROADMAP.md](ROADMAP.md), mainly "Out of scope". Features listed there are not merged. |
| 3 | Search open pull requests and issues for the same topic. For anything larger than a fix, open an issue first and wait for a yes. |
| 4 | Read the notes under [docs/decisions/](docs/decisions/README.md) for the area you touch. |

## AI use

| Rule | Detail |
|---|---|
| Name the model | Every pull request names the tool and model that wrote the change. It tells the reviewer how closely to look. "None" is a valid answer. |
| Review before the PR | At least two independent reviews of the local diff, each by a reviewer that did not write the change (a fresh agent session or subagent, or a review command). Fix or answer every valid finding and list them in the PR. Details in [AGENTS.md](AGENTS.md#before-the-pr-leaves-draft). |
| Report only what ran | Say what you tested and what you could not run. No "works" without a run. |
| Keep it short | A summary of a few sentences. Long generated descriptions get sent back. |
| No real data | Nothing from a real Teams account in code, tests, logs or screenshots: no names, chats, ids or tokens. Use `--demo`. |

## Pull requests

- One topic per pull request. No unrelated refactors, no formatting of files you did not change.
- The template asks for four things: a summary, the AI model, the reviews that ran, and a screenshot for UI changes (taken with demo data).
- CI runs the tests on Linux and Windows for every pull request.
- Commit and PR title format: `type(scope): message`, with `type` one of `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `chore`, `ci`, `build`, a lowercase scope and no emoji. CI checks the PR title.
- The PR title becomes the squash commit and a line in the release notes (`feat` under New, `fix` under Fixes). Write it as a user would read it: `fix(chat): a sent image no longer blinks`, not `fix(chat): handle download race`.
- Rebase on `main` instead of merging it in. Answer actionable review comments or say that you will not.

## Build and test

Build setup is in the [README](README.md#quick-start), the Windows cross build in [docs/build-windows.md](docs/build-windows.md). The everyday commands (demo run, format, tests, UI check) are in [AGENTS.md](AGENTS.md#commands).

## Code

- Small modules, full variable names (`request_id`, not `rid`).
- Comments only where the code cannot show the reason: ordering, timing, a workaround for outside behaviour.
- Every non-Graph endpoint sits behind its own named module.

## UI

- Divider lines run to both panel edges.
- Paddings are aligned: bottom equals side.
- Soft surfaces, no hard 1 px outlines.

## Testing with a real account

Use your own account or a test tenant and follow the table in [GUARDRAILS.md](GUARDRAILS.md#testing-with-a-real-account). Clean up afterwards.
