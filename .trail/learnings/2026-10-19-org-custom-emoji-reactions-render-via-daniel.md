---
title: Org (custom) emoji reactions render via AMS image
author: Daniel
date: 2026-10-19
branch: main
touches:
  - Cargo.lock
  - crates/app-state/src/reaction_model.rs
  - crates/app-state/src/rows.rs
  - crates/app/src/views/conversation/render_env.rs
  - crates/app/src/views/reaction_pills.rs
  - crates/chatsvc/src/lib.rs
  - crates/chatsvc/src/reactions.rs
  - crates/core/examples/live_custom_emoji_tmp.rs
---

## Decided
- Reaction key `name;<amsObjectId>` (e.g. `absolute-cinema;0-wcde-d3-...`) is a tenant custom emoji. Teams (bundle module 280767, fn JT) builds `{amsV2Domain}v1/objects/{id}/views/imgt2_anim` (or `views/img_thumb_tiny` for stills).
- We use `https://de-prod.asyncgw.teams.microsoft.com/` as amsV2Domain, same host as URLP link previews (chatsvc/links.rs). Parsing + URL in `chatsvc::custom_emoji`.
- Fetch goes through the existing image pipeline: `external_image_url` returns None -> `hosted_content` -> `is_link_image_url` (asyncgw host) -> `Messages::link_image`.
- UI: `reaction_pills::reaction_emoji` renders the image in pill, tabs and reactor list; request via `render_env::request_missing_media`.

## Tried and ruled out
- `eu-api.asm.skype.com/v1/objects/...` answers 401 from the Teams web page; asyncgw answers 200 image/gif.
- Graph `reactionContentUrl` route: emotion_sync overwrites reactions from chatsvc keys, so it would be lost; also not how Teams does it.

## Watch out
- `cargo fmt -p app` reformats dozens of unrelated files (repo is not fmt clean). Use `rustfmt --check` on single files instead.
- Validate the object id (alnum and `-` only) before putting it in a URL.
