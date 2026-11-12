# Tasks

## 1. Locale-Gerüst

- [x] 1.1 App-Katalog `en`/`de` mit `rust-i18n` (eigener Katalog, Fallback `en`) und Start-Hook anbinden; `gpui-component`/`gpui_kit` `set_locale` mitziehen. Prüfen mit `cargo test -p app --lib ui_locale` (oder schmalerer Filter sobald Modul steht).
- [x] 1.2 Preference `auto`|`en`|`de` laden/speichern (Store-Meta analog Appearance) und effektive Locale aus OS ableiten (`de*` → `de`, sonst `en`). Prüfen mit `cargo test -p app-base --lib ui_locale` bzw. dem Crate, das die Preference hält.
- [x] 1.3 Settings-Steuerung für UI-Sprache (inkl. Kennzeichnung bei `auto`); Wechsel setzt Locale live und persistiert. Prüfen mit `cargo test -p app --lib ui_locale_settings`.

## 2. Chrome und Settings

- [x] 2.1 Rail, Sidebar-Tabs und Settings-Navigation/Suchbegriffe über Locale-Lookup. Prüfen: sichtbare Labels unter Locale `de` und `en` in `cargo test -p app --lib ui_locale_chrome`.
- [x] 2.2 Häufige Dialogaktionen und Settings-Abschnitte General/Appearance/Notifications auf Katalog umstellen. Prüfen mit `cargo test -p app --lib ui_locale_settings`.

## 3. Kernflächen

- [x] 3.1 Chat-/Conversation-UI (Composer-Hinweise, Menüs, Tooltips) lokalisieren. Prüfen mit `cargo test -p app --lib ui_locale_chat`.
- [x] 3.2 Call-UI (Controls, Prejoin, Layout-Hinweise, Tooltips) lokalisieren. Prüfen mit `cargo test -p app --lib ui_locale_call`.
- [x] 3.3 Calendar- und übrige Dialoge der Kernflächen lokalisieren. Prüfen mit `cargo test -p app --lib ui_locale_calendar`.

## 4. Notices, Tray, Agents, Rest

- [x] 4.1 Stabile Notice-IDs (plus Args) in `calling`/`app-state` statt englischer Notice-Literale; UI löst Texte auf. Prüfen mit `cargo test -p calling --lib notice_id` und `cargo test -p app --lib ui_locale_notice`.
- [x] 4.2 Tray-Menü, Toasts, Fenster-Titel und Agent-UI lokalisieren; nach Locale-Wechsel Tray neu syncen. Prüfen mit `cargo test -p app --lib ui_locale_tray`.
- [x] 4.3 Restliche produktseitige Nutzertexte (Shortcuts-Beschriftungen, verbleibende Fehlerhinweise) abdecken; fehlende DE-Keys fallen auf EN zurück. Prüfen mit `cargo test -p app --lib ui_locale_fallback`.

## 5. Zusammenspiel

- [x] 5.1 `cargo test -p app --lib ui_locale`, `cargo test -p calling --lib notice_id` und die Preference-Tests der Phase 1 gemeinsam grün.
- [x] 5.2 Manuell: frische Daten ohne Meta → System `de*` zeigt DE; Override EN überlebt Neustart; Wechsel DE↔EN ohne App-Neustart; ein Call-Hinweis und das Tray-Menü folgen der Locale. Chat-Nachrichteninhalt bleibt unverändert.

## Workflow follow-up

- Nach Umsetzen mit `/opsx-apply` und Review den Change archivieren (`/opsx-archive`), damit `ui-locale` unter `openspec/specs/` landet.
