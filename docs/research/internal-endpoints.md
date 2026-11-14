# Internal endpoints

Every endpoint Rusty Teams calls that is not `graph.microsoft.com`, grouped by service. Guardrail: each lives behind a clearly named module; `crates/chatsvc/tests/internal_endpoints.rs` fails when a non-Graph host string shows up outside the allowed module paths.

How requests leave the app: Rust never sends Microsoft requests directly. `session::Session` (`crates/session/src/session.rs`) runs `crates/session/assets/fetch.js` inside the signed-in Teams or Outlook page (CDP or WebView2); the page attaches the bearer token from the MSAL cache, so the token never leaves the page. Only CDN images and sounds go through the gpui HTTP client.

Columns: R = read, W = write. "Graph?" = could Graph replace it.

## Sign-in and page hosts

| Host + path | Method | Module | Used for | R/W | Graph? |
|---|---|---|---|---|---|
| `login.microsoftonline.com/{tenant}/oauth2/v2.0/token` | POST | `crates/session/assets/fetch.js` | refresh access token per resource from the MSAL refresh token | W | no - the auth flow itself |
| `teams.cloud.microsoft/` | page load | `crates/browser/src/targets.rs`, `crates/browser/src/launch.rs`, `crates/session/src/app.rs`, `crates/webview/src/host.rs` | Teams web session that holds the MSAL cache | R | no |
| `teams.cloud.microsoft/robots.txt` | page load | `crates/session/src/app.rs` | parked tab | R | no |
| `outlook.office.com/mail/`, `outlook.cloud.microsoft/mail/`, `outlook.cloud.microsoft/owa/favicon.ico` | page load | `crates/browser/src/targets.rs`, `crates/session/src/app.rs`, `crates/webview/src/host.rs` | second token source (Graph `Chat.*` tokens), parked tab | R | no |
| `teams.cloud.microsoft/__rusty/dialog` | page load (served locally) | `crates/app-base/src/task_dialog/sdk.rs`, `crates/webview/src/host_dialog.rs` | task dialog host origin | R | no |
| bot task dialog URL (server data) | page load | `crates/webview/src/host_dialog.rs` | bot task dialogs | R | no - third-party hosts |

Token audiences (not requests): `crates/session/src/scope.rs` - `ic3.teams.office.com`, `presence.teams.microsoft.com`, `outlook.office.com`, `api.spaces.skype.com`; `crates/chatsvc/src/channel_notifications.rs` - `uis.teams.microsoft.com`; chatsvcagg for CSA.

## chatsvc (`teams.cloud.microsoft/api/chatsvc/{region}/v1`)

