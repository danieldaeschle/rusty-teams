# Proposal

## Why

Nach dem 1:1-Kamera-Fix öffnet die App auf dem Logitech C930e (USB 2.0) oft YUYV 1920×1080 bei 5 fps statt MJPEG bei 30 fps. Das wirkt wie starker Verzug und Framedrops, obwohl der Encoder nicht CPU-limited ist.

## What Changes

- Kamera-Open wählt Formate mit mindestens 30 fps; bei HD auf USB 2.0 bevorzugt MJPEG 1080p/720p.
- MJPEG-Frames werden lokal (zune-jpeg → I420) dekodiert, ohne den langsamen Roh-Closest-Pfad.
- UVC-Controls: feste Bildrate und 50-Hz-Netzfrequenz (Schweiz) best effort.
- Call-Report/Timeline zeigt gewähltes Format, Capture-fps und Decode-Zeit.

## Capabilities

### New Capabilities

- (keine)

### Modified Capabilities

- `camera-capture`: 30-fps-Formatwahl inkl. MJPEG, Decode-Pfad, UVC-Tuning und Telemetrie.

## Impact

- `crates/calling/src/camera.rs` (Open, Decode, Controls)
- ggf. Call-Timeline/Stats für Format und Capture-fps
- C930e und ähnliche UVC-Kameras ohne NV12 bei HD
