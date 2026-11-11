# Design

## Context

Siehe `proposal.md` — Why. Heute:

- `AgentSettings.write_with` (leer = Microsoft) steuert Composer-Rewrite in `crates/app/src/views/composer/rewrite.rs` über `local_writer` → `agents::complete_text`.
- Tooltips nutzen bereits den Profilnamen; Status-/Fehlerstrings und Icons bleiben «Copilot» / `auto_fix_high`.
- Die Chat-Insel in `crates/app/src/views/conversation/copilot.rs` brandet fest mit `symbols/copilot_color.svg` und ruft nur `engine.ask_copilot` (Teams Sydney).
- Agent-Threads mit Limit 20 leben in `crates/app-state/src/agents/message_context.rs` — aber an einer festen `message_id`. Die Insel braucht denselben Limit-Gedanken für den ganzen offenen Chat.

## Goals / Non-Goals

**Goals:**

- Eine gemeinsame «aktive Writer»-Auflösung (Profil oder Microsoft) für Branding und Ask/Rewrite.
- Insel-Ask lokal mit Thread-Text (max. 20 Nachrichten), ohne Tools.
- Kein Copilot-Logo/Copy, wenn der Writer lokal ist.

**Non-Goals:**

- Kein MCP/Tool-Loop in der Insel.
- Keine Provider-spezifischen Marken-Assets (Ollama-/OpenAI-Logos); ein generisches Bot-Symbol reicht.
- Kein Umschreiben der Roboter-Agenten-Delegation (Hover-Toolbar).
- Kein Entfernen des Microsoft-Pfads.

## Decisions

### 1. Gemeinsamer Writer-Resolver

Eine kleine Hilfsfunktion (App-seitig, z. B. neben Agents-Runs-Handle) liefert:

- `None` / Microsoft → leeres `write_with`
- `Some(profile)` → gültiges Chat-Profil

Composer und Insel lesen dieselbe Quelle. Anzeigename = `profile.name`.

**Alternative:** Modellstring aus `AgentBackend` — verworfen, weil das Dropdown und die Spec den Profilnamen festlegen.

### 2. Branding

| Oberfläche | Microsoft | Lokal |
|---|---|---|
| Insel-Logo / Header | `copilot_color.svg` | Symbol `smart_toy` / Bot (bestehendes Asset) |
| Labels / Status | «Copilot …» | «{profile.name} …» |
| Composer-Button | `auto_fix_high` ok oder gleiches Bot-Symbol | gleiches Non-Copilot-Symbol + Tooltip «Rewrite with {name}» |

Overflow-Menü (`chat_header_actions.rs`) nutzt denselben dynamischen Label-String statt `'static` Copilot-Text.

### 3. Insel-Ask-Pfad

```
ask
  |
  +-- local_writer Some(profile)
  |     build thread text (newest 20 non-deleted)
  |     complete_text(system, thread + question)
  |
  +-- else
        engine.ask_copilot (wie heute; ohne Engine → UNAVAILABLE)
```

Systemprompt: Fragen zum aktuellen Teams-Chat beantworten; nur den gelieferten Thread nutzen; keine erfundenen Nachrichten.

Thread-Bau: `store.messages(chat_id, None, 20)` (bzw. äquivalent), Senderauflösung analog `message_context` (`local_names` / `sender_name`). Keine Abhängigkeit von einer angeklickten `message_id`.

**Alternative:** Insel an Agent-Run + MCP anbinden — verworfen (Explorations-Entscheidung: Option 1).

### 4. Verfügbarkeit ohne Engine

- Rewrite: `rewrite_available` schon `local_writer.is_some() || engine` — belassen/härten.
- Insel-Button/Ask: bei lokalem Writer Engine nicht voraussetzen; `ask_copilot`-Zweig nur auf Microsoft-Pfad.

### 5. Settings-Hinweis

Kurztext unter «Write with» erwähnen, dass Rewrite **und** die Chat-Insel dem Eintrag folgen.

## Risks / Trade-offs

- [Dünner Kontext] 20 Nachrichten reichen für lange Chats nicht → Mitigation: gleiches Limit wie Agent-Thread; später erweiterbar, Spec bleibt «at most twenty».
- [Kanal vs Chat] Insel heute nur bei `Selection::Chat` → bleibt so; Kanäle out of scope.
- [Mehrere Insel-Turns] Folgefragen bekommen weiterhin den aktuellen Chat-Thread, nicht zwingend den Insel-Dialogverlauf → akzeptiert für v1; Dialog-Turns nur in der UI.
- [Falsches Profil gelöscht] `write_with` zeigt auf fehlende ID → wie heute Fallback Richtung Microsoft-Label in Settings; Ask sollte klar fehlschlagen oder Writer als fehlend behandeln — Implementierung: fehlendes Profil = kein local_writer (wie `local_writer` heute).

## Migration Plan

Kein Datenmigrationsschritt: `write_with` existiert. Reines UI-/Ask-Verhalten. Rollback = vorheriges Binary.

## Open Questions

Keine spez-relevanten. Icon-Wahl (`smart_toy` vs `bot`) ist Implementierungsdetail.