| Path | Method | Module | Used for | R/W | Graph? |
|---|---|---|---|---|---|
| `/users/ME/conversations?view=msnp24Equivalent&pageSize=200` (+ backwardLink) | GET | `crates/chatsvc/src/conversations.rs` | conversation states: mute, hidden, read horizon, notes hide time (`unpinnedTime`/`historyHiddenTime`) | R | partly - `/me/chats` + `viewpoint` (isMuted, lastMessageReadDateTime); no hidden flag, no msg-id horizon |
| `/users/ME/conversations/{id}?view=msnp24Equivalent` | GET | `crates/chatsvc/src/conversations.rs` (`thread_url`), called from `crates/calling/src/meeting.rs`, `crates/calling/src/signaling.rs` | live meeting state of a thread, Echo bot 1:1 check | R | partly - `/chats/{id}` metadata; no live meeting state |
| `/users/ME/conversations/{id}/properties?name=alerts` | PUT | `crates/chatsvc/src/conversations.rs` | chat alerts on/off | W | partly - `viewpoint.isMuted` (mute only) |
| `/users/ME/conversations/{id}/properties?name=consumptionhorizon` | PUT | `crates/chatsvc/src/conversations.rs` (format in `read_horizon.rs`) | mark read | W | partly - `markChatReadForUser`; no message-id horizon |
| `/users/ME/conversations/{id}/properties?name=consumptionHorizonBookmark` | PUT | `crates/chatsvc/src/conversations.rs` (format in `read_horizon.rs`) | notes chat Mark as unread, cleared on read | W | no - `markChatUnreadForUser` leaves an own last message read |
| `/users/ME/conversations/{id}/properties?name=unpinnedTime`, `?name=historyHiddenTime` | PUT | `crates/chatsvc/src/conversations.rs` (`set_hidden_at`) | notes chat Hide / Undo | W | no - `hideForUser` answers 400 for `48:notes` |
| `/users/ME/conversations/{id}/properties?name=channelNotificationSettings` | PUT | `crates/chatsvc/src/conversations.rs` | channel notification write | W | no |
| `/users/ME/conversations/{channel};messageid={root}/messages` | POST | `crates/chatsvc/src/messages.rs` (`post_message`) | channel quote reply in the Teams blockquote shape + `qtdMsgs` (Graph cannot carry it) | W | no - Graph replies lose the Teams quote |
| `/users/ME/conversations/{chat}/messages` | POST | `crates/chatsvc/src/messages.rs` (`post_message`) | chat quote reply (also `48:notes`) in the Teams blockquote shape + `qtdMsgs`, see `write-path-parity.md` | W | no - Graph beta `replyWithQuote` writes a `messageReference` attachment instead of the Teams quote |
| `/users/ME/conversations/{id}/messages?view=msnp24Equivalent&pageSize=N` | GET | `crates/chatsvc/src/links.rs` | message `properties.links` for link previews | R | no - links property not in Graph |
| `/users/ME/conversations/{id}/messages?view&pageSize` | GET | `crates/chatsvc/src/translate.rs` | message language stamps | R | no |
| `/users/ME/conversations/{thread}/messages?view&pageSize=200` | GET | `crates/chatsvc/src/recap.rs` | audio-only recording detection (videoContent) | R | no |
| `/users/ME/conversations/{id}/messages/epochTimeStamp/{anchor}?direction&...` | GET | `crates/chatsvc/src/window.rs` | message window around an anchor (jump, paging) | R | partly - `/chats/{id}/messages` paging; no anchor or bidirectional window |
| `/users/ME/conversations/{id}/messages?view=msnp24Equivalent&pageSize=200` | GET | `crates/chatsvc/src/thread_activity.rs` | meeting chat system events (`ThreadActivity/MemberJoined`, `ThreadActivity/TopicUpdate`) | R | no - Graph `/chats/{id}/messages` leaves them out for meeting chats |
| `wss://augloop.office.com/` (AugLoop session: init, seed doc, activate `TeamsCopilotComposeRewriteStreamingAnnotation`, token provision, `TeamsCopilotComposeRewriteSignal`) | WebSocket | `crates/chatsvc/src/compose_copilot.rs`, `crates/session/assets/fetch.js` | composer "Write with Copilot": Rewrite, Proofread (`PROOFREAD_SCENARIO`), Adjust (tone/length), Customised (`customPrompt`) | R | no - Graph has no compose rewrite |
| `/users/ME/conversations/{id}/messages` (Control/Typing) | POST | `crates/chatsvc/src/messages.rs` | typing indicator | W | no |
| `/users/ME/conversations/{id}/messages/{messageId}` | PUT | `crates/chatsvc/src/messages.rs` | edit own message | W | yes - PATCH `/chats/{id}/messages/{id}` (edittime not mapped) |
| `/users/ME/conversations/{id}/messages/{messageId}?behavior=softDelete` | DELETE | `crates/chatsvc/src/messages.rs` | delete own message | W | yes - `softDelete` action |
| `/users/ME/conversations/{id}/messages/{messageId}/properties?name=emotions` | PUT, DELETE | `crates/chatsvc/src/messages.rs` | add / remove reaction | W | partly - `setReaction` / `unsetReaction`; no skin tone key, no custom emotion keys |
| `/users/ME/conversations/{source}/messages/forward` | POST | `crates/chatsvc/src/forward.rs` | forward up to 5 messages | W | no - Graph has no forward |
| `/users/ME/conversations/48:notifications` | GET | `crates/chatsvc/src/activity_feed.rs` | Activity bell and Mark all as read horizons (`consumptionhorizon`, `consumptionHorizonBookmark`) | R | no - no activity feed in Graph |
| `/users/ME/conversations/48:notifications/messages?view&pageSize=200&startTime=1` (+ backwardLink) | GET | `crates/chatsvc/src/activity_feed.rs` | Activity feed items (also the channel mention badges, `activity_mentions.rs`), see `activity-feed.md` | R | no - Graph only sends activity notifications, it cannot read the feed |
| `/users/ME/conversations/48:notifications/messages/{id}/properties?name=isread` | PUT | `crates/chatsvc/src/activity_feed.rs` | mark one Activity item read (Teams `markReadStatus`) | W | no |
| `/users/ME/conversations/48:notifications/properties?name=consumptionHorizonBookmark` | PUT | `crates/chatsvc/src/activity_feed.rs` | Activity Mark all as read (`0;{now};0`) | W | no |
| `/users/ME/conversations/48:saved/messages?...` | GET | `crates/chatsvc/src/saved.rs` | saved messages list | R | no |
| `/users/ME/conversations/{id}/rcmetadata/{rootId}` | PUT | `crates/chatsvc/src/saved.rs` | save / unsave message | W | no |
| `/users/ME/conversations/48:calllogs/messages?...` | GET | `crates/chatsvc/src/calllog.rs` | call history | R | no - `callRecords` is app-only |
| `/users/ME/properties` | GET | `crates/chatsvc/src/settings.rs` | user settings bag (sections, team order, skin tone, personal sound) | R | no |
| `/users/ME/properties?name={key}` | PUT | `crates/chatsvc/src/settings.rs`, `crates/chatsvc/src/sound_settings.rs` | write user settings | W | no |
| `/users/ME/drafts?view&pageSize&startTime` | GET | `crates/chatsvc/src/drafts.rs` | scheduled messages | R | no |
| `/users/ME/drafts`, `/users/ME/drafts/{id}` | POST, PUT, DELETE | `crates/chatsvc/src/drafts.rs` | create, reschedule, cancel scheduled message | W | no |
| `/threads/{id}/consumptionhorizons` | GET | `crates/chatsvc/src/receipts.rs` | member read receipts | R | no - no per-member read horizons in Graph |
| `/agents/{botMri}/invoke` (fallback region `de`) | POST | `crates/chatsvc/src/cards.rs` | adaptive card action invoke | W | no - Bot Framework invoke |

