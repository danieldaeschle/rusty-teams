# Proposal

## Why

«Write with» kann Microsoft Copilot schon durch ein Chat-Profil ersetzen, aber Composer und Chat-Insel bleiben optisch und textlich «Copilot». Wer kein MS-Abo nutzt, sieht weiter Copilot-Branding und die Insel spricht weiterhin nur die Teams-API.

## What Changes

- Wenn «Write with» ein Chat-Profil ist, zeigen Composer-Rewrite und die Chat-Header-Insel die Identität dieses Profils (Name + Non-Copilot-Icon), nicht «Copilot».
- Die Chat-Insel beantwortet Fragen dann über dasselbe Chat-Profil; Kontext sind die letzten ~20 Nachrichten des offenen Chats als Text (kein MCP/Tools).
- Composer-Status- und Fehlertexte folgen derselben Identität.
- Mit lokalem Writer bleiben Rewrite und Insel auch ohne Teams-Engine / ohne Copilot-Lizenz erreichbar.
- Leer belassenes «Write with» behält den Microsoft-Pfad inkl. Copilot-Branding (wie heute).

Annahme (Explorations-Feinheit): sichtbarer Anzeigename ist der **Profilname** aus den Agents-Einstellungen, nicht der Backend-Modellstring.

## Capabilities

### New Capabilities

- (keine)

### Modified Capabilities

- `agent-providers`: «Write with» steuert nicht nur den Rewrite-Backend-Pfad, sondern die gemeinsame Assistenten-Identität und den Ask-Pfad der Chat-Insel.

## Impact

- UI: `crates/app/src/views/composer/rewrite.rs`, `crates/app/src/views/conversation/copilot.rs`, Overflow-Menü in `chat_header_actions.rs`
- Settings-Texte optional: `settings_section.rs` (Hinweis, dass Insel mitwechselt)
- Kontextaufbau: Wiederverwendung/Erweiterung um Chat-Thread ohne feste `message_id` (Vorbild `message_context`, Limit 20)
- Lauf: `agents::complete_text` für Insel-Ask; Microsoft `ask_copilot` nur bei leerem `write_with`
- Spec-Baseline `openspec/specs/agent-providers` nach Archive
