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

Records `0001` to `0008` are the decisions already shipped. `0001` follows Daniel's call-window note in `GUARDRAILS.md`.