## Middle tier MT (`teams.cloud.microsoft/api/mt/{region}`, `teams.microsoft.com/api/mt/...`)

| Path | Method | Module | Used for | R/W | Graph? |
|---|---|---|---|---|---|
| `/v2.1/me/calendars/calendarView?startDate&endDate` | GET | `crates/chatsvc/src/calendar.rs` | Teams calendar with join links | R | yes - `/me/calendarView` (Teams skypeTeamsData not mapped) |
| `/beta/me/calendarEvents/privateMeeting/schedulingService/create` | POST | `crates/chatsvc/src/calendar.rs` | Meet now | W | partly - `/me/onlineMeetings`; no ad-hoc chat unhide |
| `/beta/chats/{id}/apps/chatentitlements` | GET | `crates/chatsvc/src/cards.rs` | apps entitled in a chat | R | partly - `/chats/{id}/installedApps` |
| `/beta/commands/com.microsoft.teamspace.inputextension.giphy/execute?version=1.0` | POST | `crates/chatsvc/src/gifs.rs` | GIF search and trending | R | no |
| `/beta/users/fetchShortProfile?...&includeBots=true` | POST | `crates/chatsvc/src/bot_profiles.rs` | bot names and images | R | no |
| `/beta/users/useraggregatesettings` | POST | `crates/chatsvc/src/language_settings.rs`, `crates/chatsvc/src/sound_settings.rs` | translation, voice and recording policy settings | R | no |
| `/beta/users/languageAccountSettings` | PATCH | `crates/chatsvc/src/language_settings.rs` | translation preferences | W | no |
| `/beta/usersettings/{mri}/ringtonesettings` | POST | `crates/chatsvc/src/sound_settings.rs` | ringtone | W | no |
| `/beta/teams/{teamThreadId}/channels` | POST | `crates/chatsvc/src/team_admin.rs` | create standard channel | W | yes - `POST /teams/{id}/channels`, but the Teams web token has no `Channel.Create` |
| `/beta/teams/{teamThreadId}/channels/{channelId}` | PATCH, DELETE | `crates/chatsvc/src/team_admin.rs` | rename / delete standard channel | W | yes - `PATCH`/`DELETE /teams/{id}/channels/{id}`, no `Channel.ReadWrite.All` / `Channel.Delete.All` in the Teams web token |
| `teams.microsoft.com/api/mt/emea//beta/meetings/v1/options/{tenant}/{organizer}/{thread}/0?monarch=true` | GET | `crates/calling/src/meeting_options.rs` | meeting options read | R | partly - onlineMeeting GET (organizer only) |
| `/beta/meetings/v1/options/{...}/0/0?monarch=true` | POST | `crates/calling/src/meeting_options.rs` | meeting options write (lobby, presenters, chat) | W | partly - onlineMeeting PATCH (organizer only, not live) |
| `/beta/meetings/fluidboard?threadId&organizerId&tenantId&meetingTitle` | GET | `crates/calling/src/whiteboard.rs` | meeting whiteboard URL | R | no |

