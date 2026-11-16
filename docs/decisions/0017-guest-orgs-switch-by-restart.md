---
status: accepted
date: 2026-11-15
---

# ADR 0017: Guest orgs, one org per window, switch by restart

## Context

One account can be a member of its home tenant and a guest in other tenants. Teams lists these orgs in the account menu, shows one org at a time and keeps the others alive for notifications.

Rusty Teams took tokens of the home tenant only, so a guest org could not be opened at all. The app's store, engine and views assume one org for the lifetime of the process.

Options were: swap the store and engine under the running UI, run a second process per org, or restart on the chosen org.

## Decision

- Guest tenants use the same browser session. `Session::in_tenant` picks tokens of that tenant from the page and redeems the account's refresh token at that tenant's token endpoint, as Teams does. No second sign-in.
- The org list comes from `mt/{region}/beta/users/tenants` and is kept in `orgs.json` beside the caches, together with the org the window shows.
- Each org has its own cache: the home org keeps `cache.sqlite3`, a guest org gets `cache-<tenant id>.sqlite3`. Local settings stored in the cache are per org.
- The account menu lists the orgs when there is more than one. Picking another org saves it and restarts the app, which opens that org's cache at once. Teams switches without a restart; this is the deviation.
- Calls keep the home tenant's registration in every org.

## Consequences

- A guest org works with all features that go through the session, because every token request carries the tenant.
- Switching costs a restart of a second or two instead of an in-place swap. Swapping the store under the UI would touch every view that holds it.
- Calls inside a guest org and Outlook calendar for a guest (no mailbox there) are not covered.
- Every org that is not open keeps its own realtime socket through a tenant session. New chat messages from others show as toasts that name the org, without quick reply, and the menu counts chats with new messages. Clicking the toast restarts on that org with `--open-conversation`. Channel posts of other orgs do not notify.
