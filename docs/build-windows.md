# Build the Windows exe (no admin, no Visual Studio)

Cross-compile from WSL with `cargo-xwin`. Verified 2026-10-07 on `spikes/gpui-list` (gpui-kit 0.7.1, gpui-pre-windows 0.3.8).

## Commands

| Step | Command |
|---|---|
| Build spike | `scripts/build-windows.sh` |
| Build app | `scripts/build-windows.sh crates/app teams` (arg 2 = binary name) |
| Deploy app | `scripts/deploy-windows.sh`: copies the exe to `%LOCALAPPDATA%\Programs\ms-teams-linux\update\`. The running app swaps itself when idle, or on click in the status bar |
| Output | `<crate>/target/x86_64-pc-windows-msvc/release/<name>.exe` (`CARGO_TARGET_DIR` overrides, default `<crate>/target`) |
| Copy to Windows | `cp <exe> /mnt/c/Users/$USER/Desktop/` (never run from `\\wsl.localhost`) |
| Run | double-click, or `cmd.exe /c start "" C:\path\app.exe` |
| Screenshot (CI/agent) | `scripts/window-shot.ps1` (PrintWindow flag `2`). `CopyFromScreen` returns black |

## What the script does

| Problem | Fix |
|---|---|
| No MSVC / Windows SDK | `cargo xwin` downloads CRT + SDK, `XWIN_ACCEPT_LICENSE=1` |
| `gpui-pre-windows/build.rs` skips shaders when the host is not Windows | Patched copy in `target/patched/`, injected with `--config patch.crates-io` |
| Needs `fxc.exe` (Windows SDK, needs admin) | `GPUI_FXC_PATH=scripts/fxc-wsl.sh`: runs `d3dcompiler_47.dll` (ships with Windows) via PowerShell |
| `gpui` build.rs needs `llvm-rc`, bundled sqlite needs `llvm-lib` | Symlink `llvm-rc-<ver>`, `llvm-lib-<ver>` as plain names on PATH |

## One-time setup

| Need | Install |
|---|---|
| Rust + target | `rustup target add x86_64-pc-windows-msvc` (script does it) |
| `cargo-xwin` | `cargo install cargo-xwin --locked` (script does it) |
| `clang`, `llvm-rc`, `python3` | `sudo apt install clang llvm` (already present) |
| Windows side | Nothing. No Rust, no VS, no admin |

## Facts

| Item | Value |
|---|---|
| Cold build | 3 min 04 s (xwin download of CRT + SDK included, cached in `~/.cache/cargo-xwin`) |
| Exe size | 23 MB (release) |
| Cargo feature | None needed. `gpui` default `windows-manifest` works with `llvm-rc` |
| Shaders | Compiled at build time (release only). Debug builds read `shaders.hlsl` from the build path at runtime: do not ship debug |
| Fonts | Windows system fonts. Bold renders (unlike WSLg) |
| Renderer | DirectX 11 |

## Fails if

| Symptom | Cause |
|---|---|
| `Failed to find fxc.exe` | `GPUI_FXC_PATH` unset: run through the script |
| `NotAttempted("llvm-rc")` | No `llvm-rc` installed |
| `failed to find tool "llvm-lib"` | No `llvm-lib` installed (`sudo apt install llvm`) |
| Black screenshot | Wrong capture API, window is fine |
| Powershell cannot find the shim | WSL interop disabled |

## UI font (Google Sans Flex, OFL 1.1)

| Item | Value |
|---|---|
| Source | `github.com/google/fonts` -> `ofl/googlesansflex/GoogleSansFlex[GRAD,ROND,opsz,slnt,wdth,wght].ttf` + `OFL.txt` |
| Embedded | `crates/app/assets/fonts/GoogleSansFlex-{Regular,SemiBold,Bold,Italic}.ttf`, loaded in `theme::load_fonts` |
| Why static | The variable font is not used: static instances switch weights reliably under DirectWrite |
| Regenerate | `python3 scripts/font-instances.py <variable.ttf> crates/app/assets/fonts` (needs `pip install fonttools`) |
| Pinned axes | wght 400/600/700, wdth 100, opsz 14, GRAD 0, ROND 0, slnt 0 (Italic: -10) |
| Not used | Plain "Google Sans" is not openly licensed |
| Code blocks | Cascadia Code, else Consolas |

## Frame log (perf debugging)

| Item | Value |
|---|---|
| Enable | Set `RUSTY_TEAMS_FRAME_LOG=<file>` before starting the exe (`--demo` for tests) |
| Output | First line: GPU adapter. Then once per second: draws, longest gap, prepaint-to-paint mean/max |
| Idle baseline | 2 draws/s (caret blink) |
| Window drag | Every WM_MOVE forces a full uncached redraw in gpui-pre. Cost per frame must stay flat in chat count |
