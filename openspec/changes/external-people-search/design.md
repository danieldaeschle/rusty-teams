# Design

## Context

See proposal.md. Name suggestions come from substrate people requests filtered to `OrganizationUser`, `MTOUser`, `PersonalContact`, and `Guest` (`crates/chatsvc/src/substrate_search/request.rs`). A complete email is looked up in Graph and, only when that call succeeds and the mail does not match, via `externalsearchv3` with `includeTFLUsers=true` (`crates/core/src/remote.rs`, `crates/chatsvc/src/user_profiles.rs`). The search switcher drops people without `ExternalDirectoryObjectId` and subtitles them with job title and department (`crates/app/src/views/switcher_server.rs`). New chat calls `search_people` only, so people who exist solely in the people index never appear there.

## Goals / Non-Goals

**Goals:**

- One people filter for suggestions and query that also asks for implicit and external contact subtypes.
- External email resolution that still runs when the directory lookup is empty or fails, and that accepts mail or user principal name.
- Person ids from MRI, and subtitles that show email and organization.
- New chat uses the same suggestion people plus the directory and external email lookup.

**Non-Goals:**

- A worldwide directory of people the account has never met.
- Changing @mention ranking beyond what the shared email resolver already returns.
- Publishing the Joe tag to `danieldaeschle/rusty-teams`.

## Decisions

1. **Widen the existing people filter** rather than a second query. Add `ImplicitContact`, `ChatImplicitContact`, `OrganizationContact`, `EDUser`, and `Unknown` next to the current subtypes. Suggestions and query already share `people_filter`.

2. **External lookup stays behind a complete email.** `is_email_address` is unchanged. `externalsearchv3` gains `includeEDUsers=true`. The profile parser reads `email`, `mail`, `emailsInfo`, and `userPrincipalName` into the mail field, and accepts a single profile object as well as a `value` array. A personal account comes back as an `8:live` MRI with no `objectId`.

3. **Directory failure does not skip the external lookup.** For a complete email, an unknown address (HTTP 400 or 404, or an empty directory) is treated as no directory person, then `search_external` runs. Any other Graph error still tries the external lookup. If that lookup returns a person whose mail or user principal name matches, that person is the result. If it returns nobody or fails, the Graph error is returned and is not cached. A non-email Graph error is still returned without an external lookup. A person matches the query when mail or user principal name equals it, ignoring case.

4. **Id from MRI.** `person()` sets `user_id` from `ExternalDirectoryObjectId`, otherwise from an `8:orgid:` MRI with that prefix removed. An `8:live:` MRI is kept in full, because chat creation treats an id that already contains `:` as a complete MRI and would otherwise prefix `8:orgid:`. The switcher keeps requiring `user_id`.

5. **Subtitle.** Job title when set, otherwise the first email. Organization name when `company_name` is set; department remains only when there is a job title and no organization, so existing organization rows stay readable. New-chat rows use the same pair: email, then organization when known. `remember_external` already stores the tenant name; the recipient row reads it for directory-shaped users.

6. **New chat merges both sources.** After the debounce it starts `server_suggestions` with the email fallback off and `search_people` together, then dedupes by user id. Suggestion people come first. The search switcher keeps the email fallback inside `server_suggestions`. Local chat members stay first, as today.

## Risks / Trade-offs

- [A subtype the service rejects empties the people group] → The added names are the ones Teams sends; the unit test locks the list so a later edit is visible.
- [External lookup returns a person whose address fields are empty] → The row is omitted. The spec requires a mail or user principal name match.
- [Two requests from the recipient field] → They run together after the same debounce. The email path does not call `search_people` a second time. A stale generation is dropped.

## Migration Plan

No data migration. Rollback is reverting the change. Delivery is a Joe tag on Forgejo and a Forgejo pull request that links the built release. The app update feed stays on the upstream GitHub releases.
