# Proposal

## Why

In einem 1:1-Gespräch unter Hyprland starteten Kamera und Screen-Share nicht. Die Escalation-Answer enthielt eine Extra-m-line (`mid 3-0`) und Teams lehnte mit `InvalidNumberOfMedia` ab. Eine verspätete Same-peer-Answer für die Kamera wurde nach dem 40-s-Timeout verworfen. Parallel starb der lokale Kamera-Open an `CameraFormat: Failed to Fufill`.

## What Changes

- Escalation-/Teams-Answers fassen gefaltetes `x-multi-stream` wieder zu einer m-line zusammen (keine Extra-Zeile `3-0`).
- Eine Same-peer-Answer, die kurz nach dem Timeout zum zuletzt gesendeten Offer gehört, wird noch angewendet.
- Kamera-Open fällt auf Rohformate zurück, wenn die Format-Property fehlschlägt.
- **Out of scope:** Command-Queue, Portal-Lifetime und PipeWire-BGRA — erst nach Retest, falls noch nötig.

## Capabilities

### New Capabilities

- `call-media-negotiation`: SDP-/Renegotiation-Verhalten für Escalation-Answers und Same-peer-Kamera-Answers.
- `camera-capture`: Lokaler Kamera-Start trotz fehlgeschlagener Format-Property.

### Modified Capabilities

- (keine) — `screen-share` (Portal-Dialog) bleibt unverändert.

## Impact

- Signaling/SDP in `crates/calling` (`sdp.rs`, `call.rs`)
- Kamera-Open in `crates/calling/src/camera.rs`
- Call-Reports und Retarget-Fehler bei 1:1 mit Multi-Stream-Offers