## Chat service aggregator CSA (`teams.cloud.microsoft/api/csa/{region}/api/v1`)

| Path | Method | Module | Used for | R/W | Graph? |
|---|---|---|---|---|---|
| `/teams/users/me/conversationFolders?...` | GET | `crates/chatsvc/src/pins/chats.rs`, `crates/chatsvc/src/pins/folders.rs` | pinned chats and folder tree | R | no |
| `/teams/users/me/conversationFolders?...` | POST | `crates/chatsvc/src/pins/chats.rs`, `crates/chatsvc/src/pins/folders.rs` | pin, unpin, reorder, create and move folder | W | no |
| `/teams/users/me/conversationFolders/{folderId}?...` | PUT, DELETE | `crates/chatsvc/src/pins/folders.rs` | rename, expand, delete folder | W | no |
| `/teams/users/me/pinnedChannels` | GET, POST, DELETE | `crates/chatsvc/src/pins/channels.rs` | pinned channels | R/W | no |
| `/teams/users/me?isPrefetch=false&enableMembershipSummary=true` | GET | `crates/chatsvc/src/pins/teams.rs` | team and tab layout, sidebar order | R | partly - `/me/joinedTeams` + channels + tabs; no sidebar order |
| `/chats/{id}/pinbar` | GET, POST, DELETE | `crates/chatsvc/src/pins/pinbar.rs` | pinned messages with folders | R/W | partly - `/chats/{id}/pinnedMessages` (flat, no folders) |
| `/chats/{id}/messages/languages/{language}` | POST | `crates/chatsvc/src/translate.rs` | chat message translation | R | no |
| `/teams/{team}/channels/{channel}/{root}/messages/languages/{language}` | POST | `crates/chatsvc/src/translate.rs` | channel message translation | R | no |
| `/translator/languages?locale={ui}` | GET | `crates/chatsvc/src/translate.rs` | supported languages | R | no |

## Notification settings NSS (`teams.cloud.microsoft/api/nss/{region}/v1`)

| Path | Method | Module | Used for | R/W | Graph? |
|---|---|---|---|---|---|
| `/me/notificationSettings/channels/batch` | POST | `crates/chatsvc/src/channel_notifications.rs` | channel notification read (up to 100 ids) | R | no |

