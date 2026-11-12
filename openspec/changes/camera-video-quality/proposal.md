# Proposal

## Why

Die Kamera wirkt weich, weil Capture und Stage-Empfang auf ~720p festgenagelt sind, obwohl viele Kameras und der Teams-Mixer 1080p koennen. Nutzer sollen die Sende-/Empfangsqualitaet bewusst steuern koennen, ohne 4K-Komplexitaet.

## What Changes

- Kamera-Capture bevorzugt **1080p @ 30 fps** (Widescreen), mit Fallback auf 720p wenn die Kamera das nicht liefert.
- Stage-Empfangs-Limits steigen auf **1080p-Niveau** (analog heutiger Screen-Share-`max-fs`/`max-mbps`-Groessenordnung).
- Neue persistente Einstellung **Videoqualitaet**: `Auto` | `720p` | `1080p` (kein 4K).
  - Steuert Capture-Decke, `maxVideoSendCapabilities` und Stage-Request-Limits zusammen.
  - `Auto` = hoechste unterstuetzte Stufe bis 1080p.
- Small-Tile-Limits und Screen-Share-Limits bleiben unveraendert.
- Codec bleibt H.264; kein H.265.

## Capabilities

### New Capabilities

- `camera-video-quality`: Capture-Preferenz, Sende-Faehigkeiten und Stage-Empfangs-Limits fuer Kameravideo, inkl. Nutzerwahl Auto/720/1080.

### Modified Capabilities

- (keine)

## Impact

- Capture/Formatwahl und Downscale in `crates/calling` (Kamera-Pfad).
- Stage-`applyChannelParameters` / Receive-Limits und Send-Capabilities in `crates/calling`.
- UI: Kamera-Menue bzw. Call-/Geraete-Einstellungen in `crates/app`; Persistenz analog bestehender Call-Settings.
- Tests: Formatwahl, Limit-Konstanten, Setting-Persistenz; Verify mit `cargo test -p calling --lib` und gezielten App-Tests.
