# Guardrails: where Rusty Teams matches Teams and where it may differ

Rules for every change, by people and by agents. If a rule here and a habit elsewhere disagree, this file wins.

## Must match Teams

| Area | Rule |
|---|---|
| Content of a screen | Same elements, same order, same states, same meaning of icons as the Teams web client. |
| Texts | Teams strings from the bundle, in the account's Teams language. Own features get own texts in the same tone. |
| Behaviour | Click paths, shortcuts, defaults, when toasts and badges appear: as in Teams. Extras only additive, never replacing a Teams behaviour. |
| Data in the account | Same endpoints and payloads for every write. Settings Teams knows are saved to the Teams account where Teams saves them, never only locally. |
| What others see | Message formats, reactions, presence, call signaling: indistinguishable from Teams. |

## May differ

| Area | Allowed |
|---|---|
| Look | Colors, spacing, islands, radii, theme (`docs/design/islands/SPEC.md`). |
| Data source | Microsoft Graph first, it is the official API and the path for a future registered client. Results and behaviour must still equal Teams. |
| Graph gaps | Where Graph loses data Teams has (e.g. reaction skin tone), take only the missing part from the Teams service (chatsvc etc.). |
| Requests | Extra or different requests (prefetch, caching, polling) when the result in the account is the same. |
| Telemetry | Leave out Teams telemetry and analytics calls. |
| Local bugs | Fix Teams bugs that are purely local (rendering, scroll, focus). Anything touching server data or other people stays as Teams does it. |
| Performance | Faster, less memory, offline reading from the cache. |
| Platform | OS-native toasts, tray, title bar and sounds. |
| Call windows | Daniel-approved (2026-10-17): a call stays in the main window with the sidebar, Teams opens its own meeting window. A window share leaves the main window as it is (side by side), Teams minimizes it. Only our own window behaviour, nothing others see changes. |
| Own features | MCP, agents, session inbox: allowed when nobody else can tell. Nothing sent automatically, no own message formats. |
| Own settings | Settings Teams does not know stay in the local DB, never in Teams account properties. |

## Never

- UI for a Teams feature we do not build (Copilot, Loop, breakout rooms): no button, no placeholder, no dead click.
- Own properties or message formats on the server.

## Internal endpoints

Every non-Graph endpoint (chatsvc, Substrate, calling, presence service, ...) lives behind its own clearly named module and is listed in `docs/research/internal-endpoints.md` with what it is used for and whether Graph could replace it.

## Testing with the real account

| Allowed without asking | Ask first |
|---|---|
| Anything only Daniel sees: `48:notes`, Echo bot, Meet now alone, own settings | Messages, reactions or calls in chats with other people |
| Short presence or status changes, reset afterwards | |

Clean up after every test: delete test messages, restore settings and status.

## Before building

1. Find how Teams does it: `docs/research/*.md`, then the live bundle.
2. Write findings back to `docs/research/<topic>.md`.
3. If a change has to break a "must match" rule, name the deviation in the commit body.
