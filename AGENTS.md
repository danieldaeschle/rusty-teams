# Agent notes — rusty-teams

## Fork vs Upstream

Pflicht-Workflow (Cursor Rule `.cursor/rules/fork-workflow.mdc`, `alwaysApply`):

- **`fork`** (`d0dg3r/rusty-teams`) = entwickeln, pushen, eigene Releases
- **`origin`** (`danieldaeschle/rusty-teams`) = Upstream; nur fetch + gezielte kleine PRs
- Nie Alltagskomits nach `origin` pushen
- Upstream-PRs: frischer Branch von `origin/main`, eine Scheibe Thema
- Fork-Releases: Tags wie `v0.3.0-joe.1` auf `fork`, nicht Upstream-`vX.Y.Z` spiegeln wenn der Inhalt abweicht
