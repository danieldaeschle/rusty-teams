# Tasks

## 1. Scroll-Ziel und Follow-Zustand

- [x] 1.1 Feste 08:00-Scroll-Logik durch Offset aus lokaler Uhrzeit (+ Viewport-Bias, Clamp) ersetzen; Verify: Unit-Test für Offset-Hilfsfunktion (Stunde/Minute → Pixel) in `crates/app` oder extrahiertem Modul — `cargo test -p app --lib` mit Filter auf den Hilfsnamen
- [x] 1.2 `follow_now` einführen: an bei Öffnen Tag/Woche und Today, aus nach erkanntem manuellem Scroll; programmatisches `set_offset` darf Follow nicht beenden; Verify: Unit-Tests für Zustandsübergänge (an → manuell aus → Today wieder an)

## 2. Now-Linie und Timer

- [x] 2.1 Periodischen Takt in `CalendarView` solange Tag/Woche sichtbar: Now-Linie neu zeichnen; bei `follow_now` Viewport nachziehen; bei Monat/unsichtbar Timer stoppen; Verify: manuelles Smoke (Linie wandert ohne Kalender-Poll) + bestehende Calendar-Layout-Guards grün wo vorhanden

## 3. Integration

- [x] 3.1 «Today» und Öffnen der Calendar-Insel triggern Align-to-now; Monat unverändert; Verify: kurzes Smoke-Protokoll (öffenen, scrollen, Today, Monat) und `cargo test -p app --lib` für neue Hilfs-/State-Tests

## Workflow follow-up

- Nach Merge auf dem Fork: `/opsx-archive` für `calendar-scroll-now`.
- Unabhängig von `google-calendar` umsetzen und releasen.
