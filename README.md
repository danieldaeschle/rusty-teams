# Rusty Teams

A fast native Microsoft Teams chat client written in Rust with [GPUI](https://www.gpui.rs/).
It does chats and channels. No calls, no meetings, no apps.

![Rusty Teams with demo data](docs/screenshots/app-demo.png)

> Unofficial. Not affiliated with or endorsed by Microsoft. It uses the same web APIs as the Teams web app, which can change at any time.

## Features

- Chats and channels, with the Teams pin order, chat folders and hidden teams
- Live updates over the Teams realtime socket, no polling
- Send, edit, delete, react, quote-reply, @mentions
- Emoji by `:` code with English codes and German aliases, `:thumbsup:` and `:)` convert as you type
- Inline images, file cards, Adaptive Cards as text
- Read receipts, presence, unread jump
- Local full-text search and a `Ctrl+K` switcher, served from the cache
- Dark theme, opens instantly from the local cache
- Self-update from an update folder

## Status

| Platform | State |
|---|---|
| Windows | Main target. Login through an embedded WebView2 |
| Linux | Connects to a Chrome you start with `--remote-debugging-port=9222`. Built-in login is planned |

## How it works

Rusty Teams never sees your password and never stores a token.
It signs you in through a real Teams web page and lets that page make the API calls.

```
+--------------------------- Rusty Teams (Rust) ---------------------------+
|  app      GPUI window: sidebar, chat view, composer, Ctrl+K              |
|  core     sync engine, actions, mapping to view models                   |
|  store    SQLite cache + FTS5 search index                               |
|  graph    Microsoft Graph: chats, channels, messages, people, sending    |
|  chatsvc  Teams chat service: realtime, pins, folders, read receipts     |
|  session  runs requests inside the Teams page, one Transport trait       |
+------------------------------------|-------------------------------------+
                                     | DevTools protocol (CDP)
+------------------------------------v-------------------------------------+
|  Hidden browser page on teams.cloud.microsoft, own profile               |
|  webview: WebView2 (Windows)   or   browser: Chrome with a debug port    |
+------------------------------------|-------------------------------------+
                                     | HTTPS with the page's own MSAL tokens
               Graph  |  chat service (ic3)  |  presence  |  Trouter socket
```

### Sign-in and tokens

1. A hidden browser page loads Teams web with its own profile. On Windows this is WebView2, so single sign-on with the Windows account usually just works.
2. If Teams asks for a login, the login window shows itself. After that the page stays parked in the background.
3. Every API call is sent to that page over CDP. A small script (`crates/session/assets/fetch.js`) picks the matching token from the page's MSAL cache, refreshes it when needed, and runs the `fetch`.
4. Only the response goes back to Rust. Tokens never leave the page.

### Data flow

| Step | What happens |
|---|---|
| Start | The UI renders from the SQLite cache right away |
| Sync | `core` fetches chats and channels from Graph with delta queries, pins and folders from the chat service |
| Live | `chatsvc` keeps the Trouter WebSocket open. An event only says "something changed", the content is then fetched fresh |
| Write | Sends, edits and reactions go out through Graph or the chat service, then the cache updates |
| Search | Local FTS5 index over cached messages, no server round trip |

### Crates

| Crate | Job |
|---|---|
| `app` | GPUI desktop app, binary `teams` |
| `core` | Sync engine, actions, people ranking, markdown and card rendering |
| `store` | SQLite cache, migrations, search, image file cache |
| `graph` | Typed Microsoft Graph client with batching and paging |
| `chatsvc` | Teams chat service: Trouter realtime, pins, folders, receipts |
| `session` | `Transport` trait, CDP session, in-page fetch |
| `webview` | Hidden WebView2 host (Windows only) |
| `browser` | Chrome lifecycle and watchdog for the CDP transport |
| `cli` | `teams-probe`, a read-only end-to-end check |

## Build and run

| Task | Command |
|---|---|
| Demo data, no account | `cargo run -p app -- --demo` |
| Against Chrome on a debug port | `cargo run -p app -- --endpoint http://127.0.0.1:9222` |
| Windows exe from WSL/Linux | `scripts/build-windows.sh crates/app teams`, see [docs/build-windows.md](docs/build-windows.md) |
| Tests | `cargo test` |

## Docs

- [docs/research](docs/research) - how Teams does realtime, pins, read receipts and edits
- [docs/design](docs/design) - mockups
- [docs/build-windows.md](docs/build-windows.md) - cross-compiling without admin rights

## License

[MIT](LICENSE)