## Teams and channels provisioning (`teams.cloud.microsoft/fabric/{region}/templates/api`, token `https://templates.teams.microsoft.com/.default`, header `X-AnchorMailbox: OID:{me}@{tenant}`)

| Path | Method | Module | Used for | R/W | Graph? |
|---|---|---|---|---|---|
| `/teams/{groupId}/channels?forceSync=false` | POST | `crates/chatsvc/src/team_admin.rs` | create private channel | W | yes - `POST /teams/{id}/channels` with `membershipType: private`, no `Channel.Create` in the Teams web token |
| `/teams/{groupId}/channels/{OID:substrateGroupId@tenant}?forceSync=false` | PATCH, DELETE | `crates/chatsvc/src/team_admin.rs` | rename / delete private channel | W | yes - as for standard channels, same missing scopes |

## Presence (`presence.teams.microsoft.com/v1`)

| Path | Method | Module | Used for | R/W | Graph? |
|---|---|---|---|---|---|
| `/presence/getpresence/` | POST | `crates/chatsvc/src/presence/lookup.rs` | presence of many users (650 per call) | R | partly - `/communications/getPresencesByUserId` needs `Presence.Read.All`, which the web app tokens lack |
| `/presence/getpresence/` | POST | `crates/chatsvc/src/presence.rs` | own status (note, out of office, work location) | R | partly - `/me/presence`; no work location, no pinned note |
| `/pubsub/subscriptions/{endpointId}` | POST | `crates/chatsvc/src/presence/lookup.rs` | live presence push via Trouter | W | no - Graph change notifications need a public webhook |
| `/me/forceavailability/` | PUT | `crates/chatsvc/src/presence.rs` | set or clear availability | W | partly - `setUserPreferredPresence` |
| `/me/endpoints/`, `/me/endpoints/{endpointId}` | PUT, DELETE | `crates/chatsvc/src/presence.rs` | register this device as presence endpoint | W | no |
| `/me/reportmyactivity/` | POST | `crates/chatsvc/src/presence.rs` | active / inactive | W | no |
| `/me/publishnote` | PUT | `crates/chatsvc/src/presence.rs` | status note | W | partly - `setStatusMessage`; no "show when messaged" flag |
| `/me/workLocation/` | PUT | `crates/chatsvc/src/presence.rs` | work location | W | no |

## Trouter (push)

| Host + path | Method | Module | Used for | R/W | Graph? |
|---|---|---|---|---|---|
| `teams.cloud.microsoft/api/authsvc/v1.0/authz` | POST | `crates/chatsvc/assets/discover_host.js`, `crates/chatsvc/src/realtime/host.rs` | Trouter host discovery | R | no |
| `teams.cloud.microsoft/registrar/prod/V2/registrations`, `/registrations/{endpointId}` | POST, DELETE | `crates/chatsvc/assets/trouter.js` (run by `crates/chatsvc/src/realtime/runner.rs`) | register / unregister push endpoint | W | no |
| `wss://{host}/v4/c?...` (default `go-eu.trouter.teams.microsoft.com`) | WebSocket | `crates/chatsvc/assets/trouter.js`, `crates/chatsvc/src/realtime/host.rs` | messages, presence, call events | R | no - Graph notifications need a public webhook |
| `{trouterUri}callAgent/{callId}/...` | inbound | `crates/calling/src/trouter_events.rs`, `crates/calling/src/engine.rs` | callback URLs handed to the call controller | R | no |

## Substrate search (`substrate.office.com`)

| Path | Method | Module | Used for | R/W | Graph? |
|---|---|---|---|---|---|
| `/search/api/v2/init?cvid&scenario=powerbar` | GET | `crates/chatsvc/src/substrate_search/mod.rs` | search session | R | no |
| `/search/api/v1/suggestions?...` | GET, POST | `crates/chatsvc/src/substrate_search/mod.rs` | zero-query and typeahead suggestions | R | partly - `/search/query`; no people or chat ranking |
| `/searchservice/api/v2/query` | POST | `crates/chatsvc/src/substrate_search/mod.rs` | message, people and file search (KQL) | R | partly - `/search/query` chatMessage, KQL subset |

