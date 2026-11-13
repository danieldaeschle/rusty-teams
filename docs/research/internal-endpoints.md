# Internal endpoints added by Linux chat fallback

`crates/graph/src/teams_chat.rs` uses the same IC3-authenticated Teams service as the existing chatsvc module:

- GET https://teams.cloud.microsoft/api/chatsvc/{region}/v1/users/ME/conversations: sidebar chat list.
- GET .../conversations/{id}?view=msnp24Equivalent: chat metadata.
- GET .../conversations/{id}/messages and /messages/epochTimeStamp/{time}: chat history and paging.
- GET .../conversations/{id}/messages/{messageId}: individual message refresh.

Graph remains preferred when Chat.Read is granted. These reads are used only when the existing web session lacks Graph chat scopes and already holds a Teams service token. Pagination is restricted to the same Teams service origin.
