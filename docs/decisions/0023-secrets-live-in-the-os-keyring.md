---
status: proposed
date: 2026-11-23
---

# ADR 0023: Secrets live in the OS keyring, the cache is owner-only

## Context

ADR 0019 moved CalDAV app passwords to the OS keyring, but Google OAuth tokens (access and refresh), API keys of chat agent profiles and the MCP bearer token stayed in plain text in the `meta` table of the cache database, next to the cached messages. The database and its WAL files were created with the default umask, readable by other local users. Options were: encrypt the whole database (SQLCipher, key in the keyring), move only the secrets to the keyring, or only tighten file permissions.

## Decision

Every secret goes through `Store::secret` / `set_secret` / `delete_secret`. The app installs `KeyringVault` (service `rusty-teams`, user `<database file>/<key>`) on each tenant database, so tenants keep separate secrets. A store without a vault (tests, the CLI, demo mode) keeps a secret in `meta` under `secret:<key>`. Plain values written by older versions move to the vault the first time they are loaded; the secrets of a disconnected Google account or a removed agent profile are deleted. Keyring calls run on their own thread, as the keyring crate asks for its tokio Secret Service backend, and each key is read once per session.

When the keyring fails, the value stays in memory for the running session and is never written to the database instead, like the CalDAV password in ADR 0019. On unix the database file and its `-wal`/`-shm` files are `0600` and the data directory is `0700`.

## Consequences

- No token or API key in the cache database when a keyring is available; the cached messages stay unencrypted, protected by file permissions and disk encryption.
- Without a working keyring (no Secret Service on a headless Linux box) Google sign-in and API keys last one session, and the MCP token changes at every start, so installed agents need a reinstall.
- A build older than this one finds no secrets after the move: Google sign-in and API keys have to be entered again.
- Flatpak needs `org.freedesktop.secrets`, as for ADR 0019.