## Link previews (`*.asyncgw.teams.microsoft.com/urlp/v1/url`)

| Path | Method | Module | Used for | R/W | Graph? |
|---|---|---|---|---|---|
| `/info?url={encoded}` | GET | `crates/chatsvc/src/links.rs` | OpenGraph preview | R | no |
| `/image/Thumbnail?url=...` | GET (binary) | `crates/chatsvc/src/links.rs` (host check `LINK_PROXY_HOST_SUFFIX`, reused by `crates/core/src/external_image.rs`) | preview thumbnail | R | no |

## Calling

| Host + path | Method | Module | Used for | R/W | Graph? |
|---|---|---|---|---|---|
| `api-emea.flightproxy.teams.microsoft.com/api/v2/epconv` | POST | `crates/calling/src/signaling.rs` | create conversation, subscribe meeting, resolve meeting id | W | no - `/communications/calls` is app-only (bot registration), not a user client |
| `api.flightproxy.teams.microsoft.com/api/v2/ep/{cc or conv host}/{path}` | as link | `crates/calling/src/signaling.rs` (`routed_url`) | transport for every `*.cc.skype.com` / `*.conv.skype.com` link | R/W | no |
| conversation controller links (server-provided): join, end for all, broker poll, updateEndpointState, publishState, removeState, sendMessage, addParticipant(AndModality), updateEndpointMetadata, admit, removeParticipant, mute, updateMeetingSettings, updateParticipantRole, notificationLinks, leave, transfer | POST, PUT, GET, DELETE | `crates/calling/src/signaling.rs` | meeting join, roster, hand raise, spotlight, reactions, invite, lobby, mute, lock, roles, keep-alive, leave, transfer | R/W | no - no user-token call control in Graph; `updateMeetingSettings` partly via onlineMeeting PATCH |
| call leg links (server-provided, `*.cc.skype.com`): attach, progress, acceptance, callLeg, rejection, mediaAcknowledgement, applyChannelParameters, controlVideoStreaming, mediaAnswer, updateMediaDescriptions | POST, DELETE | `crates/calling/src/signaling.rs`, `crates/calling/src/call.rs`, `crates/calling/src/push.rs`, `crates/calling/src/engine.rs` | incoming call, SDP offer / answer, ringing, decline, keep-alive, renegotiation, video layout | W | no |
| recording / caption bot command URL (from roster) | POST | `crates/calling/src/signaling.rs`, `crates/calling/assets/caption_command.js` | recording start / stop, captions on / off, caption language | W | no - Graph only has post-meeting recordings and transcripts |
| `teams.cloud.microsoft/api/authsvc/v1.0/authz` | POST | `crates/calling/assets/caption_command.js` | Skype token for the bot command | R | no |
| `teams.microsoft.com/trap/tokens` | GET | `crates/calling/src/relay.rs` | TURN credentials | R | no |
| `gateway-eu.az.relay.teams.cloud.microsoft` (TURN 3478 udp, 443 tcp, turns 443) | TURN | `crates/calling/src/relay.rs` | media relay | - | no |

Link parsing only (not requests): `crates/calling/src/breakout.rs` (breakout join links), `crates/calling/src/whiteboard.rs` (whiteboard share detection).

Teams on the web links (opened in a window, not requested by us): `crates/app-state/src/call/web_call.rs` (meetup-join and chat links for the "Use Teams on the web for calls" setting).

## Teams CDN (`statics.teams.cdn.office.net`, no auth)

