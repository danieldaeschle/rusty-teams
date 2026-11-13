# Islands UI specification

Source of truth for the app chrome and look. Feature work describes what appears on a surface; this file owns shell layout, allowed surfaces, spacing, radii, type, and colour roles.

Referenced from `GUARDRAILS.md` (Look may differ from Teams; content and behaviour must match Teams).

Code anchors:

- Shell: `crates/app/src/views/shell.rs` (`islands`, `Render for AppShell`)
- Layout constants: `crates/app-ui-kit/src/layout.rs`
- Theme apply + accessors: `crates/app-base/src/theme.rs`
- Palette roles: `crates/app-base/src/theme/schemes.rs`

This document codifies the current look. It is not a redesign brief.

## Shell

Vertical stack in the main window:

1. Title bar
2. Optional connection banner
3. One horizontal row of islands

Island row (left → right):

| Slot | Content | Notes |
|------|---------|--------|
| Rail | Navigation rail | Always present; selector `shell/islands/rail` |
| Sidebar | Chat / team list | Optional; width `SIDEBAR_WIDTH` (300) |
| Main | Exactly one primary island | Conversation, Calendar, Calls, Manage team, or Quick view |

Modals and peeks render above the shell (settings, shortcuts, forward, status, agent start, calendar event peek, profile card, and similar). An active call or meeting uses a dedicated call window; the main window stays free for chat. See `GUARDRAILS.md` and ADR 0001.

Do not invent a second primary island beside the main slot, or a new chrome band outside title bar / banner / island row, without updating this file in the same change.

## Allowed surfaces

| Surface | Use |
|---------|-----|
| Island | Primary chrome panels in the shell island row (rail, sidebar, main) |
| Dialog | Modal overlays (settings, shortcuts, peeks that block the shell, …) |
| Popover / menu | Flyouts and context menus (`flyout_surface` / nested flyout) |
| Call window | Dedicated call / meeting window and its stage / toolbar |
| Toast | Transient notices |

A new surface type is allowed only when a feature change names it and this file is extended in the same change.

Elevation helpers: `ElevatedSurface` in `crates/app-base/src/theme.rs` (`flyout_surface`, `nested_flyout_surface`, `dialog_surface`, `toast_surface`). Prefer those over ad-hoc shadows.

## Measure

Canonical values live in `crates/app-ui-kit/src/layout.rs` and theme apply. Do not introduce new spacing or radius literals in views.

| Token | Value | Role |
|-------|------:|------|
| `ISLAND_GAP` | 6 | Gap between islands; outer padding of the island row |
| Island / card radius | 10 | Primary panels and cards (`theme.radius_lg`) |
| Control radius | 6 | Buttons, nav items, compact chips |
| Body text | 14 px | Default UI text size |
| Font family | Google Sans Flex | Product UI; mono Cascadia Code / Consolas when installed |
| `SIDEBAR_WIDTH` | 300 | Sidebar island width |
| `RAIL_ITEM_WIDTH` | 48 | Rail item hit target |
| `RAIL_WIDTH` | 60 | `RAIL_ITEM_WIDTH + 2 * ISLAND_GAP` |
| `ROW_PADDING` | 20 | Horizontal padding for conversation / list rows |
| `COMFY_RUNWAY_WIDTH` | 1060 | Comfortable message runway width |

Gaps between islands use `ISLAND_GAP` only. New screens reuse these constants; they do not invent parallel numbers.

## Colour

Use palette roles from `Palette` in `crates/app-base/src/theme/schemes.rs`, exposed as `theme::…` helpers or GPUI theme tokens after `theme::apply`. Views must not hard-code raw hex for product chrome.

Core roles (non-exhaustive; prefer an existing role over inventing a hex):

| Role | Typical use |
|------|-------------|
| `background` | Window / shell behind islands |
| `surface` | Island fill (sidebar, …) |
| `surface_raised` | Raised panels / secondary fills |
| `row_hover` | List row hover |
| `popup` / `popup_hover` | Flyouts and dialogs |
| `popup_nested` / `popup_nested_hover` | Nested menus |
| `border` / `border_strong` | Dividers and strong edges |
| `foreground`, `text_strong`, `text_soft`, `text_muted`, `text_faint` | Text hierarchy |
| `accent`, `accent_text`, `accent_soft`, `accent_strong`, `on_accent` | Primary actions and links |
| `bubble_own` / `bubble_other` | Message bubbles |
| `green`, `red`, `amber`, … | Status semantics already defined on the palette |

Shades: Dark (default), Light, High contrast. Schemes: Rusty Teams and Teams (`SCHEMES` in `schemes.rs`). Changing a hex belongs in the palette / scheme tables, not in a view.

## Content vs look

| Owned by | Examples |
|----------|----------|
| This file | Island shell, gaps, radii, type, colour roles, which surface to use |
| `GUARDRAILS.md` | Element order, icons, Teams strings, click paths, account writes |
| Feature changes / ADRs | Observable behaviour of a feature on those surfaces |

Feature work describes content and behaviour. It does not introduce new colours, gaps, or radii. Look changes update this file (and the shared constants) first.

## Known radius drift

Some views still use local radii other than 6 or 10 (for example 3, 4, 8, 12). That is debt:

- Do not add new occurrences of those values.
- When touching a file that uses them, move the control or island toward 6 or 10 as appropriate.
- Do not run a repo-wide visual rewrite solely to clear this debt in one pass.
