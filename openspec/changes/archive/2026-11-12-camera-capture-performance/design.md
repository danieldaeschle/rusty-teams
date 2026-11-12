# Design

## Context

`open_picked_format` startete mit `Closest(NV12 1080p)`. Auf dem C930e gibt es kein NV12; nokhwa landet bei YUYV 1080p@5. `compatible_camera_formats` scheitert oft (`Failed to Fufill`), daher bleibt der 5-fps-Stream. MJPEG 1080p@30 und 720p@30 existieren auf dem Gerät, wurden aber aus `pick_format` ausgeschlossen (`DIRECT_FORMATS = NV12|YUYV`).

## Goals / Non-Goals

**Goals**

- Nie unter 30 fps öffnen, wenn das Gerät 30 fps in irgendeinem nutzbaren Format anbietet.
- MJPEG 1080p/720p als bevorzugter HD-Pfad auf USB-2.0-Kameras.
- Schneller Decode MJPEG → I420 mit wiederverwendeten Buffers.
- Messbarkeit im Call-Report.

**Non-Goals**

- NVENC/VAAPI-Encoder in diesem Durchgang.
- Receive-/Render-Pfad-Optimierung, bevor eigene Capture-fps stimmen.

## Decisions

1. **Exact-Kandidaten mit fps-Gate:** Nach jedem Open `frame_rate() >= 30` prüfen; sonst nächster Kandidat (MJPG HD → Raw 30 fps → AbsoluteHighest).
2. **MJPEG in `pick_format`:** Bei gleicher Auflösung und 30 fps bleibt Raw (NV12 > YUYV) bevorzugt; sonst MJPEG.
3. **Decode:** `zune-jpeg` → RGBA → `abgr_to_i420`; nokhwa-`decode_image` nur für andere Formate.
4. **UVC:** `exposure_dynamic_framerate=0`, `power_line_frequency=50` best effort, Fehler nur loggen.

## Risks / Trade-offs

- MJPEG-Decode kostet CPU; bei 1080p@30 muss < ~15 ms/Frame bleiben — sonst Capture weiter senken (720p) als Fallback.
- AbsoluteHighest kann wieder Property-Fails auslösen — nur letzter Ausweg.

## Migration

Keine Datenmigration. Nach Release App neu starten; Call-Report prüfen auf `MJPG … @30` und ~30 encodierte Frames/s.
