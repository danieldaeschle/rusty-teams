# Tasks

## 1. Writer-Resolver und Chat-Thread

- [x] 1.1 Gemeinsame Auflösung von «Write with» (Chat-Profil oder Microsoft) für App-UI bereitstellen und mit `cargo test -p app --lib` bzw. einem gezielten Unit-Test für leeres/`write_with`-Profil verifizieren
- [x] 1.2 Thread-Text für einen offenen Chat bauen (max. 20 nicht gelöschte Nachrichten, Sender analog `message_context`) und Unit-Test in `app-state` oder `agents` grün halten (`cargo test -p app-state --lib` bzw. passender Filter)

## 2. Composer-Branding

- [x] 2.1 In `crates/app/src/views/composer/rewrite.rs` Working-/Failed-/Empty-/Text-only-Strings und Button-Icon an den lokalen Writer-Namen koppeln; Microsoft-Pfad unverändert lassen — verifizieren durch Review der Strings und `cargo test -p app --lib` falls vorhandene Rewrite-Tests greifen
- [x] 2.2 Settings-Hinweis unter «Write with» so anpassen, dass Rewrite und Chat-Insel erwähnt werden; manuell im Settings-Dialog prüfen

## 3. Chat-Insel: Branding und lokaler Ask

- [x] 3.1 In `copilot.rs` und `chat_header_actions.rs` Titel, Tooltips, Placeholder, Working/Failed und Logo/Icon dynamisch aus dem Writer ableiten — verifizieren: mit lokalem Profil kein `copilot_color.svg` als Identitätsmarke
- [x] 3.2 Insel-Ask bei lokalem Writer über `complete_text` mit Thread+Frage; ohne Engine erreichbar; bei leerem `write_with` weiter `ask_copilot` — verifizieren mit gezieltem Test oder manuellem Smoke (lokales Profil vs. Microsoft)
- [x] 3.3 Integration: `cargo test -p agents --lib` und `cargo test -p app-state --lib` (oder engere Filter) grün; kurzer Smoke: Write with → Ollama/OpenAI-Profil → Insel-Frage und Composer-Rewrite zeigen Profilname

## Workflow follow-up

- Archive nach Review und Merge der Spec-Deltas in `openspec/specs/agent-providers`
- Alltagskomits nach `fork` pushen, nicht nach `origin`
