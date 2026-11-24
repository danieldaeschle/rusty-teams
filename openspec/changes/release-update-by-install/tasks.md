## 1. Feed

- [x] 1.1 Keep the platform asset suffix, including Windows ARM64, for self-update
- [x] 1.2 Add a tag-only lookup and the release page URL for the notice

## 2. Paths

- [x] 2.1 Restore the Windows quiet download, idle restart, and call guard
- [x] 2.2 Keep the Linux consent swap for the installed copy
- [x] 2.3 Open the release page for every other release build
- [x] 2.4 Hide the check on Flatpak and on a build without a release tag
- [x] 2.5 Read and write `updates.enabled` on every OS

## 3. Verify

- [x] 3.1 `cargo test -p app-base --lib -- release_feed updater`