| Path | Method | Module | Used for | R/W | Graph? |
|---|---|---|---|---|---|
| `/evergreen-assets/audio/{file}.mp3` | GET | `crates/chatsvc/src/teams_cdn.rs` (URL), `crates/app-base/src/notify/sound_cache.rs` (download) | notification sounds | R | no - static asset |
| `/evergreen-assets/stickerassets/{name}-250x250/{file}?v=5` | GET | `crates/chatsvc/src/teams_cdn.rs`, `crates/app-base/src/stickers.rs` | stickers | R | no |
| `/evergreen-assets/personal-expressions/v2/assets/emoticons/{id}/default/{size}_anim_f{tone}.png` | GET | `crates/chatsvc/src/teams_cdn.rs`, `crates/app-base/src/emoticons/mod.rs` | emoticon images | R | no |
| `/evergreen-assets/backgroundimages/config.json?v=7` + images from it | GET | `crates/chatsvc/src/teams_cdn.rs`, `crates/calling/src/background.rs` | call background catalog | R | no |

## Third-party and SharePoint

| Host + path | Method | Module | Used for | R/W | Graph? |
|---|---|---|---|---|---|
| `media*.giphy.com/...` (URL from message or GIF search) | GET | `crates/app-base/src/remote_image.rs`, `crates/graph/src/content.rs` | GIF display | R | no |
| `{site}/_api/v2.1/shares/{shareId}/driveItem`, `/drives/{d}/items/{i}/listItem`, `/media/transcripts`, `/media/transcripts/{t}/streamContent`, `DELETE /items/{i}` | GET, DELETE | `crates/chatsvc/src/recap.rs`, `crates/chatsvc/src/transcripts.rs` | recording item, expiry, transcripts, delete recording | R/W | partly - Graph `/shares`, listItem, `/onlineMeetings/{id}/transcripts` (vtt only, meeting-scoped), `DELETE /drives/{d}/items/{i}` |
| pre-authenticated `downloadUrl` / `uploadUrl` (`*.sharepoint.com`, issued by Graph) | GET, PUT, DELETE | `crates/graph/src/files.rs` | file download, chunked upload | R/W | already Graph-issued; chunk PUT is the documented protocol |

## Local (not Microsoft)

| Host + path | Method | Module | Used for |
|---|---|---|---|
| `127.0.0.1:9222/json/*`, `ws://127.0.0.1:9222/devtools/page/{id}` | GET, PUT, WS | `crates/browser/src/http.rs`, `crates/browser/src/browser.rs`, `crates/session/src/session.rs` | drive headless Chrome |
| `127.0.0.1:{port}/mcp`, `/healthz` | HTTP | `crates/mcp/src/server.rs`, `crates/mcp/src/install/wsl.rs`, `crates/app-state/src/agent_api/mod.rs` | MCP server for agents |

## Graph calls made from service crates (for completeness)

`crates/chatsvc/src/calendar/reminders.rs` (`/me/reminderView`, snooze, dismiss), `crates/chatsvc/src/calendar/schedule.rs` (`/me/events`, RSVP, iCalUId lookup), `crates/chatsvc/src/recap.rs` and `crates/chatsvc/src/transcripts.rs` (`/chats/{id}/messages`, `/chats/{id}`, `/me/onlineMeetings`, attendance reports), `crates/calling/src/signaling.rs` (`/me`). These are Graph already.

## Open follow-ups

- Graph calls above live in `chatsvc` and `calling`, not in the `graph` crate.
- `chatsvc` builds its service base URLs per module (`https://teams.cloud.microsoft/api/{chatsvc,mt,csa,nss}/{region}`); CSA base exists twice (`pins/mod.rs`, `translate.rs`). One `chatsvc` base-URL module would remove the repeats.
- `recap.rs` and `transcripts.rs` duplicate the Graph chat scan and SharePoint item lookup.
- Calling: `CHATSVC_REGION` is owned by `signaling.rs` and reused by MT modules; meeting options read and write use two hosts (`teams.microsoft.com` read with `emea//beta`, `teams.cloud.microsoft` write).
- Token exchange `authsvc/v1.0/authz` appears in two JS assets (`chatsvc/assets/discover_host.js`, `calling/assets/caption_command.js`).
- Region is hard-coded `emea` in several places (`recap.rs` `DEFAULT_REGION`, calling `CHATSVC_REGION`, `relay.rs` host, flightproxy `api-emea`).
