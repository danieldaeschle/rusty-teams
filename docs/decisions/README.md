# Decisions

Short notes for choices that are easy to re-argue once more people (and agents) touch the repo. `ROADMAP.md` says what we are building. `GUARDRAILS.md` says where we must match Teams. A decision record says why we picked one reasonable path over another.

Write one when a future contributor would otherwise have to guess. Skip one for a bugfix, a refactor, or anything `GUARDRAILS.md` already settles.

## How to add one

1. Copy `0000-template.md` to `NNNN-short-title.md`. Numbers start at `0001` and only go up.
2. Fill the frontmatter: `status`, `date` (YYYY-MM-DD). `status` starts as `proposed`. It becomes `accepted` when the pull request that introduces it is merged, unless the discussion says otherwise.
3. One decision per file. If a later change replaces it, add a new ADR and set the old one to `status: superseded` and `superseded-by: NNNN-new-title.md`. Leave `superseded-by` empty or out otherwise. Do not rewrite history in place.

Statuses: `proposed`, `accepted`, `superseded`.

## Records

| | Decision |
|---|---|
| [0001](0001-dedicated-call-window.md) | Calls and meetings open in their own window |
| [0002](0002-optional-calendars-beside-outlook.md) | Optional calendars beside Outlook |
| [0003](0003-sign-in-through-a-browser-session.md) | Sign-in through a browser session |
| [0004](0004-match-teams-behavior-use-the-os-mechanism.md) | Match Teams behavior, use the OS mechanism |
| [0005](0005-screen-share-uses-the-system-picker.md) | Screen share uses the system picker |
| [0006](0006-mic-path-matches-browser-teams.md) | The mic path matches Teams in the browser |
| [0007](0007-local-agent-clis-beside-copilot.md) | Local agent CLIs sit beside Copilot |
| [0008](0008-product-ui-locale-en-de.md) | Product UI is English and German |
| [0009](0009-bundled-google-oauth-client.md) | Bundled Google OAuth Desktop client |
| [0010](0010-preferred-external-browser.md) | Preferred browser for links and Google OAuth |
| [0011](0011-calendar-event-peek.md) | Calendar events open as a Teams-like peek |
| [0012](0012-linux-close-to-tray-closes-the-window.md) | On Linux, close to tray closes the main window |
| [0013](0013-local-diagnostic-data-and-upstream-issues.md) | Local diagnostic data layout and upstream issues |
| [0014](0014-linux-installs-and-updates-itself.md) | The Linux binary installs and updates itself |
| [0015](0015-linux-desktop-toasts-use-freedesktop-notifications.md) | Linux desktop toasts use freedesktop Notifications |
| [0016](0016-calendar-week-start-and-time-scale-stay-local.md) | Calendar work hours go to the mailbox, week start and time scale stay local |
| [0017](0017-guest-orgs-switch-by-restart.md) | Guest orgs, one org per window, switch by restart |
| [0018](0018-regional-format-is-separate-from-the-app-language.md) | Regional format is separate from the app language |
| [0019](0019-caldav-app-password-and-keyring.md) | CalDAV with app password and OS keyring |
| [0020](0020-macos-tray-and-close-to-tray.md) | macOS tray and close-to-tray match Linux |
| [0021](0021-linux-packages.md) | Linux releases also ship an AppImage, a deb and an rpm |
| [0022](0022-linux-parks-the-idle-teams-tab.md) | Linux parks the idle Teams tab in a Chromium browser |
| [0023](0023-people-calendars-through-free-busy.md) | People's calendars come from free/busy, the list stays local |

Records `0001` to `0021` are the decisions already shipped. `0001` follows Daniel's call-window note in `GUARDRAILS.md`.
