# Tasks

## 1. Qualitaetsmodell und Capture

- [x] 1.1 `VideoQuality` (Auto / 720 / 1080) und Ableitung der Capture-Decke in `calling` einfuehren; Verify: Unit-Test fuer Ceiling-Mapping
- [x] 1.2 `pick_format` / `fit_widescreen` / Open-Fallback auf die Decke umstellen (Preferenz 1080 bei Auto/1080); Verify: `cargo test -p calling --lib` Filter Formatwahl / widescreen
- [x] 1.3 Bestehende Capture-Tests an neue Preferenz anpassen; Verify: `cargo test -p calling --lib camera` (oder engerer Filter) gruen

## 2. Send- und Stage-Limits

- [x] 2.1 Stage-Receive-Limits fuer Auto/1080 auf 1080p-Klasse setzen (`max-fs` 8160, kamerataugliches `max-fps`/`max-mbps`); 720 belaesst alte Stage-Budgets; Verify: Tests in `video_layout` fuer Stage-Body
- [x] 2.2 Send-Capabilities weiter an tatsaechliche Capture-Groesse koppeln und gegen Ceiling absichern; Verify: `send_capabilities_body`-Tests / Reporter
- [x] 2.3 Small-Tile- und Screen-Share-Limits unveraendert lassen; Verify: bestehende Assertions fuer Small/Screen weiterhin gruen

## 3. Setting, UI, Apply im Call

- [x] 3.1 Videoqualitaet persistieren (Default Auto) analog Geraete-/Call-Settings; Verify: Roundtrip-Test oder Settings-Serialisierung
- [x] 3.2 Auswahl Auto/720p/1080p im Kamera-Menue (Call + Prejoin); Verify: manueller UI-Smoke oder View-Test falls vorhanden
- [x] 3.3 Wechsel der Stufe startet Capture neu und aktualisiert Stage-Requests ohne App-Neustart; Verify: gezielter calling/app-Test oder dokumentierter Smoke-Schritt
- [x] 3.4 Integration: `cargo test -p calling --lib` und relevante `cargo test -p app …`-Filter gruen
