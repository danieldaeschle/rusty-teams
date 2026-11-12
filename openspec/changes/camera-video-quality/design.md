# Design

## Context

Siehe `proposal.md` — Why. Heute: `CAPTURE_HEIGHT = 720`, Preferenz exakt 720, Downscale-Decke 1080 aber `fit_widescreen` kürzt wieder auf 720; Stage-Receive `max-fs=3600` (~720p); Screen schon `8160` (~1080p). Codec-Filter bleibt H.264 (`h264_only`).

## Goals / Non-Goals

**Goals:**

- Eine Qualitaetsstufe steuert Capture-Decke, Send-Advertise und Stage-Receive gemeinsam.
- 1080p als praktisches Maximum; Auto faellt sauber auf 720p zurueck.
- Bestehende Small-Tile- und Screen-Share-Limits unangetastet.

**Non-Goals:**

- 4K-UI oder 4K-Advertise.
- H.265 / AV1.
- Mixer-Bitrate erzwingen jenseits der Teams-`controlVideoStreaming`-Antworten.
- Aenderung am Screen-Share-Capture-Pfad.

## Decisions

### 1. Eine Einstellung fuer Capture + Advertise + Stage

**Wahl:** `VideoQuality { Auto, P720, P1080 }` wirkt auf alle drei.

**Warum:** Getrennte «Senden»/«Empfangen»-Regler verdoppeln UI und erklaeren selten den wahrgenommenen Weichzeichner. Stage und Eigenbild sollen dieselbe Erwartung teilen.

**Alternative:** Nur Capture aendern — abgelehnt, weil Stage weiter 720p anfordert.

### 2. Konstanten an Macroblock-Budgets koppeln

**Wahl:** 720p-Stage bleibt `max-fs=3600` / heutige `max-mbps`/`max-fps`; 1080p-Stage uebernimmt die heutigen Screen-`ReceiveLimits` (`8160` / `135_000` / `1500` fps-Einheiten) oder eine bewusst dokumentierte 1080p-Kamera-Variante mit gleichem `max-fs`, falls Screen-FPS zu niedrig fuer Kamera wirkt — dann `max-fps` bei 1080p-Stage auf Kameraniveau (~3000) halten und `max-mbps` proportional anheben.

**Empfehlung im Apply:** `max-fs=8160`, `max-fps=3000`, `max-mbps` so skalieren, dass 1080p@30 in Macroblock·FPS passt (≈ `8160 * 30` = 244800), nicht blind Screen-1500-FPS kopieren.

### 3. Capture-API parametrisieren

**Wahl:** Preferenz-Hoehe und Downscale-Decke aus der aktiven Qualitaet ableiten statt hartem `CAPTURE_HEIGHT`. `pick_format` / `fit_widescreen` / Fallback-`Resolution` lesen dieselbe Decke. `LARGEST_DOWNSCALED_HEIGHT` fuer 1080-Ceiling darf hoehere Sensorformate bis z. B. 1440 zulassen, solange das Ergebnis auf ≤1080 skaliert — kein 4K-Modus in der UI.

### 4. Persistenz und UI-Ort

**Wahl:** Persistenz neben bestehenden Geraete-/Call-Settings; Auswahl im Kamera-Menue (Call + Prejoin), Labels «Auto», «720p», «1080p». Default `Auto`. Wechsel im Call: Kamera-Track neu oeffnen wenn noetig; Stage-Subscriptions die Limits neu posten (`applyChannelParameters`).

### 5. Codec unveraendert

H.264-only bleibt. Qualitaet ≠ Codec-Wechsel.

## Risks / Trade-offs

- [Blur/Effects bei 1080p teurer] → Mitigation: Auto trotzdem 1080; bei messbaren Drops spaeter optional «Performance» — nicht in diesem Change.
- [Mixer ignoriert hoehere Advertise] → Mitigation: lokales Self-View und Capture trotzdem schaerfer; Stage-Request erhoehen unabhaengig.
- [Kamera ohne 30fps-1080] → Mitigation: Fallback 720; Start nicht hart failen.
- [Screen-Limits fuer Stage falsch uebernehmen (niedriges max-fps)] → Mitigation: Decision 2 mit kameratauglichem `max-fps`/`max-mbps`.

## Migration Plan

- Default Auto → Nutzer ohne Setting bekommen 1080 wo moeglich (Verhalten aendert sich spuerbar).
- Rollback: Setting auf 720p oder Code-Revert der Capture-Konstanten.

## Open Questions

- Ob Prejoin-Preview dieselbe Qualitaetsstufe sofort nutzt oder erst nach Join (bevorzugt: sofort, konsistent mit Call).
