# Spec Delta

## Purpose

Signaling und SDP für Kamera und Screen-Share in verbundenen Gesprächen: Escalation-Answers und Same-peer-Renegotiation verhalten sich so, dass die Gegenseite die Media-Zeilen akzeptiert und verspätete Kamera-Answers noch greifen.

## ADDED Requirements

### Requirement: Escalation-Answer behält die Offer-Zeilenzahl
Bei einer Media-Renegotiation mit Escalation MUST die an Teams signalisierte Answer dieselbe Anzahl Media-Zeilen haben wie das Remote-Offer. Eine intern gefaltete Multi-Stream-Zeile MUST als genau eine signalisierte m-line zurückgehen.

#### Scenario: Multi-Stream-Zeile ohne Extra-Mid
- **WHEN** das Remote-Offer eine Video-Zeile mit `x-multi-stream` trägt, die intern in mehrere Browser-Mids entfaltet wurde
- **THEN** enthält die signalisierte Answer für diese Rolle genau eine m-line und keine zusätzliche Mid, die nur der internen Entfaltung dient

#### Scenario: Zeilenzahl stimmt mit dem Offer
- **WHEN** das Remote-Offer N Media-Zeilen hat
- **THEN** hat die signalisierte Escalation-Answer ebenfalls N Media-Zeilen

### Requirement: Verspätete Same-peer-Kamera-Answer gilt
Sendet die App in einem Direct-1:1 eine Same-peer-Renegotiation für Kamera an und trifft die passende Answer erst kurz nach dem lokalen Answer-Timeout ein, MUST die App diese Answer noch anwenden, sofern sie zum zuletzt gesendeten Offer gehört. Eine Answer mit empfangsseitiger Kamera-Richtung (`recvonly` aus Sicht der Gegenseite) MUST als erfolgreiches Freigeben des lokalen Sendens gelten.

#### Scenario: Answer wenige Sekunden nach Timeout
- **WHEN** das lokale Same-peer-Kamera-Offer wegen Timeout zurückgerollt wurde und kurz danach die passende Answer eintrifft
- **THEN** wendet die App die Answer an und die Kamera bleibt bzw. wird wieder als sendend geführt, statt die Answer zu verwerfen
