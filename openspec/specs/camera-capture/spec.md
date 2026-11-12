# camera-capture Specification

## Purpose

Lokaler Kamera-Start in Gesprächen und Prejoin, wenn das Gerät Formate liefert, die Property-Abfrage aber fehlschlägt.

## Requirements

### Requirement: Format-Property-Fehler blockiert den Open nicht
Schlägt die Abfrage der unterstützten Kamera-Formate oder der laufenden Format-Property fehl, MUST die App den Stream trotzdem mit einem 30-fps-fähigen Kandidaten (MJPEG oder Rohformat) öffnen, sofern das Gerät das erlaubt. Nur wenn kein 30-fps-Open gelingt, MAY die App einen langsameren Fallback nutzen; schlägt alles fehl, MUST die App dem Nutzer den Fehler anzeigen.

#### Scenario: Property fehlgeschlagen, 30-fps-Format verfügbar
- **WHEN** der Nutzer die Kamera einschaltet und die Format-Property fehlschlägt, das Gerät aber MJPEG oder Raw bei 30 fps anbietet
- **THEN** startet die lokale Kamera mit einem 30-fps-Format und zeigt keinen fatalen Format-Property-Stop

#### Scenario: Kein Fallback möglich
- **WHEN** weder Formatliste noch Fallback-Open ein nutzbares Format liefern
- **THEN** bleibt die Kamera aus und die App zeigt den Fehler klar an

### Requirement: Capture wählt mindestens 30 fps wenn verfügbar
Wenn das Gerät mindestens ein Format mit 30 fps anbietet, MUST die App einen Stream mit mindestens 30 fps öffnen. Ein Open unter 30 fps (z. B. YUYV 1080p@5) MUST verworfen und durch den nächsten Kandidaten ersetzt werden, solange ein 30-fps-Kandidat übrig ist.

#### Scenario: C930e Auto wählt MJPEG 1080p@30
- **WHEN** die Kamera YUYV nur bis 1080p@5 / 720p@10 und MJPEG bis 1080p@30 anbietet und die Qualität Auto oder 1080p ist
- **THEN** öffnet die App MJPEG 1920×1080 bei 30 fps (oder ein anderes 30-fps-HD-Format), nicht YUYV@5

#### Scenario: 720p-Qualität
- **WHEN** die Qualität 720p ist und MJPEG 1280×720@30 verfügbar ist
- **THEN** öffnet die App dieses Format (oder Raw 30 fps gleicher Höhe), nicht ein Format unter 30 fps

### Requirement: MJPEG wird dekodiert und gesendet
MJPEG-Frames MUST lokal nach I420 dekodiert und in die bestehende Sendepipeline (Effects → Encoder) gegeben werden, ohne den Nutzer mit einem Undecodable-Hinweis abzubrechen, solange der Decode gelingt.

#### Scenario: MJPEG-Frame wird I420
- **WHEN** der Capture-Stream MJPEG liefert
- **THEN** entsteht ein I420-Frame für Preview und Send, und die Timeline zählt Capture-Frames

### Requirement: UVC-Bildrate und Netzfrequenz (best effort)
Nach erfolgreichem Open SHOULD die App `exposure_dynamic_framerate` deaktivieren und `power_line_frequency` auf 50 Hz setzen, wenn die Controls existieren. Fehlende Controls MUST den Stream nicht abbrechen.

#### Scenario: Controls fehlen
- **WHEN** die Kamera eines der Controls nicht anbietet
- **THEN** läuft der Stream weiter ohne Fehlerdialog

### Requirement: Capture-Telemetrie im Call-Report
Beim Kamera-Start MUST die Timeline das gewählte Format (Pixel-Format, Auflösung, fps) nennen. Laufende Stats SHOULD Capture-fps und Decode-Zeit sichtbar machen, damit ein Call-Report zeigt, ob 30 fps ankommen.

#### Scenario: Format in der Timeline
- **WHEN** die Kamera erfolgreich startet
- **THEN** enthält die Timeline einen Eintrag mit Format wie `MJPG 1920x1080 @30`
