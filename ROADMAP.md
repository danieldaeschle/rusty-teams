# Roadmap

Feature parity with the Microsoft Teams desktop client.

**Parity: 59 %** (48 done, 4 partial, 33 missing of 85). Partial counts half.

## By area

| Area | Parity | Done | Partial | Missing |
|---|---|---|---|---|
| [Sign-in and app](#sign-in-and-app) | 75 % | 4 | 1 | 1 |
| [Chat list](#chat-list) | 60 % | 6 | 0 | 4 |
| [Reading](#reading) | 71 % | 13 | 1 | 5 |
| [Writing](#writing) | 75 % | 10 | 1 | 3 |
| [Message actions](#message-actions) | 0 % | 0 | 0 | 4 |
| [Channels](#channels) | 36 % | 2 | 1 | 4 |
| [Notifications](#notifications) | 75 % | 6 | 0 | 2 |
| [Search and navigation](#search-and-navigation) | 75 % | 3 | 0 | 1 |
| [Presence and people](#presence-and-people) | 50 % | 2 | 0 | 2 |
| [Look and settings](#look-and-settings) | 50 % | 2 | 0 | 2 |
| [Calls and meetings](#calls-and-meetings) | 0 % | 0 | 0 | 5 |

## Next

| # | Feature | Note |
|---|---|---|
| 1 | Channel: new post with subject, post cards |  |
| 2 | Sign in on Linux without Chrome on a debug port |  |
| 3 | Chat menu: mark as unread, mute, hide, leave | Unlocks quiet muted chats and the Muted section |
| 4 | Emoji picker in the composer | Reaction picker exists |
| 5 | Keyboard: Ctrl+1..9, Alt+Up/Down |  |
| 6 | Typing indicator |  |
| 7 | Unsent messages survive a restart |  |

## Sign-in and app

| Feature | State | Note |
|---|---|---|
| Sign in on Windows through embedded WebView2, single sign-on | Done |  |
| Start from the local cache, no spinner | Done |  |
| Self-update | Done |  |
| Tray icon, close to tray | Done |  |
| Sign in on Linux without starting Chrome by hand | Partial | Works with a Chrome on a debug port |
| Single instance, second start brings the window to front | Missing |  |

## Chat list

| Feature | State | Note |
|---|---|---|
| Chats and channels in one list | Done |  |
| Pinned chats, synced with Teams | Done |  |
| Chat folders (custom sections), move by menu or drag | Done |  |
| Hidden teams | Done |  |
| Unread bold, unread count, mention marker | Done |  |
| New chat: 1:1 and group with title | Done |  |
| Mark as unread | Missing |  |
| Mute chat | Missing |  |
| Hide chat, leave chat | Missing |  |
| Muted and Meeting chat sections | Missing | Teams rollout Aug-Sep 2026 |

## Reading

| Feature | State | Note |
|---|---|---|
| Text formatting: bold, italic, strike, underline, headings, links | Done |  |
| Code: inline pill, block with language, copy and syntax colors | Done |  |
| Lists: nested, numbered with start | Done |  |
| Tables, rules, block quotes, highlight and text color | Done |  |
| Superscript, subscript, font size | Missing | Shown as plain text |
| Quotes and replies | Done |  |
| Reactions shown | Done |  |
| Edited and deleted markers | Done |  |
| Inline images | Done |  |
| File cards | Done |  |
| Load older messages on scroll | Done |  |
| Unread jump with New divider | Done |  |
| Read receipts | Done |  |
| Select and copy message text | Done |  |
| Adaptive Cards | Partial | Rendered as cards; only open-URL buttons work |
| Typing indicator | Missing | Event arrives, ignored |
| Link previews | Missing |  |
| Loop components | Missing |  |
| Translate a message | Missing |  |

## Writing

| Feature | State | Note |
|---|---|---|
| Send, multi-line, Enter sends | Done |  |
| Markdown while typing: bold, code, lists, links | Done | Converts as you type or paste; Backspace or Ctrl+Z brings the raw text back |
| Quote-reply (Alt+R) | Done |  |
| @mentions with people search | Done |  |
| Emoji by colon code, English and German aliases | Done |  |
| Edit own message | Done | From the menu or Up arrow in an empty composer |
| Delete own message | Done |  |
| React to a message | Done | Hover bar, emoji picker, click a chip to toggle |
| Emoji picker | Partial | For reactions; composer still uses colon codes |
| Formatting toolbar, Ctrl+B and Ctrl+I | Done | Bar over a selection; Ctrl+U, Ctrl+Shift+X/C, Ctrl+K link; lists, quote, code block |
| Attach a file, paste or drag an image | Done | Images sit in the text at the cursor as a large preview and send at that spot; files via OneDrive or the channel's Files |
| GIFs and stickers | Missing |  |
| Schedule send | Missing |  |
| Unsent messages survive a restart | Missing | Pending sends live in memory only |

## Message actions

| Feature | State | Note |
|---|---|---|
| Forward a message | Missing |  |
| Copy link to a message | Missing |  |
| Save a message, saved list | Missing |  |
| Pin a message in a chat | Missing |  |

## Channels

| Feature | State | Note |
|---|---|---|
| Teams and channels tree | Done |  |
| Thread list, open a thread, reply | Done |  |
| New post with subject | Partial | Backend takes a subject, UI sends none |
| Post cards like Teams | Missing |  |
| Follow a channel, per-channel notifications | Missing |  |
| Channel tabs: tab bar, website and app tabs | Missing | Plan: each tab's Teams Web page in a WebView2 |
| Files tab | Missing | Plan: native, the channel's SharePoint folder via Graph |

## Notifications

| Feature | State | Note |
|---|---|---|
| Desktop toast, click opens the chat at the message | Done |  |
| Reply and Mark as read in the toast | Done |  |
| Sound, red taskbar badge, taskbar flash on new messages | Done |  |
| Quiet during Do not disturb, Focus Assist, calls | Done |  |
| Mentions-only mode, preview off | Done |  |
| Stack of 3, queue, Hide all, fade-out | Done |  |
| Muted chats stay quiet | Missing | Needs Mute chat |
| Notifications on Linux | Missing |  |

## Search and navigation

| Feature | State | Note |
|---|---|---|
| Ctrl+K switcher over chats, channels, people | Done |  |
| Full-text search over cached messages | Done |  |
| Jump to a message from search | Done |  |
| Keyboard: Ctrl+1..9, Alt+Up/Down, Esc closes thread | Missing |  |

## Presence and people

| Feature | State | Note |
|---|---|---|
| Presence dots (live push) | Done |  |
| Last known presence at start | Done |  |
| Set own status and status message | Missing |  |
| Profile card | Missing |  |

## Look and settings

| Feature | State | Note |
|---|---|---|
| Dark theme | Done |  |
| Notification settings | Done |  |
| Light theme, follow system theme | Missing |  |
| UI scale | Missing |  |

## Calls and meetings

| Feature | State | Note |
|---|---|---|
| Incoming call: ring, accept, decline | Missing | Plan: Teams Web call UI in its own WebView2 window |
| 1:1 and group calls | Missing | |
| Join a meeting from a chat | Missing | |
| Screen sharing | Missing | |
| In a call shown in presence | Missing | |

## Agents

After parity. Not counted in the parity score.

| Feature | State | Note |
|---|---|---|
| Local API in the app | Missing | Tokens, chat access and agent state stay in the app. CLI and MCP are thin clients |
| CLI `rt`: watch, read, search, claim, propose, release | Missing | First. `rt watch --once` blocks until the next event, so any agent can wait on it |
| MCP server on the same API | Missing | After the CLI. `wait_for_event(timeout)` long-polls, progress pings keep it alive |
| Agent access per chat, off by default | Missing | Other chats are invisible to agents, search included |
| Proposals: card above the composer, send, edit, discard | Missing | Agents never send. A proposal is stale once you answered first |
| Agent status per chat: working, proposal ready, declined | Missing | Claim with heartbeat, clears itself when the agent stops |
| Agent sees you type or send and can abort | Missing | `user_typing` and `user_sent` events in the watch stream |

## Out of scope

| Feature | Why |
|---|---|
| Personal apps in the app bar | Outside chat and channels |
| Server-side search | The local index answers faster |
