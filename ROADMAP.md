# Roadmap

Feature parity with the Microsoft Teams desktop client.

**Parity: 82 %** (71 done, 1 partial, 15 missing of 87). Partial counts half.

## By area

| Area | Parity | Done | Partial | Missing |
|---|---|---|---|---|
| [Sign-in and app](#sign-in-and-app) | 92 % | 5 | 1 | 0 |
| [Chat list](#chat-list) | 90 % | 9 | 0 | 1 |
| [Reading](#reading) | 89 % | 17 | 0 | 2 |
| [Writing](#writing) | 100 % | 14 | 0 | 0 |
| [Message actions](#message-actions) | 100 % | 5 | 0 | 0 |
| [Channels](#channels) | 57 % | 4 | 0 | 3 |
| [Notifications](#notifications) | 88 % | 7 | 0 | 1 |
| [Search and navigation](#search-and-navigation) | 80 % | 4 | 0 | 1 |
| [Presence and people](#presence-and-people) | 100 % | 4 | 0 | 0 |
| [Look and settings](#look-and-settings) | 50 % | 2 | 0 | 2 |
| [Calls and meetings](#calls-and-meetings) | 0 % | 0 | 0 | 5 |

## Next

| # | Feature | Note |
|---|---|---|
| 1 | Sign in on Linux without Chrome on a debug port |  |
| 2 | Keyboard: Ctrl+1..9, Alt+Up/Down |  |

## Sign-in and app

| Feature | State | Note |
|---|---|---|
| Sign in on Windows through embedded WebView2, single sign-on | Done |  |
| Start from the local cache, no spinner | Done |  |
| Self-update | Done |  |
| Tray icon, close to tray | Done |  |
| Sign in on Linux without starting Chrome by hand | Partial | Works with a Chrome on a debug port |
| Single instance, second start brings the window to front | Done |  |

## Chat list

| Feature | State | Note |
|---|---|---|
| Chats and channels in one list | Done |  |
| Pinned chats, synced with Teams | Done |  |
| Chat folders (custom sections), move by menu or drag | Done |  |
| Hidden teams | Done |  |
| Unread bold, unread count, mention marker | Done |  |
| New chat: 1:1 and group with title | Done |  |
| Mark as unread | Done | Keeps the open chat unread until you switch |
| Mute chat | Done | Synced with Teams, muted rows stay in place with a bell |
| Hide chat, leave chat | Done | Hide has Undo; leave only for group chats, with a confirmation |
| Muted and Meeting chat sections | Missing | Teams rollout Aug-Sep 2026 |

## Reading

| Feature | State | Note |
|---|---|---|
| Text formatting: bold, italic, strike, underline, headings, links | Done |  |
| Code: inline pill, block with language, copy and syntax colors | Done |  |
| Lists: nested, numbered with start | Done |  |
| Tables, rules, block quotes, highlight and text color | Done |  |
| Superscript, subscript, font size | Done | Real size and baseline shift; Teams sizes xx-small 9px, x-large 24px |
| Quotes and replies | Done |  |
| Reactions shown | Done |  |
| Edited and deleted markers | Done |  |
| Inline images | Done |  |
| File cards | Done |  |
| Load older messages on scroll | Done |  |
| Unread jump with New divider | Done |  |
| Read receipts | Done |  |
| Select and copy message text | Done |  |
| Adaptive Cards | Done | Schema 1.6 plus Teams extras: all elements incl. Table, CodeBlock, Badge, Icon, Rating, Carousel, Media and charts; all inputs with date and time pickers, live validation and Data.Query typeahead; all actions incl. overflow menu; Universal Actions auto refresh. URL task dialogs run in a hosted window on Windows, in the browser on Linux. Not yet: message-level carousel layout, chart tooltips |
| Typing indicator | Done | Shows who types as avatars above the composer (names on hover) and in the list preview; sends your own typing |
| Link previews | Done | One card per message from the chat service |
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
| Emoji picker | Done | For reactions and in the composer |
| Formatting toolbar, Ctrl+B and Ctrl+I | Done | Bar over a selection; Ctrl+U, Ctrl+Shift+X/C, Ctrl+K link; lists, quote, code block; superscript, subscript (Ctrl+Shift+= / Ctrl+=) and Small/Large shown at real size while typing |
| Attach a file, paste or drag an image | Done | Images sit in the text at the cursor as a large preview and send at that spot; files via OneDrive or the channel's Files |
| GIFs and stickers | Done | Show and send; composer picker with Emoji, GIF and Sticker tabs; GIF search needs the org to allow it |
| Schedule send | Done | Server-side, Teams delivers. Text and formatting only for now |
| Unsent messages survive a restart | Done | Sends are saved before they go out and resent on start without duplicates; failed ones keep Retry and Delete. Drafts are kept per chat and show as "Draft:" in the list |

## Message actions

| Feature | State | Note |
|---|---|---|
| Forward a message | Done | Dialog with chat and channel search, optional comment, "Forwarded" header on received forwards |
| Copy link to a message | Done | Teams deep link to the clipboard |
| Save a message, saved list | Done | Bookmark mark on saved messages, Saved panel next to the bell, synced with Teams |
| Pin a message in a chat | Done | Banner under the chat header, several pins cycle, unpin from the banner. Chats only |
| Mark as unread from a message | Done | Unread from the chosen message on |

## Channels

| Feature | State | Note |
|---|---|---|
| Teams and channels tree | Done |  |
| Posts feed, open a conversation, reply | Done | Newest activity on top, inline reply in the card |
| New post with subject | Done | Subject is stored with the message |
| Post cards like Teams | Done | One card per post: root and the last 3 replies |
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
| Muted chats stay quiet | Done | Toast, sound and taskbar badge only for @mentions |
| Notifications on Linux | Missing |  |

## Search and navigation

| Feature | State | Note |
|---|---|---|
| Ctrl+K switcher over chats, channels, people | Done |  |
| Full-text search over cached messages | Done |  |
| Jump to a message from search | Done |  |
| Esc closes the open conversation or editor | Done | Back to the channel keeps the feed position |
| Keyboard: Ctrl+1..9, Alt+Up/Down | Missing |  |

## Presence and people

| Feature | State | Note |
|---|---|---|
| Presence dots (live push) | Done |  |
| Last known presence at start | Done |  |
| Set own status and status message | Done | Avatar menu like Teams: 6 states with duration, reset, status message with clear-after and "show when people message me", work location Office or Remote for today |
| Profile card | Done | Click a name, avatar or mention: presence, status message or out of office, work location, local time, contact, manager and direct reports; Chat, Email, Copy email |

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
