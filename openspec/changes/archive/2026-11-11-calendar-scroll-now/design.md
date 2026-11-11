# Design

## Context

Siehe `proposal.md` — Why. In `crates/app/src/views/calendar_view.rs` setzt `Render` einmal `scrolled_to_morning` und scrollt auf `FIRST_VISIBLE_HOUR` (8). Now-Linie (`now_minute`) wird nur bei Re-Render gezeichnet; `CalendarView` beobachtet `AppState`, hat aber keinen eigenen Uhr-Takt. «Today» (`calendar_to_today`) setzt nur das Datum.

## Goals / Non-Goals

**Goals:**

- Scroll-Ziel = aktuelle lokale Uhrzeit (mit kleinem Offset, damit die Linie nicht am oberen Rand klebt).
- Periodischer Re-Render / Scroll-Nachzug für Now-Linie und Follow-Modus.
- Follow aus, sobald der Nutzer das Stundenraster manuell scrollt; Reset bei Today / Öffnen Tag/Woche.

**Non-Goals:**

- Monatsraster scrollen.
- Scrollen zu einem bestimmten Termin (nur Uhrzeit).
- Zeitzonen-Umschalter UI.
- Google-Kalender (separater Change).

## Decisions

### 1. Scroll-Offset aus lokaler Uhrzeit

- **Choice:** `offset_y = -(hour + minute/60) * HOUR_HEIGHT + viewport_bias`, Bias etwa ein Drittel der sichtbaren Rasterhöhe (Linie nicht am Top). Clampen an Tagesgrenzen (0–24h).
- **Why:** Ersetzt die Konstante 8 Uhr; gleiches `ScrollHandle`-API.
- **Alternative:** Immer Now an Top — wirkt gequetscht.

### 2. Follow-Flag statt «nur einmal scrollen»

- **Choice:** Zustand `follow_now: bool` (default true beim Öffnen Tag/Woche und bei Today). Manuelles Scroll → `follow_now = false`. Today / erneutes Zeigen des Grids → `true` und sofort scrollen.
- **Why:** Deckt «immer passend», ohne den Nutzer zu jagen wenn er den Nachmittag anschaut.
- **Alternative:** Immer nachführen — verworfen.

### 3. Uhr-Takt

- **Choice:** Leichter Timer in `CalendarView` (z. B. jede Minute, oder ~15–30 s) solange Tag/Woche sichtbar: `cx.notify()`; wenn `follow_now`, Scroll-Offset neu setzen.
- **Why:** Now-Linie und Follow brauchen Tick unabhängig vom Kalender-Poll.
- **Alternative:** Nur bei AppState-Events — Linie bleibt stehen.

### 4. Manuelles Scroll erkennen

- **Choice:** Scroll-Offset des Handles beobachten (oder Scroll-Events der Grid-Surface): weicht der Offset vom erwarteten Follow-Ziel ab und kam nicht von unserem Setzen → Follow aus.
- **Why:** Kein Extra-Button «Live» nötig.
- **Risk:** Eigenes `set_offset` darf Follow nicht abschalten — kurzen «programmatic» Guard setzen.

## Risks / Trade-offs

- [Falsch positives «manuelles Scroll»] → Guard um programmatisches Setzen; nur Abweichung vom Ziel zählt.
- [Timer läuft im Hintergrund] → Timer nur starten wenn Calendar-Insel Tag/Woche sichtbar; bei Monat/Tab-Weg stoppen.
- [Sehr kurze Viewport-Höhe] → Bias kleiner wählen / clampen, Now trotzdem sichtbar halten.

## Migration Plan

- Rein clientseitiges UX; kein Persistenz-Schema.
- Rollback: altes Once-Scroll auf 8 Uhr wiederherstellen.

## Open Questions

- Keine spez-ändernden Offenheiten; genaue Timer-Periode (30 s vs 60 s) ist Implementierungsdetail.
