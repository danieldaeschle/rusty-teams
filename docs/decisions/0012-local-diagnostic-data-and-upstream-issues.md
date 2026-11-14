---
status: proposed
date: 2026-11-14
---

# ADR 0012: Local diagnostic data layout and upstream issues

## Context

Users need a place for logs and support packages that works the same way on Linux, macOS, and Windows. Paths were split across call sites with no OS table in the product specs. User-facing bug reports must point at the public upstream tracker, not at a private or Issues-disabled mirror. Opening a bug report also used to pop the file manager beside the browser, which was noisy.

Options on the table: rename the data directory away from `ms-teams-linux`, put diagnostics under roaming `data_dir` on Windows, or keep the local root name and send issues to upstream `danieldaeschle/rusty-teams`.

## Decision

Diagnostic and support artifacts live under `ProjectDirs` **local** data for the product directory name `ms-teams-linux` (legacy name kept on every OS). Layout under that root: `diagnostics.log` (+ `.1` / `.2` ring), `crash.log` (+ `.1`), `call-dumps/`, `call-reports/`, `support-bundles/support-<UTC>/`. **Debug file logging is off by default** and is toggled from Help (store key `diagnostics_enabled`) or forced with `--debug-log` / `RUSTY_TEAMS_DEBUG_LOG=1`; `crash.log` still records panics when they happen. Help shows those absolute paths. Bug reports and support README text open or name only upstream `danieldaeschle/rusty-teams`; the app does not upload and does not open the file manager by itself when filing (a notice may offer Open folder). Store/Roaming migration is not part of this decision.

## Consequences

- Developers and users can find logs and bundles from Help without guessing OS conventions.
- Everyday installs do not grow `diagnostics.log` until the user opts in.
- Existing installs keep working; no rename of `ms-teams-linux`.
- End-user copy never points at a fork or mirror for issues — only upstream.
- The SQLite store may still use a different root on Windows until a later change.
