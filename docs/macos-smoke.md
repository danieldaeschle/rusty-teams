# macOS smoke checklist

Run on a real Mac (`.app` from `scripts/build-macos.sh` or a release zip). Unit tests on the `macos-arm64` Forgejo runner cover picker frames and browser discovery; this list is the live gate for a daily-driver RC.

## Sign-in

1. No Google Chrome installed (or rename it) — only Helium / Edge / Brave / Vivaldi under `/Applications`.
2. Cold start without `--endpoint`: managed browser opens, splash shows a connecting phase, Microsoft login completes.
3. Quit and start again: sidebar paints from cache without a spinner wall; browser stays headless.
4. Second Dock click focuses the existing window (single instance).

## Calls and screen share

1. Echo / test call: mic and speaker work; hang up cleans up.
2. Start screen share: ScreenCaptureKit picker stays open until you pick or cancel (no portal-unavailable flash).
3. Pick a display or window: share banner shows; stop share leaves the call up.
4. Cancel the picker: no share, call stays connected, no error toast.
5. Prefer a second person for 1:1 ring / accept when available (ROADMAP still Partial without that).
6. Camera on, no blur, standout, auto framing, brightness, or soft focus, and no share, for one minute. The call report (`host load`, every two seconds) names the threads that used CPU, says those effects are off, and states thermal state `nominal`, `fair`, `serious`, or `critical`. The next capture change follows the hottest thread in that report. This minute is the gate; it is not a substitute for guessing a GPU path.

## Desktop integration

1. Menu-bar tray icon appears; Open / presence / Quit work.
2. Settings → General → Close to tray: close hides/reopens via the tray (new window path like Linux).
3. Incoming chat still uses in-app toasts on this branch (Notification Center is a follow-up after the Linux desktop-toast path).

## Packaging

1. Ad hoc signed local build (no `MACOS_CODESIGN_IDENTITY`): TCC prompts for mic / camera / screen when first used. Each rebuild changes the code hash, so macOS may ask again; `tccutil reset ScreenCapture org.rusty-teams.teams` (or `Camera` / `Microphone`) clears a stale entry.
2. Signed + notarized build (when `MACOS_CODESIGN_IDENTITY` / `MACOS_NOTARY_PROFILE` are set): Gatekeeper opens without a right-click override.

## Report

Note OS version, browser used for sign-in, which checklist rows passed, and anything skipped. No real names, chats, ids or tokens in tickets or screenshots — use `--demo` for UI shots.
