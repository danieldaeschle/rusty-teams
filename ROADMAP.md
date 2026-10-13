# Roadmap

Feature parity with the Microsoft Teams desktop client.

**Parity: 80 %** (84 done, 28 partial, 10 missing of 122). Partial counts half.

## By area

| Area | Parity | Done | Partial | Missing |
|---|---|---|---|---|
| [Sign-in and app](#sign-in-and-app) | 92 % | 5 | 1 | 0 |
| [Chat list](#chat-list) | 100 % | 10 | 0 | 0 |
| [Reading](#reading) | 95 % | 18 | 0 | 1 |
| [Writing](#writing) | 100 % | 14 | 0 | 0 |
| [Message actions](#message-actions) | 100 % | 5 | 0 | 0 |
| [Channels](#channels) | 100 % | 7 | 0 | 0 |
| [Notifications](#notifications) | 89 % | 8 | 0 | 1 |
| [Search and navigation](#search-and-navigation) | 100 % | 5 | 0 | 0 |
| [Presence and people](#presence-and-people) | 100 % | 4 | 0 | 0 |
| [Look and settings](#look-and-settings) | 50 % | 2 | 0 | 2 |
| [Calls and meetings](#calls-and-meetings) | 50 % | 6 | 27 | 6 |

## Next

| # | Feature | Note |
|---|---|---|
| 1 | Sign in on Linux without Chrome on a debug port |  |

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
| Muted and Meeting chat sections | Done | Own sections like Teams, 5 chats plus See more, switches in the avatar menu under Chat list (synced with Teams settings), collapse synced with Teams |

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
| Translate a message | Done | Menu item on every message; offer line under foreign messages (Translate, Never translate language); See original toggle; auto-translate; settings under the avatar menu > Translation, synced with Teams |

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
| Follow a channel, per-channel notifications | Done | Right-click > Notifications: banner and activity, activity only, off, include thread replies; synced with Teams; bell icons in the sidebar |
| Channel tabs: tab bar, website and app tabs | Done | Posts, Shared, then the channel's tabs with overflow; website tabs embedded on Windows (WebView2), browser on Linux; app tabs open in Teams web |
| Files tab | Done | "Shared" like Teams: In library (folders, upload, drag and drop, new folder, open in SharePoint) and In messages (files and links from posts) |

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
| Mute notifications during calls and meetings | Done | Settings > Notifications switch, default off; message toasts and sounds stay away while a call is active, call rings still show |
| Notifications on Linux | Missing |  |

## Search and navigation

| Feature | State | Note |
|---|---|---|
| Ctrl+K switcher over chats, channels, people | Done |  |
| Full-text search over cached messages | Done |  |
| Jump to a message from search | Done |  |
| Esc closes the open conversation or editor | Done | Back to the channel keeps the feed position |
| Keyboard shortcuts like Teams | Done | Ctrl+1 Activity, 2 Chats, 3 Channels, 4 Saved; Alt+Up/Down previous and next chat or channel in the visible list, also while typing; calls Ctrl+Shift+A accept, D decline, H hang up, O camera, E share menu, K raise hand, Y lobby, M mute, J join from a meeting reminder, Space held with Ctrl to talk while muted, F11 full screen, Alt+Shift+A / V start an audio / video call in the open chat; Ctrl+. lists all shortcuts. Ctrl+5..9 not used |

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
| Test call (Echo bot) | Done | Native libwebrtc audio, call view, mini window, mute, devices |
| Call test report | Done | After every call a pseudonymized JSON and Markdown report lands in the app data folder `call-reports` (newest 30): timeline, SDP summary, stats every 2 s as min / max / last, features used with result, notices, end reason. People, e-mails, phone numbers, ids, URL queries, IPs, ICE credentials, fingerprints and chat text never reach the file. Toast "Test report saved" with Open folder; Settings > Devices switch, default on. Demo calls write one too |
| Incoming call: ring, accept, decline | Partial | Ring toast, ring tone, missed call in Activity, registrar endpoint, attach + accept. Built from the Teams code, not yet rung by a second person |
| 1:1 and group calls | Partial | Phone button, Calling and Ringing tiles, end notices, direct to mixer renegotiation. Not yet tried with a second person |
| Join a meeting from a chat | Partial | Join button from the live meeting state, joins muted, roster tiles, lobby, Leave and End meeting. Audio only. Verified with a meeting of one; not yet with other participants |
| Video and screen sharing | Partial | Send and receive over libwebrtc H264, source requests through `applyChannelParameters`, stage, strip, self view, share banner. Sent frames acknowledged by the mixer; receive only checked by a local loopback, not with a second person |
| Meeting extras | Partial | Share menu switch "Include computer sound" (remembered, mixed into the one audio track with the mic, own playback excluded on Windows 10 build 20348+), raise hand with queue badge, lower hand and Lower all hands for organizers, reactions on the sender's tile for 3 s, meeting chat in a 320 px side panel with unread badge. Checked with synthetic sources and the demo only; not yet live in a meeting or with a second person |
| Organizer controls | Partial | Amber lobby banner with View (Admit, Deny per person) and Admit all, tile menu (right-click or "..." on hover): Pin for me, Spotlight for everyone, Mute, Lower hand, Remove from meeting with a confirm dialog, "..." controls menu with Mute all. Request bodies built from the Teams code and unit tested; demo checked, not yet live with a second person |
| People panel, roles, lock, invite | Partial | People button before chat opens a 320 px panel (closes the chat): "People (N)" with Invite and "...", lobby with Admit / Deny / Admit all, raised hands in queue order, groups Organizers / Presenters / Attendees (flat "In this call" in 1:1 and group calls), row "..." with Pin, Spotlight, Mute, Disable / Allow mic and camera, Make presenter / attendee, Remove. "..." menu: Mute all, Lock / Unlock meeting ("Locked" chip, banner for organizers), mic and camera off for attendees, Add people (1:1). Invite adds a person to the running meeting or group call; in a 1:1 call it creates a group chat and escalates. Request bodies from the Teams code and unit tested; demo checked, nothing sent to the live service |
| Meeting options | Partial | Organizer "..." > Meeting options: lobby bypass, who can present, attendee mic and camera, meeting chat (On / In-meeting only / Off), reactions, record automatically. Reads and writes the `meetings/v1/options` service from the captured shapes, one request per changed option, errors inline. Not yet read or written live |
| Live captions | Partial | "..." > Turn on live captions: recorder bot joins, start command with a skype token, captions arrive on the SCTP data channel (data id 3), 2-line overlay with bold speaker that fades after 4 s. "Captions settings": spoken language (`setLanguage`), size S/M/L, position bottom/top (size and position remembered). "Show transcript": 320 px side panel with the whole call (speaker, mm:ss, text), search, Jump to latest. Message framing and parsing from the Teams code; `setLanguage` body from capture, not sent live; not yet seen with real speech |
| Backgrounds | Partial | Camera menu Background: None / Blur / Microsoft's default images / "Add image..." (remembered). The image list and thumbnails are fetched at runtime into the app data folder, nothing of Microsoft's ships with the app; the picture is cover-scaled behind the same segmentation mask (checked with a synthetic mask, demo with generated pictures, the CDN download only through `meet_now --background`). On-device MediaPipe selfie segmentation through tract (Apache-2.0 model, 11 ms per 640x360 frame in a release build), edge feathering, self view shows the result. Checked with a still photo and synthetic frames; not yet on a real camera |
| Camera effects | Partial | Camera menu "Effects" and Settings > Devices: Mirror my video (self view and camera test only, default on), Auto framing, Adjust brightness, Soft focus (Low / Medium / High in Settings). Framing, brightness and soft focus run on the send path, so others see them; one segmentation mask per frame is shared with the background. Crop planner, gamma and blend are unit tested; not yet checked on a real camera |
| Video calls (1:1 and incoming) | Partial | Camera button in 1:1 calls renegotiates a video line on the same peer connection (modalities Audio and Video), incoming video ring toast with Decline / Audio / Video, accept with video answers the video line sending, accept with audio receive-only. Offer and answer built from the Teams code and unit tested at SDP level; needs a second person |
| Recording | Partial | Organizer "..." > Record and transcribe: Start recording, Record audio only, Start transcription only; Stop recording / Stop transcription / Stop both (confirm) through the recorder bot; header chip red "Recording" / "Recording audio" or blue "Transcribing" for everyone from the meeting state, consent notice that keeps mic and camera off until OK. Bodies unit tested; video start and stop run live through `meet_now --record` (audio only, transcription only and the stop variants are from capture, not run live), consent needs a tenant with the policy on and a second person |
| Meeting recap tab | Partial | Recap in meeting chats with a recording: occurrence picker, recording card (duration, Audio only, expiry amber under 7 days), Play in the browser, Download with save dialog and percent, organizer Delete with confirm, searchable read-only transcript with .vtt and .docx download, attendance (muted note when the tenant has reports off), files shared in the chat. Parsing and expiry unit tested, demo checked; the Graph and SharePoint calls are not yet run against a real recording |
| Hold, resume, transfer | Partial | 1:1 "..." > Hold renegotiates every line inactive, "On hold" view with Resume, "You're on hold" when the other side holds, Transfer... person picker with Transfer now (blind) and Consult first (second call, first held, Transfer now). Needs a second person; redirect and park not built |
| Noise suppression, music mode | Partial | Mic menu and Settings > Devices: Auto (device track with WebRTC echo cancellation, AGC, NS) or Off, Music mode keeps echo cancellation only. No Low/High, the APM has on/off flags only. Off and Music mode run a cpal mic through our own APM; checked with synthetic sources only, not with a real mic in a call |
| Push to talk | Partial | Hold Ctrl+Space while muted to talk, release to mute; main window focused, not inside text fields; hint chip on the stage. Unit tested, not tried on the Windows build |
| Breakout rooms (participant) | Partial | Move message from the participant update or the replacement invite: notice "Moving you to <room>", leave and join the room, "Return to main meeting" in the header, a closed room goes back to the main meeting. Parsing unit tested; needs an organizer who moves people |
| Call health and encryption | Partial | Header shows signal bars and a shield; click opens "Call health" with loss, round trip, jitter, bitrate, "Secured by Teams encryption", DTLS-SRTP cipher and media path (direct or via Microsoft media server). Unstable network banner with "Turn off incoming video", overloaded computer banner with "Turn off effects". Thresholds unit tested; not yet seen on a bad live network |
| View and layout | Partial | "View" button between share and "...": Gallery (up to 3x3 of 16:9 tiles, "+N" tile with a pin list; visible tiles only swap after 2 s of speech, one swap per 5 s), Speaker (stage plus filmstrip of up to 6), Focus on content (only while someone shares), Hide people without video with a count chip, Hide me / Show me, Chat bubbles, Full screen (F11, controls hide after 3 s). Choice, Hide people without video and Chat bubbles are remembered. Checked with the demo gallery scene only; not yet in a large live meeting |
| Gallery video slots | Partial | Meetings offer 9 receive lines (mids 2-10, share on 11), so a 3x3 gallery shows up to 9 live videos. The mixer accepts the offer (self-only Meet now, live). Only visible tiles get a slot: "+N" people and people hidden by "Hide people without video" are not requested. Speaker stage asks for 3600 macroblocks (max-fs), filmstrip, strip and big-gallery tiles for 920. 1:1 calls unchanged. Slot plan and offer layout unit tested; not yet seen with nine people on a live mixer |
| Sharing tools | Partial | Received share: Ctrl+wheel zoom 1x-4x around the pointer, drag to pan, Alt+Shift+Plus / Minus / 0, "250%" chip resets; draggable divider (strip 160-400 px, remembered); "Pop out" opens the content in its own window, closing it returns it. Share menu: "Optimize for motion" (maintain framerate, 30 fps; off = maintain resolution, 15 fps) and "Show me over my content" (camera cut-out bottom right, 22 % wide, rounded; skipped without a mask). Linux: red border windows ignore the mouse on X11. Checked in the demo and unit tests; not yet live with a second person |
| Whiteboard | Partial | Stage shows "<name> is sharing a whiteboard" with Open in browser, "..." > Whiteboard opens the meeting's board. State parsing unit tested; the share callback link is registered on join but not yet seen live |
| Call history | Done | Calls tab (Ctrl+5) from the Teams call log: avatar, name, Missed call in red, Outgoing / Incoming / Meeting with duration, time like the chat list. Click calls back or opens the meeting chat; right-click Call back and Open chat. Refreshes on tab open and after a call ends. Read live (counts and shapes); no realtime update |
| Join by link or meeting ID | Done | "Join with ID" dialog, "Paste a meeting link", Join chip next to Teams meeting links in the composer and messages. IDs and /meet/ links resolve through a roster-only conversation like Teams web; verified live with a self-created scheduled meeting (same meeting chat found) |
| In a call shown in presence | Done | "In a call", "In a meeting", "Presenting" from Teams activity on profile cards and chat headers; no notification sounds during a native call |
| Schedule a meeting | Partial | Calls tab "New meeting": title, people chips, date, start, end (next half hour, 30 min), Repeat (does not repeat, daily, weekdays, weekly), details, Teams meeting always on. Graph `POST /me/events` with `isOnlineMeeting` and `teamsForBusiness`, local wall time in the own mailbox time zone (series keep their time across DST, weekday from the local date). Body building unit tested; never run against a real calendar |
| Answer meeting invitations | Partial | Upcoming rows of invited meetings show Accepted / Tentative / Declined / Respond as a menu with "Send a response" (default on); own meetings say Organizer. Graph `accept`, `tentativelyAccept`, `decline`, falls back to the iCalUId lookup when the Teams id is not a Graph id. Not run against a real invitation |
| Meeting start reminder | Partial | Toast 5 min before and at the start of an Upcoming meeting: title, time, Join, Dismiss; Ctrl+Shift+J joins from the latest. Quiet during Do not disturb and while in a call. Driven by the polled calendar; push event 128 not wired (the realtime layer does not receive it) |
| Pre-join screen | Partial | Join from Upcoming, chat, Join with ID, link or reminder opens a screen in the chat pane: camera preview, camera and mic switches, background menu, camera, microphone and speaker selects, "Computer audio" or "Don't use audio" (audio off in both directions), Join now, Cancel. Camera, mic and devices remembered. 1:1 calls and Meet now skip it. Checked in the demo only |
| Narrow windows in a call | Done | Controls that do not fit move into "..." (View, Reactions, Raise hand, People, Chat; mic, camera, share, "..." and Leave always stay). Below 960 px, or when a side panel would leave the call under 720 px, the chat list hides; still too narrow, the panel overlays the call. Layout guard checks 640x480, 1280x800, 1920x1080 |
| Give and take control | Missing | Protocol known (main-channel data channel, action codes 0-11); needs a second person and a security review |
| Breakout rooms (organizer) | Missing | Create, assign, open, close rooms; needs a second person |
| Merge calls | Missing | Needs two live callers |
| Dial-in numbers in meeting info | Missing | Numbers from the invite, Copy join info |
| Hide my name in captions | Missing | Setting not found in the captures |
| End-to-end encrypted 1:1 calls | Missing | Key exchange lives in the native desktop media stack, not in the web bundle; tenant policy is Disabled, so not testable here. The call health popover shows the transport encryption instead |

## Agents

After parity. Not counted in the parity score.

| Feature | State | Note |
|---|---|---|
| Delegate a message to an agent CLI (Claude Code, Copilot, any CLI) from the hover toolbar | Done | Robot button in the hover toolbar, actions and agents in Settings > Agents |
| Run status: chip under the message, Agents rail entry, toast, Open session | Done | Live step and time on the chip, Agents rail popover with running and done runs, toast on finish or failure, Open session resumes the agent session in a terminal |
| Local API in the app | Done | MCP only, no CLI by design. Tokens and chat access stay in the app |
| MCP server on the same API | Done | Read tools (messages with images, reactions, files, quotes, cards, links; `get_image`; `recent_messages`; `search_messages`; `list_meetings` and `get_transcript` when allowed), `wait_for_event` to follow new messages and proposals, and `propose_message` only. Settings > MCP, default port 40615 |
| One-click MCP install | Done | Settings > MCP installs the server into Claude Code, Codex CLI, Copilot CLI, VS Code and Cursor, on Windows and in WSL (needs `networkingMode=mirrored`) |
| Start dialog per run | Done | Pick the action, the agent session (new or resume) and the working directory for each delegated message |
| Runs survive a restart | Done | Interrupted runs resume after the app restarts; WSL "Open session" starts the agent through a login shell |
| Agent permissions in Settings: chats/channels All/Selected/None, propose, calendar, transcripts | Done | Other chats are invisible to agents, search included |
| Proposals: card above the composer, send, edit, discard | Done | Agents never send. A proposal is stale once you answered first |

## Out of scope

| Feature | Why |
|---|---|
| Personal apps in the app bar | Outside chat and channels |
| Server-side search | The local index answers faster |
| Copilot, intelligent recap, AI notes, Facilitator, Interpreter | Needs a Microsoft 365 Copilot licence; left out on purpose |
| Teams Premium features (translated captions, watermark, E2EE meetings, templates, branding) | Needs a Teams Premium licence |
| Phone system: dial pad, park, forwarding, voicemail, delegation, queues | Needs a Teams Phone licence; none on the test account |
| PowerPoint Live, annotation, polls, Q&A, Whiteboard editing, Live Share, meeting apps | Microsoft-hosted web apps; Whiteboard opens in the browser |
| Together mode, speaker coach, avatars | Retired by Microsoft or needs speech analysis |
| Webinars, town halls | Event hosting, not chat or meetings |
