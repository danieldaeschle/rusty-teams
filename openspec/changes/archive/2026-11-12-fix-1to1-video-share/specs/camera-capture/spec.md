# Spec Delta

## Purpose

Lokaler Kamera-Start in Gesprächen und Prejoin, wenn das Gerät Formate liefert, die Property-Abfrage aber fehlschlägt.

## ADDED Requirements

### Requirement: Format-Property-Fehler blockiert den Open nicht
Schlägt die Abfrage der unterstützten Kamera-Formate oder der laufenden Format-Property fehl, MUST die App den Stream trotzdem mit einem Rohformat (etwa NV12 oder YUYV) bzw. dem bestehenden Fallback öffnen, sofern das Gerät das erlaubt. Nur wenn kein Fallback-Open gelingt, MUST die App dem Nutzer den Fehler anzeigen.

#### Scenario: Property fehlgeschlagen, Rohformat verfügbar
- **WHEN** der Nutzer die Kamera einschaltet und die Format-Property fehlschlägt, das Gerät aber ein Rohformat bei der gewünschten Rate anbietet
- **THEN** startet die lokale Kamera und zeigt keinen fatalen Format-Property-Stop

#### Scenario: Kein Fallback möglich
- **WHEN** weder Formatliste noch Fallback-Open ein Rohformat liefern
- **THEN** bleibt die Kamera aus und die App zeigt den Fehler klar an
