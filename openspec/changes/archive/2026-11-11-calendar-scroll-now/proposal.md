# Proposal

## Why

Die Tages- und Wochenansicht scrollt heute einmal fest auf 08:00 und folgt der aktuellen Uhrzeit nicht. Beim Öffnen und bei «Today» landet man oft weit weg von «jetzt»; die Now-Linie wandert nur mit, wenn zufällig etwas anderes neu zeichnet. Der Kalender soll zur lokalen Uhrzeit passen.

## What Changes

- Beim Öffnen der Tages-/Wochenansicht und bei «Today» scrollt der Viewport so, dass die aktuelle Uhrzeit sichtbar ist (nicht fest 08:00).
- Die Now-Linie aktualisiert sich im Takt der Uhr, solange die Ansicht offen ist.
- Solange der Nutzer nicht manuell vertikal scrollt, bleibt die Ansicht an der aktuellen Uhrzeit ausgerichtet; nach manuellem Scroll bleibt die Position, bis «Today» oder erneutes Öffnen der Ansicht die Ausrichtung zurücksetzt.
- Monatsansicht unverändert (kein Stundenraster).
- Kein **BREAKING** für Daten oder APIs.

## Capabilities

### New Capabilities

- `calendar-now-view`: Tages-/Wochenraster an der lokalen Uhrzeit ausrichten (Scroll, Now-Linie, Nachführen ohne manuelles Scrollen).

### Modified Capabilities

- (keine)

## Impact

- UI: `crates/app/src/views/calendar_view.rs` (Scroll-Offset, Takt, manuelles Scroll erkennen).
- Optional leichter Takt über GPUI/App-Timer; keine Kalender-API-Änderung.
- Unabhängig vom Change `google-calendar`.
