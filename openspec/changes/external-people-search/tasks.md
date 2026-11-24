## 1. People index

- [x] 1.1 Add `ImplicitContact`, `ChatImplicitContact`, `OrganizationContact`, `EDUser`, and `Unknown` to `people_filter` in `crates/chatsvc/src/substrate_search/request.rs`
- [x] 1.2 Assert those subtypes in the suggestions request test

## 2. External email

- [x] 2.1 Call `externalsearchv3` with `includeTFLUsers=true` and `includeEDUsers=true`
- [x] 2.2 Parse `email`, `mail`, `emailsInfo`, and `userPrincipalName`, and a single profile object
- [x] 2.3 On a complete email, an unknown address still calls the external lookup. Another Graph error returns that error when the external lookup finds nobody
- [x] 2.4 Keep a search-switcher person when mail or user principal name matches the query

## 3. Identity and rows

- [x] 3.1 Fill `user_id` from `ExternalDirectoryObjectId`, an `8:orgid` MRI, or a full `8:live` MRI
- [x] 3.2 Subtitle: job title or email, plus organization name when known
- [x] 3.3 New chat merges suggestion people with `search_people` and shows email and organization
- [x] 3.4 New chat runs suggestions without the email fallback together with `search_people`

## 4. Verify

- [x] 4.1 `cargo test -p chatsvc --lib substrate_search user_profiles`
- [x] 4.2 `cargo test -p core --lib server_search native remote` (narrower filters if the names differ)
- [x] 4.3 `cargo test -p app --lib switcher_server new_chat`
- [x] 4.4 Restore the chat `thread_id` assertion in the suggestions fixture test

## 5. Delivery

- [ ] 5.1 CHANGELOG entry and the next free `v0.5.0-joe.N` tag on `origin` only
- [ ] 5.2 After the release workflow publishes assets, open a Forgejo pull request to `main` whose body links the Forgejo release and the `d0dg3r` mirror when that mirror has the tag
