# Roadmap

Sorted by priority. IDs `F*` and `V*` come from [docs/review/round-1.md](docs/review/round-1.md).

| Prio | Meaning |
|---|---|
| P1 | Blocks daily use |
| P2 | Heavy users miss it weekly |
| P3 | Nice to have |

## Open

| Prio | Feature | State | ID |
|---|---|---|---|
| P1 | Message hover toolbar: react, edit, delete, Up-arrow edits last own message | Backend done, no UI | F2 |
| P1 | New chat: 1:1 and group with title | Backend done, no UI | F5 |
| P1 | Attach file, paste or drag an image, upload progress | Missing | F9 |
| P1 | Channel: new post with subject, post cards | Backend takes a subject, UI sends none | F16, V1 |
| P1 | Built-in login on Linux, no manual Chrome with a debug port | Missing | - |
| P2 | Emoji picker for reactions (search, recent, skin tone) | Missing, colon codes exist | F10 |
| P2 | Chat triage menu: mark unread, mute, hide, leave | Missing, menu has pin, move, mark read | F13 |
| P2 | Muted and meeting chat sections | Missing | F14 |
| P2 | Keyboard model: Ctrl+1..9, Alt+Up/Down, Esc closes thread | Missing, only Ctrl+K and Alt+R | F19 |
| P2 | Formatting shortcuts: Ctrl+B, Ctrl+I, code block | Missing | F11 |
| P2 | Typing indicator | Event arrives, ignored | F12 |
| P2 | Outbox survives restart, retry state | Pending sends live in memory only | F21 |
| P2 | Set own status (Available, Busy, DND), status message | Missing | F20 |
| P2 | Save and pin messages, saved list | Missing | F17 |
| P2 | Forward message, copy link to message | Missing | F18 |
| P2 | Link previews | Missing | F15 |
| P2 | UI polish: empty chat state, contrast of faint text, switcher with avatar and type, narrow window | Open from review round 1 | V2, V6, V7, V8 |
| P3 | Schedule send | Missing | F22 |
| P3 | Notes chat `48:notes`, GIF search, praise | Missing | F23 |
| P3 | Light theme, follow system theme, UI scale | Missing, dark only | F24 |

## Done

| Feature | ID |
|---|---|
| Chats and channels, Teams pin order, chat folders, hidden teams | - |
| Pin a chat, move a chat to a folder | - |
| Live updates over the Trouter socket | - |
| Send, quote-reply (Alt+R), channel threads | F3 |
| @mention autocomplete in the composer | F4 |
| Emoji by `:` code, English codes and German aliases | F10 (partly) |
| Inline images, file cards, Adaptive Cards as text | F1 |
| Read receipts, presence dot, mark read on open | F12 (partly) |
| Unread jump with "New" divider | F7 |
| Local FTS5 search and Ctrl+K switcher | F8 |
| Load older messages on scroll up | - |
| Edited and deleted markers, reactions shown | - |
| Dark theme, instant start from the SQLite cache | - |
| Windows login through embedded WebView2 | - |
| Self-update from an update folder | - |
| Desktop notifications on Windows: toast, sound, taskbar badge, tray, settings | F6, F25 |
| Mentions inline in text, dated old channel posts | - |
| Selectable message text, last known presence at start | - |

## Out of scope

| Feature | Why |
|---|---|
| Calls, meetings | No supported way outside the official client |
| Tabs, apps | No host contract |
| Server-side search | Local index is faster |
