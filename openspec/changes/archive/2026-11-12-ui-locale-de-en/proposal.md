# Proposal

## Why

Die gesamte App-Oberfläche ist heute englisch hardcodiert. Nutzer mit deutscher Systemsprache und der Fork-Zielgruppe brauchen eine durchgängig deutschsprachige UI, ohne die englische Basis und Upstream-Kompatibilität zu verlieren. Message-Translation und Caption-Sprachen existieren schon; die App-UI-Sprache fehlt.

## What Changes

- Die App bietet zwei UI-Sprachen: Englisch und Deutsch (Deutschland-Orthografie im Produktkatalog).
- Beim ersten Start folgt die UI der OS-Locale: System-Locale `de*` → Deutsch, sonst Englisch.
- Nutzer können die UI-Sprache in den Einstellungen dauerhaft überschreiben; die Wahl wird gespeichert und gilt beim nächsten Start.
- Umschalten aktualisiert sichtbare UI-Texte ohne Neustart, soweit die Texte zur Laufzeit aufgelöst werden.
- Alle produktseitigen UI-Texte sind lokalisiert: Navigation, Settings, Dialoge, Menüs, Tooltips, Shortcuts, Toasts, Tray, Agent-UI und Call-/Fehler-Hinweise.
- Call- und ähnliche Nutzerhinweise tragen stabile Schlüssel; der Anzeigetext entsteht erst an der UI-Grenze in der aktiven Locale.
- Fehlende Übersetzungen fallen auf Englisch zurück statt leerer oder roher Keys.
- Unverändert ausserhalb: Chat-/Server-Inhalte, Adaptive Cards vom Server, Geräte-Namen vom OS, Wire-Protokoll-Strings, Captions-Spoken-Language, bestehende Message-Translation.

## Capabilities

### New Capabilities

- `ui-locale`: UI-Sprache der App wählen, aus OS-Locale ableiten, persistieren und auf die gesamte App-Oberfläche anwenden (Englisch und Deutsch).

### Modified Capabilities

- Keine. Message-Translation und Caption-Sprachen bleiben eigene, unveränderte Verhaltensbereiche.

## Impact

- UI-Crate (`crates/app` und verwandte Views), Persistenz analog Appearance/Settings-Meta, Tray- und Toast-Texte.
- Call-Hinweise und vergleichbare Nutzertexte in `crates/calling` / `app-state`: Umstellung von englischen Literalen auf stabile Hinweis-IDs, Auflösung in der UI.
- Katalog und Locale-Laufzeit analog dem bestehenden `rust-i18n`-Muster in `vendor/gpui-component` (eigener App-Katalog, Vendor-YAML nicht mischen).
- Layout-/UI-Tests, die englische sichtbare Strings erwarten, müssen locale-bewusst werden oder gegen Keys/IDs prüfen.
- Lieferung auf dem Fork (`origin` Forgejo). Kein Riesen-PR der ganzen i18n-Welle an Upstream; Infrastruktur kann später als eigene Scheibe angeboten werden.
