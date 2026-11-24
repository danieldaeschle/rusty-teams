# Proposal

## Why

Search and the new-chat recipient field only resolve people inside the signed-in organization. A name of someone in another organization, and a full email of someone with no existing chat, produce no person row. Teams shows people you have already worked with by name, and anyone else once the email is complete.

## What Changes

- Name suggestions include people from other organizations when they already exist in the people index (recent chats, meetings, contacts), not only directory users of the home tenant.
- A complete email that the directory does not contain is resolved through the external people lookup, including federated work accounts, and still appears when the address is returned only as a user principal name.
- A person row keeps a usable id when the result has an MRI and no directory object id, and shows the email and organization name alongside the job title.
- The same resolution feeds the search switcher and the new-chat "To" field. Choosing the person still opens or creates the 1:1 chat.

## Capabilities

### New Capabilities

- `people-search`: Finding people by name or email in search and when starting a chat, including people outside the signed-in organization.

### Modified Capabilities

- None.

## Impact

- Substrate people suggestions and query filter (additional people subtypes).
- Directory search fallback when a full email is absent from the tenant, and the external profile parser.
- Search switcher person rows and the new-chat recipient rows.
- Out of scope: searching the world by a partial name of someone you have never met; @mention lists stay members plus the organization directory, except that a complete external email can appear because it uses the same resolver. This change does not publish the build to the upstream GitHub release feed.
